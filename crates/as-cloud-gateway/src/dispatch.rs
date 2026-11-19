//! Starting a worker for a session, and measuring an uploaded file.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use as_cloud::WorkerLaunch;
use serde_json::Value;

use crate::config;

/// Where workers run. `launch` returns once the worker is on its way.
pub trait Backend: Send + Sync {
    fn launch(&self, launch: &WorkerLaunch) -> Result<(), String>;
}

pub fn backend(cfg: &config::Backend) -> Box<dyn Backend> {
    match cfg {
        config::Backend::Local { worker, args } => Box::new(Local { worker: worker.clone(), args: args.clone() }),
        config::Backend::Http { url, authorization, wrap } => {
            Box::new(Http { url: url.clone(), authorization: authorization.clone(), wrap: wrap.clone() })
        }
        config::Backend::None => Box::new(Nowhere),
    }
}

/// A worker program on this machine, given its launch on stdin.
pub struct Local {
    pub worker: PathBuf,
    pub args: Vec<String>,
}

impl Backend for Local {
    fn launch(&self, launch: &WorkerLaunch) -> Result<(), String> {
        let mut child = Command::new(&self.worker)
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .map_err(|e| format!("starting {}: {e}", self.worker.display()))?;
        let line = serde_json::to_vec(launch).map_err(|e| e.to_string())?;
        child.stdin.take().ok_or("no stdin")?.write_all(&line).map_err(|e| e.to_string())?;
        // The worker reports through the gateway; reap it in the background.
        std::thread::spawn(move || drop(child.wait()));
        Ok(())
    }
}

/// A serverless GPU service: the launch is POSTed as JSON.
pub struct Http {
    pub url: String,
    pub authorization: Option<String>,
    pub wrap: Option<String>,
}

impl Backend for Http {
    fn launch(&self, launch: &WorkerLaunch) -> Result<(), String> {
        let mut body = serde_json::to_value(launch).map_err(|e| e.to_string())?;
        if let Some(key) = &self.wrap {
            body = serde_json::json!({ key.as_str(): body });
        }
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(std::time::Duration::from_secs(60)))
            .build()
            .into();
        let mut req = agent.post(&self.url).header("Content-Type", "application/json");
        if let Some(auth) = &self.authorization {
            req = req.header("Authorization", auth);
        }
        let mut resp =
            req.send(serde_json::to_vec(&body).map_err(|e| e.to_string())?.as_slice()).map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            let text = resp.body_mut().read_to_string().unwrap_or_default();
            return Err(format!(
                "the GPU service answered {}: {}",
                resp.status(),
                text.chars().take(300).collect::<String>()
            ));
        }
        Ok(())
    }
}

/// No workers at all.
pub struct Nowhere;

impl Backend for Nowhere {
    fn launch(&self, _: &WorkerLaunch) -> Result<(), String> {
        Ok(())
    }
}

/// Measuring an uploaded file: its length in milliseconds, if it is video.
pub trait Prober: Send + Sync {
    fn duration_ms(&self, file: &Path) -> Result<u64, String>;
}

pub struct Ffprobe(pub PathBuf);

/// How long one ffprobe run on an uploaded file may take.
const PROBE_TIMEOUT: Duration = Duration::from_secs(300);

impl Prober for Ffprobe {
    /// The file's length, read so that a forged header can't shorten it, nor
    /// a short first stream hide a long second one (the engine scans the
    /// stream the app names): the longest, over every video stream, of the
    /// stated lengths, the last packet's time, and the packets counted at
    /// the stream's frame rate (taken no faster than 60 frames a second).
    fn duration_ms(&self, file: &Path) -> Result<u64, String> {
        let head = run(Command::new(&self.0)
            .args(["-v", "error", "-select_streams", "v", "-count_packets", "-show_entries"])
            .arg("format=duration:stream=codec_type,duration,nb_read_packets,avg_frame_rate,r_frame_rate")
            .args(["-of", "json"])
            .arg(file))?;
        let last_pts = last_packet(
            Command::new(&self.0)
                .args(["-v", "error", "-select_streams", "v", "-show_entries", "packet=pts_time", "-of", "csv=p=0"])
                .arg(file),
        )?;
        duration_of(&serde_json::from_slice(&head).map_err(|e| e.to_string())?, last_pts)
    }
}

/// Run a command for its (small) output, killing it past [`PROBE_TIMEOUT`].
fn run(cmd: &mut Command) -> Result<Vec<u8>, String> {
    waited(cmd, |out| {
        let mut b = Vec::new();
        let _ = std::io::Read::take(out, 64 << 20).read_to_end(&mut b);
        b
    })
}

/// The latest time in a packet listing, read line by line: a file of a
/// hundred million tiny packets costs no memory.
fn last_packet(cmd: &mut Command) -> Result<Option<f64>, String> {
    waited(cmd, |out| {
        let mut last: Option<f64> = None;
        for line in std::io::BufRead::lines(std::io::BufReader::new(out)) {
            let Ok(line) = line else { break };
            if let Some(t) = line.trim().trim_end_matches(',').parse::<f64>().ok().filter(|t| t.is_finite()) {
                last = Some(last.map_or(t, |m| m.max(t)));
            }
        }
        last
    })
}

/// Run `cmd`, read its output with `read` meanwhile, and kill it past
/// [`PROBE_TIMEOUT`].
fn waited<T: Send + 'static>(
    cmd: &mut Command,
    read: impl FnOnce(std::process::ChildStdout) -> T + Send + 'static,
) -> Result<T, String> {
    let mut child =
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null()).spawn().map_err(|e| e.to_string())?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let stderr = child.stderr.take().ok_or("no stderr")?;
    let out = std::thread::spawn(move || read(stdout));
    let err = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = std::io::Read::take(stderr, 1 << 20).read_to_end(&mut b);
        b
    });
    let until = Instant::now() + PROBE_TIMEOUT;
    let status = loop {
        if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
            break s;
        }
        if Instant::now() > until {
            let _ = child.kill();
            let _ = child.wait();
            return Err("measuring the file took too long".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let out = out.join().map_err(|_| "reading ffprobe failed".to_string())?;
    let err = err.join().unwrap_or_default();
    if !status.success() {
        return Err(String::from_utf8_lossy(&err).trim().chars().take(500).collect());
    }
    Ok(out)
}

fn rate(v: &Value) -> Option<f64> {
    let (n, d) = v.as_str()?.split_once('/')?;
    let (n, d) = (n.parse::<f64>().ok()?, d.parse::<f64>().ok()?);
    (d > 0.0 && n > 0.0).then_some(n / d)
}

/// The length ffprobe's answers give a file with a video stream (see
/// [`Ffprobe::duration_ms`]).
pub fn duration_of(probe: &Value, last_pts: Option<f64>) -> Result<u64, String> {
    let streams = probe["streams"].as_array().cloned().unwrap_or_default();
    let videos: Vec<&Value> = streams.iter().filter(|s| s["codec_type"] == "video").collect();
    if videos.is_empty() {
        return Err("no video stream".into());
    }
    let secs = |v: &Value| v.as_str().and_then(|d| d.parse::<f64>().ok()).filter(|d| d.is_finite() && *d > 0.0);
    let mut longest = [secs(&probe["format"]["duration"]), last_pts].into_iter().flatten().fold(0.0_f64, f64::max);
    for video in videos {
        let packets = video["nb_read_packets"].as_str().and_then(|n| n.parse::<f64>().ok()).unwrap_or(0.0);
        // A stream that claims a high rate would count its packets short.
        let fps =
            rate(&video["avg_frame_rate"]).or_else(|| rate(&video["r_frame_rate"])).unwrap_or(24.0).clamp(10.0, 60.0);
        for d in [secs(&video["duration"]), Some(packets / fps)].into_iter().flatten() {
            if d.is_finite() {
                longest = longest.max(d);
            }
        }
    }
    if longest <= 0.0 {
        return Err("no duration".into());
    }
    Ok((longest * 1000.0).ceil() as u64)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_length_is_read_only_from_video() {
        let ok = json!({"streams": [{"codec_type": "video", "avg_frame_rate": "24000/1001", "nb_read_packets": "34528"}],
                        "format": {"duration": "1440.0421"}});
        assert_eq!(duration_of(&ok, Some(1439.9)), Ok(1_440_106), "the packets at 23.976 fps: 1440.106 s");
        let audio = json!({"streams": [{"codec_type": "audio"}], "format": {"duration": "60"}});
        assert!(duration_of(&audio, None).is_err());
        let none = json!({"streams": [{"codec_type": "video"}], "format": {}});
        assert!(duration_of(&none, None).is_err());
    }

    #[test]
    fn a_forged_header_cant_shorten_a_file() {
        // The container says a millisecond; its 600 packets and their times don't.
        let forged = json!({"streams": [{"codec_type": "video", "avg_frame_rate": "10/1", "nb_read_packets": "600",
                            "duration": "0.001"}], "format": {"duration": "0.001000"}});
        assert_eq!(duration_of(&forged, None), Ok(60_000));
        assert_eq!(duration_of(&forged, Some(59.96)), Ok(60_000));
        // A forged rate counts the packets at no more than 60 a second.
        let fast = json!({"streams": [{"codec_type": "video", "avg_frame_rate": "100000/1", "nb_read_packets": "6000"}],
                          "format": {"duration": "0.001"}});
        assert_eq!(duration_of(&fast, None), Ok(100_000));
        // A one-second first stream doesn't hide a long second one.
        let two = json!({"streams": [
            {"codec_type": "video", "avg_frame_rate": "24/1", "nb_read_packets": "24", "duration": "1"},
            {"codec_type": "video", "avg_frame_rate": "24/1", "nb_read_packets": "172800"}],
            "format": {"duration": "1"}});
        assert_eq!(duration_of(&two, Some(1.0)), Ok(7_200_000));
    }
}

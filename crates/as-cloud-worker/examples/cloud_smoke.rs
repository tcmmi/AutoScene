//! A cloud scan from start to end, as the app runs one: this computer
//! registers, takes a code, uploads a video and has it measured and priced,
//! then a shots job and a faces job run on the cloud GPU, and the session
//! closes. With `--local`, the same jobs run on this computer's engine too, to
//! compare what each found.
//!
//! ```text
//! cargo run -p as-cloud-worker --example cloud_smoke -- --file EPISODE
//!     [--gateway https://api.autoscene.io] [--code CODE] [--ffmpeg DIR]
//!     [--anime] [--fast] [--slice SECONDS]
//!     [--local ENGINE --ort DIR --models DIR]
//! ```
//!
//! The faces job looks at fixed slices (`--slice`, 10 s) instead of the
//! app's shots: enough to compare two engines on the same frames.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use as_cloud::client::{Client, file_sha256, hex};
use as_cloud::{Depth, Footage, RegisterRequest, ScanKind, SessionRequest, euros};
use base64::Engine as _;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

type Res<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Default)]
struct Opts {
    gateway: String,
    file: PathBuf,
    code: Option<String>,
    ffmpeg: Option<PathBuf>,
    anime: bool,
    fast: bool,
    slice: f64,
    local: Option<PathBuf>,
    ort: Option<PathBuf>,
    models: Option<PathBuf>,
}

fn opts() -> Res<Opts> {
    let mut o = Opts { gateway: "https://api.autoscene.io".into(), slice: 10.0, ..Opts::default() };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut val = || args.next().ok_or_else(|| format!("{a} needs a value"));
        match a.as_str() {
            "--gateway" => o.gateway = val()?,
            "--file" => o.file = val()?.into(),
            "--code" => o.code = Some(val()?),
            "--ffmpeg" => o.ffmpeg = Some(val()?.into()),
            "--anime" => o.anime = true,
            "--fast" => o.fast = true,
            "--slice" => o.slice = val()?.parse()?,
            "--local" => o.local = Some(val()?.into()),
            "--ort" => o.ort = Some(val()?.into()),
            "--models" => o.models = Some(val()?.into()),
            other => return Err(format!("unknown argument {other}").into()),
        }
    }
    if o.file.as_os_str().is_empty() {
        return Err("--file is needed".into());
    }
    Ok(o)
}

/// What one engine said while running the jobs.
#[derive(Default, Debug)]
struct Tally {
    device: Option<String>,
    devices: Vec<String>,
    shot_chunks: usize,
    signal_chars: usize,
    face_chunks: usize,
    track_chars: usize,
    sprites: usize,
    failed: Option<String>,
    secs: Vec<f64>,
}

impl Tally {
    /// Counts a reply; the id of a job that ended (done or failed), or 0 when the engine gave up.
    fn take(&mut self, v: &Value) -> Option<u64> {
        let len = |key: &str| v[key].as_str().map_or(0, str::len);
        match v["event"].as_str()? {
            "ready" => self.device = v["device"].as_str().map(str::to_string),
            "devices" => {
                for m in v["models"].as_array().into_iter().flatten() {
                    self.devices.push(format!("{} on {}", m["model"], m["device"]));
                }
            }
            "shot_chunk" => {
                self.shot_chunks += 1;
                self.signal_chars += len("signals");
            }
            "face_chunk" => {
                self.face_chunks += 1;
                self.track_chars += len("tracks");
                self.sprites += v["sprites"].as_array().map_or(0, Vec::len);
            }
            "done" => return v["id"].as_u64(),
            "failed" => {
                self.failed = Some(format!("job {}: {}", v["id"], v["detail"]));
                return v["id"].as_u64();
            }
            "fatal" => {
                self.failed = Some(format!("fatal: {}", v["detail"]));
                return Some(0);
            }
            _ => {}
        }
        None
    }
}

/// The shots job and a faces job over fixed slices, with `path` for the file.
fn jobs(o: &Opts, info: &Value, index: &str, frames: u32, fps: f64, path: &str) -> Vec<Value> {
    let step = ((fps * o.slice).round() as u32).max(1);
    let shots: Vec<(u32, u32)> = (0..frames).step_by(step as usize).map(|s| (s, (s + step).min(frames))).collect();
    vec![
        json!({"op": "shots", "id": 1, "path": path, "info": info, "index": index, "resume": 0, "chunk": 2400}),
        json!({"op": "faces", "id": 2, "path": path, "info": info, "index": index, "shots": shots,
               "resume": 0, "chunk": 2400, "anime": o.anime, "fast": o.fast}),
    ]
}

fn cloud(o: &Opts, jobs: &[Value], duration_ms: u64) -> Res<Tally> {
    let mut c = Client::new(o.gateway.clone(), None);
    let hwid = hex(&Sha256::digest(b"autoscene-cloud-smoke"));
    let account = c.register(&RegisterRequest { hwid, platform: "smoke".into(), app_version: "smoke".into() })?;
    println!("cloud: account {} on {}", account.support_id, o.gateway);
    if let Some(code) = &o.code {
        let r = c.redeem(code)?;
        println!("cloud: code redeemed (added {})", euros(r.added));
    }
    let kind = ScanKind {
        footage: if o.anime { Footage::Anime } else { Footage::Live },
        depth: if o.fast { Depth::Fast } else { Depth::Standard },
        people: false,
        scenes: false,
    };
    let size = std::fs::metadata(&o.file)?.len();
    let sha256 = file_sha256(&o.file, |_| true)?;
    let s = c.open_session(&SessionRequest { kind, size, sha256, duration_ms })?;
    println!("cloud: session {} open, {} held", s.id, euros(s.estimate));
    let t = Instant::now();
    let mut shown = 0;
    c.upload(&s.id, &o.file, |sent, size| {
        let tenth = sent * 10 / size.max(1);
        if tenth > shown {
            shown = tenth;
            println!("cloud: uploaded {}%", tenth * 10);
        }
        true
    })?;
    println!("cloud: upload took {:.0} s", t.elapsed().as_secs_f64());
    let t = Instant::now();
    let ready = c.ready(&s.id)?;
    println!(
        "cloud: measured {} s, priced {}, in {:.0} s",
        ready.duration_ms / 1000,
        euros(ready.price),
        t.elapsed().as_secs_f64()
    );
    let mut tally = Tally::default();
    let mut after = 0;
    for job in jobs {
        let want = job["id"].as_u64().unwrap_or_default();
        let t = Instant::now();
        c.send_jobs(&s.id, vec![job.clone()])?;
        'job: loop {
            let r = c.events(&s.id, after)?;
            after = r.next;
            for line in &r.lines {
                if let Some(id) = tally.take(&serde_json::from_str(line)?)
                    && (id == want || id == 0)
                {
                    break 'job;
                }
            }
            if r.ended {
                tally.failed = Some(format!("the worker ended: {:?}", r.error));
                break;
            }
        }
        tally.secs.push(t.elapsed().as_secs_f64());
        println!("cloud: job {want} ended after {:.0} s", t.elapsed().as_secs_f64());
        if tally.failed.is_some() {
            break;
        }
    }
    let closed = c.close(&s.id)?;
    println!("cloud: closed, charged {}, balance {}", euros(closed.charged), euros(closed.account.balance));
    Ok(tally)
}

fn local(o: &Opts, engine: &Path, jobs: &[Value]) -> Res<Tally> {
    let (Some(ort), Some(models)) = (&o.ort, &o.models) else {
        return Err("--local needs --ort and --models".into());
    };
    let mut cmd = Command::new(engine);
    cmd.arg("--ort").arg(ort).arg("--models").arg(models);
    if let Some(ff) = &o.ffmpeg {
        cmd.arg("--ffmpeg").arg(ff);
    }
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).spawn()?;
    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    let mut lines = BufReader::new(child.stdout.take().ok_or("no stdout")?).lines();
    let mut tally = Tally::default();
    for job in jobs {
        let want = job["id"].as_u64().unwrap_or_default();
        let t = Instant::now();
        writeln!(stdin, "{job}")?;
        stdin.flush()?;
        for line in lines.by_ref() {
            if let Some(id) = tally.take(&serde_json::from_str(&line?)?)
                && (id == want || id == 0)
            {
                break;
            }
        }
        tally.secs.push(t.elapsed().as_secs_f64());
        println!("local: job {want} ended after {:.0} s", t.elapsed().as_secs_f64());
        if tally.failed.is_some() {
            break;
        }
    }
    let _ = writeln!(stdin, "{}", json!({"op": "shutdown"}));
    drop(stdin);
    let _ = child.wait();
    Ok(tally)
}

fn main() -> Res<()> {
    let o = opts()?;
    let tools = as_media::tools::Tools::locate(o.ffmpeg.as_deref())?;
    let info = as_media::probe(&tools, &o.file)?;
    let work = std::env::temp_dir().join("autoscene-cloud-smoke");
    let index = as_media::build_frame_index(&tools, &o.file, &info, &work, &as_core::CancelToken::new(), |_| {})?;
    let frames = index.len();
    let fps = info.video.r_frame_rate.num as f64 / info.video.r_frame_rate.den.max(1) as f64;
    let duration_ms = (info.duration_sec.unwrap_or(0.0) * 1000.0) as u64;
    println!("file: {} frames at {fps:.3} fps, {} s", frames, duration_ms / 1000);
    let info_v = serde_json::to_value(&info)?;
    let index_b64 = base64::engine::general_purpose::STANDARD.encode(index.to_bytes());

    let on_cloud = cloud(&o, &jobs(&o, &info_v, &index_b64, frames, fps, "uploaded"), duration_ms)?;
    println!("\ncloud: {on_cloud:#?}");
    if let Some(engine) = &o.local {
        let path = o.file.to_string_lossy().into_owned();
        let here = local(&o, engine, &jobs(&o, &info_v, &index_b64, frames, fps, &path))?;
        println!("\nlocal: {here:#?}");
        println!(
            "\ncompare: shot chunks {} vs {}, signal chars {} vs {}, face chunks {} vs {}, track chars {} vs {}, sprites {} vs {}",
            on_cloud.shot_chunks,
            here.shot_chunks,
            on_cloud.signal_chars,
            here.signal_chars,
            on_cloud.face_chunks,
            here.face_chunks,
            on_cloud.track_chars,
            here.track_chars,
            on_cloud.sprites,
            here.sprites
        );
    }
    Ok(())
}

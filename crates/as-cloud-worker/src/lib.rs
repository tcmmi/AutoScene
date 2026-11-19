//! AutoScene Cloud's GPU worker: one scan session, start to end.
//!
//! 1. Fetch the session's file from the gateway, piece by piece, and check
//!    it is the file the app announced (SHA-256).
//! 2. Start the analysis engine (`autoscene-engine`, unchanged) on it.
//! 3. Until the app closes the session or no job comes for a while: take
//!    the app's engine requests from the gateway, point them at the local
//!    copy, and hand every line the engine writes back to the gateway.
//! 4. Say how long it ran (the owner's GPU budget) and whether it failed.
//!
//! The worker keeps nothing: its folder goes when it ends.

use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use as_cloud::{WorkerDone, WorkerEvents, WorkerJobs, WorkerLaunch, path};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Bytes asked for per request of the file.
const PIECE: u64 = 32 << 20;
/// Replies sent per request at most, and how long a reply may wait for others.
const BATCH_LINES: usize = 256;
const BATCH_BYTES: usize = 8 << 20;
const BATCH_WAIT: Duration = Duration::from_millis(250);
/// How long the gateway may hold a jobs request open.
const JOBS_WAIT_SECS: u64 = 20;
/// Tries of a call to the gateway before the worker gives up.
const TRIES: u32 = 5;
/// How long the worker waits for the app to read replies before giving up.
const WAIT_LIMIT: Duration = Duration::from_secs(30 * 60);

/// The engine program and its arguments (models, runtime, device).
#[derive(Clone, Debug)]
pub struct EngineCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
}

/// How the worker checks the app's jobs against its own copy of the file.
#[derive(Clone, Debug)]
pub enum Verify {
    /// Read the file's media info and frame index here (ffmpeg and ffprobe
    /// from `tools`, else the usual places) and refuse any job whose `info`
    /// or `index` differ: the app can't choose another stream, nor shrink
    /// the frames a job's progress is counted against.
    Own { tools: Option<PathBuf> },
    /// Trust the jobs (tests with a stand-in engine and no real video).
    Off,
}

/// The file as the worker reads it: what a job's `info` and `index` must be.
struct Reading {
    info: Value,
    index: String,
}

fn read_file(file: &Path, work: &Path, tools: Option<&Path>) -> Result<Reading, String> {
    use base64::Engine as _;
    let tools = as_media::tools::Tools::locate(tools).map_err(|e| format!("ffmpeg: {e}"))?;
    let info = as_media::probe(&tools, file).map_err(|e| format!("reading the file: {e}"))?;
    let index = as_media::build_frame_index(&tools, file, &info, work, &as_core::CancelToken::new(), |_| {})
        .map_err(|e| format!("indexing the file: {e}"))?;
    Ok(Reading {
        info: serde_json::to_value(&info).map_err(|e| e.to_string())?,
        index: base64::engine::general_purpose::STANDARD.encode(index.to_bytes()),
    })
}

/// Why a job doesn't fit the file, if it doesn't.
fn misfit(job: &Value, reading: &Reading) -> Option<&'static str> {
    if job.get("info") != Some(&reading.info) {
        return Some("its media info isn't this file's");
    }
    if job.get("index").and_then(Value::as_str) != Some(reading.index.as_str()) {
        return Some("its frame index isn't this file's");
    }
    None
}

struct Gateway {
    agent: ureq::Agent,
    base: String,
    auth: String,
}

impl Gateway {
    fn new(launch: &WorkerLaunch) -> Self {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(20)))
            .build()
            .into();
        Self { agent, base: launch.base.trim_end_matches('/').to_string(), auth: format!("Bearer {}", launch.token) }
    }

    /// Call `f` until it works: a network error or a 5xx is tried again,
    /// waiting longer each time; a 429 (the app hasn't read the replies yet)
    /// waits as long as `WAIT_LIMIT`; any other 4xx is final.
    fn retry<T>(&self, what: &str, mut f: impl FnMut() -> Result<T, (Fault, String)>) -> Result<T, String> {
        let mut wait = Duration::from_millis(500);
        let mut tries = 0;
        let started = Instant::now();
        loop {
            match f() {
                Ok(v) => return Ok(v),
                Err((Fault::Final, e)) => return Err(format!("{what}: {e}")),
                Err((Fault::Wait, e)) => {
                    if started.elapsed() > WAIT_LIMIT {
                        return Err(format!("{what}: {e}"));
                    }
                    std::thread::sleep(Duration::from_secs(2));
                }
                Err((Fault::Again, e)) => {
                    tries += 1;
                    if tries >= TRIES {
                        return Err(format!("{what}: {e}"));
                    }
                    std::thread::sleep(wait);
                    wait *= 2;
                }
            }
        }
    }

    fn post(&self, p: &str, body: &impl serde::Serialize) -> Result<(), String> {
        self.post_for::<Value>(p, body).map(drop)
    }

    fn post_for<T: serde::de::DeserializeOwned>(&self, p: &str, body: &impl serde::Serialize) -> Result<T, String> {
        let body = serde_json::to_vec(body).map_err(|e| e.to_string())?;
        self.retry(p, || {
            let resp = self
                .agent
                .post(&format!("{}{p}", self.base))
                .header("Authorization", &self.auth)
                .header("Content-Type", "application/json")
                .config()
                .timeout_global(Some(Duration::from_secs(120)))
                .build()
                .send(&body[..])
                .map_err(|e| (Fault::Again, e.to_string()))?;
            let mut resp = resp;
            check(resp.status().as_u16())?;
            let body = resp.body_mut().read_to_vec().map_err(|e| (Fault::Again, e.to_string()))?;
            serde_json::from_slice(&body).map_err(|e| (Fault::Final, e.to_string()))
        })
    }

    /// The app's jobs after the first `after`.
    fn jobs(&self, session: &str, after: u64) -> Result<WorkerJobs, String> {
        let p = format!("{}?after={after}&wait={JOBS_WAIT_SECS}", path::worker_jobs(session));
        self.retry("jobs", || {
            let mut resp = self
                .agent
                .get(&format!("{}{p}", self.base))
                .header("Authorization", &self.auth)
                .config()
                .timeout_global(Some(Duration::from_secs(JOBS_WAIT_SECS + 30)))
                .build()
                .call()
                .map_err(|e| (Fault::Again, e.to_string()))?;
            check(resp.status().as_u16())?;
            let body = resp
                .body_mut()
                .with_config()
                .limit(256 << 20)
                .read_to_vec()
                .map_err(|e| (Fault::Again, e.to_string()))?;
            serde_json::from_slice(&body).map_err(|e| (Fault::Final, e.to_string()))
        })
    }

    /// The session's file into `to`, checked against its SHA-256.
    fn fetch(&self, launch: &WorkerLaunch, to: &Path) -> Result<(), String> {
        let mut out = std::fs::File::create(to).map_err(|e| e.to_string())?;
        let mut hash = Sha256::new();
        let mut at = 0u64;
        let p = path::worker_file(&launch.session);
        while at < launch.size {
            let end = (at + PIECE).min(launch.size) - 1;
            let piece = self.retry("file", || {
                let mut resp = self
                    .agent
                    .get(&format!("{}{p}", self.base))
                    .header("Authorization", &self.auth)
                    .header("Range", &format!("bytes={at}-{end}"))
                    .config()
                    .timeout_global(Some(Duration::from_secs(300)))
                    .build()
                    .call()
                    .map_err(|e| (Fault::Again, e.to_string()))?;
                check(resp.status().as_u16())?;
                resp.body_mut().with_config().limit(PIECE * 2).read_to_vec().map_err(|e| (Fault::Again, e.to_string()))
            })?;
            if piece.is_empty() {
                return Err("the gateway sent an empty piece".into());
            }
            hash.update(&piece);
            out.write_all(&piece).map_err(|e| e.to_string())?;
            at += piece.len() as u64;
        }
        out.flush().map_err(|e| e.to_string())?;
        let got: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if got != launch.sha256 {
            return Err("the file isn't the one the app announced".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fault {
    /// The session is gone, the token wrong, the request refused.
    Final,
    /// The network or the gateway failed: try again soon.
    Again,
    /// The gateway asks to wait (429).
    Wait,
}

fn check(status: u16) -> Result<(), (Fault, String)> {
    let said = format!("the gateway answered {status}");
    match status {
        200..=299 => Ok(()),
        429 => Err((Fault::Wait, said)),
        400..=499 => Err((Fault::Final, said)),
        _ => Err((Fault::Again, said)),
    }
}

/// Run one session; returns what was reported to the gateway.
pub fn run(launch: &WorkerLaunch, engine: &EngineCommand, verify: &Verify, work: &Path) -> WorkerDone {
    let started = Instant::now();
    let gw = Gateway::new(launch);
    let result = session(launch, engine, verify, work, &gw);
    let done = WorkerDone { ok: result.is_ok(), gpu_secs: started.elapsed().as_secs_f64(), error: result.err() };
    if let Err(e) = gw.post(&path::worker_done(&launch.session), &done) {
        eprintln!("reporting the end: {e}");
    }
    let _ = std::fs::remove_dir_all(work);
    done
}

fn session(
    launch: &WorkerLaunch,
    engine: &EngineCommand,
    verify: &Verify,
    work: &Path,
    gw: &Gateway,
) -> Result<(), String> {
    std::fs::create_dir_all(work).map_err(|e| e.to_string())?;
    let file = work.join("input");
    gw.fetch(launch, &file)?;
    let local = file.to_string_lossy().into_owned();
    let reading = match verify {
        Verify::Own { tools } => Some(read_file(&file, work, tools.as_deref())?),
        Verify::Off => None,
    };

    let mut child = Command::new(&engine.program)
        .args(&engine.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("starting the engine: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("no engine stdin")?;
    let stdout = child.stdout.take().ok_or("no engine stdout")?;

    // The engine's lines, to the gateway in batches. Jobs still open are
    // watched here: the worker never idles out under a running job.
    let open: Arc<Mutex<HashSet<u64>>> = Arc::default();
    let heard = Arc::new(Mutex::new(Instant::now()));
    let (tx, rx) = mpsc::channel::<String>();
    // The worker's own answers (a job refused) go the same way.
    let said = tx.clone();
    let reader = {
        let (open, heard) = (open.clone(), heard.clone());
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                *heard.lock().unwrap() = Instant::now();
                match as_cloud::engine::event(&line) {
                    Some(as_cloud::engine::Event::Done { id } | as_cloud::engine::Event::Failed { id, .. }) => {
                        open.lock().unwrap().remove(&id);
                    }
                    // A request the engine couldn't read names no job: none
                    // is running any more.
                    Some(as_cloud::engine::Event::Fatal) => open.lock().unwrap().clear(),
                    _ => {}
                }
                if tx.send(line).is_err() {
                    break;
                }
            }
        })
    };
    let events = path::worker_events(&launch.session);
    let (sent_tx, sent_rx) = mpsc::channel::<Result<(), String>>();
    let poster = {
        let base = launch.clone();
        std::thread::spawn(move || {
            let gw = Gateway::new(&base);
            let r = post_lines(&gw, &events, &rx);
            let _ = sent_tx.send(r);
        })
    };

    let idle = Duration::from_secs(launch.idle_secs.max(10));
    let mut last_job = Instant::now();
    let mut cursor = 0u64;
    let outcome = loop {
        if let Ok(Some(status)) = child.try_wait() {
            break Err(format!("the engine stopped ({status})"));
        }
        if let Ok(r) = sent_rx.try_recv() {
            break r.and(Err("the engine stopped".into()));
        }
        let jobs = match gw.jobs(&launch.session, cursor) {
            Ok(j) => j,
            Err(e) => break Err(e),
        };
        cursor = jobs.next.max(cursor);
        // A cancel alone keeps no worker alive.
        if jobs.lines.iter().any(|l| as_cloud::engine::request_op(l) != Some(as_cloud::engine::Op::Cancel)) {
            last_job = Instant::now();
        }
        let mut wrote = Ok(());
        for mut line in jobs.lines {
            let op = as_cloud::engine::request_op(&line);
            if let (Some(reading), Some(id)) = (&reading, as_cloud::engine::id(&line))
                && op != Some(as_cloud::engine::Op::Cancel)
                && let Some(why) = misfit(&line, reading)
            {
                let failed = serde_json::json!({"event": "failed", "id": id,
                    "message": "The cloud reads this file differently from this computer.",
                    "detail": format!("the job was refused: {why}"), "cancelled": false});
                let _ = said.send(failed.to_string());
                continue;
            }
            if let (Some(op), Some(id)) = (op, as_cloud::engine::id(&line))
                && op != as_cloud::engine::Op::Cancel
            {
                open.lock().unwrap().insert(id);
            }
            as_cloud::engine::set_path(&mut line, &local);
            wrote = writeln!(stdin, "{line}").and_then(|_| stdin.flush());
            if wrote.is_err() {
                break;
            }
        }
        if let Err(e) = wrote {
            break Err(format!("writing to the engine: {e}"));
        }
        if jobs.close {
            break Ok(());
        }
        let quiet = last_job.max(*heard.lock().unwrap()).elapsed();
        if quiet > idle && open.lock().unwrap().is_empty() {
            break Ok(());
        }
    };

    drop(said);
    // Let the engine finish what it was told, then stop it.
    let _ = writeln!(stdin, "{}", serde_json::json!({"op": "shutdown"}));
    drop(stdin);
    stop(&mut child, Duration::from_secs(60));
    let _ = reader.join();
    let _ = poster.join();
    let posted = sent_rx.try_recv().unwrap_or(Ok(()));
    outcome.and(posted)
}

fn post_lines(gw: &Gateway, events: &str, rx: &mpsc::Receiver<String>) -> Result<(), String> {
    // Lines the gateway has taken: each batch says where it starts.
    let mut sent = 0u64;
    loop {
        let first = match rx.recv() {
            Ok(l) => l,
            Err(_) => return Ok(()),
        };
        let mut lines = vec![first];
        let mut bytes = lines[0].len();
        let until = Instant::now() + BATCH_WAIT;
        while lines.len() < BATCH_LINES && bytes < BATCH_BYTES {
            match rx.recv_timeout(until.saturating_duration_since(Instant::now())) {
                Ok(l) => {
                    bytes += l.len();
                    lines.push(l);
                }
                Err(_) => break,
            }
        }
        let n = lines.len() as u64;
        gw.post(events, &WorkerEvents { first: sent, lines })?;
        sent += n;
    }
}

fn stop(child: &mut Child, grace: Duration) {
    let until = Instant::now() + grace;
    while Instant::now() < until {
        if let Ok(Some(_)) = child.try_wait() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// A stand-in engine for tests (`--fake-engine`): it reads requests and
/// answers each job as the real engine does in shape, checking only that the
/// job's file is there.
pub fn fake_engine() -> i32 {
    let out = std::io::stdout();
    let mut out = out.lock();
    let say = |out: &mut std::io::StdoutLock<'_>, v: Value| {
        let _ = writeln!(out, "{v}");
        let _ = out.flush();
    };
    say(&mut out, serde_json::json!({"event": "ready", "device": "fake"}));
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(req) = serde_json::from_str::<Value>(&line) else { continue };
        let id = req["id"].as_u64().unwrap_or(0);
        let readable = req["path"].as_str().is_some_and(|p| {
            let mut b = [0u8; 1];
            std::fs::File::open(p).and_then(|mut f| f.read(&mut b)).is_ok()
        });
        match req["op"].as_str() {
            Some("shutdown") => break,
            Some("cancel") => {}
            Some(op @ ("shots" | "faces" | "scenes")) => {
                if !readable {
                    say(
                        &mut out,
                        serde_json::json!({"event": "failed", "id": id, "message": "no file", "detail": "", "cancelled": false}),
                    );
                    continue;
                }
                say(&mut out, serde_json::json!({"event": "progress", "id": id, "fraction": 0.5}));
                // `fake_slow`: stop half way, as a long job would, until shut down.
                if req["fake_slow"] == true {
                    continue;
                }
                if op == "faces" {
                    say(
                        &mut out,
                        serde_json::json!({"event": "face_chunk", "id": id, "start": 0, "end": 9, "tracks": "AQ=="}),
                    );
                }
                say(&mut out, serde_json::json!({"event": "progress", "id": id, "fraction": 1.0}));
                say(&mut out, serde_json::json!({"event": "done", "id": id}));
            }
            _ => say(
                &mut out,
                serde_json::json!({"event": "failed", "id": id, "message": "unknown op", "detail": "", "cancelled": false}),
            ),
        }
    }
    0
}

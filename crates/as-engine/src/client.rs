//! The app's side: starts the engine process on demand, hands it one job at
//! a time and relays its replies. A crashed engine (GPU driver reset, out of
//! memory, killed) fails only the job it was running; the job gets a fresh
//! process and resumes from the last chunk the app received.
//!
//! A new launch ([`Engine::set_launch`], a graphics card chosen in Settings)
//! applies from the next job: an idle engine stops at once, and a running
//! job finishes on its engine, which stops right after it.
//!
//! NVIDIA's engine for GeForce RTX cards ([`Launch::accel`]) is a plugin of
//! someone else's making in the engine's process. An engine started with it
//! that dies before it is ready counts against it, and so does a job it has
//! a part in that ends badly: its engine says it is in use, and a model of
//! the job runs on it (or may, while the engine hasn't said where they run),
//! and the engine dies under the job or a model fails on it. The next engine
//! then starts without it for the rest of this session; the job runs again
//! there, without using up one of its attempts. After [`PACK_FAILURES`] in a
//! row, across sessions, engines start without it until the person tries
//! again. A job it has a part in and finishes clears the count. A job it has
//! no part in (shots, live-action faces) counts neither way: a driver reset
//! under one is a crash like any other. "Processor only" never starts it.

use std::cell::Cell;
use std::ffi::OsString;
use std::fmt;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use as_core::{CancelToken, Msg as Said, ToMsg, msg};
use as_infer::accel::{self, AccelState};
use as_infer::computer::Choice;
use as_infer::hardware::Hardware;
use as_media::MediaInfo;
use as_media::frames::Decoder;
use parking_lot::Mutex;
use thiserror::Error;

use crate::protocol::{Blob, EmbedJob, FacesJob, ModelDevice, Reply, Request, RescanFaceIn, RescanJob, ShotsJob, said};

#[derive(Debug, Error)]
pub enum EngineError {
    /// The engine didn't start or reported it can't work (`detail` for the log).
    Start {
        message: Said,
        detail: String,
    },
    Crashed(String),
    Failed {
        message: Said,
        detail: String,
    },
    Cancelled,
}

impl ToMsg for EngineError {
    fn msg(&self) -> Said {
        match self {
            Self::Start { message, .. } | Self::Failed { message, .. } => message.clone(),
            Self::Crashed(_) => {
                msg!("engine.crashed", "The analysis engine stopped unexpectedly. Details are in the log.")
            }
            Self::Cancelled => msg!("engine.cancelled", "Cancelled."),
        }
    }
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.msg().text)
    }
}

/// An engine that didn't get as far as being ready.
fn couldnt_start() -> Said {
    msg!("engine.couldntStart", "The analysis engine couldn't start. Details are in the log.")
}

impl EngineError {
    pub fn detail(&self) -> String {
        match self {
            Self::Start { detail, .. } => format!("engine start: {detail}"),
            Self::Crashed(why) => format!("engine crashed: {why}"),
            Self::Failed { detail, .. } => detail.clone(),
            Self::Cancelled => "cancelled".into(),
        }
    }

    fn saving(detail: String) -> Self {
        Self::Failed { message: msg!("engine.savingFailed", "Saving the analysis failed."), detail }
    }
}

/// How to start the engine process.
#[derive(Clone, Debug, Default)]
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    /// The folder of NVIDIA's engine ([`as_infer::accel`]), passed as
    /// `--accel` while it works (see the module doc). Never with "Processor
    /// only": the engine wouldn't use it.
    pub accel: Option<PathBuf>,
    /// More environment for the engine process (tests).
    pub env: Vec<(OsString, OsString)>,
}

impl Launch {
    /// Start `program` with `prefix`, then `cfg`'s arguments; its folder of
    /// NVIDIA's engine goes to [`Launch::accel`], unless the processor alone
    /// computes.
    pub fn engine(program: PathBuf, prefix: Vec<OsString>, cfg: &crate::Config) -> Self {
        let placement = crate::Placement { accel: None, ..cfg.placement.clone() };
        let mut args = prefix;
        args.extend(crate::args(&crate::Config { placement, ..cfg.clone() }));
        let accel = cfg.placement.accel.clone().filter(|_| cfg.placement.gpu != Choice::Processor);
        Self { program, args, accel, env: Vec::new() }
    }
}

/// Failures of NVIDIA's engine in a row after which engines start without
/// it, until the person tries again ([`pack_failures`]).
pub const PACK_FAILURES: u32 = 2;

/// How many times in a row NVIDIA's engine in `dir` ended an engine or
/// failed a model ([`accel::CRASHES`]); 0 when it never did, or since it last
/// finished a job. Deleting the file is trying again.
pub fn pack_failures(dir: &Path) -> u32 {
    std::fs::read_to_string(dir.join(accel::CRASHES)).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0)
}

fn set_pack_failures(dir: &Path, n: u32) {
    let file = dir.join(accel::CRASHES);
    let written = if n == 0 { std::fs::remove_file(&file) } else { std::fs::write(&file, n.to_string()) };
    if let Err(e) = written
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(error = %e, "couldn't note how NVIDIA's engine fared");
    }
}

/// Where a job's models run, as the engine reports it once they are loaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Devices {
    /// The device the engine tries first (`as_infer::Device::label`).
    pub preferred: String,
    /// Each model the job uses, and the device it runs on.
    pub models: Vec<(String, String)>,
}

impl Devices {
    fn new(preferred: String, models: Vec<ModelDevice>) -> Self {
        Self { preferred, models: models.into_iter().map(|m| (m.model, m.device)).collect() }
    }

    /// Models on the CPU although the engine prefers a GPU, which couldn't
    /// take them (no usable adapter, a driver reset, too little video
    /// memory): their part of the job runs many times slower.
    pub fn on_cpu(&self) -> Vec<&str> {
        let cpu = as_infer::Device::Cpu.label();
        if self.preferred == cpu {
            return Vec::new();
        }
        let mut on: Vec<&str> = Vec::new();
        for (m, d) in &self.models {
            if d == cpu && !on.contains(&m.as_str()) {
                on.push(m);
            }
        }
        on
    }

    /// The devices in one line, model by model in a fixed order: two runs
    /// computed alike exactly when theirs are equal.
    pub fn summary(&self) -> String {
        let mut each: Vec<String> = self.models.iter().map(|(m, d)| format!("{m} on {d}")).collect();
        each.sort();
        each.dedup();
        each.join(", ")
    }
}

/// What a shots job reports while it runs.
pub enum ShotsEvent {
    /// Where its models run, before the first progress of each engine that
    /// works on it, which resumes at frame `resume`.
    Devices {
        devices: Devices,
        resume: u32,
    },
    Progress(f64),
    /// Signals of frames `start..` (`as_analysis::signals` format).
    Chunk {
        start: u32,
        signals: Vec<u8>,
    },
}

/// What a faces job reports while it runs.
pub enum FacesEvent {
    /// Where its models run, before the first progress of each engine that
    /// works on it, which resumes at frame `resume`.
    Devices {
        devices: Devices,
        resume: u32,
    },
    Progress(f64),
    /// Face tracks of frames `start..end` (`as_analysis::tracks` format),
    /// the sprites of its shots and the people found there
    /// (`as_analysis::bodies` format; empty when none were looked for).
    Chunk {
        start: u32,
        end: u32,
        tracks: Vec<u8>,
        sprites: Vec<as_analysis::ShotSprite>,
        bodies: Vec<u8>,
    },
}

/// What a rescan reads.
#[derive(Clone, Debug)]
pub struct RescanInput {
    pub path: PathBuf,
    pub info: MediaInfo,
    /// `FrameIndex::to_bytes`.
    pub index: Vec<u8>,
    pub faces: Vec<RescanFaceIn>,
    pub fast: bool,
    pub anime: bool,
    /// Ask the other face model (see `RescanJob::second`).
    pub second: bool,
}

/// What a rescan reports.
#[derive(Clone, Debug)]
pub enum RescanEvent {
    Progress(f64),
    /// `embedding`: `as_analysis::tracks::embedding_to_bytes`, empty when no
    /// look read the face.
    Read {
        track: u32,
        embedding: Vec<u8>,
        quality: f32,
        tried: u32,
        found: u32,
    },
}

/// What a faces job looks at.
#[derive(Clone, Debug)]
pub struct FacesInput {
    pub path: PathBuf,
    pub info: MediaInfo,
    /// `FrameIndex::to_bytes`.
    pub index: Vec<u8>,
    /// Shot ranges, sorted.
    pub shots: Vec<(u32, u32)>,
    /// First frame to look at: 0 or the end of a chunk stored before.
    pub resume: u32,
    pub chunk: u32,
    /// Look for people too, tied to the faces.
    pub bodies: bool,
    /// Faces found before (`as_analysis::tracks::encode_known`): only
    /// people are looked for.
    pub known: Option<Vec<u8>>,
    /// 3D animation: faces are searched at more sizes
    /// (`as_analysis::FaceParams::animated`).
    pub animated: bool,
    /// Fast Scan: faces are found the way the Scener app finds them
    /// (`as_analysis::fast`); `bodies` and `animated` don't apply.
    pub fast: bool,
    /// Anime footage: heads are found and embedded the way
    /// `as_analysis::anime` does — with `fast`, in the frames Fast Scan
    /// picks; `bodies`, `known` and `animated` don't apply.
    pub anime: bool,
}

enum Msg {
    Reply(Reply),
    Exited,
}

struct Proc {
    child: Child,
    stdin: ChildStdin,
    replies: mpsc::Receiver<Msg>,
    device: String,
    /// The launch it was started with ([`Engine::set_launch`] counts them).
    generation: u64,
    /// The folder of NVIDIA's engine it was started with.
    pack: Option<PathBuf>,
    /// It said NVIDIA's engine is in use ([`AccelState::Active`]): it was
    /// registered and serves the card in use.
    pack_active: bool,
}

impl Proc {
    fn send(&mut self, r: &Request) -> std::io::Result<()> {
        let mut line = serde_json::to_string(r).expect("requests always serialise");
        line.push('\n');
        self.stdin.write_all(line.as_bytes())?;
        self.stdin.flush()
    }

    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Kill (if still running) and reap.
    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Lines of a child's pipe, whatever their encoding (a stray non-UTF-8 line
/// must not end the reader: the child would block on a full pipe).
fn pipe_lines(r: impl Read) -> impl Iterator<Item = String> {
    let mut r = BufReader::new(r);
    std::iter::from_fn(move || {
        let mut buf = Vec::new();
        match r.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => None,
            Ok(_) => {
                while buf.last().is_some_and(|&b| b == b'\n' || b == b'\r') {
                    buf.pop();
                }
                Some(String::from_utf8_lossy(&buf).into_owned())
            }
        }
    })
}

/// How long a cancelled job may take to stop before the engine is killed.
const CANCEL_GRACE: Duration = Duration::from_secs(10);
const START_TIMEOUT: Duration = Duration::from_secs(60);
/// Engine processes a job may go through before it fails.
const ATTEMPTS: usize = 3;

/// How an engine start went wrong.
enum StartError {
    /// It exited, or didn't say it was ready in time.
    Died(String),
    Other(EngineError),
}

/// How a job's engine fared with NVIDIA's engine.
#[derive(Default)]
struct PackRun {
    /// The folder it was started with.
    dir: Option<PathBuf>,
    /// NVIDIA's engine had a part in the job: it was in use, and a model of
    /// the job ran there (or may have, before the engine said where they run).
    used: bool,
    /// What ended the job it had a part in: the engine died under it, or a
    /// model failed on NVIDIA's engine.
    fault: Option<String>,
}

/// What [`Engine::watch`] hears about.
pub enum Note<'a> {
    /// An engine started and named the devices it found and opened.
    Ready(&'a Hardware),
    /// What [`Engine::pending`] or [`Engine::fell_back`] answers may have changed.
    Changed,
}

type Watch = Box<dyn Fn(Note<'_>) + Send + Sync>;

pub struct Engine {
    /// How to start the next engine, and how many launches there were.
    launch: Mutex<(Launch, u64)>,
    /// The running process; also serialises jobs.
    proc: Mutex<Option<Proc>>,
    next_id: AtomicU64,
    /// Process id of the running engine (0 = none), readable during a job.
    pid: AtomicU32,
    /// The launch of the engine running a job (0 = no job).
    busy: AtomicU64,
    /// What the last engine to start found and opened.
    hardware: Mutex<Option<Hardware>>,
    /// What became of NVIDIA's engine in the last engine to start.
    accel: Mutex<Option<AccelState>>,
    /// NVIDIA's engine failed this session: engines start without it.
    pack_broken: AtomicBool,
    /// Where the last job's models ran, as its engine last said.
    devices: Mutex<Option<Devices>>,
    /// Where the last job with a choice decoded, as its engine last said.
    decoder: Mutex<Option<Decoder>>,
    watch: OnceLock<Watch>,
}

impl Engine {
    pub fn new(launch: Launch) -> Self {
        Self {
            launch: Mutex::new((launch, 1)),
            proc: Mutex::new(None),
            next_id: AtomicU64::new(1),
            pid: AtomicU32::new(0),
            busy: AtomicU64::new(0),
            hardware: Mutex::new(None),
            accel: Mutex::new(None),
            pack_broken: AtomicBool::new(false),
            devices: Mutex::new(None),
            decoder: Mutex::new(None),
            watch: OnceLock::new(),
        }
    }

    /// What became of NVIDIA's engine in the last engine to start (`None`
    /// before one started): `Off` also when it started without it.
    pub fn accel(&self) -> Option<AccelState> {
        self.accel.lock().clone()
    }

    /// The folder of NVIDIA's engine the next engine starts with: the
    /// launch's, unless it failed this session or [`PACK_FAILURES`] times in
    /// a row.
    fn pack(&self, launch: &Launch) -> Option<PathBuf> {
        let dir = launch.accel.as_ref()?;
        let usable = !self.pack_broken.load(Ordering::Relaxed) && pack_failures(dir) < PACK_FAILURES;
        usable.then(|| dir.clone())
    }

    /// NVIDIA's engine in `dir` ended an engine or failed a model: count it,
    /// and start engines without it for the rest of this session.
    fn pack_failed(&self, dir: &Path, why: &str) {
        let n = pack_failures(dir).saturating_add(1);
        set_pack_failures(dir, n);
        self.pack_broken.store(true, Ordering::Relaxed);
        tracing::warn!(failures = n, "NVIDIA's engine failed ({why}); analysis runs without it");
        self.note(Note::Changed);
    }

    /// NVIDIA's engine in `dir` gets a new chance: the person tries it again,
    /// or new files arrived. Engines start with it once more, under the next
    /// launch.
    pub fn pack_try_again(&self, dir: &Path) {
        set_pack_failures(dir, 0);
        self.pack_broken.store(false, Ordering::Relaxed);
    }

    /// A job finished with NVIDIA's engine in `dir`: its failures in a row end.
    fn pack_worked(&self, dir: &Path) {
        if pack_failures(dir) > 0 {
            set_pack_failures(dir, 0);
        }
    }

    /// Hear when an engine names its hardware and when [`Engine::pending`]
    /// or [`Engine::fell_back`] may answer otherwise (once; later calls are
    /// ignored). Called on the thread that runs the job or the change.
    pub fn watch(&self, f: impl Fn(Note<'_>) + Send + Sync + 'static) {
        let _ = self.watch.set(Box::new(f));
    }

    fn note(&self, note: Note<'_>) {
        if let Some(f) = self.watch.get() {
            f(note);
        }
    }

    /// Start engines with `launch` from now on: an idle engine stops at once;
    /// a job that runs finishes on its engine, which then stops, so the next
    /// job (also one queued meanwhile) starts a new one with these
    /// arguments.
    pub fn set_launch(&self, launch: Launch) {
        {
            let mut current = self.launch.lock();
            *current = (launch, current.1 + 1);
        }
        // A job holds the lock for as long as it runs.
        if let Some(mut guard) = self.proc.try_lock()
            && let Some(mut p) = guard.take()
        {
            stop(&mut p);
            self.pid.store(0, Ordering::Relaxed);
        }
        if self.pending() {
            self.note(Note::Changed);
        }
    }

    /// The launch the next engine starts with, and its number.
    fn launch(&self) -> (Launch, u64) {
        self.launch.lock().clone()
    }

    /// A job runs on an engine started before the last [`Engine::set_launch`]:
    /// the change applies from the next one.
    pub fn pending(&self) -> bool {
        let busy = self.busy.load(Ordering::Relaxed);
        busy != 0 && busy != self.launch.lock().1
    }

    /// The devices the last engine to start found and opened (`None` before
    /// one started; [`Engine::probe`] starts one).
    pub fn hardware(&self) -> Option<Hardware> {
        self.hardware.lock().clone()
    }

    /// Some models of the last job ran on the CPU, which the GPU couldn't
    /// take ([`Devices::on_cpu`]).
    pub fn fell_back(&self) -> bool {
        self.devices.lock().as_ref().is_some_and(|d| !d.on_cpu().is_empty())
    }

    /// Where the last job with a choice (Fast Scan) started decoding, as
    /// its engine said; `None` before one did.
    pub fn decoder(&self) -> Option<Decoder> {
        *self.decoder.lock()
    }

    /// The devices an engine finds, starting one when none has started yet:
    /// it then waits for work until it is idle long enough to exit. While a
    /// job's engine starts, this waits for its answer instead.
    pub fn probe(&self, cancel: &CancelToken) -> Result<Hardware, EngineError> {
        loop {
            if let Some(h) = self.hardware() {
                return Ok(h);
            }
            if let Some(mut guard) = self.proc.try_lock() {
                if guard.as_mut().is_some_and(|p| !p.alive()) {
                    self.discard(&mut guard);
                }
                if guard.is_none() {
                    *guard = Some(self.start(cancel)?);
                }
                return self.hardware().ok_or_else(|| EngineError::Start {
                    message: couldnt_start(),
                    detail: "the engine named no hardware".into(),
                });
            }
            if cancel.is_cancelled() {
                return Err(EngineError::Cancelled);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Start an engine now, unless one is there or a job runs: what it finds
    /// reaches [`Engine::watch`] and [`Engine::accel`] without waiting for
    /// the next job. Its idle exit stops it.
    pub fn warm(&self, cancel: &CancelToken) -> Result<(), EngineError> {
        if let Some(mut guard) = self.proc.try_lock() {
            if guard.as_mut().is_some_and(|p| !p.alive()) {
                self.discard(&mut guard);
            }
            if guard.is_none() {
                *guard = Some(self.start(cancel)?);
            }
        }
        Ok(())
    }

    /// Where the job's models run, as its engine says: kept for
    /// [`Engine::fell_back`].
    fn devices_seen(&self, devices: Devices) {
        let fell_back = !devices.on_cpu().is_empty();
        let before = self.devices.lock().replace(devices).is_some_and(|d| !d.on_cpu().is_empty());
        if fell_back != before {
            self.note(Note::Changed);
        }
    }

    /// Process id of the engine while one runs.
    pub fn pid(&self) -> Option<u32> {
        Some(self.pid.load(Ordering::Relaxed)).filter(|&p| p != 0)
    }

    /// Where the running engine prefers to compute (`None` before it first
    /// started; a model the GPU can't take runs on the CPU, which each job
    /// reports: `ShotsEvent::Devices`, `FacesEvent::Devices`).
    pub fn device(&self) -> Option<String> {
        self.proc.lock().as_ref().map(|p| p.device.clone())
    }

    /// Measure the shot signals of a media from `resume` on. Blocks until the
    /// job ends; chunks arrive through `on` in order.
    #[allow(clippy::too_many_arguments)]
    pub fn shots(
        &self,
        path: PathBuf,
        info: MediaInfo,
        index: Vec<u8>,
        resume: u32,
        chunk: u32,
        cancel: &CancelToken,
        mut on: impl FnMut(ShotsEvent) -> Result<(), String>,
    ) -> Result<(), EngineError> {
        let resume = Cell::new(resume);
        let index = Blob(index);
        self.with_retries(
            cancel,
            |id| {
                let (path, info, index, resume) = (path.clone(), info.clone(), index.clone(), resume.get());
                Request::Shots(Box::new(ShotsJob { id, path, info, index, resume, chunk }))
            },
            |reply| match reply {
                Reply::Devices { preferred, models, .. } => {
                    on(ShotsEvent::Devices { devices: Devices::new(preferred, models), resume: resume.get() })
                }
                Reply::Progress { fraction, .. } => on(ShotsEvent::Progress(fraction)),
                Reply::ShotChunk { start, len, signals, .. } => {
                    on(ShotsEvent::Chunk { start, signals: signals.0 })?;
                    // Only what was stored counts as done.
                    resume.set(start + len);
                    Ok(())
                }
                _ => Ok(()),
            },
        )
    }

    /// Find the face tracks of a media's shots (and the people tied to
    /// them, or the people alone for faces found before) from `job.resume`
    /// on. Blocks until the job ends; chunks arrive through `on` in order.
    pub fn faces(
        &self,
        job: FacesInput,
        cancel: &CancelToken,
        mut on: impl FnMut(FacesEvent) -> Result<(), String>,
    ) -> Result<(), EngineError> {
        let resume = Cell::new(job.resume);
        let (index, known) = (Blob(job.index), job.known.map(Blob));
        self.with_retries(
            cancel,
            |id| {
                Request::Faces(Box::new(FacesJob {
                    id,
                    path: job.path.clone(),
                    info: job.info.clone(),
                    index: index.clone(),
                    shots: job.shots.clone(),
                    resume: resume.get(),
                    chunk: job.chunk,
                    bodies: job.bodies,
                    known: known.clone(),
                    animated: job.animated,
                    fast: job.fast,
                    anime: job.anime,
                }))
            },
            |reply| match reply {
                Reply::Devices { preferred, models, .. } => {
                    on(FacesEvent::Devices { devices: Devices::new(preferred, models), resume: resume.get() })
                }
                Reply::Progress { fraction, .. } => on(FacesEvent::Progress(fraction)),
                Reply::FaceChunk { start, end, tracks, sprites, bodies, .. } => {
                    let sprites = sprites
                        .into_iter()
                        .map(|s| as_analysis::ShotSprite { shot: s.shot, end: s.end, cells: s.cells, jpeg: s.jpeg.0 })
                        .collect();
                    on(FacesEvent::Chunk { start, end, tracks: tracks.0, sprites, bodies: bodies.0 })?;
                    resume.set(end);
                    Ok(())
                }
                _ => Ok(()),
            },
        )
    }

    /// Read faces again at the source's own resolution. Blocks until the job
    /// ends; a crashed engine goes on with the faces not read yet.
    pub fn rescan(
        &self,
        job: RescanInput,
        cancel: &CancelToken,
        mut on: impl FnMut(RescanEvent) -> Result<(), String>,
    ) -> Result<(), EngineError> {
        let done = std::cell::RefCell::new(std::collections::HashSet::new());
        let index = Blob(job.index);
        self.with_retries(
            cancel,
            |id| {
                let done = done.borrow();
                Request::Rescan(Box::new(RescanJob {
                    id,
                    path: job.path.clone(),
                    info: job.info.clone(),
                    index: index.clone(),
                    faces: job.faces.iter().filter(|f| !done.contains(&f.id)).cloned().collect(),
                    fast: job.fast,
                    anime: job.anime,
                    second: job.second,
                }))
            },
            |reply| match reply {
                Reply::Progress { fraction, .. } => on(RescanEvent::Progress(fraction)),
                Reply::FaceRead { track, embedding, quality, tried, found, .. } => {
                    done.borrow_mut().insert(track);
                    on(RescanEvent::Read { track, embedding: embedding.0, quality, tried, found })
                }
                _ => Ok(()),
            },
        )
    }

    /// Embed a still image (a character portrait) with the anime embedder —
    /// the direction AniList art points in is matched against grouped
    /// characters. Blocks until the vector arrives.
    pub fn embed(&self, path: PathBuf, cancel: &CancelToken) -> Result<Vec<f32>, EngineError> {
        let mut got = None;
        self.with_retries(
            cancel,
            |id| Request::Embed(Box::new(EmbedJob { id, path: path.clone() })),
            |reply| {
                if let Reply::Embedding { embedding, .. } = reply {
                    got = Some(embedding);
                }
                Ok(())
            },
        )?;
        got.ok_or(EngineError::Failed {
            message: msg!("engine.noEmbedding", "The engine gave no embedding."),
            detail: "an Embed job ended without a vector".into(),
        })
    }

    /// Run a job, giving it a fresh engine (and a fresh request, built from
    /// what has been stored so far) when the engine goes away under it.
    fn with_retries(
        &self,
        cancel: &CancelToken,
        make: impl Fn(u64) -> Request,
        mut on: impl FnMut(Reply) -> Result<(), String>,
    ) -> Result<(), EngineError> {
        let mut attempt = 0;
        // A failure of NVIDIA's engine costs no attempt, once per job.
        let mut without_pack = true;
        loop {
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            let mut pack = PackRun::default();
            let r = self.run(make(id), id, cancel, &mut on, &mut pack);
            match (&r, pack.dir, pack.fault) {
                (Ok(()), Some(dir), _) if pack.used => self.pack_worked(&dir),
                (Err(_), Some(dir), Some(why)) if !cancel.is_cancelled() => {
                    self.pack_failed(&dir, &why);
                    if without_pack {
                        without_pack = false;
                        tracing::warn!("running the job again without NVIDIA's engine");
                        continue;
                    }
                }
                _ => {}
            }
            match r {
                Err(EngineError::Crashed(why)) if attempt + 1 < ATTEMPTS && !cancel.is_cancelled() => {
                    attempt += 1;
                    tracing::warn!(%why, "engine went away; resuming in a fresh one");
                }
                r => return r,
            }
        }
    }

    /// Stop the engine (app shutdown). A running job fails with `Crashed`.
    pub fn shutdown(&self) {
        // Don't wait for a running job's lock: kill through a fresh handle.
        if let Some(mut guard) = self.proc.try_lock()
            && let Some(mut p) = guard.take()
        {
            stop(&mut p);
            self.pid.store(0, Ordering::Relaxed);
        }
        // A job holding the lock is cancelled by its own token on shutdown;
        // the process dies with the app either way (kill-on-close job object).
    }

    /// Kill and forget the engine in `slot`.
    fn discard(&self, slot: &mut Option<Proc>) {
        if let Some(mut p) = slot.take() {
            p.kill();
        }
        self.pid.store(0, Ordering::Relaxed);
    }

    fn run(
        &self,
        req: Request,
        id: u64,
        cancel: &CancelToken,
        on: &mut impl FnMut(Reply) -> Result<(), String>,
        pack: &mut PackRun,
    ) -> Result<(), EngineError> {
        let mut guard = self.proc.lock();
        // An engine of an earlier launch never takes a job, nor one with
        // NVIDIA's engine once it failed.
        let launch = self.launch.lock().1;
        let broken = self.pack_broken.load(Ordering::Relaxed);
        if guard.as_mut().is_some_and(|p| !p.alive() || p.generation != launch || (broken && p.pack.is_some())) {
            self.discard(&mut guard);
        }
        // Replies left over from an earlier job are stale; an engine that
        // exited since (idle) is replaced.
        if let Some(p) = guard.as_mut() {
            let mut exited = false;
            while let Ok(m) = p.replies.try_recv() {
                exited |= matches!(m, Msg::Exited);
            }
            if exited {
                self.discard(&mut guard);
            }
        }
        if guard.is_none() {
            *guard = Some(self.start(cancel)?);
        }
        let launch = guard.as_ref().expect("started").generation;
        pack.dir = guard.as_ref().expect("started").pack.clone();
        self.busy.store(launch, Ordering::Relaxed);
        let r = self.serve(&mut guard, req, id, cancel, on, pack);
        self.busy.store(0, Ordering::Relaxed);
        // A new launch came while the job ran: its engine stops now that it
        // is idle, so the next job starts one with the new arguments.
        if launch != self.launch.lock().1 {
            if let Some(mut p) = guard.take() {
                stop(&mut p);
                self.pid.store(0, Ordering::Relaxed);
            }
            self.note(Note::Changed);
        }
        r
    }

    /// Hand `req` to the engine in `slot` and relay its replies until the
    /// job ends. `pack` learns whether NVIDIA's engine had a part in the job,
    /// and what ended the job there, if anything did: the engine died under
    /// it, or a model failed there.
    fn serve(
        &self,
        slot: &mut Option<Proc>,
        req: Request,
        id: u64,
        cancel: &CancelToken,
        on: &mut impl FnMut(Reply) -> Result<(), String>,
        pack: &mut PackRun,
    ) -> Result<(), EngineError> {
        let p = slot.as_mut().expect("started");
        // Until the engine says where the job's models run: whether one may
        // run on NVIDIA's engine.
        let active = p.pack_active;
        pack.used = active && crate::server::job_may_use_accel(&req);
        if let Err(e) = p.send(&req) {
            self.discard(slot);
            if pack.used {
                pack.fault = Some(format!("the engine with it didn't take the job: {e}"));
            }
            return Err(EngineError::Crashed(format!("sending the job: {e}")));
        }
        // A model failed on NVIDIA's engine; the job's failure follows.
        let mut accel_failed: Option<String> = None;
        let mut cancel_sent: Option<Instant> = None;
        let mut handler_error: Option<String> = None;
        loop {
            let p = slot.as_mut().expect("running");
            if (cancel.is_cancelled() || handler_error.is_some()) && cancel_sent.is_none() {
                let _ = p.send(&Request::Cancel { id });
                cancel_sent = Some(Instant::now());
            }
            if cancel_sent.is_some_and(|t| t.elapsed() > CANCEL_GRACE) {
                tracing::warn!(id, "engine didn't stop after cancel; killing it");
                self.discard(slot);
                return Err(handler_error.map_or(EngineError::Cancelled, EngineError::saving));
            }
            match p.replies.recv_timeout(Duration::from_millis(100)) {
                Ok(Msg::Reply(r)) => match r {
                    Reply::Done { id: rid } if rid == id => {
                        return handler_error.map_or(Ok(()), |e| Err(EngineError::saving(e)));
                    }
                    Reply::Failed { id: rid, message, msg, detail, cancelled } if rid == id => {
                        return Err(match (handler_error, cancelled) {
                            (Some(e), _) => EngineError::saving(e),
                            (None, true) => EngineError::Cancelled,
                            (None, false) => {
                                pack.fault = accel_failed;
                                EngineError::Failed { message: said(message, msg), detail }
                            }
                        });
                    }
                    Reply::AccelFailed { id: rid, detail } if rid == id => {
                        tracing::warn!(id, %detail, "a model failed on NVIDIA's engine");
                        accel_failed = Some(format!("a model failed on it: {detail}"));
                    }
                    Reply::Fatal { message, msg, detail } => {
                        self.discard(slot);
                        return Err(EngineError::Start { message: said(message, msg), detail });
                    }
                    Reply::Decoding { id: rid, decoder } if rid == id => {
                        *self.decoder.lock() = Some(decoder);
                    }
                    r @ (Reply::Devices { id: rid, .. }
                    | Reply::Progress { id: rid, .. }
                    | Reply::ShotChunk { id: rid, .. }
                    | Reply::FaceChunk { id: rid, .. }
                    | Reply::FaceRead { id: rid, .. }
                    | Reply::Embedding { id: rid, .. })
                        if rid == id =>
                    {
                        if let Reply::Devices { preferred, models, .. } = &r {
                            let nvidia = as_infer::Device::NvRtx.label();
                            pack.used = active && models.iter().any(|m| m.device == nvidia);
                            self.devices_seen(Devices::new(preferred.clone(), models.clone()));
                        }
                        if handler_error.is_none()
                            && let Err(e) = on(r)
                        {
                            handler_error = Some(e);
                        }
                    }
                    _ => {}
                },
                Ok(Msg::Exited) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    let mut p = slot.take().expect("running");
                    p.kill();
                    let status = p.child.wait().map(|s| s.to_string()).unwrap_or_else(|e| e.to_string());
                    self.pid.store(0, Ordering::Relaxed);
                    // A failed save outranks the crash: retrying past an
                    // unsaved chunk would leave a hole.
                    return Err(match handler_error {
                        Some(e) => EngineError::saving(e),
                        None if cancel.is_cancelled() => EngineError::Cancelled,
                        None => {
                            if pack.used {
                                pack.fault = Some(format!("the engine with it exited under a job ({status})"));
                            }
                            EngineError::Crashed(format!("exited ({status})"))
                        }
                    });
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }

    /// Start an engine, with NVIDIA's engine while it works: one started
    /// with it that dies before it is ready counts against it, and an engine
    /// without it starts right away.
    fn start(&self, cancel: &CancelToken) -> Result<Proc, EngineError> {
        let (launch, generation) = self.launch();
        if let Some(dir) = self.pack(&launch) {
            match self.spawn(&launch, generation, Some(dir.clone()), cancel) {
                Ok(p) => return Ok(p),
                Err(StartError::Died(why)) if !cancel.is_cancelled() => {
                    self.pack_failed(&dir, &format!("the engine with it ended before it was ready: {why}"));
                }
                Err(StartError::Died(_)) => return Err(EngineError::Cancelled),
                Err(StartError::Other(e)) => return Err(e),
            }
        }
        self.spawn(&launch, generation, None, cancel).map_err(|e| match e {
            StartError::Died(detail) => EngineError::Start { message: couldnt_start(), detail },
            StartError::Other(e) => e,
        })
    }

    /// Start an engine with `launch` (and NVIDIA's engine in `pack`), and
    /// wait until it is ready.
    fn spawn(
        &self,
        launch: &Launch,
        generation: u64,
        pack: Option<PathBuf>,
        cancel: &CancelToken,
    ) -> Result<Proc, StartError> {
        let mut cmd = Command::new(&launch.program);
        cmd.args(&launch.args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        if let Some(dir) = &pack {
            cmd.arg("--accel").arg(dir);
        }
        cmd.envs(launch.env.iter().map(|(k, v)| (k, v)));
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = as_media::proc::spawn(&mut cmd).map_err(|e| {
            StartError::Other(EngineError::Start {
                message: msg!("engine.reinstall", "The analysis engine couldn't start. Reinstall AutoScene."),
                detail: format!("{}: {e}", launch.program.display()),
            })
        })?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        // The engine logs to stderr; its lines go into the app's log.
        std::thread::spawn(move || {
            for line in pipe_lines(stderr) {
                tracing::info!(target: "engine", "{line}");
            }
        });
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in pipe_lines(stdout) {
                match serde_json::from_str::<Reply>(&line) {
                    Ok(r) => {
                        if tx.send(Msg::Reply(r)).is_err() {
                            return;
                        }
                    }
                    Err(e) => tracing::warn!(target: "engine", error = %e, "unreadable reply: {line}"),
                }
            }
            let _ = tx.send(Msg::Exited);
        });
        let mut p = Proc { child, stdin, replies: rx, device: String::new(), generation, pack, pack_active: false };
        let deadline = Instant::now() + START_TIMEOUT;
        loop {
            if cancel.is_cancelled() {
                p.kill();
                return Err(StartError::Other(EngineError::Cancelled));
            }
            let left = deadline.saturating_duration_since(Instant::now());
            match p.replies.recv_timeout(left.min(Duration::from_millis(100))) {
                Ok(Msg::Reply(Reply::Ready { device, hardware, accel })) => {
                    tracing::info!(%device, pid = p.child.id(), ?accel, "analysis engine ready on {hardware}");
                    self.pid.store(p.child.id(), Ordering::Relaxed);
                    p.device = device;
                    p.pack_active = p.pack.is_some() && accel == AccelState::Active;
                    *self.hardware.lock() = Some(hardware.clone());
                    *self.accel.lock() = Some(accel);
                    self.note(Note::Ready(&hardware));
                    return Ok(p);
                }
                Ok(Msg::Reply(Reply::Fatal { message, msg, detail })) => {
                    p.kill();
                    return Err(StartError::Other(EngineError::Start { message: said(message, msg), detail }));
                }
                Ok(Msg::Reply(_)) => {}
                Err(mpsc::RecvTimeoutError::Timeout) if !left.is_zero() => {}
                Ok(Msg::Exited) | Err(_) => {
                    p.kill();
                    let status = p.child.wait().map(|s| s.to_string()).unwrap_or_else(|e| e.to_string());
                    return Err(StartError::Died(if left.is_zero() {
                        "no ready message in time".into()
                    } else {
                        format!("no ready message: it exited ({status})")
                    }));
                }
            }
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Ask an engine to exit, and kill it if it hasn't after two seconds.
fn stop(p: &mut Proc) {
    let _ = p.send(&Request::Shutdown);
    let deadline = Instant::now() + Duration::from_secs(2);
    while p.alive() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    p.kill();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_error_has_its_key_and_keeps_its_words() {
        let said = [
            (
                EngineError::Start { message: couldnt_start(), detail: "x".into() },
                "engine.couldntStart",
                "The analysis engine couldn't start. Details are in the log.",
            ),
            (
                EngineError::Crashed("x".into()),
                "engine.crashed",
                "The analysis engine stopped unexpectedly. Details are in the log.",
            ),
            (EngineError::saving("x".into()), "engine.savingFailed", "Saving the analysis failed."),
            (EngineError::Cancelled, "engine.cancelled", "Cancelled."),
        ];
        for (e, key, english) in said {
            assert_eq!((e.msg().key.as_str(), e.to_string().as_str()), (key, english));
        }
    }

    fn devices(preferred: &str, models: &[(&str, &str)]) -> Devices {
        Devices { preferred: preferred.into(), models: models.iter().map(|&(m, d)| (m.into(), d.into())).collect() }
    }

    #[test]
    fn models_the_gpu_couldnt_take_are_named() {
        let gpu = as_infer::Device::DirectMl.label();
        let all = devices(gpu, &[("scrfd-10g", gpu), ("scrfd-10g", gpu), ("arcface-glintr100", gpu)]);
        assert!(all.on_cpu().is_empty());
        let fell_back = devices(
            gpu,
            &[("scrfd-10g", gpu), ("scrfd-10g", "CPU"), ("scrfd-10g", "CPU"), ("arcface-glintr100", "CPU")],
        );
        assert_eq!(fell_back.on_cpu(), ["scrfd-10g", "arcface-glintr100"]);
        // An engine told to use the CPU runs nothing there against its will.
        assert!(devices("CPU", &[("transnetv2", "CPU")]).on_cpu().is_empty());
        // The same devices, in any order and however many looks: the same summary.
        assert_eq!(
            devices(gpu, &[("arcface-glintr100", "CPU"), ("scrfd-10g", gpu)]).summary(),
            devices(gpu, &[("scrfd-10g", gpu), ("scrfd-10g", gpu), ("arcface-glintr100", "CPU")]).summary()
        );
        assert_ne!(all.summary(), fell_back.summary());
        assert_eq!(all.summary(), "arcface-glintr100 on GPU (DirectML), scrfd-10g on GPU (DirectML)");
    }

    #[test]
    fn two_failures_in_a_row_turn_the_pack_off_until_try_again() {
        let dir = tempfile::tempdir().unwrap();
        let launch = Launch { accel: Some(dir.path().to_path_buf()), ..Launch::default() };
        let session = Engine::new(launch.clone());
        assert_eq!(session.pack(&launch).as_deref(), Some(dir.path()), "never failed: used");
        // It fails once: off for the rest of this session, and counted.
        session.pack_failed(dir.path(), "exited");
        assert_eq!((session.pack(&launch), pack_failures(dir.path())), (None, 1));
        // The next session tries it again; it fails again: off for good.
        let next = Engine::new(launch.clone());
        assert!(next.pack(&launch).is_some());
        next.pack_failed(dir.path(), "a model failed");
        assert_eq!(pack_failures(dir.path()), 2);
        assert_eq!(Engine::new(launch.clone()).pack(&launch), None, "two in a row: engines start without it");
        // Trying again deletes the count.
        std::fs::remove_file(dir.path().join(accel::CRASHES)).unwrap();
        let again = Engine::new(launch.clone());
        assert!(again.pack(&launch).is_some());
        // A failure, then a job it finishes: the failures in a row end.
        again.pack_failed(dir.path(), "exited");
        again.pack_worked(dir.path());
        assert_eq!(pack_failures(dir.path()), 0);
        assert!(!dir.path().join(accel::CRASHES).exists());
        // Without a folder there is nothing to use.
        let none = Launch::default();
        assert_eq!(Engine::new(none.clone()).pack(&none), None);
    }

    #[test]
    fn a_launch_carries_the_pack_beside_its_arguments() {
        let cfg = crate::Config {
            ort_dir: "ort".into(),
            models_dir: "models".into(),
            ffmpeg_dir: None,
            placement: crate::Placement { accel: Some("runtimes/nvidia".into()), ..crate::Placement::default() },
            idle_exit: None,
        };
        let launch = Launch::engine("autoscene".into(), vec!["--engine".into()], &cfg);
        assert_eq!(launch.accel.as_deref(), Some(Path::new("runtimes/nvidia")));
        assert_eq!(launch.args.first().map(OsString::as_os_str), Some(std::ffi::OsStr::new("--engine")));
        // Only the client adds it, while it works.
        assert!(!launch.args.iter().any(|a| a == "--accel"), "{:?}", launch.args);
        // A card chosen: it goes along, the engine decides whether it serves it.
        let card = Choice::Card { vendor: 0x1002, device: 0x73BF, nth: 0 };
        let chosen =
            crate::Config { placement: crate::Placement { gpu: card, ..cfg.placement.clone() }, ..cfg.clone() };
        assert!(Launch::engine("autoscene".into(), Vec::new(), &chosen).accel.is_some());
        // "Processor only" never uses it, so it never starts with it: a crash
        // there is never the pack's.
        let cpu = crate::Config {
            placement: crate::Placement { gpu: Choice::Processor, ..cfg.placement.clone() },
            ..cfg.clone()
        };
        assert_eq!(Launch::engine("autoscene".into(), Vec::new(), &cpu).accel, None);
    }

    #[test]
    fn pipe_lines_survive_odd_bytes() {
        let raw: &[u8] = b"one\r\n\xff\xfe bad\ntwo";
        let lines: Vec<String> = pipe_lines(raw).collect();
        assert_eq!(lines, ["one", "\u{fffd}\u{fffd} bad", "two"]);
    }
}

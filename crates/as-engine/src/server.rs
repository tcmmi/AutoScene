//! The engine process: reads requests from stdin, runs one job at a time on
//! the GPU (models stay loaded between jobs), streams replies to stdout.
//!
//! It owns no state on disk. Everything a job produces goes back to the app,
//! which stores it as it arrives — so the engine can be killed at any moment
//! and the job resumes from the app's last checkpoint.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use as_analysis::rescan::{self, FaceRead, HeadFinder, LiveFinder, RescanFace, RescanParams, SecondOpinion};
use as_analysis::{
    AnalysisError, BodyModels, DETECT_BATCH, FaceChunk, FaceParams, Faces, FastModels, FastParams, Look, PERSON_INPUT,
    PeopleParams, Search, anime, bodies, detect_looks, face_tracks, fast, fast_face_tracks, frame_sizes, shot_signals,
    signals, tracks,
};
use as_core::{CancelToken, FrameIndex, FrameRange, ToMsg, msg};
use as_infer::accel::AccelState;
use as_infer::computer::{Choice, NVIDIA};
use as_infer::hardware::{self, Hardware, Using};
use as_infer::{Device, InferError, Runtime};
use as_media::frames::Decoder;
use as_media::{Source, Tools, frames};
use as_models::{ModelStore, ids};
use as_vision::{ArcFace, Berkeley, Ccip, Fit, Norm, PersonReid, PpDet, SFace, Scrfd, TransNet, Yolo, YuNet};
use parking_lot::Mutex;

use crate::protocol::{Blob, EmbedJob, FacesJob, ModelDevice, Reply, Request, RescanJob, ShotsJob, Sprite};

/// Faces per embedding call.
const EMBED_BATCH: usize = 32;
/// People per appearance (ReID) call.
const REID_BATCH: usize = 32;
/// Fast Scan: frames per detector call — YuNet's, or on anime the head and
/// person detectors' (it looks at one frame at a time, and decides from
/// what it found where to look next) — and faces per SFace call.
const FAST_DETECT_BATCH: usize = 1;
const FAST_EMBED_BATCH: usize = 16;
/// Crops per CCIP call (a 384² crop is ~12× a face's): one session embeds
/// heads and implied heads alike.
pub const CCIP_BATCH: usize = 4;
/// Crops per Berkeley inference: the Best scan embeds a few heads per
/// track, a shot's worth at a time.
const BERKELEY_BATCH: usize = 8;
/// The anime head detector's letterboxed input (the bench's first-config
/// variant).
const ANIME_HEAD_INPUT: (usize, usize) = (640, 640);
/// The anime person detector's input: it looks at a bigger picture to find
/// characters whose heads are too small for the head detector.
const ANIME_PERSON_INPUT: (usize, usize) = (960, 544);

/// Whether the live-action models (faces and people) may run on NVIDIA's
/// engine. Off until a run of the real engine shows them faster there with
/// the identity bench unchanged: on its own, the face detector is no faster
/// there and drifts more (design-i66_67 §2.4, C4).
const LIVE_ON_ACCEL: bool = false;

/// Whether model `id` may run on NVIDIA's engine, the live-action models
/// only when `live` says so. The anime models may: they compute there what
/// the processor computes, and CCIP proves it against its goldens at every
/// load. TransNetV2 never does (the engine can't take one of its nodes), nor
/// does Fast Scan's YuNet and SFace (SFace's embeddings bend there). Where
/// a model runs no longer depends on which job loaded it first.
fn may_use_accel(id: &str, live: bool) -> bool {
    match id {
        ids::ANIME_HEAD | ids::ANIME_PERSON | ids::ANIME_CCIP => true,
        ids::SCRFD | ids::ARCFACE | ids::PERSONS | ids::PERSON_REID => live,
        _ => false,
    }
}

/// Whether job `req` may run a model on NVIDIA's engine ([`may_use_accel`]):
/// anime faces and embeddings, and live-action faces once they are let on
/// it. Shots and Fast Scan's own faces never do. Until the job's engine
/// says where its models run, the app counts a death under the job against
/// NVIDIA's engine only when this says it may have had a part in it.
pub fn job_may_use_accel(req: &Request) -> bool {
    match req {
        Request::Faces(j) => j.anime || (LIVE_ON_ACCEL && !j.fast),
        Request::Rescan(j) => j.anime || (LIVE_ON_ACCEL && !j.fast),
        Request::Embed(_) => true,
        Request::Shots(_) | Request::Cancel { .. } | Request::Shutdown => false,
    }
}

/// Live-action models that landed on NVIDIA's engine and on another device:
/// a pass that mixes the two is slower than one on either alone, so the
/// family is loaded again without it.
fn split(devices: &[Device]) -> bool {
    devices.contains(&Device::NvRtx) && devices.iter().any(|d| *d != Device::NvRtx)
}

/// How the engine finds its runtime, models and tools.
#[derive(Clone, Debug)]
pub struct Config {
    /// Folder with the ONNX Runtime library.
    pub ort_dir: PathBuf,
    /// The model store (`AppPaths::models()`).
    pub models_dir: PathBuf,
    /// Bundled ffmpeg folder, if any (otherwise `AUTOSCENE_FFMPEG_DIR` / PATH).
    pub ffmpeg_dir: Option<PathBuf>,
    /// The device the models run on (`--gpu KEY`, `--cpu`), and where their
    /// compiles are kept between runs (`--cache DIR`).
    pub placement: as_infer::Placement,
    /// Exit after this long without a job, which frees the GPU memory.
    pub idle_exit: Option<Duration>,
}

/// Shared stdout: one reply per line, flushed at once.
#[derive(Clone)]
struct Out {
    w: Arc<Mutex<Box<dyn Write + Send>>>,
    /// A model of the running job runs on NVIDIA's engine ([`send_devices`]).
    on_accel: Arc<AtomicBool>,
}

impl Out {
    fn new(w: Box<dyn Write + Send>) -> Self {
        Self { w: Arc::new(Mutex::new(w)), on_accel: Arc::default() }
    }

    fn send(&self, reply: &Reply) {
        let mut line = serde_json::to_string(reply).expect("replies always serialise");
        line.push('\n');
        {
            let mut w = self.w.lock();
            // A broken pipe means the app is gone; the reader thread ends the loop.
            let _ = w.write_all(line.as_bytes()).and_then(|()| w.flush());
        }
        if let Some(ms) = pause_at_first_chunk()
            && matches!(reply, Reply::ShotChunk { start: 0, .. } | Reply::FaceChunk { start: 0, .. })
        {
            std::thread::sleep(Duration::from_millis(ms));
        }
    }
}

/// How long a debug build's engine waits after a job's first chunk when a
/// test asks (`AUTOSCENE_TEST_PAUSE_AT_FIRST_CHUNK`, in milliseconds): a
/// test that kills the engine mid-job then kills it there however fast the
/// machine is (a small fixture is otherwise done before the kill lands).
fn pause_at_first_chunk() -> Option<u64> {
    static PAUSE: std::sync::OnceLock<Option<u64>> = std::sync::OnceLock::new();
    *PAUSE.get_or_init(|| {
        if !cfg!(debug_assertions) {
            return None;
        }
        std::env::var("AUTOSCENE_TEST_PAUSE_AT_FIRST_CHUNK").ok()?.parse().ok()
    })
}

/// Tell the app where job `id`'s models run (each model's id and device),
/// before its first progress. A model the GPU couldn't take runs on the
/// CPU, much slower: the app shows it with the job, not only in the log.
fn send_devices(out: &Out, id: u64, rt: &Runtime, models: &[(&str, Device)]) {
    let preferred = rt.preferred_device();
    let on_cpu: Vec<&str> =
        models.iter().filter(|(_, d)| *d == Device::Cpu && preferred != Device::Cpu).map(|(m, _)| *m).collect();
    if !on_cpu.is_empty() {
        tracing::warn!(id, ?on_cpu, preferred = ?preferred, "the GPU couldn't take every model; these run on the CPU");
    }
    if models.iter().any(|(_, d)| *d == Device::NvRtx) {
        out.on_accel.store(true, Ordering::Relaxed);
    }
    out.send(&Reply::Devices {
        id,
        preferred: preferred.label().into(),
        models: models.iter().map(|(m, d)| ModelDevice { model: (*m).into(), device: d.label().into() }).collect(),
    });
}

/// Run the engine until stdin closes, a `shutdown` arrives, or it has been
/// idle for `idle_exit`. Returns the process exit code.
pub fn serve(cfg: Config, input: impl BufRead + Send + 'static, output: impl Write + Send + 'static) -> i32 {
    let out = Out::new(Box::new(output));
    let crash = crash_hook(&cfg);
    let tools = match Tools::locate(cfg.ffmpeg_dir.as_deref()) {
        Ok(t) => t,
        Err(e) => {
            out.send(&Reply::fatal(e.msg(), e.detail()));
            return 1;
        }
    };
    // Before any session: no build is using a folder yet.
    if let Some(cache) = &cfg.placement.cache {
        let pruned = as_infer::cache::prune(cache, std::time::SystemTime::now());
        if pruned.folders > 0 {
            tracing::info!(
                folders = pruned.folders,
                mb = pruned.bytes / 1_000_000,
                "deleted compiled models unused for 30 days"
            );
        }
    }
    let rt = match Runtime::load_with(&cfg.ort_dir, &cfg.placement) {
        Ok(rt) => rt,
        Err(e) => {
            out.send(&Reply::fatal(e.msg(), e.detail()));
            return 1;
        }
    };
    if crash == Some(CrashAt::Start) {
        std::process::exit(CRASH_EXIT);
    }
    // The test hook's crash under a job stands for NVIDIA's engine failing
    // in use: the engine says it is.
    let accel = if crash == Some(CrashAt::Job) { AccelState::Active } else { rt.accel_state().clone() };
    out.send(&Reply::Ready {
        device: rt.preferred_device().label().to_string(),
        hardware: rt.hardware().clone(),
        accel,
    });

    // Jobs queue up here; cancellation needs no queue and is handled at once.
    let (tx, rx) = mpsc::channel::<(Request, CancelToken)>();
    let cancels: Arc<Mutex<HashMap<u64, CancelToken>>> = Arc::default();
    let reader = {
        let cancels = cancels.clone();
        let out = out.clone();
        std::thread::spawn(move || read_requests(input, tx, cancels, out))
    };

    let mut models = Models {
        store: ModelStore::new(&cfg.models_dir),
        plain: rt.without_accel(),
        live_accel_off: false,
        gpu: cfg.placement.gpu,
        decoder: None,
        transnet: None,
        scrfd: Vec::new(),
        arcface: None,
        persons: None,
        person_reid: None,
        yunet: Vec::new(),
        sface: None,
        anime_heads: None,
        anime_persons: None,
        anime_ccip: None,
        anime_berkeley: None,
    };
    loop {
        let next = match cfg.idle_exit {
            Some(idle) => rx.recv_timeout(idle).map_err(|e| e == mpsc::RecvTimeoutError::Timeout),
            None => rx.recv().map_err(|_| false),
        };
        let (job, cancel) = match next {
            Ok(j) => j,
            Err(true) => {
                tracing::info!("idle; exiting to free the GPU");
                break;
            }
            Err(false) => break,
        };
        let id = job.id().expect("only jobs are queued");
        if crash == Some(CrashAt::Job) {
            std::process::exit(CRASH_EXIT);
        }
        out.on_accel.store(false, Ordering::Relaxed);
        let run = guarded(|| match job {
            Request::Shots(j) => run_shots(&tools, &rt, &mut models, *j, &cancel, &out),
            Request::Faces(j) => run_faces(&tools, &rt, &mut models, *j, &cancel, &out),
            Request::Embed(j) => run_embed(&tools, &rt, &mut models, *j, &cancel, &out),
            Request::Rescan(j) => run_rescan(&tools, &rt, &mut models, *j, &cancel, &out),
            Request::Cancel { .. } | Request::Shutdown => Ok(()),
        });
        let result = run.unwrap_or_else(|panic| {
            // A model may have been left mid-batch: the next job loads fresh ones.
            models.unload();
            Err(AnalysisError::Internal(format!("panicked: {panic}")))
        });
        let reply = match result {
            Ok(()) => Reply::Done { id },
            Err(e) => {
                if !e.is_cancelled() {
                    tracing::warn!(id, error = %e.detail(), "job failed");
                }
                // A model failed while it ran, and some ran on NVIDIA's
                // engine: the app runs the job again without it.
                if out.on_accel.load(Ordering::Relaxed) && matches!(e, AnalysisError::Infer(InferError::Run(_))) {
                    out.send(&Reply::AccelFailed { id, detail: e.detail() });
                }
                Reply::failed(id, e.msg(), e.detail(), e.is_cancelled())
            }
        };
        cancels.lock().remove(&id);
        out.send(&reply);
    }
    // Pending cancellations of jobs that never ran: nothing to report. The
    // reader thread ends with stdin.
    drop(reader);
    0
}

/// Where a debug build's engine started with NVIDIA's engine exits on
/// purpose (`AUTOSCENE_TEST_CRASH_WITH_ACCEL`), as a native crash of the
/// plugin would end it: before it is ready, or at its first job (it then
/// says NVIDIA's engine is in use, whatever became of it). For the tests of
/// the app's side, which must start the next engine without it, and only
/// for a job it may have had a part in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CrashAt {
    Start,
    Job,
}

/// The exit code of [`CrashAt`]'s exits.
const CRASH_EXIT: i32 = 70;

fn crash_hook(cfg: &Config) -> Option<CrashAt> {
    if !cfg!(debug_assertions) || cfg.placement.accel.is_none() {
        return None;
    }
    match std::env::var("AUTOSCENE_TEST_CRASH_WITH_ACCEL").ok()?.as_str() {
        "start" => Some(CrashAt::Start),
        "job" => Some(CrashAt::Job),
        _ => None,
    }
}

/// Run one job; what it panicked with, if it did. A bug in one job's pre- or
/// post-processing must not take the engine down: the app would restart it
/// and run the same job into the same panic three times, and the person would
/// only see "the engine stopped". The job fails with its own error instead.
fn guarded(job: impl FnOnce() -> Result<(), AnalysisError>) -> Result<Result<(), AnalysisError>, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)).map_err(|panic| {
        panic
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| panic.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".into())
    })
}

fn read_requests(
    input: impl BufRead,
    tx: mpsc::Sender<(Request, CancelToken)>,
    cancels: Arc<Mutex<HashMap<u64, CancelToken>>>,
    out: Out,
) {
    // Cancels that arrive before their job (a race the app can't rule out).
    let mut early: HashSet<u64> = HashSet::new();
    for line in input.lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Request>(&line) {
            Ok(job @ (Request::Shots(_) | Request::Faces(_) | Request::Embed(_) | Request::Rescan(_))) => {
                let id = job.id().expect("a job");
                let cancel = CancelToken::new();
                if early.remove(&id) {
                    cancel.cancel();
                }
                cancels.lock().insert(id, cancel.clone());
                if tx.send((job, cancel)).is_err() {
                    break;
                }
            }
            Ok(Request::Cancel { id }) => match cancels.lock().get(&id) {
                Some(c) => c.cancel(),
                None => {
                    early.insert(id);
                }
            },
            Ok(Request::Shutdown) => break,
            Err(e) => {
                tracing::error!(error = %e, "bad request");
                out.send(&Reply::fatal(
                    msg!("engine.badRequest", "The analysis engine got a bad request."),
                    e.to_string(),
                ));
            }
        }
    }
    // Leaving: whatever runs now should stop, and the loop ends after it.
    for c in cancels.lock().values() {
        c.cancel();
    }
}

struct Models {
    store: ModelStore,
    /// The runtime without NVIDIA's engine, for the models never allowed on
    /// it ([`may_use_accel`]).
    plain: Runtime,
    /// The live-action models landed on NVIDIA's engine and elsewhere once:
    /// they stay off it for this engine's life ([`split`]).
    live_accel_off: bool,
    /// The graphics the person chose.
    gpu: Choice,
    /// Where Fast Scan decodes ([`Models::decoder`]), once a job asked.
    decoder: Option<Decoder>,
    transnet: Option<TransNet>,
    /// One face detector per look of the last faces job.
    scrfd: Vec<Scrfd>,
    arcface: Option<ArcFace>,
    persons: Option<PpDet>,
    person_reid: Option<PersonReid>,
    /// Fast Scan's detectors, one planned for each size the last media's
    /// frames are looked at, and embedder.
    yunet: Vec<YuNet>,
    sface: Option<SFace>,
    /// The anime models: head and person detectors planned for the last
    /// media's frame size, and the character embedder (also serves `Embed`
    /// jobs on stills).
    anime_heads: Option<Yolo>,
    anime_persons: Option<Yolo>,
    anime_ccip: Option<Ccip>,
    /// The Best anime scan's character embedder.
    anime_berkeley: Option<Berkeley>,
}

/// Which family of models a job keeps loaded; the other family's sessions
/// are dropped so the two passes never hold the GPU at once.
enum Family {
    Live,
    Anime,
}

impl Models {
    /// Where Fast Scan decodes on this engine ([`fast_decoder`]), asked of
    /// the driver only when a job needs it.
    fn decoder(&mut self, rt: &Runtime) -> Decoder {
        *self.decoder.get_or_insert_with(|| fast_decoder(&self.gpu, rt.hardware(), hardware::cuda_device))
    }

    fn unload(&mut self) {
        (self.transnet, self.arcface) = (None, None);
        self.scrfd.clear();
        (self.persons, self.person_reid) = (None, None);
        self.yunet.clear();
        self.sface = None;
        (self.anime_heads, self.anime_persons, self.anime_ccip) = (None, None, None);
        self.anime_berkeley = None;
    }

    /// Drop the sessions of the other family (TransNet — the shot
    /// detector — is neither's and stays).
    fn keep(&mut self, family: Family) {
        match family {
            Family::Live => {
                (self.anime_heads, self.anime_persons, self.anime_ccip) = (None, None, None);
                self.anime_berkeley = None;
            }
            Family::Anime => {
                self.scrfd.clear();
                (self.arcface, self.persons, self.person_reid) = (None, None, None);
                self.yunet.clear();
                self.sface = None;
            }
        }
    }

    /// The runtime model `id` loads from: with NVIDIA's engine where the
    /// model may use it ([`may_use_accel`]), else without.
    fn runtime_for<'a>(&'a self, rt: &'a Runtime, id: &str) -> &'a Runtime {
        if may_use_accel(id, LIVE_ON_ACCEL && !self.live_accel_off) { rt } else { &self.plain }
    }

    /// The devices of the live-action models loaded now.
    fn live_devices(&self) -> Vec<Device> {
        let detectors = self.scrfd.iter().map(Scrfd::device);
        let embedders = self.arcface.as_ref().map(ArcFace::device);
        let people = self
            .persons
            .as_ref()
            .map(PpDet::device)
            .into_iter()
            .chain(self.person_reid.as_ref().map(PersonReid::device));
        detectors.chain(embedders).chain(people).collect()
    }

    /// After the live-action models loaded: when they landed on NVIDIA's
    /// engine and elsewhere, drop them and keep them off it from now on.
    /// Returns whether they must be loaded again.
    fn unsplit_live(&mut self) -> bool {
        let devices = self.live_devices();
        if !split(&devices) {
            return false;
        }
        tracing::warn!(
            ?devices,
            "the live-action models landed on NVIDIA's engine and elsewhere; loading them without it"
        );
        self.live_accel_off = true;
        self.scrfd.clear();
        (self.arcface, self.persons, self.person_reid) = (None, None, None);
        true
    }

    /// The file of model `id`, hashed the first time this engine loads it
    /// (a damaged one loses its marker: the app's next install fetches it).
    fn path(&self, id: &str) -> Result<PathBuf, AnalysisError> {
        self.store.installed(id).map_err(|e| AnalysisError::Internal(e.detail()))?.ok_or_else(|| {
            AnalysisError::Internal(format!("{id} isn't installed (or is damaged) in {}", self.store.dir().display()))
        })
    }

    /// Load the face models, a detector planned for each look's size
    /// (loaded again when a media needs other sizes; one of a size still
    /// needed stays).
    fn load_faces(&mut self, rt: &Runtime, looks: &[Look]) -> Result<(), AnalysisError> {
        self.keep(Family::Live);
        if !self.scrfd.iter().map(Scrfd::frame_size).eq(looks.iter().map(|l| l.size)) {
            let mut old = std::mem::take(&mut self.scrfd);
            old.retain(|d| looks.iter().any(|l| l.size == d.frame_size()));
            let path = self.path(ids::SCRFD)?;
            for l in looks {
                let detector = match old.iter().position(|d| d.frame_size() == l.size) {
                    Some(i) => old.swap_remove(i),
                    None => Scrfd::load(self.runtime_for(rt, ids::SCRFD), &path, l.size, DETECT_BATCH)?,
                };
                self.scrfd.push(detector);
            }
        }
        if self.arcface.is_none() {
            let path = self.path(ids::ARCFACE)?;
            self.arcface = Some(ArcFace::load(self.runtime_for(rt, ids::ARCFACE), &path, EMBED_BATCH)?);
        }
        Ok(())
    }

    /// Load Fast Scan's models, a detector planned for each of `sizes`: the
    /// sizes a frame is looked at, in their order (loaded again when a media
    /// needs other sizes; one of a size still needed stays).
    fn load_fast(&mut self, rt: &Runtime, sizes: &[(usize, usize)]) -> Result<(), AnalysisError> {
        self.keep(Family::Live);
        if !self.yunet.iter().map(YuNet::frame_size).eq(sizes.iter().copied()) {
            let mut old = std::mem::take(&mut self.yunet);
            old.retain(|d| sizes.contains(&d.frame_size()));
            let path = self.path(ids::YUNET)?;
            for &size in sizes {
                let detector = match old.iter().position(|d| d.frame_size() == size) {
                    Some(i) => old.swap_remove(i),
                    None => YuNet::load(self.runtime_for(rt, ids::YUNET), &path, size, FAST_DETECT_BATCH)?,
                };
                self.yunet.push(detector);
            }
        }
        if self.sface.is_none() {
            let path = self.path(ids::SFACE)?;
            self.sface = Some(SFace::load(self.runtime_for(rt, ids::SFACE), &path, FAST_EMBED_BATCH)?);
        }
        Ok(())
    }

    /// Load the people models, the detector planned for `size` frames.
    fn load_people(&mut self, rt: &Runtime, size: (usize, usize)) -> Result<(), AnalysisError> {
        self.keep(Family::Live);
        if self.persons.as_ref().is_none_or(|d| d.frame_size() != size) {
            self.persons = None;
            let path = self.path(ids::PERSONS)?;
            let rt = self.runtime_for(rt, ids::PERSONS);
            self.persons = Some(PpDet::load(rt, &path, size, PERSON_INPUT, Fit::Pad, Norm::IMAGENET)?);
        }
        if self.person_reid.is_none() {
            let path = self.path(ids::PERSON_REID)?;
            self.person_reid =
                Some(PersonReid::load(self.runtime_for(rt, ids::PERSON_REID), &path, REID_BATCH, Norm::IMAGENET)?);
        }
        Ok(())
    }

    /// The anime character embedder (CCIP) alone — `Embed` jobs on stills.
    fn load_ccip(&mut self, rt: &Runtime) -> Result<(), AnalysisError> {
        self.keep(Family::Anime);
        if self.anime_ccip.is_none() {
            let path = self.path(ids::ANIME_CCIP)?;
            self.anime_ccip = Some(Ccip::load(self.runtime_for(rt, ids::ANIME_CCIP), &path, CCIP_BATCH)?);
        }
        Ok(())
    }

    /// Load the anime models, the detectors planned for `size` frames,
    /// `batch` at a time (Fast Scan looks at one frame at a time).
    /// The embedder loads first: its golden check against the CPU decides
    /// where everything else lands (a CCIP that fell back means the
    /// detectors might as well run beside it).
    fn load_anime(&mut self, rt: &Runtime, size: (usize, usize), batch: usize) -> Result<(), AnalysisError> {
        self.load_ccip(rt)?;
        let stale = |d: &Yolo| d.frame_size() != size || d.batch() != batch;
        if self.anime_heads.as_ref().is_none_or(stale) {
            self.anime_heads = None;
            let path = self.path(ids::ANIME_HEAD)?;
            self.anime_heads =
                Some(Yolo::load(self.runtime_for(rt, ids::ANIME_HEAD), &path, size, ANIME_HEAD_INPUT, batch)?);
        }
        if self.anime_persons.as_ref().is_none_or(stale) {
            self.anime_persons = None;
            let path = self.path(ids::ANIME_PERSON)?;
            self.anime_persons =
                Some(Yolo::load(self.runtime_for(rt, ids::ANIME_PERSON), &path, size, ANIME_PERSON_INPUT, batch)?);
        }
        Ok(())
    }

    /// The Best anime scan's character embedder (Berkeley), loaded with the
    /// anime models and dropped with them.
    fn load_berkeley(&mut self, rt: &Runtime) -> Result<(), AnalysisError> {
        self.keep(Family::Anime);
        if self.anime_berkeley.is_none() {
            let path = self.path(ids::ANIME_BERKELEY)?;
            self.anime_berkeley =
                Some(Berkeley::load(self.runtime_for(rt, ids::ANIME_BERKELEY), &path, BERKELEY_BATCH)?);
        }
        Ok(())
    }

    fn transnet(&mut self, rt: &Runtime) -> Result<&mut TransNet, AnalysisError> {
        if self.transnet.is_none() {
            let path = self.path(ids::TRANSNET)?;
            self.transnet = Some(TransNet::load(self.runtime_for(rt, ids::TRANSNET), &path)?);
        }
        Ok(self.transnet.as_mut().expect("just loaded"))
    }
}

fn run_shots(
    tools: &Tools,
    rt: &Runtime,
    models: &mut Models,
    job: ShotsJob,
    cancel: &CancelToken,
    out: &Out,
) -> Result<(), AnalysisError> {
    let net = models.transnet(rt)?;
    let index = FrameIndex::from_bytes(&job.index.0).map_err(|e| AnalysisError::Internal(e.to_string()))?;
    let started = Instant::now();
    let (id, file) = (job.id, job.path.file_name().unwrap_or_default());
    send_devices(out, id, rt, &[(as_models::ids::TRANSNET, net.device())]);
    tracing::info!(id, ?file, resume = job.resume, device = ?net.device(), "finding shots");
    shot_signals(
        tools,
        Source { path: Path::new(&job.path), info: &job.info, index: &index },
        net,
        job.resume,
        job.chunk,
        cancel,
        |fraction| out.send(&Reply::Progress { id, fraction }),
        |start, s| {
            out.send(&Reply::ShotChunk { id, start, len: s.len() as u32, signals: Blob(signals::encode(s)) });
            Ok(())
        },
    )?;
    let frames = index.len().saturating_sub(job.resume);
    let secs = started.elapsed().as_secs_f64();
    tracing::info!(id, frames, secs, fps = frames as f64 / secs.max(1e-3), "shots measured");
    Ok(())
}

fn run_faces(
    tools: &Tools,
    rt: &Runtime,
    models: &mut Models,
    job: FacesJob,
    cancel: &CancelToken,
    out: &Out,
) -> Result<(), AnalysisError> {
    if job.anime {
        return run_anime_faces(tools, rt, models, job, cancel, out);
    }
    if job.fast {
        return run_fast_faces(tools, rt, models, job, cancel, out);
    }
    let params = match (job.animated, rt.preferred_device()) {
        (false, _) => FaceParams::default(),
        // Searching more sizes takes four times the detector's time: on the
        // CPU, 3D animation is searched at the detection size alone.
        (true, Device::Cpu) => FaceParams { search: Search::Once, ..FaceParams::animated() },
        (true, _) => FaceParams::animated(),
    };
    let (size, _) = frame_sizes(&job.info, &params);
    let known = match &job.known {
        Some(b) => Some(
            tracks::decode_known(&b.0)
                .ok_or_else(|| AnalysisError::Internal("unreadable faces found before".into()))?,
        ),
        None => None,
    };
    let looks = detect_looks(&job.info, &params);
    if known.is_none() {
        models.load_faces(rt, &looks)?;
    }
    if job.bodies {
        models.load_people(rt, size)?;
    }
    if LIVE_ON_ACCEL && models.unsplit_live() {
        if known.is_none() {
            models.load_faces(rt, &looks)?;
        }
        if job.bodies {
            models.load_people(rt, size)?;
        }
    }
    let Models { scrfd, arcface, persons, person_reid, .. } = models;
    let device = match known {
        None => scrfd.first().map(Scrfd::device),
        Some(_) => persons.as_ref().map(PpDet::device),
    };
    if known.is_none() && scrfd.iter().any(|d| Some(d.device()) != device) {
        let devices: Vec<_> = scrfd.iter().map(|d| (d.frame_size(), d.device())).collect();
        tracing::warn!(?devices, "face detectors run on different devices");
    }
    let mut used: Vec<(&str, Device)> = Vec::new();
    if known.is_none() {
        used.extend(scrfd.iter().map(|d| (as_models::ids::SCRFD, d.device())));
        used.extend(arcface.as_ref().map(|e| (as_models::ids::ARCFACE, e.device())));
    }
    if job.bodies {
        used.extend(persons.as_ref().map(|d| (as_models::ids::PERSONS, d.device())));
        used.extend(person_reid.as_ref().map(|r| (as_models::ids::PERSON_REID, r.device())));
    }
    send_devices(out, job.id, rt, &used);
    let faces = match &known {
        Some(k) => Faces::Known(k),
        None => Faces::Find { detectors: scrfd, embedder: arcface.as_mut().expect("loaded") },
    };
    let bodies = job.bodies.then(|| BodyModels {
        detector: persons.as_mut().expect("loaded"),
        reid: person_reid.as_mut().expect("loaded"),
        params: PeopleParams::default(),
    });
    let index = FrameIndex::from_bytes(&job.index.0).map_err(|e| AnalysisError::Internal(e.to_string()))?;
    let shots: Vec<FrameRange> = job.shots.iter().map(|&(s, e)| FrameRange::new(s, e)).collect();
    let started = Instant::now();
    let id = job.id;
    let what = if job.known.is_some() { "people" } else { "faces" };
    tracing::info!(
        id,
        file = ?job.path.file_name().unwrap_or_default(),
        resume = job.resume,
        people = job.bodies,
        animated = job.animated,
        ?device,
        "finding {what}"
    );
    let (mut found, mut people) = (0usize, 0usize);
    face_tracks(
        tools,
        Source { path: Path::new(&job.path), info: &job.info, index: &index },
        faces,
        bodies,
        &shots,
        job.resume,
        job.chunk,
        &params,
        cancel,
        |fraction| out.send(&Reply::Progress { id, fraction }),
        |c| {
            found += c.tracks.len();
            people += c.bodies.len();
            let sprites = c
                .sprites
                .iter()
                .map(|s| Sprite { shot: s.shot, end: s.end, cells: s.cells, jpeg: Blob(s.jpeg.clone()) })
                .collect();
            // Nothing when no one was looked for; a list (maybe empty) when they were.
            let found_people = if job.bodies { bodies::encode(c.bodies) } else { Vec::new() };
            out.send(&Reply::FaceChunk {
                id,
                start: c.start,
                end: c.end,
                tracks: Blob(tracks::encode(c.tracks)),
                sprites,
                bodies: Blob(found_people),
            });
            Ok(())
        },
    )?;
    let frames = index.len().saturating_sub(job.resume);
    let secs = started.elapsed().as_secs_f64();
    tracing::info!(id, frames, tracks = found, people, secs, fps = frames as f64 / secs.max(1e-3), "{what} found");
    Ok(())
}

/// Read faces again at the source's own resolution, with the models that
/// found them. What the engine finds goes back as it is found.
fn run_rescan(
    tools: &Tools,
    rt: &Runtime,
    models: &mut Models,
    job: RescanJob,
    cancel: &CancelToken,
    out: &Out,
) -> Result<(), AnalysisError> {
    let index = FrameIndex::from_bytes(&job.index.0).map_err(|e| AnalysisError::Internal(e.to_string()))?;
    let id = job.id;
    let faces: Vec<RescanFace> = job
        .faces
        .iter()
        .map(|f| {
            Ok(RescanFace {
                id: f.id,
                shot: FrameRange::new(f.shot.0, f.shot.1),
                quality: f.quality,
                dets: tracks::dets_from_bytes(&f.dets.0)
                    .ok_or_else(|| AnalysisError::Internal("unreadable detections".into()))?,
            })
        })
        .collect::<Result<_, AnalysisError>>()?;
    let src = Source { path: Path::new(&job.path), info: &job.info, index: &index };
    let progress = |fraction| out.send(&Reply::Progress { id, fraction });
    let read = |r: FaceRead| {
        let embedding = r.embedding.as_deref().map(tracks::embedding_to_bytes).unwrap_or_default();
        out.send(&Reply::FaceRead {
            id,
            track: r.id,
            embedding: Blob(embedding),
            quality: r.quality,
            tried: r.tried,
            found: r.found,
        });
        Ok(())
    };
    let looks = [Look { size: (rescan::SIDE, rescan::SIDE), shrink: 1, fallback: false }];
    if job.anime {
        models.load_ccip(rt)?;
        let embedder = models.anime_ccip.as_mut().expect("loaded");
        send_devices(out, id, rt, &[(as_models::ids::ANIME_CCIP, embedder.device())]);
        let params = anime::Params::anime();
        let p = RescanParams::anime(&params);
        rescan::read_faces(tools, src, &faces, &mut HeadFinder { embedder, params }, &p, cancel, progress, read)
    } else if job.fast {
        // A crop is searched once, as it is.
        let frame = (rescan::SIDE, rescan::SIDE);
        models.load_fast(rt, &[frame])?;
        if job.second && models.arcface.is_none() {
            let path = models.path(ids::ARCFACE)?;
            models.arcface = Some(ArcFace::load(models.runtime_for(rt, ids::ARCFACE), &path, EMBED_BATCH)?);
        }
        let Models { yunet, sface, arcface, .. } = models;
        let embedder = sface.as_mut().expect("loaded");
        send_devices(
            out,
            id,
            rt,
            &[(as_models::ids::YUNET, yunet[0].device()), (as_models::ids::SFACE, embedder.device())],
        );
        let p = RescanParams { batch: FAST_EMBED_BATCH, ..RescanParams::default() };
        let finder = FastModels { detectors: yunet, embedder, frame };
        if job.second {
            let other = arcface.as_mut().expect("loaded");
            let embed = |c: &[&[u8]]| Ok(other.embed(c)?.into_iter().map(|e| (e.v, e.norm)).collect());
            let mut finder = SecondOpinion { finder, embed };
            return rescan::read_faces(tools, src, &faces, &mut finder, &p, cancel, progress, read);
        }
        let mut finder = finder;
        rescan::read_faces(tools, src, &faces, &mut finder, &p, cancel, progress, read)
    } else {
        models.load_faces(rt, &looks)?;
        if LIVE_ON_ACCEL && models.unsplit_live() {
            models.load_faces(rt, &looks)?;
        }
        if job.second && models.sface.is_none() {
            let path = models.path(ids::SFACE)?;
            models.sface = Some(SFace::load(models.runtime_for(rt, ids::SFACE), &path, FAST_EMBED_BATCH)?);
        }
        let Models { scrfd, arcface, sface, .. } = models;
        let (detector, embedder) = (&mut scrfd[0], arcface.as_mut().expect("loaded"));
        send_devices(
            out,
            id,
            rt,
            &[(as_models::ids::SCRFD, detector.device()), (as_models::ids::ARCFACE, embedder.device())],
        );
        let finder = LiveFinder { detector, embedder, params: FaceParams::default() };
        let p = RescanParams::default();
        if job.second {
            let other = sface.as_mut().expect("loaded");
            let embed = |c: &[&[u8]]| Ok(other.embed(c)?.into_iter().map(|e| (e.v, e.norm)).collect());
            let mut finder = SecondOpinion { finder, embed };
            return rescan::read_faces(tools, src, &faces, &mut finder, &p, cancel, progress, read);
        }
        let mut finder = finder;
        rescan::read_faces(tools, src, &faces, &mut finder, &p, cancel, progress, read)
    }
}

/// Where Fast Scan decodes: where the analysis runs. NVDEC on the NVIDIA card
/// in use, and in software when the person chose the processor or the
/// analysis runs on another maker's graphics: a decoder on any other card
/// would wake it, and a laptop's separate card is what Processor only is
/// chosen to spare. With several NVIDIA cards `cuda_device` names the one in
/// use, and when it can't, software is what keeps the decode off the others.
/// Only a runtime without cards to choose from (Linux) has nothing to stay on.
fn fast_decoder(choice: &Choice, hw: &Hardware, cuda_device: impl FnOnce(u64) -> Option<u32>) -> Decoder {
    if *choice == Choice::Processor {
        return Decoder::Cpu;
    }
    if hw.using == Using::Processor && hw.gpus.is_empty() {
        return Decoder::Nvdec;
    }
    let Some(card) = hw.gpu().filter(|g| g.vendor == NVIDIA) else {
        return Decoder::Cpu;
    };
    if hw.gpus.iter().filter(|g| g.vendor == NVIDIA).count() == 1 {
        return Decoder::Nvdec;
    }
    match card.luid.and_then(cuda_device) {
        Some(device) => Decoder::NvdecOn(device),
        None => {
            tracing::warn!(card = %card.label, "CUDA doesn't name this card; decoding in software");
            Decoder::Cpu
        }
    }
}

/// A faces job with Fast Scan: faces only, found as `as_analysis::fast` does.
fn run_fast_faces(
    tools: &Tools,
    rt: &Runtime,
    models: &mut Models,
    job: FacesJob,
    cancel: &CancelToken,
    out: &Out,
) -> Result<(), AnalysisError> {
    if job.known.is_some() || job.bodies {
        return Err(AnalysisError::Internal("Fast Scan looks for faces alone".into()));
    }
    let decoder = frames::decoder_for(tools, &job.info, models.decoder(rt));
    let base = if job.animated { FastParams::animated() } else { FastParams::default() };
    let params = FastParams { decoder, ..base };
    out.send(&Reply::Decoding { id: job.id, decoder });
    let frame = fast::frame_size(&job.info, &params);
    let looks = fast::looks(frame, &params);
    models.load_fast(rt, &looks)?;
    let Models { yunet, sface, .. } = models;
    let embedder = sface.as_mut().expect("loaded");
    let device = yunet[0].device();
    if embedder.device() != device || yunet.iter().any(|d| d.device() != device) {
        let detectors: Vec<_> = yunet.iter().map(|d| (d.frame_size(), d.device())).collect();
        tracing::warn!(?detectors, embedder = ?embedder.device(), "Fast Scan's models run on different devices");
    }
    send_devices(out, job.id, rt, &[(as_models::ids::YUNET, device), (as_models::ids::SFACE, embedder.device())]);
    let index = FrameIndex::from_bytes(&job.index.0).map_err(|e| AnalysisError::Internal(e.to_string()))?;
    let shots: Vec<FrameRange> = job.shots.iter().map(|&(s, e)| FrameRange::new(s, e)).collect();
    let started = Instant::now();
    let (id, file) = (job.id, job.path.file_name().unwrap_or_default());
    let decoder = params.decoder;
    tracing::info!(id, ?file, resume = job.resume, ?device, ?decoder, ?looks, "finding faces (Fast Scan)");
    let mut found = 0usize;
    fast_face_tracks(
        tools,
        Source { path: Path::new(&job.path), info: &job.info, index: &index },
        FastModels { detectors: yunet, embedder, frame },
        &shots,
        job.resume,
        job.chunk,
        &params,
        cancel,
        |fraction| out.send(&Reply::Progress { id, fraction }),
        |c| {
            found += c.tracks.len();
            let sprites = c
                .sprites
                .iter()
                .map(|s| Sprite { shot: s.shot, end: s.end, cells: s.cells, jpeg: Blob(s.jpeg.clone()) })
                .collect();
            out.send(&Reply::FaceChunk {
                id,
                start: c.start,
                end: c.end,
                tracks: Blob(tracks::encode(c.tracks)),
                sprites,
                bodies: Blob(Vec::new()),
            });
            Ok(())
        },
    )?;
    let frames = index.len().saturating_sub(job.resume);
    let secs = started.elapsed().as_secs_f64();
    tracing::info!(id, frames, tracks = found, secs, fps = frames as f64 / secs.max(1e-3), "faces found (Fast Scan)");
    Ok(())
}

/// A faces job on anime footage: characters only, found as
/// `as_analysis::anime` does (heads, implied heads of person detections,
/// CCIP) — with `fast`, in the frames Fast Scan picks. Nobody is looked for
/// by their body and no faces found before are carried in — the pass
/// stands alone like Fast Scan.
fn run_anime_faces(
    tools: &Tools,
    rt: &Runtime,
    models: &mut Models,
    job: FacesJob,
    cancel: &CancelToken,
    out: &Out,
) -> Result<(), AnalysisError> {
    if job.known.is_some() || job.bodies || job.animated {
        return Err(AnalysisError::Internal("the anime pass looks for characters alone".into()));
    }
    let params = anime::Params::anime();
    let fast_params = job.fast.then(|| {
        let decoder = frames::decoder_for(tools, &job.info, models.decoder(rt));
        FastParams { decoder, ..FastParams::anime() }
    });
    if let Some(p) = &fast_params {
        out.send(&Reply::Decoding { id: job.id, decoder: p.decoder });
    }
    let (size, batch) = match &fast_params {
        Some(p) => (fast::frame_size(&job.info, p), FAST_DETECT_BATCH),
        None => (anime::frame_size(&job.info, &params), DETECT_BATCH),
    };
    models.load_anime(rt, size, batch)?;
    let best = job.best && !job.fast;
    if best {
        models.load_berkeley(rt)?;
    }
    let (heads, persons, ccip) = (
        models.anime_heads.as_mut().expect("loaded"),
        models.anime_persons.as_mut().expect("loaded"),
        models.anime_ccip.as_mut().expect("loaded"),
    );
    let berkeley = if best { models.anime_berkeley.as_mut() } else { None };
    let device = heads.device();
    if persons.device() != device || ccip.device() != device {
        tracing::warn!(
            heads = ?device,
            persons = ?persons.device(),
            ccip = ?ccip.device(),
            "the anime models run on different devices"
        );
    }
    send_devices(
        out,
        job.id,
        rt,
        &[
            (as_models::ids::ANIME_HEAD, device),
            (as_models::ids::ANIME_PERSON, persons.device()),
            (as_models::ids::ANIME_CCIP, ccip.device()),
        ]
        .into_iter()
        .chain(berkeley.as_ref().map(|b| (as_models::ids::ANIME_BERKELEY, b.device())))
        .collect::<Vec<_>>(),
    );
    let index = FrameIndex::from_bytes(&job.index.0).map_err(|e| AnalysisError::Internal(e.to_string()))?;
    let shots: Vec<FrameRange> = job.shots.iter().map(|&(s, e)| FrameRange::new(s, e)).collect();
    let started = Instant::now();
    let (id, file) = (job.id, job.path.file_name().unwrap_or_default());
    let fast = job.fast;
    let decoder = fast_params.as_ref().map(|p| p.decoder);
    tracing::info!(id, ?file, resume = job.resume, ?device, fast, ?decoder, "finding characters (anime)");
    let mut found = 0usize;
    let src = Source { path: Path::new(&job.path), info: &job.info, index: &index };
    let anime = anime::Anime { heads, persons: Some(persons), ccip, berkeley };
    let progress = |fraction| out.send(&Reply::Progress { id, fraction });
    let on_chunk = |c: FaceChunk| {
        found += c.tracks.len();
        let sprites = c
            .sprites
            .iter()
            .map(|s| Sprite { shot: s.shot, end: s.end, cells: s.cells, jpeg: Blob(s.jpeg.clone()) })
            .collect();
        out.send(&Reply::FaceChunk {
            id,
            start: c.start,
            end: c.end,
            tracks: Blob(tracks::encode(c.tracks)),
            sprites,
            bodies: Blob(Vec::new()),
        });
        Ok(())
    };
    match &fast_params {
        Some(p) => fast_face_tracks(
            tools,
            src,
            anime::FastAnime { anime, params: &params },
            &shots,
            job.resume,
            job.chunk,
            p,
            cancel,
            progress,
            on_chunk,
        )?,
        None => {
            anime::face_tracks(tools, src, anime, &shots, job.resume, job.chunk, &params, cancel, progress, on_chunk)?
        }
    }
    let frames = index.len().saturating_sub(job.resume);
    let secs = started.elapsed().as_secs_f64();
    tracing::info!(
        id,
        frames,
        tracks = found,
        secs,
        fps = frames as f64 / secs.max(1e-3),
        fast,
        "characters found (anime)"
    );
    Ok(())
}

/// Embed a still image with the anime embedder: no detector runs on a
/// portrait — the whole picture, centred in a square of its longer side
/// (`ccip::crop` pads the rest with black), is embedded like a head's
/// context crop.
fn run_embed(
    tools: &Tools,
    rt: &Runtime,
    models: &mut Models,
    job: EmbedJob,
    cancel: &CancelToken,
    out: &Out,
) -> Result<(), AnalysisError> {
    models.load_ccip(rt)?;
    let ccip = models.anime_ccip.as_mut().expect("loaded");
    send_devices(out, job.id, rt, &[(as_models::ids::ANIME_CCIP, ccip.device())]);
    let (w, h, rgb) = frames::decode_still(tools, &job.path, cancel)?;
    let (w, h) = (w as usize, h as usize);
    let side = w.max(h) as f32;
    let square = as_vision::ccip::crop(&rgb, w, h, (w as f32 - side) / 2.0, (h as f32 - side) / 2.0, side);
    let e = ccip
        .embed(&[square.as_slice()])?
        .into_iter()
        .next()
        .ok_or_else(|| AnalysisError::Internal("the embedder gave no vector".into()))?;
    out.send(&Reply::Embedding { id: job.id, embedding: e.v });
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use as_infer::hardware::{Kind, RawDevice};

    use super::*;

    #[test]
    fn device_chain_puts_nvidia_first_only_where_allowed() {
        // The anime models, wherever the engine is registered.
        let anime = [ids::ANIME_HEAD, ids::ANIME_PERSON, ids::ANIME_CCIP];
        for id in anime {
            assert!(may_use_accel(id, false) && may_use_accel(id, true), "{id}");
        }
        // The live-action models only once they are let on it.
        let live = [ids::SCRFD, ids::ARCFACE, ids::PERSONS, ids::PERSON_REID];
        for id in live {
            assert!(!may_use_accel(id, false) && may_use_accel(id, true), "{id}");
        }
        // Never: a node it can't take, Fast Scan's models, and the Best anime
        // model (not yet checked on NVIDIA's engine).
        let never = [ids::TRANSNET, ids::YUNET, ids::SFACE, ids::ANIME_BERKELEY];
        for id in never {
            assert!(!may_use_accel(id, false) && !may_use_accel(id, true), "{id}");
        }
        // A model the store gains is placed on purpose: it has to be one of
        // these (and pass a parity check on the engine before it is let on).
        for m in as_models::manifest() {
            assert!(anime.iter().chain(&live).chain(&never).any(|id| *id == m.id), "where does {} run?", m.id);
        }
    }

    #[test]
    fn only_jobs_with_models_allowed_on_nvidias_engine_may_use_it() {
        let faces = |anime, fast| {
            Request::Faces(Box::new(FacesJob {
                id: 1,
                path: "a.mp4".into(),
                info: crate::protocol::tests::info(),
                index: Blob(Vec::new()),
                shots: Vec::new(),
                resume: 0,
                chunk: 100,
                bodies: false,
                known: None,
                animated: false,
                fast,
                anime,
                best: false,
            }))
        };
        // Anime, also with Fast Scan, and the anime embedder for AniList art.
        assert!(job_may_use_accel(&faces(true, false)));
        assert!(job_may_use_accel(&faces(true, true)));
        assert!(job_may_use_accel(&Request::Embed(Box::new(EmbedJob { id: 1, path: "a.png".into() }))));
        // Live-action faces only once they are let on it; Fast Scan's never.
        assert_eq!(job_may_use_accel(&faces(false, false)), LIVE_ON_ACCEL);
        assert!(!job_may_use_accel(&faces(false, true)));
        // Shots never: TransNetV2 can't run there.
        let shots = ShotsJob {
            id: 1,
            path: "a.mp4".into(),
            info: crate::protocol::tests::info(),
            index: Blob(Vec::new()),
            resume: 0,
            chunk: 100,
        };
        assert!(!job_may_use_accel(&Request::Shots(Box::new(shots))));
    }

    #[test]
    fn a_split_live_pass_reloads_on_directml() {
        use Device::{Cpu, DirectMl, NvRtx};
        // All on one engine, or none on NVIDIA's: kept.
        assert!(!split(&[NvRtx, NvRtx, NvRtx]));
        assert!(!split(&[DirectMl, DirectMl, Cpu]));
        assert!(!split(&[]));
        // Its detector on NVIDIA's engine and its embedder elsewhere (the
        // embedder missed its goldens there, say): loaded again without it.
        assert!(split(&[NvRtx, NvRtx, DirectMl]));
        assert!(split(&[NvRtx, Cpu]));
    }

    #[test]
    fn a_job_that_panics_fails_with_its_message() {
        assert_eq!(guarded(|| panic!("index out of bounds")).unwrap_err(), "index out of bounds");
        let n = 3;
        assert_eq!(guarded(|| panic!("frame {n} is missing")).unwrap_err(), "frame 3 is missing");
        assert!(guarded(|| Ok(())).unwrap().is_ok());
        assert!(guarded(|| Err(AnalysisError::Internal("no".into()))).unwrap().is_err());
    }

    const INTEL: u32 = 0x8086;
    const AMD: u32 = 0x1002;

    fn card(vendor: u32, device: u32, dxgi: u32, luid: u64) -> RawDevice {
        let metadata = [("DxgiAdapterNumber", dxgi.to_string()), ("LUID", luid.to_string())];
        let metadata = metadata.map(|(k, v)| (k.to_string(), v)).into();
        RawDevice { kind: Kind::Gpu, vendor, device, metadata }
    }

    /// These cards on Windows, the first the one DirectML's default opens.
    fn windows(cards: &[RawDevice]) -> Hardware {
        let order: Vec<u64> = cards.iter().map(|c| c.metadata["LUID"].parse().unwrap()).collect();
        Hardware::windows(cards, Some(&order))
    }

    /// [`fast_decoder`] for `choice`, where the driver gives every card it
    /// is asked about CUDA device 1 and says which one that was.
    fn decoder(hw: &Hardware, choice: Choice) -> (Decoder, Option<u64>) {
        let mut asked = None;
        let decoder = fast_decoder(&choice, &hw.clone().chosen(&choice), |luid| {
            asked = Some(luid);
            Some(1)
        });
        (decoder, asked)
    }

    #[test]
    fn fast_scan_decodes_on_the_nvidia_card_in_use_and_on_no_other() {
        let (intel, nvidia) = (card(INTEL, 0x46A6, 0, 10), card(NVIDIA, 0x24DD, 1, 20));
        let (on_intel, on_nvidia) = (windows(&[intel.clone(), nvidia.clone()]), windows(&[nvidia, intel]));
        let chosen = |vendor, device| Choice::Card { vendor, device, nth: 0 };

        // Where the analysis runs on the NVIDIA card; one needs no number.
        assert_eq!(decoder(&on_nvidia, Choice::Auto), (Decoder::Nvdec, None));
        assert_eq!(decoder(&on_intel, chosen(NVIDIA, 0x24DD)), (Decoder::Nvdec, None));
        // The processor, the built-in graphics: the card stays asleep.
        assert_eq!(decoder(&on_nvidia, Choice::Processor), (Decoder::Cpu, None));
        assert_eq!(decoder(&on_nvidia, chosen(INTEL, 0x46A6)), (Decoder::Cpu, None));
        assert_eq!(decoder(&on_intel, Choice::Auto), (Decoder::Cpu, None));
        // No NVIDIA card, a card DirectML picks itself, a Mac.
        assert_eq!(decoder(&windows(&[card(AMD, 0x73BF, 0, 30)]), Choice::Auto), (Decoder::Cpu, None));
        let unnumbered = |device| RawDevice { metadata: BTreeMap::new(), ..card(NVIDIA, device, 0, 0) };
        let preferred = Hardware::windows(&[unnumbered(0x2204), unnumbered(0x2206)], None);
        assert_eq!(decoder(&preferred, Choice::Auto), (Decoder::Cpu, None));
        assert_eq!(decoder(&Hardware::mac(&[], "Apple M2"), Choice::Auto), (Decoder::Cpu, None));
        // Without cards to choose from nothing was chosen (Linux), until
        // the person says the processor.
        assert_eq!(decoder(&Hardware::processor_only(&[]), Choice::Auto), (Decoder::Nvdec, None));
        assert_eq!(decoder(&Hardware::processor_only(&[]), Choice::Processor), (Decoder::Cpu, None));
    }

    #[test]
    fn of_two_nvidia_cards_fast_scan_decodes_on_the_chosen_one() {
        let twins = windows(&[card(NVIDIA, 0x2204, 0, 40), card(NVIDIA, 0x2204, 1, 50)]);
        let second = Choice::Card { vendor: NVIDIA, device: 0x2204, nth: 1 };
        assert_eq!(decoder(&twins, second), (Decoder::NvdecOn(1), Some(50)));
        assert_eq!(decoder(&twins, Choice::Auto), (Decoder::NvdecOn(1), Some(40)));
        assert_eq!(decoder(&twins, Choice::Processor), (Decoder::Cpu, None));
        // Where the driver can't say which is which, none of them.
        assert_eq!(fast_decoder(&second, &twins.chosen(&second), |_| None), Decoder::Cpu);
    }

    #[test]
    fn a_cpu_engine_never_builds_the_cuda_input_args() {
        let cfg = crate::config(["--ort", "ort", "--models", "models", "--cpu"].map(Into::into)).unwrap();
        let hw = windows(&[card(NVIDIA, 0x24DD, 0, 20)]).chosen(&cfg.placement.gpu);
        let decoder = fast_decoder(&cfg.placement.gpu, &hw, |_| panic!("the driver isn't asked"));
        assert!(frames::input_args(decoder).is_empty(), "{decoder:?}");
        // Nor does a caller that names no decoder.
        assert_eq!(FastParams::default().decoder, Decoder::Cpu);
        assert_eq!(FastParams::anime().decoder, Decoder::Cpu);
    }
}

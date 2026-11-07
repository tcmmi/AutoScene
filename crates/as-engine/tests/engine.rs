//! The engine process end to end, including a hard kill in the middle of a
//! job. Needs `cargo xtask fixtures`, `fetch-sidecars` and `fetch-models`;
//! skips with a note when they're missing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use as_core::CancelToken;
use as_engine::client::pack_failures;
use as_engine::{Config, Engine, EngineError, FacesEvent, FacesInput, Launch, ShotsEvent};
use as_infer::Placement;
use as_infer::accel::{self, AccelState};
use as_infer::computer::{self, Choice, Machine, NVIDIA};
use as_infer::hardware::{Gpu, Using};
use as_media::frames::Decoder;
use as_media::{MediaInfo, Tools, build_frame_index, probe};
use as_testkit::{fixtures_dir, load_truth, models_dir, models_required, ort_dir};

struct Media {
    path: PathBuf,
    info: MediaInfo,
    index: Vec<u8>,
    frames: u32,
}

fn setup(idle_exit: Option<Duration>) -> Option<(Engine, Media)> {
    setup_with(move |ort| launch(ort, Choice::Auto, idle_exit))
}

/// [`setup`] for a test that kills the engine at a job's first chunk: see
/// [`killable`].
fn setup_killable() -> Option<(Engine, Media)> {
    setup_with(killable)
}

/// An engine that waits at each job's first chunk (a debug build's
/// `AUTOSCENE_TEST_PAUSE_AT_FIRST_CHUNK`) until the test's kill lands,
/// however fast the GPU and however slow the kill: a job it isn't killed
/// in waits the full 30 s there.
fn killable(ort: PathBuf) -> Launch {
    let mut launch = launch(ort, Choice::Auto, None);
    launch.env.push(("AUTOSCENE_TEST_PAUSE_AT_FIRST_CHUNK".into(), "30000".into()));
    launch
}

fn setup_with(launch: impl FnOnce(PathBuf) -> Launch) -> Option<(Engine, Media)> {
    let (ort, media) = media()?;
    let store = as_models::ModelStore::new(models_dir());
    if store.installed(as_models::ids::TRANSNET).unwrap().is_none() {
        assert!(!models_required(), "TransNetV2 missing (run `cargo xtask fetch-models`)");
        eprintln!("TransNetV2 missing; run `cargo xtask fetch-models` (skipping)");
        return None;
    }
    Some((Engine::new(launch(ort)), media))
}

/// The runtime and the fixture a job needs, for a test that runs without models.
fn media() -> Option<(PathBuf, Media)> {
    load_truth()?;
    let ort = ort_dir()?;
    let tools = Tools::locate(None).expect("ffmpeg");
    let path = fixtures_dir().join("cfr24_h264.mp4");
    let info = probe(&tools, &path).unwrap();
    let work = tempfile::tempdir().unwrap();
    let index = build_frame_index(&tools, &path, &info, work.path(), &CancelToken::new(), |_| {}).unwrap();
    Some((ort, Media { path, info, frames: index.len(), index: index.to_bytes() }))
}

/// How to start the engine binary on the device `gpu` chooses.
fn launch(ort_dir: PathBuf, gpu: Choice, idle_exit: Option<Duration>) -> Launch {
    launch_with(ort_dir, Placement { gpu, ..Placement::default() }, idle_exit)
}

/// How to start the engine binary with this placement.
fn launch_with(ort_dir: PathBuf, placement: Placement, idle_exit: Option<Duration>) -> Launch {
    let cfg = Config { ort_dir, models_dir: models_dir(), ffmpeg_dir: None, placement, idle_exit };
    Launch::engine(PathBuf::from(env!("CARGO_BIN_EXE_autoscene-engine")), Vec::new(), &cfg)
}

/// [`launch_with`] Automatic and NVIDIA's engine in `pack`, whose engines
/// a debug build ends on purpose `crash` (`start`, `job`) when given.
fn launch_with_pack(ort_dir: PathBuf, pack: &Path, crash: Option<&str>) -> Launch {
    let placement = Placement { accel: Some(pack.to_path_buf()), ..Placement::default() };
    let mut launch = launch_with(ort_dir, placement, None);
    launch.env.extend(crash.map(|at| ("AUTOSCENE_TEST_CRASH_WITH_ACCEL".into(), at.into())));
    launch
}

/// Run a shots job and collect its chunks; `on_chunk` sees each chunk start.
fn collect(
    engine: &Engine,
    m: &Media,
    resume: u32,
    cancel: &CancelToken,
    mut on_chunk: impl FnMut(u32),
) -> (Result<(), EngineError>, BTreeMap<u32, Vec<u8>>) {
    let mut chunks = BTreeMap::new();
    let r = engine.shots(m.path.clone(), m.info.clone(), m.index.clone(), resume, 100, cancel, |e| {
        if let ShotsEvent::Chunk { start, signals } = e {
            assert!(chunks.insert(start, signals).is_none(), "chunk {start} arrived twice");
            on_chunk(start);
        }
        Ok(())
    });
    (r, chunks)
}

/// Same machine, same device: a resume on another device (the GPU gone
/// after a driver crash) differs in the last digits, which the app notes.
#[test]
fn a_killed_engine_resumes_with_identical_signals() {
    let Some((plain, m)) = setup(None) else { return };
    let (r, full) = collect(&plain, &m, 0, &CancelToken::new(), |_| {});
    r.unwrap();
    assert_eq!(full.keys().copied().collect::<Vec<_>>(), (0..m.frames).step_by(100).collect::<Vec<_>>());
    let total: usize = full.values().map(|b| as_analysis::signals::decode(b).unwrap().len()).sum();
    assert_eq!(total as u32, m.frames, "one signal per frame");

    // The same job, but the engine process is killed hard after its first
    // chunk (it waits there for the kill: see `killable`).
    let engine = Engine::new(killable(ort_dir().expect("the runtime the first run used")));
    let mut first_pid = None;
    let (r, resumed) = collect(&engine, &m, 0, &CancelToken::new(), |start| {
        if start == 0 && first_pid.is_none() {
            first_pid = engine.pid();
            kill_hard(first_pid.unwrap());
        }
    });
    r.unwrap();
    assert!(first_pid.is_some(), "killed at the first chunk");
    assert_ne!(engine.pid(), first_pid, "a fresh engine finished the job");
    assert_eq!(resumed, full, "signals after a hard kill match an uninterrupted run");
}

#[test]
fn every_engine_on_a_job_says_where_its_models_run() {
    let Some((engine, m)) = setup_killable() else { return };
    let mut seen = Vec::new();
    let mut killed = false;
    engine
        .shots(m.path.clone(), m.info.clone(), m.index.clone(), 0, 100, &CancelToken::new(), |e| {
            match e {
                ShotsEvent::Devices { devices, resume } => seen.push((devices, resume)),
                ShotsEvent::Progress(_) => assert!(!seen.is_empty(), "the devices come first"),
                ShotsEvent::Chunk { start, .. } => {
                    if start == 0 && !killed {
                        killed = true;
                        kill_hard(engine.pid().unwrap());
                    }
                }
            }
            Ok(())
        })
        .unwrap();
    assert!(killed);
    // The first engine from the start, the fresh one from the last chunk it got.
    assert_eq!(seen.len(), 2, "{seen:?}");
    assert_eq!(seen[0].1, 0);
    assert!(seen[1].1 > 0 && seen[1].1 % 100 == 0, "{seen:?}");
    for (d, _) in &seen {
        assert_eq!(d.models.iter().map(|(m, _)| m.as_str()).collect::<Vec<_>>(), [as_models::ids::TRANSNET]);
        assert!(!d.preferred.is_empty() && !d.summary().is_empty());
    }
}

#[test]
fn cancel_stops_a_job_quickly() {
    let Some((engine, m)) = setup(None) else { return };
    let cancel = CancelToken::new();
    let started = std::time::Instant::now();
    let (r, chunks) = collect(&engine, &m, 0, &cancel, |_| cancel.cancel());
    assert!(matches!(r, Err(EngineError::Cancelled)), "{r:?}");
    assert!(!chunks.is_empty());
    assert!(started.elapsed() < Duration::from_secs(30));
    // The engine stays usable.
    let (r, _) = collect(&engine, &m, 500, &CancelToken::new(), |_| {});
    r.unwrap();
}

/// The engine names the device it opened: on Automatic the first adapter of
/// DirectML's own list, which this process's probe classes for Fast Scan
/// (CI's Windows, with only the software adapter: the processor); each card
/// when it is chosen; nothing on Processor only.
#[test]
fn the_engine_names_the_device_it_uses() {
    let Some(ort) = ort_dir() else {
        eprintln!("onnxruntime missing; run `cargo xtask fetch-sidecars` (skipping)");
        return;
    };
    if Tools::locate(None).is_err() {
        eprintln!("ffmpeg missing (skipping)");
        return;
    }
    let auto = Engine::new(launch(ort.clone(), Choice::Auto, None));
    let hw = auto.probe(&CancelToken::new()).unwrap();
    assert_eq!(auto.hardware().as_ref(), Some(&hw), "kept");
    assert!(hw.runtime.starts_with("1."), "the runtime says its version: {:?}", hw.runtime);
    let expected = computer::choose(computer::machine(), &Choice::Auto);
    assert_eq!(hw.luid(), expected.luid, "{hw}; the probe expects {expected}");
    match computer::machine() {
        Machine::Windows(Some(adapters)) if adapters.is_empty() => assert_eq!(hw.using, Using::Processor, "{hw}"),
        Machine::CpuRuntime => assert_eq!((hw.using, hw.gpus.len()), (Using::Processor, 0)),
        Machine::Mac(_) => assert_eq!(hw.using, Using::Chip),
        _ => {}
    }
    for card in hw.gpus.iter().filter(|g| g.dxgi.is_some()) {
        let chosen = Engine::new(launch(ort.clone(), Choice::from_setting(&card.key), None));
        let got = chosen.probe(&CancelToken::new()).unwrap();
        assert_eq!((got.gpu().map(|g| &g.key), got.missing), (Some(&card.key), false), "{got}");
    }
    let cpu = Engine::new(launch(ort, Choice::Processor, None));
    let got = cpu.probe(&CancelToken::new()).unwrap();
    assert_eq!((got.using, got.luid(), got.gpus.len()), (Using::Processor, None, hw.gpus.len()));
    assert_eq!(cpu.device().as_deref(), Some(as_infer::Device::Cpu.label()));
}

/// An engine start deletes the compiled models (Core ML's, on a Mac) no
/// start has used for 30 days, and keeps the others.
#[test]
fn an_engine_start_deletes_compiled_models_unused_for_30_days() {
    let Some(ort) = ort_dir() else {
        eprintln!("onnxruntime missing; run `cargo xtask fetch-sidecars` (skipping)");
        return;
    };
    if Tools::locate(None).is_err() {
        eprintln!("ffmpeg missing (skipping)");
        return;
    }
    let cache = tempfile::tempdir().unwrap();
    let compiled = |key: &str, days: u64| {
        let dir = cache.path().join(as_infer::cache::COREML).join(key);
        std::fs::create_dir_all(dir.join("compiled_model.mlmodelc")).unwrap();
        let used = std::fs::File::create(dir.join(as_infer::cache::USED)).unwrap();
        used.set_modified(SystemTime::now() - Duration::from_secs(days * 24 * 60 * 60)).unwrap();
        dir
    };
    let (unused, recent) = (compiled("00000000000000aa", 31), compiled("00000000000000bb", 29));
    let placement =
        Placement { gpu: Choice::Processor, cache: Some(cache.path().to_path_buf()), ..Placement::default() };
    let engine = Engine::new(launch_with(ort, placement, None));
    engine.probe(&CancelToken::new()).unwrap();
    assert!(!unused.exists(), "unused for 31 days: deleted");
    assert!(recent.join("compiled_model.mlmodelc").is_dir(), "used 29 days ago: kept");
}

/// A choice made while a job runs applies from the next job, also one
/// queued meanwhile, and stops an idle engine at once.
#[test]
fn a_new_launch_applies_to_the_next_job() {
    let Some((engine, m)) = setup(None) else { return };
    let ort = ort_dir().unwrap();
    let on = |gpu| launch(ort.clone(), gpu, None);
    let started = engine.probe(&CancelToken::new()).unwrap();
    assert!(engine.pid().is_some());
    engine.set_launch(on(Choice::Auto));
    assert_eq!(engine.pid(), None, "an idle engine stops at once");
    assert!(!engine.pending());

    let cpu = as_infer::Device::Cpu.label();
    let (mut first, mut second) = (Vec::new(), Vec::new());
    let mut first_pid = None;
    std::thread::scope(|s| {
        let mut queued = None;
        engine
            .shots(m.path.clone(), m.info.clone(), m.index.clone(), 0, 100, &CancelToken::new(), |e| {
                match e {
                    ShotsEvent::Devices { devices, .. } => first.push(devices.preferred),
                    ShotsEvent::Chunk { start: 0, .. } => {
                        first_pid = engine.pid();
                        engine.set_launch(on(Choice::Processor));
                        assert!(engine.pending(), "applies from the next analysis");
                        assert_eq!(engine.pid(), first_pid, "the running job keeps its engine");
                        // The next job, queued while this one runs.
                        queued = Some(s.spawn(|| {
                            let mut seen = Vec::new();
                            let mut pid = None;
                            let r = engine.shots(
                                m.path.clone(),
                                m.info.clone(),
                                m.index.clone(),
                                0,
                                100,
                                &CancelToken::new(),
                                |e| {
                                    if let ShotsEvent::Devices { devices, .. } = e {
                                        seen.push(devices.preferred);
                                        pid = engine.pid();
                                    }
                                    Ok(())
                                },
                            );
                            (r, seen, pid)
                        }));
                    }
                    _ => {}
                }
                Ok(())
            })
            .unwrap();
        let (r, seen, pid) = queued.take().expect("queued").join().unwrap();
        r.unwrap();
        second = seen;
        assert_ne!(pid, first_pid, "a new engine ran the queued job");
    });
    assert_eq!(first.len(), 1);
    if started.using != Using::Processor {
        assert_ne!(first[0], cpu, "the first job ran where it started");
    }
    assert_eq!(second, [cpu], "the queued job ran on the processor, as chosen meanwhile");
    assert_eq!(engine.hardware().map(|h| h.using), Some(Using::Processor));
    assert!(!engine.pending());
}

fn kill_hard(pid: u32) {
    #[cfg(windows)]
    let st = std::process::Command::new("taskkill").args(["/F", "/PID", &pid.to_string()]).output().unwrap();
    #[cfg(not(windows))]
    let st = std::process::Command::new("kill").args(["-9", &pid.to_string()]).output().unwrap();
    assert!(st.status.success(), "kill {pid}: {}", String::from_utf8_lossy(&st.stderr));
}

#[test]
fn an_idle_engine_exits_and_the_next_job_starts_a_new_one() {
    let Some((engine, m)) = setup(Some(Duration::from_secs(1))) else { return };
    let (r, _) = collect(&engine, &m, 500, &CancelToken::new(), |_| {});
    r.unwrap();
    let first = engine.pid().expect("engine running");
    std::thread::sleep(Duration::from_secs(3));
    let (r, chunks) = collect(&engine, &m, 500, &CancelToken::new(), |_| {});
    r.unwrap();
    assert_eq!(chunks.keys().copied().collect::<Vec<_>>(), [500]);
    assert_ne!(engine.pid(), Some(first), "the idle engine had exited; a new one ran the job");
}

#[test]
fn face_chunks_tile_the_media_even_across_a_kill() {
    let Some((engine, m)) = setup_killable() else { return };
    let store = as_models::ModelStore::new(models_dir());
    if as_models::ids::FACES.iter().any(|id| store.installed(id).unwrap().is_none()) {
        assert!(!models_required(), "face models missing (run `cargo xtask fetch-models`)");
        eprintln!("face models missing; run `cargo xtask fetch-models` (skipping)");
        return;
    }
    // Shots of 50 frames; chunks close at the first shot start 100 frames on.
    let shots: Vec<(u32, u32)> = (0..m.frames).step_by(50).map(|s| (s, (s + 50).min(m.frames))).collect();
    // Live action, and 3D animation (searched at three sizes).
    for animated in [false, true] {
        face_chunks_tile(&engine, &m, FacesInput { animated, ..faces_input(&m, shots.clone(), false, None) });
    }
    // Fast Scan (YuNet, SFace, a few looks per shot).
    if as_models::ids::FAST_FACES.iter().any(|id| store.installed(id).unwrap().is_none()) {
        assert!(!models_required(), "Fast Scan models missing (run `cargo xtask fetch-models`)");
        eprintln!("Fast Scan models missing; run `cargo xtask fetch-models` (skipping)");
        return;
    }
    face_chunks_tile(&engine, &m, FacesInput { fast: true, ..faces_input(&m, shots.clone(), false, None) });
    // Anime (YOLO heads and people, CCIP).
    if as_models::ids::ANIME.iter().any(|id| store.installed(id).unwrap().is_none()) {
        assert!(!models_required(), "anime models missing (run `cargo xtask fetch-models`)");
        eprintln!("anime models missing; run `cargo xtask fetch-models` (skipping)");
        return;
    }
    face_chunks_tile(&engine, &m, FacesInput { anime: true, ..faces_input(&m, shots.clone(), false, None) });
    // Anime with Fast Scan (the same models, a few looks per shot).
    face_chunks_tile(&engine, &m, FacesInput { anime: true, fast: true, ..faces_input(&m, shots, false, None) });
}

/// A job with Fast Scan decodes where the engine's choice says, and says so
/// before it loads a model: Processor only in software, for Fast Scan and for
/// anime with Fast Scan, and a lone NVIDIA card on that card. So this holds
/// the jobs themselves to it, models or not. With the models each runs to its end.
#[test]
fn a_fast_scan_job_decodes_where_the_choice_says() {
    let Some((ort, m)) = media() else { return };
    let store = as_models::ModelStore::new(models_dir());
    let shots: Vec<(u32, u32)> = (0..m.frames).step_by(50).map(|s| (s, (s + 50).min(m.frames))).collect();
    let decoded = |engine: &Engine, job, needs: &[&str]| {
        let mut reached = 0;
        let r = engine.faces(job, &CancelToken::new(), |e| {
            if let FacesEvent::Chunk { start, end, .. } = e {
                assert_eq!(start, reached);
                reached = end;
            }
            Ok(())
        });
        if needs.iter().any(|id| store.installed(id).unwrap().is_none()) {
            assert!(!models_required(), "{needs:?} missing (run `cargo xtask fetch-models`)");
            assert!(r.is_err());
        } else {
            r.unwrap();
            assert_eq!(reached, m.frames);
        }
        engine.decoder()
    };
    let fast = || FacesInput { fast: true, ..faces_input(&m, shots.clone(), false, None) };
    let anime = || FacesInput { anime: true, ..fast() };
    // An engine each: one that said nothing must not pass on the last one's word.
    let processor = || Engine::new(launch(ort.clone(), Choice::Processor, None));
    assert_eq!(decoded(&processor(), fast(), &as_models::ids::FAST_FACES), Some(Decoder::Cpu));
    assert_eq!(decoded(&processor(), anime(), &as_models::ids::ANIME), Some(Decoder::Cpu));

    let auto = Engine::new(launch(ort.clone(), Choice::Auto, None));
    let hw = auto.probe(&CancelToken::new()).unwrap();
    let nvidia = |g: &&Gpu| g.vendor == NVIDIA;
    if hw.gpu().filter(nvidia).is_some() && hw.gpus.iter().filter(nvidia).count() == 1 {
        assert_eq!(decoded(&auto, fast(), &as_models::ids::FAST_FACES), Some(Decoder::Nvdec));
    }
    // A file NVDEC doesn't take runs in software, and the job says so, on
    // any machine and whatever the engine would choose.
    let av1 = || {
        let mut job = fast();
        job.info.video.codec = "av1".into();
        job
    };
    assert_eq!(decoded(&auto, av1(), &as_models::ids::FAST_FACES), Some(Decoder::Cpu));
    let anime_av1 = FacesInput { anime: true, ..av1() };
    assert_eq!(decoded(&auto, anime_av1, &as_models::ids::ANIME), Some(Decoder::Cpu));
}

fn face_chunks_tile(engine: &Engine, m: &Media, job: FacesInput) {
    let mut chunks: Vec<(u32, u32)> = Vec::new();
    let mut killed = false;
    let (fast, anime) = (job.fast, job.anime);
    let mut devices = Vec::new();
    let r =
        engine.faces(job, &CancelToken::new(), |e| {
            if let FacesEvent::Devices { devices: d, resume } = &e {
                assert_eq!(*resume, chunks.last().map_or(0, |c| c.1), "it resumes after the last chunk");
                devices.push(d.models.iter().map(|(m, _)| m.clone()).collect::<Vec<_>>());
            }
            if let FacesEvent::Chunk { start, end, tracks, sprites, bodies } = e {
                // The synthetic fixture shows barcodes, not people (what an
                // anime detector makes of them is the bench's, not this test's).
                let found = as_analysis::tracks::decode(&tracks).expect("readable tracks");
                assert!(anime || found.is_empty());
                assert!(bodies.is_empty(), "nobody was looked for");
                // Every shot of the chunk has its sprite.
                let starts: Vec<u32> = (start..end).step_by(50).collect();
                assert_eq!(sprites.iter().map(|s| s.shot).collect::<Vec<_>>(), starts);
                assert!(sprites.iter().all(|s| s.end == (s.shot + 50).min(m.frames)
                    && s.cells == 8
                    && s.jpeg.starts_with(&[0xFF, 0xD8])));
                chunks.push((start, end));
                if chunks.len() == 1 && !killed {
                    killed = true;
                    kill_hard(engine.pid().unwrap());
                }
            }
            Ok(())
        });
    r.unwrap();
    assert!(killed);
    // Both engines say which models they run: their detector(s) and the embedder, last.
    assert_eq!(devices.len(), 2, "{devices:?}");
    let (detector, embedder) = if anime {
        (as_models::ids::ANIME_HEAD, as_models::ids::ANIME_CCIP)
    } else if fast {
        (as_models::ids::YUNET, as_models::ids::SFACE)
    } else {
        (as_models::ids::SCRFD, as_models::ids::ARCFACE)
    };
    for models in &devices {
        assert!(models.iter().any(|m| m == detector) && models.last().is_some_and(|m| m == embedder), "{models:?}");
    }
    let mut expect = 0;
    for &(s, e) in &chunks {
        assert_eq!(s, expect, "chunks tile the media: {chunks:?}");
        assert!(e > s);
        expect = e;
    }
    assert_eq!(expect, m.frames);
}

/// ONNX Runtime and ffmpeg, or `None` (the test skips, saying why).
fn runtime_and_tools() -> Option<PathBuf> {
    let Some(ort) = ort_dir() else {
        eprintln!("onnxruntime missing; run `cargo xtask fetch-sidecars` (skipping)");
        return None;
    };
    if Tools::locate(None).is_err() {
        eprintln!("ffmpeg missing (skipping)");
        return None;
    }
    Some(ort)
}

/// An engine started with NVIDIA's engine that ends before it is ready (as
/// a native crash of the plugin would end it): the next one starts without
/// it, at once, and the failure is counted in the pack's folder.
#[test]
fn an_engine_that_dies_with_the_pack_restarts_without_it() {
    let Some(ort) = runtime_and_tools() else { return };
    let pack = tempfile::tempdir().unwrap();
    let engine = Engine::new(launch_with_pack(ort, pack.path(), Some("start")));
    let hw = engine.probe(&CancelToken::new()).expect("an engine without the pack started");
    assert!(engine.pid().is_some(), "{hw}");
    assert_eq!(engine.accel(), Some(AccelState::Off), "it runs without NVIDIA's engine");
    assert_eq!(pack_failures(pack.path()), 1);
}

/// Trying again starts engines with it once more: until then a new launch
/// doesn't bring it back, and an engine that is there isn't started again.
#[test]
fn trying_again_brings_the_pack_back() {
    let Some(ort) = runtime_and_tools() else { return };
    let pack = tempfile::tempdir().unwrap();
    let launch = launch_with_pack(ort, pack.path(), Some("start"));
    let engine = Engine::new(launch.clone());
    let cancel = CancelToken::new();
    engine.warm(&cancel).expect("an engine without the pack started");
    let pid = engine.pid().expect("it runs");
    assert_eq!(pack_failures(pack.path()), 1);
    engine.warm(&cancel).unwrap();
    assert_eq!(engine.pid(), Some(pid), "the one that is there stays");

    engine.set_launch(launch.clone());
    engine.warm(&cancel).unwrap();
    assert_eq!(pack_failures(pack.path()), 1, "not tried again by itself");

    engine.pack_try_again(pack.path());
    assert_eq!(pack_failures(pack.path()), 0);
    engine.set_launch(launch);
    engine.warm(&cancel).unwrap();
    assert_eq!(pack_failures(pack.path()), 1, "started with it, and it ended again");
}

/// A pack that failed this session is no reason to leave out the one that
/// replaces it: the failure was the old files', and the download says so.
#[test]
fn a_new_pack_starts_engines_after_the_one_before_it_failed() {
    let Some(ort) = runtime_and_tools() else { return };
    let failed = tempfile::tempdir().unwrap();
    let engine = Engine::new(launch_with_pack(ort.clone(), failed.path(), Some("start")));
    let cancel = CancelToken::new();
    engine.warm(&cancel).expect("an engine without the pack started");
    assert_eq!(pack_failures(failed.path()), 1);

    // The new pack is left out until the download says it is here: its
    // folder has no failures, but this session's still count.
    let fresh = tempfile::tempdir().unwrap();
    let launch = launch_with_pack(ort, fresh.path(), Some("start"));
    engine.set_launch(launch.clone());
    engine.warm(&cancel).unwrap();
    assert_eq!(pack_failures(fresh.path()), 0, "not started with it yet");

    engine.pack_try_again(fresh.path());
    engine.set_launch(launch);
    engine.warm(&cancel).unwrap();
    assert_eq!(pack_failures(fresh.path()), 1, "started with it, so --accel reached it");
}

/// One that ends under its first job while NVIDIA's engine is in use, and
/// the job may run a model there (anime): the job runs again without it, and
/// that costs none of its attempts: two more engines may still go away under
/// it (each is killed as soon as it says where its models run, long before
/// it could finish).
#[test]
fn an_engine_that_dies_with_the_pack_mid_job_runs_it_again_without_it() {
    let Some((_, m)) = setup(None) else { return };
    let store = as_models::ModelStore::new(models_dir());
    if as_models::ids::ANIME.iter().any(|id| store.installed(id).unwrap().is_none()) {
        assert!(!models_required(), "anime models missing (run `cargo xtask fetch-models`)");
        eprintln!("anime models missing; run `cargo xtask fetch-models` (skipping)");
        return;
    }
    let pack = tempfile::tempdir().unwrap();
    let engine = Engine::new(launch_with_pack(ort_dir().unwrap(), pack.path(), Some("job")));
    let shots: Vec<(u32, u32)> = (0..m.frames).step_by(50).map(|s| (s, (s + 50).min(m.frames))).collect();
    let job = FacesInput { anime: true, ..faces_input(&m, shots, false, None) };
    let (mut engines, mut kills, mut end) = (0, 0, 0);
    engine
        .faces(job, &CancelToken::new(), |e| {
            match e {
                FacesEvent::Devices { .. } => {
                    engines += 1;
                    if kills < 2 {
                        kills += 1;
                        kill_hard(engine.pid().unwrap());
                    }
                }
                FacesEvent::Chunk { end: e, .. } => end = e,
                FacesEvent::Progress(_) => {}
            }
            Ok(())
        })
        .expect("the job finished: the pack's failure cost no attempt");
    // The one with the pack ended before it said anything; three without it followed.
    assert_eq!((engines, kills), (3, 2));
    assert_eq!(end, m.frames, "it ran to the end");
    assert_eq!(pack_failures(pack.path()), 1);
    assert_eq!(engine.accel(), Some(AccelState::Off));
}

/// An engine with NVIDIA's engine in use that dies under a job none of whose
/// models may run there (shots): a crash like any other. It costs an
/// attempt, the pack stays on, and nothing counts against it.
#[test]
fn a_death_under_a_job_the_pack_has_no_part_in_is_not_its_failure() {
    let Some((_, m)) = setup(None) else { return };
    let pack = tempfile::tempdir().unwrap();
    let engine = Engine::new(launch_with_pack(ort_dir().unwrap(), pack.path(), Some("job")));
    let (r, chunks) = collect(&engine, &m, 0, &CancelToken::new(), |_| {});
    // Every engine started with the pack, and each died at the job.
    assert!(matches!(r, Err(EngineError::Crashed(_))), "{r:?}");
    assert!(chunks.is_empty());
    assert_eq!(engine.accel(), Some(AccelState::Active), "the pack was never turned off");
    assert_eq!(pack_failures(pack.path()), 0);
}

/// A pack whose libraries aren't libraries (and the stamp says they are):
/// the engine says it failed, and runs on as it would without it. Where the
/// card in use isn't one the pack serves, it never loads at all.
#[test]
fn a_broken_accel_folder_leaves_the_engine_on_directml() {
    let Some(ort) = runtime_and_tools() else { return };
    let pack = tempfile::tempdir().unwrap();
    let files: Vec<&str> = accel::PRELOAD.iter().copied().chain([accel::PLUGIN]).collect();
    for name in &files {
        std::fs::write(pack.path().join(name), b"MZ not a library").unwrap();
    }
    accel::stamp(pack.path(), &files, &accel::PRELOAD).unwrap();
    let engine = Engine::new(launch_with_pack(ort, pack.path(), None));
    let hw = engine.probe(&CancelToken::new()).unwrap();
    let state = engine.accel().expect("an engine started");
    if hw.gpu().is_some_and(accel::serves) {
        assert!(matches!(state, AccelState::Failed { ref detail } if detail.contains("cudart64_13.dll")), "{state:?}");
    } else {
        assert_eq!(state, AccelState::Off, "{hw}");
    }
    assert_eq!(pack_failures(pack.path()), 0, "a pack that didn't load ended nothing");
    // Jobs run.
    let Some((_, m)) = setup(None) else { return };
    let with_pack = Engine::new(launch_with_pack(ort_dir().unwrap(), pack.path(), None));
    let (r, chunks) = collect(&with_pack, &m, 0, &CancelToken::new(), |_| {});
    r.unwrap();
    assert_eq!(chunks.len(), m.frames.div_ceil(100) as usize);
}

/// Local only (`AUTOSCENE_ACCEL_DIR`: an installed pack): the passes that
/// stay on DirectML run beside the registered plugin, time after time.
#[test]
fn directml_passes_run_beside_the_registered_pack() {
    let Some(pack) = std::env::var_os("AUTOSCENE_ACCEL_DIR").map(PathBuf::from) else {
        eprintln!("AUTOSCENE_ACCEL_DIR unset: no NVIDIA pack to test (skipping)");
        return;
    };
    let Some((_, m)) = setup(None) else { return };
    let engine = Engine::new(launch_with_pack(ort_dir().unwrap(), &pack, None));
    let store = as_models::ModelStore::new(models_dir());
    let fast = as_models::ids::FAST_FACES.iter().all(|id| store.installed(id).unwrap().is_some());
    let shots: Vec<(u32, u32)> = (0..m.frames).step_by(50).map(|s| (s, (s + 50).min(m.frames))).collect();
    for round in 0..10 {
        let (r, _) = collect(&engine, &m, 0, &CancelToken::new(), |_| {});
        r.unwrap_or_else(|e| panic!("shots, round {round}: {e:?}"));
        assert_eq!(engine.accel(), Some(AccelState::Active), "round {round}");
        if fast {
            let job = FacesInput { fast: true, ..faces_input(&m, shots.clone(), false, None) };
            engine
                .faces(job, &CancelToken::new(), |_| Ok(()))
                .unwrap_or_else(|e| panic!("Fast Scan, round {round}: {e:?}"));
        }
    }
    assert_eq!(pack_failures(&pack), 0);
}

fn faces_input(m: &Media, shots: Vec<(u32, u32)>, bodies: bool, known: Option<Vec<u8>>) -> FacesInput {
    FacesInput {
        path: m.path.clone(),
        info: m.info.clone(),
        index: m.index.clone(),
        shots,
        resume: 0,
        chunk: 100,
        bodies,
        known,
        animated: false,
        fast: false,
        anime: false,
    }
}

#[test]
fn people_come_with_the_faces_or_alone() {
    let Some((engine, m)) = setup(None) else { return };
    let store = as_models::ModelStore::new(models_dir());
    let needed = as_models::ids::FACES.iter().chain(&as_models::ids::BODIES);
    if needed.into_iter().any(|id| store.installed(id).unwrap().is_none()) {
        assert!(!models_required(), "face or people models missing (run `cargo xtask fetch-models`)");
        eprintln!("face or people models missing; run `cargo xtask fetch-models` (skipping)");
        return;
    }
    let shots: Vec<(u32, u32)> = (0..m.frames).step_by(50).map(|s| (s, (s + 50).min(m.frames))).collect();
    // With the faces, and for faces found before (here: none), the same chunks.
    let known = as_analysis::tracks::encode_known(&[]);
    for job in [faces_input(&m, shots.clone(), true, None), faces_input(&m, shots.clone(), true, Some(known))] {
        let alone = job.known.is_some();
        let mut ends = Vec::new();
        engine
            .faces(job, &CancelToken::new(), |e| {
                if let FacesEvent::Chunk { start, end, tracks, sprites, bodies } = e {
                    // Barcodes: no faces; whatever the person detector makes of
                    // the bars is in a list for every chunk, tied to no one.
                    let people = as_analysis::bodies::decode(&bodies).expect("a list of people for every chunk");
                    assert!(people.iter().all(|b| b.faces.is_empty() && start <= b.shot && b.end <= end));
                    assert!(people.iter().all(|b| b.dets.iter().all(|d| d.frame % 6 == 0)), "every second sample");
                    assert!(as_analysis::tracks::decode(&tracks).unwrap().is_empty());
                    assert_eq!(sprites.is_empty(), alone, "sprites come with faces found now");
                    ends.push(end);
                }
                Ok(())
            })
            .unwrap();
        assert_eq!(ends.last(), Some(&m.frames));
    }
}

//! Core ML on a Mac, against the CPU, and the compiled models it keeps
//! (issue 66). macOS only.
//!
//! GitHub's Mac runners are virtual machines without the Neural Engine or a
//! GPU. So these show that Core ML takes today's options, builds the models at
//! the engine's shapes, computes what the CPU computes and keeps its compile,
//! not where it runs a model or how fast (`coreml_bench`, on a real Mac).
//! CI fetches SCRFD, ArcFace and CCIP for them (`cargo xtask fetch-models
//! --only …`) and requires them; elsewhere they skip without the models.
#![cfg(target_os = "macos")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::SystemTime;

use as_infer::onnx::InputShape;
use as_infer::{Device, Runtime, TensorRef};
use as_models::ids;
use as_testkit::{models_dir, models_required, ort_dir};
use as_vision::{ArcFace, Ccip, arcface};

/// Side of the CCIP goldens' crops.
const SIDE: usize = 384;
/// The CCIP goldens' crops (RGB24, from CC-BY stills): real pictures for
/// every model here.
const CROPS: [&[u8]; 4] = [
    include_bytes!("../fixtures/anime/ccip_0.rgb"),
    include_bytes!("../fixtures/anime/ccip_1.rgb"),
    include_bytes!("../fixtures/anime/ccip_2.rgb"),
    include_bytes!("../fixtures/anime/ccip_3.rgb"),
];
/// The engine's batches: SCRFD frames, ArcFace faces and CCIP crops per call.
const DETECT_BATCH: usize = 4;
const EMBED_BATCH: usize = 32;
const CCIP_BATCH: usize = 4;

/// The runtime, on Automatic (one per process).
fn runtime() -> Option<&'static Runtime> {
    static RT: OnceLock<Option<Runtime>> = OnceLock::new();
    RT.get_or_init(|| {
        let rt = Runtime::load(&ort_dir()?, true).expect("onnxruntime loads");
        assert_eq!(rt.preferred_device(), Device::CoreMl, "a Mac's sessions start on Core ML");
        Some(rt)
    })
    .as_ref()
}

/// Model `id`'s file; `None` (the test skips) when it wasn't fetched and
/// nothing requires it.
fn model(id: &str) -> Option<PathBuf> {
    match as_models::ModelStore::new(models_dir()).downloaded(id).unwrap() {
        Some(path) => Some(path),
        None if models_required() => panic!("{id} missing (run `cargo xtask fetch-models --only {id}`)"),
        None => {
            eprintln!("{id} missing; run `cargo xtask fetch-models` (skipping)");
            None
        }
    }
}

/// `crops` as one NCHW batch, each byte mapped by `norm`.
fn planar(crops: &[&[u8]], side: usize, norm: impl Fn(u8) -> f32) -> Vec<f32> {
    let plane = side * side;
    let mut x = vec![0.0; crops.len() * 3 * plane];
    for (crop, slot) in crops.iter().zip(x.chunks_exact_mut(3 * plane)) {
        for (p, px) in crop.as_chunks::<3>().0.iter().enumerate() {
            for c in 0..3 {
                slot[c * plane + p] = norm(px[c]);
            }
        }
    }
    x
}

/// A `from`² RGB24 crop sampled down to `to`².
fn shrink(rgb: &[u8], from: usize, to: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(to * to * 3);
    for y in 0..to {
        for x in 0..to {
            let at = ((y * from / to) * from + x * from / to) * 3;
            out.extend_from_slice(&rgb[at..at + 3]);
        }
    }
    out
}

#[test]
fn coreml_matches_the_cpu() {
    let (Some(scrfd), Some(face), Some(ccip)) = (model(ids::SCRFD), model(ids::ARCFACE), model(ids::ANIME_CCIP)) else {
        return;
    };
    let Some(rt) = runtime() else { return };
    // As the app builds them: compiled into a cache.
    let cache = tempfile::tempdir().unwrap();
    let rt = rt.with_cache(Some(cache.path().to_path_buf()));

    // SCRFD: the score maps of its three strides, the crops as frames.
    let shape = [DETECT_BATCH, 3, SIDE, SIDE];
    let x = planar(&CROPS, SIDE, |v| (v as f32 - 127.5) / 128.0);
    let scores = |rt: &Runtime| {
        let (mut session, device) = rt.session_shaped(&scrfd, InputShape::Fixed(&shape)).unwrap();
        let input = session.inputs()[0].name().to_string();
        let t = TensorRef::from_array_view((shape, &x[..])).unwrap();
        let outs = session.run(as_infer::inputs![input.as_str() => t]).unwrap();
        let maps: Vec<Vec<f32>> = (0..3).map(|i| outs[i].try_extract_tensor::<f32>().unwrap().1.to_vec()).collect();
        (maps, device)
    };
    let (coreml, device) = scores(&rt);
    assert_eq!(device, Device::CoreMl, "SCRFD builds on Core ML");
    let (cpu, _) = scores(&rt.on_cpu());
    for (stride, (a, b)) in [8, 16, 32].into_iter().zip(coreml.iter().zip(&cpu)) {
        assert_eq!(a.len(), b.len());
        let worst = a.iter().zip(b).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(worst <= 1e-2, "SCRFD's scores at stride {stride} are off by {worst}");
    }

    // ArcFace: each crop's embedding points where the CPU's does.
    let faces: Vec<Vec<u8>> = CROPS.iter().map(|c| shrink(c, SIDE, arcface::CROP)).collect();
    let faces: Vec<&[u8]> = faces.iter().map(Vec::as_slice).collect();
    let mut on_coreml = ArcFace::load(&rt, &face, EMBED_BATCH).unwrap();
    assert_eq!(on_coreml.device(), Device::CoreMl, "ArcFace builds on Core ML");
    let mut on_cpu = ArcFace::load(&rt.on_cpu(), &face, EMBED_BATCH).unwrap();
    let (a, b) = (on_coreml.embed(&faces).unwrap(), on_cpu.embed(&faces).unwrap());
    for (i, (a, b)) in a.iter().zip(&b).enumerate() {
        let cos = arcface::cosine(&a.v, &b.v);
        assert!(cos >= 0.999, "ArcFace on crop {i}: cosine {cos} to the CPU's");
    }

    // CCIP proves itself against its CPU goldens as it loads, and moves to
    // the CPU if it misses them.
    let ccip = Ccip::load(&rt, &ccip, CCIP_BATCH).unwrap();
    assert_eq!(ccip.device(), Device::CoreMl, "CCIP on Core ML matches its CPU goldens");
}

#[test]
fn compiled_core_ml_models_are_kept() {
    let Some(scrfd) = model(ids::SCRFD) else { return };
    let Some(rt) = runtime() else { return };
    let cache = tempfile::tempdir().unwrap();
    let rt = rt.with_cache(Some(cache.path().to_path_buf()));
    let build = |side: usize| {
        let (_, device) = rt.session_shaped(&scrfd, InputShape::Fixed(&[1, 3, side, side])).unwrap();
        assert_eq!(device, Device::CoreMl, "SCRFD at {side}² builds on Core ML");
    };
    let folders = || -> Vec<PathBuf> {
        let mut all: Vec<PathBuf> = std::fs::read_dir(cache.path().join(as_infer::cache::COREML))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.is_dir())
            .collect();
        all.sort();
        all
    };

    build(320);
    let first = folders();
    assert_eq!(first.len(), 1, "the first build fills one folder: {first:?}");
    let kept = files(&first[0]);
    assert!(
        kept.keys().any(|p| p.components().any(|c| c.as_os_str().to_string_lossy().ends_with(".mlmodelc"))),
        "Core ML's compiled model is in it: {kept:?}"
    );
    assert!(first[0].join(as_infer::cache::USED).is_file(), "and it is marked finished");

    build(320);
    assert_eq!(folders(), first, "the same model and shape reuse their folder");
    assert_eq!(files(&first[0]), kept, "and what is in it: nothing was compiled again");

    build(256);
    let both = folders();
    assert_eq!(both.len(), 2, "another shape compiles into a folder of its own: {both:?}");
    assert!(both.contains(&first[0]));
}

/// Every file under `dir` but `.used` (which dates the last use), with its
/// size and date.
fn files(dir: &Path) -> BTreeMap<PathBuf, (u64, SystemTime)> {
    let mut found = BTreeMap::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(at) = todo.pop() {
        for entry in std::fs::read_dir(&at).unwrap() {
            let entry = entry.unwrap();
            let meta = entry.metadata().unwrap();
            if meta.is_dir() {
                todo.push(entry.path());
            } else if entry.file_name() != as_infer::cache::USED {
                let path = entry.path().strip_prefix(dir).unwrap().to_path_buf();
                found.insert(path, (meta.len(), meta.modified().unwrap()));
            }
        }
    }
    found
}

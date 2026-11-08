//! ArcFace's goldens on the devices this machine has (issue 67): the
//! processor gives the vectors stored with them, and a GPU session holds
//! them, or it would have been built on the processor. Core ML's misses are
//! logged, not enforced, so on a Mac the cosine is printed. Skips without the
//! runtime or the model.

use as_infer::{Device, Runtime};
use as_models::ids;
use as_testkit::{models_dir, models_required, ort_dir};
use as_vision::{ArcFace, golden};

#[test]
fn arcface_goldens_hold_on_every_device() {
    let Some(ort) = ort_dir() else {
        eprintln!("onnxruntime missing; run `cargo xtask fetch-sidecars` (skipping)");
        return;
    };
    let Some(model) = as_models::ModelStore::new(models_dir()).downloaded(ids::ARCFACE).unwrap() else {
        assert!(!models_required(), "{} missing (run `cargo xtask fetch-models`)", ids::ARCFACE);
        eprintln!("{} missing; run `cargo xtask fetch-models` (skipping)", ids::ARCFACE);
        return;
    };
    let rt = Runtime::load(&ort, true).expect("onnxruntime loads");
    // The processor: what the goldens were made with, in another program.
    let mut cpu = ArcFace::load(&rt.on_cpu(), &model, 32).unwrap();
    assert_eq!(cpu.device(), Device::Cpu);
    let worst = cpu.goldens().expect("embedded");
    assert!(worst >= 0.9999, "the processor gives the stored vectors: {worst}");
    // This machine's GPU, where it has one.
    let mut gpu = ArcFace::load(&rt, &model, 32).unwrap();
    let worst = gpu.goldens().expect("embedded");
    match rt.preferred_device() {
        Device::CoreMl => eprintln!("Core ML: worst cosine {worst} ({} if enforced)", worst >= golden::COSINE),
        Device::Cpu => assert_eq!(gpu.device(), Device::Cpu),
        preferred => {
            assert_eq!(gpu.device(), preferred, "the GPU session held its goldens ({worst})");
            assert!(worst >= golden::COSINE, "{worst}");
        }
    }
}

/// Berkeley's character embedder holds its goldens the same way, and says
/// how long a batch takes here (`--nocapture`).
#[test]
fn berkeley_goldens_hold_on_every_device() {
    let Some(ort) = ort_dir() else { return };
    let Some(model) = as_models::ModelStore::new(models_dir()).downloaded(ids::ANIME_BERKELEY).unwrap() else {
        eprintln!("{} missing (skipping)", ids::ANIME_BERKELEY);
        return;
    };
    let rt = Runtime::load(&ort, true).expect("onnxruntime loads");
    let mut gpu = as_vision::Berkeley::load(&rt, &model, 8).unwrap();
    let crops: Vec<Vec<u8>> = (0..64u8).map(|i| vec![i.wrapping_mul(37); as_vision::berkeley::CROP_BYTES]).collect();
    let refs: Vec<&[u8]> = crops.iter().map(|c| &c[..]).collect();
    gpu.embed(&refs[..8]).unwrap();
    let t = std::time::Instant::now();
    let e = gpu.embed(&refs).unwrap();
    eprintln!("Berkeley on {:?}: {:.1} ms a crop", gpu.device(), t.elapsed().as_secs_f64() * 1000.0 / 64.0);
    assert_eq!((e.len(), e[0].v.len()), (64, as_vision::berkeley::DIM));
    match rt.preferred_device() {
        Device::CoreMl | Device::Cpu => {}
        preferred => assert_eq!(gpu.device(), preferred, "the GPU session held its goldens"),
    }
}

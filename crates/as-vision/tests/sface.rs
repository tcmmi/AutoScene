//! SFace on the processor: a run holds just its faces, because a padded
//! batch would be computed in full. A face's embedding must not depend on
//! what shares its run: it is what a full batch gives it, to the bit, so
//! Fast Scan's tracks stay what padded batches made them. Skips without the
//! runtime or the model.

use as_infer::{Device, Runtime};
use as_models::ids;
use as_testkit::{models_dir, models_required, ort_dir};
use as_vision::{Embedding, SFace, arcface};

/// Whether two embeddings are the same to the bit, not just close.
fn same(a: &Embedding, b: &Embedding) -> bool {
    a.v.len() == b.v.len()
        && a.v.iter().zip(&b.v).all(|(x, y)| x.to_bits() == y.to_bits())
        && a.norm.to_bits() == b.norm.to_bits()
}

#[test]
fn a_face_embeds_the_same_on_the_processor_whatever_shares_its_run() {
    let Some(ort) = ort_dir() else {
        eprintln!("onnxruntime missing; run `cargo xtask fetch-sidecars` (skipping)");
        return;
    };
    let Some(model) = as_models::ModelStore::new(models_dir()).downloaded(ids::SFACE).unwrap() else {
        assert!(!models_required(), "{} missing (run `cargo xtask fetch-models`)", ids::SFACE);
        eprintln!("{} missing; run `cargo xtask fetch-models` (skipping)", ids::SFACE);
        return;
    };
    let rt = Runtime::load(&ort, false).expect("onnxruntime loads");
    let mut sface = SFace::load(&rt, &model, 16).unwrap();
    assert_eq!(sface.device(), Device::Cpu);
    // Four faces, over and over: a full batch of 16, and one left alone.
    let distinct = arcface::GOLDEN_RGB.len();
    let faces: Vec<&[u8]> = arcface::GOLDEN_RGB.iter().cycle().take(17).map(|r| &r[..]).collect();
    let full = sface.embed(&faces).unwrap();
    assert_eq!(full.len(), 17);
    for (i, e) in full.iter().enumerate() {
        assert!(e.norm > 1.0 && (e.v.iter().map(|v| v * v).sum::<f32>() - 1.0).abs() < 1e-4, "face {i}: {}", e.norm);
        // Wherever it sits in the batch, and alone after it.
        assert!(same(e, &full[i % distinct]), "face {i} differs from its first place");
    }
    assert!(!same(&full[0], &full[1]), "two people");
    // Any number of faces to a run, one alone included.
    for n in 1..=16 {
        let part = sface.embed(&faces[..n]).unwrap();
        assert_eq!(part.len(), n);
        for (i, e) in part.iter().enumerate() {
            assert!(same(e, &full[i]), "face {i} differs in a run of {n}");
        }
    }
    // And whatever batch the session was loaded for.
    for batch in [1, 3] {
        let mut other = SFace::load(&rt, &model, batch).unwrap();
        for (i, e) in other.embed(&faces[..7]).unwrap().iter().enumerate() {
            assert!(same(e, &full[i]), "face {i} differs in runs of {batch} at most");
        }
    }
}

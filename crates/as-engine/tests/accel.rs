//! NVIDIA's engine for GeForce RTX cards in the bundled runtime (issue 67).
//! Local only: they need an installed pack, named by `AUTOSCENE_ACCEL_DIR`,
//! a GeForce RTX card it serves, and the models; they skip without.
//!
//! - Every model allowed on the engine builds there with nothing left for
//!   the processor, at the engine's shapes for three aspect ratios.
//! - ArcFace computes there what DirectML computes, and holds its goldens.
//! - The person detector finds the same people there in real frames.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use as_analysis::{DETECT_BATCH, FaceParams, PERSON_INPUT, anime, detect_looks, frame_sizes};
use as_core::CancelToken;
use as_infer::accel::AccelState;
use as_infer::{Device, Placement, Runtime};
use as_media::{MediaInfo, Tools};
use as_models::ids;
use as_testkit::{models_dir, ort_dir};
use as_vision::{ArcFace, Ccip, Fit, Norm, PersonReid, PpDet, Scrfd, Yolo, golden};

/// The runtime with the pack registered (one per process), compiled models
/// kept between runs of these tests; `None` (the tests skip) without a pack.
fn runtime() -> Option<&'static Runtime> {
    static RT: OnceLock<Option<Runtime>> = OnceLock::new();
    RT.get_or_init(|| {
        let Some(pack) = std::env::var_os("AUTOSCENE_ACCEL_DIR").map(PathBuf::from) else {
            eprintln!("AUTOSCENE_ACCEL_DIR unset: no NVIDIA pack to test (skipping)");
            return None;
        };
        let cache = Some(std::env::temp_dir().join("autoscene-accel-tests"));
        let rt = Runtime::load_with(&ort_dir()?, &Placement { accel: Some(pack), cache, ..Placement::default() })
            .expect("onnxruntime loads");
        assert_eq!(
            rt.accel_state(),
            &AccelState::Active,
            "the pack in AUTOSCENE_ACCEL_DIR loads for {}",
            rt.hardware()
        );
        Some(rt)
    })
    .as_ref()
}

/// Model `id`'s file; it must be there where a pack is.
fn model(id: &str) -> PathBuf {
    as_models::ModelStore::new(models_dir())
        .downloaded(id)
        .unwrap()
        .unwrap_or_else(|| panic!("{id} missing (run `cargo xtask fetch-models`)"))
}

/// A video's description as `probe` gives it, at `w`×`h`.
fn info(w: u32, h: u32) -> MediaInfo {
    serde_json::from_value(serde_json::json!({
        "container": "matroska,webm", "durationSec": 1400.0, "startSec": 0.0, "sizeBytes": null, "bitRate": null,
        "video": {
            "index": 0, "codec": "h264", "profile": null, "pixFmt": "yuv420p", "bitDepth": 8,
            "width": w, "height": h, "sar": as_core::Rational::new(1, 1),
            "rFrameRate": as_core::Rational::new(24, 1), "avgFrameRate": null,
            "timeBase": as_core::Rational::new(1, 1000), "colorPrimaries": null,
            "colorTransfer": null, "colorSpace": null, "colorRange": null, "fieldOrder": null,
            "rotation": 0, "hdr": "none"
        },
        "audio": [], "subtitles": []
    }))
    .unwrap()
}

/// 16:9, 4:3 and 2.39:1.
const SHAPES: [(u32, u32); 3] = [(1920, 1080), (1440, 1080), (1920, 804)];

#[test]
fn every_accel_model_builds_without_cpu_fallback() {
    let Some(rt) = runtime() else { return };
    let on = |what: &str, device: Device| assert_eq!(device, Device::NvRtx, "{what}");
    for (w, h) in SHAPES {
        let info = info(w, h);
        // The anime pass (four frames per call) and anime Fast Scan (one).
        let size = anime::frame_size(&info, &anime::Params::anime());
        for batch in [DETECT_BATCH, 1] {
            let heads = Yolo::load(rt, &model(ids::ANIME_HEAD), size, (640, 640), batch).unwrap();
            on(&format!("anime heads {size:?} b{batch}"), heads.device());
            let persons = Yolo::load(rt, &model(ids::ANIME_PERSON), size, (960, 544), batch).unwrap();
            on(&format!("anime persons {size:?} b{batch}"), persons.device());
        }
        // The live-action models, should they be let on it: every look of
        // live action and of 3D animation, and the people.
        for params in [FaceParams::default(), FaceParams::animated()] {
            for look in detect_looks(&info, &params) {
                let scrfd = Scrfd::load(rt, &model(ids::SCRFD), look.size, DETECT_BATCH).unwrap();
                on(&format!("SCRFD {:?}", look.size), scrfd.device());
            }
        }
        let (frame, _) = frame_sizes(&info, &FaceParams::default());
        let people = PpDet::load(rt, &model(ids::PERSONS), frame, PERSON_INPUT, Fit::Pad, Norm::IMAGENET).unwrap();
        on(&format!("people {frame:?}"), people.device());
    }
    on("CCIP", Ccip::load(rt, &model(ids::ANIME_CCIP), 4).unwrap().device());
    on("ArcFace", ArcFace::load(rt, &model(ids::ARCFACE), 32).unwrap().device());
    on("ReID", PersonReid::load(rt, &model(ids::PERSON_REID), 32, Norm::IMAGENET).unwrap().device());
}

#[test]
fn accel_pack_loads_into_the_bundled_runtime() {
    let Some(rt) = runtime() else { return };
    let path = model(ids::ARCFACE);
    let mut rtx = ArcFace::load(rt, &path, 32).unwrap();
    let mut dml = ArcFace::load(&rt.without_accel(), &path, 32).unwrap();
    assert_eq!((rtx.device(), dml.device()), (Device::NvRtx, Device::DirectMl), "both held their goldens");
    let worst = rtx.goldens().unwrap();
    assert!(worst >= golden::COSINE, "{worst}");
    // The goldens, and the same faces mirrored: each the same direction on both.
    let mut faces: Vec<Vec<u8>> = as_vision::arcface::GOLDEN_RGB.iter().map(|f| f.to_vec()).collect();
    for f in faces.clone() {
        let side = as_vision::arcface::CROP;
        faces.push(f.chunks(side * 3).flat_map(|row| row.chunks(3).rev().flatten().copied()).collect());
    }
    let refs: Vec<&[u8]> = faces.iter().map(Vec::as_slice).collect();
    let (a, b) = (rtx.embed(&refs).unwrap(), dml.embed(&refs).unwrap());
    for (i, (a, b)) in a.iter().zip(&b).enumerate() {
        let cos: f32 = a.v.iter().zip(&b.v).map(|(x, y)| x * y).sum();
        assert!(cos >= 0.9999, "face {i}: {cos}");
    }
}

#[test]
fn ppdet_finds_the_same_people_on_rtx() {
    let Some(rt) = runtime() else { return };
    let tools = Tools::locate(None).expect("ffmpeg");
    let stills = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app/public/footage");
    let path = model(ids::PERSONS);
    for name in ["live-1.webp", "mixed-1.webp"] {
        let (w, h, rgb) = as_media::frames::decode_still(&tools, &stills.join(name), &CancelToken::new()).unwrap();
        let frame = (w as usize, h as usize);
        let mut rtx = PpDet::load(rt, &path, frame, PERSON_INPUT, Fit::Pad, Norm::IMAGENET).unwrap();
        let mut dml = PpDet::load(&rt.without_accel(), &path, frame, PERSON_INPUT, Fit::Pad, Norm::IMAGENET).unwrap();
        assert_eq!((rtx.device(), dml.device()), (Device::NvRtx, Device::DirectMl));
        let (a, b) = (rtx.detect(&rgb, 0.3).unwrap(), dml.detect(&rgb, 0.3).unwrap());
        assert!(!b.is_empty(), "{name}: people found on DirectML");
        assert_eq!(a.len(), b.len(), "{name}: {a:?} vs {b:?}");
        for (p, q) in a.iter().zip(&b) {
            assert!(iou(p.bbox, q.bbox) >= 0.95 && (p.score - q.score).abs() <= 0.02, "{name}: {p:?} vs {q:?}");
        }
    }
}

fn iou(a: [f32; 4], b: [f32; 4]) -> f32 {
    let (x0, y0, x1, y1) = (a[0].max(b[0]), a[1].max(b[1]), a[2].min(b[2]), a[3].min(b[3]));
    let inter = (x1 - x0).max(0.0) * (y1 - y0).max(0.0);
    let area = |r: [f32; 4]| (r[2] - r[0]) * (r[3] - r[1]);
    inter / (area(a) + area(b) - inter).max(1e-6)
}

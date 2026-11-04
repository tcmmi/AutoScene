//! Dev tool for a Mac (issue 66): how Core ML runs each analysis model today,
//! and in the setups it could run them in instead, so that a change of model
//! format or compute units is decided on measurements (design-i66_67 §5.5).
//!
//! `cargo run --release -p as-analysis --example coreml_bench -- <ort_dir> <models_dir> [seconds per setup]`
//!
//! Per model at the engine's shapes, and per setup (today's; NeuralNetwork on
//! every unit, on the processor and Neural Engine, on the processor and GPU;
//! an ML program on every unit; the processor only):
//! - loading: compiled with no cache (what every start cost before compiles
//!   were kept), the first load with an empty cache (compiled and kept), and
//!   the next one (what every later start costs);
//! - milliseconds per call, after a warm-up;
//! - processor time per second of wall time while it runs (1.0 is one core);
//! - against the processor's results on the same input: the lowest cosine
//!   of any item for the embedders, the largest difference for the detectors.
//!
//! For power, run `sudo powermetrics --samplers ane_power,gpu_power,cpu_power
//! -i 1000` in a second terminal: each setup prints the time (UTC) it starts
//! timing, to match its readings. The table at the end is Markdown, to paste
//! into the issue.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

use as_infer::onnx::InputShape;
use as_infer::{CoreMlFormat, CoreMlTrial, CoreMlUnits, Device, InferResult, Precision, Runtime, Session, TensorRef};
use as_models::ids;

/// What a model gives: one embedding per item, or detections.
#[derive(Clone, Copy, PartialEq)]
enum Gives {
    Embeddings,
    Detections,
}

/// The models, at the engine's shapes (`as-engine` `server.rs`).
const MODELS: [(&str, &str, [usize; 4], Precision, Gives); 5] = [
    ("SCRFD", ids::SCRFD, [4, 3, 384, 640], Precision::Fp16Safe, Gives::Detections),
    ("ArcFace", ids::ARCFACE, [32, 3, 112, 112], Precision::Fp16Safe, Gives::Embeddings),
    ("anime heads", ids::ANIME_HEAD, [4, 3, 640, 640], Precision::Fp32, Gives::Detections),
    ("anime people", ids::ANIME_PERSON, [4, 3, 544, 960], Precision::Fp32, Gives::Detections),
    ("CCIP", ids::ANIME_CCIP, [4, 3, 384, 384], Precision::Fp32, Gives::Embeddings),
];

const fn trial(format: Option<CoreMlFormat>, units: Option<CoreMlUnits>) -> CoreMlTrial {
    CoreMlTrial { format, units }
}

const SETUPS: [(&str, CoreMlTrial); 6] = [
    ("today", trial(None, None)),
    ("NeuralNetwork, all units", trial(Some(CoreMlFormat::NeuralNetwork), Some(CoreMlUnits::All))),
    (
        "NeuralNetwork, processor + Neural Engine",
        trial(Some(CoreMlFormat::NeuralNetwork), Some(CoreMlUnits::CpuAndNeuralEngine)),
    ),
    ("NeuralNetwork, processor + GPU", trial(Some(CoreMlFormat::NeuralNetwork), Some(CoreMlUnits::CpuAndGpu))),
    ("ML program, all units", trial(Some(CoreMlFormat::MlProgram), Some(CoreMlUnits::All))),
    ("processor only", trial(None, Some(CoreMlUnits::CpuOnly))),
];

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 2 {
        eprintln!("usage: coreml_bench <ort_dir> <models_dir> [seconds per setup]");
        std::process::exit(2);
    }
    if !cfg!(target_os = "macos") {
        println!("coreml_bench measures Core ML, which only a Mac has.");
        return;
    }
    let (ort, models) = (Path::new(&a[0]), Path::new(&a[1]));
    let secs: f64 = a.get(2).map_or(5.0, |s| s.parse().expect("seconds per setup"));
    let rt = Runtime::load(ort, true).expect("onnxruntime loads");
    println!("{}", rt.hardware());
    let store = as_models::ModelStore::new(models);

    let mut rows = Vec::new();
    for (label, id, shape, precision, gives) in MODELS {
        let Some(path) = store.installed(id).expect("model store") else {
            println!("{label}: not in {} (skipped)", models.display());
            continue;
        };
        let x = noise(shape.iter().product());
        let (mut cpu, _) = load(&rt.on_cpu(), &path, shape, precision).expect("builds on the processor");
        let reference = run(&mut cpu, shape, &x).expect("runs on the processor");
        drop(cpu);
        let mut seen = Vec::new();
        for (setup, trial) in SETUPS {
            // A setup that is today's under another name is measured once.
            let options = effective(trial.options(precision, None));
            if seen.contains(&options) {
                continue;
            }
            seen.push(options);
            println!("{label} / {setup}: loading");
            match measure(&rt.trying(trial), &path, shape, precision, gives, &x, &reference, secs) {
                Ok(m) => rows.push((label, setup, m)),
                Err(e) => println!("{label} / {setup}: {}", e.detail()),
            }
        }
    }

    println!();
    println!(
        "| model | setup | runs on | compile, no cache | first load, cached | next load | ms per call | CPU per wall | vs the processor |"
    );
    println!("|---|---|---|---|---|---|---|---|---|");
    for (label, setup, m) in rows {
        println!(
            "| {label} | {setup} | {} | {:.2} s | {:.2} s | {:.2} s | {:.1} | {} | {} |",
            m.device.label(),
            m.compile.as_secs_f64(),
            m.cold.as_secs_f64(),
            m.warm.as_secs_f64(),
            m.ms,
            m.cpu.map_or("?".into(), |c| format!("{c:.2}")),
            m.parity,
        );
    }
}

struct Measured {
    device: Device,
    compile: Duration,
    cold: Duration,
    warm: Duration,
    ms: f64,
    cpu: Option<f64>,
    parity: String,
}

#[allow(clippy::too_many_arguments)]
fn measure(
    rt: &Runtime,
    model: &Path,
    shape: [usize; 4],
    precision: Precision,
    gives: Gives,
    x: &[f32],
    reference: &[Vec<f32>],
    secs: f64,
) -> InferResult<Measured> {
    let timed = |rt: &Runtime| -> InferResult<(Session, Device, Duration)> {
        let t = Instant::now();
        let (session, device) = load(rt, model, shape, precision)?;
        Ok((session, device, t.elapsed()))
    };
    let (_, _, compile) = timed(&rt.with_cache(None))?;
    let cache = tempfile::tempdir().expect("a folder to compile into");
    let kept = rt.with_cache(Some(cache.path().to_path_buf()));
    let (first, _, cold) = timed(&kept)?;
    drop(first);
    let (mut session, device, warm) = timed(&kept)?;

    let got = run(&mut session, shape, x)?;
    for _ in 0..2 {
        run(&mut session, shape, x)?;
    }
    println!("  timing from {} UTC", utc(SystemTime::now()));
    let (wall, cpu_before) = (Instant::now(), cpu_seconds());
    let mut calls = Vec::new();
    while calls.len() < 5 || wall.elapsed().as_secs_f64() < secs {
        let t = Instant::now();
        run(&mut session, shape, x)?;
        calls.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let cpu = cpu_before.zip(cpu_seconds()).map(|(a, b)| (b - a) / wall.elapsed().as_secs_f64());
    calls.sort_by(f64::total_cmp);
    let parity = match gives {
        Gives::Embeddings => format!("cos ≥ {:.6}", lowest_cosine(&got[0], &reference[0], shape[0])),
        Gives::Detections => {
            let worst = got.iter().zip(reference).flat_map(|(a, b)| a.iter().zip(b)).map(|(a, b)| (a - b).abs());
            format!("max Δ {:.2e}", worst.fold(0.0, f32::max))
        }
    };
    Ok(Measured { device, compile, cold, warm, ms: calls[calls.len() / 2], cpu, parity })
}

fn load(rt: &Runtime, model: &Path, shape: [usize; 4], precision: Precision) -> InferResult<(Session, Device)> {
    match precision {
        Precision::Fp16Safe => rt.session_shaped(model, InputShape::Fixed(&shape)),
        Precision::Fp32 => rt.fp32_sessions().session_shaped(model, InputShape::Fixed(&shape)),
    }
}

/// Every output of one call.
fn run(session: &mut Session, shape: [usize; 4], x: &[f32]) -> InferResult<Vec<Vec<f32>>> {
    let input = session.inputs()[0].name().to_string();
    let outs = session.run(as_infer::inputs![input.as_str() => TensorRef::from_array_view((shape, x))?])?;
    let mut all = Vec::with_capacity(outs.len());
    for i in 0..outs.len() {
        all.push(outs[i].try_extract_tensor::<f32>()?.1.to_vec());
    }
    Ok(all)
}

/// The same input for every setup: values in [-1, 1).
fn noise(n: usize) -> Vec<f32> {
    let mut state = 0x9E37_79B9_7F4A_7C15_u64;
    (0..n)
        .map(|_| {
            state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            (state >> 40) as f32 / (1u64 << 23) as f32 - 1.0
        })
        .collect()
}

/// Options as Core ML reads them: compute units not named are all of them.
fn effective(options: Vec<(&'static str, String)>) -> BTreeMap<&'static str, String> {
    let mut all: BTreeMap<_, _> = options.into_iter().collect();
    all.entry("MLComputeUnits").or_insert_with(|| CoreMlUnits::All.option().into());
    all
}

/// The lowest cosine between the items of two batches of embeddings.
fn lowest_cosine(a: &[f32], b: &[f32], items: usize) -> f32 {
    let dim = a.len() / items;
    (0..items)
        .map(|i| {
            let (a, b) = (&a[i * dim..(i + 1) * dim], &b[i * dim..(i + 1) * dim]);
            let dot: f32 = a.iter().zip(b).map(|(a, b)| a * b).sum();
            let norm = |v: &[f32]| v.iter().map(|v| v * v).sum::<f32>().sqrt();
            dot / (norm(a) * norm(b)).max(f32::MIN_POSITIVE)
        })
        .fold(1.0, f32::min)
}

/// This process's processor time so far, in seconds (`ps`; Unix only).
fn cpu_seconds() -> Option<f64> {
    let out =
        std::process::Command::new("ps").args(["-o", "time=", "-p", &std::process::id().to_string()]).output().ok()?;
    // "M:SS.ss" or "H:MM:SS.ss".
    let text = String::from_utf8(out.stdout).ok()?;
    text.trim().split(':').try_fold(0.0, |total, part| Some(total * 60.0 + part.parse::<f64>().ok()?))
}

/// `HH:MM:SS` of `at`, in UTC.
fn utc(at: SystemTime) -> String {
    let s = at.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs()) % 86_400;
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

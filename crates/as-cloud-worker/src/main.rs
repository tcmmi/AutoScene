//! `autoscene-cloud-worker --engine PROGRAM [--ffmpeg DIR] [--work DIR] [-- ENGINE ARGS…]`
//! with the session's launch (JSON) on stdin.
//!
//! A serverless GPU service runs it once per session, inside the image that
//! holds the engine, its runtime and the models.

use std::io::Read;
use std::path::PathBuf;

use as_cloud::WorkerLaunch;
use as_cloud_worker::{EngineCommand, Verify, fake_engine, run};

fn main() {
    let mut args = std::env::args().skip(1);
    let mut program = None;
    let mut work = None;
    let mut engine_args = Vec::new();
    let mut tools = None;
    let mut verify = true;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--fake-engine" => std::process::exit(fake_engine()),
            "--engine" => program = args.next().map(PathBuf::from),
            "--work" => work = args.next().map(PathBuf::from),
            "--ffmpeg" => tools = args.next().map(PathBuf::from),
            // Tests only: a stand-in engine on bytes that aren't video.
            "--no-verify" => verify = false,
            "--" => engine_args.extend(args.by_ref()),
            other => {
                eprintln!("unknown argument {other}");
                std::process::exit(2);
            }
        }
    }
    let Some(program) = program else {
        eprintln!("usage: autoscene-cloud-worker --engine PROGRAM [--work DIR] [-- ENGINE ARGS…] < launch.json");
        std::process::exit(2);
    };
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        std::process::exit(2);
    }
    let launch: WorkerLaunch = match serde_json::from_str(&input) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("the launch isn't readable: {e}");
            std::process::exit(2);
        }
    };
    let work = work.unwrap_or_else(std::env::temp_dir).join(format!("autoscene-cloud-{}", launch.session));
    let verify = if verify { Verify::Own { tools } } else { Verify::Off };
    let done = run(&launch, &EngineCommand { program, args: engine_args }, &verify, &work);
    if let Some(e) = &done.error {
        eprintln!("{e}");
    }
    std::process::exit(if done.ok { 0 } else { 1 });
}

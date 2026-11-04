//! The analysis engine: ONNX Runtime and ffmpeg pipelines in a process of
//! their own.
//!
//! - A GPU or driver crash never takes the UI down; the job resumes from the
//!   app's checkpoints in a fresh process.
//! - `ort` binds one runtime per process (`load-dynamic`), so switching
//!   runtimes (DirectML ↔ CUDA speed pack) is a restart.
//! - VRAM is freed completely when the engine exits after being idle.
//!
//! The app starts its own executable with `--engine` (see [`main_from_args`]);
//! the `autoscene-engine` binary is the same engine for headless use.

pub mod client;
pub mod protocol;
pub mod server;

/// The devices an engine finds and opens ([`Engine::hardware`]), and where
/// its models run ([`Config::placement`]).
pub use as_infer::{Placement, hardware};
pub use client::{Devices, Engine, EngineError, FacesEvent, FacesInput, Launch, Note, ShotsEvent};
pub use server::{Config, serve};

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use as_infer::computer::Choice;

/// Idle time after which a serving engine exits on its own.
pub const IDLE_EXIT: Duration = Duration::from_secs(120);

/// Arguments for [`main_from_args`].
pub fn args(cfg: &Config) -> Vec<OsString> {
    let mut a: Vec<OsString> =
        vec!["--ort".into(), cfg.ort_dir.clone().into(), "--models".into(), cfg.models_dir.clone().into()];
    if let Some(d) = &cfg.ffmpeg_dir {
        a.extend(["--ffmpeg".into(), d.clone().into()]);
    }
    match cfg.placement.gpu {
        Choice::Auto => {}
        Choice::Processor => a.push("--cpu".into()),
        Choice::Card { vendor, device, nth } => {
            a.extend(["--gpu".into(), as_infer::hardware::key(vendor, device, "", nth).into()]);
        }
    }
    if let Some(d) = &cfg.placement.cache {
        a.extend(["--cache".into(), d.clone().into()]);
    }
    if let Some(idle) = cfg.idle_exit {
        a.extend(["--idle".into(), idle.as_secs().to_string().into()]);
    }
    a
}

const USAGE: &str =
    "usage: autoscene-engine --ort DIR --models DIR [--ffmpeg DIR] [--gpu KEY | --cpu] [--cache DIR] [--idle SECS]";

/// Serve on stdin/stdout. `args`: `--ort DIR --models DIR [--ffmpeg DIR]
/// [--gpu KEY | --cpu] [--cache DIR] [--idle SECS]`: `--gpu` names a
/// graphics card by its key (`as_infer::hardware::key`); with neither, the
/// engine runs on Automatic's. `--cache` keeps compiled models between runs
/// (`as_infer::cache`). Logs go to stderr. Returns the exit code.
pub fn main_from_args(args: impl IntoIterator<Item = OsString>) -> i32 {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_target(false)
        .with_max_level(tracing::Level::INFO)
        .without_time()
        .init();
    match config(args) {
        Ok(cfg) => serve(cfg, std::io::BufReader::new(std::io::stdin()), std::io::stdout()),
        Err(why) => {
            eprintln!("{why}");
            2
        }
    }
}

/// The engine's configuration from its arguments: [`args`]' inverse.
fn config(args: impl IntoIterator<Item = OsString>) -> Result<Config, String> {
    let mut ort_dir = None;
    let mut models_dir = None;
    let mut ffmpeg_dir = None;
    let mut placement = Placement::default();
    let mut idle_exit = None;
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.to_str() {
            Some("--ort") => ort_dir = it.next().map(PathBuf::from),
            Some("--models") => models_dir = it.next().map(PathBuf::from),
            Some("--ffmpeg") => ffmpeg_dir = it.next().map(PathBuf::from),
            Some("--cpu") => placement.gpu = Choice::Processor,
            Some("--gpu") => {
                let key = it.next().ok_or(USAGE)?;
                placement.gpu = Choice::from_setting(&key.to_string_lossy());
            }
            Some("--cache") => placement.cache = Some(it.next().map(PathBuf::from).ok_or(USAGE)?),
            Some("--idle") => idle_exit = it.next().and_then(|s| s.to_str()?.parse().ok()).map(Duration::from_secs),
            _ => return Err(format!("unknown argument {a:?}")),
        }
    }
    let (Some(ort_dir), Some(models_dir)) = (ort_dir, models_dir) else { return Err(USAGE.into()) };
    Ok(Config { ort_dir, models_dir, ffmpeg_dir, placement, idle_exit })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_round_trip_the_gpu_choice() {
        let base = Config {
            ort_dir: PathBuf::from("ort"),
            models_dir: PathBuf::from("models"),
            ffmpeg_dir: Some(PathBuf::from("ffmpeg")),
            placement: Placement::default(),
            idle_exit: Some(Duration::from_secs(120)),
        };
        for gpu in [
            Choice::Auto,
            Choice::Processor,
            Choice::Card { vendor: 0x10DE, device: 0x24C9, nth: 0 },
            Choice::Card { vendor: 0x10DE, device: 0x2204, nth: 1 },
        ] {
            let cfg = Config { placement: Placement { gpu, ..Placement::default() }, ..base.clone() };
            let back = config(args(&cfg)).unwrap();
            assert_eq!(
                (&back.placement, &back.ffmpeg_dir, back.idle_exit),
                (&cfg.placement, &cfg.ffmpeg_dir, cfg.idle_exit)
            );
            assert_eq!((back.ort_dir, back.models_dir), (cfg.ort_dir, cfg.models_dir));
        }
        let twin = Config {
            placement: Placement { gpu: Choice::Card { vendor: 0x10DE, device: 0x2204, nth: 1 }, cache: None },
            ..base
        };
        let line: Vec<String> = args(&twin).iter().map(|a| a.to_string_lossy().into_owned()).collect();
        assert!(line.windows(2).any(|w| w == ["--gpu", "10de:2204:#2"]), "{line:?}");
        // A key as Settings stores it, description and all.
        let named =
            ["--ort", "o", "--models", "m", "--gpu", "10de:24c9:NVIDIA GeForce RTX 3060 Ti"].map(OsString::from);
        assert_eq!(config(named).unwrap().placement.gpu, Choice::Card { vendor: 0x10DE, device: 0x24C9, nth: 0 });
        assert!(config(["--ort", "o", "--models", "m", "--gpu"].map(OsString::from)).is_err());
        assert!(config(["--ort", "o", "--bogus"].map(OsString::from)).is_err());
    }

    #[test]
    fn args_round_trip_the_cache() {
        let kept = Config {
            ort_dir: PathBuf::from("ort"),
            models_dir: PathBuf::from("models"),
            ffmpeg_dir: None,
            placement: Placement { gpu: Choice::Processor, cache: Some(PathBuf::from("data dir/cache")) },
            idle_exit: None,
        };
        let line: Vec<String> = args(&kept).iter().map(|a| a.to_string_lossy().into_owned()).collect();
        assert!(line.windows(2).any(|w| w == ["--cache", "data dir/cache"]), "{line:?}");
        assert_eq!(config(args(&kept)).unwrap().placement, kept.placement);
        // Without one, nothing is kept and no flag is passed.
        let none = Config { placement: Placement::default(), ..kept };
        assert!(!args(&none).iter().any(|a| a == "--cache"));
        assert_eq!(config(args(&none)).unwrap().placement.cache, None);
        assert!(config(["--ort", "o", "--models", "m", "--cache"].map(OsString::from)).is_err());
    }
}

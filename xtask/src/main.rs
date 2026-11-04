//! Dev tasks. `cargo xtask <task>`:
//!
//! - `fixtures [--out DIR] [--ffmpeg PATH]` — render synthetic test media with
//!   frame barcodes and a truth JSON (see `fixtures.rs`).
//! - `fetch-sidecars [--root DIR]` — download the pinned, checksum-verified
//!   ffmpeg/ffprobe and ONNX Runtime builds the app bundles (see `sidecars.rs`).
//! - `fetch-models [--dir DIR] [--only ID,ID…]` — download every AI model in
//!   `models/manifest.toml`, or only the ones named, into `fixtures/models`
//!   for tests.

mod fixtures;
mod sidecars;

use std::path::PathBuf;

use anyhow::{Context, Result, bail};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let task = args.next().unwrap_or_default();
    let rest: Vec<String> = args.collect();
    match task.as_str() {
        "fixtures" => {
            let out = flag(&rest, "--out").map(PathBuf::from).unwrap_or_else(|| workspace_root().join("fixtures/out"));
            let ffmpeg = flag(&rest, "--ffmpeg").unwrap_or_else(|| "ffmpeg".into());
            fixtures::generate(&out, &ffmpeg).context("generating fixtures")
        }
        "fetch-sidecars" => {
            let root =
                flag(&rest, "--root").map(PathBuf::from).unwrap_or_else(|| workspace_root().join("app/src-tauri"));
            sidecars::fetch(&root).context("fetching sidecars")
        }
        "fetch-models" => {
            let dir =
                flag(&rest, "--dir").map(PathBuf::from).unwrap_or_else(|| workspace_root().join("fixtures/models"));
            let store = as_models::ModelStore::new(&dir);
            for m in models(flag(&rest, "--only").as_deref())? {
                let cancel = as_core::CancelToken::new();
                let path = store.install(&m.id, &cancel, |_, _| {}).map_err(|e| anyhow::anyhow!(e.detail()))?;
                println!("{} ready: {}", m.id, path.display());
            }
            let freed = store.prune().map_err(|e| anyhow::anyhow!(e.detail()))?;
            if freed > 0 {
                println!("removed models no longer in the manifest ({} MB)", freed / 1_000_000);
            }
            Ok(())
        }
        "" | "help" | "--help" => {
            println!("cargo xtask fixtures [--out DIR] [--ffmpeg PATH]");
            println!("cargo xtask fetch-sidecars [--root DIR]");
            println!("cargo xtask fetch-models [--dir DIR] [--only ID,ID…]");
            Ok(())
        }
        other => bail!("unknown task '{other}' (try `cargo xtask help`)"),
    }
}

/// The manifest's models, or only those `only` names (ids, comma-separated):
/// CI's macOS job fetches just what the Core ML tests run.
fn models(only: Option<&str>) -> Result<Vec<&'static as_models::Model>> {
    let Some(only) = only else { return Ok(as_models::manifest().iter().collect()) };
    let ids: Vec<&str> = only.split(',').map(str::trim).filter(|id| !id.is_empty()).collect();
    if ids.is_empty() {
        bail!("--only names no model (ids are in models/manifest.toml)");
    }
    ids.into_iter()
        .map(|id| as_models::model(id).map_err(|_| anyhow::anyhow!("no model '{id}' in models/manifest.toml")))
        .collect()
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().expect("xtask lives in the workspace").to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_fetches_the_models_it_names() {
        let ids = |m: Vec<&as_models::Model>| m.into_iter().map(|m| m.id.clone()).collect::<Vec<_>>();
        assert_eq!(models(None).unwrap().len(), as_models::manifest().len());
        // What CI's macOS job fetches for the Core ML tests (ci.yml).
        let only = models(Some("scrfd-10g, arcface-glintr100,anime-ccip-caformer24")).unwrap();
        assert_eq!(ids(only), ["scrfd-10g", "arcface-glintr100", "anime-ccip-caformer24"]);
        assert!(models(Some("scrfd-10g,arcface")).is_err(), "an id not in the manifest");
        assert!(models(Some(" , ")).is_err(), "no id at all");
    }
}

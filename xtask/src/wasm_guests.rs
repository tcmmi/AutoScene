//! The WebAssembly plugins the sandbox tests run (`crates/as-plugin/tests/wasm.rs`).
//!
//! The test guest (`crates/as-plugin/tests/guest`, a workspace of its own)
//! is built for wasm32-wasip2 twice: against the world in
//! `crates/as-plugin/wit` (`guest.wasm`), and against the world 1.0.0 as it
//! first shipped, `wit-frozen/1.0.0` (`guest-frozen.wasm`), which every
//! later AutoScene must still run. Its lock decides its crates (`--locked`).
//! Beside them, `guests.sources` says what they were built from, so a test
//! tells a stale guest from a fault.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

use anyhow::{Context, Result, bail};

use crate::workspace_root;

const GUEST: &str = "crates/as-plugin/tests/guest/Cargo.toml";
const TARGET: &str = "wasm32-wasip2";
const BUILT: &str = "as_plugin_test_guest.wasm";

/// Builds both guests into `out` (made if it isn't there).
pub fn build(out: &Path) -> Result<()> {
    let root = workspace_root();
    // Beside the workspace's own builds, in a folder of their own.
    let target_dir =
        env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| root.join("target")).join("wasm-guests");
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
    for (features, name) in [(None, "guest.wasm"), (Some("frozen"), "guest-frozen.wasm")] {
        let mut cmd = Command::new(&cargo);
        cmd.current_dir(&root)
            .args(["build", "--release", "--locked", "--target", TARGET, "--manifest-path"])
            .arg(root.join(GUEST))
            .arg("--target-dir")
            .arg(&target_dir);
        if let Some(features) = features {
            cmd.args(["--features", features]);
        }
        let status = cmd.status().context("spawning cargo")?;
        if !status.success() {
            bail!(
                "building the test guest ({name}) failed: {status} (is the {TARGET} target installed? `rustup target add {TARGET}`)"
            );
        }
        let built = target_dir.join(TARGET).join("release").join(BUILT);
        fs::copy(&built, out.join(name)).with_context(|| format!("copying {}", built.display()))?;
        println!("{} ({} KiB)", out.join(name).display(), fs::metadata(out.join(name))?.len() / 1024);
    }
    // What they were built from: a test refuses guests built from other
    // sources (as_testkit::fixture_file).
    let stamp = out.join(Path::new(as_testkit::WASM_GUESTS_STAMP).file_name().expect("a file name"));
    fs::write(&stamp, as_testkit::wasm_guest_sources() + "\n")
        .with_context(|| format!("writing {}", stamp.display()))?;
    Ok(())
}

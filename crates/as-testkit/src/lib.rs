//! Shared support for synthetic test media.
//!
//! `cargo xtask fixtures` renders videos whose every frame carries a binary
//! **barcode**: the frame's index in the master timeline (16 bits) and the id
//! of the timeline element it belongs to (8 bits). Because the barcode
//! survives lossy encoding, any frame decoded from any variant or export can
//! be identified exactly — which is how frame-exact cutting is proven.

pub mod barcode;
pub mod truth;

pub use truth::{Element, ElementKind, Truth, Variant};

use std::path::PathBuf;

/// `fixtures/out` in the workspace (override with `AUTOSCENE_FIXTURES`).
pub fn fixtures_dir() -> PathBuf {
    std::env::var_os("AUTOSCENE_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/out"))
}

/// Load `truth.json`. `None` (the test skips) only when fixtures were never
/// generated on this machine and `AUTOSCENE_REQUIRE_FIXTURES` is unset; CI
/// sets it, so a missing truth can never make tests pass silently. A truth
/// file that exists but doesn't parse always fails.
pub fn load_truth() -> Option<Truth> {
    let p = fixtures_dir().join("truth.json");
    let raw = match std::fs::read_to_string(&p) {
        Ok(raw) => raw,
        Err(e) if !fixtures_required() && e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("fixtures missing; run `cargo xtask fixtures` (skipping)");
            return None;
        }
        Err(e) => panic!("{}: {e} (run `cargo xtask fixtures`)", p.display()),
    };
    match serde_json::from_str(&raw) {
        Ok(t) => Some(t),
        Err(e) => panic!("{} is stale or corrupt ({e}); run `cargo xtask fixtures`", p.display()),
    }
}

/// A file under [`fixtures_dir`] that an xtask made, by its path there
/// (`wasm/guest.wasm`, from `cargo xtask wasm-guests`). `None` (the test
/// skips) only when it isn't there and `AUTOSCENE_REQUIRE_FIXTURES` is
/// unset, as for [`load_truth`].
///
/// A test guest (`wasm/*.wasm`) built from other sources than this tree's
/// fails, whatever the setting: its tests would fail with no word of why.
pub fn fixture_file(rel: &str) -> Option<PathBuf> {
    let p = fixtures_dir().join(rel);
    if p.is_file() {
        if rel.starts_with("wasm/") && rel.ends_with(".wasm") {
            let stamp = fixtures_dir().join(WASM_GUESTS_STAMP);
            let built = std::fs::read_to_string(&stamp).unwrap_or_default();
            if built.trim() != wasm_guest_sources() {
                panic!(
                    "{} is stale: it was built from other sources than this tree's \
                     (run `cargo xtask wasm-guests`)",
                    p.display()
                );
            }
        }
        return Some(p);
    }
    let task = if rel.starts_with("wasm/") { "wasm-guests" } else { "fixtures" };
    if fixtures_required() {
        panic!("{} is missing (run `cargo xtask {task}`)", p.display());
    }
    eprintln!("{rel} missing; run `cargo xtask {task}` (skipping)");
    None
}

/// Where `cargo xtask wasm-guests` writes [`wasm_guest_sources`] for the
/// guests it built, under [`fixtures_dir`].
pub const WASM_GUESTS_STAMP: &str = "wasm/guests.sources";

/// What the test guests are built from, in this tree: the worlds, the
/// guest's crate and its lock (`crates/as-plugin/tests/guest`). A hash of
/// their text (line ends as LF), which isn't kept secret from anyone: it
/// only tells a stale guest.
pub fn wasm_guest_sources() -> String {
    const SOURCES: [&str; 5] = [
        "crates/as-plugin/wit/plugin.wit",
        "crates/as-plugin/wit-frozen/1.0.0/plugin.wit",
        "crates/as-plugin/tests/guest/Cargo.toml",
        "crates/as-plugin/tests/guest/Cargo.lock",
        "crates/as-plugin/tests/guest/src/lib.rs",
    ];
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    // FNV-1a, 64 bits: no crate needed, and the same on every platform.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for b in bytes {
            hash ^= u64::from(*b);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    };
    for rel in SOURCES {
        let text = std::fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
        let text = text.replace("\r\n", "\n");
        feed(rel.as_bytes());
        feed(&(text.len() as u64).to_le_bytes());
        feed(text.as_bytes());
    }
    format!("{hash:016x}")
}

/// CI sets `AUTOSCENE_REQUIRE_FIXTURES` so missing fixtures fail instead of skip.
fn fixtures_required() -> bool {
    std::env::var_os("AUTOSCENE_REQUIRE_FIXTURES").is_some_and(|v| v != "0")
}

/// The ONNX Runtime the app bundles (`cargo xtask fetch-sidecars`), or
/// `AUTOSCENE_ORT_DIR`. `None` (tests skip) when it isn't there and
/// `AUTOSCENE_REQUIRE_MODELS` is unset.
pub fn ort_dir() -> Option<PathBuf> {
    let dir = std::env::var_os("AUTOSCENE_ORT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../app/src-tauri/onnxruntime"));
    let lib = if cfg!(windows) {
        "onnxruntime.dll"
    } else if cfg!(target_os = "macos") {
        "libonnxruntime.dylib"
    } else {
        "libonnxruntime.so"
    };
    if dir.join(lib).is_file() {
        return Some(dir);
    }
    if models_required() {
        panic!("no ONNX Runtime in {} (run `cargo xtask fetch-sidecars`)", dir.display());
    }
    eprintln!("ONNX Runtime missing; run `cargo xtask fetch-sidecars` (skipping)");
    None
}

/// Where tests find AI models: `AUTOSCENE_MODELS_DIR`, else `fixtures/models`
/// (filled by `cargo xtask fetch-models`).
pub fn models_dir() -> PathBuf {
    std::env::var_os("AUTOSCENE_MODELS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/models"))
}

/// CI sets `AUTOSCENE_REQUIRE_MODELS` so missing models fail instead of skip.
pub fn models_required() -> bool {
    std::env::var_os("AUTOSCENE_REQUIRE_MODELS").is_some_and(|v| v != "0")
}

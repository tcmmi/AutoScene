//! NVIDIA's engine for GeForce RTX cards (TensorRT for RTX; issue 67,
//! design-i66_67 §5.1 and §5.4).
//!
//! NVIDIA ships it as a plugin for ONNX Runtime, which loads into the
//! bundled DirectML runtime: no second runtime, and DirectML stays loaded in
//! the same process for every model the plugin doesn't take. Its files (the
//! "pack") are NVIDIA's own and are fetched at the person's request, never
//! bundled; a pack folder holds them and [`STAMP`], the size and SHA-256 each
//! was verified against.
//!
//! At [`crate::Runtime::load_with`], and only when the card the analysis
//! uses is one the pack serves ([`serves`]), so its libraries never load on
//! a machine without an NVIDIA driver:
//! 1. the pack's files are checked against their stamp;
//! 2. the plugin's own libraries are loaded by full path, in [`PRELOAD`]'s
//!    order: the plugin then finds them, and the C++ runtime beside ONNX
//!    Runtime, already loaded;
//! 3. the plugin is registered under [`EP`];
//! 4. its device for the card in use is picked by LUID.
//!
//! Whatever goes wrong there leaves the runtime on DirectML and says why
//! ([`AccelState`]). A native crash can't be caught here: the app's engine
//! client counts it against the pack and starts the engine without it.

use std::fmt::Write as _;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::hardware::Gpu;

/// The name the plugin is registered under. It becomes the name of the
/// plugin's devices and the prefix of its option keys: registered under any
/// other, the devices this crate looks for don't exist, and a session
/// handed no device would quietly run on the processor.
pub const EP: &str = "NvTensorRTRTXExecutionProvider";

/// The plugin library ONNX Runtime registers.
pub const PLUGIN: &str = "onnxruntime_providers_nv_tensorrt_rtx.dll";

/// The libraries the plugin needs, loaded by full path in this order before
/// it is registered: the CUDA runtime, then TensorRT for RTX and the two
/// libraries built on it, which the plugin loads by name when it first
/// needs them.
pub const PRELOAD: [&str; 4] =
    ["cudart64_13.dll", "tensorrt_rtx_1_7.dll", "tensorrt_onnxparser_rtx_1_7.dll", "tensorrt_plugins.dll"];

/// The pack's release (the PyPI wheel's version), and the folder it lives in
/// under the app's runtimes (`AppPaths::runtimes()`).
pub const VERSION: &str = "0.5.0";
pub const FOLDER: &str = "nvidia-rtx-0.5.0";

/// The pack folder's record of its files: one `<sha256> <bytes> <name>` line
/// each, written once they were verified.
pub const STAMP: &str = ".autoscene-pack";

/// How many times in a row the pack ended an engine or failed a model, in
/// its folder (the engine client keeps it).
pub const CRASHES: &str = ".autoscene-crashes";

/// Whether `dir` holds a pack whose download finished: its stamp is there.
/// Its files are checked against the stamp where the pack loads.
pub fn installed(dir: &Path) -> bool {
    dir.join(STAMP).is_file()
}

/// The option (with [`EP`]'s prefix) naming the folder the plugin keeps
/// what it compiled for this card in.
pub const RUNTIME_CACHE: &str = "NvTensorRTRTXExecutionProvider.nv_runtime_cache_path";

/// NVIDIA's PCI vendor id.
const NVIDIA: u32 = 0x10DE;
/// The first PCI device id of NVIDIA's Ampere generation (GeForce RTX 30):
/// TensorRT for RTX runs on Ampere and newer only.
const FIRST_AMPERE: u32 = 0x2200;

/// What became of the pack a runtime was given.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AccelState {
    /// No pack was given, or the card in use isn't one it serves.
    #[default]
    Off,
    /// Registered, with a device for the card in use.
    Active,
    /// Registered, but the plugin lists no device for the card in use (an
    /// older driver, say).
    Unsupported,
    /// The pack didn't load; the runtime stays on DirectML.
    Failed { detail: String },
}

/// Whether the pack serves `gpu`: an NVIDIA card of the Ampere generation or
/// newer. Older GeForce cards (GTX 16 and RTX 20 included) and other makers'
/// never are; once the pack is registered, its device list has the last word.
pub fn serves(gpu: &Gpu) -> bool {
    gpu.vendor == NVIDIA && gpu.device >= FIRST_AMPERE
}

/// A pack checked against its stamp.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pack {
    pub dir: PathBuf,
    /// The plugin's SHA-256: its build, for the keys of what it compiles
    /// ([`crate::cache::key`]).
    pub plugin: String,
}

/// The pack in `dir`, if every file [`STAMP`] lists has the listed size and
/// hash, and the plugin and every library it needs are among them.
pub fn verify(dir: &Path) -> Result<Pack, String> {
    let stamp = fs::read_to_string(dir.join(STAMP)).map_err(|e| format!("{STAMP}: {e}"))?;
    let mut listed = Vec::new();
    for line in stamp.lines().filter(|l| !l.trim().is_empty()) {
        let mut parts = line.splitn(3, ' ');
        let (Some(sha256), Some(bytes), Some(name)) = (parts.next(), parts.next(), parts.next()) else {
            return Err(format!("{STAMP}: unreadable line {line:?}"));
        };
        let bytes: u64 = bytes.parse().map_err(|_| format!("{STAMP}: unreadable size in {line:?}"))?;
        // Only plain names: a stamp never points outside its folder.
        if name.contains(['/', '\\']) || name == ".." {
            return Err(format!("{STAMP}: {name:?} isn't a file of the pack"));
        }
        listed.push((sha256.to_ascii_lowercase(), bytes, name.to_string()));
    }
    for needed in PRELOAD.iter().chain([&PLUGIN]) {
        if !listed.iter().any(|(_, _, name)| name == needed) {
            return Err(format!("{STAMP} doesn't list {needed}"));
        }
    }
    for (sha256, bytes, name) in &listed {
        let path = dir.join(name);
        let size = fs::metadata(&path).map_err(|e| format!("{name}: {e}"))?.len();
        if size != *bytes {
            return Err(format!("{name}: {size} bytes, not {bytes}"));
        }
        let got = hash(&path).map_err(|e| format!("{name}: {e}"))?;
        if &got != sha256 {
            return Err(format!("{name}: SHA-256 {got}, not {sha256}"));
        }
    }
    let plugin = listed.iter().find(|(_, _, name)| name == PLUGIN).map(|(sha256, ..)| sha256.clone());
    Ok(Pack { dir: dir.to_path_buf(), plugin: plugin.unwrap_or_default() })
}

/// Write `dir`'s [`STAMP`] for `files` (names in `dir`), as they are now:
/// for the download that verified them.
pub fn stamp(dir: &Path, files: &[&str]) -> io::Result<()> {
    let mut lines = String::new();
    for name in files {
        let path = dir.join(name);
        let bytes = fs::metadata(&path)?.len();
        writeln!(lines, "{} {bytes} {name}", hash(&path)?).expect("writing to a String");
    }
    fs::write(dir.join(STAMP), lines)
}

fn hash(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// The pack, registered: its device for the card in use is the one whose
/// LUID is `luid`.
#[derive(Clone, Debug)]
#[cfg_attr(not(all(windows, target_arch = "x86_64")), allow(dead_code))]
pub(crate) struct Registered {
    pub luid: u64,
    pub plugin: String,
}

/// Check, load and register the pack in `dir` for the card in use (`gpu`),
/// once per process (one environment per process).
#[cfg(all(windows, target_arch = "x86_64"))]
pub(crate) fn register(
    env: &ort::environment::Environment,
    dir: &Path,
    gpu: Option<&Gpu>,
) -> (AccelState, Option<Registered>) {
    let Some(gpu) = gpu.filter(|g| serves(g)) else {
        tracing::info!("NVIDIA's engine doesn't serve the card in use; analysis runs without it");
        return (AccelState::Off, None);
    };
    let Some(luid) = gpu.luid else {
        return (AccelState::Unsupported, None);
    };
    let failed = |detail: String| {
        tracing::warn!(error = %detail, "NVIDIA's engine didn't load; analysis runs without it");
        (AccelState::Failed { detail }, None)
    };
    let started = std::time::Instant::now();
    let pack = match verify(dir) {
        Ok(pack) => pack,
        Err(e) => return failed(format!("the pack doesn't match its record: {e}")),
    };
    if let Err(e) = preload(dir) {
        return failed(e);
    }
    if let Err(e) = env.register_ep_library(EP, dir.join(PLUGIN)) {
        return failed(format!("registering {PLUGIN}: {e}"));
    }
    let listed = env.devices().any(|d| d.ep().is_ok_and(|ep| ep == EP) && crate::hardware::luid_of(&d) == Some(luid));
    let secs = started.elapsed().as_secs_f64();
    if !listed {
        tracing::warn!(secs, "NVIDIA's engine lists no device for this card; a newer driver may help");
        return (AccelState::Unsupported, None);
    }
    tracing::info!(secs, "NVIDIA's engine is ready for {}", gpu.label);
    (AccelState::Active, Some(Registered { luid, plugin: pack.plugin }))
}

/// Load the plugin's libraries by full path, each resolving what it needs
/// from its own folder and the system's.
#[cfg(all(windows, target_arch = "x86_64"))]
fn preload(dir: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::System::LibraryLoader::{
        LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LoadLibraryExW,
    };
    for name in PRELOAD {
        let wide: Vec<u16> = dir.join(name).as_os_str().encode_wide().chain([0]).collect();
        // SAFETY: a valid NUL-terminated wide path; the module stays loaded
        // for the life of the process, which the plugin expects.
        let module = unsafe {
            LoadLibraryExW(
                wide.as_ptr(),
                std::ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
            )
        };
        if module.is_null() {
            return Err(format!("loading {name}: {}", io::Error::last_os_error()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(vendor: u32, device: u32) -> Gpu {
        Gpu {
            key: String::new(),
            name: String::new(),
            label: String::new(),
            vendor,
            device,
            nth: 0,
            discrete: Some(true),
            memory_mb: None,
            dxgi: Some(0),
            high_performance: Some(0),
            luid: Some(67624),
        }
    }

    #[test]
    fn the_pack_serves_geforce_rtx_30_and_newer() {
        // RTX 3060 Ti (as ORT reports it), RTX 3090, RTX 4090, RTX 5090.
        for device in [0x24C9, 0x2204, 0x2684, 0x2B85] {
            assert!(serves(&card(NVIDIA, device)), "{device:#06x}");
        }
        // RTX 2080, GTX 1660 Ti, GTX 1080: older than Ampere.
        for device in [0x1E82, 0x2182, 0x1B80] {
            assert!(!serves(&card(NVIDIA, device)), "{device:#06x}");
        }
        // Another maker's card with a device id in NVIDIA's range.
        assert!(!serves(&card(0x1002, 0x73BF)) && !serves(&card(0x8086, 0x56A0)));
    }

    #[test]
    fn the_plugin_is_registered_under_its_ep_name() {
        // The registered name prefixes the plugin's options: the cache path
        // reaches the plugin only under this one.
        assert_eq!(EP, "NvTensorRTRTXExecutionProvider");
        assert_eq!(RUNTIME_CACHE.split_once('.'), Some((EP, "nv_runtime_cache_path")));
        assert!(!PRELOAD.contains(&PLUGIN));
        // One folder per release, so a new one never loads beside an old one's files.
        assert_eq!(FOLDER, format!("nvidia-rtx-{VERSION}"));
    }

    #[test]
    fn a_pack_is_used_only_as_its_stamp_records_it() {
        let dir = tempfile::tempdir().unwrap();
        let files: Vec<&str> = PRELOAD.iter().copied().chain([PLUGIN, "TRTRTX_Acknowledgements.txt"]).collect();
        for (i, name) in files.iter().enumerate() {
            fs::write(dir.path().join(name), format!("file {i}")).unwrap();
        }
        assert!(verify(dir.path()).unwrap_err().contains(STAMP), "no stamp, no pack");
        assert!(!installed(dir.path()));
        stamp(dir.path(), &files).unwrap();
        assert!(installed(dir.path()));
        let pack = verify(dir.path()).unwrap();
        assert_eq!(pack.plugin, hash(&dir.path().join(PLUGIN)).unwrap());
        assert_eq!(pack.plugin.len(), 64);

        // A file changed after it was verified: same size, other bytes.
        fs::write(dir.path().join("tensorrt_plugins.dll"), "file X").unwrap();
        assert!(verify(dir.path()).unwrap_err().starts_with("tensorrt_plugins.dll: SHA-256"));
        fs::write(dir.path().join("tensorrt_plugins.dll"), "file 3").unwrap();
        assert!(verify(dir.path()).is_ok());
        // Another size, or gone.
        fs::write(dir.path().join(PLUGIN), "a longer file").unwrap();
        assert!(verify(dir.path()).unwrap_err().contains("bytes, not"));
        fs::remove_file(dir.path().join(PLUGIN)).unwrap();
        assert!(verify(dir.path()).is_err());

        // A stamp that leaves a library out, or points outside the folder.
        stamp(dir.path(), &files[1..4]).unwrap();
        assert!(verify(dir.path()).unwrap_err().contains("doesn't list cudart64_13.dll"));
        fs::write(dir.path().join(STAMP), "00 1 ../outside.dll\n").unwrap();
        assert!(verify(dir.path()).unwrap_err().contains("isn't a file of the pack"));
    }
}

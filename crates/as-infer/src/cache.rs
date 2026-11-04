//! Compiled models kept between runs (design-i66_67 §5.1, issue 66).
//!
//! Core ML compiles every model it takes before the model's first run, which
//! a Mac does again at every engine start unless ONNX Runtime is given a
//! folder to keep the compile in (`ModelCacheDirectory`). ORT names what it
//! keeps there after the model's path, or, for a model handed over from
//! memory (every reshaped one), after its input and node names alone: not its
//! shapes, its weights or ORT's version (`coreml_execution_provider.cc:62-90`,
//! v1.31.1). Two shapes of one model, or a new release of it, would reuse each
//! other's compile.
//!
//! So each compile gets a folder of its own, `<cache>/<family>/<key>/`, whose
//! [`key`] hashes everything the compile depends on: the exact bytes handed to
//! ORT, the runtime's build and the provider's options. Another model, shape,
//! runtime or option is another folder, and a stale compile is never reused.
//!
//! - A folder is finished once it holds `.used`, written after a session was
//!   built from it ([`kept`]); its date is the folder's last use. A folder
//!   without it is a compile that never finished (the engine was stopped half
//!   way): the next build starts it afresh.
//! - At engine start, folders unused for [`UNUSED_FOR`] are deleted ([`prune`]).

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use sha2::{Digest, Sha256};

use crate::InferResult;

/// Core ML's compiled models: `<cache>/coreml/<key>/`.
pub const COREML: &str = "coreml";

/// The families [`prune`] looks through.
const FAMILIES: [&str; 1] = [COREML];

/// Marks a folder finished, and dates its last use.
pub const USED: &str = ".used";

/// How long a compiled model is kept without being used.
pub const UNUSED_FOR: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// The folder name for a compile of `bytes` (the model exactly as handed to
/// ORT) by the runtime build `runtime` with the provider `options`
/// ([`describe`]): the first 16 hex digits of a SHA-256 over the three.
pub fn key(bytes: &[u8], runtime: &str, options: &str) -> String {
    let mut hash = Sha256::new();
    for part in [bytes, runtime.as_bytes(), options.as_bytes()] {
        // Each part's length first, so no two different triples hash alike.
        hash.update((part.len() as u64).to_le_bytes());
        hash.update(part);
    }
    hash.finalize()[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// Provider options as one line for [`key`]: `name=value;…`, in their order.
pub fn describe(options: &[(&str, String)]) -> String {
    options.iter().map(|(name, value)| format!("{name}={value};")).collect()
}

/// `build` (a session of the model `model` names, for the log) with its
/// compile kept in `<root>/<family>/<key>/`: `build` gets the folder to
/// compile into, or reuse what an earlier start compiled there.
///
/// The cache never fails a session. Where the folder can't be written,
/// `build` gets `None` and compiles for this run only; a kept compile that
/// doesn't load is compiled again; a build that fails leaves nothing behind.
/// Another build of the same key in this process waits for this one, then
/// reuses what it kept.
pub fn kept<T>(
    root: &Path,
    family: &str,
    key: &str,
    model: &str,
    mut build: impl FnMut(Option<&Path>) -> InferResult<T>,
) -> InferResult<T> {
    let mut slot = match Slot::claim(root, family, key) {
        Ok(slot) => slot,
        Err(e) => {
            tracing::warn!(model, error = %e, "can't keep the compiled model; compiling it for this run only");
            return build(None);
        }
    };
    // Whether what was built came from the slot's folder (so it is kept).
    let mut in_slot = true;
    let built = match build(Some(slot.path())) {
        Err(e) if slot.reused() => {
            tracing::warn!(model, error = %e.detail(), "the kept compile didn't load; compiling the model again");
            if slot.clear().is_ok() {
                build(Some(slot.path()))
            } else {
                in_slot = false;
                build(None)
            }
        }
        built => built,
    };
    match built {
        Ok(built) => {
            if in_slot {
                tracing::debug!(model, key, reused = slot.reused(), "the compiled model is kept");
                if let Err(e) = slot.keep() {
                    tracing::debug!(model, error = %e, "couldn't mark the compile used");
                }
            }
            Ok(built)
        }
        Err(e) => {
            slot.discard();
            Err(e)
        }
    }
}

/// Folders a build in this process holds ([`Slot`]).
static HELD: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());
static RELEASED: Condvar = Condvar::new();

/// A compile's folder, held by one build: another build of the same model,
/// shape and options in this process waits for it.
#[derive(Debug)]
struct Slot {
    path: PathBuf,
    reused: bool,
}

impl Slot {
    /// Hold `<root>/<family>/<key>/` and make sure it exists. A folder left
    /// by a compile that never finished is emptied first; [`Slot::reused`]
    /// says whether a finished one was there.
    fn claim(root: &Path, family: &str, key: &str) -> io::Result<Self> {
        let path = root.join(family).join(key);
        let mut held = HELD.lock().unwrap_or_else(PoisonError::into_inner);
        while held.contains(&path) {
            held = RELEASED.wait(held).unwrap_or_else(PoisonError::into_inner);
        }
        held.insert(path.clone());
        drop(held);
        // From here on dropping the slot, on an error too, lets it go.
        let mut slot = Self { path, reused: false };
        slot.reused = slot.path.join(USED).is_file();
        if !slot.reused && slot.path.exists() {
            fs::remove_dir_all(&slot.path)?;
        }
        fs::create_dir_all(&slot.path)?;
        Ok(slot)
    }

    /// The folder to hand the provider.
    fn path(&self) -> &Path {
        &self.path
    }

    /// A finished compile was there to reuse.
    fn reused(&self) -> bool {
        self.reused
    }

    /// A session was built from the folder: it is finished, and used now.
    fn keep(&self) -> io::Result<()> {
        let used = fs::File::options().create(true).write(true).truncate(false).open(self.path.join(USED))?;
        used.set_modified(SystemTime::now())
    }

    /// The kept compile didn't load: empty the folder for a fresh one.
    fn clear(&mut self) -> io::Result<()> {
        self.reused = false;
        fs::remove_dir_all(&self.path)?;
        fs::create_dir_all(&self.path)
    }

    /// No session could be built: delete what the attempt left.
    fn discard(self) {
        if let Err(e) = fs::remove_dir_all(&self.path) {
            tracing::debug!(error = %e, "couldn't delete an unfinished compile; the next build or start does");
        }
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        HELD.lock().unwrap_or_else(PoisonError::into_inner).remove(&self.path);
        RELEASED.notify_all();
    }
}

/// What [`prune`] deleted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pruned {
    pub folders: u32,
    pub bytes: u64,
}

/// Delete the compiled models under `root` that were last used
/// [`UNUSED_FOR`] or longer before `now`. A folder's last use is the date of
/// its `.used`, or for a compile that never finished, the folder's own.
/// Nothing here fails: a folder that can't be deleted now is at a later start.
pub fn prune(root: &Path, now: SystemTime) -> Pruned {
    let mut pruned = Pruned::default();
    for family in FAMILIES {
        let Ok(entries) = fs::read_dir(root.join(family)) else { continue };
        for entry in entries.flatten() {
            // Only folders, and never through a link.
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let path = entry.path();
            let used = fs::metadata(path.join(USED)).or_else(|_| fs::metadata(&path)).and_then(|m| m.modified());
            if !used.is_ok_and(|used| now.duration_since(used).is_ok_and(|age| age >= UNUSED_FOR)) {
                continue;
            }
            let bytes = size(&path);
            match fs::remove_dir_all(&path) {
                Ok(()) => {
                    pruned.folders += 1;
                    pruned.bytes += bytes;
                }
                Err(e) => {
                    tracing::debug!(error = %e, "couldn't delete an unused compiled model; a later start tries again")
                }
            }
        }
    }
    pruned
}

/// Bytes of the files under `dir`, links not followed.
fn size(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else { return 0 };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => size(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: Duration = Duration::from_secs(24 * 60 * 60);

    #[test]
    fn cache_keys_differ_by_bytes_runtime_and_options() {
        let options = describe(&[("ModelFormat", "NeuralNetwork".into()), ("MLComputeUnits", "ALL".into())]);
        assert_eq!(options, "ModelFormat=NeuralNetwork;MLComputeUnits=ALL;");
        let base = key(b"model", "ORT 1.31.1", &options);
        assert_eq!(base.len(), 16);
        assert!(base.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()), "{base}");
        assert_eq!(key(b"model", "ORT 1.31.1", &options), base, "the same compile, the same folder");
        // Another shape is other bytes; so is another release of the model.
        assert_ne!(key(b"model2", "ORT 1.31.1", &options), base);
        assert_ne!(key(b"model", "ORT 1.31.2", &options), base);
        assert_ne!(key(b"model", "ORT 1.31.1", "ModelFormat=MLProgram;"), base);
        // Where one part ends and the next begins counts too.
        assert_ne!(key(b"ab", "c", ""), key(b"a", "bc", ""));
    }

    #[test]
    fn a_kept_compile_is_reused_and_one_that_doesnt_load_is_compiled_again() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("coreml/0123456789abcdef");
        // A provider as ORT's Core ML one behaves: it compiles into the
        // folder unless a compile is there, and fails on one it can't load.
        let compiles = std::cell::Cell::new(0);
        let build = |dir: Option<&Path>| -> InferResult<()> {
            let dir = dir.expect("a folder to compile into");
            if dir.join("broken").exists() {
                return Err(crate::InferError::Model("the compiled model doesn't load".into()));
            }
            if !dir.join("compiled").exists() {
                compiles.set(compiles.get() + 1);
                fs::write(dir.join("compiled"), b"x").unwrap();
            }
            Ok(())
        };
        let run = || kept(root.path(), COREML, "0123456789abcdef", "scrfd_10g.onnx", &build);

        run().unwrap();
        assert_eq!(compiles.get(), 1);
        assert!(folder.join("compiled").is_file() && folder.join(USED).is_file(), "kept, and marked finished");
        run().unwrap();
        assert_eq!(compiles.get(), 1, "the next start reuses it");

        // A compile that never finished (the engine stopped half way) is never reused.
        fs::remove_file(folder.join(USED)).unwrap();
        run().unwrap();
        assert_eq!(compiles.get(), 2);

        // A kept compile that doesn't load is compiled again, and kept.
        fs::write(folder.join("broken"), b"").unwrap();
        run().unwrap();
        assert_eq!(compiles.get(), 3);
        assert!(!folder.join("broken").exists() && folder.join(USED).is_file());

        // A model no session can be built from leaves nothing behind.
        let fails = |_: Option<&Path>| -> InferResult<()> { Err(crate::InferError::Model("unsupported".into())) };
        assert!(kept(root.path(), COREML, "fedcba9876543210", "x.onnx", fails).is_err());
        assert!(!root.path().join("coreml/fedcba9876543210").exists());

        // Where nothing can be written, the model is compiled for this run only.
        let file = root.path().join("a file");
        fs::write(&file, b"").unwrap();
        let mut given = Vec::new();
        kept(&file, COREML, "0123456789abcdef", "scrfd_10g.onnx", |dir| {
            given.push(dir.map(Path::to_path_buf));
            Ok(())
        })
        .unwrap();
        assert_eq!(given, [None]);
    }

    #[test]
    fn a_second_build_of_one_compile_waits_for_the_first() {
        let root = tempfile::tempdir().unwrap();
        let first = Slot::claim(root.path(), COREML, "feedfacefeedface").unwrap();
        let path = root.path().to_path_buf();
        let second = std::thread::spawn(move || {
            let slot = Slot::claim(&path, COREML, "feedfacefeedface").unwrap();
            slot.reused()
        });
        std::thread::sleep(Duration::from_millis(100));
        assert!(!second.is_finished(), "the second build waits while the first compiles");
        first.keep().unwrap();
        drop(first);
        assert!(second.join().unwrap(), "then reuses what the first kept");
        // Another compile never waits for this one.
        let held = Slot::claim(root.path(), COREML, "feedfacefeedface").unwrap();
        assert!(Slot::claim(root.path(), COREML, "0000000000000000").is_ok());
        drop(held);
    }

    #[test]
    fn unused_cache_folders_are_pruned() {
        let root = tempfile::tempdir().unwrap();
        let now = SystemTime::now();
        let folder = |key: &str, used: Option<SystemTime>| {
            let dir = root.path().join(COREML).join(key);
            fs::create_dir_all(dir.join("model")).unwrap();
            fs::write(dir.join("model/weights"), vec![0u8; 1000]).unwrap();
            if let Some(at) = used {
                let file = fs::File::create(dir.join(USED)).unwrap();
                file.set_modified(at).unwrap();
            }
            dir
        };
        let recent = folder("recent", Some(now - 29 * DAY));
        let old = folder("old", Some(now - 31 * DAY));
        let unfinished = folder("unfinished", None);
        // Not a compiled model's folder: left alone.
        fs::write(root.path().join(COREML).join("stray"), b"x").unwrap();
        let other = root.path().join("thumbnails/old");
        fs::create_dir_all(&other).unwrap();

        assert_eq!(prune(root.path(), now), Pruned { folders: 1, bytes: 1000 });
        assert!(recent.is_dir() && !old.exists());
        assert!(unfinished.is_dir(), "an unfinished compile is dated by its folder: just now");

        // A month on, the folder of the unfinished compile and the recent one are unused too.
        assert_eq!(prune(root.path(), now + 31 * DAY), Pruned { folders: 2, bytes: 2000 });
        assert!(!recent.exists() && !unfinished.exists());
        assert!(root.path().join(COREML).join("stray").is_file() && other.is_dir());
        // Nothing to look through is nothing deleted.
        assert_eq!(prune(&root.path().join("missing"), now), Pruned::default());
    }
}

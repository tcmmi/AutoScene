//! A plugin's compiled code: one `.cwasm` file per component, named by the
//! component's SHA-256 and the engine it was compiled for, in the plugin's
//! code folder (`cache/plugin-code/<id>`).
//!
//! That folder is never under one a plugin sees (`/data` is its data
//! folder, never this one) nor under its package folder, which is checked
//! against its `files.lock`. Code AutoScene didn't compile itself is never
//! loaded: a package that carries a `.cwasm` file is refused
//! ([`crate::package`]). A file is read into memory before it is loaded,
//! never mapped: Windows would keep a mapped file locked, and uninstall or
//! update would fail on it.

use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use wasmtime::Engine;
use wasmtime::component::Component;

use super::engine::engine_key;
use crate::error::{PluginError, PluginResult};

/// The file the code of the component `component_sha256` is kept in. The
/// hash must be one (64 lowercase hex digits, as a lock has it), so the
/// name is always in `dir`: [`load`] runs what it finds there.
pub(crate) fn file_name(component_sha256: &str) -> PluginResult<String> {
    let hex = |b: u8| matches!(b, b'0'..=b'9' | b'a'..=b'f');
    if component_sha256.len() != 64 || !component_sha256.bytes().all(hex) {
        return Err(PluginError::Invalid("The component's SHA-256 isn't 64 lowercase hex digits.".into()));
    }
    Ok(format!("{component_sha256}-{}.cwasm", engine_key()?))
}

/// The code of the component `sha256` compiled before into `dir`: none
/// when there is none; why, when its file was damaged or of another
/// engine, after the file was removed (the caller compiles it again).
pub(crate) fn load(engine: &Engine, dir: &Path, sha256: &str) -> Result<Option<Component>, String> {
    let path = dir.join(file_name(sha256).map_err(|e| e.detail())?);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(rebuilt(&path, e.to_string())),
    };
    #[allow(unsafe_code)]
    // SAFETY: the file is AutoScene's own output of Component::serialize, in
    // AutoScene's cache folder, which no plugin can see (it is never under a
    // preopen). wasmtime checks its header and engine before use. A process
    // that could rewrite it already runs as this user.
    let component = unsafe { Component::deserialize(engine, &bytes) };
    component.map(Some).map_err(|e| rebuilt(&path, format!("{e:#}")))
}

fn rebuilt(path: &Path, why: String) -> String {
    if let Err(e) = fs::remove_file(path)
        && e.kind() != ErrorKind::NotFound
    {
        tracing::warn!(error = %e, file = %path.display(), "couldn't remove a damaged compiled plugin");
    }
    tracing::info!(why = %why, "cache: compiled plugin rebuilt");
    why
}

/// Keeps the code of `component` (whose SHA-256 is `sha256`) in `dir`, made
/// if it isn't there: written beside its name first, then put in its
/// place. Gives its file.
pub(crate) fn save(dir: &Path, sha256: &str, component: &Component) -> PluginResult<PathBuf> {
    let bytes = component.serialize().map_err(|e| io::Error::other(format!("{e:#}")))?;
    fs::create_dir_all(dir)?;
    let file = dir.join(file_name(sha256)?);
    let part = dir.join(format!(".{}.part", uuid::Uuid::new_v4()));
    let written = fs::write(&part, &bytes).and_then(|()| fs::rename(&part, &file));
    if written.is_err() {
        let _ = fs::remove_file(&part);
    }
    written?;
    Ok(file)
}

/// Removes everything in the code folder `dir` but the file `keep`, as far
/// as it can: the code of an older version, and what a save left.
pub fn keep_only(dir: &Path, keep: &str) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if entry.file_name() == keep {
            continue;
        }
        let path = entry.path();
        let gone = match entry.file_type() {
            Ok(t) if t.is_dir() => fs::remove_dir_all(&path),
            _ => fs::remove_file(&path),
        };
        if let Err(e) = gone {
            tracing::debug!(error = %e, file = %path.display(), "a compiled plugin stays");
        }
    }
}

/// Removes the code folder `dir`, as far as it can (uninstall).
pub fn remove(dir: &Path) {
    if let Err(e) = fs::remove_dir_all(dir)
        && e.kind() != ErrorKind::NotFound
    {
        tracing::debug!(error = %e, dir = %dir.display(), "a plugin's compiled code stays");
    }
}

#[cfg(test)]
mod tests {
    use super::super::engine::engine;
    use super::*;

    const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn component() -> Component {
        Component::new(engine().unwrap(), wat::parse_str("(component)").unwrap()).unwrap()
    }

    #[test]
    fn a_file_is_named_by_its_component_and_the_engine() {
        let name = file_name(SHA).unwrap();
        let (sha, rest) = name.split_once('-').unwrap();
        assert_eq!(sha, SHA);
        let key = rest.strip_suffix(".cwasm").unwrap();
        assert_eq!(key.len(), 16);
        assert!(key.bytes().all(|b| b.is_ascii_hexdigit()), "{name}");
        assert_eq!(file_name(SHA).unwrap(), name);
        // Anything else never names a file, in the folder or out of it.
        for bad in ["../x", "", &SHA.to_uppercase(), &SHA[1..], &format!("{SHA}0"), &format!("../{}", &SHA[3..])] {
            assert!(matches!(file_name(bad), Err(PluginError::Invalid(_))), "{bad}");
        }
        let dir = tempfile::tempdir().unwrap();
        assert!(load(engine().unwrap(), dir.path(), "../x").is_err());
    }

    #[test]
    fn kept_code_loads_again() {
        let dir = tempfile::tempdir().unwrap();
        let code = dir.path().join("dev.autoscene.hello");
        let engine = engine().unwrap();
        assert!(matches!(load(engine, &code, SHA), Ok(None)));
        let file = save(&code, SHA, &component()).unwrap();
        assert_eq!(file, code.join(file_name(SHA).unwrap()));
        assert!(matches!(load(engine, &code, SHA), Ok(Some(_))));
        // Nothing but the file is left beside it.
        assert_eq!(fs::read_dir(&code).unwrap().count(), 1);
    }

    #[test]
    fn a_damaged_file_is_removed_and_compiled_again() {
        let dir = tempfile::tempdir().unwrap();
        let engine = engine().unwrap();
        let file = dir.path().join(file_name(SHA).unwrap());
        fs::write(&file, b"not code").unwrap();
        let Err(why) = load(engine, dir.path(), SHA) else { panic!("damaged code loaded") };
        assert!(!why.is_empty());
        assert!(!file.exists());
        assert!(matches!(load(engine, dir.path(), SHA), Ok(None)));
    }

    #[test]
    fn only_the_newest_code_stays() {
        let dir = tempfile::tempdir().unwrap();
        let kept = save(dir.path(), SHA, &component()).unwrap();
        fs::write(dir.path().join("older-0123456789abcdef.cwasm"), b"old").unwrap();
        fs::write(dir.path().join(".left.part"), b"part").unwrap();
        fs::create_dir(dir.path().join("stray")).unwrap();
        keep_only(dir.path(), &file_name(SHA).unwrap());
        let left: Vec<_> = fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().path()).collect();
        assert_eq!(left, [kept]);
        remove(dir.path());
        assert!(!dir.path().exists());
        // Gone already: nothing to do.
        remove(dir.path());
    }
}

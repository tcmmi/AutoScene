//! What a plugin keeps through `state.*` calls: `page-state.json` in its
//! data folder, one JSON object of keys and their values. A page in its
//! frame has no browser storage, so this is where it keeps what it needs
//! between visits; a run or a service may call it too.
//!
//! A key is 1 to [`MAX_KEY_CHARS`] letters, digits, dots, colons, dashes or
//! underscores; a value is at most [`MAX_VALUE_BYTES`] as JSON, and the
//! file at most [`MAX_FILE_BYTES`]. A write goes to a file beside it, which
//! is synced and then renamed over it, so a crash leaves the state as it
//! was or as it is now. A file that isn't JSON, or is too large, reads as
//! empty and is kept as `page-state.json.bad`. One that can't be read now
//! (another program holds it, the disk failed) fails the call and stays as
//! it is, with all it keeps. The calls for one folder take turns, and a
//! write whose caller is gone by its turn (a run that ended, say) writes
//! nothing. No write makes the folder: it is made for the plugin's runs,
//! and an uninstall may have just deleted it.
//!
//! The file is the host's: a plugin's program may read it in its data
//! folder, and must not write it. Windows refuses to replace it while a
//! program has it open without sharing its deletion (as Python's `open()`
//! does), so a write tries a few times. It goes with the plugin's data.
//!
//! Nothing stops a plugin from putting a link in its place, or in place of
//! the file a write goes to (a WebAssembly plugin can make links in its
//! `/data`), so the host follows neither: a link where the state should be
//! is never read (the call fails, as for a folder there), and a write makes
//! its file new, never opening one a plugin left. Else its page could read
//! a file of the person's, or another plugin's state, and a write could
//! change a file outside the folder.

use std::collections::{BTreeMap, HashMap};
use std::fs::{File, OpenOptions};
use std::io::{self, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use std::{fs, thread};

use as_core::CancelToken;
use parking_lot::Mutex;
use serde_json::Value;

use crate::error::{PluginError, PluginResult};

pub const FILE: &str = "page-state.json";
pub const MAX_KEY_CHARS: usize = 128;
pub const MAX_VALUE_BYTES: usize = 256 << 10;
pub const MAX_FILE_BYTES: usize = 1 << 20;

/// How many times a rename is tried again while it is refused, waiting
/// twice as long each time from [`RENAME_WAIT`]: 0.63 s in all.
const RENAME_RETRIES: u32 = 6;
const RENAME_WAIT: Duration = Duration::from_millis(10);

/// Opens a link itself, not what it leads to (`winbase.h`).
#[cfg(windows)]
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

/// The value of `key` in the state in `dir`, if it has one.
pub fn get(dir: &Path, key: &str) -> PluginResult<Option<Value>> {
    check_key(key)?;
    in_turn(dir, || Ok(read(dir)?.remove(key)))
}

/// Sets `key` to `value` in the state in `dir`, unless `ended` fired before
/// its turn came ([`PluginError::Cancelled`]).
pub fn set(dir: &Path, key: &str, value: Value, ended: &CancelToken) -> PluginResult<()> {
    check_key(key)?;
    if serde_json::to_vec(&value).map_or(true, |v| v.len() > MAX_VALUE_BYTES) {
        return Err(invalid(format!("The value of {key} is larger than 256 KB.")));
    }
    in_turn(dir, || {
        still(ended)?;
        let mut state = read(dir)?;
        state.insert(key.to_owned(), value);
        write(dir, &state)
    })
}

/// Removes `key` from the state in `dir`, unless `ended` fired before its
/// turn came; one it doesn't hold is no error.
pub fn delete(dir: &Path, key: &str, ended: &CancelToken) -> PluginResult<()> {
    check_key(key)?;
    in_turn(dir, || {
        still(ended)?;
        let mut state = read(dir)?;
        match state.remove(key) {
            Some(_) => write(dir, &state),
            None => Ok(()),
        }
    })
}

/// The keys of the state in `dir`, in order.
pub fn keys(dir: &Path) -> PluginResult<Vec<String>> {
    in_turn(dir, || Ok(read(dir)?.into_keys().collect()))
}

fn check_key(key: &str) -> PluginResult<()> {
    let plain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-');
    match (1..=MAX_KEY_CHARS).contains(&key.len()) && key.chars().all(plain) {
        true => Ok(()),
        false => Err(invalid("A state key is 1 to 128 letters, digits, dots, colons, dashes or underscores.")),
    }
}

/// Whether a write may still go ahead: not once its caller is gone.
fn still(ended: &CancelToken) -> PluginResult<()> {
    match ended.is_cancelled() {
        true => Err(PluginError::Cancelled),
        false => Ok(()),
    }
}

/// Does `f` while no other call reads or writes the state in `dir`.
fn in_turn<T>(dir: &Path, f: impl FnOnce() -> T) -> T {
    static TURNS: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> = OnceLock::new();
    let turn = TURNS.get_or_init(Mutex::default).lock().entry(dir.to_path_buf()).or_default().clone();
    let _turn = turn.lock();
    f()
}

/// The state in `dir`: empty when there is none, or when it is broken (then
/// it is kept as `.bad`, and the next write starts again). A file that
/// can't be read now is an error: a write would lose what it keeps. So is
/// anything else in its place, a folder or a link.
fn read(dir: &Path) -> PluginResult<BTreeMap<String, Value>> {
    let path = dir.join(FILE);
    let mut bytes = Vec::new();
    let read = open_plain(&path).and_then(|f| f.take(MAX_FILE_BYTES as u64 + 1).read_to_end(&mut bytes));
    match read {
        Ok(_) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(e) => {
            tracing::warn!(error = %e, "a plugin's page state can't be read");
            return Err(e.into());
        }
    }
    match serde_json::from_slice(&bytes).ok().filter(|_| bytes.len() <= MAX_FILE_BYTES) {
        Some(state) => Ok(state),
        None => {
            rename(&path, &dir.join(format!("{FILE}.bad")))?;
            Ok(BTreeMap::new())
        }
    }
}

/// `path` opened to read when it is a plain file, never through a link: on
/// Unix the open fails on one, on Windows it opens the link itself, which
/// isn't a plain file. Anything else is an error.
fn open_plain(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut options, libc::O_NOFOLLOW);
    #[cfg(windows)]
    std::os::windows::fs::OpenOptionsExt::custom_flags(&mut options, FILE_FLAG_OPEN_REPARSE_POINT);
    let file = options.open(path)?;
    match file.metadata()?.is_file() {
        true => Ok(file),
        false => Err(io::Error::other(format!("{FILE} isn't a plain file"))),
    }
}

fn write(dir: &Path, state: &BTreeMap<String, Value>) -> PluginResult<()> {
    let json = serde_json::to_vec(state).expect("page state is plain JSON");
    if json.len() > MAX_FILE_BYTES {
        return Err(invalid("The plugin keeps more than 1 MB. Delete something it keeps first."));
    }
    let tmp = dir.join(format!("{FILE}.tmp"));
    // Made new: what is in its place (one a crash left, or a plugin's
    // link) is removed, and `create_new` refuses whatever came back since,
    // a dangling link too.
    if let Err(e) = fs::remove_file(&tmp)
        && e.kind() != ErrorKind::NotFound
    {
        return Err(e.into());
    }
    let mut file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
    file.write_all(&json)?;
    file.sync_all()?;
    drop(file);
    rename(&tmp, &dir.join(FILE))?;
    Ok(())
}

/// Renames `from` to `to`, over what is there, trying again a few times
/// while it is refused: on Windows, while a program reads either file.
fn rename(from: &Path, to: &Path) -> io::Result<()> {
    let mut wait = RENAME_WAIT;
    for _ in 0..RENAME_RETRIES {
        match fs::rename(from, to) {
            Err(e) if e.kind() == ErrorKind::PermissionDenied => thread::sleep(wait),
            done => return done,
        }
        wait *= 2;
    }
    fs::rename(from, to)
}

fn invalid(message: impl Into<String>) -> PluginError {
    PluginError::Manifest(message.into())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A call's caller still there.
    fn on() -> CancelToken {
        CancelToken::new()
    }

    #[test]
    fn state_survives_a_crash_mid_write() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path().join("data");
        // Nothing kept yet, and no folder: a write makes none.
        assert_eq!((get(&dir, "seen").unwrap(), keys(&dir).unwrap()), (None, Vec::<String>::new()));
        assert!(matches!(set(&dir, "seen", json!(["e1"]), &on()), Err(PluginError::Io(_))) && !dir.exists());
        fs::create_dir(&dir).unwrap();
        set(&dir, "seen", json!(["e1"]), &on()).unwrap();
        set(&dir, "a:b.c-d_e", json!({"n": 1}), &on()).unwrap();
        // A write that stopped before its rename left its file beside the state.
        fs::write(dir.join(format!("{FILE}.tmp")), b"{\"seen\": [\"e1\", \"e").unwrap();
        assert_eq!(get(&dir, "seen").unwrap(), Some(json!(["e1"])));
        set(&dir, "seen", json!(["e1", "e2"]), &on()).unwrap();
        assert_eq!(get(&dir, "seen").unwrap(), Some(json!(["e1", "e2"])));
        assert_eq!(keys(&dir).unwrap(), ["a:b.c-d_e", "seen"]);
        delete(&dir, "a:b.c-d_e", &on()).unwrap();
        delete(&dir, "never", &on()).unwrap();
        assert_eq!(keys(&dir).unwrap(), ["seen"]);

        // A state file someone cut short reads as empty, and is kept aside.
        fs::write(dir.join(FILE), b"{\"seen\": [\"e1\"").unwrap();
        assert_eq!(get(&dir, "seen").unwrap(), None);
        assert_eq!(fs::read(dir.join(format!("{FILE}.bad"))).unwrap(), b"{\"seen\": [\"e1\"");
        set(&dir, "seen", json!([]), &on()).unwrap();
        assert_eq!(fs::read_to_string(dir.join(FILE)).unwrap(), r#"{"seen":[]}"#);
        // So does one that isn't an object.
        fs::write(dir.join(FILE), b"[1, 2]").unwrap();
        assert_eq!(keys(&dir).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn state_that_cant_be_read_now_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        // A folder where the file should be: every call fails, and nothing moves.
        fs::create_dir(dir.join(FILE)).unwrap();
        assert!(get(dir, "seen").is_err() && keys(dir).is_err());
        assert!(set(dir, "seen", json!(1), &on()).is_err() && delete(dir, "seen", &on()).is_err());
        assert!(dir.join(FILE).is_dir() && !dir.join(format!("{FILE}.bad")).exists());
        fs::remove_dir(dir.join(FILE)).unwrap();

        // A file another program holds whole (Windows): a write fails and loses nothing.
        set(dir, "kept", json!(1), &on()).unwrap();
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            let held = fs::OpenOptions::new().read(true).share_mode(0).open(dir.join(FILE)).unwrap();
            assert!(matches!(set(dir, "new", json!(2), &on()), Err(PluginError::Io(_))));
            drop(held);
            assert_eq!(keys(dir).unwrap(), ["kept"]);
        }
        set(dir, "new", json!(2), &on()).unwrap();
        assert_eq!(keys(dir).unwrap(), ["kept", "new"]);
    }

    #[test]
    fn links_a_plugin_left_in_its_folder_are_never_followed() {
        let root = tempfile::tempdir().unwrap();
        let (dir, outside) = (root.path().join("data"), root.path().join("outside"));
        fs::create_dir(&dir).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("secret.json"), br#"{"secret": 1}"#).unwrap();
        fs::write(outside.join("target.json"), b"untouched").unwrap();
        let up = |name: &str| Path::new("..").join("outside").join(name);
        // Windows makes links only in developer mode.
        if !link(&up("secret.json"), &dir.join(FILE)) {
            eprintln!("this account can't make links (skipping)");
            return;
        }
        // As for a folder there: every call fails, nothing is read, and nothing moves.
        assert!(get(&dir, "secret").is_err() && keys(&dir).is_err());
        assert!(set(&dir, "k", json!(1), &on()).is_err());
        assert!(fs::symlink_metadata(dir.join(FILE)).unwrap().is_symlink());
        assert!(!dir.join(format!("{FILE}.bad")).exists());
        fs::remove_file(dir.join(FILE)).unwrap();

        // A write makes its file new, never through a link in its place.
        assert!(link(&up("target.json"), &dir.join(format!("{FILE}.tmp"))));
        set(&dir, "k", json!(1), &on()).unwrap();
        assert_eq!(fs::read(outside.join("target.json")).unwrap(), b"untouched");
        assert!(!fs::symlink_metadata(dir.join(FILE)).unwrap().is_symlink());
        assert_eq!(keys(&dir).unwrap(), ["k"]);
    }

    /// Makes `link` a link to `target`: false where this account can't
    /// (Windows without developer mode).
    fn link(target: &Path, link: &Path) -> bool {
        #[cfg(unix)]
        return std::os::unix::fs::symlink(target, link).map(|()| true).unwrap();
        #[cfg(windows)]
        return std::os::windows::fs::symlink_file(target, link).is_ok();
    }

    /// As Python's `open()` reads it: others may read and write it, but not
    /// delete it, which is what replacing it does on Windows.
    #[cfg(windows)]
    #[test]
    fn a_write_waits_for_a_program_reading_the_state() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        set(dir, "kept", json!(1), &on()).unwrap();
        // FILE_SHARE_READ | FILE_SHARE_WRITE
        let reader = fs::OpenOptions::new().read(true).share_mode(1 | 2).open(dir.join(FILE)).unwrap();
        assert!(fs::rename(dir.join(FILE), dir.join("moved")).is_err());
        // Longer than half the time a write waits.
        let reading = thread::spawn(move || {
            thread::sleep(Duration::from_millis(400));
            drop(reader);
        });
        set(dir, "new", json!(2), &on()).unwrap();
        reading.join().unwrap();
        assert_eq!(keys(dir).unwrap(), ["kept", "new"]);
    }

    #[test]
    fn state_calls_take_turns() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        // Several callers, each with keys of its own, all at once: none
        // loses another's write, and none fails on another's file.
        thread::scope(|s| {
            for caller in 0..8 {
                s.spawn(move || {
                    for n in 0..25 {
                        set(dir, &format!("k{caller}-{n}"), json!(n), &on()).unwrap();
                    }
                });
            }
        });
        assert_eq!(keys(dir).unwrap().len(), 8 * 25);
    }

    #[test]
    fn a_write_whose_caller_ended_while_it_waited_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        let ended = CancelToken::new();
        set(dir, "kept", json!(1), &ended).unwrap();
        // Another call has its turn while this one waits, and its caller ends.
        thread::scope(|s| {
            let waiting = in_turn(dir, || {
                let waiting = s.spawn(|| (set(dir, "late", json!(2), &ended), delete(dir, "kept", &ended)));
                thread::sleep(Duration::from_millis(100));
                ended.cancel();
                waiting
            });
            let (set, delete) = waiting.join().unwrap();
            assert!(matches!((set, delete), (Err(PluginError::Cancelled), Err(PluginError::Cancelled))));
        });
        assert_eq!(keys(dir).unwrap(), ["kept"]);
    }

    #[test]
    fn state_past_its_size_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        let text = |bytes: usize| json!("x".repeat(bytes - 2));
        set(dir, "big", text(MAX_VALUE_BYTES), &on()).unwrap();
        let e = set(dir, "bigger", text(MAX_VALUE_BYTES + 1), &on()).unwrap_err();
        assert_eq!(e.to_string(), "The value of bigger is larger than 256 KB.");
        // Values that fit one by one, until the file would pass 1 MB.
        for key in ["b", "c"] {
            set(dir, key, text(MAX_VALUE_BYTES), &on()).unwrap();
        }
        let e = set(dir, "d", text(MAX_VALUE_BYTES), &on()).unwrap_err();
        assert_eq!(e.to_string(), "The plugin keeps more than 1 MB. Delete something it keeps first.");
        // What was kept stays as it was.
        assert_eq!(keys(dir).unwrap(), ["b", "big", "c"]);
        assert!(fs::metadata(dir.join(FILE)).unwrap().len() <= MAX_FILE_BYTES as u64);
        // Keys are short and plain.
        for key in ["", "a b", "../x", "ключ", &"k".repeat(MAX_KEY_CHARS + 1)] {
            assert!(matches!(get(dir, key), Err(PluginError::Manifest(_))), "{key}");
            assert!(set(dir, key, json!(1), &on()).is_err(), "{key}");
        }
        set(dir, &"k".repeat(MAX_KEY_CHARS), json!(1), &on()).unwrap();
    }
}

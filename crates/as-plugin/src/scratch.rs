//! A run's scratch folder: a folder of its own, which AutoScene empties when
//! the run ends.
//!
//! A run of a plugin of protocol 2, each start of a service and every run of
//! a WebAssembly plugin get one ([`Scratch`]): `<temp>/as-<8 of a-z0-9>`,
//! made as `mkdtemp` makes one. `<temp>` is the system's temp folder, but for
//! a WebAssembly plugin: on some systems that folder is memory, and all that
//! keeps a sandboxed plugin from filling it is its limit on time. Its folder
//! is on the disk instead ([`Scratch::on_disk`]), in AutoScene's data folder.
//! A name that is taken (by a folder, a file or
//! a link) is never used, and another is tried; on Linux and macOS only its
//! user may enter it. It holds the run's [`MARKER`] and the run's temp
//! folder [`TEMP`], and the plugin is given that one alone: a WebAssembly
//! plugin can't reach the marker, and a plugin that copies or lists its
//! temp folder never meets it. The path is short on purpose: Python puts
//! its sockets in `$TMPDIR/pymp-xxxxxxxx/sock-xxxxxxxx`, which must fit in
//! 108 bytes on Linux and 92 on macOS, and a longer folder sends them
//! elsewhere or fails. It sits where a plugin's temp files went before, on
//! the same drive.
//!
//! The run holds a lock on the marker while it lives, and while its folder
//! is removed. When it ends, the folder is removed on a thread of its own
//! (20,000 files take seconds): a link in it is removed, never followed, and
//! so is a link that took the folder's place. On Linux and macOS a folder
//! in it that refuses (one a plugin left read only) is given this user's
//! leave first, as Python does for its own temp folders. The marker goes
//! last. A file still held (by a virus scanner, or a process still ending)
//! is retried for about ten seconds ([`RETRY_WAITS`]); then the folder is
//! left, with its marker (as it is when the app ends first), to the next
//! start's [`sweep`], which removes each scratch folder whose marker no run
//! holds. A folder without the marker is never touched: it isn't
//! AutoScene's.

use std::cell::RefCell;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

/// How each scratch folder's name starts.
pub const PREFIX: &str = "as-";
/// The characters of a name after its [`PREFIX`], of `a-z0-9`.
pub const NAME_CHARS: usize = 8;
/// The file in a scratch folder that marks it as a run's; the run holds a
/// lock on it while it lives, and while its folder is removed.
pub const MARKER: &str = ".autoscene-run";
/// The run's temp folder in its scratch folder, beside the marker: all of
/// it the plugin is given ([`Scratch::path`]).
pub const TEMP: &str = "t";
/// The marker while it is made: it has its name only once it is locked, so
/// a sweep never finds the marker of a live run unlocked.
const NEW_MARKER: &str = ".autoscene-run.new";
/// Names tried before a new folder gives up.
const TRIES: usize = 8;
/// The waits between the tries of a removal that failed: five more, about
/// ten seconds in all. Then the folder is the sweep's.
pub const RETRY_WAITS: [Duration; 5] = [
    Duration::from_millis(500),
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(3),
    Duration::from_millis(3500),
];

thread_local! {
    /// Where this thread makes its scratch folders, when not in the
    /// system's temp folder ([`with_temp_root`]).
    static TEMP_ROOT: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Runs `f` with the scratch folders this thread makes ([`Scratch::new`])
/// in `root` in place of the system's temp folder: for tests, which keep
/// theirs apart from a real AutoScene's, or have them fail.
#[doc(hidden)]
pub fn with_temp_root<R>(root: &Path, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<PathBuf>);
    impl Drop for Restore {
        fn drop(&mut self) {
            TEMP_ROOT.set(self.0.take());
        }
    }
    let _restore = Restore(TEMP_ROOT.replace(Some(root.to_owned())));
    f()
}

/// A run's scratch folder, removed when it drops.
#[derive(Debug)]
pub struct Scratch {
    /// `<temp>/as-xxxxxxxx`, with the marker and the run's temp folder.
    root: PathBuf,
    /// The run's temp folder, [`TEMP`] in `root`.
    path: PathBuf,
    /// The marker, locked while the run lives and its folder is removed.
    marker: Option<File>,
}

impl Scratch {
    /// A new scratch folder in the system's temp folder.
    pub fn new() -> io::Result<Self> {
        match TEMP_ROOT.with_borrow(Clone::clone) {
            Some(root) => Self::new_in(&root),
            None => Self::new_in(&std::env::temp_dir()),
        }
    }

    /// A new scratch folder in `root`, which is made when it isn't there:
    /// for a run that shouldn't use the system's temp folder.
    pub fn on_disk(root: &Path) -> io::Result<Self> {
        match TEMP_ROOT.with_borrow(Clone::clone) {
            Some(root) => Self::new_in(&root),
            None => {
                fs::create_dir_all(root)?;
                Self::new_in(root)
            }
        }
    }

    /// A new scratch folder in `temp`.
    pub fn new_in(temp: &Path) -> io::Result<Self> {
        Self::named(temp, std::iter::repeat_with(random_name))
    }

    /// A new scratch folder in `temp`, named by the first of `names` that
    /// no folder, file or link has, of the first [`TRIES`].
    fn named(temp: &Path, names: impl IntoIterator<Item = String>) -> io::Result<Self> {
        let mut taken = None;
        for name in names.into_iter().take(TRIES) {
            let root = temp.join(format!("{PREFIX}{name}"));
            match make_dir(&root) {
                Ok(()) => return Self::marked(root),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => taken = Some(e),
                Err(e) => return Err(e),
            }
        }
        Err(taken.unwrap_or_else(|| io::Error::new(io::ErrorKind::AlreadyExists, "no scratch folder name was free")))
    }

    /// The folder at `root`, made just now, with the run's temp folder in
    /// it and its marker locked; it is gone again when that fails.
    fn marked(root: PathBuf) -> io::Result<Self> {
        let marked = (|| {
            let path = root.join(TEMP);
            make_dir(&path)?;
            let new = root.join(NEW_MARKER);
            let marker = OpenOptions::new().read(true).write(true).create_new(true).open(&new)?;
            match marker.try_lock() {
                Ok(()) => {}
                // No one else has it: it was made just now.
                Err(TryLockError::WouldBlock) => return Err(io::Error::other("a new marker was locked already")),
                // A file system that can't lock (some network or FUSE mounts):
                // the run goes on, and a sweep, which can't lock either, never
                // takes the folder.
                Err(TryLockError::Error(e)) => tracing::debug!(error = %e, "a scratch folder's marker can't be locked"),
            }
            fs::rename(&new, root.join(MARKER))?;
            Ok((path, marker))
        })();
        match marked {
            Ok((path, marker)) => Ok(Self { root, path, marker: Some(marker) }),
            Err(e) => {
                fs::remove_dir_all(&root).ok();
                Err(e)
            }
        }
    }

    /// The run's temp folder: the folder the plugin is given.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The scratch folder, with the run's temp folder and the marker.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// Its run ended: the folder is removed on a thread of its own, retried for
/// a while ([`RETRY_WAITS`]), then left to the sweep. The marker stays open
/// and locked until all else is gone: no sweep takes the folder meanwhile,
/// and Windows moves no folder while a file in it is open.
impl Drop for Scratch {
    fn drop(&mut self) {
        let marker = self.marker.take();
        let root = std::mem::take(&mut self.root);
        let gone = root.clone();
        let spawned = thread::Builder::new()
            .name("plugin-scratch".into())
            .spawn(move || remove_retrying(&gone, marker, &RETRY_WAITS, thread::sleep));
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "a scratch folder's removal couldn't start a thread");
            remove_retrying(&root, None, &[], |_| {});
        }
    }
}

/// Removes the scratch folder `path`, and tries again after each of `waits`
/// (waited by `sleep`) while that fails. `marker` is the run's open marker:
/// closed just before the marker goes, or once the removal gave up. Whether
/// it went; one that didn't is logged, and left to the sweep with its
/// marker.
pub(crate) fn remove_retrying(
    path: &Path,
    mut marker: Option<File>,
    waits: &[Duration],
    sleep: impl Fn(Duration),
) -> bool {
    let mut waits = waits.iter();
    loop {
        match remove(path, &mut marker) {
            Ok(()) => return true,
            Err(e) => match waits.next() {
                Some(wait) => sleep(*wait),
                None => {
                    drop(marker);
                    // Its name, never its path: that names the user.
                    let folder = path.file_name().unwrap_or_default().to_string_lossy();
                    tracing::warn!(%folder, error = %e, "a plugin run's scratch folder is left for the next start");
                    return false;
                }
            },
        }
    }
}

/// Removes the scratch folder `path` and all it holds ([`remove_once`]). On
/// Linux and macOS one that fails is tried once more, once this user was
/// given leave to change each folder of its own in it ([`allow_removal`]).
fn remove(path: &Path, marker: &mut Option<File>) -> io::Result<()> {
    let removed = remove_once(path, marker);
    #[cfg(unix)]
    if removed.is_err() {
        allow_removal(path);
        return remove_once(path, marker);
    }
    removed
}

/// Removes the scratch folder `path` and all it holds: a link in it is
/// removed, never followed, and so is a link (or a file) that took its
/// place. Its marker goes last, closed (`marker`) just before, so a removal
/// that fails, or that ends with the app, leaves the folder marked for the
/// sweep; one that failed past the marker gets it back, never through a
/// link. Something of the marker's name that isn't a file goes as all else
/// does.
///
/// What it holds is read by its path. A plugin's tree has ended by then,
/// and a WebAssembly plugin only ever had [`TEMP`] in it. On Windows the
/// open marker keeps the folder in place until all else is gone. On Linux
/// and macOS a process of the person's that swapped the folder for a link
/// at the moment it is read could have the removal take what the link leads
/// to: a process with the person's rights, which could remove that itself.
fn remove_once(path: &Path, marker: &mut Option<File>) -> io::Result<()> {
    let kind = match fs::symlink_metadata(path) {
        Ok(m) => m.file_type(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if !kind.is_dir() {
        return remove_entry(path);
    }
    let emptied = (|| {
        let mut failed = None;
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let is_marker = entry.file_name() == MARKER && entry.file_type().is_ok_and(|t| t.is_file());
            if !is_marker && let Err(e) = remove_entry(&entry.path()) {
                failed.get_or_insert(e);
            }
        }
        if let Some(e) = failed {
            return Err(e);
        }
        drop(marker.take());
        gone(fs::remove_file(path.join(MARKER)))?;
        gone(fs::remove_dir(path))
    })();
    if emptied.is_err() && fs::symlink_metadata(path).is_ok_and(|m| m.is_dir()) {
        OpenOptions::new().write(true).create_new(true).open(path.join(MARKER)).ok();
    }
    gone(emptied)
}

/// Removes `path` as what it is: a folder with all it holds (a link in it
/// goes as a link), or a link or a file alone, never what a link leads to.
fn remove_entry(path: &Path) -> io::Result<()> {
    let kind = match fs::symlink_metadata(path) {
        Ok(m) => m.file_type(),
        Err(e) => return gone(Err(e)),
    };
    gone(if kind.is_dir() {
        remove_tree(path)
    } else if is_dir_link(&kind) {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    })
}

/// Removes the folder `path` and all it holds.
#[cfg(windows)]
fn remove_tree(path: &Path) -> io::Result<()> {
    fs::remove_dir_all(path)
}

/// Removes the folder `path` and all it holds, however deep. std's removal
/// keeps a file open for each level, so a plugin that nests folders past the
/// limit on open files (a WebAssembly plugin can, with `create_dir` alone)
/// would leave a tree no try ever removes. Here the folders are lifted
/// instead: a folder in a folder of `path` is renamed into `path`, with all
/// it holds, and the one it left goes. Nothing holds more than one folder
/// open, no path has more than three names, and each pass leaves the tree a
/// level flatter. A removal that stops part way leaves it flatter too, under
/// the same `path`.
#[cfg(not(windows))]
fn remove_tree(path: &Path) -> io::Result<()> {
    let mut lifted = 0_u64;
    loop {
        let entries = fs::read_dir(path)?.collect::<io::Result<Vec<_>>>()?;
        if entries.is_empty() {
            return fs::remove_dir(path);
        }
        for entry in entries {
            if !entry.file_type()?.is_dir() {
                gone(fs::remove_file(entry.path()))?;
                continue;
            }
            let folder = entry.path();
            for inner in fs::read_dir(&folder)? {
                let inner = inner?;
                if !inner.file_type()?.is_dir() {
                    gone(fs::remove_file(inner.path()))?;
                    continue;
                }
                // A rename would take the place of an empty folder of this name.
                let free = loop {
                    lifted += 1;
                    let free = path.join(format!("lifted-{lifted}"));
                    if fs::symlink_metadata(&free).is_err() {
                        break free;
                    }
                };
                fs::rename(inner.path(), free)?;
            }
            gone(fs::remove_dir(&folder))?;
        }
    }
}

/// Gives this user leave to read, change and enter `dir` and each folder in
/// it (`u+rwx`), as Python's `shutil.rmtree` does for its temp folders when
/// a removal is refused: a folder a plugin left read only (an unpacked
/// archive, a copied tree, a `chmod`) can then be emptied. A link is never
/// followed, and a folder of another user is left as it is. Each folder is
/// changed by its path: a process of the person's that swapped it for a
/// link meanwhile could have the change made to what the link leads to, as
/// it could make that change itself.
#[cfg(unix)]
fn allow_removal(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut folders = vec![dir.to_owned()];
    while let Some(dir) = folders.pop() {
        let Ok(m) = fs::symlink_metadata(&dir) else { continue };
        if !m.is_dir() || !mine(&m) {
            continue;
        }
        let mode = m.permissions().mode() & 0o7777;
        if mode & 0o700 != 0o700 {
            fs::set_permissions(&dir, fs::Permissions::from_mode(mode | 0o700)).ok();
        }
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        folders.extend(entries.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path()));
    }
}

/// `r`, where a thing already gone counts as removed.
fn gone(r: io::Result<()>) -> io::Result<()> {
    match r {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        r => r,
    }
}

/// Whether `kind` is a link to a folder that Windows removes as a folder (a
/// junction, or a folder's symlink); on Linux and macOS a link is a file.
#[cfg(windows)]
fn is_dir_link(kind: &fs::FileType) -> bool {
    use std::os::windows::fs::FileTypeExt;
    kind.is_symlink_dir()
}

#[cfg(not(windows))]
fn is_dir_link(_: &fs::FileType) -> bool {
    false
}

/// Removes the scratch folders in `temp` that runs left (an app that
/// crashed, a removal that failed): each folder of this user named as a
/// scratch folder is, whose marker no run holds. One without the marker (or
/// with something else of its name), a link, and one whose marker a run
/// holds (of this AutoScene, or another one) stay. Gives how many went.
/// AutoScene does this once at its start; a run that starts meanwhile holds
/// its marker before it is named.
pub fn sweep(temp: &Path) -> usize {
    let Ok(entries) = fs::read_dir(temp) else { return 0 };
    let mut removed = 0;
    for entry in entries.flatten() {
        if !is_scratch_name(&entry.file_name()) {
            continue;
        }
        let path = entry.path();
        let ours = fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir() && mine(&m));
        if !ours || !unheld(&path) {
            continue;
        }
        // Its marker stays when it fails: a later start tries again.
        if remove(&path, &mut None).is_ok() {
            removed += 1;
        } else {
            let folder = entry.file_name().to_string_lossy().into_owned();
            tracing::warn!(%folder, "a plugin run's scratch folder couldn't be removed");
        }
    }
    removed
}

/// Whether `name` is a scratch folder's: [`PREFIX`] and [`NAME_CHARS`] of
/// `a-z0-9`.
fn is_scratch_name(name: &OsStr) -> bool {
    name.to_str().and_then(|n| n.strip_prefix(PREFIX)).is_some_and(|rest| {
        rest.len() == NAME_CHARS && rest.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    })
}

/// Whether the folder `folder` has a marker, a file of its own, that no run
/// holds: one this process could lock. A marker that can't be locked at all
/// (a file system without locks) counts as held.
fn unheld(folder: &Path) -> bool {
    let marker = folder.join(MARKER);
    if !fs::symlink_metadata(&marker).is_ok_and(|m| m.is_file()) {
        return false;
    }
    let Ok(file) = File::open(&marker) else { return false };
    // Unlocked again as it drops.
    file.try_lock().is_ok()
}

/// Whether this user owns the folder of `m`.
#[cfg(unix)]
fn mine(m: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    m.uid() == effective_uid()
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn effective_uid() -> u32 {
    // SAFETY: geteuid takes nothing, can't fail and touches no memory of ours.
    unsafe { libc::geteuid() }
}

/// Windows keeps a temp folder per user.
#[cfg(not(unix))]
fn mine(_: &fs::Metadata) -> bool {
    true
}

/// Makes the folder `path`, only for this user on Linux and macOS. Fails
/// when anything has its name, a link too, which is never followed.
fn make_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(path)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir(path)
    }
}

/// [`NAME_CHARS`] random characters of `a-z0-9`, from the 62 random bits at
/// the end of a version 4 UUID.
fn random_name() -> String {
    const CHARS: &[u8; 36] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut bits = uuid::Uuid::new_v4().as_u128() & ((1 << 62) - 1);
    (0..NAME_CHARS)
        .map(|_| {
            let c = CHARS[(bits % 36) as usize];
            bits /= 36;
            char::from(c)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    /// Whether `marker` is held: this process can't lock it.
    fn held(marker: &Path) -> bool {
        matches!(File::open(marker).unwrap().try_lock(), Err(TryLockError::WouldBlock))
    }

    #[test]
    fn a_scratch_folder_is_named_short_and_marked_while_it_lives() {
        let temp = tempfile::tempdir().unwrap();
        let s = Scratch::new_in(temp.path()).unwrap();
        let name = s.root().file_name().unwrap().to_owned();
        assert!(is_scratch_name(&name), "{name:?}");
        assert_eq!(s.root().parent(), Some(temp.path()));
        // The marker, held by the run, sits beside the run's temp folder,
        // which is all the plugin is given, and empty.
        assert_eq!(s.path(), s.root().join(TEMP));
        let mut names: Vec<_> = fs::read_dir(s.root()).unwrap().map(|e| e.unwrap().file_name()).collect();
        names.sort();
        assert_eq!(names, [MARKER, TEMP]);
        assert_eq!(fs::read_dir(s.path()).unwrap().count(), 0);
        assert!(held(&s.root().join(MARKER)));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for dir in [s.root(), s.path()] {
                assert_eq!(fs::metadata(dir).unwrap().permissions().mode() & 0o077, 0, "{}", dir.display());
            }
        }
        // Two runs, two folders.
        let other = Scratch::new_in(temp.path()).unwrap();
        assert_ne!(other.root(), s.root());
        // A thread told where to make them makes them there, and fails
        // where it can't; then it is back to the system's temp folder.
        let elsewhere = tempfile::tempdir().unwrap();
        let there = with_temp_root(elsewhere.path(), Scratch::new).unwrap();
        assert_eq!(there.root().parent(), Some(elsewhere.path()));
        assert!(with_temp_root(&elsewhere.path().join("missing"), Scratch::new).is_err());
        assert!(TEMP_ROOT.with_borrow(Option::is_none));
    }

    /// How many levels a tree needs to be deeper than this process may hold
    /// files open, up to what a test should build.
    #[cfg(not(windows))]
    #[allow(unsafe_code)]
    fn past_the_open_files() -> usize {
        let mut limit = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
        // SAFETY: getrlimit only writes the struct it is handed.
        assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &raw mut limit) }, 0);
        usize::try_from(limit.rlim_cur).unwrap_or(usize::MAX).saturating_add(100).min(20_000)
    }

    /// A plugin that nests folders past the limit on open files doesn't
    /// outlive its run. Windows has no such limit, and refuses to move a
    /// folder while a scanner reads a file in it.
    #[cfg(not(windows))]
    #[test]
    fn a_tree_of_any_depth_goes() {
        let temp = tempfile::tempdir().unwrap();
        let mut s = Scratch::new_in(temp.path()).unwrap();
        // Built from the inside out: a path this long has no name.
        let (tree, wrap) = (s.path().join("tree"), s.path().join("wrap"));
        fs::create_dir(&tree).unwrap();
        fs::write(tree.join("file"), "deep").unwrap();
        for _ in 0..past_the_open_files() {
            fs::create_dir(&wrap).unwrap();
            fs::rename(&tree, wrap.join("d")).unwrap();
            fs::write(wrap.join("file"), "on the way").unwrap();
            fs::rename(&wrap, &tree).unwrap();
        }
        let root = s.root().to_owned();
        let marker = s.marker.take();
        assert!(remove_retrying(&root, marker, &[], |_| {}));
        assert!(fs::symlink_metadata(&root).is_err(), "{} is still there", root.display());
    }

    /// The names a tree is lifted to may be taken, by what the plugin made:
    /// nothing of it is overwritten or left.
    #[test]
    fn a_wide_tree_with_names_of_its_own_goes() {
        let temp = tempfile::tempdir().unwrap();
        let mut s = Scratch::new_in(temp.path()).unwrap();
        for name in ["lifted-1", "lifted-2", "a/lifted-1/x", "a/b/c/d", "a/e/f", "g/h"] {
            fs::create_dir_all(s.path().join(name)).unwrap();
            fs::write(s.path().join(name).join("file"), name).unwrap();
        }
        let root = s.root().to_owned();
        let marker = s.marker.take();
        assert!(remove_retrying(&root, marker, &[], |_| {}));
        assert!(fs::symlink_metadata(&root).is_err(), "{} is still there", root.display());
    }

    #[test]
    fn a_scratch_folder_on_the_disk_is_made_where_it_is_asked_for() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("data").join("plugin-scratch");
        let s = Scratch::on_disk(&root).unwrap();
        assert_eq!(s.root().parent(), Some(root.as_path()));
        assert!(is_scratch_name(s.root().file_name().unwrap()));
        // A file where the folder should be: no scratch folder, and no other.
        let file = temp.path().join("file");
        fs::write(&file, "").unwrap();
        assert!(Scratch::on_disk(&file).is_err());
        // A thread told where to make them makes them there.
        let elsewhere = tempfile::tempdir().unwrap();
        let there = with_temp_root(elsewhere.path(), || Scratch::on_disk(&root)).unwrap();
        assert_eq!(there.root().parent(), Some(elsewhere.path()));
    }

    #[test]
    fn a_new_folder_never_reuses_a_name() {
        let temp = tempfile::tempdir().unwrap();
        let victim = temp.path().join("victim");
        fs::create_dir(&victim).unwrap();
        // A folder, a file and a link have the first three names.
        fs::create_dir(temp.path().join("as-aaaaaaaa")).unwrap();
        fs::write(temp.path().join("as-bbbbbbbb"), "a file").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&victim, temp.path().join("as-cccccccc")).unwrap();
        #[cfg(windows)]
        junction(&temp.path().join("as-cccccccc"), &victim);
        let names = ["aaaaaaaa", "bbbbbbbb", "cccccccc", "dddddddd"].map(String::from);
        let s = Scratch::named(temp.path(), names).unwrap();
        assert_eq!(s.root(), temp.path().join("as-dddddddd"));
        // The others are as they were, and nothing went through the link.
        assert_eq!(fs::read_dir(temp.path().join("as-aaaaaaaa")).unwrap().count(), 0);
        assert_eq!(fs::read_to_string(temp.path().join("as-bbbbbbbb")).unwrap(), "a file");
        assert_eq!(fs::read_dir(&victim).unwrap().count(), 0);
        // When every name it tries is taken, it says so and makes nothing.
        let taken = ["aaaaaaaa", "bbbbbbbb"].map(String::from);
        let e = Scratch::named(temp.path(), taken).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);
        // At most eight names are tried.
        let tried = Cell::new(0);
        let names = std::iter::repeat_with(|| {
            tried.set(tried.get() + 1);
            "aaaaaaaa".to_owned()
        });
        assert!(Scratch::named(temp.path(), names).is_err());
        assert_eq!(tried.get(), TRIES);
    }

    #[test]
    fn the_sweep_skips_a_locked_folder_and_one_without_a_marker() {
        let temp = tempfile::tempdir().unwrap();
        let t = temp.path();
        // A run that lives.
        let live = Scratch::new_in(t).unwrap();
        fs::write(live.path().join("frame.png"), "live").unwrap();
        // Runs that ended and left their folders: one emptied but for its
        // marker, one with what it wrote, also in a folder and read only.
        let left = |name: &str| {
            let dir = t.join(name);
            let frames = dir.join(TEMP).join("frames");
            fs::create_dir_all(&frames).unwrap();
            fs::write(dir.join(MARKER), "").unwrap();
            fs::write(frames.join("0001.png"), "left").unwrap();
            dir
        };
        let crashed = left("as-crashed1");
        let also = left("as-0123abcd");
        let frame = also.join(TEMP).join("frames").join("0001.png");
        let mut perms = fs::metadata(&frame).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&frame, perms).unwrap();
        // Not ours: no marker, another name, a file, a marker that is a folder.
        let unmarked = t.join("as-nomarker");
        fs::create_dir(&unmarked).unwrap();
        fs::write(unmarked.join("x"), "").unwrap();
        let named = [t.join("as-short"), t.join("as-UPPERCAS"), t.join("as-toolong12"), t.join("xs-abcdefgh")];
        for dir in &named {
            fs::create_dir(dir).unwrap();
            fs::write(dir.join(MARKER), "").unwrap();
        }
        fs::write(t.join("as-afile123"), "").unwrap();
        let odd = t.join("as-oddmark1");
        fs::create_dir_all(odd.join(MARKER)).unwrap();
        // A link named as a scratch folder, to a folder with a marker.
        let victim = t.join("victim");
        fs::create_dir(&victim).unwrap();
        fs::write(victim.join(MARKER), "").unwrap();
        fs::write(victim.join("keep.txt"), "keep").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&victim, t.join("as-linked1")).unwrap();
        #[cfg(windows)]
        junction(&t.join("as-linked1"), &victim);

        assert_eq!(sweep(t), 2);
        assert!(!crashed.exists() && !also.exists());
        assert!(live.path().join("frame.png").exists() && held(&live.root().join(MARKER)));
        assert!(unmarked.join("x").exists() && odd.join(MARKER).is_dir());
        assert!(named.iter().all(|d| d.join(MARKER).exists()) && t.join("as-afile123").is_file());
        assert!(fs::symlink_metadata(t.join("as-linked1")).is_ok());
        assert_eq!(fs::read_to_string(victim.join("keep.txt")).unwrap(), "keep");
        assert!(victim.join(MARKER).exists());
        // Once its run ended, the live one goes too: by itself, or by a sweep.
        let root = live.root().to_owned();
        drop(live);
        sweep(t);
        wait_gone(&root);
        // A temp folder that isn't there is nothing to sweep.
        assert_eq!(sweep(&t.join("gone")), 0);
    }

    #[test]
    fn a_folder_in_place_of_the_marker_goes_with_the_scratch_folder() {
        let temp = tempfile::tempdir().unwrap();
        let s = Scratch::new_in(temp.path()).unwrap();
        // A process of the person's took the marker's place with a folder,
        // and filled it (the run's open marker lets it go).
        let marker = s.root().join(MARKER);
        fs::remove_file(&marker).unwrap();
        fs::create_dir_all(marker.join("deep")).unwrap();
        fs::write(marker.join("deep").join("big.bin"), "big").unwrap();
        fs::write(s.path().join("frame.png"), "frame").unwrap();
        // AutoScene made the folder: all of it goes when the run ends.
        let root = s.root().to_owned();
        drop(s);
        wait_gone(&root);
        // So does a folder left so: AutoScene's removal takes it whole.
        let left = temp.path().join("as-oddmark2");
        fs::create_dir_all(left.join(MARKER).join("deep")).unwrap();
        fs::write(left.join(MARKER).join("deep").join("big.bin"), "big").unwrap();
        fs::create_dir(left.join(TEMP)).unwrap();
        assert!(remove_retrying(&left, None, &[], |_| {}));
        assert!(fs::symlink_metadata(&left).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn a_failed_removal_keeps_the_marker_for_the_sweep() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("as-failing1");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join(MARKER), "run").unwrap();
        fs::write(dir.join("held.bin"), "held").unwrap();
        fs::write(dir.join("other.txt"), "other").unwrap();
        let hold = Hold::new(&dir);
        let waited = Cell::new(Duration::ZERO);
        let waits = [Duration::from_millis(1), Duration::from_millis(2)];
        assert!(!remove_retrying(&dir, None, &waits, |w| waited.set(waited.get() + w)));
        // Each wait came. What could go went, and the marker, which goes
        // last, is the run's still, for the sweep to take.
        assert_eq!(waited.get(), Duration::from_millis(3));
        assert!(!dir.join("other.txt").exists());
        assert_eq!(fs::read_to_string(dir.join(MARKER)).unwrap(), "run");
        assert!(unheld(&dir));
        drop(hold);
        assert_eq!(sweep(temp.path()), 1);
        assert!(!dir.exists());
    }

    #[cfg(windows)]
    #[test]
    fn a_hold_let_go_while_retrying_lets_the_folder_go() {
        let temp = tempfile::tempdir().unwrap();
        let s = Scratch::new_in(temp.path()).unwrap();
        fs::write(s.path().join("held.bin"), "held").unwrap();
        let hold = RefCell::new(Some(Hold::new(s.path())));
        // The run's marker is held while it tries, and goes with the folder.
        let mut s = s;
        let (root, marker) = (s.root().to_owned(), s.marker.take());
        let tries = Cell::new(0);
        let waits = [Duration::from_millis(1), Duration::from_millis(1)];
        let removed = remove_retrying(&root, marker, &waits, |_| {
            assert!(held(&root.join(MARKER)));
            tries.set(tries.get() + 1);
            hold.borrow_mut().take();
        });
        assert!(removed);
        assert_eq!(tries.get(), 1);
        assert!(fs::symlink_metadata(&root).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_left_read_only_is_removed_too() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let mut s = Scratch::new_in(temp.path()).unwrap();
        // What `tarfile.extractall` or a copy of a read-only tree leaves:
        // folders that can't be changed, one in another, one that can't even
        // be read, and the run's temp folder itself made read only.
        let deep = s.path().join("pkg").join("data");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("x.bin"), "x").unwrap();
        let closed = s.path().join("closed");
        fs::create_dir(&closed).unwrap();
        fs::write(closed.join("y.bin"), "y").unwrap();
        let mode = |dir: &Path, mode| fs::set_permissions(dir, fs::Permissions::from_mode(mode)).unwrap();
        mode(&deep, 0o500);
        mode(&s.path().join("pkg"), 0o500);
        mode(&closed, 0o000);
        mode(s.path(), 0o500);
        let (root, marker) = (s.root().to_owned(), s.marker.take());
        assert!(remove_retrying(&root, marker, &[], |_| {}));
        assert!(fs::symlink_metadata(&root).is_err());
    }

    #[test]
    fn a_link_in_place_of_the_folder_is_removed_alone() {
        let temp = tempfile::tempdir().unwrap();
        let victim = temp.path().join("victim");
        fs::create_dir(&victim).unwrap();
        fs::write(victim.join(MARKER), "").unwrap();
        fs::write(victim.join("keep.txt"), "keep").unwrap();
        let link = temp.path().join("as-swapped1");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&victim, &link).unwrap();
        #[cfg(windows)]
        junction(&link, &victim);
        assert!(remove_retrying(&link, None, &[], |_| {}));
        assert!(fs::symlink_metadata(&link).is_err());
        assert_eq!(fs::read_to_string(victim.join("keep.txt")).unwrap(), "keep");
        assert!(victim.join(MARKER).is_file());
        // A file in its place goes alone too, and nothing at all is gone already.
        let file = temp.path().join("as-swapped2");
        fs::write(&file, "x").unwrap();
        assert!(remove_retrying(&file, None, &[], |_| {}));
        assert!(!file.exists());
        assert!(remove_retrying(&file, None, &[], |_| {}));
    }

    /// What keeps a folder from being removed on Windows: its `held.bin`,
    /// opened without leave to delete it, until it drops. (On Linux and
    /// macOS nothing holds a folder of the person's: an open file doesn't,
    /// and a folder without leave to change it is given that leave.)
    #[cfg(windows)]
    struct Hold {
        _file: File,
    }

    #[cfg(windows)]
    impl Hold {
        fn new(folder: &Path) -> Self {
            use std::os::windows::fs::OpenOptionsExt;
            let file = OpenOptions::new().read(true).share_mode(0).open(folder.join("held.bin")).unwrap();
            Self { _file: file }
        }
    }

    fn wait_gone(path: &Path) {
        let until = std::time::Instant::now() + Duration::from_secs(20);
        while fs::symlink_metadata(path).is_ok() {
            assert!(std::time::Instant::now() < until, "{} is still there", path.display());
            thread::sleep(Duration::from_millis(20));
        }
    }

    /// A junction at `link` to `target` (a symlink would need developer mode).
    #[cfg(windows)]
    fn junction(link: &Path, target: &Path) {
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
    }
}

//! Plugin packages (`.asp`) opened from outside AutoScene: a double-click
//! on one, "Open with", or a start with the file on the command line. Each
//! opens the install review, never more: the page takes the paths
//! (`plugins_take_opened`) and checks each one again as it reads it
//! (`plugin_inspect`).
//!
//! They come three ways: the arguments of the start that opens the window,
//! those of a second start the single instance hands over (with its working
//! folder), and on macOS the files the system asks the running app to open
//! (`RunEvent::Opened`). Any process of the same user can start AutoScene
//! with arguments, and send them through the single-instance channel, so
//! they are untrusted: only plain paths of existing files that end in
//! `.asp` are taken, and the queue holds a few.
//!
//! The queue lives outside the app's state: the first start's paths, and
//! macOS's, arrive before the page listens (and the first before the state
//! exists). The page asks for them when it loads and on `plugins:opened`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use tauri::{AppHandle, Url};
use tauri_specta::Event;

use crate::events::PluginsOpened;
use crate::paths;

/// The most packages waiting for the page; the oldest go first.
pub const MAX_QUEUED: usize = 8;

static OPENED: Queue = Queue::new();

/// The packages among `args`: absolute (a relative one joined to `cwd`, and
/// dropped without one), plain ([`paths::is_plain`]), spelt in UTF-8 (the
/// page gets them as text), ending in `.asp` in any case, and a regular
/// file. Each once, in the order given.
pub fn packages(args: impl IntoIterator<Item = OsString>, cwd: Option<&Path>) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    for arg in args {
        let path = PathBuf::from(arg);
        let path = match cwd {
            _ if path.is_absolute() => path,
            Some(cwd) => cwd.join(path),
            None => continue,
        };
        if path.to_str().is_some()
            && paths::is_plain(&path)
            && path.extension().is_some_and(|e| e.eq_ignore_ascii_case("asp"))
            && path.is_file()
            && !found.contains(&path)
        {
            found.push(path);
        }
    }
    found
}

/// The packages among the `file:` URLs macOS asks the app to open.
pub fn from_urls(urls: &[Url]) -> Vec<PathBuf> {
    let files = urls.iter().filter(|u| u.scheme() == "file").filter_map(|u| u.to_file_path().ok());
    packages(files.map(PathBuf::into_os_string), None)
}

/// Queues `paths` for the page and tells it. Nothing to queue, nothing said.
pub fn offer(app: &AppHandle, paths: Vec<PathBuf>) {
    if paths.is_empty() {
        return;
    }
    log::info!("{} plugin package(s) opened from outside", paths.len());
    OPENED.push(paths);
    if let Err(e) = (PluginsOpened {}).emit(app) {
        log::warn!("telling the page of opened plugin packages: {e}");
    }
}

/// The packages waiting, oldest first; the queue is empty after.
pub fn take() -> Vec<PathBuf> {
    OPENED.take()
}

struct Queue(Mutex<Vec<PathBuf>>);

impl Queue {
    const fn new() -> Self {
        Self(parking_lot::const_mutex(Vec::new()))
    }

    /// One already waiting moves to the end, as the newest; past
    /// [`MAX_QUEUED`] the oldest are dropped.
    fn push(&self, paths: Vec<PathBuf>) {
        let mut queue = self.0.lock();
        for path in paths {
            queue.retain(|p| *p != path);
            queue.push(path);
        }
        let over = queue.len().saturating_sub(MAX_QUEUED);
        queue.drain(..over);
    }

    fn take(&self) -> Vec<PathBuf> {
        std::mem::take(&mut *self.0.lock())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of its own for one test, removed after it.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(test: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("autoscene-opened-{test}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn args(list: &[&Path]) -> Vec<OsString> {
        list.iter().map(|p| p.as_os_str().to_owned()).collect()
    }

    #[test]
    fn only_plain_asp_files_are_taken() {
        let dir = Scratch::new("plain");
        let root = dir.path();
        for name in ["a.asp", "B.ASP", "c.zip", "d.asp.txt"] {
            std::fs::write(root.join(name), b"PK").unwrap();
        }
        std::fs::create_dir(root.join("folder.asp")).unwrap();
        // x is there, so x/../a.asp names a.asp on every system: only `..` drops it.
        std::fs::create_dir(root.join("x")).unwrap();
        let a = root.join("a.asp");
        let upper = root.join("B.ASP");
        let climbing = root.join("x").join("..").join("a.asp");
        assert!(climbing.is_file());
        let given = [
            a.as_path(),
            upper.as_path(),
            &root.join("c.zip"),
            &root.join("d.asp.txt"),
            // A folder, one that isn't there, one climbing out with `..`.
            &root.join("folder.asp"),
            &root.join("gone.asp"),
            &climbing,
            // Flags and words, as a start may also get.
            Path::new("--flag"),
            Path::new("a.asp"),
            // Twice.
            a.as_path(),
        ];
        assert_eq!(packages(args(&given), None), vec![a, upper]);
    }

    #[cfg(windows)]
    #[test]
    fn device_and_verbatim_spellings_are_refused() {
        let dir = Scratch::new("device");
        let file = dir.path().join("p.asp");
        std::fs::write(&file, b"PK").unwrap();
        let verbatim = PathBuf::from(format!(r"\\?\{}", file.display()));
        let device = PathBuf::from(format!(r"\\.\{}", file.display()));
        // Real files, both: the spelling alone is refused.
        assert!(verbatim.is_file());
        assert!(packages(args(&[&verbatim, &device]), None).is_empty());
        assert_eq!(packages(args(&[&file]), None), vec![file]);
    }

    #[test]
    fn a_relative_path_joins_the_second_launchs_folder() {
        let dir = Scratch::new("relative");
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub").join("p.asp"), b"PK").unwrap();
        let relative = Path::new("sub").join("p.asp");
        assert_eq!(packages(args(&[&relative]), Some(dir.path())), vec![dir.path().join("sub").join("p.asp")]);
        // Without a folder to join it to, it is dropped.
        assert!(packages(args(&[&relative]), None).is_empty());
        // `..` isn't plain, even where it lands on the package.
        let up = Path::new("..").join("sub").join("p.asp");
        let cwd = dir.path().join("sub");
        assert!(cwd.join(&up).is_file());
        assert!(packages(args(&[&up]), Some(&cwd)).is_empty());
    }

    #[test]
    fn the_queue_keeps_a_few_and_drops_repeats() {
        let queue = Queue::new();
        let p = |n: usize| PathBuf::from(format!("/p/{n}.asp"));
        queue.push((0..5).map(p).collect());
        // 2 again: it moves to the end, once.
        queue.push(vec![p(2), p(5)]);
        assert_eq!(queue.take(), [0, 1, 3, 4, 2, 5].map(p));
        assert!(queue.take().is_empty());

        queue.push((0..MAX_QUEUED + 3).map(p).collect());
        assert_eq!(queue.take(), (3..MAX_QUEUED + 3).map(p).collect::<Vec<_>>());
    }

    #[test]
    fn file_urls_become_paths() {
        let dir = Scratch::new("urls");
        let file = dir.path().join("my plugin.asp");
        std::fs::write(&file, b"PK").unwrap();
        let url = Url::from_file_path(&file).unwrap();
        // Spaces are escaped in the URL, not in the path.
        assert!(url.as_str().contains("my%20plugin.asp"), "{url}");
        let urls = [url, Url::parse("https://example.com/p.asp").unwrap(), Url::parse("autoscene://p.asp").unwrap()];
        assert_eq!(from_urls(&urls), vec![file]);
    }
}

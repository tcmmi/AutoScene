//! Well-known locations shared by the app, the engine and the CLI.
//!
//! | what | Windows | macOS |
//! |---|---|---|
//! | app data (models, runtimes, library.db, logs) | `%LOCALAPPDATA%\AutoScene` | `~/Library/Application Support/AutoScene` |
//! | default library (project folders) | `Documents\AutoScene Library` | `~/Documents/AutoScene Library` |
//! | NVIDIA's engine, once downloaded | app data `runtimes\nvidia-rtx-<version>` | not offered |
//! | plugins, what they keep, their logs | app data `plugins`, `plugin-data`, `logs\plugins` | app data `plugins`, `plugin-data`, `logs/plugins` |
//! | plugin packages from the catalog, while they download | app data `cache\plugin-downloads` | app data `cache/plugin-downloads` |
//! | WebAssembly plugins' compiled code | app data `cache\plugin-code` | app data `cache/plugin-code` |
//! | the large files plugins name, a folder per plugin id | app data `cache\plugin-resources` | app data `cache/plugin-resources` |
//! | updater override | `~/.autoscene/updates.json` | same |
//!
//! `AUTOSCENE_DATA_DIR` overrides the app data root (tests, portable installs).

use std::borrow::Cow;
use std::path::{Path, PathBuf};

const APP_DIR: &str = "AutoScene";
/// How the folder of a release of NVIDIA's engine starts ([`AppPaths::accel_dir`]).
pub const ACCEL_PREFIX: &str = "nvidia-rtx-";

#[derive(Clone, Debug)]
pub struct AppPaths {
    pub data: PathBuf,
}

impl AppPaths {
    pub fn resolve() -> Self {
        let data = std::env::var_os("AUTOSCENE_DATA_DIR").map(PathBuf::from).unwrap_or_else(|| {
            let base = if cfg!(target_os = "macos") { dirs::data_dir() } else { dirs::data_local_dir() };
            base.unwrap_or_else(|| PathBuf::from(".")).join(APP_DIR)
        });
        Self { data }
    }

    pub fn at(data: impl Into<PathBuf>) -> Self {
        Self { data: data.into() }
    }

    pub fn library_db(&self) -> PathBuf {
        self.data.join("library.db")
    }
    pub fn models(&self) -> PathBuf {
        self.data.join("models")
    }
    pub fn runtimes(&self) -> PathBuf {
        self.data.join("runtimes")
    }
    /// NVIDIA's engine at its release `version`, fetched at the person's
    /// request (`as_infer::accel`). Each release has its own folder.
    pub fn accel_dir(&self, version: &str) -> PathBuf {
        self.runtimes().join(format!("{ACCEL_PREFIX}{version}"))
    }
    pub fn logs(&self) -> PathBuf {
        self.data.join("logs")
    }
    pub fn cache(&self) -> PathBuf {
        self.data.join("cache")
    }
    /// Character images from AniList, by AniList character id (only while
    /// names from AniList are on).
    pub fn anilist(&self) -> PathBuf {
        self.data.join("anilist")
    }
    /// Character images from TVmaze, by TVmaze character id (only while
    /// names from AniList and TVmaze are on).
    pub fn tvmaze(&self) -> PathBuf {
        self.data.join("tvmaze")
    }
    /// Character pictures from films' Fandom wikis (only while character
    /// names are on).
    pub fn fandom(&self) -> PathBuf {
        self.data.join("fandom")
    }
    /// Installed plugins, a folder per plugin id.
    pub fn plugins(&self) -> PathBuf {
        self.data.join("plugins")
    }
    /// What plugins keep between runs, a folder per plugin id.
    pub fn plugin_data(&self) -> PathBuf {
        self.data.join("plugin-data")
    }
    /// The scratch folders of WebAssembly plugins' runs: on the disk, as the
    /// system's temp folder may be memory (a `tmpfs`), and nothing bounds
    /// what a sandboxed plugin writes there. Emptied as runs end, and at
    /// each start.
    pub fn plugin_scratch(&self) -> PathBuf {
        self.data.join("plugin-scratch")
    }
    /// A note for each unfinished file of a pack action's run, saying where
    /// it is: what a crash left in the person's folder goes at the next start.
    pub fn plugin_parts(&self) -> PathBuf {
        self.data.join("plugin-parts")
    }
    /// Plugins' logs, a file per plugin id.
    pub fn plugin_logs(&self) -> PathBuf {
        self.logs().join("plugins")
    }
    /// Plugin packages from the catalog while they download, until they are
    /// checked; emptied at each start. Outside [`AppPaths::plugins`], whose
    /// folders are taken for plugins.
    pub fn plugin_downloads(&self) -> PathBuf {
        self.cache().join("plugin-downloads")
    }
    /// WebAssembly plugins' compiled code, a folder per plugin id. Never
    /// under a folder a plugin sees, nor under [`AppPaths::plugins`], whose
    /// package folders are checked against their locks.
    pub fn plugin_code(&self) -> PathBuf {
        self.cache().join("plugin-code")
    }

    /// The large files plugins name in their `plugin.json` (`resources`),
    /// a folder per plugin id, each file checked by its hash. Kept while an
    /// installed version names them.
    pub fn plugin_resources(&self) -> PathBuf {
        self.cache().join("plugin-resources")
    }

    /// Where new projects go unless the user picks another folder.
    pub fn default_library_root() -> PathBuf {
        dirs::document_dir().or_else(dirs::home_dir).unwrap_or_else(|| PathBuf::from(".")).join("AutoScene Library")
    }

    /// Legacy `~/.autoscene` dir — kept only for the updater override file.
    pub fn legacy_dot_dir() -> PathBuf {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".autoscene")
    }
}

/// Just the file (or folder) name, for the log: full paths name the user
/// (their home folder) and their folders, and logs get shared.
pub fn name(path: &Path) -> Cow<'_, str> {
    path.file_name().map_or(Cow::Borrowed("?"), |n| n.to_string_lossy())
}

/// The bytes the current user may still write on the drive of `dir`.
#[cfg(windows)]
pub fn free_space(dir: &Path) -> std::io::Result<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain([0]).collect();
    let mut free = 0u64;
    // SAFETY: a NUL-terminated wide path and a valid out pointer; the totals
    // it isn't asked for may be null.
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) };
    if ok == 0 { Err(std::io::Error::last_os_error()) } else { Ok(free) }
}

/// The bytes an unprivileged user may still write on the drive of `dir`.
#[cfg(unix)]
#[allow(clippy::useless_conversion)] // the fields are 32 bits wide on some systems, 64 on others
pub fn free_space(dir: &Path) -> std::io::Result<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(dir.as_os_str().as_bytes())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let mut st = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
    // SAFETY: a NUL-terminated path and a buffer statvfs fills on success.
    if unsafe { libc::statvfs(path.as_ptr(), st.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: statvfs succeeded, so it filled the buffer.
    let st = unsafe { st.assume_init() };
    Ok(u64::from(st.f_bavail).saturating_mul(u64::from(st.f_frsize)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_gets_only_a_files_name() {
        assert_eq!(name(Path::new("/home/someone/Videos/Show - 01.mkv")), "Show - 01.mkv");
        assert_eq!(name(&Path::new("/data").join("models")), "models");
        assert_eq!(name(Path::new("/")), "?");
        #[cfg(windows)]
        assert_eq!(
            name(Path::new(r"C:\Users\someone\AppData\Local\AutoScene\runtimes\onnxruntime.dll")),
            "onnxruntime.dll"
        );
    }
}

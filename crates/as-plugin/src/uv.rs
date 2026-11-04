//! AutoScene's own uv, which runs uv-script plugins ([`crate::runtime`]).
//! A uv on the person's PATH is never used: a plugin runs with the uv
//! AutoScene pinned, whatever else is installed.
//!
//! `uv.toml` (written by `cargo xtask pin-uv`, never by hand) pins one uv
//! release for each platform AutoScene runs on: the release asset's
//! address, size and SHA-256, which the xtask checked against GitHub's
//! digest and the asset's build attestation, and the size and SHA-256 of
//! uv in it. uv comes the first time a plugin needs it, as AutoScene's
//! models do ([`as_models`]): from a zip, `uv.exe` alone, by its byte
//! ranges; from a tar.gz, the whole archive, whose hash is checked before
//! `uv` is taken out of it and marked runnable. It is kept in
//! `runtimes/uv-<version>/`, and counts while it is the pinned file
//! ([`installed`]). What uv fetches for the scripts it runs stays beside
//! it, in `runtimes/uv-cache/` and `runtimes/uv-python/` (the runtime sets
//! them). A uv of another version is removed at the next start
//! ([`prune`]); what it fetched stays.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, PoisonError};

use as_core::CancelToken;
use as_models::{ModelStore, Packed, Pinned};
use serde::Deserialize;

use crate::error::{PluginError, PluginResult};
use crate::fetch::fetch_error;
use crate::manifest::current_platform;

/// The folder beside uv's where it keeps the packages it fetched.
pub const CACHE: &str = "uv-cache";
/// The folder beside uv's where it keeps the Pythons it installed.
pub const PYTHONS: &str = "uv-python";

/// uv for one platform, as `uv.toml` pins it.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UvPin {
    /// A key of [`crate::manifest::PLATFORMS`].
    pub platform: String,
    /// The release asset: a zip (Windows) or a tar.gz.
    pub url: String,
    pub archive_size: u64,
    pub archive_sha256: String,
    /// uv's path in the archive.
    pub member: String,
    /// Of uv itself.
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pins {
    version: String,
    pin: Vec<UvPin>,
}

static PINS: LazyLock<Pins> =
    LazyLock::new(|| toml::from_str(include_str!("../uv.toml")).expect("crates/as-plugin/uv.toml is valid"));

/// The stores of uv's folders: one per folder for the process, so uv is
/// hashed the first time it is asked for, and again only once its file
/// changed.
static STORES: LazyLock<Mutex<HashMap<PathBuf, ModelStore>>> = LazyLock::new(Mutex::default);

/// The version of uv AutoScene runs plugins with.
pub fn version() -> &'static str {
    &PINS.version
}

/// uv for this computer, unless AutoScene runs on a platform it has none for.
pub fn pin() -> Option<&'static UvPin> {
    PINS.pin.iter().find(|p| p.platform == current_platform())
}

/// The folder of this version of uv: `<runtimes>/uv-<version>`.
pub fn dir(runtimes: &Path) -> PathBuf {
    runtimes.join(format!("uv-{}", version()))
}

/// uv, while it is here and still the pinned file. A file that no longer
/// is loses its mark, and is got again.
pub fn installed(runtimes: &Path) -> Option<PathBuf> {
    installed_as(&dir(runtimes), pin()?)
}

/// Gets uv, unless it is here (then at once): `progress(done, total)` as its
/// bytes come; `cancel` ends it. Waits while another install of it runs,
/// in this process or another. What fails says so in words about uv.
pub fn install(runtimes: &Path, cancel: &CancelToken, progress: impl FnMut(u64, u64)) -> PluginResult<PathBuf> {
    let pin = pin().ok_or(PluginError::UvMissing)?;
    install_as(&dir(runtimes), pin, cancel, progress)
}

/// What getting uv downloads at most: its archive, whole for a tar.gz, of
/// which a zip gives only uv.
pub fn download_bytes() -> u64 {
    pin().map_or(0, |p| p.archive_size)
}

/// Removes the folders of other versions of uv from `runtimes`, as much as
/// it can: a uv still running (Windows keeps its file) stays until the next
/// start. What uv fetched ([`CACHE`], [`PYTHONS`]) stays.
pub fn prune(runtimes: &Path) {
    prune_but(runtimes, version());
}

fn prune_but(runtimes: &Path, keep: &str) {
    let Ok(entries) = fs::read_dir(runtimes) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(version) = name.to_str().and_then(|n| n.strip_prefix("uv-")) else { continue };
        if version == keep || !is_version(version) || !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        match fs::remove_dir_all(entry.path()) {
            Ok(()) => tracing::info!(version, "removed a uv AutoScene no longer uses"),
            Err(e) => tracing::info!(version, error = %e, "a uv AutoScene no longer uses stays until the next start"),
        }
    }
}

/// Whether `v` reads as a version of uv (`0.13.7`): the folders uv keeps
/// what it fetched in are named otherwise.
fn is_version(v: &str) -> bool {
    !v.is_empty() && v.split('.').all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// The store of uv's folder `dir`, which remembers what it hashed.
fn store(dir: &Path) -> ModelStore {
    let mut stores = STORES.lock().unwrap_or_else(PoisonError::into_inner);
    stores.entry(dir.to_path_buf()).or_insert_with(|| ModelStore::new(dir)).clone()
}

fn installed_as(dir: &Path, pin: &UvPin) -> Option<PathBuf> {
    match store(dir).installed_pinned(&pinned(pin)) {
        Ok(found) => found,
        // Getting it again says what is wrong.
        Err(e) => {
            tracing::warn!(error = %e.detail(), "AutoScene's uv couldn't be read");
            None
        }
    }
}

fn install_as(dir: &Path, pin: &UvPin, cancel: &CancelToken, progress: impl FnMut(u64, u64)) -> PluginResult<PathBuf> {
    store(dir).fetch_pinned(&pinned(pin), cancel, progress).map_err(|e| fetch_error("uv", e))
}

/// `pin` as the file the store fetches: uv by the name it has in its archive
/// (`uv.exe`, `uv`), runnable. From a zip, it comes alone by its byte
/// ranges; from a tar.gz, the archive's own size and hash are checked
/// first.
fn pinned(pin: &UvPin) -> Pinned {
    let file = pin.member.rsplit('/').next().unwrap_or(&pin.member).to_owned();
    let packed = match pin.url.ends_with(".zip") {
        true => Packed::ZipMember(pin.member.clone()),
        false => Packed::TarGzMember {
            member: pin.member.clone(),
            archive_size: pin.archive_size,
            archive_sha256: pin.archive_sha256.clone(),
        },
    };
    Pinned {
        id: "uv".into(),
        file,
        urls: vec![pin.url.clone()],
        sha256: pin.sha256.clone(),
        size: pin.size,
        packed,
        exec: true,
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, Write};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, SystemTime};

    use sha2::{Digest, Sha256};

    use super::*;
    use crate::manifest::PLATFORMS;

    fn sha(bytes: &[u8]) -> String {
        Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
    }

    fn hex64(s: &str) -> bool {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }

    #[test]
    fn every_platform_has_a_pin_of_the_one_version() {
        let v = version();
        assert!(is_version(v), "{v}");
        assert_eq!(PINS.pin.len(), PLATFORMS.len());
        // Each platform's own release asset, as cargo-dist names them.
        let assets = [
            ("windows-x86_64", "uv-x86_64-pc-windows-msvc.zip", "uv.exe"),
            ("windows-aarch64", "uv-aarch64-pc-windows-msvc.zip", "uv.exe"),
            ("macos-aarch64", "uv-aarch64-apple-darwin.tar.gz", "uv-aarch64-apple-darwin/uv"),
            ("macos-x86_64", "uv-x86_64-apple-darwin.tar.gz", "uv-x86_64-apple-darwin/uv"),
            ("linux-x86_64", "uv-x86_64-unknown-linux-gnu.tar.gz", "uv-x86_64-unknown-linux-gnu/uv"),
            ("linux-aarch64", "uv-aarch64-unknown-linux-gnu.tar.gz", "uv-aarch64-unknown-linux-gnu/uv"),
        ];
        assert_eq!(assets.map(|a| a.0), PLATFORMS);
        for (platform, asset, member) in assets {
            let pins: Vec<_> = PINS.pin.iter().filter(|p| p.platform == platform).collect();
            let [p] = pins[..] else { panic!("{platform}: {} pins", pins.len()) };
            assert_eq!(p.url, format!("https://github.com/astral-sh/uv/releases/download/{v}/{asset}"));
            assert_eq!(p.member, member);
            assert!(hex64(&p.archive_sha256) && hex64(&p.sha256), "{platform}");
            assert!(p.archive_size > 1_000_000 && p.size > 1_000_000, "{platform}");
            let got = pinned(p);
            assert!(got.exec && got.urls == [p.url.clone()] && got.size == p.size && got.sha256 == p.sha256);
            match platform.starts_with("windows") {
                // uv.exe alone, by its byte ranges.
                true => assert!(got.file == "uv.exe" && matches!(&got.packed, Packed::ZipMember(m) if m == member)),
                // The whole archive, checked before uv comes out of it.
                false => assert!(
                    got.file == "uv"
                        && matches!(&got.packed, Packed::TarGzMember { member: m, archive_size, archive_sha256 }
                            if m == member && *archive_size == p.archive_size && *archive_sha256 == p.archive_sha256)
                ),
            }
        }
        let here = pin().expect("AutoScene's platforms each have uv");
        assert_eq!(here.platform, current_platform());
        assert_eq!(download_bytes(), here.archive_size);
        assert_eq!(dir(Path::new("runtimes")), Path::new("runtimes").join(format!("uv-{v}")));
    }

    /// A uv of `bytes` in a tar.gz served at `url`, as `uv.toml` would pin it.
    fn test_pin(url: &str, archive: &[u8], bytes: &[u8]) -> UvPin {
        UvPin {
            platform: current_platform().into(),
            url: url.into(),
            archive_size: archive.len() as u64,
            archive_sha256: sha(archive),
            member: "uv-test/uv".into(),
            size: bytes.len() as u64,
            sha256: sha(bytes),
        }
    }

    #[test]
    fn a_managed_uv_counts_only_while_it_is_the_pinned_file() {
        let tmp = tempfile::tempdir().unwrap();
        // None is here yet, and an empty folder holds none.
        assert_eq!(installed(tmp.path()), None);
        let uv = b"#!/bin/sh\necho uv\n".repeat(64);
        let pin = test_pin("http://127.0.0.1:9/never.tar.gz", b"never", &uv);
        let dir = tmp.path().join("uv-0.13.7");
        assert_eq!(installed_as(&dir, &pin), None);
        // A file put there that AutoScene never checked isn't uv.
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("uv"), &uv).unwrap();
        assert_eq!(installed_as(&dir, &pin), None);
        // Checked, it is.
        fs::write(dir.join("uv.verified"), &pin.sha256).unwrap();
        assert_eq!(installed_as(&dir, &pin), Some(dir.join("uv")));
        // Changed since, it no longer is, and loses its mark.
        let mut other = uv.clone();
        other[0] = b'?';
        fs::write(dir.join("uv"), &other).unwrap();
        let later = SystemTime::now() + Duration::from_secs(5);
        fs::File::options().write(true).open(dir.join("uv")).unwrap().set_modified(later).unwrap();
        assert_eq!(installed_as(&dir, &pin), None);
        assert!(!dir.join("uv.verified").exists());
        // Nor does a file of another size.
        fs::write(dir.join("uv"), &uv[1..]).unwrap();
        fs::write(dir.join("uv.verified"), &pin.sha256).unwrap();
        assert_eq!(installed_as(&dir, &pin), None);
    }

    #[test]
    fn other_uv_versions_are_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let runtimes = tmp.path();
        let put = |path: &str| {
            let path = runtimes.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"x").unwrap();
        };
        let kept = [
            format!("uv-{}/uv", version()),
            format!("{CACHE}/wheels/x"),
            format!("{PYTHONS}/cpython-3.13/python"),
            "onnxruntime.dll".into(),
            // Not a folder of uv: a file, and a folder not named by a version.
            "uv-0.1.0".into(),
            "uv-next/uv".into(),
        ];
        for path in &kept {
            put(path);
        }
        put("uv-0.12.3/uv");
        put("uv-0.12.3/uv.verified");
        put("uv-0.13.10/uv");
        prune(runtimes);
        assert!(!runtimes.join("uv-0.12.3").exists() && !runtimes.join("uv-0.13.10").exists());
        for path in &kept {
            assert!(runtimes.join(path).is_file(), "{path}");
        }
        // No folder of runtimes, nothing to do.
        prune(&runtimes.join("missing"));
    }

    /// A gzipped tar of `entries`: (path, bytes).
    fn tar_gz(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut builder = tar::Builder::new(gz);
        for (path, bytes) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Regular);
            header.set_mode(0o644);
            header.set_size(bytes.len() as u64);
            builder.append_data(&mut header, path, *bytes).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    /// Serves `data` whole on 127.0.0.1 to each request, as a release's
    /// address does: its address, and how many requests it answered.
    fn serve(data: Vec<u8>) -> (String, Arc<AtomicUsize>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/uv-test.tar.gz", listener.local_addr().unwrap());
        let answered = Arc::new(AtomicUsize::new(0));
        let count = answered.clone();
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { return };
                let mut reader = std::io::BufReader::new(conn.try_clone().unwrap());
                // The request, up to its blank line.
                let mut line = String::new();
                while reader.read_line(&mut line).unwrap_or(0) > 0 && !line.trim().is_empty() {
                    line.clear();
                }
                count.fetch_add(1, Ordering::SeqCst);
                let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", data.len());
                let _ = conn.write_all(head.as_bytes()).and_then(|()| conn.write_all(&data));
            }
        });
        (url, answered)
    }

    #[test]
    fn a_uv_pin_downloads_like_a_model() {
        let uv = b"uv, the binary\n".repeat(20_000);
        let archive = tar_gz(&[("uv-test/uvx", b"not uv"), ("uv-test/uv", &uv)]);
        let (url, answered) = serve(archive.clone());
        let pin = test_pin(&url, &archive, &uv);
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("uv-0.13.7");
        let mut last = (0, 0);
        let path = install_as(&dir, &pin, &CancelToken::new(), |done, total| last = (done, total)).unwrap();
        assert_eq!(path, dir.join("uv"));
        assert_eq!(fs::read(&path).unwrap(), uv);
        assert_eq!(last, (archive.len() as u64, archive.len() as u64), "progress counts the archive's bytes");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o755);
        }
        // Only uv is kept, with its mark; and it counts, without a second download.
        let mut names: Vec<_> = fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        names.sort();
        assert_eq!(names, ["uv", "uv.lock", "uv.verified"]);
        assert_eq!(installed_as(&dir, &pin), Some(path.clone()));
        assert_eq!(install_as(&dir, &pin, &CancelToken::new(), |_, _| {}).unwrap(), path);
        assert_eq!(answered.load(Ordering::SeqCst), 1);

        // An archive that isn't the pinned one is never unpacked, and fails as uv's download.
        let other = tar_gz(&[("uv-test/uv", b"something else")]);
        let (url, _) = serve(other);
        let changed = UvPin { url, ..pin.clone() };
        let elsewhere = tmp.path().join("uv-0.13.8");
        let e = install_as(&elsewhere, &changed, &CancelToken::new(), |_, _| {}).unwrap_err();
        assert!(matches!(&e, PluginError::Download { what, .. } if what == "uv"), "{e:?}");
        assert_eq!(e.to_string(), "AutoScene couldn't get uv. Check the internet connection, then try again.");
        assert!(!elsewhere.join("uv").exists());
        // Cancelled before it starts, it fetches nothing.
        let cancel = CancelToken::new();
        cancel.cancel();
        let e = install_as(&tmp.path().join("uv-0.13.9"), &pin, &cancel, |_, _| {}).unwrap_err();
        assert!(matches!(e, PluginError::Cancelled), "{e:?}");
        assert_eq!(answered.load(Ordering::SeqCst), 1);
    }
}

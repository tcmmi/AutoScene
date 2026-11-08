//! AI models: the pinned manifest (`models/manifest.toml`) and a store in the
//! app data folder that downloads, resumes and verifies them.
//!
//! A model counts as installed when its file has the pinned size, a
//! `<file>.verified` marker records the pinned SHA256 (written once its
//! download was checked), and the file still hashes to it: a process hashes
//! a model the first time it asks for it, and again whenever the file
//! changed since. A file that no longer matches loses its marker and is
//! downloaded again. That repairs silent damage (a failing disk, a sync
//! tool, an antivirus restore); it is no lock against a local user who means
//! harm, who could as well replace the runtime libraries the app loads from
//! the same folder tree.
//!
//! Downloads go to `<file>.part` (a whole archive a file is taken out of:
//! `<file>.archive.part`) and resume with an HTTP range request after an
//! interruption; one that resumed and then fails its hash is fetched once
//! more from its first byte. A download ends at its pinned size: however the
//! server frames or compresses it, a byte more fails it and its part goes, as
//! a part does when its drive fills up. One install of a model runs at a time, across processes
//! (`<file>.lock`), and only when its drive has room for it. A model that is
//! one file of a zip archive (`member`) is fetched alone, by the byte ranges
//! the archive's directory gives (see [`zip`]), and unpacked.
//!
//! The store fetches any [`Pinned`] file this way, not only models: a
//! runtime, a plugin package. Such a file may also be one member of a
//! `.tar.gz` (see [`targz`]), come from one of several addresses, and be
//! marked runnable once verified.

pub mod net;
mod targz;
mod zip;

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use as_core::paths::free_space;
use as_core::{CancelToken, Msg, ToMsg, msg};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::net::{agent, get};

pub type ModelResult<T> = Result<T, ModelError>;

#[derive(Debug, Error)]
pub enum ModelError {
    Unknown(String),
    Download(String),
    Checksum {
        id: String,
        got: String,
    },
    /// The file at the pinned source isn't the pinned one any more.
    Changed(String),
    /// The drive of the models folder hasn't room for a download.
    NoSpace {
        dir: PathBuf,
        need: u64,
        free: u64,
    },
    Cancelled,
    Io(#[from] std::io::Error),
}

impl ToMsg for ModelError {
    fn msg(&self) -> Msg {
        match self {
            Self::Unknown(id) => msg!("model.unknown", "Unknown model {{id}}.", id = id),
            Self::Download(_) => msg!(
                "model.downloadFailed",
                "The AI model couldn't be downloaded. Check the internet connection and try again."
            ),
            Self::Checksum { .. } => {
                msg!("model.damaged", "The downloaded AI model is damaged. It will be downloaded again.")
            }
            Self::Changed(_) => msg!(
                "model.changed",
                "An AI model changed where AutoScene downloads it from. Update AutoScene to get it."
            ),
            Self::NoSpace { need, free, .. } => msg!(
                "model.noSpace",
                "There isn't enough free space for the AI model. Free up {{mb}} MB on the drive of AutoScene's data folder and try again.",
                mb = missing_mb(need, free)
            ),
            Self::Cancelled => msg!("model.cancelled", "Cancelled."),
            Self::Io(e) => e.msg(),
        }
    }
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.msg().text)
    }
}

impl ModelError {
    /// Technical description for logs.
    pub fn detail(&self) -> String {
        match self {
            Self::Download(why) => format!("model download failed: {why}"),
            Self::Checksum { id, got } => format!("{id}: sha256 {got} doesn't match the manifest"),
            Self::Changed(why) => format!("model source changed: {why}"),
            Self::NoSpace { dir, need, free } => {
                format!("{}: a model download needs {need} bytes free, the drive has {free}", dir.display())
            }
            Self::Io(e) => format!("model files: {e}"),
            e => e.to_string(),
        }
    }
}

/// Megabytes (rounded up) that `free` lacks of `need`.
fn missing_mb(need: &u64, free: &u64) -> u64 {
    need.saturating_sub(*free).div_ceil(1_000_000)
}

#[derive(Clone, Debug, Deserialize)]
pub struct Model {
    pub id: String,
    pub file: String,
    pub purpose: String,
    pub url: String,
    /// The model is this file of the zip archive at `url`; `sha256` and
    /// `size` are the file's, unpacked.
    #[serde(default)]
    pub member: Option<String>,
    pub sha256: String,
    pub size: u64,
    pub license: String,
    /// Its licence allows non-commercial use only (the app says so where it
    /// lists the models and where a setting decides which ones it fetches).
    #[serde(default)]
    pub non_commercial: bool,
    /// The model's card, shown in About where its terms can be read.
    #[serde(default)]
    pub card: Option<String>,
    /// What the licence means for using the model here, in plain words
    /// (shown in About when the licence name alone wouldn't say it).
    #[serde(default)]
    pub terms: Option<String>,
    pub source: String,
}

impl Model {
    /// The model as the pinned file the store fetches.
    pub fn pinned(&self) -> Pinned {
        Pinned {
            id: self.id.clone(),
            file: self.file.clone(),
            urls: vec![self.url.clone()],
            sha256: self.sha256.clone(),
            size: self.size,
            packed: self.member.clone().map_or(Packed::File, Packed::ZipMember),
            exec: false,
        }
    }
}

/// A file AutoScene downloads by its pinned hash: a model, a runtime, a
/// plugin package.
#[derive(Clone, Debug)]
pub struct Pinned {
    /// Names it in logs and errors.
    pub id: String,
    /// Its name in the store's folder.
    pub file: String,
    /// Tried in turn; a file that fails its hash is never kept.
    pub urls: Vec<String>,
    /// Of the file as kept.
    pub sha256: String,
    pub size: u64,
    pub packed: Packed,
    /// Unix: made runnable (0o755) once verified.
    pub exec: bool,
}

/// How the file travels.
#[derive(Clone, Debug)]
pub enum Packed {
    File,
    /// One member of a zip, fetched alone by its byte ranges.
    ZipMember(String),
    /// One regular-file member of a .tar.gz, taken out of the whole
    /// archive, whose own size and hash are pinned too.
    TarGzMember {
        member: String,
        archive_size: u64,
        archive_sha256: String,
    },
}

/// What the store's steps take: a pinned file, or a model as the one it is
/// (M1's tests, and `install_model`, hand them a model).
trait AsPinned {
    fn as_pinned(&self) -> Cow<'_, Pinned>;
}

impl AsPinned for Pinned {
    fn as_pinned(&self) -> Cow<'_, Pinned> {
        Cow::Borrowed(self)
    }
}

impl AsPinned for Model {
    fn as_pinned(&self) -> Cow<'_, Pinned> {
        Cow::Owned(self.pinned())
    }
}

#[derive(Debug, Deserialize)]
struct Manifest {
    /// Files of models a better one replaced.
    #[serde(default)]
    retired: Vec<String>,
    model: Vec<Model>,
}

static MANIFEST: LazyLock<Manifest> = LazyLock::new(|| {
    toml::from_str::<Manifest>(include_str!("../../../models/manifest.toml")).expect("models/manifest.toml is valid")
});

/// Every model AutoScene knows about.
pub fn manifest() -> &'static [Model] {
    &MANIFEST.model
}

/// Files of models AutoScene no longer uses (see [`ModelStore::prune`]).
pub fn retired() -> &'static [String] {
    &MANIFEST.retired
}

pub fn model(id: &str) -> ModelResult<&'static Model> {
    manifest().iter().find(|m| m.id == id).ok_or_else(|| ModelError::Unknown(id.into()))
}

/// Ids, so code can't misspell them.
pub mod ids {
    pub const TRANSNET: &str = "transnetv2";
    pub const SCRFD: &str = "scrfd-10g";
    /// The face embedder. Media whose faces another one found are found
    /// again (`Project::refresh_outdated_faces`).
    pub const ARCFACE: &str = "arcface-glintr100";
    /// What the face pass needs.
    pub const FACES: [&str; 2] = [SCRFD, ARCFACE];
    /// Fast Scan's detector and embedder (the Scener app's).
    pub const YUNET: &str = "yunet-2023mar";
    pub const SFACE: &str = "sface-2021dec";
    /// What the Fast Scan face pass needs.
    pub const FAST_FACES: [&str; 2] = [YUNET, SFACE];
    /// People (their whole bodies), and how they look (clothes, build).
    pub const PERSONS: &str = "person-ppyoloe";
    pub const PERSON_REID: &str = "person-reid-0277";
    /// What finding people without a face needs, on top of [`FACES`].
    pub const BODIES: [&str; 2] = [PERSONS, PERSON_REID];
    /// Anime: heads to follow characters by, people to catch missed heads,
    /// and the CCIP character-image embedder (the anime embedder; media
    /// whose characters another one found are found again, as ARCFACE is).
    pub const ANIME_HEAD: &str = "anime-head-v2m";
    pub const ANIME_PERSON: &str = "anime-person-v1m";
    pub const ANIME_CCIP: &str = "anime-ccip-b36";
    /// The anime embedder before it: faces it found are still anime heads,
    /// until the anime pass finds them again.
    pub const ANIME_CCIP_OLD: &str = "anime-ccip-caformer24";
    /// What the anime character pass needs.
    pub const ANIME: [&str; 3] = [ANIME_HEAD, ANIME_PERSON, ANIME_CCIP];
    /// The Best anime scan's character embedder (Berkeley's DINOv2-L): the
    /// pass finds and follows heads with [`ANIME`], then embeds each
    /// track's clearest heads with it. Faces it found are keyed by it.
    pub const ANIME_BERKELEY: &str = "anime-berkeley-dino-l";
    /// What the Best anime scan needs.
    pub const ANIME_BEST: [&str; 4] = [ANIME_HEAD, ANIME_PERSON, ANIME_CCIP, ANIME_BERKELEY];
}

/// A download that receives nothing for this long fails (and resumes later).
const STALL_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a rename waits for Windows: a virus scanner may still hold a
/// file just written.
const RENAME_WAIT: Duration = Duration::from_secs(1);
/// Bytes left free on the drive besides a download: the library's database
/// and the logs live there too.
const SPARE: u64 = 64 << 20;

/// A file's size and time of change, which a hash taken of it holds for.
type Stamp = (u64, Option<SystemTime>);

#[derive(Clone, Debug)]
pub struct ModelStore {
    dir: PathBuf,
    /// Models this process hashed, with the stamp of their file then: they
    /// aren't hashed again until it changes. Shared by the store's clones.
    checked: Arc<Mutex<HashMap<String, Stamp>>>,
    /// How long a download may receive nothing before it fails.
    stall: Duration,
    /// The free bytes on the drive of a folder.
    free_space: fn(&Path) -> std::io::Result<u64>,
    /// Writes a download's bytes to its part: a test's full drive or its count,
    /// else the file.
    write_part: fn(&mut File, &[u8]) -> std::io::Result<()>,
}

impl ModelStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into(), checked: Arc::default(), stall: STALL_TIMEOUT, free_space, write_part: File::write_all }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn file(&self, p: &Pinned) -> PathBuf {
        self.dir.join(&p.file)
    }

    fn marker(&self, p: &Pinned) -> PathBuf {
        self.dir.join(format!("{}.verified", p.file))
    }

    /// Where a download of `p` goes, and where the archive of a tar.gz member does.
    fn parts(&self, p: &Pinned) -> [PathBuf; 2] {
        [self.dir.join(format!("{}.part", p.file)), self.dir.join(format!("{}.archive.part", p.file))]
    }

    fn checked(&self) -> MutexGuard<'_, HashMap<String, Stamp>> {
        self.checked.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Path of an installed model, `None` when it has to be downloaded —
    /// again, when its file no longer hashes to the pinned SHA256 (see the
    /// module doc). The first call for a model in a process reads its file.
    pub fn installed(&self, id: &str) -> ModelResult<Option<PathBuf>> {
        self.ready(model(id)?)
    }

    /// Path of an installed pinned file, as [`Self::installed`] finds a model's.
    /// What a store hashed it remembers in itself and its clones only: a
    /// caller that wants a file hashed once per process keeps one store for
    /// its folder.
    pub fn installed_pinned(&self, p: &Pinned) -> ModelResult<Option<PathBuf>> {
        self.ready(p)
    }

    /// [`Self::installed_pinned`], whose hash ends with `Cancelled` once
    /// `cancel` fires: a file of a few GiB takes a while to read.
    pub fn installed_pinned_until(&self, p: &Pinned, cancel: &CancelToken) -> ModelResult<Option<PathBuf>> {
        self.ready_until(p, Some(cancel))
    }

    /// Whether [`Self::installed_pinned`] would read all of `p`'s file now:
    /// it is there with its marker, and this store hasn't hashed it as it
    /// is. Reads sizes and times alone.
    pub fn would_hash(&self, p: &Pinned) -> bool {
        self.marked(p).is_some_and(|path| stamp(&path).map_or(true, |now| self.checked().get(&p.file) != Some(&now)))
    }

    /// Fails when the drive of the store's folder hasn't room (and
    /// [`SPARE`]) for what fetching all of `ps` still downloads: each one's
    /// size less the part an earlier try left. One check before the first
    /// of several files, so the whole shortfall is told at once, before
    /// gigabytes came for nothing.
    pub fn room_for_pinned(&self, ps: &[Pinned]) -> ModelResult<()> {
        let left = |p: &Pinned| {
            let [part, _] = self.parts(p);
            let have = fs::metadata(part).map(|md| md.len()).unwrap_or(0);
            if have > p.size { p.size } else { p.size - have }
        };
        self.room_for(ps.iter().map(left).fold(0, u64::saturating_add))
    }

    /// Path of a model whose download finished and was verified, without
    /// hashing its file again: for listing the models, not for loading one.
    pub fn downloaded(&self, id: &str) -> ModelResult<Option<PathBuf>> {
        Ok(self.marked(&model(id)?.pinned()))
    }

    /// The file of `p` when it has the pinned size and its marker the pinned hash.
    fn marked(&self, p: &Pinned) -> Option<PathBuf> {
        let path = self.file(p);
        let size_ok = fs::metadata(&path).is_ok_and(|md| md.len() == p.size);
        let verified = fs::read_to_string(self.marker(p)).is_ok_and(|s| s.trim() == p.sha256);
        (size_ok && verified).then_some(path)
    }

    /// The file of `p` when it is installed and still the pinned one.
    fn ready(&self, p: &impl AsPinned) -> ModelResult<Option<PathBuf>> {
        self.ready_until(p, None)
    }

    /// [`Self::ready`], whose hash ends once `cancel` fires.
    fn ready_until(&self, p: &impl AsPinned, cancel: Option<&CancelToken>) -> ModelResult<Option<PathBuf>> {
        let p = &*p.as_pinned();
        plain_name(p)?;
        let Some(path) = self.marked(p) else { return Ok(None) };
        let now = stamp(&path)?;
        if self.checked().get(&p.file) == Some(&now) {
            return Ok(Some(path));
        }
        let got = sha256(&path, cancel)?;
        if got != p.sha256 {
            let damaged = ModelError::Checksum { id: p.id.clone(), got };
            tracing::warn!(model = %p.id, error = %damaged.detail(), "an installed file is damaged; it is downloaded again");
            // Without its marker, the next install fetches it over the damaged file.
            let _ = fs::remove_file(self.marker(p));
            return Ok(None);
        }
        self.checked().insert(p.file.clone(), now);
        Ok(Some(path))
    }

    /// Download (or resume) `id` and verify it. `progress(done, total)` runs
    /// as bytes arrive. Returns the model's path. Waits while another
    /// install of the same model runs (in this process or another one).
    pub fn install(&self, id: &str, cancel: &CancelToken, progress: impl FnMut(u64, u64)) -> ModelResult<PathBuf> {
        self.install_model(model(id)?, cancel, progress)
    }

    fn install_model(&self, m: &Model, cancel: &CancelToken, progress: impl FnMut(u64, u64)) -> ModelResult<PathBuf> {
        self.fetch_pinned(&m.pinned(), cancel, progress)
    }

    /// Download (or resume) the pinned file `p` and verify it, as
    /// [`Self::install`] does a model: its path once it is there.
    pub fn fetch_pinned(
        &self,
        p: &Pinned,
        cancel: &CancelToken,
        progress: impl FnMut(u64, u64),
    ) -> ModelResult<PathBuf> {
        self.fetch_pinned_through(p, None, cancel, progress)
    }

    /// [`Self::fetch_pinned`], each request through `agent`: one whose
    /// names resolve only to public addresses, say, or a test's plain-http
    /// one. Redirects keep its `https_only`, as [`net::agent`]'s do.
    pub fn fetch_pinned_via(
        &self,
        p: &Pinned,
        agent: &ureq::Agent,
        cancel: &CancelToken,
        progress: impl FnMut(u64, u64),
    ) -> ModelResult<PathBuf> {
        self.fetch_pinned_through(p, Some(agent), cancel, progress)
    }

    fn fetch_pinned_through(
        &self,
        p: &Pinned,
        via: Option<&ureq::Agent>,
        cancel: &CancelToken,
        progress: impl FnMut(u64, u64),
    ) -> ModelResult<PathBuf> {
        if let Some(path) = self.ready_until(p, Some(cancel))? {
            return Ok(path);
        }
        fs::create_dir_all(&self.dir)?;
        let _lock = self.lock(p, cancel)?;
        // The install that held the lock may have brought the file.
        if let Some(path) = self.ready_until(p, Some(cancel))? {
            return Ok(path);
        }
        self.fetch_through(p, via, cancel, progress)
    }

    /// Hold `<file>.lock` of `p` (until the file is dropped), waiting while
    /// another install holds it. Two installs of one model would write the
    /// same `.part` and `.unpacked` files, and an unpacked file the other one
    /// truncated could even pass the hash, which covers what each wrote. The
    /// lock file stays: removing it would let a third install lock a new one
    /// while the second still waits on the old.
    fn lock(&self, p: &Pinned, cancel: &CancelToken) -> ModelResult<File> {
        let path = self.dir.join(format!("{}.lock", p.file));
        let file = OpenOptions::new().create(true).write(true).truncate(false).open(&path)?;
        let mut waiting = false;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(file),
                Err(TryLockError::WouldBlock) => {
                    if !waiting {
                        tracing::info!(model = %p.id, "another install of the file runs; waiting for it");
                        waiting = true;
                    }
                    if cancel.is_cancelled() {
                        return Err(ModelError::Cancelled);
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                // A file system that can't lock (some network or FUSE
                // mounts): installs there go on as before, unguarded.
                Err(TryLockError::Error(e)) => {
                    tracing::warn!(model = %p.id, error = %e, "can't lock the install; installing without");
                    return Ok(file);
                }
            }
        }
    }

    /// Fetch `p` from its addresses in turn, with [`net::agent`]'s.
    #[cfg(test)]
    fn fetch(&self, p: &impl AsPinned, cancel: &CancelToken, progress: impl FnMut(u64, u64)) -> ModelResult<PathBuf> {
        self.fetch_through(p, None, cancel, progress)
    }

    /// Fetch `p` from its addresses in turn, through `via` (else each
    /// address's [`net::agent`]). A failed download, a wrong hash or a
    /// changed source moves on to the next one, without the bytes the last
    /// one sent (a resumed part from another host could only fail the
    /// hash); the last address's error is the one returned. A download that
    /// resumed bytes of an earlier try and then fails its hash is fetched
    /// once more from its first byte at the same address: those bytes may
    /// be of another file (a pin that changed after a cancel, the same name
    /// with another hash), and the address may well be right.
    fn fetch_through(
        &self,
        p: &impl AsPinned,
        via: Option<&ureq::Agent>,
        cancel: &CancelToken,
        mut progress: impl FnMut(u64, u64),
    ) -> ModelResult<PathBuf> {
        let p = &*p.as_pinned();
        let watch = Watch { cancel, stall: self.stall };
        watch.check()?;
        fs::create_dir_all(&self.dir)?;
        let mut last = ModelError::Download(format!("{}: no address to download it from", p.id));
        for (i, url) in p.urls.iter().enumerate() {
            if i > 0 {
                tracing::warn!(model = %p.id, error = %last.detail(), "trying the next address");
                self.drop_parts(p);
            }
            let mut resumed = 0;
            let mut fetched = self.fetch_from(p, url, via, watch, &mut progress, &mut resumed);
            if resumed > 0
                && let Err(e @ ModelError::Checksum { .. }) = &fetched
            {
                tracing::warn!(model = %p.id, error = %e.detail(), "a resumed download failed its hash; fetching it whole");
                self.drop_parts(p);
                fetched = self.fetch_from(p, url, via, watch, &mut progress, &mut resumed);
            }
            match fetched {
                Err(e @ (ModelError::Download(_) | ModelError::Checksum { .. } | ModelError::Changed(_))) => last = e,
                done => return done,
            }
        }
        Err(last)
    }

    /// Removes what earlier tries at `p` downloaded.
    fn drop_parts(&self, p: &Pinned) {
        for part in self.parts(p) {
            let _ = fs::remove_file(part);
        }
    }

    /// Fetch `p` from `url` (through `via`, else its own [`net::agent`]),
    /// verify it and put it in place with its marker. `resumed`: how many
    /// of the bytes an earlier try had left the download went on from
    /// (none when the server sent everything again).
    fn fetch_from(
        &self,
        p: &Pinned,
        url: &str,
        via: Option<&ureq::Agent>,
        watch: Watch,
        progress: &mut dyn FnMut(u64, u64),
        resumed: &mut u64,
    ) -> ModelResult<PathBuf> {
        *resumed = 0;
        let id = p.id.as_str();
        let [part, _] = self.parts(p);
        let own;
        let agent = match via {
            Some(agent) => agent,
            None => {
                own = agent(url);
                &own
            }
        };
        // The file as it will be kept, and its hash.
        let (ready, got) = match &p.packed {
            Packed::File => {
                *resumed = self.download(agent, id, url, &part, p.size, 0, watch, progress)?;
                (part.clone(), sha256(&part, Some(watch.cancel))?)
            }
            Packed::ZipMember(member) => {
                self.download_member(agent, p, url, member, &part, watch, progress, resumed)?
            }
            Packed::TarGzMember { member, archive_size, archive_sha256 } => self.download_tar_member(
                agent,
                p,
                url,
                member,
                (*archive_size, archive_sha256),
                watch,
                progress,
                resumed,
            )?,
        };
        if got != p.sha256 {
            let _ = fs::remove_file(&part);
            let _ = fs::remove_file(&ready);
            return Err(ModelError::Checksum { id: id.into(), got });
        }
        #[cfg(unix)]
        if p.exec {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&ready, fs::Permissions::from_mode(0o755))?;
        }
        let path = self.file(p);
        rename(&ready, &path)?;
        if ready != part {
            let _ = fs::remove_file(&part);
        }
        fs::write(self.marker(p), &p.sha256)?;
        // Just hashed: not again in this process unless the file changes.
        if let Ok(now) = stamp(&path) {
            self.checked().insert(p.file.clone(), now);
        }
        tracing::info!(model = id, "installed");
        Ok(path)
    }

    /// Fail before `bytes` are downloaded when the models folder's drive
    /// hasn't room for them (and [`SPARE`]). A drive whose free space can't
    /// be read is let through: a full one then fails the write, which says so.
    pub fn room_for(&self, bytes: u64) -> ModelResult<()> {
        let need = bytes.saturating_add(SPARE);
        match (self.free_space)(&self.dir) {
            Ok(free) if free < need => Err(ModelError::NoSpace { dir: self.dir.clone(), need, free }),
            Ok(_) => Ok(()),
            Err(e) => {
                tracing::warn!(dir = %as_core::paths::name(&self.dir), "free space unknown: {e}");
                Ok(())
            }
        }
    }

    /// Download (or resume) the whole file at `url`, `size` bytes, into
    /// `part`, when the drive has room for what is missing and `extra`
    /// bytes more (what is unpacked from it). Returns how many of its bytes
    /// were there from an earlier try.
    #[allow(clippy::too_many_arguments)]
    fn download(
        &self,
        agent: &ureq::Agent,
        id: &str,
        url: &str,
        part: &Path,
        size: u64,
        extra: u64,
        watch: Watch,
        progress: &mut dyn FnMut(u64, u64),
    ) -> ModelResult<u64> {
        let mut have = fs::metadata(part).map(|md| md.len()).unwrap_or(0);
        if have > size {
            fs::remove_file(part)?;
            have = 0;
        }
        if have == size {
            return Ok(have);
        }
        self.room_for((size - have).saturating_add(extra))?;
        tracing::info!(model = %id, resume_from = have, "downloading");
        let range = (have > 0).then(|| format!("bytes={have}-"));
        let resp = get(agent, url, range.as_deref(), None, watch)?;
        // A server that ignores the range sends everything again.
        if have > 0 && resp.status() != 206 {
            have = 0;
        }
        self.stream(resp, part, have, size, id, url, watch, progress)?;
        Ok(have)
    }

    /// Fetch `member` of the zip archive at `url` alone: the archive's
    /// directory (at its end), the member's local header, then its packed
    /// bytes into `part` (resumable), unpacked next to where the file goes.
    /// Returns the unpacked file and its hash; `resumed`: the packed bytes
    /// an earlier try had left.
    #[allow(clippy::too_many_arguments)]
    fn download_member(
        &self,
        agent: &ureq::Agent,
        p: &Pinned,
        url: &str,
        member: &str,
        part: &Path,
        watch: Watch,
        progress: &mut dyn FnMut(u64, u64),
        resumed: &mut u64,
    ) -> ModelResult<(PathBuf, String)> {
        let fail = |e: String| ModelError::Download(format!("{} in {url}: {e}", p.id));
        // Room for the packed bytes still to come and the unpacked file
        // beside them, before anything travels: as many as a member of the
        // pinned size can take (the directory only tells later how many).
        let have = fs::metadata(part).map(|md| md.len()).unwrap_or(0);
        self.room_for(zip::packed_at_most(p.size).saturating_sub(have).saturating_add(p.size))?;
        let total = remote_len(agent, url, watch)?;
        let tail_start = total - zip::TAIL.min(total);
        let tail = get_range(agent, url, tail_start, total - tail_start, watch)?;
        let (offset, size) = zip::directory(&tail).map_err(fail)?;
        let dir = match offset.checked_sub(tail_start) {
            Some(at) if offset + size <= total => tail[at as usize..(at + size) as usize].to_vec(),
            _ => get_range(agent, url, offset, size, watch)?,
        };
        let entry = zip::find(&dir, member).map_err(fail)?;
        if entry.size != p.size {
            return Err(ModelError::Changed(format!(
                "{}: {member} in {url} is {} bytes, the pinned file {}",
                p.id, entry.size, p.size
            )));
        }
        // Nothing much bigger than the file is downloaded for it.
        if entry.compressed > zip::packed_at_most(p.size) {
            return Err(ModelError::Changed(format!(
                "{}: {member} in {url} takes {} bytes for {}",
                p.id, entry.compressed, p.size
            )));
        }
        let local = get_range(agent, url, entry.header, zip::LOCAL_LEN, watch)?;
        let data = entry.header + zip::data_offset(&local).map_err(fail)?;

        let mut have = fs::metadata(part).map(|md| md.len()).unwrap_or(0);
        if have > entry.compressed {
            fs::remove_file(part)?;
            have = 0;
        }
        *resumed = have;
        if have < entry.compressed {
            tracing::info!(model = %p.id, member, resume_from = have, "downloading");
            let (from, to) = (data + have, data + entry.compressed - 1);
            let resp = get(agent, url, Some(&format!("bytes={from}-{to}")), None, watch)?;
            if resp.status() != 206 {
                return Err(fail(format!("the server sent {} instead of the member's bytes", resp.status())));
            }
            self.stream(resp, part, have, entry.compressed, &p.id, url, watch, progress)?;
        }
        // Unpacked and hashed in one go — never more than the file and a
        // byte, whatever the data claims; an archive replaced meanwhile (a
        // resumed download of another one) fails the hash and starts over.
        // The hash covers what this install wrote, which is what the file
        // holds because no other install runs beside it (`lock`).
        let unpacked = self.dir.join(format!("{}.unpacked", p.file));
        let mut out = Hashing::new(File::create(&unpacked)?);
        let n = match unpack_member(&entry, p, member, part, &mut out) {
            Ok(n) => n,
            Err(e) => {
                drop(out);
                let _ = fs::remove_file(&unpacked);
                return Err(e);
            }
        };
        out.file.sync_all()?;
        let got = if n == p.size { hex(&out.hash.finalize()) } else { format!("{n} bytes unpacked") };
        Ok((unpacked, got))
    }

    /// Fetch the whole tar.gz at `url` (`archive`: its pinned size and
    /// hash) into `<file>.archive.part` (resumable), check its hash, then
    /// copy `member` out of it next to where the file goes. The archive's
    /// hash comes first, so what is decompressed is the pinned archive and
    /// no other. An archive that fails its hash after a resume is damaged
    /// here (a power cut can leave junk at a part's end), like a wrong hash;
    /// one downloaded whole in this call is another archive: the source
    /// changed. Returns the unpacked file and its hash; `resumed`: the
    /// archive's bytes an earlier try had left.
    #[allow(clippy::too_many_arguments)]
    fn download_tar_member(
        &self,
        agent: &ureq::Agent,
        p: &Pinned,
        url: &str,
        member: &str,
        (archive_size, archive_sha256): (u64, &str),
        watch: Watch,
        progress: &mut dyn FnMut(u64, u64),
        resumed: &mut u64,
    ) -> ModelResult<(PathBuf, String)> {
        let [_, archive] = self.parts(p);
        *resumed = self.download(agent, &p.id, url, &archive, archive_size, p.size, watch, progress)?;
        let resumed = *resumed;
        let got = sha256(&archive, Some(watch.cancel))?;
        if got != archive_sha256 {
            let _ = fs::remove_file(&archive);
            if resumed > 0 {
                return Err(ModelError::Checksum {
                    id: p.id.clone(),
                    got: format!("{got} (the archive, resumed at byte {resumed})"),
                });
            }
            return Err(ModelError::Changed(format!("{}: {url} isn't the pinned archive (sha256 {got})", p.id)));
        }
        // Opened before anything is created, so a failure leaves nothing behind.
        let packed = std::io::BufReader::new(File::open(&archive)?);
        let unpacked = self.dir.join(format!("{}.unpacked", p.file));
        let mut out = Hashing::new(File::create(&unpacked)?);
        let taken = targz::take(packed, member, p.size, &mut out);
        let changed = |why: String| ModelError::Changed(format!("{}: {member} in {url} {why}", p.id));
        let result = match taken {
            Ok(targz::Taken::Copied(n)) => out.file.sync_all().map(|()| n).map_err(ModelError::from),
            Ok(targz::Taken::Missing) => Err(ModelError::Changed(format!("{}: {member} isn't in {url}", p.id))),
            Ok(targz::Taken::Size(n)) => Err(changed(format!("is {n} bytes, the pinned file {}", p.size))),
            // A full disk keeps the archive for the next try.
            Err(e) if out.write_failed => {
                drop(out);
                let _ = fs::remove_file(&unpacked);
                return Err(ModelError::Io(e));
            }
            Err(e) => Err(changed(format!("doesn't unpack: {e}"))),
        };
        let _ = fs::remove_file(&archive);
        match result {
            Ok(n) => {
                let got = if n == p.size { hex(&out.hash.finalize()) } else { format!("{n} bytes unpacked") };
                Ok((unpacked, got))
            }
            Err(e) => {
                drop(out);
                let _ = fs::remove_file(&unpacked);
                Err(e)
            }
        }
    }

    /// Write the body of `resp` to `part` — after its first `have` bytes,
    /// which it keeps — until it holds `total`, and never more: a length past
    /// the missing bytes fails the download at its headers, and a chunk that
    /// would pass `total` fails it unwritten, however the body is framed or
    /// compressed. A body that long isn't the file, so the part goes, with
    /// the bytes it went on from. Any other failure leaves it for a resume,
    /// a full drive too: the pin bounds the part, and [`Self::room_for`]
    /// counts what it holds. `progress(done, total)` as bytes come.
    #[allow(clippy::too_many_arguments)]
    fn stream(
        &self,
        resp: ureq::http::Response<ureq::Body>,
        part: &Path,
        have: u64,
        total: u64,
        id: &str,
        url: &str,
        watch: Watch,
        progress: &mut dyn FnMut(u64, u64),
    ) -> ModelResult<()> {
        let past = || ModelError::Download(format!("{id}: {url} sent more than {total} bytes"));
        // ureq names no length for a body it unpacks: that one counts below.
        if resp.body().content_length().is_some_and(|len| len > total - have) {
            let _ = fs::remove_file(part);
            return Err(past());
        }
        let mut out = OpenOptions::new().create(true).write(true).append(have > 0).truncate(have == 0).open(part)?;
        let mut done = have;
        let mut too_long = false;
        progress(done, total);
        let got = pump(resp, id, watch, |chunk| {
            let len = chunk.len() as u64;
            if len > total - done {
                too_long = true;
                return Err(past());
            }
            (self.write_part)(&mut out, chunk)?;
            done += len;
            progress(done, total);
            Ok(())
        })
        .and_then(|()| Ok(out.sync_all()?));
        if too_long {
            drop(out);
            let _ = fs::remove_file(part);
        }
        got?;
        if done != total {
            return Err(ModelError::Download(format!("{id}: got {done} of {total} bytes")));
        }
        Ok(())
    }

    /// Delete the files of models a better one replaced (the manifest's
    /// `retired` list), with their `.verified` markers, `.lock` files and
    /// unfinished `.part` (and `.archive.part`) downloads — nothing else in
    /// the folder. Returns the bytes freed.
    pub fn prune(&self) -> ModelResult<u64> {
        let retired: Vec<&str> = retired().iter().map(String::as_str).collect();
        self.prune_files(&retired)
    }

    fn prune_files(&self, retired: &[&str]) -> ModelResult<u64> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e.into()),
        };
        let mut freed = 0;
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let file = [".verified", ".archive.part", ".part", ".unpacked", ".lock"]
                .iter()
                .find_map(|suffix| name.strip_suffix(suffix))
                .unwrap_or(name);
            if !retired.contains(&file) || manifest().iter().any(|m| m.file == file) || !entry.file_type()?.is_file() {
                continue;
            }
            let len = entry.metadata().map_or(0, |m| m.len());
            match fs::remove_file(entry.path()) {
                Ok(()) => {
                    freed += len;
                    tracing::info!(file = name, "removed a model that is no longer used");
                }
                Err(e) => tracing::warn!(file = name, "removing a model that is no longer used: {e}"),
            }
        }
        Ok(freed)
    }
}

/// Refuse a pinned `file` that isn't one plain name: its file, marker, lock
/// and parts would land outside the store's folder (`../x`, an absolute
/// path) or in a folder of it (`x/`).
fn plain_name(p: &Pinned) -> ModelResult<()> {
    let mut parts = Path::new(&p.file).components();
    match (parts.next(), parts.next()) {
        (Some(std::path::Component::Normal(name)), None) if name == p.file.as_str() => Ok(()),
        _ => Err(ModelError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidFilename,
            format!("{}: {:?} isn't a plain file name", p.id, p.file),
        ))),
    }
}

/// The size and time of change of the file at `path`.
fn stamp(path: &Path) -> std::io::Result<Stamp> {
    let md = fs::metadata(path)?;
    Ok((md.len(), md.modified().ok()))
}

/// Unpack `entry` (`member` of `p`'s archive), whose packed bytes are in
/// `part`, into `out`. Bytes that don't unpack are damaged: they go, like a
/// wrong hash. A write that fails is the disk's (full, most likely), not
/// the download's: its bytes stay for the next try.
fn unpack_member<W: Write>(
    entry: &zip::Entry,
    p: &impl AsPinned,
    member: &str,
    part: &Path,
    out: &mut Hashing<W>,
) -> ModelResult<u64> {
    let p = &*p.as_pinned();
    let packed = std::io::BufReader::new(File::open(part)?.take(entry.compressed));
    match zip::unpack(entry.method, packed, out, p.size + 1) {
        Ok(n) => Ok(n),
        Err(e) if out.write_failed => Err(ModelError::Io(e)),
        Err(e) => {
            let _ = fs::remove_file(part);
            Err(ModelError::Checksum { id: p.id.clone(), got: format!("{member} doesn't unpack: {e}") })
        }
    }
}

/// How long a small request (a file's size, a few bytes of an archive) may
/// take in all, its body included: a minute, and a second per 16 KiB. Never
/// set on the agent: a whole model takes as long as the line needs.
fn small_request_budget(len: u64) -> Duration {
    Duration::from_secs(60 + len / 16_384)
}

/// What ends a transfer early: a cancel, or a connection quiet for `stall`.
#[derive(Clone, Copy)]
pub(crate) struct Watch<'a> {
    pub(crate) cancel: &'a CancelToken,
    pub(crate) stall: Duration,
}

impl Watch<'_> {
    /// `Cancelled` once cancelled; checked before every request.
    fn check(&self) -> ModelResult<()> {
        if self.cancel.is_cancelled() { Err(ModelError::Cancelled) } else { Ok(()) }
    }
}

/// Hand the body of `resp` to `sink` as it comes. The body is read on its
/// own thread, so cancelling (or a stalled connection) never waits on a
/// socket that has gone quiet. `what` names the transfer in errors. Nothing
/// here limits how much comes: a body may never end, so `sink` counts what
/// it gets (unpacked, when the answer was compressed) and stops it.
pub(crate) fn pump(
    resp: ureq::http::Response<ureq::Body>,
    what: &str,
    watch: Watch,
    mut sink: impl FnMut(&[u8]) -> ModelResult<()>,
) -> ModelResult<()> {
    let (tx, rx) = std::sync::mpsc::sync_channel::<std::io::Result<Vec<u8>>>(16);
    let mut body = resp.into_body().into_reader();
    std::thread::spawn(move || {
        let mut buf = vec![0u8; 1 << 16];
        loop {
            let r = body.read(&mut buf).map(|n| buf[..n].to_vec());
            let end = !matches!(&r, Ok(b) if !b.is_empty());
            if tx.send(r).is_err() || end {
                return;
            }
        }
    });
    let mut last_data = Instant::now();
    loop {
        watch.check()?;
        let chunk = match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(r) => r.map_err(|e| ModelError::Download(format!("{what}: {e}")))?,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if last_data.elapsed() > watch.stall {
                    return Err(ModelError::Download(format!("{what}: no data for {:?}", watch.stall)));
                }
                continue;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        };
        if chunk.is_empty() {
            return Ok(());
        }
        last_data = Instant::now();
        sink(&chunk)?;
    }
}

/// Bytes `from..from + len` of the file at `url` (`len` > 0): only those,
/// or an error.
fn get_range(agent: &ureq::Agent, url: &str, from: u64, len: u64, watch: Watch) -> ModelResult<Vec<u8>> {
    let what = format!("{url} bytes {from}+{len}");
    let failed = |e: String| ModelError::Download(format!("{what}: {e}"));
    let range = format!("bytes={from}-{}", from + len.max(1) - 1);
    let resp = get(agent, url, Some(&range), Some(small_request_budget(len)), watch)?;
    if resp.status() != 206 {
        return Err(failed(format!("the server answered {} to a range request", resp.status())));
    }
    let mut body = Vec::new();
    pump(resp, &what, watch, |chunk| {
        if body.len() as u64 + chunk.len() as u64 > len {
            return Err(failed(format!("more than {len} bytes")));
        }
        body.extend_from_slice(chunk);
        Ok(())
    })?;
    if body.len() as u64 != len {
        return Err(failed(format!("got {} bytes", body.len())));
    }
    Ok(body)
}

/// How big the file at `url` is: the total its server names for its first byte.
fn remote_len(agent: &ureq::Agent, url: &str, watch: Watch) -> ModelResult<u64> {
    let failed = |e: String| ModelError::Download(format!("{url}: {e}"));
    let resp = get(agent, url, Some("bytes=0-0"), Some(small_request_budget(1)), watch)?;
    let total = resp
        .headers()
        .get("content-range")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit('/').next())
        .and_then(|t| t.trim().parse::<u64>().ok());
    match (resp.status().as_u16(), total) {
        (206, Some(total)) if total > 0 => Ok(total),
        (status, _) => Err(failed(format!("no size in the answer ({status}) to a range request"))),
    }
}

/// Renames `from` to `to`, trying again for a moment while Windows refuses:
/// a virus scanner may still hold a file just written there.
fn rename(from: &Path, to: &Path) -> std::io::Result<()> {
    let start = Instant::now();
    loop {
        match fs::rename(from, to) {
            Err(e)
                if cfg!(windows)
                    && e.kind() == std::io::ErrorKind::PermissionDenied
                    && start.elapsed() < RENAME_WAIT =>
            {
                std::thread::sleep(Duration::from_millis(50));
            }
            renamed => return renamed,
        }
    }
}

/// A file being written and hashed at once.
struct Hashing<W = File> {
    file: W,
    hash: Sha256,
    /// A write failed: the trouble is the disk's, not the data's.
    write_failed: bool,
}

impl<W> Hashing<W> {
    fn new(file: W) -> Self {
        Self { file, hash: Sha256::new(), write_failed: false }
    }
}

impl<W: Write> Write for Hashing<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.file.write(buf).inspect_err(|_| self.write_failed = true)?;
        self.hash.update(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The SHA-256 of the file at `path`, read a MiB at a time: `Cancelled`
/// once `cancel` fires, between two of them.
fn sha256(path: &Path, cancel: Option<&CancelToken>) -> ModelResult<String> {
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        if cancel.is_some_and(CancelToken::is_cancelled) {
            return Err(ModelError::Cancelled);
        }
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

#[cfg(test)]
mod tests {
    use std::io::BufRead;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    #[test]
    fn each_error_has_its_key_and_keeps_its_words() {
        let said = [
            (ModelError::Unknown("a".into()), "model.unknown", "Unknown model a."),
            (
                ModelError::Download("x".into()),
                "model.downloadFailed",
                "The AI model couldn't be downloaded. Check the internet connection and try again.",
            ),
            (
                ModelError::Checksum { id: "a".into(), got: "0".into() },
                "model.damaged",
                "The downloaded AI model is damaged. It will be downloaded again.",
            ),
            (
                ModelError::Changed("x".into()),
                "model.changed",
                "An AI model changed where AutoScene downloads it from. Update AutoScene to get it.",
            ),
            (
                ModelError::NoSpace { dir: PathBuf::from("m"), need: 1_500_000_001, free: 1 },
                "model.noSpace",
                "There isn't enough free space for the AI model. Free up 1500 MB on the drive of AutoScene's data folder and try again.",
            ),
            (ModelError::Cancelled, "model.cancelled", "Cancelled."),
            (ModelError::Io(std::io::ErrorKind::StorageFull.into()), "file.diskFull", "The disk is full."),
        ];
        for (e, key, english) in said {
            assert_eq!((e.msg().key.as_str(), e.to_string().as_str()), (key, english));
        }
    }

    #[test]
    fn manifest_is_complete() {
        assert!(!manifest().is_empty());
        for m in manifest() {
            assert_eq!(m.sha256.len(), 64, "{}: sha256", m.id);
            assert!(m.url.starts_with("https://"), "{}: url", m.id);
            // Hugging Face files pin an immutable revision, not a branch.
            if m.url.contains("huggingface.co") {
                let rev = m.url.split("/resolve/").nth(1).and_then(|r| r.split('/').next());
                assert!(
                    rev.is_some_and(|r| r.len() == 40 && r.chars().all(|c| c.is_ascii_hexdigit())),
                    "{}: pinned revision",
                    m.id
                );
            }
            assert!(m.size > 0, "{}: size", m.id);
            assert!(!m.license.is_empty(), "{}: license", m.id);
            assert_eq!(
                m.non_commercial,
                m.license.to_lowercase().contains("non-commercial"),
                "{}: non_commercial says what the licence says",
                m.id
            );
        }
        for id in
            [ids::TRANSNET, ids::SCRFD, ids::ARCFACE, ids::PERSONS, ids::PERSON_REID].into_iter().chain(ids::ANIME)
        {
            assert!(model(id).is_ok(), "{id}");
        }
        assert!(matches!(model("nope"), Err(ModelError::Unknown(_))));
        // Model files land flat in the models folder: two entries may not
        // write to the same name, and every id is unique.
        let mut files: Vec<&str> = manifest().iter().map(|m| m.file.as_str()).collect();
        files.sort_unstable();
        assert!(files.windows(2).all(|w| w[0] != w[1]), "duplicate model file names");
        let mut seen: Vec<&str> = manifest().iter().map(|m| m.id.as_str()).collect();
        seen.sort_unstable();
        assert!(seen.windows(2).all(|w| w[0] != w[1]), "duplicate model ids");
        // A card link, where there is one, is an https page.
        for m in manifest() {
            if let Some(card) = &m.card {
                assert!(card.starts_with("https://"), "{}: card", m.id);
            }
        }
        // Files of one archive land as the files they are.
        for id in ids::BODIES {
            let m = model(id).unwrap();
            assert_eq!(m.member.as_deref(), Some(m.file.as_str()), "{id}");
        }
        // The full face pass and the people are InsightFace's, for
        // non-commercial use; the shots and Fast Scan aren't restricted.
        for id in ids::FACES.into_iter().chain(ids::BODIES) {
            assert!(model(id).unwrap().non_commercial, "{id}");
        }
        for id in ids::FAST_FACES.into_iter().chain([ids::TRANSNET]) {
            assert!(!model(id).unwrap().non_commercial, "{id}");
        }
    }

    /// The range a request's headers ask for, within a file of `len` bytes
    /// (`None`: the whole file).
    fn read_range(reader: &mut impl BufRead, len: u64) -> Option<(u64, u64)> {
        let mut range = None;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim().is_empty() {
                return range;
            }
            if let Some(r) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                let (a, b) = r.trim().split_once('-').unwrap();
                let a: u64 = a.parse().unwrap();
                let b: u64 = if b.is_empty() { len - 1 } else { b.parse().unwrap() };
                range = Some((a, b.min(len - 1)));
            }
        }
    }

    /// A server for one file that answers byte ranges, counting what it sent.
    /// `/moved/…` redirects to `/…` and `/loop/…` to itself, each redirect
    /// announcing a body it sends a few bytes of, then holding the
    /// connection without a word more. `/gone/…` answers 404.
    fn serve(data: Vec<u8>) -> (String, Arc<AtomicU64>) {
        serve_padded(data, 0)
    }

    /// [`serve`], with `pad` bytes too many after any body of 100 000 bytes
    /// or more: the small reads of an archive's directory come as asked.
    fn serve_padded(data: Vec<u8>, pad: usize) -> (String, Arc<AtomicU64>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/cheetah.zip", listener.local_addr().unwrap());
        let sent = Arc::new(AtomicU64::new(0));
        let counted = sent.clone();
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { return };
                let mut reader = std::io::BufReader::new(conn.try_clone().unwrap());
                let mut request = String::new();
                let _ = reader.read_line(&mut request);
                let path = request.split(' ').nth(1).unwrap_or("/").to_string();
                let range = read_range(&mut reader, data.len() as u64);
                if let Some(to) = path.strip_prefix("/moved").or_else(|| path.starts_with("/loop/").then_some(&*path)) {
                    let head = format!("HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 4096\r\n\r\n");
                    let _ = conn.write_all(head.as_bytes()).and_then(|()| conn.write_all(&[b'.'; 16]));
                    held.push(conn);
                    continue;
                }
                if path.starts_with("/gone/") {
                    let _ = conn.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    continue;
                }
                let (status, body, extra) = match range {
                    Some((a, b)) => (
                        "206 Partial Content",
                        &data[a as usize..=b as usize],
                        format!("Content-Range: bytes {a}-{b}/{}\r\n", data.len()),
                    ),
                    None => ("200 OK", &data[..], String::new()),
                };
                counted.fetch_add(body.len() as u64, Ordering::Relaxed);
                let pad = vec![0u8; if body.len() >= 100_000 { pad } else { 0 }];
                let len = body.len() + pad.len();
                let head = format!("HTTP/1.1 {status}\r\nContent-Length: {len}\r\n{extra}Connection: close\r\n\r\n");
                let _ = conn.write_all(head.as_bytes()).and_then(|()| conn.write_all(body));
                let _ = conn.write_all(&pad);
            }
        });
        (url, sent)
    }

    /// A server that answers every request with `head`, then `body` `rounds`
    /// times or until the client hangs up: its address, and what it was
    /// asked (in lower case).
    fn serve_flood(head: String, body: Vec<u8>, rounds: u32) -> (String, Arc<Mutex<String>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/flood.onnx", listener.local_addr().unwrap());
        let asked = Arc::new(Mutex::new(String::new()));
        let heard = asked.clone();
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { return };
                let mut reader = std::io::BufReader::new(conn.try_clone().unwrap());
                let mut line = String::new();
                while reader.read_line(&mut line).unwrap_or(0) > 0 && !line.trim().is_empty() {
                    heard.lock().unwrap().push_str(&line.to_ascii_lowercase());
                    line.clear();
                }
                let _ = conn.write_all(head.as_bytes());
                for _ in 0..rounds {
                    if conn.write_all(&body).is_err() {
                        break;
                    }
                }
            }
        });
        (url, asked)
    }

    /// A server that answers every range request with its headers and half
    /// its bytes, then holds the connection without a word more.
    fn serve_stalling(data: Vec<u8>) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/cheetah.zip", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { return };
                let mut reader = std::io::BufReader::new(conn.try_clone().unwrap());
                let (a, b) = read_range(&mut reader, data.len() as u64).unwrap_or((0, data.len() as u64 - 1));
                let body = &data[a as usize..=b as usize];
                let head = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {a}-{b}/{}\r\n\r\n",
                    body.len(),
                    data.len()
                );
                let _ = conn.write_all(head.as_bytes()).and_then(|()| conn.write_all(&body[..body.len() / 2]));
                held.push(conn);
            }
        });
        url
    }

    /// `fetch` on a thread of its own: a request that hangs fails the test
    /// instead of holding it up for good.
    fn fetch_within(store: ModelStore, m: Model, limit: Duration) -> ModelResult<PathBuf> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || drop(tx.send(store.fetch(&m, &CancelToken::new(), |_, _| {}))));
        rx.recv_timeout(limit).expect("the download hung")
    }

    fn sha(bytes: &[u8]) -> String {
        hex(&Sha256::digest(bytes))
    }

    /// Bytes that look like a model's (nothing a zip can shrink much).
    fn model_bytes(len: u32) -> Vec<u8> {
        (0..len).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect()
    }

    /// A model of `bytes` at `url`, kept as `file`.
    fn test_model(file: &str, url: String, member: Option<&str>, bytes: &[u8]) -> Model {
        Model {
            id: format!("test-{file}"),
            file: file.into(),
            purpose: String::new(),
            url,
            member: member.map(Into::into),
            sha256: sha(bytes),
            size: bytes.len() as u64,
            license: String::new(),
            non_commercial: false,
            source: String::new(),
            card: None,
            terms: None,
        }
    }

    /// An archive with a big file around `model` (`reid_0265.onnx`).
    fn archive_around(model: &[u8]) -> Vec<u8> {
        let other = vec![7u8; 2_000_000];
        zip::tests::archive(
            &[("w600k_r50.onnx", &other, false), ("reid_0265.onnx", model, true), ("MODEL.LICENSE", b"{}", false)],
            b"",
        )
    }

    #[test]
    fn a_model_inside_a_zip_is_fetched_alone() {
        // An archive with a big file around the model: only the model's bytes travel.
        let model = model_bytes(300_000);
        let zip = archive_around(&model);
        let (url, sent) = serve(zip.clone());
        let m = test_model("reid_0265.onnx", url, Some("reid_0265.onnx"), &model);
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path());
        // Cancelled before it starts: nothing travels.
        let cancelled = CancelToken::new();
        cancelled.cancel();
        assert!(matches!(store.fetch(&m, &cancelled, |_, _| {}), Err(ModelError::Cancelled)));
        assert_eq!(sent.load(Ordering::Relaxed), 0);
        // Half of it was there from an interrupted try.
        let entry = {
            let tail = &zip[zip.len() - (zip::TAIL as usize).min(zip.len())..];
            let (offset, size) = zip::directory(tail).unwrap();
            zip::find(&zip[offset as usize..(offset + size) as usize], "reid_0265.onnx").unwrap()
        };
        let data = (entry.header + zip::data_offset(&zip[entry.header as usize..]).unwrap()) as usize;
        let half = entry.compressed as usize / 2;
        fs::write(dir.path().join("reid_0265.onnx.part"), &zip[data..data + half]).unwrap();
        let mut last = (0, 0);
        let path = store.fetch(&m, &CancelToken::new(), |d, t| last = (d, t)).unwrap();
        assert_eq!(fs::read(&path).unwrap(), model);
        assert_eq!(fs::read_to_string(dir.path().join("reid_0265.onnx.verified")).unwrap(), m.sha256);
        assert_eq!(last, (entry.compressed, entry.compressed), "progress counts the packed bytes");
        assert!(
            !dir.path().join("reid_0265.onnx.part").exists() && !dir.path().join("reid_0265.onnx.unpacked").exists()
        );
        let travelled = sent.load(Ordering::Relaxed);
        assert!(
            travelled < entry.compressed + zip::TAIL + 1024,
            "{travelled} bytes for a {} byte member",
            entry.compressed
        );

        // A part left from another archive fails the hash, goes, and the
        // member is fetched whole at once.
        let again = tempfile::tempdir().unwrap();
        let store = ModelStore::new(again.path());
        fs::write(again.path().join("reid_0265.onnx.part"), vec![0u8; half]).unwrap();
        assert_eq!(fs::read(store.fetch(&m, &CancelToken::new(), |_, _| {}).unwrap()).unwrap(), model);
        assert!(!again.path().join("reid_0265.onnx.part").exists());

        // A member that isn't the pinned size: the source changed, nothing is downloaded.
        let changed = Model { size: m.size + 1, ..m };
        let other_dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            ModelStore::new(other_dir.path()).fetch(&changed, &CancelToken::new(), |_, _| {}),
            Err(ModelError::Changed(_))
        ));
    }

    #[test]
    fn a_quiet_server_fails_the_download_and_a_cancel_ends_it_at_once() {
        // The archive's end comes halfway, then nothing.
        let model = model_bytes(300_000);
        let m = test_model("reid_0265.onnx", serve_stalling(archive_around(&model)), Some("reid_0265.onnx"), &model);
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore { stall: Duration::from_millis(300), ..ModelStore::new(dir.path()) };
        let started = Instant::now();
        let r = store.fetch(&m, &CancelToken::new(), |_, _| {});
        assert!(matches!(&r, Err(ModelError::Download(why)) if why.contains("no data")), "{r:?}");
        assert!(started.elapsed() < Duration::from_secs(10), "{:?}", started.elapsed());

        // A cancel doesn't wait for the stall.
        let store = ModelStore::new(dir.path());
        let cancel = CancelToken::new();
        let later = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            later.cancel();
        });
        let started = Instant::now();
        assert!(matches!(store.fetch(&m, &cancel, |_, _| {}), Err(ModelError::Cancelled)));
        assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
    }

    #[test]
    fn a_redirect_whose_body_never_ends_holds_up_nothing() {
        // A whole model and one inside a zip, each behind a redirect that
        // announces a body, sends a little of it and goes quiet: ureq
        // would wait for the rest inside `.call()`, with nothing to end it.
        let model = model_bytes(300_000);
        let dir = tempfile::tempdir().unwrap();
        let (url, _) = serve(model.clone());
        let whole = test_model("whole.onnx", url.replace("/cheetah", "/moved/cheetah"), None, &model);
        let path = fetch_within(ModelStore::new(dir.path()), whole, Duration::from_secs(20)).unwrap();
        assert_eq!(fs::read(path).unwrap(), model);
        let (url, _) = serve(archive_around(&model));
        let member =
            test_model("reid_0265.onnx", url.replace("/cheetah", "/moved/cheetah"), Some("reid_0265.onnx"), &model);
        let path = fetch_within(ModelStore::new(dir.path()), member, Duration::from_secs(20)).unwrap();
        assert_eq!(fs::read(path).unwrap(), model);

        // A redirect back to itself ends at the limit.
        let looping = test_model("loop.onnx", url.replace("/cheetah", "/loop/cheetah"), None, &model);
        let r = fetch_within(ModelStore::new(dir.path()), looping, Duration::from_secs(20));
        assert!(matches!(&r, Err(ModelError::Download(why)) if why.contains("redirects")), "{r:?}");
    }

    #[test]
    fn a_model_on_https_is_never_fetched_in_the_clear() {
        assert!(agent("https://huggingface.co/immich-app/buffalo_l/resolve/x/model.onnx").config().https_only());
        // Only a test server speaks plain http.
        assert!(!agent("http://127.0.0.1:1/cheetah.zip").config().https_only());
    }

    #[test]
    fn a_download_needs_room_on_the_drive() {
        fn one_mb(_: &Path) -> std::io::Result<u64> {
            Ok(1 << 20)
        }
        fn unknown(_: &Path) -> std::io::Result<u64> {
            Err(std::io::Error::other("no such call"))
        }
        let model = model_bytes(300_000);
        let (url, sent) = serve(model.clone());
        let m = test_model("room.onnx", url, None, &model);
        let dir = tempfile::tempdir().unwrap();
        let full = ModelStore { free_space: one_mb, ..ModelStore::new(dir.path()) };
        let e = full.fetch(&m, &CancelToken::new(), |_, _| {}).unwrap_err();
        assert!(
            matches!(e, ModelError::NoSpace { need, free, .. } if need == m.size + SPARE && free == 1 << 20),
            "{e:?}"
        );
        assert_eq!(
            e.to_string(),
            "There isn't enough free space for the AI model. Free up 67 MB on the drive of AutoScene's data folder and try again."
        );
        assert_eq!(sent.load(Ordering::Relaxed), 0, "nothing travels");

        // A member needs room for its packed bytes and the file, before anything travels.
        let (url, sent) = serve(archive_around(&model));
        let member = test_model("reid_0265.onnx", url, Some("reid_0265.onnx"), &model);
        let e = full.fetch(&member, &CancelToken::new(), |_, _| {}).unwrap_err();
        let need = zip::packed_at_most(member.size) + member.size + SPARE;
        assert!(matches!(e, ModelError::NoSpace { need: n, .. } if n == need), "{e:?}");
        assert_eq!(sent.load(Ordering::Relaxed), 0, "nothing travels");

        // A drive whose free space can't be read lets the download through.
        let unread = ModelStore { free_space: unknown, ..ModelStore::new(dir.path()) };
        assert_eq!(fs::read(unread.fetch(&m, &CancelToken::new(), |_, _| {}).unwrap()).unwrap(), model);
        // This drive has room.
        assert!(free_space(dir.path()).unwrap() > 0);
    }

    /// A writer on a full disk.
    struct FullDisk;

    impl Write for FullDisk {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::StorageFull.into())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_full_disk_says_so_and_keeps_the_download() {
        let e = ModelError::from(std::io::Error::from(std::io::ErrorKind::StorageFull));
        assert_eq!(e.to_string(), "The disk is full.");
        assert!(e.detail().starts_with("model files: "), "{}", e.detail());

        // Unpacking onto a full disk: the downloaded bytes stay for the next try.
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("full.onnx.part");
        let model = model_bytes(1000);
        fs::write(&part, &model).unwrap();
        let m = test_model("full.onnx", String::new(), Some("full.onnx"), &model);
        let stored = zip::Entry { method: zip::Method::Stored, compressed: 1000, size: 1000, header: 0 };
        let r = unpack_member(&stored, &m, "full.onnx", &part, &mut Hashing::new(FullDisk));
        assert!(matches!(&r, Err(ModelError::Io(e)) if e.kind() == std::io::ErrorKind::StorageFull), "{r:?}");
        assert!(part.exists());
        // Bytes that don't unpack are damaged, and go.
        fs::write(&part, [0xFFu8; 1000]).unwrap();
        let deflated = zip::Entry { method: zip::Method::Deflated, ..stored };
        let r = unpack_member(&deflated, &m, "full.onnx", &part, &mut Hashing::new(Vec::new()));
        assert!(matches!(r, Err(ModelError::Checksum { .. })), "{r:?}");
        assert!(!part.exists());
    }

    #[test]
    fn one_install_of_a_model_runs_at_a_time() {
        let model = model_bytes(300_000);
        let (url, sent) = serve(model.clone());
        let m = test_model("lock.onnx", url, None, &model);
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path());
        // Another install (another process, say) holds the model's lock.
        let held = File::create(dir.path().join("lock.onnx.lock")).unwrap();
        held.lock().unwrap();
        let waiting = {
            let (store, m) = (store.clone(), m.clone());
            std::thread::spawn(move || store.install_model(&m, &CancelToken::new(), |_, _| {}))
        };
        std::thread::sleep(Duration::from_millis(400));
        assert!(!waiting.is_finished(), "it waits");
        assert_eq!(sent.load(Ordering::Relaxed), 0, "nothing travels while the other install runs");
        // The other install brings the model and lets go: this one finds it.
        fs::write(dir.path().join("lock.onnx"), &model).unwrap();
        fs::write(dir.path().join("lock.onnx.verified"), &m.sha256).unwrap();
        held.unlock().unwrap();
        assert_eq!(waiting.join().unwrap().unwrap(), dir.path().join("lock.onnx"));
        assert_eq!(sent.load(Ordering::Relaxed), 0, "and downloads nothing");
        assert!(dir.path().join("lock.onnx.lock").exists(), "the lock file stays");

        // An install that waits can be cancelled.
        let other = test_model("other.onnx", m.url.clone(), None, &model);
        let held = File::create(dir.path().join("other.onnx.lock")).unwrap();
        held.lock().unwrap();
        let cancel = CancelToken::new();
        let waiting = {
            let (store, cancel) = (store.clone(), cancel.clone());
            std::thread::spawn(move || store.install_model(&other, &cancel, |_, _| {}))
        };
        std::thread::sleep(Duration::from_millis(200));
        cancel.cancel();
        assert!(matches!(waiting.join().unwrap(), Err(ModelError::Cancelled)));
        assert_eq!(sent.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn prune_removes_only_retired_models() {
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path());
        let current = &model(ids::TRANSNET).unwrap().file;
        // A model of someone's own (a bench export) is not ours to delete.
        let keep = [
            current.clone(),
            format!("{current}.verified"),
            format!("{current}.part"),
            format!("{current}.archive.part"),
            format!("{current}.lock"),
            "notes.txt".into(),
            "bench_export.onnx".into(),
        ];
        let gone = [
            "old_model.onnx",
            "old_model.onnx.verified",
            "old_model.onnx.lock",
            "older.onnx.part",
            "older.onnx.archive.part",
        ];
        for f in keep.iter().map(String::as_str).chain(gone) {
            fs::write(dir.path().join(f), b"1234").unwrap();
        }
        fs::create_dir(dir.path().join("folder.onnx")).unwrap();
        assert_eq!(store.prune_files(&["old_model.onnx", "older.onnx", "folder.onnx"]).unwrap(), 20);
        for f in &keep {
            assert!(dir.path().join(f).exists(), "{f} kept");
        }
        for f in gone {
            assert!(!dir.path().join(f).exists(), "{f} removed");
        }
        assert!(dir.path().join("folder.onnx").is_dir());
        // A store that was never used has nothing to prune.
        assert_eq!(ModelStore::new(dir.path().join("missing")).prune().unwrap(), 0);
    }

    /// The pinned files from their real source (network: `cargo test -p
    /// as-models -- --ignored`).
    #[test]
    #[ignore = "downloads from the internet"]
    fn the_pinned_body_models_download() {
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path());
        for id in ids::BODIES {
            let path = store.install(id, &CancelToken::new(), |_, _| {}).unwrap();
            assert_eq!(fs::metadata(&path).unwrap().len(), model(id).unwrap().size, "{id}");
            assert_eq!(store.installed(id).unwrap(), Some(path));
        }
    }

    /// A gzipped tar of `entries`: (path, type, bytes, link target).
    fn tar_gz(entries: &[(&str, tar::EntryType, &[u8], Option<&str>)]) -> Vec<u8> {
        let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut builder = tar::Builder::new(gz);
        for (path, kind, bytes, link) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(*kind);
            header.set_mode(0o644);
            header.set_size(bytes.len() as u64);
            if let Some(link) = link {
                header.set_link_name(link).unwrap();
            }
            builder.append_data(&mut header, path, *bytes).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    /// `member` of the tar.gz `archive` at `url`, kept as `uv`.
    fn tar_pin(url: String, member: &str, archive: &[u8], bytes: &[u8]) -> Pinned {
        Pinned {
            id: "test-uv".into(),
            file: "uv".into(),
            urls: vec![url],
            sha256: sha(bytes),
            size: bytes.len() as u64,
            packed: Packed::TarGzMember {
                member: member.into(),
                archive_size: archive.len() as u64,
                archive_sha256: sha(archive),
            },
            exec: false,
        }
    }

    /// The names in `dir`, sorted.
    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> =
            fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort_unstable();
        names
    }

    const REGULAR: tar::EntryType = tar::EntryType::Regular;

    #[test]
    fn a_member_of_a_tar_gz_is_unpacked_alone_and_hashed() {
        let uv = model_bytes(200_000);
        let archive = tar_gz(&[
            ("uv-x86_64-unknown-linux-gnu/", tar::EntryType::Directory, b"", None),
            ("uv-x86_64-unknown-linux-gnu/uvx", REGULAR, &model_bytes(50_000), None),
            ("uv-x86_64-unknown-linux-gnu/uv", REGULAR, &uv, None),
        ]);
        let (url, _) = serve(archive.clone());
        let p = tar_pin(url, "uv-x86_64-unknown-linux-gnu/uv", &archive, &uv);
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path());
        assert_eq!(store.installed_pinned(&p).unwrap(), None);
        // Half the archive was there from an interrupted try.
        fs::write(dir.path().join("uv.archive.part"), &archive[..archive.len() / 2]).unwrap();
        let mut last = (0, 0);
        let path = store.fetch_pinned(&p, &CancelToken::new(), |d, t| last = (d, t)).unwrap();
        assert_eq!(path, dir.path().join("uv"));
        assert_eq!(fs::read(&path).unwrap(), uv);
        let total = archive.len() as u64;
        assert_eq!(last, (total, total), "progress counts the archive's bytes");
        // Only the member is kept, with its marker; the archive went.
        assert_eq!(names(dir.path()), ["uv", "uv.lock", "uv.verified"]);
        assert_eq!(fs::read_to_string(dir.path().join("uv.verified")).unwrap(), p.sha256);
        assert_eq!(store.installed_pinned(&p).unwrap(), Some(path.clone()));
        // A new store finds it by its hash too.
        assert_eq!(ModelStore::new(dir.path()).installed_pinned(&p).unwrap(), Some(path));
    }

    #[test]
    fn a_tar_gz_unlike_its_pin_is_refused_before_it_is_unpacked() {
        let uv = model_bytes(100_000);
        let pinned = tar_gz(&[("uv", REGULAR, &uv, None)]);
        // The address now serves another archive of the same size: the
        // member it holds is never read.
        let mut other = pinned.clone();
        let at = other.len() / 2;
        other[at] ^= 0xFF;
        let (url, _) = serve(other);
        let p = tar_pin(url, "uv", &pinned, &uv);
        let dir = tempfile::tempdir().unwrap();
        let r = ModelStore::new(dir.path()).fetch_pinned(&p, &CancelToken::new(), |_, _| {});
        assert!(matches!(&r, Err(ModelError::Changed(why)) if why.contains("isn't the pinned archive")), "{r:?}");
        assert_eq!(names(dir.path()), ["uv.lock"], "neither the archive nor anything of it stays");
    }

    #[test]
    fn a_link_named_like_the_member_is_never_taken() {
        let uv = model_bytes(1000);
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path());
        // Links by its name, and the member only in another folder: nothing to take.
        let links = tar_gz(&[
            ("bin/uv", tar::EntryType::Symlink, b"", Some("/etc/passwd")),
            ("bin/uv", tar::EntryType::Link, b"", Some("bin/other")),
            ("other/bin/uv", REGULAR, &uv, None),
        ]);
        let (url, _) = serve(links.clone());
        let r = store.fetch_pinned(&tar_pin(url, "bin/uv", &links, &uv), &CancelToken::new(), |_, _| {});
        assert!(matches!(&r, Err(ModelError::Changed(why)) if why.contains("bin/uv isn't in")), "{r:?}");
        assert!(!dir.path().join("uv").exists());
        // The regular file after them is the one taken.
        let real = model_bytes(2000);
        let both =
            tar_gz(&[("bin/uv", tar::EntryType::Symlink, b"", Some("/etc/passwd")), ("bin/uv", REGULAR, &real, None)]);
        let (url, _) = serve(both.clone());
        let path = store.fetch_pinned(&tar_pin(url, "bin/uv", &both, &real), &CancelToken::new(), |_, _| {}).unwrap();
        assert_eq!(fs::read(path).unwrap(), real);
        // A member past the first entries isn't looked for.
        let mut many: Vec<(String, Vec<u8>)> = (0..targz::MAX_ENTRIES).map(|i| (format!("f{i}"), vec![1])).collect();
        many.push(("bin/uv".into(), uv.clone()));
        let entries: Vec<_> = many.iter().map(|(n, b)| (n.as_str(), REGULAR, b.as_slice(), None)).collect();
        let late = tar_gz(&entries);
        let mut out = Vec::new();
        assert_eq!(targz::take(&late[..], "bin/uv", uv.len() as u64, &mut out).unwrap(), targz::Taken::Missing);
        assert!(out.is_empty());
    }

    #[test]
    fn a_member_larger_than_its_pin_stops_at_the_pin() {
        let uv = model_bytes(5000);
        let archive = tar_gz(&[("uv", REGULAR, &model_bytes(5001), None)]);
        // Taken out alone, nothing of it is copied.
        let mut out = Vec::new();
        assert_eq!(targz::take(&archive[..], "uv", 5000, &mut out).unwrap(), targz::Taken::Size(5001));
        assert!(out.is_empty());
        // Fetched: the source changed, and nothing is kept.
        let (url, _) = serve(archive.clone());
        let dir = tempfile::tempdir().unwrap();
        let p = tar_pin(url, "uv", &archive, &uv);
        let r = ModelStore::new(dir.path()).fetch_pinned(&p, &CancelToken::new(), |_, _| {});
        assert!(matches!(&r, Err(ModelError::Changed(why)) if why.contains("is 5001 bytes")), "{r:?}");
        assert_eq!(names(dir.path()), ["uv.lock"]);
        // A member of the pinned size is copied whole, and hashed as it is written.
        let mut copied = Hashing::new(Vec::new());
        let fits = tar_gz(&[("uv", REGULAR, &uv, None)]);
        assert_eq!(targz::take(&fits[..], "uv", 5000, &mut copied).unwrap(), targz::Taken::Copied(5000));
        assert_eq!(hex(&copied.hash.finalize()), sha(&uv));
    }

    /// The failure of a body that passed `size` bytes.
    fn past(r: &ModelResult<PathBuf>, size: u64) -> bool {
        matches!(r, Err(ModelError::Download(why)) if why.contains(&format!("more than {size} bytes")))
    }

    #[test]
    fn a_body_past_its_pin_stops_there_and_nothing_of_it_stays() {
        /// What reached a part, whatever became of it.
        static WRITTEN: AtomicU64 = AtomicU64::new(0);
        fn counted(out: &mut File, bytes: &[u8]) -> std::io::Result<()> {
            WRITTEN.fetch_add(bytes.len() as u64, Ordering::Relaxed);
            out.write_all(bytes)
        }
        let model = model_bytes(200_000);
        // No flood is endless (64 MiB at most): a test that fails can't fill
        // the disk it runs on.
        let junk = model_bytes(1 << 16);
        let mut chunk = format!("{:x}\r\n", junk.len()).into_bytes();
        chunk.extend_from_slice(&junk);
        chunk.extend_from_slice(b"\r\n");
        let answers = [
            ("chunked", "Transfer-Encoding: chunked", chunk, model.len() as u64),
            ("until the connection closes", "Connection: close", junk.clone(), model.len() as u64),
            ("with a length past its pin", "Content-Length: 1000000000000", junk.clone(), 0),
        ];
        for (how, header, body, at_most) in answers {
            let (url, _) = serve_flood(format!("HTTP/1.1 200 OK\r\n{header}\r\n\r\n"), body, 1024);
            let m = test_model("flood.onnx", url.clone(), None, &model);
            let dir = tempfile::tempdir().unwrap();
            let store = ModelStore { write_part: counted, ..ModelStore::new(dir.path()) };
            let mut most = 0;
            let r = store.fetch(&m, &CancelToken::new(), |done, _| most = most.max(done));
            assert!(past(&r, m.size), "{how}: {r:?}");
            assert!(matches!(&r, Err(ModelError::Download(why)) if why.contains(&url)), "{how}: {r:?}");
            assert!(most <= at_most, "{how}: {most} bytes reported");
            let written = WRITTEN.swap(0, Ordering::Relaxed);
            assert!(written <= at_most, "{how}: {written} bytes written");
            assert!(names(dir.path()).is_empty(), "{how}: {:?}", names(dir.path()));
        }

        // A resume answered with more than what is missing: the bytes it
        // went on from go too. A length says so before anything is written.
        let length = format!("Content-Length: {}\r\n", model.len() - 999);
        for (header, at_most) in [("", model.len() as u64 - 1000), (length.as_str(), 0)] {
            let (url, asked) = serve_flood(format!("HTTP/1.1 206 Partial Content\r\n{header}\r\n"), junk.clone(), 1024);
            let m = test_model("flood.onnx", url, None, &model);
            let dir = tempfile::tempdir().unwrap();
            fs::write(dir.path().join("flood.onnx.part"), &model[..1000]).unwrap();
            let store = ModelStore { write_part: counted, ..ModelStore::new(dir.path()) };
            let r = store.fetch(&m, &CancelToken::new(), |_, _| {});
            assert!(past(&r, m.size), "{r:?}");
            let written = WRITTEN.swap(0, Ordering::Relaxed);
            assert!(written <= at_most, "{written} bytes written");
            assert!(names(dir.path()).is_empty(), "{:?}", names(dir.path()));
            assert!(asked.lock().unwrap().contains("range: bytes=1000-"));
        }

        // A length that hides the rest: it ends the body, and the file is whole.
        let mut more = model.clone();
        more.extend_from_slice(&junk);
        let (url, _) = serve_flood(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", model.len()), more, 1);
        let m = test_model("flood.onnx", url, None, &model);
        let dir = tempfile::tempdir().unwrap();
        let path = ModelStore::new(dir.path()).fetch(&m, &CancelToken::new(), |_, _| {}).unwrap();
        assert_eq!(fs::read(path).unwrap(), model);
        assert_eq!(names(dir.path()), ["flood.onnx", "flood.onnx.verified"]);
    }

    #[test]
    fn a_compressed_body_counts_what_it_unpacks_to() {
        let model = model_bytes(200_000);
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        gz.write_all(&vec![0u8; 8 << 20]).unwrap();
        let bomb = gz.finish().unwrap();
        assert!((bomb.len() as u64) < model.len() as u64, "{} bytes travel", bomb.len());
        let head = format!("HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\n\r\n", bomb.len());
        let (url, asked) = serve_flood(head, bomb, 1);
        // Sent though nobody asked for it: stopped at the pin all the same.
        let m = test_model("bomb.onnx", url.clone(), None, &model);
        let dir = tempfile::tempdir().unwrap();
        let r = ModelStore::new(dir.path()).fetch(&m, &CancelToken::new(), |_, _| {});
        assert!(past(&r, m.size), "{r:?}");
        assert!(names(dir.path()).is_empty(), "{:?}", names(dir.path()));
        assert!(asked.lock().unwrap().contains("accept-encoding: identity"));
        // A capped get takes compression, and its cap counts what comes out.
        asked.lock().unwrap().clear();
        let r = net::get_capped(&url, 1000, Duration::from_secs(20), &CancelToken::new());
        assert!(matches!(&r, Err(why) if why.contains("more than 1000 bytes")), "{r:?}");
        assert!(asked.lock().unwrap().contains("accept-encoding: gzip"));

        // The length of a packed body says nothing of what comes out: a file
        // that grew in packing still comes.
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::none());
        gz.write_all(&model).unwrap();
        let packed = gz.finish().unwrap();
        assert!(packed.len() > model.len(), "{} bytes travel", packed.len());
        let head = format!("HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\n\r\n", packed.len());
        let (url, _) = serve_flood(head, packed, 1);
        let m = test_model("packed.onnx", url, None, &model);
        let dir = tempfile::tempdir().unwrap();
        let path = ModelStore::new(dir.path()).fetch(&m, &CancelToken::new(), |_, _| {}).unwrap();
        assert_eq!(fs::read(path).unwrap(), model);
    }

    #[test]
    fn an_archive_or_a_member_past_its_pin_stops_there_too() {
        // A tar.gz, taken whole: the archive's size is the pin that counts.
        let uv = model_bytes(5000);
        let archive = tar_gz(&[("uv", REGULAR, &uv, None)]);
        let mut more = archive.clone();
        more.extend_from_slice(&model_bytes(1000));
        let (url, _) = serve(more);
        let dir = tempfile::tempdir().unwrap();
        let p = tar_pin(url, "uv", &archive, &uv);
        let r = ModelStore::new(dir.path()).fetch_pinned(&p, &CancelToken::new(), |_, _| {});
        assert!(past(&r, archive.len() as u64), "{r:?}");
        assert_eq!(names(dir.path()), ["uv.lock"]);

        // A member of a zip: its packed size in the archive.
        let model = model_bytes(300_000);
        let zip = zip::tests::archive(&[("reid_0265.onnx", &model, false), ("other", &[7u8; 200_000], false)], b"");
        let (url, _) = serve_padded(zip, 1000);
        let m = test_model("reid_0265.onnx", url, Some("reid_0265.onnx"), &model);
        let dir = tempfile::tempdir().unwrap();
        let r = ModelStore::new(dir.path()).fetch(&m, &CancelToken::new(), |_, _| {});
        assert!(past(&r, m.size), "{r:?}");
        assert!(names(dir.path()).is_empty(), "{:?}", names(dir.path()));
    }

    #[test]
    fn a_drive_that_fills_up_keeps_what_came_for_the_next_try() {
        /// Half a chunk fits, then the drive is full.
        fn fills_up(out: &mut File, bytes: &[u8]) -> std::io::Result<()> {
            out.write_all(&bytes[..bytes.len() / 2])?;
            Err(std::io::ErrorKind::StorageFull.into())
        }
        let file = model_bytes(300_000);
        let (first, _) = serve(file.clone());
        let (next, sent) = serve(file.clone());
        let mut p = test_model("full.onnx", String::new(), None, &file).pinned();
        p.urls = vec![first, next.clone()];
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("full.onnx.part");
        // What an earlier try left stays too, with what this one added.
        fs::write(&part, &file[..1000]).unwrap();
        let full = ModelStore { write_part: fills_up, ..ModelStore::new(dir.path()) };
        let r = full.fetch_pinned(&p, &CancelToken::new(), |_, _| {});
        assert!(matches!(&r, Err(ModelError::Io(e)) if e.kind() == std::io::ErrorKind::StorageFull), "{r:?}");
        let kept = fs::metadata(&part).unwrap().len();
        assert!(kept > 1000 && kept < file.len() as u64, "{kept} bytes kept");
        assert_eq!(sent.load(Ordering::Relaxed), 0, "no other address is tried");
        // With room again, only the rest travels.
        p.urls = vec![next];
        let path = ModelStore::new(dir.path()).fetch_pinned(&p, &CancelToken::new(), |_, _| {}).unwrap();
        assert_eq!(fs::read(path).unwrap(), file);
        assert_eq!(sent.load(Ordering::Relaxed), file.len() as u64 - kept);

        // The archive of a tar.gz member, as a file's part.
        let uv = model_bytes(5000);
        let archive = tar_gz(&[("uv", REGULAR, &uv, None)]);
        let (url, _) = serve(archive.clone());
        let dir = tempfile::tempdir().unwrap();
        let full = ModelStore { write_part: fills_up, ..ModelStore::new(dir.path()) };
        let r = full.fetch_pinned(&tar_pin(url, "uv", &archive, &uv), &CancelToken::new(), |_, _| {});
        assert!(matches!(&r, Err(ModelError::Io(_))), "{r:?}");
        assert_eq!(names(dir.path()), ["uv.archive.part", "uv.lock"]);
    }

    #[test]
    fn the_next_address_is_tried_when_one_fails() {
        let file = model_bytes(100_000);
        let (good, _) = serve(file.clone());
        let (wrong, _) = serve(model_bytes(100_001)[1..].to_vec());
        let gone = good.replace("/cheetah", "/gone/cheetah");
        let mut p = test_model("next.onnx", String::new(), None, &file).pinned();
        // A 404, then the file. A part left from an earlier try at the first
        // address doesn't travel on: resumed at the second, it would fail the hash.
        p.urls = vec![gone.clone(), good.clone()];
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("next.onnx.part"), [0u8; 10]).unwrap();
        let path = ModelStore::new(dir.path()).fetch_pinned(&p, &CancelToken::new(), |_, _| {}).unwrap();
        assert_eq!(fs::read(path).unwrap(), file);
        assert_eq!(names(dir.path()), ["next.onnx", "next.onnx.lock", "next.onnx.verified"]);
        // A file that isn't the pinned one, then the file.
        p.urls = vec![wrong.clone(), good];
        let again = tempfile::tempdir().unwrap();
        let path = ModelStore::new(again.path()).fetch_pinned(&p, &CancelToken::new(), |_, _| {}).unwrap();
        assert_eq!(fs::read(path).unwrap(), file);
        // Every address failing: the last one's error is returned.
        p.urls = vec![wrong, gone.clone()];
        let other = tempfile::tempdir().unwrap();
        let r = ModelStore::new(other.path()).fetch_pinned(&p, &CancelToken::new(), |_, _| {});
        assert!(matches!(&r, Err(ModelError::Download(why)) if why.starts_with(&gone)), "{r:?}");
        // No address at all.
        p.urls.clear();
        let r = ModelStore::new(other.path()).fetch_pinned(&p, &CancelToken::new(), |_, _| {});
        assert!(matches!(&r, Err(ModelError::Download(why)) if why.contains("no address")), "{r:?}");
    }

    #[test]
    fn a_cancel_or_a_full_drive_tries_no_other_address() {
        fn one_mb(_: &Path) -> std::io::Result<u64> {
            Ok(1 << 20)
        }
        let file = model_bytes(100_000);
        let (next, sent) = serve(file.clone());
        let mut p = test_model("stop.onnx", String::new(), None, &file).pinned();
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("stop.onnx.part");
        // Cancelled while the first address sends: the part stays for the
        // next try, and the second address is never asked.
        p.urls = vec![serve_stalling(file.clone()), next.clone()];
        fs::write(&part, &file[..1000]).unwrap();
        let cancel = CancelToken::new();
        let later = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            later.cancel();
        });
        let r = ModelStore::new(dir.path()).fetch_pinned(&p, &cancel, |_, _| {});
        assert!(matches!(r, Err(ModelError::Cancelled)), "{r:?}");
        assert!(fs::metadata(&part).unwrap().len() >= 1000, "the part stays");
        assert_eq!(sent.load(Ordering::Relaxed), 0);
        // No room on the drive: no address would have any, and the part stays.
        p.urls = vec![next.clone(), next];
        let full = ModelStore { free_space: one_mb, ..ModelStore::new(dir.path()) };
        let r = full.fetch_pinned(&p, &CancelToken::new(), |_, _| {});
        assert!(matches!(r, Err(ModelError::NoSpace { .. })), "{r:?}");
        assert!(part.exists());
        assert_eq!(sent.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn a_tar_gz_member_needs_room_for_the_archive_and_the_member() {
        /// Room for the archive below, not for the member beside it.
        fn archive_only(_: &Path) -> std::io::Result<u64> {
            Ok(SPARE + 100_000)
        }
        let uv = model_bytes(200_000);
        let archive = tar_gz(&[("uv", REGULAR, &uv, None)]);
        assert!(archive.len() < 100_000, "{}", archive.len());
        let (url, sent) = serve(archive.clone());
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore { free_space: archive_only, ..ModelStore::new(dir.path()) };
        let r = store.fetch_pinned(&tar_pin(url, "uv", &archive, &uv), &CancelToken::new(), |_, _| {});
        let need = (archive.len() + uv.len()) as u64 + SPARE;
        assert!(matches!(r, Err(ModelError::NoSpace { need: n, .. }) if n == need), "{r:?}");
        assert_eq!(sent.load(Ordering::Relaxed), 0, "nothing travels");
    }

    #[test]
    fn a_damaged_resumed_archive_is_downloaded_again_not_a_changed_source() {
        let uv = model_bytes(50_000);
        let archive = tar_gz(&[("uv", REGULAR, &uv, None)]);
        let (url, sent) = serve(archive.clone());
        let p = tar_pin(url, "uv", &archive, &uv);
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path());
        // A power cut left the archive's last blocks as zeros.
        let mut damaged = archive.clone();
        let len = damaged.len();
        damaged[len - 512..].fill(0);
        fs::write(dir.path().join("uv.archive.part"), &damaged).unwrap();
        // The damaged archive goes, and it is downloaded whole at once.
        assert_eq!(fs::read(store.fetch_pinned(&p, &CancelToken::new(), |_, _| {}).unwrap()).unwrap(), uv);
        assert_eq!(sent.load(Ordering::Relaxed), archive.len() as u64);
        assert_eq!(names(dir.path()), ["uv", "uv.lock", "uv.verified"]);
        // An archive at the address that isn't the pinned one, after a
        // resume: the source changed, and nothing is kept.
        let other = tempfile::tempdir().unwrap();
        let mut changed = archive.clone();
        changed[len / 2] ^= 0xFF;
        let (url, _) = serve(changed);
        fs::write(other.path().join("uv.archive.part"), &archive[..len / 2]).unwrap();
        let r = ModelStore::new(other.path()).fetch_pinned(
            &tar_pin(url, "uv", &archive, &uv),
            &CancelToken::new(),
            |_, _| {},
        );
        assert!(matches!(&r, Err(ModelError::Changed(why)) if why.contains("isn't the pinned archive")), "{r:?}");
        assert_eq!(names(other.path()), ["uv.lock"]);
    }

    #[test]
    fn a_resumed_part_that_fails_its_hash_is_fetched_whole_once_more() {
        let file = model_bytes(100_000);
        let len = file.len() as u64;
        let (url, sent) = serve(file.clone());
        let p = test_model("weights.bin", url, None, &file).pinned();
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path());
        // A part of another file (its pin changed after a cancel: the same
        // name, another hash): resumed, it fails the hash; fetched whole at
        // the same address, it is the file.
        fs::write(dir.path().join("weights.bin.part"), vec![7u8; 40_000]).unwrap();
        let mut last = (0, 0);
        let path = store.fetch_pinned(&p, &CancelToken::new(), |d, t| last = (d, t)).unwrap();
        assert_eq!(fs::read(&path).unwrap(), file);
        assert_eq!(sent.load(Ordering::Relaxed), (len - 40_000) + len, "the rest, then all of it once");
        assert_eq!(last, (len, len));
        assert_eq!(names(dir.path()), ["weights.bin", "weights.bin.lock", "weights.bin.verified"]);

        // Only once: an address whose file isn't the pinned one fails after
        // its second try, and keeps nothing.
        let (wrong, sent) = serve(model_bytes(100_001)[1..].to_vec());
        let other = tempfile::tempdir().unwrap();
        fs::write(other.path().join("weights.bin.part"), vec![7u8; 40_000]).unwrap();
        let p = Pinned { urls: vec![wrong], ..p };
        let r = ModelStore::new(other.path()).fetch_pinned(&p, &CancelToken::new(), |_, _| {});
        assert!(matches!(r, Err(ModelError::Checksum { .. })), "{r:?}");
        assert_eq!(sent.load(Ordering::Relaxed), (len - 40_000) + len);
        assert_eq!(names(other.path()), ["weights.bin.lock"]);
        // A download that started at its first byte isn't tried again.
        let r = ModelStore::new(other.path()).fetch_pinned(&p, &CancelToken::new(), |_, _| {});
        assert!(matches!(r, Err(ModelError::Checksum { .. })), "{r:?}");
        assert_eq!(sent.load(Ordering::Relaxed), (len - 40_000) + 2 * len);
        // Nor one whose part was longer than the pin: it went before
        // anything travelled, so the wrong file came once.
        let (wrong, sent) = serve(model_bytes(100_001)[1..].to_vec());
        let third = tempfile::tempdir().unwrap();
        fs::write(third.path().join("weights.bin.part"), vec![7u8; 200_000]).unwrap();
        let p = Pinned { urls: vec![wrong], ..p };
        let r = ModelStore::new(third.path()).fetch_pinned(&p, &CancelToken::new(), |_, _| {});
        assert!(matches!(r, Err(ModelError::Checksum { .. })), "{r:?}");
        assert_eq!(sent.load(Ordering::Relaxed), len);
    }

    #[test]
    fn a_hash_ends_at_a_cancel_and_a_store_tells_when_it_would_hash() {
        let model = model_bytes(3 << 20);
        let m = test_model("big.onnx", String::new(), None, &model);
        let p = m.pinned();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.onnx");
        fs::write(&path, &model).unwrap();
        fs::write(dir.path().join("big.onnx.verified"), &m.sha256).unwrap();
        let store = ModelStore::new(dir.path());
        assert!(store.would_hash(&p));
        // A cancel ends the hash, and says nothing of the file: its marker stays.
        let cancelled = CancelToken::new();
        cancelled.cancel();
        assert!(matches!(store.installed_pinned_until(&p, &cancelled), Err(ModelError::Cancelled)));
        assert!(matches!(store.fetch_pinned(&p, &cancelled, |_, _| {}), Err(ModelError::Cancelled)));
        assert!(dir.path().join("big.onnx.verified").exists());
        assert!(store.would_hash(&p));
        // Hashed once, it isn't again in this store.
        assert_eq!(store.installed_pinned_until(&p, &CancelToken::new()).unwrap(), Some(path));
        assert!(!store.would_hash(&p));
        assert!(ModelStore::new(dir.path()).would_hash(&p));
        // Nothing there, nothing to hash.
        let empty = tempfile::tempdir().unwrap();
        assert!(!ModelStore::new(empty.path()).would_hash(&p));
    }

    #[test]
    fn room_for_several_files_is_checked_at_once() {
        fn one_mb(_: &Path) -> std::io::Result<u64> {
            Ok(1 << 20)
        }
        let dir = tempfile::tempdir().unwrap();
        let a = test_model("a.bin", String::new(), None, &model_bytes(300_000)).pinned();
        let b = test_model("b.bin", String::new(), None, &model_bytes(200_000)).pinned();
        let c = test_model("c.bin", String::new(), None, &model_bytes(100_000)).pinned();
        // b half there; c's part longer than its pin, so it goes: all of c comes.
        fs::write(dir.path().join("b.bin.part"), vec![0u8; 50_000]).unwrap();
        fs::write(dir.path().join("c.bin.part"), vec![0u8; 150_000]).unwrap();
        let full = ModelStore { free_space: one_mb, ..ModelStore::new(dir.path()) };
        let e = full.room_for_pinned(&[a.clone(), b.clone(), c.clone()]).unwrap_err();
        let need = 300_000 + 150_000 + 100_000 + SPARE;
        assert!(matches!(e, ModelError::NoSpace { need: n, free, .. } if n == need && free == 1 << 20), "{e:?}");
        ModelStore::new(dir.path()).room_for_pinned(&[a, b, c]).unwrap();
    }

    #[test]
    fn a_pinned_file_comes_through_the_agent_it_is_given() {
        let file = model_bytes(10_000);
        let (url, sent) = serve(file.clone());
        let p = test_model("via.bin", url.clone(), None, &file).pinned();
        let dir = tempfile::tempdir().unwrap();
        // An agent that may only speak https never reaches the plain-http server.
        let https_only: ureq::Agent = net::config("https://example.com/").build().into();
        let r = ModelStore::new(dir.path()).fetch_pinned_via(&p, &https_only, &CancelToken::new(), |_, _| {});
        assert!(matches!(&r, Err(ModelError::Download(_))), "{r:?}");
        assert_eq!(sent.load(Ordering::Relaxed), 0);
        // One that may, does.
        let plain = net::agent(&url);
        let path = ModelStore::new(dir.path()).fetch_pinned_via(&p, &plain, &CancelToken::new(), |_, _| {}).unwrap();
        assert_eq!(fs::read(path).unwrap(), file);
    }

    #[test]
    fn a_pinned_file_lands_only_in_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path().join("store"));
        let pin = test_model("x.bin", String::new(), None, b"x").pinned();
        let absolute = dir.path().join("abs.bin").to_string_lossy().into_owned();
        let mut files = vec!["../escaped.bin", "sub/x.bin", "x.bin/", "./x.bin", "", ".", "..", &absolute];
        if cfg!(windows) {
            files.extend(["sub\\x.bin", "C:x.bin"]);
        }
        let refused =
            |r: ModelResult<_>| matches!(r, Err(ModelError::Io(e)) if e.kind() == std::io::ErrorKind::InvalidFilename);
        for file in files {
            let p = Pinned { file: file.into(), ..pin.clone() };
            assert!(refused(store.fetch_pinned(&p, &CancelToken::new(), |_, _| {}).map(drop)), "{file:?}");
            assert!(refused(store.installed_pinned(&p).map(drop)), "{file:?}");
        }
        assert!(fs::read_dir(dir.path()).unwrap().next().is_none(), "nothing was written");
    }

    #[cfg(unix)]
    #[test]
    fn an_executable_pin_is_marked_runnable() {
        use std::os::unix::fs::PermissionsExt;
        let uv = model_bytes(10_000);
        let archive = tar_gz(&[("uv", REGULAR, &uv, None)]);
        let (url, _) = serve(archive.clone());
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path());
        let p = Pinned { exec: true, ..tar_pin(url, "uv", &archive, &uv) };
        let path = store.fetch_pinned(&p, &CancelToken::new(), |_, _| {}).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o755);
        // A file that isn't one to run keeps the usual mode.
        let (url, _) = serve(uv.clone());
        let plain = Pinned { file: "data.bin".into(), urls: vec![url], packed: Packed::File, exec: false, ..p };
        let path = store.fetch_pinned(&plain, &CancelToken::new(), |_, _| {}).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o111, 0);
    }

    #[test]
    fn a_capped_get_refuses_a_longer_body() {
        let body = model_bytes(1000);
        let (url, _) = serve(body.clone());
        let budget = Duration::from_secs(20);
        assert_eq!(net::get_capped(&url, 1000, budget, &CancelToken::new()).unwrap(), body);
        let r = net::get_capped(&url, 999, budget, &CancelToken::new());
        assert!(matches!(&r, Err(why) if why.contains("more than 999 bytes")), "{r:?}");
        // Only a 200 will do.
        let r = net::get_capped(&url.replace("/cheetah", "/gone/cheetah"), 1000, budget, &CancelToken::new());
        assert!(matches!(&r, Err(why) if why.contains("404")), "{r:?}");
        // Cancelled before it starts.
        let cancelled = CancelToken::new();
        cancelled.cancel();
        assert_eq!(net::get_capped(&url, 1000, budget, &cancelled), Err("Cancelled.".into()));
        // A budget too long to reach is none.
        assert_eq!(net::get_capped(&url, 1000, Duration::MAX, &CancelToken::new()).unwrap(), body);

        // A body that trickles in, a byte every 100 ms, ends with the budget.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let slow = format!("http://{}/slow", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { return };
                read_range(&mut std::io::BufReader::new(conn.try_clone().unwrap()), 1);
                let _ = conn.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\n");
                while conn.write_all(b".").is_ok() {
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        });
        let started = Instant::now();
        let r = net::get_capped(&slow, 1000, Duration::from_secs(1), &CancelToken::new());
        assert!(r.is_err(), "{r:?}");
        assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
    }

    #[test]
    fn a_capped_get_gives_up_after_ten_redirects() {
        let body = model_bytes(1000);
        let (url, _) = serve(body.clone());
        let budget = Duration::from_secs(20);
        // A redirect whose body never ends is followed without waiting on it.
        let moved = url.replace("/cheetah", "/moved/cheetah");
        assert_eq!(net::get_capped(&moved, 1000, budget, &CancelToken::new()).unwrap(), body);
        let started = Instant::now();
        let r = net::get_capped(&url.replace("/cheetah", "/loop/cheetah"), 1000, budget, &CancelToken::new());
        assert!(matches!(&r, Err(why) if why.contains("more than 10 redirects")), "{r:?}");
        assert!(started.elapsed() < budget, "{:?}", started.elapsed());
    }

    #[test]
    fn needs_size_marker_and_content_to_count_as_installed() {
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path());
        let m = model(ids::TRANSNET).unwrap();
        let marker = dir.path().join(format!("{}.verified", m.file));
        assert_eq!(store.installed(ids::TRANSNET).unwrap(), None);
        // Right size but never verified: not installed.
        File::create(dir.path().join(&m.file)).unwrap().set_len(m.size).unwrap();
        assert_eq!(store.installed(ids::TRANSNET).unwrap(), None);
        // A marker beside content that isn't the pinned one: not installed
        // either, and the marker goes, so the next install fetches it again.
        fs::write(&marker, &m.sha256).unwrap();
        assert_eq!(
            store.downloaded(ids::TRANSNET).unwrap(),
            Some(dir.path().join(&m.file)),
            "the listing doesn't hash"
        );
        assert_eq!(store.installed(ids::TRANSNET).unwrap(), None);
        assert!(!marker.exists());
    }

    #[test]
    fn a_process_hashes_a_model_once_and_again_when_it_changes() {
        let model = model_bytes(100_000);
        let m = test_model("check.onnx", String::new(), None, &model);
        let dir = tempfile::tempdir().unwrap();
        let (path, marker) = (dir.path().join("check.onnx"), dir.path().join("check.onnx.verified"));
        let install = || {
            fs::write(&path, &model).unwrap();
            fs::write(&marker, &m.sha256).unwrap();
        };
        let touch = |at: SystemTime| File::options().write(true).open(&path).unwrap().set_modified(at).unwrap();
        install();
        let store = ModelStore::new(dir.path());
        assert_eq!(store.ready(&m).unwrap(), Some(path.clone()));
        // Damaged in place, its size and time of change kept: this process
        // hashed the file already and doesn't again…
        let when = fs::metadata(&path).unwrap().modified().unwrap();
        let mut damaged = model.clone();
        damaged[500] ^= 1;
        fs::write(&path, &damaged).unwrap();
        touch(when);
        assert_eq!(store.clone().ready(&m).unwrap(), Some(path.clone()));
        // …but a new one does (the engine's next start, the app's next
        // launch): the marker goes, and the model is downloaded again.
        assert_eq!(ModelStore::new(dir.path()).ready(&m).unwrap(), None);
        assert!(!marker.exists());

        // A file that changed is hashed again in any process.
        install();
        touch(when + Duration::from_secs(10));
        assert_eq!(store.ready(&m).unwrap(), Some(path.clone()));
        fs::write(&path, &damaged).unwrap();
        touch(when + Duration::from_secs(20));
        assert_eq!(store.ready(&m).unwrap(), None);
        assert!(!marker.exists());
    }
}

//! NVIDIA acceleration: the files of NVIDIA's engine, which the person
//! downloads once from Settings (design-i66_67 §5.4).
//!
//! AutoScene neither bundles nor hosts them. "Agree and download" fetches
//! each file alone from NVIDIA's wheel on the Python Package Index, checks it
//! against the hash `cargo xtask pin-accel` pinned, and writes the folder's
//! stamp last, so a folder without a stamp is never used. The row in
//! Settings is the download's only home: no Activity job, a toast when it
//! ends while Settings isn't open.
//!
//! - **Each release has its own folder.** When an update of the app pins a
//!   new release, the old folder keeps working, by its own stamp, while the
//!   new one downloads in the background; it goes once the new one is
//!   verified. It comes only while this computer's analysis runs on a card
//!   the pack serves: else nothing would load what it fetched. That download
//!   fails or stops quietly, as the old pack still works, and is tried again
//!   at the next start, unless the person stopped it. A new licence asks the
//!   person again instead.
//! - **Removing waits for the engine.** Windows can't delete a loaded
//!   library. A pack marked [`REMOVE`] is never used again, also after a
//!   restart, and its files go once no engine holds them.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use as_api::{AccelInfo, AccelRow};
use as_core::paths::ACCEL_PREFIX;
use as_core::{AppPaths, CancelToken};
use as_engine::accel::{self, AccelState, PinnedFile, Pins};
use as_engine::hardware::Hardware;
use as_engine::{Engine, PACK_FAILURES, cache, pack_failures};
use as_models::{ModelError, ModelResult, ModelStore, Packed, Pinned};
use as_project::computer::Choice;
use parking_lot::Mutex;
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::events::{AccelFinished, AccelProgress, HardwareChanged};
use crate::state::AppState;

/// In a pack folder: the licence the person agreed to when downloading it.
const LICENCE: &str = ".autoscene-licence";
/// In a pack folder: its files are to go.
const REMOVE: &str = ".autoscene-remove";
/// In the folder of the release this build pins: the person stopped its
/// update, so no start brings it until they ask.
const STOPPED: &str = ".autoscene-update-stopped";

/// How often a removal looks whether the engine let go of the files, and how
/// many times it tries while no job holds them. What is left then goes at
/// the next start.
const LOOK_AGAIN: Duration = Duration::from_millis(250);
const TRIES: u32 = 120;

/// The progress bar moves by this many bytes at least.
const STEP: u64 = 1_000_000;

/// The pack folders under `paths`.
#[derive(Clone, Debug)]
pub struct Packs {
    paths: AppPaths,
}

impl Packs {
    pub fn new(paths: &AppPaths) -> Self {
        Self { paths: paths.clone() }
    }

    /// The folder of the release this build loads.
    fn current(&self) -> PathBuf {
        self.paths.accel_dir(accel::VERSION)
    }

    /// Every pack folder, finished or not.
    fn folders(&self) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(self.paths.runtimes()) else { return Vec::new() };
        entries
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(ACCEL_PREFIX) && e.path().is_dir())
            .map(|e| e.path())
            .collect()
    }

    /// Finished packs of other releases.
    fn older(&self) -> Vec<PathBuf> {
        let current = self.current();
        self.folders().into_iter().filter(|dir| *dir != current && finished(dir)).collect()
    }

    /// The pack to use: this build's release, else an older one until the
    /// update brought this one.
    pub fn usable(&self) -> Option<PathBuf> {
        let current = self.current();
        if finished(&current) { Some(current) } else { self.older().into_iter().next() }
    }

    /// An update of the app pinned a new release of a pack that is here,
    /// whose licence the person agreed to: it downloads without asking,
    /// unless the person stopped that.
    fn update_due(&self, pins: &Pins) -> bool {
        let current = self.current();
        !finished(&current) && !current.join(STOPPED).exists() && self.older().iter().any(|dir| agreed(dir, pins))
    }

    /// Files of a pack are here, finished or not, that aren't on their way out.
    fn any_files(&self) -> bool {
        self.folders().iter().any(|dir| !dir.join(REMOVE).exists())
    }

    /// A pack was here before, in whatever shape: a download now is no first
    /// install.
    fn had_one(&self) -> bool {
        self.folders().iter().any(|dir| once_complete(dir))
    }

    /// The pack to use is on the licence pinned: else it is asked for again.
    fn licence_agreed(&self, pins: &Pins) -> bool {
        finished(&self.current()) || self.usable().is_some_and(|dir| agreed(&dir, pins))
    }

    /// Download what is missing of `pins` and verify it. `progress(done,
    /// total)` counts the bytes that travel. Stamped last: nothing uses the
    /// folder before every file is in it.
    fn download(&self, pins: &Pins, cancel: &CancelToken, mut progress: impl FnMut(u64, u64)) -> ModelResult<()> {
        let dir = self.paths.accel_dir(&pins.version);
        fs::create_dir_all(&dir)?;
        // The person wants it again after all, or its stamp no longer holds.
        let _ = fs::remove_file(dir.join(REMOVE));
        let _ = fs::remove_file(dir.join(STOPPED));
        let _ = fs::remove_file(dir.join(accel::STAMP));
        let store = ModelStore::new(&dir);
        let missing = pins.files.iter().filter(|f| !dir.join(&f.name).is_file());
        store.room_for(missing.map(|f| f.size + f.packed).sum())?;
        let total = pins.download_bytes();
        let mut base = 0;
        progress(base, total);
        for file in &pins.files {
            store.fetch_pinned(&pinned(pins, file), cancel, |done, _| progress((base + done).min(total), total))?;
            base += file.packed;
            progress(base, total);
        }
        fs::write(dir.join(LICENCE), &pins.licence)?;
        let names: Vec<&str> = pins.files.iter().map(|f| f.name.as_str()).collect();
        accel::stamp(&dir, &names, &accel::PRELOAD)?;
        Ok(())
    }

    /// Mark every pack for removal: from now on none is used. Returns them.
    fn mark_removed(&self) -> io::Result<Vec<PathBuf>> {
        let dirs = self.folders();
        for dir in &dirs {
            fs::write(dir.join(REMOVE), "")?;
        }
        Ok(dirs)
    }

    /// Delete `dirs`, marked for removal, once no job needs them: `busy` says
    /// one does, `release` stops an engine left idle with them. Then what
    /// NVIDIA's engine compiled goes too.
    fn delete_when_free(&self, dirs: &[PathBuf], busy: impl Fn() -> bool, release: impl Fn()) {
        let mut tries = 0;
        while tries < TRIES {
            if busy() {
                thread::sleep(LOOK_AGAIN);
                continue;
            }
            tries += 1;
            match dirs.iter().try_for_each(|dir| delete_pack(dir)) {
                Ok(()) => break,
                Err(e) => {
                    if tries == 1 {
                        log::info!("NVIDIA's files can't go yet: {e}");
                    }
                    release();
                    thread::sleep(LOOK_AGAIN);
                }
            }
        }
        if tries == TRIES {
            log::warn!("NVIDIA's files are still in use; they go at the next start");
        }
    }

    /// What NVIDIA's engine compiled for this card.
    fn forget_compiled(&self) {
        remove_tree(&self.paths.cache().join(cache::TRTRTX));
    }

    /// A download that was cancelled leaves nothing behind.
    fn forget_download(&self) {
        remove_tree(&self.current());
    }

    /// A repair that was cancelled keeps the files that were fine, and only
    /// the half-fetched ones go.
    fn forget_parts(&self) {
        let Ok(entries) = fs::read_dir(self.current()) else { return };
        for entry in entries.flatten() {
            if entry.path().extension().is_some_and(|e| e == "part")
                && let Err(e) = fs::remove_file(entry.path())
            {
                log::warn!("removing {}: {e}", as_core::paths::name(&entry.path()));
            }
        }
    }

    /// The person stopped an update: what it fetched goes, and it stays away
    /// until they ask.
    fn stop_update(&self) -> io::Result<()> {
        self.forget_download();
        fs::create_dir_all(self.current())?;
        fs::write(self.current().join(STOPPED), "")
    }

    /// At the start, before any engine uses a pack: what a removal left with
    /// what it compiled, an old release an update replaced, and an old
    /// release's unfinished download. The current release's unfinished
    /// download stays: it resumes.
    fn tidy(&self) {
        let current = self.current();
        let replaced = finished(&current);
        for dir in self.folders() {
            if dir.join(REMOVE).exists() {
                match delete_pack(&dir) {
                    Ok(()) => self.forget_compiled(),
                    Err(e) => log::warn!("removing {}: {e}", as_core::paths::name(&dir)),
                }
            } else if dir != current && (replaced || !accel::installed(&dir)) {
                remove_tree(&dir);
            }
        }
    }
}

/// `dir` holds a pack that may be used: its download finished, and it isn't
/// on its way out.
fn finished(dir: &Path) -> bool {
    accel::installed(dir) && !dir.join(REMOVE).exists()
}

/// `dir`'s download finished once, so what it holds may be worth keeping
/// even if its stamp no longer holds: the licence goes in last but for it.
fn once_complete(dir: &Path) -> bool {
    dir.join(LICENCE).is_file() && !dir.join(REMOVE).exists()
}

/// The person agreed to the licence `pins` names when downloading `dir`.
fn agreed(dir: &Path, pins: &Pins) -> bool {
    fs::read_to_string(dir.join(LICENCE)).is_ok_and(|url| url.trim() == pins.licence)
}

/// The file as the model store fetches it.
fn pinned(pins: &Pins, file: &PinnedFile) -> Pinned {
    Pinned {
        id: format!("nvidia-rtx/{}", file.name),
        file: file.name.clone(),
        urls: vec![pins.url.clone()],
        sha256: file.sha256.clone(),
        size: file.size,
        packed: Packed::ZipMember(file.member.clone()),
        exec: false,
    }
}

/// Delete a pack folder, its [`REMOVE`] mark last: cut short, the next start
/// goes on. A library still loaded stops it there.
fn delete_pack(dir: &Path) -> io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let entry = entry?;
        if entry.file_name() != REMOVE {
            fs::remove_file(entry.path())?;
        }
    }
    fs::remove_dir_all(dir)
}

fn remove_tree(dir: &Path) {
    if let Err(e) = fs::remove_dir_all(dir)
        && e.kind() != io::ErrorKind::NotFound
    {
        log::warn!("removing {}: {e}", as_core::paths::name(dir));
    }
}

/// What the row says after the person cancelled a repair, which can't leave
/// the pack as it was: the stamp went when it began.
const REPAIR_CANCELLED: &str = "The download was cancelled, so analysis runs without NVIDIA acceleration. Try again for the files that are missing.";

/// What the person is told when a download fails.
fn message(e: &ModelError) -> String {
    match e {
        ModelError::Download(_) => {
            "NVIDIA's files couldn't be downloaded. Check the internet connection and try again.".into()
        }
        ModelError::Checksum { .. } => {
            "NVIDIA's files didn't arrive as they should. Try again, and update AutoScene if it happens again.".into()
        }
        ModelError::Changed(_) => {
            "NVIDIA changed its files where AutoScene downloads them from. Update AutoScene to get them.".into()
        }
        ModelError::NoSpace { need, free, .. } => format!(
            "There isn't enough free space for NVIDIA's files. Free up {} MB on the drive of AutoScene's data folder and try again.",
            need.saturating_sub(*free).div_ceil(1_000_000)
        ),
        other => other.to_string(),
    }
}

/// What the row is doing right now.
#[derive(Clone, Default)]
enum Phase {
    #[default]
    Idle,
    Downloading {
        done: u64,
        total: u64,
        cancel: CancelToken,
    },
    Failed(String),
    Removing,
}

/// What decides the row.
struct Seen {
    phase: Phase,
    /// This computer's analysis runs on a card the pack serves.
    offered: bool,
    installed: bool,
    /// The person agreed to the licence pinned now.
    agreed: bool,
    /// It failed too often, or doesn't load.
    stopped: bool,
    /// It lists no device for this card.
    unsupported: bool,
}

/// The first state that applies: Downloading, Download failed, Removing,
/// Available, Not used, Stopped working, Needs a newer driver, On or off.
fn row(seen: Seen) -> AccelRow {
    match seen.phase {
        Phase::Downloading { done, total, .. } => AccelRow::Downloading { done, total },
        Phase::Failed(message) => AccelRow::Failed { message },
        Phase::Removing => AccelRow::Removing,
        Phase::Idle if seen.offered && !(seen.installed && seen.agreed) => AccelRow::Available,
        Phase::Idle if !seen.installed => AccelRow::Hidden,
        Phase::Idle if !seen.offered => AccelRow::NotUsed,
        Phase::Idle if seen.stopped => AccelRow::Stopped,
        Phase::Idle if seen.unsupported => AccelRow::NeedsDriver,
        Phase::Idle => AccelRow::Ready,
    }
}

/// What a download or a removal needs from the app around it: the app is
/// this, and a test stands in for it.
trait Host {
    /// Engines start as the settings and the files now say.
    fn relaunch(&self);
    /// Start an engine now, so the row knows whether the card takes the pack.
    fn warm(&self);
    /// A job runs, on an engine that may hold the files.
    fn busy(&self) -> bool;
    /// The pack in `dir` gets a new chance: failures so far don't count.
    fn renewed(&self, dir: &Path);
    /// `AppSettings::nvidia_acceleration`.
    fn switched_on(&self) -> bool;
    fn switch_on(&self);
    /// This computer's analysis runs on a card the pack serves, as an engine
    /// finds it now.
    fn offered(&self) -> bool;
    /// The row may read otherwise.
    fn changed(&self);
    fn progress(&self, done: u64, total: u64);
    /// A download the person asked for ended; `error` says why it failed.
    fn finished(&self, error: Option<String>);
}

impl Host for AppHandle {
    fn relaunch(&self) {
        self.state::<AppState>().relaunch();
    }

    fn warm(&self) {
        if let Err(e) = self.state::<AppState>().engine.warm(&CancelToken::new()) {
            log::warn!("starting the analysis engine: {}", e.detail());
        }
    }

    fn busy(&self) -> bool {
        self.state::<AppState>().engine.pending()
    }

    fn renewed(&self, dir: &Path) {
        self.state::<AppState>().engine.pack_try_again(dir);
    }

    fn switched_on(&self) -> bool {
        self.state::<AppState>().library.nvidia_acceleration()
    }

    fn switch_on(&self) {
        if let Err(e) = self.state::<AppState>().library.set_nvidia_acceleration(true) {
            log::warn!("switching NVIDIA acceleration on: {}", e.detail());
        }
    }

    fn offered(&self) -> bool {
        let state = self.state::<AppState>();
        match state.engine.probe(&CancelToken::new()) {
            Ok(found) => offered(&found.chosen(&Choice::from_setting(&state.library.gpu()))),
            Err(e) => {
                log::warn!("looking for graphics cards: {}", e.detail());
                false
            }
        }
    }

    fn changed(&self) {
        let _ = HardwareChanged.emit(self);
    }

    fn progress(&self, done: u64, total: u64) {
        let _ = AccelProgress { done, total }.emit(self);
    }

    fn finished(&self, error: Option<String>) {
        let _ = AccelFinished { error }.emit(self);
    }
}

/// NVIDIA's files, and the download or removal under way.
pub struct Accel {
    pub packs: Packs,
    pins: Pins,
    phase: Mutex<Phase>,
}

impl Accel {
    pub fn new(paths: &AppPaths) -> Self {
        Self::with_pins(paths, accel::pins().clone())
    }

    fn with_pins(paths: &AppPaths, pins: Pins) -> Self {
        Self { packs: Packs::new(paths), pins, phase: Mutex::default() }
    }

    /// The pack the next engine starts with, with the switch `on`.
    pub fn pack(&self, on: bool) -> Option<PathBuf> {
        on.then(|| self.packs.usable()).flatten()
    }

    /// What Settings shows when analysis runs on `chosen`, with the switch
    /// `on`, and what the last `engine` made of the pack.
    pub fn info(&self, chosen: &Hardware, on: bool, engine: &Engine) -> AccelInfo {
        let state = engine.accel();
        let row = self.row(offered(chosen), state.as_ref());
        AccelInfo {
            active: on && row == AccelRow::Ready && state == Some(AccelState::Active),
            row,
            installed: self.packs.usable().is_some(),
            removable: self.packs.any_files(),
            download_bytes: self.pins.download_bytes(),
            licence: self.pins.licence.clone(),
            engine: self.pins.engine.clone(),
        }
    }

    /// The row where the card in use is `offered` the pack, and the last
    /// engine made `state` of it. A hidden row can't go on with an unfinished
    /// download, nor remove it, and only a row could: it goes here.
    fn row(&self, offered: bool, state: Option<&AccelState>) -> AccelRow {
        let pack = self.packs.usable();
        let row = row(Seen {
            phase: self.phase.lock().clone(),
            offered,
            installed: pack.is_some(),
            agreed: self.packs.licence_agreed(&self.pins),
            stopped: matches!(state, Some(AccelState::Failed { .. }))
                || pack.as_deref().is_some_and(|dir| pack_failures(dir) >= PACK_FAILURES),
            unsupported: state == Some(&AccelState::Unsupported),
        });
        if row == AccelRow::Hidden {
            self.packs.forget_download();
        }
        row
    }

    fn set(&self, phase: Phase) {
        *self.phase.lock() = phase;
    }

    /// The download reached `at` of `of` bytes.
    fn progressed(&self, at: u64, of: u64) {
        if let Phase::Downloading { done, total, .. } = &mut *self.phase.lock() {
            (*done, *total) = (at, of);
        }
    }

    /// Stop the download under way, if there is one.
    pub fn cancel(&self) {
        if let Phase::Downloading { cancel, .. } = &*self.phase.lock() {
            cancel.cancel();
        }
    }

    /// The row takes up a download, unless a download or a removal runs.
    fn begin(&self, cancel: &CancelToken) -> bool {
        let mut phase = self.phase.lock();
        if matches!(*phase, Phase::Downloading { .. } | Phase::Removing) {
            return false;
        }
        *phase = Phase::Downloading { done: 0, total: self.pins.download_bytes(), cancel: cancel.clone() };
        true
    }

    /// Download the pack, after [`Accel::begin`]. `asked`: the person did,
    /// and hears how it ended; an update that runs by itself says nothing,
    /// and the row stays as it was while the old pack works.
    fn download(&self, host: &impl Host, cancel: &CancelToken, asked: bool) {
        let current = self.packs.current();
        // The stamp goes first: a pack being repaired is no engine's meanwhile.
        let repairing = once_complete(&current);
        // Only a first install says it is on: else the switch is the person's.
        let first = asked && !self.packs.had_one();
        let mut shown = 0;
        let result = self.packs.download(&self.pins, cancel, |done, total| {
            self.progressed(done, total);
            if done.abs_diff(shown) >= STEP || done == total {
                shown = done;
                host.progress(done, total);
            }
            if repairing && done == 0 {
                host.relaunch();
            }
        });
        match result {
            Ok(()) => {
                // New files: what failed this session was the old ones.
                host.renewed(&current);
                if first {
                    host.switch_on();
                }
                self.set(Phase::Idle);
                host.relaunch();
                host.changed();
                if asked {
                    host.finished(None);
                }
                let older = self.packs.older();
                if !older.is_empty() {
                    self.packs.delete_when_free(&older, || host.busy(), || host.relaunch());
                }
                // The row says at once whether the card takes it.
                host.warm();
            }
            Err(ModelError::Cancelled) => {
                if !asked {
                    if let Err(e) = self.packs.stop_update() {
                        log::warn!("noting that the update was stopped: {e}");
                    }
                    self.set(Phase::Idle);
                } else if repairing {
                    self.packs.forget_parts();
                    self.set(Phase::Failed(REPAIR_CANCELLED.into()));
                } else {
                    self.packs.forget_download();
                    self.set(Phase::Idle);
                }
                host.changed();
            }
            Err(e) if !asked => {
                log::warn!("updating NVIDIA's files, again at the next start: {}", e.detail());
                self.set(Phase::Idle);
                host.changed();
            }
            Err(e) => {
                log::warn!("downloading NVIDIA's files: {}", e.detail());
                let told = message(&e);
                self.set(Phase::Failed(told.clone()));
                host.changed();
                host.finished(Some(told));
            }
        }
    }

    /// The row takes up a removal: no engine starts with a pack from now on.
    /// Returns the packs to delete, unless a download or a removal runs.
    fn begin_removal(&self, host: &impl Host) -> AppResult<Option<Vec<PathBuf>>> {
        {
            let mut phase = self.phase.lock();
            if matches!(*phase, Phase::Downloading { .. } | Phase::Removing) {
                return Ok(None);
            }
            *phase = Phase::Removing;
        }
        let dirs = self.packs.mark_removed().map_err(|e| {
            self.set(Phase::Idle);
            log::warn!("removing NVIDIA's files: {e}");
            AppError::new(ErrorCode::Io, "NVIDIA's files couldn't be removed.")
        })?;
        // An idle engine stops here; a busy one finishes its job first.
        host.relaunch();
        host.changed();
        Ok(Some(dirs))
    }

    /// `dirs`, marked for removal, and what they compiled go as soon as no
    /// engine holds them. What failed goes with them: a pack downloaded
    /// after this starts afresh.
    fn finish_removal(&self, host: &impl Host, dirs: &[PathBuf]) {
        self.packs.delete_when_free(dirs, || host.busy(), || host.relaunch());
        self.packs.forget_compiled();
        for dir in dirs {
            host.renewed(dir);
        }
        self.set(Phase::Idle);
        host.changed();
    }

    /// Give the pack another chance after it stopped working. Whether it has
    /// to come again, as it no longer matches its stamp.
    fn try_again(&self, host: &impl Host) -> bool {
        let Some(dir) = self.packs.usable() else { return false };
        host.renewed(&dir);
        if let Err(e) = accel::verify(&dir) {
            log::warn!("NVIDIA's files don't match their record: {e}");
            return true;
        }
        host.relaunch();
        host.changed();
        host.warm();
        false
    }

    /// At the start: clear what a removal or an update left. Whether the
    /// release this build pins is to come, an older one being here and on,
    /// for a card that uses it. Asked last: finding the card starts an
    /// engine, which only a start with an update due should pay for.
    fn upkeep(&self, host: &impl Host) -> bool {
        self.packs.tidy();
        host.switched_on() && self.packs.update_due(&self.pins) && host.offered()
    }
}

/// Whether analysis on `chosen` is offered the pack, on this computer.
fn offered(chosen: &Hardware) -> bool {
    accel::offered(std::env::consts::OS, std::env::consts::ARCH, chosen.gpu())
}

/// Download the pack, unless a download or a removal runs. `asked`: the
/// person did, and hears how it ended.
pub fn install(app: &AppHandle, asked: bool) {
    let cancel = CancelToken::new();
    if !app.state::<AppState>().accel.begin(&cancel) {
        return;
    }
    app.changed();
    let app = app.clone();
    thread::spawn(move || app.state::<AppState>().accel.download(&app, &cancel, asked));
}

/// Remove the pack: no engine starts with it from now on, and its files and
/// what it compiled go as soon as none holds them.
pub fn remove(app: &AppHandle) -> AppResult<()> {
    if let Some(dirs) = app.state::<AppState>().accel.begin_removal(app)? {
        let app = app.clone();
        thread::spawn(move || app.state::<AppState>().accel.finish_removal(&app, &dirs));
    }
    Ok(())
}

/// Give the pack another chance after it stopped working: the engine starts
/// with it again, and a pack that no longer matches its stamp is downloaded
/// again, which brings back only the files that changed.
pub fn try_again(app: &AppHandle) {
    if app.state::<AppState>().accel.try_again(app) {
        install(app, true);
    }
}

/// Open NVIDIA's third-party notices, in the pack that is here.
pub fn open_notices(state: &AppState) -> AppResult<()> {
    let opened = state.accel.packs.usable().map(|dir| dir.join(accel::NOTICES)).map(|file| {
        tauri_plugin_opener::open_path(file.display().to_string(), None::<&str>).map_err(|e| log::warn!("{e}"))
    });
    match opened {
        Some(Ok(())) => Ok(()),
        _ => Err(AppError::new(ErrorCode::Io, "NVIDIA's notices couldn't be opened.")),
    }
}

/// At the start: clear what a removal or an update left, and bring the
/// release this build pins when an older one is here and switched on.
pub fn upkeep(app: &AppHandle) {
    if app.state::<AppState>().accel.upkeep(app) {
        install(app, false);
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Cursor, Write};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

    use sha2::{Digest, Sha256};

    use super::*;

    /// Files named like the pack's, so the store's stamp is one it may load.
    fn files(seed: u8) -> Vec<(String, Vec<u8>)> {
        accel::PRELOAD
            .iter()
            .copied()
            .chain([accel::PLUGIN, accel::NOTICES])
            .enumerate()
            .map(|(i, name)| (name.to_string(), noise(seed as u32 * 7919 + i as u32 + 1, 60_000 + i * 5_000)))
            .collect()
    }

    /// Bytes that don't repeat.
    fn noise(mut state: u32, len: usize) -> Vec<u8> {
        (0..len)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 24) as u8
            })
            .collect()
    }

    /// A wheel of `files` (stored, so a member's packed bytes are its own),
    /// and the pins of it at `url`.
    fn wheel(files: &[(String, Vec<u8>)]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, bytes) in files {
            zip.start_file(format!("pkg/{name}"), stored).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn pins(url: &str, files: &[(String, Vec<u8>)]) -> Pins {
        Pins {
            version: accel::VERSION.into(),
            engine: "1.7.1".into(),
            licence: "https://example.test/licence".into(),
            url: url.into(),
            size: 0,
            sha256: String::new(),
            files: files
                .iter()
                .map(|(name, bytes)| PinnedFile {
                    name: name.clone(),
                    member: format!("pkg/{name}"),
                    packed: bytes.len() as u64,
                    size: bytes.len() as u64,
                    sha256: Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect(),
                })
                .collect(),
        }
    }

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

    /// A local server for one file that answers byte ranges, counting what it sent.
    fn serve(data: Vec<u8>) -> (String, Arc<AtomicU64>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/wheel.whl", listener.local_addr().unwrap());
        let sent = Arc::new(AtomicU64::new(0));
        let counted = sent.clone();
        thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { return };
                let mut reader = BufReader::new(conn.try_clone().unwrap());
                let mut request = String::new();
                let _ = reader.read_line(&mut request);
                let range = read_range(&mut reader, data.len() as u64);
                let (status, body, extra) = match range {
                    Some((a, b)) => (
                        "206 Partial Content",
                        &data[a as usize..=b as usize],
                        format!("Content-Range: bytes {a}-{b}/{}\r\n", data.len()),
                    ),
                    None => ("200 OK", &data[..], String::new()),
                };
                counted.fetch_add(body.len() as u64, Ordering::Relaxed);
                let head =
                    format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n", body.len());
                let _ = conn.write_all(head.as_bytes()).and_then(|()| conn.write_all(body));
            }
        });
        (url, sent)
    }

    /// Packs in a data folder of their own.
    fn packs() -> (tempfile::TempDir, Packs) {
        let dir = tempfile::tempdir().unwrap();
        let packs = Packs::new(&AppPaths::at(dir.path()));
        (dir, packs)
    }

    fn fetch(packs: &Packs, pins: &Pins) -> ModelResult<()> {
        packs.download(pins, &CancelToken::new(), |_, _| {})
    }

    /// What a release with another TensorRT than this build's loads.
    const OLD_LOAD: [&str; 4] =
        ["cudart64_12.dll", "tensorrt_rtx_1_6.dll", "tensorrt_onnxparser_rtx_1_6.dll", "tensorrt_plugins.dll"];

    /// A finished pack of another release, agreed under `licence`, as that
    /// release's download left it.
    fn old_pack(packs: &Packs, licence: &str) -> PathBuf {
        let dir = packs.paths.accel_dir("0.4.0");
        fs::create_dir_all(&dir).unwrap();
        let names: Vec<&str> = OLD_LOAD.iter().copied().chain([accel::PLUGIN, accel::NOTICES]).collect();
        for name in &names {
            fs::write(dir.join(name), format!("old {name}")).unwrap();
        }
        accel::stamp(&dir, &names, &OLD_LOAD).unwrap();
        fs::write(dir.join(LICENCE), licence).unwrap();
        dir
    }

    /// The app around a download, noting what it was told in order.
    #[derive(Default)]
    struct Fake {
        calls: Mutex<Vec<String>>,
        on: AtomicBool,
        /// Analysis runs on the processor, or on a card the pack doesn't serve.
        elsewhere: AtomicBool,
        /// Cancelled at the first progress the row hears.
        stop: Option<CancelToken>,
    }

    impl Fake {
        fn on() -> Self {
            Self { on: AtomicBool::new(true), ..Self::default() }
        }

        fn note(&self, call: impl Into<String>) {
            self.calls.lock().push(call.into());
        }

        /// What it was told since the last look, but for the progress.
        fn calls(&self) -> Vec<String> {
            std::mem::take(&mut *self.calls.lock())
        }
    }

    impl Host for Fake {
        fn relaunch(&self) {
            self.note("relaunch");
        }

        fn warm(&self) {
            self.note("warm");
        }

        fn busy(&self) -> bool {
            false
        }

        fn renewed(&self, dir: &Path) {
            self.note(format!("renewed {}", as_core::paths::name(dir)));
        }

        fn switched_on(&self) -> bool {
            self.on.load(Ordering::Relaxed)
        }

        fn switch_on(&self) {
            self.on.store(true, Ordering::Relaxed);
            self.note("switch on");
        }

        fn offered(&self) -> bool {
            !self.elsewhere.load(Ordering::Relaxed)
        }

        fn changed(&self) {
            self.note("changed");
        }

        fn progress(&self, _: u64, _: u64) {
            if let Some(cancel) = &self.stop {
                cancel.cancel();
            }
        }

        fn finished(&self, error: Option<String>) {
            self.note(format!("finished {}", error.as_deref().unwrap_or("well")));
        }
    }

    /// The row and download of `pins`, in a data folder of their own.
    fn flow(pins: Pins) -> (tempfile::TempDir, Accel) {
        let dir = tempfile::tempdir().unwrap();
        let accel = Accel::with_pins(&AppPaths::at(dir.path()), pins);
        (dir, accel)
    }

    /// A download from start to end, as the row starts one.
    fn run(accel: &Accel, host: &Fake, asked: bool) {
        let cancel = host.stop.clone().unwrap_or_default();
        assert!(accel.begin(&cancel));
        accel.download(host, &cancel, asked);
    }

    /// The row with the card served and nothing reported by an engine.
    fn row_now(accel: &Accel) -> AccelRow {
        accel.row(true, None)
    }

    fn at(calls: &[String], call: &str) -> usize {
        calls.iter().position(|c| c.starts_with(call)).unwrap_or_else(|| panic!("no {call} in {calls:?}"))
    }

    fn pack_name(accel: &Accel) -> String {
        as_core::paths::name(&accel.packs.current()).into_owned()
    }

    #[test]
    fn every_file_installs_and_verifies() {
        let files = files(1);
        let (url, _) = serve(wheel(&files));
        let (_dir, packs) = packs();
        let pins = pins(&url, &files);
        assert_eq!(packs.usable(), None);

        let mut seen = Vec::new();
        packs.download(&pins, &CancelToken::new(), |done, total| seen.push((done, total))).unwrap();
        let total = pins.download_bytes();
        assert!(seen.iter().all(|&(_, t)| t == total), "one total for the whole download");
        assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0), "the bar never goes back");
        assert_eq!((seen[0].0, seen.last().unwrap().0), (0, total));

        let dir = packs.usable().expect("a finished pack");
        assert_eq!(dir, packs.paths.accel_dir(accel::VERSION));
        for (name, bytes) in &files {
            assert_eq!(&fs::read(dir.join(name)).unwrap(), bytes, "{name}");
        }
        accel::verify(&dir).expect("the stamp holds");
        assert!(agreed(&dir, &pins), "the licence agreed to is kept with it");
    }

    #[test]
    fn a_file_unlike_its_pin_is_refused_and_nothing_is_stamped() {
        let files = files(2);
        let (url, _) = serve(wheel(&files));

        let (_dir, packs_a) = packs();
        let mut wrong_hash = pins(&url, &files);
        wrong_hash.files[1].sha256 = "0".repeat(64);
        let e = fetch(&packs_a, &wrong_hash).unwrap_err();
        assert!(matches!(e, ModelError::Checksum { .. }), "{e:?}");
        assert!(message(&e).contains("didn't arrive as they should"), "{}", message(&e));
        assert_eq!(packs_a.usable(), None);

        let (_dir, packs_b) = packs();
        let mut wrong_size = pins(&url, &files);
        wrong_size.files[2].size += 1;
        let e = fetch(&packs_b, &wrong_size).unwrap_err();
        assert!(matches!(e, ModelError::Changed(_)), "{e:?}");
        assert!(message(&e).contains("Update AutoScene"), "{}", message(&e));
        assert_eq!(packs_b.usable(), None);
    }

    #[test]
    fn a_file_resumes_where_it_stopped() {
        let mut files = files(3);
        files[0].1 = noise(3, 2_000_000);
        let (url, sent) = serve(wheel(&files));
        let pins = pins(&url, &files);
        let (_whole, whole) = packs();
        fetch(&whole, &pins).unwrap();
        let everything = sent.swap(0, Ordering::Relaxed);

        let (_dir, packs) = packs();
        // The first file's first half was there from an interrupted try.
        let dir = packs.paths.accel_dir(accel::VERSION);
        fs::create_dir_all(&dir).unwrap();
        let (name, bytes) = &files[0];
        let half = bytes.len() / 2;
        fs::write(dir.join(format!("{name}.part")), &bytes[..half]).unwrap();

        fetch(&packs, &pins).unwrap();
        accel::verify(&packs.usable().unwrap()).unwrap();
        assert_eq!(&fs::read(dir.join(name)).unwrap(), bytes);
        assert_eq!(everything - sent.load(Ordering::Relaxed), half as u64, "the part wasn't fetched again");

        // A second run finds everything in place and fetches nothing.
        let before = sent.load(Ordering::Relaxed);
        fetch(&packs, &pins).unwrap();
        assert_eq!(sent.load(Ordering::Relaxed), before);
    }

    #[test]
    fn a_pack_that_no_longer_matches_gets_back_only_what_changed() {
        let files = files(11);
        let (url, sent) = serve(wheel(&files));
        let (_dir, packs) = packs();
        let pins = pins(&url, &files);
        fetch(&packs, &pins).unwrap();
        let dir = packs.usable().unwrap();
        // Another file of the same size: a failing disk, a restored backup.
        let (name, bytes) = &files[1];
        fs::write(dir.join(name), vec![0u8; bytes.len()]).unwrap();
        assert!(accel::verify(&dir).is_err());

        sent.store(0, Ordering::Relaxed);
        fetch(&packs, &pins).unwrap();
        accel::verify(&dir).expect("as pinned again");
        assert_eq!(&fs::read(dir.join(name)).unwrap(), bytes);
        let total: u64 = pins.download_bytes();
        assert!(sent.load(Ordering::Relaxed) < total / 2, "only that file travelled");
    }

    #[test]
    fn cancelling_stops_within_a_file_and_leaves_nothing() {
        let mut files = files(4);
        files[0].1 = noise(4, 8_000_000);
        let (url, _) = serve(wheel(&files));
        let (_dir, packs) = packs();
        let pins = pins(&url, &files);
        let cancel = CancelToken::new();
        let first = pins.files[0].packed;
        let r = packs.download(&pins, &cancel, |done, _| {
            if done > 0 && done < first {
                cancel.cancel();
            }
        });
        assert!(matches!(r, Err(ModelError::Cancelled)), "{r:?}");
        assert_eq!(packs.usable(), None, "no stamp");
        packs.forget_download();
        assert!(!packs.current().exists());
    }

    #[test]
    fn a_drive_without_room_says_so_before_anything_travels() {
        let files = files(5);
        let (url, sent) = serve(wheel(&files));
        let (_dir, packs) = packs();
        let mut pins = pins(&url, &files);
        // The last file is the one that doesn't fit: the first would travel
        // if the room were only looked at file by file.
        pins.files.last_mut().unwrap().size = u64::MAX / 4;
        let e = fetch(&packs, &pins).unwrap_err();
        assert!(matches!(e, ModelError::NoSpace { .. }), "{e:?}");
        assert!(message(&e).starts_with("There isn't enough free space for NVIDIA's files."), "{}", message(&e));
        assert_eq!(sent.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn removing_waits_for_the_engine_then_deletes_only_its_folder_and_cache() {
        let files = files(6);
        let (url, _) = serve(wheel(&files));
        let (_dir, packs) = packs();
        fetch(&packs, &pins(&url, &files)).unwrap();
        let pack = packs.usable().unwrap();
        // What stays: other runtimes, and what Core ML compiled.
        let runtimes = packs.paths.runtimes();
        fs::write(runtimes.join("keep.dll"), "other").unwrap();
        let cache = packs.paths.cache();
        for family in [cache::TRTRTX, cache::COREML] {
            fs::create_dir_all(cache.join(family).join("key")).unwrap();
        }

        let dirs = packs.mark_removed().unwrap();
        assert_eq!(dirs, std::slice::from_ref(&pack));
        assert_eq!(packs.usable(), None, "no engine starts with it from here on");

        // A job runs on the engine: nothing goes until it is done.
        let looks = AtomicU32::new(0);
        let busy = || {
            let n = looks.fetch_add(1, Ordering::Relaxed);
            assert!(pack.join(accel::PLUGIN).is_file(), "gone while the engine had it");
            n < 3
        };
        packs.delete_when_free(&dirs, busy, || panic!("the job's engine stops by itself"));
        assert_eq!(looks.load(Ordering::Relaxed), 4);
        packs.forget_compiled();
        assert!(!pack.exists() && !cache.join(cache::TRTRTX).exists());
        assert!(runtimes.join("keep.dll").is_file() && cache.join(cache::COREML).join("key").is_dir());
    }

    /// Windows keeps a file open as a loaded library keeps its own.
    #[cfg(windows)]
    #[test]
    fn an_idle_engine_holding_the_files_is_stopped_first() {
        use std::os::windows::fs::OpenOptionsExt;

        let files = files(7);
        let (url, _) = serve(wheel(&files));
        let (_dir, packs) = packs();
        fetch(&packs, &pins(&url, &files)).unwrap();
        let pack = packs.usable().unwrap();
        // Share nothing, not even deleting: as a loaded library does.
        let held = fs::OpenOptions::new().read(true).share_mode(0).open(pack.join(accel::PLUGIN)).unwrap();
        let dirs = packs.mark_removed().unwrap();
        let holding = Mutex::new(Some(held));
        packs.delete_when_free(&dirs, || false, || drop(holding.lock().take()));
        assert!(holding.lock().is_none(), "the engine was stopped");
        assert!(!pack.exists());
    }

    #[test]
    fn a_removal_cut_short_finishes_at_the_next_start() {
        let files = files(8);
        let (url, _) = serve(wheel(&files));
        let (_dir, packs) = packs();
        fetch(&packs, &pins(&url, &files)).unwrap();
        let pack = packs.usable().unwrap();
        let compiled = packs.paths.cache().join(cache::TRTRTX).join("key");
        fs::create_dir_all(&compiled).unwrap();
        packs.mark_removed().unwrap();
        // The app quit before the engine let go: it isn't used, and it goes,
        // with what it compiled.
        assert_eq!(packs.usable(), None);
        packs.tidy();
        assert!(!pack.exists());
        assert!(!compiled.exists());
    }

    #[test]
    fn a_start_with_nothing_to_remove_keeps_what_was_compiled() {
        let (_dir, packs) = packs();
        let old = old_pack(&packs, "https://example.test/licence");
        let compiled = packs.paths.cache().join(cache::TRTRTX).join("key");
        fs::create_dir_all(&compiled).unwrap();
        packs.tidy();
        assert!(old.exists() && compiled.exists());
    }

    #[test]
    fn a_download_under_way_is_kept_for_the_next_try_but_an_old_releases_is_not() {
        let (_dir, packs) = packs();
        let current = packs.current();
        fs::create_dir_all(&current).unwrap();
        fs::write(current.join("tensorrt_rtx_1_7.dll.part"), "half").unwrap();
        let stale = packs.paths.accel_dir("0.3.0");
        fs::create_dir_all(&stale).unwrap();
        fs::write(stale.join("a.dll.part"), "half").unwrap();
        packs.tidy();
        assert!(current.join("tensorrt_rtx_1_7.dll.part").is_file());
        assert!(!stale.exists());
    }

    #[test]
    fn an_update_downloads_the_new_pack_and_keeps_the_old_until_it_verifies() {
        let files = files(9);
        let (url, _) = serve(wheel(&files));
        let (_dir, packs) = packs();
        let good = pins(&url, &files);
        let old = old_pack(&packs, &good.licence);
        // Working means the engine takes it: this build's own list of
        // libraries has other names, so the old one loads what it stamped.
        let keeps_working = || assert_eq!(accel::verify(&old).expect("the old pack loads").load, OLD_LOAD);
        assert_eq!(packs.usable().as_ref(), Some(&old), "it keeps working");
        keeps_working();
        assert!(packs.update_due(&good));

        // The new one fails: the old one is still the one in use.
        let mut bad = good.clone();
        bad.files[0].sha256 = "0".repeat(64);
        fetch(&packs, &bad).unwrap_err();
        assert_eq!(packs.usable().as_ref(), Some(&old));
        keeps_working();

        fetch(&packs, &good).unwrap();
        assert_eq!(packs.usable(), Some(packs.current()));
        assert_eq!(accel::verify(&packs.current()).unwrap().load, accel::PRELOAD);
        assert_eq!(packs.older(), std::slice::from_ref(&old), "not gone yet: an engine may hold it");
        keeps_working();
        assert!(!packs.update_due(&good));
        packs.tidy();
        assert!(!old.exists());
        assert_eq!(packs.usable(), Some(packs.current()));
    }

    #[test]
    fn a_new_licence_asks_again_and_the_old_pack_works_meanwhile() {
        let (_dir, packs) = packs();
        let pins = pins("http://unused.test/", &files(10));
        let old = old_pack(&packs, "https://example.test/the-old-licence");
        assert!(!packs.update_due(&pins), "nothing downloads without the person");
        assert!(!packs.licence_agreed(&pins));
        assert_eq!(packs.usable(), Some(old));
    }

    #[test]
    fn a_first_download_is_on_and_a_new_chance_before_any_engine_starts() {
        let files = files(12);
        let (url, _) = serve(wheel(&files));
        let (_dir, accel) = flow(pins(&url, &files));
        let host = Fake::default();
        assert_eq!(row_now(&accel), AccelRow::Available);

        run(&accel, &host, true);
        let calls = host.calls();
        assert!(host.switched_on());
        assert_eq!(row_now(&accel), AccelRow::Ready);
        // A failure earlier this session would otherwise leave the new pack out.
        assert!(at(&calls, "renewed") < at(&calls, "relaunch"), "{calls:?}");
        assert!(calls.contains(&format!("renewed {}", pack_name(&accel))), "{calls:?}");
        // The settings are read again on the news, so the switch goes first.
        assert!(at(&calls, "switch on") < at(&calls, "finished well"), "{calls:?}");
        assert!(at(&calls, "relaunch") < at(&calls, "warm"), "{calls:?}");
        assert_eq!(calls.last().map(String::as_str), Some("warm"), "{calls:?}");
    }

    #[test]
    fn a_download_that_runs_blocks_another_and_a_removal() {
        let (_dir, accel) = flow(pins("http://unused.test/", &files(13)));
        let host = Fake::default();
        assert!(accel.begin(&CancelToken::new()));
        assert!(!accel.begin(&CancelToken::new()));
        assert!(accel.begin_removal(&host).unwrap().is_none());
        assert!(host.calls().is_empty());
    }

    #[test]
    fn a_failure_the_person_asked_for_says_why_in_the_row_and_the_news() {
        let files = files(14);
        let (url, _) = serve(wheel(&files));
        let mut pins = pins(&url, &files);
        pins.files[1].sha256 = "0".repeat(64);
        let (_dir, accel) = flow(pins);
        let host = Fake::default();

        run(&accel, &host, true);
        let calls = host.calls();
        let AccelRow::Failed { message } = row_now(&accel) else { panic!("{:?}", row_now(&accel)) };
        assert_eq!(calls, ["changed".to_string(), format!("finished {message}")]);
        assert!(!host.switched_on());
    }

    #[test]
    fn an_update_leaves_the_switch_alone_and_keeps_quiet() {
        let files = files(15);
        let (url, _) = serve(wheel(&files));
        let (_dir, accel) = flow(pins(&url, &files));
        let old = old_pack(&accel.packs, &accel.pins.licence);
        // Switched off while it ran.
        let host = Fake::default();

        run(&accel, &host, false);
        let calls = host.calls();
        assert!(!host.switched_on());
        assert!(calls.iter().all(|c| c != "switch on" && !c.starts_with("finished")), "{calls:?}");
        assert!(at(&calls, "renewed") < at(&calls, "relaunch"), "{calls:?}");
        assert_eq!(accel.packs.usable(), Some(accel.packs.current()));
        assert!(!old.exists(), "no engine held it, so it went");
    }

    #[test]
    fn an_update_that_fails_keeps_the_working_row_and_comes_again_at_the_next_start() {
        let files = files(16);
        let (url, _) = serve(wheel(&files));
        let mut bad = pins(&url, &files);
        bad.files[1].sha256 = "0".repeat(64);
        let (_dir, accel) = flow(bad);
        let old = old_pack(&accel.packs, &accel.pins.licence);
        let host = Fake::on();
        assert!(accel.upkeep(&host), "the release this build pins is due");

        run(&accel, &host, false);
        assert_eq!(host.calls(), ["changed"], "no news of it");
        assert_eq!(row_now(&accel), AccelRow::Ready, "the switch and Remove stay");
        assert_eq!(accel.pack(true), Some(old));
        assert!(accel.upkeep(&host));
        assert!(!accel.upkeep(&Fake::default()), "not when it is switched off");
        // Processor only, or the other card: nothing would load what it fetched.
        let elsewhere = Fake { elsewhere: AtomicBool::new(true), ..Fake::on() };
        assert!(!accel.upkeep(&elsewhere), "not for a card that doesn't use it");
    }

    #[test]
    fn an_update_the_person_stopped_stays_stopped_until_they_ask() {
        let mut files = files(17);
        files[0].1 = noise(17, 8_000_000);
        let (url, _) = serve(wheel(&files));
        let (_dir, accel) = flow(pins(&url, &files));
        let old = old_pack(&accel.packs, &accel.pins.licence);
        let stopping = Fake { on: AtomicBool::new(true), stop: Some(CancelToken::new()), ..Fake::default() };

        run(&accel, &stopping, false);
        assert_eq!(stopping.calls(), ["changed"]);
        assert_eq!(row_now(&accel), AccelRow::Ready, "the old pack goes on");
        assert_eq!(accel.pack(true), Some(old.clone()));
        assert!(!accel.packs.current().join(&accel.pins.files[0].name).exists(), "what it fetched went");
        let host = Fake::on();
        assert!(!accel.upkeep(&host), "no start brings it");
        assert!(accel.packs.current().join(STOPPED).exists(), "the next start knows too");

        // The person asks, by Agree and download or a repair of what is here.
        run(&accel, &host, true);
        assert_eq!(accel.pack(true), Some(accel.packs.current()));
        assert!(!accel.packs.current().join(STOPPED).exists());
    }

    #[test]
    fn a_repair_leaves_the_switch_alone_and_brings_the_pack_back() {
        let files = files(18);
        let (url, _) = serve(wheel(&files));
        let (_dir, accel) = flow(pins(&url, &files));
        let first = Fake::default();
        run(&accel, &first, true);
        let dir = accel.packs.usable().unwrap();
        let (name, bytes) = &files[1];
        fs::write(dir.join(name), vec![0u8; bytes.len()]).unwrap();

        // Switched off, and it stopped working.
        let host = Fake::default();
        assert!(accel.try_again(&host), "it has to come again");
        assert_eq!(host.calls(), [format!("renewed {}", pack_name(&accel))]);
        run(&accel, &host, true);
        let calls = host.calls();
        accel::verify(&dir).expect("as pinned again");
        assert!(!host.switched_on(), "{calls:?}");
        assert!(at(&calls, "relaunch") < at(&calls, "renewed"), "the engine leaves it first: {calls:?}");
    }

    #[test]
    fn trying_again_with_files_that_match_only_starts_an_engine() {
        let files = files(19);
        let (url, sent) = serve(wheel(&files));
        let (_dir, accel) = flow(pins(&url, &files));
        run(&accel, &Fake::default(), true);
        sent.store(0, Ordering::Relaxed);

        let host = Fake::default();
        assert!(!accel.try_again(&host));
        assert_eq!(
            host.calls(),
            [format!("renewed {}", pack_name(&accel)), "relaunch".into(), "changed".into(), "warm".into()]
        );
        assert_eq!(sent.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn nothing_here_to_try_again_does_nothing() {
        let (_dir, accel) = flow(pins("http://unused.test/", &files(20)));
        let host = Fake::default();
        assert!(!accel.try_again(&host));
        assert!(host.calls().is_empty());
    }

    #[test]
    fn cancelling_a_repair_keeps_the_good_files_and_the_row_can_finish_it() {
        let mut files = files(21);
        files[0].1 = noise(21, 8_000_000);
        let (url, sent) = serve(wheel(&files));
        let (_dir, accel) = flow(pins(&url, &files));
        run(&accel, &Fake::default(), true);
        let dir = accel.packs.usable().unwrap();
        // The biggest file is the damaged one, so the download stops in it.
        let (name, bytes) = &files[0];
        fs::write(dir.join(name), vec![0u8; bytes.len()]).unwrap();

        let stopping = Fake { stop: Some(CancelToken::new()), ..Fake::default() };
        run(&accel, &stopping, true);
        assert_eq!(row_now(&accel), AccelRow::Failed { message: REPAIR_CANCELLED.into() });
        assert!(accel.packs.any_files(), "Remove stays");
        for (name, _) in &files[1..] {
            assert!(dir.join(name).is_file(), "{name} was fine");
        }
        let parts = fs::read_dir(&dir).unwrap().flatten().filter(|e| e.path().extension().is_some_and(|x| x == "part"));
        assert_eq!(parts.count(), 0);

        // Try again in the row: only the damaged file travels, and being
        // cancelled once more wouldn't delete the rest either.
        sent.store(0, Ordering::Relaxed);
        run(&accel, &Fake::default(), true);
        accel::verify(&dir).expect("as pinned again");
        let others: u64 = files[1..].iter().map(|(_, b)| b.len() as u64).sum();
        assert!(sent.load(Ordering::Relaxed) < bytes.len() as u64 + others / 2, "the others stayed");
    }

    #[test]
    fn cancelling_a_first_download_leaves_nothing_and_the_row_as_it_was() {
        let mut files = files(22);
        files[0].1 = noise(22, 8_000_000);
        let (url, _) = serve(wheel(&files));
        let (_dir, accel) = flow(pins(&url, &files));
        let stopping = Fake { stop: Some(CancelToken::new()), ..Fake::default() };
        run(&accel, &stopping, true);
        assert_eq!(stopping.calls(), ["changed"], "it says nothing else: the person did it");
        assert_eq!(row_now(&accel), AccelRow::Available);
        assert!(!accel.packs.current().exists());
    }

    #[test]
    fn removing_stops_the_engine_first_and_gives_the_next_pack_a_new_chance() {
        let files = files(23);
        let (url, _) = serve(wheel(&files));
        let (_dir, accel) = flow(pins(&url, &files));
        run(&accel, &Fake::default(), true);
        let host = Fake::default();

        let dirs = accel.begin_removal(&host).unwrap().expect("nothing else runs");
        assert_eq!(host.calls(), ["relaunch", "changed"]);
        assert_eq!(row_now(&accel), AccelRow::Removing);
        assert_eq!(accel.pack(true), None, "the launch that followed has no pack");
        assert!(accel.begin_removal(&host).unwrap().is_none());
        assert!(!accel.begin(&CancelToken::new()));

        accel.finish_removal(&host, &dirs);
        assert_eq!(host.calls(), [format!("renewed {}", pack_name(&accel)), "changed".into()]);
        assert_eq!(row_now(&accel), AccelRow::Available);
        assert!(!accel.packs.current().exists());

        // The switch was the person's before; a pack that comes anew is on.
        run(&accel, &host, true);
        assert!(host.switched_on());
    }

    #[test]
    fn a_new_licence_or_a_half_download_can_still_be_removed() {
        let (_dir, accel) = flow(pins("http://unused.test/", &files(24)));
        assert!(!accel.packs.any_files());

        // Declining the new licence: Available, with the old pack still used.
        old_pack(&accel.packs, "https://example.test/the-old-licence");
        assert_eq!(row_now(&accel), AccelRow::Available);
        assert!(accel.packs.any_files());
        accel.packs.mark_removed().unwrap();
        assert_eq!(accel.pack(true), None);
        assert!(!accel.packs.any_files(), "on its way out already");
        accel.packs.tidy();

        // Half a download, after a restart.
        let current = accel.packs.current();
        fs::create_dir_all(&current).unwrap();
        fs::write(current.join("tensorrt_rtx_1_7.dll.part"), "half").unwrap();
        assert_eq!(row_now(&accel), AccelRow::Available);
        assert!(accel.packs.any_files());
    }

    #[test]
    fn half_a_download_goes_where_no_row_can_show_it() {
        let (_dir, accel) = flow(pins("http://unused.test/", &files(25)));
        let current = accel.packs.current();
        fs::create_dir_all(&current).unwrap();
        fs::write(current.join("tensorrt_rtx_1_7.dll.part"), "half").unwrap();
        assert_eq!(accel.row(false, None), AccelRow::Hidden);
        assert!(!current.exists());
    }

    #[test]
    fn a_pack_not_in_use_here_keeps_its_row_and_its_files() {
        let (_dir, accel) = flow(pins("http://unused.test/", &files(26)));
        let old = old_pack(&accel.packs, &accel.pins.licence);
        assert_eq!(accel.row(false, None), AccelRow::NotUsed);
        assert!(old.exists());
    }

    fn seen() -> Seen {
        Seen { phase: Phase::Idle, offered: true, installed: true, agreed: true, stopped: false, unsupported: false }
    }

    #[test]
    fn the_row_shows_the_first_state_that_applies() {
        let downloading = || Phase::Downloading { done: 1, total: 2, cancel: CancelToken::new() };
        let failed = || Phase::Failed("Why".into());
        assert_eq!(row(seen()), AccelRow::Ready);
        // Every state's condition at once: each outranks those after it.
        let all = || Seen { offered: false, installed: false, stopped: true, unsupported: true, ..seen() };
        assert_eq!(row(Seen { phase: downloading(), ..all() }), AccelRow::Downloading { done: 1, total: 2 });
        assert_eq!(row(Seen { phase: failed(), ..all() }), AccelRow::Failed { message: "Why".into() });
        assert_eq!(row(Seen { phase: Phase::Removing, ..all() }), AccelRow::Removing);
        assert_eq!(row(Seen { offered: true, ..all() }), AccelRow::Available);
        assert_eq!(row(Seen { offered: true, installed: true, agreed: false, ..all() }), AccelRow::Available);
        assert_eq!(row(Seen { installed: true, ..all() }), AccelRow::NotUsed);
        assert_eq!(row(Seen { offered: true, installed: true, ..all() }), AccelRow::Stopped);
        assert_eq!(row(Seen { stopped: false, unsupported: true, ..seen() }), AccelRow::NeedsDriver);
        // Nothing here and no card for it: no row at all.
        assert_eq!(row(Seen { offered: false, installed: false, ..seen() }), AccelRow::Hidden);
    }

    /// NVIDIA's real wheel, served from a file (`AUTOSCENE_ACCEL_WHEEL`, what
    /// `cargo xtask pin-accel --work` kept), through the real pins.
    #[test]
    fn the_pinned_wheel_installs_file_by_file() {
        let Some(file) = std::env::var_os("AUTOSCENE_ACCEL_WHEEL") else {
            eprintln!("AUTOSCENE_ACCEL_WHEEL isn't set (skipping)");
            return;
        };
        let (url, sent) = serve(fs::read(file).unwrap());
        let (_dir, packs) = packs();
        let pins = Pins { url, ..accel::pins().clone() };
        let mut last = (0, 0);
        packs.download(&pins, &CancelToken::new(), |done, total| last = (done, total)).unwrap();
        assert_eq!(last, (pins.download_bytes(), pins.download_bytes()));
        accel::verify(&packs.usable().unwrap()).expect("every file as pinned");
        assert!(sent.load(Ordering::Relaxed) < pins.size + 1_000_000, "each file once, and the wheel's directory");
    }
}

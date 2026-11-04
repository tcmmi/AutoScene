//! AutoScene's plugin catalog as this app holds it ([`CatalogHost`]): its
//! signed index, fetched at start and once a day while the setting is on,
//! and "Check now" whatever the setting says.
//!
//! What AutoScene took of the catalog is a row of `library.db` ([`ROW`], a
//! [`CatalogState`]): the number and hash of the index held, when it was
//! fetched and last tried, why the last try failed, and the plugins it
//! withdrew. The index itself is kept in `cache/catalog/`. The row is
//! written before the file, so a crash between them leaves a file the row
//! doesn't name: it isn't used, and the next fetch brings the index again.
//! A lost or changed file loses what the catalog reviewed until then, never
//! what it withdrew. A row a later AutoScene wrote turns the catalog off in
//! this one, which never writes over it.
//!
//! Each check tries every address and keeps the newest index that checks
//! out ([`as_plugin::catalog::check`]). The store then judges plugins by it
//! ([`as_plugin::Store::set_catalog`]), and the runs of a plugin it newly
//! withdrew stop, with its pages and its service ([`refresh_and_apply`]).
//!
//! A plugin installs and updates from the catalog in two steps, as one from
//! a file does: its package is got and checked ([`CatalogHost::get`]), then
//! the person says yes to the review (`plugin_install`). The package comes
//! as AutoScene's models do, from the addresses the index lists, and is kept
//! only with the size and SHA-256 it lists. It waits in
//! `cache/plugin-downloads` until it is checked; that folder is emptied at
//! each start. Only a version the catalog offers here is got, newer than
//! the one installed, from an index that hasn't run out, and only the
//! version it lists, under the lock it reviewed, is staged.
//!
//! Debug builds read `~/.autoscene/catalog.json`, which names other
//! addresses (http too) and more keys: a catalog of one's own to try the
//! app against. What it takes is kept apart ([`DEV_ROW`], `cache/catalog-dev`):
//! debug and release builds share the data folder, and a release build
//! never sees its numbers or its withdrawals. Release builds don't have this
//! code at all.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use as_core::{AppPaths, CancelToken};
use as_db::LibraryDb;
use as_jobs::JobRunner;
use as_models::{ModelStore, Packed, Pinned};
use as_plugin::catalog::{
    self, Catalog, CatalogKeys, CatalogState, Checked, Entry, MAX_INDEX_BYTES, MAX_SIG_BYTES, NEEDS_NEWER_APP, Offer,
    STATE_VERSION,
};
use as_plugin::fetch::fetch_error;
use as_plugin::keys::CATALOG_URLS;
use as_plugin::manifest::current_platform;
use as_plugin::{Installed, Manifest, PluginError, PluginId, PluginResult, RuntimeKind, Store, Ticket, package};
use parking_lot::Mutex;
use semver::Version;
use serde::Serialize;
use serde_json::Value;
use specta::Type;
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

use super::{PluginHost, serde_name};
use crate::error::AppResult;
use crate::events::{CatalogChanged, CatalogProgress, PluginsChanged};
use crate::plugins;
use crate::state::AppState;

/// The row of `library.db` that holds what AutoScene took of the catalog.
/// Outside `plugin.` and `plugin_settings.`, which the store reads as
/// plugins and their settings.
pub const ROW: &str = "plugins_catalog";
/// [`ROW`] of a catalog of one's own (the debug override). Outside the
/// store's rows too.
pub const DEV_ROW: &str = "plugins_catalog.dev";
/// How long after the start the first check waits.
pub const FIRST_CHECK: Duration = Duration::from_secs(10);
/// How often the catalog is checked while the app is open.
pub const EVERY: Duration = Duration::from_secs(24 * 3600);
/// How long after a failed check the next one waits.
pub const RETRY: Duration = Duration::from_secs(3600);
/// A start checks, unless the last good fetch is younger than this.
pub const STARTUP_FRESH: Duration = Duration::from_secs(3600);
/// How often the loop looks whether a check is due.
pub const TICK: Duration = Duration::from_secs(600);
/// How long one GET may take, its body included.
pub const BUDGET: Duration = Duration::from_secs(30);
/// How often `catalog:progress` is sent at most while a package comes.
pub const PROGRESS_EVERY: Duration = Duration::from_millis(100);

/// The files of the index held, in its folder.
const INDEX: &str = "index.json";
const SIG: &str = "index.json.minisig";

/// What a check says when an address didn't answer as it should (the log
/// has what went wrong).
const UNREACHABLE: &str = "AutoScene couldn't reach its plugin catalog.";
/// What a get says when the index ran out, or there is none.
const OUT_OF_DATE: &str = "The plugin catalog is out of date. Check it again, then try again.";
/// What a get says when this build has no catalog (no keys or addresses):
/// checking again changes nothing.
const NO_CATALOG: &str = "This AutoScene has no plugin catalog.";
/// What a get says when the package isn't the version it lists, under the
/// lock it reviewed (the log has which).
const NOT_LISTED: &str = "The catalog's package isn't the one it lists. Try again later.";

/// Gets a file of the catalog: its whole body, at most `max` bytes, or why
/// not (for the log).
pub trait Fetch: Send + Sync {
    fn get(&self, url: &str, max: u64) -> Result<Vec<u8>, String>;
}

/// [`Fetch`] from the internet, within [`BUDGET`] each.
pub struct UreqFetch;

impl Fetch for UreqFetch {
    fn get(&self, url: &str, max: u64) -> Result<Vec<u8>, String> {
        as_models::net::get_capped(url, max, BUDGET, &CancelToken::new())
    }
}

/// Gets the package of a catalog entry into a folder: its file there, with
/// the entry's size and SHA-256, or why not (in words about the plugin).
/// `progress(done, total)` as its bytes come; `cancel` ends it.
pub trait GetPackage: Send + Sync {
    fn get(
        &self,
        e: &Entry,
        dir: &Path,
        cancel: &CancelToken,
        progress: &mut dyn FnMut(u64, u64),
    ) -> PluginResult<PathBuf>;
}

/// [`GetPackage`] from the internet, as AutoScene's models come: from each
/// of the entry's addresses in turn, resumed after an interruption, and kept
/// only once it has the entry's size and SHA-256.
pub struct ModelDownload;

impl GetPackage for ModelDownload {
    fn get(
        &self,
        e: &Entry,
        dir: &Path,
        cancel: &CancelToken,
        progress: &mut dyn FnMut(u64, u64),
    ) -> PluginResult<PathBuf> {
        ModelStore::new(dir).fetch_pinned(&pinned(e), cancel, progress).map_err(|err| fetch_error(&e.name, err))
    }
}

/// The package of `e` as the file the store downloads.
fn pinned(e: &Entry) -> Pinned {
    Pinned {
        id: format!("{} {}", e.id, e.version),
        file: package_file(e),
        urls: e.package.urls.clone(),
        sha256: e.package.sha256.clone(),
        size: e.package.size,
        packed: Packed::File,
        exec: false,
    }
}

/// The name of the package of `e` while it downloads: its SHA-256, so a
/// part of one package is never resumed as another's, whichever index
/// listed them, and no two ids and versions share a file.
fn package_file(e: &Entry) -> String {
    format!("{}.asp", e.package.sha256)
}

/// The catalog of this app: where it is fetched from, the keys it is checked
/// with, and what was taken of it.
pub struct CatalogHost {
    keys: CatalogKeys,
    /// Each ends in `/`; `index.json` and its signature are appended.
    urls: Vec<String>,
    /// [`ROW`], or [`DEV_ROW`] for a catalog of one's own.
    row: &'static str,
    /// `cache/catalog` (`cache/catalog-dev`): the index held, and its signature.
    dir: PathBuf,
    db: Arc<LibraryDb>,
    fetch: Arc<dyn Fetch>,
    /// Where packages come from ([`CatalogHost::get`]).
    packages: Arc<dyn GetPackage>,
    /// `cache/plugin-downloads`: the packages that are coming.
    downloads_dir: PathBuf,
    /// The gets under way, by plugin: one each at a time, and what ends it.
    downloads: Mutex<HashMap<PluginId, CancelToken>>,
    /// One check at a time: the daily one and "Check now" take turns.
    one: Mutex<()>,
    /// One [`apply`] at a time, from what the store judged by before to the
    /// catalog it judges by after: a withdrawal is new to one of them only,
    /// and an older catalog is never set over a newer one.
    applying: Mutex<()>,
    /// A later AutoScene wrote the row: the catalog is off here.
    newer_row: AtomicBool,
}

impl CatalogHost {
    /// The catalog of the data folder `paths`, whose row is in `db`, checked
    /// with `keys` at `urls` through `fetch`, whose packages come through
    /// `packages`.
    pub fn new(
        paths: &AppPaths,
        db: Arc<LibraryDb>,
        keys: CatalogKeys,
        urls: Vec<String>,
        fetch: Arc<dyn Fetch>,
        packages: Arc<dyn GetPackage>,
    ) -> Self {
        Self::open(paths, db, keys, urls, (fetch, packages), false)
    }

    /// [`CatalogHost::new`]; `dev` keeps what it takes in [`DEV_ROW`] and
    /// `cache/catalog-dev`.
    fn open(
        paths: &AppPaths,
        db: Arc<LibraryDb>,
        keys: CatalogKeys,
        urls: Vec<String>,
        (fetch, packages): (Arc<dyn Fetch>, Arc<dyn GetPackage>),
        dev: bool,
    ) -> Self {
        let host = Self {
            keys,
            urls,
            row: if dev { DEV_ROW } else { ROW },
            dir: paths.cache().join(if dev { "catalog-dev" } else { "catalog" }),
            db,
            fetch,
            packages,
            // One folder for both: a package's file is named by its hash
            // ([`package_file`]), whichever index listed it.
            downloads_dir: paths.plugin_downloads(),
            downloads: Mutex::default(),
            one: Mutex::new(()),
            applying: Mutex::new(()),
            newer_row: AtomicBool::new(false),
        };
        // Whose the row is decides whether the catalog is on.
        host.row();
        host
    }

    /// This app's catalog: the compiled keys and addresses, and in debug
    /// builds those of `~/.autoscene/catalog.json`, kept apart.
    pub fn builtin(paths: &AppPaths, db: Arc<LibraryDb>) -> Self {
        #[cfg(debug_assertions)]
        if let Some(o) = dev::read(&dev::path()) {
            let (keys, urls) = dev::configured(o);
            return Self::open(paths, db, keys, urls, (Arc::new(UreqFetch), Arc::new(ModelDownload)), true);
        }
        Self::new(paths, db, CatalogKeys::builtin(), compiled_urls(), Arc::new(UreqFetch), Arc::new(ModelDownload))
    }

    /// Whether this build checks the catalog: it has keys and addresses,
    /// and the row is this AutoScene's.
    pub fn ready(&self) -> bool {
        !self.keys.is_empty() && !self.urls.is_empty() && !self.newer_row()
    }

    /// Whether a later AutoScene wrote the row.
    pub fn newer_row(&self) -> bool {
        self.newer_row.load(Ordering::Relaxed)
    }

    /// The catalog as it was taken: the row, and the index kept, if it is
    /// still the one the row names. Never the network.
    pub fn load(&self) -> Catalog {
        let state = self.row();
        self.with_cache(state, None)
    }

    /// Checks the catalog at every address at `now`: the newest index that
    /// checks out is taken, or the one held is again. `Err` when none did,
    /// with the catalog held (its row says why). `current` is the catalog in
    /// hand: its index stays while the row still names it, whatever became
    /// of the file.
    pub fn refresh(&self, now: u64, current: &Catalog) -> Result<Catalog, Box<Catalog>> {
        let _one = self.one.lock();
        let held = self.row();
        if !self.ready() {
            return Err(Box::new(self.with_cache(held, Some(current))));
        }
        let mut newest: Option<(Box<catalog::Accepted>, Vec<u8>, String)> = None;
        let mut same = None;
        let mut errors = Vec::new();
        for base in &self.urls {
            match self.check(base, &held, now) {
                Ok((Checked::Newer(a), bytes, sig)) => {
                    let number = (a.index.epoch, a.index.seq);
                    if newest.as_ref().is_none_or(|(b, _, _)| number > (b.index.epoch, b.index.seq)) {
                        newest = Some((a, bytes, sig));
                    }
                }
                Ok((Checked::Same, bytes, sig)) => same = Some((bytes, sig)),
                Err(e) => errors.push(e),
            }
        }
        if let Some((a, bytes, sig)) = newest {
            let state = held.after(&a, now);
            // The row first: a cache it doesn't name is never used.
            if self.write_row(&state)
                && let Err(e) = self.write_cache(&bytes, &sig)
            {
                log::warn!("keeping the plugin catalog: {e}");
            }
            log::info!("plugin catalog epoch {} seq {} taken", a.index.epoch, a.index.seq);
            return Ok(Catalog::new(Some(a.index), state));
        }
        if let Some((bytes, sig)) = same {
            let state = CatalogState { fetched_at: now, checked_at: now, last_error: None, ..held };
            self.write_row(&state);
            let kept = self.with_cache(state.clone(), Some(current));
            if kept.index().is_some() {
                return Ok(kept);
            }
            // Its file was lost or changed: the same index, fetched again,
            // takes its place.
            return Ok(match catalog::reload(&bytes, &sig, &self.keys, &state) {
                Ok(index) => {
                    if let Err(e) = self.write_cache(&bytes, &sig) {
                        log::warn!("keeping the plugin catalog: {e}");
                    }
                    Catalog::new(Some(index), state)
                }
                Err(e) => {
                    log::warn!("the plugin catalog fetched again: {}", e.detail());
                    kept
                }
            });
        }
        let error = errors.first().map_or_else(|| UNREACHABLE.to_owned(), PluginError::to_string);
        let state = held.tried(now, Some(error));
        self.write_row(&state);
        Err(Box::new(self.with_cache(state, Some(current))))
    }

    /// Whether a check is due at `now`, with `daily` the setting, at the
    /// app's start (`first`) or later: a start checks unless the last good
    /// fetch is younger than [`STARTUP_FRESH`], then every [`EVERY`]; a
    /// failed check waits [`RETRY`] before the next. A time still to come
    /// was written while the clock ran ahead: it counts as long ago, so a
    /// corrected clock checks at once.
    pub fn due(&self, now: u64, daily: bool, first: bool) -> bool {
        if !daily || !self.ready() {
            return false;
        }
        let s = self.row();
        let wait = if first { STARTUP_FRESH } else { EVERY };
        let stale = s.fetched_at > now || now >= s.fetched_at.saturating_add(wait.as_secs());
        let retry = s.last_error.is_none() || s.checked_at > now || now >= s.checked_at.saturating_add(RETRY.as_secs());
        stale && retry
    }

    /// Gets the package of the version of `download`'s plugin the catalog
    /// offers here, and checks it in `store` for the person's review: one
    /// newer than the version installed (never an older one), from an index
    /// that hasn't run out. `progress(done, total)` as its bytes come. The
    /// package is the file the index pins, and the ticket the version it
    /// lists, under the lock it reviewed. [`CatalogHost::cancel`] ends it,
    /// and a cancel wins over whatever else went wrong meanwhile. The
    /// package goes once it is checked.
    pub fn get(
        &self,
        store: &Store,
        download: &Downloading<'_>,
        mut progress: impl FnMut(u64, u64),
    ) -> PluginResult<Ticket> {
        let id = &download.id;
        let catalog = store.catalog();
        if !catalog.living(catalog::now()) {
            return Err(PluginError::Invalid(self.not_living(&catalog).into()));
        }
        let e = match catalog.offer(id, store.app_version(), current_platform()) {
            Some(Offer::Here(e)) => e,
            Some(Offer::NeedsNewerApp(_)) => return Err(PluginError::TooNew("engines.autoscene".into())),
            Some(Offer::NotHere(_)) => return Err(PluginError::Platform),
            None => return Err(unlisted(&catalog, id)),
        };
        match store.get(id) {
            Ok(p) if !newer(e, &p.record.version) => {
                return Err(PluginError::Invalid(format!("Version {} is installed already.", p.record.version)));
            }
            Ok(_) | Err(PluginError::NotFound(_)) => {}
            Err(failed) => return Err(failed),
        }
        let got = self.packages.get(e, &self.downloads_dir, &download.cancel, &mut progress);
        let staged = got.and_then(|path| {
            let staged = store.stage(&path);
            forget(&path);
            staged
        });
        // Cancelled, it says so, whatever else went wrong meanwhile: in the
        // download, its check, or the checks below.
        let t = staged.map_err(|e| if download.cancelled() { PluginError::Cancelled } else { e })?;
        if download.cancelled() {
            store.discard(t);
            return Err(PluginError::Cancelled);
        }
        // The version the entry names, which the catalog reviewed: its lock
        // is the one it lists (an index lists each version once).
        let listed = t.manifest.id == e.id && same_version(&t.manifest.version, &e.version);
        if !listed || t.approval.is_none() {
            let lock = package::sha256_hex(&t.lock_bytes);
            let (got, approved) = (format!("{} {}", t.manifest.id, t.manifest.version), t.approval.is_some());
            let latest = store.catalog();
            store.discard(t);
            if listed && !latest.living(catalog::now()) {
                // It ran out while the package came.
                return Err(PluginError::Invalid(self.not_living(&latest).into()));
            }
            log::warn!(
                "the catalog's package of {} {} is {got} with lock {lock} (reviewed: {approved})",
                e.id,
                e.version
            );
            return Err(PluginError::Invalid(NOT_LISTED.into()));
        }
        Ok(t)
    }

    /// Why nothing is got from `catalog`, which isn't living: only a newer
    /// AutoScene can check it, this build has no catalog, or it ran out (as
    /// the view tells them apart).
    fn not_living(&self, catalog: &Catalog) -> &'static str {
        if self.newer_row() || catalog.state().last_error.as_deref() == Some(NEEDS_NEWER_APP) {
            NEEDS_NEWER_APP
        } else if !self.ready() {
            NO_CATALOG
        } else {
            OUT_OF_DATE
        }
    }

    /// Ends the get of `id` under way, if there is one.
    pub fn cancel(&self, id: &PluginId) {
        if let Some(cancel) = self.downloads.lock().get(id) {
            cancel.cancel();
        }
    }

    /// Marks a get of `id` as under way, before anything else is done for
    /// it: from here, [`CatalogHost::cancel`] ends it, until
    /// [`Downloading::end`] (or the guard drops). A second one is refused
    /// meanwhile, naming the plugin as `store`'s catalog lists it.
    pub fn begin(&self, store: &Store, id: &PluginId) -> PluginResult<Downloading<'_>> {
        let mut downloads = self.downloads.lock();
        if !downloads.contains_key(id) {
            let cancel = CancelToken::new();
            downloads.insert(id.clone(), cancel.clone());
            return Ok(Downloading { downloads: &self.downloads, id: id.clone(), cancel, ended: false });
        }
        drop(downloads);
        let catalog = store.catalog();
        let listed = catalog.index().and_then(|i| i.plugins.iter().find(|e| &e.id == id));
        let name = listed.map_or_else(|| id.to_string(), |e| e.name.clone());
        Err(PluginError::Invalid(format!("AutoScene is getting {name} already.")))
    }

    /// Removes what gets that never finished left (at the start): partly
    /// downloaded packages, and finished ones never checked. Not while a get
    /// runs: one that starts meanwhile waits for this.
    pub fn clear_downloads(&self) {
        let downloads = self.downloads.lock();
        if !downloads.is_empty() {
            return;
        }
        let dir = &self.downloads_dir;
        let removed = match std::fs::symlink_metadata(dir) {
            Ok(md) if md.is_dir() => std::fs::remove_dir_all(dir),
            Ok(_) => std::fs::remove_file(dir),
            Err(e) => Err(e),
        };
        match removed {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                log::warn!("removing the plugin downloads left from before: {e}");
            }
            _ => {}
        }
    }

    /// Fetches the index and its signature at `base`, and checks them
    /// against `held`; gives them with what [`catalog::check`] made of them.
    fn check(&self, base: &str, held: &CatalogState, now: u64) -> PluginResult<(Checked, Vec<u8>, String)> {
        let get = |name: &str, max: u64| {
            let url = format!("{base}{name}");
            self.fetch.get(&url, max).map_err(|e| {
                log::warn!("plugin catalog: {e}");
                PluginError::Catalog(UNREACHABLE.into())
            })
        };
        let bytes = get(INDEX, MAX_INDEX_BYTES)?;
        let sig = String::from_utf8(get(SIG, MAX_SIG_BYTES)?)
            .map_err(|_| PluginError::Catalog("The catalog's signature can't be read.".into()))?;
        match catalog::check(&bytes, &sig, &self.keys, held, now) {
            Ok(checked) => Ok((checked, bytes, sig)),
            Err(e) => {
                log::warn!("plugin catalog at {base}: {}", e.detail());
                Err(e)
            }
        }
    }

    /// The row as it is now: empty when there is none or it can't be read.
    /// One a later AutoScene wrote turns the catalog off here; its
    /// withdrawals still hold if this one can read them.
    fn row(&self) -> CatalogState {
        let raw = match self.db.setting(self.row) {
            Ok(raw) => raw,
            Err(e) => {
                log::warn!("reading the plugin catalog's row: {e}");
                None
            }
        };
        let Some(raw) = raw else { return CatalogState::default() };
        let value: Value = match serde_json::from_str(&raw) {
            Ok(value) => value,
            Err(e) => {
                log::warn!("the plugin catalog's row can't be read, so it counts as empty: {e}");
                return CatalogState::default();
            }
        };
        if value.get("v").and_then(Value::as_u64).is_some_and(|v| v > u64::from(STATE_VERSION))
            && !self.newer_row.swap(true, Ordering::Relaxed)
        {
            log::warn!("a later AutoScene checked the plugin catalog: it is off in this one");
        }
        serde_json::from_value(value).unwrap_or_else(|e| {
            log::warn!("the plugin catalog's row can't be read, so it counts as empty: {e}");
            CatalogState::default()
        })
    }

    /// Writes `state` as the row, unless a later AutoScene's is there.
    /// Whether it was written.
    fn write_row(&self, state: &CatalogState) -> bool {
        if self.newer_row() {
            return false;
        }
        let json = serde_json::to_string(state).expect("a catalog state serializes");
        match self.db.set_setting(self.row, &json) {
            Ok(()) => true,
            Err(e) => {
                log::warn!("writing the plugin catalog's row: {e}");
                false
            }
        }
    }

    /// The catalog of `state`, with the index it names: that of `current`
    /// if it is that one, or else the one kept if it is, signed and sound.
    /// Without it, what `state` withdrew holds.
    fn with_cache(&self, state: CatalogState, current: Option<&Catalog>) -> Catalog {
        if state.sha256.is_none() || self.keys.is_empty() || self.newer_row() {
            return Catalog::new(None, state);
        }
        // Checked by its hash when it was taken: not read again.
        let same = current.filter(|c| c.state().sha256 == state.sha256);
        if let Some(index) = same.and_then(Catalog::index) {
            return Catalog::new(Some(index.clone()), state);
        }
        let kept = read_capped(&self.dir.join(INDEX), MAX_INDEX_BYTES).and_then(|bytes| {
            let sig = String::from_utf8(read_capped(&self.dir.join(SIG), MAX_SIG_BYTES)?)
                .map_err(|_| PluginError::Catalog("The catalog's signature can't be read.".into()))?;
            catalog::reload(&bytes, &sig, &self.keys, &state)
        });
        match kept {
            Ok(index) => Catalog::new(Some(index), state),
            Err(e) => {
                log::info!("the plugin catalog kept isn't used: {}", e.detail());
                Catalog::new(None, state)
            }
        }
    }

    /// Keeps the index `bytes` and its signature `sig`, each written beside
    /// and then moved over the one before.
    fn write_cache(&self, bytes: &[u8], sig: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        for (name, data) in [(INDEX, bytes), (SIG, sig.as_bytes())] {
            let tmp = self.dir.join(format!("{name}.tmp"));
            std::fs::write(&tmp, data)?;
            std::fs::rename(&tmp, self.dir.join(name))?;
        }
        Ok(())
    }
}

/// A get of a plugin's package under way ([`CatalogHost::begin`]).
pub struct Downloading<'a> {
    downloads: &'a Mutex<HashMap<PluginId, CancelToken>>,
    id: PluginId,
    /// What ends it.
    cancel: CancelToken,
    /// [`Downloading::end`] took it out of `downloads` already.
    ended: bool,
}

impl Downloading<'_> {
    /// Whether it was cancelled.
    pub fn cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// Ends the get, once its review is ready: whether it was cancelled
    /// before. A cancel after this finds no get, and the review stands.
    pub fn end(mut self) -> bool {
        let cancelled = {
            let mut downloads = self.downloads.lock();
            downloads.remove(&self.id);
            self.cancel.is_cancelled()
        };
        self.ended = true;
        cancelled
    }
}

impl Drop for Downloading<'_> {
    fn drop(&mut self) {
        // Once ended, the entry may already be a later get's.
        if !self.ended {
            self.downloads.lock().remove(&self.id);
        }
    }
}

/// Why the catalog offers nothing of `id`: it withdrew each version it
/// lists, or it lists none.
fn unlisted(catalog: &Catalog, id: &PluginId) -> PluginError {
    let listed = catalog.index().into_iter().flat_map(|i| &i.plugins).filter(|e| &e.id == id);
    let withdrawn = listed.filter_map(|e| Some((e, catalog.withdrawal(id, &e.version)?))).next();
    match withdrawn {
        Some((e, w)) => PluginError::Withdrawn { name: e.name.clone(), reason: w.reason.clone() },
        None => PluginError::Invalid(format!("AutoScene's plugin catalog doesn't list {id}.")),
    }
}

/// Removes the package downloaded at `path`, the store's mark that it was
/// checked, and the lock its download held: best-effort, as the start
/// empties their folder anyway. One get of a plugin runs at a time, and its
/// file is named by its hash, so no other download waits on that lock.
fn forget(path: &Path) {
    let beside = |ext: &str| {
        let mut name = path.as_os_str().to_owned();
        name.push(ext);
        PathBuf::from(name)
    };
    for file in [path.to_path_buf(), beside(".verified"), beside(".lock")] {
        if let Err(e) = std::fs::remove_file(file)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            log::info!("a plugin download stays until the next start: {e}");
        }
    }
}

/// Whether the versions `a` and `b` are the same, as semver reads them.
fn same_version(a: &str, b: &str) -> bool {
    matches!((Version::parse(a), Version::parse(b)), (Ok(a), Ok(b)) if a == b)
}

/// The file at `path`, if it has at most `max` bytes.
fn read_capped(path: &Path, max: u64) -> PluginResult<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?.take(max + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(PluginError::Catalog("The catalog is too large.".into()));
    }
    Ok(bytes)
}

/// Checks the catalog when it is due, from [`FIRST_CHECK`] after the start
/// on, looking every [`TICK`] (which copes with sleep and with a day that
/// passes while the app is open).
pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        let mut first = true;
        loop {
            let app2 = app.clone();
            let _ = tauri::async_runtime::spawn_blocking(move || {
                let state = app2.state::<AppState>();
                let (Some(host), Ok(s)) = (state.plugins.clone(), state.library.settings()) else { return };
                if let Some((_, withdrawn, changed)) =
                    tick(&host, &state.jobs, catalog::now(), s.plugin_catalog_checks, first)
                {
                    tell(&app2, &withdrawn, changed);
                }
            })
            .await;
            first = false;
            tokio::time::sleep(TICK).await;
        }
    });
}

/// The daily checks went from `before` to `after` (Settings): when they
/// went on and a check is due, it runs now, and the page hears what it
/// withdrew.
pub fn checks_changed(app: &AppHandle, before: bool, after: bool) {
    let state = app.state::<AppState>();
    let Some(host) = state.plugins.clone() else { return };
    if let Some((_, withdrawn, changed)) = turned_on(&host, &state.jobs, catalog::now(), before, after) {
        tell(app, &withdrawn, changed);
    }
}

/// Checks the catalog now and judges plugins by what it says; tells the
/// page what it withdrew. A failed check is no error: the view says why.
pub fn refresh_and_apply(app: &AppHandle) -> AppResult<CatalogView> {
    let state = app.state::<AppState>();
    let host = plugins::host(&state)?;
    let daily = state.library.settings()?.plugin_catalog_checks;
    let (view, withdrawn, changed) = apply(&host, &state.jobs, catalog::now(), daily);
    tell(app, &withdrawn, changed);
    Ok(view)
}

/// [`CatalogHost::get`] under `download`, telling the page how far its
/// package came (`catalog:progress`, [`told`]).
pub fn get(app: &AppHandle, host: &PluginHost, download: &Downloading<'_>) -> PluginResult<Ticket> {
    let plugin_id = download.id.to_string();
    told(&host.catalog, &host.store, download, |done, total| {
        let _ = CatalogProgress { plugin_id: plugin_id.clone(), done: done as f64, total: total as f64 }.emit(app);
    })
}

/// [`CatalogHost::get`], telling `tell` how far the package came: at most
/// every [`PROGRESS_EVERY`], at once when all of it came (before its
/// check), and what was left untold when it failed.
fn told(
    catalog: &CatalogHost,
    store: &Store,
    download: &Downloading<'_>,
    mut tell: impl FnMut(u64, u64),
) -> PluginResult<Ticket> {
    let mut telling = Telling::default();
    let got = catalog.get(store, download, |done, total| {
        if telling.tell(Instant::now(), done, total) {
            tell(done, total);
        }
    });
    if let Some((done, total)) = telling.untold {
        tell(done, total);
    }
    got
}

/// What of a download's progress is told: at most every [`PROGRESS_EVERY`],
/// all of it at once, and what is left untold for its end.
#[derive(Default)]
struct Telling {
    /// When it was last told.
    last: Option<Instant>,
    /// What came since, untold.
    untold: Option<(u64, u64)>,
}

impl Telling {
    /// Whether `done` of `total` is told at `now`.
    fn tell(&mut self, now: Instant, done: u64, total: u64) -> bool {
        let all = done >= total;
        if !all && self.last.is_some_and(|last| now.saturating_duration_since(last) < PROGRESS_EVERY) {
            self.untold = Some((done, total));
            return false;
        }
        (self.last, self.untold) = (Some(now), None);
        true
    }
}

/// Tells the page what a check withdrew, and whether the plugins withdrawn
/// `changed`.
fn tell(app: &AppHandle, withdrawn: &[PluginId], changed: bool) {
    if changed {
        let _ = PluginsChanged { plugin_id: None }.emit(app);
    }
    let _ = CatalogChanged { withdrawn: withdrawn.iter().map(ToString::to_string).collect() }.emit(app);
}

/// What [`apply`] gives: the view; the installed plugins the new index
/// withdrew; and whether the plugins withdrawn changed.
pub(super) type Applied = (CatalogView, Vec<PluginId>, bool);

/// A look of [`start`]'s loop at `now`, with `daily` the setting, `first`
/// at the app's start: [`apply`] when a check is due.
pub(super) fn tick(host: &PluginHost, jobs: &JobRunner, now: u64, daily: bool, first: bool) -> Option<Applied> {
    host.catalog.due(now, daily, first).then(|| apply(host, jobs, now, daily))
}

/// [`checks_changed`] on `host` at `now`: when the setting went on, a look
/// as on any day but the first.
pub(super) fn turned_on(host: &PluginHost, jobs: &JobRunner, now: u64, before: bool, after: bool) -> Option<Applied> {
    if before || !after {
        return None;
    }
    tick(host, jobs, now, true, false)
}

/// [`refresh_and_apply`] on `host` at `now`, with `daily` the setting: the
/// view; the installed plugins the new index withdrew, whose runs stopped,
/// on `jobs`, with their pages and their services; and whether the
/// plugins withdrawn changed. One at a time.
pub(super) fn apply(host: &PluginHost, jobs: &JobRunner, now: u64, daily: bool) -> Applied {
    let _applying = host.catalog.applying.lock();
    let before = host.withdrawn();
    let catalog = host.catalog.refresh(now, &host.store.catalog()).unwrap_or_else(|held| *held);
    host.store.set_catalog(Arc::new(catalog));
    let after = host.withdrawn();
    let mut new: Vec<PluginId> = after.difference(&before).cloned().collect();
    new.sort_unstable();
    host.stop_withdrawn(jobs, &new);
    (view(host, now, daily), new, after != before)
}

/// The catalog as Settings shows it.
#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CatalogView {
    pub status: CatalogStatus,
    /// What it offers, by plugin, while its index lives; none otherwise.
    pub plugins: Vec<CatalogPlugin>,
}

/// Where AutoScene stands with its catalog. Times are Unix ms.
#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CatalogStatus {
    /// This build has the catalog's keys, and this AutoScene owns its row.
    pub ready: bool,
    /// It is checked once a day (the setting).
    pub daily: bool,
    pub published: Option<f64>,
    pub expires: Option<f64>,
    /// When the index held was last fetched and checked out.
    pub fetched_at: Option<f64>,
    /// When AutoScene last tried.
    pub checked_at: Option<f64>,
    /// There is no index, or it ran out.
    pub out_of_date: bool,
    /// Why the last check failed.
    pub error: Option<String>,
    /// A later AutoScene checked the catalog, or its index needs one.
    pub needs_newer_app: bool,
}

/// What the catalog offers of one plugin: the newest version this AutoScene
/// runs here, or the newest it can't. Times are Unix ms.
#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CatalogPlugin {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub author: String,
    pub license: Option<String>,
    pub version: String,
    /// Empty: all.
    pub platforms: Vec<String>,
    pub permissions: Vec<String>,
    pub hosts: Vec<String>,
    pub gpu: bool,
    pub uv_script: bool,
    pub runtime: String,
    /// Where the catalog built it from.
    pub repository: String,
    pub commit: String,
    pub path: String,
    /// A `data:` URL the catalog's checks passed.
    pub icon: Option<String>,
    pub size_bytes: f64,
    pub approved: f64,
    /// The version installed.
    pub installed: Option<String>,
    /// It runs here and is newer than the one installed.
    pub update: bool,
    /// What it asks that the installed one wasn't granted.
    pub adds_permissions: Vec<String>,
    pub adds_hosts: Vec<String>,
    /// How it would run past what the person agreed to, as an update's
    /// review says (`PluginUpdate`): each keeps it out of Update all.
    pub starts_program: bool,
    pub becomes_service: bool,
    pub leaves_sandbox: bool,
    pub runs_here: bool,
    pub needs_newer_app: bool,
    /// What the large files it names take, in bytes: what its first run
    /// downloads.
    pub resource_bytes: f64,
    /// Hosts those files come from that the installed version's
    /// `plugin.json` doesn't name (all of them when it can't be read).
    pub adds_resource_hosts: Vec<String>,
}

/// The catalog of `host` at `now`, with `daily` the setting.
pub fn view(host: &PluginHost, now: u64, daily: bool) -> CatalogView {
    let catalog = host.store.catalog();
    let state = catalog.state();
    let living = catalog.living(now);
    let installed: HashMap<PluginId, Installed> = host
        .store
        .list()
        .unwrap_or_else(|e| {
            log::warn!("listing plugins for the catalog: {}", e.detail());
            Vec::new()
        })
        .into_iter()
        .map(|p| (p.record.id.clone(), p))
        .collect();
    let plugins = if living {
        let app = host.store.app_version();
        let listed = catalog.ids().into_iter().filter_map(|id| listed(&catalog, id, installed.get(id), app));
        listed.collect()
    } else {
        Vec::new()
    };
    let status = CatalogStatus {
        ready: host.catalog.ready(),
        daily,
        published: ms(state.published),
        expires: ms(state.expires),
        fetched_at: ms(state.fetched_at),
        checked_at: ms(state.checked_at),
        out_of_date: !living,
        error: state.last_error.clone(),
        needs_newer_app: host.catalog.newer_row() || state.last_error.as_deref() == Some(NEEDS_NEWER_APP),
    };
    CatalogView { status, plugins }
}

/// What `catalog` offers of `id` to the AutoScene `app`, with `installed`
/// the version installed. One it withdrew every version of is listed only
/// while it is installed.
fn listed(catalog: &Catalog, id: &PluginId, installed: Option<&Installed>, app: &Version) -> Option<CatalogPlugin> {
    let (entry, runs_here, needs_newer_app) = match catalog.offer(id, app, current_platform()) {
        Some(Offer::Here(e)) => (e, true, false),
        Some(Offer::NeedsNewerApp(e)) => (e, false, true),
        Some(Offer::NotHere(e)) => (e, false, false),
        None => {
            installed?;
            let entries = catalog.index()?.plugins.iter().filter(|e| &e.id == id);
            let newest = entries
                .filter_map(|e| Some((e, Version::parse(&e.version).ok()?)))
                .max_by(|(_, a), (_, b)| a.cmp_precedence(b));
            (newest?.0, false, false)
        }
    };
    let record = installed.map(|p| &p.record);
    let granted: HashSet<String> = record.map(|r| r.permissions.iter().map(serde_name).collect()).unwrap_or_default();
    let hosts: HashSet<&str> = record.map(|r| r.network_hosts.iter().map(String::as_str).collect()).unwrap_or_default();
    let new = |names: &[String], has: &dyn Fn(&str) -> bool| -> Vec<String> {
        if record.is_none() { Vec::new() } else { names.iter().filter(|n| !has(n)).cloned().collect() }
    };
    // Where the installed version's files come from, by its plugin.json,
    // which its lock fixes: none known when it can't be read.
    let fetched_from = installed.and_then(|p| p.manifest.as_ref()).map(Manifest::resource_hosts).unwrap_or_default();
    // A runtime it doesn't name, or this AutoScene doesn't know, counts as a
    // program: nothing is said to stay in the sandbox that may not.
    let runs = serde_json::from_value(Value::from(entry.runtime.as_str())).unwrap_or(RuntimeKind::Process);
    let runtime = installed.map(|p| p.extra.agreed_runtime().change_to(runs)).unwrap_or_default();
    Some(CatalogPlugin {
        id: entry.id.to_string(),
        name: entry.name.clone(),
        summary: entry.summary.clone(),
        author: entry.author.clone(),
        license: entry.license.clone(),
        version: entry.version.clone(),
        platforms: entry.platforms.clone(),
        permissions: entry.permissions.clone(),
        hosts: entry.hosts.clone(),
        gpu: entry.gpu,
        uv_script: entry.uv_script,
        runtime: entry.runtime.clone(),
        repository: entry.source.repo.clone(),
        commit: entry.source.commit.clone(),
        path: entry.source.path.clone(),
        icon: entry.icon.clone(),
        size_bytes: entry.package.size as f64,
        approved: entry.approved as f64 * 1000.0,
        installed: record.map(|r| r.version.clone()),
        update: runs_here && record.is_some_and(|r| newer(entry, &r.version)),
        adds_permissions: new(&entry.permissions, &|p| granted.contains(p)),
        adds_hosts: new(&entry.hosts, &|h| hosts.contains(h)),
        starts_program: runtime.starts_program,
        becomes_service: runtime.becomes_service,
        leaves_sandbox: runtime.leaves_sandbox,
        runs_here,
        needs_newer_app,
        resource_bytes: entry.resource_bytes as f64,
        adds_resource_hosts: new(&entry.resource_hosts, &|h| fetched_from.iter().any(|f| f == h)),
    })
}

/// Whether `entry` is a newer version than `installed`.
fn newer(entry: &Entry, installed: &str) -> bool {
    match (Version::parse(&entry.version), Version::parse(installed)) {
        (Ok(offered), Ok(installed)) => offered.cmp_precedence(&installed).is_gt(),
        _ => false,
    }
}

/// Unix seconds `secs` as Unix ms, none for 0 (never).
fn ms(secs: u64) -> Option<f64> {
    (secs > 0).then_some(secs as f64 * 1000.0)
}

/// The compiled catalog's addresses.
fn compiled_urls() -> Vec<String> {
    CATALOG_URLS.iter().map(|u| (*u).to_owned()).collect()
}

/// `~/.autoscene/catalog.json`, a catalog of one's own: debug builds only,
/// release builds don't have this code.
#[cfg(debug_assertions)]
mod dev {
    use std::path::{Path, PathBuf};

    use as_core::AppPaths;
    use as_plugin::catalog::CatalogKeys;
    use as_plugin::keys::KeyRole;
    use serde::Deserialize;

    use super::compiled_urls;

    pub fn path() -> PathBuf {
        AppPaths::legacy_dot_dir().join("catalog.json")
    }

    #[derive(Debug, Default, Deserialize)]
    pub struct Override {
        /// They replace the compiled addresses (http too).
        #[serde(default)]
        urls: Vec<String>,
        /// They are added to the compiled keys.
        #[serde(default)]
        keys: Vec<OverrideKey>,
    }

    #[derive(Debug, Deserialize)]
    struct OverrideKey {
        role: Role,
        /// The base64 line of a minisign `.pub` file.
        key: String,
    }

    #[derive(Clone, Copy, Debug, Deserialize)]
    #[serde(rename_all = "lowercase")]
    enum Role {
        Online,
        Offline,
    }

    /// The override at `path`: none when there is no file, or it can't be
    /// read.
    pub fn read(path: &Path) -> Option<Override> {
        let raw = std::fs::read_to_string(path).ok()?;
        match serde_json::from_str::<Override>(&raw) {
            Ok(o) => {
                log::warn!(
                    "the plugin catalog follows the dev override in ~/.autoscene/catalog.json, \
                     and keeps what it takes apart (plugins_catalog.dev, cache/catalog-dev)"
                );
                Some(o)
            }
            Err(e) => {
                log::warn!("ignoring ~/.autoscene/catalog.json: {e}");
                None
            }
        }
    }

    /// The keys and addresses of the compiled catalog, with what `o` changes.
    pub fn configured(o: Override) -> (CatalogKeys, Vec<String>) {
        let keys = CatalogKeys::builtin();
        let extra: Vec<(KeyRole, &str)> = o
            .keys
            .iter()
            .map(|k| (if matches!(k.role, Role::Online) { KeyRole::Online } else { KeyRole::Offline }, k.key.as_str()))
            .collect();
        let keys = keys.clone().with(&extra).unwrap_or_else(|e| {
            log::warn!("ignoring the keys of ~/.autoscene/catalog.json: {}", e.detail());
            keys
        });
        let urls = if o.urls.is_empty() {
            compiled_urls()
        } else {
            o.urls.into_iter().map(|u| if u.ends_with('/') { u } else { format!("{u}/") }).collect()
        };
        (keys, urls)
    }
}

#[cfg(test)]
pub(super) mod tests {
    use std::sync::Barrier;

    use as_jobs::Class;
    use as_plugin::keys::KeyRole;
    use minisign::KeyPair;
    use serde_json::json;

    use super::super::tests::{CATALOG_AT, NoStart, host_cataloged};
    use super::*;
    use crate::error::ErrorCode;

    pub const NOW: u64 = 1_800_000_000;
    pub const DAY: u64 = 86_400;
    const A: &str = "https://a.example/";
    const B: &str = "https://b.example/";

    /// A minisign key pair, as `asp keygen` makes one.
    pub struct Key(KeyPair);

    impl Key {
        pub fn new() -> Self {
            Self(KeyPair::generate_unencrypted_keypair().unwrap())
        }

        pub fn keys(&self) -> CatalogKeys {
            CatalogKeys::new(&[(KeyRole::Online, &self.0.pk.to_base64())]).unwrap()
        }

        /// The index `index` as the catalog's CI publishes it: its bytes
        /// and their signature.
        pub fn sign(&self, index: &Value) -> (Vec<u8>, String) {
            let bytes = serde_json::to_vec(index).unwrap();
            let n = |key: &str| index[key].as_u64().unwrap();
            let comment = catalog::index_comment(n("epoch"), n("seq"), n("expires"));
            let sig = minisign::sign(Some(&self.0.pk), &self.0.sk, &bytes[..], Some(&comment), None).unwrap();
            (bytes, sig.into_string())
        }
    }

    /// An index of `seq` (epoch 1), published a day ago, running out in 29
    /// days, that lists `plugins` and withdraws `withdrawn`.
    pub fn index(seq: u64, plugins: Vec<Value>, withdrawn: Vec<Value>) -> Value {
        json!({
            "catalogVersion": 1,
            "epoch": 1,
            "seq": seq,
            "published": NOW - DAY,
            "expires": NOW + 29 * DAY,
            "plugins": plugins,
            "withdrawn": withdrawn,
        })
    }

    /// An entry of `id` at `version`, as the catalog's CI writes one.
    pub fn entry(id: &str, version: &str) -> Value {
        json!({
            "id": id,
            "version": version,
            "lock": as_plugin::package::sha256_hex(version.as_bytes()),
            "package": {
                "urls": [format!("https://github.com/tcmmi/autoscene-plugins/releases/download/{id}-{version}/{id}.asp")],
                "size": 4096,
                "sha256": "b".repeat(64),
            },
            "authorKey": null,
            "name": "Hello Export",
            "summary": "Says hello after an export.",
            "author": "AutoScene",
            "license": "MIT",
            "platforms": [],
            "runtime": "process",
            "gpu": false,
            "uvScript": true,
            "permissions": ["exports:read", "network"],
            "hosts": ["discord.com"],
            "engines": ">=1.0.0, <2",
            "source": {
                "repo": "https://github.com/tcmmi/autoscene",
                "commit": "c".repeat(40),
                "path": "examples/plugins/hello-export",
            },
            "icon": null,
            "approved": NOW - 2 * DAY,
        })
    }

    pub fn withdrawal(id: &str, versions: &str) -> Value {
        json!({"id": id, "versions": versions, "reason": "It sent export paths to a server.", "since": NOW - DAY})
    }

    /// Serves files by their address, and counts the asks.
    #[derive(Default)]
    pub struct Served {
        pub files: Mutex<HashMap<String, Result<Vec<u8>, String>>>,
        pub asked: Mutex<Vec<String>>,
    }

    impl Served {
        /// Serves `index`, signed by `key`, at `base`.
        pub fn index(&self, base: &str, key: &Key, index: &Value) {
            let (bytes, sig) = key.sign(index);
            let mut files = self.files.lock();
            files.insert(format!("{base}{INDEX}"), Ok(bytes));
            files.insert(format!("{base}{SIG}"), Ok(sig.into_bytes()));
        }

        /// `base` doesn't answer.
        pub fn down(&self, base: &str) {
            self.files.lock().insert(format!("{base}{INDEX}"), Err(format!("{base}{INDEX}: connection refused")));
        }
    }

    impl Fetch for Arc<Served> {
        fn get(&self, url: &str, max: u64) -> Result<Vec<u8>, String> {
            self.asked.lock().push(url.to_owned());
            let got = self.files.lock().get(url).cloned().unwrap_or_else(|| Err(format!("{url}: 404")));
            got.and_then(|b| if b.len() as u64 > max { Err(format!("{url}: more than {max} bytes")) } else { Ok(b) })
        }
    }

    /// A catalog in `tmp`, checked with `keys` at `urls` through `served`.
    fn host(tmp: &Path, keys: CatalogKeys, urls: &[&str], served: &Arc<Served>) -> (Arc<LibraryDb>, CatalogHost) {
        let paths = AppPaths::at(tmp.join("data"));
        std::fs::create_dir_all(&paths.data).unwrap();
        let db = Arc::new(LibraryDb::open(&paths.library_db()).unwrap());
        let urls = urls.iter().map(|u| (*u).to_owned()).collect();
        let host = CatalogHost::new(&paths, db.clone(), keys, urls, Arc::new(served.clone()), shelf());
        (db, host)
    }

    fn scratch(name: &str) -> PathBuf {
        let tmp = std::env::temp_dir().join(format!("autoscene-catalog-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        tmp
    }

    fn row(db: &LibraryDb) -> Option<CatalogState> {
        db.setting(ROW).unwrap().map(|raw| serde_json::from_str(&raw).unwrap())
    }

    fn seq(c: &Catalog) -> Option<u64> {
        c.index().map(|i| i.seq)
    }

    const HELLO: &str = "dev.autoscene.hello-export";

    #[test]
    fn the_newest_index_of_all_addresses_is_kept() {
        let tmp = scratch("newest");
        let (key, served) = (Key::new(), Arc::new(Served::default()));
        served.index(A, &key, &index(3, vec![entry(HELLO, "1.0.0")], vec![]));
        served.index(B, &key, &index(5, vec![entry(HELLO, "1.1.0")], vec![]));
        let (db, host) = host(&tmp, key.keys(), &[A, B], &served);
        let taken = host.refresh(NOW, &Catalog::none()).unwrap();
        assert_eq!(seq(&taken), Some(5));
        assert_eq!(row(&db).unwrap().seq, 5);
        // An address that falls behind afterwards changes nothing.
        let later = host.refresh(NOW + 60, &Catalog::none()).unwrap();
        assert_eq!(seq(&later), Some(5));
        assert_eq!((row(&db).unwrap().fetched_at, row(&db).unwrap().last_error), (NOW + 60, None));
        assert_eq!(seq(&host.load()), Some(5));
        drop((db, host));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn an_address_that_fails_does_not_stop_the_others() {
        let tmp = scratch("fails");
        let (key, served) = (Key::new(), Arc::new(Served::default()));
        served.down(A);
        served.index(B, &key, &index(2, vec![entry(HELLO, "1.0.0")], vec![]));
        let (db, host) = host(&tmp, key.keys(), &[A, B], &served);
        assert_eq!(seq(&host.refresh(NOW, &Catalog::none()).unwrap()), Some(2));
        // One signed by another key is refused, the other still taken.
        let other = Key::new();
        served.index(A, &other, &index(9, vec![], vec![]));
        served.index(B, &key, &index(3, vec![], vec![]));
        assert_eq!(seq(&host.refresh(NOW, &Catalog::none()).unwrap()), Some(3));
        assert_eq!(row(&db).unwrap().last_error, None);
        drop((db, host));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn nothing_but_the_failure_is_written_when_no_index_checks_out() {
        let tmp = scratch("failure");
        let (key, served) = (Key::new(), Arc::new(Served::default()));
        served.index(A, &key, &index(2, vec![entry(HELLO, "1.0.0")], vec![withdrawal("dev.example.leaky", "*")]));
        let (db, host) = host(&tmp, key.keys(), &[A, B], &served);
        host.refresh(NOW, &Catalog::none()).unwrap();
        let held = row(&db).unwrap();
        let kept = std::fs::read(tmp.join("data/cache/catalog").join(INDEX)).unwrap();

        // Both addresses fail: one is down, the other serves an older index.
        served.down(A);
        served.index(B, &key, &index(1, vec![], vec![]));
        let failed = host.refresh(NOW + 3600, &Catalog::none()).unwrap_err();
        let after = row(&db).unwrap();
        assert_eq!(
            after,
            CatalogState { checked_at: NOW + 3600, last_error: Some(UNREACHABLE.into()), ..held.clone() }
        );
        // The index held is still the one in hand, and on disk.
        assert_eq!(seq(&failed), Some(2));
        assert!(failed.withdrawal(&PluginId::parse("dev.example.leaky").unwrap(), "1.0.0").is_some());
        assert_eq!(std::fs::read(tmp.join("data/cache/catalog").join(INDEX)).unwrap(), kept);
        // The first address's error is the one said.
        served.index(A, &key, &index(1, vec![], vec![]));
        host.refresh(NOW + 7200, &Catalog::none()).unwrap_err();
        let said = row(&db).unwrap().last_error.unwrap();
        assert_eq!(said, "This catalog is older than the one AutoScene has.");
        drop((db, host));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn the_row_is_written_before_the_cache() {
        let tmp = scratch("order");
        let (key, served) = (Key::new(), Arc::new(Served::default()));
        served.index(A, &key, &index(2, vec![entry(HELLO, "1.0.0")], vec![withdrawal("dev.example.leaky", "*")]));
        let (db, host) = host(&tmp, key.keys(), &[A], &served);
        // The row can't be written: no cache is either, as a crash between
        // the two would leave the file without the withdrawals.
        let sql = rusqlite::Connection::open(AppPaths::at(tmp.join("data")).library_db()).unwrap();
        sql.execute_batch(
            "CREATE TRIGGER no_row BEFORE INSERT ON settings WHEN NEW.key = 'plugins_catalog'
             BEGIN SELECT RAISE(ABORT, 'no'); END;",
        )
        .unwrap();
        let taken = host.refresh(NOW, &Catalog::none()).unwrap();
        assert_eq!(seq(&taken), Some(2), "the index fetched is used all the same");
        assert!(row(&db).is_none());
        assert!(!tmp.join("data/cache/catalog").exists());
        sql.execute_batch("DROP TRIGGER no_row;").unwrap();
        drop(sql);
        // The cache can't be written: a file stands where its folder goes.
        std::fs::create_dir_all(tmp.join("data/cache")).unwrap();
        std::fs::write(tmp.join("data/cache/catalog"), b"").unwrap();
        let taken = host.refresh(NOW + 30, &Catalog::none()).unwrap();
        assert_eq!(seq(&taken), Some(2), "the index fetched is used all the same");
        let held = row(&db).unwrap();
        assert_eq!((held.seq, held.withdrawn.len()), (2, 1));
        // At the next start the index is gone, its withdrawals stay, and
        // the next fetch takes the same index again.
        let loaded = host.load();
        assert!(loaded.index().is_none());
        assert!(loaded.withdrawal(&PluginId::parse("dev.example.leaky").unwrap(), "2.0.0").is_some());
        std::fs::remove_file(tmp.join("data/cache/catalog")).unwrap();
        assert_eq!(seq(&host.refresh(NOW + 60, &Catalog::none()).unwrap()), Some(2));
        assert_eq!(seq(&host.load()), Some(2));
        drop((db, host));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_lost_cache_keeps_the_withdrawals() {
        let tmp = scratch("lost");
        let (key, served) = (Key::new(), Arc::new(Served::default()));
        served.index(A, &key, &index(2, vec![entry(HELLO, "1.0.0")], vec![withdrawal("dev.example.leaky", "<=1.0.4")]));
        let (db, host) = host(&tmp, key.keys(), &[A], &served);
        host.refresh(NOW, &Catalog::none()).unwrap();
        assert_eq!(seq(&host.load()), Some(2));
        std::fs::remove_dir_all(tmp.join("data/cache/catalog")).unwrap();
        let loaded = host.load();
        let leaky = PluginId::parse("dev.example.leaky").unwrap();
        assert!(loaded.index().is_none() && !loaded.living(NOW));
        assert!(loaded.withdrawal(&leaky, "1.0.4").is_some());
        assert!(loaded.withdrawal(&leaky, "1.0.5").is_none());
        drop((db, host));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_cache_changed_on_disk_is_not_used() {
        let tmp = scratch("changed");
        let (key, served) = (Key::new(), Arc::new(Served::default()));
        served.index(A, &key, &index(2, vec![entry(HELLO, "1.0.0")], vec![withdrawal("dev.example.leaky", "*")]));
        let (db, host) = host(&tmp, key.keys(), &[A], &served);
        host.refresh(NOW, &Catalog::none()).unwrap();
        // Another index, signed by the same key, in its place: not the one the row names.
        let (bytes, sig) = key.sign(&index(2, vec![entry(HELLO, "9.0.0")], vec![]));
        let dir = tmp.join("data/cache/catalog");
        std::fs::write(dir.join(INDEX), bytes).unwrap();
        std::fs::write(dir.join(SIG), sig).unwrap();
        let loaded = host.load();
        assert!(loaded.index().is_none());
        assert!(loaded.withdrawal(&PluginId::parse("dev.example.leaky").unwrap(), "1.0.0").is_some());
        // A changed byte of the one it names, too.
        host.refresh(NOW + 60, &Catalog::none()).unwrap();
        assert!(host.load().index().is_some());
        let mut bytes = std::fs::read(dir.join(INDEX)).unwrap();
        *bytes.last_mut().unwrap() = b' ';
        std::fs::write(dir.join(INDEX), bytes).unwrap();
        assert!(host.load().index().is_none());
        drop((db, host));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_row_from_a_newer_autoscene_turns_the_catalog_off_and_is_never_written() {
        let tmp = scratch("newer-row");
        let (key, served) = (Key::new(), Arc::new(Served::default()));
        served.index(A, &key, &index(2, vec![entry(HELLO, "1.0.0")], vec![]));
        let paths = AppPaths::at(tmp.join("data"));
        std::fs::create_dir_all(&paths.data).unwrap();
        let db = Arc::new(LibraryDb::open(&paths.library_db()).unwrap());
        let mut later = serde_json::to_value(CatalogState {
            v: STATE_VERSION + 1,
            withdrawn: vec![serde_json::from_value(withdrawal("dev.example.leaky", "*")).unwrap()],
            ..CatalogState::default()
        })
        .unwrap();
        later["somethingNew"] = json!(true);
        let written = later.to_string();
        db.set_setting(ROW, &written).unwrap();
        let host = CatalogHost::new(&paths, db.clone(), key.keys(), vec![A.into()], Arc::new(served.clone()), shelf());
        assert!(!host.ready() && host.newer_row());
        // Nothing is fetched, at start or when the person asks, and the row stays as it is.
        assert!(!host.due(NOW, true, true));
        let held = host.refresh(NOW, &Catalog::none()).unwrap_err();
        assert!(served.asked.lock().is_empty());
        assert_eq!(db.setting(ROW).unwrap().unwrap(), written);
        // What it withdrew still holds, if this AutoScene can read it.
        assert!(held.withdrawal(&PluginId::parse("dev.example.leaky").unwrap(), "1.0.0").is_some());
        assert!(host.load().index().is_none());
        drop((db, host));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_row_from_a_newer_autoscene_never_reads_the_index_kept() {
        let tmp = scratch("newer-row-cache");
        let (key, served) = (Key::new(), Arc::new(Served::default()));
        served.index(A, &key, &index(2, vec![entry(HELLO, "1.0.0")], vec![withdrawal("dev.example.leaky", "*")]));
        let (db, first) = host(&tmp, key.keys(), &[A], &served);
        let taken = first.refresh(NOW, &Catalog::none()).unwrap();
        assert_eq!(seq(&first.load()), Some(2));
        drop(first);
        // A later AutoScene took the same index: its row names the file kept.
        let later = CatalogState { v: STATE_VERSION + 1, ..row(&db).unwrap() };
        db.set_setting(ROW, &serde_json::to_string(&later).unwrap()).unwrap();
        let paths = AppPaths::at(tmp.join("data"));
        let host = CatalogHost::new(&paths, db.clone(), key.keys(), vec![A.into()], Arc::new(served.clone()), shelf());
        assert!(host.newer_row());
        let loaded = host.load();
        assert!(loaded.index().is_none());
        assert!(loaded.withdrawal(&PluginId::parse("dev.example.leaky").unwrap(), "1.0.0").is_some());
        // Nor the one in hand.
        assert!(host.refresh(NOW + 60, &taken).unwrap_err().index().is_none());
        drop((db, host));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_failed_check_keeps_the_index_in_hand() {
        let tmp = scratch("in-hand");
        let (key, served) = (Key::new(), Arc::new(Served::default()));
        served.index(A, &key, &index(2, vec![entry(HELLO, "1.0.0")], vec![]));
        let (db, host) = host(&tmp, key.keys(), &[A], &served);
        let taken = host.refresh(NOW, &Catalog::none()).unwrap();
        // Its file is gone (it couldn't be written, or can't be read now),
        // and the next check fails.
        std::fs::remove_dir_all(tmp.join("data/cache/catalog")).unwrap();
        served.down(A);
        let failed = host.refresh(NOW + 3600, &taken).unwrap_err();
        assert_eq!(seq(&failed), Some(2));
        assert!(failed.living(NOW + 3600));
        assert_eq!(failed.state().last_error.as_deref(), Some(UNREACHABLE));
        // Without it, there is none.
        assert!(host.refresh(NOW + 7200, &Catalog::none()).unwrap_err().index().is_none());
        // One the row doesn't name isn't kept.
        let other = CatalogState { sha256: Some("0".repeat(64)), ..taken.state().clone() };
        let stale = Catalog::new(taken.index().cloned(), other);
        assert!(host.refresh(NOW + 10_800, &stale).unwrap_err().index().is_none());
        drop((db, host));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_catalog_of_one_s_own_keeps_what_it_took_apart() {
        let tmp = scratch("dev-row");
        let (dev_key, real_key, served) = (Key::new(), Key::new(), Arc::new(Served::default()));
        served.index(A, &dev_key, &index(50, vec![], vec![withdrawal(HELLO, "*")]));
        served.index(B, &real_key, &index(3, vec![entry(HELLO, "1.0.0")], vec![]));
        let paths = AppPaths::at(tmp.join("data"));
        std::fs::create_dir_all(&paths.data).unwrap();
        let db = Arc::new(LibraryDb::open(&paths.library_db()).unwrap());
        let fetch = || -> Arc<dyn Fetch> { Arc::new(served.clone()) };
        // A debug build follows the override, on the data folder a release build uses too.
        let dev = CatalogHost::open(&paths, db.clone(), dev_key.keys(), vec![A.into()], (fetch(), shelf()), true);
        let taken = dev.refresh(NOW, &Catalog::none()).unwrap();
        assert_eq!(seq(&taken), Some(50));
        assert!(row(&db).is_none());
        assert!(!tmp.join("data/cache/catalog").exists());
        // The release build takes its own catalog, and the dev one's withdrawal isn't in it.
        let release = CatalogHost::new(&paths, db.clone(), real_key.keys(), vec![B.into()], fetch(), shelf());
        assert!(release.due(NOW, true, true));
        let taken = release.refresh(NOW + 60, &Catalog::none()).unwrap();
        assert_eq!(seq(&taken), Some(3));
        assert!(taken.withdrawal(&PluginId::parse(HELLO).unwrap(), "1.0.0").is_none());
        assert_eq!((row(&db).unwrap().seq, row(&db).unwrap().withdrawn.len()), (3, 0));
        // Each keeps its own, in its own folder.
        assert_eq!(seq(&dev.load()), Some(50));
        assert_eq!(seq(&release.load()), Some(3));
        let dev_row: CatalogState = serde_json::from_str(&db.setting(DEV_ROW).unwrap().unwrap()).unwrap();
        assert_eq!(dev_row.seq, 50);
        assert!(tmp.join("data/cache/catalog-dev").join(INDEX).exists());
        drop((db, dev, release));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_check_is_due_at_start_after_a_day_or_an_hour_after_a_failure() {
        let tmp = scratch("due");
        let (key, served) = (Key::new(), Arc::new(Served::default()));
        let (db, host) = host(&tmp, key.keys(), &[A], &served);
        // Never checked: due at start, and later, but only with the setting on.
        assert!(host.due(NOW, true, true) && host.due(NOW, true, false));
        assert!(!host.due(NOW, false, true));
        let set = |s: CatalogState| db.set_setting(ROW, &serde_json::to_string(&s).unwrap()).unwrap();
        let good = CatalogState { v: 1, fetched_at: NOW, checked_at: NOW, ..CatalogState::default() };
        set(good.clone());
        // A start checks once the last good fetch is an hour old; later checks wait a day.
        assert!(!host.due(NOW + 3599, true, true));
        assert!(host.due(NOW + 3600, true, true));
        assert!(!host.due(NOW + 3600, true, false));
        assert!(!host.due(NOW + DAY - 1, true, false));
        assert!(host.due(NOW + DAY, true, false));
        // A failed check waits an hour before the next, at start too.
        set(good.tried(NOW + DAY, Some(UNREACHABLE.into())));
        assert!(!host.due(NOW + DAY + 3599, true, false) && !host.due(NOW + DAY + 3599, true, true));
        assert!(host.due(NOW + DAY + 3600, true, false));
        drop((db, host));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn times_written_while_the_clock_ran_ahead_count_as_long_ago() {
        let tmp = scratch("due-ahead");
        let (key, served) = (Key::new(), Arc::new(Served::default()));
        let (db, host) = host(&tmp, key.keys(), &[A], &served);
        let set = |s: CatalogState| db.set_setting(ROW, &serde_json::to_string(&s).unwrap()).unwrap();
        // An index taken while the clock was 20 days ahead; the clock is right again.
        let ahead =
            CatalogState { v: 1, fetched_at: NOW + 20 * DAY, checked_at: NOW + 20 * DAY, ..CatalogState::default() };
        set(ahead.clone());
        assert!(host.due(NOW + 2 * DAY, true, true) && host.due(NOW + 2 * DAY, true, false));
        assert!(!host.due(NOW + 2 * DAY, false, true));
        // A failure tried then doesn't hold the next check back either.
        set(CatalogState { fetched_at: NOW, ..ahead.tried(NOW + 20 * DAY, Some(UNREACHABLE.into())) });
        assert!(host.due(NOW + 2 * DAY, true, false));
        drop((db, host));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn without_keys_nothing_is_fetched() {
        let tmp = scratch("no-keys");
        let served = Arc::new(Served::default());
        served.index(A, &Key::new(), &index(2, vec![], vec![]));
        let (db, host) = host(&tmp, CatalogKeys::new(&[]).unwrap(), &[A], &served);
        assert!(!host.ready());
        assert!(!host.due(NOW, true, true));
        assert!(host.refresh(NOW, &Catalog::none()).is_err());
        assert!(served.asked.lock().is_empty());
        assert!(row(&db).is_none());
        drop((db, host));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    /// Release builds don't have the override's code (`mod dev`), so only a
    /// debug build can test it.
    #[cfg(debug_assertions)]
    #[test]
    fn the_override_names_other_addresses_and_more_keys() {
        let tmp = scratch("override");
        std::fs::create_dir_all(&tmp).unwrap();
        let path = tmp.join("catalog.json");
        let key = Key::new();
        let (bytes, sig) = key.sign(&index(2, vec![], vec![]));
        let takes = |keys: &CatalogKeys| catalog::check(&bytes, &sig, keys, &CatalogState::default(), NOW).is_ok();
        // Without one: none, and the compiled keys don't take its index.
        assert!(dev::read(&path).is_none());
        assert!(!takes(&CatalogKeys::builtin()));
        assert_eq!(compiled_urls(), CATALOG_URLS);
        // Its addresses replace the compiled ones, its keys are added.
        let o = json!({"urls": ["http://127.0.0.1:8123"], "keys": [{"role": "online", "key": key.0.pk.to_base64()}]});
        std::fs::write(&path, o.to_string()).unwrap();
        let (keys, urls) = dev::configured(dev::read(&path).unwrap());
        assert!(takes(&keys));
        assert_eq!(urls, ["http://127.0.0.1:8123/"]);
        // No addresses: the compiled ones.
        std::fs::write(&path, json!({"keys": []}).to_string()).unwrap();
        assert_eq!(dev::configured(dev::read(&path).unwrap()).1, CATALOG_URLS);
        // One that can't be read is no override.
        std::fs::write(&path, "{not json").unwrap();
        assert!(dev::read(&path).is_none());
        // Keys that aren't keys are left out, the addresses still taken.
        let bad = json!({"urls": [A], "keys": [{"role": "online", "key": key.0.pk.to_base64()}, {"role": "offline", "key": "RWnope"}]});
        std::fs::write(&path, bad.to_string()).unwrap();
        let (keys, urls) = dev::configured(dev::read(&path).unwrap());
        assert!(!takes(&keys));
        assert_eq!(urls, [A]);
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn the_view_offers_the_newest_version_and_says_what_an_update_adds() {
        let tmp = scratch("view-index");
        let (key, served) = (Key::new(), Arc::new(Served::default()));
        let mut newer = entry(HELLO, "2.0.0");
        newer["engines"] = json!(">=9.0.0");
        let mut future = entry("dev.a.future", "2.0.0");
        future["engines"] = json!(">=9.0.0");
        let mut elsewhere = entry("dev.a.elsewhere", "2.0.0");
        let other = if current_platform() == "linux-x86_64" { "windows-x86_64" } else { "linux-x86_64" };
        elsewhere["platforms"] = json!([other]);
        let plugins = vec![entry(HELLO, "1.1.0"), newer, entry("dev.a.leaky", "1.0.0"), future, elsewhere];
        served.index(A, &key, &index(2, plugins, vec![withdrawal("dev.a.leaky", "*")]));
        let (db, host) = host(&tmp, key.keys(), &[A], &served);
        let catalog = host.refresh(NOW, &Catalog::none()).unwrap();
        let app = Version::parse("1.0.3-BETA").unwrap();
        let hello = PluginId::parse(HELLO).unwrap();
        // Not installed: the newest version this AutoScene runs, and nothing "added".
        let shown = listed(&catalog, &hello, None, &app).unwrap();
        assert_eq!((shown.version.as_str(), shown.runs_here, shown.update), ("1.1.0", true, false));
        assert_eq!((shown.approved, shown.size_bytes), ((NOW - 2 * DAY) as f64 * 1000.0, 4096.0));
        assert!(shown.adds_permissions.is_empty() && shown.installed.is_none());
        // Installed at 1.0.0 with no grants: an update, and what it asks is new.
        let mut installed = super::super::tests::plugin(HELLO, as_plugin::Trust::Reviewed, false);
        let shown = listed(&catalog, &hello, Some(&installed), &app).unwrap();
        assert_eq!((shown.installed.as_deref(), shown.update), (Some("1.0.0"), true));
        assert_eq!(
            (shown.adds_permissions, shown.adds_hosts),
            (vec!["exports:read".to_owned(), "network".to_owned()], vec!["discord.com".to_owned()])
        );
        // Granted already, nothing is new; at 1.1.0 there is no update.
        installed.record.permissions = serde_json::from_value(json!(["exports:read", "network"])).unwrap();
        installed.record.network_hosts = vec!["discord.com".into()];
        installed.record.version = "1.1.0".into();
        let shown = listed(&catalog, &hello, Some(&installed), &app).unwrap();
        assert!(shown.adds_permissions.is_empty() && shown.adds_hosts.is_empty() && !shown.update);
        // A plugin it withdrew whole is listed only while it is installed, and offered nowhere.
        let leaky = PluginId::parse("dev.a.leaky").unwrap();
        assert!(listed(&catalog, &leaky, None, &app).is_none());
        let installed = super::super::tests::plugin("dev.a.leaky", as_plugin::Trust::Reviewed, false);
        let shown = listed(&catalog, &leaky, Some(&installed), &app).unwrap();
        assert!(!shown.runs_here && !shown.update && !shown.needs_newer_app);
        // A newer version that needs a newer AutoScene, or another computer, is no update.
        for (id, needs_newer_app) in [("dev.a.future", true), ("dev.a.elsewhere", false)] {
            let installed = super::super::tests::plugin(id, as_plugin::Trust::Reviewed, false);
            let shown = listed(&catalog, &PluginId::parse(id).unwrap(), Some(&installed), &app).unwrap();
            assert_eq!(shown.installed.as_deref(), Some("1.0.0"));
            assert_eq!((shown.version.as_str(), shown.runs_here), ("2.0.0", false));
            assert_eq!((shown.update, shown.needs_newer_app), (false, needs_newer_app), "{id}");
        }
        drop((db, host));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn the_view_says_how_an_update_would_run_past_what_the_person_agreed_to() {
        use RuntimeKind::{None as Pages, Process, Service, Wasm};
        let app = Version::parse("1.0.3-BETA").unwrap();
        let hello = PluginId::parse(HELLO).unwrap();
        let shown = |agreed: Option<RuntimeKind>, runtime: &str| {
            let mut update = entry(HELLO, "1.1.0");
            update["runtime"] = json!(runtime);
            let catalog = living(vec![update], vec![]);
            let mut installed = super::super::tests::plugin(HELLO, as_plugin::Trust::Reviewed, false);
            let shown = match agreed {
                Some(agreed) => {
                    installed.extra = as_plugin::Extra::agreed(agreed);
                    listed(&catalog, &hello, Some(&installed), &app).unwrap()
                }
                None => listed(&catalog, &hello, None, &app).unwrap(),
            };
            (shown.starts_program, shown.becomes_service, shown.leaves_sandbox)
        };
        assert_eq!(shown(Some(Wasm), "process"), (false, false, true));
        assert_eq!(shown(Some(Wasm), "service"), (false, true, true));
        assert_eq!(shown(Some(Process), "service"), (false, true, false));
        assert_eq!(shown(Some(Pages), "process"), (true, false, false));
        assert_eq!(shown(Some(Pages), "wasm"), (true, false, false));
        // By what the person agreed to, and it may do no more.
        for (agreed, runtime) in [(Process, "process"), (Wasm, "wasm"), (Process, "wasm"), (Service, "process")] {
            assert_eq!(shown(Some(agreed), runtime), (false, false, false), "{agreed:?} to {runtime}");
        }
        // A runtime it doesn't name, or this AutoScene doesn't know, counts as a program.
        for runtime in ["", "quantum"] {
            assert_eq!(shown(Some(Wasm), runtime), (false, false, true), "{runtime}");
            assert_eq!(shown(Some(Pages), runtime), (true, false, false), "{runtime}");
        }
        // Not installed: nothing to go past.
        assert_eq!(shown(None, "process"), (false, false, false));
    }

    #[test]
    fn adds_resource_hosts_compares_with_the_installed_manifest() {
        let tmp = scratch("view-resources");
        let (key, served) = (Key::new(), Arc::new(Served::default()));
        let mut big = entry(HELLO, "1.1.0");
        big["resourceBytes"] = json!(312_000_000u64);
        big["resourceHosts"] = json!(["huggingface.co", "cdn.example.com"]);
        served.index(A, &key, &index(2, vec![big], vec![]));
        let (db, host) = host(&tmp, key.keys(), &[A], &served);
        let catalog = host.refresh(NOW, &Catalog::none()).unwrap();
        let app = Version::parse("1.0.3-BETA").unwrap();
        let hello = PluginId::parse(HELLO).unwrap();
        // Not installed: how much its first run downloads, and nothing "added".
        let shown = listed(&catalog, &hello, None, &app).unwrap();
        assert_eq!((shown.resource_bytes, shown.adds_resource_hosts.len()), (312e6, 0));
        // Installed, its plugin.json naming files from huggingface.co: the other host is new.
        let parse = |v: &Value| as_plugin::manifest::parse_for_any_app(v.to_string().as_bytes()).unwrap().manifest;
        let model = super::super::tests::resource("model.onnx", super::super::tests::HF, 300);
        let mut installed = super::super::tests::plugin(HELLO, as_plugin::Trust::Reviewed, false);
        installed.manifest = Some(parse(&super::super::tests::with_resources(HELLO, json!([model]))));
        let shown = listed(&catalog, &hello, Some(&installed), &app).unwrap();
        assert_eq!(shown.adds_resource_hosts, ["cdn.example.com"]);
        let sent = serde_json::to_value(&shown).unwrap();
        assert_eq!((&sent["resourceBytes"], &sent["addsResourceHosts"]), (&json!(312e6), &json!(["cdn.example.com"])));
        // Its plugin.json naming none: both are new; so they are when it can't be read.
        installed.manifest = Some(parse(&super::super::tests::manifest(HELLO, false)));
        let shown = listed(&catalog, &hello, Some(&installed), &app).unwrap();
        assert_eq!(shown.adds_resource_hosts, ["huggingface.co", "cdn.example.com"]);
        installed.manifest = None;
        assert_eq!(listed(&catalog, &hello, Some(&installed), &app).unwrap().adds_resource_hosts.len(), 2);
        drop((db, host));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    fn jobs() -> JobRunner {
        JobRunner::new(&[(Class::Plugin, 1)], |_| {})
    }

    fn host_scratch(name: &str) -> PathBuf {
        let tmp = std::env::temp_dir().join(format!("autoscene-catalog-host-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        tmp
    }

    #[test]
    fn the_view_says_a_newer_autoscene_is_needed_when_the_check_said_so() {
        let tmp = host_scratch("needs-newer");
        let (key, site) = (Key::new(), Arc::new(Served::default()));
        let (lib, host, _) = host_cataloged(&tmp, NoStart, &key, &site);
        let mut later = index(1, vec![], vec![]);
        later["catalogVersion"] = json!(2);
        site.index(CATALOG_AT, &key, &later);
        let (view, _, _) = apply(&host, &jobs(), NOW, true);
        assert_eq!(view.status.error.as_deref(), Some(NEEDS_NEWER_APP));
        assert!(view.status.needs_newer_app && view.status.ready && !host.catalog.newer_row());
        // Any other failure doesn't say it.
        site.down(CATALOG_AT);
        let (view, _, _) = apply(&host, &jobs(), NOW + 3600, true);
        assert!(view.status.error.is_some() && !view.status.needs_newer_app);
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn the_loop_checks_at_start_unless_the_last_fetch_is_fresh_then_daily() {
        let tmp = host_scratch("tick");
        let (key, site) = (Key::new(), Arc::new(Served::default()));
        let (lib, host, _) = host_cataloged(&tmp, NoStart, &key, &site);
        site.index(CATALOG_AT, &key, &index(1, vec![], vec![]));
        let jobs = jobs();
        let asked = || site.asked.lock().len();
        assert!(tick(&host, &jobs, NOW, false, true).is_none(), "the setting is off");
        assert_eq!(asked(), 0);
        assert!(tick(&host, &jobs, NOW, true, true).is_some());
        assert_eq!(host.store.catalog().state().seq, 1);
        // A start within the hour fetches nothing; one later does.
        let n = asked();
        assert!(tick(&host, &jobs, NOW + 1800, true, true).is_none());
        assert_eq!(asked(), n);
        assert!(tick(&host, &jobs, NOW + 3600, true, true).is_some());
        // After the start, a day passes between checks.
        assert!(tick(&host, &jobs, NOW + 7200, true, false).is_none());
        assert!(tick(&host, &jobs, NOW + 3600 + DAY, true, false).is_some());
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn turning_the_checks_on_checks_when_one_is_due() {
        let tmp = host_scratch("turned-on");
        let (key, site) = (Key::new(), Arc::new(Served::default()));
        let (lib, host, _) = host_cataloged(&tmp, NoStart, &key, &site);
        site.index(CATALOG_AT, &key, &index(1, vec![], vec![withdrawal(HELLO, "*")]));
        let jobs = jobs();
        // Left on, or turned off: nothing.
        assert!(turned_on(&host, &jobs, NOW, true, true).is_none());
        assert!(turned_on(&host, &jobs, NOW, true, false).is_none());
        assert!(turned_on(&host, &jobs, NOW, false, false).is_none());
        assert!(site.asked.lock().is_empty());
        // Turned on, never checked: it checks, and the store judges by it.
        assert!(turned_on(&host, &jobs, NOW, false, true).is_some());
        assert!(host.store.catalog().withdrawal(&PluginId::parse(HELLO).unwrap(), "1.0.0").is_some());
        // Turned on again within the day: not due.
        assert!(turned_on(&host, &jobs, NOW + 3600, false, true).is_none());
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn two_checks_at_once_stop_a_withdrawn_plugin_once() {
        let tmp = host_scratch("at-once");
        let (key, site) = (Key::new(), Arc::new(Served::default()));
        let (lib, host, _) = host_cataloged(&tmp, NoStart, &key, &site);
        let t = super::super::tests::folder(&host, &tmp, &super::super::tests::manifest("dev.a.leaky", false));
        host.store.commit(t, true, true).unwrap();
        let jobs = jobs();
        // Every other index withdraws it: each of those is new to one check only.
        for round in 0..20 {
            let withdrawn = if round % 2 == 0 { vec![withdrawal("dev.a.leaky", "*")] } else { vec![] };
            site.index(CATALOG_AT, &key, &index(round + 1, vec![], withdrawn));
            let barrier = Barrier::new(2);
            let said: Vec<Vec<PluginId>> = std::thread::scope(|s| {
                let both = [0, 1].map(|_| {
                    s.spawn(|| {
                        barrier.wait();
                        apply(&host, &jobs, NOW + round, true).1
                    })
                });
                both.map(|h| h.join().unwrap()).into()
            });
            let total: usize = said.iter().map(Vec::len).sum();
            assert_eq!(total, usize::from(round % 2 == 0), "round {round}: {said:?}");
            assert_eq!(host.withdrawn().is_empty(), round % 2 == 1);
            assert_eq!(host.store.catalog().state().seq, round + 1, "the newest is the one in hand");
        }
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    // Getting a plugin's package (`CatalogHost::get`).

    /// What a get of the [`Shelf`] does first, as its package comes: a
    /// test's hand in it.
    pub type During = Arc<dyn Fn(&Entry, &CancelToken) + Send + Sync>;

    /// Packages by `<id>-<version>`, as the catalog's addresses serve them
    /// (whatever the entry pins): each is copied into the folder as the
    /// entry's file, with the mark the store writes once a download checked
    /// out. Keeps what was asked for.
    #[derive(Default)]
    pub struct Shelf {
        pub packages: Mutex<HashMap<String, Vec<u8>>>,
        pub asked: Mutex<Vec<String>>,
        pub during: Mutex<Option<During>>,
    }

    impl Shelf {
        /// Serves `bytes` as the package of `id` at `version`.
        fn put(&self, id: &str, version: &str, bytes: &[u8]) {
            self.packages.lock().insert(format!("{id}-{version}"), bytes.to_vec());
        }
    }

    impl GetPackage for Arc<Shelf> {
        fn get(
            &self,
            e: &Entry,
            dir: &Path,
            cancel: &CancelToken,
            progress: &mut dyn FnMut(u64, u64),
        ) -> PluginResult<PathBuf> {
            let (asked, file) = (format!("{}-{}", e.id, e.version), package_file(e));
            self.asked.lock().push(asked.clone());
            let during = self.during.lock().clone();
            if let Some(during) = during {
                during(e, cancel);
            }
            // None at its address fails, cancelled or not.
            let bytes = self.packages.lock().get(&asked).cloned();
            let bytes = bytes.ok_or_else(|| PluginError::Download { what: e.name.clone(), detail: asked })?;
            if cancel.is_cancelled() {
                return Err(PluginError::Cancelled);
            }
            let total = bytes.len() as u64;
            progress(0, total);
            std::fs::create_dir_all(dir)?;
            std::fs::write(dir.join(&file), &bytes)?;
            std::fs::write(dir.join(format!("{file}.verified")), package::sha256_hex(&bytes))?;
            progress(total, total);
            Ok(dir.join(file))
        }
    }

    /// Nothing on it.
    pub fn shelf() -> Arc<dyn GetPackage> {
        Arc::new(Arc::new(Shelf::default()))
    }

    /// The package of the plugin `id` at `version` (the plugins.rs tests'
    /// `manifest`), as asp packs it, unsigned, with `more` at the end of its
    /// script: its bytes, and the SHA-256 of its lock.
    fn packed(id: &str, version: &str, more: &str) -> (Vec<u8>, String) {
        let mut m = super::super::tests::manifest(id, false);
        m["version"] = json!(version);
        zipped(&[
            ("plugin.json", serde_json::to_vec_pretty(&m).unwrap()),
            ("main.py", format!("print('hello'){more}\n").into_bytes()),
            ("main.py.lock", b"version = 1\n".to_vec()),
        ])
    }

    /// The package of `files`, and the SHA-256 of its lock.
    fn zipped(files: &[(&str, Vec<u8>)]) -> (Vec<u8>, String) {
        let listed = files.iter().map(|(path, bytes)| as_plugin::LockedFile {
            path: (*path).into(),
            size: bytes.len() as u64,
            sha256: package::sha256_hex(bytes),
            exec: false,
        });
        let lock = as_plugin::FilesLock { lock_version: package::LOCK_VERSION, files: listed.collect() };
        let lock = serde_json::to_vec(&lock).unwrap();
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, bytes) in files.iter().map(|(p, b)| (*p, b.as_slice())).chain([(package::LOCK, lock.as_slice())]) {
            zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut zip, bytes).unwrap();
        }
        (zip.finish().unwrap().into_inner(), package::sha256_hex(&lock))
    }

    /// The entry of the package `bytes`, whose lock has the SHA-256 `lock`,
    /// as the catalog's CI lists it: the plugin `id` at `version`.
    fn entry_of(id: &str, version: &str, (bytes, lock): &(Vec<u8>, String)) -> Value {
        let mut e = entry(id, version);
        e["lock"] = json!(lock);
        e["package"]["size"] = json!(bytes.len());
        e["package"]["sha256"] = json!(package::sha256_hex(bytes));
        e
    }

    /// A catalog in hand that lists `plugins` and withdraws `withdrawn`,
    /// published `published` and running out at `expires`.
    fn held(plugins: Vec<Value>, withdrawn: Vec<Value>, published: u64, expires: u64) -> Arc<Catalog> {
        let mut index = index(1, plugins, withdrawn);
        (index["published"], index["expires"]) = (json!(published), json!(expires));
        let index: catalog::Index = serde_json::from_value(index).unwrap();
        let state = CatalogState {
            v: STATE_VERSION,
            epoch: index.epoch,
            seq: index.seq,
            published,
            expires,
            withdrawn: index.withdrawn.clone(),
            ..CatalogState::default()
        };
        Arc::new(Catalog::new(Some(index), state))
    }

    /// [`held`], living now: published a day ago, running out in 29 days.
    /// The store judges by the clock, so these tests use it too.
    fn living(plugins: Vec<Value>, withdrawn: Vec<Value>) -> Arc<Catalog> {
        let now = catalog::now();
        held(plugins, withdrawn, now - DAY, now + 29 * DAY)
    }

    /// [`held`], run out a day ago.
    fn run_out(plugins: Vec<Value>) -> Arc<Catalog> {
        let now = catalog::now();
        held(plugins, Vec::new(), now - 31 * DAY, now - DAY)
    }

    /// A store and its catalog in a folder of their own, whose packages
    /// come off `shelf`.
    struct Getting {
        tmp: PathBuf,
        paths: AppPaths,
        db: Arc<LibraryDb>,
        store: Arc<Store>,
        host: CatalogHost,
        shelf: Arc<Shelf>,
    }

    impl Getting {
        fn new(name: &str) -> Self {
            let tmp = scratch(&format!("get-{name}"));
            let paths = AppPaths::at(tmp.join("data"));
            std::fs::create_dir_all(&paths.data).unwrap();
            let db = Arc::new(LibraryDb::open(&paths.library_db()).unwrap());
            let app = Version::parse("1.0.3-BETA").unwrap();
            let secrets = Arc::new(as_plugin::MemorySecrets::default());
            let store = Arc::new(Store::open(&paths, db.clone(), app, secrets).unwrap());
            let shelf = Arc::new(Shelf::default());
            let host = Self::host(&paths, &db, Arc::new(shelf.clone()));
            Self { tmp, paths, db, store, host, shelf }
        }

        /// A catalog of `paths` whose packages come through `packages`.
        /// Nothing is ever fetched: the catalog is the one set in the store.
        fn host(paths: &AppPaths, db: &Arc<LibraryDb>, packages: Arc<dyn GetPackage>) -> CatalogHost {
            let fetch = Arc::new(Arc::new(Served::default()));
            CatalogHost::new(paths, db.clone(), Key::new().keys(), vec![CATALOG_AT.into()], fetch, packages)
        }

        fn get(&self, id: &str) -> PluginResult<Ticket> {
            self.get_with(id, |_, _| {})
        }

        /// A get of `id` as `catalog_get` makes one: begun, got and ended,
        /// with `progress` as its package comes.
        fn get_with(&self, id: &str, progress: impl FnMut(u64, u64)) -> PluginResult<Ticket> {
            let download = self.host.begin(&self.store, &PluginId::parse(id).unwrap())?;
            let t = self.host.get(&self.store, &download, progress)?;
            if download.end() {
                self.store.discard(t);
                return Err(PluginError::Cancelled);
            }
            Ok(t)
        }

        /// The names in `plugins/.staging` and `cache/plugin-downloads`.
        fn left(&self) -> Vec<String> {
            let names = |dir: PathBuf| -> Vec<String> {
                let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
                entries.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect()
            };
            [names(self.paths.plugins().join(".staging")), names(self.paths.plugin_downloads())].concat()
        }

        fn done(self) {
            let Self { tmp, paths, db, store, host, shelf } = self;
            drop((paths, db, store, host, shelf));
            std::fs::remove_dir_all(&tmp).unwrap();
        }
    }

    /// Waits until `flag` is set (a get in another thread came that far).
    fn wait_for(flag: &AtomicBool) {
        let start = Instant::now();
        while !flag.load(Ordering::SeqCst) {
            assert!(start.elapsed() < Duration::from_secs(10), "the get never started");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// A [`During`] that holds the get of `id` until it is cancelled,
    /// having set `started`.
    fn holding(id: &'static str, started: &Arc<AtomicBool>) -> During {
        let started = started.clone();
        Arc::new(move |e: &Entry, cancel: &CancelToken| {
            if e.id.as_str() == id {
                started.store(true, Ordering::SeqCst);
                let start = Instant::now();
                while !cancel.is_cancelled() && start.elapsed() < Duration::from_secs(10) {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        })
    }

    #[test]
    fn a_catalog_package_is_got_checked_and_reviewed() {
        let g = Getting::new("reviewed");
        let v110 = packed(HELLO, "1.1.0", "");
        g.shelf.put(HELLO, "1.1.0", &v110.0);
        g.store.set_catalog(living(vec![entry_of(HELLO, "1.1.0", &v110)], vec![]));
        let mut came = Vec::new();
        let t = g.get_with(HELLO, |done, total| came.push((done, total))).unwrap();
        assert_eq!(*g.shelf.asked.lock(), [format!("{HELLO}-1.1.0")]);
        let size = v110.0.len() as u64;
        assert_eq!(came, [(0, size), (size, size)]);
        // The version it lists, reviewed by it, built where it says.
        assert_eq!((t.manifest.id.as_str(), t.manifest.version.as_str()), (HELLO, "1.1.0"));
        assert_eq!(t.verdict.trust, as_plugin::Trust::Reviewed);
        let built = t.approval.clone().unwrap();
        assert_eq!(
            (built.repo.as_str(), built.commit.as_str(), built.path.as_str()),
            ("https://github.com/tcmmi/autoscene", "c".repeat(40).as_str(), "examples/plugins/hello-export")
        );
        assert!(t.previous.is_none());
        // It installs as reviewed: no setting for others, no typed yes.
        let p = g.store.commit(t, false, false).unwrap();
        assert_eq!((p.record.version.as_str(), p.record.trust), ("1.1.0", as_plugin::Trust::Reviewed));
        // A newer version is an update of it.
        let v120 = packed(HELLO, "1.2.0", "");
        g.shelf.put(HELLO, "1.2.0", &v120.0);
        g.store.set_catalog(living(vec![entry_of(HELLO, "1.1.0", &v110), entry_of(HELLO, "1.2.0", &v120)], vec![]));
        let t = g.get(HELLO).unwrap();
        assert_eq!(t.manifest.version, "1.2.0");
        assert_eq!(t.previous.as_ref().map(|r| r.version.as_str()), Some("1.1.0"));
        assert_eq!(g.store.commit(t, false, false).unwrap().record.version, "1.2.0");
        g.done();
    }

    /// The review of the package `bytes` as `host` keeps one, from the file
    /// `name` in `tmp`: its ticket's id.
    fn reviewing(host: &PluginHost, tmp: &Path, name: &str, (bytes, _): &(Vec<u8>, String)) -> String {
        let path = tmp.join(name);
        std::fs::write(&path, bytes).unwrap();
        let t = host.store.stage(&path).unwrap();
        let id = t.id.to_string();
        host.keep(t);
        id
    }

    #[test]
    fn an_update_out_of_the_sandbox_takes_the_persons_own_yes_whatever_the_page_says() {
        // The test guest (`cargo xtask wasm-guests`): the installed version runs it.
        let Some(guest) = as_testkit::fixture_file("wasm/guest.wasm") else { return };
        let tmp = host_scratch("out-of-the-sandbox");
        std::fs::create_dir_all(&tmp).unwrap();
        let (lib, host) = super::super::tests::host_in(&tmp);
        let m = super::super::tests::wasm_manifest(HELLO, None);
        let sandboxed =
            zipped(&[("plugin.json", m.to_string().into_bytes()), ("plugin.wasm", std::fs::read(guest).unwrap())]);
        // The same permissions and hosts, as a program with the person's rights.
        let program = packed(HELLO, "1.1.0", "");
        let listed = vec![entry_of(HELLO, "1.0.0", &sandboxed), entry_of(HELLO, "1.1.0", &program)];
        host.store.set_catalog(living(listed, vec![]));

        let first = reviewing(&host, &tmp, "first.asp", &sandboxed);
        let p = super::super::install(&host, &first, false, false, |q| panic!("a new plugin asked {q:?}")).unwrap();
        assert_eq!(p.extra.agreed_runtime(), RuntimeKind::Wasm);

        let update = reviewing(&host, &tmp, "update.asp", &program);
        // The page's consent is no answer: the question comes whatever it says.
        for page in [false, true] {
            let mut asked = Vec::new();
            let no = super::super::install(&host, &update, false, page, |q| {
                asked.push(q);
                false
            });
            assert_eq!(no.unwrap_err().code, ErrorCode::Cancelled);
            let [(title, text, yes)] = asked.as_slice() else { panic!("{asked:?}") };
            assert_eq!((title.as_str(), *yes), (format!("Update Plugin {HELLO}?").as_str(), "Update"));
            let says = "AutoScene reviewed this version. The installed version ran in a sandbox. This version \
                        runs with your user rights: it can read, change and send anything you can.";
            assert!(text.starts_with(says), "{text}");
            assert!(text.contains("Version: 1.1.0 (1.0.0 is installed)"), "{text}");
            let now = host.store.get(&PluginId::parse(HELLO).unwrap()).unwrap();
            assert_eq!((now.record.version.as_str(), now.extra.agreed_runtime()), ("1.0.0", RuntimeKind::Wasm));
        }
        // A no kept the review: the yes installs it, and that is what was agreed to.
        let p = super::super::install(&host, &update, false, false, |_| true).unwrap();
        assert_eq!((p.record.version.as_str(), p.extra.agreed_runtime()), ("1.1.0", RuntimeKind::Process));
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_reviewed_update_asks_only_when_it_may_do_more() {
        let tmp = host_scratch("asks-when-more");
        std::fs::create_dir_all(&tmp).unwrap();
        let (lib, host) = super::super::tests::host_in(&tmp);
        let files = |m: &Value| {
            zipped(&[
                ("plugin.json", serde_json::to_vec_pretty(m).unwrap()),
                ("main.py", b"print('hello')\n".to_vec()),
                ("main.py.lock", b"version = 1\n".to_vec()),
                ("ui/main.html", b"<p>hi</p>".to_vec()),
            ])
        };
        let mut m = super::super::tests::manifest(HELLO, false);
        let v100 = files(&m);
        // Asks for what the installed version never did.
        m["version"] = json!("1.1.0");
        m["permissions"] = json!(["network"]);
        m["network"] = json!({"hosts": ["discord.com"]});
        let v110 = files(&m);
        // The same again: the person agreed to that with 1.1.0.
        m["version"] = json!("1.2.0");
        let v120 = files(&m);
        // Keeps its program running now.
        m["version"] = json!("1.3.0");
        (m["manifestVersion"], m["engines"]["protocol"]) = (json!(2), json!(2));
        m["runtime"]["type"] = json!("service");
        m["contributes"]["pages"] = json!([{"id": "main", "title": "Main", "entry": "ui/main.html"}]);
        let v130 = files(&m);
        let listed = [("1.0.0", &v100), ("1.1.0", &v110), ("1.2.0", &v120), ("1.3.0", &v130)];
        host.store.set_catalog(living(listed.iter().map(|(v, package)| entry_of(HELLO, v, package)).collect(), vec![]));

        let quietly = |name: &str, package| {
            let t = reviewing(&host, &tmp, name, package);
            super::super::install(&host, &t, false, false, |q| panic!("it asked {q:?}")).unwrap();
        };
        let asking = |name: &str, package| {
            let t = reviewing(&host, &tmp, name, package);
            let mut text = String::new();
            let yes = |(_, said, _): super::super::Question| {
                text = said;
                true
            };
            super::super::install(&host, &t, false, false, yes).unwrap();
            text
        };
        quietly("1.0.0.asp", &v100);
        let text = asking("1.1.0.asp", &v110);
        assert!(
            text.contains("\n\nNew in this version:\n· Talks to the internet\n· Talks to discord.com\n\n"),
            "{text}"
        );
        quietly("1.2.0.asp", &v120);
        let text = asking("1.3.0.asp", &v130);
        assert!(text.contains(" It now keeps its program running while its page is open."), "{text}");
        assert!(!text.contains("New in this version"), "{text}");
        let p = host.store.get(&PluginId::parse(HELLO).unwrap()).unwrap();
        assert_eq!((p.record.version.as_str(), p.extra.agreed_runtime()), ("1.3.0", RuntimeKind::Service));
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_running_plugin_is_refused_before_the_question_and_a_yes_a_run_cut_off_is_asked_again() {
        let tmp = host_scratch("question-while-running");
        std::fs::create_dir_all(&tmp).unwrap();
        let (lib, host) = super::super::tests::host_in(&tmp);
        let mut m = super::super::tests::manifest(HELLO, false);
        let files = |m: &Value| {
            zipped(&[
                ("plugin.json", serde_json::to_vec_pretty(m).unwrap()),
                ("main.py", b"print('hello')\n".to_vec()),
                ("main.py.lock", b"version = 1\n".to_vec()),
            ])
        };
        let v100 = files(&m);
        m["version"] = json!("1.1.0");
        m["permissions"] = json!(["network"]);
        let v110 = files(&m);
        let listed = vec![entry_of(HELLO, "1.0.0", &v100), entry_of(HELLO, "1.1.0", &v110)];
        host.store.set_catalog(living(listed, vec![]));
        let first = reviewing(&host, &tmp, "1.0.0.asp", &v100);
        super::super::install(&host, &first, false, false, |q| panic!("it asked {q:?}")).unwrap();

        let id = PluginId::parse(HELLO).unwrap();
        let (job, update) = (as_core::JobId::new(), reviewing(&host, &tmp, "1.1.0.asp", &v110));
        let start = || host.busy.lock().running.insert(job, id.clone());
        let end = || host.busy.lock().running.remove(&job);
        let running = |e: crate::error::AppError| {
            assert_eq!(e.code, ErrorCode::NotReady, "{}", e.message);
            assert!(e.message.ends_with("is running. Wait for it to finish, or stop it in Activity."), "{}", e.message);
        };

        // While it runs nothing is asked: a yes couldn't take effect.
        start();
        running(super::super::install(&host, &update, false, false, |q| panic!("it asked {q:?}")).unwrap_err());
        end();

        // A run that starts during the question: the yes is lost, the review kept.
        let lost = super::super::install(&host, &update, false, false, |_| {
            start();
            true
        });
        running(lost.unwrap_err());
        end();
        let kept = host.take(&update).unwrap();
        assert!(!kept.confirmed);
        host.keep(kept);

        // Asked again, and a no stands, whatever was answered before.
        let mut asked = 0;
        let no = super::super::install(&host, &update, false, false, |_| {
            asked += 1;
            false
        });
        assert_eq!((no.unwrap_err().code, asked), (ErrorCode::Cancelled, 1));
        assert_eq!(host.store.get(&id).unwrap().record.version, "1.0.0");

        let p = super::super::install(&host, &update, false, false, |_| true).unwrap();
        assert_eq!(p.record.version, "1.1.0");
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_package_that_is_not_its_entry_is_refused() {
        let g = Getting::new("not-its-entry");
        let (v100, v110) = (packed(HELLO, "1.0.0", ""), packed(HELLO, "1.1.0", ""));
        let other = packed("dev.a.other", "1.1.0", "");
        // The others are listed too, so the catalog reviewed them: still not what 1.1.0's entry names.
        let listed = vec![
            entry_of(HELLO, "1.0.0", &v100),
            entry_of(HELLO, "1.1.0", &v110),
            entry_of("dev.a.other", "1.1.0", &other),
        ];
        g.store.set_catalog(living(listed, vec![]));
        let changed = packed(HELLO, "1.1.0", "  # changed");
        for (what, bytes) in [("another id", &other.0), ("another version", &v100.0), ("another lock", &changed.0)] {
            g.shelf.put(HELLO, "1.1.0", bytes);
            let refused = g.get(HELLO).unwrap_err();
            assert!(matches!(refused, PluginError::Invalid(_)), "{what}: {refused:?}");
            assert_eq!(refused.to_string(), NOT_LISTED, "{what}");
            // Nothing of it stays: not staged, not downloaded.
            assert_eq!(g.left(), Vec::<String>::new(), "{what}");
        }
        // The one it lists is got.
        g.shelf.put(HELLO, "1.1.0", &v110.0);
        let t = g.get(HELLO).unwrap();
        assert_eq!(t.manifest.version, "1.1.0");
        g.store.discard(t);
        g.done();
    }

    #[test]
    fn an_older_or_the_same_version_is_never_offered() {
        let g = Getting::new("older");
        let (v100, v110) = (packed(HELLO, "1.0.0", ""), packed(HELLO, "1.1.0", ""));
        g.shelf.put(HELLO, "1.1.0", &v110.0);
        g.store.set_catalog(living(vec![entry_of(HELLO, "1.0.0", &v100), entry_of(HELLO, "1.1.0", &v110)], vec![]));
        g.store.commit(g.get(HELLO).unwrap(), false, false).unwrap();
        let asked = g.shelf.asked.lock().len();
        // The same version is installed: nothing is got.
        assert_eq!(g.get(HELLO).unwrap_err().to_string(), "Version 1.1.0 is installed already.");
        // Only an older one offered: never a downgrade.
        g.store.set_catalog(living(vec![entry_of(HELLO, "1.0.0", &v100)], vec![]));
        assert_eq!(g.get(HELLO).unwrap_err().to_string(), "Version 1.1.0 is installed already.");
        // Newer ones this AutoScene can't run, or not on this computer: not offered either.
        let v200 = packed(HELLO, "2.0.0", "");
        let mut future = entry_of(HELLO, "2.0.0", &v200);
        future["engines"] = json!(">=9.0.0");
        g.store.set_catalog(living(vec![future], vec![]));
        assert!(matches!(g.get(HELLO), Err(PluginError::TooNew(_))));
        let mut elsewhere = entry_of(HELLO, "2.0.0", &v200);
        let other = if current_platform() == "linux-x86_64" { "windows-x86_64" } else { "linux-x86_64" };
        elsewhere["platforms"] = json!([other]);
        g.store.set_catalog(living(vec![elsewhere], vec![]));
        assert!(matches!(g.get(HELLO), Err(PluginError::Platform)));
        // One it doesn't list at all.
        let unknown = g.get("dev.a.unknown").unwrap_err();
        assert_eq!(unknown.to_string(), "AutoScene's plugin catalog doesn't list dev.a.unknown.");
        assert_eq!(g.shelf.asked.lock().len(), asked);
        assert_eq!(g.store.get(&PluginId::parse(HELLO).unwrap()).unwrap().record.version, "1.1.0");
        g.done();
    }

    #[test]
    fn nothing_installs_from_a_catalog_past_its_expiry_or_a_withdrawn_version() {
        let g = Getting::new("expiry");
        let (v100, v110) = (packed(HELLO, "1.0.0", ""), packed(HELLO, "1.1.0", ""));
        g.shelf.put(HELLO, "1.0.0", &v100.0);
        g.shelf.put(HELLO, "1.1.0", &v110.0);
        let both = || vec![entry_of(HELLO, "1.0.0", &v100), entry_of(HELLO, "1.1.0", &v110)];
        // No index, or one that ran out.
        for catalog in [Arc::new(Catalog::none()), run_out(both())] {
            g.store.set_catalog(catalog);
            assert_eq!(g.get(HELLO).unwrap_err().to_string(), OUT_OF_DATE);
        }
        // Every version withdrawn: it says why.
        g.store.set_catalog(living(both(), vec![withdrawal(HELLO, "*")]));
        let withdrawn = g.get(HELLO).unwrap_err();
        assert!(matches!(withdrawn, PluginError::Withdrawn { .. }), "{withdrawn:?}");
        assert_eq!(withdrawn.to_string(), "AutoScene withdrew Hello Export. It sent export paths to a server.");
        assert!(g.shelf.asked.lock().is_empty());
        // The newest withdrawn: the version before it is got, never it.
        g.store.set_catalog(living(both(), vec![withdrawal(HELLO, ">=1.1.0")]));
        let t = g.get(HELLO).unwrap();
        assert_eq!(t.manifest.version, "1.0.0");
        g.store.discard(t);
        // Withdrawn while it came: its check refuses it.
        let (store, withdrawing) = (g.store.clone(), living(both(), vec![withdrawal(HELLO, "*")]));
        g.store.set_catalog(living(both(), vec![]));
        *g.shelf.during.lock() =
            Some(Arc::new(move |_: &Entry, _: &CancelToken| store.set_catalog(withdrawing.clone())));
        assert!(matches!(g.get(HELLO), Err(PluginError::Withdrawn { .. })));
        // Run out while it came: out of date.
        let (store, out) = (g.store.clone(), run_out(both()));
        g.store.set_catalog(living(both(), vec![]));
        *g.shelf.during.lock() = Some(Arc::new(move |_: &Entry, _: &CancelToken| store.set_catalog(out.clone())));
        assert_eq!(g.get(HELLO).unwrap_err().to_string(), OUT_OF_DATE);
        assert_eq!(g.left(), Vec::<String>::new());
        g.done();
    }

    #[test]
    fn one_download_per_plugin_at_a_time_and_cancel_ends_it() {
        let g = Getting::new("one-at-a-time");
        const POSTER: &str = "dev.a.poster";
        let (hello, poster) = (packed(HELLO, "1.1.0", ""), packed(POSTER, "1.0.0", ""));
        g.shelf.put(HELLO, "1.1.0", &hello.0);
        g.shelf.put(POSTER, "1.0.0", &poster.0);
        g.store.set_catalog(living(vec![entry_of(HELLO, "1.1.0", &hello), entry_of(POSTER, "1.0.0", &poster)], vec![]));
        // Hello Export's package comes until it is cancelled.
        let started = Arc::new(AtomicBool::new(false));
        *g.shelf.during.lock() = Some(holding(HELLO, &started));
        std::thread::scope(|s| {
            let first = s.spawn(|| g.get(HELLO));
            wait_for(&started);
            // A second get of it is refused while the first runs (as no run: it isn't in Activity).
            let second = g.get(HELLO).unwrap_err();
            assert!(matches!(&second, PluginError::Invalid(_)), "{second:?}");
            assert_eq!(second.to_string(), "AutoScene is getting Hello Export already.");
            // Another plugin's comes meanwhile.
            let t = g.get(POSTER).unwrap();
            g.store.discard(t);
            g.host.cancel(&PluginId::parse(HELLO).unwrap());
            assert!(matches!(first.join().unwrap(), Err(PluginError::Cancelled)));
        });
        // Once it ended, it may be got again.
        *g.shelf.during.lock() = None;
        g.store.discard(g.get(HELLO).unwrap());
        // Cancelled once its package came, before its check was done: no ticket, and nothing stays.
        let id = PluginId::parse(HELLO).unwrap();
        let cancel_once_it_came = |done, total| {
            if done == total {
                g.host.cancel(&id);
            }
        };
        let cancelled = g.get_with(HELLO, cancel_once_it_came);
        assert!(matches!(cancelled, Err(PluginError::Cancelled)), "{cancelled:?}");
        assert_eq!(g.left(), Vec::<String>::new());
        // Also when its checks then fail (the package isn't the one listed): the cancel wins.
        g.shelf.put(HELLO, "1.1.0", &poster.0);
        let cancelled = g.get_with(HELLO, cancel_once_it_came);
        assert!(matches!(cancelled, Err(PluginError::Cancelled)), "{cancelled:?}");
        assert_eq!(g.left(), Vec::<String>::new());
        g.shelf.put(HELLO, "1.1.0", &hello.0);
        // Cancelled once its package was checked, while its review was made: it ends as cancelled.
        let download = g.host.begin(&g.store, &id).unwrap();
        let t = g.host.get(&g.store, &download, |_, _| {}).unwrap();
        g.host.cancel(&id);
        assert!(download.end());
        g.store.discard(t);
        // A cancel once it ended finds no get: the review stands, and the next get is its own.
        let download = g.host.begin(&g.store, &id).unwrap();
        let t = g.host.get(&g.store, &download, |_, _| {}).unwrap();
        assert!(!download.end());
        g.host.cancel(&id);
        let next = g.host.begin(&g.store, &id).unwrap();
        assert!(!next.cancelled());
        drop(next);
        g.store.discard(t);
        // Cancelled while it failed for another reason (an address that never answered): cancelled.
        g.shelf.packages.lock().clear();
        *g.shelf.during.lock() = Some(Arc::new(|_: &Entry, cancel: &CancelToken| cancel.cancel()));
        assert!(matches!(g.get(HELLO), Err(PluginError::Cancelled)));
        *g.shelf.during.lock() = None;
        g.shelf.put(HELLO, "1.1.0", &hello.0);
        // Cancelling none is nothing.
        g.host.cancel(&id);
        g.store.discard(g.get(HELLO).unwrap());
        g.done();
    }

    #[test]
    fn the_download_is_removed_once_it_is_staged() {
        let g = Getting::new("removed");
        let v110 = packed(HELLO, "1.1.0", "");
        g.shelf.put(HELLO, "1.1.0", &v110.0);
        g.store.set_catalog(living(vec![entry_of(HELLO, "1.1.0", &v110)], vec![]));
        let downloads = g.paths.plugin_downloads();
        // The package and the store's mark of it go; what was staged stays for the review.
        let t = g.get(HELLO).unwrap();
        assert!(downloads.is_dir());
        assert_eq!(std::fs::read_dir(&downloads).unwrap().count(), 0);
        assert!(t.staged.as_ref().unwrap().join("plugin.json").is_file());
        g.store.discard(t);
        // Also when its check fails.
        g.shelf.put(HELLO, "1.1.0", &packed(HELLO, "1.1.0", "  # changed").0);
        assert!(g.get(HELLO).is_err());
        assert_eq!(g.left(), Vec::<String>::new());
        // What a get that never finished left goes at the start.
        std::fs::write(downloads.join(format!("{HELLO}-1.1.0.asp.part")), b"half").unwrap();
        g.host.clear_downloads();
        assert!(!downloads.exists());
        g.host.clear_downloads();
        // Never under a get that runs.
        let started = Arc::new(AtomicBool::new(false));
        *g.shelf.during.lock() = Some(holding(HELLO, &started));
        std::thread::scope(|s| {
            let running = s.spawn(|| g.get(HELLO));
            wait_for(&started);
            std::fs::create_dir_all(&downloads).unwrap();
            std::fs::write(downloads.join(format!("{HELLO}-1.1.0.asp.part")), b"coming").unwrap();
            g.host.clear_downloads();
            assert!(downloads.join(format!("{HELLO}-1.1.0.asp.part")).is_file());
            g.host.cancel(&PluginId::parse(HELLO).unwrap());
            assert!(matches!(running.join().unwrap(), Err(PluginError::Cancelled)));
        });
        g.done();
    }

    #[test]
    fn a_package_comes_as_its_entry_pins_it_and_a_failure_names_the_plugin() {
        let tmp = scratch("model-download");
        let mut e: Entry = serde_json::from_value(entry(HELLO, "1.1.0")).unwrap();
        let p = pinned(&e);
        // Named by its hash: never another package's part, whichever index listed it.
        assert_eq!(p.file, format!("{}.asp", "b".repeat(64)));
        assert_eq!((p.urls, p.size, p.sha256), (e.package.urls.clone(), 4096, "b".repeat(64)));
        assert!(matches!(p.packed, Packed::File) && !p.exec);
        // An address that can't be read fails as a download of the plugin,
        // in its own words (never the AI model's): no request leaves.
        e.package.urls = vec!["https://exa mple.com/hello.asp".into()];
        let failed = ModelDownload.get(&e, &tmp, &CancelToken::new(), &mut |_, _| {}).unwrap_err();
        assert!(matches!(&failed, PluginError::Download { what, .. } if what == "Hello Export"), "{failed:?}");
        assert_eq!(
            failed.to_string(),
            "AutoScene couldn't get Hello Export. Check the internet connection, then try again."
        );
        let cancel = CancelToken::new();
        cancel.cancel();
        assert!(matches!(ModelDownload.get(&e, &tmp, &cancel, &mut |_, _| {}), Err(PluginError::Cancelled)));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn progress_is_told_at_most_every_100_ms_and_what_is_left_at_the_end() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let mut telling = Telling::default();
        assert!(telling.tell(at(0), 0, 100));
        assert!(!telling.tell(at(40), 10, 100));
        assert!(!telling.tell(at(99), 20, 100));
        assert!(telling.tell(at(100), 30, 100));
        assert_eq!(telling.untold, None);
        assert!(!telling.tell(at(150), 60, 100));
        // A download that fails here tells what came last at its end.
        assert_eq!(telling.untold, Some((60, 100)));
        // All of it is told at once, before the package is checked.
        assert!(telling.tell(at(160), 100, 100));
        assert_eq!(telling.untold, None);
    }

    /// A package that comes in three steps, then fails, as a connection
    /// that drops.
    struct Dropping;

    impl GetPackage for Dropping {
        fn get(
            &self,
            e: &Entry,
            _: &Path,
            _: &CancelToken,
            progress: &mut dyn FnMut(u64, u64),
        ) -> PluginResult<PathBuf> {
            for done in [0, 10, 20] {
                progress(done, 100);
            }
            Err(PluginError::Download { what: e.name.clone(), detail: "connection reset".into() })
        }
    }

    #[test]
    fn the_page_hears_how_far_a_package_came_all_of_it_at_once_and_the_last_on_a_failure() {
        let g = Getting::new("told");
        let v110 = packed(HELLO, "1.1.0", "");
        g.shelf.put(HELLO, "1.1.0", &v110.0);
        g.store.set_catalog(living(vec![entry_of(HELLO, "1.1.0", &v110)], vec![]));
        let (id, size) = (PluginId::parse(HELLO).unwrap(), v110.0.len() as u64);
        // Within 100 ms: its start, and all of it at once, while it waits for its check.
        let (mut heard, downloads) = (Vec::new(), g.paths.plugin_downloads());
        let waiting = || std::fs::read_dir(&downloads).map_or(0, Iterator::count);
        let download = g.host.begin(&g.store, &id).unwrap();
        let t = told(&g.host, &g.store, &download, |done, total| heard.push((done, total, waiting()))).unwrap();
        assert_eq!(heard, [(0, size, 0), (size, size, 2)]);
        g.store.discard(t);
        drop(download);
        // One that failed: what came last is told too.
        let dropping = Getting::host(&g.paths, &g.db, Arc::new(Dropping));
        let mut heard = Vec::new();
        let download = dropping.begin(&g.store, &id).unwrap();
        let failed = told(&dropping, &g.store, &download, |done, total| heard.push((done, total)));
        assert!(matches!(failed, Err(PluginError::Download { .. })), "{failed:?}");
        assert_eq!(heard, [(0, 100), (20, 100)]);
        drop(download);
        drop(dropping);
        g.done();
    }

    /// Serves `data` on 127.0.0.1 to each request, as a package's address
    /// does; its address.
    fn serve(data: Vec<u8>) -> String {
        use std::io::{BufRead, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/hello-export.asp", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { return };
                let mut reader = std::io::BufReader::new(conn.try_clone().unwrap());
                // The request, up to its blank line.
                let mut line = String::new();
                while reader.read_line(&mut line).unwrap_or(0) > 0 && !line.trim().is_empty() {
                    line.clear();
                }
                let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", data.len());
                let _ = conn.write_all(head.as_bytes()).and_then(|()| conn.write_all(&data));
            }
        });
        url
    }

    #[test]
    fn a_package_comes_from_its_address_as_a_model_does_and_a_cancel_ends_it_midway() {
        let g = Getting::new("from-its-address");
        // Big enough to come in many pieces: what zip can't shrink much.
        let noise: String = (0..6_000u32).map(|i| package::sha256_hex(&i.to_le_bytes())).collect();
        let v110 = packed(HELLO, "1.1.0", &format!("  # {noise}"));
        assert!(v110.0.len() > 2 << 16, "{} bytes come in one piece", v110.0.len());
        let mut e = entry_of(HELLO, "1.1.0", &v110);
        e["package"]["urls"] = json!([serve(v110.0.clone())]);
        g.store.set_catalog(living(vec![e], vec![]));
        let host = Getting::host(&g.paths, &g.db, Arc::new(ModelDownload));
        let (id, size) = (PluginId::parse(HELLO).unwrap(), v110.0.len() as u64);
        // It comes, is checked and waits for the review; nothing of the download stays.
        let mut heard = Vec::new();
        let download = host.begin(&g.store, &id).unwrap();
        let t = told(&host, &g.store, &download, |done, total| heard.push((done, total))).unwrap();
        assert!(!download.end());
        assert_eq!((heard.first(), heard.last()), (Some(&(0, size)), Some(&(size, size))), "{heard:?}");
        assert_eq!((t.manifest.version.as_str(), t.verdict.trust), ("1.1.0", as_plugin::Trust::Reviewed));
        assert_eq!(std::fs::read_dir(g.paths.plugin_downloads()).unwrap().count(), 0);
        g.store.discard(t);
        // Cancelled while it comes: it ends there, and nothing is staged.
        let mut came = Vec::new();
        let download = host.begin(&g.store, &id).unwrap();
        let cancelled = host.get(&g.store, &download, |done, total| {
            came.push(done);
            if done > 0 && done < total {
                host.cancel(&id);
            }
        });
        assert!(matches!(cancelled, Err(PluginError::Cancelled)), "{came:?}: {:?}", cancelled.map(|_| ()));
        assert!(came.last().is_some_and(|&done| done > 0 && done < size), "{came:?} of {size}");
        drop(download);
        // Only what a later get resumes (or the next start removes) is left.
        let left = g.left();
        assert!(left.iter().all(|n| n.ends_with(".part") || n.ends_with(".lock")), "{left:?}");
        drop(host);
        g.done();
    }

    #[test]
    fn a_get_says_why_when_the_catalog_can_t_be_checked_again() {
        let g = Getting::new("not-checked");
        let id = PluginId::parse(HELLO).unwrap();
        let said = |host: &CatalogHost| {
            let download = host.begin(&g.store, &id).unwrap();
            host.get(&g.store, &download, |_, _| {}).unwrap_err().to_string()
        };
        // An index that ran out, or none: check it again.
        g.store.set_catalog(Arc::new(Catalog::none()));
        assert_eq!(said(&g.host), OUT_OF_DATE);
        // A build without a catalog: checking again changes nothing.
        let fetch = Arc::new(Arc::new(Served::default()));
        let none = CatalogHost::new(&g.paths, g.db.clone(), CatalogKeys::new(&[]).unwrap(), Vec::new(), fetch, shelf());
        assert_eq!(said(&none), NO_CATALOG);
        // The last check found a catalog only a newer AutoScene reads.
        let state = CatalogState { last_error: Some(NEEDS_NEWER_APP.into()), ..CatalogState::default() };
        g.store.set_catalog(Arc::new(Catalog::new(None, state)));
        assert_eq!(said(&g.host), NEEDS_NEWER_APP);
        // A newer AutoScene checked it.
        g.store.set_catalog(Arc::new(Catalog::none()));
        g.db.set_setting(ROW, &json!({ "v": STATE_VERSION + 1 }).to_string()).unwrap();
        let newer = Getting::host(&g.paths, &g.db, shelf());
        assert!(newer.newer_row());
        assert_eq!(said(&newer), NEEDS_NEWER_APP);
        assert!(g.shelf.asked.lock().is_empty());
        drop((none, newer));
        g.done();
    }
}

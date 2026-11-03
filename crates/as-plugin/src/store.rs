//! The installed plugins: their files under `plugins/<id>/`, and a row for
//! each in `library.db`'s settings, `plugin.<id>`, which holds its
//! [`Record`]: the version, where its files are, its trust and pinned author
//! key, and whether it is on. The values the person changed in its settings
//! sit beside it in `plugin_settings.<id>`, and what else the person agreed
//! to in `plugin_extra.<id>` ([`Extra`]): the runtime it was installed with,
//! and which of its secret settings have a value in the keychain
//! ([`crate::secrets`]; the values never come into a row). The keychain
//! keeps them under the library's own id (`plugins_keychain`, made with its
//! first secret), so another library on the computer never touches them.
//! Rows, not a table: a new library schema would make every older
//! AutoScene refuse the library, also for people who never install a
//! plugin. An AutoScene of version 1.0 knows only the first two: it writes
//! a record back without the fields it doesn't know, so nothing new goes
//! into the record, and an extra row it leaves behind when it uninstalls
//! the plugin, or installs it again, counts as none from the next open, and
//! goes after it with its secrets.
//!
//! A package installs in two steps. [`Store::stage`] unpacks it into
//! `plugins/.staging/<uuid>` and checks it there, which gives a [`Ticket`]:
//! what the review dialog shows. [`Store::commit`] checks the files and the
//! signature again, renames the folder to `plugins/<id>/<version>+<8 hex>`
//! and writes the row; the version before goes only once the row names the
//! new one. What is thrown away goes through `plugins/.trash`, so it is gone
//! from its place at once. If AutoScene stops at any point, the plugin is as
//! it was or as it is now, and what lies about is moved out of the way at the
//! next open, and removed after it ([`Store::remove_leftovers`]).
//! A power cut can also undo the last rows written (library.db syncs
//! lazily): the plugin is then listed as missing its files. A developer's
//! folder is loaded where it is ([`Store::stage_folder`]) and is never
//! removed.
//!
//! An update never takes away the trust its plugin had: a plugin AutoScene
//! reviewed takes only reviewed versions, and one pinned to its author's key
//! only versions that key signed or AutoScene reviewed, which keep the pin
//! ([`trust::judge`]). An unreviewed one takes any, and one its author signed
//! pins the key. None goes back to an older version or changes its files
//! under the same one. A row this
//! AutoScene can't read (a later AutoScene's) is listed as such and never
//! written over, and so is a plugin that doesn't work with this AutoScene
//! (one put back to an older version).

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use as_core::AppPaths;
use as_db::LibraryDb;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use parking_lot::Mutex;
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::calls::HostCalls;
use crate::error::{PluginError, PluginResult};
use crate::manifest::{
    self, Manifest, Parsed, Permission, PluginId, RuntimeKind, SettingKind, SettingSpec, SettingValue,
    current_platform, invalid, shown,
};
use crate::package::{self, FilesLock, LOCK, LOCK_VERSION, LockedFile, MANIFEST, MAX_LOCK_BYTES, Unpacked, file_in};
use crate::protocol::{ActionContext, AppContext, PluginContext, RunMessage, ServeMessage, ToolPaths};
use crate::runtime::RunSpec;
use crate::secrets::{self, SecretStore};
use crate::service::{READY_WAIT, ServiceSpec};
use crate::trust::{self, AuthorKey, Keys, Trust, Verdict};

/// The [`Record`] version this AutoScene reads and writes. A row of another
/// is a later AutoScene's: listed as needing it, and never written over.
pub const RECORD_VERSION: u32 = 1;
/// The [`Extra`] version this AutoScene reads and writes; as for a record.
pub const EXTRA_VERSION: u32 = 1;

const ROW: &str = "plugin.";
const SETTINGS_ROW: &str = "plugin_settings.";
/// It sorts after `plugin/`, so the registry's rows (`plugin.` up to
/// `plugin/`) never take it for a plugin's, and outside `plugin_settings.`.
pub const EXTRA_ROW: &str = "plugin_extra.";
/// The id the keychain keeps this library's secrets under. Outside each
/// plugin's rows, and never removed: an entry under it may outlive its
/// plugin, when the keychain refused to remove it.
const KEYCHAIN_ROW: &str = "plugins_keychain";
const STAGING: &str = ".staging";
const TRASH: &str = ".trash";
/// Why a plugin whose folder is gone can't run.
pub const MISSING: &str = "Its files are missing. Install it again.";
const CHANGED: &str = "Its files were changed. Install it again.";
const NEWER: &str = "Needs a newer AutoScene.";
const RUNTIME_CHANGED: &str = "Its runtime changed. Load it again to agree to that.";
/// How long a rename waits for Windows: a virus scanner may still hold a
/// file it just unpacked.
const RENAME_WAIT: Duration = Duration::from_secs(1);

/// What the registry holds of an installed plugin (`plugin.<id>`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    /// [`RECORD_VERSION`].
    pub v: u32,
    pub id: PluginId,
    pub version: String,
    pub source: Source,
    pub trust: Trust,
    /// The author key its updates must be signed with (see [`trust::judge`]).
    pub author_key: Option<AuthorKey>,
    /// What it said it does when the person installed it.
    pub permissions: Vec<Permission>,
    pub network_hosts: Vec<String>,
    pub enabled: bool,
    /// Its export actions that run after every export.
    pub after_export: Vec<String>,
    /// The SHA-256 of its `files.lock`; none for a loaded folder.
    pub lock_sha256: Option<String>,
    /// When this version was installed (Unix ms).
    pub installed_at: i64,
}

/// What the person agreed to beside the [`Record`] (`plugin_extra.<id>`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Extra {
    /// [`EXTRA_VERSION`].
    pub v: u32,
    /// The [`Record::installed_at`] of the install it was written with. A
    /// row of another is left from before AutoScene 1.0 installed the plugin
    /// again, which writes only the record: it counts as none.
    #[serde(default)]
    pub installed_at: i64,
    /// The runtime it was installed with: a loaded folder that changes it
    /// waits to be loaded again. None for a plugin installed without the row
    /// (by AutoScene 1.0), which runs as a process.
    #[serde(default)]
    pub runtime: Option<RuntimeKind>,
    /// The keys of its secret settings that have a value in the keychain;
    /// never the values.
    #[serde(default)]
    pub secrets: Vec<String>,
}

impl Default for Extra {
    fn default() -> Self {
        Self { v: EXTRA_VERSION, installed_at: 0, runtime: None, secrets: Vec::new() }
    }
}

impl Extra {
    /// Of a plugin installed to run as `runtime`, with no secret set yet.
    pub fn agreed(runtime: RuntimeKind) -> Self {
        Self { runtime: Some(runtime), ..Self::default() }
    }

    /// The runtime the person agreed to.
    pub fn agreed_runtime(&self) -> RuntimeKind {
        self.runtime.unwrap_or(RuntimeKind::Process)
    }
}

/// Where a plugin's files are.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    /// Installed from a package: the folder `<version>+<8 hex>` under
    /// `plugins/<id>/`.
    Package { folder: String },
    /// Loaded from a developer's folder (an absolute path), used where it is.
    Folder { path: String },
}

/// An installed plugin, as its row and its folder say.
#[derive(Clone, Debug)]
pub struct Installed {
    pub record: Record,
    /// Its manifest, unless it is broken.
    pub manifest: Option<Manifest>,
    pub warnings: Vec<String>,
    /// Its folder: the package's under `plugins/<id>/`, or the loaded one.
    pub dir: PathBuf,
    /// Why it can't run, as a sentence: its files are missing or were
    /// changed, a later AutoScene wrote its rows, it doesn't work with this
    /// AutoScene, or a loaded folder's runtime is not the one agreed to.
    pub broken: Option<String>,
    /// What the person agreed to beside its record.
    pub extra: Extra,
}

impl Installed {
    /// The keys of its secret settings that have a value in the keychain:
    /// those its row lists that its manifest still has.
    pub fn secrets_stored(&self) -> Vec<String> {
        let Some(m) = &self.manifest else { return Vec::new() };
        m.secret_keys().filter(|k| self.extra.secrets.iter().any(|s| s == k)).map(str::to_owned).collect()
    }
}

/// A package (or a folder) checked and waiting for the person's yes: what
/// the review dialog shows, and what [`Store::commit`] installs.
#[derive(Debug)]
pub struct Ticket {
    pub id: Uuid,
    pub manifest: Manifest,
    pub warnings: Vec<String>,
    /// The trust it gets, and the author key it is pinned to after.
    pub verdict: Verdict,
    /// Its lock and the lock's exact bytes; none for a folder.
    pub lock: Option<FilesLock>,
    pub lock_bytes: Vec<u8>,
    /// Its `files.lock.minisig` and `author.pub`, for the check at commit.
    pub signature: Option<String>,
    pub author_pub: Option<String>,
    pub source: Source,
    /// The folder it was unpacked into, under `plugins/.staging`.
    pub staged: Option<PathBuf>,
    /// The record of the version it replaces.
    pub previous: Option<Record>,
    pub created: Instant,
}

/// The plugins installed in one app data folder.
pub struct Store {
    root: PathBuf,
    data: PathBuf,
    logs: PathBuf,
    db: Arc<LibraryDb>,
    keys: Keys,
    /// The version of this AutoScene, which each plugin must work with.
    app: Version,
    /// Where the values of secret settings are kept.
    secrets: Arc<dyn SecretStore>,
    /// The id they are kept under there ([`KEYCHAIN_ROW`]), once read.
    library: OnceLock<String>,
    /// Held while a plugin's rows change, so each change starts from the
    /// rows the one before wrote.
    changing: Mutex<()>,
    /// What the open moved into `.trash`, for [`Store::remove_leftovers`].
    leftovers: Mutex<Vec<PathBuf>>,
    /// The plugins whose extra row the open found left from before
    /// ([`orphaned`]), for [`Store::remove_leftovers`].
    orphans: Mutex<Vec<PluginId>>,
}

impl Store {
    /// Opens the plugins of `paths` (registered in `db`, judged by `keys`,
    /// with their secrets in `secrets`) for the AutoScene of version `app`,
    /// and moves into `.trash` what an install or uninstall that never
    /// finished left: staged folders, versions a row no longer names, and
    /// folders of plugins without a row. A plugin whose row a later
    /// AutoScene wrote keeps its folder as it is. Moving is quick; removing
    /// them, and what `.trash` held, is [`Store::remove_leftovers`]'s, which
    /// a package's 4096 files can keep busy for a second. So is an extra row
    /// left from before (AutoScene 1.0 uninstalled its plugin, or installed
    /// it again), after its secrets, as the keychain may ask the person or
    /// take its time; it counts as none meanwhile.
    pub fn open(
        paths: &AppPaths,
        db: Arc<LibraryDb>,
        keys: Keys,
        app: Version,
        secrets: Arc<dyn SecretStore>,
    ) -> PluginResult<Self> {
        let store = Self {
            root: paths.plugins(),
            data: paths.plugin_data(),
            logs: paths.plugin_logs(),
            db,
            keys,
            app,
            secrets,
            library: OnceLock::new(),
            changing: Mutex::default(),
            leftovers: Mutex::default(),
            orphans: Mutex::default(),
        };
        *store.orphans.lock() = store.orphaned_extras()?;
        let (staging, trash) = (store.root.join(STAGING), store.root.join(TRASH));
        for dir in [&staging, &trash] {
            // The store's own folders: a file in their place goes.
            if fs::symlink_metadata(dir).is_ok_and(|f| !f.is_dir()) {
                remove_all(dir)?;
            }
            fs::create_dir_all(dir)?;
        }
        let mut leftovers = fs::read_dir(&trash)?.map(|entry| Ok(entry?.path())).collect::<io::Result<Vec<_>>>()?;
        for entry in fs::read_dir(&staging)? {
            leftovers.extend(store.move_away(&entry?.path()));
        }
        leftovers.extend(store.sweep()?);
        *store.leftovers.lock() = leftovers;
        Ok(store)
    }

    /// Removes what [`Store::open`] moved out of the way. Each lies in
    /// `.trash` under a name of its own, so what is staged, installed or
    /// thrown away meanwhile is never touched: this may run beside them.
    /// Then the extra rows it found left from before, after their secrets.
    pub fn remove_leftovers(&self) {
        let leftovers = std::mem::take(&mut *self.leftovers.lock());
        for path in leftovers {
            if let Err(e) = remove_all(&path) {
                tracing::warn!(path = %as_core::paths::name(&path), error = %e, "a plugin's old files stay");
            }
        }
        let orphans = std::mem::take(&mut *self.orphans.lock());
        for id in orphans {
            if let Err(e) = self.forget_orphan(&id) {
                tracing::warn!(plugin = %id, error = %e.detail(), "a plugin's old extra row stays");
            }
        }
    }

    /// Moves into `.trash` each folder under `plugins/<id>/` its row doesn't
    /// name, and `plugins/<id>/` of a plugin without a row (or loaded from a
    /// folder), and gives where they went. Names that are no plugin id are
    /// left as they are.
    fn sweep(&self) -> PluginResult<Vec<PathBuf>> {
        let mut moved = Vec::new();
        let rows: BTreeMap<PluginId, String> = self.rows()?.into_iter().collect();
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let Some(id) = entry.file_name().to_str().and_then(|name| PluginId::parse(name).ok()) else { continue };
            let keep = match rows.get(&id).map(|value| read_row(&id, value)) {
                Some(Some(Record { source: Source::Package { folder }, .. })) => folder,
                // A later AutoScene's row: so may its files be.
                Some(None) => continue,
                Some(Some(_)) | None => {
                    moved.extend(self.move_away(&entry.path()));
                    continue;
                }
            };
            // A file or a link in its folder's place goes.
            if !entry.file_type().is_ok_and(|f| f.is_dir()) {
                moved.extend(self.move_away(&entry.path()));
                continue;
            }
            // A folder that can't be read now may hold the version in use.
            let Ok(versions) = fs::read_dir(entry.path()) else { continue };
            for old in versions.flatten() {
                if old.file_name() != keep.as_str() {
                    moved.extend(self.move_away(&old.path()));
                }
            }
        }
        Ok(moved)
    }

    /// The plugins whose extra row is left from before ([`orphaned`]).
    fn orphaned_extras(&self) -> PluginResult<Vec<PluginId>> {
        let rows: BTreeMap<PluginId, String> = self.rows()?.into_iter().collect();
        let mut orphans = Vec::new();
        for (key, value) in self.db.settings_in(EXTRA_ROW)? {
            let Ok(id) = PluginId::parse(&key[EXTRA_ROW.len()..]) else { continue };
            if orphaned(&id, rows.get(&id).map(String::as_str), &value) {
                orphans.push(id);
            }
        }
        Ok(orphans)
    }

    /// Removes the extra row of `id` while it is left from before
    /// ([`orphaned`]), after the secrets it lists. Those go best-effort:
    /// what the keychain refuses stays there, unlisted, so no plugin reads
    /// it.
    fn forget_orphan(&self, id: &PluginId) -> PluginResult<()> {
        let _changing = self.changing.lock();
        let Some(value) = self.db.setting(&extra_key(id))? else { return Ok(()) };
        if orphaned(id, self.db.setting(&row_key(id))?.as_deref(), &value) {
            self.forget_secrets(id, &secret_names(&value));
            self.db.write_settings(&[(&extra_key(id), None)])?;
        }
        Ok(())
    }

    /// Every installed plugin, by id.
    pub fn list(&self) -> PluginResult<Vec<Installed>> {
        Ok(self.rows()?.into_iter().map(|(id, value)| self.installed(id, &value)).collect())
    }

    /// The installed plugin `id`, also when it is broken.
    pub fn get(&self, id: &PluginId) -> PluginResult<Installed> {
        let value = self.db.setting(&row_key(id))?.ok_or_else(|| not_found(id))?;
        Ok(self.installed(id.clone(), &value))
    }

    /// Unpacks the package `archive` and checks it for this AutoScene: its
    /// manifest, the files its command starts on this computer, its pages,
    /// its icon and its signature, and that it may replace the installed
    /// version. A package that fails leaves no folder.
    pub fn stage(&self, archive: &Path) -> PluginResult<Ticket> {
        let id = Uuid::now_v7();
        let staged = self.root.join(STAGING).join(id.simple().to_string());
        let unpacked = package::unpack(archive, &staged)?;
        let ticket = self.check_package(id, &staged, unpacked);
        if ticket.is_err() {
            self.throw_away(&staged);
        }
        ticket
    }

    fn check_package(&self, id: Uuid, staged: &Path, unpacked: Unpacked) -> PluginResult<Ticket> {
        let bytes = manifest::read_file(&staged.join(MANIFEST))?;
        let Parsed { manifest: m, warnings } = manifest::parse(&bytes, &self.app)?;
        if m.runs_program() {
            package::check_launch(&unpacked.lock, &m.launch()?, current_platform())?;
        }
        package::check_pages(staged, Some(&unpacked.lock), &m.pages)?;
        if let Some(icon) = &m.icon {
            package::check_icon(staged, &unpacked.lock, icon)?;
        }
        let previous = self.record(&m.id)?;
        let Unpacked { lock, lock_bytes, signature, author_key } = unpacked;
        let pin = previous.as_ref().and_then(|p| p.author_key.as_ref());
        let verdict = trust::judge(&self.keys, &lock_bytes, signature.as_deref(), author_key.as_deref(), &m, pin)?;
        if let Some(p) = &previous {
            check_update(p, &m, verdict.trust, Some(&package::sha256_hex(&lock_bytes)))?;
        }
        // The id's random end: two installs of one version get two folders.
        let folder = format!("{}+{}", m.version, &id.simple().to_string()[24..]);
        Ok(Ticket {
            id,
            manifest: m,
            warnings,
            verdict,
            lock: Some(lock),
            lock_bytes,
            signature,
            author_pub: author_key,
            source: Source::Package { folder },
            staged: Some(staged.to_path_buf()),
            previous,
            created: Instant::now(),
        })
    }

    /// Checks the plugin in the developer's folder `dir` for this AutoScene,
    /// to load it where it is: it has no lock, so it is Unreviewed, and its
    /// files are read as they are each time.
    pub fn stage_folder(&self, dir: &Path) -> PluginResult<Ticket> {
        let dir = std::path::absolute(dir)?;
        if !dir.is_dir() {
            return Err(invalid("That isn't a folder."));
        }
        // An installed version's folder would be thrown away by an update.
        if fs::canonicalize(&dir)?.starts_with(fs::canonicalize(&self.root)?) {
            return Err(invalid("That folder belongs to an installed plugin. Load the folder you work in."));
        }
        let path = dir.to_str().ok_or_else(|| invalid("AutoScene can't load a folder whose path isn't text."))?;
        let bytes = manifest::read_file(&dir.join(MANIFEST)).map_err(|e| match e.kind() {
            ErrorKind::NotFound => invalid("That folder has no plugin.json."),
            _ => e.into(),
        })?;
        let Parsed { manifest: m, warnings } = manifest::parse(&bytes, &self.app)?;
        let starts = if m.runs_program() { m.launch()?.files() } else { Vec::new() };
        for file in starts {
            if !fs::metadata(file_in(&dir, &file)).is_ok_and(|f| f.is_file()) {
                return Err(invalid(format!(
                    "The plugin folder is missing {}, which its command needs.",
                    shown(&file)
                )));
            }
        }
        package::check_pages(&dir, None, &m.pages)?;
        let previous = self.record(&m.id)?;
        let pin = previous.as_ref().and_then(|p| p.author_key.as_ref());
        let verdict = trust::judge(&self.keys, &[], None, None, &m, pin).map_err(|e| match e {
            // What judge says of an unsigned update: the folder isn't signed at all.
            PluginError::KeyChanged { .. } => {
                invalid("The installed version is signed by its author. Uninstall it to load a folder.")
            }
            e => e,
        })?;
        if let Some(p) = &previous {
            check_update(p, &m, verdict.trust, None)?;
        }
        Ok(Ticket {
            id: Uuid::now_v7(),
            manifest: m,
            warnings,
            verdict,
            lock: None,
            lock_bytes: Vec::new(),
            signature: None,
            author_pub: None,
            source: Source::Folder { path: path.to_owned() },
            staged: None,
            previous,
            created: Instant::now(),
        })
    }

    /// Installs the plugin of `t`. One AutoScene hasn't reviewed needs
    /// `allow_unreviewed` (the setting) and `accept_unreviewed` (the
    /// person's typed yes). Its files and signature are checked again, and
    /// what it replaces is the installed version now, which may have
    /// changed since it was staged. Whatever fails, the plugin stays as it
    /// was, and the staged folder is removed.
    pub fn commit(&self, t: Ticket, allow_unreviewed: bool, accept_unreviewed: bool) -> PluginResult<Installed> {
        let installed = self.install(&t, allow_unreviewed, accept_unreviewed);
        if installed.is_err() {
            self.discard(t);
        }
        installed
    }

    fn install(&self, t: &Ticket, allow_unreviewed: bool, accept_unreviewed: bool) -> PluginResult<Installed> {
        // Before the files are hashed again, when the answer is no already.
        allowed(t.verdict.trust, allow_unreviewed, accept_unreviewed)?;
        let m = &t.manifest;
        let package = match (&t.source, self.staged(t)) {
            (Source::Package { folder }, Some(staged)) => {
                self.check_staged(staged, &t.lock_bytes)?;
                Some((folder, staged))
            }
            (Source::Folder { .. }, None) => None,
            _ => return Err(changed_while_waiting()),
        };
        let _changing = self.changing.lock();
        let previous = self.record(&m.id)?;
        let pin = previous.as_ref().and_then(|p| p.author_key.as_ref());
        let verdict = trust::judge(&self.keys, &t.lock_bytes, t.signature.as_deref(), t.author_pub.as_deref(), m, pin)?;
        allowed(verdict.trust, allow_unreviewed, accept_unreviewed)?;
        let lock_sha256 = package.is_some().then(|| package::sha256_hex(&t.lock_bytes));
        if let Some(p) = &previous {
            check_update(p, m, verdict.trust, lock_sha256.as_deref())?;
        }
        // The secrets it has keep their values; it runs as its manifest says
        // from now, which the person agreed to. A value kept under the key of
        // a secret setting, from before it was a secret, goes.
        let kept = match &previous {
            Some(p) => self.extra(p)?.secrets,
            None => Vec::new(),
        };
        let installed_at = as_api::now_ms();
        let extra = Extra { installed_at, secrets: kept, ..Extra::agreed(m.runtime.kind) };
        let settings = self.settings_without_secrets(&m.id, m)?;

        let plugin_dir = self.root.join(m.id.as_str());
        let dir = match package {
            Some((folder, staged)) => {
                fs::create_dir_all(&plugin_dir)?;
                let dir = plugin_dir.join(folder);
                if let Err(e) = rename(staged, &dir) {
                    // Empty unless the version before lies in it.
                    let _ = fs::remove_dir(&plugin_dir);
                    return Err(e.into());
                }
                Some(dir)
            }
            None => None,
        };
        let actions: Vec<&str> = m.export_actions.iter().map(|a| a.id.as_str()).collect();
        let record = Record {
            v: RECORD_VERSION,
            id: m.id.clone(),
            version: m.version.clone(),
            source: t.source.clone(),
            trust: verdict.trust,
            author_key: verdict.author_key,
            permissions: m.permissions.clone(),
            network_hosts: m.network_hosts.clone(),
            enabled: previous.as_ref().is_none_or(|p| p.enabled),
            after_export: previous
                .as_ref()
                .map(|p| p.after_export.iter().filter(|a| actions.contains(&a.as_str())).cloned().collect())
                .unwrap_or_default(),
            lock_sha256,
            installed_at,
        };
        if let Err(e) = self.write_installed(&record, &extra, settings.as_ref().map(Option::as_deref)) {
            if let Some(dir) = &dir {
                self.throw_away(dir);
                // Empty unless the version before lies in it.
                let _ = fs::remove_dir(&plugin_dir);
            }
            return Err(e);
        }
        // The row names the new version: the one before can go.
        match (previous.map(|p| p.source), &dir) {
            (Some(Source::Package { .. }), None) => self.throw_away(&plugin_dir),
            (Some(Source::Package { folder }), Some(dir)) if plugin_dir.join(&folder) != *dir => {
                self.throw_away(&plugin_dir.join(folder));
            }
            _ => {}
        }
        Ok(self.load(record))
    }

    /// Drops `t`, and the folder it was staged in.
    pub fn discard(&self, t: Ticket) {
        if let Some(staged) = self.staged(&t) {
            self.throw_away(staged);
        }
    }

    /// The folder `t` was staged in, when it lies in `.staging`.
    fn staged<'a>(&self, t: &'a Ticket) -> Option<&'a Path> {
        t.staged.as_deref().filter(|staged| staged.parent() == Some(self.root.join(STAGING).as_path()))
    }

    /// Checks the staged folder `staged` before it is installed: it holds
    /// the lock `lock_bytes` it was staged with, and each file that lock
    /// lists as it was unpacked.
    fn check_staged(&self, staged: &Path, lock_bytes: &[u8]) -> PluginResult<()> {
        let mut on_disk = Vec::new();
        match File::open(staged.join(LOCK)) {
            Ok(file) => file.take(MAX_LOCK_BYTES + 1).read_to_end(&mut on_disk)?,
            Err(e) if e.kind() == ErrorKind::NotFound => return Err(changed_while_waiting()),
            Err(e) => return Err(e.into()),
        };
        if on_disk != lock_bytes {
            return Err(changed_while_waiting());
        }
        package::verify_dir(staged, &package::read_lock(&on_disk)?)
    }

    /// Uninstalls `id` (also one whose row a later AutoScene wrote): its
    /// rows go in one transaction (its pinned author key with them, so
    /// installing it again is a first install), then its secrets,
    /// best-effort, and its installed files. A loaded folder is the
    /// developer's and stays where it is. `delete_data` also removes what it
    /// kept ([`Store::data_dir`]).
    pub fn uninstall(&self, id: &PluginId, delete_data: bool) -> PluginResult<()> {
        let _changing = self.changing.lock();
        let Some(row) = self.db.setting(&row_key(id))? else { return Err(not_found(id)) };
        // Those its row lists, and each of its secret settings, listed or
        // not: AutoScene may have stopped before the row named one, or a
        // keychain refused to remove one at an uninstall before.
        let mut secrets = self.db.setting(&extra_key(id))?.map(|value| secret_names(&value)).unwrap_or_default();
        if let Some(m) = self.installed(id.clone(), &row).manifest {
            let unlisted: Vec<String> =
                m.secret_keys().filter(|k| !secrets.iter().any(|s| s == k)).map(str::to_owned).collect();
            secrets.extend(unlisted);
        }
        self.db.write_settings(&[(&row_key(id), None), (&settings_key(id), None), (&extra_key(id), None)])?;
        self.forget_secrets(id, &secrets);
        self.throw_away(&self.root.join(id.as_str()));
        if delete_data {
            self.throw_away(&self.data_dir(id));
        }
        Ok(())
    }

    /// Turns `id` on or off.
    pub fn set_enabled(&self, id: &PluginId, on: bool) -> PluginResult<Installed> {
        let _changing = self.changing.lock();
        let mut record = self.record(id)?.ok_or_else(|| not_found(id))?;
        record.enabled = on;
        self.write_record(&record)?;
        Ok(self.load(record))
    }

    /// Turns the export action `action` of `id` on or off for after every
    /// export.
    pub fn set_after_export(&self, id: &PluginId, action: &str, on: bool) -> PluginResult<Installed> {
        let _changing = self.changing.lock();
        let record = self.record(id)?.ok_or_else(|| not_found(id))?;
        let mut p = self.load(record);
        let m = manifest_of(&p)?;
        if !m.export_actions.iter().any(|a| a.id == action) {
            return Err(invalid(format!("The plugin has no export action {}.", shown(action))));
        }
        let after = &p.record.after_export;
        p.record.after_export = m
            .export_actions
            .iter()
            .filter(|a| if a.id == action { on } else { after.contains(&a.id) })
            .map(|a| a.id.clone())
            .collect();
        self.write_record(&p.record)?;
        Ok(p)
    }

    /// The value of each of the settings of `id`: the person's while it
    /// fits the setting, else its default (a setting without either has
    /// none).
    pub fn values(&self, id: &PluginId) -> PluginResult<BTreeMap<String, SettingValue>> {
        let p = self.get(id)?;
        Ok(merged(&manifest_of(&p)?.settings, &self.stored_values(id)?.unwrap_or_default()))
    }

    /// Sets the settings of `id` named in `values`, each checked against
    /// its setting; one that doesn't fit refuses them all, and a key that
    /// names no setting is dropped. Only values that differ from their
    /// default are kept, so a setting left alone follows the plugin's
    /// default. Gives every value, as [`Store::values`] does. Values a later
    /// AutoScene saved are never written over: a row this one can't read
    /// refuses the change, and the value of a setting it skipped (a type it
    /// doesn't know) stays as it is. A secret setting is refused: its value
    /// goes to the keychain ([`Store::secret_set`]), never into a row.
    pub fn set_values(
        &self,
        id: &PluginId,
        values: BTreeMap<String, SettingValue>,
    ) -> PluginResult<BTreeMap<String, SettingValue>> {
        let _changing = self.changing.lock();
        let p = self.get(id)?;
        let specs = &manifest_of(&p)?.settings;
        let spec = |key: &str| specs.iter().find(|s| s.key == key);
        let mut stored = self.stored_row(id)?.ok_or_else(|| {
            invalid("A newer AutoScene saved this plugin's settings, so this one can't change them. Update AutoScene, or uninstall the plugin.")
        })?;
        for (key, value) in values {
            if let Some(spec) = spec(&key) {
                if matches!(spec.kind, SettingKind::Secret { .. }) {
                    return Err(invalid("A secret is set with its own button, never as a value."));
                }
                check_value(spec, &value)?;
                stored.insert(key, serde_json::to_value(value).expect("a setting value serializes"));
            }
        }
        stored.retain(|key, value| match spec(key) {
            Some(s) => serde_json::from_value::<SettingValue>(value.clone())
                .is_ok_and(|v| check_value(s, &v).is_ok() && default_of(&s.kind) != Some(v)),
            None => true,
        });
        let json = serde_json::to_string(&stored).expect("setting values serialize");
        self.db.write_settings(&[(&settings_key(id), (!stored.is_empty()).then_some(json.as_str()))])?;
        Ok(merged(specs, &typed(stored)))
    }

    /// Keeps `value` in the keychain as the secret setting `key` of `id`,
    /// in place of the one before; gives the plugin as it is now. Its row
    /// lists the key once the keychain holds it.
    pub fn secret_set(&self, id: &PluginId, key: &str, value: &str) -> PluginResult<Installed> {
        secrets::check(value)?;
        let _changing = self.changing.lock();
        let record = self.record(id)?.ok_or_else(|| not_found(id))?;
        let p = self.load(record);
        let m = manifest_of(&p)?;
        if !m.secret_keys().any(|k| k == key) {
            return Err(invalid(format!("The plugin has no secret setting {}.", shown(key))));
        }
        // A plain value kept under its key goes too: a loaded folder's
        // plugin.json may have made it a secret since.
        let settings = self.settings_without_secrets(id, m)?;
        let mut extra = self.extra(&p.record)?;
        self.secrets.set(self.library_or_new()?, id, key, value)?;
        let listed = extra.secrets.iter().any(|k| k == key);
        if !listed {
            extra.secrets.push(key.to_owned());
        }
        if !listed || settings.is_some() {
            self.write_extra(id, &extra, settings.as_ref().map(Option::as_deref))?;
        }
        Ok(self.load(p.record))
    }

    /// Removes the secret setting `key` of `id` from the keychain; gives the
    /// plugin as it is now. Also one its row lists that the plugin no longer
    /// has, or a broken plugin's, though the Settings page offers only its
    /// secret settings now: such a secret goes with its uninstall.
    pub fn secret_clear(&self, id: &PluginId, key: &str) -> PluginResult<Installed> {
        let _changing = self.changing.lock();
        let record = self.record(id)?.ok_or_else(|| not_found(id))?;
        let p = self.load(record);
        let mut extra = self.extra(&p.record)?;
        let listed = extra.secrets.iter().any(|k| k == key);
        if !listed && !p.manifest.as_ref().is_some_and(|m| m.secret_keys().any(|k| k == key)) {
            return Err(invalid(format!("The plugin has no secret setting {}.", shown(key))));
        }
        if let Some(library) = self.library()? {
            self.secrets.delete(library, id, key)?;
        }
        if listed {
            extra.secrets.retain(|k| k != key);
            self.write_extra(id, &extra, None)?;
        }
        Ok(self.load(p.record))
    }

    /// The value of the secret setting `key` of `id`, for the plugin alone:
    /// never for a row, a log, a run's environment or its run line. None
    /// while the person hasn't set it; the keychain is asked only once they
    /// have. A key that isn't one of its secret settings now is refused.
    pub fn secret(&self, id: &PluginId, key: &str) -> PluginResult<Option<String>> {
        let p = self.get(id)?;
        if !manifest_of(&p)?.secret_keys().any(|k| k == key) {
            return Err(PluginError::Denied(format!("{} isn't one of the plugin's secret settings.", shown(key))));
        }
        match (p.extra.secrets.iter().any(|k| k == key), self.library()?) {
            (true, Some(library)) => self.secrets.get(library, id, key),
            _ => Ok(None),
        }
    }

    /// The id the keychain keeps the secrets of this library under; none
    /// while it has never kept one.
    fn library(&self) -> PluginResult<Option<&str>> {
        if self.library.get().is_none()
            && let Some(library) = self.db.setting(KEYCHAIN_ROW)?
        {
            let _ = self.library.set(library);
        }
        Ok(self.library.get().map(String::as_str))
    }

    /// [`Store::library`], made and written for the first secret. Made
    /// under `changing`, so only once.
    fn library_or_new(&self) -> PluginResult<&str> {
        if let Some(library) = self.library()? {
            return Ok(library);
        }
        let library = Uuid::new_v4().simple().to_string();
        self.db.write_settings(&[(KEYCHAIN_ROW, Some(&library))])?;
        Ok(self.library.get_or_init(|| library))
    }

    /// The version of this AutoScene.
    pub fn app_version(&self) -> &Version {
        &self.app
    }

    /// Where `id` keeps what it needs between runs (its runs start there).
    pub fn data_dir(&self, id: &PluginId) -> PathBuf {
        self.data.join(id.as_str())
    }

    /// Where the runs of `id` write their log.
    pub fn log_path(&self, id: &PluginId) -> PathBuf {
        self.logs.join(format!("{id}.log"))
    }

    /// Where the service of `id` writes its log: beside its runs', since a
    /// run and the service may write at once.
    pub fn service_log_path(&self, id: &PluginId) -> PathBuf {
        self.logs.join(format!("{id}.service.log"))
    }

    /// A run of `action` of `p`, as [`crate::runtime::run`] takes it: its
    /// command from its folder, started in [`Store::data_dir`] and logged to
    /// [`Store::log_path`], and a run line with a new run id that names the
    /// plugin, this AutoScene, the app's FFmpeg (`ffmpeg` and `ffprobe`, of
    /// `ffmpeg_version`) and each of its settings, as [`Store::values`] gives
    /// them. The run speaks the protocol the plugin declared. The project and
    /// the export it runs on are the host's to add, and so are the answers to
    /// its calls. A broken plugin can't run.
    pub fn run_spec(
        &self,
        p: &Installed,
        action: ActionContext,
        ffmpeg: &Path,
        ffprobe: &Path,
        ffmpeg_version: &str,
    ) -> PluginResult<RunSpec> {
        let m = manifest_of(p)?;
        let id = &p.record.id;
        let data_dir = self.data_dir(id);
        let message = RunMessage {
            protocol: m.engines.protocol,
            run_id: Uuid::now_v7().to_string(),
            plugin: PluginContext {
                id: id.clone(),
                version: p.record.version.clone(),
                data_dir: data_dir.to_string_lossy().into_owned(),
            },
            app: AppContext { version: self.app.to_string(), platform: current_platform().into() },
            tools: ToolPaths {
                ffmpeg: ffmpeg.to_string_lossy().into_owned(),
                ffprobe: ffprobe.to_string_lossy().into_owned(),
                ffmpeg_version: ffmpeg_version.into(),
            },
            settings: merged(&m.settings, &self.stored_values(id)?.unwrap_or_default()),
            action,
            project: None,
            export: None,
            input: None,
        };
        Ok(RunSpec {
            name: m.name.clone(),
            dir: p.dir.clone(),
            launch: m.launch()?,
            data_dir,
            log_path: self.log_path(id),
            ffmpeg: ffmpeg.into(),
            ffprobe: ffprobe.into(),
            protocol: m.engines.protocol,
            calls: None,
            message,
        })
    }

    /// The service of `p`, as [`crate::service::ProcessService::start`]
    /// takes it: its command from its folder, started in
    /// [`Store::data_dir`] and logged to [`Store::service_log_path`], with
    /// `calls` answering its calls, and a serve line with a new id that
    /// names what a run line does but a task. Only a plugin whose runtime is
    /// a service has one, and it speaks protocol 2; a broken plugin has
    /// none.
    pub fn serve_spec(
        &self,
        p: &Installed,
        ffmpeg: &Path,
        ffprobe: &Path,
        ffmpeg_version: &str,
        calls: Arc<dyn HostCalls>,
    ) -> PluginResult<ServiceSpec> {
        let m = manifest_of(p)?;
        if m.runtime.kind != RuntimeKind::Service {
            return Err(invalid(format!("{} runs no service.", m.name)));
        }
        let id = &p.record.id;
        let data_dir = self.data_dir(id);
        let message = ServeMessage {
            protocol: m.engines.protocol,
            service_id: Uuid::now_v7().to_string(),
            plugin: PluginContext {
                id: id.clone(),
                version: p.record.version.clone(),
                data_dir: data_dir.to_string_lossy().into_owned(),
            },
            app: AppContext { version: self.app.to_string(), platform: current_platform().into() },
            tools: ToolPaths {
                ffmpeg: ffmpeg.to_string_lossy().into_owned(),
                ffprobe: ffprobe.to_string_lossy().into_owned(),
                ffmpeg_version: ffmpeg_version.into(),
            },
            settings: merged(&m.settings, &self.stored_values(id)?.unwrap_or_default()),
        };
        Ok(ServiceSpec {
            name: m.name.clone(),
            dir: p.dir.clone(),
            launch: m.launch()?,
            data_dir,
            log_path: self.service_log_path(id),
            ffmpeg: ffmpeg.into(),
            ffprobe: ffprobe.into(),
            ready_wait: READY_WAIT,
            message,
            calls,
        })
    }

    /// The icon of `p` as a `data:` URL, while it is the picture its
    /// package listed (a loaded folder's: a picture of an icon's size).
    pub fn icon_data_url(&self, p: &Installed) -> Option<String> {
        let icon = p.manifest.as_ref()?.icon.as_deref()?;
        let path = file_in(&p.dir, icon);
        let lock = match &p.record.lock_sha256 {
            Some(sha256) => installed_lock(&p.dir, sha256).ok()?,
            None => {
                let size = fs::symlink_metadata(&path).ok().filter(|f| f.is_file())?.len();
                let listed = LockedFile { path: icon.to_owned(), size, sha256: String::new(), exec: false };
                FilesLock { lock_version: LOCK_VERSION, files: vec![listed] }
            }
        };
        package::check_icon(&p.dir, &lock, icon).ok()?;
        let listed = lock.files.iter().find(|f| f.path == icon)?;
        let mut bytes = Vec::new();
        File::open(&path).ok()?.take(listed.size + 1).read_to_end(&mut bytes).ok()?;
        let hashed = p.record.lock_sha256.is_none() || package::sha256_hex(&bytes) == listed.sha256;
        if bytes.len() as u64 != listed.size || !hashed {
            return None;
        }
        let kind = if icon.to_ascii_lowercase().ends_with(".svg") { "image/svg+xml" } else { "image/png" };
        Some(format!("data:{kind};base64,{}", STANDARD.encode(&bytes)))
    }

    /// The registry's rows, by id. A key whose end is no plugin id is no
    /// plugin's.
    fn rows(&self) -> PluginResult<Vec<(PluginId, String)>> {
        let rows = self.db.settings_in(ROW)?;
        Ok(rows
            .into_iter()
            .filter_map(|(key, value)| Some((PluginId::parse(&key[ROW.len()..]).ok()?, value)))
            .collect())
    }

    /// The record of `id`, to change or replace: none when it isn't
    /// installed, and an error when a later AutoScene wrote its row.
    fn record(&self, id: &PluginId) -> PluginResult<Option<Record>> {
        let Some(value) = self.db.setting(&row_key(id))? else { return Ok(None) };
        read_row(id, &value).map(Some).ok_or_else(installed_by_a_newer_autoscene)
    }

    fn write_record(&self, record: &Record) -> PluginResult<()> {
        let json = serde_json::to_string(record).expect("a plugin record serializes");
        Ok(self.db.write_settings(&[(&row_key(&record.id), Some(&json))])?)
    }

    /// Writes the record of a plugin just installed and its extra row, and
    /// its settings row `settings` when given (none removes it), in one
    /// transaction.
    fn write_installed(&self, record: &Record, extra: &Extra, settings: Option<Option<&str>>) -> PluginResult<()> {
        let json = serde_json::to_string(record).expect("a plugin record serializes");
        self.write_rows(&record.id, Some(&json), extra, settings)
    }

    /// Writes the extra row of `id`, and its settings row `settings` when
    /// given (none removes it), in one transaction.
    fn write_extra(&self, id: &PluginId, extra: &Extra, settings: Option<Option<&str>>) -> PluginResult<()> {
        self.write_rows(id, None, extra, settings)
    }

    /// Writes the rows of `id` in one transaction: its record when given,
    /// its extra row, and its settings row when given (none removes it).
    fn write_rows(
        &self,
        id: &PluginId,
        record: Option<&str>,
        extra: &Extra,
        settings: Option<Option<&str>>,
    ) -> PluginResult<()> {
        let extra_json = serde_json::to_string(extra).expect("a plugin's extra row serializes");
        let (row, extra_row, settings_row) = (row_key(id), extra_key(id), settings_key(id));
        let mut rows: Vec<(&str, Option<&str>)> = record.map(|json| (row.as_str(), Some(json))).into_iter().collect();
        rows.push((&extra_row, Some(&extra_json)));
        rows.extend(settings.map(|value| (settings_row.as_str(), value)));
        Ok(self.db.write_settings(&rows)?)
    }

    /// The extra row of the plugin of `record` as this AutoScene reads it:
    /// none when a later AutoScene wrote it, else the one of its install,
    /// empty when there is none (AutoScene 1.0 installed it) or the row is
    /// left from before ([`orphaned`]).
    fn stored_extra(&self, record: &Record) -> PluginResult<Option<Extra>> {
        let empty = Extra { installed_at: record.installed_at, ..Extra::default() };
        let Some(value) = self.db.setting(&extra_key(&record.id))? else { return Ok(Some(empty)) };
        Ok(read_extra(&record.id, &value)
            .map(|extra| if extra.installed_at == record.installed_at { extra } else { empty }))
    }

    /// The extra row of the plugin of `record`, to change: an error when a
    /// later AutoScene wrote it.
    fn extra(&self, record: &Record) -> PluginResult<Extra> {
        self.stored_extra(record)?.ok_or_else(installed_by_a_newer_autoscene)
    }

    /// The settings row of `id` without the values kept under the keys of
    /// the secret settings of `m`, from before they were secrets: its text,
    /// or none when nothing else is left in it. None when it holds no such
    /// value, or doesn't read (a later AutoScene's, which stays as it is).
    fn settings_without_secrets(&self, id: &PluginId, m: &Manifest) -> PluginResult<Option<Option<String>>> {
        let Some(mut row) = self.stored_row(id)? else { return Ok(None) };
        let before = row.len();
        row.retain(|key, _| !m.secret_keys().any(|k| k == key));
        if row.len() == before {
            return Ok(None);
        }
        Ok(Some((!row.is_empty()).then(|| serde_json::to_string(&row).expect("setting values serialize"))))
    }

    /// Removes the secrets of `id` named `keys` from the keychain,
    /// best-effort: one it refuses is logged by its key, and stays. A
    /// library that never kept a secret has none there.
    fn forget_secrets(&self, id: &PluginId, keys: &[String]) {
        if keys.is_empty() {
            return;
        }
        let library = match self.library() {
            Ok(Some(library)) => library,
            Ok(None) => return,
            Err(e) => {
                tracing::warn!(plugin = %id, error = %e.detail(), "a plugin's secrets stay in the keychain");
                return;
            }
        };
        for key in keys {
            if let Err(e) = self.secrets.delete(library, id, key) {
                tracing::warn!(plugin = %id, key = %key, error = %e.detail(), "a plugin's secret stays in the keychain");
            }
        }
    }

    /// The plugin of the row `value`.
    fn installed(&self, id: PluginId, value: &str) -> Installed {
        match read_row(&id, value) {
            Some(record) => self.load(record),
            None => Installed {
                dir: self.root.join(id.as_str()),
                record: unreadable(id, value),
                manifest: None,
                warnings: Vec::new(),
                broken: Some(NEWER.into()),
                extra: Extra::default(),
            },
        }
    }

    /// The plugin of `record`, with the manifest in its folder, while it
    /// works with this AutoScene and runs as the person agreed to.
    fn load(&self, record: Record) -> Installed {
        let dir = match &record.source {
            Source::Package { folder } => self.root.join(record.id.as_str()).join(folder),
            Source::Folder { path } => PathBuf::from(path),
        };
        let (extra, read) = match self.stored_extra(&record) {
            Ok(Some(extra)) => {
                // Checked again here: AutoScene may have been put back to an older version.
                let read =
                    read_manifest(&record, &dir).and_then(|parsed| match parsed.manifest.requirement_met(&self.app) {
                        Ok(()) => Ok(parsed),
                        Err(PluginError::TooNew(_)) => Err(NEWER.into()),
                        Err(e) => Err(e.to_string()),
                    });
                // A package's plugin.json is its lock's; a loaded folder's may say another now.
                let read = read.and_then(|parsed| match parsed.manifest.runtime.kind == extra.agreed_runtime() {
                    true => Ok(parsed),
                    false => Err(RUNTIME_CHANGED.to_owned()),
                });
                (extra, read)
            }
            Ok(None) => (Extra::default(), Err(NEWER.into())),
            Err(e) => (Extra::default(), Err(e.to_string())),
        };
        let (manifest, warnings, broken) = match read {
            Ok(Parsed { manifest, warnings }) => (Some(manifest), warnings, None),
            Err(broken) => (None, Vec::new(), Some(broken)),
        };
        Installed { record, manifest, warnings, dir, broken, extra }
    }

    /// Removes `path` (a file or folder), best-effort. It is moved into
    /// `.trash` first ([`Store::move_away`]), so it is gone from its place at
    /// once, also when a file in it can't be removed yet; the next open
    /// removes what stays there.
    fn throw_away(&self, path: &Path) {
        let Some(gone) = self.move_away(path) else { return };
        if let Err(e) = remove_all(&gone) {
            tracing::warn!(path = %as_core::paths::name(path), error = %e, "a plugin's old files stay");
        }
    }

    /// Moves `path` into `.trash` under a name of its own, and gives where
    /// it is to be removed: there, or where it is for a plugin's data on
    /// another drive. None when it is gone already, or can't be moved (a
    /// running program holds it on Windows): it stays whole where it is, for
    /// the next open to sweep.
    fn move_away(&self, path: &Path) -> Option<PathBuf> {
        let trash = self.root.join(TRASH);
        // Made again when something removed it, so the rename has a place.
        let _ = fs::create_dir_all(&trash);
        let trash = trash.join(Uuid::now_v7().simple().to_string());
        match fs::rename(path, &trash) {
            Ok(()) => Some(trash),
            // Gone already; not a trash folder that is missing.
            Err(_) if fs::symlink_metadata(path).is_err_and(|e| e.kind() == ErrorKind::NotFound) => None,
            Err(_) if !path.starts_with(&self.root) => Some(path.to_path_buf()),
            Err(e) => {
                tracing::warn!(path = %as_core::paths::name(path), error = %e, "a plugin's old files stay");
                None
            }
        }
    }

    /// The person's values of the settings of `id`, or none when their row
    /// doesn't read (a later AutoScene's). A value that doesn't read is left
    /// out.
    fn stored_values(&self, id: &PluginId) -> PluginResult<Option<BTreeMap<String, SettingValue>>> {
        Ok(self.stored_row(id)?.map(typed))
    }

    /// The person's values of the settings of `id` as their row holds them,
    /// or none when it doesn't read (a later AutoScene's).
    fn stored_row(&self, id: &PluginId) -> PluginResult<Option<BTreeMap<String, Value>>> {
        let Some(json) = self.db.setting(&settings_key(id))? else { return Ok(Some(BTreeMap::new())) };
        match serde_json::from_str(&json) {
            Ok(values) => Ok(Some(values)),
            Err(e) => {
                tracing::warn!(plugin = %id, error = %e, "a plugin's settings don't read");
                Ok(None)
            }
        }
    }
}

/// The values of a settings row that read as setting values.
fn typed(row: BTreeMap<String, Value>) -> BTreeMap<String, SettingValue> {
    row.into_iter().filter_map(|(key, value)| Some((key, serde_json::from_value(value).ok()?))).collect()
}

fn row_key(id: &PluginId) -> String {
    format!("{ROW}{id}")
}

fn settings_key(id: &PluginId) -> String {
    format!("{SETTINGS_ROW}{id}")
}

fn extra_key(id: &PluginId) -> String {
    format!("{EXTRA_ROW}{id}")
}

fn not_found(id: &PluginId) -> PluginError {
    PluginError::NotFound(id.to_string())
}

fn installed_by_a_newer_autoscene() -> PluginError {
    invalid(
        "A newer AutoScene installed this plugin, so this one can't change it. Update AutoScene, or uninstall the plugin.",
    )
}

/// The extra row `value` of `id`, when this AutoScene can read it: of
/// [`EXTRA_VERSION`], naming a runtime it knows.
fn read_extra(id: &PluginId, value: &str) -> Option<Extra> {
    match serde_json::from_str::<Extra>(value) {
        Ok(extra) if extra.v == EXTRA_VERSION => Some(extra),
        Ok(_) => None,
        Err(e) => {
            tracing::debug!(plugin = %id, error = %e, "a plugin's extra row doesn't read");
            None
        }
    }
}

/// The keys of the secrets an extra row `value` lists, also of one a later
/// AutoScene wrote, as far as they read: what to remove from the keychain
/// with it.
fn secret_names(value: &str) -> Vec<String> {
    let row: Value = serde_json::from_str(value).unwrap_or_default();
    let listed = row["secrets"].as_array().map_or(&[][..], Vec::as_slice);
    listed.iter().filter_map(|k| k.as_str().map(str::to_owned)).collect()
}

/// Whether the extra row `value` of `id` is left from before the plugin's
/// row `row` (none when it has none): AutoScene 1.0 uninstalled the plugin,
/// or installed it again since and wrote only its record. Rows a later
/// AutoScene wrote are as they are.
fn orphaned(id: &PluginId, row: Option<&str>, value: &str) -> bool {
    let Some(row) = row else { return true };
    match (read_row(id, row), read_extra(id, value)) {
        (Some(record), Some(extra)) => extra.installed_at != record.installed_at,
        _ => false,
    }
}

fn changed_while_waiting() -> PluginError {
    invalid("The plugin's files changed while it waited to be installed. Open it again.")
}

/// The record of the row `value` of `id`, when this AutoScene can read it:
/// of [`RECORD_VERSION`], for `id`, with a version, and with its files in
/// one folder under `plugins/<id>/` or a loaded folder's absolute path.
fn read_row(id: &PluginId, value: &str) -> Option<Record> {
    let record = match serde_json::from_str::<Record>(value) {
        Ok(record) => record,
        Err(e) => {
            tracing::debug!(plugin = %id, error = %e, "a plugin's row doesn't read");
            return None;
        }
    };
    let sound = match &record.source {
        Source::Package { folder } => {
            !folder.contains('/') && package::check_path(folder).is_ok() && record.lock_sha256.is_some()
        }
        Source::Folder { path } => Path::new(path).is_absolute() && record.lock_sha256.is_none(),
    };
    (record.v == RECORD_VERSION && record.id == *id && Version::parse(&record.version).is_ok() && sound)
        .then_some(record)
}

/// What can be shown of the row `value` this AutoScene can't read: its id
/// and version. It counts as off, and its files as unknown.
fn unreadable(id: PluginId, value: &str) -> Record {
    let row: Value = serde_json::from_str(value).unwrap_or_default();
    Record {
        v: row["v"].as_u64().and_then(|v| u32::try_from(v).ok()).unwrap_or_default(),
        id,
        version: row["version"].as_str().filter(|v| Version::parse(v).is_ok()).unwrap_or_default().to_owned(),
        source: Source::Package { folder: String::new() },
        trust: Trust::Unreviewed,
        author_key: None,
        permissions: Vec::new(),
        network_hosts: Vec::new(),
        enabled: false,
        after_export: Vec::new(),
        lock_sha256: None,
        installed_at: 0,
    }
}

/// The manifest in the folder `dir` of `record`, or why there is none to
/// go by: a package's must be the one its install checked (its lock is the
/// recorded one and lists it as it is), a loaded folder's must name the
/// same plugin.
fn read_manifest(record: &Record, dir: &Path) -> Result<Parsed, String> {
    if !dir.is_dir() {
        return Err(MISSING.into());
    }
    let bytes = manifest::read_file(&dir.join(MANIFEST));
    let bytes = match (&record.lock_sha256, bytes) {
        (Some(sha256), Ok(bytes)) => {
            let lock = installed_lock(dir, sha256)?;
            let listed = lock.files.iter().find(|f| f.path == MANIFEST);
            if listed.is_none_or(|f| f.sha256 != package::sha256_hex(&bytes)) {
                return Err(CHANGED.into());
            }
            bytes
        }
        (Some(_), Err(_)) => return Err(CHANGED.into()),
        (None, Ok(bytes)) => bytes,
        (None, Err(e)) if e.kind() == ErrorKind::NotFound => return Err(MISSING.into()),
        (None, Err(e)) => return Err(as_core::io_message(&e).into()),
    };
    let parsed = manifest::parse_for_any_app(&bytes).map_err(|e| e.to_string())?;
    if parsed.manifest.id != record.id {
        return Err("Its plugin.json names another plugin now.".into());
    }
    Ok(parsed)
}

/// The lock in an installed package's folder `dir`, when it is the one its
/// install recorded (of SHA-256 `sha256`); else why it can't be used.
pub(crate) fn installed_lock(dir: &Path, sha256: &str) -> Result<FilesLock, String> {
    let mut bytes = Vec::new();
    let read = File::open(dir.join(LOCK)).and_then(|file| file.take(MAX_LOCK_BYTES + 1).read_to_end(&mut bytes));
    if read.is_err() || package::sha256_hex(&bytes) != sha256 {
        return Err(CHANGED.into());
    }
    package::read_lock(&bytes).map_err(|_| CHANGED.into())
}

/// Whether a plugin of `trust` may be installed: one AutoScene hasn't
/// reviewed needs the setting that allows them, and the person's yes.
fn allowed(trust: Trust, allow_unreviewed: bool, accept_unreviewed: bool) -> PluginResult<()> {
    match trust {
        Trust::Reviewed => Ok(()),
        _ if !allow_unreviewed => Err(PluginError::NotAllowed),
        _ if !accept_unreviewed => Err(invalid("Confirm that you want to install it.")),
        _ => Ok(()),
    }
}

/// Whether the plugin of manifest `m`, of `trust` and with the lock of
/// SHA-256 `lock_sha256` (none for a folder), may replace the installed
/// `previous`: a reviewed plugin takes only reviewed versions, none goes
/// back to an older version, and a package's version installs again only
/// with the same files (a repair). The pin is [`trust::judge`]'s.
fn check_update(previous: &Record, m: &Manifest, trust: Trust, lock_sha256: Option<&str>) -> PluginResult<()> {
    if previous.trust == Trust::Reviewed && trust != Trust::Reviewed {
        return Err(invalid("AutoScene reviewed the installed version of this plugin, and not this one."));
    }
    let (Ok(installed), Ok(new)) = (Version::parse(&previous.version), Version::parse(&m.version)) else {
        return Err(invalid("The plugin's version isn't valid."));
    };
    match installed.cmp_precedence(&new) {
        Ordering::Greater => Err(invalid(format!(
            "Version {installed} is installed, which is newer than {new}. Uninstall it first to go back."
        ))),
        Ordering::Equal
            if matches!(previous.source, Source::Package { .. }) && previous.lock_sha256.as_deref() != lock_sha256 =>
        {
            Err(invalid(format!(
                "Version {installed} is installed already, with other files. A changed plugin needs a new version."
            )))
        }
        _ => Ok(()),
    }
}

/// The manifest of `p`, or why it has none.
pub(crate) fn manifest_of(p: &Installed) -> PluginResult<&Manifest> {
    p.manifest.as_ref().ok_or_else(|| invalid(p.broken.clone().unwrap_or_else(|| MISSING.into())))
}

/// The value of each setting of `specs`: the stored one while it fits, else
/// its default.
fn merged(specs: &[SettingSpec], stored: &BTreeMap<String, SettingValue>) -> BTreeMap<String, SettingValue> {
    let value = |s: &SettingSpec| {
        stored.get(&s.key).filter(|v| check_value(s, v).is_ok()).cloned().or_else(|| default_of(&s.kind))
    };
    specs.iter().filter_map(|s| Some((s.key.clone(), value(s)?))).collect()
}

fn default_of(kind: &SettingKind) -> Option<SettingValue> {
    match kind {
        SettingKind::String { default, .. } | SettingKind::Select { default, .. } => {
            default.clone().map(SettingValue::Text)
        }
        SettingKind::Number { default, .. } => default.map(SettingValue::Number),
        SettingKind::Boolean { default } => Some(SettingValue::Bool(*default)),
        SettingKind::Secret { .. } => None,
    }
}

/// Checks `value` against its setting `spec`: text of at most its length
/// without NUL, a finite number within its bounds (whole when it must be),
/// on or off, or one of a select's values. A secret has no value here: so
/// one kept under its key before it was a secret is never handed on.
fn check_value(spec: &SettingSpec, value: &SettingValue) -> PluginResult<()> {
    let refuse = |why: String| Err(invalid(format!("{} {why}.", shown(&spec.title))));
    match (&spec.kind, value) {
        (SettingKind::String { max_length, .. }, SettingValue::Text(text)) => {
            if text.contains('\0') {
                return refuse("can't hold a NUL character".into());
            }
            if text.chars().count() > *max_length as usize {
                return refuse(format!("can be at most {max_length} characters"));
            }
        }
        (SettingKind::String { .. }, _) => return refuse("must be text".into()),
        (SettingKind::Number { min, max, integer, .. }, SettingValue::Number(n)) => {
            if !n.is_finite() {
                return refuse("must be a number".into());
            }
            if *integer && n.fract() != 0.0 {
                return refuse("must be a whole number".into());
            }
            if let Some(min) = min.filter(|min| n < min) {
                return refuse(format!("must be at least {min}"));
            }
            if let Some(max) = max.filter(|max| n > max) {
                return refuse(format!("must be at most {max}"));
            }
        }
        (SettingKind::Number { .. }, _) => return refuse("must be a number".into()),
        (SettingKind::Boolean { .. }, SettingValue::Bool(_)) => {}
        (SettingKind::Boolean { .. }, _) => return refuse("must be on or off".into()),
        (SettingKind::Select { options, .. }, SettingValue::Text(text)) if options.iter().any(|o| o.value == *text) => {
        }
        (SettingKind::Select { .. }, _) => return refuse("must be one of its choices".into()),
        (SettingKind::Secret { .. }, _) => return refuse("is kept in your system's keychain, never as a value".into()),
    }
    Ok(())
}

/// Renames `from` to `to`, trying again for a moment while Windows refuses:
/// a virus scanner may still hold a file just unpacked there.
fn rename(from: &Path, to: &Path) -> io::Result<()> {
    let start = Instant::now();
    loop {
        match fs::rename(from, to) {
            Err(e) if cfg!(windows) && e.kind() == ErrorKind::PermissionDenied && start.elapsed() < RENAME_WAIT => {
                std::thread::sleep(Duration::from_millis(50));
            }
            renamed => return renamed,
        }
    }
}

/// Removes the file or folder `path`.
fn remove_all(path: &Path) -> io::Result<()> {
    if fs::symlink_metadata(path)?.is_dir() { fs::remove_dir_all(path) } else { fs::remove_file(path) }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use minisign::KeyPair;
    use serde_json::json;
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    use super::*;
    use crate::package::{AUTHOR_KEY, SIG};
    use crate::protocol::{ActionKind, Trigger};
    use crate::secrets::MemorySecrets;

    const ID: &str = "dev.autoscene.hello-export";

    type Files = Vec<(String, Vec<u8>)>;

    fn app() -> Version {
        Version::parse("1.0.3-BETA").unwrap()
    }

    fn id() -> PluginId {
        PluginId::parse(ID).unwrap()
    }

    /// A test's app data folder and library.db, with room beside them for
    /// packages and a developer's folder.
    struct Place {
        tmp: tempfile::TempDir,
        paths: AppPaths,
        db: Arc<LibraryDb>,
        /// The keychain of its stores.
        secrets: Arc<MemorySecrets>,
    }

    impl Place {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let paths = AppPaths::at(tmp.path().join("data"));
            let db = Arc::new(LibraryDb::open(&paths.library_db()).unwrap());
            Self { tmp, paths, db, secrets: Arc::default() }
        }

        fn store(&self) -> Store {
            self.store_with(Keys::new(&[]).unwrap())
        }

        fn store_with(&self, keys: Keys) -> Store {
            Store::open(&self.paths, Arc::clone(&self.db), keys, app(), self.secrets.clone()).unwrap()
        }

        /// The id the keychain keeps its secrets under.
        fn library(&self) -> String {
            self.db.setting(KEYCHAIN_ROW).unwrap().expect("a secret was set")
        }

        /// What the library's rows hold, all of them.
        fn rows(&self) -> String {
            self.db.settings_in("").unwrap().into_iter().map(|(key, value)| format!("{key}={value}\n")).collect()
        }

        /// What `plugins/` holds, two levels deep: paths with `/`, sorted.
        fn plugins(&self) -> Vec<String> {
            let mut out = Vec::new();
            for entry in fs::read_dir(self.paths.plugins()).unwrap() {
                let entry = entry.unwrap();
                let name = entry.file_name().to_string_lossy().into_owned();
                if entry.file_type().unwrap().is_dir() {
                    for inner in fs::read_dir(entry.path()).unwrap() {
                        out.push(format!("{name}/{}", inner.unwrap().file_name().to_string_lossy()));
                    }
                }
                out.push(name);
            }
            out.sort();
            out
        }

        /// Makes library.db refuse to write the row of [`ID`], until
        /// [`Place::allow_writes`].
        fn refuse_writes(&self) {
            self.sql(&format!(
                "CREATE TRIGGER refuse_insert BEFORE INSERT ON settings WHEN NEW.key = 'plugin.{ID}'
                     BEGIN SELECT RAISE(ABORT, 'refused'); END;
                 CREATE TRIGGER refuse_update BEFORE UPDATE ON settings WHEN NEW.key = 'plugin.{ID}'
                     BEGIN SELECT RAISE(ABORT, 'refused'); END;"
            ));
        }

        fn allow_writes(&self) {
            self.sql("DROP TRIGGER refuse_insert; DROP TRIGGER refuse_update;");
        }

        fn sql(&self, sql: &str) {
            rusqlite::Connection::open(self.paths.library_db()).unwrap().execute_batch(sql).unwrap();
        }

        /// A package of `files` (`plugin.json` first) with their lock, as
        /// asp packs it: signed by `signer` if given, with its author.pub
        /// when `with_pub`.
        fn package(&self, files: &Files, signer: Option<&KeyPair>, with_pub: bool) -> PathBuf {
            let listed = files.iter().map(|(path, bytes)| LockedFile {
                path: path.clone(),
                size: bytes.len() as u64,
                sha256: package::sha256_hex(bytes),
                exec: false,
            });
            let lock = FilesLock { lock_version: LOCK_VERSION, files: listed.collect() };
            let lock_bytes = serde_json::to_vec(&lock).unwrap();
            let mut entries = files.clone();
            entries.push((LOCK.into(), lock_bytes.clone()));
            if let Some(pair) = signer {
                let m: Value = serde_json::from_slice(&files[0].1).unwrap();
                let (id, version) =
                    (PluginId::parse(m["id"].as_str().unwrap()).unwrap(), m["version"].as_str().unwrap());
                let comment = trust::trusted_comment(&id, version, &package::sha256_hex(&lock_bytes));
                let sig = minisign::sign(Some(&pair.pk), &pair.sk, &lock_bytes[..], Some(&comment), None).unwrap();
                entries.push((SIG.into(), sig.into_string().into_bytes()));
                if with_pub {
                    entries.push((AUTHOR_KEY.into(), pair.pk.to_base64().into_bytes()));
                }
            }
            let path = self.tmp.path().join(format!("{}.asp", Uuid::now_v7().simple()));
            let mut zip = ZipWriter::new(File::create(&path).unwrap());
            for (name, bytes) in &entries {
                zip.start_file(name.as_str(), SimpleFileOptions::default()).unwrap();
                zip.write_all(bytes).unwrap();
            }
            zip.finish().unwrap();
            path
        }

        fn unsigned(&self, version: &str) -> PathBuf {
            self.package(&hello(version), None, false)
        }

        /// A developer's folder of `files`.
        fn folder(&self, name: &str, files: &Files) -> PathBuf {
            let dir = self.tmp.path().join(name);
            fs::create_dir(&dir).unwrap();
            for (path, bytes) in files {
                fs::write(dir.join(path), bytes).unwrap();
            }
            dir
        }
    }

    /// A plugin's manifest: two export actions and a setting of each kind.
    fn manifest(id: &str, version: &str) -> Value {
        json!({
            "manifestVersion": 1,
            "id": id,
            "version": version,
            "name": "Hello Export",
            "description": "Says hello after an export.",
            "author": {"name": "AutoScene"},
            "engines": {"autoscene": ">=1.0.0, <2", "protocol": 1},
            "runtime": {"type": "process", "command": {"*": ["uv", "run", "--script", "--locked", "main.py"]}},
            "permissions": ["exports:read"],
            "contributes": {
                "exportActions": [{"id": "hello", "title": "Say hello"}, {"id": "wave", "title": "Wave"}],
                "settings": [
                    {"key": "greeting", "type": "string", "title": "Greeting", "default": "Hi", "maxLength": 5},
                    {"key": "count", "type": "number", "title": "Count", "default": 3, "min": 1, "max": 10,
                     "integer": true},
                    {"key": "loud", "type": "boolean", "title": "Loud", "default": true},
                    {"key": "mode", "type": "select", "title": "Mode", "default": "a",
                     "options": [{"value": "a", "label": "A"}, {"value": "b", "label": "B"}]}
                ]
            }
        })
    }

    /// The files of the plugin of `manifest`: a uv script and its lock.
    fn files(manifest: &Value) -> Files {
        vec![
            ("plugin.json".into(), serde_json::to_vec_pretty(manifest).unwrap()),
            ("main.py".into(), b"print('hello')\n".to_vec()),
            ("main.py.lock".into(), b"version = 1\n".to_vec()),
        ]
    }

    fn hello(version: &str) -> Files {
        files(&manifest(ID, version))
    }

    /// [`hello`] in manifest version 2 and protocol 2, with a secret setting
    /// of each of `keys`.
    fn with_secrets(version: &str, keys: &[&str]) -> Files {
        let mut m = manifest(ID, version);
        m["manifestVersion"] = json!(2);
        m["engines"]["protocol"] = json!(2);
        let settings = m["contributes"]["settings"].as_array_mut().unwrap();
        settings.extend(keys.iter().map(|k| json!({"key": k, "type": "secret", "title": format!("The {k}")})));
        files(&m)
    }

    fn extra_row(place: &Place) -> Option<String> {
        place.db.setting(&format!("plugin_extra.{ID}")).unwrap()
    }

    /// The extra row of the process plugin `p` that lists `secrets`.
    fn extra_of(p: &Installed, secrets: &[&str]) -> Option<String> {
        let secrets = serde_json::to_string(secrets).unwrap();
        Some(format!(r#"{{"v":1,"installedAt":{},"runtime":"process","secrets":{secrets}}}"#, p.record.installed_at))
    }

    fn settings_row(place: &Place) -> Option<String> {
        place.db.setting(&format!("plugin_settings.{ID}")).unwrap()
    }

    /// Installs the package at `path`, with the setting on and the person's yes.
    fn install(store: &Store, path: &Path) -> PluginResult<Installed> {
        store.commit(store.stage(path)?, true, true)
    }

    /// What `plugins/` holds with only `p` installed.
    fn holding(p: &Installed) -> Vec<String> {
        let Source::Package { folder } = &p.record.source else { panic!("{:?}", p.record.source) };
        vec![".staging".into(), ".trash".into(), ID.into(), format!("{ID}/{folder}")]
    }

    fn key_pair() -> KeyPair {
        KeyPair::generate_unencrypted_keypair().unwrap()
    }

    fn values(pairs: &[(&str, SettingValue)]) -> BTreeMap<String, SettingValue> {
        pairs.iter().map(|(key, value)| (key.to_string(), value.clone())).collect()
    }

    #[test]
    fn an_install_is_all_or_nothing() {
        let place = Place::new();
        let store = place.store();
        let package = place.unsigned("1.0.0");
        place.refuse_writes();
        let t = store.stage(&package).unwrap();
        assert_eq!(place.plugins().len(), 3, "staged: {:?}", place.plugins());
        assert!(matches!(store.commit(t, true, true), Err(PluginError::Db(_))));
        assert!(store.list().unwrap().is_empty());
        assert_eq!(place.plugins(), [".staging", ".trash"]);

        place.allow_writes();
        let p = install(&store, &package).unwrap();
        assert_eq!((p.record.version.as_str(), p.broken.as_deref()), ("1.0.0", None));
        assert_eq!((p.record.enabled, p.record.trust), (true, Trust::Unreviewed));
        assert_eq!(place.plugins(), holding(&p));
        assert!(p.dir.join("main.py").is_file());
    }

    #[test]
    fn an_update_replaces_the_old_version_after_the_row_is_written() {
        let place = Place::new();
        let store = place.store();
        let old = install(&store, &place.unsigned("1.0.0")).unwrap();
        store.set_enabled(&id(), false).unwrap();
        store.set_after_export(&id(), "wave", true).unwrap();
        store.set_after_export(&id(), "hello", true).unwrap();
        // 1.1.0 has no wave any more.
        let mut m = manifest(ID, "1.1.0");
        m["contributes"]["exportActions"] = json!([{"id": "hello", "title": "Say hello"}]);
        let update = place.package(&files(&m), None, false);

        place.refuse_writes();
        assert!(install(&store, &update).is_err());
        let p = store.get(&id()).unwrap();
        assert_eq!(
            (p.record.version.as_str(), p.record.after_export.clone()),
            ("1.0.0", vec!["hello".into(), "wave".into()])
        );
        assert_eq!((p.dir.as_path(), p.broken.as_deref()), (old.dir.as_path(), None));
        assert_eq!(place.plugins(), holding(&old));

        place.allow_writes();
        let new = install(&store, &update).unwrap();
        assert_eq!(new.record.version, "1.1.0");
        assert!(!new.record.enabled, "a plugin that was off stays off");
        assert_eq!(new.record.after_export, ["hello"]);
        assert_eq!(store.get(&id()).unwrap().record, new.record);
        assert_eq!(place.plugins(), holding(&new));
    }

    #[test]
    fn leftovers_from_a_crash_are_swept_at_open() {
        let place = Place::new();
        let p = install(&place.store(), &place.unsigned("1.0.0")).unwrap();
        let make = |path: &str| {
            let path = place.paths.plugins().join(path);
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join("x"), "x").unwrap();
        };
        // A staged and a thrown-away package, a version an update left, a
        // plugin whose row went, and a later AutoScene's plugin.
        make(".staging/0190f0e8a3b47c1d");
        make(".trash/0190f0e8a3b47c1e");
        make(&format!("{ID}/0.9.0+0badf00d"));
        make("dev.autoscene.gone/1.0.0+12345678");
        make("dev.autoscene.later/2.0.0+12345678");
        let later = r#"{"v":2,"id":"dev.autoscene.later","version":"2.0.0"}"#;
        place.db.set_setting("plugin.dev.autoscene.later", later).unwrap();

        let store = place.store();
        let mut kept = holding(&p);
        kept.extend(["dev.autoscene.later".into(), "dev.autoscene.later/2.0.0+12345678".into()]);
        // Out of their places at once, all in the trash, whose old files stay
        // with them until they are removed.
        let trash = place.paths.plugins().join(TRASH);
        assert_eq!(fs::read_dir(&trash).unwrap().count(), 4);
        // What is staged meanwhile isn't touched by their removal.
        let staged = store.stage(&place.unsigned("1.1.0")).unwrap();
        store.remove_leftovers();
        assert_eq!(fs::read_dir(&trash).unwrap().count(), 0);
        assert!(staged.staged.as_ref().unwrap().join("plugin.json").is_file());
        store.discard(staged);
        assert_eq!(place.plugins(), kept);
        assert!(store.get(&id()).unwrap().broken.is_none());
        assert_eq!(place.db.setting("plugin.dev.autoscene.later").unwrap().as_deref(), Some(later));
    }

    #[test]
    fn a_row_whose_folder_is_gone_is_listed_as_broken() {
        let place = Place::new();
        let package = place.unsigned("1.0.0");
        let p = install(&place.store(), &package).unwrap();
        fs::remove_dir_all(&p.dir).unwrap();
        let store = place.store();
        let listed = store.list().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!((listed[0].broken.as_deref(), listed[0].manifest.is_none()), (Some(MISSING), true));
        assert!(store.values(&id()).is_err());

        // Installed again, it works again.
        let p = install(&store, &package).unwrap();
        assert!(p.broken.is_none());
        // A file of it changed is as good as gone.
        fs::write(p.dir.join("plugin.json"), b"{}").unwrap();
        assert_eq!(store.get(&id()).unwrap().broken.as_deref(), Some(CHANGED));
        // So is another files.lock, also one that lists the same files.
        let p = install(&store, &package).unwrap();
        let lock: Value = serde_json::from_slice(&fs::read(p.dir.join(LOCK)).unwrap()).unwrap();
        fs::write(p.dir.join(LOCK), serde_json::to_vec_pretty(&lock).unwrap()).unwrap();
        assert_eq!(store.get(&id()).unwrap().broken.as_deref(), Some(CHANGED));
    }

    #[test]
    fn a_row_that_does_not_fit_its_plugin_is_listed_as_needing_a_newer_autoscene() {
        let place = Place::new();
        let store = place.store();
        install(&store, &place.unsigned("1.0.0")).unwrap();
        let row = format!("plugin.{ID}");
        let written: Value = serde_json::from_str(&place.db.setting(&row).unwrap().unwrap()).unwrap();
        let changes: [fn(&mut Value); 5] = [
            |r| r["id"] = json!("dev.autoscene.other"),
            |r| r["version"] = json!("one"),
            |r| r["source"]["folder"] = json!("1.0.0+0badf00d/x"),
            |r| r["lockSha256"] = Value::Null,
            // A loaded folder's path must be absolute.
            |r| {
                r["source"] = json!({"kind": "folder", "path": "hello-export"});
                r["lockSha256"] = Value::Null;
            },
        ];
        for change in changes {
            let mut r = written.clone();
            change(&mut r);
            place.db.set_setting(&row, &r.to_string()).unwrap();
            assert_eq!(store.get(&id()).unwrap().broken.as_deref(), Some(NEWER), "{r}");
        }
    }

    #[test]
    fn a_plugin_is_listed_as_broken_on_an_autoscene_it_does_not_work_with() {
        let place = Place::new();
        let p = install(&place.store(), &place.unsigned("1.0.0")).unwrap();
        // It works with ">=1.0.0, <2".
        let broken_on = |app: &str| {
            let (keys, app) = (Keys::new(&[]).unwrap(), Version::parse(app).unwrap());
            let store = Store::open(&place.paths, Arc::clone(&place.db), keys, app, place.secrets.clone()).unwrap();
            store.get(&id()).unwrap().broken
        };
        assert_eq!(broken_on("0.9.0").as_deref(), Some(NEWER));
        assert_eq!(broken_on("2.0.0").as_deref(), Some("This plugin only works with an older AutoScene."));
        assert_eq!(broken_on("1.5.0"), None);
        assert_eq!(place.plugins(), holding(&p), "its files stay");
    }

    #[test]
    fn an_unreadable_row_is_kept_and_listed_as_needing_a_newer_autoscene() {
        let place = Place::new();
        let later = r#"{"v":2,"id":"dev.autoscene.later","version":"2.0.0","runtime":"wasm"}"#;
        place.db.set_setting("plugin.dev.autoscene.later", later).unwrap();
        place.db.set_setting("plugin.dev.autoscene.damaged", "not json").unwrap();
        let store = place.store();
        let listed = store.list().unwrap();
        let shown: Vec<_> =
            listed.iter().map(|p| (p.record.id.as_str(), p.record.version.as_str(), p.broken.as_deref())).collect();
        assert_eq!(shown, [("dev.autoscene.damaged", "", Some(NEWER)), ("dev.autoscene.later", "2.0.0", Some(NEWER))]);

        let later_id = PluginId::parse("dev.autoscene.later").unwrap();
        assert!(store.set_enabled(&later_id, true).is_err());
        assert!(store.set_after_export(&later_id, "hello", true).is_err());
        assert!(store.set_values(&later_id, values(&[("count", SettingValue::Number(4.0))])).is_err());
        let package = place.package(&files(&manifest("dev.autoscene.later", "3.0.0")), None, false);
        assert!(store.stage(&package).is_err());
        assert_eq!(place.db.setting("plugin.dev.autoscene.later").unwrap().as_deref(), Some(later));
        assert_eq!(place.db.setting("plugin_settings.dev.autoscene.later").unwrap(), None);
        assert_eq!(place.plugins(), [".staging", ".trash"]);

        // Uninstalling it is the way out.
        store.uninstall(&later_id, false).unwrap();
        assert_eq!(store.list().unwrap().len(), 1);

        // A record this AutoScene could read, but of a later version.
        install(&store, &place.unsigned("1.0.0")).unwrap();
        let row = format!("plugin.{ID}");
        let v2 = place.db.setting(&row).unwrap().unwrap().replace(r#""v":1,"#, r#""v":2,"#);
        place.db.set_setting(&row, &v2).unwrap();
        assert_eq!(store.get(&id()).unwrap().broken.as_deref(), Some(NEWER));
        assert!(store.set_enabled(&id(), false).is_err());
        assert!(store.stage(&place.unsigned("1.1.0")).is_err());
        assert_eq!(place.db.setting(&row).unwrap(), Some(v2));
    }

    #[test]
    fn uninstalling_a_loaded_folder_never_deletes_it() {
        let place = Place::new();
        let store = place.store();
        let dev = place.folder("hello-export", &hello("1.0.0"));
        let t = store.stage_folder(&dev).unwrap();
        assert_eq!((t.verdict.trust, t.lock.is_none()), (Trust::Unreviewed, true));
        let p = store.commit(t, true, true).unwrap();
        assert_eq!(p.record.source, Source::Folder { path: dev.to_str().unwrap().into() });
        assert_eq!((p.dir.as_path(), p.broken.as_deref()), (dev.as_path(), None));
        fs::create_dir_all(store.data_dir(&id())).unwrap();
        fs::write(store.data_dir(&id()).join("state.json"), b"{}").unwrap();

        store.uninstall(&id(), true).unwrap();
        assert!(store.list().unwrap().is_empty());
        assert!(!store.data_dir(&id()).exists(), "its data went, as asked");
        let left = |dir: &Path| {
            let mut names: Vec<String> =
                fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
            names.sort();
            names
        };
        assert_eq!(left(&dev), ["main.py", "main.py.lock", "plugin.json"]);

        // Nor does a package that replaces it.
        store.commit(store.stage_folder(&dev).unwrap(), true, true).unwrap();
        let p = install(&store, &place.unsigned("1.1.0")).unwrap();
        assert_eq!(left(&dev), ["main.py", "main.py.lock", "plugin.json"]);
        // An installed version's folder isn't loaded: an update would throw it away.
        let e = store.stage_folder(&p.dir).unwrap_err();
        assert_eq!(e.to_string(), "That folder belongs to an installed plugin. Load the folder you work in.");
        assert_eq!(place.plugins(), holding(&p));
    }

    #[test]
    fn a_loaded_folder_replaces_a_package_and_leaves_no_files_in_plugins() {
        let place = Place::new();
        let store = place.store();
        install(&store, &place.unsigned("1.0.0")).unwrap();
        let dev = place.folder("hello-export", &hello("1.1.0"));
        let p = store.commit(store.stage_folder(&dev).unwrap(), true, true).unwrap();
        assert_eq!(store.list().unwrap().iter().map(|p| &p.record).collect::<Vec<_>>(), [&p.record]);
        assert_eq!(place.plugins(), [".staging", ".trash"]);
        // What lies in plugins/<id>/ of a loaded folder is no version of it.
        fs::create_dir_all(place.paths.plugins().join(ID).join("1.0.0+0badf00d")).unwrap();
        place.store().remove_leftovers();
        assert_eq!(place.plugins(), [".staging", ".trash"]);
        assert_eq!(fs::read_dir(&dev).unwrap().count(), 3);
    }

    #[test]
    fn a_loaded_folder_is_checked_as_it_is() {
        let place = Place::new();
        let store = place.store();
        let mut no_script = hello("1.0.0");
        no_script.retain(|(path, _)| path != "main.py");
        let e = store.stage_folder(&place.folder("no-script", &no_script)).unwrap_err();
        assert_eq!(e.to_string(), "The plugin folder is missing main.py, which its command needs.");

        let dev = place.folder("hello-export", &hello("1.0.0"));
        store.commit(store.stage_folder(&dev).unwrap(), true, true).unwrap();
        let other = serde_json::to_vec(&manifest("dev.autoscene.other", "1.0.0")).unwrap();
        fs::write(dev.join("plugin.json"), other).unwrap();
        assert_eq!(store.get(&id()).unwrap().broken.as_deref(), Some("Its plugin.json names another plugin now."));
    }

    #[test]
    fn an_unknown_plugin_or_action_is_refused() {
        let place = Place::new();
        let store = place.store();
        assert!(matches!(store.uninstall(&id(), false), Err(PluginError::NotFound(_))));
        install(&store, &place.unsigned("1.0.0")).unwrap();
        let e = store.set_after_export(&id(), "bow", true).unwrap_err();
        assert_eq!(e.to_string(), "The plugin has no export action bow.");
        assert!(store.get(&id()).unwrap().record.after_export.is_empty());
    }

    #[test]
    fn throwing_away_needs_no_trash_folder() {
        let place = Place::new();
        let store = place.store();
        let trash = place.paths.plugins().join(TRASH);
        install(&store, &place.unsigned("1.0.0")).unwrap();
        fs::create_dir_all(store.data_dir(&id())).unwrap();
        fs::remove_dir(&trash).unwrap();
        store.uninstall(&id(), true).unwrap();
        assert!(!store.data_dir(&id()).exists(), "its data went, as asked");
        assert_eq!(place.plugins(), [".staging", ".trash"]);

        // A file in its place: the data goes where it is, the plugin's
        // folder at the next open.
        install(&store, &place.unsigned("1.0.0")).unwrap();
        fs::create_dir_all(store.data_dir(&id())).unwrap();
        fs::remove_dir(&trash).unwrap();
        fs::write(&trash, b"x").unwrap();
        store.uninstall(&id(), true).unwrap();
        assert!(!store.data_dir(&id()).exists());
        place.store().remove_leftovers();
        assert_eq!(place.plugins(), [".staging", ".trash"]);
    }

    #[test]
    fn settings_values_are_checked_against_their_spec() {
        use SettingValue::{Bool, Number, Text};
        let place = Place::new();
        let store = place.store();
        install(&store, &place.unsigned("1.0.0")).unwrap();
        let with = |count: f64, loud: bool| {
            values(&[
                ("count", Number(count)),
                ("greeting", Text("Hi".into())),
                ("loud", Bool(loud)),
                ("mode", Text("a".into())),
            ])
        };
        assert_eq!(store.values(&id()).unwrap(), with(3.0, true));
        for (key, value, why) in [
            ("greeting", Text("Hello!".into()), "Greeting can be at most 5 characters."),
            ("greeting", Text("a\0b".into()), "Greeting can't hold a NUL character."),
            ("greeting", Number(1.0), "Greeting must be text."),
            ("count", Number(11.0), "Count must be at most 10."),
            ("count", Number(0.0), "Count must be at least 1."),
            ("count", Number(2.5), "Count must be a whole number."),
            ("count", Number(f64::NAN), "Count must be a number."),
            ("count", Text("4".into()), "Count must be a number."),
            ("loud", Text("yes".into()), "Loud must be on or off."),
            ("mode", Text("c".into()), "Mode must be one of its choices."),
        ] {
            // A value that fits is refused with one that doesn't.
            let e = store.set_values(&id(), values(&[("count", Number(4.0)), (key, value)])).unwrap_err();
            assert_eq!(e.to_string(), why);
        }
        assert_eq!(store.values(&id()).unwrap(), with(3.0, true));

        let set = values(&[("count", Number(4.0)), ("mode", Text("a".into())), ("colour", Text("red".into()))]);
        assert_eq!(store.set_values(&id(), set).unwrap(), with(4.0, true));
        // Only what differs from its default is kept.
        let row = format!("plugin_settings.{ID}");
        assert_eq!(place.db.setting(&row).unwrap().as_deref(), Some(r#"{"count":4.0}"#));

        // A value that doesn't fit any more (an update changed its setting)
        // gives the default.
        let later = r##"{"count":40,"mode":"z","loud":false,"later":{"secret":1},"tint":"#ff0000"}"##;
        place.db.set_setting(&row, later).unwrap();
        assert_eq!(store.values(&id()).unwrap(), with(3.0, false));
        // Back at their defaults, nothing of its settings is kept; the values
        // of settings this AutoScene skipped (a later one's) stay as they are.
        store.set_values(&id(), values(&[("loud", Bool(true))])).unwrap();
        assert_eq!(place.db.setting(&row).unwrap().as_deref(), Some(r##"{"later":{"secret":1},"tint":"#ff0000"}"##));
        store.set_values(&id(), values(&[("count", Number(4.0))])).unwrap();
        assert_eq!(
            place.db.setting(&row).unwrap().as_deref(),
            Some(r##"{"count":4.0,"later":{"secret":1},"tint":"#ff0000"}"##)
        );
        assert_eq!(store.values(&id()).unwrap(), with(4.0, true));
        place.db.set_setting(&row, r#"{"loud":false}"#).unwrap();
        store.set_values(&id(), values(&[("loud", Bool(true))])).unwrap();
        assert_eq!(place.db.setting(&row).unwrap(), None);

        // Values this AutoScene can't read (a later one's) give the defaults,
        // and are never written over.
        place.db.set_setting(&row, "[1]").unwrap();
        assert_eq!(store.values(&id()).unwrap(), with(3.0, true));
        assert!(store.set_values(&id(), values(&[("count", Number(4.0))])).is_err());
        assert_eq!(place.db.setting(&row).unwrap().as_deref(), Some("[1]"));
    }

    #[test]
    fn plugin_rows_never_match_plugin_settings_rows() {
        let place = Place::new();
        let store = place.store();
        install(&store, &place.unsigned("1.0.0")).unwrap();
        store.set_values(&id(), values(&[("count", SettingValue::Number(5.0))])).unwrap();
        // Rows beside the registry's, which a `LIKE 'plugin%'` would find.
        place.db.set_setting("plugin_settings.dev.autoscene.other", "{}").unwrap();
        place.db.set_setting("plugins.dev.autoscene.other", "{}").unwrap();
        // An extra row of a plugin that is installed is no plugin either.
        place.db.set_setting("plugin_extra.dev.autoscene.another", r#"{"v":1}"#).unwrap();
        place.db.set_setting("plugin.dev.autoscene.another", "not json").unwrap();
        assert!(extra_row(&place).is_some());
        let listed = store.list().unwrap();
        let ids = ["dev.autoscene.another", ID];
        assert_eq!(listed.iter().map(|p| p.record.id.as_str()).collect::<Vec<_>>(), ids);
        assert!(listed[1].broken.is_none());

        store.uninstall(&id(), false).unwrap();
        assert_eq!(place.db.setting(&format!("plugin_settings.{ID}")).unwrap(), None);
        assert_eq!(extra_row(&place), None);
        assert_eq!(store.list().unwrap().len(), 1);
        assert_eq!(place.db.setting("plugin_settings.dev.autoscene.other").unwrap().as_deref(), Some("{}"));
        assert!(place.db.setting("plugin_extra.dev.autoscene.another").unwrap().is_some());
    }

    #[test]
    fn unreviewed_needs_the_setting_and_the_consent() {
        let place = Place::new();
        let (review, author) = (key_pair(), key_pair());
        let store = place.store_with(Keys::new(&[&review.pk.to_base64()]).unwrap());
        for (package, trust) in [
            (place.unsigned("1.0.0"), Trust::Unreviewed),
            (place.package(&hello("1.0.0"), Some(&author), true), Trust::Pinned),
        ] {
            let t = store.stage(&package).unwrap();
            assert_eq!(t.verdict.trust, trust);
            assert!(matches!(store.commit(t, false, true), Err(PluginError::NotAllowed)));
            let t = store.stage(&package).unwrap();
            assert_eq!(store.commit(t, true, false).unwrap_err().to_string(), "Confirm that you want to install it.");
            assert!(store.list().unwrap().is_empty());
            assert_eq!(place.plugins(), [".staging", ".trash"], "a refused package leaves nothing");
            assert_eq!(install(&store, &package).unwrap().record.trust, trust);
            store.uninstall(&id(), false).unwrap();
        }
        let dev = place.folder("hello-export", &hello("1.0.0"));
        let t = store.stage_folder(&dev).unwrap();
        assert!(matches!(store.commit(t, false, true), Err(PluginError::NotAllowed)));

        // One AutoScene reviewed needs neither.
        let t = store.stage(&place.package(&hello("1.0.0"), Some(&review), false)).unwrap();
        assert_eq!(t.verdict.trust, Trust::Reviewed);
        assert_eq!(store.commit(t, false, false).unwrap().record.trust, Trust::Reviewed);
    }

    #[test]
    fn a_pinned_plugin_takes_a_reviewed_update_and_keeps_its_pin() {
        let place = Place::new();
        let (review, author) = (key_pair(), key_pair());
        let store = place.store_with(Keys::new(&[&review.pk.to_base64()]).unwrap());
        let p = install(&store, &place.package(&hello("1.0.0"), Some(&author), true)).unwrap();
        let pin = p.record.author_key.clone();
        assert!(pin.is_some());
        let p = install(&store, &place.package(&hello("1.1.0"), Some(&review), false)).unwrap();
        assert_eq!((p.record.trust, &p.record.author_key), (Trust::Reviewed, &pin));
        // Reviewed now, it takes no version its author alone signed.
        let e = store.stage(&place.package(&hello("1.2.0"), Some(&author), true)).unwrap_err();
        assert_eq!(e.to_string(), "AutoScene reviewed the installed version of this plugin, and not this one.");

        // An unreviewed plugin takes a signed update, which pins its key.
        store.uninstall(&id(), false).unwrap();
        install(&store, &place.unsigned("1.0.0")).unwrap();
        let p = install(&store, &place.package(&hello("1.1.0"), Some(&author), true)).unwrap();
        assert_eq!((p.record.trust, &p.record.author_key), (Trust::Pinned, &pin));
    }

    #[test]
    fn a_ticket_never_throws_away_a_folder_outside_staging() {
        let place = Place::new();
        let store = place.store();
        let dev = place.folder("hello-export", &hello("1.0.0"));
        // A loaded folder's ticket, made to name that folder as staged.
        let mut t = store.stage_folder(&dev).unwrap();
        t.staged = Some(dev.clone());
        store.discard(t);
        assert!(dev.join("plugin.json").is_file(), "discard kept the developer's folder");
        // A package's, whose commit fails.
        let mut t = store.stage(&place.unsigned("1.0.0")).unwrap();
        let staged = t.staged.replace(dev.clone()).unwrap();
        assert!(store.commit(t, true, true).is_err());
        assert!(dev.join("plugin.json").is_file(), "a failed commit kept the developer's folder");
        assert!(store.list().unwrap().is_empty());
        let _ = remove_all(&staged);
    }

    #[test]
    fn a_reviewed_plugin_takes_only_reviewed_updates() {
        let place = Place::new();
        let (review, author) = (key_pair(), key_pair());
        let store = place.store_with(Keys::new(&[&review.pk.to_base64()]).unwrap());
        let reviewed = |version: &str| place.package(&hello(version), Some(&review), false);
        store.commit(store.stage(&reviewed("1.0.0")).unwrap(), false, false).unwrap();
        for update in [place.unsigned("1.1.0"), place.package(&hello("1.1.0"), Some(&author), true)] {
            let e = store.stage(&update).unwrap_err();
            assert_eq!(e.to_string(), "AutoScene reviewed the installed version of this plugin, and not this one.");
        }
        let dev = place.folder("hello-export", &hello("1.1.0"));
        assert!(store.stage_folder(&dev).is_err());

        let p = store.commit(store.stage(&reviewed("1.1.0")).unwrap(), false, false).unwrap();
        assert_eq!((p.record.version.as_str(), p.record.trust), ("1.1.0", Trust::Reviewed));
    }

    #[test]
    fn an_older_version_does_not_replace_a_newer_one() {
        let place = Place::new();
        let store = place.store();
        install(&store, &place.unsigned("1.2.0")).unwrap();
        for older in ["1.1.9", "1.2.0-beta.1"] {
            let e = store.stage(&place.unsigned(older)).unwrap_err();
            let why =
                format!("Version 1.2.0 is installed, which is newer than {older}. Uninstall it first to go back.");
            assert_eq!(e.to_string(), why);
        }
        // Checked again as it is installed: a newer version installed meanwhile stays.
        let a = store.stage(&place.unsigned("1.3.0")).unwrap();
        let b = store.stage(&place.unsigned("1.4.0")).unwrap();
        let p = store.commit(b, true, true).unwrap();
        assert!(store.commit(a, true, true).is_err());
        assert_eq!(store.get(&id()).unwrap().record.version, "1.4.0");
        assert_eq!(place.plugins(), holding(&p));
    }

    #[test]
    fn the_same_version_installs_again_only_with_the_same_files() {
        let place = Place::new();
        let store = place.store();
        let package = place.unsigned("1.0.0");
        let first = install(&store, &package).unwrap();
        let again = install(&store, &package).unwrap();
        assert_ne!(first.dir, again.dir);
        assert_eq!(place.plugins(), holding(&again));

        let mut changed = hello("1.0.0");
        changed[1].1 = b"print('bye')\n".to_vec();
        let e = store.stage(&place.package(&changed, None, false)).unwrap_err();
        let why = "Version 1.0.0 is installed already, with other files. A changed plugin needs a new version.";
        assert_eq!(e.to_string(), why);
        // A build of its own is the same version: the message names the installed one.
        assert_eq!(store.stage(&place.unsigned("1.0.0+other")).unwrap_err().to_string(), why);
        assert_eq!(store.get(&id()).unwrap().record, again.record);
    }

    #[test]
    fn a_version_installed_while_a_ticket_waits_keeps_its_files() {
        let place = Place::new();
        let store = place.store();
        let mut changed = hello("1.0.0");
        changed[1].1 = b"print('bye')\n".to_vec();
        // Both staged before either is installed, so nothing to compare with yet.
        let a = store.stage(&place.unsigned("1.0.0")).unwrap();
        let b = store.stage(&place.package(&changed, None, false)).unwrap();
        let p = store.commit(a, true, true).unwrap();
        let e = store.commit(b, true, true).unwrap_err();
        let why = "Version 1.0.0 is installed already, with other files. A changed plugin needs a new version.";
        assert_eq!(e.to_string(), why);
        assert_eq!(store.get(&id()).unwrap().record, p.record);
        assert_eq!(place.plugins(), holding(&p));
    }

    #[test]
    fn a_package_without_what_its_command_needs_or_with_a_wrong_icon_is_refused() {
        let place = Place::new();
        let store = place.store();
        let mut no_lock = hello("1.0.0");
        no_lock.retain(|(path, _)| path != "main.py.lock");
        let mut m = manifest(ID, "1.0.0");
        m["icon"] = json!("icon.png");
        let mut wrong_icon = files(&m);
        wrong_icon.push(("icon.png".into(), b"GIF89a".to_vec()));
        for (files, why) in [
            (no_lock, "The plugin package is missing main.py.lock, which its command needs."),
            (wrong_icon, "The plugin's icon icon.png isn't a PNG image."),
        ] {
            let e = store.stage(&place.package(&files, None, false)).unwrap_err();
            assert_eq!(e.to_string(), why);
            assert_eq!(place.plugins(), [".staging", ".trash"]);
        }
    }

    #[test]
    fn a_plugin_that_runs_no_program_installs_with_its_pages() {
        let place = Place::new();
        let store = place.store();
        let mut m = manifest(ID, "1.0.0");
        m["manifestVersion"] = json!(2);
        m["runtime"] = json!({"type": "none"});
        m["contributes"] = json!({"pages": [{"id": "main", "title": "Main", "entry": "ui/index.html"}]});
        let manifest = ("plugin.json".to_owned(), serde_json::to_vec_pretty(&m).unwrap());
        let page = ("ui/index.html".to_owned(), b"<!doctype html>".to_vec());

        // A loaded folder holds its page as a file of its own.
        let dir = place.tmp.path().join("pages");
        fs::create_dir_all(dir.join("ui")).unwrap();
        fs::write(dir.join("plugin.json"), &manifest.1).unwrap();
        let e = store.stage_folder(&dir).unwrap_err();
        assert_eq!(e.to_string(), "The plugin folder is missing its page ui/index.html, or it is a link.");
        fs::write(dir.join("ui/index.html"), &page.1).unwrap();
        store.discard(store.stage_folder(&dir).unwrap());

        // A package lists it.
        let e = store.stage(&place.package(&vec![manifest.clone()], None, false)).unwrap_err();
        assert_eq!(e.to_string(), "The plugin package is missing its page ui/index.html.");
        assert_eq!(place.plugins(), [".staging", ".trash"]);
        let t = store.stage(&place.package(&vec![manifest, page], None, false)).unwrap();
        let p = store.commit(t, true, true).unwrap();
        assert_eq!((p.extra.runtime, p.broken.as_deref()), (Some(RuntimeKind::None), None));
        assert_eq!(store.get(&id()).unwrap().manifest.unwrap().pages[0].entry, "ui/index.html");
        // It has nothing to run.
        let action = ActionContext { kind: ActionKind::Command, id: "main".into(), trigger: Trigger::Palette };
        let e = store.run_spec(&p, action, Path::new("ffmpeg"), Path::new("ffprobe"), "9.0").err().unwrap();
        assert_eq!(e.to_string(), "Hello Export runs no program.");
    }

    #[test]
    fn an_update_keeps_its_authors_pin() {
        let place = Place::new();
        let store = place.store();
        let (author, other) = (key_pair(), key_pair());
        let pin = AuthorKey { base64: author.pk.to_base64(), key_id: trust::key_id(&author.pk.to_base64()).unwrap() };
        let p = install(&store, &place.package(&hello("1.0.0"), Some(&author), true)).unwrap();
        assert_eq!((p.record.trust, p.record.author_key.as_ref()), (Trust::Pinned, Some(&pin)));
        // Signed by the pinned key, without author.pub: the pin checks it, and stays.
        let p = install(&store, &place.package(&hello("1.1.0"), Some(&author), false)).unwrap();
        assert_eq!((p.record.trust, p.record.author_key.as_ref()), (Trust::Pinned, Some(&pin)));
        for update in [place.package(&hello("1.2.0"), Some(&other), true), place.unsigned("1.2.0")] {
            assert!(matches!(store.stage(&update), Err(PluginError::KeyChanged { .. })));
        }
        let dev = place.folder("hello-export", &hello("1.2.0"));
        let e = store.stage_folder(&dev).unwrap_err();
        assert_eq!(e.to_string(), "The installed version is signed by its author. Uninstall it to load a folder.");
        assert_eq!(store.get(&id()).unwrap().record.author_key, Some(pin));
    }

    #[test]
    fn a_pin_set_while_a_ticket_waits_refuses_it() {
        let place = Place::new();
        let store = place.store();
        let (author, other) = (key_pair(), key_pair());
        // Staged while nothing is installed: pinned to the key it brings.
        let t = store.stage(&place.package(&hello("1.1.0"), Some(&other), true)).unwrap();
        assert_eq!(t.verdict.trust, Trust::Pinned);
        // Meanwhile the author's version is installed, and pins the author's key.
        let p = install(&store, &place.package(&hello("1.0.0"), Some(&author), true)).unwrap();
        assert!(matches!(store.commit(t, true, true), Err(PluginError::KeyChanged { .. })));
        assert_eq!(store.get(&id()).unwrap().record, p.record);
        assert_eq!(place.plugins(), holding(&p));
    }

    #[test]
    fn uninstalling_forgets_the_authors_key() {
        let place = Place::new();
        let store = place.store();
        let (author, other) = (key_pair(), key_pair());
        install(&store, &place.package(&hello("1.0.0"), Some(&author), true)).unwrap();
        store.uninstall(&id(), false).unwrap();
        // Installed again, it is a first install: another author's key pins it.
        let p = install(&store, &place.package(&hello("1.0.0"), Some(&other), true)).unwrap();
        assert_eq!(p.record.author_key.map(|k| k.base64), Some(other.pk.to_base64()));
    }

    #[test]
    fn a_package_changed_while_it_waits_is_not_installed() {
        let place = Place::new();
        let store = place.store();
        let package = place.unsigned("1.0.0");
        let t = store.stage(&package).unwrap();
        fs::write(t.staged.as_ref().unwrap().join("main.py"), b"import os\n").unwrap();
        let e = store.commit(t, true, true).unwrap_err();
        assert_eq!(e.to_string(), "The plugin's main.py doesn't match its files.lock.");
        let t = store.stage(&package).unwrap();
        fs::write(t.staged.as_ref().unwrap().join(LOCK), br#"{"lockVersion":1,"files":[]}"#).unwrap();
        let e = store.commit(t, true, true).unwrap_err();
        assert_eq!(e.to_string(), "The plugin's files changed while it waited to be installed. Open it again.");
        assert!(store.list().unwrap().is_empty());
        assert_eq!(place.plugins(), [".staging", ".trash"]);
    }

    #[test]
    fn an_icon_is_shown_while_it_is_the_listed_picture() {
        let place = Place::new();
        let store = place.store();
        let mut m = manifest(ID, "1.0.0");
        m["icon"] = json!("icon.png");
        let mut with_icon = files(&m);
        with_icon.push(("icon.png".into(), b"\x89PNG\r\n\x1a\nrest".to_vec()));
        let p = install(&store, &place.package(&with_icon, None, false)).unwrap();
        assert_eq!(store.icon_data_url(&p).as_deref(), Some("data:image/png;base64,iVBORw0KGgpyZXN0"));
        fs::write(p.dir.join("icon.png"), b"\x89PNG\r\n\x1a\nRest").unwrap();
        assert_eq!(store.icon_data_url(&p), None);

        // A loaded folder's, as it is.
        store.uninstall(&id(), false).unwrap();
        m["icon"] = json!("icon.svg");
        let mut with_icon = files(&m);
        with_icon.push(("icon.svg".into(), b"<svg/>".to_vec()));
        let dev = place.folder("hello-export", &with_icon);
        let p = store.commit(store.stage_folder(&dev).unwrap(), true, true).unwrap();
        assert_eq!(store.icon_data_url(&p).as_deref(), Some("data:image/svg+xml;base64,PHN2Zy8+"));
        fs::write(dev.join("icon.svg"), b"not a picture").unwrap();
        assert_eq!(store.icon_data_url(&p), None);
    }

    #[test]
    fn secret_values_never_reach_the_settings_row() {
        let place = Place::new();
        let store = place.store();
        let p = install(&store, &place.package(&with_secrets("1.0.0", &["token"]), None, false)).unwrap();
        assert_eq!((p.secrets_stored(), p.extra.secrets.len()), (Vec::<String>::new(), 0));
        assert_eq!(store.secret(&id(), "token").unwrap(), None);
        let p = store.secret_set(&id(), "token", "hunter2").unwrap();
        assert_eq!(p.secrets_stored(), ["token"]);
        assert_eq!(store.secret(&id(), "token").unwrap().as_deref(), Some("hunter2"));
        // Its row names it, and no row holds its value.
        assert_eq!(extra_row(&place), extra_of(&p, &["token"]));
        assert!(!place.rows().contains("hunter2"), "{}", place.rows());

        // Never set as a value, and never handed on as one: also not one kept
        // under its key before it was a secret.
        let e = store.set_values(&id(), values(&[("token", SettingValue::Text("hunter2".into()))])).unwrap_err();
        assert_eq!(e.to_string(), "A secret is set with its own button, never as a value.");
        place.db.set_setting(&format!("plugin_settings.{ID}"), r#"{"token":"plain","count":4}"#).unwrap();
        assert!(!store.values(&id()).unwrap().contains_key("token"));
        let action = ActionContext { kind: ActionKind::ExportAction, id: "hello".into(), trigger: Trigger::Menu };
        let spec = store.run_spec(&p, action, Path::new("ffmpeg"), Path::new("ffprobe"), "9.0").unwrap();
        let line = serde_json::to_string(&spec.message).unwrap();
        assert!(line.contains(r#""count":4.0"#) && !line.contains("token") && !line.contains("hunter2"), "{line}");
        // Set again, it is replaced, and the plain value goes from the row.
        store.secret_set(&id(), "token", "swordfish").unwrap();
        assert_eq!(store.secret(&id(), "token").unwrap().as_deref(), Some("swordfish"));
        assert_eq!(extra_row(&place), extra_of(&p, &["token"]));
        assert_eq!(settings_row(&place).as_deref(), Some(r#"{"count":4}"#));
        assert!(!place.rows().contains("plain"), "{}", place.rows());
        // A plain setting isn't a secret.
        for (key, value, why) in [
            ("greeting", "x", "The plugin has no secret setting greeting."),
            ("token", "", "A secret can't be empty. Remove it instead."),
            ("token", "a\0b", "A secret can't hold a NUL character."),
        ] {
            assert_eq!(store.secret_set(&id(), key, value).unwrap_err().to_string(), why);
        }
        let e = store.secret(&id(), "greeting").unwrap_err();
        assert!(matches!(&e, PluginError::Denied(_)), "{e:?}");
        assert_eq!(place.secrets.len(), 1);
    }

    #[test]
    fn uninstalling_forgets_its_secrets() {
        let place = Place::new();
        let store = place.store();
        install(&store, &place.package(&with_secrets("1.0.0", &["token", "webhook"]), None, false)).unwrap();
        store.secret_set(&id(), "token", "hunter2").unwrap();
        store.secret_set(&id(), "webhook", "https://example.org/hook").unwrap();
        // One removed by its button.
        let p = store.secret_clear(&id(), "webhook").unwrap();
        assert_eq!((p.secrets_stored(), place.secrets.len()), (vec!["token".to_owned()], 1));
        assert!(store.secret_clear(&id(), "greeting").is_err());
        // An update keeps them.
        let p = install(&store, &place.package(&with_secrets("1.1.0", &["token", "webhook"]), None, false)).unwrap();
        assert_eq!((p.record.version.as_str(), p.secrets_stored()), ("1.1.0", vec!["token".to_owned()]));

        store.uninstall(&id(), false).unwrap();
        assert!(place.secrets.is_empty());
        assert_eq!(extra_row(&place), None);
        // Installed again, it starts without them.
        let p = install(&store, &place.package(&with_secrets("1.1.0", &["token"]), None, false)).unwrap();
        assert_eq!(p.secrets_stored(), Vec::<String>::new());
        assert_eq!(store.secret(&id(), "token").unwrap(), None);
    }

    #[test]
    fn a_secret_of_a_setting_the_plugin_dropped_is_not_listed() {
        let place = Place::new();
        let store = place.store();
        let dev = place.folder("hello-export", &with_secrets("1.0.0", &["token", "webhook"]));
        store.commit(store.stage_folder(&dev).unwrap(), true, true).unwrap();
        store.secret_set(&id(), "token", "hunter2").unwrap();
        store.secret_set(&id(), "webhook", "https://example.org/hook").unwrap();
        // Its plugin.json no longer has the webhook.
        fs::write(dev.join("plugin.json"), &with_secrets("1.0.0", &["token"])[0].1).unwrap();
        let p = store.get(&id()).unwrap();
        assert_eq!(p.secrets_stored(), ["token"]);
        assert_eq!(p.extra.secrets, ["token", "webhook"], "listed, so an uninstall removes it");
        assert!(matches!(store.secret(&id(), "webhook"), Err(PluginError::Denied(_))));
        // It may still be removed.
        store.secret_clear(&id(), "webhook").unwrap();
        assert_eq!(place.secrets.len(), 1);
        // A version 1 plugin.json has no secrets at all.
        fs::write(dev.join("plugin.json"), &hello("1.0.0")[0].1).unwrap();
        assert_eq!(store.get(&id()).unwrap().secrets_stored(), Vec::<String>::new());
        assert!(store.secret(&id(), "token").is_err());
    }

    /// A keychain that refuses everything.
    struct Locked;

    impl SecretStore for Locked {
        fn get(&self, _: &str, _: &PluginId, _: &str) -> PluginResult<Option<String>> {
            Err(locked())
        }

        fn set(&self, _: &str, _: &PluginId, _: &str, _: &str) -> PluginResult<()> {
            Err(locked())
        }

        fn delete(&self, _: &str, _: &PluginId, _: &str) -> PluginResult<()> {
            Err(locked())
        }
    }

    fn locked() -> PluginError {
        PluginError::Keychain { reason: "it is locked.".into(), detail: None }
    }

    #[test]
    fn a_keychain_that_fails_says_so() {
        let place = Place::new();
        install(&place.store(), &place.package(&with_secrets("1.0.0", &["token"]), None, false)).unwrap();
        place.store().secret_set(&id(), "token", "hunter2").unwrap();
        let locked = Store::open(&place.paths, Arc::clone(&place.db), Keys::new(&[]).unwrap(), app(), Arc::new(Locked));
        let store = locked.unwrap();
        let said = "Your system's keychain can't be used: it is locked.";
        assert_eq!(store.secret(&id(), "token").unwrap_err().to_string(), said);
        assert_eq!(store.secret_set(&id(), "token", "swordfish").unwrap_err().to_string(), said);
        assert_eq!(store.secret_clear(&id(), "token").unwrap_err().to_string(), said);
        // Nothing changed: still listed, and kept.
        assert_eq!(store.get(&id()).unwrap().secrets_stored(), ["token"]);
        assert_eq!(place.store().secret(&id(), "token").unwrap().as_deref(), Some("hunter2"));
        // A plugin without secrets never asks it; an uninstall goes on without it.
        assert!(store.values(&id()).is_ok());
        store.uninstall(&id(), false).unwrap();
        assert!(store.list().unwrap().is_empty());
        assert_eq!(extra_row(&place), None);
    }

    #[test]
    fn a_secret_the_keychain_kept_is_not_the_next_installs() {
        let place = Place::new();
        let token = with_secrets("1.0.0", &["token"]);
        install(&place.store(), &place.package(&token, None, false)).unwrap();
        place.store().secret_set(&id(), "token", "owner-token").unwrap();
        // The keychain refuses to remove it: the plugin goes, its secret stays.
        let locked = Store::open(&place.paths, Arc::clone(&place.db), Keys::new(&[]).unwrap(), app(), Arc::new(Locked));
        locked.unwrap().uninstall(&id(), false).unwrap();
        assert_eq!(place.secrets.len(), 1);
        // A plugin of that id installed next (by anyone: the pin went) never reads it.
        let store = place.store();
        let p = install(&store, &place.package(&token, None, false)).unwrap();
        assert_eq!((p.secrets_stored(), store.secret(&id(), "token").unwrap()), (Vec::<String>::new(), None));
        // Its uninstall removes each of its secret settings, listed or not.
        store.uninstall(&id(), false).unwrap();
        assert!(place.secrets.is_empty());
    }

    #[test]
    fn a_setting_that_becomes_a_secret_leaves_no_plain_value() {
        let place = Place::new();
        let store = place.store();
        // Version 1.0.0 keeps the webhook as text.
        let mut m = manifest(ID, "1.0.0");
        let webhook = json!({"key": "webhook", "type": "string", "title": "Webhook"});
        m["contributes"]["settings"].as_array_mut().unwrap().push(webhook);
        install(&store, &place.package(&files(&m), None, false)).unwrap();
        let hook = SettingValue::Text("https://example.org/hooks/SECRET".into());
        store.set_values(&id(), values(&[("webhook", hook), ("count", SettingValue::Number(4.0))])).unwrap();
        // 1.1.0 keeps it in the keychain: its update drops the plain value and keeps the others.
        install(&store, &place.package(&with_secrets("1.1.0", &["webhook"]), None, false)).unwrap();
        assert_eq!(settings_row(&place).as_deref(), Some(r#"{"count":4.0}"#));
        assert!(!place.rows().contains("SECRET"), "{}", place.rows());
        store.uninstall(&id(), false).unwrap();

        // A loaded folder's plugin.json makes it a secret without an install:
        // the value is never handed on, and goes once the secret is set.
        let dev = place.folder("hello-export", &files(&m));
        store.commit(store.stage_folder(&dev).unwrap(), true, true).unwrap();
        store
            .set_values(&id(), values(&[("webhook", SettingValue::Text("https://example.org/hooks/SECRET".into()))]))
            .unwrap();
        fs::write(dev.join("plugin.json"), &with_secrets("1.0.0", &["webhook"])[0].1).unwrap();
        assert!(!store.values(&id()).unwrap().contains_key("webhook"));
        store.secret_set(&id(), "webhook", "https://example.org/hooks/NEW").unwrap();
        assert_eq!(settings_row(&place), None);
        assert!(!place.rows().contains("SECRET"), "{}", place.rows());
    }

    #[test]
    fn a_folder_whose_runtime_changed_is_broken_until_loaded_again() {
        let place = Place::new();
        let store = place.store();
        let dev = place.folder("hello-export", &with_secrets("1.0.0", &["token"]));
        let p = store.commit(store.stage_folder(&dev).unwrap(), true, true).unwrap();
        assert_eq!((p.extra.runtime, p.broken.as_deref()), (Some(RuntimeKind::Process), None));
        store.secret_set(&id(), "token", "hunter2").unwrap();
        // Agreed to as a service, it runs as a process now (or the other way).
        let row = format!("plugin_extra.{ID}");
        let service =
            format!(r#"{{"v":1,"installedAt":{},"runtime":"service","secrets":["token"]}}"#, p.record.installed_at);
        place.db.set_setting(&row, &service).unwrap();
        let p = store.get(&id()).unwrap();
        assert_eq!(p.broken.as_deref(), Some("Its runtime changed. Load it again to agree to that."));
        assert!(p.manifest.is_none());
        assert!(crate::calls::Grants::of(&p).is_err());
        assert!(store.secret(&id(), "token").is_err());
        // Loaded again, it is what the person agreed to now, and keeps its secrets.
        let p = store.commit(store.stage_folder(&dev).unwrap(), true, true).unwrap();
        assert_eq!((p.extra.runtime, p.broken.as_deref()), (Some(RuntimeKind::Process), None));
        assert_eq!(store.secret(&id(), "token").unwrap().as_deref(), Some("hunter2"));
    }

    #[test]
    fn an_m1_install_counts_as_a_process_plugin() {
        let place = Place::new();
        let store = place.store();
        let p = install(&store, &place.unsigned("1.0.0")).unwrap();
        assert_eq!(extra_row(&place), extra_of(&p, &[]));
        // AutoScene 1.0 writes no extra row.
        place.db.write_settings(&[(&format!("plugin_extra.{ID}"), None)]).unwrap();
        let p = place.store().get(&id()).unwrap();
        assert_eq!((p.broken.as_deref(), p.extra.runtime, p.extra.secrets.len()), (None, None, 0));
        assert_eq!(p.extra.agreed_runtime(), RuntimeKind::Process);
        assert_eq!(crate::calls::Grants::of(&p).unwrap().runtime, RuntimeKind::Process);
        // Turning it on or off writes no extra row either; an update does.
        store.set_enabled(&id(), false).unwrap();
        assert_eq!(extra_row(&place), None);
        install(&store, &place.unsigned("1.1.0")).unwrap();
        assert!(extra_row(&place).is_some());

        // One a later AutoScene wrote is kept, and stops this one changing the plugin.
        for later in [r#"{"v":2,"runtime":"process"}"#, r#"{"v":1,"runtime":"hologram"}"#, "not json"] {
            place.db.set_setting(&format!("plugin_extra.{ID}"), later).unwrap();
            let p = store.get(&id()).unwrap();
            assert_eq!(p.broken.as_deref(), Some(NEWER), "{later}");
            assert!(store.secret_set(&id(), "token", "x").is_err());
            assert!(store.stage(&place.unsigned("1.2.0")).is_ok_and(|t| store.commit(t, true, true).is_err()));
            assert_eq!(extra_row(&place).as_deref(), Some(later));
        }
    }

    #[test]
    fn an_extra_row_without_its_plugin_is_swept_with_its_secrets() {
        let place = Place::new();
        let store = place.store();
        install(&store, &place.package(&with_secrets("1.0.0", &["token", "webhook"]), None, false)).unwrap();
        store.secret_set(&id(), "token", "hunter2").unwrap();
        store.secret_set(&id(), "webhook", "https://example.org/hook").unwrap();
        // AutoScene 1.0 uninstalls it: its two rows go, the extra row stays.
        place.db.write_settings(&[(&format!("plugin.{ID}"), None), (&format!("plugin_settings.{ID}"), None)]).unwrap();
        // A later AutoScene's row of a plugin gone the same way.
        let later = PluginId::parse("dev.autoscene.later").unwrap();
        place.secrets.set(&place.library(), &later, "key", "value").unwrap();
        place.db.set_setting("plugin_extra.dev.autoscene.later", r#"{"v":2,"secrets":["key"],"more":1}"#).unwrap();
        // One of an installed plugin stays.
        let kept = PluginId::parse("dev.autoscene.kept").unwrap();
        place.secrets.set(&place.library(), &kept, "key", "value").unwrap();
        place.db.set_setting("plugin_extra.dev.autoscene.kept", r#"{"v":1,"secrets":["key"]}"#).unwrap();
        place.db.set_setting("plugin.dev.autoscene.kept", r#"{"v":2}"#).unwrap();
        assert_eq!(place.secrets.len(), 4);

        // The open leaves the keychain alone; what it left goes after it.
        let store = place.store();
        assert_eq!(place.secrets.len(), 4);
        store.remove_leftovers();
        assert_eq!(place.secrets.len(), 1);
        assert_eq!(place.secrets.get(&place.library(), &kept, "key").unwrap().as_deref(), Some("value"));
        let extras: Vec<String> = place.db.settings_in("plugin_extra.").unwrap().into_iter().map(|(k, _)| k).collect();
        assert_eq!(extras, ["plugin_extra.dev.autoscene.kept"]);
        // Installed again, it is a first install, with no secrets.
        let p = install(&store, &place.package(&with_secrets("1.0.0", &["token"]), None, false)).unwrap();
        assert_eq!(p.secrets_stored(), Vec::<String>::new());
    }

    #[test]
    fn an_extra_row_from_an_install_before_is_swept_with_its_secrets() {
        let place = Place::new();
        let store = place.store();
        install(&store, &place.package(&with_secrets("1.0.0", &["token"]), None, false)).unwrap();
        store.secret_set(&id(), "token", "owner-token").unwrap();
        // AutoScene 1.0 uninstalls it and installs a plugin of that id again:
        // it writes only the record, with the time of its install.
        let row = format!("plugin.{ID}");
        let mut record: Value = serde_json::from_str(&place.db.setting(&row).unwrap().unwrap()).unwrap();
        record["installedAt"] = json!(record["installedAt"].as_i64().unwrap() + 1);
        place
            .db
            .write_settings(&[(&row, Some(&record.to_string())), (&format!("plugin_settings.{ID}"), None)])
            .unwrap();

        // The extra row isn't that plugin's: it inherits neither its secrets nor its runtime.
        let store = place.store();
        let p = store.get(&id()).unwrap();
        assert_eq!((p.broken.as_deref(), p.extra.runtime, p.secrets_stored()), (None, None, Vec::<String>::new()));
        assert_eq!(store.secret(&id(), "token").unwrap(), None);
        assert_eq!(place.secrets.len(), 1);
        // It goes after the open, with its secrets.
        store.remove_leftovers();
        assert!(place.secrets.is_empty());
        assert_eq!(extra_row(&place), None);
        // An update gives it a row of its own.
        let p = install(&store, &place.package(&with_secrets("1.1.0", &["token"]), None, false)).unwrap();
        assert_eq!(extra_row(&place), extra_of(&p, &[]));
        store.secret_set(&id(), "token", "new-token").unwrap();
        place.store().remove_leftovers();
        assert_eq!(store.secret(&id(), "token").unwrap().as_deref(), Some("new-token"));
    }

    #[test]
    fn a_plugin_installed_after_the_open_keeps_its_extra_row_and_secrets() {
        let place = Place::new();
        let token = with_secrets("1.0.0", &["token"]);
        let store = place.store();
        install(&store, &place.package(&token, None, false)).unwrap();
        store.secret_set(&id(), "token", "owner-token").unwrap();
        // AutoScene 1.0 uninstalls it: the open finds its extra row left from before.
        place.db.write_settings(&[(&format!("plugin.{ID}"), None), (&format!("plugin_settings.{ID}"), None)]).unwrap();
        let store = place.store();
        // Installed again, and its secret set, before the sweep gets to it.
        let p = install(&store, &place.package(&token, None, false)).unwrap();
        store.secret_set(&id(), "token", "new-token").unwrap();
        store.remove_leftovers();
        assert_eq!(extra_row(&place), extra_of(&p, &["token"]));
        assert_eq!(store.secret(&id(), "token").unwrap().as_deref(), Some("new-token"));
    }

    #[test]
    fn two_libraries_never_share_a_secret() {
        // Two data folders on one computer: the person's and a test build's.
        let ours = Place::new();
        let theirs = Place { secrets: Arc::clone(&ours.secrets), ..Place::new() };
        let token = with_secrets("1.0.0", &["token"]);
        let (store, other) = (ours.store(), theirs.store());
        install(&store, &ours.package(&token, None, false)).unwrap();
        install(&other, &theirs.package(&token, None, false)).unwrap();
        // A library gets its keychain id with its first secret.
        assert_eq!(ours.db.setting(KEYCHAIN_ROW).unwrap(), None);
        store.secret_set(&id(), "token", "ours").unwrap();
        assert_eq!(other.secret(&id(), "token").unwrap(), None);
        other.secret_set(&id(), "token", "theirs").unwrap();
        assert_ne!(ours.library(), theirs.library());
        assert_eq!(store.secret(&id(), "token").unwrap().as_deref(), Some("ours"));
        assert_eq!(other.secret(&id(), "token").unwrap().as_deref(), Some("theirs"));
        // Their uninstall removes theirs alone, and so does their sweep.
        other.uninstall(&id(), false).unwrap();
        theirs.db.set_setting(&format!("plugin_extra.{ID}"), r#"{"v":1,"secrets":["token"]}"#).unwrap();
        theirs.store().remove_leftovers();
        assert_eq!(store.secret(&id(), "token").unwrap().as_deref(), Some("ours"));
        assert_eq!(ours.secrets.len(), 1);
        // A library that never kept a secret writes no id, and asks no keychain.
        let none = Place::new();
        let locked = Store::open(&none.paths, Arc::clone(&none.db), Keys::new(&[]).unwrap(), app(), Arc::new(Locked));
        let locked = locked.unwrap();
        install(&locked, &none.package(&token, None, false)).unwrap();
        assert_eq!(locked.secret(&id(), "token").unwrap(), None);
        locked.uninstall(&id(), false).unwrap();
        assert_eq!(none.db.setting(KEYCHAIN_ROW).unwrap(), None);
    }
}

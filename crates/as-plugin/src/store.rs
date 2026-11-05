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
//! A WebAssembly plugin's component is only read while it is checked: what
//! it imports, for its review ([`Store::stage`]). It is compiled once the
//! setting, the catalog and the person let it run ([`Store::compile`]), in a
//! process AutoScene can end ([`Store::compile_with`]), and its code kept in
//! its own folder under `cache/plugin-code/<id>` ([`Store::cache_dir`]),
//! outside every folder a plugin sees: once the row names a version, only
//! that version's code stays, an uninstall takes it, an install that fails
//! leaves none, and the next open sweeps what nothing installed uses. A
//! loaded folder that no longer runs in the sandbox, or that starts to,
//! waits to be loaded again, as any whose runtime changed.
//!
//! What AutoScene reviewed is what its catalog lists ([`Store::set_catalog`],
//! [`verdict`]): a package whose lock the living index lists, for its plugin
//! and version, installs as Reviewed. A version the catalog withdrew doesn't
//! install, and one installed is listed as withdrawn
//! ([`Installed::withdrawn`]), whatever its source, so it doesn't run. A
//! withdrawal, and where the catalog says a plugin was built from, never go
//! into its rows: they come from the catalog in hand, so a withdrawal holds
//! for the rows AutoScene 1.0 wrote too.
//!
//! An update never takes away the trust its plugin had: a plugin AutoScene
//! reviewed takes only reviewed versions, and one pinned to its author's key
//! only versions that key signed or AutoScene reviewed, which keep the pin
//! or take the one the catalog names ([`verdict`]). An unreviewed one takes
//! any, and one its author signed pins the key. None goes back to an older
//! version or changes its files under the same one. A row this
//! AutoScene can't read (a later AutoScene's) is listed as such and never
//! written over, and so is a plugin that doesn't work with this AutoScene
//! (one put back to an older version).
//!
//! Reviewed or not, an update may do no more than the person agreed to
//! without their yes ([`Widens`]): for one AutoScene reviewed that yes is
//! [`Ticket::confirmed`], which nothing the page says can set. It covers
//! updates: what was agreed to goes with the plugin's row, so uninstalling
//! and installing again is a first install. A review of an update never
//! lands as one: it is refused, and has to be opened again.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use as_core::{AppPaths, CancelToken};
use as_db::LibraryDb;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use parking_lot::{Mutex, RwLock};
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::calls::HostCalls;
use crate::catalog::{self, Catalog};
use crate::error::{PluginError, PluginResult};
use crate::manifest::{
    self, Launch, Manifest, Parsed, Permission, PluginId, RuntimeChange, RuntimeKind, SettingKind, SettingSpec,
    SettingValue, current_platform, invalid, shown,
};
use crate::package::{
    self, FilesLock, LOCK, LOCK_VERSION, LockedFile, MANIFEST, MAX_LOCK_BYTES, Picture, PictureFormat, Unpacked,
    file_in,
};
use crate::protocol::{ActionContext, AppContext, PluginContext, RunMessage, ServeMessage, ToolPaths};
use crate::runtime::RunSpec;
use crate::secrets::{self, SecretStore};
use crate::service::{READY_WAIT, ServiceSpec};
use crate::trust::{self, AuthorKey, Trust, Verdict};
#[cfg(feature = "wasm")]
use crate::wasm::{self, Limits, WasmSpec};

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
/// Why a plugin whose files don't match its lock can't run.
pub(crate) const CHANGED: &str = "Its files were changed. Install it again.";
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
    /// The author key its updates must be signed with (see [`verdict`]).
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
    /// The ids of its events the person turned on, in its `plugin.json`'s
    /// order. Left out of the row while there are none, so a row stays as
    /// AutoScene 1.1 wrote it; 1.1 forgets them when it writes the row.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<String>,
}

impl Default for Extra {
    fn default() -> Self {
        Self { v: EXTRA_VERSION, installed_at: 0, runtime: None, secrets: Vec::new(), events: Vec::new() }
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
    /// Why AutoScene withdrew its version, in the catalog's words: it
    /// doesn't run. Also of a broken plugin, by its record's version.
    pub withdrawn: Option<String>,
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
    /// What its plugin.json says that this AutoScene skipped.
    pub warnings: Vec<String>,
    /// What reading a WebAssembly plugin's component found that may surprise
    /// its author: an import it can't use, a permission it can't use.
    /// Nothing was skipped: the component runs as it is.
    pub component_warnings: Vec<String>,
    /// The trust it gets, and the author key it is pinned to after.
    pub verdict: Verdict,
    /// What the catalog says of it, when it reviewed it: checked again at
    /// [`Store::commit`].
    pub approval: Option<Approval>,
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
    /// A WebAssembly plugin's code in its folder of [`Store::cache_dir`], the
    /// one code [`Store::commit`] keeps: none until [`Store::compile`] made it.
    pub compiled: Option<PathBuf>,
    /// The person said yes to it in a dialog of the system's own. Only the
    /// app sets it, once they did: no command takes it from the page.
    pub confirmed: bool,
    pub created: Instant,
}

impl Ticket {
    /// Whether it has a component that [`Store::compile`] hasn't compiled.
    pub fn compiles(&self) -> bool {
        self.manifest.is_sandboxed() && self.compiled.is_none()
    }
}

/// What an update may do that the person never agreed to, by the installed
/// version's record and the runtime agreed to. New pages, new hosts for its
/// large files and higher limits in the sandbox aren't in it: its review
/// marks them, but they let it reach no more than it could (a page gets only
/// what its plugin was granted, AutoScene is what fetches those files, by
/// their hashes, and a limit is how much of the same it may use).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Widens {
    /// Permissions it says it needs that the installed version didn't.
    pub permissions: Vec<Permission>,
    pub hosts: Vec<String>,
    pub runtime: RuntimeChange,
}

impl Widens {
    /// What `m` adds to the installed `previous`, agreed to run as `agreed`
    /// (none when its row is gone: nothing is said of how it runs).
    pub fn of(previous: &Record, agreed: Option<RuntimeKind>, m: &Manifest) -> Self {
        Self {
            permissions: m.permissions.iter().filter(|p| !previous.permissions.contains(p)).copied().collect(),
            hosts: m.network_hosts.iter().filter(|h| !previous.network_hosts.contains(h)).cloned().collect(),
            runtime: agreed.map(|a| a.change_to(m.runtime.kind)).unwrap_or_default(),
        }
    }

    /// Whether it may do anything more.
    pub fn any(&self) -> bool {
        *self != Self::default()
    }
}

/// Where AutoScene's catalog says a package it reviewed was built from,
/// for the review dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Approval {
    /// When the index that lists it was published, in Unix seconds.
    pub published: u64,
    /// `https://github.com/<owner>/<repo>`.
    pub repo: String,
    /// The commit, 40 lowercase hex digits.
    pub commit: String,
    /// Its folder in the repository; empty or `.` for the top.
    pub path: String,
}

/// Where a store compiles WebAssembly plugins.
#[cfg(feature = "wasm")]
enum Compiling {
    /// Nowhere, until it is told ([`Store::compile_with`]): an app that
    /// forgot refuses to install them.
    Nowhere,
    /// In this process ([`Store::compile_here`]).
    Here,
    /// In a process of its own.
    Apart(wasm::Compiler),
}

/// Which compiled code of an installed plugin is in use
/// ([`Store::sweep_code`]).
enum CodeInUse {
    /// None: it runs no component, or is broken.
    None,
    /// A package's: the code of the component of this SHA-256, as its lock
    /// lists it.
    Component(String),
    /// A loaded folder's: its newest code.
    Newest,
}

impl CodeInUse {
    /// Whether the file `name` in its code folder may be its code.
    fn may_be(&self, name: &str) -> bool {
        match self {
            Self::None => false,
            Self::Component(sha256) => name.starts_with(&format!("{sha256}-")) && name.ends_with(".cwasm"),
            Self::Newest => name.ends_with(".cwasm"),
        }
    }
}

/// How the name of code this AutoScene's engine compiled ends
/// (`-<engine>.cwasm`); none without the sandbox, or when its engine can't
/// be made.
fn own_code_end() -> Option<String> {
    #[cfg(feature = "wasm")]
    return wasm::own_code_end();
    #[cfg(not(feature = "wasm"))]
    None
}

/// The plugins installed in one app data folder.
pub struct Store {
    root: PathBuf,
    data: PathBuf,
    logs: PathBuf,
    /// `cache/plugin-code`: WebAssembly plugins' compiled code.
    code: PathBuf,
    /// Where the large files plugins name are kept ([`crate::resources`]).
    resources: PathBuf,
    db: Arc<LibraryDb>,
    /// What AutoScene's catalog reviewed and withdrew ([`Store::set_catalog`]).
    catalog: RwLock<Arc<Catalog>>,
    /// The version of this AutoScene, which each plugin must work with.
    app: Version,
    /// Where the values of secret settings are kept.
    secrets: Arc<dyn SecretStore>,
    /// The id they are kept under there ([`KEYCHAIN_ROW`]), once read.
    library: OnceLock<String>,
    /// Held while a plugin's rows change, so each change starts from the
    /// rows the one before wrote.
    changing: Mutex<()>,
    /// What compiles a WebAssembly plugin.
    #[cfg(feature = "wasm")]
    compiling: Compiling,
    /// What the open moved into `.trash`, for [`Store::remove_leftovers`].
    leftovers: Mutex<Vec<PathBuf>>,
    /// The plugins whose extra row the open found left from before
    /// ([`orphaned`]), for [`Store::remove_leftovers`].
    orphans: Mutex<Vec<PluginId>>,
}

impl Store {
    /// Opens the plugins of `paths` (registered in `db`, with their secrets
    /// in `secrets`) for the AutoScene of version `app`, with no catalog yet,
    /// and moves into `.trash` what an install or uninstall that never
    /// finished left: staged folders, versions a row no longer names,
    /// folders of plugins without a row, and compiled code nothing installed
    /// uses. A plugin whose row a later AutoScene wrote keeps its folder as
    /// it is. Moving is quick; removing
    /// them, and what `.trash` held, is [`Store::remove_leftovers`]'s, which
    /// a package's 4096 files can keep busy for a second. So is an extra row
    /// left from before (AutoScene 1.0 uninstalled its plugin, or installed
    /// it again), after its secrets, as the keychain may ask the person or
    /// take its time; it counts as none meanwhile.
    pub fn open(
        paths: &AppPaths,
        db: Arc<LibraryDb>,
        app: Version,
        secrets: Arc<dyn SecretStore>,
    ) -> PluginResult<Self> {
        let store = Self {
            root: paths.plugins(),
            data: paths.plugin_data(),
            logs: paths.plugin_logs(),
            code: paths.plugin_code(),
            resources: paths.plugin_resources(),
            db,
            catalog: RwLock::new(Arc::new(Catalog::none())),
            app,
            secrets,
            library: OnceLock::new(),
            changing: Mutex::default(),
            #[cfg(feature = "wasm")]
            compiling: Compiling::Nowhere,
            leftovers: Mutex::default(),
            orphans: Mutex::default(),
        };
        *store.orphans.lock() = store.orphaned_extras()?;
        let (staging, trash) = (store.root.join(STAGING), store.root.join(TRASH));
        for dir in [&staging, &trash, &store.code] {
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
        leftovers.extend(store.sweep_code());
        *store.leftovers.lock() = leftovers;
        Ok(store)
    }

    /// Compiles WebAssembly plugins with `compiler`, in a process of its own
    /// that ends at its limits. A store told nothing compiles none: it fails
    /// the install, and never compiles where nothing can stop it.
    #[cfg(feature = "wasm")]
    pub fn compile_with(mut self, compiler: wasm::Compiler) -> Self {
        self.compiling = Compiling::Apart(compiler);
        self
    }

    /// Compiles WebAssembly plugins in this process, where nothing can stop
    /// them: for tests, never for the app.
    #[cfg(feature = "wasm")]
    pub fn compile_here(mut self) -> Self {
        self.compiling = Compiling::Here;
        self
    }

    /// Judges plugins by `catalog` from now on: what it reviewed, and what it
    /// withdrew. A ticket staged before is judged by it at its commit.
    pub fn set_catalog(&self, catalog: Arc<Catalog>) {
        *self.catalog.write() = catalog;
    }

    /// The catalog plugins are judged by: none until [`Store::set_catalog`].
    pub fn catalog(&self) -> Arc<Catalog> {
        Arc::clone(&self.catalog.read())
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

    /// Moves into `.trash` the compiled code nothing installed uses, and
    /// gives where it went: in `cache/plugin-code/`, each folder whose name
    /// is no plugin with a row this AutoScene reads, and in the folder of
    /// one, each file but the code of the component it runs. Of a package,
    /// that is code of the component its lock lists; of a loaded folder,
    /// which a developer rebuilds often, the newest. Code of this
    /// AutoScene's engine goes before another's. A plugin that runs no
    /// component, or is broken, keeps none. What can't be read stays.
    fn sweep_code(&self) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(&self.code) else { return Vec::new() };
        let rows: BTreeMap<PluginId, String> = self.rows().unwrap_or_default().into_iter().collect();
        let mut moved = Vec::new();
        for entry in entries.flatten() {
            let id = entry.file_name().to_str().and_then(|name| PluginId::parse(name).ok());
            let record = id.and_then(|id| read_row(&id, rows.get(&id)?));
            let (Some(record), true) = (record, entry.file_type().is_ok_and(|f| f.is_dir())) else {
                moved.extend(self.move_away(&entry.path()));
                continue;
            };
            let in_use = self.code_in_use(&self.load(record));
            if matches!(in_use, CodeInUse::None) {
                moved.extend(self.move_away(&entry.path()));
                continue;
            }
            let Ok(files) = fs::read_dir(entry.path()) else { continue };
            let files: Vec<_> = files.flatten().collect();
            // Of the files that may be its code, one this AutoScene's engine
            // compiled, the newest of them: another engine's is another
            // AutoScene's (an older one, or a beta sharing this data
            // folder), however new, and this one can't load it.
            let own = own_code_end();
            let newest = files
                .iter()
                .filter(|f| f.file_type().is_ok_and(|t| t.is_file()) && in_use.may_be(&f.file_name().to_string_lossy()))
                .max_by_key(|f| {
                    let mine = own.as_deref().is_some_and(|end| f.file_name().to_string_lossy().ends_with(end));
                    (mine, f.metadata().and_then(|m| m.modified()).ok())
                })
                .map(fs::DirEntry::file_name);
            for file in files {
                if Some(file.file_name()) != newest {
                    moved.extend(self.move_away(&file.path()));
                }
            }
        }
        moved
    }

    /// Which compiled code of `p` is in use.
    fn code_in_use(&self, p: &Installed) -> CodeInUse {
        let Some(Ok(Launch::Wasm { component })) = p.manifest.as_ref().map(Manifest::launch) else {
            return CodeInUse::None;
        };
        let Some(sha256) = &p.record.lock_sha256 else { return CodeInUse::Newest };
        let listed =
            installed_lock(&p.dir, sha256).ok().and_then(|l| l.files.into_iter().find(|f| f.path == component));
        listed.map_or(CodeInUse::None, |f| CodeInUse::Component(f.sha256))
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
    /// its icon and banner, its trust (the catalog's, or its signature's),
    /// and that it may replace the installed version. A version the catalog
    /// withdrew is refused. Last, what a WebAssembly plugin's component
    /// imports is read and checked: nothing of it is compiled, whoever opened
    /// the package and whatever the setting says ([`Store::commit`] does
    /// that). A package that fails leaves no folder.
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
        let launch = if m.runs_program() { Some(m.launch()?) } else { None };
        if let Some(launch) = &launch {
            package::check_launch(&unpacked.lock, launch, current_platform())?;
        }
        package::check_pages(staged, Some(&unpacked.lock), &m.pages)?;
        package::check_pictures(staged, &unpacked.lock, &m)?;
        let previous = self.record(&m.id)?;
        let Unpacked { lock, lock_bytes, signature, author_key } = unpacked;
        let pin = previous.as_ref().and_then(|p| p.author_key.as_ref());
        let (sig, author_pub) = (signature.as_deref(), author_key.as_deref());
        let (verdict, approval) =
            verdict(&m, Some(&lock_bytes), sig, author_pub, pin, &self.catalog(), catalog::now())?;
        if let Some(p) = &previous {
            check_update(p, &m, verdict.trust, Some(&package::sha256_hex(&lock_bytes)))?;
        }
        let component_warnings = read_component(&m, staged, launch.as_ref(), Some(&lock))?;
        // The id's random end: two installs of one version get two folders.
        let folder = format!("{}+{}", m.version, &id.simple().to_string()[24..]);
        Ok(Ticket {
            id,
            manifest: m,
            warnings,
            component_warnings,
            verdict,
            approval,
            lock: Some(lock),
            lock_bytes,
            signature,
            author_pub: author_key,
            source: Source::Package { folder },
            staged: Some(staged.to_path_buf()),
            previous,
            compiled: None,
            confirmed: false,
            created: Instant::now(),
        })
    }

    /// Checks the plugin in the developer's folder `dir` for this AutoScene,
    /// to load it where it is: it has no lock, so it is Unreviewed, and its
    /// files are read as they are each time. A version the catalog withdrew
    /// is refused here too. A WebAssembly plugin's component is read as a
    /// package's is, and compiled at its commit too.
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
        let launch = if m.runs_program() { Some(m.launch()?) } else { None };
        for file in launch.iter().flat_map(Launch::files) {
            if !fs::metadata(file_in(&dir, &file)).is_ok_and(|f| f.is_file()) {
                let needs = if m.is_sandboxed() { "runtime" } else { "command" };
                return Err(invalid(format!(
                    "The plugin folder is missing {}, which its {needs} needs.",
                    shown(&file)
                )));
            }
        }
        package::check_pages(&dir, None, &m.pages)?;
        let previous = self.record(&m.id)?;
        let pin = previous.as_ref().and_then(|p| p.author_key.as_ref());
        let (verdict, approval) =
            verdict(&m, None, None, None, pin, &self.catalog(), catalog::now()).map_err(|e| match e {
                // What judge says of an unsigned update: the folder isn't signed at all.
                PluginError::KeyChanged { .. } => {
                    invalid("The installed version is signed by its author. Uninstall it to load a folder.")
                }
                e => e,
            })?;
        if let Some(p) = &previous {
            check_update(p, &m, verdict.trust, None)?;
        }
        let component_warnings = read_component(&m, &dir, launch.as_ref(), None)?;
        Ok(Ticket {
            id: Uuid::now_v7(),
            manifest: m,
            warnings,
            component_warnings,
            verdict,
            approval,
            lock: None,
            lock_bytes: Vec::new(),
            signature: None,
            author_pub: None,
            source: Source::Folder { path: path.to_owned() },
            staged: None,
            previous,
            compiled: None,
            confirmed: false,
            created: Instant::now(),
        })
    }

    /// What the plugin of `m` may do that the version installed now, as the
    /// person agreed to it, may not: nothing when none is.
    pub fn widens(&self, m: &Manifest) -> PluginResult<Widens> {
        let Some(previous) = self.record(&m.id)? else { return Ok(Widens::default()) };
        Ok(Widens::of(&previous, Some(self.extra(&previous)?.agreed_runtime()), m))
    }

    /// Installs the plugin of `t`. One AutoScene hasn't reviewed needs
    /// `allow_unreviewed` (the setting) and `accept_unreviewed` (the
    /// person's typed yes); one it reviewed that [`Store::widens`] its
    /// installed version needs [`Ticket::confirmed`]. Its files and its
    /// trust are checked again, by the catalog of now, and what it replaces
    /// is the installed version now, which may have changed since it was
    /// staged. One the catalog reviewed when it was staged and doesn't now,
    /// or withdrew meanwhile, is refused, and so is one staged as an update
    /// of a plugin uninstalled since. Whatever fails, the plugin stays as it
    /// was, and the staged folder and any code compiled for it are removed.
    ///
    /// A WebAssembly plugin is compiled here, unless [`Store::compile`] did,
    /// past the answers above and before anything changes, so one that
    /// can't run replaces nothing.
    pub fn commit(&self, mut t: Ticket, allow_unreviewed: bool, accept_unreviewed: bool) -> PluginResult<Installed> {
        let installed = self
            .compile(&mut t, allow_unreviewed, accept_unreviewed, &CancelToken::new())
            .and_then(|()| self.install(&t, allow_unreviewed, accept_unreviewed));
        if installed.is_err() {
            self.discard(t);
        }
        installed
    }

    /// Compiles the plugin of `t`, if it is a WebAssembly plugin not compiled
    /// yet, once all [`Store::commit`] asks of its answers holds now, and its
    /// files are as they were staged. So nothing the catalog pulled since,
    /// or that was never allowed, is compiled. It touches nothing installed
    /// and no lock is held, as it can take a while: a caller does this first
    /// when what it does around the commit shouldn't wait for it, or fail
    /// with it. `cancel` ends it ([`PluginError::Cancelled`]). `t` stays the
    /// caller's whatever comes of it, to commit or [`Store::discard`].
    pub fn compile(
        &self,
        t: &mut Ticket,
        allow_unreviewed: bool,
        accept_unreviewed: bool,
        cancel: &CancelToken,
    ) -> PluginResult<()> {
        // Before the files are hashed, when the answer is no already.
        self.judged(t, self.record(&t.manifest.id)?.as_ref(), allow_unreviewed, accept_unreviewed)?;
        #[cfg(feature = "wasm")]
        if t.compiles()
            && let Launch::Wasm { component } = t.manifest.launch()?
        {
            if cancel.is_cancelled() {
                return Err(PluginError::Cancelled);
            }
            if let Some(staged) = self.staged(t) {
                self.check_staged(staged, &t.lock_bytes)?;
            }
            let dir = match (&t.source, self.staged(t)) {
                (Source::Folder { path }, _) => Path::new(path),
                (_, Some(staged)) => staged,
                _ => return Err(changed_while_waiting()),
            };
            let file = file_in(dir, &component);
            let sha256 = listed_sha256(t.lock.as_ref(), &component)?;
            let (m, cache) = (&t.manifest, self.cache_dir(&t.manifest.id));
            t.compiled = Some(match &self.compiling {
                Compiling::Apart(compiler) => compiler.prepare(&file, sha256, &cache, m, cancel)?,
                Compiling::Here => wasm::prepare(&file, sha256, &cache, m)?.1,
                Compiling::Nowhere => {
                    tracing::error!(plugin = %m.id, "this store wasn't told how to compile plugins");
                    return Err(invalid(wasm::COMPILE_FAILED));
                }
            });
        }
        #[cfg(not(feature = "wasm"))]
        let _ = cancel;
        Ok(())
    }

    /// What `t` is by the catalog of now and the installed version
    /// `previous`, when all that is asked of its answers holds: the second
    /// look at what was staged.
    fn judged(
        &self,
        t: &Ticket,
        previous: Option<&Record>,
        allow_unreviewed: bool,
        accept_unreviewed: bool,
    ) -> PluginResult<Verdict> {
        let m = &t.manifest;
        // What was agreed to went with its row, so nothing below could tell
        // what this update adds: a first install has its own review.
        if t.previous.is_some() && previous.is_none() {
            return Err(invalid("This plugin was uninstalled while its update waited. Open it again."));
        }
        let pin = previous.and_then(|p| p.author_key.as_ref());
        let lock = matches!(t.source, Source::Package { .. }).then_some(t.lock_bytes.as_slice());
        let (sig, author_pub) = (t.signature.as_deref(), t.author_pub.as_deref());
        let judged = verdict(m, lock, sig, author_pub, pin, &self.catalog(), catalog::now());
        // The person said yes to a plugin AutoScene reviewed: when the
        // catalog no longer does, that is why, not what the plugin's
        // signature (unread while it was reviewed) or pin would say now.
        let (verdict, _) = match judged {
            Err(e @ PluginError::Withdrawn { .. }) => return Err(e),
            r if t.approval.is_some() && !matches!(r, Ok((_, Some(_)))) => {
                return Err(invalid("The catalog changed while this plugin waited. Open it again."));
            }
            r => r?,
        };
        allowed(verdict.trust, allow_unreviewed, accept_unreviewed)?;
        if let Some(p) = previous {
            check_update(p, m, verdict.trust, lock.map(package::sha256_hex).as_deref())?;
        }
        // By the installed version now: the page's word is no yes here, and
        // one AutoScene hasn't reviewed had its own above.
        if verdict.trust == Trust::Reviewed && !t.confirmed && self.widens(m)?.any() {
            return Err(invalid("This update asks for more than you agreed to. Open it again."));
        }
        Ok(verdict)
    }

    fn install(&self, t: &Ticket, allow_unreviewed: bool, accept_unreviewed: bool) -> PluginResult<Installed> {
        let m = &t.manifest;
        let package = match (&t.source, self.staged(t)) {
            (Source::Package { folder }, Some(staged)) => Some((folder, staged)),
            (Source::Folder { .. }, None) => None,
            _ => return Err(changed_while_waiting()),
        };
        let _changing = self.changing.lock();
        let previous = self.record(&m.id)?;
        let verdict = self.judged(t, previous.as_ref(), allow_unreviewed, accept_unreviewed)?;
        // Here too, past a compile that can take minutes: what goes in place
        // now is what was staged.
        if let Some((_, staged)) = package {
            self.check_staged(staged, &t.lock_bytes)?;
        }
        let lock_sha256 = package.is_some().then(|| package::sha256_hex(&t.lock_bytes));
        // The secrets it has keep their values, and the events it still has
        // stay on; it runs as its manifest says from now, which the person
        // agreed to. A value kept under the key of a secret setting, from
        // before it was a secret, goes.
        let (kept, events) = match &previous {
            Some(p) => {
                let extra = self.extra(p)?;
                (extra.secrets, events_kept(m, &extra.events))
            }
            None => (Vec::new(), Vec::new()),
        };
        // As the person turned them on for the version before, read from
        // its files while they are still there.
        let after_export = match &previous {
            Some(p) => after_export_kept(m, self.load(p.clone()).manifest.as_ref(), &p.after_export),
            None => Vec::new(),
        };
        let installed_at = as_api::now_ms();
        let extra = Extra { installed_at, secrets: kept, events, ..Extra::agreed(m.runtime.kind) };
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
            after_export,
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
        // The row names the new version: the one before can go, and its code.
        match (previous.map(|p| p.source), &dir) {
            (Some(Source::Package { .. }), None) => self.throw_away(&plugin_dir),
            (Some(Source::Package { folder }), Some(dir)) if plugin_dir.join(&folder) != *dir => {
                self.throw_away(&plugin_dir.join(folder));
            }
            _ => {}
        }
        self.keep_code(&m.id, t.compiled.as_deref());
        Ok(self.load(record))
    }

    /// Drops `t`, the folder it was staged in, and the code compiled for it
    /// unless the installed version runs that same code.
    pub fn discard(&self, t: Ticket) {
        if let Some(staged) = self.staged(&t) {
            self.throw_away(staged);
        }
        if let Some(file) = &t.compiled {
            self.discard_code(&t.manifest.id, file);
        }
    }

    /// Removes `file`, the code of `id` nothing was installed with, and its
    /// folder when that is all it held. One of a package has the name its
    /// component's hash gives it, so a repair and an update that keeps its
    /// component share the file of the version installed.
    fn discard_code(&self, id: &PluginId, file: &Path) {
        let _changing = self.changing.lock();
        let name = file.file_name().unwrap_or_default().to_string_lossy();
        if self.get(id).is_ok_and(|p| self.code_in_use(&p).may_be(&name)) {
            return;
        }
        if let Err(e) = fs::remove_file(file)
            && e.kind() != ErrorKind::NotFound
        {
            tracing::debug!(error = %e, plugin = %id, "a compiled plugin stays until the next start");
        }
        let _ = fs::remove_dir(self.cache_dir(id));
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
    /// best-effort, and its installed files and compiled code. A loaded
    /// folder is the developer's and stays where it is. `delete_data` also
    /// removes what it kept ([`Store::data_dir`]).
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
        self.keep_code(id, None);
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

    /// Turns the event `event` of `id` on or off: on, it runs by itself
    /// when what it waits for happens.
    pub fn set_event(&self, id: &PluginId, event: &str, on: bool) -> PluginResult<Installed> {
        let _changing = self.changing.lock();
        let record = self.record(id)?.ok_or_else(|| not_found(id))?;
        let p = self.load(record);
        let m = manifest_of(&p)?;
        if !m.events.iter().any(|e| e.id == event) {
            return Err(invalid(format!("The plugin has no event {}.", shown(event))));
        }
        let mut extra = self.extra(&p.record)?;
        let mut turned_on = extra.events.clone();
        turned_on.retain(|e| e != event);
        if on {
            turned_on.push(event.to_owned());
        }
        extra.events = events_kept(m, &turned_on);
        self.write_extra(id, &extra, None)?;
        Ok(self.load(p.record))
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

    /// What the large files `p`'s manifest names take on the disk now, by
    /// their sizes ([`crate::resources::kept_bytes_in`]): never a hash.
    pub fn resource_bytes(&self, p: &Installed) -> u64 {
        let named = p.manifest.as_ref().map_or(&[][..], |m| &m.resources);
        crate::resources::kept_bytes_in(&self.resources.join(p.record.id.as_str()), named)
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

    /// Where the compiled code of the WebAssembly plugin `id` is kept
    /// (`cache/plugin-code/<id>`): never under a folder a plugin sees.
    pub fn cache_dir(&self, id: &PluginId) -> PathBuf {
        self.code.join(id.as_str())
    }

    /// Keeps, of the compiled code of `id`, only the file `compiled` (the
    /// code of the version just installed); none keeps none. Best-effort:
    /// what stays is swept at the next open ([`Store::sweep_code`]). An
    /// AutoScene without the sandbox compiles nothing to keep.
    fn keep_code(&self, id: &PluginId, compiled: Option<&Path>) {
        #[cfg(feature = "wasm")]
        match compiled.and_then(Path::file_name).and_then(|name| name.to_str()) {
            Some(keep) => wasm::keep_only(&self.cache_dir(id), keep),
            None => wasm::remove(&self.cache_dir(id)),
        }
        #[cfg(not(feature = "wasm"))]
        let _ = (id, compiled);
    }

    /// A run of `action` of `p`, as [`crate::runtime::run`] takes it: its
    /// command from its folder, started in [`Store::data_dir`] and logged to
    /// [`Store::log_path`], and a run line with a new run id that names the
    /// plugin, this AutoScene, the app's FFmpeg (`ffmpeg` and `ffprobe`, of
    /// `ffmpeg_version`) and each of its settings, as [`Store::values`] gives
    /// them. The run speaks the protocol the plugin declared. The project and
    /// the export it runs on are the host's to add, and so are the answers to
    /// its calls, a uv script's uv and the folder of the large files its
    /// plugin names ([`crate::resources`]). A broken plugin can't run.
    pub fn run_spec(
        &self,
        p: &Installed,
        action: ActionContext,
        ffmpeg: &Path,
        ffprobe: &Path,
        ffmpeg_version: &str,
    ) -> PluginResult<RunSpec> {
        let m = manifest_of(p)?;
        let tools = ToolPaths {
            ffmpeg: ffmpeg.to_string_lossy().into_owned(),
            ffprobe: ffprobe.to_string_lossy().into_owned(),
            ffmpeg_version: ffmpeg_version.into(),
        };
        let message = self.run_message(p, m, action, tools)?;
        let id = &p.record.id;
        Ok(RunSpec {
            name: m.name.clone(),
            dir: p.dir.clone(),
            launch: m.launch()?,
            data_dir: self.data_dir(id),
            log_path: self.log_path(id),
            ffmpeg: ffmpeg.into(),
            ffprobe: ffprobe.into(),
            uv: None,
            resources: None,
            protocol: m.engines.protocol,
            calls: None,
            verdicts: None,
            message,
        })
    }

    /// A run of `action` of the WebAssembly plugin `p`, as
    /// [`crate::wasm::run`] takes it: its component from its folder (a
    /// package's by the SHA-256 its lock lists), its code kept in
    /// [`Store::cache_dir`], what it may use by its grants
    /// ([`Grants::of`](crate::calls::Grants::of): what the person agreed to
    /// that its manifest still asks for) and its limits, and a run as
    /// [`Store::run_spec`] makes it, without FFmpeg, which it can't start.
    /// The values of its secrets and the folder of the large files its
    /// plugin names ([`crate::resources`]) are the host's to add. A broken
    /// plugin can't run, nor one that isn't a WebAssembly plugin.
    #[cfg(feature = "wasm")]
    pub fn wasm_spec(&self, p: &Installed, action: ActionContext) -> PluginResult<WasmSpec> {
        let m = manifest_of(p)?;
        let Launch::Wasm { component } = m.launch()? else {
            return Err(invalid(format!("{} isn't a WebAssembly plugin.", m.name)));
        };
        let grants = crate::calls::Grants::of(p)?;
        let component_sha256 = match &p.record.lock_sha256 {
            Some(sha256) => {
                let lock = installed_lock(&p.dir, sha256).map_err(invalid)?;
                let listed = lock.files.into_iter().find(|f| f.path == component);
                Some(listed.ok_or_else(|| invalid(CHANGED))?.sha256)
            }
            None => None,
        };
        let id = &p.record.id;
        Ok(WasmSpec {
            name: m.name.clone(),
            component: file_in(&p.dir, &component),
            component_sha256,
            cache: self.cache_dir(id),
            data_dir: self.data_dir(id),
            resources_dir: None,
            log_path: self.log_path(id),
            grants,
            limits: Limits::of(m.runtime.limits.unwrap_or_default()),
            secrets: BTreeMap::new(),
            message: self.run_message(p, m, action, ToolPaths::default())?,
        })
    }

    /// The run line of `action` of `p` (whose manifest is `m`), with a new
    /// run id, naming the plugin, this AutoScene, the tools `tools` and each
    /// of its settings, as [`Store::values`] gives them, in the protocol it
    /// declared. The project and the export it runs on are the host's to
    /// add.
    fn run_message(
        &self,
        p: &Installed,
        m: &Manifest,
        action: ActionContext,
        tools: ToolPaths,
    ) -> PluginResult<RunMessage> {
        let id = &p.record.id;
        Ok(RunMessage {
            event: None,
            protocol: m.engines.protocol,
            run_id: Uuid::now_v7().to_string(),
            plugin: PluginContext {
                id: id.clone(),
                version: p.record.version.clone(),
                data_dir: self.data_dir(id).to_string_lossy().into_owned(),
                resources_dir: None,
                // The run's own, made as it starts.
                temp_dir: None,
            },
            app: AppContext { version: self.app.to_string(), platform: current_platform().into() },
            tools,
            settings: merged(&m.settings, &self.stored_values(id)?.unwrap_or_default()),
            action,
            project: None,
            export: None,
            input: None,
            shots: None,
            output: None,
            pack: None,
        })
    }

    /// The service of `p`, as [`crate::service::ProcessService::start`]
    /// takes it: its command from its folder, started in
    /// [`Store::data_dir`] and logged to [`Store::service_log_path`], with
    /// `calls` answering its calls, and a serve line with a new id that
    /// names what a run line does but a task. Only a plugin whose runtime is
    /// a service has one, and it speaks protocol 2; a broken plugin has
    /// none. A uv script's uv, and the folder of the large files its plugin
    /// names, are the host's to get.
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
                resources_dir: None,
                // The start's own, made as it starts.
                temp_dir: None,
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
            uv: None,
            resources: None,
            ready_wait: READY_WAIT,
            message,
            calls,
        })
    }

    /// The icon of `p` as a `data:` URL, while it is the picture its
    /// package listed (a loaded folder's: a picture of an icon's size).
    pub fn icon_data_url(&self, p: &Installed) -> Option<String> {
        self.picture_data_url(p, p.manifest.as_ref()?.icon.as_deref()?, Picture::Icon)
    }

    /// The banner of `p` as a `data:` URL, as [`Store::icon_data_url`] reads
    /// its icon. Read only when it is shown: a list carries its
    /// [`Store::banner_hash`].
    pub fn banner_data_url(&self, p: &Installed) -> Option<String> {
        self.picture_data_url(p, p.manifest.as_ref()?.banner.as_deref()?, Picture::Banner)
    }

    /// The `picture` of `p` at the package path `path` as a `data:` URL,
    /// while it is the picture its package listed: its size and SHA-256
    /// those of its `files.lock`. A loaded folder's has no lock: a sound
    /// picture, as it is now, a plain file in plain folders. None for a path
    /// a package may not name ([`package::check_path`]).
    pub fn picture_data_url(&self, p: &Installed, path: &str, picture: Picture) -> Option<String> {
        package::check_path(path).ok()?;
        let format = PictureFormat::of(path)?;
        let bytes = self.picture_bytes(p, path, picture)?;
        Some(format!("data:{};base64,{}", format.mime(), STANDARD.encode(&bytes)))
    }

    /// What tells one banner of `p` from another, without its bytes: the
    /// SHA-256 its package's `files.lock` lists, once the banner passes
    /// [`package::check_picture`], or a loaded folder's banner's SHA-256 as
    /// it is now (a file of at most [`package::MAX_BANNER`]). A new version
    /// or a changed file gives a new one.
    pub fn banner_hash(&self, p: &Installed) -> Option<String> {
        let banner = p.manifest.as_ref()?.banner.as_deref()?;
        match p.record.lock_sha256 {
            Some(_) => Some(self.listed_picture(p, banner, Picture::Banner)?.sha256),
            None => Some(package::sha256_hex(&self.picture_bytes(p, banner, Picture::Banner)?)),
        }
    }

    /// The bytes of the `picture` of `p` at `path`, while they are the ones
    /// [`Store::listed_picture`] lists and a sound picture.
    fn picture_bytes(&self, p: &Installed, path: &str, picture: Picture) -> Option<Vec<u8>> {
        let listed = self.listed_picture(p, path, picture)?;
        let mut bytes = Vec::new();
        File::open(file_in(&p.dir, path)).ok()?.take(listed.size + 1).read_to_end(&mut bytes).ok()?;
        let hashed = p.record.lock_sha256.is_none() || package::sha256_hex(&bytes) == listed.sha256;
        let sound = package::picture_bytes_ok(picture, PictureFormat::of(path)?, &bytes);
        (bytes.len() as u64 == listed.size && hashed && sound).then_some(bytes)
    }

    /// The `files.lock` entry of the `picture` of `p` at `path`, once it
    /// passes [`package::check_picture`]. A loaded folder's is made from the
    /// file as it is, without a hash, while it is a plain file in plain
    /// folders: no link on the way leads out of the folder, as for its pages.
    fn listed_picture(&self, p: &Installed, path: &str, picture: Picture) -> Option<LockedFile> {
        let lock = match &p.record.lock_sha256 {
            Some(sha256) => installed_lock(&p.dir, sha256).ok()?,
            None => {
                if !package::plain_file(&p.dir, path) {
                    return None;
                }
                let size = fs::symlink_metadata(file_in(&p.dir, path)).ok().filter(|f| f.is_file())?.len();
                let listed = LockedFile { path: path.to_owned(), size, sha256: String::new(), exec: false };
                FilesLock { lock_version: LOCK_VERSION, files: vec![listed] }
            }
        };
        package::check_picture(&p.dir, &lock, path, picture).ok()?;
        lock.files.into_iter().find(|f| f.path == path)
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
            None => {
                let record = unreadable(id, value);
                Installed {
                    dir: self.root.join(record.id.as_str()),
                    withdrawn: self.withdrawn(&record.id, &record.version),
                    record,
                    manifest: None,
                    warnings: Vec::new(),
                    broken: Some(NEWER.into()),
                    extra: Extra::default(),
                }
            }
        }
    }

    /// Why the catalog withdrew version `version` of `id`, if it did.
    fn withdrawn(&self, id: &PluginId, version: &str) -> Option<String> {
        self.catalog().withdrawal(id, version).map(|w| w.reason.clone())
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
                // Without the sandbox, one a newer AutoScene installed to run
                // in it waits for it, as at install ([`Store::compile`]).
                #[cfg(not(feature = "wasm"))]
                let read = read.and_then(|parsed| match parsed.manifest.is_sandboxed() {
                    false => Ok(parsed),
                    true => Err(NEWER.into()),
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
        // The version that runs: a loaded folder's plugin.json may name
        // another than its record. A broken plugin is matched by its record's.
        let version = manifest.as_ref().map_or(record.version.as_str(), |m| m.version.as_str());
        let withdrawn = self.withdrawn(&record.id, version);
        Installed { record, manifest, warnings, dir, broken, extra, withdrawn }
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

/// Of the events `turned_on`, those `m` still has, in its order.
fn events_kept(m: &Manifest, turned_on: &[String]) -> Vec<String> {
    m.events.iter().filter(|e| turned_on.contains(&e.id)).map(|e| e.id.clone()).collect()
}

/// Of the export actions `turned_on` for after every export in the version
/// of manifest `before` (none when it can't be read), those that stay on in
/// the version of `m`: each it still has, unless it now runs after a failed
/// export too and didn't then. The person agreed to whole exports only, so
/// they turn it on again.
pub fn after_export_kept(m: &Manifest, before: Option<&Manifest>, turned_on: &[String]) -> Vec<String> {
    let failed_before =
        |id: &str| before.is_some_and(|b| b.export_actions.iter().any(|a| a.id == id && a.after_failed));
    turned_on
        .iter()
        .filter(|id| m.export_actions.iter().any(|a| a.id == **id && (!a.after_failed || failed_before(id))))
        .cloned()
        .collect()
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

/// The trust of the plugin of manifest `m` by AutoScene's `catalog` at
/// `now`, and the catalog's approval of it when it reviewed it. `lock` is a
/// package's `files.lock` (none for a loaded folder), `sig` and `author_pub`
/// its `files.lock.minisig` and `author.pub`, and `pin` the author key its
/// plugin is pinned to, when it is installed.
///
/// A version the catalog withdrew is [`PluginError::Withdrawn`], whatever
/// else it is. A package whose lock the living catalog lists for its plugin
/// and version is Reviewed, and its signature isn't read: the files the
/// catalog reviewed are the ones its lock hashes. It is pinned to the author
/// key the catalog names, or keeps its pin when the catalog names none.
/// Anything else is [`trust::judge`]'s.
pub fn verdict(
    m: &Manifest,
    lock: Option<&[u8]>,
    sig: Option<&str>,
    author_pub: Option<&str>,
    pin: Option<&AuthorKey>,
    catalog: &Catalog,
    now: u64,
) -> PluginResult<(Verdict, Option<Approval>)> {
    if let Some(w) = catalog.withdrawal(&m.id, &m.version) {
        return Err(PluginError::Withdrawn { name: m.name.clone(), reason: w.reason.clone() });
    }
    let approved = lock.and_then(|lock| catalog.approval(&m.id, &m.version, &package::sha256_hex(lock), now));
    let Some(entry) = approved else {
        return Ok((trust::judge(lock.unwrap_or_default(), sig, author_pub, m, pin)?, None));
    };
    let author_key = match &entry.author_key {
        Some(base64) => Some(AuthorKey { base64: base64.clone(), key_id: trust::key_id(base64)? }),
        None => pin.cloned(),
    };
    let approval = Approval {
        published: catalog.index().map_or(0, |i| i.published),
        repo: entry.source.repo.clone(),
        commit: entry.source.commit.clone(),
        path: entry.source.path.clone(),
    };
    Ok((Verdict { trust: Trust::Reviewed, author_key }, Some(approval)))
}

/// The warnings of what the component of the WebAssembly plugin of `m` in
/// its folder `dir` imports ([`wasm::inspect`]), which compiles nothing: none
/// for a plugin whose `launch` runs no component. A package's component must
/// be the one its `lock` lists. An AutoScene without the sandbox refuses it:
/// it needs a newer one.
fn read_component(
    m: &Manifest,
    dir: &Path,
    launch: Option<&Launch>,
    lock: Option<&FilesLock>,
) -> PluginResult<Vec<String>> {
    let Some(Launch::Wasm { component }) = launch else { return Ok(Vec::new()) };
    #[cfg(feature = "wasm")]
    {
        Ok(wasm::inspect(&file_in(dir, component), listed_sha256(lock, component)?, m)?.warnings)
    }
    #[cfg(not(feature = "wasm"))]
    {
        let _ = (m, dir, component, lock);
        Err(PluginError::TooNew("runtime".into()))
    }
}

/// The SHA-256 `lock` gives `component`; none without a lock (a folder).
#[cfg(feature = "wasm")]
fn listed_sha256<'a>(lock: Option<&'a FilesLock>, component: &str) -> PluginResult<Option<&'a str>> {
    let Some(lock) = lock else { return Ok(None) };
    match lock.files.iter().find(|f| f.path == component) {
        Some(listed) => Ok(Some(listed.sha256.as_str())),
        None => Err(invalid(format!("The plugin package is missing {}.", shown(component)))),
    }
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
/// with the same files (a repair). The pin is [`verdict`]'s.
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
    use crate::catalog::{CatalogState, Entry, Index, PackageRef, SourceRef, Withdrawal};
    use crate::package::{AUTHOR_KEY, SIG};
    use crate::protocol::{ActionKind, Trigger};
    use crate::secrets::MemorySecrets;

    const ID: &str = "dev.autoscene.hello-export";
    const DAY: u64 = 86_400;
    const LEAKY: &str = "It sent export paths to a server.";

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
            let store = Store::open(&self.paths, Arc::clone(&self.db), app(), self.secrets.clone()).unwrap();
            #[cfg(feature = "wasm")]
            let store = store.compile_here();
            store
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

    /// The catalog's entry of the package at `path`, as its CI writes it:
    /// its plugin, version and lock, built from AutoScene's repository.
    fn entry_of(path: &Path) -> Entry {
        let mut zip = zip::ZipArchive::new(File::open(path).unwrap()).unwrap();
        let mut read = |name: &str| {
            let mut bytes = Vec::new();
            zip.by_name(name).unwrap().read_to_end(&mut bytes).unwrap();
            bytes
        };
        let m: Value = serde_json::from_slice(&read(MANIFEST)).unwrap();
        let lock = package::sha256_hex(&read(LOCK));
        Entry {
            id: PluginId::parse(m["id"].as_str().unwrap()).unwrap(),
            version: m["version"].as_str().unwrap().to_owned(),
            lock,
            package: PackageRef {
                urls: vec!["https://github.com/tcmmi/autoscene-plugins/releases/download/x/x.asp".into()],
                size: fs::metadata(path).unwrap().len(),
                sha256: package::sha256_hex(&fs::read(path).unwrap()),
            },
            author_key: None,
            name: "Hello Export".into(),
            summary: String::new(),
            author: "AutoScene".into(),
            license: None,
            platforms: Vec::new(),
            runtime: "process".into(),
            gpu: false,
            uv_script: true,
            permissions: vec!["exports:read".into()],
            hosts: Vec::new(),
            engines: ">=1.0.0, <2".into(),
            source: SourceRef {
                repo: "https://github.com/tcmmi/AutoScene".into(),
                commit: "c".repeat(40),
                path: "examples/plugins/hello-export".into(),
            },
            icon: None,
            approved: catalog::now() - DAY,
            resource_bytes: 0,
            resource_hosts: Vec::new(),
        }
    }

    /// A catalog published a day ago that runs out at `expires`, listing
    /// `plugins` and withdrawing `withdrawn`.
    fn catalog_of(plugins: Vec<Entry>, withdrawn: Vec<Withdrawal>, expires: u64) -> Arc<Catalog> {
        let published = catalog::now() - DAY;
        let index = Index {
            catalog_version: 1,
            epoch: 1,
            seq: 1,
            published,
            expires,
            epoch_certificate: None,
            plugins,
            withdrawn: withdrawn.clone(),
        };
        let state = CatalogState { v: 1, epoch: 1, seq: 1, published, expires, withdrawn, ..CatalogState::default() };
        Arc::new(Catalog::new(Some(index), state))
    }

    /// Has `store` judge by a living catalog that lists `packages`.
    fn reviewed(store: &Store, packages: &[&Path]) {
        let entries = packages.iter().map(|p| entry_of(p)).collect();
        store.set_catalog(catalog_of(entries, Vec::new(), catalog::now() + 29 * DAY));
    }

    /// The catalog's withdrawal of the versions `versions` of Hello Export.
    fn withdrawal(versions: &str) -> Withdrawal {
        Withdrawal { id: id(), versions: versions.into(), reason: LEAKY.into(), since: catalog::now() - DAY }
    }

    fn withdrawn(r: PluginResult<Ticket>) -> String {
        match r {
            Err(e @ PluginError::Withdrawn { .. }) => e.to_string(),
            r => panic!("expected Withdrawn, got {r:?}"),
        }
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
    fn an_update_that_runs_after_failed_exports_too_is_turned_on_again() {
        let place = Place::new();
        let store = place.store();
        let failing = |version: &str, after_failed: &[&str]| {
            let mut m = manifest(ID, version);
            m["manifestVersion"] = json!(2);
            m["engines"]["protocol"] = json!(2);
            for a in m["contributes"]["exportActions"].as_array_mut().unwrap() {
                let id = a["id"].as_str().unwrap().to_owned();
                a["afterFailed"] = json!(after_failed.contains(&id.as_str()));
            }
            place.package(&files(&m), None, false)
        };
        install(&store, &failing("1.0.0", &["wave"])).unwrap();
        store.set_after_export(&id(), "hello", true).unwrap();
        store.set_after_export(&id(), "wave", true).unwrap();
        // 1.1.0 runs hello after a failed export too, and the person agreed
        // to whole exports only; wave ran so already.
        let new = install(&store, &failing("1.1.0", &["hello", "wave"])).unwrap();
        assert_eq!(new.record.after_export, ["wave"]);
        assert_eq!(store.get(&id()).unwrap().record.after_export, ["wave"]);
        // Turned on again, it stays on through an update that keeps it so.
        store.set_after_export(&id(), "hello", true).unwrap();
        let kept = install(&store, &failing("1.2.0", &["hello", "wave"])).unwrap();
        assert_eq!(kept.record.after_export, ["hello", "wave"]);
        // One that stops running after a failed export keeps it on.
        assert_eq!(install(&store, &failing("1.3.0", &[])).unwrap().record.after_export, ["hello", "wave"]);
    }

    #[test]
    fn turned_on_events_survive_an_update_that_keeps_them() {
        let place = Place::new();
        let store = place.store();
        let waiting_for = |version: &str, events: &[&str]| {
            let mut m = manifest(ID, version);
            m["manifestVersion"] = json!(2);
            let events: Vec<Value> =
                events.iter().map(|e| json!({"id": e, "title": e, "on": "analysisFinished"})).collect();
            m["contributes"]["events"] = json!(events);
            place.package(&files(&m), None, false)
        };
        let p = install(&store, &waiting_for("1.0.0", &["done", "tally"])).unwrap();
        // Off until the person turns them on, and then in its plugin.json's order.
        assert_eq!(p.extra.events, Vec::<String>::new());
        store.set_event(&id(), "tally", true).unwrap();
        assert_eq!(store.set_event(&id(), "done", true).unwrap().extra.events, ["done", "tally"]);
        assert_eq!(store.get(&id()).unwrap().extra.events, ["done", "tally"]);
        assert_eq!(store.set_event(&id(), "done", false).unwrap().extra.events, ["tally"]);
        let e = store.set_event(&id(), "later", true).unwrap_err();
        assert_eq!(e.to_string(), "The plugin has no event later.");
        store.set_event(&id(), "done", true).unwrap();

        // 1.1.0 has no tally any more, and a new event, which is off.
        let update = waiting_for("1.1.0", &["fresh", "done"]);
        place.refuse_writes();
        assert!(install(&store, &update).is_err());
        assert_eq!(store.get(&id()).unwrap().extra.events, ["done", "tally"]);
        place.allow_writes();
        let new = install(&store, &update).unwrap();
        assert_eq!(new.extra.events, ["done"]);
        assert_eq!(store.get(&id()).unwrap().extra.events, ["done"]);
        // One that drops it forgets it: turned on again, it starts off.
        install(&store, &waiting_for("1.2.0", &["fresh"])).unwrap();
        assert_eq!(install(&store, &waiting_for("1.3.0", &["done"])).unwrap().extra.events, Vec::<String>::new());

        // None on, the row is written as AutoScene 1.1 wrote it.
        let row = place.db.setting(&extra_key(&id())).unwrap().unwrap();
        assert!(!row.contains("events"), "{row}");
        store.set_event(&id(), "done", true).unwrap();
        let row = place.db.setting(&extra_key(&id())).unwrap().unwrap();
        assert!(row.contains(r#""events":["done"]"#), "{row}");
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
            let app = Version::parse(app).unwrap();
            let store = Store::open(&place.paths, Arc::clone(&place.db), app, place.secrets.clone()).unwrap();
            #[cfg(feature = "wasm")]
            let store = store.compile_here();
            store.get(&id()).unwrap().broken
        };
        assert_eq!(broken_on("0.9.0").as_deref(), Some(NEWER));
        assert_eq!(broken_on("2.0.0").as_deref(), Some("This plugin only works with an older AutoScene."));
        assert_eq!(broken_on("1.5.0"), None);
        assert_eq!(place.plugins(), holding(&p), "its files stay");
    }

    /// Without the sandbox (the feature `wasm`), as AutoScene 1.1 is.
    #[cfg(not(feature = "wasm"))]
    #[test]
    fn a_wasm_package_waits_for_a_newer_autoscene() {
        let place = Place::new();
        let store = place.store();
        let mut m = manifest(ID, "1.0.0");
        m["manifestVersion"] = json!(2);
        m["runtime"] = json!({"type": "wasm", "component": "plugin.wasm"});
        let plugin_json = ("plugin.json".to_owned(), serde_json::to_vec_pretty(&m).unwrap());
        let component = ("plugin.wasm".to_owned(), b"\0asm\x0d\x00\x01\x00".to_vec());
        let files: Files = vec![plugin_json.clone(), component];

        // Its manifest reads, and its files check out, but nothing here runs it yet.
        let staged = store.stage(&place.package(&files, None, false));
        assert!(matches!(staged, Err(PluginError::TooNew(ref f)) if f == "runtime"), "{staged:?}");
        assert_eq!(place.plugins(), [".staging", ".trash"], "the staged package is gone");
        let dev = place.folder("hello-wasm", &files);
        let loaded = store.stage_folder(&dev);
        assert!(matches!(loaded, Err(PluginError::TooNew(ref f)) if f == "runtime"), "{loaded:?}");
        assert!(store.list().unwrap().is_empty());

        // A package or folder without its component says so first.
        let Err(e) = store.stage(&place.package(&vec![plugin_json.clone()], None, false)) else { panic!() };
        assert_eq!(e.to_string(), "The plugin package is missing plugin.wasm, which its runtime needs.");
        let Err(e) = store.stage_folder(&place.folder("no-component", &vec![plugin_json])) else { panic!() };
        assert_eq!(e.to_string(), "The plugin folder is missing plugin.wasm, which its runtime needs.");

        // One a newer AutoScene loaded, read back here, waits for it too.
        let process = place.folder("hello-later", &with_secrets("1.0.0", &[]));
        let p = store.commit(store.stage_folder(&process).unwrap(), true, true).unwrap();
        fs::write(process.join("plugin.json"), &files[0].1).unwrap();
        fs::write(process.join("plugin.wasm"), &files[1].1).unwrap();
        let row = format!(r#"{{"v":1,"installedAt":{},"runtime":"wasm"}}"#, p.record.installed_at);
        place.db.set_setting(&format!("plugin_extra.{ID}"), &row).unwrap();
        let p = store.get(&id()).unwrap();
        assert_eq!((p.broken.as_deref(), p.manifest.is_none()), (Some(NEWER), true));
        assert!(crate::calls::Grants::of(&p).is_err());
    }

    /// The test guest's component (`cargo xtask wasm-guests`); none (the
    /// test skips) when it wasn't built.
    #[cfg(feature = "wasm")]
    fn guest() -> Option<Vec<u8>> {
        Some(fs::read(as_testkit::fixture_file("wasm/guest.wasm")?).unwrap())
    }

    /// `component` with a custom section named `name` at its end: another
    /// component (another SHA-256), which compiles and runs as it does.
    #[cfg(feature = "wasm")]
    fn other(component: &[u8], name: &str) -> Vec<u8> {
        let payload = [&[name.len() as u8][..], name.as_bytes()].concat();
        [component, &[0, payload.len() as u8], &payload].concat()
    }

    /// The files of the WebAssembly plugin `id` of `version` whose
    /// component is `component`: it reads the export (see [`manifest`]).
    #[cfg(feature = "wasm")]
    fn wasm_plugin(id: &str, version: &str, component: &[u8]) -> Files {
        let mut m = manifest(id, version);
        m["manifestVersion"] = json!(2);
        m["runtime"] = json!({"type": "wasm", "component": "plugin.wasm"});
        vec![("plugin.json".into(), serde_json::to_vec_pretty(&m).unwrap()), ("plugin.wasm".into(), component.to_vec())]
    }

    /// What the code folder of `id` holds, by name, sorted; none when it
    /// isn't there.
    #[cfg(feature = "wasm")]
    fn code(store: &Store, id: &str) -> Option<Vec<String>> {
        let entries = fs::read_dir(store.cache_dir(&PluginId::parse(id).unwrap())).ok()?;
        let mut names: Vec<String> = entries.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        Some(names)
    }

    #[cfg(feature = "wasm")]
    fn hello_action() -> ActionContext {
        ActionContext { kind: ActionKind::ExportAction, id: "hello".into(), trigger: Trigger::Menu }
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn a_wasm_package_is_read_at_its_check_and_compiled_at_its_commit() {
        let Some(guest) = guest() else { return };
        let place = Place::new();
        let store = place.store();
        let t = store.stage(&place.package(&wasm_plugin(ID, "1.0.0", &guest), None, false)).unwrap();
        // The test guest imports wasi:http and doesn't ask for network: the
        // check says so apart from what its plugin.json had skipped.
        assert_eq!(
            (t.warnings.as_slice(), t.component_warnings.as_slice()),
            (
                &[][..],
                &["It imports wasi:http but doesn't ask for network: every request it makes is refused.".to_owned()][..]
            )
        );
        // Nothing of it is compiled yet.
        assert_eq!((t.compiled.as_ref(), code(&store, ID)), (None, None));
        let p = store.commit(t, true, true).unwrap();
        assert_eq!((p.broken.as_deref(), p.extra.agreed_runtime()), (None, RuntimeKind::Wasm));
        // Its code is kept where no plugin looks, by its component's hash.
        let [name] = code(&store, ID).unwrap().try_into().unwrap();
        assert!(name.starts_with(&format!("{}-", package::sha256_hex(&guest))) && name.ends_with(".cwasm"), "{name}");
        assert!(place.paths.plugin_code().join(ID).join(&name).is_file());

        // A run of it: its component as its lock lists it, what it agreed
        // to, its limits, and no FFmpeg.
        let spec = store.wasm_spec(&p, hello_action()).unwrap();
        assert_eq!(spec.component, p.dir.join("plugin.wasm"));
        assert_eq!(spec.component_sha256, Some(package::sha256_hex(&guest)));
        assert_eq!((&spec.cache, &spec.data_dir), (&store.cache_dir(&id()), &store.data_dir(&id())));
        assert_eq!(spec.limits, Limits::of(crate::manifest::WasmLimits::default()));
        assert_eq!(
            (spec.grants.runtime, spec.grants.permissions.clone()),
            (RuntimeKind::Wasm, vec![Permission::ExportsRead])
        );
        assert!(spec.secrets.is_empty());
        assert_eq!((spec.message.tools.ffmpeg.as_str(), spec.message.action.id.as_str()), ("", "hello"));
        // It runs on the code kept at its commit: nothing is compiled again.
        let reports = Mutex::new(Vec::new());
        let said = wasm::run(spec, &as_core::CancelToken::new(), &|stage, _| reports.lock().push(stage.to_owned()));
        assert!(said.is_ok(), "{said:?}");
        assert!(!reports.lock().iter().any(|r| r.starts_with("Getting")), "{:?}", reports.lock());
        assert!(store.data_dir(&id()).join("hello.txt").is_file());

        // A program's run is no WebAssembly run.
        let process = install(&store, &place.unsigned("1.1.0")).unwrap();
        let Err(e) = store.wasm_spec(&process, hello_action()) else { panic!("a program ran in the sandbox") };
        assert_eq!(e.to_string(), "Hello Export isn't a WebAssembly plugin.");
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn a_wasm_package_that_does_not_load_leaves_no_files() {
        let place = Place::new();
        let store = place.store();
        // A component's header and nothing that loads; a core module.
        let damaged = [&b"\0asm\x0d\x00\x01\x00"[..], &[0xff, 0x01]].concat();
        let module = b"\0asm\x01\x00\x00\x00".to_vec();
        for (component, says) in
            [(&damaged, "The plugin's component doesn't load"), (&module, "The plugin's component isn't")]
        {
            let files = wasm_plugin(ID, "1.0.0", component);
            let staged = store.stage(&place.package(&files, None, false));
            assert!(matches!(&staged, Err(PluginError::Invalid(why)) if why.starts_with(says)), "{staged:?}");
            assert_eq!(place.plugins(), [".staging", ".trash"], "the staged package is gone");
            let loaded = store.stage_folder(&place.folder(&format!("dev-{}", Uuid::now_v7().simple()), &files));
            assert!(matches!(&loaded, Err(PluginError::Invalid(why)) if why.starts_with(says)), "{loaded:?}");
            assert_eq!(code(&store, ID), None, "no code is kept");
        }
        assert!(store.list().unwrap().is_empty());

        // A package or folder without its component says so first.
        let plugin_json = wasm_plugin(ID, "1.0.0", b"")[0].clone();
        let Err(e) = store.stage(&place.package(&vec![plugin_json.clone()], None, false)) else { panic!() };
        assert_eq!(e.to_string(), "The plugin package is missing plugin.wasm, which its runtime needs.");
        let Err(e) = store.stage_folder(&place.folder("no-component", &vec![plugin_json])) else { panic!() };
        assert_eq!(e.to_string(), "The plugin folder is missing plugin.wasm, which its runtime needs.");
    }

    /// A component whose top level reads, with a core module that can't
    /// compile in it: a check that compiled would refuse it.
    #[cfg(feature = "wasm")]
    const NEVER_COMPILES: &[u8] = b"\0asm\x0d\x00\x01\x00\x01\x02\xff\xff";

    #[cfg(feature = "wasm")]
    #[test]
    fn nothing_is_compiled_before_the_setting_and_the_person_say_yes() {
        let place = Place::new();
        let store = place.store();
        let files = wasm_plugin(ID, "1.0.0", NEVER_COMPILES);
        let package = place.package(&files, None, false);
        let folder = place.folder("dev-never", &files);
        let tickets = || [store.stage(&package).unwrap(), store.stage_folder(&folder).unwrap()];

        // A compile at any of these would fail here, and say so.
        for t in tickets() {
            assert_eq!((t.compiled.as_ref(), t.component_warnings.len()), (None, 0));
            assert!(matches!(store.commit(t, false, true), Err(PluginError::NotAllowed)));
        }
        for t in tickets() {
            let no = store.commit(t, true, false).unwrap_err();
            assert_eq!(no.to_string(), "Confirm that you want to install it.");
        }
        assert_eq!(code(&store, ID), None);
        assert_eq!(place.plugins(), [".staging", ".trash"]);

        // Past both answers it is compiled, which refuses it: it leaves nothing.
        for t in tickets() {
            let e = store.commit(t, true, true).unwrap_err();
            assert!(e.to_string().starts_with("The plugin's component doesn't load: "), "{e}");
        }
        assert_eq!(code(&store, ID), None);
        assert_eq!(place.plugins(), [".staging", ".trash"]);
        assert!(store.list().unwrap().is_empty());
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn an_update_that_can_not_compile_replaces_nothing() {
        let Some(guest) = guest() else { return };
        let place = Place::new();
        let store = place.store();
        let p = install(&store, &place.package(&wasm_plugin(ID, "1.0.0", &guest), None, false)).unwrap();
        let first = code(&store, ID).unwrap();
        let broken = place.package(&wasm_plugin(ID, "1.1.0", NEVER_COMPILES), None, false);
        assert!(install(&store, &broken).is_err());
        assert_eq!(code(&store, ID).unwrap(), first);
        let kept = store.get(&id()).unwrap();
        assert_eq!((kept.record.version.as_str(), kept.broken.as_deref()), ("1.0.0", None));
        assert_eq!(place.plugins(), holding(&p));
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn a_compile_process_that_can_not_start_fails_the_install() {
        let Some(guest) = guest() else { return };
        let place = Place::new();
        let store = place.store().compile_with(wasm::Compiler::new(place.paths.plugins().join("no-such-program")));
        let t = store.stage(&place.package(&wasm_plugin(ID, "1.0.0", &guest), None, false)).unwrap();
        assert!(store.commit(t, true, true).is_err());
        assert_eq!(code(&store, ID), None);
        assert!(store.list().unwrap().is_empty());
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn a_store_told_nothing_compiles_nothing() {
        let Some(guest) = guest() else { return };
        let place = Place::new();
        let store = Store::open(&place.paths, Arc::clone(&place.db), app(), place.secrets.clone()).unwrap();
        let t = store.stage(&place.package(&wasm_plugin(ID, "1.0.0", &guest), None, false)).unwrap();
        assert_eq!(store.commit(t, true, true).unwrap_err().to_string(), wasm::COMPILE_FAILED);
        assert_eq!(code(&store, ID), None);
        assert!(store.list().unwrap().is_empty());
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn what_the_catalog_pulled_after_its_review_opened_is_not_compiled() {
        let place = Place::new();
        let store = place.store();
        let package = place.package(&wasm_plugin(ID, "1.0.0", NEVER_COMPILES), None, false);
        let stage = || {
            reviewed(&store, &[&package]);
            let t = store.stage(&package).unwrap();
            assert_eq!(t.verdict.trust, Trust::Reviewed);
            t
        };
        let cancel = CancelToken::new();
        // Withdrawn while the review was open: a compile would refuse it for
        // another reason, and leave its work behind in a run of 2 minutes.
        let mut t = stage();
        store.set_catalog(catalog_of(Vec::new(), vec![withdrawal("<=1.0.4")], catalog::now() + DAY));
        let e = store.compile(&mut t, false, false, &cancel).unwrap_err();
        assert!(matches!(e, PluginError::Withdrawn { .. }), "{e:?}");
        assert_eq!(t.compiled, None);
        store.discard(t);
        // No longer reviewed, with the setting off.
        let mut t = stage();
        store.set_catalog(catalog_of(Vec::new(), Vec::new(), catalog::now() + DAY));
        let e = store.compile(&mut t, false, false, &cancel).unwrap_err();
        assert_eq!(e.to_string(), "The catalog changed while this plugin waited. Open it again.");
        store.discard(t);
        assert_eq!(code(&store, ID), None);
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn a_cancelled_compile_keeps_its_ticket_and_compiles_nothing() {
        let Some(guest) = guest() else { return };
        let place = Place::new();
        let store = place.store();
        let mut t = store.stage(&place.package(&wasm_plugin(ID, "1.0.0", &guest), None, false)).unwrap();
        let cancel = CancelToken::new();
        cancel.cancel();
        assert!(matches!(store.compile(&mut t, true, true, &cancel), Err(PluginError::Cancelled)));
        assert_eq!((t.compiled.as_ref(), code(&store, ID)), (None, None));
        // The same ticket installs, and what was compiled isn't compiled again.
        store.compile(&mut t, true, true, &CancelToken::new()).unwrap();
        let file = t.compiled.clone().unwrap();
        assert!(!t.compiles());
        fs::remove_file(&file).unwrap();
        store.compile(&mut t, true, true, &CancelToken::new()).unwrap();
        assert!(!file.exists());
        store.commit(t, true, true).unwrap();
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn files_changed_during_a_compile_are_not_installed() {
        let Some(guest) = guest() else { return };
        let place = Place::new();
        let store = place.store();
        let mut t = store.stage(&place.package(&wasm_plugin(ID, "1.0.0", &guest), None, false)).unwrap();
        store.compile(&mut t, true, true, &CancelToken::new()).unwrap();
        fs::write(t.staged.as_ref().unwrap().join("plugin.json"), b"{}").unwrap();
        assert!(store.commit(t, true, true).is_err());
        assert!(store.list().unwrap().is_empty());
        assert_eq!(code(&store, ID), None);
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn discarding_a_ticket_takes_its_code_but_never_the_installed_versions() {
        let Some(guest) = guest() else { return };
        let place = Place::new();
        let store = place.store();
        let compiled = |files: &Files| {
            let mut t = store.stage(&place.package(files, None, false)).unwrap();
            store.compile(&mut t, true, true, &CancelToken::new()).unwrap();
            assert!(t.compiled.as_ref().unwrap().is_file());
            t
        };

        // Of a plugin that isn't installed, its code and its folder go.
        store.discard(compiled(&wasm_plugin(ID, "1.0.0", &guest)));
        assert_eq!(code(&store, ID), None);

        install(&store, &place.package(&wasm_plugin(ID, "1.0.0", &guest), None, false)).unwrap();
        let first = code(&store, ID).unwrap();
        // An update of the same component shares the installed version's file.
        store.discard(compiled(&wasm_plugin(ID, "1.1.0", &guest)));
        assert_eq!(code(&store, ID).unwrap(), first);
        // Another component's goes, beside it.
        let t = compiled(&wasm_plugin(ID, "1.1.0", &other(&guest, "v2")));
        assert_eq!(code(&store, ID).unwrap().len(), 2);
        store.discard(t);
        assert_eq!(code(&store, ID).unwrap(), first);
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn an_install_that_fails_past_its_compile_leaves_no_code() {
        let Some(guest) = guest() else { return };
        let place = Place::new();
        let store = place.store();
        let t = store.stage(&place.package(&wasm_plugin(ID, "1.0.0", &guest), None, false)).unwrap();
        place.refuse_writes();
        assert!(matches!(store.commit(t, true, true), Err(PluginError::Db(_))));
        assert_eq!(code(&store, ID), None);
        assert_eq!(place.plugins(), [".staging", ".trash"]);

        place.allow_writes();
        install(&store, &place.package(&wasm_plugin(ID, "1.0.0", &guest), None, false)).unwrap();
        let first = code(&store, ID).unwrap();
        let t = store.stage(&place.package(&wasm_plugin(ID, "1.1.0", &other(&guest, "v2")), None, false)).unwrap();
        place.refuse_writes();
        assert!(matches!(store.commit(t, true, true), Err(PluginError::Db(_))));
        assert_eq!(code(&store, ID).unwrap(), first, "the installed version keeps its code");
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn an_update_keeps_only_the_new_compiled_plugin() {
        let Some(guest) = guest() else { return };
        let place = Place::new();
        let store = place.store();
        install(&store, &place.package(&wasm_plugin(ID, "1.0.0", &guest), None, false)).unwrap();
        let first = code(&store, ID).unwrap();
        let t = store.stage(&place.package(&wasm_plugin(ID, "1.1.0", &other(&guest, "v2")), None, false)).unwrap();
        // Until the person agrees, nothing of it is compiled.
        assert_eq!(code(&store, ID).unwrap(), first);
        let p = store.commit(t, true, true).unwrap();
        assert_eq!(code(&store, ID).unwrap().len(), 1);
        assert_ne!(code(&store, ID).unwrap(), first);
        assert_eq!(
            store.wasm_spec(&p, hello_action()).unwrap().component_sha256,
            Some(package::sha256_hex(&other(&guest, "v2")))
        );

        // An update that leaves the sandbox keeps none.
        install(&store, &place.unsigned("1.2.0")).unwrap();
        assert_eq!(code(&store, ID), None);
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn uninstalling_drops_the_compiled_plugin() {
        let Some(guest) = guest() else { return };
        let place = Place::new();
        let store = place.store();
        install(&store, &place.package(&wasm_plugin(ID, "1.0.0", &guest), None, false)).unwrap();
        assert_eq!(code(&store, ID).unwrap().len(), 1);
        store.uninstall(&id(), false).unwrap();
        assert_eq!(code(&store, ID), None);
        assert_eq!(place.plugins(), [".staging", ".trash"]);
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn leftover_compiled_plugins_are_swept() {
        let Some(guest) = guest() else { return };
        let place = Place::new();
        let store = place.store();
        // A package, the code of an update AutoScene ended in the middle of,
        // and what a save or an older AutoScene left beside its code.
        install(&store, &place.package(&wasm_plugin(ID, "1.0.0", &guest), None, false)).unwrap();
        let kept = code(&store, ID).unwrap();
        let mut update =
            store.stage(&place.package(&wasm_plugin(ID, "1.1.0", &other(&guest, "v2")), None, false)).unwrap();
        store.compile(&mut update, true, true, &CancelToken::new()).unwrap();
        update.compiled = None;
        store.discard(update);
        let mine = store.cache_dir(&id());
        let older_engine = mine.join(format!("{}-0123456789abcdef.cwasm", package::sha256_hex(&guest)));
        fs::write(&older_engine, b"old").unwrap();
        // Newer than its code, as a beta sharing this data folder leaves
        // it: still not this AutoScene's.
        let later = std::time::SystemTime::now() + Duration::from_secs(86_400);
        File::options().write(true).open(&older_engine).unwrap().set_modified(later).unwrap();
        fs::write(mine.join(".0190f0e8a3b47c1d.part"), b"part").unwrap();
        fs::create_dir(mine.join("stray")).unwrap();
        assert_eq!(code(&store, ID).unwrap().len(), 5);
        // A loaded folder, with another engine's code of an earlier build,
        // newer than its own.
        let loaded = "dev.autoscene.loaded";
        let dev = place.folder("loaded", &wasm_plugin(loaded, "1.0.0", &guest));
        store.commit(store.stage_folder(&dev).unwrap(), true, true).unwrap();
        let newest = code(&store, loaded).unwrap();
        let rebuilt = store
            .cache_dir(&PluginId::parse(loaded).unwrap())
            .join(format!("{}-0123456789abcdef.cwasm", "a".repeat(64)));
        fs::write(&rebuilt, b"before").unwrap();
        File::options().write(true).open(&rebuilt).unwrap().set_modified(later).unwrap();
        // A program with code left from when it ran in the sandbox, a
        // plugin without a row, and a file that is no plugin's.
        let program = "dev.autoscene.program";
        install(&store, &place.package(&files(&manifest(program, "1.0.0")), None, false)).unwrap();
        for dir in [program, "dev.autoscene.gone"] {
            fs::create_dir_all(place.paths.plugin_code().join(dir)).unwrap();
            fs::write(
                place.paths.plugin_code().join(dir).join(format!("{}-0123456789abcdef.cwasm", "b".repeat(64))),
                b"x",
            )
            .unwrap();
        }
        fs::write(place.paths.plugin_code().join("stray.cwasm"), b"x").unwrap();

        let store = place.store();
        store.remove_leftovers();
        assert_eq!(code(&store, ID).unwrap(), kept);
        assert_eq!(code(&store, loaded).unwrap(), newest);
        assert_eq!(code(&store, program), None);
        let mut left: Vec<String> = fs::read_dir(place.paths.plugin_code())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, [ID, loaded]);
        assert_eq!(fs::read_dir(place.paths.plugins().join(TRASH)).unwrap().count(), 0);
        // What stays runs.
        let p = store.get(&id()).unwrap();
        assert!(
            wasm::run(store.wasm_spec(&p, hello_action()).unwrap(), &as_core::CancelToken::new(), &|_, _| {}).is_ok()
        );
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn a_loaded_folder_that_leaves_the_sandbox_is_broken_until_loaded_again() {
        let Some(guest) = guest() else { return };
        let place = Place::new();
        let store = place.store();
        let dev = place.folder("hello-wasm", &wasm_plugin(ID, "1.0.0", &guest));
        let p = store.commit(store.stage_folder(&dev).unwrap(), true, true).unwrap();
        assert_eq!((p.broken.as_deref(), p.extra.agreed_runtime()), (None, RuntimeKind::Wasm));
        assert_eq!(code(&store, ID).unwrap().len(), 1);

        // Its plugin.json now starts a program, with the person's rights.
        for (path, bytes) in hello("1.0.0") {
            fs::write(dev.join(path), bytes).unwrap();
        }
        let p = store.get(&id()).unwrap();
        assert_eq!((p.broken.as_deref(), p.manifest.is_none()), (Some(RUNTIME_CHANGED), true));
        assert!(store.wasm_spec(&p, hello_action()).is_err());
        assert!(crate::calls::Grants::of(&p).is_err());

        // Loading it again goes by what the person agreed to, not by what
        // its folder says now, and the dialog's yes stands for no typed one.
        let mut t = store.stage_folder(&dev).unwrap();
        assert!(store.widens(&t.manifest).unwrap().runtime.leaves_sandbox);
        t.confirmed = true;
        assert_eq!(store.commit(t, true, false).unwrap_err().to_string(), "Confirm that you want to install it.");

        // Loaded again, the person agreed to that; its code goes.
        let p = store.commit(store.stage_folder(&dev).unwrap(), true, true).unwrap();
        assert_eq!((p.broken.as_deref(), p.extra.agreed_runtime()), (None, RuntimeKind::Process));
        assert_eq!(code(&store, ID), None);
        // And back into the sandbox: the same.
        for (path, bytes) in wasm_plugin(ID, "1.0.0", &guest) {
            fs::write(dev.join(path), bytes).unwrap();
        }
        assert_eq!(store.get(&id()).unwrap().broken.as_deref(), Some(RUNTIME_CHANGED));
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn grants_never_exceed_what_was_agreed() {
        let Some(guest) = guest() else { return };
        let place = Place::new();
        let store = place.store();
        let mut files = wasm_plugin(ID, "1.0.0", &guest);
        let dev = place.folder("hello-wasm", &files);
        store.commit(store.stage_folder(&dev).unwrap(), true, true).unwrap();
        // Its plugin.json asks for more after the person agreed.
        let mut m: Value = serde_json::from_slice(&files[0].1).unwrap();
        m["permissions"] = json!(["exports:read", "exports:write", "project:read", "network"]);
        m["network"] = json!({"hosts": ["discord.com"]});
        files[0].1 = serde_json::to_vec_pretty(&m).unwrap();
        fs::write(dev.join("plugin.json"), &files[0].1).unwrap();
        let p = store.get(&id()).unwrap();
        assert_eq!(p.broken, None);
        assert_eq!(p.manifest.as_ref().unwrap().permissions.len(), 4);
        let spec = store.wasm_spec(&p, hello_action()).unwrap();
        assert_eq!(spec.grants.permissions, [Permission::ExportsRead]);
        assert!(spec.grants.hosts.is_empty());
        // A loaded folder's component is hashed at its run.
        assert_eq!(spec.component_sha256, None);
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
        let author = key_pair();
        let store = place.store();
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
        let package = place.unsigned("1.0.0");
        reviewed(&store, &[&package]);
        let t = store.stage(&package).unwrap();
        assert_eq!(t.verdict.trust, Trust::Reviewed);
        assert_eq!(store.commit(t, false, false).unwrap().record.trust, Trust::Reviewed);
    }

    #[test]
    fn a_pinned_plugin_takes_a_reviewed_update_and_keeps_its_pin() {
        let place = Place::new();
        let (other, author) = (key_pair(), key_pair());
        let store = place.store();
        let p = install(&store, &place.package(&hello("1.0.0"), Some(&author), true)).unwrap();
        let pin = p.record.author_key.clone();
        assert!(pin.is_some());
        // Signed by another key: the catalog's review is what counts.
        let update = place.package(&hello("1.1.0"), Some(&other), false);
        assert!(matches!(store.stage(&update), Err(PluginError::KeyChanged { .. })));
        reviewed(&store, &[&update]);
        let p = install(&store, &update).unwrap();
        assert_eq!((p.record.trust, &p.record.author_key), (Trust::Reviewed, &pin));
        // Reviewed now, it takes no version its author alone signed.
        let e = store.stage(&place.package(&hello("1.2.0"), Some(&author), true)).unwrap_err();
        assert_eq!(e.to_string(), "AutoScene reviewed the installed version of this plugin, and not this one.");

        // An unreviewed plugin takes a signed update, which pins its key
        // (with no catalog, which lists those files).
        store.set_catalog(Arc::new(Catalog::none()));
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
        let author = key_pair();
        let store = place.store();
        // The versions the catalog lists: files of their own.
        let listed = |version: &str| {
            let mut files = hello(version);
            files[1].1 = b"print('reviewed')\n".to_vec();
            place.package(&files, None, false)
        };
        let (first, second) = (listed("1.0.0"), listed("1.1.0"));
        reviewed(&store, &[&first, &second]);
        store.commit(store.stage(&first).unwrap(), false, false).unwrap();
        for update in [place.unsigned("1.1.0"), place.package(&hello("1.1.0"), Some(&author), true)] {
            let e = store.stage(&update).unwrap_err();
            assert_eq!(e.to_string(), "AutoScene reviewed the installed version of this plugin, and not this one.");
        }
        let dev = place.folder("hello-export", &hello("1.1.0"));
        assert!(store.stage_folder(&dev).is_err());

        let p = store.commit(store.stage(&second).unwrap(), false, false).unwrap();
        assert_eq!((p.record.version.as_str(), p.record.trust), ("1.1.0", Trust::Reviewed));
    }

    #[test]
    fn a_package_the_catalog_lists_is_reviewed() {
        let place = Place::new();
        let store = place.store();
        let (package, signed) = (place.unsigned("1.0.0"), place.package(&hello("1.0.0"), Some(&key_pair()), true));
        assert_eq!(store.stage(&package).unwrap().verdict.trust, Trust::Unreviewed);
        assert_eq!(store.stage(&signed).unwrap().verdict.trust, Trust::Pinned);
        reviewed(&store, &[&package]);
        // Unsigned or signed, it is the lock the catalog lists: no pin.
        let reviewed = Verdict { trust: Trust::Reviewed, author_key: None };
        let approval = Approval {
            published: store.catalog().index().unwrap().published,
            repo: "https://github.com/tcmmi/AutoScene".into(),
            commit: "c".repeat(40),
            path: "examples/plugins/hello-export".into(),
        };
        for package in [&signed, &package] {
            let t = store.stage(package).unwrap();
            assert_eq!((&t.verdict, t.approval.as_ref()), (&reviewed, Some(&approval)));
            store.discard(t);
        }
        let p = store.commit(store.stage(&package).unwrap(), false, false).unwrap();
        assert_eq!((p.record.trust, p.record.author_key.as_ref()), (Trust::Reviewed, None));
        assert_eq!(store.get(&id()).unwrap().record.trust, Trust::Reviewed);
        // A loaded folder of the same files has no lock: never reviewed, not
        // even by an entry of the empty lock's hash.
        store.uninstall(&id(), false).unwrap();
        let empty = Entry { lock: package::sha256_hex(b""), ..entry_of(&package) };
        store.set_catalog(catalog_of(vec![entry_of(&package), empty], Vec::new(), catalog::now() + DAY));
        let t = store.stage_folder(&place.folder("hello-export", &hello("1.0.0"))).unwrap();
        assert_eq!((t.verdict.trust, t.approval.is_none()), (Trust::Unreviewed, true));
    }

    #[test]
    fn a_package_the_catalog_lists_under_another_lock_is_not() {
        let place = Place::new();
        let store = place.store();
        let package = place.unsigned("1.0.0");
        let mut changed = hello("1.0.0");
        changed[1].1 = b"print('bye')\n".to_vec();
        let changed = place.package(&changed, None, false);
        reviewed(&store, &[&package]);
        let t = store.stage(&changed).unwrap();
        assert_eq!((t.verdict.trust, t.approval.is_none()), (Trust::Unreviewed, true));
        store.discard(t);
        // Its lock, listed for another version or another plugin, isn't its.
        let entry = entry_of(&package);
        let other = PluginId::parse("dev.autoscene.other").unwrap();
        let elsewhere = [Entry { version: "1.0.1".into(), ..entry.clone() }, Entry { id: other, ..entry }];
        store.set_catalog(catalog_of(elsewhere.to_vec(), Vec::new(), catalog::now() + DAY));
        let t = store.stage(&package).unwrap();
        assert_eq!((t.verdict.trust, t.approval.is_none()), (Trust::Unreviewed, true));
    }

    #[test]
    fn a_catalog_past_its_expiry_reviews_nothing_new() {
        let place = Place::new();
        let store = place.store();
        let (first, update) = (place.unsigned("1.0.0"), place.unsigned("1.1.0"));
        reviewed(&store, &[&first, &update]);
        install(&store, &first).unwrap();
        let ran_out = catalog_of(vec![entry_of(&first), entry_of(&update)], Vec::new(), catalog::now() - 1);
        store.set_catalog(ran_out);
        // What it reviewed stays so; what it lists isn't reviewed any more.
        assert_eq!(store.get(&id()).unwrap().record.trust, Trust::Reviewed);
        let e = store.stage(&update).unwrap_err();
        assert_eq!(e.to_string(), "AutoScene reviewed the installed version of this plugin, and not this one.");
        store.uninstall(&id(), false).unwrap();
        let t = store.stage(&first).unwrap();
        assert_eq!((t.verdict.trust, t.approval.is_none()), (Trust::Unreviewed, true));
    }

    #[test]
    fn a_withdrawn_version_is_refused_and_listed_as_withdrawn() {
        let place = Place::new();
        let store = place.store();
        let (old, listed, fixed) = (place.unsigned("1.0.0"), place.unsigned("1.0.3"), place.unsigned("1.0.5"));
        let p = install(&store, &old).unwrap();
        assert_eq!(p.withdrawn, None);
        let entries = vec![entry_of(&listed), entry_of(&fixed)];
        store.set_catalog(catalog_of(entries, vec![withdrawal("<=1.0.4")], catalog::now() + DAY));
        // Listed as withdrawn, in the catalog's words.
        let listed_now = store.list().unwrap();
        assert_eq!(listed_now[0].withdrawn.as_deref(), Some(LEAKY));
        assert_eq!(store.get(&id()).unwrap().withdrawn.as_deref(), Some(LEAKY));
        // It doesn't install again, not even as the catalog lists it.
        let why = format!("AutoScene withdrew Hello Export. {LEAKY}");
        for package in [&old, &listed] {
            assert_eq!(withdrawn(store.stage(package)), why);
        }
        assert_eq!(place.plugins(), holding(&p), "a refused package leaves nothing");
        // Its fixed version does, reviewed.
        let fixed = install(&store, &fixed).unwrap();
        assert_eq!((fixed.record.trust, fixed.withdrawn.as_deref()), (Trust::Reviewed, None));
        // A broken plugin is matched by its record's version.
        store.uninstall(&id(), false).unwrap();
        store.set_catalog(catalog_of(Vec::new(), Vec::new(), catalog::now() + DAY));
        let p = install(&store, &old).unwrap();
        store.set_catalog(catalog_of(Vec::new(), vec![withdrawal("<=1.0.4")], catalog::now() + DAY));
        fs::remove_dir_all(&p.dir).unwrap();
        let p = store.get(&id()).unwrap();
        assert_eq!((p.broken.as_deref(), p.withdrawn.as_deref()), (Some(MISSING), Some(LEAKY)));
        // It goes as any other.
        store.uninstall(&id(), false).unwrap();
        assert!(store.list().unwrap().is_empty());
        // So is a row of a later AutoScene.
        let later = format!(r#"{{"v":99,"id":"{ID}","version":"1.0.0"}}"#);
        place.db.set_setting(&format!("{ROW}{ID}"), &later).unwrap();
        let p = store.get(&id()).unwrap();
        assert_eq!((p.broken.as_deref(), p.withdrawn.as_deref()), (Some(NEWER), Some(LEAKY)));
    }

    #[test]
    fn withdrawal_holds_for_a_loaded_folder_too() {
        let place = Place::new();
        let store = place.store();
        let dev = place.folder("hello-export", &hello("1.0.0"));
        store.commit(store.stage_folder(&dev).unwrap(), true, true).unwrap();
        store.set_catalog(catalog_of(Vec::new(), vec![withdrawal("*")], catalog::now() + DAY));
        assert_eq!(store.get(&id()).unwrap().withdrawn.as_deref(), Some(LEAKY));
        assert_eq!(withdrawn(store.stage_folder(&dev)), format!("AutoScene withdrew Hello Export. {LEAKY}"));
        // Also once the index ran out, or its file is gone: the state keeps it.
        let ran_out = catalog_of(Vec::new(), vec![withdrawal("*")], catalog::now() - 1);
        let lost = Arc::new(Catalog::new(None, ran_out.state().clone()));
        for catalog in [ran_out, lost] {
            store.set_catalog(catalog);
            assert_eq!(store.get(&id()).unwrap().withdrawn.as_deref(), Some(LEAKY));
            assert!(matches!(store.stage_folder(&dev), Err(PluginError::Withdrawn { .. })));
        }
        assert!(dev.join("plugin.json").is_file());
        // The version its plugin.json names now counts, not its record's.
        store.set_catalog(catalog_of(Vec::new(), vec![withdrawal("<=1.0.4")], catalog::now() + DAY));
        for (version, withdrawn) in [("2.0.0", None), ("1.0.0", Some(LEAKY))] {
            fs::write(dev.join("plugin.json"), &hello(version)[0].1).unwrap();
            let p = store.get(&id()).unwrap();
            assert_eq!(p.record.version, "1.0.0");
            assert_eq!((p.manifest.unwrap().version, p.withdrawn.as_deref()), (version.to_owned(), withdrawn));
        }
    }

    #[test]
    fn a_reviewed_version_pins_the_key_the_catalog_names() {
        let place = Place::new();
        let store = place.store();
        let (author, catalogued) = (key_pair(), key_pair());
        let key = |pair: &KeyPair| {
            let base64 = pair.pk.to_base64();
            AuthorKey { key_id: trust::key_id(&base64).unwrap(), base64 }
        };
        let p = install(&store, &place.package(&hello("1.0.0"), Some(&author), true)).unwrap();
        assert_eq!(p.record.author_key, Some(key(&author)));
        // Unsigned, and listed with another author key: that key is the pin.
        let update = place.unsigned("1.1.0");
        let entry = Entry { author_key: Some(catalogued.pk.to_base64()), ..entry_of(&update) };
        store.set_catalog(catalog_of(vec![entry], Vec::new(), catalog::now() + DAY));
        let t = store.stage(&update).unwrap();
        assert_eq!(t.verdict, Verdict { trust: Trust::Reviewed, author_key: Some(key(&catalogued)) });
        let p = store.commit(t, false, false).unwrap();
        assert_eq!((p.record.trust, p.record.author_key), (Trust::Reviewed, Some(key(&catalogued))));
    }

    #[test]
    fn a_pinned_plugin_takes_an_unsigned_version_the_catalog_reviewed() {
        let place = Place::new();
        let store = place.store();
        let author = key_pair();
        let p = install(&store, &place.package(&hello("1.0.0"), Some(&author), true)).unwrap();
        let pin = p.record.author_key.clone();
        let update = place.unsigned("1.1.0");
        assert!(matches!(store.stage(&update), Err(PluginError::KeyChanged { .. })));
        reviewed(&store, &[&update]);
        let p = store.commit(store.stage(&update).unwrap(), false, false).unwrap();
        assert_eq!((p.record.trust, p.record.author_key), (Trust::Reviewed, pin));
    }

    #[test]
    fn a_review_that_the_catalog_dropped_while_it_waited_is_not_installed() {
        let place = Place::new();
        let store = place.store();
        let package = place.unsigned("1.0.0");
        reviewed(&store, &[&package]);
        // The catalog no longer lists it: even with the setting and a yes.
        let t = store.stage(&package).unwrap();
        assert!(t.approval.is_some());
        store.set_catalog(Arc::new(Catalog::none()));
        let e = store.commit(t, true, true).unwrap_err();
        assert_eq!(e.to_string(), "The catalog changed while this plugin waited. Open it again.");
        assert!(store.list().unwrap().is_empty());
        assert_eq!(place.plugins(), [".staging", ".trash"]);
        // The catalog withdrew it.
        reviewed(&store, &[&package]);
        let t = store.stage(&package).unwrap();
        store.set_catalog(catalog_of(vec![entry_of(&package)], vec![withdrawal("*")], catalog::now() + DAY));
        assert!(matches!(store.commit(t, true, true), Err(PluginError::Withdrawn { .. })));
        assert!(store.list().unwrap().is_empty());
        assert_eq!(place.plugins(), [".staging", ".trash"]);
        // A newer index that still lists it.
        reviewed(&store, &[&package]);
        let t = store.stage(&package).unwrap();
        reviewed(&store, &[&place.unsigned("1.1.0"), &package]);
        assert_eq!(store.commit(t, false, false).unwrap().record.trust, Trust::Reviewed);
    }

    #[test]
    fn a_dropped_review_is_the_reason_whatever_else_the_package_says() {
        let place = Place::new();
        let store = place.store();
        let dropped = "The catalog changed while this plugin waited. Open it again.";
        // An unsigned version of a pinned plugin, which only the review let through.
        let author = key_pair();
        install(&store, &place.package(&hello("1.0.0"), Some(&author), true)).unwrap();
        let update = place.unsigned("1.1.0");
        reviewed(&store, &[&update]);
        let t = store.stage(&update).unwrap();
        store.set_catalog(Arc::new(Catalog::none()));
        assert_eq!(store.commit(t, true, true).unwrap_err().to_string(), dropped);
        assert_eq!(store.get(&id()).unwrap().record.version, "1.0.0");
        // A package whose signature is another lock's: unread while it was reviewed.
        store.uninstall(&id(), false).unwrap();
        let package = place.package(&hello("1.0.0"), Some(&author), true);
        reviewed(&store, &[&package]);
        let mut t = store.stage(&package).unwrap();
        let other = b"another lock";
        let comment = trust::trusted_comment(&id(), "1.0.0", &package::sha256_hex(other));
        let stale = minisign::sign(Some(&author.pk), &author.sk, &other[..], Some(&comment), None).unwrap();
        t.signature = Some(stale.into_string());
        store.set_catalog(Arc::new(Catalog::none()));
        assert_eq!(store.commit(t, true, true).unwrap_err().to_string(), dropped);
        assert!(store.list().unwrap().is_empty());
        assert_eq!(place.plugins(), [".staging", ".trash"]);
    }

    #[test]
    fn catalog_rows_never_read_as_plugins() {
        let place = Place::new();
        let store = place.store();
        // The catalog's rows (the app's, and a debug build's own) sort
        // outside the registry's and the settings'.
        for name in ["plugins_catalog", "plugins_catalog.dev"] {
            for prefix in [ROW, SETTINGS_ROW, EXTRA_ROW] {
                assert!(!name.starts_with(prefix));
            }
            assert!(name > "plugin/");
        }
        let state = CatalogState { withdrawn: vec![withdrawal("*")], ..CatalogState::default() };
        let row = serde_json::to_string(&state).unwrap();
        place.db.set_setting("plugins_catalog", &row).unwrap();
        place.db.set_setting("plugins_catalog.dev", &row).unwrap();
        assert!(store.list().unwrap().is_empty());
        install(&store, &place.unsigned("1.0.0")).unwrap();
        let listed = store.list().unwrap();
        assert_eq!(listed.iter().map(|p| p.record.id.as_str()).collect::<Vec<_>>(), [ID]);
        // Its withdrawals count only once the catalog is in hand.
        assert_eq!(listed[0].withdrawn, None);
        store.uninstall(&id(), false).unwrap();
        assert_eq!(place.db.setting("plugins_catalog").unwrap().as_deref(), Some(row.as_str()));
        assert!(store.list().unwrap().is_empty());
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
        let wrong_banner = with_banner("art.jpg", b"\x89PNG\r\n\x1a\n");
        for (files, why) in [
            (no_lock, "The plugin package is missing main.py.lock, which its command needs."),
            (wrong_icon, "The plugin's icon icon.png isn't a PNG image."),
            (wrong_banner, "The plugin's banner art.jpg isn't a JPEG image."),
        ] {
            let e = store.stage(&place.package(&files, None, false)).unwrap_err();
            assert_eq!(e.to_string(), why);
            assert_eq!(place.plugins(), [".staging", ".trash"]);
        }
    }

    /// [`hello`] at `version` that asks for `permissions` and may talk to
    /// `hosts`.
    fn asking(version: &str, permissions: &[&str], hosts: &[&str]) -> Files {
        let mut m = manifest(ID, version);
        m["permissions"] = json!(permissions);
        if !hosts.is_empty() {
            m["network"] = json!({"hosts": hosts});
        }
        files(&m)
    }

    /// Hello Export at `version` as a plugin of pages only.
    fn pages_only(version: &str) -> Files {
        let mut m = manifest(ID, version);
        m["manifestVersion"] = json!(2);
        m["runtime"] = json!({"type": "none"});
        m["contributes"] = json!({"pages": [{"id": "main", "title": "Main", "entry": "ui/index.html"}]});
        let page = ("ui/index.html".to_owned(), b"<!doctype html>".to_vec());
        vec![("plugin.json".to_owned(), serde_json::to_vec_pretty(&m).unwrap()), page]
    }

    /// Hello Export at `version` with a program it keeps running while its
    /// page is open.
    fn serving(version: &str) -> Files {
        let mut m = manifest(ID, version);
        m["manifestVersion"] = json!(2);
        m["engines"]["protocol"] = json!(2);
        m["runtime"]["type"] = json!("service");
        m["contributes"]["pages"] = json!([{"id": "main", "title": "Main", "entry": "ui/index.html"}]);
        let mut files = files(&m);
        files.push(("ui/index.html".to_owned(), b"<!doctype html>".to_vec()));
        files
    }

    #[test]
    fn a_reviewed_update_that_may_do_more_takes_a_yes_the_page_cant_give() {
        let place = Place::new();
        let store = place.store();
        let net = ["exports:read", "network"];
        #[cfg_attr(not(feature = "wasm"), expect(unused_mut))]
        let mut cases = vec![
            ("a permission", hello("1.0.0"), asking("1.1.0", &["exports:read", "project:read"], &[])),
            ("a host", asking("1.0.0", &net, &["a.example"]), asking("1.1.0", &net, &["a.example", "b.example"])),
            ("a program", pages_only("1.0.0"), hello("1.1.0")),
            ("a program it keeps running", hello("1.0.0"), serving("1.1.0")),
        ];
        #[cfg(feature = "wasm")]
        if let Some(guest) = guest() {
            cases.push(("a program, out of the sandbox", wasm_plugin(ID, "1.0.0", &guest), hello("1.1.0")));
            cases.push(("a service, out of the sandbox", wasm_plugin(ID, "1.0.0", &guest), serving("1.1.0")));
        }
        for (what, before, after) in cases {
            let (old, new) = (place.package(&before, None, false), place.package(&after, None, false));
            reviewed(&store, &[&old, &new]);
            let installed = store.commit(store.stage(&old).unwrap(), false, false).unwrap();
            // Whatever the page says: neither is the person's yes.
            for page in [false, true] {
                let t = store.stage(&new).unwrap();
                assert_eq!(t.verdict.trust, Trust::Reviewed, "{what}");
                assert!(store.widens(&t.manifest).unwrap().any(), "{what}");
                let e = store.commit(t, page, page).unwrap_err();
                assert_eq!(e.to_string(), "This update asks for more than you agreed to. Open it again.", "{what}");
                let now = store.get(&id()).unwrap();
                assert_eq!(now.record.version, "1.0.0", "{what}");
                assert_eq!(now.extra.agreed_runtime(), installed.extra.agreed_runtime(), "{what}");
                assert_eq!(place.plugins(), holding(&installed), "{what}: a refused update leaves nothing");
            }
            let mut t = store.stage(&new).unwrap();
            t.confirmed = true;
            let p = store.commit(t, false, false).unwrap();
            let runs = p.manifest.as_ref().unwrap().runtime.kind;
            assert_eq!((p.record.version.as_str(), p.extra.agreed_runtime()), ("1.1.0", runs), "{what}");
            store.uninstall(&id(), false).unwrap();
        }
    }

    #[test]
    fn a_reviewed_update_that_may_do_no_more_takes_no_such_yes() {
        let place = Place::new();
        let store = place.store();
        #[cfg_attr(not(feature = "wasm"), expect(unused_mut))]
        let mut cases = vec![
            ("the same", hello("1.0.0"), hello("1.1.0")),
            ("a permission less", asking("1.0.0", &["exports:read", "project:read"], &[]), hello("1.1.0")),
            ("a program it no longer keeps running", serving("1.0.0"), hello("1.1.0")),
            ("no program", hello("1.0.0"), pages_only("1.1.0")),
        ];
        #[cfg(feature = "wasm")]
        if let Some(guest) = guest() {
            cases.push(("into the sandbox", hello("1.0.0"), wasm_plugin(ID, "1.1.0", &guest)));
            cases.push(("pages out of the sandbox", wasm_plugin(ID, "1.0.0", &guest), pages_only("1.1.0")));
        }
        for (what, before, after) in cases {
            let (old, new) = (place.package(&before, None, false), place.package(&after, None, false));
            reviewed(&store, &[&old, &new]);
            store.commit(store.stage(&old).unwrap(), false, false).unwrap();
            let p = store.commit(store.stage(&new).unwrap(), false, false).unwrap();
            assert_eq!(p.record.version, "1.1.0", "{what}");
            store.uninstall(&id(), false).unwrap();
        }
    }

    #[test]
    fn a_reviewed_update_goes_by_the_version_installed_when_it_lands() {
        let place = Place::new();
        let store = place.store();
        let both = ["exports:read", "project:read"];
        // What it replaced when it was opened, what replaced that meanwhile
        // without a question, and the update: no more than the first, more
        // than the second.
        #[cfg_attr(not(feature = "wasm"), expect(unused_mut))]
        let mut cases =
            vec![("a permission", asking("1.0.0", &both, &[]), hello("1.1.0"), asking("1.2.0", &both, &[]))];
        #[cfg(feature = "wasm")]
        if let Some(guest) = guest() {
            cases.push(("the sandbox", hello("1.0.0"), wasm_plugin(ID, "1.1.0", &guest), hello("1.2.0")));
        }
        for (what, first, meanwhile, update) in cases {
            let packages = [&first, &meanwhile, &update].map(|files| place.package(files, None, false));
            reviewed(&store, &[&packages[0], &packages[1], &packages[2]]);
            store.commit(store.stage(&packages[0]).unwrap(), false, false).unwrap();
            let t = store.stage(&packages[2]).unwrap();
            assert!(!store.widens(&t.manifest).unwrap().any(), "{what}");
            store.commit(store.stage(&packages[1]).unwrap(), false, false).unwrap();
            let e = store.commit(t, false, false).unwrap_err();
            assert_eq!(e.to_string(), "This update asks for more than you agreed to. Open it again.", "{what}");
            assert_eq!(store.get(&id()).unwrap().record.version, "1.1.0", "{what}");
            store.uninstall(&id(), false).unwrap();
        }
    }

    #[test]
    fn a_review_of_an_update_doesnt_land_as_a_first_install() {
        let place = Place::new();
        let store = place.store();
        #[cfg_attr(not(feature = "wasm"), expect(unused_mut))]
        let mut cases = vec![
            ("a permission", hello("1.0.0"), asking("1.1.0", &["exports:read", "project:read"], &[])),
            ("nothing more", hello("1.0.0"), hello("1.1.0")),
        ];
        #[cfg(feature = "wasm")]
        if let Some(guest) = guest() {
            cases.push(("out of the sandbox", wasm_plugin(ID, "1.0.0", &guest), hello("1.1.0")));
        }
        for (what, before, after) in cases {
            let (old, new) = (place.package(&before, None, false), place.package(&after, None, false));
            reviewed(&store, &[&old, &new]);
            for confirmed in [false, true] {
                store.commit(store.stage(&old).unwrap(), false, false).unwrap();
                let mut t = store.stage(&new).unwrap();
                t.confirmed = confirmed;
                assert!(t.previous.is_some(), "{what}");
                store.uninstall(&id(), false).unwrap();
                let e = store.commit(t, false, false).unwrap_err();
                let said = "This plugin was uninstalled while its update waited. Open it again.";
                assert_eq!(e.to_string(), said, "{what}");
                assert!(store.get(&id()).is_err(), "{what}");
            }
            // Opened again, it is a first install: nothing is known of what was agreed to.
            let p = store.commit(store.stage(&new).unwrap(), false, false).unwrap();
            assert_eq!(p.record.version, "1.1.0", "{what}");
            store.uninstall(&id(), false).unwrap();
        }
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn a_package_autoscene_hasnt_reviewed_keeps_its_own_gate_when_it_leaves_the_sandbox() {
        let Some(guest) = guest() else { return };
        let place = Place::new();
        let store = place.store();
        install(&store, &place.package(&wasm_plugin(ID, "1.0.0", &guest), None, false)).unwrap();
        let update = place.unsigned("1.1.0");
        let t = store.stage(&update).unwrap();
        assert_eq!(t.verdict.trust, Trust::Unreviewed);
        assert!(store.widens(&t.manifest).unwrap().runtime.leaves_sandbox);
        assert!(matches!(store.commit(t, false, true), Err(PluginError::NotAllowed)));
        let t = store.stage(&update).unwrap();
        assert_eq!(store.commit(t, true, false).unwrap_err().to_string(), "Confirm that you want to install it.");
        assert_eq!(store.get(&id()).unwrap().extra.agreed_runtime(), RuntimeKind::Wasm);
        let p = install(&store, &update).unwrap();
        assert_eq!((p.record.version.as_str(), p.extra.agreed_runtime()), ("1.1.0", RuntimeKind::Process));
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

    /// [`hello`] in manifest version 2, with the banner `path` of `bytes`.
    fn with_banner(path: &str, bytes: &[u8]) -> Files {
        let mut m = manifest(ID, "1.0.0");
        m["manifestVersion"] = json!(2);
        m["banner"] = json!(path);
        let mut files = files(&m);
        files.push((path.into(), bytes.to_vec()));
        files
    }

    #[test]
    fn the_icon_reads_as_before() {
        let place = Place::new();
        let store = place.store();
        let mut m = manifest(ID, "1.0.0");
        m["manifestVersion"] = json!(2);
        m["icon"] = json!("icon.svg");
        m["banner"] = json!("art.png");
        // A banner the size of no icon.
        let mut art = b"\x89PNG\r\n\x1a\n".to_vec();
        art.resize(package::MAX_ICON_PNG as usize + 1, 0);
        let mut both = files(&m);
        both.push(("icon.svg".into(), b"<svg/>".to_vec()));
        both.push(("art.png".into(), art.clone()));
        let p = install(&store, &place.package(&both, None, false)).unwrap();
        assert_eq!(store.icon_data_url(&p).as_deref(), Some("data:image/svg+xml;base64,PHN2Zy8+"));
        assert_eq!(store.picture_data_url(&p, "icon.svg", Picture::Icon), store.icon_data_url(&p));
        // Each picture by its own rules: an SVG is no banner, and the
        // banner is too large for an icon.
        assert_eq!(store.picture_data_url(&p, "icon.svg", Picture::Banner), None);
        assert_eq!(store.picture_data_url(&p, "art.png", Picture::Icon), None);
        let banner = format!("data:image/png;base64,{}", STANDARD.encode(&art));
        assert_eq!(store.banner_data_url(&p), Some(banner));
    }

    #[test]
    fn the_banner_shows_only_while_its_bytes_match_the_lock() {
        let place = Place::new();
        let store = place.store();
        let jpeg = b"\xFF\xD8\xFF\xE0 art";
        let p = install(&store, &place.package(&with_banner("art.jpg", jpeg), None, false)).unwrap();
        let shown = format!("data:image/jpeg;base64,{}", STANDARD.encode(jpeg));
        assert_eq!(store.banner_data_url(&p).as_ref(), Some(&shown));
        // Bytes of its size, but not the listed ones.
        fs::write(p.dir.join("art.jpg"), b"\xFF\xD8\xFF\xE0 Art").unwrap();
        assert_eq!(store.banner_data_url(&p), None);
        fs::write(p.dir.join("art.jpg"), jpeg).unwrap();
        assert_eq!(store.banner_data_url(&p), Some(shown));
        // A plugin without one has none.
        store.uninstall(&id(), false).unwrap();
        let p = install(&store, &place.unsigned("1.0.0")).unwrap();
        assert_eq!((store.banner_data_url(&p), store.banner_hash(&p)), (None, None));

        // A loaded folder's, as it is, while it is a banner's size.
        store.uninstall(&id(), false).unwrap();
        let webp = b"RIFF\x04\0\0\0WEBPVP8X";
        let dev = place.folder("hello-export", &with_banner("art.webp", webp));
        let p = store.commit(store.stage_folder(&dev).unwrap(), true, true).unwrap();
        let shown = format!("data:image/webp;base64,{}", STANDARD.encode(webp));
        assert_eq!(store.banner_data_url(&p), Some(shown));
        let mut big = webp.to_vec();
        big.resize(package::MAX_BANNER as usize + 1, 0);
        fs::write(dev.join("art.webp"), &big).unwrap();
        assert_eq!(store.banner_data_url(&p), None);
        assert_eq!(store.banner_hash(&p), None);
    }

    #[test]
    fn the_banner_hash_is_the_locks_and_changes_with_a_folders_file() {
        let place = Place::new();
        let store = place.store();
        let png = b"\x89PNG\r\n\x1a\nart";
        let p = install(&store, &place.package(&with_banner("art.png", png), None, false)).unwrap();
        let listed = package::sha256_hex(png);
        assert_eq!(store.banner_hash(&p).as_ref(), Some(&listed));
        // Its file changed: the hash stays the lock's, and its picture is gone.
        fs::write(p.dir.join("art.png"), b"\x89PNG\r\n\x1a\nArt").unwrap();
        assert_eq!(store.banner_hash(&p), Some(listed));
        assert_eq!(store.banner_data_url(&p), None);

        // A loaded folder's: its file's, as it is now.
        store.uninstall(&id(), false).unwrap();
        let dev = place.folder("hello-export", &with_banner("art.png", png));
        let p = store.commit(store.stage_folder(&dev).unwrap(), true, true).unwrap();
        assert_eq!(store.banner_hash(&p), Some(package::sha256_hex(png)));
        let new = b"\x89PNG\r\n\x1a\nnew art";
        fs::write(dev.join("art.png"), new).unwrap();
        assert_eq!(store.banner_hash(&p), Some(package::sha256_hex(new)));
        fs::write(dev.join("art.png"), b"GIF89a").unwrap();
        assert_eq!(store.banner_hash(&p), None);
    }

    #[test]
    fn a_folders_picture_is_never_read_through_a_link_or_from_outside_it() {
        let place = Place::new();
        let store = place.store();
        let png = b"\x89PNG\r\n\x1a\nart";
        let mut files = with_banner("art/banner.png", png);
        let banner = files.pop().unwrap();
        let dev = place.folder("hello-export", &files);
        fs::create_dir(dev.join("art")).unwrap();
        fs::write(dev.join(&banner.0), &banner.1).unwrap();
        let p = store.commit(store.stage_folder(&dev).unwrap(), true, true).unwrap();
        assert_eq!(store.banner_hash(&p), Some(package::sha256_hex(png)));

        // Its art folder became a link to one outside it, with the same file.
        let elsewhere = place.tmp.path().join("elsewhere");
        fs::create_dir(&elsewhere).unwrap();
        fs::write(elsewhere.join("banner.png"), png).unwrap();
        fs::remove_dir_all(dev.join("art")).unwrap();
        link_folder(&elsewhere, &dev.join("art"));
        assert!(fs::read(dev.join("art/banner.png")).is_ok());
        assert_eq!((store.banner_data_url(&p), store.banner_hash(&p)), (None, None));
        // Nor a path no package may name, to a picture beside the folder.
        fs::write(place.tmp.path().join("beside.png"), png).unwrap();
        assert_eq!(store.picture_data_url(&p, "../beside.png", Picture::Banner), None);
    }

    /// Links `link` to the folder `target`: a symlink on Linux and macOS, a
    /// junction on Windows, which needs no administrator (its symlinks do).
    fn link_folder(target: &Path, link: &Path) {
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).unwrap();
        #[cfg(windows)]
        {
            // mklink takes a "/" in a path for an option.
            let (link, target): (PathBuf, PathBuf) = (link.components().collect(), target.components().collect());
            let made = std::process::Command::new("cmd")
                .arg("/C")
                .arg("mklink")
                .arg("/J")
                .arg(link)
                .arg(target)
                .output()
                .unwrap();
            assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stderr));
        }
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
        let locked = Store::open(&place.paths, Arc::clone(&place.db), app(), Arc::new(Locked));
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
        let locked = Store::open(&place.paths, Arc::clone(&place.db), app(), Arc::new(Locked));
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
        let locked = Store::open(&none.paths, Arc::clone(&none.db), app(), Arc::new(Locked));
        let locked = locked.unwrap();
        install(&locked, &none.package(&token, None, false)).unwrap();
        assert_eq!(locked.secret(&id(), "token").unwrap(), None);
        locked.uninstall(&id(), false).unwrap();
        assert_eq!(none.db.setting(KEYCHAIN_ROW).unwrap(), None);
    }
}

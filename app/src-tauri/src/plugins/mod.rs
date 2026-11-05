//! The plugin host: the installed plugins ([`Store`]) as the app shows them
//! ([`PluginInfo`]), the packages checked and waiting for the person's yes
//! (tickets), and plugin runs as jobs ([`queue_run`]).
//!
//! A run is a job of its own (`JobKind::Plugin`) on `Class::Plugin`, or on
//! `Class::Analysis` when the plugin uses the GPU, which the engine needs
//! too. It never carries a media id: the episode lists would show it as that
//! episode's work. An export action's run carries its export's id. A plugin
//! runs only while it is active ([`active`]): turned on, not broken, and
//! reviewed by AutoScene unless the setting lets the others run. That is
//! checked when a run is queued and again when it starts, so a run queued
//! before the plugin (or the setting) went off fails instead; one running
//! or still waiting then is stopped ([`PluginHost::stop`],
//! [`PluginHost::allow_changed`]). A loaded folder is never reviewed,
//! whatever its row says. A plugin runs once at a time, and a run that
//! starts while the plugin is installed, updated or uninstalled waits for
//! that ([`PluginHost::hold`]); a change is refused while a run of it runs
//! ([`PluginHost::change`]).
//!
//! An export that finished whole queues each export action that active
//! plugins turned on for after every export ([`queue_after_export`]), off
//! the export's job: a plugin never fails or holds up an export. An export
//! that failed, or made only some of its clips, queues those of them that
//! say they run then too (`afterFailed`, [`export_failed`]). A project whose
//! last analysis of a batch ended (no indexing, analysis or regrouping of it
//! left) queues the events turned on for that ([`batch_job_ended`]). Both
//! come from the job runner's listener ([`job_ended`]), which only hands
//! them on. Queued runs aren't kept anywhere: one still waiting at quit is
//! lost.
//!
//! A run of a plugin that speaks protocol 2 may call AutoScene for its
//! project, its export and its settings, and reach the hosts it names on
//! the internet ([`calls`]).
//!
//! A WebAssembly plugin runs in AutoScene's sandbox instead of as a program
//! ([`as_plugin::wasm`]): it gets what its grants give it, the values of its
//! secrets read before it starts (no keychain call happens while it runs),
//! and never FFmpeg, so a run of it doesn't wait for the app's tools. Its
//! runs are jobs as a program's are: Stop, turning it off, uninstalling it
//! and the setting going off cancel it, and it is ended 5 seconds later.
//!
//! A page of such a plugin may start its actions (`actions.run`): each is
//! queued as the person's own runs are, shown in Activity and stopped with
//! them, in the project the page is open in, and gets what the page handed
//! it as its `input`. The same run with the same input waits once. Only a
//! program of protocol 2, or a WebAssembly plugin, is started by a page: a
//! program of protocol 1 would read a trigger it doesn't know.
//!
//! A plugin's pages open in a frame of the app's page, each as a session
//! the `asp` scheme serves ([`pages`]). A page opens only while its plugin
//! is active, and closes, saying why, when the plugin is turned off or
//! changes, and when the setting goes off for one AutoScene hasn't reviewed.
//! A page opens under the busy lock, and pages close under it: one that
//! was opening as its plugin or the setting went off is refused or closed.
//! A page calls AutoScene through its session ([`calls::page_call`]).
//!
//! A plugin whose runtime is a service keeps its program running while one
//! of its pages is open, which answers the pages' requests ([`services`]).
//! A change of the plugin stops it before its files change, and so do
//! turning it off, the setting going off for one AutoScene hasn't reviewed,
//! and quitting ([`PluginHost::shutdown`]).
//!
//! A uv script runs through AutoScene's own uv ([`as_plugin::uv`]): the
//! first run or service start that needs it gets it ([`GetUv`]), and says
//! so ([`GETTING_UV`]). The large files a plugin names come next, the same
//! way, each checked by its hash ([`GetResources`], [`as_plugin::resources`]):
//! a run says "Getting files for" its plugin, a service's page
//! [`GETTING_FILES`]; and before the first hash of kept ones in a process
//! (a minute for 8 GiB from a cold disk, which a cancel ends), "Checking
//! files for" it, or [`CHECKING_FILES`]. They go with the plugin's uninstall
//! ([`PluginHost::uninstall`]), and at the next start once no installed
//! version names them ([`PluginHost::prune_resources`]).
//!
//! AutoScene's catalog says which plugins it reviewed and which it withdrew
//! ([`catalog`]). It is read before the window opens and checked at start
//! and every day; a plugin it newly withdraws stops as one turned off does
//! ([`PluginHost::stop_withdrawn`]).
//!
//! A plugin's shot filters judge a character's shots for their scenepack,
//! an episode per run, in a chain of runs that lets other runs in between
//! ([`filters`]).
//!
//! A plugin's pack actions save a character's scenepack as one file, where
//! the person chose, before anything is rendered ([`pack_actions`]).

pub mod calls;
pub mod catalog;
pub mod filters;
pub mod opened;
pub mod pack_actions;
pub mod pages;
pub mod services;

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use as_api::{ExportStatus, ExportSummary, JobInfo, JobKind, JobState};
use as_core::{AppPaths, CancelToken, CharacterId, ExportId, JobId, ProjectId};
use as_jobs::{Class, JobCtx, JobError, JobResult, JobRunner, JobSpec};
use as_plugin::net::{Transport, UreqTransport};
use as_plugin::pages::{Mount, Pages};
use as_plugin::protocol::{ClipContext, EpisodeCounts, EventContext, ExportContext, ProjectContext, RunMessage};
use as_plugin::service::STOP_GRACE;
use as_plugin::store::RECORD_VERSION;
use as_plugin::wasm;
use as_plugin::{
    ActionContext, ActionKind, Approval, Caller, CommandSpec, EventKind, Extra, Grants, HostCalls, HostMethod,
    Installed, Launch, Manifest, OsKeyring, PackActionSpec, PageSpec, Permission, PluginError, PluginId, PluginResult,
    Record, Resource, Resources, RunSpec, RuntimeChange, RuntimeKind, SettingSpec, Source, Store, Ticket, Trigger,
    Trust, Widens, package, runtime,
};
use as_project::{Library, Project, ProjectError, ProjectResult};
use parking_lot::{Condvar, Mutex};
use serde::Serialize;
use serde_json::Value;
use specta::Type;
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::events::{PageEvent, PluginCompiling, PluginPageEvent};
use crate::state::AppState;
use crate::work::job_error;
use catalog::CatalogHost;
use services::{Services, StartService};

/// How long a checked package waits for the person's yes. Its unpacked
/// files go once it has waited longer, at the next package checked.
pub const TICKET_TTL: Duration = Duration::from_secs(600);

/// How often a run waiting for its turn ([`PluginHost::hold`]) looks
/// whether it was cancelled.
const WAIT_TICK: Duration = Duration::from_millis(50);

/// How many runs of a plugin may wait or run before its pages queue no
/// more: a page can't fill Activity.
const MAX_PAGE_RUNS: usize = 16;

/// What a run, or a service's start, says while it gets uv.
pub const GETTING_UV: &str = "Getting uv";

/// What a service's start says while it gets its plugin's large files (a
/// run names the plugin: "Getting files for Big Model").
pub const GETTING_FILES: &str = "Getting files";

/// What a service's start says while it hashes the large files its plugin
/// kept, the first time in a process (a run names the plugin: "Checking
/// files for Big Model").
pub const CHECKING_FILES: &str = "Checking files";

/// Tells the app's page what became of one of its open plugin pages.
pub type PageEvents = Box<dyn Fn(PluginPageEvent) + Send + Sync>;

/// Told the ticket id of a compile that starts ([`PluginHost::compile`]).
pub type CompileTold = Box<dyn Fn(&str) + Send + Sync>;

/// Where the uv of uv scripts comes from: AutoScene's own ([`ManagedUv`]),
/// or a test's stand-in.
pub trait GetUv: Send + Sync {
    /// uv, while it is here and still the pinned file.
    fn installed(&self) -> Option<PathBuf>;
    /// Gets uv (at once when it is here): `progress(done, total)` as its
    /// bytes come; `cancel` ends it.
    fn install(&self, cancel: &CancelToken, progress: &mut dyn FnMut(u64, u64)) -> PluginResult<PathBuf>;
}

/// AutoScene's own uv, pinned, kept in the data folder's `runtimes`.
pub struct ManagedUv(pub PathBuf);

impl GetUv for ManagedUv {
    fn installed(&self) -> Option<PathBuf> {
        as_plugin::uv::installed(&self.0)
    }

    fn install(&self, cancel: &CancelToken, progress: &mut dyn FnMut(u64, u64)) -> PluginResult<PathBuf> {
        as_plugin::uv::install(&self.0, cancel, progress)
    }
}

/// Where the large files plugins name come from and are kept: AutoScene's
/// cache ([`Resources`]), or a test's stand-in. One for the app, so a file
/// is hashed once per process.
pub trait GetResources: Send + Sync {
    /// The folder of `id`'s files, which its program gets.
    fn dir(&self, id: &PluginId) -> PathBuf;
    /// The ones of `rs` not kept yet (a kept file is hashed the first time
    /// this process asks, until `cancel` fires).
    fn missing<'a>(&self, id: &PluginId, rs: &'a [Resource], cancel: &CancelToken) -> PluginResult<Vec<&'a Resource>>;
    /// Whether [`GetResources::missing`] would hash a kept file of `rs` now:
    /// by sizes and times alone.
    fn needs_check(&self, id: &PluginId, rs: &[Resource]) -> bool;
    /// What getting the files of `rs` would download, by their sizes and
    /// marks alone: for the review, which hashes nothing.
    fn to_get(&self, id: &PluginId, rs: &[Resource]) -> u64;
    /// Gets those of `rs` not kept yet, for the plugin named `name`:
    /// `progress(done, total)` as their bytes come; `cancel` ends it.
    fn fetch(
        &self,
        id: &PluginId,
        name: &str,
        rs: &[Resource],
        cancel: &CancelToken,
        progress: &mut dyn FnMut(u64, u64),
    ) -> PluginResult<PathBuf>;
    /// Removes what no installed plugin names ([`Resources::prune`]).
    fn prune(&self, installed: &[(PluginId, Option<Vec<Resource>>)]);
    /// Removes from `id`'s folder what `rs` doesn't name
    /// ([`Resources::prune_unnamed`]).
    fn prune_unnamed(&self, id: &PluginId, rs: &[Resource]);
    /// Removes `id`'s files: it was uninstalled.
    fn remove(&self, id: &PluginId);
}

impl GetResources for Resources {
    fn dir(&self, id: &PluginId) -> PathBuf {
        Resources::dir(self, id)
    }

    fn missing<'a>(&self, id: &PluginId, rs: &'a [Resource], cancel: &CancelToken) -> PluginResult<Vec<&'a Resource>> {
        Resources::missing(self, id, rs, cancel)
    }

    fn needs_check(&self, id: &PluginId, rs: &[Resource]) -> bool {
        Resources::needs_check(self, id, rs)
    }

    fn to_get(&self, id: &PluginId, rs: &[Resource]) -> u64 {
        Resources::to_get(self, id, rs)
    }

    fn fetch(
        &self,
        id: &PluginId,
        name: &str,
        rs: &[Resource],
        cancel: &CancelToken,
        progress: &mut dyn FnMut(u64, u64),
    ) -> PluginResult<PathBuf> {
        Resources::fetch(self, id, name, rs, cancel, progress)
    }

    fn prune(&self, installed: &[(PluginId, Option<Vec<Resource>>)]) {
        Resources::prune(self, installed);
    }

    fn prune_unnamed(&self, id: &PluginId, rs: &[Resource]) {
        Resources::prune_unnamed(self, id, rs);
    }

    fn remove(&self, id: &PluginId) {
        Resources::remove(self, id);
    }
}

/// What a plugin's program gets before it starts: AutoScene's uv, for a uv
/// script, and the folder of its plugin's large files, when it names some.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fetched {
    pub uv: Option<PathBuf>,
    pub resources: Option<PathBuf>,
}

/// The plugins of this app, and what they are doing.
pub struct PluginHost {
    pub store: Store,
    /// AutoScene's catalog, which the store judges plugins by.
    pub catalog: CatalogHost,
    /// The open plugin pages, which the asp scheme serves ([`pages`]).
    pub pages: Pages,
    /// Where an open page hears that it was closed, and why.
    page_events: PageEvents,
    /// Packages and folders checked and waiting for the person's yes, by
    /// ticket id.
    tickets: Mutex<HashMap<String, Ticket>>,
    /// What ends each compile under way, by ticket id.
    compiles: Mutex<HashMap<String, CancelToken>>,
    /// Told the ticket id of a compile that starts, which the page shows.
    compile_told: CompileTold,
    /// The runs [`queue_run`] queued that may not have ended yet, by job:
    /// what each runs, so the same run isn't queued twice, a plugin turned
    /// off loses those still waiting too, and its pages queue only so many
    /// ([`MAX_PAGE_RUNS`]). Taken before the job runner's lock, never inside
    /// it (see [`PluginHost::submit`]).
    runs: Mutex<HashMap<JobId, RunKey>>,
    /// What runs, and what is being changed ([`PluginHost::hold`],
    /// [`PluginHost::change`]). Never held while the job runner is called.
    busy: Mutex<Busy>,
    /// Told whenever a run or a change of a plugin ends.
    freed: Condvar,
    /// Where `net.fetch` calls go: the internet ([`UreqTransport`]), or a
    /// test's stand-in.
    net: Box<dyn Transport>,
    /// The `net.fetch` calls of each plugin waiting for their answer.
    fetching: Mutex<HashMap<PluginId, usize>>,
    /// What each open page did that it may do only so often.
    page_limits: calls::PageLimits,
    /// When the app's page last said it is there for each open page (macOS
    /// and Linux, see [`pages::watch`]).
    pub beats: pages::Heartbeats,
    /// The plugins' services, which run while their pages are open.
    pub services: Services,
    /// Where uv scripts' uv comes from.
    uv: Box<dyn GetUv>,
    /// Where the plugins' large files come from and are kept.
    resources: Box<dyn GetResources>,
    /// The analyses of each project's batch that ended, until its event.
    batches: Batches,
    /// Each event run that waits, with its job and what it will be told:
    /// an event that comes meanwhile is told by it too.
    told: Mutex<Vec<(RunKey, JobId, EventContext)>>,
    /// The shot filters' checks that failed or were stopped.
    pub checks: filters::Checks,
}

/// The plugins in use: by a run, or by an install, update or uninstall.
#[derive(Default)]
struct Busy {
    /// The plugin each running plugin job runs. Its folder stays as it is
    /// meanwhile: an update or uninstall would throw it away under the run.
    running: HashMap<JobId, PluginId>,
    /// The plugins being installed, updated or uninstalled, whose runs wait.
    changing: HashSet<PluginId>,
}

/// What a run runs, on what, and with what: two runs alike are the same
/// run, whatever started them.
#[derive(Clone, Debug, PartialEq, Eq)]
struct RunKey {
    plugin: PluginId,
    kind: ActionKind,
    /// The action's id in the plugin's `plugin.json`.
    action: String,
    project_id: Option<ProjectId>,
    export_id: Option<ExportId>,
    /// The SHA-256 of what a page handed the run (its JSON), if it handed
    /// it anything: the same action with other input is another run.
    input_sha256: Option<String>,
    /// The character whose shots a shot filter checks, or whose pack a pack
    /// action saves: one character's waits beside another's. None for
    /// every other kind.
    character_id: Option<CharacterId>,
}

impl RunKey {
    /// The run of `action` (of `kind`) of the plugin `plugin`, in the
    /// project `project_id` and on the export `export_id`, with `input`.
    fn new(
        plugin: &PluginId,
        kind: ActionKind,
        action: &str,
        project_id: Option<ProjectId>,
        export_id: Option<ExportId>,
        input: Option<&Value>,
    ) -> Self {
        Self {
            plugin: plugin.clone(),
            kind,
            action: action.to_owned(),
            project_id,
            export_id,
            input_sha256: input.map(|v| package::sha256_hex(v.to_string().as_bytes())),
            character_id: None,
        }
    }
}

/// An installed plugin, as Plugins shows it.
#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// Its author's name.
    pub author: String,
    pub license: Option<String>,
    /// An `https://` link.
    pub repository: Option<String>,
    pub trust: Trust,
    /// The key its updates must be signed with, as minisign shows it.
    pub author_key_id: Option<String>,
    pub source: PluginSource,
    /// Its folder: its package's, or the one it was loaded from.
    pub folder: String,
    pub enabled: bool,
    /// Turned on, not broken, and allowed to run: its actions are offered.
    pub active: bool,
    /// Why it can't run while it is turned on (the setting keeps plugins
    /// AutoScene hasn't reviewed from running).
    pub blocked_reason: Option<String>,
    /// Why it can't run at all: its files are missing or changed, or it
    /// needs another AutoScene.
    pub broken: Option<String>,
    /// Why AutoScene withdrew its version, in the catalog's words: it
    /// doesn't run, whatever else it is.
    pub withdrawn: Option<String>,
    /// What it says it does: declared by its author, and enforced only for
    /// a plugin that runs in the sandbox (`sandbox`).
    pub permissions: Vec<Permission>,
    pub network_hosts: Vec<String>,
    /// It uses the GPU, so its runs wait for the analysis.
    pub gpu: bool,
    /// It is a Python script that uv runs here: on its first run, uv
    /// downloads Python (when this computer has none) and the packages its
    /// lock names.
    pub uv_script: bool,
    pub export_actions: Vec<PluginExportAction>,
    pub commands: Vec<CommandSpec>,
    /// What it saves a character's scenepack as, from the pack's ⋯ menu.
    pub pack_actions: Vec<PackActionSpec>,
    pub settings: Vec<SettingSpec>,
    /// What it may run on by itself, once the person turns it on.
    pub events: Vec<PluginEvent>,
    /// How it runs: a program for each run, a WebAssembly component in
    /// AutoScene's sandbox, or none (it has only pages).
    pub runtime: RuntimeKind,
    /// What a run may use, when it runs in AutoScene's sandbox (a
    /// WebAssembly plugin): its permissions are then enforced.
    pub sandbox: Option<PluginSandbox>,
    /// The protocol its program speaks (`engines.protocol`), which its
    /// pages see too.
    pub protocol: u32,
    pub pages: Vec<PageSpec>,
    /// The keys of its secret settings that have a value in the system's
    /// keychain. Never the values.
    pub secrets_set: Vec<String>,
    /// A `data:` URL of its icon (PNG or SVG), when it has a sound one.
    pub icon: Option<String>,
    /// What tells its banner from another, when it has one: the SHA-256 of
    /// its file. The list never carries the picture: `plugin_banner` gives
    /// it, once for each hash.
    pub banner_hash: Option<String>,
    /// Where its runs write their log.
    pub log_path: String,
    /// Where its service writes its log, when it runs one.
    pub service_log_path: Option<String>,
    /// What was skipped or changed in its `plugin.json`.
    pub warnings: Vec<String>,
    /// What the large files its `plugin.json` names take on the disk now,
    /// in bytes (by their sizes; 0 for one that names none or got none yet).
    pub resource_bytes: f64,
}

/// What a run of a plugin in AutoScene's sandbox may use before it is
/// stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginSandbox {
    /// The memory of all its memories together, in MB.
    pub memory_mb: u32,
    /// How long a run may take, in minutes.
    pub minutes: u32,
}

/// Where a plugin's files are.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PluginSource {
    /// Installed from a package.
    Package,
    /// Loaded from a developer's folder, used where it is.
    Folder,
}

/// An entry a plugin adds to each export's menu.
#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginExportAction {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    /// It runs after every export that finishes.
    pub after_export: bool,
    /// Turned on for after every export, it also runs after one that failed
    /// or made only some of its clips.
    pub after_failed: bool,
}

/// A moment a plugin may run on by itself.
#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginEvent {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    /// What it waits for.
    pub on: EventKind,
    /// The person turned it on: it runs when that happens.
    pub turned_on: bool,
}

/// A package (or folder) checked for install, for the person to say yes
/// (`plugin_install`) or no (`plugin_discard`).
#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginReview {
    pub ticket_id: String,
    /// The plugin as it would be once installed.
    pub plugin: PluginInfo,
    /// What changes from the installed version, when it replaces one.
    pub update: Option<PluginUpdate>,
    /// AutoScene hasn't reviewed it: it needs the setting, and the person's
    /// typed yes.
    pub needs_unreviewed: bool,
    /// It may be installed now (reviewed, or the setting is on).
    pub allowed: bool,
    /// It is a uv script, and AutoScene's uv isn't here yet: its first run
    /// gets it.
    pub uv_missing: bool,
    /// What getting uv downloads at most, in bytes.
    pub uv_bytes: f64,
    /// What its first run still downloads of the large files it names, in
    /// bytes: 0 when it names none, or AutoScene keeps them all already.
    pub resource_bytes: f64,
    /// Where those files come from: the hosts of their addresses, each
    /// once, as it names them.
    pub resource_hosts: Vec<String>,
    /// Where AutoScene's catalog says the plugin was built from, when the
    /// catalog reviewed it.
    pub catalog: Option<ReviewCatalog>,
    /// What the check of a WebAssembly plugin's component found that may
    /// surprise its author (its plugin.json's skipped parts are the
    /// plugin's `warnings`).
    pub component_warnings: Vec<String>,
}

/// What AutoScene's catalog says of a plugin it reviewed.
#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReviewCatalog {
    /// When the catalog that lists it was published (Unix ms).
    pub published: f64,
    /// The repository it was built from: `https://github.com/<owner>/<repo>`.
    pub repository: String,
    /// The commit it was built from, 40 hex digits.
    pub commit: String,
    /// Its folder in the repository; empty or `.` for the top.
    pub path: String,
}

impl From<&Approval> for ReviewCatalog {
    fn from(a: &Approval) -> Self {
        Self {
            published: a.published as f64 * 1000.0,
            repository: a.repo.clone(),
            commit: a.commit.clone(),
            path: a.path.clone(),
        }
    }
}

/// What an update changes.
#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginUpdate {
    pub from_version: String,
    pub trust_before: Trust,
    /// Where the installed version's files are: a package replacing a
    /// loaded folder leaves that folder as it is.
    pub source_before: PluginSource,
    /// Permissions it says it needs that the installed version didn't.
    pub added_permissions: Vec<Permission>,
    pub added_hosts: Vec<String>,
    /// The ids of the pages it adds.
    pub added_pages: Vec<String>,
    /// It runs a program, and the installed version, as the person agreed
    /// to it, runs none: from now on it runs with their user rights.
    pub starts_program: bool,
    /// It keeps its program running while its page is open, and the
    /// installed version, as the person agreed to it, doesn't.
    pub becomes_service: bool,
    /// The installed version, as the person agreed to it, runs in the
    /// sandbox, and this one runs a program outside it: with their user
    /// rights. One of pages only isn't said to: its pages run in theirs.
    pub leaves_sandbox: bool,
    /// It stays in the sandbox and may use more memory or run longer than
    /// the installed version's `plugin.json` says (it is said to when that
    /// can't be read). No more reach, so no system dialog, but the person
    /// read those limits: its review marks them.
    pub raised_limits: bool,
    /// Hosts its large files come from that the installed version's
    /// `plugin.json` doesn't name (all of them when that can't be read).
    pub added_resource_hosts: Vec<String>,
}

/// The plugin host of this app's data folder, with plugins' secrets in the
/// system's keychain, their pages framed by the app's page and their
/// services run as their programs, or none when its store can't open (the
/// plugin commands then say so).
pub fn open(app: &AppHandle, library: &Library, paths: &AppPaths) -> Option<Arc<PluginHost>> {
    let version = app.package_info().version.clone();
    let pages = Pages::new(pages::app_kit(app.clone()), pages::ancestors());
    let to = app.clone();
    // Only the main webview frames pages.
    let page_events: PageEvents = Box::new(move |e: PluginPageEvent| {
        let _ = e.emit_to(&to, "main");
    });
    match Store::open(paths, library.shared_db(), version, Arc::new(OsKeyring)) {
        Ok(store) => {
            // This very executable compiles plugins, as it is the engine:
            // without it, installing a WebAssembly plugin fails and says so.
            let program = std::env::current_exe().unwrap_or_else(|e| {
                log::error!("can't locate the app's executable to compile plugins: {e}");
                PathBuf::new()
            });
            let store = store.compile_with(as_plugin::wasm::Compiler::new(program));
            let to = app.clone();
            let compile_told: CompileTold = Box::new(move |ticket_id| {
                let _ = PluginCompiling { ticket_id: ticket_id.to_owned() }.emit_to(&to, "main");
            });
            // Read before the window opens: a package opened at launch is
            // judged by the catalog held. It is small, and never fetched here.
            let catalog = CatalogHost::builtin(paths, library.shared_db());
            store.set_catalog(Arc::new(catalog.load()));
            let start = Box::new(services::ProgramStart(app.clone()));
            let uv = Box::new(ManagedUv(paths.runtimes()));
            let resources = Box::new(Resources::new(paths.plugin_resources()));
            let host = PluginHost::new(store, catalog, pages, page_events, start, uv, resources);
            let host = Arc::new(host.telling_compiles(compile_told));
            pages::watch(app.clone(), Arc::downgrade(&host));
            services::reap(Arc::downgrade(&host));
            Some(host)
        }
        Err(e) => {
            log::warn!("plugins couldn't start: {}", e.detail());
            None
        }
    }
}

/// The plugin host, for a command.
pub fn host(state: &AppState) -> AppResult<Arc<PluginHost>> {
    state
        .plugins
        .clone()
        .ok_or_else(|| AppError::new(ErrorCode::NotReady, "Plugins couldn't start. Details are in the log."))
}

impl PluginHost {
    /// The host of `store`, judged by `catalog`, with no page open yet in
    /// `pages`, whose open pages hear what becomes of them through
    /// `page_events`, whose services start through `start`, whose uv
    /// scripts get uv from `uv`, and whose plugins get their large files
    /// from `resources`.
    pub fn new(
        store: Store,
        catalog: CatalogHost,
        pages: Pages,
        page_events: PageEvents,
        start: Box<dyn StartService>,
        uv: Box<dyn GetUv>,
        resources: Box<dyn GetResources>,
    ) -> Self {
        Self::reaching(store, catalog, pages, page_events, Box::new(UreqTransport::new()), (start, uv, resources))
    }

    /// [`PluginHost::new`], whose plugins' `net.fetch` calls go to `net`.
    fn reaching(
        store: Store,
        catalog: CatalogHost,
        pages: Pages,
        page_events: PageEvents,
        net: Box<dyn Transport>,
        (start, uv, resources): (Box<dyn StartService>, Box<dyn GetUv>, Box<dyn GetResources>),
    ) -> Self {
        Self {
            store,
            catalog,
            pages,
            page_events,
            tickets: Mutex::default(),
            compiles: Mutex::default(),
            compile_told: Box::new(|_| {}),
            runs: Mutex::default(),
            busy: Mutex::default(),
            freed: Condvar::new(),
            net,
            fetching: Mutex::default(),
            page_limits: calls::PageLimits::default(),
            beats: pages::Heartbeats::default(),
            services: Services::new(start),
            uv,
            resources,
            batches: Batches::default(),
            told: Mutex::default(),
            checks: filters::Checks::default(),
        }
    }

    /// The uv a program of `launch` starts through: none but for a uv
    /// script, which gets AutoScene's own, at once when it is here. When
    /// it isn't, it comes now, as `report` hears ([`GETTING_UV`], and how
    /// far once its bytes come), until `cancel` fires. Two that need it at
    /// once get it once: the second waits for the first.
    fn uv_for(
        &self,
        launch: &Launch,
        cancel: &CancelToken,
        report: &dyn Fn(&str, Option<f64>),
    ) -> PluginResult<Option<PathBuf>> {
        if !matches!(launch, Launch::UvScript { .. }) {
            return Ok(None);
        }
        if let Some(uv) = self.uv.installed() {
            return Ok(Some(uv));
        }
        report(GETTING_UV, None);
        let mut progress = |done: u64, total: u64| report(GETTING_UV, Some(done as f64 / total.max(1) as f64));
        self.uv.install(cancel, &mut progress).map(Some)
    }

    /// The folder of the large files `m`, the manifest of the plugin `id`,
    /// names: none when it names none. What the folder holds that `m` no
    /// longer names goes first, so a run sees nothing of a version before.
    /// A file kept already is hashed the first time this process asks,
    /// which `report` hears first (`checking`) since it takes a while. Those
    /// not kept yet come now, as `report` hears (`stage`, and how far once
    /// their bytes come). Either ends once `cancel` fires; a cancel keeps
    /// what came for the next try.
    fn resources_for(
        &self,
        id: &PluginId,
        m: &Manifest,
        (checking, stage): (&str, &str),
        cancel: &CancelToken,
        report: &dyn Fn(&str, Option<f64>),
    ) -> PluginResult<Option<PathBuf>> {
        if m.resources.is_empty() {
            return Ok(None);
        }
        self.resources.prune_unnamed(id, &m.resources);
        if self.resources.needs_check(id, &m.resources) {
            report(checking, None);
        }
        if self.resources.missing(id, &m.resources, cancel)?.is_empty() {
            return Ok(Some(self.resources.dir(id)));
        }
        report(stage, None);
        let mut progress = |done: u64, total: u64| report(stage, Some(done as f64 / total.max(1) as f64));
        self.resources.fetch(id, &m.name, &m.resources, cancel, &mut progress).map(Some)
    }

    /// The folder of the large files the plugin of `p` names, for its run
    /// ([`PluginHost::resources_for`]: got now when they aren't kept, as
    /// `report` hears, naming the plugin, until `cancel` fires); none when
    /// it names none.
    fn files_for_run(
        &self,
        p: &Installed,
        cancel: &CancelToken,
        report: &dyn Fn(&str, Option<f64>),
    ) -> PluginResult<Option<PathBuf>> {
        let Some(m) = &p.manifest else { return Ok(None) };
        let stages = (format!("Checking files for {}", m.name), format!("Getting files for {}", m.name));
        self.resources_for(&p.record.id, m, (&stages.0, &stages.1), cancel, report)
    }

    /// A run of `action` of `p`, as [`Store::run_spec`] makes it, through
    /// AutoScene's uv when it is a uv script ([`PluginHost::uv_for`]: got
    /// now when it isn't here, as `report` hears, until `cancel` fires),
    /// and then with the large files its plugin names
    /// ([`PluginHost::files_for_run`], got the same way).
    fn run_spec(
        &self,
        p: &Installed,
        action: ActionContext,
        (ffmpeg, ffprobe, ffmpeg_version): (&Path, &Path, &str),
        cancel: &CancelToken,
        report: &dyn Fn(&str, Option<f64>),
    ) -> PluginResult<RunSpec> {
        let mut spec = self.store.run_spec(p, action, ffmpeg, ffprobe, ffmpeg_version)?;
        spec.uv = self.uv_for(&spec.launch, cancel, report)?;
        spec.resources = self.files_for_run(p, cancel, report)?;
        Ok(spec)
    }

    /// Uninstalls `id`, with what it kept when `delete_data`, and the large
    /// files it downloaded; a folder it was loaded from stays. Refused while
    /// it runs. Its runs still waiting go, on `jobs`; one that starts
    /// meanwhile waits, then finds it gone.
    pub fn uninstall(&self, jobs: &JobRunner, id: &PluginId, delete_data: bool) -> PluginResult<()> {
        let name = self.store.get(id).ok().and_then(|p| p.manifest.map(|m| m.name));
        let _changing = self.change(id, name.as_deref().unwrap_or(id.as_str()), "It is being uninstalled.")?;
        self.store.uninstall(id, delete_data)?;
        self.resources.remove(id);
        self.stop(jobs, id);
        Ok(())
    }

    /// Removes the large files no installed plugin names any more: those an
    /// update stopped naming, and those of plugins uninstalled while they
    /// couldn't go. A plugin whose `plugin.json` can't be read keeps its
    /// files. At the app's start, off its thread.
    pub fn prune_resources(&self) {
        let plugins = match self.store.list() {
            Ok(plugins) => plugins,
            Err(e) => {
                log::warn!("listing plugins for their downloaded files: {}", e.detail());
                return;
            }
        };
        let installed: Vec<_> = plugins.into_iter().map(|p| (p.record.id, p.manifest.map(|m| m.resources))).collect();
        self.resources.prune(&installed);
    }

    /// Keeps `t` for [`PluginHost::take`] under its id, and drops the
    /// tickets that waited longer than [`TICKET_TTL`], with their files.
    pub fn keep(&self, t: Ticket) {
        let expired: Vec<Ticket> = {
            let mut tickets = self.tickets.lock();
            let old: Vec<String> =
                tickets.iter().filter(|(_, t)| t.created.elapsed() > TICKET_TTL).map(|(id, _)| id.clone()).collect();
            let expired = old.iter().filter_map(|id| tickets.remove(id)).collect();
            tickets.insert(t.id.to_string(), t);
            expired
        };
        for t in expired {
            self.store.discard(t);
        }
    }

    /// The ticket `id`, unless it was taken already (or its review was
    /// closed), or it waited longer than [`TICKET_TTL`] (it is dropped then).
    pub fn take(&self, id: &str) -> AppResult<Ticket> {
        let t = self
            .tickets
            .lock()
            .remove(id)
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "This review was closed. Open the plugin again."))?;
        if t.created.elapsed() > TICKET_TTL {
            self.store.discard(t);
            return Err(AppError::new(
                ErrorCode::NotFound,
                "This plugin waited too long to be installed. Open it again.",
            ));
        }
        Ok(t)
    }

    /// The developers' folders plugins are loaded from.
    pub fn loaded_folders(&self) -> Vec<PathBuf> {
        let plugins = self.store.list().unwrap_or_default();
        plugins.into_iter().filter(|p| source(&p.record) == PluginSource::Folder).map(|p| p.dir).collect()
    }

    /// Whether a run of `id` is running.
    fn is_running(&self, id: &PluginId) -> bool {
        self.busy.lock().running.values().any(|p| p == id)
    }

    /// Tells `told` the ticket id of each compile that starts.
    fn telling_compiles(mut self, told: CompileTold) -> Self {
        self.compile_told = told;
        self
    }

    /// [`Store::compile`] of `t`, the ticket `ticket_id`, which
    /// [`PluginHost::stop_compile`] can end. The page hears when a component
    /// starts to compile, past every question.
    fn compile(
        &self,
        ticket_id: &str,
        t: &mut Ticket,
        allow_unreviewed: bool,
        accept_unreviewed: bool,
    ) -> PluginResult<()> {
        let cancel = CancelToken::new();
        self.compiles.lock().insert(ticket_id.to_owned(), cancel.clone());
        if t.compiles() {
            (self.compile_told)(ticket_id);
        }
        let compiled = self.store.compile(t, allow_unreviewed, accept_unreviewed, &cancel);
        self.compiles.lock().remove(ticket_id);
        compiled
    }

    /// Ends the compile of the ticket `ticket_id`, if one is under way.
    pub fn stop_compile(&self, ticket_id: &str) {
        if let Some(cancel) = self.compiles.lock().get(ticket_id) {
            cancel.cancel();
        }
    }

    /// Marks `id` (named `name`) as being installed, updated or uninstalled
    /// until the guard drops: a run of it that starts meanwhile waits, and
    /// then finds the plugin as the change left it, and a page of it isn't
    /// opened. Refused while a run of it runs, from the folder the change
    /// would throw away. Looking and marking are one step, so no run starts
    /// between them. Another change of it goes first. Its service stops, so
    /// its program no longer holds its folder, and its open pages close and
    /// hear `reason` (it is being updated, or it was uninstalled).
    pub fn change(&self, id: &PluginId, name: &str, reason: &str) -> PluginResult<Changing<'_>> {
        let mut busy = self.busy.lock();
        while busy.changing.contains(id) {
            self.freed.wait(&mut busy);
        }
        if busy.running.values().any(|p| p == id) {
            return Err(PluginError::Busy(name.to_owned()));
        }
        busy.changing.insert(id.clone());
        drop(busy);
        let changing = Changing { host: self, id: id.clone() };
        // A start of it that comes now is refused: it is marked.
        self.services.stop(id, STOP_GRACE);
        self.close_pages(|m| m.plugin == *id, reason);
        Ok(changing)
    }

    /// Stops the runs of `id`, running or still waiting, closes its pages
    /// and stops its service (it was turned off, or uninstalled).
    pub fn stop(&self, jobs: &JobRunner, id: &PluginId) {
        self.stop_saying(jobs, id, "It was turned off.");
    }

    /// [`PluginHost::stop`], telling its pages `reason`.
    fn stop_saying(&self, jobs: &JobRunner, id: &PluginId, reason: &str) {
        let stop: Vec<JobId> = self.runs().into_iter().filter(|(_, p)| p == id).map(|(job, _)| job).collect();
        if !stop.is_empty() {
            jobs.cancel_where(|j| stop.contains(&j.id));
        }
        // Its pages first: a service starts only for a page still open.
        self.close_pages(|m| m.plugin == *id, reason);
        self.services.stop(id, STOP_GRACE);
    }

    /// The installed plugins the catalog withdrew, as the store judges them
    /// now.
    pub fn withdrawn(&self) -> HashSet<PluginId> {
        let plugins = self.store.list().unwrap_or_else(|e| {
            log::warn!("listing plugins for their withdrawals: {}", e.detail());
            Vec::new()
        });
        plugins.into_iter().filter(|p| p.withdrawn.is_some()).map(|p| p.record.id).collect()
    }

    /// The catalog withdrew `ids`: their runs stop, running or still
    /// waiting, their pages close and say why, and their services stop.
    pub fn stop_withdrawn(&self, jobs: &JobRunner, ids: &[PluginId]) {
        for id in ids {
            let reason = match self.store.get(id) {
                Ok(p) => match runnable(&p, true) {
                    Err(e @ PluginError::Withdrawn { .. }) => e.to_string(),
                    _ => format!("AutoScene withdrew {}.", name(&p)),
                },
                Err(_) => "AutoScene withdrew it.".to_owned(),
            };
            log::warn!("{id} was withdrawn by AutoScene's catalog: its runs and its pages stop");
            self.stop_saying(jobs, id, &reason);
        }
    }

    /// The setting that lets plugins AutoScene hasn't reviewed run went
    /// from `before` to `after`: when it went off, their runs stop, running
    /// or still waiting, their pages close, and their services stop.
    pub fn allow_changed(&self, jobs: &JobRunner, before: bool, after: bool) {
        if !before || after {
            return;
        }
        let stop = unreviewed_runs(&self.runs(), |id| self.store.get(id).is_ok_and(|p| reviewed(&p)));
        if !stop.is_empty() {
            jobs.cancel_where(|j| stop.contains(&j.id));
        }
        // A page keeps the trust it opened with, and its service the trust of
        // the page it started for: a change of the plugin closes and stops them.
        self.close_pages(|m| m.trust != Trust::Reviewed, &PluginError::NotAllowed.to_string());
        self.services.stop_where(|trust| trust != Trust::Reviewed, STOP_GRACE);
    }

    /// AutoScene quits: every service stops, all at once, each within
    /// [`services::QUIT_GRACE`] and the system's grace after it, and none
    /// starts any more (a page still open asks in vain).
    pub fn shutdown(&self) {
        self.services.stop_all(services::QUIT_GRACE);
    }

    /// AutoScene didn't quit after [`PluginHost::shutdown`] after all: its
    /// pages' services start again when they ask.
    pub fn resume(&self) {
        self.services.resume();
    }

    /// The app's page loaded again (a reload, or one after its renderer
    /// crashed): its frames went with it, so every page closes, and its
    /// plugin's service may stop once it idles.
    pub fn close_all_pages(&self) {
        self.close_pages(|_| true, "AutoScene's window loaded again.");
    }

    /// Closes the open pages `close` picks, and tells each why (`reason`).
    /// They close under the busy lock, which a page is opened under
    /// ([`pages::open`]): a page that was opening as what closes it was
    /// stored is closed too. They are told after the lock is let go.
    fn close_pages(&self, close: impl Fn(&Mount) -> bool, reason: &str) {
        let closed = {
            let _busy = self.busy.lock();
            self.pages.close_where(close)
        };
        self.tell_closed(closed, reason);
    }

    /// Tells each page of `sessions`, which closed, why (`reason`).
    fn tell_closed(&self, sessions: Vec<String>, reason: &str) {
        for session in sessions {
            self.forget_page(&session);
            (self.page_events)(PluginPageEvent { session, event: PageEvent::Closed { reason: reason.to_owned() } });
        }
    }

    /// Closes the page `session`: its frame went away, or the person closed
    /// it. One closed already is no error.
    pub fn close_page(&self, session: &str) {
        self.pages.close(session);
        self.forget_page(session);
    }

    /// Forgets what the page `session`, which closed, did and said.
    fn forget_page(&self, session: &str) {
        self.page_limits.forget(session);
        self.beats.forget(session);
    }

    /// Tells the open pages of `id` that its settings changed, and its
    /// service what they are now (one on its way once it runs).
    pub fn settings_changed(&self, id: &PluginId) {
        for session in self.pages.tokens_of(id) {
            (self.page_events)(PluginPageEvent { session, event: PageEvent::Settings });
        }
        if let Some(service) = self.services.settings_changed(id) {
            match self.store.values(id) {
                Ok(values) => service.settings_changed(values),
                Err(e) => log::warn!("{id}'s settings for its service: {}", e.detail()),
            }
        }
    }

    /// The plugin jobs and their plugins, queued or running, as they are
    /// now: what is done with them happens after the locks are let go,
    /// since a job ending takes one too. A job may be listed twice, and one
    /// that ended meanwhile too.
    fn runs(&self) -> Vec<(JobId, PluginId)> {
        let mut runs: Vec<_> = self.runs.lock().iter().map(|(job, key)| (*job, key.plugin.clone())).collect();
        runs.extend(self.busy.lock().running.iter().map(|(job, id)| (*job, id.clone())));
        runs
    }

    /// Queues `work` as the run `key`, unless the same run waits or runs
    /// already (`None`, and `work` is dropped). Two callers can't both find
    /// none: the second waits for the first to note its job. The runs it
    /// compares with are copied before the job runner is called, so the
    /// runner's lock never waits for this one.
    fn submit(
        &self,
        jobs: &JobRunner,
        key: RunKey,
        spec: JobSpec,
        work: impl FnOnce(&JobCtx) -> JobResult + Send + 'static,
    ) -> Option<JobId> {
        self.submit_after(jobs, None, key, spec, work)
    }

    /// [`PluginHost::submit`], as the next run of the job `current` (the
    /// run it is part of, which is ending): `current` no longer counts as
    /// the same run, in the same step, so nothing else queues it between.
    fn submit_after(
        &self,
        jobs: &JobRunner,
        current: Option<JobId>,
        key: RunKey,
        spec: JobSpec,
        work: impl FnOnce(&JobCtx) -> JobResult + Send + 'static,
    ) -> Option<JobId> {
        let mut runs = self.runs.lock();
        let live: HashSet<JobId> = jobs.jobs().into_iter().filter(|j| !j.state.is_finished()).map(|j| j.id).collect();
        runs.retain(|job, _| live.contains(job) && Some(*job) != current);
        // An event's run is deduped where it is told ([`PluginHost::tell`]):
        // one that runs already doesn't keep the next from waiting.
        let same: Vec<JobId> = match key.kind {
            ActionKind::Event => Vec::new(),
            _ => runs.iter().filter(|(_, k)| **k == key).map(|(job, _)| *job).collect(),
        };
        let job = jobs.submit_unless(|j| same.contains(&j.id), spec, work)?;
        runs.insert(job, key);
        Some(job)
    }

    /// Whether the run `key` waits or runs, as `jobs` has them now.
    fn is_queued(&self, jobs: &JobRunner, key: &RunKey) -> bool {
        let live: HashSet<JobId> = jobs.jobs().into_iter().filter(|j| !j.state.is_finished()).map(|j| j.id).collect();
        self.runs.lock().iter().any(|(job, k)| k == key && live.contains(job))
    }

    /// The work of `job` is over, whatever the job runner says of it yet:
    /// it no longer counts as its run.
    fn release(&self, job: JobId) {
        self.runs.lock().remove(&job);
    }

    /// Queues on `jobs` the run `key`, started by `trigger`, as
    /// [`queue_run`] says, with `allow_unreviewed` the setting: `work` makes
    /// what its job does, given whether it waits as analysis. A page queues
    /// none while [`MAX_PAGE_RUNS`] of its plugin's wait or run.
    fn queue<W>(
        &self,
        jobs: &JobRunner,
        allow_unreviewed: bool,
        key: RunKey,
        trigger: Trigger,
        work: impl FnOnce(bool) -> W,
    ) -> PluginResult<Option<JobId>>
    where
        W: FnOnce(&JobCtx) -> JobResult + Send + 'static,
    {
        let p = self.store.get(&key.plugin)?;
        let m = runnable(&p, allow_unreviewed)?;
        let title = action_title(m, key.kind, &key.action, key.project_id, key.export_id)?;
        takes(&p, trigger)?;
        if trigger == Trigger::Page && self.live_runs(jobs, &key.plugin) >= MAX_PAGE_RUNS {
            return Err(PluginError::Busy(m.name.clone()));
        }
        let spec = job_spec(m, &title, key.project_id, key.export_id);
        Ok(self.submit(jobs, key, spec, work(m.runtime.gpu)))
    }

    /// How many runs of `id` [`queue_run`] queued that wait or run, as
    /// `jobs` has them now. Counted outside [`PluginHost::submit`]: pages
    /// that ask at once may each find one fewer.
    fn live_runs(&self, jobs: &JobRunner, id: &PluginId) -> usize {
        let live: HashSet<JobId> = jobs.jobs().into_iter().filter(|j| !j.state.is_finished()).map(|j| j.id).collect();
        self.runs.lock().iter().filter(|(job, key)| key.plugin == *id && live.contains(job)).count()
    }

    /// Holds the job of `ctx` as the run of `id` until the guard drops, once
    /// no other run of `id` runs and no change of it is on its way: a
    /// plugin runs once at a time (a loaded folder that moved to or from the
    /// GPU runs on the other class), and never from files an update or an
    /// uninstall is replacing. A job cancelled while it waits ends so.
    fn hold(&self, ctx: &JobCtx, id: &PluginId) -> Result<Running<'_>, JobError> {
        let mut said = false;
        let mut busy = self.busy.lock();
        loop {
            let changing = busy.changing.contains(id);
            if !changing && !busy.running.values().any(|p| p == id) {
                busy.running.insert(ctx.id, id.clone());
                return Ok(Running { host: self, job: ctx.id });
            }
            if ctx.cancel.is_cancelled() {
                return Err(JobError::Cancelled);
            }
            if !said {
                // The job runner is told without the lock.
                drop(busy);
                ctx.stage(if changing { "Waiting while it is updated" } else { "Waiting for its other run" });
                said = true;
                busy = self.busy.lock();
                continue;
            }
            self.freed.wait_for(&mut busy, WAIT_TICK);
        }
    }
}

impl PluginHost {
    /// Queues with `queue` the run `key` of an event, to tell it `event`
    /// (with `jobs` the job runner), unless a run of it waits: that one
    /// tells of `event` too, and None is given. A run that started was told
    /// already, so one is queued after it. Told while no other event is
    /// told, so the same run never waits twice.
    fn tell<E>(
        &self,
        jobs: &JobRunner,
        key: RunKey,
        event: &EventContext,
        queue: impl FnOnce(RunKey) -> Result<Option<JobId>, E>,
    ) -> Result<Option<JobId>, E> {
        let mut told = self.told.lock();
        let live: HashSet<JobId> = jobs.jobs().into_iter().filter(|j| !j.state.is_finished()).map(|j| j.id).collect();
        // One that ended before it started is told nothing more.
        told.retain(|(_, job, _)| live.contains(job));
        if let Some((_, _, waiting)) = told.iter_mut().find(|(k, _, _)| *k == key) {
            waiting.episodes.done += event.episodes.done;
            waiting.episodes.failed += event.episodes.failed;
            waiting.at = waiting.at.max(event.at);
            return Ok(None);
        }
        let job = queue(key.clone())?;
        if let Some(job) = job {
            told.push((key, job, event.clone()));
        }
        Ok(job)
    }

    /// What the event run `job` tells, as it starts: from now on an event
    /// queues another run.
    fn take_event(&self, job: JobId) -> Option<EventContext> {
        let mut told = self.told.lock();
        let at = told.iter().position(|(_, j, _)| *j == job)?;
        Some(told.remove(at).2)
    }
}

/// A run's place in [`Busy::running`].
struct Running<'a> {
    host: &'a PluginHost,
    job: JobId,
}

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.host.busy.lock().running.remove(&self.job);
        self.host.freed.notify_all();
    }
}

/// A plugin's place in [`Busy::changing`] (see [`PluginHost::change`]).
pub struct Changing<'a> {
    host: &'a PluginHost,
    id: PluginId,
}

impl Drop for Changing<'_> {
    fn drop(&mut self) {
        self.host.busy.lock().changing.remove(&self.id);
        self.host.freed.notify_all();
    }
}

/// Whether `p` may run now: not broken, not withdrawn by AutoScene's
/// catalog, turned on, and reviewed by AutoScene unless `allow_unreviewed`
/// (the setting) lets the others run.
pub fn active(p: &Installed, allow_unreviewed: bool) -> bool {
    runnable(p, allow_unreviewed).is_ok()
}

/// The manifest of `p` while it may run (see [`active`]), else why it may
/// not. Every run, page and service of a plugin starts past it.
fn runnable(p: &Installed, allow_unreviewed: bool) -> PluginResult<&Manifest> {
    let m = match (&p.manifest, &p.broken) {
        (Some(m), None) => m,
        (_, Some(broken)) => return Err(PluginError::Invalid(broken.clone())),
        (None, None) => return Err(PluginError::Invalid(as_plugin::store::MISSING.into())),
    };
    if let Some(reason) = &p.withdrawn {
        return Err(PluginError::Withdrawn { name: m.name.clone(), reason: reason.clone() });
    }
    if !p.record.enabled {
        return Err(PluginError::Invalid(format!("{} is turned off.", m.name)));
    }
    blocked(p, allow_unreviewed).map_or(Ok(m), Err)
}

/// Why `p` may not run while it is turned on: AutoScene hasn't reviewed it,
/// and the setting keeps such plugins from running.
fn blocked(p: &Installed, allow_unreviewed: bool) -> Option<PluginError> {
    (!reviewed(p) && !allow_unreviewed).then_some(PluginError::NotAllowed)
}

/// Whether AutoScene reviewed `p` (see [`trust`]).
fn reviewed(p: &Installed) -> bool {
    trust(&p.record) == Trust::Reviewed
}

/// How far the plugin of `r` is trusted: as its row says, but a plugin
/// loaded from a folder is never reviewed or signed, whatever its row says
/// (another program could have written it).
fn trust(r: &Record) -> Trust {
    match r.source {
        Source::Package { .. } => r.trust,
        Source::Folder { .. } => Trust::Unreviewed,
    }
}

/// Where the files of the plugin of `r` are.
fn source(r: &Record) -> PluginSource {
    match r.source {
        Source::Package { .. } => PluginSource::Package,
        Source::Folder { .. } => PluginSource::Folder,
    }
}

/// The name of `p`, or its id when it is broken.
fn name(p: &Installed) -> &str {
    p.manifest.as_ref().map_or(p.record.id.as_str(), |m| m.name.as_str())
}

/// `p` as the app shows it, with `allow_unreviewed` the setting.
pub fn info(p: &Installed, store: &Store, allow_unreviewed: bool) -> PluginInfo {
    let events = p.manifest.as_ref().map_or(&[][..], |m| &m.events);
    let m = p.manifest.as_ref();
    let id = &p.record.id;
    let actions = m.map_or(&[][..], |m| &m.export_actions);
    PluginInfo {
        id: id.to_string(),
        name: name(p).to_owned(),
        version: p.record.version.clone(),
        description: m.map(|m| m.description.clone()).unwrap_or_default(),
        author: m.map(|m| m.author.name.clone()).unwrap_or_default(),
        license: m.and_then(|m| m.license.clone()),
        repository: m.and_then(|m| m.repository.clone()),
        trust: trust(&p.record),
        author_key_id: p.record.author_key.as_ref().map(|k| k.key_id.clone()),
        source: source(&p.record),
        folder: p.dir.to_string_lossy().into_owned(),
        enabled: p.record.enabled,
        active: active(p, allow_unreviewed),
        blocked_reason: blocked(p, allow_unreviewed).filter(|_| p.broken.is_none()).map(|e| e.to_string()),
        broken: p.broken.clone(),
        withdrawn: p.withdrawn.clone(),
        // What its plugin.json says now: a loaded folder's may have changed.
        permissions: m.map_or_else(|| p.record.permissions.clone(), |m| m.permissions.clone()),
        network_hosts: m.map_or_else(|| p.record.network_hosts.clone(), |m| m.network_hosts.clone()),
        gpu: m.is_some_and(|m| m.runtime.gpu),
        uv_script: m.is_some_and(|m| matches!(m.launch(), Ok(Launch::UvScript { .. }))),
        export_actions: actions
            .iter()
            .map(|a| PluginExportAction {
                id: a.id.clone(),
                title: a.title.clone(),
                description: a.description.clone(),
                after_export: p.record.after_export.contains(&a.id),
                after_failed: a.after_failed,
            })
            .collect(),
        commands: m.map(|m| m.commands.clone()).unwrap_or_default(),
        pack_actions: m.map(|m| m.pack_actions.clone()).unwrap_or_default(),
        settings: m.map(|m| m.settings.clone()).unwrap_or_default(),
        events: events
            .iter()
            .map(|e| PluginEvent {
                id: e.id.clone(),
                title: e.title.clone(),
                description: e.description.clone(),
                on: e.on,
                turned_on: p.extra.events.contains(&e.id),
            })
            .collect(),
        runtime: m.map_or_else(|| p.extra.agreed_runtime(), |m| m.runtime.kind),
        sandbox: m.and_then(sandbox),
        protocol: m.map_or(1, |m| m.engines.protocol),
        pages: m.map(|m| m.pages.clone()).unwrap_or_default(),
        secrets_set: p.secrets_stored(),
        icon: store.icon_data_url(p),
        banner_hash: store.banner_hash(p),
        log_path: store.log_path(id).to_string_lossy().into_owned(),
        service_log_path: m
            .filter(|m| m.runtime.kind == RuntimeKind::Service)
            .map(|_| store.service_log_path(id).to_string_lossy().into_owned()),
        warnings: p.warnings.clone(),
        resource_bytes: store.resource_bytes(p) as f64,
    }
}

/// What a run of the plugin of `m` may use, when it runs in the sandbox.
fn sandbox(m: &Manifest) -> Option<PluginSandbox> {
    let limits = m.is_sandboxed().then(|| m.runtime.limits.unwrap_or_default())?;
    Some(PluginSandbox { memory_mb: limits.memory_mb, minutes: limits.minutes })
}

/// What a dialog of the system's own asks: its title, its text and the yes.
pub type Question = (String, String, &'static str);

/// Installs the plugin the review `ticket_id` showed. One AutoScene hasn't
/// reviewed needs the setting, `accept_unreviewed` (the person typed its
/// name), and their yes to `ask`, a dialog of the system's own. So does one
/// AutoScene reviewed that may do more than the version it replaces, as the
/// person agreed to that ([`Store::widens`]), whatever the page says: the
/// page can't answer that dialog. A no keeps the review (`cancelled`).
/// Refused while the version it replaces runs, before anything is asked: a
/// yes to what can't happen now would have to be given again.
///
/// A WebAssembly plugin compiles past that yes and before the plugin is
/// touched: what can't compile, or takes too long, leaves its runs, its
/// pages and its service as they were, and a Stop ([`PluginHost::stop_compile`])
/// keeps the review, as a no does.
pub fn install(
    host: &PluginHost,
    ticket_id: &str,
    allow_unreviewed: bool,
    accept_unreviewed: bool,
    ask: impl FnOnce(Question) -> bool,
) -> AppResult<Installed> {
    let mut ticket = host.take(ticket_id)?;
    if host.is_running(&ticket.manifest.id) {
        let busy = PluginError::Busy(ticket.manifest.name.clone());
        host.keep(ticket);
        return Err(busy.into());
    }
    // What can't be read here, the commit refuses.
    let widens = host.store.widens(&ticket.manifest).unwrap_or_default();
    let asks = match ticket.verdict.trust {
        Trust::Reviewed => widens.any(),
        // Only when the page said yes and the setting allows it: else the
        // commit refuses anyway.
        _ => allow_unreviewed && accept_unreviewed,
    };
    if asks && !ask(install_question(&ticket, &widens)) {
        host.keep(ticket);
        return Err(AppError::new(ErrorCode::Cancelled, "It wasn't installed."));
    }
    ticket.confirmed = asks;
    match host.compile(ticket_id, &mut ticket, allow_unreviewed, accept_unreviewed) {
        Ok(()) => {}
        Err(PluginError::Cancelled) => {
            ticket.confirmed = false;
            host.keep(ticket);
            return Err(AppError::new(ErrorCode::Cancelled, "It wasn't installed."));
        }
        Err(e) => {
            host.store.discard(ticket);
            return Err(e.into());
        }
    }
    // Its runs wait from here until the row names the new version. One that
    // started during the question loses the yes: the review kept asks again.
    let changing = match host.change(&ticket.manifest.id, &ticket.manifest.name, "It is being updated.") {
        Ok(changing) => changing,
        Err(busy) => {
            ticket.confirmed = false;
            host.keep(ticket);
            return Err(busy.into());
        }
    };
    let p = host.store.commit(ticket, allow_unreviewed, accept_unreviewed)?;
    drop(changing);
    Ok(p)
}

/// What installing the plugin of `t` asks, from what was checked here (not
/// what the page shows), with `widens` what it may do that the installed
/// version, as the person agreed to it, may not.
fn install_question(t: &Ticket, widens: &Widens) -> Question {
    let m = &t.manifest;
    let yes = match (&t.source, &t.previous) {
        (Source::Folder { .. }, _) => "Load",
        (_, Some(_)) => "Update",
        _ => "Install",
    };
    let reviewed = t.verdict.trust == Trust::Reviewed;
    let who = match (reviewed, &t.verdict.author_key) {
        (true, _) => format!("By: {}", m.author.name),
        (_, Some(key)) if t.verdict.trust == Trust::Pinned => {
            format!("Says it is by: {}\nSigned with key {}", m.author.name, key.key_id)
        }
        _ => format!("Says it is by: {}\nNot signed", m.author.name),
    };
    let place = match &t.source {
        Source::Folder { path } => format!("\nFolder: {path}"),
        Source::Package { .. } => String::new(),
    };
    let installed = t.previous.as_ref().map_or(String::new(), |p| format!(" ({} is installed)", p.version));
    // A WebAssembly plugin can't do what a program can: the question says
    // what is true of it, as the review does.
    let how = match m.is_sandboxed() {
        true => "in a sandbox: it sees only its own folder and what it asks for.",
        false => "with your user rights: it can read, change and send anything you can.",
    };
    let runs = match widens.runtime {
        RuntimeChange { leaves_sandbox: true, .. } => {
            format!("The installed version ran in a sandbox. This version runs {how}")
        }
        RuntimeChange { starts_program: true, .. } => {
            format!("The installed version runs no program. This version runs one {how}")
        }
        _ if !m.runs_program() => "It runs no program: its pages run in a sandbox.".to_owned(),
        _ => format!("It runs {how}"),
    };
    let service = match widens.runtime.becomes_service {
        true => " It now keeps its program running while its page is open.",
        false => "",
    };
    let new = new_lines(m, widens);
    let new = match new.is_empty() {
        true => String::new(),
        false => format!("\n\nNew in this version:\n· {}", new.join("\n· ")),
    };
    let first = match reviewed {
        true => "AutoScene reviewed this version.",
        false => "AutoScene has not reviewed it.",
    };
    let text = format!(
        "{first} {runs}{service}{new}\n\n\
         Id: {}\nVersion: {}{installed}\n{who}{place}",
        m.id, m.version
    );
    (format!("{yes} {}?", m.name), text, yes)
}

/// What `widens` adds to the plugin of `m`, a line each, as its review says it.
fn new_lines(m: &Manifest, widens: &Widens) -> Vec<String> {
    let sandbox = m.is_sandboxed().then_some(m.network_hosts.as_slice());
    let mut new: Vec<String> = widens.permissions.iter().map(|p| permission_words(*p, sandbox).to_owned()).collect();
    match sandbox {
        // Its line on the network ends in the sites, as in the review.
        Some(_) if !widens.hosts.is_empty() => {
            let words = permission_words(Permission::Network, sandbox);
            new.retain(|line| line != words);
            new.push(format!("{words} {}", widens.hosts.join(", ")));
        }
        _ => new.extend(widens.hosts.iter().map(|h| format!("Talks to {h}"))),
    }
    new
}

/// What `p` lets a plugin do, as the review says it (`permissionLabel`, whose
/// tests hold these words to its own): what it says it does, or, for one in
/// the sandbox (`sandbox`, with the sites it may reach), what the sandbox lets
/// it do.
fn permission_words(p: Permission, sandbox: Option<&[String]>) -> &'static str {
    match (p, sandbox) {
        (Permission::ExportsRead, None) => "Reads the exported clips",
        (Permission::ExportsWrite, None) => "Changes or adds files beside the exported clips",
        (Permission::ProjectRead, None) => "Reads the project: its episodes and characters",
        (Permission::Network, None) => "Talks to the internet",
        (Permission::ExportsRead, Some(_)) => "Reads the files in the export's folder",
        (Permission::ExportsWrite, Some(_)) => "Reads, changes and deletes files in the export's folder",
        (Permission::ProjectRead, Some(_)) => "Reads the project's folder",
        (Permission::Network, Some([])) => "May use the network, but names no site: every request is refused.",
        (Permission::Network, Some(_)) => "Talks to these sites, over https only:",
    }
}

/// `t` for the review dialog of `host`, with `allow_unreviewed` the
/// setting.
pub fn review(t: &Ticket, host: &PluginHost, allow_unreviewed: bool) -> PluginReview {
    let store = &host.store;
    let needs_unreviewed = t.verdict.trust != Trust::Reviewed;
    // The installed version as it is now: the pages a loaded folder's
    // plugin.json may have changed since its record was written, the
    // runtime the person agreed to, and the events they turned on.
    let before = t.previous.as_ref().and_then(|p| store.get(&p.id).ok());
    let plugin = info(&as_installed(t, before.as_ref()), store, allow_unreviewed);
    PluginReview {
        ticket_id: t.id.to_string(),
        update: t.previous.as_ref().map(|previous| update(previous, before.as_ref(), &t.manifest)),
        needs_unreviewed,
        allowed: !needs_unreviewed || allow_unreviewed,
        uv_missing: plugin.uv_script && host.uv.installed().is_none(),
        uv_bytes: as_plugin::uv::download_bytes() as f64,
        resource_bytes: host.resources.to_get(&t.manifest.id, &t.manifest.resources) as f64,
        resource_hosts: t.manifest.resource_hosts(),
        catalog: t.approval.as_ref().map(ReviewCatalog::from),
        component_warnings: t.component_warnings.clone(),
        plugin,
    }
}

/// The plugin of `t` as it would be once installed, in the folder it was
/// checked in (as [`Store::commit`] keeps what the version before had,
/// which is `before` as it is now; its secrets aren't shown before it is).
fn as_installed(t: &Ticket, before: Option<&Installed>) -> Installed {
    let m = &t.manifest;
    let previous = t.previous.as_ref();
    let after_export = previous.map_or_else(Vec::new, |p| {
        as_plugin::store::after_export_kept(m, before.and_then(|b| b.manifest.as_ref()), &p.after_export)
    });
    let turned_on = before.map_or(&[][..], |b| &b.extra.events);
    let events = m.events.iter().filter(|e| turned_on.contains(&e.id)).map(|e| e.id.clone()).collect();
    let dir = match (&t.staged, &t.source) {
        (Some(staged), _) => staged.clone(),
        (None, Source::Folder { path }) => PathBuf::from(path),
        (None, Source::Package { .. }) => PathBuf::new(),
    };
    Installed {
        record: Record {
            v: RECORD_VERSION,
            id: m.id.clone(),
            version: m.version.clone(),
            source: t.source.clone(),
            trust: t.verdict.trust,
            author_key: t.verdict.author_key.clone(),
            permissions: m.permissions.clone(),
            network_hosts: m.network_hosts.clone(),
            enabled: previous.is_none_or(|p| p.enabled),
            after_export,
            lock_sha256: t.lock.is_some().then(|| package::sha256_hex(&t.lock_bytes)),
            installed_at: 0,
        },
        manifest: Some(m.clone()),
        warnings: t.warnings.clone(),
        dir,
        broken: None,
        extra: Extra { events, ..Extra::agreed(m.runtime.kind) },
        // A version the catalog withdrew is never staged.
        withdrawn: None,
    }
}

/// What the plugin of manifest `m` changes from the installed `previous`,
/// which is `before` as it is now (none when its row is gone; its manifest
/// none when it is broken).
fn update(previous: &Record, before: Option<&Installed>, m: &Manifest) -> PluginUpdate {
    let pages_before = before.and_then(|b| b.manifest.as_ref()).map_or(&[][..], |b| &b.pages);
    // Where its files came from, by the plugin.json its lock fixes: none
    // known when it can't be read, so each host is said to be new.
    let fetched_from = before.and_then(|b| b.manifest.as_ref()).map(Manifest::resource_hosts).unwrap_or_default();
    // Only by what the person agreed to: one whose row is gone isn't said
    // to change how it runs.
    let agreed = before.map(|b| b.extra.agreed_runtime());
    let widens = Widens::of(previous, agreed, m);
    // The limits the review showed, by the plugin.json its lock fixes: raised
    // when that can't be read. One new to the sandbox had none to raise.
    let limits_before = before.and_then(|b| b.manifest.as_ref()).and_then(sandbox);
    let raised_limits = agreed == Some(RuntimeKind::Wasm)
        && sandbox(m).is_some_and(|now| {
            limits_before.is_none_or(|was| now.memory_mb > was.memory_mb || now.minutes > was.minutes)
        });
    PluginUpdate {
        from_version: previous.version.clone(),
        trust_before: trust(previous),
        source_before: source(previous),
        added_permissions: widens.permissions,
        added_hosts: widens.hosts,
        added_pages: m
            .pages
            .iter()
            .filter(|p| !pages_before.iter().any(|b| b.id == p.id))
            .map(|p| p.id.clone())
            .collect(),
        starts_program: widens.runtime.starts_program,
        becomes_service: widens.runtime.becomes_service,
        leaves_sandbox: widens.runtime.leaves_sandbox,
        raised_limits,
        added_resource_hosts: m.resource_hosts().into_iter().filter(|h| !fetched_from.contains(h)).collect(),
    }
}

/// Queues a run of `action` (an action of `kind`) of the plugin `id`,
/// started by `trigger`: an export action runs on the export `export_id`
/// of the project `project_id`, a command in the project when it needs
/// one. `input` is what a page handed it, and comes only with
/// `Trigger::Page`. None when the same run waits or runs already. A plugin
/// that isn't active is refused, and so is a run from a page of one whose
/// program doesn't speak protocol 2 (a WebAssembly plugin takes it).
#[allow(clippy::too_many_arguments)]
pub fn queue_run(
    app: &AppHandle,
    id: &PluginId,
    kind: ActionKind,
    action: &str,
    trigger: Trigger,
    project_id: Option<ProjectId>,
    export_id: Option<ExportId>,
    input: Option<Value>,
) -> AppResult<Option<JobId>> {
    let key = RunKey::new(id, kind, action, project_id, export_id, input.as_ref());
    queue_key(app, key, trigger, input)
}

/// Queues the run `key`, started by `trigger`, as [`queue_run`] says: with
/// `input` from a page. A run of an event takes what happened as it starts
/// ([`PluginHost::tell`]).
fn queue_key(app: &AppHandle, key: RunKey, trigger: Trigger, input: Option<Value>) -> AppResult<Option<JobId>> {
    let state = app.state::<AppState>();
    let host = host(&state)?;
    let work = |gpu| {
        let (app, host, key) = (app.clone(), host.clone(), key.clone());
        move |ctx: &JobCtx| run(&app, &host, ctx, &key, trigger, input, gpu)
    };
    Ok(host.queue(&state.jobs, state.library.allows_unreviewed_plugins()?, key.clone(), trigger, work)?)
}

/// Whether `p` takes a run that `trigger` started: one from a page only
/// while its program speaks protocol 2 or it is a WebAssembly plugin, as
/// its page's call was checked, with the plugin as it is now. A protocol 1
/// program would read a trigger it doesn't know.
fn takes(p: &Installed, trigger: Trigger) -> PluginResult<()> {
    match trigger {
        Trigger::Page => {
            as_plugin::calls::check(HostMethod::ActionsRun, &Caller::Page { project: None }, &Grants::of(p)?)
        }
        Trigger::Menu | Trigger::AfterExport | Trigger::Palette | Trigger::Event | Trigger::Pack => Ok(()),
    }
}

/// The title of the action `action` of `kind` in `m`, when it may run with
/// what it is given: an export action needs an export (with its project),
/// a command never runs on one, and one that needs a project needs it. An
/// event runs in the project where it happened, on no export.
fn action_title(
    m: &Manifest,
    kind: ActionKind,
    action: &str,
    project_id: Option<ProjectId>,
    export_id: Option<ExportId>,
) -> PluginResult<String> {
    let refused = |why: &str| Err(PluginError::Invalid(why.into()));
    let no_such = || PluginError::Invalid(format!("{} has no such action.", m.name));
    // A page names an action without its kind: one of the other kind is
    // said to be so.
    let is_command = || m.commands.iter().any(|c| c.id == action);
    let is_export_action = || m.export_actions.iter().any(|a| a.id == action);
    match kind {
        // Only the scenepack builder starts one (see filters).
        ActionKind::ShotFilter => {
            let f = m.shot_filters.iter().find(|f| f.id == action).ok_or_else(no_such)?;
            if project_id.is_none() || export_id.is_some() {
                return refused("A shot filter checks a character's scenepack.");
            }
            Ok(f.title.clone())
        }
        // Only a scenepack's menu starts one (see pack_actions).
        ActionKind::PackAction => {
            let a = m.pack_actions.iter().find(|a| a.id == action).ok_or_else(no_such)?;
            if project_id.is_none() || export_id.is_some() {
                return refused("A pack action saves a character's scenepack.");
            }
            Ok(a.title.clone())
        }
        ActionKind::ExportAction => {
            let Some(a) = m.export_actions.iter().find(|a| a.id == action) else {
                return if is_command() { refused("A command doesn't run on an export.") } else { Err(no_such()) };
            };
            if export_id.is_none() || project_id.is_none() {
                return refused("Choose an export to run it on.");
            }
            Ok(a.title.clone())
        }
        ActionKind::Command => {
            let Some(c) = m.commands.iter().find(|c| c.id == action) else {
                return if is_export_action() { refused("Choose an export to run it on.") } else { Err(no_such()) };
            };
            if export_id.is_some() {
                return refused("A command doesn't run on an export.");
            }
            if c.needs_project && project_id.is_none() {
                return refused("Open a project to run it in.");
            }
            Ok(c.title.clone())
        }
        ActionKind::Event => {
            let Some(e) = m.events.iter().find(|e| e.id == action) else { return Err(no_such()) };
            if export_id.is_some() || project_id.is_none() {
                return refused("An event runs in the project where it happened.");
            }
            Ok(e.title.clone())
        }
    }
}

/// The job of a run titled `title` of the plugin of `m`: a plugin job, on
/// the analysis' class when the plugin uses the GPU, in its project and on
/// its export. Never of a media: the episode lists would show it as that
/// episode's work.
fn job_spec(m: &Manifest, title: &str, project_id: Option<ProjectId>, export_id: Option<ExportId>) -> JobSpec {
    JobSpec {
        kind: JobKind::Plugin,
        class: if m.runtime.gpu { Class::Analysis } else { Class::Plugin },
        // Cleaned and cut when its plugin.json was read, as its name is.
        title: title.to_owned(),
        subtitle: Some(m.name.clone()),
        project_id,
        media_id: None,
        export_id,
    }
}

/// The queued run `key`, started by `trigger` with `input` (a run of an
/// event with what it was told), now it starts; `gpu` is whether it waited
/// as analysis. A program of protocol 2 gets its calls answered; a
/// WebAssembly plugin runs in the sandbox.
fn run(
    app: &AppHandle,
    host: &Arc<PluginHost>,
    ctx: &JobCtx,
    key: &RunKey,
    trigger: Trigger,
    input: Option<Value>,
    gpu: bool,
) -> JobResult {
    // Held before it looks whether it may run: a change to the plugin or
    // the setting from here on finds it and stops it.
    let _running = host.hold(ctx, &key.plugin)?;
    let state = app.state::<AppState>();
    let p = startable(&host.store, &state.library, key, trigger, gpu)?;
    let action = ActionContext { kind: key.kind, id: key.action.clone(), trigger };
    let report = |stage: &str, f: Option<f64>| ctx.report(stage, f);
    // What a run of an event tells, taken as it starts, whichever way it
    // runs: until then, one that comes is told by it too.
    let told = || match key.kind {
        ActionKind::Event => {
            let nothing = || JobError::Failed("Nothing happened for it to tell.".into());
            host.take_event(ctx.id).map(Some).ok_or_else(nothing)
        }
        _ => Ok(None),
    };
    let said = if sandboxed(&p) {
        let mut spec = wasm_spec_for(host, &*state.library, (&p, key), action, input, &ctx.cancel, &report)?;
        spec.message.event = told()?;
        ctx.stage(&format!("Starting {}", spec.name));
        wasm::run(spec, &ctx.cancel, &|stage, f| ctx.report(stage, f))
    } else {
        let tools = state.tools().map_err(|e| JobError::Failed(e.message))?;
        let tools_at = (tools.ffmpeg.as_path(), tools.ffprobe.as_path(), tools.version.as_str());
        let mut spec = host.run_spec(&p, action, tools_at, &ctx.cancel, &report).map_err(plugin_job_error)?;
        add_context(&mut spec.message, &*state.library, key, input).map_err(job_error)?;
        spec.message.event = told()?;
        let calls = calls_for(&state.library, host, &p, key, spec.protocol).map_err(plugin_job_error)?;
        spec.calls = calls.map(|c| Arc::new(c) as Arc<dyn HostCalls>);
        ctx.stage(&format!("Starting {}", spec.name));
        runtime::run(spec, &ctx.cancel, &|stage, f| ctx.report(stage, f))
    };
    if let Some(message) = said.map_err(plugin_job_error)? {
        ctx.message(&message);
    }
    Ok(())
}

/// Whether a run of `p` runs in the sandbox: it is a WebAssembly plugin,
/// whose run needs no FFmpeg.
fn sandboxed(p: &Installed) -> bool {
    p.manifest.as_ref().and_then(|m| m.launch().ok()).is_some_and(|l| !needs_tools(&l))
}

/// The run `key` of the WebAssembly plugin `p` of `host`, of `action`, with
/// `input` from its page: what [`Store::wasm_spec`] gives, with the values
/// of its set secrets and what it runs on, as `places` has them now, and
/// last the folder of the large files its plugin names, its `/resources`
/// ([`PluginHost::files_for_run`]: got now when they aren't kept, as
/// `report` hears, until `cancel` fires). So they are here before its
/// component is compiled and run, and a run refused for its spec, its
/// secrets, what it runs on or a folder of it that is gone
/// ([`wasm::check_folders`]) gets none of them. One whose component then
/// fails to load got them all the same; they are kept for the next.
fn wasm_spec_for(
    host: &PluginHost,
    places: &dyn calls::Places,
    (p, key): (&Installed, &RunKey),
    action: ActionContext,
    input: Option<Value>,
    cancel: &CancelToken,
    report: &dyn Fn(&str, Option<f64>),
) -> Result<wasm::WasmSpec, JobError> {
    let store = &host.store;
    let mut spec = store.wasm_spec(p, action).map_err(plugin_job_error)?;
    spec.secrets = secret_values(store, &p.record.id, &spec.grants.secrets).map_err(plugin_job_error)?;
    add_context(&mut spec.message, places, key, input).map_err(job_error)?;
    wasm::check_folders(&spec).map_err(plugin_job_error)?;
    spec.resources_dir = host.files_for_run(p, cancel, report).map_err(plugin_job_error)?;
    Ok(spec)
}

/// Whether a run that starts as `launch` needs the app's FFmpeg: a program
/// does; a WebAssembly component can't start one, so a broken FFmpeg never
/// stops it.
fn needs_tools(launch: &Launch) -> bool {
    !matches!(launch, Launch::Wasm { .. })
}

/// The values of the secret settings `keys` of the plugin `id` that are
/// set, by key: read from the keychain once, before a WebAssembly plugin's
/// run starts, which asks for them while it runs. A keychain that refuses
/// fails the run, as it fails a page's `secrets.get`.
fn secret_values(store: &Store, id: &PluginId, keys: &[String]) -> PluginResult<BTreeMap<String, String>> {
    let mut values = BTreeMap::new();
    for key in keys {
        if let Some(value) = store.secret(id, key)? {
            values.insert(key.clone(), value);
        }
    }
    Ok(values)
}

/// The answers to the calls of the run `key` of `p`, whose plugin speaks
/// `protocol`: one of protocol 2 calls AutoScene, one of protocol 1 never
/// does.
fn calls_for(
    lib: &Arc<Library>,
    host: &Arc<PluginHost>,
    p: &Installed,
    key: &RunKey,
    protocol: u32,
) -> PluginResult<Option<calls::AppCalls>> {
    match protocol {
        2.. => calls::AppCalls::run(lib.clone(), host.clone(), p, key).map(Some),
        _ => Ok(None),
    }
}

/// The plugin of the queued run `key`, started by `trigger`, now it
/// starts, unless it may no longer run (see [`active`]) or no longer runs
/// `key` as it was queued: an update, or a loaded folder's changed
/// `plugin.json`, may have taken the action away, changed what it needs,
/// moved the plugin to or from the GPU (`gpu`: it waited as analysis), or
/// left its page's run to a program of protocol 1. A run that was to start
/// by itself, which the person turned off while it waited, ends as cancelled
/// ([`turned_off`]): they said so, and nothing went wrong.
fn startable(store: &Store, lib: &Library, key: &RunKey, trigger: Trigger, gpu: bool) -> Result<Installed, JobError> {
    let refused = |e: PluginError| JobError::Failed(e.to_string());
    let p = store.get(&key.plugin).map_err(plugin_job_error)?;
    let allow = lib.allows_unreviewed_plugins().map_err(job_error)?;
    let m = runnable(&p, allow).map_err(refused)?;
    action_title(m, key.kind, &key.action, key.project_id, key.export_id).map_err(refused)?;
    if m.runtime.gpu != gpu || takes(&p, trigger).is_err() {
        return Err(JobError::Failed(format!("{} changed since this run was queued. Run it again.", m.name)));
    }
    if turned_off(&p, key, trigger) {
        return Err(JobError::Cancelled);
    }
    Ok(p)
}

/// Whether the run `key` starts by itself (an event's, or one after an
/// export), and the person no longer has `p` do that. Who asked for a run
/// from a menu or a page asked for that run.
fn turned_off(p: &Installed, key: &RunKey, trigger: Trigger) -> bool {
    match (key.kind, trigger) {
        (ActionKind::Event, _) => !p.extra.events.contains(&key.action),
        (_, Trigger::AfterExport) => !p.record.after_export.contains(&key.action),
        _ => false,
    }
}

/// Gives `message` what the run `key` is for: the project it runs in and
/// the export it runs on, as `places` has them now (how the export went
/// only for a run of protocol 2, whose line has it), and `input`, what its
/// page handed it.
fn add_context(
    message: &mut RunMessage,
    places: &dyn calls::Places,
    key: &RunKey,
    input: Option<Value>,
) -> ProjectResult<()> {
    message.input = input;
    let Some(project_id) = key.project_id else { return Ok(()) };
    let project = places.project(project_id)?;
    message.project = Some(project_context(&project)?);
    if let Some(export_id) = key.export_id {
        let mut export = export_context(&project, &places.export(&project, export_id)?)?;
        if message.protocol < 2 {
            export.status = None;
        }
        message.export = Some(export);
    }
    Ok(())
}

/// `e` as its job ends with it. The plugin's own error line is its answer,
/// given before any cancel (else the run says Cancelled): it stays the
/// job's error when a cancel comes after it.
fn plugin_job_error(e: PluginError) -> JobError {
    match e {
        PluginError::Cancelled => JobError::Cancelled,
        e @ PluginError::Failed { .. } => {
            log::warn!("{}", e.detail());
            JobError::FailedFirst(e.to_string())
        }
        e => {
            log::warn!("{}", e.detail());
            JobError::Failed(e.to_string())
        }
    }
}

fn project_context(project: &Project) -> ProjectResult<ProjectContext> {
    Ok(ProjectContext {
        id: project.id.to_string(),
        title: project.title()?,
        folder: project.dir.to_string_lossy().into_owned(),
    })
}

/// `export` as a run gets it: each clip with the episode it shows, as the
/// project has it now. An episode removed since has no file or name.
fn export_context(project: &Project, export: &ExportSummary) -> ProjectResult<ExportContext> {
    let mut episodes = HashMap::new();
    let mut clips = Vec::with_capacity(export.items.len());
    for item in &export.items {
        let media = match episodes.entry(item.media_id) {
            Entry::Occupied(known) => known.into_mut(),
            Entry::Vacant(new) => new.insert(match project.media(item.media_id) {
                Ok(m) => Some(m),
                Err(ProjectError::NotFound(_)) => None,
                Err(e) => return Err(e),
            }),
        };
        let media = media.as_ref();
        clips.push(ClipContext {
            name: item.name.clone(),
            path: item.path.clone(),
            status: serde_name(&item.status),
            media_id: item.media_id.to_string(),
            source_path: media.map(|m| m.path.clone()).unwrap_or_default(),
            episode: media
                .map(|m| if m.label.is_empty() { m.file_name.clone() } else { m.label.clone() })
                .unwrap_or_default(),
            start: item.start.into(),
            end: item.end.into(),
            fps: media.and_then(|m| m.fps).map(|fps| fps.to_string()),
        });
    }
    Ok(ExportContext {
        id: export.id.to_string(),
        label: export.label.clone(),
        preset: export.preset.id.clone(),
        out_dir: export.out_dir.clone(),
        created_at: export.created_at,
        merged_file: export.merged.as_ref().and_then(|f| f.path.clone()),
        clips,
        status: export_outcome(export).map(str::to_owned),
    })
}

/// How `export` went, as a run of protocol 2 is told: `done`, `partial`
/// (some of what it was to make is missing) or `failed` (it made nothing).
/// None while it waits or runs, and once it was cancelled: it didn't go
/// any way yet, or went no way the person would be told of.
fn export_outcome(export: &ExportSummary) -> Option<&'static str> {
    if !matches!(export.status, ExportStatus::Done | ExportStatus::Failed) {
        return None;
    }
    let made = |path: &Option<String>, error: &Option<String>| path.is_some() && error.is_none();
    let clips_made = export.items.iter().filter(|i| i.status == ExportStatus::Done && made(&i.path, &i.error)).count();
    let (whole, some) = match &export.merged {
        // Its clips are parts of the one file.
        Some(file) => {
            let file = made(&file.path, &file.error);
            (file && clips_made == export.items.len(), file)
        }
        None => (clips_made == export.items.len(), clips_made > 0),
    };
    Some(match (whole && export.status == ExportStatus::Done, some) {
        (true, _) => "done",
        (false, true) => "partial",
        (false, false) => "failed",
    })
}

/// The name `value` (a unit enum) has in JSON: what the UI calls it too.
pub(crate) fn serde_name(value: &impl Serialize) -> String {
    serde_json::to_value(value).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}

/// Queues, for the export `summary` of `project` that finished whole, the
/// export actions that active plugins turned on for after every export.
/// Off the export's job, which it never fails or holds up: what goes wrong
/// here is only logged.
pub fn queue_after_export(app: &AppHandle, project: &Project, summary: &ExportSummary) {
    let (app, project_id, export_id) = (app.clone(), project.id, summary.id);
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let Some(host) = state.plugins.clone() else { return };
        let plugins = host.store.list().map_err(|e| e.detail());
        let allow = state.library.allows_unreviewed_plugins().map_err(|e| e.detail());
        let (plugins, allow) = match (plugins, allow) {
            (Ok(plugins), Ok(allow)) => (plugins, allow),
            (Err(e), _) | (_, Err(e)) => {
                log::warn!("plugins after export {export_id}: {e}");
                return;
            }
        };
        for (id, action) in after_export_runs(&plugins, allow) {
            let queued = queue_run(
                &app,
                id,
                ActionKind::ExportAction,
                action,
                Trigger::AfterExport,
                Some(project_id),
                Some(export_id),
                None,
            );
            match queued {
                Ok(Some(_)) => {}
                Ok(None) => log::info!("{id} {action} after export {export_id}: waits or runs already"),
                Err(e) => log::warn!("running {id} after export {export_id}: {}", e.message),
            }
        }
    });
}

/// The export actions to run after an export: of each active plugin, those
/// it turned on for after every export, in the order its plugin.json lists
/// them.
fn after_export_runs(plugins: &[Installed], allow_unreviewed: bool) -> Vec<(&PluginId, &str)> {
    plugins
        .iter()
        .filter(|p| active(p, allow_unreviewed))
        .flat_map(|p| {
            let actions = p.manifest.as_ref().map_or(&[][..], |m| &m.export_actions);
            actions.iter().filter(|a| p.record.after_export.contains(&a.id)).map(|a| (&p.record.id, a.id.as_str()))
        })
        .collect()
}

/// The export actions to run after an export that failed or made only some
/// of its clips: of each active plugin, those it turned on for after every
/// export that say they run then too (`afterFailed`), in its order.
fn after_failed_runs(plugins: &[Installed], allow_unreviewed: bool) -> Vec<(&PluginId, &str)> {
    plugins
        .iter()
        .filter(|p| active(p, allow_unreviewed))
        .flat_map(|p| {
            let actions = p.manifest.as_ref().map_or(&[][..], |m| &m.export_actions);
            actions
                .iter()
                .filter(|a| a.after_failed && p.record.after_export.contains(&a.id))
                .map(|a| (&p.record.id, a.id.as_str()))
        })
        .collect()
}

/// The events to run when what `on` names happened: of each active plugin,
/// those waiting for it that the person turned on, in its order.
fn event_runs(plugins: &[Installed], allow_unreviewed: bool, on: EventKind) -> Vec<(&PluginId, &str)> {
    plugins
        .iter()
        .filter(|p| active(p, allow_unreviewed))
        .flat_map(|p| {
            let events = p.manifest.as_ref().map_or(&[][..], |m| &m.events);
            events
                .iter()
                .filter(|e| e.on == on && p.extra.events.contains(&e.id))
                .map(|e| (&p.record.id, e.id.as_str()))
        })
        .collect()
}

/// What a job that ended sets off for plugins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Moment {
    /// An export failed, or made only some of its clips: the export
    /// actions turned on that run then too ([`after_failed_runs`]).
    ExportFailed { project: ProjectId, export: ExportId },
    /// An indexing, analysis or regrouping of a project ended: perhaps the
    /// last of a batch ([`Batches`]).
    BatchJob(ProjectId),
}

/// What `info`, a job that ended, sets off. An export that finished whole
/// queues its runs itself ([`queue_after_export`]), so each runs once, and
/// one that was cancelled tells no one.
fn moment(info: &JobInfo) -> Option<Moment> {
    match (info.kind, info.state, info.project_id) {
        (JobKind::Export, JobState::Failed, Some(project)) => {
            info.export_id.map(|export| Moment::ExportFailed { project, export })
        }
        (JobKind::Ingest | JobKind::Analyze | JobKind::Regroup, state, Some(project)) if state.is_finished() => {
            Some(Moment::BatchJob(project))
        }
        _ => None,
    }
}

/// Tells the plugins of `app` that the job `info` changed, as the job
/// runner's listener does: what one that ended sets off is queued off the
/// listener, which only hands it on.
pub fn job_ended(app: &AppHandle, info: &JobInfo) {
    if moment(info).is_none() {
        return;
    }
    let (app, info) = (app.clone(), info.clone());
    tauri::async_runtime::spawn_blocking(move || hand_on(&info, &app));
}

/// Where [`job_ended`] hands what a job's end sets off: the app, or a
/// test's stand-in.
trait SetOff {
    /// The export `export` of `project` failed, or made only some of its
    /// clips.
    fn export_failed(&self, project: ProjectId, export: ExportId);
    /// `info`, an indexing, analysis or regrouping of a project, ended.
    fn batch_job_ended(&self, info: &JobInfo);
}

impl SetOff for AppHandle {
    fn export_failed(&self, project: ProjectId, export: ExportId) {
        export_failed(self, project, export);
    }

    fn batch_job_ended(&self, info: &JobInfo) {
        batch_job_ended(self, info);
    }
}

/// Hands what `info`, a job that ended, sets off to `to`.
fn hand_on(info: &JobInfo, to: &impl SetOff) {
    match moment(info) {
        Some(Moment::ExportFailed { project, export }) => to.export_failed(project, export),
        Some(Moment::BatchJob(_)) => to.batch_job_ended(info),
        None => {}
    }
}

/// The installed plugins of `app` and the setting, for runs it queues by
/// itself; `what` names those runs in the log when they can't be read.
fn plugins_now(app: &AppHandle, what: &str) -> Option<(Vec<Installed>, bool)> {
    let state = app.try_state::<AppState>()?;
    let host = state.plugins.clone()?;
    let plugins = host.store.list().map_err(|e| e.detail());
    let allow = state.library.allows_unreviewed_plugins().map_err(|e| e.detail());
    match (plugins, allow) {
        (Ok(plugins), Ok(allow)) => Some((plugins, allow)),
        (Err(e), _) | (_, Err(e)) => {
            log::warn!("plugins {what}: {e}");
            None
        }
    }
}

/// Queues, for the export `export` of `project` that failed or made only
/// some of its clips, the export actions turned on that run then too.
/// What goes wrong is only logged.
fn export_failed(app: &AppHandle, project: ProjectId, export: ExportId) {
    let what = format!("after failed export {export}");
    let Some((plugins, allow)) = plugins_now(app, &what) else { return };
    for (id, action) in after_failed_runs(&plugins, allow) {
        let queued = queue_run(
            app,
            id,
            ActionKind::ExportAction,
            action,
            Trigger::AfterExport,
            Some(project),
            Some(export),
            None,
        );
        match queued {
            Ok(Some(_)) => {}
            Ok(None) => log::info!("{id} {action} {what}: waits or runs already"),
            Err(e) => log::warn!("running {id} {what}: {}", e.message),
        }
    }
}

/// Notes that `info`, an indexing, analysis or regrouping of a project,
/// ended; once it was the last of the project's batch, queues the events
/// turned on for an analysis that finished.
fn batch_job_ended(app: &AppHandle, info: &JobInfo) {
    let Some(state) = app.try_state::<AppState>() else { return };
    let (Some(host), Some(project)) = (state.plugins.clone(), info.project_id) else { return };
    if let Some(episodes) = host.batches.ended(info, || state.jobs.jobs()) {
        let event = EventContext { kind: EventKind::AnalysisFinished, at: as_api::now_ms(), episodes };
        queue_event(app, project, event);
    }
}

/// Queues the events that wait for `event`, which happened in `project`,
/// of each active plugin that turned them on. The same event of a plugin
/// waits once: one that waits tells of `event` too, and one that runs was
/// told its own, so another follows it ([`PluginHost::tell`]). What goes
/// wrong is only logged.
fn queue_event(app: &AppHandle, project: ProjectId, event: EventContext) {
    let what = format!("on {} in {project}", event.kind.name());
    let Some((plugins, allow)) = plugins_now(app, &what) else { return };
    let Some(state) = app.try_state::<AppState>() else { return };
    let Some(host) = state.plugins.clone() else { return };
    for (id, name) in event_runs(&plugins, allow, event.kind) {
        let key = RunKey::new(id, ActionKind::Event, name, Some(project), None, None);
        match host.tell(&state.jobs, key, &event, |key| queue_key(app, key, Trigger::Event, None)) {
            Ok(Some(_)) => {}
            Ok(None) => log::info!("{id} {name} {what}: the run that waits tells of it too"),
            Err(e) => log::warn!("running {id} {name} {what}: {}", e.message),
        }
    }
}

/// Notes that the analysis job `job` of `project` analysed what it had to,
/// before it ends: done or failed, an analysis event counts it. One that
/// found its episode analysed already, or handed it to a restart, isn't
/// counted.
pub fn analysis_worked(state: &AppState, project: ProjectId, job: JobId) {
    if let Some(host) = &state.plugins {
        host.batches.worked(project, job);
    }
}

/// The analyses of each project that ended since it last told of a batch,
/// and the jobs of its batches whose end was noted: an analysis event fires
/// once a project's indexing, analysis and regrouping are all over, and
/// counts only analyses that had something to do.
#[derive(Default)]
struct Batches(Mutex<HashMap<ProjectId, Batch>>);

#[derive(Default)]
struct Batch {
    /// The analyses that ended since the last event: done or failed (one
    /// cancelled isn't counted).
    episodes: EpisodeCounts,
    /// The batch jobs whose end was noted, of those the job runner lists.
    noted: HashSet<JobId>,
    /// The analyses that said they analysed something ([`analysis_worked`]),
    /// until their end is noted.
    worked: HashSet<JobId>,
}

impl Batches {
    /// Notes that the analysis `job` of `project` analysed something.
    fn worked(&self, project: ProjectId, job: JobId) {
        self.0.lock().entry(project).or_default().worked.insert(job);
    }

    /// Notes the end of `info`, a batch job of a project (an indexing,
    /// analysis or regrouping), with `jobs` giving the job runner's jobs:
    /// the episodes to tell of when nothing of the batch is left, else
    /// none. A job of the batch that waits or runs is left; so is one that
    /// ended and whose end isn't noted yet: that note tells. The jobs are
    /// read once the note holds the lock, so the last note sees every end
    /// before it: two analyses that end at once tell once, whichever note
    /// comes first.
    fn ended(&self, info: &JobInfo, jobs: impl FnOnce() -> Vec<JobInfo>) -> Option<EpisodeCounts> {
        let project = info.project_id?;
        let mut batches = self.0.lock();
        let jobs = jobs();
        let batch = batches.entry(project).or_default();
        // Only an analysis that analysed something is counted.
        if batch.noted.insert(info.id) && info.kind == JobKind::Analyze && batch.worked.remove(&info.id) {
            match info.state {
                JobState::Done => batch.episodes.done += 1,
                JobState::Failed => batch.episodes.failed += 1,
                _ => {}
            }
        }
        let of_batch: Vec<&JobInfo> = jobs
            .iter()
            .filter(|j| {
                j.project_id == Some(project) && matches!(j.kind, JobKind::Ingest | JobKind::Analyze | JobKind::Regroup)
            })
            .collect();
        // What the runner no longer lists can't be waited for.
        batch.noted.retain(|id| of_batch.iter().any(|j| j.id == *id));
        batch.worked.retain(|id| of_batch.iter().any(|j| j.id == *id));
        let left = of_batch.iter().any(|j| !j.state.is_finished() || !batch.noted.contains(&j.id));
        if left || batch.episodes == EpisodeCounts::default() {
            return None;
        }
        Some(std::mem::take(&mut batch.episodes))
    }
}

/// Of the plugin jobs `runs`, those to stop once plugins AutoScene hasn't
/// reviewed may no longer run: each whose plugin isn't `reviewed`, also
/// when it can't be read now.
fn unreviewed_runs(runs: &[(JobId, PluginId)], reviewed: impl Fn(&PluginId) -> bool) -> Vec<JobId> {
    runs.iter().filter(|(_, id)| !reviewed(id)).map(|(job, _)| *job).collect()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Instant;

    use as_api::{AppSettings, ContentKind, ExportItem, ExportStatus, JobState, NewProject, ProjectKind};
    use as_plugin::pages::MemoryKit;

    use super::*;

    const UV: [&str; 5] = ["uv", "run", "--script", "--locked", "main.py"];

    /// The `plugin.json` of a plugin `id` with the export actions `post` and
    /// `tag`.
    pub(super) fn manifest(id: &str, gpu: bool) -> serde_json::Value {
        serde_json::json!({
            "manifestVersion": 1,
            "id": id,
            "version": "1.0.0",
            "name": format!("Plugin {id}"),
            "description": "Does a thing.",
            "author": {"name": "Someone"},
            "engines": {"autoscene": ">=1.0.0, <2", "protocol": 1},
            "runtime": {"type": "process", "command": {"*": UV}, "gpu": gpu},
            "contributes": {
                "exportActions": [{"id": "post", "title": "Post"}, {"id": "tag", "title": "Tag"}],
                "commands": [
                    {"id": "hello", "title": "Say hello"},
                    {"id": "count", "title": "Count clips", "needsProject": true}
                ]
            }
        })
    }

    /// An installed plugin `id` of `trust` (see [`manifest`]).
    pub(super) fn plugin(id: &str, trust: Trust, gpu: bool) -> Installed {
        let m = as_plugin::manifest::parse_for_any_app(manifest(id, gpu).to_string().as_bytes()).unwrap().manifest;
        Installed {
            record: Record {
                v: RECORD_VERSION,
                id: m.id.clone(),
                version: m.version.clone(),
                source: Source::Package { folder: "1.0.0+0123abcd".into() },
                trust,
                author_key: None,
                permissions: Vec::new(),
                network_hosts: Vec::new(),
                enabled: true,
                after_export: Vec::new(),
                lock_sha256: Some("0".repeat(64)),
                installed_at: 0,
            },
            manifest: Some(m),
            warnings: Vec::new(),
            dir: PathBuf::new(),
            broken: None,
            extra: Extra::default(),
            withdrawn: None,
        }
    }

    fn after(mut p: Installed, actions: &[&str]) -> Installed {
        p.record.after_export = actions.iter().map(|a| a.to_string()).collect();
        p
    }

    /// `p` as if loaded from a folder.
    fn loaded(mut p: Installed) -> Installed {
        p.record.source = Source::Folder { path: "/plugins/dev".into() };
        p.record.lock_sha256 = None;
        p
    }

    /// A plugin host of a library of its own in `tmp`.
    pub(super) fn host_in(tmp: &Path) -> (Library, Arc<PluginHost>) {
        host_reaching(tmp, UreqTransport::new())
    }

    /// [`host_in`], with plugins' `net.fetch` calls going to `net`.
    pub(super) fn host_reaching(tmp: &Path, net: impl Transport + 'static) -> (Library, Arc<PluginHost>) {
        host_with(tmp, net, Box::new(|_: PluginPageEvent| {}))
    }

    /// What a host told the open pages.
    pub(super) type Heard = Arc<Mutex<Vec<PluginPageEvent>>>;

    /// [`host_in`], keeping what it tells the open pages.
    pub(super) fn host_hearing(tmp: &Path) -> (Library, Arc<PluginHost>, Heard) {
        let heard = Heard::default();
        let keep = heard.clone();
        let (lib, host) = host_with(tmp, UreqTransport::new(), Box::new(move |e: PluginPageEvent| keep.lock().push(e)));
        (lib, host, heard)
    }

    fn host_with(tmp: &Path, net: impl Transport + 'static, page_events: PageEvents) -> (Library, Arc<PluginHost>) {
        let secrets = Arc::new(as_plugin::MemorySecrets::default());
        host_of(tmp, net, page_events, (Box::new(NoStart), FakeUv::here(), None), secrets, None)
    }

    /// [`host_in`], whose secrets `secrets` keeps.
    pub(super) fn host_keeping(tmp: &Path, secrets: Arc<dyn as_plugin::SecretStore>) -> (Library, Arc<PluginHost>) {
        let page_events = Box::new(|_: PluginPageEvent| {});
        host_of(tmp, UreqTransport::new(), page_events, (Box::new(NoStart), FakeUv::here(), None), secrets, None)
    }

    /// [`host_hearing`], whose services start through `start`.
    pub(super) fn host_starting(tmp: &Path, start: impl StartService + 'static) -> (Library, Arc<PluginHost>, Heard) {
        host_getting_uv(tmp, start, FakeUv::here())
    }

    /// [`host_starting`], whose uv scripts get uv from `uv`.
    pub(super) fn host_getting_uv(
        tmp: &Path,
        start: impl StartService + 'static,
        uv: Arc<FakeUv>,
    ) -> (Library, Arc<PluginHost>, Heard) {
        host_getting(tmp, start, uv, None)
    }

    /// [`host_getting_uv`], whose plugins get their large files from
    /// `files` (else from AutoScene's cache in `tmp`).
    pub(super) fn host_getting(
        tmp: &Path,
        start: impl StartService + 'static,
        uv: Arc<FakeUv>,
        files: Option<Arc<FakeFiles>>,
    ) -> (Library, Arc<PluginHost>, Heard) {
        let heard = Heard::default();
        let keep = heard.clone();
        let page_events = Box::new(move |e: PluginPageEvent| keep.lock().push(e));
        let secrets = Arc::new(as_plugin::MemorySecrets::default());
        let (lib, host) = host_of(tmp, UreqTransport::new(), page_events, (Box::new(start), uv, files), secrets, None);
        (lib, host, heard)
    }

    /// [`host_starting`], whose catalog is checked with `key` at one
    /// address, which `served` serves.
    pub(super) fn host_cataloged(
        tmp: &Path,
        start: impl StartService + 'static,
        key: &catalog::tests::Key,
        served: &Arc<catalog::tests::Served>,
    ) -> (Library, Arc<PluginHost>, Heard) {
        let heard = Heard::default();
        let keep = heard.clone();
        let page_events = Box::new(move |e: PluginPageEvent| keep.lock().push(e));
        let secrets = Arc::new(as_plugin::MemorySecrets::default());
        let fetch: Arc<dyn catalog::Fetch> = Arc::new(served.clone());
        let checked = Some((key.keys(), fetch));
        let started = (Box::new(start) as Box<dyn StartService>, FakeUv::here(), None);
        let (lib, host) = host_of(tmp, UreqTransport::new(), page_events, started, secrets, checked);
        (lib, host, heard)
    }

    fn host_of(
        tmp: &Path,
        net: impl Transport + 'static,
        page_events: PageEvents,
        (start, uv, files): (Box<dyn StartService>, Arc<FakeUv>, Option<Arc<FakeFiles>>),
        secrets: Arc<dyn as_plugin::SecretStore>,
        checked: Option<(as_plugin::CatalogKeys, Arc<dyn catalog::Fetch>)>,
    ) -> (Library, Arc<PluginHost>) {
        let paths = AppPaths::at(tmp.join("data"));
        let lib = Library::open(&paths).unwrap();
        let root = tmp.join("library").display().to_string();
        lib.update_settings(&AppSettings { library_root: root, ..lib.settings().unwrap() }).unwrap();
        let app = semver::Version::parse("1.0.3-BETA").unwrap();
        let store = Store::open(&paths, lib.shared_db(), app, secrets).unwrap().compile_here();
        let (keys, fetch) = checked.unwrap_or_else(|| {
            let none: Arc<dyn catalog::Fetch> = Arc::new(Arc::new(catalog::tests::Served::default()));
            (as_plugin::CatalogKeys::new(&[]).unwrap(), none)
        });
        let urls = vec![CATALOG_AT.to_owned()];
        let shelf: Arc<dyn catalog::GetPackage> = Arc::new(Arc::new(catalog::tests::Shelf::default()));
        let catalog = CatalogHost::new(&paths, lib.shared_db(), keys, urls, fetch, shelf);
        let pages = Pages::new(Arc::new(MemoryKit::default()), Vec::new());
        let resources: Box<dyn GetResources> = match files {
            Some(files) => Box::new(files),
            None => Box::new(Resources::new(paths.plugin_resources())),
        };
        let host =
            PluginHost::reaching(store, catalog, pages, page_events, Box::new(net), (start, Box::new(uv), resources));
        (lib, Arc::new(host))
    }

    /// Where a test's catalog is served.
    pub(super) const CATALOG_AT: &str = "https://catalog.example/";

    /// The uv the tests' uv scripts are said to start through: AutoScene's
    /// uv never runs in them.
    pub(super) const TEST_UV: &str = "runtimes/uv-test/uv";

    /// AutoScene's uv as a test has it: here already, else got (once, until
    /// it is here), unless its download fails.
    #[derive(Default)]
    pub(super) struct FakeUv {
        here: Mutex<Option<PathBuf>>,
        fails: bool,
        /// How often it was got.
        pub(super) got: std::sync::atomic::AtomicUsize,
    }

    impl FakeUv {
        /// Here already, as most tests have it.
        pub(super) fn here() -> Arc<Self> {
            Arc::new(Self { here: Mutex::new(Some(TEST_UV.into())), ..Self::default() })
        }

        /// Not here yet; its download `fails`, or it comes.
        pub(super) fn missing(fails: bool) -> Arc<Self> {
            Arc::new(Self { fails, ..Self::default() })
        }
    }

    impl GetUv for Arc<FakeUv> {
        fn installed(&self) -> Option<PathBuf> {
            self.here.lock().clone()
        }

        fn install(&self, cancel: &CancelToken, progress: &mut dyn FnMut(u64, u64)) -> PluginResult<PathBuf> {
            if cancel.is_cancelled() {
                return Err(PluginError::Cancelled);
            }
            progress(0, 4);
            if self.fails {
                return Err(PluginError::Download { what: "uv".into(), detail: "connection refused".into() });
            }
            progress(4, 4);
            self.got.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            *self.here.lock() = Some(TEST_UV.into());
            Ok(TEST_UV.into())
        }
    }

    /// Where the tests' large files are said to be kept: they never are.
    pub(super) const TEST_FILES: &str = "cache/plugin-resources-test";

    /// The plugins' large files as a test has them: each missing until it is
    /// got, unless its download fails; never on the disk.
    #[derive(Default)]
    pub(super) struct FakeFiles {
        kept: Mutex<HashSet<(PluginId, String)>>,
        /// Kept files this process hasn't hashed yet: the next look hashes
        /// them, which a cancel ends.
        unchecked: Mutex<HashSet<(PluginId, String)>>,
        fails: bool,
        /// How often files were got.
        pub(super) got: std::sync::atomic::AtomicUsize,
        /// The plugins whose files were removed.
        pub(super) removed: Mutex<Vec<PluginId>>,
        /// The plugins whose folders lost what they no longer name, before a run.
        pub(super) pruned: Mutex<Vec<PluginId>>,
    }

    impl FakeFiles {
        /// None kept yet; their download `fails`, or they come.
        pub(super) fn missing(fails: bool) -> Arc<Self> {
            Arc::new(Self { fails, ..Self::default() })
        }

        fn kept(&self, id: &PluginId, r: &Resource) -> bool {
            self.kept.lock().contains(&(id.clone(), r.name.clone()))
        }

        /// As after the app's next start: every kept file is hashed at the
        /// next look.
        pub(super) fn forget_checks(&self) {
            let kept = self.kept.lock().clone();
            *self.unchecked.lock() = kept;
        }
    }

    impl GetResources for Arc<FakeFiles> {
        fn dir(&self, id: &PluginId) -> PathBuf {
            Path::new(TEST_FILES).join(id.as_str())
        }

        fn missing<'a>(
            &self,
            id: &PluginId,
            rs: &'a [Resource],
            cancel: &CancelToken,
        ) -> PluginResult<Vec<&'a Resource>> {
            if self.needs_check(id, rs) {
                if cancel.is_cancelled() {
                    return Err(PluginError::Cancelled);
                }
                let mut unchecked = self.unchecked.lock();
                for r in rs {
                    unchecked.remove(&(id.clone(), r.name.clone()));
                }
            }
            Ok(rs.iter().filter(|r| !self.kept(id, r)).collect())
        }

        fn needs_check(&self, id: &PluginId, rs: &[Resource]) -> bool {
            let unchecked = self.unchecked.lock();
            rs.iter().any(|r| unchecked.contains(&(id.clone(), r.name.clone())))
        }

        fn to_get(&self, id: &PluginId, rs: &[Resource]) -> u64 {
            rs.iter().filter(|r| !self.kept(id, r)).map(Resource::bytes).sum()
        }

        fn fetch(
            &self,
            id: &PluginId,
            name: &str,
            rs: &[Resource],
            cancel: &CancelToken,
            progress: &mut dyn FnMut(u64, u64),
        ) -> PluginResult<PathBuf> {
            if cancel.is_cancelled() {
                return Err(PluginError::Cancelled);
            }
            let total = self.to_get(id, rs);
            progress(0, total);
            if self.fails {
                return Err(PluginError::Download {
                    what: format!("{name}'s files"),
                    detail: "connection refused".into(),
                });
            }
            progress(total, total);
            self.kept.lock().extend(rs.iter().map(|r| (id.clone(), r.name.clone())));
            self.got.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(self.dir(id))
        }

        fn prune(&self, _: &[(PluginId, Option<Vec<Resource>>)]) {}

        fn prune_unnamed(&self, id: &PluginId, _: &[Resource]) {
            self.pruned.lock().push(id.clone());
        }

        fn remove(&self, id: &PluginId) {
            self.kept.lock().retain(|(kept, _)| kept != id);
            self.removed.lock().push(id.clone());
        }
    }

    /// The `plugin.json` of [`manifest`]'s plugin `id`, of manifest version
    /// 2, naming `resources`.
    pub(super) fn with_resources(id: &str, resources: serde_json::Value) -> serde_json::Value {
        let mut m = manifest(id, false);
        m["manifestVersion"] = serde_json::json!(2);
        m["resources"] = resources;
        m
    }

    /// A large file `name` of `size` bytes at `url`.
    pub(super) fn resource(name: &str, url: &str, size: u64) -> serde_json::Value {
        serde_json::json!({"name": name, "urls": [url], "sha256": package::sha256_hex(name.as_bytes()), "size": size})
    }

    /// No service starts in the tests that don't start one.
    pub(super) struct NoStart;

    impl StartService for NoStart {
        fn start(
            &self,
            _: &Arc<PluginHost>,
            _: &Installed,
            _: Fetched,
            _: as_plugin::OnEvent,
            _: &as_core::CancelToken,
        ) -> PluginResult<Arc<dyn as_plugin::Service>> {
            Err(PluginError::Invalid("No service starts here.".into()))
        }
    }

    #[test]
    fn a_folder_loaded_again_says_what_is_new_and_takes_the_same_yes() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-load-again-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let (lib, host) = host_in(&tmp);
        let m = manifest("dev.a.dev", false);
        let review = |m: &serde_json::Value| {
            let t = folder(&host, &tmp, m);
            let id = t.id.to_string();
            host.keep(t);
            id
        };
        install(&host, &review(&m), true, true, |_| true).unwrap();

        let mut more = m.clone();
        more["version"] = serde_json::json!("1.1.0");
        more["permissions"] = serde_json::json!(["network"]);
        more["network"] = serde_json::json!({"hosts": ["discord.com"]});
        // The setting off, or no typed name: refused, and nothing is asked.
        for (allow, typed) in [(false, true), (true, false)] {
            let t = review(&more);
            assert!(install(&host, &t, allow, typed, |q| panic!("it asked {q:?}")).is_err());
        }
        // A no keeps the review, and the plugin as it was.
        let t = review(&more);
        let mut asked = None;
        let no = install(&host, &t, true, true, |q| {
            asked = Some(q);
            false
        });
        assert_eq!(no.unwrap_err().code, ErrorCode::Cancelled);
        let (title, text, yes) = asked.unwrap();
        assert_eq!((title.as_str(), yes), ("Load Plugin dev.a.dev?", "Load"));
        assert!(text.starts_with("AutoScene has not reviewed it. It runs with your user rights"), "{text}");
        assert!(
            text.contains("\n\nNew in this version:\n· Talks to the internet\n· Talks to discord.com\n\n"),
            "{text}"
        );
        assert!(text.contains("Version: 1.1.0 (1.0.0 is installed)") && text.contains("Folder: "), "{text}");
        let id = PluginId::parse("dev.a.dev").unwrap();
        assert_eq!(host.store.get(&id).unwrap().record.version, "1.0.0");

        let p = install(&host, &t, true, true, |_| true).unwrap();
        assert_eq!(
            (p.record.version.as_str(), p.record.network_hosts.as_slice()),
            ("1.1.0", &["discord.com".to_owned()][..])
        );
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn the_question_says_a_plugin_of_pages_only_runs_no_program() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-pages-question-{}", std::process::id()));
        std::fs::create_dir_all(tmp.join("pages/ui")).unwrap();
        let (lib, host) = host_in(&tmp);
        let m =
            page_only("dev.a.pages", serde_json::json!([{"id": "main", "title": "Main", "entry": "ui/index.html"}]));
        std::fs::write(tmp.join("pages/plugin.json"), m.to_string()).unwrap();
        std::fs::write(tmp.join("pages/ui/index.html"), b"<p>hi</p>").unwrap();
        let t = host.store.stage_folder(&tmp.join("pages")).unwrap();
        let (_, text, _) = install_question(&t, &Widens::default());
        assert!(
            text.starts_with("AutoScene has not reviewed it. It runs no program: its pages run in a sandbox."),
            "{text}"
        );
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn the_question_says_how_it_ran_and_how_it_will_run() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-question-runs-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let (lib, host) = host_in(&tmp);
        let mut t = folder(&host, &tmp, &manifest("dev.a.runs", false));
        let starts =
            Widens { runtime: RuntimeChange { starts_program: true, ..RuntimeChange::default() }, ..Widens::default() };
        let (_, text, _) = install_question(&t, &starts);
        let says = "AutoScene has not reviewed it. The installed version runs no program. This version runs one \
                    with your user rights: it can read, change and send anything you can.";
        assert!(text.starts_with(says), "{text}");
        t.manifest.runtime.kind = RuntimeKind::Wasm;
        let (_, text, _) = install_question(&t, &starts);
        let says = "The installed version runs no program. This version runs one in a sandbox: it sees only its \
                    own folder and what it asks for.";
        assert!(text.contains(says), "{text}");
        drop((t, host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn the_question_says_who_it_is_by_as_far_as_autoscene_checked() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-question-who-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let (lib, host) = host_in(&tmp);
        let mut t = folder(&host, &tmp, &manifest("dev.a.who", false));
        let by = t.manifest.author.name.clone();
        let says = |t: &Ticket| install_question(t, &Widens::default()).1;
        assert!(says(&t).contains(&format!("\nSays it is by: {by}\nNot signed\n")), "{}", says(&t));

        let key = as_plugin::AuthorKey { base64: "a2V5".into(), key_id: "ABCD1234".into() };
        t.verdict = as_plugin::Verdict { trust: Trust::Pinned, author_key: Some(key.clone()) };
        assert!(says(&t).contains(&format!("\nSays it is by: {by}\nSigned with key ABCD1234\n")), "{}", says(&t));

        // A key that isn't what the plugin is held to says nothing of who signed it.
        t.verdict = as_plugin::Verdict { trust: Trust::Unreviewed, author_key: Some(key.clone()) };
        assert!(says(&t).contains("\nNot signed\n"), "{}", says(&t));

        // Reviewed, it is by whom the catalog says, signed or not.
        t.verdict = as_plugin::Verdict { trust: Trust::Reviewed, author_key: Some(key) };
        let text = says(&t);
        assert!(
            text.starts_with("AutoScene reviewed this version.") && text.contains(&format!("\nBy: {by}")),
            "{text}"
        );
        assert!(!text.contains("Says it is by") && !text.contains("igned"), "{text}");
        drop((t, host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn the_question_says_what_is_new_as_the_review_does() {
        use Permission::{ExportsRead, ExportsWrite, Network, ProjectRead};
        let parsed =
            |m: serde_json::Value| as_plugin::manifest::parse_for_any_app(m.to_string().as_bytes()).unwrap().manifest;
        let program = parsed(manifest("dev.a.new", false));
        let boxed = parsed(wasm_manifest("dev.a.new", None));
        let adding = |permissions: &[Permission], hosts: &[&str]| Widens {
            permissions: permissions.to_vec(),
            hosts: hosts.iter().map(|h| (*h).to_owned()).collect(),
            ..Widens::default()
        };
        let cases = [
            (ExportsRead, "Reads the exported clips", "Reads the files in the export's folder"),
            (
                ExportsWrite,
                "Changes or adds files beside the exported clips",
                "Reads, changes and deletes files in the export's folder",
            ),
            (ProjectRead, "Reads the project: its episodes and characters", "Reads the project's folder"),
            (Network, "Talks to the internet", "May use the network, but names no site: every request is refused."),
        ];
        for (p, as_a_program, in_the_sandbox) in cases {
            assert_eq!(new_lines(&program, &adding(&[p], &[])), [as_a_program], "{p:?}");
            assert_eq!(new_lines(&boxed, &adding(&[p], &[])), [in_the_sandbox], "{p:?}");
        }

        // The sites it names: a program's are a line each, the sandbox's end its line on the network.
        let hosts = ["discord.com", "api.example.org"];
        let mut named = boxed.clone();
        named.network_hosts = hosts.map(str::to_owned).to_vec();
        let sites = "Talks to these sites, over https only: discord.com, api.example.org";
        assert_eq!(
            new_lines(&program, &adding(&[Network], &hosts)),
            ["Talks to the internet", "Talks to discord.com", "Talks to api.example.org"]
        );
        assert_eq!(new_lines(&named, &adding(&[Network], &hosts)), [sites]);
        assert_eq!(new_lines(&named, &adding(&[], &hosts)), [sites]);
        assert_eq!(new_lines(&named, &adding(&[Network, ProjectRead], &hosts)), ["Reads the project's folder", sites]);
        assert!(new_lines(&named, &Widens::default()).is_empty());
    }

    /// The `plugin.json` of a plugin `id` that runs no program and has `pages`.
    pub(super) fn page_only(id: &str, pages: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "manifestVersion": 2,
            "id": id,
            "version": "1.0.0",
            "name": format!("Page {id}"),
            "description": "Shows a page.",
            "author": {"name": "Someone"},
            "engines": {"autoscene": ">=1.0.0, <2", "protocol": 2},
            "runtime": {"type": "none"},
            "contributes": {"pages": pages}
        })
    }

    /// A page-only plugin `id` of `trust`, its files in `dir`: installed
    /// from a package, or `loaded` from its folder.
    pub(super) fn paged(dir: &Path, id: &str, trust: Trust, loaded: bool) -> Installed {
        let manifest = page_only(id, serde_json::json!([{"id": "main", "title": "Main", "entry": "ui/index.html"}]));
        let files = [("plugin.json", manifest.to_string().into_bytes()), ("ui/index.html", b"<p>hi</p>".to_vec())];
        for (path, bytes) in &files {
            std::fs::create_dir_all(dir.join("ui")).unwrap();
            std::fs::write(dir.join(path), bytes).unwrap();
        }
        let listed = files.iter().map(|(path, bytes)| as_plugin::LockedFile {
            path: (*path).into(),
            size: bytes.len() as u64,
            sha256: package::sha256_hex(bytes),
            exec: false,
        });
        let lock = serde_json::to_vec(&as_plugin::FilesLock { lock_version: 1, files: listed.collect() }).unwrap();
        std::fs::write(dir.join(package::LOCK), &lock).unwrap();
        let mut p = plugin(id, trust, false);
        p.manifest = Some(as_plugin::manifest::parse_for_any_app(manifest.to_string().as_bytes()).unwrap().manifest);
        p.record.lock_sha256 = Some(package::sha256_hex(&lock));
        p.dir = dir.to_path_buf();
        if loaded { self::loaded(p) } else { p }
    }

    /// Opens the page of `p` (see [`paged`]); gives its session.
    pub(super) fn open_page(host: &PluginHost, p: &Installed) -> String {
        let page = &p.manifest.as_ref().unwrap().pages[0];
        host.pages.open(p, Grants::of(p).unwrap(), page, None).unwrap().0.token.clone()
    }

    /// The pages `heard` was told were closed, with why, in session order.
    pub(super) fn closed(heard: &Heard) -> Vec<(String, String)> {
        let mut closed: Vec<(String, String)> = heard
            .lock()
            .iter()
            .filter_map(|e| match &e.event {
                PageEvent::Closed { reason } => Some((e.session.clone(), reason.clone())),
                _ => None,
            })
            .collect();
        closed.sort();
        closed
    }

    #[test]
    fn a_change_closes_its_pages() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-page-change-{}", std::process::id()));
        let (lib, host, heard) = host_hearing(&tmp);
        let one = paged(&tmp.join("one"), "dev.a.one", Trust::Reviewed, false);
        let two = paged(&tmp.join("two"), "dev.a.two", Trust::Reviewed, false);
        let (first, second, other) = (open_page(&host, &one), open_page(&host, &one), open_page(&host, &two));

        // An update or an uninstall: its pages close before its files go, and say why.
        let changing = host.change(&one.record.id, "One", "It is being updated.").unwrap();
        let mut both = [first.clone(), second.clone()];
        both.sort();
        let updated = |s: &String| (s.clone(), "It is being updated.".to_owned());
        assert_eq!(closed(&heard), both.iter().map(updated).collect::<Vec<_>>());
        assert!(host.pages.get(&first).is_none() && host.pages.get(&second).is_none());
        assert_eq!(host.pages.open_count(&two.record.id), 1);
        drop(changing);
        let (again, _) =
            host.pages.open(&one, Grants::of(&one).unwrap(), &one.manifest.as_ref().unwrap().pages[0], None).unwrap();
        heard.lock().clear();
        drop(host.change(&one.record.id, "One", "It is being uninstalled.").unwrap());
        assert_eq!(closed(&heard), [(again.token.clone(), "It is being uninstalled.".to_owned())]);

        // Its settings: the other plugin's page hears, and stays open.
        host.settings_changed(&two.record.id);
        let last = heard.lock().last().cloned().unwrap();
        assert!(matches!(last.event, PageEvent::Settings) && last.session == other);
        assert!(host.pages.get(&other).is_some());

        // Turned off: its pages close too.
        heard.lock().clear();
        let jobs = JobRunner::new(&[(Class::Plugin, 1)], |_| {});
        host.stop(&jobs, &two.record.id);
        assert_eq!(closed(&heard), [(other.clone(), "It was turned off.".to_owned())]);
        assert_eq!(host.pages.open_count(&two.record.id), 0);
        // As the page hears it.
        let event = serde_json::to_value(&heard.lock()[0]).unwrap();
        assert_eq!(
            event,
            serde_json::json!({"session": other, "event": {"type": "closed", "reason": "It was turned off."}})
        );
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn turning_the_setting_off_closes_unreviewed_pages() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-page-allow-{}", std::process::id()));
        let (lib, host, heard) = host_hearing(&tmp);
        let reviewed = paged(&tmp.join("reviewed"), "dev.a.reviewed", Trust::Reviewed, false);
        let pinned = paged(&tmp.join("pinned"), "dev.a.pinned", Trust::Pinned, false);
        // Its row says reviewed, but a folder never is.
        let folder = paged(&tmp.join("folder"), "dev.a.folder", Trust::Reviewed, true);
        let sessions = [&reviewed, &pinned, &folder].map(|p| open_page(&host, p));
        let jobs = JobRunner::new(&[(Class::Plugin, 1)], |_| {});

        // On, or staying on, closes nothing.
        host.allow_changed(&jobs, false, true);
        host.allow_changed(&jobs, true, true);
        assert!(heard.lock().is_empty());
        // Off: every page of a plugin AutoScene hasn't reviewed closes, and says why.
        host.allow_changed(&jobs, true, false);
        let why = PluginError::NotAllowed.to_string();
        let mut gone = vec![(sessions[1].clone(), why.clone()), (sessions[2].clone(), why)];
        gone.sort();
        assert_eq!(closed(&heard), gone);
        assert!(host.pages.get(&sessions[0]).is_some());
        assert!(host.pages.get(&sessions[1]).is_none() && host.pages.get(&sessions[2]).is_none());
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    /// Writes the plugin of `manifest` into its folder in `tmp`, and checks
    /// it for loading.
    pub(super) fn folder(host: &PluginHost, tmp: &Path, manifest: &serde_json::Value) -> Ticket {
        let dir = tmp.join(manifest["id"].as_str().unwrap());
        std::fs::create_dir_all(&dir).unwrap();
        for file in ["main.py", "main.py.lock"] {
            std::fs::write(dir.join(file), b"").unwrap();
        }
        std::fs::write(dir.join("plugin.json"), manifest.to_string()).unwrap();
        host.store.stage_folder(&dir).unwrap()
    }

    /// The run of `action` of `kind` of the plugin `id`, in `project` and on
    /// `export`.
    pub(super) fn key(
        id: &PluginId,
        kind: ActionKind,
        action: &str,
        project: Option<ProjectId>,
        export: Option<ExportId>,
    ) -> RunKey {
        RunKey::new(id, kind, action, project, export, None)
    }

    #[test]
    fn after_export_runs_only_turned_on_actions_of_active_plugins() {
        let reviewed = after(plugin("dev.a.reviewed", Trust::Reviewed, false), &["tag", "post", "gone"]);
        let pinned = after(plugin("dev.a.pinned", Trust::Pinned, false), &["post"]);
        let unreviewed = after(plugin("dev.a.unreviewed", Trust::Unreviewed, false), &["tag"]);
        let mut off = after(plugin("dev.a.off", Trust::Reviewed, false), &["post"]);
        off.record.enabled = false;
        let mut broken = after(plugin("dev.a.broken", Trust::Reviewed, false), &["post"]);
        broken.broken = Some("Its files are missing. Install it again.".into());
        let none = plugin("dev.a.none", Trust::Reviewed, false);
        let plugins = [reviewed, pinned, unreviewed, off, broken, none];
        let runs = |allow| {
            after_export_runs(&plugins, allow).into_iter().map(|(id, a)| format!("{id} {a}")).collect::<Vec<_>>()
        };

        // In the order the plugin lists them; an action it no longer has is left out.
        assert_eq!(runs(false), ["dev.a.reviewed post", "dev.a.reviewed tag"]);
        // With the setting on, those AutoScene hasn't reviewed run too; never one turned off or broken.
        assert_eq!(
            runs(true),
            ["dev.a.reviewed post", "dev.a.reviewed tag", "dev.a.pinned post", "dev.a.unreviewed tag"]
        );
    }

    #[test]
    fn unreviewed_plugins_stop_when_the_setting_goes_off() {
        let plugins = [
            plugin("dev.a.reviewed", Trust::Reviewed, false),
            plugin("dev.a.pinned", Trust::Pinned, false),
            plugin("dev.a.unreviewed", Trust::Unreviewed, false),
            // Its row says reviewed, but a folder never is.
            loaded(plugin("dev.a.folder", Trust::Reviewed, false)),
        ];
        let id = |s: &str| PluginId::parse(s).unwrap();
        let jobs: Vec<JobId> = (0..5).map(|_| JobId::new()).collect();
        let runs = [
            (jobs[0], id("dev.a.reviewed")),
            (jobs[1], id("dev.a.pinned")),
            (jobs[2], id("dev.a.unreviewed")),
            (jobs[3], id("dev.a.folder")),
            (jobs[4], id("dev.a.gone")),
        ];
        let known = |p: &PluginId| plugins.iter().find(|i| i.record.id == *p).is_some_and(reviewed);
        // Signed by its author is still not reviewed; one that can't be read now stops too.
        assert_eq!(unreviewed_runs(&runs, known), &jobs[1..]);

        // And none of them runs again: each is refused, and says why.
        let allow = false;
        assert!(active(&plugins[0], allow));
        for p in &plugins[1..] {
            assert!(!active(p, allow) && active(p, true));
            assert_eq!(
                runnable(p, allow).unwrap_err().to_string(),
                "Plugins that AutoScene has not reviewed are turned off. You can allow them in Plugins."
            );
        }
        // Plugins shows the folder as what it is.
        assert_eq!(trust(&plugins[3].record), Trust::Unreviewed);
    }

    #[test]
    fn a_withdrawn_plugin_is_not_active_and_says_why() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-withdrawn-{}", std::process::id()));
        let (lib, host) = host_in(&tmp);
        let reason = "It sent export paths to a server.";
        let why = format!("AutoScene withdrew Plugin dev.a.leaky. {reason}");
        let withdraw = |mut p: Installed| {
            p.withdrawn = Some(reason.to_owned());
            p
        };
        // Whatever it is: reviewed, loaded from a folder, or turned off.
        let mut off = plugin("dev.a.leaky", Trust::Reviewed, false);
        off.record.enabled = false;
        for p in
            [plugin("dev.a.leaky", Trust::Reviewed, false), loaded(plugin("dev.a.leaky", Trust::Pinned, false)), off]
        {
            let p = withdraw(p);
            for allow in [false, true] {
                assert!(!active(&p, allow));
                let e = runnable(&p, allow).unwrap_err();
                assert!(matches!(e, PluginError::Withdrawn { .. }), "{e:?}");
                assert_eq!(e.to_string(), why);
            }
            // Settings says so, and that it doesn't run; nothing of it runs after an export.
            let shown = info(&p, &host.store, true);
            assert_eq!((shown.withdrawn.as_deref(), shown.active), (Some(reason), false));
            assert!(after_export_runs(&[after(p, &["post"])], true).is_empty());
        }
        // A broken one says first what is wrong with its files.
        let mut broken = withdraw(plugin("dev.a.leaky", Trust::Reviewed, false));
        broken.broken = Some(as_plugin::store::MISSING.into());
        assert_eq!(runnable(&broken, true).unwrap_err().to_string(), as_plugin::store::MISSING);
        assert_eq!(info(&broken, &host.store, true).withdrawn.as_deref(), Some(reason));
        // Its page doesn't open, and a withdrawal of none is no withdrawal.
        let page = withdraw(paged(&tmp.join("paged"), "dev.a.leaky", Trust::Reviewed, false));
        assert_eq!(
            runnable(&page, true).unwrap_err().to_string(),
            format!("AutoScene withdrew Page dev.a.leaky. {reason}")
        );
        assert!(active(&plugin("dev.a.leaky", Trust::Reviewed, false), false));
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn the_review_says_where_the_catalog_says_a_reviewed_plugin_was_built_from() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-review-catalog-{}", std::process::id()));
        let (lib, host) = host_in(&tmp);
        // The store's verdict gives the approval (its tests check that); a
        // folder is never reviewed, so its ticket stands in for a package's.
        let mut t = folder(&host, &tmp, &manifest("dev.a.reviewed", false));
        assert!(review(&t, &host, true).catalog.is_none());
        let commit = "c".repeat(40);
        t.approval = Some(Approval {
            published: 1_790_000_000,
            repo: "https://github.com/tcmmi/autoscene-plugins".into(),
            commit: commit.clone(),
            path: "plugins/reviewed".into(),
        });
        let shown = serde_json::to_value(review(&t, &host, true).catalog).unwrap();
        let catalog = serde_json::json!({
            "published": 1_790_000_000_000.0,
            "repository": "https://github.com/tcmmi/autoscene-plugins",
            "commit": commit,
            "path": "plugins/reviewed"
        });
        assert_eq!(shown, catalog);
        host.store.discard(t);
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn plugin_jobs_never_carry_a_media_id() {
        let (project, export) = (Some(ProjectId::new()), Some(ExportId::new()));
        let p = plugin("dev.a.post", Trust::Reviewed, false);
        let m = p.manifest.as_ref().unwrap();
        let title = action_title(m, ActionKind::ExportAction, "post", project, export).unwrap();
        let spec = job_spec(m, &title, project, export);
        assert_eq!((spec.kind, spec.class, spec.media_id), (JobKind::Plugin, Class::Plugin, None));
        assert_eq!((spec.title.as_str(), spec.subtitle.as_deref()), ("Post", Some("Plugin dev.a.post")));
        assert_eq!((spec.project_id, spec.export_id), (project, export));
        // One that uses the GPU waits its turn with the analysis.
        let gpu = plugin("dev.a.gpu", Trust::Reviewed, true);
        assert_eq!(job_spec(gpu.manifest.as_ref().unwrap(), "Post", project, export).class, Class::Analysis);
        // A palette command runs without a project, unless it needs one.
        let spec = job_spec(m, &action_title(m, ActionKind::Command, "hello", None, None).unwrap(), None, None);
        assert_eq!((spec.project_id, spec.media_id, spec.export_id), (None, None, None));

        let refused = |kind, action, project, export| action_title(m, kind, action, project, export).unwrap_err();
        let said = |e: PluginError| e.to_string();
        assert_eq!(said(refused(ActionKind::Command, "count", None, None)), "Open a project to run it in.");
        assert_eq!(said(refused(ActionKind::ExportAction, "post", project, None)), "Choose an export to run it on.");
        assert_eq!(said(refused(ActionKind::ExportAction, "post", None, export)), "Choose an export to run it on.");
        assert_eq!(said(refused(ActionKind::Command, "hello", project, export)), "A command doesn't run on an export.");
        assert_eq!(said(refused(ActionKind::Command, "tell", None, None)), "Plugin dev.a.post has no such action.");
        // An action asked for as the other kind (a page names none) says what it needs.
        assert_eq!(said(refused(ActionKind::Command, "post", project, None)), "Choose an export to run it on.");
        assert_eq!(
            said(refused(ActionKind::ExportAction, "hello", project, export)),
            "A command doesn't run on an export."
        );
    }

    #[test]
    fn a_plugins_own_error_stays_its_error_after_a_cancel() {
        let failed = PluginError::Failed { message: "No webhook is set.".into(), detail: None };
        assert!(matches!(plugin_job_error(failed), JobError::FailedFirst(m) if m == "No webhook is set."));
        // What the host says of a run a cancel may have caused gives way to the cancel.
        let crashed = PluginError::Crashed { name: "Poster".into(), detail: "exit code: 1".into() };
        assert!(matches!(plugin_job_error(crashed), JobError::Failed(m) if m == "Poster stopped unexpectedly."));
        assert!(matches!(plugin_job_error(PluginError::Cancelled), JobError::Cancelled));
    }

    #[test]
    fn an_update_shows_what_it_adds() {
        let mut previous = plugin("dev.a.post", Trust::Pinned, false).record;
        previous.permissions = vec![Permission::ExportsRead];
        previous.network_hosts = vec!["discord.com".into()];
        let manifest = serde_json::json!({
            "manifestVersion": 1,
            "id": "dev.a.post",
            "version": "1.1.0",
            "name": "Poster",
            "description": "Posts clips.",
            "author": {"name": "Someone"},
            "engines": {"autoscene": ">=1.0.0", "protocol": 1},
            "runtime": {"type": "process", "command": {"*": UV}},
            "permissions": ["exports:read", "network"],
            "network": {"hosts": ["discord.com", "example.com"]}
        });
        let m = as_plugin::manifest::parse_for_any_app(manifest.to_string().as_bytes()).unwrap().manifest;
        let u = update(&previous, None, &m);
        assert_eq!(
            (u.from_version.as_str(), u.trust_before, u.source_before),
            ("1.0.0", Trust::Pinned, PluginSource::Package)
        );
        assert_eq!(u.added_permissions, [Permission::Network]);
        assert_eq!(u.added_hosts, ["example.com"]);
        assert_eq!((u.added_pages, u.starts_program), (Vec::<String>::new(), false));
        // A package replacing a loaded folder says so: that folder was never reviewed.
        let folder = loaded(plugin("dev.a.post", Trust::Reviewed, false)).record;
        let u = update(&folder, None, &m);
        assert_eq!((u.trust_before, u.source_before), (Trust::Unreviewed, PluginSource::Folder));

        // The pages it adds, by their titles: those the installed version hasn't.
        let pages = |list: serde_json::Value| {
            let m = page_only("dev.a.post", list).to_string();
            as_plugin::manifest::parse_for_any_app(m.as_bytes()).unwrap().manifest
        };
        let mut before = plugin("dev.a.post", Trust::Pinned, false);
        before.manifest = Some(pages(serde_json::json!([{"id": "main", "title": "Main", "entry": "ui/main.html"}])));
        before.extra = Extra::agreed(RuntimeKind::None);
        let after = pages(serde_json::json!([
            {"id": "main", "title": "Home", "entry": "ui/home.html"},
            {"id": "clips", "title": "Clips", "entry": "ui/clips.html"}
        ]));
        assert_eq!(update(&previous, Some(&before), &after).added_pages, ["clips"]);
        assert_eq!(update(&previous, None, &after).added_pages, ["main", "clips"]);

        // A plugin the person agreed to as one that runs no program, which
        // now runs one: said, by what they agreed to, not by its manifest.
        assert!(update(&previous, Some(&before), &m).starts_program);
        assert!(!update(&previous, Some(&before), &after).starts_program, "pages again");
        before.manifest = None;
        assert!(update(&previous, Some(&before), &m).starts_program, "broken, it was still agreed to so");
        for agreed in [Extra::default(), Extra::agreed(RuntimeKind::Process)] {
            before.extra = agreed;
            assert!(!update(&previous, Some(&before), &m).starts_program, "it ran one already");
        }
        assert!(!update(&previous, None, &m).starts_program, "its row gone: nothing said that may not be new");

        // One that keeps its program running while its page is open: said
        // when the person agreed to anything else.
        let mut service =
            page_only("dev.a.post", serde_json::json!([{"id": "main", "title": "Main", "entry": "ui/main.html"}]));
        service["runtime"] = serde_json::json!({"type": "service", "command": {"*": UV}});
        let service = as_plugin::manifest::parse_for_any_app(service.to_string().as_bytes()).unwrap().manifest;
        for agreed in [Extra::default(), Extra::agreed(RuntimeKind::Process), Extra::agreed(RuntimeKind::None)] {
            before.extra = agreed;
            assert!(update(&previous, Some(&before), &service).becomes_service);
            assert!(!update(&previous, Some(&before), &m).becomes_service);
        }
        before.extra = Extra::agreed(RuntimeKind::Service);
        assert!(!update(&previous, Some(&before), &service).becomes_service, "it served already");
        assert!(!update(&previous, None, &service).becomes_service, "its row gone");
    }

    /// The `plugin.json` of the WebAssembly plugin `id`, with `limits`.
    pub(super) fn wasm_manifest(id: &str, limits: Option<serde_json::Value>) -> serde_json::Value {
        let mut m = manifest(id, false);
        m["manifestVersion"] = serde_json::json!(2);
        m["runtime"] = serde_json::json!({"type": "wasm", "component": "plugin.wasm"});
        if let Some(limits) = limits {
            m["runtime"]["limits"] = limits;
        }
        m
    }

    /// The WebAssembly plugin `id` of `trust`, as the person agreed to it.
    fn wasm_plugin(id: &str, trust: Trust, limits: Option<serde_json::Value>) -> Installed {
        let mut p = plugin(id, trust, false);
        let m = wasm_manifest(id, limits).to_string();
        p.manifest = Some(as_plugin::manifest::parse_for_any_app(m.as_bytes()).unwrap().manifest);
        p.extra = Extra::agreed(RuntimeKind::Wasm);
        p
    }

    #[test]
    fn a_wasm_run_never_waits_for_the_gpu() {
        let (project, export) = (Some(ProjectId::new()), Some(ExportId::new()));
        let p = wasm_plugin("dev.a.boxed", Trust::Reviewed, None);
        let m = p.manifest.as_ref().unwrap();
        // A WebAssembly plugin can't ask for the GPU: its runs are plugin jobs.
        let mut gpu = wasm_manifest("dev.a.boxed", None);
        gpu["runtime"]["gpu"] = serde_json::json!(true);
        assert!(as_plugin::manifest::parse_for_any_app(gpu.to_string().as_bytes()).is_err());
        assert!(!m.runtime.gpu);
        let spec = job_spec(m, "Post", project, export);
        assert_eq!((spec.kind, spec.class, spec.media_id), (JobKind::Plugin, Class::Plugin, None));
    }

    #[test]
    fn a_wasm_run_needs_no_ffmpeg() {
        assert!(!needs_tools(&Launch::Wasm { component: "plugin.wasm".into() }));
        assert!(needs_tools(&Launch::UvScript { script: "main.py".into() }));
        assert!(needs_tools(&Launch::Exe { program: "bin/tool.exe".into(), args: Vec::new() }));
    }

    #[test]
    fn a_wasm_plugin_says_it_is_sandboxed_and_its_limits() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-sandbox-info-{}", std::process::id()));
        let (_lib, host) = host_in(&tmp);
        let boxed = info(&wasm_plugin("dev.a.boxed", Trust::Unreviewed, None), &host.store, true);
        assert_eq!(boxed.runtime, RuntimeKind::Wasm);
        assert_eq!(boxed.sandbox, Some(PluginSandbox { memory_mb: 512, minutes: 30 }));
        // It runs no uv script and no service, and its pages may start its runs.
        assert!(!boxed.uv_script && boxed.service_log_path.is_none());
        let limited =
            wasm_plugin("dev.a.small", Trust::Reviewed, Some(serde_json::json!({"memoryMb": 64, "minutes": 2})));
        assert_eq!(info(&limited, &host.store, false).sandbox, Some(PluginSandbox { memory_mb: 64, minutes: 2 }));
        assert!(takes(&limited, Trigger::Page).is_ok());
        // A program isn't, whatever it says; nor is a broken plugin, which doesn't run.
        assert_eq!(info(&plugin("dev.a.program", Trust::Reviewed, false), &host.store, true).sandbox, None);
        let mut broken = wasm_plugin("dev.a.broken", Trust::Reviewed, None);
        (broken.manifest, broken.broken) = (None, Some(as_plugin::store::MISSING.into()));
        assert_eq!(info(&broken, &host.store, true).sandbox, None);
        // The app's page reads it by these names.
        let json = serde_json::to_value(&boxed).unwrap();
        assert_eq!(json["sandbox"], serde_json::json!({"memoryMb": 512, "minutes": 30}));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn an_update_that_leaves_the_sandbox_says_so() {
        let previous = plugin("dev.a.boxed", Trust::Unreviewed, false).record;
        let before = wasm_plugin("dev.a.boxed", Trust::Unreviewed, None);
        let program = plugin("dev.a.boxed", Trust::Unreviewed, false).manifest.unwrap();
        let sandboxed = before.manifest.clone().unwrap();
        let u = update(&previous, Some(&before), &program);
        assert!(u.leaves_sandbox);
        assert!(!u.starts_program && !u.becomes_service);
        // Staying in it, or never in it, says nothing; nor one whose row is gone.
        assert!(!update(&previous, Some(&before), &sandboxed).leaves_sandbox);
        let mut process = before.clone();
        process.extra = Extra::agreed(RuntimeKind::Process);
        assert!(!update(&previous, Some(&process), &program).leaves_sandbox);
        assert!(!update(&previous, None, &program).leaves_sandbox);
        // By what the person agreed to, also while the installed version is broken.
        let mut broken = before.clone();
        broken.manifest = None;
        assert!(update(&previous, Some(&broken), &program).leaves_sandbox);
        // A plugin of pages only runs no program outside it: its pages run
        // in their own sandbox, as the review says.
        let pages =
            page_only("dev.a.boxed", serde_json::json!([{"id": "main", "title": "Main", "entry": "ui/main.html"}]));
        let pages = as_plugin::manifest::parse_for_any_app(pages.to_string().as_bytes()).unwrap().manifest;
        let u = update(&previous, Some(&before), &pages);
        assert!(!u.leaves_sandbox && !u.starts_program);
        // A service leaves it.
        let mut service =
            page_only("dev.a.boxed", serde_json::json!([{"id": "main", "title": "Main", "entry": "ui/main.html"}]));
        service["runtime"] = serde_json::json!({"type": "service", "command": {"*": UV}});
        let service = as_plugin::manifest::parse_for_any_app(service.to_string().as_bytes()).unwrap().manifest;
        assert!(update(&previous, Some(&before), &service).leaves_sandbox);
        let u = update(&previous, Some(&before), &program);
        assert_eq!(serde_json::to_value(&u).unwrap()["leavesSandbox"], serde_json::json!(true));
    }

    #[test]
    fn an_update_that_raises_its_sandbox_limits_says_so() {
        const ID: &str = "dev.a.boxed";
        let previous = plugin(ID, Trust::Reviewed, false).record;
        let before = wasm_plugin(ID, Trust::Reviewed, Some(serde_json::json!({"memoryMb": 512, "minutes": 30})));
        let limits = |memory: u32, minutes: u32| {
            let limits = serde_json::json!({"memoryMb": memory, "minutes": minutes});
            wasm_plugin(ID, Trust::Reviewed, Some(limits)).manifest.unwrap()
        };
        let raised = |before: Option<&Installed>, m: &Manifest| update(&previous, before, m).raised_limits;
        for (memory, minutes, more) in [
            (512, 30, false),
            (256, 10, false),
            (4096, 30, true),
            (512, 1440, true),
            (4096, 10, true),
            (256, 1440, true),
        ] {
            assert_eq!(raised(Some(&before), &limits(memory, minutes)), more, "{memory} MB, {minutes} minutes");
        }
        // Limits it leaves unset are the default ones.
        let unset = wasm_plugin(ID, Trust::Reviewed, None);
        assert!(!raised(Some(&unset), &limits(512, 30)));
        assert!(raised(Some(&unset), &limits(1024, 30)));
        assert!(!raised(Some(&before), unset.manifest.as_ref().unwrap()));
        // It takes no system dialog: its reach is the same.
        assert!(!update(&previous, Some(&before), &limits(4096, 1440)).leaves_sandbox);

        // One new to the sandbox had none to raise, and a program has none.
        let mut process = before.clone();
        process.extra = Extra::agreed(RuntimeKind::Process);
        process.manifest = plugin(ID, Trust::Reviewed, false).manifest;
        assert!(!raised(Some(&process), &limits(4096, 1440)));
        assert!(!raised(Some(&before), &plugin(ID, Trust::Reviewed, false).manifest.unwrap()));
        // Nothing is said of one whose row is gone.
        assert!(!raised(None, &limits(4096, 1440)));
        // What can't be read may have been lower.
        let mut broken = before.clone();
        broken.manifest = None;
        assert!(raised(Some(&broken), &limits(512, 30)));
        let u = update(&previous, Some(&before), &limits(4096, 30));
        assert_eq!(serde_json::to_value(&u).unwrap()["raisedLimits"], serde_json::json!(true));
    }

    /// The library, whose project has one export, `.1`, whatever its id.
    struct GoneExport<'a>(&'a Library, ExportSummary);

    impl calls::Places for GoneExport<'_> {
        fn project(&self, id: ProjectId) -> ProjectResult<Arc<Project>> {
            self.0.project(id)
        }

        fn exports(&self, _: &Project) -> ProjectResult<Vec<ExportSummary>> {
            Ok(vec![self.1.clone()])
        }

        fn export(&self, _: &Project, _: ExportId) -> ProjectResult<ExportSummary> {
            Ok(self.1.clone())
        }
    }

    #[test]
    fn a_wasm_run_gets_its_set_secrets_its_files_and_what_it_runs_on() {
        // The test guest (`cargo xtask wasm-guests`): its component is compiled at load.
        let Some(guest) = as_testkit::fixture_file("wasm/guest.wasm") else { return };
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-wasm-spec-{}", std::process::id()));
        let files = FakeFiles::missing(false);
        let (lib, host, _) = host_getting(&tmp, NoStart, FakeUv::here(), Some(files.clone()));
        // The test guest as the WebAssembly plugin `id`, of `m`.
        let install = |id: &str, m: serde_json::Value| {
            let dir = tmp.join(id);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::copy(&guest, dir.join("plugin.wasm")).unwrap();
            std::fs::write(dir.join("plugin.json"), m.to_string()).unwrap();
            host.store.commit(host.store.stage_folder(&dir).unwrap(), true, true).unwrap().record.id
        };
        let mut m = wasm_manifest("dev.a.keeper", None);
        m["contributes"]["settings"] = serde_json::json!([
            {"key": "token", "type": "secret", "title": "Token"},
            {"key": "spare", "type": "secret", "title": "Spare"}
        ]);
        m["resources"] = serde_json::json!([resource("model.onnx", HF, 300)]);
        m["permissions"] = serde_json::json!(["exports:read"]);
        let id = install("dev.a.keeper", m);
        host.store.secret_set(&id, "token", "s3cret").unwrap();
        let p = host.store.get(&id).unwrap();
        assert!(sandboxed(&p));

        let input = serde_json::json!({"from": "its page"});
        let run = RunKey::new(&id, ActionKind::ExportAction, "post", None, None, Some(&input));
        let action = ActionContext { kind: ActionKind::ExportAction, id: "post".into(), trigger: Trigger::Page };
        let heard = Mutex::new(Vec::new());
        let report = |stage: &str, f: Option<f64>| heard.lock().push((stage.to_owned(), f));
        let cancel = CancelToken::new();
        let spec_in = |places: &dyn calls::Places, p: &Installed, run: &RunKey, cancel: &CancelToken| {
            wasm_spec_for(&host, places, (p, run), action.clone(), Some(input.clone()), cancel, &report)
        };
        let spec_of = |p: &Installed, run: &RunKey, cancel: &CancelToken| spec_in(&lib, p, run, cancel);
        let got = || files.got.load(std::sync::atomic::Ordering::SeqCst);

        // A run that can't start gets none of its files: not in a project
        // the library doesn't have…
        let (project, export) = (Some(ProjectId::new()), Some(ExportId::new()));
        let nowhere = RunKey::new(&id, ActionKind::ExportAction, "post", project, export, Some(&input));
        assert!(spec_of(&p, &nowhere, &cancel).is_err());
        // …nor on an export whose folder is gone.
        let footage = tmp.join("footage/Show S01E02.mkv");
        std::fs::create_dir_all(footage.parent().unwrap()).unwrap();
        std::fs::write(&footage, b"x").unwrap();
        let req = NewProject {
            title: "Show".into(),
            kind: ProjectKind::Series,
            content: ContentKind::Live,
            files: vec![footage.display().to_string()],
        };
        let (summary, _, _) = lib.create_project(&req).unwrap();
        let gone = GoneExport(
            &lib,
            ExportSummary {
                id: ExportId::new(),
                project_id: summary.id,
                created_at: 7,
                preset: as_export::builtin_presets()[0].clone(),
                out_dir: tmp.join("gone").display().to_string(),
                status: ExportStatus::Done,
                items: Vec::new(),
                label: None,
                merged: None,
            },
        );
        let on_gone =
            RunKey::new(&id, ActionKind::ExportAction, "post", Some(summary.id), Some(gone.1.id), Some(&input));
        let Err(JobError::Failed(why)) = spec_in(&gone, &p, &on_gone, &cancel) else { panic!("it started") };
        assert!(why.contains("The export's folder is gone."), "{why}");
        assert!(heard.lock().is_empty());
        assert_eq!((got(), files.pruned.lock().len()), (0, 0));

        let spec = spec_of(&p, &run, &cancel).unwrap();
        // Only the secrets that are set, by key, read before it starts.
        assert_eq!(spec.secrets, BTreeMap::from([("token".into(), "s3cret".into())]));
        // No FFmpeg, which it can't start; what its page handed it.
        assert_eq!((spec.message.tools.ffmpeg.as_str(), spec.message.tools.ffprobe.as_str()), ("", ""));
        assert_eq!(spec.message.input, Some(input.clone()));
        assert_eq!(spec.message.action.id, "post");
        // Its large files, got before it starts, as a program's are: their
        // folder is its /resources.
        assert_eq!(spec.resources_dir, Some(Path::new(TEST_FILES).join("dev.a.keeper")));
        let getting = |f| ("Getting files for Plugin dev.a.keeper".to_owned(), f);
        assert_eq!(*heard.lock(), [getting(None), getting(Some(0.0)), getting(Some(1.0))]);
        assert_eq!(got(), 1);
        // Its folder lost what its version no longer names first.
        assert_eq!(*files.pruned.lock(), std::slice::from_ref(&id));
        // Kept, the next run gets them at once; a cancel while they are
        // checked fails its start.
        heard.lock().clear();
        assert_eq!(spec_of(&p, &run, &cancel).unwrap().resources_dir, Some(Path::new(TEST_FILES).join("dev.a.keeper")));
        assert!(heard.lock().is_empty());
        files.forget_checks();
        let cancelled = CancelToken::new();
        cancelled.cancel();
        assert!(matches!(spec_of(&p, &run, &cancelled), Err(JobError::Cancelled)));
        assert_eq!(*heard.lock(), [("Checking files for Plugin dev.a.keeper".to_owned(), None)]);
        // One that names none gets no /resources, and nothing is got for it.
        let plain = install("dev.a.plain", wasm_manifest("dev.a.plain", None));
        let plain = host.store.get(&plain).unwrap();
        let run = RunKey::new(&plain.record.id, ActionKind::ExportAction, "post", None, None, Some(&input));
        heard.lock().clear();
        assert_eq!(spec_of(&plain, &run, &cancel).unwrap().resources_dir, None);
        assert!(heard.lock().is_empty());
        assert_eq!(got(), 1);
        assert!(!files.pruned.lock().contains(&plain.record.id));
        // A key that isn't its secret is refused.
        assert!(secret_values(&host.store, &id, &["greeting".to_owned()]).is_err());
        // A program runs outside it.
        assert!(!sandboxed(&plugin("dev.a.program", Trust::Reviewed, false)));
        drop((host, lib));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn turning_a_plugin_or_the_setting_off_stops_its_runs() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-host-{}", std::process::id()));
        let (lib, host) = host_in(&tmp);
        // Two plugins loaded from their folders, so neither is reviewed.
        let ids = ["dev.a.one", "dev.a.two"].map(|id| {
            let t = folder(&host, &tmp, &manifest(id, false));
            let shown = review(&t, &host, false);
            assert!(shown.needs_unreviewed && !shown.allowed && shown.update.is_none());
            // A uv script: the review says what uv fetches. AutoScene's uv is here.
            assert!(shown.plugin.uv_script && !shown.uv_missing);
            // A process plugin's runs have a log; it runs no service to have one.
            assert_eq!(shown.plugin.service_log_path, None);
            assert_eq!(
                (shown.plugin.source, shown.plugin.folder.as_str()),
                (PluginSource::Folder, tmp.join(id).to_str().unwrap())
            );
            assert_eq!(shown.plugin.blocked_reason.as_deref(), Some(PluginError::NotAllowed.to_string().as_str()));
            host.store.commit(t, true, true).unwrap().record.id
        });
        // Their folders may be shown.
        assert_eq!(host.loaded_folders(), ids.each_ref().map(|id| tmp.join(id.as_str())));

        let jobs = JobRunner::new(&[(Class::Plugin, 3)], |_| {});
        let (tx, started) = mpsc::channel();
        let run = |id: &str| {
            let (host, tx, id) = (host.clone(), tx.clone(), PluginId::parse(id).unwrap());
            let p = plugin(id.as_str(), Trust::Unreviewed, false);
            jobs.submit(job_spec(p.manifest.as_ref().unwrap(), "Post", None, None), move |ctx| {
                let _running = host.hold(ctx, &id)?;
                tx.send(()).unwrap();
                while !ctx.cancel.is_cancelled() {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(JobError::Cancelled)
            })
        };
        // The third plugin was uninstalled meanwhile.
        let runs = [run("dev.a.one"), run("dev.a.two"), run("dev.a.gone")];
        for _ in runs {
            started.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        let states = || runs.map(|run| jobs.jobs().into_iter().find(|j| j.id == run).unwrap().state);
        let wait_for = |want: [JobState; 3]| {
            let start = Instant::now();
            while states() != want {
                assert!(start.elapsed() < Duration::from_secs(10), "{:?} never became {want:?}", states());
                thread::sleep(Duration::from_millis(5));
            }
        };
        assert!(host.is_running(&ids[0]) && host.is_running(&ids[1]));

        // Turned off, its run stops, and only its.
        host.store.set_enabled(&ids[0], false).unwrap();
        host.stop(&jobs, &ids[0]);
        wait_for([JobState::Cancelled, JobState::Running, JobState::Running]);
        // The setting off: every plugin AutoScene hasn't reviewed stops, and
        // one that can't be read now with them.
        host.allow_changed(&jobs, true, false);
        wait_for([JobState::Cancelled; 3]);
        let start = Instant::now();
        while host.is_running(&ids[1]) {
            assert!(start.elapsed() < Duration::from_secs(10), "a stopped run stays in running");
            thread::sleep(Duration::from_millis(5));
        }
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_plugin_withdrawn_by_a_new_index_has_its_runs_stopped() {
        use catalog::tests::{Key, NOW, Served, index, withdrawal};
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-withdrawn-runs-{}", std::process::id()));
        let (key, site) = (Key::new(), Arc::new(Served::default()));
        let (lib, host, _) = host_cataloged(&tmp, NoStart, &key, &site);
        let ids = ["dev.a.leaky", "dev.a.kept"].map(|id| {
            let t = folder(&host, &tmp, &manifest(id, false));
            host.store.commit(t, true, true).unwrap().record.id
        });
        let jobs = JobRunner::new(&[(Class::Plugin, 2)], |_| {});
        let (tx, started) = mpsc::channel();
        let runs = ids.each_ref().map(|id| {
            let (host, tx, id) = (host.clone(), tx.clone(), id.clone());
            let p = plugin(id.as_str(), Trust::Unreviewed, false);
            jobs.submit(job_spec(p.manifest.as_ref().unwrap(), "Post", None, None), move |ctx| {
                let _running = host.hold(ctx, &id)?;
                tx.send(()).unwrap();
                while !ctx.cancel.is_cancelled() {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(JobError::Cancelled)
            })
        });
        for _ in runs {
            started.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        let states = || runs.map(|run| jobs.jobs().into_iter().find(|j| j.id == run).unwrap().state);
        let wait_for = |want: [JobState; 2]| {
            let start = Instant::now();
            while states() != want {
                assert!(start.elapsed() < Duration::from_secs(10), "{:?} never became {want:?}", states());
                thread::sleep(Duration::from_millis(5));
            }
        };

        // The first index withdraws nothing: both run on.
        site.index(CATALOG_AT, &key, &index(1, vec![], vec![]));
        let (view, withdrawn, changed) = catalog::apply(&host, &jobs, NOW, true);
        assert!(withdrawn.is_empty() && !changed);
        assert!(view.status.ready && !view.status.out_of_date && view.status.error.is_none());
        assert_eq!(states(), [JobState::Running; 2]);

        // The next withdraws one: its run stops, the other's goes on, and it is said withdrawn.
        site.index(CATALOG_AT, &key, &index(2, vec![], vec![withdrawal("dev.a.leaky", "*")]));
        let (_, withdrawn, changed) = catalog::apply(&host, &jobs, NOW + 60, true);
        assert_eq!((withdrawn, changed), (vec![ids[0].clone()], true));
        wait_for([JobState::Cancelled, JobState::Running]);
        let leaky = host.store.get(&ids[0]).unwrap();
        assert_eq!(leaky.withdrawn.as_deref(), Some("It sent export paths to a server."));
        assert!(!active(&leaky, true) && active(&host.store.get(&ids[1]).unwrap(), true));
        // Fetched again, the same index stops nothing more; a failed check
        // keeps what it withdrew.
        let (_, withdrawn, changed) = catalog::apply(&host, &jobs, NOW + 120, true);
        assert!(withdrawn.is_empty() && !changed);
        site.down(CATALOG_AT);
        let (view, withdrawn, changed) = catalog::apply(&host, &jobs, NOW + 180, true);
        assert!(withdrawn.is_empty() && !changed && view.status.error.is_some());
        assert!(host.store.get(&ids[0]).unwrap().withdrawn.is_some());
        assert_eq!(states(), [JobState::Cancelled, JobState::Running]);
        jobs.cancel_where(|_| true);
        wait_for([JobState::Cancelled; 2]);
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_plugin_runs_once_at_a_time_and_never_while_it_changes() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-busy-{}", std::process::id()));
        let (lib, host) = host_in(&tmp);
        let id = PluginId::parse("dev.a.one").unwrap();
        // A loaded folder's plugin.json moved it to the GPU while a run of it ran.
        let jobs = JobRunner::new(&[(Class::Plugin, 1), (Class::Analysis, 1)], |_| {});
        let (tx, held) = mpsc::channel();
        let run = |class, done: Arc<AtomicBool>| {
            let (host, tx, id) = (host.clone(), tx.clone(), id.clone());
            let spec = JobSpec {
                class,
                ..job_spec(plugin(id.as_str(), Trust::Unreviewed, false).manifest.as_ref().unwrap(), "Post", None, None)
            };
            jobs.submit(spec, move |ctx| {
                let _running = host.hold(ctx, &id)?;
                tx.send(ctx.id).unwrap();
                while !done.load(Ordering::SeqCst) && !ctx.cancel.is_cancelled() {
                    thread::sleep(Duration::from_millis(5));
                }
                Ok(())
            })
        };
        let job = |id: JobId| jobs.jobs().into_iter().find(|j| j.id == id).unwrap();
        let flag = || Arc::new(AtomicBool::new(false));
        let release = |done: &Arc<AtomicBool>| done.store(true, Ordering::SeqCst);
        let wait = Duration::from_millis(300);

        let first_done = flag();
        let first = run(Class::Plugin, first_done.clone());
        assert_eq!(held.recv_timeout(Duration::from_secs(10)).unwrap(), first);
        // No update or uninstall while it runs.
        let busy = host.change(&id, "One", "It is being updated.").err().unwrap();
        assert_eq!(busy.to_string(), "One is running. Wait for it to finish, or stop it in Activity.");
        // A second run on the other class waits for the first.
        let second_done = flag();
        let second = run(Class::Analysis, second_done.clone());
        assert!(held.recv_timeout(wait).is_err());
        assert_eq!(job(second).stage.as_deref(), Some("Waiting for its other run"));
        release(&first_done);
        assert_eq!(held.recv_timeout(Duration::from_secs(10)).unwrap(), second);
        release(&second_done);
        while host.is_running(&id) {
            thread::sleep(Duration::from_millis(5));
        }

        // A run that starts while the plugin changes waits for the change.
        let changing = host.change(&id, "One", "It is being updated.").unwrap();
        let third = run(Class::Plugin, flag());
        assert!(held.recv_timeout(wait).is_err());
        assert_eq!(job(third).stage.as_deref(), Some("Waiting while it is updated"));
        drop(changing);
        assert_eq!(held.recv_timeout(Duration::from_secs(10)).unwrap(), third);
        jobs.cancel(third);
        // Cancelled while it waits, it ends so.
        while host.is_running(&id) {
            thread::sleep(Duration::from_millis(5));
        }
        let changing = host.change(&id, "One", "It is being updated.").unwrap();
        let fourth = run(Class::Analysis, flag());
        while job(fourth).stage.is_none() {
            thread::sleep(Duration::from_millis(5));
        }
        jobs.cancel(fourth);
        let start = Instant::now();
        while job(fourth).state != JobState::Cancelled {
            assert!(start.elapsed() < Duration::from_secs(10), "a waiting run never stopped");
            thread::sleep(Duration::from_millis(5));
        }
        assert!(!host.is_running(&id));
        drop(changing);
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn the_same_run_is_queued_once_and_what_waits_stops_too() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-runs-{}", std::process::id()));
        let (lib, host) = host_in(&tmp);
        let ids = ["dev.a.one", "dev.a.two"]
            .map(|id| host.store.commit(folder(&host, &tmp, &manifest(id, false)), true, true).unwrap().record.id);
        let jobs = JobRunner::new(&[(Class::Plugin, 1)], |_| {});
        let (tx, started) = mpsc::channel();
        let (project, export, other) = (Some(ProjectId::new()), Some(ExportId::new()), Some(ExportId::new()));
        // Every run looks alike in Activity: one plugin is the other's fork,
        // with its name and the same action.
        let submit = |key: RunKey| {
            let tx = tx.clone();
            let spec = JobSpec {
                kind: JobKind::Plugin,
                class: Class::Plugin,
                title: "Post".into(),
                subtitle: Some("Discord".into()),
                project_id: key.project_id,
                media_id: None,
                export_id: key.export_id,
            };
            host.submit(&jobs, key, spec, move |ctx| {
                tx.send(ctx.id).unwrap();
                while !ctx.cancel.is_cancelled() {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(JobError::Cancelled)
            })
        };
        let post = key(&ids[0], ActionKind::ExportAction, "post", project, export);
        let first = submit(post.clone()).unwrap();
        assert_eq!(started.recv_timeout(Duration::from_secs(10)).unwrap(), first);
        // The same run again isn't queued; on another export, or of the other plugin, it is.
        assert_eq!(submit(post.clone()), None);
        let elsewhere = submit(key(&ids[0], ActionKind::ExportAction, "post", project, other)).unwrap();
        let fork = submit(key(&ids[1], ActionKind::ExportAction, "post", project, export)).unwrap();
        let state = |id: JobId| jobs.jobs().into_iter().find(|j| j.id == id).unwrap().state;
        assert_eq!([state(elsewhere), state(fork)], [JobState::Queued; 2]);

        // The setting going on, or staying on, stops nothing.
        host.allow_changed(&jobs, false, true);
        host.allow_changed(&jobs, true, true);
        assert_eq!(
            [state(first), state(elsewhere), state(fork)],
            [JobState::Running, JobState::Queued, JobState::Queued]
        );
        // Turned off, a plugin's runs stop, also the one still waiting; the fork's runs on.
        host.stop(&jobs, &ids[0]);
        assert_eq!(state(elsewhere), JobState::Cancelled);
        assert_eq!(started.recv_timeout(Duration::from_secs(10)).unwrap(), fork);
        assert_eq!(state(first), JobState::Cancelled);
        // One that ended doesn't keep the same run from being queued again.
        let again = submit(post).unwrap();
        // The setting off: what waits stops with what runs.
        host.allow_changed(&jobs, true, false);
        assert_eq!(state(again), JobState::Cancelled);
        let start = Instant::now();
        while state(fork) != JobState::Cancelled {
            assert!(start.elapsed() < Duration::from_secs(10), "the fork's run never stopped");
            thread::sleep(Duration::from_millis(5));
        }
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_run_starts_only_as_it_was_queued() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-start-{}", std::process::id()));
        let (lib, host) = host_in(&tmp);
        let id = host.store.commit(folder(&host, &tmp, &manifest("dev.a.one", false)), true, true).unwrap().record.id;
        let allow = |on| lib.set_allows_unreviewed_plugins(on).unwrap();
        let said_to = |key: &RunKey, trigger, gpu| match startable(&host.store, &lib, key, trigger, gpu) {
            Ok(_) => "starts".to_owned(),
            Err(JobError::Failed(why) | JobError::FailedFirst(why)) => why,
            Err(JobError::Cancelled) => "cancelled".to_owned(),
        };
        let said = |key: &RunKey, gpu| said_to(key, Trigger::Menu, gpu);
        let (project, export) = (Some(ProjectId::new()), Some(ExportId::new()));
        let post = key(&id, ActionKind::ExportAction, "post", project, export);
        let tag = key(&id, ActionKind::ExportAction, "tag", project, export);
        let hello = key(&id, ActionKind::Command, "hello", None, None);

        // Loaded from a folder, it is never reviewed: the setting decides.
        assert_eq!(
            said(&post, false),
            "Plugins that AutoScene has not reviewed are turned off. You can allow them in Plugins."
        );
        allow(true);
        assert_eq!([said(&post, false), said(&hello, false)], ["starts", "starts"]);
        // Its plugin.json changed while the run waited.
        assert_eq!(said(&post, true), "Plugin dev.a.one changed since this run was queued. Run it again.");
        // A page's run starts only while its program speaks protocol 2.
        let stale = "Plugin dev.a.one changed since this run was queued. Run it again.";
        assert_eq!(said_to(&hello, Trigger::Page, false), stale);
        let mut two = manifest("dev.a.one", false);
        two["engines"]["protocol"] = serde_json::json!(2);
        std::fs::write(tmp.join("dev.a.one").join("plugin.json"), two.to_string()).unwrap();
        assert_eq!(said_to(&hello, Trigger::Page, false), "starts");
        let mut changed = manifest("dev.a.one", false);
        changed["contributes"]["exportActions"] = serde_json::json!([{"id": "tag", "title": "Tag"}]);
        changed["contributes"]["commands"][0]["needsProject"] = serde_json::json!(true);
        std::fs::write(tmp.join("dev.a.one").join("plugin.json"), changed.to_string()).unwrap();
        assert_eq!(said(&post, false), "Plugin dev.a.one has no such action.");
        assert_eq!(said(&hello, false), "Open a project to run it in.");
        assert_eq!(said(&tag, false), "starts");
        // Turned off, or gone.
        host.store.set_enabled(&id, false).unwrap();
        assert_eq!(said(&tag, false), "Plugin dev.a.one is turned off.");
        host.store.uninstall(&id, false).unwrap();
        assert_eq!(said(&tag, false), "dev.a.one isn't installed.");
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    /// Waiting behind another run or an analysis, a run that starts by
    /// itself may outlast the person's wish for it: it may post to the
    /// network, so it asks again as it starts.
    #[test]
    fn what_runs_by_itself_stays_off_once_the_person_turned_it_off() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-turned-off-{}", std::process::id()));
        let (lib, host) = host_in(&tmp);
        lib.set_allows_unreviewed_plugins(true).unwrap();
        let mut m = manifest("dev.a.tell", false);
        m["manifestVersion"] = serde_json::json!(2);
        m["contributes"]["events"] =
            serde_json::json!([{"id": "done", "title": "Tell the channel", "on": "analysisFinished"}]);
        let id = host.store.commit(folder(&host, &tmp, &m), true, true).unwrap().record.id;
        let starts = |key: &RunKey, trigger| match startable(&host.store, &lib, key, trigger, false) {
            Ok(_) => "starts".to_owned(),
            Err(JobError::Cancelled) => "cancelled".to_owned(),
            Err(JobError::Failed(why) | JobError::FailedFirst(why)) => why,
        };
        let (project, export) = (Some(ProjectId::new()), Some(ExportId::new()));

        // An event: queued while it was on, it starts only while it still is.
        let event = key(&id, ActionKind::Event, "done", project, None);
        host.store.set_event(&id, "done", true).unwrap();
        assert_eq!(starts(&event, Trigger::Event), "starts");
        host.store.set_event(&id, "done", false).unwrap();
        assert_eq!(starts(&event, Trigger::Event), "cancelled");
        host.store.set_event(&id, "done", true).unwrap();
        assert_eq!(starts(&event, Trigger::Event), "starts");

        // After an export, also one that failed: the same.
        let post = key(&id, ActionKind::ExportAction, "post", project, export);
        host.store.set_after_export(&id, "post", true).unwrap();
        assert_eq!(starts(&post, Trigger::AfterExport), "starts");
        host.store.set_after_export(&id, "post", false).unwrap();
        assert_eq!(starts(&post, Trigger::AfterExport), "cancelled");
        // From the menu it is the person's own run, turned on or not.
        assert_eq!(starts(&post, Trigger::Menu), "starts");
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_run_gets_its_project_and_export_as_they_are_now() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-place-{}", std::process::id()));
        let (lib, host) = host_in(&tmp);
        let files = ["Show S01E02.mkv", "Pilot.mkv", "Gone.mkv"].map(|name| {
            let file = tmp.join("footage").join(name);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, b"x").unwrap();
            file.display().to_string()
        });
        let req = NewProject {
            title: "Show".into(),
            kind: ProjectKind::Series,
            content: ContentKind::Live,
            files: files.to_vec(),
        };
        let (summary, project, _) = lib.create_project(&req).unwrap();
        let episodes = project.media_list_uncounted().unwrap();
        let [show, pilot, gone] = ["Show S01E02.mkv", "Pilot.mkv", "Gone.mkv"]
            .map(|name| episodes.iter().find(|m| m.file_name == name).unwrap().id);
        project.remove_media(gone).unwrap();
        let clip = |media_id, name: &str, start, end| ExportItem {
            id: 0,
            media_id,
            start,
            end,
            name: name.into(),
            status: ExportStatus::Done,
            path: Some(format!("/out/{name}.mp4")),
            verified: true,
            encoder: None,
            bytes: None,
            error: None,
        };
        let export = ExportSummary {
            id: ExportId::new(),
            project_id: summary.id,
            created_at: 7,
            preset: as_export::builtin_presets()[0].clone(),
            out_dir: "/out".into(),
            status: ExportStatus::Done,
            items: vec![
                clip(show, "One", 10, 20),
                clip(pilot, "Two", 0, 5),
                clip(gone, "Three", 3, 4),
                clip(show, "Four", 30, 40),
            ],
            label: Some("Walter · scenepack".into()),
            merged: None,
        };

        let got = export_context(&project, &export).unwrap();
        assert_eq!((got.id, got.label.as_deref()), (export.id.to_string(), Some("Walter · scenepack")));
        assert_eq!((got.preset, got.out_dir.as_str(), got.created_at), (export.preset.id.clone(), "/out", 7));
        // Every clip made: it went whole (a run of protocol 1 isn't told).
        assert_eq!(got.status.as_deref(), Some("done"));
        let clips: Vec<_> = got
            .clips
            .iter()
            .map(|c| (c.name.as_str(), c.episode.as_str(), c.source_path.as_str(), c.start, c.end))
            .collect();
        // An episode is named by its number, else its file; one removed since has neither.
        assert_eq!(
            clips,
            [
                ("One", "S01E02", files[0].as_str(), 10, 20),
                ("Two", "Pilot.mkv", files[1].as_str(), 0, 5),
                ("Three", "", "", 3, 4),
                ("Four", "S01E02", files[0].as_str(), 30, 40),
            ]
        );
        let c = &got.clips[0];
        assert_eq!(
            (c.status.as_str(), c.path.as_deref(), c.media_id.clone()),
            ("done", Some("/out/One.mp4"), show.to_string())
        );

        // A command in the project gets the project, and no export.
        let p = host.store.commit(folder(&host, &tmp, &manifest("dev.a.one", false)), true, true).unwrap();
        let action = ActionContext { kind: ActionKind::Command, id: "count".into(), trigger: Trigger::Palette };
        let mut message =
            host.store.run_spec(&p, action, Path::new("ffmpeg"), Path::new("ffprobe"), "9.0").unwrap().message;
        let count = key(&p.record.id, ActionKind::Command, "count", Some(summary.id), None);
        add_context(&mut message, &lib, &count, None).unwrap();
        let place = message.project.as_ref().unwrap();
        assert_eq!(
            (place.id.clone(), place.title.as_str(), place.folder.clone()),
            (summary.id.to_string(), "Show", summary.dir)
        );
        assert!(message.export.is_none());
        // An export removed since stops the run.
        let removed = key(&p.record.id, ActionKind::ExportAction, "post", Some(summary.id), Some(ExportId::new()));
        assert!(matches!(add_context(&mut message, &lib, &removed, None), Err(ProjectError::NotFound(_))));

        // A run of protocol 1 is never told how the export went: its line
        // stays as it was. One of protocol 2 is.
        struct Exported<'a>(&'a Library, &'a ExportSummary);
        impl calls::Places for Exported<'_> {
            fn project(&self, id: ProjectId) -> ProjectResult<Arc<Project>> {
                self.0.project(id)
            }
            fn exports(&self, _: &Project) -> ProjectResult<Vec<ExportSummary>> {
                Ok(vec![self.1.clone()])
            }
            fn export(&self, _: &Project, _: ExportId) -> ProjectResult<ExportSummary> {
                Ok(self.1.clone())
            }
        }
        let post = key(&p.record.id, ActionKind::ExportAction, "post", Some(summary.id), Some(export.id));
        let action =
            || ActionContext { kind: ActionKind::ExportAction, id: "post".into(), trigger: Trigger::AfterExport };
        let mut message =
            host.store.run_spec(&p, action(), Path::new("ffmpeg"), Path::new("ffprobe"), "9.0").unwrap().message;
        assert_eq!(message.protocol, 1);
        add_context(&mut message, &Exported(&lib, &export), &post, None).unwrap();
        let told = message.export.as_ref().map(|e| (e.id.clone(), e.status.clone()));
        assert_eq!(told, Some((export.id.to_string(), None)));
        message.protocol = 2;
        add_context(&mut message, &Exported(&lib, &export), &post, None).unwrap();
        assert_eq!(message.export.and_then(|e| e.status).as_deref(), Some("done"));
        drop((host, project, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_uv_script_gets_uv_before_it_runs() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-uv-run-{}", std::process::id()));
        let uv = FakeUv::missing(false);
        let (lib, host, _) = host_getting_uv(&tmp, NoStart, uv.clone());
        let p = host.store.commit(folder(&host, &tmp, &manifest("dev.a.uv", false)), true, true).unwrap();
        let action = ActionContext { kind: ActionKind::Command, id: "hello".into(), trigger: Trigger::Palette };
        let spec = host.store.run_spec(&p, action.clone(), Path::new("ffmpeg"), Path::new("ffprobe"), "9.0").unwrap();
        assert!(matches!(spec.launch, Launch::UvScript { .. }) && spec.uv.is_none());
        let heard = Mutex::new(Vec::new());
        let report = |stage: &str, f: Option<f64>| heard.lock().push((stage.to_owned(), f));
        // Not here yet: the run gets it before it starts, says so, and its
        // program starts through it.
        let cancel = CancelToken::new();
        let tools = (Path::new("ffmpeg"), Path::new("ffprobe"), "9.0");
        let run = |action: &ActionContext| host.run_spec(&p, action.clone(), tools, &cancel, &report).unwrap();
        assert_eq!(run(&action).uv, Some(PathBuf::from(TEST_UV)));
        let getting = |f| (GETTING_UV.to_owned(), f);
        assert_eq!(*heard.lock(), [getting(None), getting(Some(0.0)), getting(Some(1.0))]);
        // Here, the next run starts through it at once, without a word.
        heard.lock().clear();
        assert_eq!(run(&action).uv, Some(PathBuf::from(TEST_UV)));
        assert!(heard.lock().is_empty());
        assert_eq!(uv.got.load(std::sync::atomic::Ordering::SeqCst), 1);
        // A program needs none.
        let program = Launch::Exe { program: "bin/tool".into(), args: Vec::new() };
        assert_eq!(host.uv_for(&program, &cancel, &report).unwrap(), None);
        drop((host, lib));

        // A download that fails fails the run in its words; a cancel cancels it.
        let (lib, host, _) = host_getting_uv(&tmp.join("failing"), NoStart, FakeUv::missing(true));
        let failed = plugin_job_error(host.uv_for(&spec.launch, &cancel, &report).unwrap_err());
        let words = "AutoScene couldn't get uv. Check the internet connection, then try again.";
        assert!(matches!(&failed, JobError::Failed(m) if m == words), "{failed:?}");
        cancel.cancel();
        let cancelled = plugin_job_error(host.uv_for(&spec.launch, &cancel, &report).unwrap_err());
        assert!(matches!(cancelled, JobError::Cancelled), "{cancelled:?}");
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn the_review_says_uv_will_be_got() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-uv-review-{}", std::process::id()));
        let uv = FakeUv::missing(false);
        let (lib, host, _) = host_getting_uv(&tmp, NoStart, uv.clone());
        let t = folder(&host, &tmp, &manifest("dev.a.uv", false));
        let shown = review(&t, &host, true);
        assert!(shown.plugin.uv_script && shown.uv_missing);
        // What it downloads at most: uv's archive for this computer, about 15 MB.
        assert_eq!(shown.uv_bytes, as_plugin::uv::download_bytes() as f64);
        assert!((10e6..30e6).contains(&shown.uv_bytes), "{}", shown.uv_bytes);
        let sent = serde_json::to_value(&shown).unwrap();
        assert_eq!(
            (&sent["uvMissing"], &sent["uvBytes"]),
            (&serde_json::json!(true), &serde_json::json!(shown.uv_bytes))
        );
        // Once it is here, nothing is got.
        uv.install(&CancelToken::new(), &mut |_, _| {}).unwrap();
        assert!(!review(&t, &host, true).uv_missing);
        host.store.discard(t);
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    /// Where the tests' plugins' files come from: a pinned Hugging Face
    /// revision, and a host of its own.
    pub(super) const HF: &str =
        "https://huggingface.co/org/repo/resolve/0123456789abcdef0123456789abcdef01234567/model.onnx";
    const CDN: &str = "https://cdn.example.com/vocab.bin";

    #[test]
    fn a_plugin_gets_its_resources_before_it_runs() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-files-run-{}", std::process::id()));
        let (uv, files) = (FakeUv::missing(false), FakeFiles::missing(false));
        let (lib, host, _) = host_getting(&tmp, NoStart, uv.clone(), Some(files.clone()));
        let big = with_resources(
            "dev.a.big",
            serde_json::json!([resource("model.onnx", HF, 300), resource("vocab.bin", CDN, 12)]),
        );
        let p = host.store.commit(folder(&host, &tmp, &big), true, true).unwrap();
        let action = ActionContext { kind: ActionKind::Command, id: "hello".into(), trigger: Trigger::Palette };
        let heard = Mutex::new(Vec::new());
        let report = |stage: &str, f: Option<f64>| heard.lock().push((stage.to_owned(), f));
        let cancel = CancelToken::new();
        let tools = (Path::new("ffmpeg"), Path::new("ffprobe"), "9.0");
        let run = |action: &ActionContext| host.run_spec(&p, action.clone(), tools, &cancel, &report);
        // Not kept yet: after uv, the run gets them, says so, and its program
        // is given their folder.
        let spec = run(&action).unwrap();
        let kept_in = Path::new(TEST_FILES).join("dev.a.big");
        assert_eq!((spec.uv, spec.resources), (Some(PathBuf::from(TEST_UV)), Some(kept_in.clone())));
        let files_stage = |f| ("Getting files for Plugin dev.a.big".to_owned(), f);
        let uv_stage = |f| (GETTING_UV.to_owned(), f);
        let said = vec![
            uv_stage(None),
            uv_stage(Some(0.0)),
            uv_stage(Some(1.0)),
            files_stage(None),
            files_stage(Some(0.0)),
            files_stage(Some(1.0)),
        ];
        assert_eq!(*heard.lock(), said);
        // Kept, the next run is given them at once, without a word.
        heard.lock().clear();
        assert_eq!(run(&action).unwrap().resources, Some(kept_in.clone()));
        assert!(heard.lock().is_empty());
        assert_eq!(files.got.load(std::sync::atomic::Ordering::SeqCst), 1);
        // After the app's next start, the first run hashes them, and says so
        // first; a cancel ends the hash.
        files.forget_checks();
        let cancelled = CancelToken::new();
        cancelled.cancel();
        let e = host.run_spec(&p, action.clone(), tools, &cancelled, &report).err().unwrap();
        assert!(matches!(plugin_job_error(e), JobError::Cancelled));
        let checking = ("Checking files for Plugin dev.a.big".to_owned(), None);
        assert_eq!(*heard.lock(), std::slice::from_ref(&checking));
        heard.lock().clear();
        assert_eq!(run(&action).unwrap().resources, Some(kept_in));
        assert_eq!(*heard.lock(), [checking]);
        heard.lock().clear();
        assert_eq!(files.got.load(std::sync::atomic::Ordering::SeqCst), 1);
        // A plugin that names none gets none, and nothing is got for it.
        let plain = host.store.commit(folder(&host, &tmp, &manifest("dev.a.plain", false)), true, true).unwrap();
        let spec = host.run_spec(&plain, action.clone(), tools, &cancel, &report).unwrap();
        assert_eq!(spec.resources, None);
        assert!(heard.lock().is_empty());
        drop((host, lib));

        // A download that fails fails the run in its words; a cancel cancels it.
        let (lib, host, _) =
            host_getting(&tmp.join("failing"), NoStart, FakeUv::here(), Some(FakeFiles::missing(true)));
        let p = host.store.commit(folder(&host, &tmp.join("failing"), &big), true, true).unwrap();
        let e = host.run_spec(&p, action.clone(), tools, &cancel, &report).err().unwrap();
        let words = "AutoScene couldn't get Plugin dev.a.big's files. Check the internet connection, then try again.";
        assert!(matches!(plugin_job_error(e), JobError::Failed(m) if m == words));
        cancel.cancel();
        let e = host.run_spec(&p, action.clone(), tools, &cancel, &report).err().unwrap();
        assert!(matches!(plugin_job_error(e), JobError::Cancelled));
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn the_review_says_what_a_plugin_downloads_and_from_where() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-files-review-{}", std::process::id()));
        let files = FakeFiles::missing(false);
        let (lib, host, _) = host_getting(&tmp, NoStart, FakeUv::here(), Some(files.clone()));
        let big = with_resources(
            "dev.a.big",
            serde_json::json!([resource("model.onnx", HF, 300_000_000), resource("vocab.bin", CDN, 12_000_000)]),
        );
        let t = folder(&host, &tmp, &big);
        let shown = review(&t, &host, true);
        assert_eq!(shown.resource_bytes, 312e6);
        assert_eq!(shown.resource_hosts, ["huggingface.co", "cdn.example.com"]);
        let sent = serde_json::to_value(&shown).unwrap();
        assert_eq!(sent["resourceBytes"], serde_json::json!(312e6));
        assert_eq!(sent["resourceHosts"], serde_json::json!(["huggingface.co", "cdn.example.com"]));
        // Kept already: nothing to download, from the same hosts.
        let rs = &t.manifest.resources;
        files.fetch(&t.manifest.id, "Big", rs, &CancelToken::new(), &mut |_, _| {}).unwrap();
        let shown = review(&t, &host, true);
        assert_eq!((shown.resource_bytes, shown.resource_hosts.len()), (0.0, 2));
        // A plugin that names none downloads none.
        let plain = folder(&host, &tmp, &manifest("dev.a.plain", false));
        let shown = review(&plain, &host, true);
        assert_eq!((shown.resource_bytes, shown.resource_hosts.len()), (0.0, 0));
        host.store.discard(t);
        host.store.discard(plain);
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn an_update_with_a_new_download_host_marks_it_new() {
        let parse =
            |v: &serde_json::Value| as_plugin::manifest::parse_for_any_app(v.to_string().as_bytes()).unwrap().manifest;
        let previous = plugin("dev.a.big", Trust::Reviewed, false).record;
        let mut before = plugin("dev.a.big", Trust::Reviewed, false);
        before.manifest =
            Some(parse(&with_resources("dev.a.big", serde_json::json!([resource("model.onnx", HF, 300)]))));
        let both = serde_json::json!([resource("model.onnx", HF, 300), resource("vocab.bin", CDN, 12)]);
        let mut next = with_resources("dev.a.big", both);
        next["version"] = serde_json::json!("1.1.0");
        let m = parse(&next);
        // The installed version's files come from huggingface.co: the other host is new.
        let u = update(&previous, Some(&before), &m);
        assert_eq!(u.added_resource_hosts, ["cdn.example.com"]);
        let sent = serde_json::to_value(&u).unwrap();
        assert_eq!(sent["addedResourceHosts"], serde_json::json!(["cdn.example.com"]));
        // The same hosts, or fewer, are nothing new.
        assert!(update(&previous, Some(&before), before.manifest.as_ref().unwrap()).added_resource_hosts.is_empty());
        // One whose plugin.json names none: both are new. So are they when
        // its plugin.json can't be read, or its row is gone.
        before.manifest = Some(parse(&manifest("dev.a.big", false)));
        assert_eq!(update(&previous, Some(&before), &m).added_resource_hosts, ["huggingface.co", "cdn.example.com"]);
        before.manifest = None;
        assert_eq!(update(&previous, Some(&before), &m).added_resource_hosts.len(), 2);
        assert_eq!(update(&previous, None, &m).added_resource_hosts.len(), 2);
    }

    #[test]
    fn uninstall_removes_its_downloaded_files() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-files-uninstall-{}", std::process::id()));
        let (lib, host) = host_in(&tmp);
        let big = with_resources("dev.a.big", serde_json::json!([resource("model.onnx", HF, 300)]));
        let p = host.store.commit(folder(&host, &tmp, &big), true, true).unwrap();
        // Its files, as a run left them in AutoScene's cache.
        let kept = AppPaths::at(tmp.join("data")).plugin_resources().join("dev.a.big");
        std::fs::create_dir_all(&kept).unwrap();
        std::fs::write(kept.join("model.onnx"), [7u8; 300]).unwrap();
        std::fs::write(kept.join("model.onnx.verified"), package::sha256_hex(b"model.onnx")).unwrap();
        // Plugins says how much they take, by their sizes.
        assert_eq!(info(&p, &host.store, true).resource_bytes, 300.0);
        assert_eq!(info(&plugin("dev.a.none", Trust::Reviewed, false), &host.store, true).resource_bytes, 0.0);
        // Another plugin's files stay.
        let other = AppPaths::at(tmp.join("data")).plugin_resources().join("dev.a.other");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("x.bin"), b"x").unwrap();
        let jobs = JobRunner::new(&[(Class::Plugin, 1)], |_| {});
        host.uninstall(&jobs, &p.record.id, false).unwrap();
        assert!(!kept.exists(), "its files went with it");
        assert!(other.join("x.bin").exists());
        assert!(host.store.list().unwrap().is_empty());
        // The next start removes the files of a plugin that isn't installed.
        host.prune_resources();
        assert!(!other.exists());
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    /// `p` with the export actions `actions` running after a failed export
    /// too.
    fn failing(mut p: Installed, actions: &[&str]) -> Installed {
        for a in &mut p.manifest.as_mut().unwrap().export_actions {
            a.after_failed = actions.contains(&a.id.as_str());
        }
        p
    }

    /// `p` with the events `events` (waiting for an analysis), of which the
    /// person turned on `on`.
    fn waiting(mut p: Installed, events: &[&str], on: &[&str]) -> Installed {
        p.manifest.as_mut().unwrap().events = events
            .iter()
            .map(|e| as_plugin::EventSpec {
                id: (*e).into(),
                title: format!("Tell {e}"),
                description: None,
                on: EventKind::AnalysisFinished,
            })
            .collect();
        p.extra.events = on.iter().map(|e| (*e).to_owned()).collect();
        p
    }

    fn named(runs: Vec<(&PluginId, &str)>) -> Vec<String> {
        runs.into_iter().map(|(id, a)| format!("{id} {a}")).collect()
    }

    /// A job of `kind` of `project` (on `export`) as the runner says it ended.
    fn job(kind: JobKind, state: JobState, project: Option<ProjectId>, export: Option<ExportId>) -> JobInfo {
        JobInfo {
            id: JobId::new(),
            kind,
            title: "A job".into(),
            subtitle: None,
            project_id: project,
            media_id: None,
            export_id: export,
            state,
            progress: None,
            stage: None,
            eta_ms: None,
            frames: None,
            error: None,
            message: None,
            created_at: 0,
            finished_at: Some(1),
            rev: 1,
        }
    }

    /// An export of `items` (each its status, and whether its file was
    /// made), joined into one file when `merged` says whether that was made.
    fn exported(status: ExportStatus, items: &[(ExportStatus, bool)], merged: Option<bool>) -> ExportSummary {
        let items = items
            .iter()
            .enumerate()
            .map(|(i, (status, made))| ExportItem {
                id: i as i64,
                media_id: as_core::MediaId::new(),
                start: 0,
                end: 24,
                name: format!("Clip {i}"),
                status: *status,
                path: made.then(|| format!("/out/Clip {i}.mp4")),
                verified: *made,
                encoder: None,
                bytes: None,
                error: (!made).then(|| "It failed.".to_owned()),
            })
            .collect();
        ExportSummary {
            id: ExportId::new(),
            project_id: ProjectId::new(),
            created_at: 7,
            preset: as_export::builtin_presets()[0].clone(),
            out_dir: "/out".into(),
            status,
            items,
            label: None,
            merged: merged.map(|made| as_api::MergedFile {
                name: "All".into(),
                path: made.then(|| "/out/All.mp4".to_owned()),
                verified: made,
                bytes: None,
                error: (!made).then(|| "Joining failed.".to_owned()),
            }),
        }
    }

    #[test]
    fn after_failed_runs_for_partial_and_failed_never_cancelled() {
        let post = failing(after(plugin("dev.a.post", Trust::Reviewed, false), &["post", "tag"]), &["post"]);
        // It may run after a failed export, but the person never turned it on.
        let unasked = failing(after(plugin("dev.a.unasked", Trust::Reviewed, false), &[]), &["post"]);
        let mut off = failing(after(plugin("dev.a.off", Trust::Reviewed, false), &["post"]), &["post"]);
        off.record.enabled = false;
        let unreviewed = failing(after(plugin("dev.a.unreviewed", Trust::Unreviewed, false), &["tag"]), &["tag"]);
        let plugins = [post, unasked, off, unreviewed];
        assert_eq!(named(after_failed_runs(&plugins, false)), ["dev.a.post post"]);
        assert_eq!(named(after_failed_runs(&plugins, true)), ["dev.a.post post", "dev.a.unreviewed tag"]);

        // An export job that failed sets them off; one cancelled or done doesn't.
        let (project, export) = (Some(ProjectId::new()), Some(ExportId::new()));
        let failed = job(JobKind::Export, JobState::Failed, project, export);
        assert_eq!(moment(&failed), Some(Moment::ExportFailed { project: project.unwrap(), export: export.unwrap() }));
        for state in [JobState::Cancelled, JobState::Done, JobState::Running] {
            assert_eq!(moment(&job(JobKind::Export, state, project, export)), None, "{state:?}");
        }
        assert_eq!(moment(&job(JobKind::Export, JobState::Failed, project, None)), None);
        assert_eq!(moment(&job(JobKind::Plugin, JobState::Failed, project, export)), None);

        // The run is told how it went: some clips made, or none.
        use ExportStatus::{Cancelled, Queued, Running};
        use ExportStatus::{Done, Failed};
        let outcome = |status, items: &[(ExportStatus, bool)], merged| export_outcome(&exported(status, items, merged));
        assert_eq!(outcome(Failed, &[(Done, true), (Failed, false)], None), Some("partial"));
        assert_eq!(outcome(Failed, &[(Failed, false), (Failed, false)], None), Some("failed"));
        assert_eq!(outcome(Done, &[(Done, true), (Done, true)], None), Some("done"));
        // A joined export is its one file.
        assert_eq!(outcome(Failed, &[(Done, true), (Done, true)], Some(false)), Some("failed"));
        assert_eq!(outcome(Failed, &[(Done, true), (Failed, false)], Some(true)), Some("partial"));
        assert_eq!(outcome(Done, &[(Done, true)], Some(true)), Some("done"));
        // One that waits, runs or was cancelled went no way yet (exports.get
        // shows each).
        assert_eq!(outcome(Queued, &[(Queued, false); 2], None), None);
        assert_eq!(outcome(Running, &[(Done, true), (Running, false)], None), None);
        assert_eq!(outcome(Running, &[(Running, false), (Queued, false)], None), None);
        assert_eq!(outcome(Cancelled, &[(Done, true), (Cancelled, false)], None), None);
        assert_eq!(outcome(Cancelled, &[(Cancelled, false)], Some(false)), None);
    }

    #[test]
    fn after_failed_runs_when_ffmpeg_is_missing() {
        // The export's job fails before it renders anything, as it does when
        // FFmpeg isn't found: the job runner's word is all there is.
        let (tx, ended) = mpsc::channel();
        let tx = Mutex::new(tx);
        let jobs = JobRunner::new(&[(Class::Export, 1)], move |e| {
            if let as_jobs::JobEvent::Changed(info) = e
                && info.state.is_finished()
            {
                drop(tx.lock().send(info));
            }
        });
        let (project, export) = (ProjectId::new(), ExportId::new());
        let spec = JobSpec {
            kind: JobKind::Export,
            class: Class::Export,
            title: "Exporting 2 clips".into(),
            subtitle: None,
            project_id: Some(project),
            media_id: None,
            export_id: Some(export),
        };
        jobs.submit(spec, |_| Err(JobError::Failed("FFmpeg wasn't found.".into())));
        let info = ended.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(moment(&info), Some(Moment::ExportFailed { project, export }));
        // The export it marked failed made nothing.
        let unmade = exported(ExportStatus::Failed, &[(ExportStatus::Queued, false); 2], None);
        assert_eq!(export_outcome(&unmade), Some("failed"));
    }

    /// What a job's end set off, as [`hand_on`] handed it on.
    #[derive(Default)]
    struct SetOffs(Mutex<Vec<String>>);

    impl SetOff for SetOffs {
        fn export_failed(&self, _: ProjectId, export: ExportId) {
            self.0.lock().push(format!("export {export} failed"));
        }

        fn batch_job_ended(&self, info: &JobInfo) {
            self.0.lock().push(format!("{:?} {:?}", info.kind, info.state));
        }
    }

    #[test]
    fn a_job_s_end_is_handed_on_to_what_it_sets_off() {
        let (project, export) = (Some(ProjectId::new()), Some(ExportId::new()));
        let set_off = |kind, state| {
            let to = SetOffs::default();
            hand_on(&job(kind, state, project, export), &to);
            to.0.into_inner()
        };
        // A failed export queues the actions that run then too; one done
        // queued its own, and one cancelled tells no one.
        assert_eq!(set_off(JobKind::Export, JobState::Failed), [format!("export {} failed", export.unwrap())]);
        for state in [JobState::Done, JobState::Cancelled, JobState::Running, JobState::Queued] {
            assert!(set_off(JobKind::Export, state).is_empty(), "{state:?}");
        }
        // Each batch job that ended is noted, however it ended; one that
        // waits or runs isn't.
        for kind in [JobKind::Ingest, JobKind::Analyze, JobKind::Regroup] {
            for state in [JobState::Done, JobState::Failed, JobState::Cancelled] {
                assert_eq!(set_off(kind, state), [format!("{kind:?} {state:?}")]);
            }
            for state in [JobState::Queued, JobState::Running] {
                assert!(set_off(kind, state).is_empty(), "{kind:?} {state:?}");
            }
        }
        assert!(set_off(JobKind::Portraits, JobState::Done).is_empty());
        assert!(set_off(JobKind::Plugin, JobState::Failed).is_empty());
    }

    #[test]
    fn a_whole_export_queues_each_action_once() {
        let p = failing(after(plugin("dev.a.post", Trust::Reviewed, false), &["post", "tag"]), &["post"]);
        let plugins = [p];
        // The export queues each action it turned on itself; its job's end adds none.
        let (project, export) = (Some(ProjectId::new()), Some(ExportId::new()));
        assert_eq!(moment(&job(JobKind::Export, JobState::Done, project, export)), None);
        assert_eq!(named(after_export_runs(&plugins, true)), ["dev.a.post post", "dev.a.post tag"]);
        // A failed one queues only those that run then too, once.
        assert_eq!(named(after_failed_runs(&plugins, true)), ["dev.a.post post"]);
    }

    #[test]
    fn events_of_plugins_that_may_not_run_are_not_queued() {
        let on = waiting(plugin("dev.a.on", Trust::Reviewed, false), &["done", "tally"], &["tally", "done", "gone"]);
        let unasked = waiting(plugin("dev.a.unasked", Trust::Reviewed, false), &["done"], &[]);
        let mut off = waiting(plugin("dev.a.off", Trust::Reviewed, false), &["done"], &["done"]);
        off.record.enabled = false;
        let mut broken = waiting(plugin("dev.a.broken", Trust::Reviewed, false), &["done"], &["done"]);
        broken.broken = Some(as_plugin::store::MISSING.into());
        let mut withdrawn = waiting(plugin("dev.a.withdrawn", Trust::Reviewed, false), &["done"], &["done"]);
        withdrawn.withdrawn = Some("It sent export paths to a server.".into());
        let unreviewed = waiting(plugin("dev.a.unreviewed", Trust::Unreviewed, false), &["done"], &["done"]);
        let plugins = [on, unasked, off, broken, withdrawn, unreviewed];
        // In the order its plugin.json lists them; one it no longer has is left out.
        let runs = |allow| named(event_runs(&plugins, allow, EventKind::AnalysisFinished));
        assert_eq!(runs(false), ["dev.a.on done", "dev.a.on tally"]);
        assert_eq!(runs(true), ["dev.a.on done", "dev.a.on tally", "dev.a.unreviewed done"]);

        // Settings shows each, and whether it is on.
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-events-{}", std::process::id()));
        let (lib, host) = host_in(&tmp);
        let shown = info(&plugins[0], &host.store, false);
        let events: Vec<_> = shown.events.iter().map(|e| (e.id.as_str(), e.on, e.turned_on)).collect();
        assert_eq!(events, [("done", EventKind::AnalysisFinished, true), ("tally", EventKind::AnalysisFinished, true)]);
        assert!(!info(&plugins[1], &host.store, false).events[0].turned_on);
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn the_same_event_waits_once() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-event-once-{}", std::process::id()));
        let (lib, host) = host_in(&tmp);
        lib.set_allows_unreviewed_plugins(true).unwrap();
        let mut m = manifest("dev.a.tell", false);
        m["manifestVersion"] = serde_json::json!(2);
        m["contributes"]["events"] =
            serde_json::json!([{"id": "done", "title": "Tell the channel", "on": "analysisFinished"}]);
        let id = host.store.commit(folder(&host, &tmp, &m), true, true).unwrap().record.id;
        host.store.set_event(&id, "done", true).unwrap();
        let jobs = JobRunner::new(&[(Class::Plugin, 1)], |_| {});
        let (project, elsewhere) = (ProjectId::new(), ProjectId::new());
        // A run holds the one Plugin slot until the test lets it go; the
        // event it was told, once it starts, goes to `started`.
        let (started, told) = mpsc::channel::<Option<EventContext>>();
        let (go, gate) = (Arc::new(AtomicBool::new(false)), host.clone());
        let queue = |key: RunKey| {
            host.queue(&jobs, true, key, Trigger::Event, |_| {
                let (started, go, host) = (started.clone(), go.clone(), gate.clone());
                move |ctx: &JobCtx| {
                    drop(started.send(host.take_event(ctx.id)));
                    while !go.load(Ordering::SeqCst) && !ctx.cancel.is_cancelled() {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Ok(())
                }
            })
        };
        let event = |done: u32, at: i64| EventContext {
            kind: EventKind::AnalysisFinished,
            at,
            episodes: EpisodeCounts { done, failed: 0 },
        };
        let done = |project| key(&id, ActionKind::Event, "done", project, None);
        let tell = |project, e: &EventContext| host.tell(&jobs, done(Some(project)), e, queue);
        // The first runs; the one after it waits.
        let first = tell(project, &event(1, 10)).unwrap().unwrap();
        assert_eq!(told.recv_timeout(Duration::from_secs(10)).unwrap(), Some(event(1, 10)));
        let second = tell(project, &event(2, 20)).unwrap().unwrap();
        // Told again while it waits: it isn't queued twice, and tells of both.
        assert_eq!(tell(project, &event(3, 30)).unwrap(), None);
        let other = tell(elsewhere, &event(4, 40)).unwrap().unwrap();
        let title = jobs.jobs().into_iter().find(|j| j.id == first).unwrap().title;
        assert_eq!(title, "Tell the channel");
        go.store(true, Ordering::SeqCst);
        let mut later: Vec<_> = (0..2).map(|_| told.recv_timeout(Duration::from_secs(10)).unwrap()).collect();
        later.sort_by_key(|e| e.as_ref().map(|e| e.at));
        assert_eq!(later, [Some(event(5, 30)), Some(event(4, 40))]);
        for job in [first, second, other] {
            let ended = || jobs.jobs().into_iter().find(|j| j.id == job).is_none_or(|j| j.state.is_finished());
            let start = Instant::now();
            while !ended() && start.elapsed() < Duration::from_secs(10) {
                thread::sleep(Duration::from_millis(5));
            }
            assert!(ended(), "{job}");
        }
        // It runs in the project where it happened, on no export; and only one it has.
        let said = |key: RunKey| queue(key).unwrap_err().to_string();
        assert_eq!(said(done(None)), "An event runs in the project where it happened.");
        let on_export = key(&id, ActionKind::Event, "done", Some(project), Some(ExportId::new()));
        assert_eq!(said(on_export), "An event runs in the project where it happened.");
        assert_eq!(
            said(key(&id, ActionKind::Event, "later", Some(project), None)),
            "Plugin dev.a.tell has no such action."
        );
        // It starts as it was queued.
        assert!(startable(&host.store, &lib, &done(Some(project)), Trigger::Event, false).is_ok());
        drop((host, gate, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    /// A job runner whose batch jobs end when a test says, with the
    /// [`Batches`] its ends are noted in.
    struct Batching {
        jobs: JobRunner,
        ended: mpsc::Receiver<JobInfo>,
        /// Ends the runner told of before the test waited for them.
        early: Mutex<Vec<JobInfo>>,
        gates: Mutex<HashMap<JobId, mpsc::Sender<JobResult>>>,
        batches: Arc<Batches>,
    }

    impl Batching {
        fn new() -> Self {
            let (tx, ended) = mpsc::channel();
            let tx = Mutex::new(tx);
            let jobs = JobRunner::new(&[(Class::Analysis, 16)], move |e| {
                if let as_jobs::JobEvent::Changed(info) = e
                    && info.state.is_finished()
                {
                    drop(tx.lock().send(info));
                }
            });
            Self { jobs, ended, early: Mutex::default(), gates: Mutex::default(), batches: Arc::default() }
        }

        /// Queues a job of `kind` of `project`, which runs until it is
        /// ended or cancelled; an analysis analyses something.
        fn queue(&self, kind: JobKind, project: ProjectId) -> JobId {
            self.queue_with(kind, project, true)
        }

        /// [`Batching::queue`]; an analysis says it analysed something as
        /// it ends when `works`, as work.rs's does (after ending as it
        /// was asked, whatever that is).
        fn queue_with(&self, kind: JobKind, project: ProjectId, works: bool) -> JobId {
            let (tx, rx) = mpsc::channel::<JobResult>();
            let spec = JobSpec {
                kind,
                class: Class::Analysis,
                title: format!("{kind:?}"),
                subtitle: None,
                project_id: Some(project),
                media_id: None,
                export_id: None,
            };
            let batches = self.batches.clone();
            let job = self.jobs.submit(spec, move |ctx| {
                let result = loop {
                    match rx.recv_timeout(Duration::from_millis(5)) {
                        Ok(result) => break result,
                        Err(mpsc::RecvTimeoutError::Timeout) if !ctx.cancel.is_cancelled() => {}
                        Err(_) => break Err(JobError::Cancelled),
                    }
                };
                if works && kind == JobKind::Analyze {
                    batches.worked(project, ctx.id);
                }
                result
            });
            self.gates.lock().insert(job, tx);
            job
        }

        /// Ends `job` with `result`; once the runner says it ended, its end
        /// as the runner gave it, not yet noted.
        fn end(&self, job: JobId, result: JobResult) -> JobInfo {
            self.gates.lock().remove(&job).unwrap().send(result).unwrap();
            self.wait(job)
        }

        /// Cancels `job`, as its end is given.
        fn cancel(&self, job: JobId) -> JobInfo {
            self.jobs.cancel(job);
            self.wait(job)
        }

        fn wait(&self, job: JobId) -> JobInfo {
            let early = self.early.lock().iter().position(|i| i.id == job);
            if let Some(at) = early {
                return self.early.lock().remove(at);
            }
            loop {
                let info = self.ended.recv_timeout(Duration::from_secs(10)).unwrap();
                if info.id == job {
                    return info;
                }
                self.early.lock().push(info);
            }
        }

        /// Notes the end `info`: the episodes it tells of, done and failed.
        fn note(&self, info: &JobInfo) -> Option<(u32, u32)> {
            self.batches.ended(info, || self.jobs.jobs()).map(|e| (e.done, e.failed))
        }
    }

    fn failed() -> JobResult {
        Err(JobError::Failed("The engine stopped.".into()))
    }

    #[test]
    fn an_analysis_event_waits_for_the_project_s_last_analysis() {
        let b = Batching::new();
        let (show, other) = (ProjectId::new(), ProjectId::new());
        let [one, two] = [(); 2].map(|_| b.queue(JobKind::Analyze, show));
        let elsewhere = b.queue(JobKind::Analyze, other);
        assert_eq!(b.note(&b.end(one, Ok(()))), None);
        // Another project's batch is its own.
        assert_eq!(b.note(&b.end(elsewhere, Ok(()))), Some((1, 0)));
        // A failed analysis counts too, and the last one tells of all.
        assert_eq!(b.note(&b.end(two, failed())), Some((1, 1)));

        // Two that end at once tell once, whichever end is noted first.
        let [three, four] = [(); 2].map(|_| b.queue(JobKind::Analyze, show));
        let (three, four) = (b.end(three, Ok(())), b.end(four, Ok(())));
        assert_eq!(b.note(&four), None);
        assert_eq!(b.note(&three), Some((2, 0)));
        assert_eq!(b.note(&four), None);
        // Portraits and posters change pictures, not who is who: they aren't waited for.
        let portraits = b.queue(JobKind::Portraits, show);
        let five = b.queue(JobKind::Analyze, show);
        assert_eq!(b.note(&b.end(five, Ok(()))), Some((1, 0)));
        b.end(portraits, Ok(()));
    }

    #[test]
    fn an_analysis_that_ends_while_an_ingest_waits_tells_no_one_yet() {
        let b = Batching::new();
        let show = ProjectId::new();
        let (indexing, first) = (b.queue(JobKind::Ingest, show), b.queue(JobKind::Analyze, show));
        assert_eq!(b.note(&b.end(first, Ok(()))), None);
        // An ingest queues its episode's analysis as it ends.
        let second = b.queue(JobKind::Analyze, show);
        assert_eq!(b.note(&b.end(indexing, Ok(()))), None);
        assert_eq!(b.note(&b.end(second, Ok(()))), Some((2, 0)));
        // One that failed queues none: its end is the batch's.
        let (indexing, third) = (b.queue(JobKind::Ingest, show), b.queue(JobKind::Analyze, show));
        assert_eq!(b.note(&b.end(third, Ok(()))), None);
        assert_eq!(b.note(&b.end(indexing, failed())), Some((1, 0)));
    }

    #[test]
    fn a_regroup_after_the_analysis_is_waited_for() {
        let b = Batching::new();
        let show = ProjectId::new();
        let analysis = b.queue(JobKind::Analyze, show);
        // The cast is grouped again before the analysis says it ended.
        let regroup = b.queue(JobKind::Regroup, show);
        assert_eq!(b.note(&b.end(analysis, Ok(()))), None);
        assert_eq!(b.note(&b.end(regroup, Ok(()))), Some((1, 0)));
        // A regroup alone has no analysis to tell of.
        let regroup = b.queue(JobKind::Regroup, show);
        assert_eq!(b.note(&b.end(regroup, Ok(()))), None);
    }

    #[test]
    fn a_restarted_analysis_never_fires_early_and_counts_once() {
        let b = Batching::new();
        let show = ProjectId::new();
        // The old analysis queues the new one, then ends done without
        // saying it analysed anything: as work.rs ends it.
        let analysis = b.queue_with(JobKind::Analyze, show, false);
        let again = b.queue(JobKind::Analyze, show);
        assert_eq!(b.note(&b.end(analysis, Ok(()))), None);
        assert_eq!(b.note(&b.end(again, Ok(()))), Some((1, 0)));
        // Stopped by a cancel first, it is the same.
        let analysis = b.queue_with(JobKind::Analyze, show, false);
        let again = b.queue(JobKind::Analyze, show);
        assert_eq!(b.note(&b.cancel(analysis)), None);
        assert_eq!(b.note(&b.end(again, Ok(()))), Some((1, 0)));
    }

    #[test]
    fn an_analysis_that_had_nothing_to_do_tells_no_one() {
        // A folder found again queues an analysis of each of its episodes,
        // which finds it analysed already.
        let b = Batching::new();
        let show = ProjectId::new();
        let [one, two] = [(); 2].map(|_| b.queue_with(JobKind::Analyze, show, false));
        assert_eq!(b.note(&b.end(one, Ok(()))), None);
        assert_eq!(b.note(&b.end(two, Ok(()))), None);
        // Beside one that analysed an episode, that one is told of.
        let idle = b.queue_with(JobKind::Analyze, show, false);
        let new = b.queue(JobKind::Analyze, show);
        assert_eq!(b.note(&b.end(new, failed())), None);
        assert_eq!(b.note(&b.end(idle, Ok(()))), Some((0, 1)));
    }

    #[test]
    fn a_note_reads_the_jobs_once_it_holds_the_lock() {
        // Two analyses end close together, and the second's note comes
        // first: the last note to come must see the other's end noted.
        let b = Batching::new();
        let show = ProjectId::new();
        let [one, two] = [(); 2].map(|_| b.queue(JobKind::Analyze, show));
        let (one, two) = (b.end(one, Ok(())), b.end(two, Ok(())));
        let read = || {
            assert!(b.batches.0.try_lock().is_none(), "the jobs were read before the note held the lock");
            b.jobs.jobs()
        };
        assert_eq!(b.batches.ended(&two, read), None);
        assert_eq!(b.batches.ended(&one, read).map(|e| (e.done, e.failed)), Some((2, 0)));
    }

    #[test]
    fn a_batch_cancelled_whole_tells_no_one() {
        let b = Batching::new();
        let show = ProjectId::new();
        let [one, two] = [(); 2].map(|_| b.queue(JobKind::Analyze, show));
        let indexing = b.queue(JobKind::Ingest, show);
        for job in [indexing, one, two] {
            assert_eq!(b.note(&b.cancel(job)), None);
        }
        // The next batch tells of its own episodes only.
        let three = b.queue(JobKind::Analyze, show);
        assert_eq!(b.note(&b.end(three, Ok(()))), Some((1, 0)));
    }
}

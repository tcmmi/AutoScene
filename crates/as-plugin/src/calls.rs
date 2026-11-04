//! Host methods (protocol 2): what a plugin may ask AutoScene for, from its
//! pages, its runs and its service, and what each needs. A call is checked
//! here before the host answers it ([`HostCalls`]): params past the method's
//! cap are refused first ([`check_size`]), then the method must be one its
//! caller may make, and the plugin must hold what the method needs
//! ([`check`], [`Grants`]): a permission the person agreed to and its
//! `plugin.json` still asks for, a secret setting, a service, or page runs.
//!
//! A page and a run read only the project they are in, and a run on an
//! export reads only that export ([`Caller::project`], [`Caller::export`]).
//! A service names the project it reads: its program runs with the person's
//! rights anyway.
//!
//! What a program does itself (its own files, its own sockets) is not
//! checked: a process plugin runs with the person's user rights. Only what
//! it asks AutoScene for is.
//!
//! The params and results are camelCase, and none holds a 64-bit integer,
//! since they reach pages as JavaScript numbers: times are Unix ms and
//! sizes bytes, both as `f64`, and frames `u32`.

use std::path::Path;

use as_core::CancelToken;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_json::value::RawValue;
use specta::Type;

use crate::error::{PluginError, PluginResult};
use crate::manifest::{Permission, RuntimeKind, shown};
use crate::protocol::ActionKind;
use crate::store::{Installed, manifest_of};

const KIB: usize = 1 << 10;
const MIB: usize = 1 << 20;

/// A method a plugin may call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub enum HostMethod {
    #[serde(rename = "settings.get")]
    SettingsGet,
    #[serde(rename = "secrets.get")]
    SecretsGet,
    #[serde(rename = "project.get")]
    ProjectGet,
    #[serde(rename = "exports.list")]
    ExportsList,
    #[serde(rename = "exports.get")]
    ExportsGet,
    #[serde(rename = "files.resolve")]
    FilesResolve,
    #[serde(rename = "net.fetch")]
    NetFetch,
    #[serde(rename = "state.get")]
    StateGet,
    #[serde(rename = "state.set")]
    StateSet,
    #[serde(rename = "state.delete")]
    StateDelete,
    #[serde(rename = "state.keys")]
    StateKeys,
    #[serde(rename = "service.request")]
    ServiceRequest,
    #[serde(rename = "actions.run")]
    ActionsRun,
    #[serde(rename = "ui.toast")]
    UiToast,
    #[serde(rename = "ui.navigate")]
    UiNavigate,
    #[serde(rename = "ui.copyText")]
    UiCopyText,
    #[serde(rename = "ui.openExternal")]
    UiOpenExternal,
    #[serde(rename = "files.save")]
    FilesSave,
    #[serde(rename = "files.open")]
    FilesOpen,
}

impl HostMethod {
    pub const ALL: [Self; 19] = [
        Self::SettingsGet,
        Self::SecretsGet,
        Self::ProjectGet,
        Self::ExportsList,
        Self::ExportsGet,
        Self::FilesResolve,
        Self::NetFetch,
        Self::StateGet,
        Self::StateSet,
        Self::StateDelete,
        Self::StateKeys,
        Self::ServiceRequest,
        Self::ActionsRun,
        Self::UiToast,
        Self::UiNavigate,
        Self::UiCopyText,
        Self::UiOpenExternal,
        Self::FilesSave,
        Self::FilesOpen,
    ];

    /// Its name in a call.
    pub fn name(self) -> &'static str {
        match self {
            Self::SettingsGet => "settings.get",
            Self::SecretsGet => "secrets.get",
            Self::ProjectGet => "project.get",
            Self::ExportsList => "exports.list",
            Self::ExportsGet => "exports.get",
            Self::FilesResolve => "files.resolve",
            Self::NetFetch => "net.fetch",
            Self::StateGet => "state.get",
            Self::StateSet => "state.set",
            Self::StateDelete => "state.delete",
            Self::StateKeys => "state.keys",
            Self::ServiceRequest => "service.request",
            Self::ActionsRun => "actions.run",
            Self::UiToast => "ui.toast",
            Self::UiNavigate => "ui.navigate",
            Self::UiCopyText => "ui.copyText",
            Self::UiOpenExternal => "ui.openExternal",
            Self::FilesSave => "files.save",
            Self::FilesOpen => "files.open",
        }
    }

    /// The method named `s`, if this AutoScene knows it.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.name() == s)
    }

    /// Who may call it: a page, a run, a service.
    pub fn callers(self) -> Callers {
        const ANY: Callers = Callers { page: true, run: true, service: true };
        const PAGE: Callers = Callers { page: true, run: false, service: false };
        match self {
            Self::SettingsGet
            | Self::SecretsGet
            | Self::ProjectGet
            | Self::ExportsList
            | Self::ExportsGet
            | Self::FilesResolve
            | Self::NetFetch
            | Self::StateGet
            | Self::StateSet
            | Self::StateDelete
            | Self::StateKeys => ANY,
            Self::ServiceRequest
            | Self::ActionsRun
            | Self::UiToast
            | Self::UiNavigate
            | Self::UiCopyText
            | Self::UiOpenExternal
            | Self::FilesSave
            | Self::FilesOpen => PAGE,
        }
    }

    /// What the plugin must hold to call it.
    pub fn needs(self) -> Need {
        match self {
            Self::SecretsGet => Need::SecretKey,
            Self::ProjectGet => Need::Permission(Permission::ProjectRead),
            Self::ExportsList | Self::ExportsGet => Need::Permission(Permission::ExportsRead),
            Self::FilesResolve => Need::Files,
            Self::NetFetch => Need::Network,
            Self::ServiceRequest => Need::Service,
            Self::ActionsRun => Need::PageRuns,
            Self::SettingsGet
            | Self::StateGet
            | Self::StateSet
            | Self::StateDelete
            | Self::StateKeys
            | Self::UiToast
            | Self::UiNavigate
            | Self::UiCopyText
            | Self::UiOpenExternal
            | Self::FilesSave
            | Self::FilesOpen => Need::Nothing,
        }
    }

    /// The most bytes its params may take as JSON.
    pub fn max_params(self) -> usize {
        match self {
            Self::NetFetch => 5 * MIB,
            Self::StateSet => 300 * KIB,
            Self::ServiceRequest => MIB,
            Self::ActionsRun | Self::UiCopyText => 64 * KIB,
            Self::UiToast => 2 * KIB,
            Self::UiOpenExternal => 4 * KIB,
            // The base64 of 32 MB.
            Self::FilesSave => 48 * MIB,
            Self::SettingsGet
            | Self::SecretsGet
            | Self::ProjectGet
            | Self::ExportsList
            | Self::ExportsGet
            | Self::FilesResolve
            | Self::StateGet
            | Self::StateDelete
            | Self::StateKeys
            | Self::UiNavigate
            | Self::FilesOpen => KIB,
        }
    }
}

/// What a method needs of the plugin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Need {
    Nothing,
    Permission(Permission),
    /// The network permission; `net.fetch` checks the host itself.
    Network,
    /// A secret setting; `secrets.get` checks the key itself.
    SecretKey,
    /// `exports:read` or `project:read`; [`check_file`] checks the one the
    /// file needs once its kind is known.
    Files,
    /// A plugin that runs a service ([`Grants::service`]).
    Service,
    /// A plugin whose pages may start its runs ([`Grants::takes_page_runs`]).
    PageRuns,
}

/// Who may call a method.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Callers {
    pub page: bool,
    pub run: bool,
    pub service: bool,
}

/// What a plugin may use: what the person agreed to (its record) and its
/// manifest still asks for. Made once per run, page session and service
/// start; every change of the plugin ends those, so it never goes stale.
#[derive(Clone, Debug, PartialEq)]
pub struct Grants {
    /// The runtime the person agreed to.
    pub runtime: RuntimeKind,
    pub permissions: Vec<Permission>,
    /// Where `net.fetch` may connect; empty without the network permission.
    pub hosts: Vec<String>,
    /// The keys of its secret settings, which `secrets.get` reads; a
    /// manifest of version 1 has none.
    pub secrets: Vec<String>,
    /// The protocol it declared (`engines.protocol`).
    pub protocol: u32,
}

impl Grants {
    /// The grants of `p`: its record's permissions and hosts that its
    /// manifest still asks for, so a loaded folder whose `plugin.json` asks
    /// for more later gets nothing more, the runtime the person agreed to
    /// (a loaded folder that changed it is broken until loaded again), and
    /// its secret settings. A broken plugin gets none.
    pub fn of(p: &Installed) -> PluginResult<Self> {
        let m = manifest_of(p)?;
        let r = &p.record;
        let permissions: Vec<Permission> =
            r.permissions.iter().filter(|g| m.permissions.contains(g)).copied().collect();
        let hosts = match permissions.contains(&Permission::Network) {
            true => r.network_hosts.iter().filter(|h| m.network_hosts.contains(h)).cloned().collect(),
            false => Vec::new(),
        };
        Ok(Self {
            runtime: p.extra.agreed_runtime(),
            permissions,
            hosts,
            secrets: m.secret_keys().map(str::to_owned).collect(),
            protocol: m.engines.protocol,
        })
    }

    /// Whether it runs a service.
    pub fn service(&self) -> bool {
        self.runtime == RuntimeKind::Service
    }

    /// Whether its pages may start its runs: its program speaks protocol 2,
    /// or it is a WebAssembly plugin, whose world has a page's trigger and
    /// input from its first version.
    pub fn takes_page_runs(&self) -> bool {
        match self.runtime {
            RuntimeKind::Process | RuntimeKind::Service => self.protocol >= 2,
            RuntimeKind::Wasm => true,
            RuntimeKind::None => false,
        }
    }

    pub fn has(&self, p: Permission) -> bool {
        self.permissions.contains(&p)
    }
}

/// Who calls, and where: the project a page is open in, the project and
/// export a run is in and on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Caller<'a> {
    Page { project: Option<&'a str> },
    Run { project: Option<&'a str>, export: Option<&'a str> },
    Service,
}

impl<'a> Caller<'a> {
    fn who(&self) -> &'static str {
        match self {
            Self::Page { .. } => "A page",
            Self::Run { .. } => "A run",
            Self::Service => "A service",
        }
    }

    /// The project a call that names `asked` (or none) reads. A page or a
    /// run reads its own, and names it or none; a service names the one it
    /// reads.
    pub fn project(&self, asked: Option<&'a str>) -> PluginResult<&'a str> {
        let (own, outside, other) = match *self {
            Self::Page { project } => {
                (project, "This page isn't open in a project.", "A page reads only the project it is open in.")
            }
            Self::Run { project, .. } => {
                (project, "This run isn't in a project.", "A run reads only the project it runs in.")
            }
            Self::Service => return asked.ok_or_else(|| invalid("A service names the project it reads: projectId.")),
        };
        match (own, asked) {
            (Some(own), None) => Ok(own),
            (Some(own), Some(asked)) if asked == own => Ok(own),
            (None, None) => Err(invalid(outside)),
            _ => Err(PluginError::Denied(other.into())),
        }
    }

    /// Whether a call may read the export `asked` of its project: a run on
    /// an export reads only that one.
    pub fn export(&self, asked: &str) -> PluginResult<()> {
        match *self {
            Self::Run { export: Some(own), .. } if own != asked => {
                Err(PluginError::Denied("A run reads only the export it runs on.".into()))
            }
            _ => Ok(()),
        }
    }
}

/// Whether `caller` may call `method` with `grants`: Ok, or
/// [`PluginError::Denied`] with the sentence the plugin's log and its page
/// get. The project and export it names are [`Caller::project`]'s and
/// [`Caller::export`]'s to check.
pub fn check(method: HostMethod, caller: &Caller, grants: &Grants) -> PluginResult<()> {
    let name = method.name();
    let callers = method.callers();
    let allowed = match caller {
        Caller::Page { .. } => callers.page,
        Caller::Run { .. } => callers.run,
        Caller::Service => callers.service,
    };
    if !allowed {
        return Err(denied(format!("{} can't call {name}.", caller.who())));
    }
    let granted = match method.needs() {
        Need::Nothing => true,
        Need::Permission(p) => grants.has(p),
        Need::Network => grants.has(Permission::Network),
        Need::SecretKey => !grants.secrets.is_empty(),
        Need::Files => grants.has(Permission::ExportsRead) || grants.has(Permission::ProjectRead),
        Need::Service => grants.service(),
        Need::PageRuns => grants.takes_page_runs(),
    };
    if granted {
        return Ok(());
    }
    Err(denied(match method.needs() {
        Need::Permission(p) => format!("{name} needs the {} permission.", p.name()),
        Need::Network => format!("{name} needs the network permission."),
        Need::SecretKey => format!("{name} needs a secret setting, and this plugin has none."),
        Need::Files => format!("{name} needs the exports:read or the project:read permission."),
        Need::Service => format!("{name} needs a plugin that runs a service."),
        Need::PageRuns | Need::Nothing => format!("{name} needs a plugin whose program speaks protocol 2."),
    }))
}

/// Whether a call of `method` may have params of `len` bytes of JSON.
/// Checked before anything else: the host reads no more of them.
pub fn check_size(method: HostMethod, len: usize) -> PluginResult<()> {
    let max = method.max_params();
    if len <= max {
        return Ok(());
    }
    let max = if max >= MIB { format!("{} MB", max / MIB) } else { format!("{} KB", max / KIB) };
    Err(invalid(format!("The params of {} are larger than {max}.", method.name())))
}

/// Whether `grants` let a plugin resolve `file`: a clip or a merged file
/// needs `exports:read`, an episode `project:read`.
///
/// A run or a service that may read an export also gets, with it, the path
/// of the episode each of its clips comes from (`exports.get` gives it the
/// [`crate::protocol::ExportContext`] an export action's run line always
/// had). That is no way around this check: an episode here may be any of
/// the project's, and a page gets no path from an export, so for a page
/// this is all that stands between `exports:read` and the episodes.
pub fn check_file(file: &FileRef, grants: &Grants) -> PluginResult<()> {
    let need = file.need();
    match grants.has(need) {
        true => Ok(()),
        false => Err(denied(format!("files.resolve needs the {} permission for that file.", need.name()))),
    }
}

/// Whether `grants` let a plugin read the secret setting `key`: one of its
/// own.
pub fn check_secret(key: &str, grants: &Grants) -> PluginResult<()> {
    match grants.secrets.iter().any(|k| k == key) {
        true => Ok(()),
        false => {
            Err(denied(format!("secrets.get reads only the plugin's secret settings, and {} isn't one.", shown(key))))
        }
    }
}

/// The params of a call of `method`, read as `T` from the text the plugin
/// wrote; none read as `{}`. Only `T` is made of them, so a call of
/// [`HostMethod::max_params`] bytes takes about as much as it is long.
pub fn params<T: DeserializeOwned>(method: HostMethod, params: &RawValue) -> PluginResult<T> {
    let text = match params.get() {
        "null" => "{}",
        text => text,
    };
    serde_json::from_str(text).map_err(|e| {
        // Without where it broke: that counts from the params, not the line.
        let e = e.to_string();
        let e = e.rsplit_once(" at line ").map_or(e.as_str(), |(e, _)| e);
        invalid(format!("The params of {} don't fit: {e}.", method.name()))
    })
}

/// The media type of the picture, video or sound at `path`, by its
/// extension; none for any other file. A run or a service gets the path of
/// any episode, an mkv or an mxf too, so this knows more types than a page
/// is served: a page's `files.resolve` keeps to those its scheme serves.
pub fn media_type(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "avi" => "video/x-msvideo",
        "ts" | "m2ts" | "mts" => "video/mp2t",
        "mxf" => "application/mxf",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "opus" => "audio/opus",
        "flac" => "audio/flac",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        _ => return None,
    })
}

fn denied(message: String) -> PluginError {
    PluginError::Denied(message)
}

fn invalid(message: impl Into<String>) -> PluginError {
    PluginError::Invalid(message.into())
}

/// What kind of error a call got.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CallCode {
    /// The plugin has no grant for it.
    Denied,
    /// The call itself is wrong (an unknown method, params that don't fit).
    Invalid,
    NotFound,
    /// Too many calls at once, or the plugin is busy.
    Busy,
    /// AutoScene couldn't answer it.
    Failed,
    /// The page that called was closed.
    Closed,
    Cancelled,
}

impl CallCode {
    /// Its name in a reply.
    pub fn name(self) -> &'static str {
        match self {
            Self::Denied => "denied",
            Self::Invalid => "invalid",
            Self::NotFound => "not_found",
            Self::Busy => "busy",
            Self::Failed => "failed",
            Self::Closed => "closed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// The error a call got: its kind, and a sentence for the plugin's log or
/// its page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct CallError {
    pub code: CallCode,
    pub message: String,
}

impl CallError {
    pub fn new(code: CallCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

impl From<&PluginError> for CallError {
    fn from(e: &PluginError) -> Self {
        let code = match e {
            PluginError::Denied(_) => CallCode::Denied,
            PluginError::Invalid(_) | PluginError::TooNew(_) => CallCode::Invalid,
            PluginError::NotFound(_) => CallCode::NotFound,
            PluginError::Busy(_) => CallCode::Busy,
            PluginError::Cancelled => CallCode::Cancelled,
            PluginError::Closed => CallCode::Closed,
            _ => CallCode::Failed,
        };
        Self::new(code, e.to_string())
    }
}

impl From<PluginError> for CallError {
    fn from(e: PluginError) -> Self {
        Self::from(&e)
    }
}

/// What AutoScene answers a run's or a service's calls with (the app
/// implements it). Called on a thread of the call's own, so it may wait.
/// `params` is the text the plugin wrote: [`check_size`] it, then read it
/// with [`params`]. `ended` fires once its caller is gone (a run that was
/// cancelled, or whose program ended): a call that sees it changes nothing
/// more, and one that waits stops waiting.
pub trait HostCalls: Send + Sync {
    fn call(&self, method: HostMethod, params: &RawValue, ended: &CancelToken) -> Result<Value, CallError>;
}

// The params and results of each method.

/// `secrets.get`; it gives `{ value: string | null }`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SecretsGet {
    pub key: String,
}

/// `project.get` and `exports.list`: the caller's project, or the one a
/// service names.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRef {
    #[serde(default)]
    pub project_id: Option<String>,
}

/// `exports.get`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExportRef {
    #[serde(default)]
    pub project_id: Option<String>,
    pub export_id: String,
}

/// `files.resolve`: a clip of an export (by its place in the export), an
/// export's merged file, or an episode.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum FileRef {
    Clip {
        #[serde(default)]
        project_id: Option<String>,
        export_id: String,
        index: u32,
    },
    Merged {
        #[serde(default)]
        project_id: Option<String>,
        export_id: String,
    },
    Episode {
        #[serde(default)]
        project_id: Option<String>,
        media_id: String,
    },
}

impl FileRef {
    pub fn project_id(&self) -> Option<&str> {
        match self {
            Self::Clip { project_id, .. } | Self::Merged { project_id, .. } | Self::Episode { project_id, .. } => {
                project_id.as_deref()
            }
        }
    }

    /// The permission a plugin needs to resolve it.
    pub fn need(&self) -> Permission {
        match self {
            Self::Clip { .. } | Self::Merged { .. } => Permission::ExportsRead,
            Self::Episode { .. } => Permission::ProjectRead,
        }
    }
}

/// A file `files.resolve` found: a page gets a URL to show it by, a run or
/// a service its path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedFile {
    pub url: Option<String>,
    pub path: Option<String>,
    pub mime: String,
    pub size: f64,
}

/// A project as a page sees it: no paths.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PageProject {
    pub id: String,
    pub title: String,
    pub episodes: Vec<PageEpisode>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PageEpisode {
    pub id: String,
    /// Its number (`S01E03`), else its file's name.
    pub name: String,
    pub frames: Option<u32>,
    /// Its frame rate as a fraction (`24000/1001`), if known.
    pub fps: Option<String>,
    /// Its file isn't where it was.
    pub missing: bool,
}

/// A project as a run or a service gets it: with its folder and its
/// episodes' files.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProgramProject {
    pub id: String,
    pub title: String,
    pub folder: String,
    pub episodes: Vec<ProgramEpisode>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProgramEpisode {
    pub id: String,
    /// Its number (`S01E03`), else its file's name.
    pub name: String,
    pub path: String,
    pub frames: Option<u32>,
    /// Its frame rate as a fraction (`24000/1001`), if known.
    pub fps: Option<String>,
    /// Its file isn't where it was.
    pub missing: bool,
}

/// An export in `exports.list`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PageExportSummary {
    pub id: String,
    pub label: Option<String>,
    pub preset: String,
    /// Unix ms.
    pub created_at: f64,
    /// Its clips are joined into one file.
    pub merged: bool,
    pub clips: u32,
    /// Clips written so far.
    pub done: u32,
    pub status: String,
}

/// An export as a page sees it: no paths.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PageExport {
    pub id: String,
    pub label: Option<String>,
    pub preset: String,
    /// Unix ms.
    pub created_at: f64,
    pub merged: bool,
    pub clips: Vec<PageClip>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PageClip {
    /// Its place in the export, as `files.resolve` takes it.
    pub index: u32,
    pub name: String,
    pub status: String,
    pub media_id: String,
    pub episode: String,
    /// The first frame of the episode it shows.
    pub start: u32,
    /// The frame after its last one.
    pub end: u32,
    pub fps: Option<String>,
}

/// `net.fetch`: an `https://` request to a host the plugin names. A body
/// is text or base64, never both.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NetRequest {
    pub url: String,
    /// `GET` when left out.
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub body_base64: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NetResponse {
    pub status: u16,
    /// Where it was answered from, after redirects.
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// The body as text when it is UTF-8, else as base64.
    pub body: Option<String>,
    pub body_base64: Option<String>,
}

/// `state.get` (it gives `{ value }`, `null` for none) and `state.delete`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StateKey {
    pub key: String,
}

/// `state.set`; it gives `null`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StateSet {
    pub key: String,
    pub value: Value,
}

/// `service.request`: a request to the plugin's own service.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ServiceRequestParams {
    pub method: String,
    #[serde(default)]
    pub params: Value,
    #[serde(default)]
    pub timeout_ms: Option<u32>,
}

/// `actions.run`: runs an export action (with `exportId`) or a command; it
/// gives `{ jobId }`, `null` when the same run waits already.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ActionsRun {
    pub action: String,
    #[serde(default)]
    pub export_id: Option<String>,
    /// Handed to the run as its `input`.
    #[serde(default)]
    pub input: Option<Value>,
}

impl ActionsRun {
    /// What it runs: an export action when it names an export, else a
    /// command.
    pub fn kind(&self) -> ActionKind {
        match self.export_id {
            Some(_) => ActionKind::ExportAction,
            None => ActionKind::Command,
        }
    }
}

/// `ui.toast`; it gives `{ shown }`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Toast {
    pub text: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tone: ToastTone,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ToastTone {
    #[default]
    Neutral,
    Error,
}

/// `ui.navigate`: a path inside the page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Navigate {
    pub path: String,
    #[serde(default)]
    pub replace: bool,
}

/// `ui.copyText`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CopyText {
    pub text: String,
}

/// `ui.openExternal`; it gives `{ opened }`, once the person said yes or no.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OpenExternal {
    pub url: String,
}

/// The kinds of file a save or open dialog offers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FileFilter {
    pub name: String,
    pub extensions: Vec<String>,
}

/// `files.save`: a file the person saves where they choose, as text or as
/// base64, never both; it gives `{ saved }`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SaveFile {
    pub name: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub data_base64: Option<String>,
    #[serde(default)]
    pub filters: Vec<FileFilter>,
}

/// `files.open`: a file the person picks; it gives an [`OpenedFile`], or
/// `null` when they picked none.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OpenFile {
    #[serde(default)]
    pub filters: Vec<FileFilter>,
    #[serde(default)]
    pub max_bytes: Option<u32>,
}

/// A file the person picked: its name and contents, never its path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OpenedFile {
    pub name: String,
    pub size: f64,
    pub data_base64: String,
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::PathBuf;

    use serde_json::json;

    use super::*;
    use crate::manifest::parse_for_any_app;
    use crate::store::{Extra, RECORD_VERSION, Record, Source};
    use crate::trust::Trust;

    fn grants(permissions: &[Permission]) -> Grants {
        Grants {
            runtime: RuntimeKind::Process,
            permissions: permissions.to_vec(),
            hosts: Vec::new(),
            secrets: Vec::new(),
            protocol: 2,
        }
    }

    /// Everything any method may need.
    fn all() -> Grants {
        Grants {
            runtime: RuntimeKind::Service,
            permissions: vec![
                Permission::ExportsRead,
                Permission::ExportsWrite,
                Permission::ProjectRead,
                Permission::Network,
            ],
            hosts: vec!["discord.com".into()],
            secrets: vec!["token".into()],
            protocol: 2,
        }
    }

    const PAGE: Caller = Caller::Page { project: Some("p1") };

    #[test]
    fn every_method_is_refused_without_its_grant() {
        use HostMethod as M;
        let (any, page) = ((true, true, true), (true, false, false));
        let read = |p| Need::Permission(p);
        // The method table: who may call it, what it needs, its params' cap.
        let table = [
            (M::SettingsGet, any, Need::Nothing, KIB),
            (M::SecretsGet, any, Need::SecretKey, KIB),
            (M::ProjectGet, any, read(Permission::ProjectRead), KIB),
            (M::ExportsList, any, read(Permission::ExportsRead), KIB),
            (M::ExportsGet, any, read(Permission::ExportsRead), KIB),
            (M::FilesResolve, any, Need::Files, KIB),
            (M::NetFetch, any, Need::Network, 5 * MIB),
            (M::StateGet, any, Need::Nothing, KIB),
            (M::StateSet, any, Need::Nothing, 300 * KIB),
            (M::StateDelete, any, Need::Nothing, KIB),
            (M::StateKeys, any, Need::Nothing, KIB),
            (M::ServiceRequest, page, Need::Service, MIB),
            (M::ActionsRun, page, Need::PageRuns, 64 * KIB),
            (M::UiToast, page, Need::Nothing, 2 * KIB),
            (M::UiNavigate, page, Need::Nothing, KIB),
            (M::UiCopyText, page, Need::Nothing, 64 * KIB),
            (M::UiOpenExternal, page, Need::Nothing, 4 * KIB),
            (M::FilesSave, page, Need::Nothing, 48 * MIB),
            (M::FilesOpen, page, Need::Nothing, KIB),
        ];
        assert_eq!(table.map(|row| row.0), HostMethod::ALL);
        for (m, (p, r, s), need, cap) in table {
            assert_eq!(m.callers(), Callers { page: p, run: r, service: s }, "{m:?}");
            assert_eq!((m.needs(), m.max_params()), (need, cap), "{m:?}");
            // Each by its own name, in a call and in JSON.
            assert_eq!(HostMethod::parse(m.name()), Some(m));
            assert_eq!(serde_json::to_value(m).unwrap(), json!(m.name()));
        }
        assert_eq!(HostMethod::ALL.map(HostMethod::name).into_iter().collect::<HashSet<_>>().len(), 19);
        assert_eq!(HostMethod::parse("files.delete"), None);

        let none = Grants { runtime: RuntimeKind::Process, protocol: 1, ..grants(&[]) };
        for m in HostMethod::ALL {
            assert!(check(m, &PAGE, &all()).is_ok(), "{m:?}");
            let without = match m.needs() {
                Need::Nothing => {
                    assert!(check(m, &PAGE, &none).is_ok(), "{m:?}");
                    continue;
                }
                Need::Permission(p) => {
                    Grants { permissions: all().permissions.into_iter().filter(|g| *g != p).collect(), ..all() }
                }
                Need::Network => {
                    Grants { hosts: Vec::new(), ..grants(&[Permission::ExportsRead, Permission::ProjectRead]) }
                }
                Need::SecretKey => Grants { secrets: Vec::new(), ..all() },
                Need::Files => Grants { permissions: vec![Permission::ExportsWrite, Permission::Network], ..all() },
                Need::Service => Grants { runtime: RuntimeKind::Process, ..all() },
                Need::PageRuns => Grants { protocol: 1, ..all() },
            };
            for g in [&without, &none] {
                match check(m, &PAGE, g) {
                    Err(PluginError::Denied(why)) => assert!(why.starts_with(m.name()), "{why}"),
                    other => panic!("{m:?}: {other:?}"),
                }
            }
        }
        // One of the two permissions lets a plugin resolve files; the file's own is checked once its kind is known.
        assert!(check(M::FilesResolve, &PAGE, &grants(&[Permission::ProjectRead])).is_ok());
        let clip = FileRef::Clip { project_id: None, export_id: "e1".into(), index: 0 };
        let episode = FileRef::Episode { project_id: None, media_id: "m1".into() };
        assert_eq!(
            check_file(&clip, &grants(&[Permission::ProjectRead])).unwrap_err().to_string(),
            "files.resolve needs the exports:read permission for that file."
        );
        assert!(check_file(&episode, &grants(&[Permission::ProjectRead])).is_ok());
        assert!(check_file(&episode, &grants(&[Permission::ExportsRead])).is_err());
        assert_eq!(
            check(M::ProjectGet, &PAGE, &none).unwrap_err().to_string(),
            "project.get needs the project:read permission."
        );
    }

    #[test]
    fn a_method_is_refused_to_a_caller_it_isnt_for() {
        let run = Caller::Run { project: Some("p1"), export: None };
        for m in HostMethod::ALL {
            assert!(check(m, &PAGE, &all()).is_ok(), "{m:?}");
            let page_only = !m.callers().run;
            for (caller, who) in [(run, "A run"), (Caller::Service, "A service")] {
                match check(m, &caller, &all()) {
                    Err(PluginError::Denied(why)) if page_only => {
                        assert_eq!(why, format!("{who} can't call {}.", m.name()))
                    }
                    Ok(()) if !page_only => {}
                    other => panic!("{m:?} from {who}: {other:?}"),
                }
            }
        }
        // Page runs need a program that speaks protocol 2, or a WebAssembly
        // plugin of any protocol.
        assert!(Grants { protocol: 2, ..grants(&[]) }.takes_page_runs());
        assert!(!Grants { protocol: 1, ..grants(&[]) }.takes_page_runs());
        assert!(!Grants { runtime: RuntimeKind::None, ..grants(&[]) }.takes_page_runs());
        assert!(Grants { runtime: RuntimeKind::Wasm, protocol: 1, ..grants(&[]) }.takes_page_runs());
        assert!(!grants(&[]).service() && all().service());
    }

    #[test]
    fn a_run_reads_only_its_own_project() {
        let run = Caller::Run { project: Some("p1"), export: Some("e1") };
        assert_eq!(run.project(None).unwrap(), "p1");
        assert_eq!(run.project(Some("p1")).unwrap(), "p1");
        let other = run.project(Some("p2")).unwrap_err();
        assert!(matches!(&other, PluginError::Denied(_)), "{other:?}");
        assert_eq!(other.to_string(), "A run reads only the project it runs in.");
        // A run on an export reads only that one; a command reads any of its project's.
        assert!(run.export("e1").is_ok());
        assert_eq!(run.export("e2").unwrap_err().to_string(), "A run reads only the export it runs on.");
        let command = Caller::Run { project: Some("p1"), export: None };
        assert!(command.export("e2").is_ok());
        // A run outside a project has none to read.
        let outside = Caller::Run { project: None, export: None };
        assert!(matches!(outside.project(None), Err(PluginError::Invalid(_))));
        assert!(matches!(outside.project(Some("p1")), Err(PluginError::Denied(_))));
        // A page reads the one it is open in.
        assert_eq!(PAGE.project(None).unwrap(), "p1");
        assert_eq!(PAGE.project(Some("p2")).unwrap_err().to_string(), "A page reads only the project it is open in.");
        // A service names the one it reads.
        assert_eq!(Caller::Service.project(Some("p2")).unwrap(), "p2");
        assert!(matches!(Caller::Service.project(None), Err(PluginError::Invalid(_))));
    }

    /// An installed plugin whose record holds `agreed` and whose manifest
    /// asks for `asked`, each as `(permissions, hosts)`.
    fn installed(agreed: (&[&str], &[&str]), asked: (&[&str], &[&str])) -> Installed {
        let m = parse_for_any_app(
            json!({
                "manifestVersion": 1,
                "id": "dev.autoscene.poster",
                "version": "1.0.0",
                "name": "Poster",
                "description": "Posts clips.",
                "author": {"name": "Someone"},
                "engines": {"autoscene": ">=1.0.0", "protocol": 2},
                "runtime": {"type": "process", "command": {"*": ["uv", "run", "--script", "--locked", "main.py"]}},
                "permissions": asked.0,
                "network": {"hosts": asked.1}
            })
            .to_string()
            .as_bytes(),
        )
        .unwrap()
        .manifest;
        let permissions = agreed.0.iter().map(|p| serde_json::from_value(json!(p)).unwrap()).collect();
        Installed {
            record: Record {
                v: RECORD_VERSION,
                id: m.id.clone(),
                version: m.version.clone(),
                source: Source::Folder { path: "/plugins/poster".into() },
                trust: Trust::Unreviewed,
                author_key: None,
                permissions,
                network_hosts: agreed.1.iter().map(|h| h.to_string()).collect(),
                enabled: true,
                after_export: Vec::new(),
                lock_sha256: None,
                installed_at: 0,
            },
            manifest: Some(m),
            warnings: Vec::new(),
            dir: PathBuf::from("/plugins/poster"),
            broken: None,
            extra: Extra::default(),
            withdrawn: None,
        }
    }

    #[test]
    fn grants_never_widen_with_a_changed_manifest() {
        // Loaded with exports:read; its plugin.json asks for more since.
        let p = installed((&["exports:read"], &[]), (&["exports:read", "network", "project:read"], &["evil.example"]));
        let g = Grants::of(&p).unwrap();
        assert_eq!(g, Grants { protocol: 2, ..grants(&[Permission::ExportsRead]) });
        assert!(check(HostMethod::ProjectGet, &PAGE, &g).is_err());
        // A host is granted only with the network permission, and only while both name it.
        let p = installed(
            (&["network", "exports:read"], &["discord.com", "gone.example"]),
            (&["network"], &["discord.com", "new.example"]),
        );
        let g = Grants::of(&p).unwrap();
        assert_eq!((g.permissions, g.hosts), (vec![Permission::Network], vec!["discord.com".to_owned()]));
        let p = installed((&["exports:read"], &["discord.com"]), (&["exports:read"], &["discord.com"]));
        assert_eq!(Grants::of(&p).unwrap().hosts, Vec::<String>::new());
        // One installed without an extra row runs as a process, with no secrets in version 1.
        let g = Grants::of(&p).unwrap();
        assert_eq!((g.runtime, g.secrets.len()), (RuntimeKind::Process, 0));
        // The runtime is the one agreed to.
        let mut service = installed((&[], &[]), (&[], &[]));
        service.extra = Extra::agreed(RuntimeKind::Service);
        assert!(Grants::of(&service).unwrap().service());
        // A broken plugin gets none.
        let mut broken = p;
        broken.broken = Some("Its files were changed. Install it again.".into());
        broken.manifest = None;
        assert_eq!(Grants::of(&broken).unwrap_err().to_string(), "Its files were changed. Install it again.");
    }

    #[test]
    fn a_plugin_reads_only_its_own_secrets() {
        let mut p = installed((&[], &[]), (&[], &[]));
        let v2 = json!({
            "manifestVersion": 2,
            "id": "dev.autoscene.poster",
            "version": "1.0.0",
            "name": "Poster",
            "description": "Posts clips.",
            "author": {"name": "Someone"},
            "engines": {"autoscene": ">=1.0.0", "protocol": 2},
            "runtime": {"type": "process", "command": {"*": ["uv", "run", "--script", "--locked", "main.py"]}},
            "contributes": {"settings": [
                {"key": "token", "type": "secret", "title": "Token"},
                {"key": "channel", "type": "string", "title": "Channel"}
            ]}
        });
        p.manifest = Some(parse_for_any_app(v2.to_string().as_bytes()).unwrap().manifest);
        let g = Grants::of(&p).unwrap();
        assert_eq!(g.secrets, ["token"]);
        assert!(check(HostMethod::SecretsGet, &PAGE, &g).is_ok());
        assert!(check_secret("token", &g).is_ok());
        // A plain setting is no secret, and another's key is none of its own.
        for key in ["channel", "webhook"] {
            let e = check_secret(key, &g).unwrap_err();
            assert!(matches!(&e, PluginError::Denied(_)), "{e:?}");
            assert_eq!(
                e.to_string(),
                format!("secrets.get reads only the plugin's secret settings, and {key} isn't one.")
            );
        }
    }

    #[test]
    fn params_past_their_cap_are_refused_before_dispatch() {
        for m in HostMethod::ALL {
            assert!(check_size(m, m.max_params()).is_ok(), "{m:?}");
            let e = check_size(m, m.max_params() + 1).unwrap_err();
            assert!(matches!(e, PluginError::Invalid(_)), "{m:?}: {e:?}");
        }
        assert_eq!(
            check_size(HostMethod::ProjectGet, 1025).unwrap_err().to_string(),
            "The params of project.get are larger than 1 KB."
        );
        assert_eq!(
            check_size(HostMethod::FilesSave, 48 * MIB + 1).unwrap_err().to_string(),
            "The params of files.save are larger than 48 MB."
        );
    }

    #[test]
    fn params_read_with_their_defaults() {
        // As the plugin wrote them.
        let text = |t: &str| RawValue::from_string(t.to_owned()).unwrap();
        let r: ProjectRef = params(HostMethod::ProjectGet, &text("null")).unwrap();
        assert_eq!(r, ProjectRef::default());
        let r: ExportRef = params(HostMethod::ExportsGet, &text(r#"{"exportId": "e1", "later": [1]}"#)).unwrap();
        assert_eq!((r.project_id, r.export_id.as_str()), (None, "e1"));
        let f: FileRef =
            params(HostMethod::FilesResolve, &text(r#"{"kind":"clip","exportId":"e1","index":2}"#)).unwrap();
        assert_eq!(f, FileRef::Clip { project_id: None, export_id: "e1".into(), index: 2 });
        let f: FileRef = params(HostMethod::FilesResolve, &text(r#"{"kind":"episode","mediaId":"m1"}"#)).unwrap();
        assert_eq!((f.project_id(), f.need()), (None, Permission::ProjectRead));
        // Said without where in the params it broke.
        let e = params::<ExportRef>(HostMethod::ExportsGet, &text("{}")).unwrap_err();
        assert_eq!(e.to_string(), "The params of exports.get don't fit: missing field `exportId`.");
        let e = params::<ExportRef>(HostMethod::ExportsGet, &text("7")).unwrap_err();
        assert_eq!(
            e.to_string(),
            "The params of exports.get don't fit: invalid type: integer `7`, expected struct ExportRef."
        );
        let t: Toast = params(HostMethod::UiToast, &text(r#"{"text":"Hi"}"#)).unwrap();
        assert_eq!(t.tone, ToastTone::Neutral);
        // A run names an export for an export action; without, it runs a command.
        let r: ActionsRun = params(HostMethod::ActionsRun, &text(r#"{"action":"post","exportId":"e1"}"#)).unwrap();
        assert_eq!((r.kind(), r.input), (ActionKind::ExportAction, None));
        let r: ActionsRun = params(HostMethod::ActionsRun, &text(r#"{"action":"hello","input":{"n":1}}"#)).unwrap();
        assert_eq!((r.kind(), r.input), (ActionKind::Command, Some(json!({"n": 1}))));
    }

    #[test]
    fn plugin_errors_become_call_errors_with_their_words() {
        let code = |e: PluginError| {
            let c = CallError::from(&e);
            assert_eq!(c.message, e.to_string());
            c.code
        };
        assert_eq!(code(PluginError::Denied("No.".into())), CallCode::Denied);
        assert_eq!(code(PluginError::Invalid("Bad.".into())), CallCode::Invalid);
        assert_eq!(code(PluginError::TooNew("protocol".into())), CallCode::Invalid);
        assert_eq!(code(PluginError::NotFound("dev.a.b".into())), CallCode::NotFound);
        assert_eq!(code(PluginError::Busy("Poster".into())), CallCode::Busy);
        assert_eq!(code(PluginError::Cancelled), CallCode::Cancelled);
        assert_eq!(code(PluginError::Closed), CallCode::Closed);
        assert_eq!(code(PluginError::Io(std::io::Error::other("disk"))), CallCode::Failed);
        assert_eq!(
            serde_json::to_string(&CallError::new(CallCode::NotFound, "Gone.")).unwrap(),
            r#"{"code":"not_found","message":"Gone."}"#
        );
        for c in [CallCode::Denied, CallCode::NotFound, CallCode::Busy] {
            assert_eq!(serde_json::to_value(c).unwrap(), json!(c.name()));
        }
    }

    #[test]
    fn media_files_have_their_types() {
        let t = |name: &str| media_type(Path::new(name));
        assert_eq!(t("/out/Jesse 001.MP4"), Some("video/mp4"));
        assert_eq!(t("S01E01.mkv"), Some("video/x-matroska"));
        assert_eq!(t("a.flac"), Some("audio/flac"));
        assert_eq!((t("notes.txt"), t("Makefile"), t("page.html")), (None, None, None));
    }
}

//! What AutoScene answers a plugin's calls with (protocol 2): its settings
//! and its secrets, its project, the project's exports and their files, the
//! state it keeps, and the hosts it names on the internet. [`AppCalls`]
//! answers a run's calls, bound to the plugin, project and export the run
//! is for, and an open page's ([`page_call`]), bound to its session. Each
//! call is checked first (its params' size, then its method against the
//! plugin's grants, then the project and the export it names; see
//! [`as_plugin::calls`]), and only then is anything read. A call of a run
//! that was cancelled, or whose program ended, writes nothing, and sends
//! nothing more: a request on its way stops where it is
//! ([`as_plugin::net`]).
//!
//! A page gets no path: a project and its exports without them, and a file
//! as an address of its own session that the asp scheme serves. Its own
//! methods reach outside AutoScene's data only through the person's step
//! ([`PageIo`]): a toast the app's page shows, always starting with
//! `Plugin “<name>”:`, a move inside the page, the clipboard, a link they
//! agree to open, and a file they save or pick in the system's dialog. Its
//! toasts, copies and dialogs are counted ([`PageLimits`]): a page can't
//! fill the screen or the clipboard, nor keep the person in its dialogs. A
//! page that closes ends what it still waits for ([`Mount::closed`]).
//!
//! A page's request goes to its plugin's service, started for it when none
//! runs ([`super::services`]). The service's own calls are answered as a
//! run's, and it names the project it reads: its program runs with the
//! person's rights anyway.
//!
//! A page starts one of its plugin's actions as a run, queued as the
//! person's own are ([`super::queue_run`]): in the project the page is open
//! in, and on an export of that project only.

use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use as_api::{ExportStatus, ExportSummary, MediaStatus};
use as_core::{CancelToken, ExportId, JobId, MediaId, ProjectId};
use as_plugin::calls::{
    self, ActionsRun, CallCode, CallError, Caller, CopyText, ExportRef, FileFilter, FileRef, Grants, HostCalls,
    HostMethod, Navigate, NetRequest, OpenExternal, OpenFile, OpenedFile, PageClip, PageEpisode, PageExport,
    PageExportSummary, PageProject, ProgramEpisode, ProgramProject, ProjectRef, ResolvedFile, SaveFile, SecretsGet,
    ServiceRequestParams, StateKey, StateSet, Toast, ToastTone, runnable_extension, runnable_name,
};
use as_plugin::manifest::plain_link;
use as_plugin::pages::{self, Mount};
use as_plugin::service::{MAX_REQUEST_TIMEOUT, REQUEST_TIMEOUT};
use as_plugin::{
    ActionKind, Installed, PluginError, PluginId, PluginResult, RequestContext, net, package, page_state, runtime,
};
use as_project::{Library, Project, ProjectError, ProjectResult};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::value::RawValue;
use serde_json::{Value, json};
use specta::Type;
use specta_typescript::Unknown;

use super::{PluginHost, RunKey, export_context, serde_name};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::paths;

/// A run's or an open page's calls, bound to its plugin and the project
/// (and a run's export) it is for. Its grants are the plugin's as the run
/// started or the page opened: a change of the plugin stops its runs and
/// closes its pages.
pub struct AppCalls {
    places: Arc<dyn Places>,
    host: Arc<PluginHost>,
    plugin: PluginId,
    grants: Grants,
    project: Option<String>,
    export: Option<String>,
    /// The open page that calls, when a page does.
    page: Option<Arc<Mount>>,
    /// Its plugin's service calls: it names the project it reads.
    service: bool,
}

/// Where calls, and a run's line, find a project and its exports: the
/// library (a test's stand-in has exports no export job wrote).
pub(super) trait Places: Send + Sync {
    fn project(&self, id: ProjectId) -> ProjectResult<Arc<Project>>;
    fn exports(&self, project: &Project) -> ProjectResult<Vec<ExportSummary>>;
    fn export(&self, project: &Project, id: ExportId) -> ProjectResult<ExportSummary>;
}

impl Places for Library {
    fn project(&self, id: ProjectId) -> ProjectResult<Arc<Project>> {
        Library::project(self, id)
    }

    fn exports(&self, project: &Project) -> ProjectResult<Vec<ExportSummary>> {
        project.exports()
    }

    fn export(&self, project: &Project, id: ExportId) -> ProjectResult<ExportSummary> {
        project.export(id)
    }
}

impl AppCalls {
    /// The calls of the run `key` of `p`.
    pub(super) fn run(library: Arc<Library>, host: Arc<PluginHost>, p: &Installed, key: &RunKey) -> PluginResult<Self> {
        Ok(Self {
            places: library,
            host,
            plugin: p.record.id.clone(),
            grants: Grants::of(p)?,
            project: key.project_id.map(|id| id.to_string()),
            export: key.export_id.map(|id| id.to_string()),
            page: None,
            service: false,
        })
    }

    /// The calls of the service of `p`, in no project of its own.
    pub(super) fn service(library: Arc<Library>, host: Arc<PluginHost>, p: &Installed) -> PluginResult<Self> {
        Ok(Self {
            places: library,
            host,
            plugin: p.record.id.clone(),
            grants: Grants::of(p)?,
            project: None,
            export: None,
            page: None,
            service: true,
        })
    }

    /// The calls of the open page `m`, in the project it is open in.
    fn page(places: Arc<dyn Places>, host: Arc<PluginHost>, m: Arc<Mount>) -> Self {
        Self {
            places,
            host,
            plugin: m.plugin.clone(),
            grants: m.grants.clone(),
            project: m.project.clone(),
            export: None,
            page: Some(m),
            service: false,
        }
    }

    fn caller(&self) -> Caller<'_> {
        match (&self.page, self.service) {
            (Some(_), _) => Caller::Page { project: self.project.as_deref() },
            (None, true) => Caller::Service,
            (None, false) => Caller::Run { project: self.project.as_deref(), export: self.export.as_deref() },
        }
    }

    /// Whether a call may still change something: not once its run was
    /// cancelled, its program ended or its service stopped. A change of the
    /// plugin (an uninstall that deletes its data, say) stops them and waits
    /// for them, and they for what their calls write.
    fn still_running(&self, ended: &CancelToken) -> Result<(), CallError> {
        match ended.is_cancelled() {
            true => Err(self.ended()),
            false => Ok(()),
        }
    }

    /// The answer of a call that came, or still waited, as its run ended or
    /// its service stopped.
    fn ended(&self) -> CallError {
        let said = if self.service { "This service has stopped." } else { "This run has ended." };
        CallError::new(CallCode::Cancelled, said)
    }

    /// `e` as a call gets it, said as [`AppCalls::still_running`] says it
    /// when its run ended while the call waited its turn.
    fn unless_ended(&self, e: PluginError) -> CallError {
        match e {
            PluginError::Cancelled => self.ended(),
            e => e.into(),
        }
    }

    /// The project a call that names `asked` (or none) reads.
    fn project(&self, asked: Option<&str>) -> Result<Arc<Project>, CallError> {
        let id = self.caller().project(asked)?;
        let id = ProjectId::parse(id).map_err(|_| not_found("That project doesn't exist."))?;
        self.places.project(id).map_err(project_error)
    }

    /// The export `id` of `project`, when the call may read it.
    fn export(&self, project: &Project, id: &str) -> Result<ExportSummary, CallError> {
        self.caller().export(id)?;
        let id = ExportId::parse(id).map_err(|_| not_found("That export doesn't exist."))?;
        self.places.export(project, id).map_err(project_error)
    }

    /// The path of `file`, which the call may read.
    fn path(&self, file: &FileRef) -> Result<String, CallError> {
        calls::check_file(file, &self.grants)?;
        let project = self.project(file.project_id())?;
        match file {
            FileRef::Clip { export_id, index, .. } => {
                let export = self.export(&project, export_id)?;
                let clip = export
                    .items
                    .get(*index as usize)
                    .ok_or_else(|| not_found(format!("That export has no clip {index}.")))?;
                clip.path.clone().ok_or_else(|| not_found("That clip isn't written yet."))
            }
            FileRef::Merged { export_id, .. } => {
                let merged = self.export(&project, export_id)?.merged;
                let merged = merged.ok_or_else(|| not_found("That export isn't joined into one file."))?;
                merged.path.ok_or_else(|| not_found("That export's file isn't written yet."))
            }
            FileRef::Episode { media_id, .. } => {
                let id = MediaId::parse(media_id).map_err(|_| not_found("That episode isn't in the project."))?;
                Ok(project.media(id).map_err(project_error)?.path)
            }
        }
    }
}

impl HostCalls for AppCalls {
    fn call(&self, method: HostMethod, params: &RawValue, ended: &CancelToken) -> Result<Value, CallError> {
        self.checked(method, params)?;
        self.answer(method, params, ended)
    }
}

impl AppCalls {
    /// Whether the call may be answered at all. Its size first: nothing else
    /// is read of params past their cap.
    fn checked(&self, method: HostMethod, params: &RawValue) -> Result<(), CallError> {
        calls::check_size(method, params.get().len())?;
        Ok(calls::check(method, &self.caller(), &self.grants)?)
    }

    /// The answer to a call [`AppCalls::checked`] let through, of a method
    /// a run may call too.
    fn answer(&self, method: HostMethod, params: &RawValue, ended: &CancelToken) -> Result<Value, CallError> {
        let (store, id) = (&self.host.store, &self.plugin);
        match method {
            HostMethod::SettingsGet => Ok(answer(store.values(id)?)),
            // Read from the keychain at each call, never kept: the value goes
            // only into this reply.
            HostMethod::SecretsGet => {
                let SecretsGet { key } = calls::params(method, params)?;
                calls::check_secret(&key, &self.grants)?;
                Ok(json!({ "value": store.secret(id, &key)? }))
            }
            // A page's without paths.
            HostMethod::ProjectGet => {
                let asked: ProjectRef = calls::params(method, params)?;
                let project = self.project(asked.project_id.as_deref())?;
                match self.page {
                    Some(_) => Ok(answer(page_project(&project).map_err(project_error)?)),
                    None => Ok(answer(program_project(&project).map_err(project_error)?)),
                }
            }
            HostMethod::ExportsList => {
                let asked: ProjectRef = calls::params(method, params)?;
                let project = self.project(asked.project_id.as_deref())?;
                let exports = self.places.exports(&project).map_err(project_error)?;
                Ok(answer(exports.iter().map(export_summary).collect::<Vec<_>>()))
            }
            // As an export action's run line has it, with each clip's
            // episode file: a run's program may read it anyway. A page's
            // without paths.
            HostMethod::ExportsGet => {
                let asked: ExportRef = calls::params(method, params)?;
                let project = self.project(asked.project_id.as_deref())?;
                let export = self.export(&project, &asked.export_id)?;
                let context = export_context(&project, &export).map_err(project_error)?;
                match self.page {
                    Some(_) => Ok(answer(page_export(context))),
                    None => Ok(answer(context)),
                }
            }
            // A page gets an address of its session in place of the path,
            // for a picture, a video or a sound only.
            HostMethod::FilesResolve => {
                let path = self.path(&calls::params(method, params)?)?;
                let size = std::fs::metadata(&path)
                    .ok()
                    .filter(|f| f.is_file())
                    .ok_or_else(|| not_found("That file isn't where it was."))?
                    .len();
                let file = match &self.page {
                    Some(m) => {
                        let url = self.host.pages.resolve(m, PathBuf::from(&path))?;
                        let mime = pages::media_type(Path::new(&path)).unwrap_or("application/octet-stream");
                        ResolvedFile { url: Some(url), path: None, mime: mime.to_owned(), size: size as f64 }
                    }
                    None => {
                        let mime = calls::media_type(Path::new(&path)).unwrap_or("application/octet-stream");
                        ResolvedFile { url: None, path: Some(path), mime: mime.to_owned(), size: size as f64 }
                    }
                };
                Ok(answer(file))
            }
            HostMethod::StateGet => {
                let StateKey { key } = calls::params(method, params)?;
                Ok(json!({ "value": page_state::get(&store.data_dir(id), &key)? }))
            }
            HostMethod::StateSet => {
                let StateSet { key, value } = calls::params(method, params)?;
                self.still_running(ended)?;
                page_state::set(&store.data_dir(id), &key, value, ended).map_err(|e| self.unless_ended(e))?;
                Ok(Value::Null)
            }
            HostMethod::StateDelete => {
                let StateKey { key } = calls::params(method, params)?;
                self.still_running(ended)?;
                page_state::delete(&store.data_dir(id), &key, ended).map_err(|e| self.unless_ended(e))?;
                Ok(Value::Null)
            }
            HostMethod::StateKeys => Ok(answer(page_state::keys(&store.data_dir(id))?)),
            // On this call's own thread, which waits for the answer. It may
            // post somewhere, which a run that ended does no more: fetch
            // stops where it is once `ended` fires.
            HostMethod::NetFetch => {
                let req: NetRequest = calls::params(method, params)?;
                self.still_running(ended)?;
                let _fetching = Fetching::start(&self.host, id)?;
                let agent = format!("AutoScene/{} (plugin {id})", store.app_version());
                match net::fetch(req, &self.grants.hosts, &agent, &*self.host.net, ended) {
                    Ok(got) => Ok(answer(got)),
                    Err(PluginError::Cancelled) => Err(self.ended()),
                    Err(e) => Err(e.into()),
                }
            }
            // A page's own: `check` refused them to a run, and
            // `page_answer` answers a page's before it comes here.
            HostMethod::ServiceRequest
            | HostMethod::ActionsRun
            | HostMethod::UiToast
            | HostMethod::UiNavigate
            | HostMethod::UiCopyText
            | HostMethod::UiOpenExternal
            | HostMethod::FilesSave
            | HostMethod::FilesOpen => Err(PluginError::Denied(format!("Only a page calls {}.", method.name())).into()),
        }
    }

    /// The answer to a call of the open page `m`, at `now`: of one of its
    /// own methods, else as [`AppCalls::answer`] gives it. A page that
    /// closes meanwhile ends the call: it writes nothing more, and does
    /// nothing with what the person said in a dialog it opened.
    fn page_answer(
        &self,
        m: &Mount,
        method: HostMethod,
        params: &RawValue,
        io: &dyn PageIo,
        now: Instant,
    ) -> Result<PageAnswer, CallError> {
        self.checked(method, params)?;
        let limits = &self.host.page_limits;
        match method {
            HostMethod::UiToast => {
                let Toast { text, description, tone } = calls::params(method, params)?;
                let (text, description) = toast_words(&text, description.as_deref())?;
                if !limits.toast(&m.token, now) {
                    return Ok(PageAnswer::value(json!({ "shown": false })));
                }
                // The plugin's words after a word it can't choose: its page
                // may be called anything, AutoScene too.
                let title = format!("Plugin “{}”: {text}", m.name);
                Ok(PageAnswer::effect(PageEffect::Toast { title, description, tone }, json!({ "shown": true })))
            }
            HostMethod::UiNavigate => {
                let Navigate { path, replace } = calls::params(method, params)?;
                check_page_path(&path)?;
                Ok(PageAnswer::effect(PageEffect::Navigate { path, replace }, Value::Null))
            }
            HostMethod::UiCopyText => {
                let CopyText { text } = calls::params(method, params)?;
                if text.len() > MAX_COPIED || text.contains('\0') {
                    return Err(invalid("ui.copyText copies text of up to 64 KB, without NUL characters."));
                }
                if !io.in_front() {
                    return Err(denied("A page copies only while AutoScene is in front."));
                }
                if !limits.copy(&m.token, now) {
                    return Err(CallError::new(CallCode::Busy, "A page copies at most 10 times a minute."));
                }
                io.copy_text(&text).map_err(|e| failed("AutoScene couldn't copy that.", &e))?;
                Ok(PageAnswer::value(Value::Null))
            }
            // The person's yes in a dialog of the system's own is the gate:
            // the page can neither answer nor change it.
            HostMethod::UiOpenExternal => {
                let OpenExternal { url } = calls::params(method, params)?;
                if !plain_link(&url) {
                    return Err(invalid("ui.openExternal opens only plain https:// links."));
                }
                let asked = format!("Plugin “{}” asks to open:\n\n{url}", m.name);
                let dialog = limits.dialog(m, io, now)?;
                let yes = io.confirm(OPEN_LINK_TITLE, &asked, OPEN_LINK_YES);
                dialog.closed(yes)?;
                if !yes {
                    return Ok(PageAnswer::value(json!({ "opened": false })));
                }
                io.open_url(&url).map_err(|e| failed("AutoScene couldn't open the link.", &e))?;
                Ok(PageAnswer::value(json!({ "opened": true })))
            }
            // Where the person chose in the system's dialog: no grant needed.
            // Never a program, a script or a shortcut, which would run as
            // the person opens it.
            HostMethod::FilesSave => {
                let SaveFile { name, text, data_base64, filters } = calls::params(method, params)?;
                check_save_name(&name)?;
                check_filters(&filters)?;
                if filters.iter().flat_map(|f| &f.extensions).any(|e| runnable_extension(e)) {
                    return Err(runnable_refused());
                }
                let too_large = || invalid("A page saves files of up to 32 MB.");
                let bytes = match (text, data_base64) {
                    (Some(text), None) => text.into_bytes(),
                    // Four letters of base64 are three bytes: one too large
                    // isn't decoded.
                    (None, Some(data)) if data.len() / 4 * 3 > MAX_SAVED + 3 => return Err(too_large()),
                    (None, Some(data)) => {
                        BASE64.decode(data).map_err(|_| invalid("The dataBase64 of files.save isn't base64."))?
                    }
                    _ => return Err(invalid("files.save takes text or dataBase64, one of them.")),
                };
                if bytes.len() > MAX_SAVED {
                    return Err(too_large());
                }
                let dialog = limits.dialog(m, io, now)?;
                let path = io.save_to(&format!("Plugin “{}” saves a file", m.name), &name, &filters);
                dialog.closed(path.is_some())?;
                let Some(path) = path else { return Ok(PageAnswer::value(json!({ "saved": false }))) };
                paths::check(&path).map_err(|e| invalid(e.message))?;
                // The name the person typed, or the one the dialog made of it.
                if path.file_name().is_some_and(|n| runnable_name(&n.to_string_lossy())) {
                    return Err(runnable_refused());
                }
                write_whole(&path, &bytes).map_err(|e| failed("AutoScene couldn't save the file.", &e.to_string()))?;
                Ok(PageAnswer::value(json!({ "saved": true })))
            }
            // Its contents only: a page never gets the path.
            HostMethod::FilesOpen => {
                let OpenFile { filters, max_bytes } = calls::params(method, params)?;
                check_filters(&filters)?;
                let most = max_bytes.map_or(MAX_OPENED, |n| u64::from(n).min(MAX_OPENED));
                let dialog = limits.dialog(m, io, now)?;
                let path = io.pick(&format!("Plugin “{}” asks for a file", m.name), &filters);
                dialog.closed(path.is_some())?;
                let Some(path) = path else { return Ok(PageAnswer::value(Value::Null)) };
                paths::check(&path).map_err(|e| invalid(e.message))?;
                Ok(PageAnswer::value(answer(read_opened(&path, most)?)))
            }
            // A read may ask the person in a dialog of the system's (macOS's
            // keychain after an update, a locked one on Linux): one at a
            // time, and none again once one failed, until the page opens
            // again. So a page can't keep the person in those dialogs either.
            HostMethod::SecretsGet => {
                let SecretsGet { key } = calls::params(method, params)?;
                calls::check_secret(&key, &self.grants)?;
                let turn = limits.keychain(&m.token);
                let mut refused = turn.lock();
                if let Some(e) = &*refused {
                    return Err(e.clone());
                }
                match self.host.store.secret(&self.plugin, &key) {
                    Ok(value) => Ok(PageAnswer::value(json!({ "value": value }))),
                    Err(e) => Err(refused.insert(e.into()).clone()),
                }
            }
            // Its plugin's service, started for it when none runs. Its answer
            // is the page's only while the page is open.
            HostMethod::ServiceRequest => {
                let ServiceRequestParams { method: asked, params, timeout_ms } = calls::params(method, params)?;
                if !(1..=MAX_SERVICE_METHOD).contains(&asked.chars().count()) {
                    return Err(invalid("service.request names a method of 1 to 64 characters."));
                }
                let timeout = timeout_ms.map_or(REQUEST_TIMEOUT, |ms| Duration::from_millis(ms.into()));
                // Its start counts too: the page gives up once its time is
                // over, and the start goes on for its plugin's other pages.
                let until = Instant::now() + timeout.min(MAX_REQUEST_TIMEOUT);
                let service = self.host.service(m, Some(until))?;
                let context = RequestContext { page: m.page.id.clone(), project_id: m.project.clone() };
                let left = until.saturating_duration_since(Instant::now());
                match service.request(&asked, params, context, left, &m.closed) {
                    Err(_) if m.closed.is_cancelled() => Err(PluginError::Closed.into()),
                    answered => Ok(PageAnswer::value(answered?)),
                }
            }
            // One of its plugin's actions as a run in Activity, queued as the
            // person's own are: in the project the page is open in, and an
            // export action on an export of that project only.
            HostMethod::ActionsRun => {
                let asked: ActionsRun = calls::params(method, params)?;
                let kind = asked.kind();
                let ActionsRun { action, export_id, input } = asked;
                let export_id = match export_id {
                    Some(id) => Some(self.export(&*self.project(None)?, &id)?.id),
                    None => None,
                };
                let project_id = m.project.as_deref().and_then(|id| ProjectId::parse(id).ok());
                let job = io.queue_run(&m.plugin, kind, &action, project_id, export_id, input).map_err(app_error)?;
                Ok(PageAnswer::value(json!({ "jobId": job })))
            }
            // As a run's call, which its page's close ends as a run's end does.
            _ => match self.answer(method, params, &m.closed) {
                Err(_) if m.closed.is_cancelled() => Err(PluginError::Closed.into()),
                answered => Ok(PageAnswer::value(answered?)),
            },
        }
    }
}

/// The text and description of a toast as the app shows them: 1 to 200
/// characters and at most 300, without what shows nothing or moves the
/// text around.
fn toast_words(text: &str, description: Option<&str>) -> Result<(String, Option<String>), CallError> {
    let n = text.chars().count();
    let shown = runtime::sanitize(text, MAX_TOAST);
    if !(1..=MAX_TOAST).contains(&n) || shown.is_empty() {
        return Err(invalid("A toast says 1 to 200 characters."));
    }
    if description.is_some_and(|d| d.chars().count() > MAX_TOAST_DESCRIPTION) {
        return Err(invalid("A toast's description is at most 300 characters."));
    }
    let description = description.map(|d| runtime::sanitize(d, MAX_TOAST_DESCRIPTION)).filter(|d| !d.is_empty());
    Ok((shown, description))
}

/// Whether `path` stays inside the page: relative, of at most 512
/// characters, each part of the letters a URL path keeps as they are (a `%`
/// only before two hex digits, and no `$`, which marks a param to the app's
/// router), and none of them `.` or `..`, or empty but the last.
fn check_page_path(path: &str) -> Result<(), CallError> {
    let hex = |b: Option<&u8>| b.is_some_and(u8::is_ascii_hexdigit);
    let escaped =
        |part: &[u8]| (0..part.len()).all(|i| part[i] != b'%' || (hex(part.get(i + 1)) && hex(part.get(i + 2))));
    let fits = |part: &str| {
        part.bytes().all(|b| b.is_ascii_alphanumeric() || b"._~!&'()*+,;=:@%-".contains(&b)) && escaped(part.as_bytes())
    };
    let parts: Vec<&str> = path.split('/').collect();
    let last = parts.len() - 1;
    let inside = path.len() <= MAX_PAGE_PATH
        && parts.iter().enumerate().all(|(i, p)| fits(p) && *p != "." && *p != ".." && (i == last || !p.is_empty()));
    match inside {
        true => Ok(()),
        false => Err(invalid("ui.navigate takes a path inside the page, like clips/3.")),
    }
}

/// Whether `name` is a plain file name of up to 100 characters, as
/// `files.save` offers it: one part, of the letters a package's paths use,
/// and of no kind that runs ([`calls::RUNNABLE`]).
fn check_save_name(name: &str) -> Result<(), CallError> {
    if !(name.len() <= MAX_SAVE_NAME && !name.contains('/') && package::check_path(name).is_ok()) {
        return Err(invalid("files.save takes a plain file name of up to 100 characters, like clips.txt."));
    }
    match runnable_name(name) {
        true => Err(runnable_refused()),
        false => Ok(()),
    }
}

fn runnable_refused() -> CallError {
    invalid("A page can't save a program, a script or a shortcut.")
}

/// Whether the kinds of file a dialog offers are plain: at most 16, each
/// named, with extensions of up to 16 letters and digits.
fn check_filters(filters: &[FileFilter]) -> Result<(), CallError> {
    let plain = |f: &FileFilter| {
        let ext = |e: &String| (1..=16).contains(&e.len()) && e.bytes().all(|b| b.is_ascii_alphanumeric());
        let name = f.name.chars().count();
        (1..=60).contains(&name)
            && f.name == runtime::sanitize(&f.name, 60)
            && f.extensions.len() <= 16
            && f.extensions.iter().all(ext)
    };
    match filters.len() <= 16 && filters.iter().all(plain) {
        true => Ok(()),
        false => Err(invalid("A file dialog offers at most 16 kinds of file, each named, with plain extensions.")),
    }
}

/// The file at `path` that the person picked, when it has at most `most`
/// bytes: its name and contents.
fn read_opened(path: &Path, most: u64) -> Result<OpenedFile, CallError> {
    let larger = || invalid(format!("That file is larger than {}.", size_words(most)));
    let unread = |e: std::io::Error| failed("AutoScene couldn't read that file.", &e.to_string());
    let file = std::fs::File::open(path).map_err(unread)?;
    if file.metadata().map_err(unread)?.len() > most {
        return Err(larger());
    }
    // It may have grown since: never more than `most` is read.
    let mut bytes = Vec::new();
    file.take(most + 1).read_to_end(&mut bytes).map_err(unread)?;
    if bytes.len() as u64 > most {
        return Err(larger());
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(OpenedFile { name, size: bytes.len() as f64, data_base64: BASE64.encode(&bytes) })
}

/// Writes `bytes` to `path` whole or not at all: into a file beside it
/// first, which then takes its place, marked as a download
/// ([`mark_downloaded`]).
fn write_whole(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let name = path.file_name().ok_or_else(|| std::io::Error::other("no file name"))?;
    let part = path.with_file_name(format!(".{}.part", name.to_string_lossy()));
    let written = std::fs::write(&part, bytes).and_then(|()| {
        mark_downloaded(&part);
        std::fs::rename(&part, path)
    });
    written.inspect_err(|_| {
        let _ = std::fs::remove_file(&part);
    })
}

/// Marks the file at `path` as one from the internet, as a browser marks
/// its downloads: a plugin's code made it (a page's `files.save`, a pack
/// action). Windows then asks before it runs
/// anything in it, and opens a document in it read-only. A disk that keeps
/// no such marks (FAT, some network shares) saves the file without one.
#[cfg(windows)]
pub(super) fn mark_downloaded(path: &Path) {
    let mut zone = path.as_os_str().to_owned();
    zone.push(":Zone.Identifier");
    if let Err(e) = std::fs::write(&zone, "[ZoneTransfer]\r\nZoneId=3\r\n") {
        log::warn!("a plugin's saved file isn't marked as a download: {e}");
    }
}

/// Marks the file at `path` as one from the internet, as a browser marks
/// its downloads: Gatekeeper then checks what it is, or what comes out of
/// it, before it runs. A disk that keeps no such marks saves the file
/// without one.
#[cfg(target_os = "macos")]
pub(super) fn mark_downloaded(path: &Path) {
    use std::os::unix::ffi::OsStrExt;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    // Downloaded, by AutoScene, not yet opened.
    let value = format!("0083;{now:x};AutoScene;");
    let (Ok(path), Ok(name)) =
        (std::ffi::CString::new(path.as_os_str().as_bytes()), std::ffi::CString::new("com.apple.quarantine"))
    else {
        return;
    };
    // SAFETY: both names are NUL-terminated, and `value` lives for the call.
    let set = unsafe { libc::setxattr(path.as_ptr(), name.as_ptr(), value.as_ptr().cast(), value.len(), 0, 0) };
    if set != 0 {
        log::warn!("a plugin's saved file isn't marked as a download: {}", std::io::Error::last_os_error());
    }
}

/// Nothing marks a file so here: [`calls::RUNNABLE`] keeps out what would run.
#[cfg(not(any(windows, target_os = "macos")))]
pub(super) fn mark_downloaded(_: &Path) {}

/// `n` bytes as people read them: 16 MB, 512 KB, 100 bytes.
fn size_words(n: u64) -> String {
    let short = |x: f64| {
        let s = format!("{x:.1}");
        s.strip_suffix(".0").map(str::to_owned).unwrap_or(s)
    };
    match n {
        n if n >= 1 << 20 => format!("{} MB", short(n as f64 / f64::from(1 << 20))),
        n if n >= 1 << 10 => format!("{} KB", short(n as f64 / f64::from(1 << 10))),
        n => format!("{n} bytes"),
    }
}

/// The most characters of a toast's text, and of its description.
const MAX_TOAST: usize = 200;
const MAX_TOAST_DESCRIPTION: usize = 300;
/// The longest path inside a page.
const MAX_PAGE_PATH: usize = 512;
/// The most text a page copies at once.
const MAX_COPIED: usize = 64 << 10;
/// The longest name of a file a page saves, and the most it saves.
const MAX_SAVE_NAME: usize = 100;
const MAX_SAVED: usize = 32 << 20;
/// The most of a picked file a page reads.
const MAX_OPENED: u64 = 16 << 20;
/// The longest name of a method of a service that a page asks for.
const MAX_SERVICE_METHOD: usize = 64;
/// The most calls of one page AutoScene answers at once.
pub const MAX_PAGE_CALLS: usize = 16;

/// The title and the yes of the dialog that asks before a plugin's link
/// opens: one its page asks to open (`ui.openExternal`), and its source
/// (`plugin_open_source`).
pub(crate) const OPEN_LINK_TITLE: &str = "Open this link in your browser?";
pub(crate) const OPEN_LINK_YES: &str = "Open";

/// What an open page's call gets: its answer; an effect the app's page
/// carries out, then answers with `result`; or an error.
#[derive(Clone, Debug, PartialEq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PageReply {
    Ok {
        #[specta(type = Unknown)]
        result: Value,
    },
    /// The app's page does `effect`, then answers the page with `result`.
    Effect {
        effect: PageEffect,
        #[specta(type = Unknown)]
        result: Value,
    },
    Err {
        error: CallError,
    },
}

/// What a page's call asks the app's page to do.
#[derive(Clone, Debug, PartialEq, Serialize, Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PageEffect {
    /// A toast. `title` is its whole first line, `Plugin “<name>”: <text>`:
    /// the app's page shows it as it is.
    Toast { title: String, description: Option<String>, tone: ToastTone },
    /// The path inside the page becomes `path` (the route's splat).
    Navigate { path: String, replace: bool },
}

/// A page's call answered, with what the app's page does first.
struct PageAnswer {
    result: Value,
    effect: Option<PageEffect>,
}

impl PageAnswer {
    fn value(result: Value) -> Self {
        Self { result, effect: None }
    }

    fn effect(effect: PageEffect, result: Value) -> Self {
        Self { result, effect: Some(effect) }
    }
}

/// What a page's own methods reach outside AutoScene's data, each only
/// through the person's step: the clipboard, a link they said yes to in a
/// dialog of the system's own, and a file they save or pick in the system's
/// dialog; and the runs it starts, which the person sees and stops in
/// Activity. The app goes through Tauri's plugins and its job runner
/// (`commands::plugins`), a test through its own.
pub trait PageIo: Send + Sync {
    /// Whether AutoScene's window has the person's focus: a page copies and
    /// opens dialogs only then, never while they work in another app.
    fn in_front(&self) -> bool;
    fn copy_text(&self, text: &str) -> Result<(), String>;
    /// Asks the person `title`, as `text` says, with `yes` and Cancel:
    /// whether they said yes.
    fn confirm(&self, title: &str, text: &str, yes: &str) -> bool;
    /// Opens `url` in the person's browser.
    fn open_url(&self, url: &str) -> Result<(), String>;
    /// Where the person saves the file `name`, in a dialog titled `title`;
    /// none when they cancel.
    fn save_to(&self, title: &str, name: &str, filters: &[FileFilter]) -> Option<PathBuf>;
    /// The file the person picks in a dialog titled `title`; none when they
    /// cancel.
    fn pick(&self, title: &str, filters: &[FileFilter]) -> Option<PathBuf>;
    /// Queues the run of `action` (of `kind`) of the plugin `id` that its
    /// page started, in the project `project_id` and on the export
    /// `export_id`, with what the page handed it, as [`super::queue_run`]
    /// does: none when the same run waits or runs already.
    fn queue_run(
        &self,
        id: &PluginId,
        kind: ActionKind,
        action: &str,
        project_id: Option<ProjectId>,
        export_id: Option<ExportId>,
        input: Option<Value>,
    ) -> AppResult<Option<JobId>>;
}

/// The call of `method` with `params` (their JSON, as the app's page hands
/// it on) of the open page `session`. A page that closed gets `closed`,
/// never an error of the command, so a closed page's last calls make no
/// toast. Past [`MAX_PAGE_CALLS`] at once a call gets `busy`.
pub fn page_call(
    library: Arc<Library>,
    host: &Arc<PluginHost>,
    io: &dyn PageIo,
    session: &str,
    method: &str,
    params: &str,
) -> PageReply {
    page_reply(library, host, io, session, method, params, Instant::now())
}

/// [`page_call`] at `now`, with the project and its exports from `places`.
/// Params past their method's cap are refused before anything reads them:
/// they are only text until then, never a tree of values.
fn page_reply(
    places: Arc<dyn Places>,
    host: &Arc<PluginHost>,
    io: &dyn PageIo,
    session: &str,
    method: &str,
    params: &str,
    now: Instant,
) -> PageReply {
    let refused = |code, message: String| PageReply::Err { error: CallError::new(code, message) };
    let Some(m) = host.pages.get(session) else {
        return refused(CallCode::Closed, PluginError::Closed.to_string());
    };
    let Some(method) = HostMethod::parse(method) else {
        return refused(CallCode::Invalid, format!("AutoScene has no method {}.", runtime::sanitize(method, 40)));
    };
    if let Err(e) = calls::check_size(method, params.len()) {
        return PageReply::Err { error: e.into() };
    }
    let Ok(params) = serde_json::from_str::<&RawValue>(params) else {
        return refused(CallCode::Invalid, format!("The params of {} aren't JSON.", method.name()));
    };
    let Some(_waiting) = Waiting::start(&m) else {
        return refused(CallCode::Busy, format!("This page has {MAX_PAGE_CALLS} calls waiting already."));
    };
    let calls = AppCalls::page(places, host.clone(), m.clone());
    match calls.page_answer(&m, method, params, io, now) {
        Ok(PageAnswer { result, effect: None }) => PageReply::Ok { result },
        Ok(PageAnswer { result, effect: Some(effect) }) => PageReply::Effect { effect, result },
        Err(error) => PageReply::Err { error },
    }
}

/// A call of a page waiting for its answer, counted while it lives.
struct Waiting<'a>(&'a Mount);

impl<'a> Waiting<'a> {
    fn start(m: &'a Mount) -> Option<Self> {
        let before = m.inflight.fetch_add(1, Ordering::SeqCst);
        let waiting = Self(m);
        (before < MAX_PAGE_CALLS).then_some(waiting)
    }
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.0.inflight.fetch_sub(1, Ordering::SeqCst);
    }
}

/// How often each open page did what it may do only so often: a toast (one
/// in 2 s, and none while two of its last minute's may still be on screen,
/// which shows three: AutoScene's own keep their place), a copy to the
/// clipboard (ten a minute), a dialog of the system's (one at a time, only
/// while AutoScene is in front, and none for [`DECLINED_GAP`] after the
/// person said no to one, so they can reach the page's Close between them),
/// and a read of the keychain (one at a time, and none after one failed).
/// Forgotten as the page closes.
#[derive(Default)]
pub struct PageLimits(Mutex<HashMap<String, Used>>);

#[derive(Default)]
struct Used {
    toasts: VecDeque<Instant>,
    copies: VecDeque<Instant>,
    /// One of its dialogs is open.
    dialog: bool,
    /// When the person last said no to one of its dialogs.
    declined: Option<Instant>,
    /// Its turn at the keychain, and why its read failed, once one did.
    keychain: Arc<Mutex<Option<CallError>>>,
}

/// How long a page's toast counts, how many count at once, and how long
/// after one the next may come.
const TOAST_WINDOW: Duration = Duration::from_secs(60);
const TOASTS_AT_ONCE: usize = 2;
const TOAST_GAP: Duration = Duration::from_secs(2);
/// How many copies a page makes in a minute.
const COPIES_A_MINUTE: usize = 10;
/// How long after the person said no to a page's dialog it may open another.
const DECLINED_GAP: Duration = Duration::from_secs(3);

impl PageLimits {
    /// Whether the page `session` may show a toast at `now`; one it may is
    /// counted.
    fn toast(&self, session: &str, now: Instant) -> bool {
        let mut pages = self.0.lock();
        let toasts = &mut pages.entry(session.to_owned()).or_default().toasts;
        within(toasts, now, TOAST_WINDOW);
        let soon = toasts.back().is_some_and(|&last| now.saturating_duration_since(last) < TOAST_GAP);
        if soon || toasts.len() >= TOASTS_AT_ONCE {
            return false;
        }
        toasts.push_back(now);
        true
    }

    /// Whether the page `session` may copy at `now`; one it may is counted.
    fn copy(&self, session: &str, now: Instant) -> bool {
        let mut pages = self.0.lock();
        let copies = &mut pages.entry(session.to_owned()).or_default().copies;
        within(copies, now, Duration::from_secs(60));
        if copies.len() >= COPIES_A_MINUTE {
            return false;
        }
        copies.push_back(now);
        true
    }

    /// The dialog the page `m` opens at `now`, while AutoScene is in front
    /// (`io` says), none of its others is open and the person didn't just
    /// say no to one. It counts as open while it lives.
    fn dialog<'a>(&'a self, m: &'a Mount, io: &dyn PageIo, now: Instant) -> Result<Dialog<'a>, CallError> {
        if !io.in_front() {
            return Err(denied("A page opens a dialog only while AutoScene is in front."));
        }
        let mut pages = self.0.lock();
        let used = pages.entry(m.token.clone()).or_default();
        if used.dialog {
            return Err(CallError::new(CallCode::Busy, "This page has a dialog open already."));
        }
        if used.declined.is_some_and(|at| now.saturating_duration_since(at) < DECLINED_GAP) {
            return Err(CallError::new(CallCode::Busy, "A page asks again only 3 seconds after the person says no."));
        }
        used.dialog = true;
        Ok(Dialog { limits: self, m, now, opened: Instant::now() })
    }

    /// The page `session`'s turn at the keychain: held while it reads, it
    /// keeps why a read failed.
    fn keychain(&self, session: &str) -> Arc<Mutex<Option<CallError>>> {
        self.0.lock().entry(session.to_owned()).or_default().keychain.clone()
    }

    /// Forgets what the page `session` did: it closed.
    pub fn forget(&self, session: &str) {
        self.0.lock().remove(session);
    }
}

/// A dialog of a page's, open while it lives ([`PageLimits::dialog`]).
struct Dialog<'a> {
    limits: &'a PageLimits,
    m: &'a Mount,
    /// When its call came, and when it opened: it closes at `now` and the
    /// time it was open.
    now: Instant,
    opened: Instant,
}

impl Dialog<'_> {
    /// The person closed it, with a yes (a file chosen, the link to open)
    /// or not. A page that closed meanwhile does nothing with that.
    fn closed(self, yes: bool) -> Result<(), CallError> {
        if !yes && let Some(used) = self.limits.0.lock().get_mut(&self.m.token) {
            used.declined = Some(self.now + self.opened.elapsed());
        }
        match self.m.closed.is_cancelled() {
            true => Err(PluginError::Closed.into()),
            false => Ok(()),
        }
    }
}

impl Drop for Dialog<'_> {
    fn drop(&mut self) {
        if let Some(used) = self.limits.0.lock().get_mut(&self.m.token) {
            used.dialog = false;
        }
    }
}

/// Keeps of `times` those less than `window` before `now`.
fn within(times: &mut VecDeque<Instant>, now: Instant, window: Duration) {
    while times.front().is_some_and(|&t| now.saturating_duration_since(t) >= window) {
        times.pop_front();
    }
}

/// A `net.fetch` call of a plugin waiting for its answer, counted while it
/// lives: a plugin has at most [`net::MAX_IN_FLIGHT`] at once.
struct Fetching<'a> {
    host: &'a PluginHost,
    plugin: &'a PluginId,
}

impl<'a> Fetching<'a> {
    fn start(host: &'a PluginHost, plugin: &'a PluginId) -> Result<Self, CallError> {
        let mut fetching = host.fetching.lock();
        let calls = fetching.entry(plugin.clone()).or_default();
        if *calls >= net::MAX_IN_FLIGHT {
            return Err(CallError::new(CallCode::Busy, "Too many net.fetch calls at once."));
        }
        *calls += 1;
        Ok(Self { host, plugin })
    }
}

impl Drop for Fetching<'_> {
    fn drop(&mut self) {
        let mut fetching = self.host.fetching.lock();
        if let Some(calls) = fetching.get_mut(self.plugin) {
            *calls -= 1;
            if *calls == 0 {
                fetching.remove(self.plugin);
            }
        }
    }
}

/// `e`, the app's, as a page's call gets it.
fn app_error(e: AppError) -> CallError {
    let code = match e.code {
        ErrorCode::NotFound => CallCode::NotFound,
        ErrorCode::Invalid => CallCode::Invalid,
        ErrorCode::NotReady => CallCode::Busy,
        ErrorCode::Cancelled => CallCode::Cancelled,
        ErrorCode::Media | ErrorCode::Io | ErrorCode::Update | ErrorCode::Internal => CallCode::Failed,
    };
    CallError::new(code, e.message)
}

fn not_found(message: impl Into<String>) -> CallError {
    CallError::new(CallCode::NotFound, message)
}

fn invalid(message: impl Into<String>) -> CallError {
    CallError::new(CallCode::Invalid, message)
}

fn denied(message: impl Into<String>) -> CallError {
    CallError::new(CallCode::Denied, message)
}

/// What AutoScene couldn't do for a page, said as `message`; why (`detail`)
/// goes to the app's log.
fn failed(message: &str, detail: &str) -> CallError {
    log::warn!("a plugin page's call: {detail}");
    CallError::new(CallCode::Failed, message)
}

/// `e` as a call gets it. What a plugin can't help (the database, the
/// disk) goes to the app's log too.
fn project_error(e: ProjectError) -> CallError {
    let code = match &e {
        ProjectError::NotFound(_) => CallCode::NotFound,
        ProjectError::Invalid(_) => CallCode::Invalid,
        ProjectError::NotReady(_) => CallCode::Busy,
        ProjectError::Cancelled => CallCode::Cancelled,
        ProjectError::Media(_) | ProjectError::Export(_) | ProjectError::Io(_) | ProjectError::Db(_) => {
            log::warn!("a plugin's call: {}", e.detail());
            CallCode::Failed
        }
    };
    CallError::new(code, e.to_string())
}

/// `value` as a call's result.
fn answer(value: impl Serialize) -> Value {
    serde_json::to_value(value).expect("a call's answer is plain data")
}

/// `project` as a run gets it: with its folder and its episodes' files. An
/// episode is named by its number, else by its file.
fn program_project(project: &Project) -> ProjectResult<ProgramProject> {
    let episodes = project.media_list_uncounted()?.into_iter().map(|m| ProgramEpisode {
        id: m.id.to_string(),
        name: if m.label.is_empty() { m.file_name.clone() } else { m.label.clone() },
        path: m.path,
        frames: m.frames,
        fps: m.fps.map(|fps| fps.to_string()),
        missing: m.status == MediaStatus::Missing,
    });
    Ok(ProgramProject {
        id: project.id.to_string(),
        title: project.title()?,
        folder: project.dir.to_string_lossy().into_owned(),
        episodes: episodes.collect(),
    })
}

/// `project` as a page gets it: its episodes, without their files.
fn page_project(project: &Project) -> ProjectResult<PageProject> {
    let episodes = project.media_list_uncounted()?.into_iter().map(|m| PageEpisode {
        id: m.id.to_string(),
        name: if m.label.is_empty() { m.file_name.clone() } else { m.label.clone() },
        frames: m.frames,
        fps: m.fps.map(|fps| fps.to_string()),
        missing: m.status == MediaStatus::Missing,
    });
    Ok(PageProject { id: project.id.to_string(), title: project.title()?, episodes: episodes.collect() })
}

/// An export as a page gets it (from what a run gets): each clip by its
/// place, without its file or its episode's.
fn page_export(export: as_plugin::protocol::ExportContext) -> PageExport {
    let clips = export.clips.into_iter().enumerate().map(|(index, c)| PageClip {
        index: index as u32,
        name: c.name,
        status: c.status,
        media_id: c.media_id,
        episode: c.episode,
        // An export's frames are u32 (`ExportItem`).
        start: c.start as u32,
        end: c.end as u32,
        fps: c.fps,
    });
    PageExport {
        id: export.id,
        label: export.label,
        preset: export.preset,
        created_at: export.created_at as f64,
        merged: export.merged_file.is_some(),
        clips: clips.collect(),
    }
}

/// `export` as `exports.list` lists it.
fn export_summary(export: &ExportSummary) -> PageExportSummary {
    let done = export.items.iter().filter(|i| i.status == ExportStatus::Done).count();
    PageExportSummary {
        id: export.id.to_string(),
        label: export.label.clone(),
        preset: export.preset.id.clone(),
        created_at: export.created_at as f64,
        merged: export.merged.is_some(),
        clips: export.items.len() as u32,
        done: done as u32,
        status: serde_name(&export.status),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;
    use std::sync::{Barrier, mpsc};

    use as_api::{ContentKind, ExportItem, JobInfo, JobKind, MergedFile, NewProject, ProjectKind};
    use as_jobs::{Class, JobCtx, JobError, JobRunner};
    use as_plugin::net::{Incoming, Outgoing, Transport};
    use as_plugin::{ActionContext, Permission, RunMessage, RuntimeKind, Trigger, Trust};

    use super::super::tests::{folder, host_in, host_keeping, host_reaching, key, page_only, plugin};
    use super::super::{MAX_PAGE_RUNS, RunKey, add_context, calls_for, startable};
    use super::*;

    /// A call of a run still going, and one of a run `ended` tells about.
    trait Ask {
        fn ask(&self, method: HostMethod, params: Value) -> Result<Value, CallError>;
        fn ask_ended(&self, method: HostMethod, params: Value, ended: &CancelToken) -> Result<Value, CallError>;
    }

    impl Ask for AppCalls {
        fn ask(&self, method: HostMethod, params: Value) -> Result<Value, CallError> {
            self.ask_ended(method, params, &CancelToken::new())
        }

        /// With `params` as a plugin writes them.
        fn ask_ended(&self, method: HostMethod, params: Value, ended: &CancelToken) -> Result<Value, CallError> {
            self.call(method, &serde_json::value::to_raw_value(&params).unwrap(), ended)
        }
    }

    /// The library, with exports of its own.
    struct Stand {
        library: Library,
        exports: Vec<ExportSummary>,
    }

    impl Places for Stand {
        fn project(&self, id: ProjectId) -> ProjectResult<Arc<Project>> {
            self.library.project(id)
        }

        fn exports(&self, _: &Project) -> ProjectResult<Vec<ExportSummary>> {
            Ok(self.exports.clone())
        }

        fn export(&self, _: &Project, id: ExportId) -> ProjectResult<ExportSummary> {
            let e = self.exports.iter().find(|e| e.id == id);
            e.cloned().ok_or_else(|| ProjectError::NotFound(as_core::Msg::plain("This export no longer exists.")))
        }
    }

    /// A project with two episodes (one of them gone from its folder), and
    /// two exports of it in `tmp`: one with a clip written and one not yet,
    /// and one joined into a file.
    struct Place {
        stand: Arc<Stand>,
        host: Arc<PluginHost>,
        project: ProjectId,
        episodes: [MediaId; 2],
        exports: [ExportId; 2],
        files: [String; 3],
    }

    fn place(tmp: &Path) -> Place {
        place_with(tmp, host_in(tmp))
    }

    /// [`place`], of `library` and `host`.
    fn place_with(tmp: &Path, (library, host): (Library, Arc<PluginHost>)) -> Place {
        let file = |name: &str| {
            let file = tmp.join("files").join(name);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, b"clip").unwrap();
            file.display().to_string()
        };
        let files = ["Show S01E02.mkv", "Gone.mkv", "One.mp4"].map(file);
        let req = NewProject {
            title: "Show".into(),
            kind: ProjectKind::Series,
            content: ContentKind::Live,
            files: files[..2].to_vec(),
        };
        let (summary, project, _) = library.create_project(&req).unwrap();
        std::fs::remove_file(&files[1]).unwrap();
        let listed = project.media_list_uncounted().unwrap();
        let episodes = ["Show S01E02.mkv", "Gone.mkv"].map(|n| listed.iter().find(|m| m.file_name == n).unwrap().id);
        let clip = |name: &str, path: Option<&str>| ExportItem {
            id: 0,
            media_id: episodes[0],
            start: 10,
            end: 20,
            name: name.into(),
            status: if path.is_some() { ExportStatus::Done } else { ExportStatus::Queued },
            path: path.map(str::to_owned),
            verified: true,
            encoder: None,
            bytes: None,
            error: None,
        };
        let export = |items, merged| ExportSummary {
            id: ExportId::new(),
            project_id: summary.id,
            created_at: 7,
            preset: as_export::builtin_presets()[0].clone(),
            out_dir: tmp.join("files").display().to_string(),
            status: ExportStatus::Running,
            items,
            label: Some("Walter · scenepack".into()),
            merged,
        };
        let merged =
            MergedFile { name: "One".into(), path: Some(files[2].clone()), verified: true, bytes: None, error: None };
        let exports = vec![
            export(vec![clip("One", Some(&files[2])), clip("Two", None)], None),
            export(vec![clip("Three", None)], Some(merged)),
        ];
        let ids = [exports[0].id, exports[1].id];
        drop(project);
        Place { stand: Arc::new(Stand { library, exports }), host, project: summary.id, episodes, exports: ids, files }
    }

    impl Place {
        /// The calls of a run on `export` (a command's, without one), of a
        /// plugin granted `permissions`.
        fn run(&self, export: Option<ExportId>, permissions: &[Permission]) -> AppCalls {
            AppCalls {
                places: self.stand.clone(),
                host: self.host.clone(),
                plugin: PluginId::parse("dev.a.calls").unwrap(),
                grants: Grants {
                    runtime: RuntimeKind::Process,
                    permissions: permissions.to_vec(),
                    hosts: Vec::new(),
                    secrets: Vec::new(),
                    protocol: 2,
                },
                project: Some(self.project.to_string()),
                export: export.map(|e| e.to_string()),
                page: None,
                service: false,
            }
        }
    }

    fn said(r: Result<Value, CallError>) -> (CallCode, String) {
        let e = r.unwrap_err();
        (e.code, e.message)
    }

    #[test]
    fn a_run_gets_its_export_without_another_one() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-calls-export-{}", std::process::id()));
        let place = place(&tmp);
        let [mine, other] = place.exports;
        let run = place.run(Some(mine), &[Permission::ExportsRead]);
        let get = |calls: &AppCalls, export: ExportId| {
            calls.ask(HostMethod::ExportsGet, json!({"exportId": export.to_string()}))
        };

        let got = get(&run, mine).unwrap();
        assert_eq!((got["id"].clone(), got["label"].clone()), (json!(mine.to_string()), json!("Walter · scenepack")));
        let clips = got["clips"].as_array().unwrap();
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[0]["path"], json!(place.files[2]));
        assert_eq!(
            (clips[0]["episode"].clone(), clips[0]["sourcePath"].clone()),
            (json!("S01E02"), json!(place.files[0]))
        );
        // Another export of its project, or its own named as another project's: no.
        assert_eq!(said(get(&run, other)), (CallCode::Denied, "A run reads only the export it runs on.".into()));
        let elsewhere = json!({"projectId": ProjectId::new().to_string(), "exportId": mine.to_string()});
        assert_eq!(
            said(run.ask(HostMethod::ExportsGet, elsewhere)),
            (CallCode::Denied, "A run reads only the project it runs in.".into())
        );
        // A command in the project reads any of its exports, and lists them.
        let command = place.run(None, &[Permission::ExportsRead]);
        assert_eq!(get(&command, other).unwrap()["id"], json!(other.to_string()));
        assert_eq!(said(get(&command, ExportId::new())), (CallCode::NotFound, "This export no longer exists.".into()));
        let listed = run.ask(HostMethod::ExportsList, Value::Null).unwrap();
        assert_eq!(
            listed[0],
            json!({
                "id": mine.to_string(), "label": "Walter · scenepack", "preset": as_export::builtin_presets()[0].id,
                "createdAt": 7.0, "merged": false, "clips": 2, "done": 1, "status": "running"
            })
        );
        assert_eq!((listed[1]["merged"].clone(), listed[1]["done"].clone()), (json!(true), json!(0)));
        // Without exports:read it reads none; the project needs project:read.
        let blind = place.run(Some(mine), &[Permission::ProjectRead]);
        assert_eq!(
            said(get(&blind, mine)),
            (CallCode::Denied, "exports.get needs the exports:read permission.".into())
        );
        assert_eq!(said(run.ask(HostMethod::ProjectGet, json!({}))).0, CallCode::Denied);
        let project = blind.ask(HostMethod::ProjectGet, json!({})).unwrap();
        assert_eq!((project["title"].clone(), project["episodes"].as_array().unwrap().len()), (json!("Show"), 2));
        let gone = project["episodes"].as_array().unwrap().iter().find(|e| e["name"] == "Gone.mkv").unwrap();
        assert_eq!(gone["missing"], json!(true));
        // Params past their cap are refused before anything is read.
        let padded = json!({"exportId": mine.to_string(), "pad": "x".repeat(1024)});
        assert_eq!(said(run.ask(HostMethod::ExportsGet, padded)).0, CallCode::Invalid);
        drop((run, command, blind, place));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn files_resolve_gives_a_run_the_clip_path() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-calls-files-{}", std::process::id()));
        let place = place(&tmp);
        let [mine, merged] = place.exports;
        let run = place.run(None, &[Permission::ExportsRead]);
        let resolve = |calls: &AppCalls, file: Value| calls.ask(HostMethod::FilesResolve, file);
        let clip = |index: u32| json!({"kind": "clip", "exportId": mine.to_string(), "index": index});

        assert_eq!(
            resolve(&run, clip(0)).unwrap(),
            json!({"url": null, "path": place.files[2], "mime": "video/mp4", "size": 4.0})
        );
        assert_eq!(said(resolve(&run, clip(1))), (CallCode::NotFound, "That clip isn't written yet.".into()));
        assert_eq!(said(resolve(&run, clip(5))), (CallCode::NotFound, "That export has no clip 5.".into()));
        let joined = json!({"kind": "merged", "exportId": merged.to_string()});
        assert_eq!(resolve(&run, joined).unwrap()["path"], json!(place.files[2]));
        let apart = json!({"kind": "merged", "exportId": mine.to_string()});
        assert_eq!(said(resolve(&run, apart)), (CallCode::NotFound, "That export isn't joined into one file.".into()));
        // A run on the other export resolves none of this one's clips.
        let on_merged = place.run(Some(merged), &[Permission::ExportsRead]);
        assert_eq!(said(resolve(&on_merged, clip(0))).0, CallCode::Denied);

        // An episode needs project:read, and its file where it was.
        let episode = |media: MediaId| json!({"kind": "episode", "mediaId": media.to_string()});
        assert_eq!(
            said(resolve(&run, episode(place.episodes[0]))),
            (CallCode::Denied, "files.resolve needs the project:read permission for that file.".into())
        );
        let reader = place.run(None, &[Permission::ProjectRead]);
        let found = resolve(&reader, episode(place.episodes[0])).unwrap();
        assert_eq!((found["path"].clone(), found["mime"].clone()), (json!(place.files[0]), json!("video/x-matroska")));
        assert_eq!(
            said(resolve(&reader, episode(place.episodes[1]))),
            (CallCode::NotFound, "That file isn't where it was.".into())
        );
        assert_eq!(said(resolve(&reader, clip(0))).0, CallCode::Denied);
        drop((run, on_merged, reader, place));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_run_keeps_its_state_and_reads_its_settings() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-calls-state-{}", std::process::id()));
        let place = place(&tmp);
        let run = place.run(None, &[]);
        let call = |method, params| run.ask(method, params);
        assert_eq!(call(HostMethod::StateGet, json!({"key": "seen"})).unwrap(), json!({"value": null}));
        // In the data folder its run made.
        let data = place.host.store.data_dir(&run.plugin);
        std::fs::create_dir_all(&data).unwrap();
        assert_eq!(call(HostMethod::StateSet, json!({"key": "seen", "value": [1, 2]})).unwrap(), Value::Null);
        assert_eq!(call(HostMethod::StateGet, json!({"key": "seen"})).unwrap(), json!({"value": [1, 2]}));
        assert_eq!(call(HostMethod::StateKeys, Value::Null).unwrap(), json!(["seen"]));
        assert_eq!(call(HostMethod::StateDelete, json!({"key": "seen"})).unwrap(), Value::Null);
        assert_eq!(said(call(HostMethod::StateGet, json!({"key": "no way"}))).0, CallCode::Invalid);
        assert!(data.join(page_state::FILE).is_file());
        // Once its run was cancelled or its program ended, a call reads what
        // it kept and writes nothing.
        let ended = CancelToken::new();
        ended.cancel();
        let over = (CallCode::Cancelled, "This run has ended.".to_owned());
        assert_eq!(said(run.ask_ended(HostMethod::StateSet, json!({"key": "late", "value": 1}), &ended)), over);
        assert_eq!(said(run.ask_ended(HostMethod::StateDelete, json!({"key": "seen"}), &ended)), over);
        assert_eq!(run.ask_ended(HostMethod::StateKeys, Value::Null, &ended).unwrap(), json!([]));
        // Nor does a call bring back a data folder deleted with the plugin's data.
        std::fs::remove_dir_all(&data).unwrap();
        assert_eq!(said(call(HostMethod::StateSet, json!({"key": "seen", "value": 1}))).0, CallCode::Failed);
        assert!(!data.exists());
        // A plugin that isn't installed has no settings.
        assert_eq!(said(call(HostMethod::SettingsGet, Value::Null)).0, CallCode::NotFound);
        // A plugin without secret settings has none to read.
        assert_eq!(
            said(call(HostMethod::SecretsGet, json!({"key": "token"}))),
            (CallCode::Denied, "secrets.get needs a secret setting, and this plugin has none.".into())
        );
        // A page's own methods aren't a run's.
        assert_eq!(
            said(call(HostMethod::UiToast, json!({"text": "Hi"}))),
            (CallCode::Denied, "A run can't call ui.toast.".into())
        );
        drop((run, place));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_run_reads_only_its_own_secrets() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-calls-secrets-{}", std::process::id()));
        let (library, host) = host_in(&tmp);
        let library = Arc::new(library);
        let manifest = json!({
            "manifestVersion": 2,
            "id": "dev.a.secrets",
            "version": "1.0.0",
            "name": "Poster",
            "description": "Posts clips.",
            "author": {"name": "Someone"},
            "engines": {"autoscene": ">=1.0.0", "protocol": 2},
            "runtime": {"type": "process", "command": {"*": ["uv", "run", "--script", "--locked", "main.py"]}},
            "contributes": {
                "commands": [{"id": "post", "title": "Post"}],
                "settings": [
                    {"key": "token", "type": "secret", "title": "Token"},
                    {"key": "webhook", "type": "secret", "title": "Webhook"},
                    {"key": "channel", "type": "string", "title": "Channel", "default": "clips"}
                ]
            }
        });
        let p = host.store.commit(folder(&host, &tmp, &manifest), true, true).unwrap();
        let id = p.record.id.clone();
        host.store.secret_set(&id, "token", "hunter2").unwrap();
        let post = key(&id, ActionKind::Command, "post", None, None);
        let run = calls_for(&library, &host, &p, &post, 2).unwrap().unwrap();
        let get = |key: &str| run.ask(HostMethod::SecretsGet, json!({ "key": key }));

        assert_eq!(get("token").unwrap(), json!({"value": "hunter2"}));
        // One the person hasn't set is none; it is read at each call.
        assert_eq!(get("webhook").unwrap(), json!({"value": null}));
        host.store.secret_set(&id, "webhook", "https://example.org/hook").unwrap();
        assert_eq!(get("webhook").unwrap(), json!({"value": "https://example.org/hook"}));
        // A plain setting is no secret, and settings.get gives none.
        assert_eq!(
            said(get("channel")),
            (CallCode::Denied, "secrets.get reads only the plugin's secret settings, and channel isn't one.".into())
        );
        assert_eq!(run.ask(HostMethod::SettingsGet, Value::Null).unwrap(), json!({"channel": "clips"}));
        // Removed, it is none again.
        host.store.secret_clear(&id, "token").unwrap();
        assert_eq!(get("token").unwrap(), json!({"value": null}));
        drop((run, host, library));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    /// The internet as a test sees it: each request is kept, and gets a
    /// little JSON. One to `/moved` ends `run` and is redirected; one to
    /// `/silent` waits until its call ends, as a host that doesn't answer.
    #[derive(Clone, Default)]
    struct Answering {
        sent: Arc<Mutex<Vec<Outgoing>>>,
        run: CancelToken,
    }

    impl Transport for Answering {
        fn send(&self, o: &Outgoing, ended: &CancelToken) -> PluginResult<Incoming> {
            self.sent.lock().push(o.clone());
            if o.url.ends_with("/silent") {
                let until = Instant::now() + Duration::from_secs(10);
                while !ended.is_cancelled() && Instant::now() < until {
                    std::thread::sleep(Duration::from_millis(1));
                }
                return Err(PluginError::Failed { message: "Couldn't reach discord.com.".into(), detail: None });
            }
            let (status, header) = match o.url.ends_with("/moved") {
                true => {
                    self.run.cancel();
                    (302, ("location".to_owned(), "https://discord.com/here".to_owned()))
                }
                false => (200, ("content-type".to_owned(), "application/json".to_owned())),
            };
            Ok(Incoming { status, headers: vec![header], body: Box::new(&br#"{"id":"1"}"#[..]) })
        }
    }

    #[test]
    fn a_run_fetches_from_its_hosts_a_few_at_a_time() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-calls-net-{}", std::process::id()));
        let internet = Answering::default();
        let (library, host) = host_reaching(&tmp, internet.clone());
        let places: Arc<dyn Places> = Arc::new(library);
        let plugin = PluginId::parse("dev.a.calls").unwrap();
        let run = |permissions: Vec<Permission>| AppCalls {
            places: places.clone(),
            host: host.clone(),
            plugin: plugin.clone(),
            grants: Grants {
                runtime: RuntimeKind::Process,
                permissions,
                hosts: vec!["discord.com".into()],
                secrets: Vec::new(),
                protocol: 2,
            },
            project: None,
            export: None,
            page: None,
            service: false,
        };
        let online = run(vec![Permission::Network]);
        let post = json!({
            "url": "https://discord.com/api/webhooks/1/x",
            "method": "POST",
            "headers": [["content-type", "application/json"]],
            "body": "{}"
        });
        let fetch = || online.ask(HostMethod::NetFetch, post.clone());

        let got = fetch().unwrap();
        assert_eq!((got["status"].clone(), got["body"].clone()), (json!(200), json!(r#"{"id":"1"}"#)));
        // Sent as it asked, saying which plugin asks.
        let sent = internet.sent.lock()[0].clone();
        assert_eq!(
            (sent.method, sent.url.as_str(), sent.body.as_slice()),
            ("POST", "https://discord.com/api/webhooks/1/x", &b"{}"[..])
        );
        let agent = ("user-agent".to_owned(), "AutoScene/1.0.3-BETA (plugin dev.a.calls)".to_owned());
        assert_eq!(sent.headers, [("content-type".to_owned(), "application/json".to_owned()), agent]);
        // Only to the hosts it names, and only with the network permission.
        let elsewhere = online.ask(HostMethod::NetFetch, json!({"url": "https://evil.example.org/"}));
        assert_eq!(said(elsewhere).0, CallCode::Denied);
        assert_eq!(
            said(run(Vec::new()).ask(HostMethod::NetFetch, post.clone())),
            (CallCode::Denied, "net.fetch needs the network permission.".into())
        );
        assert!(host.fetching.lock().is_empty());
        // Eight wait at most; each counts until it has its answer.
        host.fetching.lock().insert(plugin.clone(), net::MAX_IN_FLIGHT);
        assert_eq!(said(fetch()), (CallCode::Busy, "Too many net.fetch calls at once.".into()));
        host.fetching.lock().insert(plugin.clone(), net::MAX_IN_FLIGHT - 1);
        fetch().unwrap();
        assert_eq!(host.fetching.lock().get(&plugin), Some(&(net::MAX_IN_FLIGHT - 1)));
        host.fetching.lock().clear();
        fetch().unwrap();
        assert!(host.fetching.lock().is_empty());
        // Once its program ended, a run sends nothing.
        let over = (CallCode::Cancelled, "This run has ended.".to_owned());
        let ended = CancelToken::new();
        ended.cancel();
        assert_eq!(said(online.ask_ended(HostMethod::NetFetch, post.clone(), &ended)), over);
        assert_eq!(internet.sent.lock().len(), 3);
        // Nor where a request it sent before was redirected.
        let moved = json!({"url": "https://discord.com/moved"});
        assert_eq!(said(online.ask_ended(HostMethod::NetFetch, moved, &internet.run)), over);
        let urls: Vec<String> = internet.sent.lock().iter().map(|o| o.url.clone()).collect();
        assert_eq!(urls.last().map(String::as_str), Some("https://discord.com/moved"));
        assert_eq!(urls.len(), 4);
        // One still waiting stops as the run ends, said the same, and no
        // longer counts.
        let ended = CancelToken::new();
        let ending = ended.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            ending.cancel();
        });
        let started = Instant::now();
        let silent = json!({"url": "https://discord.com/silent"});
        assert_eq!(said(online.ask_ended(HostMethod::NetFetch, silent, &ended)), over);
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(host.fetching.lock().is_empty());
        drop((online, places, host));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn only_a_run_of_protocol_2_gets_its_calls_answered_where_it_runs() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-calls-for-{}", std::process::id()));
        let (library, host) = host_in(&tmp);
        let library = Arc::new(library);
        // Agreed to both; its plugin.json asks for exports:read only now.
        let mut p = plugin("dev.a.calls", Trust::Reviewed, false);
        p.record.permissions = vec![Permission::ExportsRead, Permission::ProjectRead];
        let m = p.manifest.as_mut().unwrap();
        (m.permissions, m.engines.protocol) = (vec![Permission::ExportsRead], 2);
        let (project, export) = (ProjectId::new(), ExportId::new());
        let on_export: RunKey = key(&p.record.id, ActionKind::ExportAction, "post", Some(project), Some(export));

        let run = calls_for(&library, &host, &p, &on_export, 2).unwrap().unwrap();
        assert_eq!(run.plugin, p.record.id);
        assert_eq!((&run.project, &run.export), (&Some(project.to_string()), &Some(export.to_string())));
        assert_eq!(run.grants.permissions, [Permission::ExportsRead]);
        // Bound there: another project's export is refused before anything is read.
        let other = json!({"exportId": ExportId::new().to_string(), "projectId": ProjectId::new().to_string()});
        assert_eq!(said(run.ask(HostMethod::ExportsGet, other)).0, CallCode::Denied);
        let command = key(&p.record.id, ActionKind::Command, "hello", None, None);
        let command = calls_for(&library, &host, &p, &command, 2).unwrap().unwrap();
        assert_eq!((&command.project, &command.export), (&None, &None));
        std::fs::create_dir_all(host.store.data_dir(&p.record.id)).unwrap();
        assert_eq!(command.ask(HostMethod::StateSet, json!({"key": "k", "value": 1})).unwrap(), Value::Null);

        // A plugin of protocol 1 makes no calls; a broken one doesn't run.
        assert!(calls_for(&library, &host, &p, &on_export, 1).unwrap().is_none());
        p.manifest = None;
        p.broken = Some("Its files are missing. Install it again.".into());
        assert!(calls_for(&library, &host, &p, &on_export, 2).is_err());
        drop((run, command, host, library));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    /// What the person does when a page asks: the clipboard they copy to,
    /// what a dialog asks them and their answer, the links they open, and
    /// where they save a file or which they pick (none: they cancel). With
    /// `held`, a dialog waits there as it opens, and again for its answer.
    #[derive(Default)]
    struct Person {
        copied: Mutex<Vec<String>>,
        asked: Mutex<Vec<(String, String, String)>>,
        yes: AtomicBool,
        opened: Mutex<Vec<String>>,
        save_at: Mutex<Option<PathBuf>>,
        pick: Mutex<Option<PathBuf>>,
        dialogs: Mutex<Vec<String>>,
        held: Mutex<Option<Arc<Barrier>>>,
        /// They work in another app: AutoScene isn't in front.
        away: AtomicBool,
    }

    impl Person {
        /// A dialog stays open until the test lets it close.
        fn hold(&self) {
            let held = self.held.lock().clone();
            if let Some(held) = held {
                held.wait();
                held.wait();
            }
        }
    }

    impl PageIo for Person {
        fn in_front(&self) -> bool {
            !self.away.load(Ordering::SeqCst)
        }

        fn copy_text(&self, text: &str) -> Result<(), String> {
            self.copied.lock().push(text.to_owned());
            Ok(())
        }

        fn confirm(&self, title: &str, text: &str, yes: &str) -> bool {
            self.asked.lock().push((title.to_owned(), text.to_owned(), yes.to_owned()));
            self.hold();
            self.yes.load(Ordering::SeqCst)
        }

        fn open_url(&self, url: &str) -> Result<(), String> {
            self.opened.lock().push(url.to_owned());
            Ok(())
        }

        fn save_to(&self, title: &str, name: &str, _: &[FileFilter]) -> Option<PathBuf> {
            self.dialogs.lock().push(format!("{title}: {name}"));
            self.save_at.lock().clone()
        }

        fn pick(&self, title: &str, _: &[FileFilter]) -> Option<PathBuf> {
            self.dialogs.lock().push(title.to_owned());
            self.pick.lock().clone()
        }

        fn queue_run(
            &self,
            _: &PluginId,
            _: ActionKind,
            _: &str,
            _: Option<ProjectId>,
            _: Option<ExportId>,
            _: Option<Value>,
        ) -> AppResult<Option<JobId>> {
            unreachable!("its pages start no run")
        }
    }

    impl Place {
        /// Opens the page of a page-only plugin called `name`, granted
        /// `permissions`, in the project when `in_project`: its session.
        fn open_page(&self, tmp: &Path, name: &str, permissions: &[Permission], in_project: bool) -> String {
            let id = format!("dev.a.{}", name.to_ascii_lowercase());
            let dir = tmp.join(&id);
            std::fs::create_dir_all(dir.join("ui")).unwrap();
            std::fs::write(dir.join("ui/index.html"), b"<!doctype html>").unwrap();
            let mut manifest = page_only(&id, json!([{"id": "main", "title": "Main", "entry": "ui/index.html"}]));
            manifest["name"] = json!(name);
            std::fs::write(dir.join("plugin.json"), manifest.to_string()).unwrap();
            let p = self.host.store.commit(self.host.store.stage_folder(&dir).unwrap(), true, true).unwrap();
            let grants = Grants {
                runtime: RuntimeKind::None,
                permissions: permissions.to_vec(),
                hosts: Vec::new(),
                secrets: Vec::new(),
                protocol: 2,
            };
            let page = p.manifest.as_ref().unwrap().pages[0].clone();
            let project = in_project.then(|| self.project.to_string());
            self.host.pages.open(&p, grants, &page, project).unwrap().0.token.clone()
        }

        /// The page `session` calls `method` with `params` at `now`.
        fn page_asks(&self, io: &dyn PageIo, session: &str, method: &str, params: Value, now: Instant) -> PageReply {
            page_reply(self.stand.clone(), &self.host, io, session, method, &params.to_string(), now)
        }
    }

    /// A clock that moves on past [`DECLINED_GAP`] at each reading: a test's
    /// person may say no to each dialog its page opens.
    fn unhurried() -> impl Fn() -> Instant {
        let (start, readings) = (Instant::now(), std::cell::Cell::new(0));
        move || {
            readings.set(readings.get() + 1);
            start + (DECLINED_GAP + Duration::from_secs(1)) * readings.get()
        }
    }

    /// What a page's call got: its result (after its effect, if any).
    fn got(reply: PageReply) -> Value {
        match reply {
            PageReply::Ok { result } | PageReply::Effect { result, .. } => result,
            PageReply::Err { error } => panic!("{error:?}"),
        }
    }

    /// The error a page's call got.
    fn refused(reply: PageReply) -> (CallCode, String) {
        match reply {
            PageReply::Err { error } => (error.code, error.message),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_page_without_project_read_gets_denied() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-page-project-{}", std::process::id()));
        let place = place(&tmp);
        let person = Person::default();
        let blind = place.open_page(&tmp, "Blind", &[Permission::ExportsRead], true);
        let reader = place.open_page(&tmp, "Reader", &[Permission::ProjectRead], true);
        let outside = place.open_page(&tmp, "Outside", &[Permission::ProjectRead], false);
        let now = Instant::now();
        let ask = |session: &str, method: &str, params: Value| place.page_asks(&person, session, method, params, now);

        assert_eq!(
            refused(ask(&blind, "project.get", json!({}))),
            (CallCode::Denied, "project.get needs the project:read permission.".into())
        );
        let episode = json!({"kind": "episode", "mediaId": place.episodes[0].to_string()});
        assert_eq!(refused(ask(&blind, "files.resolve", episode)).0, CallCode::Denied);
        // With it, the project as a page sees it: no folder, no files.
        let project = got(ask(&reader, "project.get", json!({})));
        assert_eq!((project["title"].clone(), project.get("folder")), (json!("Show"), None));
        let episodes = project["episodes"].as_array().unwrap();
        assert_eq!(episodes.len(), 2);
        assert!(episodes.iter().all(|e| e.get("path").is_none()), "{episodes:?}");
        let gone = episodes.iter().find(|e| e["name"] == "Gone.mkv").unwrap();
        assert_eq!(gone["missing"], json!(true));
        // Only the project it is open in, and none outside one.
        let other = json!({"projectId": ProjectId::new().to_string()});
        assert_eq!(
            refused(ask(&reader, "project.get", other)),
            (CallCode::Denied, "A page reads only the project it is open in.".into())
        );
        assert_eq!(
            refused(ask(&outside, "project.get", json!({}))),
            (CallCode::Invalid, "This page isn't open in a project.".into())
        );

        // A page names only its session: an unknown one is closed, an
        // unknown method is no call, and past 16 at once a call waits.
        assert_eq!(refused(ask("0123456789abcdef0123456789abcdef", "project.get", json!({}))).0, CallCode::Closed);
        assert_eq!(
            refused(ask(&reader, "app.install", json!({}))),
            (CallCode::Invalid, "AutoScene has no method app.install.".into())
        );
        let m = place.host.pages.get(&reader).unwrap();
        m.inflight.store(MAX_PAGE_CALLS, Ordering::SeqCst);
        assert_eq!(refused(ask(&reader, "project.get", json!({}))).0, CallCode::Busy);
        assert_eq!(m.inflight.load(Ordering::SeqCst), MAX_PAGE_CALLS);
        m.inflight.store(0, Ordering::SeqCst);
        got(ask(&reader, "project.get", json!({})));
        assert_eq!(m.inflight.load(Ordering::SeqCst), 0);
        // What its plugin can't do, its page can't ask for.
        assert_eq!(
            refused(ask(&reader, "service.request", json!({"method": "count"}))),
            (CallCode::Denied, "service.request needs a plugin that runs a service.".into())
        );
        drop((m, place));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_page_gets_urls_not_paths() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-page-files-{}", std::process::id()));
        let place = place(&tmp);
        let person = Person::default();
        let page = place.open_page(&tmp, "Clips", &[Permission::ExportsRead, Permission::ProjectRead], true);
        let ask = |method: &str, params: Value| place.page_asks(&person, &page, method, params, Instant::now());
        let [mine, merged] = place.exports.map(|e| e.to_string());

        // A clip's address in its session, which serves the file.
        let clip = got(ask("files.resolve", json!({"kind": "clip", "exportId": mine, "index": 0})));
        let url = clip["url"].as_str().unwrap();
        assert!(url.starts_with(&format!("{}/{page}/~f/", pages::base_url())), "{url}");
        assert_eq!(
            (clip["path"].clone(), clip["mime"].clone(), clip["size"].clone()),
            (Value::Null, json!("video/mp4"), json!(4.0))
        );
        let served = place.host.pages.serve("GET", &url[pages::base_url().len()..], None);
        assert_eq!((served.status, served.body.as_slice()), (200, &b"clip"[..]));
        let joined = got(ask("files.resolve", json!({"kind": "merged", "exportId": merged})));
        assert!(joined["url"].as_str().is_some() && joined["path"].is_null(), "{joined}");
        // An episode in a file no page is served (an mkv) isn't given.
        let episode = json!({"kind": "episode", "mediaId": place.episodes[0].to_string()});
        assert_eq!(
            refused(ask("files.resolve", episode)),
            (CallCode::Denied, "A page can show only pictures, video and sound.".into())
        );

        // Its exports without the files of their clips or episodes.
        let export = got(ask("exports.get", json!({ "exportId": mine })));
        let clips = export["clips"].as_array().unwrap();
        assert_eq!(clips.len(), 2);
        for (i, c) in clips.iter().enumerate() {
            assert_eq!(c["index"], json!(i));
            assert!(c.get("path").is_none() && c.get("sourcePath").is_none(), "{c}");
        }
        assert_eq!((clips[0]["episode"].clone(), clips[0]["start"].clone()), (json!("S01E02"), json!(10)));
        assert_eq!((export["merged"].clone(), export.get("outDir")), (json!(false), None));
        assert_eq!(got(ask("exports.list", json!({}))).as_array().unwrap().len(), 2);
        drop(place);
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn toasts_past_one_in_two_seconds_are_dropped() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-page-toasts-{}", std::process::id()));
        let place = place(&tmp);
        let person = Person::default();
        let (page, other) = (place.open_page(&tmp, "Toaster", &[], false), place.open_page(&tmp, "Other", &[], false));
        let start = Instant::now();
        let toast = |session: &str, after: u64| {
            let at = start + Duration::from_secs(after);
            match place.page_asks(&person, session, "ui.toast", json!({"text": "Saved"}), at) {
                PageReply::Effect { result, .. } => (true, result),
                PageReply::Ok { result } => (false, result),
                PageReply::Err { error } => panic!("{error:?}"),
            }
        };
        let (shown, dropped) = ((true, json!({"shown": true})), (false, json!({"shown": false})));

        assert_eq!(toast(&page, 0), shown);
        assert_eq!(toast(&page, 1), dropped);
        assert_eq!(toast(&page, 2), shown);
        // Two of its last minute's may still be on screen: no third.
        assert_eq!(toast(&page, 5), dropped);
        assert_eq!(toast(&page, 59), dropped);
        // A minute after the first, one more; not two in 2 s.
        assert_eq!(toast(&page, 60), shown);
        assert_eq!(toast(&page, 61), dropped);
        // Each page counts its own.
        assert_eq!(toast(&other, 1), shown);
        // One that can't be shown isn't counted, nor drawn.
        let ask = |text: Value| place.page_asks(&person, &other, "ui.toast", text, start + Duration::from_secs(9));
        for wrong in [json!({"text": ""}), json!({"text": "\u{200B}"}), json!({"text": "x".repeat(201)})] {
            assert_eq!(refused(ask(wrong)), (CallCode::Invalid, "A toast says 1 to 200 characters.".into()));
        }
        assert_eq!(refused(ask(json!({"text": "Saved", "description": "x".repeat(301)}))).0, CallCode::Invalid);
        assert_eq!(toast(&other, 9), shown);
        // A page that closed is forgotten.
        place.host.close_page(&page);
        let late = place.page_asks(&person, &page, "ui.toast", json!({"text": "Hi"}), start);
        assert_eq!(refused(late).0, CallCode::Closed);
        assert!(!place.host.page_limits.0.lock().contains_key(&page));
        drop(place);
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_page_toast_always_starts_with_plugin_and_its_name() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-page-toast-name-{}", std::process::id()));
        let place = place(&tmp);
        let person = Person::default();
        let now = Instant::now();
        // A plugin may call itself AutoScene, and say what AutoScene would.
        let page = place.open_page(&tmp, "AutoScene", &[], false);
        let reply = place.page_asks(&person, &page, "ui.toast", json!({"text": "AutoScene: update now"}), now);
        assert_eq!(
            serde_json::to_value(&reply).unwrap(),
            json!({
                "kind": "effect",
                "effect": {
                    "type": "toast",
                    "title": "Plugin “AutoScene”: AutoScene: update now",
                    "description": null,
                    "tone": "neutral"
                },
                "result": {"shown": true}
            })
        );
        // Its words without what would turn them around or hide them.
        let other = place.open_page(&tmp, "Clips", &[], false);
        let error = json!({"text": "\u{202E}Couldn't save", "description": "Disk\u{0007} full", "tone": "error"});
        let PageReply::Effect { effect, .. } = place.page_asks(&person, &other, "ui.toast", error, now) else {
            panic!("no toast")
        };
        assert_eq!(
            effect,
            PageEffect::Toast {
                title: "Plugin “Clips”: Couldn't save".into(),
                description: Some("Disk full".into()),
                tone: ToastTone::Error
            }
        );
        drop(place);
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn navigate_keeps_the_page_inside_its_route() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-page-navigate-{}", std::process::id()));
        let place = place(&tmp);
        let person = Person::default();
        let page = place.open_page(&tmp, "Mover", &[], false);
        let go = |params: Value| place.page_asks(&person, &page, "ui.navigate", params, Instant::now());

        for inside in ["", "clips", "clips/3", "a/b/", "x:y@z~!&'()*+,;=%20-._", "%2e%2e/%2E%2e/a%2Fb"] {
            let PageReply::Effect { effect, result } = go(json!({ "path": inside })) else { panic!("{inside}") };
            assert_eq!((effect, result), (PageEffect::Navigate { path: inside.into(), replace: false }, Value::Null));
        }
        let replaced = go(json!({"path": "clips", "replace": true}));
        assert!(matches!(replaced, PageReply::Effect { effect: PageEffect::Navigate { replace: true, .. }, .. }));
        let long = "a".repeat(513);
        // Nor what the app's router reads as its own: `$` names a route's
        // param, and a `%` without its two hex digits is no address.
        let outside = ["/settings", "..", "a/../b", "./a", "a//b", "a?b", "a#b", "a\\b", "é", " a", long.as_str()];
        let routers = ["$plugin", "$pid/x", "$", "a$", "%", "%zz", "a%2", "%2/x", "50%"];
        for outside in outside.into_iter().chain(routers) {
            assert_eq!(
                refused(go(json!({ "path": outside }))),
                (CallCode::Invalid, "ui.navigate takes a path inside the page, like clips/3.".into()),
                "{outside}"
            );
        }
        drop(place);
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn open_external_takes_only_plain_https() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-page-link-{}", std::process::id()));
        let place = place(&tmp);
        let person = Person::default();
        let page = place.open_page(&tmp, "Linker", &[], false);
        let now = unhurried();
        let open = |url: &str| place.page_asks(&person, &page, "ui.openExternal", json!({ "url": url }), now());

        let long = format!("https://example.org/{}", "a".repeat(2000));
        for url in [
            "http://example.org/",
            "https://user@example.org/",
            "https://example.org@evil.example/",
            "https://example.org:0/",
            "javascript:alert(1)",
            "file:///C:/Windows",
            "https://example.org/a b",
            "https://exämple.org/",
            "asp://localhost/x",
            long.as_str(),
        ] {
            assert_eq!(
                refused(open(url)),
                (CallCode::Invalid, "ui.openExternal opens only plain https:// links.".into()),
                "{url}"
            );
        }
        assert!(person.asked.lock().is_empty());
        // The person says no, then yes, in a dialog the page can't answer.
        let url = "https://example.org/docs?x=1#y";
        assert_eq!(got(open(url)), json!({"opened": false}));
        assert!(person.opened.lock().is_empty());
        person.yes.store(true, Ordering::SeqCst);
        assert_eq!(got(open(url)), json!({"opened": true}));
        assert_eq!(*person.opened.lock(), [url]);
        let asked = (
            "Open this link in your browser?".to_owned(),
            format!("Plugin “Linker” asks to open:\n\n{url}"),
            "Open".to_owned(),
        );
        assert_eq!(*person.asked.lock(), [asked.clone(), asked]);
        drop(place);
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn params_past_their_cap_or_not_json_are_refused_unread() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-page-params-{}", std::process::id()));
        let place = place(&tmp);
        let page = place.open_page(&tmp, "Sender", &[], false);
        let ask = |method: &str, params: &str| {
            page_reply(place.stand.clone(), &place.host, &Person::default(), &page, method, params, Instant::now())
        };
        // A toast's params of 2 KB and one more byte: never read, whatever they hold.
        let large = format!("[{}0]", "0,".repeat(1023));
        assert_eq!(large.len(), 2049);
        assert_eq!(
            refused(ask("ui.toast", &large)),
            (CallCode::Invalid, "The params of ui.toast are larger than 2 KB.".into())
        );
        assert_eq!(
            refused(ask("ui.toast", "{\"text\": ")),
            (CallCode::Invalid, "The params of ui.toast aren't JSON.".into())
        );
        assert_eq!(refused(ask("ui.nope", "{}")).0, CallCode::Invalid);
        assert_eq!(got(ask("ui.toast", r#"{"text": "Hi"}"#)), json!({"shown": true}));
        drop(place);
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    /// A keychain the person keeps locked: each read asks them, and they
    /// say no, until they unlock it.
    #[derive(Default)]
    struct Locked {
        kept: as_plugin::MemorySecrets,
        reads: std::sync::atomic::AtomicUsize,
        locked: AtomicBool,
    }

    impl as_plugin::SecretStore for Locked {
        fn get(&self, library: &str, plugin: &PluginId, key: &str) -> PluginResult<Option<String>> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            match self.locked.load(Ordering::SeqCst) {
                true => {
                    Err(PluginError::Keychain { reason: "it is locked, or refused AutoScene.".into(), detail: None })
                }
                false => self.kept.get(library, plugin, key),
            }
        }

        fn set(&self, library: &str, plugin: &PluginId, key: &str, value: &str) -> PluginResult<()> {
            self.kept.set(library, plugin, key, value)
        }

        fn delete(&self, library: &str, plugin: &PluginId, key: &str) -> PluginResult<()> {
            self.kept.delete(library, plugin, key)
        }
    }

    #[test]
    fn a_page_asks_a_keychain_that_said_no_again_only_once_it_opens_again() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-page-keychain-{}", std::process::id()));
        let keychain = Arc::new(Locked::default());
        let place = place_with(&tmp, host_keeping(&tmp, keychain.clone()));
        let dir = tmp.join("dev.a.poster");
        std::fs::create_dir_all(dir.join("ui")).unwrap();
        std::fs::write(dir.join("ui/index.html"), b"<!doctype html>").unwrap();
        let mut manifest =
            page_only("dev.a.poster", json!([{"id": "main", "title": "Main", "entry": "ui/index.html"}]));
        manifest["contributes"]["settings"] = json!([{"key": "token", "type": "secret", "title": "Token"}]);
        std::fs::write(dir.join("plugin.json"), manifest.to_string()).unwrap();
        let p = place.host.store.commit(place.host.store.stage_folder(&dir).unwrap(), true, true).unwrap();
        place.host.store.secret_set(&p.record.id, "token", "hunter2").unwrap();
        let open = || {
            let grants = Grants {
                runtime: RuntimeKind::None,
                permissions: Vec::new(),
                hosts: Vec::new(),
                secrets: vec!["token".into()],
                protocol: 2,
            };
            let page = p.manifest.as_ref().unwrap().pages[0].clone();
            place.host.pages.open(&p, grants, &page, None).unwrap().0.token.clone()
        };
        let page = open();
        let person = Person::default();
        let get = |page: &str| place.page_asks(&person, page, "secrets.get", json!({"key": "token"}), Instant::now());

        // It said no: the page's next reads get that, without asking again.
        keychain.locked.store(true, Ordering::SeqCst);
        let no = refused(get(&page));
        for _ in 0..5 {
            assert_eq!(refused(get(&page)), no);
        }
        keychain.locked.store(false, Ordering::SeqCst);
        assert_eq!(refused(get(&page)), no);
        assert_eq!(keychain.reads.load(Ordering::SeqCst), 1);
        // Opened again, it reads.
        let again = open();
        assert_eq!(got(get(&again)), json!({"value": "hunter2"}));
        assert_eq!(keychain.reads.load(Ordering::SeqCst), 2);
        drop(place);
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn copy_text_past_ten_a_minute_is_refused() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-page-copy-{}", std::process::id()));
        let place = place(&tmp);
        let person = Person::default();
        let page = place.open_page(&tmp, "Copier", &[], false);
        let start = Instant::now();
        let copy = |text: &str, after: u64| {
            let at = start + Duration::from_secs(after);
            place.page_asks(&person, &page, "ui.copyText", json!({ "text": text }), at)
        };

        for i in 0..10 {
            assert_eq!(got(copy(&format!("clip {i}"), i)), Value::Null);
        }
        assert_eq!(refused(copy("one more", 10)), (CallCode::Busy, "A page copies at most 10 times a minute.".into()));
        assert_eq!(person.copied.lock().len(), 10);
        // A minute after the first two, two more.
        assert_eq!(got(copy("later", 61)), Value::Null);
        assert_eq!(got(copy("later", 61)), Value::Null);
        assert_eq!(refused(copy("later", 61)).0, CallCode::Busy);
        // Text only, of up to 64 KB.
        assert_eq!(refused(copy("a\0b", 200)).0, CallCode::Invalid);
        assert_eq!(refused(copy(&"x".repeat(64 << 10), 200)).0, CallCode::Invalid);
        assert_eq!(person.copied.lock().len(), 12);
        // None while the person works in another app, nor a dialog.
        person.away.store(true, Ordering::SeqCst);
        assert_eq!(
            refused(copy("swapped", 300)),
            (CallCode::Denied, "A page copies only while AutoScene is in front.".into())
        );
        let away = (CallCode::Denied, "A page opens a dialog only while AutoScene is in front.".to_owned());
        let at = start + Duration::from_secs(300);
        assert_eq!(refused(place.page_asks(&person, &page, "files.open", json!({}), at)), away);
        let link = json!({"url": "https://example.org/"});
        assert_eq!(refused(place.page_asks(&person, &page, "ui.openExternal", link, at)), away);
        assert_eq!(person.copied.lock().len(), 12);
        assert!(person.dialogs.lock().is_empty() && person.asked.lock().is_empty());
        drop(place);
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn files_save_takes_one_plain_name() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-page-save-{}", std::process::id()));
        let place = place(&tmp);
        let person = Person::default();
        let page = place.open_page(&tmp, "Saver", &[], false);
        let now = unhurried();
        let save = |params: Value| place.page_asks(&person, &page, "files.save", params, now());
        let out = std::path::absolute(tmp.join("saved").join("clips.txt")).unwrap();
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();

        let long = "a".repeat(101);
        for name in ["a/b.txt", "..", "", long.as_str(), "a b.txt", "C:\\x.txt", "../x.txt", "NUL.txt"] {
            assert_eq!(
                refused(save(json!({ "name": name, "text": "hi" }))),
                (
                    CallCode::Invalid,
                    "files.save takes a plain file name of up to 100 characters, like clips.txt.".into()
                ),
                "{name}"
            );
        }
        // Nor a program, a script or a shortcut, in any case.
        let runs =
            ["setup.exe", "run.bat", "x.cmd", "x.lnk", "x.url", "x.hta", "x.scr", "x.ps1", "x.desktop", "SETUP.EXE"];
        for name in runs.into_iter().chain(["notes.Command", "a.b.js", "x.jar", "x.dmg", "clips.vbs"]) {
            assert_eq!(
                refused(save(json!({ "name": name, "text": "hi" }))),
                (CallCode::Invalid, "A page can't save a program, a script or a shortcut.".into()),
                "{name}"
            );
        }
        // Nor offers to save as one.
        let as_program =
            json!({"name": "clips", "text": "hi", "filters": [{"name": "Clips", "extensions": ["txt", "EXE"]}]});
        assert_eq!(
            refused(save(as_program)),
            (CallCode::Invalid, "A page can't save a program, a script or a shortcut.".into())
        );
        assert!(person.dialogs.lock().is_empty());
        // Text or base64, one of them.
        assert_eq!(refused(save(json!({"name": "clips.txt"}))).0, CallCode::Invalid);
        assert_eq!(refused(save(json!({"name": "clips.txt", "text": "a", "dataBase64": "YQ=="}))).0, CallCode::Invalid);
        assert_eq!(refused(save(json!({"name": "clips.txt", "dataBase64": "not base64!"}))).0, CallCode::Invalid);

        // The person cancels; then saves where they chose, whole.
        assert_eq!(got(save(json!({"name": "clips.txt", "text": "one"}))), json!({"saved": false}));
        *person.save_at.lock() = Some(out.clone());
        assert_eq!(got(save(json!({"name": "clips.txt", "text": "one"}))), json!({"saved": true}));
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "one");
        assert_eq!(got(save(json!({"name": "clips.txt", "dataBase64": "dHdv"}))), json!({"saved": true}));
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "two");
        assert_eq!(std::fs::read_dir(out.parent().unwrap()).unwrap().count(), 1, "nothing left beside it");
        // The dialog says whose it is.
        assert_eq!(person.dialogs.lock()[0], "Plugin “Saver” saves a file: clips.txt");
        // Marked as a download is, so Windows asks before it runs anything in it.
        if cfg!(windows) {
            let zone = std::fs::read_to_string(format!("{}:Zone.Identifier", out.display())).unwrap();
            assert_eq!(zone, "[ZoneTransfer]\r\nZoneId=3\r\n");
        }
        // Never to a place that isn't plain.
        *person.save_at.lock() = Some(PathBuf::from("relative.txt"));
        assert_eq!(refused(save(json!({"name": "clips.txt", "text": "x"}))).0, CallCode::Invalid);
        // Nor as a program the person, or the dialog, named it.
        for name in ["setup.exe", "setup.exe.", "setup.exe . ."] {
            *person.save_at.lock() = Some(out.with_file_name(name));
            assert_eq!(
                refused(save(json!({"name": "clips.txt", "text": "x"}))),
                (CallCode::Invalid, "A page can't save a program, a script or a shortcut.".into()),
                "{name}"
            );
        }
        assert_eq!(std::fs::read_dir(out.parent().unwrap()).unwrap().count(), 1, "nothing saved as a program");
        drop(place);
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn files_open_refuses_past_max_bytes() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-page-open-{}", std::process::id()));
        let place = place(&tmp);
        let person = Person::default();
        let page = place.open_page(&tmp, "Opener", &[], false);
        let now = unhurried();
        let pick = |params: Value| place.page_asks(&person, &page, "files.open", params, now());
        let note = std::path::absolute(tmp.join("note.txt")).unwrap();
        std::fs::write(&note, b"0123456789").unwrap();

        // The person cancels: none.
        assert_eq!(got(pick(json!({}))), Value::Null);
        *person.pick.lock() = Some(note);
        assert_eq!(got(pick(json!({}))), json!({"name": "note.txt", "size": 10.0, "dataBase64": "MDEyMzQ1Njc4OQ=="}));
        assert_eq!(got(pick(json!({"maxBytes": 10})))["size"], json!(10.0));
        assert_eq!(
            refused(pick(json!({"maxBytes": 9}))),
            (CallCode::Invalid, "That file is larger than 9 bytes.".into())
        );
        // Past 16 MB, whatever the page asks for.
        let big = std::path::absolute(tmp.join("big.bin")).unwrap();
        std::fs::File::create(&big).unwrap().set_len((16 << 20) + 1).unwrap();
        *person.pick.lock() = Some(big);
        for asked in [json!({}), json!({"maxBytes": u32::MAX})] {
            assert_eq!(refused(pick(asked)), (CallCode::Invalid, "That file is larger than 16 MB.".into()));
        }
        assert_eq!(size_words(1536), "1.5 KB");
        drop(place);
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_file_dialog_offers_only_plain_kinds_of_file() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-page-filters-{}", std::process::id()));
        let place = place(&tmp);
        let person = Person::default();
        let page = place.open_page(&tmp, "Filters", &[], false);
        let pick = |filters: Value| {
            place.page_asks(&person, &page, "files.open", json!({ "filters": filters }), Instant::now())
        };
        let kind = |name: &str, extensions: &[&str]| json!({ "name": name, "extensions": extensions });

        let many: Vec<Value> = (0..17).map(|i| kind(&format!("Kind {i}"), &["txt"])).collect();
        let long_ext = "a".repeat(17);
        for filters in [
            json!(many),
            json!([kind("", &["txt"])]),
            json!([kind(&"n".repeat(61), &["txt"])]),
            json!([kind("Text", &["e x e"])]),
            json!([kind("Text", &[""])]),
            json!([kind("Text", &[".txt"])]),
            json!([kind("Text", &[long_ext.as_str()])]),
            json!([kind("Text", &["txt"; 17])]),
            json!([kind("Text\u{202E}txt.exe", &["txt"])]),
            json!([kind("Text\nFiles", &["txt"])]),
        ] {
            assert_eq!(
                refused(pick(filters.clone())),
                (
                    CallCode::Invalid,
                    "A file dialog offers at most 16 kinds of file, each named, with plain extensions.".into()
                ),
                "{filters}"
            );
        }
        assert!(person.dialogs.lock().is_empty());
        // Sixteen plain ones open the dialog.
        let sixteen: Vec<Value> = (0..16).map(|i| kind(&format!("Kind {i}"), &["txt", "srt"])).collect();
        assert_eq!(got(pick(json!(sixteen))), Value::Null);
        assert_eq!(*person.dialogs.lock(), ["Plugin “Filters” asks for a file"]);
        drop(place);
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_page_opens_one_dialog_at_a_time_and_waits_after_a_no() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-page-dialogs-{}", std::process::id()));
        let place = place(&tmp);
        let person = Person::default();
        let page = place.open_page(&tmp, "Asker", &[], false);
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let link = json!({"url": "https://example.org/"});
        let ask =
            |method: &str, params: &Value, ms: u64| place.page_asks(&person, &page, method, params.clone(), at(ms));
        let waits = (CallCode::Busy, "A page asks again only 3 seconds after the person says no.".to_owned());

        // A no holds its next dialog back 3 s, whichever it is: the person
        // can reach the page's Close between them.
        assert_eq!(got(ask("ui.openExternal", &link, 0)), json!({"opened": false}));
        assert_eq!(refused(ask("ui.openExternal", &link, 1000)), waits);
        assert_eq!(refused(ask("files.save", &json!({"name": "a.txt", "text": "a"}), 2000)), waits);
        assert_eq!(refused(ask("files.open", &json!({}), 2900)), waits);
        assert_eq!(person.asked.lock().len(), 1);
        assert!(person.dialogs.lock().is_empty());
        // A cancelled file dialog counts as a no.
        assert_eq!(got(ask("files.open", &json!({}), 3500)), Value::Null);
        assert_eq!(refused(ask("ui.openExternal", &link, 4500)), waits);
        // A yes doesn't.
        person.yes.store(true, Ordering::SeqCst);
        assert_eq!(got(ask("ui.openExternal", &link, 7000)), json!({"opened": true}));
        assert_eq!(got(ask("ui.openExternal", &link, 7001)), json!({"opened": true}));
        // Each page counts its own.
        person.yes.store(false, Ordering::SeqCst);
        assert_eq!(got(ask("ui.openExternal", &link, 8000)), json!({"opened": false}));
        let other = place.open_page(&tmp, "Other", &[], false);
        assert_eq!(
            got(place.page_asks(&person, &other, "ui.openExternal", link.clone(), at(8001))),
            json!({"opened": false})
        );

        // While one is open, the page gets no other; once it closes, it may.
        let held = Arc::new(Barrier::new(2));
        *person.held.lock() = Some(held.clone());
        person.yes.store(true, Ordering::SeqCst);
        std::thread::scope(|s| {
            let first = s.spawn(|| ask("ui.openExternal", &link, 20_000));
            held.wait();
            let busy = (CallCode::Busy, "This page has a dialog open already.".to_owned());
            assert_eq!(refused(ask("ui.openExternal", &link, 20_001)), busy);
            assert_eq!(refused(ask("files.open", &json!({}), 20_002)), busy);
            held.wait();
            assert_eq!(got(first.join().unwrap()), json!({"opened": true}));
        });
        *person.held.lock() = None;
        assert_eq!(got(ask("ui.openExternal", &link, 20_003)), json!({"opened": true}));
        assert_eq!(person.opened.lock().len(), 4);

        // A page that closes while its dialog is open does nothing with the yes.
        *person.held.lock() = Some(held.clone());
        std::thread::scope(|s| {
            let first = s.spawn(|| ask("ui.openExternal", &link, 30_000));
            held.wait();
            place.host.close_page(&page);
            held.wait();
            assert_eq!(refused(first.join().unwrap()), (CallCode::Closed, PluginError::Closed.to_string()));
        });
        assert_eq!(person.opened.lock().len(), 4);
        assert!(!place.host.page_limits.0.lock().contains_key(&page), "forgotten as it closed");
        drop(place);
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_closed_pages_waiting_call_writes_nothing() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-page-closed-{}", std::process::id()));
        let place = place(&tmp);
        let person = Person::default();
        let page = place.open_page(&tmp, "Keeper", &[], false);
        let m = place.host.pages.get(&page).unwrap();
        let data = place.host.store.data_dir(&m.plugin);
        std::fs::create_dir_all(&data).unwrap();
        let calls = AppCalls::page(place.stand.clone(), place.host.clone(), m.clone());
        let set = |key: &str| {
            let params = serde_json::value::to_raw_value(&json!({"key": key, "value": 1})).unwrap();
            calls.page_answer(&m, HostMethod::StateSet, &params, &person, Instant::now()).map(|a| a.result)
        };

        assert_eq!(set("open").unwrap(), Value::Null);
        // A call that got its page before it closed, and comes to its turn
        // after: as an uninstall deletes the plugin's data, say.
        place.host.close_page(&page);
        assert_eq!(said(set("closed")), (CallCode::Closed, PluginError::Closed.to_string()));
        assert_eq!(page_state::keys(&data).unwrap(), ["open"]);
        drop((calls, m, place));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    /// Where a page's runs go here: a job runner of their own, whose jobs
    /// start as `queue_run`'s do and hand on the line their program would
    /// get, then wait until they are stopped. A page reaches nothing else.
    struct Activity {
        stand: Arc<Stand>,
        host: Arc<PluginHost>,
        jobs: JobRunner,
        lines: mpsc::Sender<RunMessage>,
    }

    impl Activity {
        /// The runs of the pages of `place`, with the setting on (its
        /// plugins are loaded folders), and where their lines arrive.
        fn of(place: &Place) -> (Self, mpsc::Receiver<RunMessage>) {
            place.stand.library.set_allows_unreviewed_plugins(true).unwrap();
            let (lines, heard) = mpsc::channel();
            let jobs = JobRunner::new(&[(Class::Plugin, 1)], |_| {});
            (Self { stand: place.stand.clone(), host: place.host.clone(), jobs, lines }, heard)
        }

        /// The job `id` names, as a page's call gave it.
        fn job(&self, id: &Value) -> JobInfo {
            self.jobs.jobs().into_iter().find(|j| json!(j.id) == *id).unwrap()
        }

        /// Stops every run, and waits until each ended.
        fn stop(&self) {
            self.jobs.cancel_where(|_| true);
            let until = Instant::now() + Duration::from_secs(10);
            while self.jobs.jobs().iter().any(|j| !j.state.is_finished()) {
                assert!(Instant::now() < until, "a run never stopped");
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    impl PageIo for Activity {
        fn in_front(&self) -> bool {
            true
        }

        fn copy_text(&self, _: &str) -> Result<(), String> {
            unreachable!()
        }

        fn confirm(&self, _: &str, _: &str, _: &str) -> bool {
            unreachable!()
        }

        fn open_url(&self, _: &str) -> Result<(), String> {
            unreachable!()
        }

        fn save_to(&self, _: &str, _: &str, _: &[FileFilter]) -> Option<PathBuf> {
            unreachable!()
        }

        fn pick(&self, _: &str, _: &[FileFilter]) -> Option<PathBuf> {
            unreachable!()
        }

        fn queue_run(
            &self,
            id: &PluginId,
            kind: ActionKind,
            action: &str,
            project_id: Option<ProjectId>,
            export_id: Option<ExportId>,
            input: Option<Value>,
        ) -> AppResult<Option<JobId>> {
            let key = RunKey::new(id, kind, action, project_id, export_id, input.as_ref());
            let work = |gpu| {
                let (stand, host, key, lines) =
                    (self.stand.clone(), self.host.clone(), key.clone(), self.lines.clone());
                move |ctx: &JobCtx| {
                    let _running = host.hold(ctx, &key.plugin)?;
                    let p = startable(&host.store, &stand.library, &key, Trigger::Page, gpu)?;
                    let action = ActionContext { kind: key.kind, id: key.action.clone(), trigger: Trigger::Page };
                    let failed = |e: String| JobError::Failed(e);
                    let spec = host.store.run_spec(&p, action, Path::new("ffmpeg"), Path::new("ffprobe"), "9.0");
                    let mut message = spec.map_err(|e| failed(e.to_string()))?.message;
                    add_context(&mut message, &*stand, &key, input).map_err(|e| failed(e.to_string()))?;
                    lines.send(message).unwrap();
                    while !ctx.cancel.is_cancelled() {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(JobError::Cancelled)
                }
            };
            let allow = self.stand.library.allows_unreviewed_plugins()?;
            Ok(self.host.queue(&self.jobs, allow, key.clone(), Trigger::Page, work)?)
        }
    }

    impl Place {
        /// Loads the plugin `name` from its folder in `tmp`, a program of
        /// `protocol` with the export action `post`, the command `hello` and
        /// the command `count` (which needs a project), and opens its page,
        /// in the project when `in_project`: its session.
        fn open_actions(&self, tmp: &Path, name: &str, protocol: u32, in_project: bool) -> String {
            let id = format!("dev.a.{}", name.to_ascii_lowercase());
            let dir = tmp.join(&id);
            std::fs::create_dir_all(dir.join("ui")).unwrap();
            for file in ["main.py", "main.py.lock", "ui/index.html"] {
                std::fs::write(dir.join(file), b"").unwrap();
            }
            let manifest = json!({
                "manifestVersion": 2,
                "id": id,
                "version": "1.0.0",
                "name": name,
                "description": "Runs from its page.",
                "author": {"name": "Someone"},
                "engines": {"autoscene": ">=1.0.0, <2", "protocol": protocol},
                "runtime": {"type": "process", "command": {"*": ["uv", "run", "--script", "--locked", "main.py"]}},
                "contributes": {
                    "pages": [{"id": "main", "title": "Main", "entry": "ui/index.html"}],
                    "exportActions": [{"id": "post", "title": "Post"}],
                    "commands": [
                        {"id": "hello", "title": "Say hello"},
                        {"id": "count", "title": "Count clips", "needsProject": true}
                    ]
                }
            });
            std::fs::write(dir.join("plugin.json"), manifest.to_string()).unwrap();
            let p = self.host.store.commit(self.host.store.stage_folder(&dir).unwrap(), true, true).unwrap();
            let page = p.manifest.as_ref().unwrap().pages[0].clone();
            let project = in_project.then(|| self.project.to_string());
            self.host.pages.open(&p, Grants::of(&p).unwrap(), &page, project).unwrap().0.token.clone()
        }
    }

    const STARTED: Duration = Duration::from_secs(10);

    #[test]
    fn a_page_run_is_a_job_with_its_input() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-page-run-{}", std::process::id()));
        let place = place(&tmp);
        let (activity, lines) = Activity::of(&place);
        let page = place.open_actions(&tmp, "Poster", 2, true);
        let ask = |params: Value| got(place.page_asks(&activity, &page, "actions.run", params, Instant::now()));
        let mine = place.exports[0];
        let input = json!({"caption": "Walter", "clips": [0, 1]});

        // An export action on an export of its project: a job in Activity,
        // as one from the export's menu is.
        let ran = ask(json!({"action": "post", "exportId": mine.to_string(), "input": input}));
        let job = activity.job(&ran["jobId"]);
        assert_eq!((job.kind, job.title.as_str(), job.subtitle.as_deref()), (JobKind::Plugin, "Post", Some("Poster")));
        assert_eq!((job.project_id, job.export_id, job.media_id), (Some(place.project), Some(mine), None));
        // Its program is told the page started it, and gets what the page handed it.
        let line = lines.recv_timeout(STARTED).unwrap();
        assert_eq!(
            (line.protocol, line.action.kind, line.action.trigger),
            (2, ActionKind::ExportAction, Trigger::Page)
        );
        assert_eq!((line.action.id.as_str(), line.input.as_ref()), ("post", Some(&input)));
        let (project, export) = (line.project.unwrap(), line.export.unwrap());
        assert_eq!((project.id, export.id), (place.project.to_string(), mine.to_string()));
        assert_eq!(export.clips.len(), 2);

        // A command runs in the project the page is open in. Without input,
        // its line has none.
        activity.stop();
        let ran = ask(json!({"action": "count"}));
        let line = lines.recv_timeout(STARTED).unwrap();
        assert_eq!((line.action.kind, line.action.trigger, line.input), (ActionKind::Command, Trigger::Page, None));
        assert_eq!((line.project.unwrap().id, line.export.is_none()), (place.project.to_string(), true));
        let job = activity.job(&ran["jobId"]);
        assert_eq!((job.title.as_str(), job.project_id, job.export_id), ("Count clips", Some(place.project), None));
        activity.stop();
        drop((activity, place));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_protocol_1_plugin_can_t_be_run_from_a_page() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-page-run-old-{}", std::process::id()));
        let place = place(&tmp);
        let (activity, lines) = Activity::of(&place);
        let ask = |session: &str| {
            place.page_asks(&activity, session, "actions.run", json!({"action": "hello"}), Instant::now())
        };
        let needs = "actions.run needs a plugin whose program speaks protocol 2.";

        // Its program would read a trigger it doesn't know; a page-only
        // plugin has no program to run.
        let old = place.open_actions(&tmp, "Elder", 1, true);
        let viewer = place.open_page(&tmp, "Viewer", &[], true);
        for session in [&old, &viewer] {
            assert_eq!(refused(ask(session)), (CallCode::Denied, needs.to_owned()));
        }
        // Nor once its plugin.json went back to protocol 1 after its page
        // opened: the run is refused as it is queued.
        let shifter = place.open_actions(&tmp, "Shifter", 2, true);
        let file = tmp.join("dev.a.shifter").join("plugin.json");
        let manifest = std::fs::read_to_string(&file).unwrap().replace(r#""protocol":2"#, r#""protocol":1"#);
        std::fs::write(&file, manifest).unwrap();
        assert_eq!(refused(ask(&shifter)), (CallCode::Invalid, needs.to_owned()));
        assert!(activity.jobs.jobs().is_empty());
        assert!(lines.try_recv().is_err());

        // From the palette it runs as it always did.
        let elder = PluginId::parse("dev.a.elder").unwrap();
        let hello = key(&elder, ActionKind::Command, "hello", None, None);
        let palette = place.host.queue(&activity.jobs, true, hello, Trigger::Palette, |_| |_: &JobCtx| Ok(()));
        assert!(palette.unwrap().is_some());
        activity.stop();
        drop((activity, place));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn the_same_page_run_is_queued_once() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-page-run-once-{}", std::process::id()));
        let place = place(&tmp);
        let (activity, lines) = Activity::of(&place);
        let page = place.open_actions(&tmp, "Counter", 2, true);
        let ask = |params: Value| place.page_asks(&activity, &page, "actions.run", params, Instant::now());
        let job = |params: Value| got(ask(params))["jobId"].clone();
        let one = json!({"action": "count", "input": {"n": 1}});

        let first = job(one.clone());
        lines.recv_timeout(STARTED).unwrap();
        // The same run again, while it runs: none.
        assert_eq!(got(ask(one.clone())), json!({"jobId": null}));
        // With other input, or none, it is another run, which waits its turn.
        let other = job(json!({"action": "count", "input": {"n": 2}}));
        let bare = job(json!({"action": "count"}));
        assert!(other.is_string() && bare.is_string() && other != bare, "{other} {bare}");
        // The person's own run of it, from the palette, is the one without input.
        let counter = PluginId::parse("dev.a.counter").unwrap();
        let count = key(&counter, ActionKind::Command, "count", Some(place.project), None);
        let palette = place.host.queue(&activity.jobs, true, count, Trigger::Palette, |_| |_: &JobCtx| Ok(()));
        assert_eq!(palette.unwrap(), None);
        // Once it ended, the same run is queued again.
        let first: JobId = serde_json::from_value(first).unwrap();
        activity.jobs.cancel(first);
        let until = Instant::now() + STARTED;
        while !activity.job(&json!(first)).state.is_finished() {
            assert!(Instant::now() < until, "a stopped run never ended");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(job(one).is_string());

        // A page can't fill Activity: with MAX_PAGE_RUNS of its plugin's
        // runs waiting or running, it queues none more.
        for n in 3..MAX_PAGE_RUNS {
            assert!(job(json!({"action": "count", "input": {"n": n}})).is_string(), "{n}");
        }
        assert_eq!(
            refused(ask(json!({"action": "count", "input": {"n": "more"}}))),
            (CallCode::Busy, "Counter is running. Wait for it to finish, or stop it in Activity.".into())
        );
        activity.stop();
        assert!(job(json!({"action": "count", "input": {"n": "more"}})).is_string());
        activity.stop();
        drop((activity, place));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_page_run_of_an_export_action_needs_an_export_of_its_project() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-page-run-export-{}", std::process::id()));
        let place = place(&tmp);
        let (activity, lines) = Activity::of(&place);
        let inside = place.open_actions(&tmp, "Poster", 2, true);
        let outside = place.open_actions(&tmp, "Rover", 2, false);
        let ask =
            |session: &str, params: Value| place.page_asks(&activity, session, "actions.run", params, Instant::now());
        let mine = place.exports[0].to_string();
        let invalid = |why: &str| (CallCode::Invalid, why.to_owned());

        // Without one, an export action says what it needs.
        assert_eq!(refused(ask(&inside, json!({"action": "post"}))), invalid("Choose an export to run it on."));
        // Only an export of the project its page is open in.
        let gone = json!({"action": "post", "exportId": ExportId::new().to_string()});
        assert_eq!(refused(ask(&inside, gone)), (CallCode::NotFound, "This export no longer exists.".into()));
        let wrong = json!({"action": "post", "exportId": "e1"});
        assert_eq!(refused(ask(&inside, wrong)), (CallCode::NotFound, "That export doesn't exist.".into()));
        let elsewhere = json!({"action": "post", "exportId": mine});
        assert_eq!(refused(ask(&outside, elsewhere)), invalid("This page isn't open in a project."));
        // A command never runs on one, and one that needs a project needs the page's.
        let on_export = json!({"action": "hello", "exportId": mine});
        assert_eq!(refused(ask(&inside, on_export)), invalid("A command doesn't run on an export."));
        assert_eq!(refused(ask(&outside, json!({"action": "count"}))), invalid("Open a project to run it in."));
        // An action its plugin hasn't is named by the plugin, not by the page's words.
        assert_eq!(refused(ask(&inside, json!({"action": "rm -rf /"}))), invalid("Poster has no such action."));
        assert!(activity.jobs.jobs().is_empty());

        // On an export of its project, it runs on that one.
        let ran = got(ask(&inside, json!({"action": "post", "exportId": mine})));
        assert_eq!(activity.job(&ran["jobId"]).export_id, Some(place.exports[0]));
        assert_eq!(lines.recv_timeout(STARTED).unwrap().export.unwrap().id, mine);
        // A page outside a project runs a command that needs none.
        let ran = got(ask(&outside, json!({"action": "hello"})));
        assert_eq!(activity.job(&ran["jobId"]).project_id, None);
        activity.stop();
        drop((activity, place));
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}

//! Plugins' sources at work: links the person adds, in New project or a
//! project's Add links…, become runs that fetch their videos into a folder
//! AutoScene names, and each video a run finishes joins the project at once
//! ([`queue_links`]).
//!
//! Links are read here, never by the page alone: a pasted text is split into
//! links, each cleaned (`https://`, its host in lower case, no `#…` part) and
//! given to the first active plugin whose source takes its host
//! ([`parse_links`]). The page shows that for feedback; queueing reads the
//! links again.
//!
//! One run per link, on the download lane (`Class::Download`), so a long
//! download never holds up another plugin's run; a plugin still runs one run
//! at a time. The same link in the same project waits once ([`RunKey`] with
//! the link as its input).
//!
//! Each run gets `outDir`, `<library>/Downloads/<Title>`, beside the project's
//! folder, and `partDir`, a folder in it for what isn't finished, which goes
//! when the run ends, however it ends, and at the next start after a crash (a
//! note in the plugins' parts folder says where). A file line counts only
//! for a plain file inside `outDir`, outside `partDir`, of a kind of video
//! AutoScene imports and not empty; anything else goes to the plugin's log.
//!
//! Links survive a quit: a queued link is kept in the library until its run
//! ends done, failed or stopped by the person, and queued again at the next
//! start ([`resume`]). A quit's own stop keeps it.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use as_api::JobKind;
use as_core::{Msg, ProjectId, ToMsg, msg};
use as_jobs::{Class, JobCtx, JobError, JobResult, JobRunner, JobSpec};
use as_plugin::protocol::{ProjectContext, SourceContext};
use as_plugin::{ActionContext, ActionKind, FileSink, HostCalls, Installed, PluginId, Trigger, runtime};
use as_project::Library;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager};

use super::filters::{RunFn, ToolsFn};
use super::{PluginHost, RunKey, calls_for, plugin_job_error, project_context, runnable, startable};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::paths;
use crate::state::AppState;
use crate::work::job_error;

/// The most links one paste or one queue takes.
pub const MAX_LINKS: usize = 100;
/// The longest link taken, in bytes.
pub const MAX_LINK_BYTES: usize = 2048;
/// The most files one run adds.
pub const MAX_FILES_PER_RUN: usize = 1000;
/// The library setting that keeps the links still to fetch.
const PENDING_KEY: &str = "sources.pending";
/// How a run's folder for unfinished files starts.
const PART_PREFIX: &str = ".autoscene-part-";
/// How a note of such a folder is named, in the plugins' parts folder (a
/// pack action's notes are named by their job id alone).
const NOTE_PREFIX: &str = "source-";

/// A link of a pasted text, as AutoScene reads it.
#[derive(Clone, Debug, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LinkItem {
    /// What was pasted.
    pub text: String,
    /// The link, cleaned; empty when it isn't one.
    pub link: String,
    /// The link, short, to show: its host without `www.` and its path.
    pub short: String,
    /// The plugin and source that take it, when one does.
    pub plugin_id: Option<String>,
    pub source_id: Option<String>,
    /// What that source fetches from, as its plugin.json says ("YouTube and
    /// TikTok"), and the plugin's name.
    pub source_title: Option<String>,
    pub plugin_name: Option<String>,
    /// Why it can't be added, when it can't.
    pub refused: Option<Msg>,
}

/// What queueing links did.
#[derive(Clone, Debug, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LinksQueued {
    /// How many runs were queued.
    pub queued: u32,
    /// How many were waiting or running already.
    pub waiting: u32,
    /// Links that couldn't be queued, with why.
    pub refused: Vec<LinkItem>,
    /// Where their videos go.
    pub out_dir: String,
}

/// A source of an active plugin: its plugin's id and name, and the source.
#[derive(Clone, Debug)]
pub struct Offered {
    pub plugin: PluginId,
    pub name: String,
    pub source: as_plugin::SourceSpec,
}

/// The sources of the active plugins in `plugins`, in their order.
pub fn offered(plugins: &[Installed], allow_unreviewed: bool) -> Vec<Offered> {
    let mut out = Vec::new();
    for p in plugins {
        let Ok(m) = runnable(p, allow_unreviewed) else { continue };
        for s in &m.sources {
            out.push(Offered { plugin: m.id.clone(), name: m.name.clone(), source: s.clone() });
        }
    }
    out
}

/// Splits `text` into links (by white space and commas), each cleaned and
/// given to the first of `sources` that takes its host. A link pasted twice
/// is listed once. At most [`MAX_LINKS`]: more are refused.
pub fn parse_links(text: &str, sources: &[Offered]) -> Vec<LinkItem> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for token in text.split(|c: char| c.is_whitespace() || c == ',').filter(|t| !t.is_empty()) {
        let mut item = LinkItem {
            text: token.chars().take(200).collect(),
            link: String::new(),
            short: String::new(),
            plugin_id: None,
            source_id: None,
            source_title: None,
            plugin_name: None,
            refused: None,
        };
        match clean_link(token) {
            Err(why) => item.refused = Some(why),
            Ok((link, host)) => {
                if !seen.insert(link.clone()) {
                    continue;
                }
                item.short = short_link(&link);
                item.link = link;
                match sources.iter().find(|o| takes(&o.source.link_hosts, &host)) {
                    Some(o) => {
                        item.plugin_id = Some(o.plugin.to_string());
                        item.source_id = Some(o.source.id.clone());
                        item.source_title = Some(o.source.title.clone());
                        item.plugin_name = Some(o.name.clone());
                    }
                    None => {
                        item.refused =
                            Some(msg!("plugin.noSourceForHost", "No plugin takes links from {{host}}.", host = &host))
                    }
                }
            }
        }
        if out.len() >= MAX_LINKS {
            item.refused = Some(msg!("plugin.tooManyLinks", "At most {{max}} links at once.", max = MAX_LINKS));
        }
        out.push(item);
    }
    out
}

/// Whether a link of `host` is one a source of `hosts` takes: the host
/// itself, or one of its subdomains.
fn takes(hosts: &[String], host: &str) -> bool {
    hosts.iter().any(|h| host == h || host.strip_suffix(h.as_str()).is_some_and(|rest| rest.ends_with('.')))
}

/// `token` as a link a source may fetch, and its host: `https://` (a link
/// without a scheme gets it, `http://` is raised to it), a name on the
/// internet as its host (no address, no user or password, no other port),
/// without its `#…` part.
fn clean_link(token: &str) -> Result<(String, String), Msg> {
    let not_a_link = || msg!("plugin.notALink", "That isn't a web link.");
    if token.len() > MAX_LINK_BYTES {
        return Err(msg!("plugin.linkTooLong", "That link is too long."));
    }
    let with_scheme = if token.contains("://") { token.to_owned() } else { format!("https://{token}") };
    let mut url = url::Url::parse(&with_scheme).map_err(|_| not_a_link())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(not_a_link());
    }
    let host = match url.host() {
        Some(url::Host::Domain(d)) => d.trim_end_matches('.').to_ascii_lowercase(),
        _ => return Err(not_a_link()),
    };
    if !host.contains('.') || !url.username().is_empty() || url.password().is_some() {
        return Err(not_a_link());
    }
    if url.port().is_some_and(|p| p != 443 && p != 80) {
        return Err(not_a_link());
    }
    url.set_scheme("https").map_err(|()| not_a_link())?;
    let _ = url.set_port(None);
    url.set_fragment(None);
    Ok((url.to_string(), host))
}

/// `link` short enough to show: its host without `www.`, its path and
/// query, cut to 60 characters.
pub fn short_link(link: &str) -> String {
    let rest = link.strip_prefix("https://").unwrap_or(link);
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    let rest = rest.trim_end_matches('/');
    if rest.chars().count() <= 60 {
        rest.to_owned()
    } else {
        format!("{}…", rest.chars().take(59).collect::<String>())
    }
}

/// A link still to fetch, kept in the library until its run ends.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pending {
    project_id: ProjectId,
    plugin: String,
    source: String,
    link: String,
}

/// One change of the kept links at a time.
static PENDING: Mutex<()> = Mutex::new(());

fn pending(lib: &Library) -> Vec<Pending> {
    match lib.shared_db().setting(PENDING_KEY) {
        Ok(Some(text)) => serde_json::from_str(&text).unwrap_or_default(),
        Ok(None) => Vec::new(),
        Err(e) => {
            log::warn!("reading the links still to fetch: {e}");
            Vec::new()
        }
    }
}

fn change_pending(lib: &Library, change: impl FnOnce(&mut Vec<Pending>)) {
    let _one = PENDING.lock();
    let mut all = pending(lib);
    change(&mut all);
    let text = serde_json::to_string(&all).unwrap_or_else(|_| "[]".into());
    if let Err(e) = lib.shared_db().set_setting(PENDING_KEY, &text) {
        log::warn!("keeping the links still to fetch: {e}");
    }
}

/// Adds a checked file to a project and starts indexing it: whether it
/// was new.
pub type AddFn = dyn Fn(ProjectId, &str) -> Result<bool, Msg> + Send + Sync;

/// What sources use of the app.
#[derive(Clone)]
pub struct Sourcing {
    pub host: Arc<PluginHost>,
    pub jobs: JobRunner,
    pub lib: Arc<Library>,
    pub tools: Arc<ToolsFn>,
    pub run: Arc<RunFn>,
    /// Adds a checked file to a project and starts indexing it: whether it
    /// was new.
    pub add: Arc<AddFn>,
}

impl Sourcing {
    /// The app's.
    pub fn of(app: &AppHandle) -> AppResult<Self> {
        let state = app.state::<AppState>();
        let tools = app.clone();
        let adder = app.clone();
        let lib = state.library.clone();
        let add_lib = lib.clone();
        Ok(Self {
            host: super::host(&state)?,
            jobs: state.jobs.clone(),
            lib,
            tools: Arc::new(move || {
                let t = tools.state::<AppState>().tools().map_err(|e| e.msg)?;
                Ok((t.ffmpeg.clone(), t.ffprobe.clone(), t.version.clone()))
            }),
            run: Arc::new(|spec, cancel, report| runtime::run(spec, cancel, report)),
            add: Arc::new(move |project, file| {
                crate::work::add_files(&adder, &add_lib, project, &[file.to_owned()])
                    .map(|n| n > 0)
                    .map_err(|e| e.msg())
            }),
        })
    }

    fn sources(&self) -> AppResult<Vec<Offered>> {
        let allow = self.lib.allows_unreviewed_plugins()?;
        Ok(offered(&self.host.store.list()?, allow))
    }
}

/// Reads `text` as links, for the page's feedback.
pub fn read_links(sv: &Sourcing, text: &str) -> AppResult<Vec<LinkItem>> {
    Ok(parse_links(text, &sv.sources()?))
}

/// Queues a run for each of `links` that a source of an active plugin
/// takes, for the project `project`. Links are read again here.
pub fn queue_links(sv: &Sourcing, project: ProjectId, links: &[String]) -> AppResult<LinksQueued> {
    if links.len() > MAX_LINKS {
        let many = msg!("plugin.tooManyLinks", "At most {{max}} links at once.", max = MAX_LINKS);
        return Err(AppError::new(ErrorCode::Invalid, many));
    }
    let summary = sv.lib.summary(project)?;
    let out_dir = sv.lib.default_download_dir(&summary)?;
    let items = parse_links(&links.join("\n"), &sv.sources()?);
    let mut done = LinksQueued { queued: 0, waiting: 0, refused: Vec::new(), out_dir: out_dir.display().to_string() };
    for item in items {
        let (Some(plugin), Some(source), None) = (&item.plugin_id, &item.source_id, &item.refused) else {
            done.refused.push(item);
            continue;
        };
        let entry =
            Pending { project_id: project, plugin: plugin.clone(), source: source.clone(), link: item.link.clone() };
        // Kept before its job exists: a job that ends at once removes it after.
        let had = pending(&sv.lib).contains(&entry);
        if !had {
            change_pending(&sv.lib, |all| all.push(entry.clone()));
        }
        let unkeep = || {
            if !had {
                change_pending(&sv.lib, |all| all.retain(|e| *e != entry));
            }
        };
        match queue_one(sv, &entry, &out_dir) {
            Ok(true) => done.queued += 1,
            Ok(false) => {
                unkeep();
                done.waiting += 1;
            }
            Err(e) => {
                unkeep();
                let mut item = item;
                item.refused = Some(e.msg);
                done.refused.push(item);
            }
        }
    }
    Ok(done)
}

/// Queues the run of `entry` into `out_dir`: false when it waits or runs
/// already.
fn queue_one(sv: &Sourcing, entry: &Pending, out_dir: &Path) -> AppResult<bool> {
    let plugin = PluginId::parse(&entry.plugin)?;
    let p = sv.host.store.get(&plugin)?;
    let allow = sv.lib.allows_unreviewed_plugins()?;
    let m = runnable(&p, allow)?;
    let Some(s) = m.sources.iter().find(|s| s.id == entry.source) else {
        let gone = msg!("plugin.noSuchSource", "{{name}} doesn't take links any more.", name = &m.name);
        return Err(AppError::new(ErrorCode::NotFound, gone));
    };
    let input = serde_json::Value::String(entry.link.clone());
    let key = RunKey::new(&plugin, ActionKind::Source, &s.id, Some(entry.project_id), None, Some(&input));
    if sv.host.is_queued(&sv.jobs, &key) {
        return Ok(false);
    }
    let job = JobSpec {
        kind: JobKind::Download,
        class: Class::Download,
        title: msg!("plugin.downloading", "Downloading {{link}}", link = short_link(&entry.link)),
        subtitle: Some(m.name.clone()),
        project_id: Some(entry.project_id),
        media_id: None,
        export_id: None,
    };
    let gpu = m.runtime.gpu;
    // Goes with the work, run or not: a job stopped while it waits never
    // runs, and its link is no longer the person's to fetch.
    let kept = Kept { lib: sv.lib.clone(), entry: entry.clone(), quitting: sv.host.quitting() };
    let work = {
        let (sv, key, link, out) = (sv.clone(), key.clone(), entry.link.clone(), out_dir.to_path_buf());
        move |ctx: &JobCtx| {
            let _kept = kept;
            fetch(&sv, ctx, &key, &link, &out, gpu)
        }
    };
    Ok(sv.host.submit(&sv.jobs, key, job, work).is_some())
}

/// A link kept for the next start while its job waits or runs: it goes as
/// the job's work goes (run, stopped while waiting, or dropped with its
/// plugin), unless AutoScene quits, which keeps it.
struct Kept {
    lib: Arc<Library>,
    entry: Pending,
    /// The host's [`PluginHost::set_quitting`] flag.
    quitting: Arc<AtomicBool>,
}

impl Drop for Kept {
    fn drop(&mut self) {
        if !self.quitting.load(Ordering::SeqCst) {
            let entry = &self.entry;
            change_pending(&self.lib, |all| all.retain(|e| e != entry));
        }
    }
}

/// At the start (and when a quit didn't happen after all): queues again the
/// links a quit left, whose project and source are still there; drops the
/// rest. Only what it drops leaves the list: a job that ends meanwhile
/// removes its own.
pub fn resume(sv: &Sourcing) {
    let left = pending(&sv.lib);
    let mut dropped = Vec::new();
    for entry in left {
        let queued = sv
            .lib
            .summary(entry.project_id)
            .map_err(AppError::from)
            .and_then(|summary| Ok(sv.lib.default_download_dir(&summary)?))
            .and_then(|out| queue_one(sv, &entry, &out));
        if let Err(e) = queued {
            log::info!("a link left to fetch is dropped: {}", e.msg.text);
            dropped.push(entry);
        }
    }
    if !dropped.is_empty() {
        change_pending(&sv.lib, |all| all.retain(|e| !dropped.contains(e)));
    }
}

/// The queued run `key` of the link `link`, now it starts: fetches its
/// videos into `out`, each added to the project as it is finished.
fn fetch(sv: &Sourcing, ctx: &JobCtx, key: &RunKey, link: &str, out: &Path, gpu: bool) -> JobResult {
    let _running = sv.host.hold(ctx, &key.plugin)?;
    let p = startable(&sv.host.store, &sv.lib, key, Trigger::Links, gpu)?;
    let project = key.project_id.ok_or_else(|| {
        JobError::Failed(msg!("plugin.sourceNeedsProject", "A source adds videos to a project from links."))
    })?;
    let context: ProjectContext = sv.lib.project(project).and_then(|pr| project_context(&pr)).map_err(job_error)?;
    let (ffmpeg, ffprobe, version) = (sv.tools)().map_err(JobError::Failed)?;
    let report = |stage: &Msg, f: Option<f64>| ctx.report(stage, f);
    let tools = (ffmpeg.as_path(), ffprobe.as_path(), version.as_str());
    let action = ActionContext { kind: ActionKind::Source, id: key.action.clone(), trigger: Trigger::Links };
    let mut run = sv.host.run_spec(&p, action, tools, &ctx.cancel, &report).map_err(plugin_job_error)?;
    let unwritable = |e: io::Error| {
        log::warn!("making the folder for a source's videos: {e}");
        JobError::Failed(msg!(
            "plugin.downloadFolderFailed",
            "AutoScene couldn't make the folder for the videos: {{why}}",
            why = e.to_string()
        ))
    };
    std::fs::create_dir_all(out).map_err(unwritable)?;
    let part = PartDir::make(out, &ctx.id.to_string(), sv.host.store.parts_dir()).map_err(unwritable)?;
    run.message.project = Some(context);
    run.message.source = Some(SourceContext {
        link: link.to_owned(),
        out_dir: out.to_string_lossy().into_owned(),
        part_dir: part.path.to_string_lossy().into_owned(),
    });
    let calls = calls_for(&sv.lib, &sv.host, &p, key, run.protocol).map_err(plugin_job_error)?;
    run.calls = calls.map(|c| Arc::new(c) as Arc<dyn HostCalls>);
    let added = Arc::new(Mutex::new(0usize));
    run.files = Some(file_sink(sv.add.clone(), project, out, &part.path, added.clone(), run.log_path.clone()));
    let name = run.name.clone();
    let said = (sv.run)(run, &ctx.cancel, &report).map_err(plugin_job_error);
    drop(part);
    let said = said?;
    if let Some(message) = said {
        log::info!("{} {} fetched {}: {message}", key.plugin, key.action, short_link(link));
    }
    if *added.lock() == 0 {
        return Err(JobError::Failed(msg!(
            "plugin.sourceAddedNothing",
            "{{name}} added no video from {{link}}.",
            name = &name,
            link = short_link(link)
        )));
    }
    Ok(())
}

/// Where a source's run hands the files it finished: each checked
/// ([`check_file`]) and added to `project`; what isn't added goes to the
/// plugin's log at `log`.
fn file_sink(
    add: Arc<AddFn>,
    project: ProjectId,
    out: &Path,
    part: &Path,
    added: Arc<Mutex<usize>>,
    log: PathBuf,
) -> FileSink {
    let written = out.to_path_buf();
    let (out, part) = (std::fs::canonicalize(out).ok(), std::fs::canonicalize(part).ok());
    let mut seen = 0usize;
    Box::new(move |path: &str, title: Option<&str>| {
        seen += 1;
        let said = |line: String| note_in_log(&log, &line);
        if seen > MAX_FILES_PER_RUN {
            if seen == MAX_FILES_PER_RUN + 1 {
                said(format!("[host] more than {MAX_FILES_PER_RUN} files: the rest aren't added"));
            }
            return;
        }
        let (Some(out), Some(part)) = (&out, &part) else {
            said("[host] the run's folders are gone: nothing is added".into());
            return;
        };
        let file = paths::name(Path::new(path)).into_owned();
        match check_file(path, &written, out, part) {
            Err(why) => said(format!("[host] not added: {file}: {why}")),
            Ok(()) => match add(project, path) {
                Ok(new) => {
                    *added.lock() += 1;
                    let what = title.unwrap_or(&file);
                    said(format!("[host] added {file}{}", if new { "" } else { " (the project has it already)" }));
                    log::info!("a source added {what}");
                }
                Err(why) => said(format!("[host] not added: {file}: {}", why.text)),
            },
        }
    })
}

/// Appends `line` to the plugin's log; a log that can't be written loses it.
fn note_in_log(log: &Path, line: &str) {
    use std::io::Write as _;
    if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(log) {
        let _ = writeln!(f, "{line}");
    }
}

/// Why the file a source named at `path` isn't added, if it isn't: it must
/// be an absolute, plain path to a file (not a link, not a reparse point)
/// inside `out` (canonical) and outside `part` (canonical), of a kind of
/// video AutoScene imports, and not empty.
fn check_file(path: &str, written: &Path, out: &Path, part: &Path) -> Result<(), &'static str> {
    let p = Path::new(path);
    if !p.is_absolute() || !paths::is_plain(p) {
        return Err("not a plain absolute path");
    }
    // Before anything reads it: a path elsewhere (a share, say) is never touched.
    if !lexically_inside(p, written) {
        return Err("outside the run's outDir");
    }
    if !as_project::naming::is_video(p) {
        return Err("not a kind of video AutoScene imports");
    }
    let meta = std::fs::symlink_metadata(p).map_err(|_| "not there")?;
    if !meta.file_type().is_file() {
        return Err("not a plain file");
    }
    if meta.len() == 0 {
        return Err("empty");
    }
    let real = std::fs::canonicalize(p).map_err(|_| "not there")?;
    if !real.starts_with(out) {
        return Err("outside the run's outDir");
    }
    if real.starts_with(part) {
        return Err("in the run's partDir, so not finished");
    }
    Ok(())
}

/// Whether `path`, as written, is under `out`, also as written: compared by
/// components (case-blind on Windows), before any file is read.
fn lexically_inside(path: &Path, out: &Path) -> bool {
    use std::path::Component;
    let parts = |p: &Path| -> Option<Vec<String>> {
        let mut v = Vec::new();
        for c in p.components() {
            match c {
                Component::ParentDir => return None,
                Component::CurDir => {}
                c => {
                    let s = c.as_os_str().to_string_lossy().replace('/', std::path::MAIN_SEPARATOR_STR);
                    v.push(if cfg!(windows) { s.to_lowercase() } else { s });
                }
            }
        }
        Some(v)
    };
    match (parts(path), parts(out)) {
        (Some(p), Some(o)) => p.len() > o.len() && p.starts_with(&o),
        _ => false,
    }
}

/// A run's folder for unfinished files, in its `outDir`: removed as it
/// drops, whatever ended the run; a note says where for a crash.
struct PartDir {
    path: PathBuf,
    note: PathBuf,
}

impl PartDir {
    /// `<out>/.autoscene-part-<the last 8 of the job id>`, noted in `notes`.
    fn make(out: &Path, job: &str, notes: &Path) -> io::Result<Self> {
        let tail = &job[job.len().saturating_sub(8)..];
        let path = out.join(format!("{PART_PREFIX}{tail}"));
        if path.exists() {
            remove_part_dir(&path)?;
        }
        std::fs::create_dir(&path)?;
        let note = notes.join(format!("{NOTE_PREFIX}{job}"));
        let noted =
            std::fs::create_dir_all(notes).and_then(|()| std::fs::write(&note, path.to_string_lossy().as_bytes()));
        if let Err(e) = noted {
            log::warn!("noting a source's unfinished files: {e}");
        }
        Ok(Self { path, note })
    }
}

impl Drop for PartDir {
    fn drop(&mut self) {
        match remove_part_dir(&self.path) {
            Ok(()) => {
                let _ = std::fs::remove_file(&self.note);
            }
            // What can't go now has its note for the next start.
            Err(e) => log::warn!("a source's unfinished files: {e}"),
        }
    }
}

/// Whether `path` is named as [`PartDir::make`] names one.
fn is_part_dir(path: &Path) -> bool {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    name.strip_prefix(PART_PREFIX).is_some_and(|tail| tail.len() == 8)
}

/// Removes the folder `path`, a run's for unfinished files, with what is in
/// it; never what a link in it points to.
fn remove_part_dir(path: &Path) -> io::Result<()> {
    if !is_part_dir(path) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a source's folder for unfinished files"));
    }
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
        Ok(m) if m.file_type().is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
    }
}

/// At the start, before any run can note one: removes the folders that runs
/// noted in `notes` left, as AutoScene crashed. Only a source's notes; a
/// pack action's stay for its own clean-up.
pub fn remove_left_parts(notes: &Path) {
    let Ok(entries) = std::fs::read_dir(notes) else { return };
    for note in entries.flatten() {
        if !note.file_name().to_string_lossy().starts_with(NOTE_PREFIX) {
            continue;
        }
        if let Ok(dir) = std::fs::read_to_string(note.path()).map(PathBuf::from)
            && let Err(e) = remove_part_dir(&dir)
            && e.kind() != io::ErrorKind::InvalidInput
        {
            // Tried again at the next start.
            log::warn!("a source's unfinished files: {e}");
            continue;
        }
        let _ = std::fs::remove_file(note.path());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yt() -> Offered {
        Offered {
            plugin: PluginId::parse("dev.autoscene.reel-in").unwrap(),
            name: "Reel In".into(),
            source: as_plugin::SourceSpec {
                id: "links".into(),
                title: "YouTube and TikTok".into(),
                description: None,
                link_hosts: vec!["youtube.com".into(), "youtu.be".into(), "tiktok.com".into()],
                settings: vec![],
            },
        }
    }

    fn text(m: &Option<Msg>) -> String {
        m.as_ref().map(|m| m.text.clone()).unwrap_or_default()
    }

    #[test]
    fn pasted_links_are_cleaned_and_routed() {
        let pasted = "https://www.youtube.com/watch?v=abc#t=10  youtu.be/xyz, http://VM.TikTok.com/ZMabc/\n\
                      https://www.youtube.com/watch?v=abc https://vimeo.com/1 notalink ftp://youtube.com/x \
                      https://user:pw@youtube.com/x https://10.0.0.1/x https://youtube.com:8443/x https://fakeyoutube.com/x";
        let got = parse_links(pasted, &[yt()]);
        let links: Vec<_> = got.iter().map(|i| (i.link.as_str(), text(&i.refused))).collect();
        assert_eq!(
            links,
            [
                ("https://www.youtube.com/watch?v=abc", String::new()),
                ("https://youtu.be/xyz", String::new()),
                ("https://vm.tiktok.com/ZMabc/", String::new()),
                ("https://vimeo.com/1", "No plugin takes links from vimeo.com.".into()),
                ("", "That isn't a web link.".into()),
                ("", "That isn't a web link.".into()),
                ("", "That isn't a web link.".into()),
                ("", "That isn't a web link.".into()),
                ("", "That isn't a web link.".into()),
                ("https://fakeyoutube.com/x", "No plugin takes links from fakeyoutube.com.".into()),
            ]
        );
        assert_eq!(got[0].plugin_id.as_deref(), Some("dev.autoscene.reel-in"));
        assert_eq!(got[0].short, "youtube.com/watch?v=abc");
        // Without a source, every link is refused.
        assert!(parse_links("youtu.be/x", &[]).iter().all(|i| i.refused.is_some()));
        // At most 100 at once.
        let many: String = (0..101).map(|i| format!("youtu.be/{i} ")).collect();
        let got = parse_links(&many, &[yt()]);
        assert_eq!(got.iter().filter(|i| i.refused.is_some()).count(), 1);
        assert!(text(&got[100].refused).contains("At most 100"));
    }

    use std::time::{Duration, Instant};

    use as_api::{AppSettings, ContentKind, JobInfo, JobState, NewProject, ProjectKind};
    use as_plugin::{PluginError, RunMessage};
    use serde_json::json;

    use crate::plugins::tests::{folder, host_in};

    /// A plugin with a source of YouTube links.
    fn manifest() -> serde_json::Value {
        json!({
            "manifestVersion": 2,
            "id": "dev.a.reel",
            "version": "1.0.0",
            "name": "Reel",
            "description": "Fetches videos by their links.",
            "author": {"name": "Someone"},
            "engines": {"autoscene": ">=1.0.0, <2", "protocol": 2},
            "runtime": {"type": "process", "command": {"*": ["uv", "run", "--script", "--locked", "main.py"]}},
            "permissions": ["network"],
            "contributes": {"sources": [{"id": "links", "title": "YouTube", "linkHosts": ["youtube.com"]}]}
        })
    }

    /// How a stand-in run ends, given its run line and its file sink.
    type Answer = Box<dyn Fn(&RunMessage, &mut FileSink) -> as_plugin::PluginResult<Option<String>> + Send + Sync>;

    struct Bench {
        sv: Sourcing,
        project: ProjectId,
        added: Arc<Mutex<Vec<String>>>,
        tmp: PathBuf,
    }

    impl Drop for Bench {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.tmp);
        }
    }

    impl Bench {
        fn new(name: &str, answer: Answer) -> Self {
            let tmp = std::env::temp_dir().join(format!("autoscene-sources-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&tmp);
            let (lib, host) = host_in(&tmp);
            let lib = Arc::new(lib);
            lib.set_allows_unreviewed_plugins(true).unwrap();
            let settings =
                AppSettings { library_root: tmp.join("Library").display().to_string(), ..lib.settings().unwrap() };
            lib.update_settings(&settings).unwrap();
            host.store.commit(folder(&host, &tmp, &manifest()), true, true).unwrap();
            let req = NewProject {
                title: "Sprite Fright".into(),
                kind: ProjectKind::Series,
                content: ContentKind::Animation,
                files: vec![],
            };
            let project = lib.create_project(&req).unwrap().0.id;
            let added = Arc::new(Mutex::new(Vec::new()));
            let keep = added.clone();
            let answer = Arc::new(answer);
            let sv = Sourcing {
                host,
                jobs: JobRunner::new(&[(Class::Download, 1)], |_| {}),
                lib,
                tools: Arc::new(|| Ok(("ffmpeg".into(), "ffprobe".into(), "9.0".into()))),
                run: Arc::new(move |mut spec, cancel, _| {
                    let mut sink = spec.files.take().expect("a source's run has a file sink");
                    let end = answer(&spec.message, &mut sink);
                    if cancel.is_cancelled() { Err(PluginError::Cancelled) } else { end }
                }),
                add: Arc::new(move |_, file| {
                    let mut all = keep.lock();
                    let new = !all.iter().any(|f| f == file);
                    all.push(file.to_owned());
                    Ok(new)
                }),
            };
            Self { sv, project, added, tmp }
        }

        fn jobs(&self) -> Vec<JobInfo> {
            self.sv.jobs.jobs().into_iter().filter(|j| j.kind == JobKind::Download).collect()
        }

        fn settle(&self) {
            let start = Instant::now();
            while self.sv.jobs.jobs().iter().any(|j| !j.state.is_finished()) {
                assert!(start.elapsed() < Duration::from_secs(20), "the runs never ended");
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    /// Writes `name` in the run's outDir (and a part in its partDir), and
    /// names it, then a file outside it.
    fn fetches(name: &'static str) -> Answer {
        Box::new(move |m, sink| {
            let src = m.source.as_ref().unwrap();
            assert!(Path::new(&src.part_dir).is_dir(), "its partDir is made");
            std::fs::write(Path::new(&src.part_dir).join("half.part"), b"x").unwrap();
            let file = Path::new(&src.out_dir).join(name);
            std::fs::write(&file, b"video").unwrap();
            sink(&file.to_string_lossy(), Some("Sprite Fright"));
            sink(&Path::new(&src.out_dir).join("..").join("x.mkv").to_string_lossy(), None);
            Ok(Some("Fetched.".into()))
        })
    }

    #[test]
    fn links_become_runs_and_their_videos_join_the_project() {
        let b = Bench::new("join", fetches("Sprite Fright [x].mkv"));
        let link = "https://www.youtube.com/watch?v=x".to_owned();
        let q = queue_links(&b.sv, b.project, &[link.clone(), "https://vimeo.com/1".into()]).unwrap();
        assert_eq!((q.queued, q.waiting, q.refused.len()), (1, 0, 1));
        assert!(q.out_dir.ends_with("Sprite Fright"), "{}", q.out_dir);
        // The same link again, while it waits or runs, waits once.
        let again = queue_links(&b.sv, b.project, std::slice::from_ref(&link)).unwrap();
        assert_eq!(again.queued + again.waiting, 1);
        b.settle();
        let jobs = b.jobs();
        assert_eq!(jobs[0].state, JobState::Done, "{:?}", jobs[0].error);
        assert_eq!(jobs[0].title.text, "Downloading youtube.com/watch?v=x");
        assert_eq!(jobs[0].message, None, "a download that ends well says nothing");
        // Only the finished file inside outDir was added.
        let added = b.added.lock().clone();
        assert_eq!(added.len(), 1, "{added:?}");
        assert!(added[0].ends_with("Sprite Fright [x].mkv"));
        // Its partDir went, and the link isn't kept for the next start.
        let out = PathBuf::from(&q.out_dir);
        let left: Vec<_> = std::fs::read_dir(&out).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(left, ["Sprite Fright [x].mkv"]);
        assert!(pending(&b.sv.lib).is_empty());
    }

    #[test]
    fn a_run_that_adds_nothing_fails_and_a_quit_keeps_its_link() {
        let b = Bench::new("nothing", Box::new(|_, _| Ok(None)));
        queue_links(&b.sv, b.project, &["youtube.com/watch?v=y".into()]).unwrap();
        b.settle();
        let job = &b.jobs()[0];
        assert_eq!(job.state, JobState::Failed);
        assert_eq!(job.error.as_ref().unwrap().text, "Reel added no video from youtube.com/watch?v=y.");
        assert!(pending(&b.sv.lib).is_empty(), "a failed link isn't kept");

        // A quit stops a run: its link is kept, and queued again at the start.
        drop(b);
        let b = Bench::new(
            "quit",
            Box::new(|_, _| {
                std::thread::sleep(Duration::from_millis(300));
                Ok(None)
            }),
        );
        queue_links(&b.sv, b.project, &["youtube.com/watch?v=z".into()]).unwrap();
        assert_eq!(pending(&b.sv.lib).len(), 1, "kept while it waits");
        b.sv.host.set_quitting(true);
        let id = b.jobs()[0].id;
        b.sv.jobs.cancel(id);
        b.settle();
        b.sv.host.set_quitting(false);
        assert_eq!(pending(&b.sv.lib).len(), 1, "a quit keeps it");
        resume(&b.sv);
        b.settle();
        assert_eq!(b.jobs().len(), 2, "queued again");
    }

    #[test]
    fn a_waiting_download_the_person_stops_isnt_kept() {
        let b = Bench::new(
            "stop",
            Box::new(|_, _| {
                std::thread::sleep(Duration::from_millis(300));
                Ok(None)
            }),
        );
        let links = ["youtube.com/watch?v=a".to_owned(), "youtube.com/watch?v=b".to_owned()];
        queue_links(&b.sv, b.project, &links).unwrap();
        assert_eq!(pending(&b.sv.lib).len(), 2);
        // The second waits behind the first: stopped, it never runs.
        let waiting = b.jobs().into_iter().find(|j| j.state == JobState::Queued).unwrap().id;
        b.sv.jobs.cancel(waiting);
        b.settle();
        assert!(pending(&b.sv.lib).is_empty(), "{:?}", pending(&b.sv.lib));
    }

    #[test]
    fn a_path_elsewhere_is_refused_before_it_is_read() {
        let out = Path::new(if cfg!(windows) { r"D:\Library\Downloads\Show" } else { "/lib/Downloads/Show" });
        let inside = out.join("a.mkv");
        assert!(lexically_inside(&inside, out));
        assert!(!lexically_inside(out, out));
        assert!(!lexically_inside(&out.join("..").join("x.mkv"), out));
        assert!(!lexically_inside(Path::new(if cfg!(windows) { r"\evil\s.mkv" } else { "/evil/a.mkv" }), out));
        #[cfg(windows)]
        assert!(lexically_inside(Path::new(r"d:\library\downloads\show\A.mkv"), out));
    }

    #[test]
    fn short_links_fit() {
        assert_eq!(short_link("https://www.tiktok.com/@blender/video/1"), "tiktok.com/@blender/video/1");
        let long = format!("https://youtube.com/{}", "a".repeat(100));
        assert_eq!(short_link(&long).chars().count(), 60);
    }

    #[test]
    fn only_finished_videos_in_the_folder_are_taken() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("out");
        let part = out.join(".autoscene-part-1a2b3c4d");
        std::fs::create_dir_all(&part).unwrap();
        let (out_c, part_c) = (std::fs::canonicalize(&out).unwrap(), std::fs::canonicalize(&part).unwrap());
        let write = |p: &Path, bytes: &[u8]| {
            std::fs::write(p, bytes).unwrap();
            p.to_string_lossy().into_owned()
        };
        let good = write(&out.join("Sprite Fright [x].mkv"), b"video");
        assert_eq!(check_file(&good, &out, &out_c, &part_c), Ok(()));
        let unfinished = write(&part.join("a.mkv"), b"video");
        assert!(check_file(&unfinished, &out, &out_c, &part_c).is_err());
        let outside = write(&tmp.path().join("b.mkv"), b"video");
        assert!(check_file(&outside, &out, &out_c, &part_c).is_err());
        let climbing = out.join("..").join("b.mkv").to_string_lossy().into_owned();
        assert!(check_file(&climbing, &out, &out_c, &part_c).is_err());
        let empty = write(&out.join("c.mkv"), b"");
        assert!(check_file(&empty, &out, &out_c, &part_c).is_err());
        let text = write(&out.join("d.txt"), b"text");
        assert!(check_file(&text, &out, &out_c, &part_c).is_err());
        assert!(check_file("relative.mkv", &out, &out_c, &part_c).is_err());
        assert!(check_file(&out.join("gone.mkv").to_string_lossy(), &out, &out_c, &part_c).is_err());
        // A link out of the folder, to a video outside it, isn't taken.
        #[cfg(unix)]
        {
            let link = out.join("link.mkv");
            std::os::unix::fs::symlink(tmp.path().join("b.mkv"), &link).unwrap();
            assert!(check_file(&link.to_string_lossy(), &out, &out_c, &part_c).is_err());
        }
    }

    #[test]
    fn a_part_folder_goes_however_the_run_ends_and_after_a_crash() {
        let tmp = tempfile::tempdir().unwrap();
        let (out, notes) = (tmp.path().join("out"), tmp.path().join("notes"));
        std::fs::create_dir_all(&out).unwrap();
        let job = "0190aaaa-bbbb-7ccc-8ddd-eeee1a2b3c4d";
        let part = PartDir::make(&out, job, &notes).unwrap();
        let dir = part.path.clone();
        std::fs::write(dir.join("half.part"), b"x").unwrap();
        assert!(notes.join(format!("source-{job}")).is_file());
        drop(part);
        assert!(!dir.exists() && !notes.join(format!("source-{job}")).exists());
        assert!(out.is_dir(), "outDir stays");

        // A crash: the note is left, and the next start removes the folder.
        let part = PartDir::make(&out, job, &notes).unwrap();
        let dir = part.path.clone();
        std::mem::forget(part);
        // A pack action's note stays for its own clean-up.
        std::fs::write(notes.join("0190pack"), "elsewhere.part-1a2b3c4d").unwrap();
        remove_left_parts(&notes);
        assert!(!dir.exists());
        assert!(notes.join("0190pack").exists());
        // Never a folder it didn't make.
        let other = out.join("Keep");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(notes.join("source-x"), other.to_string_lossy().as_bytes()).unwrap();
        remove_left_parts(&notes);
        assert!(other.is_dir());
    }
}

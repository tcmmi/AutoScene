//! Plugins' pack actions at work: a character's scenepack saved as one file
//! by a plugin (an FCPXML, a cut sheet), before anything is rendered
//! ([`queue_pack_action`]).
//!
//! A click asks the person where first, in the system's save dialog, then
//! queues the run. A pack action of a character's pack is one run as
//! [`RunKey`] sees it (the plugin, the action, the project and the
//! character): clicked again while it waits or runs, it asks nothing and
//! queues nothing, while one of another character's pack is queued beside
//! it. As an export of the pack would be, it is refused while a shot filter
//! the pack turns on checks shots, and it takes the pack as the grid showed
//! it: the clips an export of it would cut, fixed at the click
//! (`Project::pack_cut`), or the export's own refusal when the pack changed
//! since.
//!
//! The run writes a file of its own beside the chosen one
//! (`<name>.part-<8 hex>`, its `output`). Only when the run ends well does
//! that file take the chosen name (replacing one the person agreed to
//! replace); when it fails, is stopped or panics, that file goes, and what
//! was at the chosen name stays. One a crash left goes at the next start. A
//! pack action never saves a program, a script or a shortcut: its
//! plugin.json can't name such a kind of file,
//! and the name the person types is checked too. What it saves is marked as
//! a download, as a page's `files.save` marks it, and runs as nothing on
//! Linux or macOS either. Its job then says "Saved Walter.fcpxml"; what the
//! plugin said goes to the log.
//!
//! A pack action of a plugin with `"gpu": true` waits for the analysis of
//! episodes, as each of its runs does: AutoScene can't tell whether its
//! program takes the graphics card as it starts.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use as_api::{PackNaming, PackSpec};
use as_core::{CharacterId, JobId, ProjectId, msg};
use as_jobs::{JobCtx, JobError, JobResult, JobRunner};
use as_plugin::calls::{FileFilter, runnable_name};
use as_plugin::protocol::{CharacterContext, EpisodeContext, PackClipContext, PackContext, ProjectContext};
use as_plugin::{ActionContext, ActionKind, HostCalls, PluginId, Trigger, runtime};
use as_project::naming::safe_component;
use as_project::{Library, PackCut, PluginFilters, ProjectResult};
use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Manager};

use super::calls::mark_downloaded;
use super::filters::{self, RunFn, ToolsFn};
use super::{PluginHost, RunKey, calls_for, job_spec, plugin_job_error, project_context, runnable, startable};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::paths;
use crate::state::AppState;
use crate::work::job_error;

/// What a click on a pack action did.
#[derive(Clone, Debug, PartialEq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PackActionStart {
    /// Its run waits in Activity as `job`; once it ends well, the file is
    /// at `path`.
    Queued { job: JobId, path: String },
    /// The same pack action of this pack waits or runs already: nothing
    /// was asked, and nothing queued.
    Waits,
    /// The person closed the save dialog: nothing runs.
    Cancelled,
}

/// A pack as the grid showed it, to be saved: its settings, how its clips
/// are named, and its stamp.
pub struct Shown<'a> {
    pub spec: &'a PackSpec,
    pub naming: &'a PackNaming,
    pub stamp: &'a str,
}

/// What a pack action reads of a project: the library's projects
/// (`as_project::Project`), or a test's stand-in.
pub trait Cuts: Send + Sync {
    /// `Project::pack_cut` of `c`'s pack in `project`, with the plugins'
    /// `filters`.
    fn cut(&self, project: ProjectId, c: CharacterId, shown: &Shown, filters: &PluginFilters)
    -> ProjectResult<PackCut>;
    /// The project as a run gets it.
    fn project(&self, id: ProjectId) -> ProjectResult<ProjectContext>;
    /// Where the project's exports go by default
    /// (`Library::default_export_dir`).
    fn export_dir(&self, id: ProjectId) -> ProjectResult<PathBuf>;
}

impl Cuts for Arc<Library> {
    fn cut(
        &self,
        project: ProjectId,
        c: CharacterId,
        shown: &Shown,
        filters: &PluginFilters,
    ) -> ProjectResult<PackCut> {
        Library::project(self, project)?.pack_cut(c, shown.spec, filters, shown.naming, shown.stamp)
    }

    fn project(&self, id: ProjectId) -> ProjectResult<ProjectContext> {
        let project = Library::project(self, id)?;
        project_context(&project)
    }

    fn export_dir(&self, id: ProjectId) -> ProjectResult<PathBuf> {
        self.default_export_dir(&self.summary(id)?)
    }
}

/// The system's save dialog, given its title, the name it offers, the kinds
/// of file and the folder it opens in: where the person chose, or none when
/// they closed it.
pub type AskFn = dyn Fn(&str, &str, &[FileFilter], Option<&Path>) -> Option<PathBuf> + Send + Sync;

/// The longest name, in bytes, a save dialog offers for a pack: the
/// character's name is cut to fit.
const MAX_OFFERED_NAME: usize = 200;

/// The longest name, in bytes, a pack action saves: its part
/// (`<name>.part-<8 hex>`) still fits the 255 of every common file system.
const MAX_CHOSEN_NAME: usize = 240;

/// What pack actions use of the app.
#[derive(Clone)]
pub struct Saving {
    pub host: Arc<PluginHost>,
    pub jobs: JobRunner,
    pub lib: Arc<Library>,
    pub packs: Arc<dyn Cuts>,
    pub tools: Arc<ToolsFn>,
    pub run: Arc<RunFn>,
    pub ask: Arc<AskFn>,
}

impl Saving {
    /// The app's, asking where with `ask`.
    pub fn of(app: &AppHandle, ask: Arc<AskFn>) -> AppResult<Self> {
        let state = app.state::<AppState>();
        let tools = app.clone();
        Ok(Self {
            host: super::host(&state)?,
            jobs: state.jobs.clone(),
            lib: state.library.clone(),
            packs: Arc::new(state.library.clone()),
            tools: Arc::new(move || {
                let t = tools.state::<AppState>().tools().map_err(|e| e.message)?;
                Ok((t.ffmpeg.clone(), t.ffprobe.clone(), t.version.clone()))
            }),
            run: Arc::new(|spec, cancel, report| runtime::run(spec, cancel, report)),
            ask,
        })
    }
}

/// Saves `c`'s pack in `project`, as the grid showed it (`shown`), with the
/// pack action `key` (`<pluginId>/<actionId>`): asks the person where first,
/// then queues its run. Refused while its plugin isn't active, while a
/// filter the pack turns on checks shots, and when the pack changed since
/// it was shown or takes nothing, as an export of it would be.
pub fn queue_pack_action(
    sv: &Saving,
    project: ProjectId,
    c: CharacterId,
    key: &str,
    shown: &Shown,
) -> AppResult<PackActionStart> {
    let unknown =
        || AppError::new(ErrorCode::Invalid, msg!("plugin.notAPackAction", "That isn't a plugin's pack action."));
    let (plugin, action) = key.split_once('/').ok_or_else(unknown)?;
    let plugin = PluginId::parse(plugin).map_err(|_| unknown())?;
    let p = sv.host.store.get(&plugin)?;
    let allow = sv.lib.allows_unreviewed_plugins()?;
    let m = runnable(&p, allow)?;
    let Some(a) = m.pack_actions.iter().find(|a| a.id == action) else {
        let gone = msg!("plugin.noSuchPackAction", "{{name}} has no such pack action.", name = &m.name);
        return Err(AppError::new(ErrorCode::NotFound, gone));
    };
    let run = RunKey {
        character_id: Some(c),
        ..RunKey::new(&plugin, ActionKind::PackAction, &a.id, Some(project), None, None)
    };
    if sv.host.is_queued(&sv.jobs, &run) {
        return Ok(PackActionStart::Waits);
    }
    // As an export: not while the pack still changes, and only as it was shown.
    let filters = filters::pack_filters(Some(&sv.host), &sv.jobs, allow, project, c, shown.spec);
    if let Some(why) = filters::export_waits(&filters, shown.spec) {
        return Err(AppError::new(ErrorCode::Invalid, why));
    }
    let cut = sv.packs.cut(project, c, shown, &filters)?;
    let name = offered_name(&a.file.name, &cut.name);
    let kinds = FileFilter {
        name: a.file.extensions.iter().map(|e| e.to_uppercase()).collect::<Vec<_>>().join(", "),
        extensions: a.file.extensions.clone(),
    };
    let asked = format!("Plugin “{}” saves {}'s scenepack", m.name, cut.name);
    // Where its exports go, or the nearest folder above that is there yet.
    let exports = sv.packs.export_dir(project).ok();
    let folder = exports.as_deref().and_then(|d| d.ancestors().take(3).find(|d| d.is_dir()));
    let Some(path) = (sv.ask)(&asked, &name, &[kinds], folder) else { return Ok(PackActionStart::Cancelled) };
    paths::check(&path)?;
    // The name the person typed, or the one the dialog made of it: its
    // plugin.json names no kind that runs, but the person may type one.
    let chosen = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if chosen.is_empty() || runnable_name(&chosen) {
        return Err(runnable_refused());
    }
    if chosen.len() > MAX_CHOSEN_NAME {
        let refused = msg!("plugin.saveNameTooLong", "That file name is too long. Choose a shorter one.");
        return Err(AppError::new(ErrorCode::Invalid, refused));
    }
    // Named by the character too: Walter's pack and Jesse's each have their run.
    let title = format!("{} · {}", a.title.trim_end_matches(['…', '.', ' ']), cut.name);
    // A GPU plugin's waits for the analysis, as its other runs do.
    let job = job_spec(m, &title, Some(project), None);
    let (pack, gpu) = (pack_context(&cut), m.runtime.gpu);
    let work = {
        let (sv, run, path) = (sv.clone(), run.clone(), path.clone());
        move |ctx: &JobCtx| save(&sv, ctx, &run, &path, pack, gpu)
    };
    Ok(match sv.host.submit(&sv.jobs, run, job, work) {
        Some(job) => PackActionStart::Queued { job, path: path.to_string_lossy().into_owned() },
        // Another click queued it meanwhile.
        None => PackActionStart::Waits,
    })
}

fn runnable_refused() -> AppError {
    AppError::new(
        ErrorCode::Invalid,
        msg!("plugin.saveRunnable", "A plugin can't save a program, a script or a shortcut."),
    )
}

/// The name a save dialog offers for `character`'s pack: `template` with
/// their name for each `{character}`, cut to fit [`MAX_OFFERED_NAME`].
fn offered_name(template: &str, character: &str) -> String {
    const FIELD: &str = "{character}";
    let fields = template.matches(FIELD).count();
    let mut name = safe_component(character, " ", 80);
    // A plugin.json's name has up to 100 bytes: each field has room.
    let room = MAX_OFFERED_NAME.saturating_sub(template.len() - fields * FIELD.len()).checked_div(fields);
    if let Some(room) = room
        && name.len() > room
    {
        let mut end = room;
        while !name.is_char_boundary(end) {
            end -= 1;
        }
        // Cut, it may end in a dot or a space again.
        name = safe_component(&name[..end], " ", 80);
    }
    template.replace(FIELD, &name)
}

/// `cut` as a run gets it.
fn pack_context(cut: &PackCut) -> PackContext {
    PackContext {
        character: CharacterContext { id: cut.character.to_string(), name: cut.name.clone() },
        episodes: cut
            .episodes
            .iter()
            .map(|e| EpisodeContext {
                media_id: e.media.to_string(),
                path: e.path.clone(),
                fps: e.fps.map(|fps| fps.to_string()),
                frames: e.frames.map(u64::from),
            })
            .collect(),
        clips: cut
            .clips
            .iter()
            .zip(0..)
            .map(|(clip, index)| PackClipContext {
                index,
                name: clip.name.clone().unwrap_or_default(),
                media_id: clip.media_id.to_string(),
                start: clip.start.into(),
                end: clip.end.into(),
            })
            .collect(),
    }
}

/// The queued pack action `key`, which saves `pack` to `path`, now it
/// starts (`gpu`: it waited as analysis).
fn save(sv: &Saving, ctx: &JobCtx, key: &RunKey, path: &Path, pack: PackContext, gpu: bool) -> JobResult {
    let _running = sv.host.hold(ctx, &key.plugin)?;
    let p = startable(&sv.host.store, &sv.lib, key, Trigger::Menu, gpu)?;
    let (ffmpeg, ffprobe, version) = (sv.tools)().map_err(JobError::Failed)?;
    let action = ActionContext { kind: ActionKind::PackAction, id: key.action.clone(), trigger: Trigger::Menu };
    // Through AutoScene's uv and with the plugin's large files, got first
    // when they aren't kept, as any run of it is.
    let report = |stage: &str, f: Option<f64>| ctx.report(stage, f);
    let tools = (ffmpeg.as_path(), ffprobe.as_path(), version.as_str());
    let mut run = sv.host.run_spec(&p, action, tools, &ctx.cancel, &report).map_err(plugin_job_error)?;
    let project =
        key.project_id.ok_or_else(|| JobError::Failed("A pack action saves a character's scenepack.".into()))?;
    run.message.project = Some(sv.packs.project(project).map_err(job_error)?);
    run.message.pack = Some(pack);
    // Gone however the run ends, but well.
    let part = Part::beside(path, ctx.id, sv.host.store.parts_dir());
    run.message.output = Some(part.path().to_string_lossy().into_owned());
    let calls = calls_for(&sv.lib, &sv.host, &p, key, run.protocol).map_err(plugin_job_error)?;
    run.calls = calls.map(|c| Arc::new(c) as Arc<dyn HostCalls>);
    let (file, name) = (paths::name(path).into_owned(), run.name.clone());
    ctx.stage(&format!("Saving {file}"));
    let said = (sv.run)(run, &ctx.cancel, &report).map_err(plugin_job_error)?;
    if let Some(message) = said {
        log::info!("{} {} saved {file}: {message}", key.plugin, key.action);
    }
    part.keep(path, &name, &file).map_err(JobError::Failed)?;
    ctx.message(&format!("Saved {file}"));
    Ok(())
}

/// The file a pack action's run writes, beside the one the person chose: it
/// takes that one's place once the run ends well ([`Part::keep`]), and goes
/// otherwise, also when the run panics. A crash ends it with neither: a note
/// in the app's data folder says where it is, for [`remove_left_parts`].
struct Part {
    path: Option<PathBuf>,
    note: PathBuf,
}

impl Part {
    /// Beside `path`, named after it and the run's job `job` (the random
    /// end of its id), so two runs never write one file. Noted in `notes`:
    /// a note that can't be written costs only that clean-up.
    fn beside(path: &Path, job: JobId, notes: &Path) -> Self {
        let id = job.to_string();
        let tail = &id[id.len().saturating_sub(8)..];
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let part = path.with_file_name(format!("{name}.part-{tail}"));
        let note = notes.join(&id);
        let noted =
            std::fs::create_dir_all(notes).and_then(|()| std::fs::write(&note, part.to_string_lossy().as_bytes()));
        if let Err(e) = noted {
            log::warn!("noting a pack action's unfinished {}: {e}", paths::name(&part));
        }
        Self { path: Some(part), note }
    }

    fn path(&self) -> &Path {
        self.path.as_deref().expect("a part is kept once, and then it is done")
    }

    /// Puts what the plugin `plugin` wrote in the place of `path` (`file`):
    /// a file, never a folder or a link to one, marked as a download, and
    /// one that doesn't run on Linux or macOS either.
    fn keep(mut self, path: &Path, plugin: &str, file: &str) -> Result<(), String> {
        let part = self.path().to_path_buf();
        let unsaved = |e: io::Error| format!("AutoScene couldn't save {file}: {e}");
        match std::fs::symlink_metadata(&part) {
            Ok(m) if m.is_file() => not_runnable(&part, &m).map_err(unsaved)?,
            Ok(_) => return Err(format!("{plugin} didn't save {file} as a file.")),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(format!("{plugin} didn't save {file}.")),
            Err(e) => return Err(format!("AutoScene couldn't read what {plugin} saved as {file}: {e}")),
        }
        mark_downloaded(&part);
        std::fs::rename(&part, path).map_err(unsaved)?;
        self.path = None;
        Ok(())
    }
}

/// Takes the bits that let the file at `path` (`meta`) run on Linux and
/// macOS, which its plugin may have set.
#[cfg(unix)]
fn not_runnable(path: &Path, meta: &std::fs::Metadata) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = meta.permissions().mode();
    match mode & 0o111 {
        0 => Ok(()),
        _ => std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode & !0o111)),
    }
}

/// Nothing runs a file on Windows for a bit it has: [`runnable_name`] and
/// the download mark keep out what would.
#[cfg(not(unix))]
fn not_runnable(_: &Path, _: &std::fs::Metadata) -> io::Result<()> {
    Ok(())
}

impl Drop for Part {
    fn drop(&mut self) {
        // What can't go now has its note for the next start.
        if let Some(part) = self.path.take()
            && let Err(e) = remove_part(&part)
        {
            log::warn!("a pack action's unfinished {}: {e}", paths::name(&part));
            return;
        }
        let _ = std::fs::remove_file(&self.note);
    }
}

/// Removes the unfinished file `part`, and never more than it: a file, a
/// link, or a folder only while empty.
fn remove_part(part: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(part) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
        Ok(m) if m.is_dir() => std::fs::remove_dir(part),
        Ok(_) => std::fs::remove_file(part).or_else(|_| std::fs::remove_dir(part)),
    }
}

/// Whether `path` is named as [`Part::beside`] names one: a note is taken at
/// its word for nothing else in the person's folders.
fn is_part(path: &Path) -> bool {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    name.rsplit_once(".part-").is_some_and(|(_, tail)| tail.len() == 8 && tail.bytes().all(|c| c.is_ascii_hexdigit()))
}

/// At the start, before any run can note one: removes what the runs noted in
/// `notes` left, as AutoScene crashed or was killed during a save. One try:
/// what can't go then stays.
pub fn remove_left_parts(notes: &Path) {
    let Ok(entries) = std::fs::read_dir(notes) else { return };
    for note in entries.flatten() {
        if let Ok(part) = std::fs::read_to_string(note.path()).map(PathBuf::from)
            && is_part(&part)
            && let Err(e) = remove_part(&part)
        {
            log::warn!("a pack action's unfinished {}: {e}", paths::name(&part));
        }
        if let Err(e) = std::fs::remove_file(note.path()) {
            log::warn!("a pack action's note {}: {e}", paths::name(&note.path()));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    use as_api::{ClipRequest, JobInfo, JobKind, JobState};
    use as_core::{MediaId, Rational};
    use as_jobs::{Class, JobSpec};
    use as_plugin::{PluginError, PluginResult, RunMessage};
    use as_project::{CutEpisode, ProjectError};
    use parking_lot::Mutex;
    use serde_json::json;

    use super::*;
    use crate::plugins::tests::{TEST_UV, folder, host_in};

    const FCPXML: &str = "dev.a.timeline/fcpxml";

    /// A plugin that saves a pack as an FCPXML.
    fn manifest() -> serde_json::Value {
        json!({
            "manifestVersion": 2,
            "id": "dev.a.timeline",
            "version": "1.0.0",
            "name": "Timeline",
            "description": "Saves a scenepack as a timeline.",
            "author": {"name": "Someone"},
            "engines": {"autoscene": ">=1.0.0, <2", "protocol": 2},
            "runtime": {"type": "process", "command": {"*": ["uv", "run", "--script", "--locked", "main.py"]}},
            "permissions": ["project:read"],
            "contributes": {"packActions": [
                {"id": "fcpxml", "title": "Save as FCPXML…", "file": {"name": "{character}.fcpxml", "extensions": ["fcpxml"]}}
            ]}
        })
    }

    /// A plugin with the shot filter "Speaks".
    fn speech() -> serde_json::Value {
        let mut m = manifest();
        m["id"] = json!("dev.a.speech");
        m["name"] = json!("Speech Filter");
        m["contributes"] = json!({"shotFilters": [{"id": "speaks", "title": "Speaks", "reason": "No speech"}]});
        m
    }

    /// Walter's pack, two clips of one episode, as the grid showed it with
    /// the stamp `s1`; any other stamp, or while `changed`, it changed since.
    /// Its exports go to `exports`.
    struct Fake {
        media: MediaId,
        changed: AtomicBool,
        exports: PathBuf,
    }

    impl Cuts for Fake {
        fn cut(&self, _: ProjectId, c: CharacterId, shown: &Shown, _: &PluginFilters) -> ProjectResult<PackCut> {
            if shown.stamp != "s1" || self.changed.load(Ordering::SeqCst) {
                return Err(ProjectError::Invalid("The pack changed meanwhile. Check it and export again.".into()));
            }
            let clip =
                |start, end, name: &str| ClipRequest { media_id: self.media, start, end, name: Some(name.into()) };
            Ok(PackCut {
                character: c,
                name: "Walter White".into(),
                episodes: vec![CutEpisode {
                    media: self.media,
                    path: "/media/S01E01.mkv".into(),
                    fps: Some(Rational::new(24000, 1001)),
                    frames: Some(83136),
                }],
                clips: vec![
                    clip(1188, 1462, "Walter White - S01E01 - 0001"),
                    clip(2000, 2100, "Walter White - S01E01 - 0002"),
                ],
            })
        }

        fn project(&self, id: ProjectId) -> ProjectResult<ProjectContext> {
            Ok(ProjectContext { id: id.to_string(), title: "Show".into(), folder: "/show".into() })
        }

        fn export_dir(&self, _: ProjectId) -> ProjectResult<PathBuf> {
            Ok(self.exports.clone())
        }
    }

    /// How a stand-in run ends, given its run line: it may write its output
    /// first.
    type Answer = Box<dyn Fn(&RunMessage) -> PluginResult<Option<String>> + Send + Sync>;

    /// A folder that goes when this does.
    struct Gone(PathBuf);

    impl Drop for Gone {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A dialog shown: its title, the name it offered, the kinds of file,
    /// the folder it opened in.
    type Asked = (String, String, Vec<FileFilter>, Option<PathBuf>);

    /// Pack actions with the stand-ins: a pack of Walter's, plugin runs that
    /// `answer`, one at a time, and a save dialog that answers `choice`.
    struct Bench {
        sv: Saving,
        fake: Arc<Fake>,
        /// Each run line the stand-in got.
        ran: Arc<Mutex<Vec<RunMessage>>>,
        asked: Arc<Mutex<Vec<Asked>>>,
        /// Where the dialog answers: none is the person closing it.
        choice: Arc<Mutex<Option<PathBuf>>>,
        project: ProjectId,
        walter: CharacterId,
        /// The folder the person saves to.
        cuts: PathBuf,
        /// Last: its folder goes once the rest let go of its files.
        _tmp: Gone,
    }

    impl Bench {
        fn new(name: &str, answer: Answer) -> Self {
            let tmp = std::env::temp_dir().join(format!("autoscene-pack-actions-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&tmp);
            let (lib, host) = host_in(&tmp);
            let lib = Arc::new(lib);
            lib.set_allows_unreviewed_plugins(true).unwrap();
            host.store.commit(folder(&host, &tmp, &manifest()), true, true).unwrap();
            let cuts = tmp.join("Cuts");
            std::fs::create_dir_all(&cuts).unwrap();
            // The project's exports go to a folder not made yet, in the one the person saves to.
            let exports = cuts.join("Show");
            let fake = Arc::new(Fake { media: MediaId::new(), changed: AtomicBool::new(false), exports });
            let (ran, asked) = (Arc::new(Mutex::new(Vec::new())), Arc::new(Mutex::new(Vec::new())));
            let choice = Arc::new(Mutex::new(Some(cuts.join("Walter White.fcpxml"))));
            let (log, shown, answers) = (ran.clone(), asked.clone(), choice.clone());
            let sv = Saving {
                host,
                jobs: JobRunner::new(&[(Class::Plugin, 1)], |_| {}),
                lib,
                packs: fake.clone(),
                tools: Arc::new(|| Ok(("ffmpeg".into(), "ffprobe".into(), "9.0".into()))),
                run: Arc::new(move |spec, cancel, _| {
                    assert_eq!(spec.uv.as_deref(), Some(Path::new(TEST_UV)), "it is a uv script");
                    log.lock().push(spec.message.clone());
                    let end = answer(&spec.message);
                    if cancel.is_cancelled() { Err(PluginError::Cancelled) } else { end }
                }),
                ask: Arc::new(move |title: &str, name: &str, kinds: &[FileFilter], folder: Option<&Path>| {
                    shown.lock().push((
                        title.to_owned(),
                        name.to_owned(),
                        kinds.to_vec(),
                        folder.map(Path::to_path_buf),
                    ));
                    answers.lock().clone()
                }),
            };
            let (project, walter) = (ProjectId::new(), CharacterId::new());
            Self { sv, fake, ran, asked, choice, project, walter, cuts, _tmp: Gone(tmp) }
        }

        /// A click on the pack action `key` of `c`'s pack shown with `spec`
        /// and the stamp `stamp`.
        fn click_as(&self, c: CharacterId, key: &str, spec: &PackSpec, stamp: &str) -> AppResult<PackActionStart> {
            let naming = PackNaming::default();
            queue_pack_action(&self.sv, self.project, c, key, &Shown { spec, naming: &naming, stamp })
        }

        /// A click on "Save as FCPXML…" of `c`'s pack as the grid showed it.
        fn click(&self, c: CharacterId) -> PackActionStart {
            self.click_as(c, FCPXML, &PackSpec::default(), "s1").unwrap()
        }

        fn jobs(&self) -> Vec<JobInfo> {
            self.sv.jobs.jobs().into_iter().filter(|j| j.kind == JobKind::Plugin).collect()
        }

        /// Waits until no job waits or runs.
        fn settle(&self) {
            let start = Instant::now();
            while self.sv.jobs.jobs().iter().any(|j| !j.state.is_finished()) {
                assert!(start.elapsed() < Duration::from_secs(20), "the runs never ended");
                thread::sleep(Duration::from_millis(5));
            }
        }

        /// The names in the folder the person saves to, sorted.
        fn files(&self) -> Vec<String> {
            let mut names: Vec<String> = std::fs::read_dir(&self.cuts)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        }

        fn chosen(&self) -> PathBuf {
            self.cuts.join("Walter White.fcpxml")
        }

        /// How many unfinished files are noted for the next start.
        fn notes(&self) -> usize {
            std::fs::read_dir(self.sv.host.store.parts_dir()).map_or(0, Iterator::count)
        }
    }

    /// Writes "<timeline>" to the file it is given, and ends well.
    fn writes() -> Answer {
        Box::new(|m| {
            std::fs::write(m.output.as_deref().expect("a pack action's run has an output"), "<timeline>").unwrap();
            Ok(Some("Wrote 2 clips.".into()))
        })
    }

    /// A stand-in whose runs wait until let go, then write and end well.
    fn held() -> (mpsc::Sender<()>, Answer) {
        let (go, wait) = mpsc::channel::<()>();
        let wait = Mutex::new(wait);
        let write = writes();
        let answer: Answer = Box::new(move |m| {
            let _ = wait.lock().recv_timeout(Duration::from_secs(10));
            write(m)
        });
        (go, answer)
    }

    #[test]
    fn the_file_appears_only_when_the_run_succeeds() {
        let b = Bench::new("saves", writes());
        let PackActionStart::Queued { job, path } = b.click(b.walter) else { panic!("not queued") };
        assert_eq!(PathBuf::from(&path), b.chosen());
        b.settle();
        // The file, and nothing beside it, marked as a download as a page's files.save marks it.
        assert_eq!(std::fs::read_to_string(b.chosen()).unwrap(), "<timeline>");
        assert_eq!(b.files(), ["Walter White.fcpxml"]);
        assert_eq!(b.notes(), 0, "the next start has nothing to do");
        if cfg!(windows) {
            let zone = std::fs::read_to_string(format!("{}:Zone.Identifier", b.chosen().display())).unwrap();
            assert!(zone.contains("ZoneId=3"), "{zone}");
        }
        let run = b.jobs().into_iter().find(|j| j.id == job).unwrap();
        assert_eq!(
            (run.state, run.title.as_str(), run.message.as_deref()),
            (JobState::Done, "Save as FCPXML · Walter White", Some("Saved Walter White.fcpxml"))
        );
        // The person was asked where, with the name and kind of file the plugin names, in the
        // folder the project's exports go to, or the nearest one above it that is there.
        let fcpxml = FileFilter { name: "FCPXML".into(), extensions: vec!["fcpxml".into()] };
        assert_eq!(
            b.asked.lock().clone(),
            [(
                String::from("Plugin “Timeline” saves Walter White's scenepack"),
                String::from("Walter White.fcpxml"),
                vec![fcpxml],
                Some(b.cuts.clone())
            )]
        );
        // The run got the pack as the grid showed it, and a file of its own beside the chosen one.
        let m = b.ran.lock()[0].clone();
        assert_eq!(
            (m.action.kind, m.action.id.as_str(), m.action.trigger),
            (ActionKind::PackAction, "fcpxml", Trigger::Menu)
        );
        assert_eq!(m.project.as_ref().map(|p| p.id.clone()), Some(b.project.to_string()));
        let output = PathBuf::from(m.output.clone().unwrap());
        assert_eq!(output.parent(), Some(b.cuts.as_path()));
        let part = output.file_name().unwrap().to_string_lossy().into_owned();
        let tail = part.strip_prefix("Walter White.fcpxml.part-").unwrap();
        assert!(tail.len() == 8 && tail.bytes().all(|c| c.is_ascii_hexdigit()), "{part}");
        assert!(job.to_string().ends_with(tail));
        let pack = serde_json::to_value(m.pack.unwrap()).unwrap();
        let media = b.fake.media.to_string();
        assert_eq!(
            pack,
            json!({
                "character": {"id": b.walter.to_string(), "name": "Walter White"},
                "episodes": [{"mediaId": media, "path": "/media/S01E01.mkv", "fps": "24000/1001", "frames": 83136}],
                "clips": [
                    {"index": 0, "name": "Walter White - S01E01 - 0001", "mediaId": media, "start": 1188, "end": 1462},
                    {"index": 1, "name": "Walter White - S01E01 - 0002", "mediaId": media, "start": 2000, "end": 2100}
                ]
            })
        );
        assert!(m.shots.is_none() && m.export.is_none() && m.input.is_none());

        // Saved again: it takes the place of the file there, as the person agreed in the dialog.
        std::fs::write(b.chosen(), "old").unwrap();
        assert!(matches!(b.click(b.walter), PackActionStart::Queued { .. }));
        b.settle();
        assert_eq!(std::fs::read_to_string(b.chosen()).unwrap(), "<timeline>");

        // One that fails after writing: its file goes, and what was there stays.
        let fails = Bench::new(
            "fails",
            Box::new(|m| {
                std::fs::write(m.output.as_deref().unwrap(), "<half>").unwrap();
                Err(PluginError::Failed { message: "The clips don't fit a timeline.".into(), detail: None })
            }),
        );
        std::fs::write(fails.chosen(), "old").unwrap();
        assert!(matches!(fails.click(fails.walter), PackActionStart::Queued { .. }));
        fails.settle();
        assert_eq!(
            (fails.files(), std::fs::read_to_string(fails.chosen()).unwrap()),
            (vec!["Walter White.fcpxml".into()], "old".into())
        );
        let run = fails.jobs().remove(0);
        assert_eq!((run.state, run.error.as_deref()), (JobState::Failed, Some("The clips don't fit a timeline.")));

        // One that says it's done but wrote nothing, or a folder: no file, and it says so.
        let mute = Bench::new("mute", Box::new(|_| Ok(None)));
        mute.click(mute.walter);
        mute.settle();
        assert_eq!(mute.files(), Vec::<String>::new());
        assert_eq!(mute.jobs()[0].error.as_deref(), Some("Timeline didn't save Walter White.fcpxml."));
        let folder = Bench::new(
            "folder",
            Box::new(|m| {
                std::fs::create_dir(m.output.as_deref().unwrap()).unwrap();
                Ok(None)
            }),
        );
        folder.click(folder.walter);
        folder.settle();
        assert_eq!(folder.files(), Vec::<String>::new(), "its empty folder goes too");
        assert_eq!(folder.jobs()[0].error.as_deref(), Some("Timeline didn't save Walter White.fcpxml as a file."));
    }

    #[test]
    fn a_stopped_or_broken_run_leaves_no_file() {
        // Stopped in Activity while it runs, after it wrote: nothing is left.
        let (go, answer) = held();
        let b = Bench::new("stopped", answer);
        let PackActionStart::Queued { job, .. } = b.click(b.walter) else { panic!("not queued") };
        let start = Instant::now();
        while b.ran.lock().is_empty() {
            assert!(start.elapsed() < Duration::from_secs(10));
            thread::sleep(Duration::from_millis(5));
        }
        b.sv.jobs.cancel(job);
        go.send(()).unwrap();
        b.settle();
        assert_eq!((b.files(), b.jobs()[0].state), (Vec::<String>::new(), JobState::Cancelled));
        assert_eq!(b.notes(), 0, "the next start has nothing to do");

        // A run that panics after writing: its file goes as it unwinds.
        let broken = Bench::new(
            "panics",
            Box::new(|m| {
                std::fs::write(m.output.as_deref().unwrap(), "<half>").unwrap();
                panic!("a stand-in that breaks");
            }),
        );
        broken.click(broken.walter);
        broken.settle();
        assert_eq!((broken.files(), broken.jobs()[0].state), (Vec::<String>::new(), JobState::Failed));
        assert_eq!(broken.notes(), 0);
    }

    /// A crash runs no `Drop`: only the note says the file is there.
    #[test]
    fn what_a_crash_left_beside_the_chosen_file_goes_at_the_next_start() {
        let tmp = tempfile::tempdir().unwrap();
        let notes = tmp.path().join("plugin-parts");
        let chosen = tmp.path().join("Walter.fcpxml");
        std::fs::write(&chosen, "the person's").unwrap();
        let part = Part::beside(&chosen, JobId::new(), &notes);
        let left = part.path().to_path_buf();
        std::fs::write(&left, "<half>").unwrap();
        std::mem::forget(part);
        assert_eq!(std::fs::read_dir(&notes).unwrap().count(), 1);

        // Nothing else a note may name goes: this one names no part.
        let other = tmp.path().join("Jesse.fcpxml");
        std::fs::write(&other, "the person's").unwrap();
        std::fs::write(notes.join("other"), other.to_string_lossy().as_bytes()).unwrap();

        remove_left_parts(&notes);
        assert!(!left.exists());
        assert!(chosen.is_file() && other.is_file());
        assert_eq!(std::fs::read_dir(&notes).unwrap().count(), 0);
        // No folder of notes yet: nothing to do.
        remove_left_parts(&tmp.path().join("none"));
    }

    #[test]
    fn a_cancelled_dialog_queues_nothing() {
        let b = Bench::new("cancelled", writes());
        *b.choice.lock() = None;
        assert_eq!(b.click(b.walter), PackActionStart::Cancelled);
        assert_eq!((b.asked.lock().len(), b.jobs().len(), b.ran.lock().len()), (1, 0, 0));
        assert_eq!(b.files(), Vec::<String>::new());
        // Asked again, it asks again.
        *b.choice.lock() = Some(b.chosen());
        assert!(matches!(b.click(b.walter), PackActionStart::Queued { .. }));
        assert_eq!(b.asked.lock().len(), 2);
        b.settle();
    }

    #[test]
    fn the_same_pack_action_waits_once() {
        let (go, answer) = held();
        let b = Bench::new("once", answer);
        assert!(matches!(b.click(b.walter), PackActionStart::Queued { .. }));
        // Clicked again while it waits or runs: nothing is asked, nothing queued.
        assert_eq!(b.click(b.walter), PackActionStart::Waits);
        assert_eq!((b.asked.lock().len(), b.jobs().len()), (1, 1));
        // Jesse's pack is another run, beside it.
        let jesse = CharacterId::new();
        assert!(matches!(b.click(jesse), PackActionStart::Queued { .. }));
        assert_eq!((b.asked.lock().len(), b.jobs().len()), (2, 2));
        for _ in 0..2 {
            go.send(()).unwrap();
        }
        b.settle();
        assert!(b.jobs().iter().all(|j| j.state == JobState::Done), "{:?}", b.jobs());
        // Once it ended, a click saves again.
        go.send(()).unwrap();
        assert!(matches!(b.click(b.walter), PackActionStart::Queued { .. }));
        b.settle();
        assert_eq!(b.ran.lock().len(), 3);
    }

    #[test]
    fn a_pack_that_changed_or_still_changes_asks_nothing() {
        let b = Bench::new("refused", writes());
        let message = |r: AppResult<PackActionStart>| r.unwrap_err().message;
        let spec = PackSpec::default();
        // Changed since the grid showed it: the export's words, and no dialog.
        assert_eq!(
            message(b.click_as(b.walter, FCPXML, &spec, "s0")),
            "The pack changed meanwhile. Check it and export again."
        );
        b.fake.changed.store(true, Ordering::SeqCst);
        assert!(message(b.click_as(b.walter, FCPXML, &spec, "s1")).contains("changed meanwhile"));
        b.fake.changed.store(false, Ordering::SeqCst);

        // While a filter it turns on checks shots, it waits, as an export does.
        let speech = b.sv.host.store.commit(folder(&b.sv.host, &b._tmp.0, &speech()), true, true).unwrap().record.id;
        let filtered = PackSpec { plugin_filters: vec!["dev.a.speech/speaks".into()], ..PackSpec::default() };
        let check = RunKey {
            character_id: Some(b.walter),
            ..RunKey::new(&speech, ActionKind::ShotFilter, "speaks", Some(b.project), None, None)
        };
        let (free, wait) = mpsc::channel::<()>();
        let busy = JobSpec {
            kind: JobKind::Plugin,
            class: Class::Plugin,
            title: "Speaks · Walter".into(),
            subtitle: None,
            project_id: Some(b.project),
            media_id: None,
            export_id: None,
        };
        b.sv.host.submit(&b.sv.jobs, check, busy, move |_| {
            let _ = wait.recv_timeout(Duration::from_secs(10));
            Ok(())
        });
        assert_eq!(
            message(b.click_as(b.walter, FCPXML, &filtered, "s1")),
            "The pack waits for “Speaks” to check its shots."
        );
        // One it doesn't turn on doesn't hold it up.
        assert!(matches!(b.click_as(b.walter, FCPXML, &spec, "s1"), Ok(PackActionStart::Queued { .. })));
        free.send(()).unwrap();
        b.settle();

        // Never a program: its plugin.json can't name one, nor can the person type one.
        let tool = json!({"id": "tool", "title": "Save as a tool…", "file": {"name": "{character}.exe", "extensions": ["exe"]}});
        let mut m = manifest();
        m["id"] = json!("dev.a.tool");
        m["contributes"]["packActions"] = json!([tool]);
        let dir = b._tmp.0.join("dev.a.tool");
        std::fs::create_dir_all(&dir).unwrap();
        for file in ["main.py", "main.py.lock"] {
            std::fs::write(dir.join(file), b"").unwrap();
        }
        std::fs::write(dir.join("plugin.json"), m.to_string()).unwrap();
        let e = b.sv.host.store.stage_folder(&dir).map(|_| ()).unwrap_err();
        assert!(e.to_string().contains("a kind of file that runs"), "{e}");
        let program = "A plugin can't save a program, a script or a shortcut.";
        for typed in ["Walter White.lnk", "Walter White.exe. ", "Walter White.PS1"] {
            *b.choice.lock() = Some(b.cuts.join(typed));
            assert_eq!(message(b.click_as(b.walter, FCPXML, &spec, "s1")), program, "{typed}");
        }
        // Nor a name its part wouldn't fit beside it.
        *b.choice.lock() = Some(b.cuts.join(format!("{}.fcpxml", "w".repeat(234))));
        assert_eq!(
            message(b.click_as(b.walter, FCPXML, &spec, "s1")),
            "That file name is too long. Choose a shorter one."
        );
        // Nor a place AutoScene doesn't take.
        *b.choice.lock() = Some(PathBuf::from("Walter White.fcpxml"));
        assert!(message(b.click_as(b.walter, FCPXML, &spec, "s1")).contains("can't use that location"));
        // A pack action it doesn't have, or of a plugin that is off.
        assert!(message(b.click_as(b.walter, "dev.a.timeline/edl", &spec, "s1")).contains("no such pack action"));
        assert!(message(b.click_as(b.walter, "timeline", &spec, "s1")).contains("isn't a plugin's pack action"));
        b.sv.host.store.set_enabled(&PluginId::parse("dev.a.timeline").unwrap(), false).unwrap();
        assert_eq!(message(b.click_as(b.walter, FCPXML, &spec, "s1")), "Timeline is turned off.");
        // Only the one run that was let through ran.
        assert_eq!((b.ran.lock().len(), b.asked.lock().len()), (1, 6));
    }

    #[test]
    fn a_plugin_turned_off_while_its_pack_action_waits_saves_nothing() {
        let (go, answer) = held();
        let b = Bench::new("off", answer);
        // Walter's runs; Jesse's waits behind it.
        assert!(matches!(b.click(b.walter), PackActionStart::Queued { .. }));
        let start = Instant::now();
        while b.ran.lock().is_empty() {
            assert!(start.elapsed() < Duration::from_secs(10));
            thread::sleep(Duration::from_millis(5));
        }
        *b.choice.lock() = Some(b.cuts.join("Jesse.fcpxml"));
        let PackActionStart::Queued { job, .. } = b.click(CharacterId::new()) else { panic!("not queued") };
        // Turned off before it starts: it looks again then, and runs nothing.
        b.sv.host.store.set_enabled(&PluginId::parse("dev.a.timeline").unwrap(), false).unwrap();
        go.send(()).unwrap();
        b.settle();
        let jesse = b.jobs().into_iter().find(|j| j.id == job).unwrap();
        assert_eq!((jesse.state, jesse.error.as_deref()), (JobState::Failed, Some("Timeline is turned off.")));
        assert_eq!((b.ran.lock().len(), b.files()), (1, vec![String::from("Walter White.fcpxml")]));
    }

    #[test]
    fn the_name_offered_fits_a_file_system() {
        assert_eq!(offered_name("{character}.fcpxml", "Walter White"), "Walter White.fcpxml");
        assert_eq!(offered_name("cut-sheet.csv", "Walter White"), "cut-sheet.csv");
        // What a file name can't hold goes, as in an export's names.
        assert_eq!(offered_name("{character}.fcpxml", "AC/DC: Live?"), "AC DC Live.fcpxml");
        // However many fields its plugin.json names, and however long the name.
        let long = "Ä".repeat(80);
        let name = offered_name(&format!("{}.fcpxml", "{character}".repeat(8)), &long);
        assert!(name.len() <= MAX_OFFERED_NAME && name.ends_with(".fcpxml"), "{} {name}", name.len());
        assert!(name.starts_with("ÄÄÄ"));
        let one = offered_name(&format!("{}-{{character}}.fcpxml", "a".repeat(81)), &long);
        assert_eq!(one, format!("{}-{}.fcpxml", "a".repeat(81), "Ä".repeat(55)));
        // Cut where a space was, it doesn't end in it.
        let spaced = format!("{} {}", "Ä".repeat(55), "b".repeat(30));
        let template = format!("{{character}}.{}", "x".repeat(88));
        assert_eq!(offered_name(&template, &spaced), format!("{}.{}", "Ä".repeat(55), "x".repeat(88)));
    }
}

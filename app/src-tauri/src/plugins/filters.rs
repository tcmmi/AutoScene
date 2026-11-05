//! Plugins' shot filters at work: what a scenepack shows of them
//! ([`pack_filters`]) and the checks that ask a filter's plugin for its
//! verdicts on a character's shots ([`queue_check`]).
//!
//! A check of one filter for one character is a chain of runs, one episode
//! per run, in episode order. A run gets the shots of its episode that the
//! pack's own filters take and that lack the filter's verdicts on its basis
//! now (`as_project::verdicts`). When it ends well, the next episode with
//! such shots is queued, behind whatever waits already: a check of a whole
//! series never holds the one plugin slot (or, for a plugin that uses the
//! GPU, the analysis') for more than an episode. A chain is one run as
//! [`RunKey`] sees it (the plugin, the filter, the project and the
//! character), so a check for Jesse is queued beside Walter's, and a second
//! check for Walter is not.
//!
//! Each verdict names a shot the run was sent, or it counts for nothing
//! (`as_plugin::runtime`); a later word on a shot replaces the earlier one.
//! The run's verdicts are kept once it ends, however it ends. Once a run's
//! work is over (it ran, or it was dropped before it started), it no longer
//! counts as its check's, and the pack hears `pack:changed`: once per run,
//! when what it reads is what comes next (the next episode waits, or the
//! check is ready or failed). A check that fails, that leaves shots without
//! a word or whose word couldn't be kept, or that is stopped (in Activity,
//! or because its plugin went off) is marked failed with its basis: it
//! starts again only when asked to again ("Try again"), or once its basis
//! changed, never by itself. A filter whose plugin isn't active is off: the
//! pack ignores it.
//!
//! A run is a job in Activity, named by its filter and the character. What
//! its plugin says it did goes to the log, not to a toast: a check of a
//! season is a run per episode.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use as_api::{PackFilterState, PackSpec};
use as_core::{CancelToken, CharacterId, FrameIdx, JobId, MediaId, Msg, ProjectId, msg};
use as_jobs::{JobCtx, JobError, JobResult, JobRunner};
use as_plugin::protocol::{CharacterContext, EpisodeContext, FaceBox, ProjectContext, ShotContext, ShotsContext};
use as_plugin::runtime::{self, RunSpec};
use as_plugin::{
    ActionContext, ActionKind, HostCalls, Manifest, PluginId, PluginResult, SettingValue, ShotFilterSpec, Trigger,
};
use as_project::naming::FileWords;
use as_project::{FilterCheck, Library, PluginFilter, PluginFilters, ProjectResult};
use parking_lot::Mutex;
use serde_json::Value;
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

use super::{PluginHost, RunKey, active, calls_for, job_spec, plugin_job_error, project_context, runnable, startable};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::events::PackChanged;
use crate::native_text::Words;
use crate::state::AppState;
use crate::work::job_error;

/// The checks that failed or were stopped, each with the basis it ran on.
/// Kept while AutoScene runs.
#[derive(Default)]
pub struct Checks(Mutex<HashMap<Chain, String>>);

impl Checks {
    fn fail(&self, chain: &Chain, basis: &str) {
        self.0.lock().insert(chain.clone(), basis.to_owned());
    }

    fn clear(&self, chain: &Chain) {
        self.0.lock().remove(chain);
    }

    fn failed(&self, chain: &Chain, basis: &str) -> bool {
        self.0.lock().get(chain).is_some_and(|b| b == basis)
    }
}

/// A check: of a plugin's filter, for a character of a project.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Chain {
    plugin: PluginId,
    filter: String,
    project: ProjectId,
    character: CharacterId,
}

impl Chain {
    /// The check of the filter `key` (`<pluginId>/<filterId>`).
    fn of(key: &str, project: ProjectId, character: CharacterId) -> AppResult<Self> {
        let unknown =
            || AppError::new(ErrorCode::Invalid, msg!("plugin.notAFilter", "That isn't a plugin's shot filter."));
        let (plugin, filter) = key.split_once('/').ok_or_else(unknown)?;
        let plugin = PluginId::parse(plugin).map_err(|_| unknown())?;
        Ok(Self { plugin, filter: filter.to_owned(), project, character })
    }

    /// `<pluginId>/<filterId>`, as a pack's spec names it.
    fn key(&self) -> String {
        format!("{}/{}", self.plugin, self.filter)
    }

    /// Its runs, as the plugin host sees them: one run, whatever episode.
    fn run(&self) -> RunKey {
        RunKey {
            character_id: Some(self.character),
            ..RunKey::new(&self.plugin, ActionKind::ShotFilter, &self.filter, Some(self.project), None, None)
        }
    }
}

/// What a filter's verdicts are judged on now (`as_project::basis`): its
/// id, its revision and the values of the settings it names, of `values`
/// (each of its plugin's settings, as a run gets them).
fn basis_of(f: &ShotFilterSpec, values: &BTreeMap<String, SettingValue>) -> String {
    let values: BTreeMap<String, Value> =
        values.iter().map(|(k, v)| (k.clone(), serde_json::to_value(v).unwrap_or(Value::Null))).collect();
    as_project::basis(&f.id, f.revision, &f.settings, &values)
}

/// The plugins' shot filters a pack of `c` in `project` shows with `spec`:
/// each of an active plugin (`allow_unreviewed` is the setting), with the
/// basis its verdicts need now and what its check is doing, and each
/// `spec` turns on that no active plugin has (off). Without plugins, every
/// one `spec` turns on is off.
pub fn pack_filters(
    host: Option<&PluginHost>,
    jobs: &JobRunner,
    allow_unreviewed: bool,
    project: ProjectId,
    c: CharacterId,
    spec: &PackSpec,
) -> PluginFilters {
    let mut out = Vec::new();
    let plugins = host.map_or_else(Vec::new, |h| {
        h.store.list().unwrap_or_else(|e| {
            log::warn!("listing plugins for a pack's filters: {}", e.detail());
            Vec::new()
        })
    });
    for p in &plugins {
        let Some(m) = p.manifest.as_ref().filter(|m| !m.shot_filters.is_empty()) else { continue };
        let on = active(p, allow_unreviewed);
        let values = match (on, host) {
            (true, Some(h)) => h.store.values(&p.record.id).unwrap_or_else(|e| {
                log::warn!("{}'s settings for its filters: {}", p.record.id, e.detail());
                BTreeMap::new()
            }),
            _ => BTreeMap::new(),
        };
        for f in &m.shot_filters {
            let key = format!("{}/{}", p.record.id, f.id);
            if !on && !spec.plugin_filters.contains(&key) {
                continue;
            }
            let chain = Chain { plugin: p.record.id.clone(), filter: f.id.clone(), project, character: c };
            let basis = basis_of(f, &values);
            let state = match host {
                _ if !on => PackFilterState::Off,
                Some(h) if h.is_queued(jobs, &chain.run()) => PackFilterState::Checking,
                Some(h) if h.checks.failed(&chain, &basis) => PackFilterState::Failed,
                _ => PackFilterState::Ready,
            };
            out.push(PluginFilter {
                key,
                title: f.title.clone(),
                description: f.description.clone(),
                reason: f.reason.clone(),
                plugin: m.name.clone(),
                basis,
                state,
            });
        }
    }
    // Turned on, and no plugin has it now (uninstalled, or an update took it
    // away): nothing of it is known but its key, which is no name to show.
    // Without plugins at all, nothing can be said of it but that it is off.
    for key in &spec.plugin_filters {
        if out.iter().all(|f| f.key != *key) {
            let (plugin, filter) = key.split_once('/').unwrap_or(("", key));
            let (title, plugin, state) = match host {
                Some(_) => ("Removed filter", "", PackFilterState::Gone),
                None => (filter, plugin, PackFilterState::Off),
            };
            out.push(PluginFilter {
                key: key.clone(),
                title: title.to_owned(),
                description: None,
                reason: String::new(),
                plugin: plugin.to_owned(),
                basis: String::new(),
                state,
            });
        }
    }
    PluginFilters(out)
}

/// What a pack's filters are read from: the app's plugins, its jobs and
/// its library, taken from its state so they are read off the async
/// runtime ([`Source::filters`] reads the plugins' files).
#[derive(Clone)]
pub struct Source {
    host: Option<Arc<PluginHost>>,
    jobs: JobRunner,
    lib: Arc<Library>,
}

impl Source {
    pub fn of(state: &AppState) -> Self {
        Self { host: state.plugins.clone(), jobs: state.jobs.clone(), lib: state.library.clone() }
    }

    /// [`pack_filters`] of the app's plugins.
    pub fn filters(&self, project: ProjectId, c: CharacterId, spec: &PackSpec) -> AppResult<PluginFilters> {
        let allow = self.lib.allows_unreviewed_plugins()?;
        Ok(pack_filters(self.host.as_deref(), &self.jobs, allow, project, c, spec))
    }
}

/// Why an export of the pack `spec` makes waits: a filter it turns on is
/// checking its shots. A failed check doesn't hold it up: the shots it
/// didn't judge are in the pack, as the grid shows them.
pub fn export_waits(filters: &PluginFilters, spec: &PackSpec) -> Option<Msg> {
    let checking = |f: &&PluginFilter| f.state == PackFilterState::Checking && spec.plugin_filters.contains(&f.key);
    filters
        .0
        .iter()
        .find(checking)
        .map(|f| msg!("pack.waitsForFilter", "The pack waits for “{{title}}” to check its shots.", title = &f.title))
}

/// What a check reads and writes of a project: the library's projects
/// (`as_project::Project`), or a test's stand-in.
pub trait Packs: Send + Sync {
    /// `Project::filter_next`.
    fn next(&self, at: &Place, after: Option<MediaId>) -> ProjectResult<Option<MediaId>>;
    /// `Project::filter_shots`.
    fn shots(&self, at: &Place, media: MediaId) -> ProjectResult<Option<FilterCheck>>;
    /// `Project::keep_verdicts`.
    fn keep(&self, at: &Place, media: MediaId, basis: &str, given: &[(FrameIdx, bool)]) -> ProjectResult<bool>;
    /// The project as a run gets it.
    fn project(&self, id: ProjectId) -> ProjectResult<ProjectContext>;
    /// `Project::character_label`.
    fn name(&self, project: ProjectId, c: CharacterId, words: &dyn FileWords) -> ProjectResult<String>;
}

/// Where a check looks: a filter (`key`) of `filters`, for the pack `spec`
/// of `character` in `project`.
pub struct Place<'a> {
    pub project: ProjectId,
    pub character: CharacterId,
    pub spec: &'a PackSpec,
    pub filters: &'a PluginFilters,
    pub key: &'a str,
    /// What an unnamed character goes by.
    pub words: &'a dyn FileWords,
}

impl Packs for Arc<Library> {
    fn next(&self, at: &Place, after: Option<MediaId>) -> ProjectResult<Option<MediaId>> {
        Library::project(self, at.project)?.filter_next(at.character, at.spec, at.filters, at.key, after)
    }

    fn shots(&self, at: &Place, media: MediaId) -> ProjectResult<Option<FilterCheck>> {
        Library::project(self, at.project)?.filter_shots(at.character, at.spec, at.filters, at.key, media, at.words)
    }

    fn keep(&self, at: &Place, media: MediaId, basis: &str, given: &[(FrameIdx, bool)]) -> ProjectResult<bool> {
        Library::project(self, at.project)?.keep_verdicts(at.key, at.character, media, basis, given)
    }

    fn project(&self, id: ProjectId) -> ProjectResult<ProjectContext> {
        let project = Library::project(self, id)?;
        project_context(&project)
    }

    fn name(&self, project: ProjectId, c: CharacterId, words: &dyn FileWords) -> ProjectResult<String> {
        Library::project(self, project)?.character_label(c, words)
    }
}

/// How a check's run runs: `as_plugin::runtime::run`, or a test's stand-in.
pub type RunFn =
    dyn Fn(RunSpec, &CancelToken, &dyn Fn(&str, Option<f64>)) -> PluginResult<Option<String>> + Send + Sync;

/// FFmpeg as a run gets it (ffmpeg, ffprobe, its version), or why it isn't
/// there.
pub type ToolsFn = dyn Fn() -> Result<(PathBuf, PathBuf, String), String> + Send + Sync;

/// What checks use of the app.
#[derive(Clone)]
pub struct Checking {
    pub host: Arc<PluginHost>,
    pub jobs: JobRunner,
    pub lib: Arc<Library>,
    pub packs: Arc<dyn Packs>,
    pub tools: Arc<ToolsFn>,
    pub run: Arc<RunFn>,
    /// Told that a character's pack changed: a run of a check of theirs is
    /// over.
    pub changed: Arc<dyn Fn(ProjectId, CharacterId) + Send + Sync>,
    /// The language an unnamed character is called by in its runs.
    pub words: &'static Words,
}

impl Checking {
    /// The app's.
    pub fn of(app: &AppHandle) -> AppResult<Self> {
        let state = app.state::<AppState>();
        let (tools, changed) = (app.clone(), app.clone());
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
            changed: Arc::new(move |project_id, character_id| {
                let _ = PackChanged { project_id, character_id }.emit(&changed);
            }),
            words: state.words(),
        })
    }

    /// [`pack_filters`] of these plugins.
    fn filters(&self, project: ProjectId, c: CharacterId, spec: &PackSpec) -> AppResult<PluginFilters> {
        let allow = self.lib.allows_unreviewed_plugins()?;
        Ok(pack_filters(Some(&self.host), &self.jobs, allow, project, c, spec))
    }
}

/// Starts a check of the filter `key` for the pack `spec` of `c` in
/// `project`, from its first episode with shots it hasn't judged: `true`
/// when one waits or runs now. One that failed with the basis it has now
/// starts only `again` (the person's Try again); one that waits or runs
/// already is that check, also when another call queued it meanwhile.
/// Refused while its plugin isn't active.
pub fn queue_check(
    ck: &Checking,
    project: ProjectId,
    c: CharacterId,
    key: &str,
    spec: &PackSpec,
    again: bool,
) -> AppResult<bool> {
    let chain = Chain::of(key, project, c)?;
    let p = ck.host.store.get(&chain.plugin)?;
    let m = runnable(&p, ck.lib.allows_unreviewed_plugins()?)?;
    let Some(f) = m.shot_filters.iter().find(|f| f.id == chain.filter) else {
        let gone = msg!("plugin.noSuchFilter", "{{name}} has no such filter.", name = &m.name);
        return Err(AppError::new(ErrorCode::NotFound, gone));
    };
    if ck.host.is_queued(&ck.jobs, &chain.run()) {
        return Ok(true);
    }
    let basis = basis_of(f, &ck.host.store.values(&chain.plugin)?);
    if !again && ck.host.checks.failed(&chain, &basis) {
        return Ok(false);
    }
    ck.host.checks.clear(&chain);
    let filters = ck.filters(project, c, spec)?;
    let at = Place { project, character: c, spec, filters: &filters, key, words: ck.words };
    let Some(media) = ck.packs.next(&at, None)? else { return Ok(false) };
    // Asked twice at once, one call queues it, and both say it waits.
    Ok(queue_episode(ck, &chain, m, spec, media, &basis, None).is_some() || ck.host.is_queued(&ck.jobs, &chain.run()))
}

/// Queues the run of `chain` on the episode `media` (of `m`'s filter),
/// for the pack `spec`, queued with `basis`: as the run after `current`
/// when that run queues it.
fn queue_episode(
    ck: &Checking,
    chain: &Chain,
    m: &Manifest,
    spec: &PackSpec,
    media: MediaId,
    basis: &str,
    current: Option<JobId>,
) -> Option<JobId> {
    let title = m.shot_filters.iter().find(|f| f.id == chain.filter).map_or(chain.filter.as_str(), |f| &f.title);
    // Named by the character too: Walter's check and Jesse's each have their runs.
    let title = match ck.packs.name(chain.project, chain.character, ck.words) {
        Ok(name) => format!("{title} · {name}"),
        Err(e) => {
            log::warn!("{} for {}: their name: {e}", chain.key(), chain.character);
            title.to_owned()
        }
    };
    let job = job_spec(m, &title, Some(chain.project), None);
    let queued = Arc::new(AtomicBool::new(false));
    let run =
        Run { ck: ck.clone(), chain: chain.clone(), basis: basis.to_owned(), queued: queued.clone(), started: None };
    let (spec, gpu) = (spec.clone(), m.runtime.gpu);
    let work = move |ctx: &JobCtx| {
        let mut run = run;
        run.started = Some(ctx.id);
        check_episode(&run.ck, ctx, &run.chain, &spec, media, gpu, &run.basis)
    };
    let job = ck.host.submit_after(&ck.jobs, current, chain.run(), job, work);
    queued.store(job.is_some(), Ordering::SeqCst);
    job
}

/// A queued run of a check. Once its work is over (it ran, it panicked, or
/// it was dropped before it started: stopped while it waited, or at quit),
/// it no longer counts as its check's run and the pack hears so. Dropped
/// before it started, or panicking, it marks its check failed. One the
/// runner refused (the check waits already) never was one.
struct Run {
    ck: Checking,
    chain: Chain,
    basis: String,
    queued: Arc<AtomicBool>,
    /// Its job, once it started.
    started: Option<JobId>,
}

impl Drop for Run {
    fn drop(&mut self) {
        // One that started was queued, even before its queuer heard so.
        if self.started.is_none() && !self.queued.load(Ordering::SeqCst) {
            return;
        }
        if self.started.is_none() || std::thread::panicking() {
            self.ck.host.checks.fail(&self.chain, &self.basis);
        }
        if let Some(job) = self.started {
            self.ck.host.release(job);
        }
        (self.ck.changed)(self.chain.project, self.chain.character);
    }
}

/// The run of `chain` on `media`, for the pack `spec` (`gpu`: it waited as
/// analysis), queued with the basis `queued`, and the next episode's queued
/// when it ended well.
fn check_episode(
    ck: &Checking,
    ctx: &JobCtx,
    chain: &Chain,
    spec: &PackSpec,
    media: MediaId,
    gpu: bool,
    queued: &str,
) -> JobResult {
    let fail = |basis: &str| ck.host.checks.fail(chain, basis);
    let _running = ck.host.hold(ctx, &chain.plugin).inspect_err(|_| fail(queued))?;
    let key = chain.run();
    let p = startable(&ck.host.store, &ck.lib, &key, Trigger::Pack, gpu).inspect_err(|_| fail(queued))?;
    let m = p.manifest.as_ref().expect("a plugin that may start has its manifest");
    let f = m.shot_filters.iter().find(|f| f.id == chain.filter).expect("startable found the filter");
    let (ffmpeg, ffprobe, version) = (ck.tools)().map_err(|e| {
        fail(queued);
        JobError::Failed(e)
    })?;
    let action = ActionContext { kind: ActionKind::ShotFilter, id: chain.filter.clone(), trigger: Trigger::Pack };
    // Through AutoScene's uv and with the plugin's large files, got first
    // when they aren't kept, as any run of it is.
    let report = |stage: &str, f: Option<f64>| ctx.report(stage, f);
    let tools = (ffmpeg.as_path(), ffprobe.as_path(), version.as_str());
    let mut run = ck
        .host
        .run_spec(&p, action, tools, &ctx.cancel, &report)
        .map_err(plugin_job_error)
        .inspect_err(|_| fail(queued))?;
    // On the settings the run gets.
    let basis = basis_of(f, &run.message.settings);
    let filters = ck.filters(chain.project, chain.character, spec).map_err(|e| {
        fail(&basis);
        JobError::Failed(e.message)
    })?;
    let filter = chain.key();
    let at = Place {
        project: chain.project,
        character: chain.character,
        spec,
        filters: &filters,
        key: &filter,
        words: ck.words,
    };
    let Some(check) = ck.packs.shots(&at, media).map_err(job_error).inspect_err(|_| fail(&basis))? else {
        // Judged meanwhile (or gone): on with the next.
        go_on(ck, ctx, chain, m, spec, media, &basis);
        return Ok(());
    };
    run.message.project = Some(ck.packs.project(chain.project).map_err(job_error).inspect_err(|_| fail(&basis))?);
    run.message.shots = Some(shots_context(&check));
    // A shot's last word counts: one per shot sent, however often a plugin repeats itself.
    let said = Arc::new(Mutex::new(HashMap::new()));
    let sink = said.clone();
    // The runtime hands on only shots of this run, and so does this.
    let sent: HashMap<String, FrameIdx> =
        check.shots.iter().map(|s| (format!("{}:{}", check.media, s.start), s.start)).collect();
    run.verdicts = Some(Box::new(move |shot: &str, keep| match sent.get(shot) {
        Some(&start) => {
            sink.lock().insert(start, keep);
        }
        None => log::info!("a verdict for a shot its run wasn't sent ({shot}): ignored"),
    }));
    let calls =
        calls_for(&ck.lib, &ck.host, &p, &key, run.protocol).map_err(plugin_job_error).inspect_err(|_| fail(&basis))?;
    run.calls = calls.map(|c| Arc::new(c) as Arc<dyn HostCalls>);
    let episode = std::path::Path::new(&check.path).file_name().map(|n| n.to_string_lossy().into_owned());
    ctx.stage(&match episode {
        Some(episode) => format!("Checking {}'s shots in {episode}", check.name),
        None => format!("Checking {}'s shots", check.name),
    });
    let result = (ck.run)(run, &ctx.cancel, &report);

    // What it said counts, however it ended.
    let said = std::mem::take(&mut *said.lock());
    let given: Vec<(FrameIdx, bool)> = said.iter().map(|(start, keep)| (*start, *keep)).collect();
    let kept = if given.is_empty() { Ok(true) } else { ck.packs.keep(&at, media, &basis, &given) };
    let left = match kept {
        Ok(true) => check.shots.iter().filter(|s| !said.contains_key(&s.start)).count(),
        // Its shots are being found again, or it is gone: none of them is left to judge.
        Ok(false) => {
            log::info!("{filter} for {}: {media} was found again meanwhile", chain.character);
            0
        }
        // Its word is lost, and a check that went on would lose the next one's too.
        Err(e) => {
            log::warn!("keeping {filter}'s verdicts for {} in {media}: {e}", chain.character);
            fail(&basis);
            return Err(job_error(e));
        }
    };
    match result {
        Ok(message) => {
            if left > 0 {
                log::warn!("{filter} left {left} of {} shots of {media} without a word", check.shots.len());
                fail(&basis);
            }
            if let Some(message) = message {
                log::info!("{filter} for {} in {media}: {message}", chain.character);
            }
            // Stopped once it had said its word: the check stops too.
            match ctx.cancel.is_cancelled() {
                true => fail(&basis),
                false => go_on(ck, ctx, chain, m, spec, media, &basis),
            }
            Ok(())
        }
        Err(e) => {
            fail(&basis);
            Err(plugin_job_error(e))
        }
    }
}

/// Queues the run of `chain` on the episode after `media` that has shots
/// it hasn't judged, as the next run of the job of `ctx`.
fn go_on(ck: &Checking, ctx: &JobCtx, chain: &Chain, m: &Manifest, spec: &PackSpec, media: MediaId, basis: &str) {
    let next = ck.filters(chain.project, chain.character, spec).and_then(|filters| {
        let key = chain.key();
        let at = Place {
            project: chain.project,
            character: chain.character,
            spec,
            filters: &filters,
            key: &key,
            words: ck.words,
        };
        Ok(ck.packs.next(&at, Some(media))?)
    });
    match next {
        Ok(Some(next)) => {
            if queue_episode(ck, chain, m, spec, next, basis, Some(ctx.id)).is_none() {
                log::info!("{} for {}: its check waits already", chain.key(), chain.character);
            }
        }
        Ok(None) => {}
        Err(e) => {
            log::warn!("{} for {}: the next episode: {}", chain.key(), chain.character, e.message);
            ck.host.checks.fail(chain, basis);
        }
    }
}

/// `check` as a run gets it.
fn shots_context(check: &FilterCheck) -> ShotsContext {
    let media_id = check.media.to_string();
    ShotsContext {
        character: CharacterContext { id: check.character.to_string(), name: check.name.clone() },
        episodes: vec![EpisodeContext {
            media_id: media_id.clone(),
            path: check.path.clone(),
            fps: check.fps.map(|fps| fps.to_string()),
            frames: check.frames.map(u64::from),
        }],
        shots: check
            .shots
            .iter()
            .map(|s| ShotContext {
                key: format!("{media_id}:{}", s.start),
                media_id: media_id.clone(),
                start: s.start.into(),
                end: s.end.into(),
                face_height: s.face_height,
                faceless: s.faceless,
                face: s.face.map(|(frame, b)| FaceBox {
                    frame: frame.into(),
                    x: b[0],
                    y: b[1],
                    w: b[2] - b[0],
                    h: b[3] - b[1],
                }),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    use as_api::{JobKind, JobState};
    use as_core::Rational;
    use as_jobs::{Class, JobSpec};
    use as_plugin::{PluginError, SettingValue};
    use as_project::{CheckShot, ProjectError};

    use super::*;
    use crate::plugins::tests::{TEST_UV, folder, host_in};

    const SPEAKS: &str = "dev.a.speech/speaks";

    /// A plugin with the shot filter "Speaks", which depends on its setting
    /// `sensitivity` and not on its `webhook`.
    fn manifest() -> serde_json::Value {
        serde_json::json!({
            "manifestVersion": 2,
            "id": "dev.a.speech",
            "version": "1.0.0",
            "name": "Speech Filter",
            "description": "Keeps the shots where they talk.",
            "author": {"name": "Someone"},
            "engines": {"autoscene": ">=1.0.0, <2", "protocol": 2},
            "runtime": {"type": "process", "command": {"*": ["uv", "run", "--script", "--locked", "main.py"]}},
            "permissions": ["project:read"],
            "contributes": {
                "shotFilters": [{"id": "speaks", "title": "Speaks", "reason": "No speech", "settings": ["sensitivity"]}],
                "settings": [
                    {"key": "sensitivity", "type": "number", "title": "Sensitivity", "default": 0.5},
                    {"key": "webhook", "type": "string", "title": "Webhook"}
                ]
            }
        })
    }

    /// A project's episodes, in order, each with the starts of the
    /// character's shots in it, and the verdicts kept, by character and
    /// episode: their basis, and each shot's word. While `full`, nothing
    /// can be kept.
    #[derive(Default)]
    struct Fake {
        episodes: Vec<(MediaId, Vec<FrameIdx>)>,
        kept: Mutex<HashMap<(CharacterId, MediaId), Said>>,
        full: AtomicBool,
    }

    /// A row of verdicts: its basis, and each shot's word.
    type Said = (String, BTreeMap<FrameIdx, bool>);

    impl Fake {
        fn basis(at: &Place) -> Option<String> {
            let ignored = [PackFilterState::Off, PackFilterState::Gone];
            at.filters.find(at.key).filter(|(_, f)| !ignored.contains(&f.state)).map(|(_, f)| f.basis.clone())
        }

        /// The shots of `media` the filter hasn't judged on its basis now.
        fn lacking(&self, at: &Place, media: MediaId) -> Vec<FrameIdx> {
            let Some(basis) = Self::basis(at) else { return Vec::new() };
            let kept = self.kept.lock();
            let row = kept.get(&(at.character, media)).filter(|(b, _)| *b == basis);
            let shots = self.episodes.iter().find(|(m, _)| *m == media).map_or(&[][..], |(_, s)| s);
            shots.iter().copied().filter(|s| row.is_none_or(|(_, said)| !said.contains_key(s))).collect()
        }

        fn said(&self, c: CharacterId, media: MediaId) -> Vec<(FrameIdx, bool)> {
            self.kept
                .lock()
                .get(&(c, media))
                .map(|(_, s)| s.iter().map(|(k, v)| (*k, *v)).collect())
                .unwrap_or_default()
        }
    }

    impl Packs for Fake {
        fn next(&self, at: &Place, after: Option<MediaId>) -> ProjectResult<Option<MediaId>> {
            let from = after.and_then(|a| self.episodes.iter().position(|(m, _)| *m == a)).map_or(0, |i| i + 1);
            Ok(self.episodes[from..].iter().map(|(m, _)| *m).find(|m| !self.lacking(at, *m).is_empty()))
        }

        fn shots(&self, at: &Place, media: MediaId) -> ProjectResult<Option<FilterCheck>> {
            let lacking = self.lacking(at, media);
            Ok((!lacking.is_empty()).then(|| FilterCheck {
                character: at.character,
                name: "Walter".into(),
                media,
                path: format!("/media/{media}.mkv"),
                fps: Some(Rational::new(24, 1)),
                frames: Some(2400),
                shots: lacking
                    .iter()
                    .map(|&start| CheckShot { start, end: start + 10, face_height: 0.3, faceless: false, face: None })
                    .collect(),
            }))
        }

        fn keep(&self, at: &Place, media: MediaId, basis: &str, given: &[(FrameIdx, bool)]) -> ProjectResult<bool> {
            if self.full.load(Ordering::SeqCst) {
                return Err(ProjectError::Invalid(as_core::Msg::plain("The disk is full.")));
            }
            let mut kept = self.kept.lock();
            let row = kept.entry((at.character, media)).or_default();
            if row.0 != basis {
                *row = (basis.to_owned(), BTreeMap::new());
            }
            row.1.extend(given.iter().copied());
            Ok(true)
        }

        fn project(&self, id: ProjectId) -> ProjectResult<ProjectContext> {
            Ok(ProjectContext { id: id.to_string(), title: "Show".into(), folder: "/show".into() })
        }

        fn name(&self, _: ProjectId, _: CharacterId, _: &dyn FileWords) -> ProjectResult<String> {
            Ok("Walter".into())
        }
    }

    /// How a stand-in run answers: the verdicts it gives (of the shots it
    /// was sent, by their keys, and any other key) and how it ends.
    type Answer = Box<dyn Fn(&ShotsContext) -> (Vec<(String, bool)>, PluginResult<Option<String>>) + Send + Sync>;

    /// Checks with the stand-ins: a project of `episodes`, plugins whose
    /// runs `answer`, one plugin run at a time. What ran, in order, is in
    /// `ran`: each run's episode, or another run's name. Each time a pack
    /// heard it changed is in `changed`: whose, and the state its filter
    /// read then.
    struct Bench {
        lib: Arc<Library>,
        ck: Checking,
        fake: Arc<Fake>,
        ran: Arc<Mutex<Vec<String>>>,
        changed: Arc<Mutex<Vec<(CharacterId, PackFilterState)>>>,
        project: ProjectId,
        walter: CharacterId,
        /// Last: its folder goes once the rest let go of its files.
        _tmp: Gone,
    }

    /// A folder that goes when this does.
    struct Gone(std::path::PathBuf);

    impl Drop for Gone {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    impl Bench {
        fn new(name: &str, episodes: usize, answer: Answer) -> Self {
            let tmp = std::env::temp_dir().join(format!("autoscene-filters-{name}-{}", std::process::id()));
            let (lib, host) = host_in(&tmp);
            let lib = Arc::new(lib);
            lib.set_allows_unreviewed_plugins(true).unwrap();
            host.store.commit(folder(&host, &tmp, &manifest()), true, true).unwrap();
            let fake = Arc::new(Fake {
                episodes: (0..episodes).map(|i| (MediaId::new(), vec![0, 100 + i as u32, 300])).collect(),
                ..Fake::default()
            });
            let (ran, changed) = (Arc::new(Mutex::new(Vec::new())), Arc::new(Mutex::new(Vec::new())));
            let names: HashMap<String, usize> =
                fake.episodes.iter().enumerate().map(|(i, (m, _))| (m.to_string(), i + 1)).collect();
            let (log, heard) = (ran.clone(), changed.clone());
            let jobs = JobRunner::new(&[(Class::Plugin, 1)], |_| {});
            let project = ProjectId::new();
            // What a pack refetched when it heard so would read.
            let (reader, read_jobs) = (host.clone(), jobs.clone());
            let ck = Checking {
                host,
                jobs,
                lib: lib.clone(),
                packs: fake.clone(),
                tools: Arc::new(|| Ok(("ffmpeg".into(), "ffprobe".into(), "9.0".into()))),
                run: Arc::new(move |mut spec, cancel, _| {
                    let shots = spec.message.shots.clone().expect("a check's run gets shots");
                    log.lock().push(format!("ep{}", names[&shots.episodes[0].media_id]));
                    assert_eq!(spec.message.action.kind, ActionKind::ShotFilter);
                    assert_eq!(spec.uv.as_deref(), Some(std::path::Path::new(TEST_UV)), "it is a uv script");
                    let (verdicts, end) = answer(&shots);
                    let sink = spec.verdicts.as_mut().expect("a check's run has a sink");
                    for (key, keep) in verdicts {
                        sink(&key, keep);
                    }
                    if cancel.is_cancelled() { Err(PluginError::Cancelled) } else { end }
                }),
                changed: Arc::new(move |p, c| {
                    let state = pack_filters(Some(&reader), &read_jobs, true, p, c, &spec()).0[0].state;
                    heard.lock().push((c, state));
                }),
                words: crate::native_text::tests::english(),
            };
            let (walter, _tmp) = (CharacterId::new(), Gone(tmp));
            Self { lib, ck, fake, ran, changed, project, walter, _tmp }
        }

        fn check(&self, c: CharacterId, again: bool) -> bool {
            queue_check(&self.ck, self.project, c, SPEAKS, &spec(), again).unwrap()
        }

        fn state(&self, c: CharacterId) -> PackFilterState {
            let allow = self.lib.allows_unreviewed_plugins().unwrap();
            pack_filters(Some(&self.ck.host), &self.ck.jobs, allow, self.project, c, &spec()).0[0].state
        }

        fn jobs(&self) -> usize {
            self.ck.jobs.jobs().iter().filter(|j| j.kind == JobKind::Plugin).count()
        }

        /// Waits until no job waits or runs.
        fn settle(&self) {
            let start = Instant::now();
            while self.ck.jobs.jobs().iter().any(|j| !j.state.is_finished()) {
                assert!(start.elapsed() < Duration::from_secs(20), "the checks never ended");
                thread::sleep(Duration::from_millis(5));
            }
        }

        fn ran(&self) -> Vec<String> {
            self.ran.lock().clone()
        }

        /// What `c`'s pack read each time it heard it changed.
        fn heard(&self, c: CharacterId) -> Vec<PackFilterState> {
            self.changed.lock().iter().filter(|(who, _)| *who == c).map(|(_, state)| *state).collect()
        }
    }

    fn spec() -> PackSpec {
        PackSpec { plugin_filters: vec![SPEAKS.into()], ..PackSpec::default() }
    }

    /// Keeps the shots that start at a whole hundred; ends well.
    fn hundreds() -> Answer {
        Box::new(|shots| (shots.shots.iter().map(|s| (s.key.clone(), s.start % 100 == 0)).collect(), Ok(None)))
    }

    #[test]
    fn a_check_runs_an_episode_at_a_time_and_lets_other_runs_in_between() {
        let (go, wait) = mpsc::channel::<()>();
        let wait = Mutex::new(wait);
        let first = std::sync::atomic::AtomicBool::new(true);
        let b = Bench::new(
            "turns",
            3,
            Box::new(move |shots| {
                // The first episode waits to be let go.
                if first.swap(false, Ordering::SeqCst) {
                    wait.lock().recv_timeout(Duration::from_secs(10)).unwrap();
                }
                (shots.shots.iter().map(|s| (s.key.clone(), s.start % 100 == 0)).collect(), Ok(Some("Checked.".into())))
            }),
        );
        assert!(b.check(b.walter, false));
        let start = Instant::now();
        while b.ran().is_empty() {
            assert!(start.elapsed() < Duration::from_secs(10));
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(b.state(b.walter), PackFilterState::Checking);
        // Another plugin run waits for the one slot meanwhile.
        let log = b.ran.clone();
        let other = JobSpec {
            kind: JobKind::Plugin,
            class: Class::Plugin,
            title: "Post".into(),
            subtitle: None,
            project_id: None,
            media_id: None,
            export_id: None,
        };
        b.ck.jobs.submit(other, move |_| {
            log.lock().push("post".into());
            Ok(())
        });
        go.send(()).unwrap();
        b.settle();
        // It ran between the first episode and the second: each episode queues the next behind it.
        assert_eq!(b.ran(), ["ep1", "post", "ep2", "ep3"]);
        let (walter, eps) = (b.walter, &b.fake.episodes);
        assert_eq!(b.fake.said(walter, eps[1].0), [(0, true), (101, false), (300, true)]);
        // A pack:changed once each run is over, when the pack reads what comes next: the
        // next episode, then a check that is ready.
        assert_eq!(b.heard(walter), [PackFilterState::Checking, PackFilterState::Checking, PackFilterState::Ready]);
        assert_eq!(b.state(walter), PackFilterState::Ready);
        // Each episode is a run of its own in Activity, named by the filter and the character.
        // What the plugin said goes to the log: a season is no stack of toasts.
        let runs: Vec<_> =
            b.ck.jobs.jobs().into_iter().filter(|j| j.kind == JobKind::Plugin && j.title != "Post").collect();
        assert_eq!(runs.len(), 3);
        assert!(runs.iter().all(|j| j.title == "Speaks · Walter" && j.state == JobState::Done), "{runs:?}");
        assert!(runs.iter().all(|j| j.message.is_none()), "{runs:?}");
        // Nothing left: nothing to check.
        assert!(!b.check(walter, false));
    }

    /// A stand-in whose runs wait until let go: the check stays queued or running.
    fn held() -> (mpsc::Sender<()>, Answer) {
        let (go, wait) = mpsc::channel::<()>();
        let wait = Mutex::new(wait);
        let answer: Answer = Box::new(move |shots| {
            let _ = wait.lock().recv_timeout(Duration::from_secs(10));
            (shots.shots.iter().map(|s| (s.key.clone(), true)).collect(), Ok(None))
        });
        (go, answer)
    }

    #[test]
    fn checking_jesse_while_walter_waits_queues_a_run() {
        let (go, answer) = held();
        let b = Bench::new("jesse", 1, answer);
        let jesse = CharacterId::new();
        assert!(b.check(b.walter, false));
        assert!(b.check(jesse, false));
        assert_eq!(b.jobs(), 2);
        assert_eq!((b.state(b.walter), b.state(jesse)), (PackFilterState::Checking, PackFilterState::Checking));
        for _ in 0..2 {
            go.send(()).unwrap();
        }
        b.settle();
        let ep = b.fake.episodes[0].0;
        assert_eq!((b.fake.said(b.walter, ep).len(), b.fake.said(jesse, ep).len()), (3, 3));
    }

    #[test]
    fn the_same_check_waits_once() {
        let (go, answer) = held();
        let b = Bench::new("once", 2, answer);
        assert!(b.check(b.walter, false));
        // Asked again, by the pack or by Try again, while it waits or runs: the same check.
        assert!(b.check(b.walter, false));
        assert!(b.check(b.walter, true));
        assert_eq!(b.jobs(), 1);
        go.send(()).unwrap();
        // The next episode is the same check too.
        let start = Instant::now();
        while b.jobs() < 2 {
            assert!(start.elapsed() < Duration::from_secs(10));
            thread::sleep(Duration::from_millis(5));
        }
        assert!(b.check(b.walter, false));
        assert_eq!(b.jobs(), 2);
        go.send(()).unwrap();
        b.settle();
        assert_eq!(b.ran(), ["ep1", "ep2"]);
    }

    #[test]
    fn a_verdict_for_a_shot_it_wasn_t_sent_is_ignored() {
        let b = Bench::new(
            "odd",
            1,
            Box::new(|shots| {
                let media = &shots.episodes[0].media_id;
                let mut said: Vec<(String, bool)> = shots.shots.iter().map(|s| (s.key.clone(), false)).collect();
                said.extend([
                    (format!("{media}:99999"), false),
                    (format!("{}:0", MediaId::new()), true),
                    ("junk".into(), false),
                    (format!("{media}:100"), true),
                ]);
                (said, Ok(None))
            }),
        );
        assert!(b.check(b.walter, false));
        b.settle();
        // Its later word on a shot it was sent counts; the rest is nothing.
        assert_eq!(b.fake.said(b.walter, b.fake.episodes[0].0), [(0, false), (100, true), (300, false)]);
        assert_eq!(b.state(b.walter), PackFilterState::Ready);
    }

    #[test]
    fn a_settings_change_asks_for_new_verdicts() {
        let b = Bench::new("settings", 1, hundreds());
        assert!(b.check(b.walter, false));
        b.settle();
        assert!(!b.check(b.walter, false), "all judged");
        let id = PluginId::parse("dev.a.speech").unwrap();
        // A setting it doesn't name: the same verdicts.
        let webhook = BTreeMap::from([("webhook".to_owned(), SettingValue::Text("https://a.example".into()))]);
        b.ck.host.store.set_values(&id, webhook).unwrap();
        assert!(!b.check(b.walter, false));
        // A new sensitivity: asked again, on the new basis, each shot.
        let louder = BTreeMap::from([("sensitivity".to_owned(), SettingValue::Number(0.9))]);
        b.ck.host.store.set_values(&id, louder).unwrap();
        assert!(b.check(b.walter, false));
        b.settle();
        assert_eq!(b.ran(), ["ep1", "ep1"]);
        assert!(!b.check(b.walter, false));
    }

    #[test]
    fn a_failed_check_keeps_the_verdicts_it_gave_and_never_loops() {
        let b = Bench::new(
            "fails",
            2,
            Box::new(|shots| {
                let said = vec![(shots.shots[0].key.clone(), false)];
                (said, Err(PluginError::Failed { message: "No audio.".into(), detail: None }))
            }),
        );
        assert!(b.check(b.walter, false));
        b.settle();
        let ep = b.fake.episodes[0].0;
        assert_eq!(b.fake.said(b.walter, ep), [(0, false)]);
        assert_eq!(b.heard(b.walter), [PackFilterState::Failed], "the pack hears it failed");
        // It stopped there: the second episode never ran, and nothing asks again by itself.
        assert_eq!((b.ran(), b.state(b.walter)), (vec!["ep1".to_owned()], PackFilterState::Failed));
        assert!(!b.check(b.walter, false));
        assert_eq!(b.jobs(), 1);
        let failed = b.ck.jobs.jobs().into_iter().find(|j| j.kind == JobKind::Plugin).unwrap();
        assert_eq!((failed.state, failed.error.as_deref()), (JobState::Failed, Some("No audio.")));
        // Try again: it goes on with the shots it lacks.
        assert!(b.check(b.walter, true));
        b.settle();
        assert_eq!(b.ran(), ["ep1", "ep1"]);

        // One that ends well but leaves shots without a word counts as failed too, and goes on.
        let quiet = Bench::new("quiet", 2, Box::new(|shots| (vec![(shots.shots[0].key.clone(), true)], Ok(None))));
        assert!(quiet.check(quiet.walter, false));
        quiet.settle();
        assert_eq!(
            (quiet.ran(), quiet.state(quiet.walter)),
            (vec!["ep1".into(), "ep2".into()], PackFilterState::Failed)
        );
        assert_eq!(quiet.heard(quiet.walter).last(), Some(&PackFilterState::Failed));
        assert!(!quiet.check(quiet.walter, false));

        // One that fails without a word: the pack hears it failed all the same.
        let mute = Bench::new(
            "mute",
            2,
            Box::new(|_| (vec![], Err(PluginError::Failed { message: "No audio.".into(), detail: None }))),
        );
        assert!(mute.check(mute.walter, false));
        mute.settle();
        assert!(mute.fake.kept.lock().is_empty());
        assert_eq!((mute.ran(), mute.heard(mute.walter)), (vec!["ep1".to_owned()], vec![PackFilterState::Failed]));
        assert!(!mute.check(mute.walter, false));

        // Stopped in Activity, while it waits or runs: the same.
        let (go, answer) = held();
        let stopped = Bench::new("stopped", 1, answer);
        let blocker = JobSpec {
            kind: JobKind::Plugin,
            class: Class::Plugin,
            title: "Busy".into(),
            subtitle: None,
            project_id: None,
            media_id: None,
            export_id: None,
        };
        let (free, wait) = mpsc::channel::<()>();
        let busy = stopped.ck.jobs.submit(blocker, move |_| {
            let _ = wait.recv_timeout(Duration::from_secs(10));
            Ok(())
        });
        assert!(stopped.check(stopped.walter, false));
        stopped.ck.jobs.cancel_where(|j| j.id != busy);
        free.send(()).unwrap();
        stopped.settle();
        drop(go);
        assert_eq!((stopped.ran(), stopped.state(stopped.walter)), (Vec::<String>::new(), PackFilterState::Failed));
        assert_eq!(stopped.heard(stopped.walter), [PackFilterState::Failed]);
        assert!(!stopped.check(stopped.walter, false));
    }

    #[test]
    fn verdicts_that_can_t_be_kept_fail_the_check() {
        let b = Bench::new("full", 2, hundreds());
        b.fake.full.store(true, Ordering::SeqCst);
        assert!(b.check(b.walter, false));
        b.settle();
        // Its word is lost: the check stops there, failed, and never starts again by itself.
        assert_eq!((b.ran(), b.state(b.walter)), (vec!["ep1".to_owned()], PackFilterState::Failed));
        assert_eq!(b.heard(b.walter), [PackFilterState::Failed]);
        let run = b.ck.jobs.jobs().into_iter().find(|j| j.kind == JobKind::Plugin).unwrap();
        assert_eq!((run.state, run.error.as_deref()), (JobState::Failed, Some("The disk is full.")));
        assert!(!b.check(b.walter, false));
        assert_eq!(b.jobs(), 1);
        // With room again, Try again asks for each shot.
        b.fake.full.store(false, Ordering::SeqCst);
        assert!(b.check(b.walter, true));
        b.settle();
        assert_eq!((b.ran().len(), b.state(b.walter)), (3, PackFilterState::Ready));
        assert_eq!(b.fake.said(b.walter, b.fake.episodes[0].0).len(), 3);
    }

    #[test]
    fn a_plugin_that_repeats_itself_is_kept_once_per_shot() {
        let b = Bench::new(
            "repeats",
            1,
            Box::new(|shots| {
                let first = shots.shots[0].key.clone();
                let mut said: Vec<(String, bool)> = (0..10_000).map(|i| (first.clone(), i % 2 == 0)).collect();
                said.extend(shots.shots.iter().skip(1).map(|s| (s.key.clone(), true)));
                (said, Ok(None))
            }),
        );
        assert!(b.check(b.walter, false));
        b.settle();
        // Its last word on the shot counts.
        assert_eq!(b.fake.said(b.walter, b.fake.episodes[0].0), [(0, false), (100, true), (300, true)]);
        assert_eq!(b.state(b.walter), PackFilterState::Ready);
    }

    #[test]
    fn export_waits_only_while_a_check_runs() {
        let filter = |key: &str, state| PluginFilter {
            key: key.into(),
            title: "Speaks".into(),
            description: None,
            reason: "No speech".into(),
            plugin: "Speech Filter".into(),
            basis: "b".into(),
            state,
        };
        let on = spec();
        let waits = |state| export_waits(&PluginFilters(vec![filter(SPEAKS, state)]), &on);
        assert_eq!(
            waits(PackFilterState::Checking).map(|m| m.text).as_deref(),
            Some("The pack waits for “Speaks” to check its shots.")
        );
        // Failed: the shots it didn't judge are in, as the grid shows them.
        for state in [PackFilterState::Ready, PackFilterState::Failed, PackFilterState::Off] {
            assert_eq!(waits(state), None, "{state:?}");
        }
        // A filter that isn't on doesn't hold it up while it checks.
        let other = PluginFilters(vec![filter("dev.a.speech/shouts", PackFilterState::Checking)]);
        assert_eq!(export_waits(&other, &on), None);
    }

    #[test]
    fn a_pack_shows_each_filter_of_active_plugins_and_those_it_turns_on() {
        let tmp = std::env::temp_dir().join(format!("autoscene-filters-list-{}", std::process::id()));
        let (lib, host) = host_in(&tmp);
        let jobs = JobRunner::new(&[(Class::Plugin, 1)], |_| {});
        let (project, c) = (ProjectId::new(), CharacterId::new());
        let id = host.store.commit(folder(&host, &tmp, &manifest()), true, true).unwrap().record.id;
        let list = |allow, spec: &PackSpec| pack_filters(Some(&host), &jobs, allow, project, c, spec).0;
        let gone = PackSpec { plugin_filters: vec![SPEAKS.into(), "dev.a.gone/loud".into()], ..PackSpec::default() };
        // Active: its filter, named by its plugin, with the basis of its settings.
        let shown = list(true, &PackSpec::default());
        assert_eq!(shown.len(), 1);
        let f = &shown[0];
        assert_eq!(
            (f.key.as_str(), f.title.as_str(), f.reason.as_str(), f.plugin.as_str()),
            (SPEAKS, "Speaks", "No speech", "Speech Filter")
        );
        assert_eq!((f.state, f.basis.len()), (PackFilterState::Ready, 64));
        // Not active (a loaded folder while the setting is off): only when the spec turns it on, off.
        assert!(list(false, &PackSpec::default()).is_empty());
        let off = list(false, &gone);
        assert_eq!(
            off.iter().map(|f| (f.key.as_str(), f.state)).collect::<Vec<_>>(),
            [(SPEAKS, PackFilterState::Off), ("dev.a.gone/loud", PackFilterState::Gone)]
        );
        // There is nothing to turn on, and its id is no name.
        assert_eq!((off[1].title.as_str(), off[1].plugin.as_str()), ("Removed filter", ""));
        // Turned off: off too. Without plugins at all, each the spec turns on is off.
        host.store.set_enabled(&id, false).unwrap();
        assert_eq!(list(true, &gone)[0].state, PackFilterState::Off);
        let none = pack_filters(None, &jobs, true, project, c, &gone).0;
        assert_eq!(none.iter().map(|f| f.state).collect::<Vec<_>>(), [PackFilterState::Off; 2]);
        // An update that took the filter away from a plugin that is on: gone too.
        host.store.set_enabled(&id, true).unwrap();
        let mut without = manifest();
        without["contributes"]["shotFilters"] = serde_json::json!([]);
        std::fs::write(tmp.join("dev.a.speech").join("plugin.json"), without.to_string()).unwrap();
        assert_eq!(list(true, &gone)[0].state, PackFilterState::Gone);
        drop((host, lib));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

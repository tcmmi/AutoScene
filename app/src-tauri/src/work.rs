//! Background work: ingest, playback proxies, analysis (shots, faces and —
//! when the setting asks for it — people), exports and upkeep,
//! queued on `as-jobs` workers. Each job reports through `job:*` events and announces
//! what it changed (`media:changed`, `analysis:changed`, `library:changed`,
//! `export:changed`, `cast:changed`, `portraits:changed`) so the UI refetches
//! exactly what moved.

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use as_api::{
    AnalysisStage, CastEdit, ContentKind, ExportSummary, JobInfo, JobKind, JobState, MediaItem, MediaStatus,
    ProxyState, RescanRequest, StageState,
};
use as_core::{CancelToken, ExportId, MediaId, Msg, ProjectId, ToMsg, msg};
use as_engine::protocol::{Blob, RescanFaceIn};
use as_engine::{Devices, EngineError, FacesEvent, FacesInput, RescanEvent, RescanInput, ShotsEvent};
use as_jobs::{Class, JobCtx, JobError, JobResult, JobRunner, JobSpec};
use as_models::ModelError;
use as_project::{DoneRead, Pass, Portrait, Project, ProjectError, RescanCast, StoppedStage};
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

use crate::events::{AnalysisChanged, CastChanged, ExportChanged, LibraryChanged, PortraitsChanged, RescanChanged};
use crate::state::{AnalysisRuns, AppState};

pub(crate) fn job_error(e: ProjectError) -> JobError {
    match e {
        ProjectError::Cancelled => JobError::Cancelled,
        e => {
            log::warn!("{}", e.detail());
            JobError::Failed(e.msg())
        }
    }
}

fn media_title(m: &MediaItem) -> String {
    if m.label.is_empty() { m.file_name.clone() } else { format!("{} · {}", m.label, m.file_name) }
}

fn media_changed(app: &AppHandle, project: &Project, media_id: Option<MediaId>) {
    crate::events::emit_media_changed(app, project.id, media_id);
}

/// The cast of a project changed: the UI refetches it, and covers new since
/// get their portraits (see [`queue_portraits`]) — looked for off the
/// caller's thread: a decision in the review answers at once.
pub fn cast_changed(app: &AppHandle, project: &Arc<Project>) {
    let _ = CastChanged { project_id: project.id }.emit(app);
    let (app, project) = (app.clone(), project.clone());
    tauri::async_runtime::spawn_blocking(move || queue_portraits(&app, &project));
}

/// Decisions come in runs (the review answers a face per key press): the
/// other faces are placed again once they pause this long.
const PLACE_FACES_AFTER: std::time::Duration = std::time::Duration::from_millis(1200);

/// The user decided about the cast. The UI shows the decision at once; when
/// it changed who a face is, the other faces follow a moment after the last
/// such decision ([`Project::reassign`]), and the UI refetches again.
pub fn cast_edited(app: &AppHandle, project: &Arc<Project>, edit: &CastEdit) {
    cast_changed(app, project);
    if !edit.moves_faces() {
        return;
    }
    let state = app.state::<AppState>();
    let decision = {
        let mut decisions = state.decisions.lock();
        let n = decisions.entry(project.id).or_default();
        *n += 1;
        *n
    };
    // Only the id waits: a project closed or deleted meanwhile isn't held open.
    let (app, id) = (app.clone(), project.id);
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(PLACE_FACES_AFTER).await;
        let state = app.state::<AppState>();
        if state.decisions.lock().get(&id).copied() != Some(decision) {
            return;
        }
        let lib = state.library.clone();
        let placed = tauri::async_runtime::spawn_blocking(move || {
            let project = lib.project(id)?;
            project.reassign().map(|()| project)
        })
        .await;
        match placed {
            Ok(Ok(project)) => cast_changed(&app, &project),
            Ok(Err(e)) => log::warn!("placing faces after a decision: {}", e.detail()),
            Err(e) => log::warn!("placing faces after a decision: {e}"),
        }
    });
}

/// Refresh the project's Library card after its media changed.
pub fn library_changed(app: &AppHandle, project: &Project) {
    let state = app.state::<AppState>();
    if let Err(e) = state.library.refresh_stats(project) {
        log::warn!("refreshing project stats: {}", e.detail());
    }
    let _ = LibraryChanged { project_id: Some(project.id) }.emit(app);
}

/// Runs `then` when the job it is moved into is dropped without having
/// started (cancelled while queued, refused as a duplicate, dropped by a
/// shutdown); [`Unstarted::started`] disarms it.
struct Unstarted<F: FnOnce()>(Option<F>);

impl<F: FnOnce()> Unstarted<F> {
    fn started(mut self) {
        self.0 = None;
    }
}

impl<F: FnOnce()> Drop for Unstarted<F> {
    fn drop(&mut self) {
        if let Some(then) = self.0.take() {
            then();
        }
    }
}

/// Index a media (probe, frame index, poster). Queues its preview proxy if it needs one.
pub fn queue_ingest(app: &AppHandle, project: &Arc<Project>, media_id: MediaId) {
    let state = app.state::<AppState>();
    let Ok(m) = project.media(media_id) else { return };
    let spec = JobSpec {
        kind: JobKind::Ingest,
        class: Class::Media,
        title: msg!("job.indexing", "Indexing {{name}}", name = media_title(&m)),
        subtitle: project.title().ok(),
        project_id: Some(project.id),
        media_id: Some(media_id),
        export_id: None,
    };
    let unstarted = {
        let (app, project) = (app.clone(), project.clone());
        Unstarted(Some(move || ingest_dropped(&app, &project, media_id)))
    };
    let (app2, project) = (app.clone(), project.clone());
    let indexing = move |j: &JobInfo| j.kind == JobKind::Ingest && j.media_id == Some(media_id);
    state.jobs.submit_unless(indexing, spec, move |ctx| {
        unstarted.started();
        let _awake = crate::keep_awake::hold();
        let state = app2.state::<AppState>();
        let tools = state.tools().map_err(|e| JobError::Failed(e.msg))?;
        let language = state.library.audio_language().unwrap_or_else(|e| {
            log::warn!("preferred audio language: {}", e.detail());
            String::new()
        });
        let result = project.ingest(&tools, media_id, &language, &ctx.cancel, &|stage, p, frames| match frames {
            Some(frames) => ctx.frames(stage, p, frames),
            None => ctx.progress(stage, p),
        });
        if matches!(result, Err(ProjectError::Cancelled))
            && !state.jobs.is_shutting_down()
            && let Err(e) = project.set_media_failed(media_id, &indexing_cancelled())
        {
            log::warn!("marking a cancelled ingest: {}", e.detail());
        }
        media_changed(&app2, &project, Some(media_id));
        library_changed(&app2, &project);
        let m = result.map_err(job_error)?;
        for poster in [&m.poster, &m.poster_small].into_iter().flatten() {
            crate::state::allow_asset(&app2, std::path::Path::new(poster));
        }
        if m.proxy == ProxyState::Pending {
            queue_proxy(&app2, &project, media_id);
        }
        queue_analysis(&app2, &project, media_id);
        // A file that was missing is back (its faces' covers can get
        // portraits), or its analysis went with a new frame index.
        cast_changed(&app2, &project);
        Ok(())
    });
}

fn indexing_cancelled() -> Msg {
    msg!("episode.indexingCancelled", "Indexing was cancelled.")
}

/// An ingest dropped before it started. Cancelled (from Activity), the
/// media says so and offers Retry, as when it is stopped while indexing —
/// not "Queued" with nothing to index it, and not indexed anyway on the
/// next launch. A shutdown leaves it queued for the next launch, and a
/// duplicate refused leaves it to the job that is there.
fn ingest_dropped(app: &AppHandle, project: &Project, media_id: MediaId) {
    let state = app.state::<AppState>();
    if state.jobs.is_shutting_down()
        || state.jobs.is_active(|j| j.kind == JobKind::Ingest && j.media_id == Some(media_id))
        || !project.media(media_id).is_ok_and(|m| m.status == MediaStatus::Queued)
    {
        return;
    }
    match project.set_media_failed(media_id, &indexing_cancelled()) {
        Ok(()) => media_changed(app, project, Some(media_id)),
        Err(e) => log::warn!("marking a cancelled ingest: {}", e.detail()),
    }
}

/// Why an analysis stage stopped.
enum StageError {
    Cancelled,
    /// Something about this media or the analysis itself.
    Failed(Msg),
    /// The environment (offline model download, an engine that can't start):
    /// the stage waits as pending and runs again on the next launch.
    Later(Msg),
    /// A panic while the stage wasn't recorded running (before it started,
    /// or once it was done, grouping the cast): the stage stays as it is
    /// recorded, the job fails.
    Bug(Msg),
}

impl From<StageError> for JobError {
    fn from(e: StageError) -> Self {
        match e {
            StageError::Cancelled => JobError::Cancelled,
            StageError::Failed(m) | StageError::Later(m) | StageError::Bug(m) => JobError::Failed(m),
        }
    }
}

/// Run an analysis stage; a panic in it (a bug in the app's own code: the
/// engine runs in a process of its own) ends it like a failure. A stage
/// recorded running (`recorded`) is recorded failed with what the panic
/// said: the job's own catch would leave it "running" until the next launch,
/// re-run it on every launch, and drop a restart asked for meanwhile.
fn guarded(
    recorded: impl FnOnce() -> Option<StageState>,
    run: impl FnOnce() -> Result<(), StageError>,
) -> Result<(), StageError> {
    catch_unwind(AssertUnwindSafe(run)).unwrap_or_else(|panic| {
        let said = as_jobs::panic_message(&*panic);
        log::error!("analysis panicked: {said}");
        let said = Msg::plain(said);
        match recorded() {
            Some(StageState::Running) => Err(StageError::Failed(said)),
            _ => Err(StageError::Bug(said)),
        }
    })
}

fn stage_error(e: ProjectError) -> StageError {
    match e {
        ProjectError::Cancelled => StageError::Cancelled,
        e => {
            log::warn!("{}", e.detail());
            StageError::Failed(e.msg())
        }
    }
}

fn engine_error(e: EngineError) -> StageError {
    match e {
        EngineError::Cancelled => StageError::Cancelled,
        e @ EngineError::Start { .. } => {
            log::warn!("analysis: {}", e.detail());
            StageError::Later(e.msg())
        }
        e => {
            log::warn!("analysis: {}", e.detail());
            StageError::Failed(e.msg())
        }
    }
}

/// Record how a stage ended (`set(state, error)`); a quit leaves it
/// `running` so it resumes on the next launch.
fn record_stage(
    state: &AppState,
    r: &Result<(), StageError>,
    set: impl FnOnce(StageState, Option<&Msg>) -> as_project::ProjectResult<()>,
) {
    let outcome = match r {
        Ok(()) | Err(StageError::Bug(_)) => return,
        Err(StageError::Cancelled) if state.jobs.is_shutting_down() => return,
        // Stopped by the user: stays stopped (not resumed on the next launch).
        Err(StageError::Cancelled) => set(StageState::Stopped, None),
        Err(StageError::Later(msg)) => set(StageState::Pending, Some(msg)),
        Err(StageError::Failed(msg)) => set(StageState::Failed, Some(msg)),
    };
    if let Err(e) = outcome {
        log::warn!("recording the analysis state: {}", e.detail());
    }
}

/// Media whose faces another embedder found (another model, or the other of
/// Fast Scan and the full pass) are found again: embeddings of two models
/// can't be grouped together. Their decisions wait in
/// `track_edits`. Returns them (pending now, not queued). Only call it once
/// the current embedder is installed, so the old faces stay until then.
fn refresh_outdated_faces(
    app: &AppHandle,
    project: &Arc<Project>,
    model: &str,
) -> as_project::ProjectResult<Vec<MediaId>> {
    let ids = project.refresh_outdated_faces(model)?;
    if !ids.is_empty() {
        let title = project.title().unwrap_or_default();
        log::info!("'{title}': {} media get their faces found with the new model", ids.len());
        media_changed(app, project, None);
        library_changed(app, project);
        cast_changed(app, project);
    }
    Ok(ids)
}

/// Analyse a media in the engine: shots, then faces, then the project's
/// cast is clustered again. Resumes from checkpoints; downloads the models
/// it needs first. Nothing happens when its analysis is already queued or
/// running.
/// Analyse again the media whose faces don't fit the project any more: an
/// older embedder found them, or another search than its footage's (3D
/// animation is searched harder). Their old faces stay until then.
pub fn queue_outdated_faces(app: &AppHandle, project: &Arc<Project>) {
    match project.faces_by_other_model(project.face_embedder()) {
        Ok(ids) => {
            for id in ids {
                if project.media(id).is_ok_and(|m| m.status != MediaStatus::Missing) {
                    queue_analysis(app, project, id);
                }
            }
        }
        Err(e) => log::warn!("checking for outdated faces: {}", e.detail()),
    }
}

/// Fast Scan or the footage was switched: the faces passes (and the people
/// passes after them) that run the old way start over the new way at once,
/// instead of finishing the old way first. A pass still waiting reads the
/// new way when it gets there; one the user cancelled stays cancelled.
pub fn restart_outdated_passes(app: &AppHandle, project: &Project) {
    let ids = match project.outdated_passes() {
        Ok(ids) => ids,
        Err(e) => {
            log::warn!("checking for passes running the old way: {}", e.detail());
            return;
        }
    };
    let state = app.state::<AppState>();
    let restarted = start_over_now(&state.jobs, &state.analysis, &ids);
    if !restarted.is_empty() {
        let title = project.title().unwrap_or_default();
        log::info!("'{title}': {} media find their faces again the new way at once", restarted.len());
    }
}

/// The analyses of `ids` that run are handed a restart from their faces and
/// stopped: each starts over once it has stopped writing. Only an analysis
/// that runs, that nobody cancelled, and that has registered (see
/// [`AnalysisRuns`]); one that hasn't yet finds its faces the new way.
/// Returns the media started over.
fn start_over_now(jobs: &JobRunner, runs: &parking_lot::Mutex<AnalysisRuns>, ids: &[MediaId]) -> Vec<MediaId> {
    // Held throughout: a run can't unregister between the check and the cancel.
    let mut runs = runs.lock();
    ids.iter()
        .copied()
        .filter(|&media| {
            let runs_it =
                |j: &JobInfo| j.kind == JobKind::Analyze && j.media_id == Some(media) && j.state == JobState::Running;
            if !jobs.is_under_way(runs_it) || !hand_over_restart(&mut runs, media, AnalysisStage::Faces) {
                return false;
            }
            jobs.cancel_where(runs_it);
            true
        })
        .collect()
}

/// Ask the analysis of `media` that runs to clear the results from `stage`
/// on and analyse again once it has stopped (see [`Running::finish`]); a
/// restart from the shots wins over one from the faces. False when none
/// runs (or it hasn't registered yet).
pub fn hand_over_restart(runs: &mut AnalysisRuns, media: MediaId, stage: AnalysisStage) -> bool {
    if !runs.running.contains(&media) {
        return false;
    }
    let earliest = match runs.redo.get(&media) {
        Some(AnalysisStage::Shots) => AnalysisStage::Shots,
        _ => stage,
    };
    runs.redo.insert(media, earliest);
    true
}

pub fn queue_analysis(app: &AppHandle, project: &Arc<Project>, media_id: MediaId) {
    submit_analysis(app, project, media_id, true);
}

/// Unregisters a running analysis however its job ends (a panic in a stage
/// ends that stage as failed, and the restart owed is made; one anywhere
/// else drops it: the stages stay as they were and can be retried).
struct Running<'a> {
    state: &'a AppState,
    media: MediaId,
}

impl Running<'_> {
    /// The job is done writing: the restart asked for meanwhile, if any.
    fn finish(self) -> Option<AnalysisStage> {
        let mut runs = self.state.analysis.lock();
        runs.running.remove(&self.media);
        let redo = runs.redo.remove(&self.media);
        drop(runs);
        std::mem::forget(self);
        redo
    }
}

impl Drop for Running<'_> {
    fn drop(&mut self) {
        let mut runs = self.state.analysis.lock();
        runs.running.remove(&self.media);
        runs.redo.remove(&self.media);
    }
}

/// Queue an analysis job for a media; `only_one`: unless one is queued or
/// running already. (A duplicate finds its stages done and ends at once:
/// one analysis runs at a time.)
fn submit_analysis(app: &AppHandle, project: &Arc<Project>, media_id: MediaId, only_one: bool) {
    let state = app.state::<AppState>();
    let Ok(m) = project.media(media_id) else { return };
    let spec = JobSpec {
        kind: JobKind::Analyze,
        class: Class::Analysis,
        title: msg!("job.analysing", "Analysing {{name}}", name = media_title(&m)),
        subtitle: project.title().ok(),
        project_id: Some(project.id),
        media_id: Some(media_id),
        export_id: None,
    };
    let (app2, project) = (app.clone(), project.clone());
    let analysing = move |j: &JobInfo| only_one && j.kind == JobKind::Analyze && j.media_id == Some(media_id);
    state.jobs.submit_unless(analysing, spec, move |ctx| {
        let state = app2.state::<AppState>();
        state.analysis.lock().running.insert(media_id);
        let running = Running { state: &state, media: media_id };
        // Before it runs: whether it analyses anything plugins are told of.
        let due = analysis_due(&project, media_id);
        let _awake = crate::keep_awake::hold();
        let notify = || {
            let _ = AnalysisChanged { project_id: project.id, media_id }.emit(&app2);
            media_changed(&app2, &project, Some(media_id));
        };
        let recorded = |stage: fn(&MediaItem) -> StageState| project.media(media_id).ok().map(|m| stage(&m));
        let shots = guarded(|| recorded(|m| m.shots), || run_shots(&app2, &state, &project, media_id, ctx));
        record_stage(&state, &shots, |s, e| project.set_shots_state(media_id, s, e));
        notify();
        let result = shots.and_then(|()| {
            let faces = guarded(|| recorded(|m| m.faces), || run_faces(&app2, &state, &project, media_id, ctx));
            record_stage(&state, &faces, |s, e| project.set_faces_state(media_id, s, e));
            notify();
            faces?;
            if !project.finds_bodies() {
                return Ok(());
            }
            let bodies = guarded(|| recorded(|m| m.bodies), || run_bodies(&app2, &state, &project, media_id, ctx));
            record_stage(&state, &bodies, |s, e| project.set_bodies_state(media_id, s, e));
            notify();
            bodies
        });
        // Asked to start over while this ran: now nothing writes any more.
        // (The new analysis is the one plugins are told of.)
        if let Some(stage) = running.finish() {
            restart_analysis(&app2, &project, media_id, stage);
            return ended_for_restart(&ctx.cancel);
        }
        if due {
            crate::plugins::analysis_worked(&state, project.id, ctx.id);
        }
        result.map_err(JobError::from)
    });
}

/// How a run ends that hands its media a restart. Stopped to start over
/// (the hand-over cancels it), it ends cancelled: Activity shows it
/// cancelled beside the new run, not done at 100 % although it was cut
/// short. A run nothing stopped ends done.
fn ended_for_restart(cancel: &CancelToken) -> JobResult {
    if cancel.is_cancelled() { Err(JobError::Cancelled) } else { Ok(()) }
}

/// The user cancelled an analysis of `media_id` before it began: its media
/// stops where it would have begun, as a cancel of a running analysis
/// leaves it ("Stopped · Continue"), instead of waiting as pending for the
/// next launch to queue it again (see [`Project::stop_unstarted_analysis`]).
/// Nothing is recorded while another analysis of it is under way or still
/// writing: that one records its own stages.
pub fn stop_unstarted_analysis(app: &AppHandle, project_id: ProjectId, media_id: MediaId) {
    let state = app.state::<AppState>();
    if analysed_otherwise(&state.jobs, &state.analysis, media_id) {
        return;
    }
    let project = match state.library.project(project_id) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("recording a cancelled analysis: {}", e.detail());
            return;
        }
    };
    let stopped = match project.stop_unstarted_analysis(media_id) {
        Ok(Some(stopped)) => stopped,
        Ok(None) => return,
        Err(e) => {
            log::warn!("recording a cancelled analysis: {}", e.detail());
            return;
        }
    };
    if stopped == StoppedStage::FacesForgotten {
        // Its faces found the old way are gone: the cast is grouped without
        // them, the user's decisions on them waiting for Continue.
        if let Err(e) = project.recluster() {
            log::warn!("grouping the cast without the forgotten faces: {}", e.detail());
        }
        library_changed(app, &project);
        cast_changed(app, &project);
    }
    let _ = AnalysisChanged { project_id, media_id }.emit(app);
    media_changed(app, &project, Some(media_id));
}

/// Is an analysis of `media` running and still writing (also one cancelled,
/// until it stops), or under way and not cancelled?
fn analysed_otherwise(jobs: &JobRunner, runs: &parking_lot::Mutex<AnalysisRuns>, media: MediaId) -> bool {
    runs.lock().running.contains(&media)
        || jobs.is_under_way(|j| j.kind == JobKind::Analyze && j.media_id == Some(media))
}

/// Whether an analysis of `media_id` has anything to do: a stage it runs
/// isn't ready, or its faces are an older embedder's or another search's.
/// One of an episode analysed already (as a folder found again queues)
/// finds nothing to do.
fn analysis_due(project: &Project, media_id: MediaId) -> bool {
    let Ok(m) = project.media(media_id) else { return false };
    let ready = |s: StageState| s == StageState::Ready;
    !ready(m.shots)
        || !ready(m.faces)
        || (project.finds_bodies() && !ready(m.bodies))
        || project.faces_by_other_model(project.face_embedder()).is_ok_and(|ids| ids.contains(&media_id))
}

/// Clear a media's results from `stage` on and analyse it again. Only call
/// it when no analysis of the media runs (see [`AnalysisRuns`]).
pub fn restart_analysis(app: &AppHandle, project: &Arc<Project>, media_id: MediaId, stage: AnalysisStage) {
    let reset = match stage {
        AnalysisStage::Shots => project.reset_shots(media_id),
        AnalysisStage::Faces => project.reset_faces(media_id),
    };
    // The cast forgets this media's faces; the user's decisions on them wait
    // for the new tracks.
    if let Err(e) = reset.and_then(|()| project.recluster().map(|_| ())) {
        log::warn!("resetting the analysis: {}", e.detail());
    }
    cast_changed(app, project);
    let _ = AnalysisChanged { project_id: project.id, media_id }.emit(app);
    media_changed(app, project, Some(media_id));
    submit_analysis(app, project, media_id, false);
}

fn run_shots(
    app: &AppHandle,
    state: &AppState,
    project: &Project,
    media_id: MediaId,
    ctx: &JobCtx,
) -> Result<(), StageError> {
    if project.media(media_id).map_err(stage_error)?.shots == StageState::Ready {
        return Ok(());
    }
    install_models(state, &[as_models::ids::TRANSNET], ctx)?;
    project.set_shots_state(media_id, StageState::Running, None).map_err(stage_error)?;
    media_changed(app, project, Some(media_id));
    let plan = project.shots_plan(media_id).map_err(stage_error)?;
    let finding = msg!("job.stage.findingShots", "Finding shots");
    if plan.resume < plan.frames {
        ctx.progress(&finding, plan.resume as f64 / plan.frames as f64);
        let mut stage = finding.clone();
        state
            .engine
            .shots(plan.path, plan.info, plan.index, plan.resume, plan.chunk, &ctx.cancel, |e| match e {
                ShotsEvent::Devices { devices, resume } => {
                    stage = devices_reported(project, media_id, Pass::Shots, &finding, &devices, resume);
                    Ok(())
                }
                ShotsEvent::Progress(f) => {
                    ctx.progress(&stage, f);
                    Ok(())
                }
                ShotsEvent::Chunk { start, signals } => {
                    project.store_shot_chunk(media_id, start, &signals).map_err(|e| e.detail())
                }
            })
            .map_err(engine_error)?;
    }
    ctx.stage(&finding);
    let shots = project.finish_shots(media_id).map_err(stage_error)?;
    log::info!("{shots} shots found (engine prefers {})", state.engine.device().unwrap_or_default());
    Ok(())
}

fn run_faces(
    app: &AppHandle,
    state: &AppState,
    project: &Arc<Project>,
    media_id: MediaId,
    ctx: &JobCtx,
) -> Result<(), StageError> {
    // Faces an older embedder found (anywhere in the project) are found
    // again before this pass adds new ones.
    let model = project.face_embedder();
    let models = as_project::face_models(model);
    let outdated = project.faces_by_other_model(model).map_err(stage_error)?;
    let ready = project.media(media_id).map_err(stage_error)?.faces == StageState::Ready;
    if ready && !outdated.contains(&media_id) {
        return Ok(());
    }
    if ready {
        // Its old faces stay until the new model is here: a failed or
        // cancelled download leaves the cast as it was (and tries again on
        // the next launch).
        if let Err(e) = install_models(state, models, ctx) {
            if !matches!(e, StageError::Cancelled) {
                log::warn!("finding faces again with the new model has to wait");
            }
            return Ok(());
        }
    } else {
        install_models(state, models, ctx)?;
    }
    if !outdated.is_empty() {
        for id in refresh_outdated_faces(app, project, model).map_err(stage_error)? {
            if id != media_id {
                queue_analysis(app, project, id);
            }
        }
    }
    // People are found in the same pass when their models are here; when
    // they can't be downloaded now, the people pass finds them later.
    // Neither the fast pass nor the anime pass look for anyone by their body.
    let mut people = model != as_models::ids::SFACE
        && !as_project::anime_embedder(model)
        && project.finds_bodies()
        && match install_models(state, &as_models::ids::BODIES, ctx) {
            Ok(()) => true,
            Err(StageError::Cancelled) => return Err(StageError::Cancelled),
            Err(_) => false,
        };
    project.set_faces_state(media_id, StageState::Running, None).map_err(stage_error)?;
    media_changed(app, project, Some(media_id));
    let finding = msg!("job.stage.findingCharacters", "Finding characters");
    loop {
        let plan = project.faces_plan(media_id, model).map_err(stage_error)?;
        if plan.resume >= plan.frames {
            break;
        }
        ctx.progress(&finding, plan.resume as f64 / plan.frames as f64);
        let job = FacesInput {
            path: plan.path,
            info: plan.info,
            index: plan.index,
            shots: plan.shots,
            resume: plan.resume,
            chunk: plan.chunk,
            bodies: people,
            known: None,
            animated: plan.animated,
            fast: plan.fast,
            anime: plan.anime,
            best: plan.best,
        };
        let mut stage = finding.clone();
        let found = state.engine.faces(job, &ctx.cancel, |e| match e {
            FacesEvent::Devices { devices, resume } => {
                stage = devices_reported(project, media_id, Pass::Faces, &finding, &devices, resume);
                Ok(())
            }
            FacesEvent::Progress(f) => {
                ctx.progress(&stage, f);
                Ok(())
            }
            FacesEvent::Chunk { start, end, tracks, sprites, bodies } => project
                .store_face_chunk(media_id, start, end, &tracks, &sprites, &bodies)
                .map(|_| ())
                .map_err(|e| e.detail()),
        });
        match found {
            Ok(()) => break,
            // The optional people never cost the faces: the faces go on
            // alone from the last checkpoint, the people pass finds the rest
            // of the people after (or fails on its own).
            Err(e @ (EngineError::Failed { .. } | EngineError::Crashed(_))) if people => {
                log::warn!("finding people with the faces: {}; the faces go on alone", e.detail());
                people = false;
            }
            Err(e) => return Err(engine_error(e)),
        }
    }
    // The project moved to the other pipeline (Fast Scan, 3D animation)
    // while this pass ran: its faces are found again the new way.
    if project.faces_outdated(media_id).map_err(stage_error)? {
        log::info!("faces are now found another way; finding them again");
        return run_faces(app, state, project, media_id, ctx);
    }
    ctx.stage(&grouping());
    project.finish_faces(media_id).map_err(stage_error)?;
    let characters = project.recluster().map_err(stage_error)?;
    log::info!("faces found; the cast has {characters} characters");
    cast_changed(app, project);
    crate::commands::anilist::look_up(app, project.id, true);
    Ok(())
}

/// What an analysis says while it groups what it found.
fn grouping() -> Msg {
    msg!("job.stage.groupingCharacters", "Grouping characters")
}

/// Find a media's people for its faces (when the setting asks for it):
/// whatever the face pass didn't find with them — all of them, for a media
/// whose faces were found before — then the user's word on them is put
/// back and every face placed again with them.
fn run_bodies(
    app: &AppHandle,
    state: &AppState,
    project: &Arc<Project>,
    media_id: MediaId,
    ctx: &JobCtx,
) -> Result<(), StageError> {
    let m = project.media(media_id).map_err(stage_error)?;
    if m.bodies == StageState::Ready || m.faces != StageState::Ready {
        return Ok(());
    }
    install_models(state, &as_models::ids::BODIES, ctx)?;
    project.set_bodies_state(media_id, StageState::Running, None).map_err(stage_error)?;
    media_changed(app, project, Some(media_id));
    let plan = project.bodies_plan(media_id).map_err(stage_error)?;
    let finding = msg!("job.stage.findingPeople", "Finding people");
    if plan.resume < plan.frames {
        ctx.progress(&finding, plan.resume as f64 / plan.frames as f64);
        let job = FacesInput {
            path: plan.path,
            info: plan.info,
            index: plan.index,
            shots: plan.shots,
            resume: plan.resume,
            chunk: plan.chunk,
            bodies: true,
            known: Some(plan.known),
            animated: false,
            fast: false,
            anime: false,
            best: false,
        };
        let mut stage = finding.clone();
        state
            .engine
            .faces(job, &ctx.cancel, |e| match e {
                FacesEvent::Devices { devices, resume } => {
                    stage = devices_reported(project, media_id, Pass::People, &finding, &devices, resume);
                    Ok(())
                }
                FacesEvent::Progress(f) => {
                    ctx.progress(&stage, f);
                    Ok(())
                }
                FacesEvent::Chunk { start, end, bodies, .. } => {
                    project.store_body_chunk(media_id, start, end, &bodies).map(|_| ()).map_err(|e| e.detail())
                }
            })
            .map_err(engine_error)?;
    }
    ctx.stage(&grouping());
    project.finish_bodies(media_id).map_err(stage_error)?;
    project.reassign().map_err(stage_error)?;
    log::info!("people found");
    cast_changed(app, project);
    Ok(())
}

/// The anime model setting came to another model here: every anime
/// project's characters are found again with it (their decisions kept).
pub fn anime_model_changed(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let Ok(projects) = state.library.projects() else { return };
        for summary in projects.into_iter().filter(|p| p.content == ContentKind::Anime) {
            match state.library.project(summary.id) {
                Ok(project) => {
                    // A pass under way keeps the model it began with: it starts again.
                    restart_outdated_passes(&app, &project);
                    queue_outdated_faces(&app, &project);
                }
                Err(e) => log::warn!("project '{}' unavailable: {}", summary.title, e.detail()),
            }
        }
    });
}

/// The setting to find people by their bodies changed: every project
/// places its faces again with (or without) them — and when it is on, the
/// media whose people aren't found yet are analysed for them.
pub fn bodies_setting_changed(app: &AppHandle, on: bool) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let projects = match state.library.projects() {
            Ok(p) => p,
            Err(e) => {
                log::warn!("listing projects: {}", e.detail());
                return;
            }
        };
        for summary in projects {
            let project = match state.library.project(summary.id) {
                Ok(p) => p,
                Err(e) => {
                    log::warn!("project '{}' unavailable: {}", summary.title, e.detail());
                    continue;
                }
            };
            if let Err(e) = project.reassign() {
                log::warn!("placing the faces of '{}' again: {}", summary.title, e.detail());
            }
            cast_changed(&app, &project);
            if on && project.finds_bodies() {
                // A people pass left "running" with nothing running it (a
                // quit or failed update install mid-way) waits too; one
                // under way isn't queued again (see `queue_analysis`).
                let waiting = project.waiting_bodies().and_then(|mut ids| {
                    for m in project.media_list_uncounted()? {
                        let stuck = m.bodies == StageState::Running && m.faces == StageState::Ready;
                        if stuck && m.status == MediaStatus::Ready && !ids.contains(&m.id) {
                            ids.push(m.id);
                        }
                    }
                    Ok(ids)
                });
                match waiting {
                    Ok(ids) => {
                        let ids: Vec<MediaId> = ids
                            .into_iter()
                            .filter(|&id| project.media(id).is_ok_and(|m| m.status != MediaStatus::Missing))
                            .collect();
                        if !ids.is_empty() {
                            log::info!("'{}': {} media get their people found", summary.title, ids.len());
                        }
                        for id in ids {
                            queue_analysis(&app, &project, id);
                        }
                    }
                    Err(e) => log::warn!("media of '{}' without their people: {}", summary.title, e.detail()),
                }
            }
        }
    });
}

/// Where a pass's models run, as the engine says when it starts on it: the
/// project records the devices with the pass's checkpoints (noting a pass
/// resumed on others), and a pass the GPU couldn't take says in its stage
/// that it runs on the CPU, many times slower. Returns the stage to show.
fn devices_reported(
    project: &Project,
    media_id: MediaId,
    pass: Pass,
    stage: &Msg,
    devices: &Devices,
    resume: u32,
) -> Msg {
    let named = &stage.text;
    match project.record_devices(media_id, pass, &devices.summary(), resume) {
        Ok(Some(before)) => log::warn!(
            "{named}: resumed at frame {resume} with {} after checkpoints made with {before}; \
             its numbers may differ from an uninterrupted run's in the last digits",
            devices.summary()
        ),
        Ok(None) => {}
        Err(e) => log::warn!("recording the devices of '{named}': {}", e.detail()),
    }
    let on_cpu = devices.on_cpu();
    if on_cpu.is_empty() {
        return stage.clone();
    }
    log::warn!("{named}: the GPU couldn't take {}; on the CPU (slower)", on_cpu.join(", "));
    on_the_cpu(stage.clone())
}

/// `stage`, saying that it runs on the CPU.
fn on_the_cpu(stage: Msg) -> Msg {
    msg!("job.stage.onCpu", "{{stage}} · on the CPU (slower)", stage = stage)
}

/// Download AI models on first use (verified; resumes an interrupted download).
fn install_models(state: &AppState, ids: &[&str], ctx: &JobCtx) -> Result<(), StageError> {
    for &id in ids {
        let installed = state.models.installed(id).map_err(|e| StageError::Failed(e.msg()))?;
        if installed.is_some() {
            continue;
        }
        let stage = msg!("job.stage.downloadingModel", "Downloading AI model");
        // Unknown until the first count: a resumed download starts at the
        // bytes it has, which mustn't count as fetched in no time.
        ctx.stage(&stage);
        state
            .models
            .install(id, &ctx.cancel, |done, total| ctx.progress(&stage, done as f64 / total.max(1) as f64))
            .map_err(|e| match e {
                ModelError::Cancelled => StageError::Cancelled,
                e => {
                    log::warn!("{}", e.detail());
                    StageError::Later(e.msg())
                }
            })?;
    }
    Ok(())
}

/// Build the playback proxy for a media whose source the webview can't play frame-accurately.
pub fn queue_proxy(app: &AppHandle, project: &Arc<Project>, media_id: MediaId) {
    let state = app.state::<AppState>();
    // A proxy for a different audio track may still be running: replace it.
    state.jobs.cancel_where(|j| j.kind == JobKind::Proxy && j.media_id == Some(media_id));
    let Ok(m) = project.media(media_id) else { return };
    let spec = JobSpec {
        kind: JobKind::Proxy,
        class: Class::Media,
        title: msg!("job.preparingPreview", "Preparing preview of {{name}}", name = media_title(&m)),
        subtitle: project.title().ok(),
        project_id: Some(project.id),
        media_id: Some(media_id),
        export_id: None,
    };
    let (app2, project) = (app.clone(), project.clone());
    state.jobs.submit(spec, move |ctx| {
        let _awake = crate::keep_awake::hold();
        let state = app2.state::<AppState>();
        let tools = state.tools().map_err(|e| JobError::Failed(e.msg))?;
        ctx.stage(&msg!("job.stage.preparingPreview", "Preparing preview"));
        let result = project.build_proxy(&tools, media_id, &ctx.cancel, &|stage, p| ctx.report(stage, p));
        media_changed(&app2, &project, Some(media_id));
        result.map_err(job_error)
    });
}

/// Make posters from before the current sizes again (sharp full-bleed
/// artwork, small files for small places), one media at a time on the upkeep
/// worker: it never holds up new work. Each media and the Library card show
/// the new pictures as soon as they exist; until then the old ones stay.
pub fn queue_posters(app: &AppHandle, project: &Arc<Project>, ids: Vec<MediaId>) {
    let state = app.state::<AppState>();
    if ids.is_empty() {
        return;
    }
    let spec = JobSpec {
        kind: JobKind::Posters,
        class: Class::Upkeep,
        title: msg!("job.sharpeningPosters", "Sharpening posters"),
        subtitle: project.title().ok(),
        project_id: Some(project.id),
        media_id: None,
        export_id: None,
    };
    let (app2, project) = (app.clone(), project.clone());
    let pid = project.id;
    let sharpening = move |j: &JobInfo| j.kind == JobKind::Posters && j.project_id == Some(pid);
    state.jobs.submit_unless(sharpening, spec, move |ctx| {
        let state = app2.state::<AppState>();
        let tools = state.tools().map_err(|e| JobError::Failed(e.msg))?;
        for (i, &id) in ids.iter().enumerate() {
            let stage =
                msg!("job.stage.poster", "Poster {{n, number}} of {{total, number}}", n = i + 1, total = ids.len());
            ctx.progress(&stage, i as f64 / ids.len() as f64);
            match project.refresh_posters(&tools, id, &ctx.cancel) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(ProjectError::Cancelled) => return Err(JobError::Cancelled),
                // The old poster stays; the next launch tries again.
                Err(e) => {
                    log::warn!("poster of {id}: {}", e.detail());
                    continue;
                }
            }
            // The Library card first: the project refetched on `media:changed`
            // must not show a picture that was just removed.
            sync_library_posters(&app2, &project);
            media_changed(&app2, &project, Some(id));
        }
        Ok(())
    });
}

/// Bring a project's cast up to date on the upkeep worker (see
/// [`Project::cast_upkeep`]): grouped by an older version of the grouping,
/// it is grouped again — the user's decisions stay, characters that turn
/// out to be one person become one — and covers made before show the
/// character, not the face in front of them.
pub fn queue_regroup(app: &AppHandle, project: &Arc<Project>) {
    let state = app.state::<AppState>();
    let spec = JobSpec {
        kind: JobKind::Regroup,
        class: Class::Upkeep,
        title: msg!("job.groupingCharacters", "Grouping characters"),
        subtitle: project.title().ok(),
        project_id: Some(project.id),
        media_id: None,
        export_id: None,
    };
    let (app2, project) = (app.clone(), project.clone());
    // One waiting is enough; one already running may have read the cast
    // (or footage) before the change that queues this.
    let pid = project.id;
    let waiting =
        move |j: &JobInfo| j.kind == JobKind::Regroup && j.project_id == Some(pid) && j.state == JobState::Queued;
    state.jobs.submit_unless(waiting, spec, move |ctx| {
        ctx.stage(&grouping());
        let regrouped = project.cast_upkeep(&ctx.cancel).map_err(job_error)?;
        if let Some(characters) = regrouped {
            log::info!("characters grouped again; the cast has {characters} characters");
        }
        cast_changed(&app2, &project);
        if regrouped.is_some() {
            crate::commands::anilist::look_up(&app2, project.id, true);
        }
        Ok(())
    });
}

/// How often the UI hears which faces Rescan read.
const RESCAN_SHOWN_EVERY: std::time::Duration = std::time::Duration::from_millis(250);

/// What Rescan keeps for the person changed.
pub fn rescan_changed(app: &AppHandle, project: &Arc<Project>) {
    let _ = RescanChanged { project_id: project.id, looked: Vec::new(), done: 0, total: 0, results: true }.emit(app);
}

/// Read faces again and say who they are, on the analysis worker (it needs
/// the engine). Nothing of the cast changes: what it finds waits for the
/// person's answer. Stopped, it says what it can of the faces it read.
pub fn queue_rescan(app: &AppHandle, project: &Arc<Project>, request: RescanRequest) {
    let state = app.state::<AppState>();
    let spec = JobSpec {
        kind: JobKind::Rescan,
        class: Class::Analysis,
        title: msg!("job.rescanning", "Rescanning faces"),
        subtitle: project.title().ok(),
        project_id: Some(project.id),
        media_id: None,
        export_id: None,
    };
    let (app, project) = (app.clone(), project.clone());
    state.jobs.submit(spec, move |ctx| {
        let state = app.state::<AppState>();
        let _awake = crate::keep_awake::hold();
        run_rescan(&app, &state, &project, &request, ctx).map_err(JobError::from)
    });
}

fn run_rescan(
    app: &AppHandle,
    state: &AppState,
    project: &Arc<Project>,
    request: &RescanRequest,
    ctx: &JobCtx,
) -> Result<(), StageError> {
    let reading = msg!("job.stage.rescanning", "Reading faces again");
    ctx.stage(&reading);
    let cast = project.rescan_cast().map_err(stage_error)?;
    let subjects = cast.subjects(request);
    let unclear = cast.unclear(&subjects);
    if !unclear.is_empty() {
        install_models(state, as_project::face_models(project.face_embedder()), ctx)?;
    }
    let (reads, unread, stopped) = read_again(app, state, project, &cast, &unclear, &reading, false, ctx)?;
    // Stopped: the faces it got to, nothing about the others.
    let asked: Vec<usize> = if stopped { reads.iter().map(|r| r.track).collect() } else { subjects };
    let tally = project.rescan_keep(&cast, &asked, &reads, unread).map_err(stage_error)?;
    log::info!("rescan: {tally:?}");
    rescan_changed(app, project);
    if stopped {
        return Err(StageError::Cancelled);
    }
    same_person(app, state, project, &cast, request, ctx)?;
    rescan_changed(app, project);
    Ok(())
}

/// The second check: characters a face could be either of, and characters
/// chosen together that look alike, are compared on their clearest faces read
/// from the source, by a second model where the footage has one.
fn same_person(
    app: &AppHandle,
    state: &AppState,
    project: &Arc<Project>,
    cast: &RescanCast,
    request: &RescanRequest,
    ctx: &JobCtx,
) -> Result<(), StageError> {
    let pairs = project.rescan_pairs_to_check(cast, request).map_err(stage_error)?;
    if pairs.is_empty() {
        return Ok(());
    }
    let comparing = msg!("job.stage.comparingCharacters", "Comparing characters");
    ctx.stage(&comparing);
    let mut faces: Vec<usize> = pairs.iter().flat_map(|p| p.a_faces.iter().chain(&p.b_faces).copied()).collect();
    faces.sort_unstable();
    faces.dedup();
    let (main, _, stopped) = read_again(app, state, project, cast, &faces, &comparing, false, ctx)?;
    if stopped {
        return Err(StageError::Cancelled);
    }
    // Faces of 3D animation and anime have no second model that knows them.
    let second = |model| {
        install_models(state, &[model], ctx)?;
        match read_again(app, state, project, cast, &faces, &comparing, true, ctx)? {
            (_, _, true) => Err(StageError::Cancelled),
            (reads, _, false) => Ok(reads),
        }
    };
    let other = match project.second_model().map(second) {
        Some(Ok(reads)) => Some(reads),
        Some(Err(StageError::Cancelled)) => return Err(StageError::Cancelled),
        // No second opinion is no failure.
        Some(Err(_)) | None => None,
    };
    let kept = project.rescan_keep_pairs(cast, &pairs, &main, other.as_deref()).map_err(stage_error)?;
    log::info!("rescan: {kept} of {} pairs may be one person", pairs.len());
    Ok(())
}

/// Read `tracks` from their files (`second`: by the other face model). What
/// was read, how many faces can't be read as their file is missing, and
/// whether it was stopped part way.
#[allow(clippy::too_many_arguments)]
fn read_again(
    app: &AppHandle,
    state: &AppState,
    project: &Arc<Project>,
    cast: &RescanCast,
    tracks: &[usize],
    reading: &Msg,
    second: bool,
    ctx: &JobCtx,
) -> Result<(Vec<DoneRead>, u32, bool), StageError> {
    let total = tracks.len() as u32;
    let (mut reads, mut unread) = (Vec::with_capacity(tracks.len()), 0u32);
    let mut failed = None;
    let (mut looked, mut told) = (Vec::new(), std::time::Instant::now());
    for media in cast.media_of(tracks) {
        let src = match project.rescan_source(media) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("rescan: {}", e.detail());
                unread += cast.faces_of(media, tracks, &[]).len() as u32;
                continue;
            }
        };
        let faces = cast
            .faces_of(media, tracks, &src.shots)
            .into_iter()
            .map(|f| RescanFaceIn {
                id: f.id,
                shot: (f.shot.start, f.shot.end),
                quality: f.quality,
                dets: Blob(cast.rows[cast.row_of[f.id as usize]].det_bytes.clone()),
            })
            .collect();
        let job = RescanInput {
            path: src.path.clone(),
            info: src.info.clone(),
            index: src.index.to_bytes(),
            faces,
            fast: src.fast,
            anime: src.anime,
            second,
        };
        let read = state.engine.rescan(job, &ctx.cancel, |e| {
            if let RescanEvent::Read { track, embedding, quality, tried, found } = e {
                let track = track as usize;
                reads.push(DoneRead { track, embedding, quality, tried, found });
                looked.push(cast.rows[cast.row_of[track]].id as u32);
                let done = reads.len() as u32 + unread;
                ctx.progress(reading, f64::from(done) / f64::from(total.max(1)));
                if told.elapsed() >= RESCAN_SHOWN_EVERY {
                    let looked = std::mem::take(&mut looked);
                    let _ = RescanChanged { project_id: project.id, looked, done, total, results: false }.emit(app);
                    told = std::time::Instant::now();
                }
            }
            Ok(())
        });
        match read {
            Ok(()) => {}
            Err(EngineError::Cancelled) => return Ok((reads, unread, true)),
            // A file that can't be read doesn't cost what was read of the others.
            Err(e) => {
                log::warn!("rescan: {}", e.detail());
                failed = Some(e);
            }
        }
    }
    if let (Some(e), true) = (failed, reads.is_empty()) {
        return Err(engine_error(e));
    }
    unread = total - reads.len() as u32;
    let done = total;
    let _ = RescanChanged { project_id: project.id, looked, done, total, results: false }.emit(app);
    Ok((reads, unread, false))
}

/// How often the UI hears of new portraits while they are being made.
const PORTRAITS_SHOWN_EVERY: std::time::Duration = std::time::Duration::from_secs(2);

/// Make the missing portraits of character covers — 3:4, from the exact
/// frame of the cover's clearest face at the source's full resolution — on
/// the upkeep worker, in the order characters were found (the main cast
/// first, mostly). A card shows its face thumbnail until its portrait
/// exists, then turns sharp.
pub fn queue_portraits(app: &AppHandle, project: &Arc<Project>) {
    let state = app.state::<AppState>();
    // A job queued or running looks again before it ends.
    if let Some(changed) = state.portraits.lock().get_mut(&project.id) {
        *changed = true;
        return;
    }
    match project.missing_portraits() {
        Ok(missing) if missing.is_empty() => return,
        Ok(_) => {}
        Err(e) => {
            log::warn!("covers without portraits: {}", e.detail());
            return;
        }
    }
    // Another change may have queued one meanwhile.
    let mut job = {
        let mut jobs = state.portraits.lock();
        if let Some(changed) = jobs.get_mut(&project.id) {
            *changed = true;
            return;
        }
        jobs.insert(project.id, false);
        PortraitsJob { jobs: state.portraits.clone(), project: project.id, ended: false }
    };
    let spec = JobSpec {
        kind: JobKind::Portraits,
        class: Class::Upkeep,
        title: msg!("job.sharpeningPortraits", "Sharpening portraits"),
        subtitle: project.title().ok(),
        project_id: Some(project.id),
        media_id: None,
        export_id: None,
    };
    let (app2, project) = (app.clone(), project.clone());
    state.jobs.submit(spec, move |ctx| {
        let state = app2.state::<AppState>();
        let tools = state.tools().map_err(|e| JobError::Failed(e.msg))?;
        // The cast is fetched again when portraits are new, at most every
        // couple of seconds: cards turn sharp in batches, not one by one.
        let changed = || {
            let _ = PortraitsChanged { project_id: project.id }.emit(&app2);
        };
        let (mut unseen, mut told) = (false, std::time::Instant::now());
        // What's missing now, and again while the cast changed meanwhile.
        loop {
            let tracks = project.missing_portraits().map_err(job_error)?;
            for (i, &track) in tracks.iter().enumerate() {
                let stage = msg!(
                    "job.stage.portrait",
                    "Portrait {{n, number}} of {{total, number}}",
                    n = i + 1,
                    total = tracks.len()
                );
                ctx.progress(&stage, i as f64 / tracks.len() as f64);
                match project.portrait(&tools, track, &ctx.cancel) {
                    Ok(Portrait::Made(_)) => unseen = true,
                    Ok(Portrait::Ready(_) | Portrait::Unavailable) => {}
                    Err(ProjectError::Cancelled) => {
                        if unseen {
                            changed();
                        }
                        return Err(JobError::Cancelled);
                    }
                    // The thumbnail stays; the next launch tries again.
                    Err(e) => log::warn!("portrait of track {track}: {}", e.detail()),
                }
                if unseen && told.elapsed() >= PORTRAITS_SHOWN_EVERY {
                    changed();
                    (unseen, told) = (false, std::time::Instant::now());
                }
            }
            if !job.look_again() {
                break;
            }
        }
        if unseen {
            changed();
        }
        Ok(())
    });
}

/// A project's portraits job as `AppState::portraits` knows it: unregistered
/// when it ends, however it ends — also when it is dropped unstarted
/// (cancelled while queued).
struct PortraitsJob {
    jobs: Arc<parking_lot::Mutex<HashMap<ProjectId, bool>>>,
    project: ProjectId,
    ended: bool,
}

impl PortraitsJob {
    /// Whether the cast changed since the job last looked; if not, the job
    /// ends — under the same lock, so a change can't slip in between.
    fn look_again(&mut self) -> bool {
        let mut jobs = self.jobs.lock();
        if jobs.get(&self.project) == Some(&true) {
            jobs.insert(self.project, false);
            return true;
        }
        jobs.remove(&self.project);
        self.ended = true;
        false
    }
}

impl Drop for PortraitsJob {
    fn drop(&mut self) {
        if !self.ended {
            self.jobs.lock().remove(&self.project);
        }
    }
}

/// Show a project's current posters on its Library card, without moving it
/// to the front.
fn sync_library_posters(app: &AppHandle, project: &Project) {
    match app.state::<AppState>().library.refresh_posters(project) {
        Ok(true) => {
            let _ = LibraryChanged { project_id: Some(project.id) }.emit(app);
        }
        Ok(false) => {}
        Err(e) => log::warn!("refreshing project posters: {}", e.detail()),
    }
}

/// Render an export created with `Project::create_export`.
pub fn queue_export(app: &AppHandle, project: &Arc<Project>, export: &ExportSummary) {
    let state = app.state::<AppState>();
    let export_id = export.id;
    let spec = JobSpec {
        kind: JobKind::Export,
        class: Class::Export,
        title: export_title(export.items.len(), export.merged.is_some()),
        subtitle: project.title().ok(),
        project_id: Some(project.id),
        media_id: None,
        export_id: Some(export_id),
    };
    let unstarted = {
        let (app, project) = (app.clone(), project.clone());
        Unstarted(Some(move || export_dropped(&app, &project, export_id)))
    };
    let (app2, project) = (app.clone(), project.clone());
    state.jobs.submit(spec, move |ctx| {
        unstarted.started();
        let _awake = crate::keep_awake::hold();
        let state = app2.state::<AppState>();
        let notify = || {
            let _ = ExportChanged { project_id: project.id, export_id }.emit(&app2);
        };
        let tools = match state.tools() {
            Ok(t) => t,
            Err(e) => {
                if let Err(fail) = project.fail_export(export_id, &e.msg) {
                    log::warn!("marking export {export_id} failed: {}", fail.detail());
                }
                notify();
                return Err(JobError::Failed(e.msg));
            }
        };
        notify();
        let run = || project.run_export(&tools, export_id, &ctx.cancel, &|stage, p| ctx.report(stage, p), &notify);
        // A bug in it fails it like an error of its own: it doesn't show
        // "Exporting" until the next launch.
        let result = catch_unwind(AssertUnwindSafe(run)).unwrap_or_else(|panic| {
            let msg = as_jobs::panic_message(&*panic);
            log::error!("export {export_id} panicked: {msg}");
            Err(ProjectError::Invalid(Msg::plain(msg)))
        });
        if let Err(e) = &result
            && !matches!(e, ProjectError::Cancelled)
            && let Err(fail) = project.abandon_export(export_id, &e.msg())
        {
            log::warn!("marking export {export_id} failed: {}", fail.detail());
        }
        notify();
        let summary = result.map_err(job_error)?;
        if let Some(error) = summary.merged.as_ref().and_then(|f| f.error.clone()) {
            return Err(JobError::Failed(error));
        }
        let failed = summary.items.iter().filter(|i| i.error.is_some()).count();
        if failed > 0 {
            return Err(JobError::Failed(failed_message(failed, summary.items.len(), summary.merged.is_some())));
        }
        // Only a whole export: what plugins run after it is queued apart,
        // and never fails it.
        crate::plugins::queue_after_export(&app2, &project, &summary);
        Ok(())
    });
}

/// An export dropped before it started (cancelled from Activity while
/// queued): it says Cancelled at once, not Queued until the next launch. A
/// shutdown leaves it to the next launch, which cancels what didn't finish.
fn export_dropped(app: &AppHandle, project: &Project, export_id: ExportId) {
    if app.state::<AppState>().jobs.is_shutting_down() {
        return;
    }
    match project.cancel_unstarted_export(export_id) {
        Ok(()) => {
            let _ = ExportChanged { project_id: project.id, export_id }.emit(app);
        }
        Err(e) => log::warn!("marking a cancelled export: {}", e.detail()),
    }
}

/// The title of the job of an export of `clips`, in `one_file` or a file each.
fn export_title(clips: usize, one_file: bool) -> Msg {
    if one_file {
        msg!(
            "job.exportingJoined",
            one = "Exporting {{count}} clip into one file",
            other = "Exporting {{count}} clips into one file",
            count = clips
        )
    } else {
        msg!("job.exporting", one = "Exporting {{count}} clip", other = "Exporting {{count}} clips", count = clips)
    }
}

/// What an export with `failed` of its `clips` failing says.
fn failed_message(failed: usize, clips: usize, one_file: bool) -> Msg {
    match (one_file, clips.saturating_sub(failed)) {
        (false, _) => msg!(
            "job.clipsFailed",
            one = "{{count}} clip failed to export.",
            other = "{{count}} clips failed to export.",
            count = failed
        ),
        (true, 1) => msg!(
            "job.clipsFailedOneLeft",
            one = "{{count}} clip couldn't be exported; the file has the other one.",
            other = "{{count}} clips couldn't be exported; the file has the other one.",
            count = failed
        ),
        (true, rest) => msg!(
            "job.clipsFailedSomeLeft",
            one = "{{count}} clip couldn't be exported; the file has the other {{rest, number}}.",
            other = "{{count}} clips couldn't be exported; the file has the other {{rest, number}}.",
            count = failed,
            rest = rest
        ),
    }
}

/// On launch (and when a shutdown for an update is undone, see
/// `state::resume_work`): reopen every project, resume interrupted
/// indexing, pending previews and analyses, make outdated posters again and
/// the portraits covers don't have yet.
pub fn recover_all(app: &AppHandle) {
    let state = app.state::<AppState>();
    let projects = match state.library.projects() {
        Ok(p) => p,
        Err(e) => {
            log::error!("listing projects: {}", e.detail());
            return;
        }
    };
    for summary in projects {
        let project = match state.library.project(summary.id) {
            Ok(p) => p,
            Err(e) => {
                log::warn!("project '{}' unavailable: {}", summary.title, e.detail());
                continue;
            }
        };
        crate::state::allow_asset_dir(app, &project.thumbs_dir());
        // A quit between a poster refresh and its Library card leaves the card
        // on a removed picture; the project's own database knows better.
        sync_library_posters(app, &project);
        match project.recover() {
            Ok(r) => {
                if !r.ingest.is_empty() || !r.proxies.is_empty() || !r.posters.is_empty() {
                    log::info!(
                        "resuming '{}': {} to index, {} previews, {} posters",
                        summary.title,
                        r.ingest.len(),
                        r.proxies.len(),
                        r.posters.len()
                    );
                }
                for id in r.ingest {
                    queue_ingest(app, &project, id);
                }
                for id in r.proxies {
                    queue_proxy(app, &project, id);
                }
                queue_posters(app, &project, r.posters);
            }
            Err(e) => log::warn!("recovering '{}': {}", summary.title, e.detail()),
        }
        // Before the portraits: covers may change.
        match project.cast_needs_upkeep() {
            Ok(true) => {
                log::info!("'{}': the cast is brought up to date", summary.title);
                queue_regroup(app, &project);
            }
            Ok(false) => {}
            Err(e) => log::warn!("checking the cast of '{}': {}", summary.title, e.detail()),
        }
        queue_portraits(app, &project);
        let pending = project.pending_shots().and_then(|mut ids| {
            // Faces of an older embedder: their analysis installs the new
            // one first, then finds them again (see `run_faces`).
            for id in project.faces_by_other_model(project.face_embedder())? {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            for id in project.pending_faces()? {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            if project.finds_bodies() {
                for id in project.pending_bodies()? {
                    if !ids.contains(&id) {
                        ids.push(id);
                    }
                }
            }
            Ok(ids)
        });
        // Files that aren't there wait (pending) instead of failing.
        let pending = pending.map(|ids| {
            ids.into_iter()
                .filter(|&id| project.media(id).is_ok_and(|m| m.status != MediaStatus::Missing))
                .collect::<Vec<_>>()
        });
        match pending {
            Ok(ids) => {
                if !ids.is_empty() {
                    log::info!("resuming '{}': {} to analyse", summary.title, ids.len());
                }
                for id in ids {
                    queue_analysis(app, &project, id);
                }
            }
            Err(e) => log::warn!("recovering analysis of '{}': {}", summary.title, e.detail()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::sync::mpsc;
    use std::time::Duration;

    use as_api::{AnalysisStage, JobInfo, JobKind, JobState, StageState};
    use as_core::{CancelToken, JobId, MediaId, Msg, MsgArg, msg};
    use as_jobs::{Class, JobError, JobEvent, JobRunner, JobSpec};

    use super::{
        StageError, Unstarted, analysed_otherwise, ended_for_restart, export_title, failed_message, guarded,
        hand_over_restart, on_the_cpu, stage_error, start_over_now,
    };
    use crate::state::AnalysisRuns;

    #[test]
    fn a_stage_that_panics_fails_only_while_it_runs() {
        let breaks = || -> Result<(), StageError> { panic!("grouping broke") };
        let r = guarded(|| Some(StageState::Running), breaks);
        assert!(matches!(r, Err(StageError::Failed(m)) if m.text == "grouping broke"), "recorded failed");
        // Done before it broke (or not begun): it stays as it is recorded.
        let r = guarded(|| Some(StageState::Ready), breaks);
        assert!(matches!(r, Err(StageError::Bug(m)) if m.text == "grouping broke"));
        assert!(matches!(guarded(|| unreachable!(), || Err(StageError::Cancelled)), Err(StageError::Cancelled)));
    }

    #[test]
    fn a_stage_that_fails_keeps_the_key_of_what_went_wrong() {
        let said = as_core::msg!("episode.stillIndexing", "Still indexing.");
        let r = stage_error(as_project::ProjectError::NotReady(said.clone()));
        assert!(matches!(r, StageError::Failed(m) if m == said));
        assert!(matches!(stage_error(as_project::ProjectError::Cancelled), StageError::Cancelled));
    }

    #[test]
    fn a_run_stopped_to_start_over_ends_cancelled() {
        // The hand-over cancels the run it restarts; a run nothing stopped is done.
        let cut = CancelToken::new();
        cut.cancel();
        assert!(matches!(ended_for_restart(&cut), Err(JobError::Cancelled)));
        assert!(ended_for_restart(&CancelToken::new()).is_ok());

        // How Activity shows each: the cut run stopped, not done at 100 %.
        let (tx, rx) = mpsc::channel();
        let jobs = JobRunner::new(&[(Class::Analysis, 1)], move |e| drop(tx.send(e)));
        let spec = || JobSpec {
            kind: JobKind::Analyze,
            class: Class::Analysis,
            title: Msg::plain("Analysing Pilot"),
            subtitle: None,
            project_id: None,
            media_id: None,
            export_id: None,
        };
        let finished = |id: JobId| -> JobInfo {
            loop {
                match rx.recv_timeout(Duration::from_secs(10)).expect("job event") {
                    JobEvent::Changed(i) if i.id == id && i.state.is_finished() => return i,
                    _ => {}
                }
            }
        };
        let (started_tx, started) = mpsc::channel();
        let stopped = jobs.submit(spec(), move |ctx| {
            started_tx.send(()).unwrap();
            while !ctx.cancel.is_cancelled() {
                std::thread::sleep(Duration::from_millis(5));
            }
            ended_for_restart(&ctx.cancel)
        });
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        jobs.cancel(stopped);
        let info = finished(stopped);
        assert_eq!(info.state, JobState::Cancelled);
        assert_ne!(info.progress, Some(1.0));
        let whole = jobs.submit(spec(), |ctx| ended_for_restart(&ctx.cancel));
        assert_eq!(finished(whole).state, JobState::Done);
    }

    #[test]
    fn a_cancel_before_it_began_leaves_the_stages_to_an_analysis_under_way() {
        let jobs = JobRunner::new(&[(Class::Analysis, 1)], |_| {});
        let runs = parking_lot::Mutex::new(AnalysisRuns::default());
        let (media, other) = (MediaId::new(), MediaId::new());
        let analysis = |m: MediaId| JobSpec {
            kind: JobKind::Analyze,
            class: Class::Analysis,
            title: Msg::plain("Analysing"),
            subtitle: None,
            project_id: None,
            media_id: Some(m),
            export_id: None,
        };
        assert!(!analysed_otherwise(&jobs, &runs, media), "nothing else analyses it");
        let (started_tx, started) = mpsc::channel();
        let (stop_tx, stop) = mpsc::channel::<()>();
        let running = jobs.submit(analysis(media), move |_| {
            started_tx.send(()).unwrap();
            stop.recv_timeout(Duration::from_secs(10)).unwrap();
            Err(JobError::Cancelled)
        });
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        let waiting = jobs.submit(analysis(other), |_| Ok(()));
        assert!(analysed_otherwise(&jobs, &runs, media), "running");
        assert!(analysed_otherwise(&jobs, &runs, other), "waiting");
        // Cancelled, it only stops: it counts while it still writes its stages.
        jobs.cancel(running);
        assert!(!analysed_otherwise(&jobs, &runs, media));
        runs.lock().running.insert(media);
        assert!(analysed_otherwise(&jobs, &runs, media), "still writing its stages");
        assert!(jobs.cancel(waiting).is_some());
        assert!(!analysed_otherwise(&jobs, &runs, other));
        stop_tx.send(()).unwrap();
    }

    #[test]
    fn a_restart_goes_to_the_analysis_that_runs_and_from_the_earliest_stage() {
        use AnalysisStage::{Faces, Shots};
        let mut runs = AnalysisRuns::default();
        let (media, other) = (MediaId::new(), MediaId::new());
        assert!(!hand_over_restart(&mut runs, media, Faces), "none runs: the caller restarts it");
        assert!(runs.redo.is_empty());
        runs.running.insert(media);
        assert!(hand_over_restart(&mut runs, media, Faces));
        assert_eq!(runs.redo.get(&media), Some(&Faces));
        // From the shots wins, whichever came first: they come before the faces.
        assert!(hand_over_restart(&mut runs, media, Shots));
        assert!(hand_over_restart(&mut runs, media, Faces));
        assert_eq!(runs.redo.get(&media), Some(&Shots));
        assert!(!hand_over_restart(&mut runs, other, Shots), "only to its own analysis");
        assert_eq!(runs.redo.len(), 1);
    }

    #[test]
    fn only_passes_under_way_and_not_cancelled_start_over() {
        let jobs = JobRunner::new(&[(Class::Analysis, 3)], |_| {});
        let runs = parking_lot::Mutex::new(AnalysisRuns::default());
        let (started_tx, started) = mpsc::channel();
        let mut stops = Vec::new();
        let mut analyse = |media| {
            let (stop_tx, stop) = mpsc::channel::<()>();
            stops.push(stop_tx);
            let started_tx = started_tx.clone();
            let spec = JobSpec {
                kind: JobKind::Analyze,
                class: Class::Analysis,
                title: Msg::plain("Analysing"),
                subtitle: None,
                project_id: None,
                media_id: Some(media),
                export_id: None,
            };
            jobs.submit(spec, move |ctx| {
                started_tx.send(()).unwrap();
                let _ = stop.recv_timeout(Duration::from_secs(10));
                if ctx.cancel.is_cancelled() { Err(JobError::Cancelled) } else { Ok(()) }
            })
        };
        let [running, cancelled, unregistered, waiting, idle] = std::array::from_fn(|_| MediaId::new());
        analyse(running);
        let by_user = analyse(cancelled);
        analyse(unregistered);
        for _ in 0..3 {
            started.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        analyse(waiting);
        // Each registers as it begins its stages; the person cancelled one,
        // which still writes them as it stops.
        runs.lock().running.extend([running, cancelled]);
        assert!(jobs.cancel(by_user).is_none());

        let ids = [running, cancelled, unregistered, waiting, idle];
        assert_eq!(start_over_now(&jobs, &runs, &ids), [running]);
        // Handed its restart from the faces, and stopped.
        assert_eq!(runs.lock().redo.get(&running), Some(&AnalysisStage::Faces));
        assert!(!jobs.is_under_way(|j| j.media_id == Some(running)));
        assert!(jobs.is_active(|j| j.media_id == Some(running)), "it stops on its own");
        // The cancelled one isn't started again; the others go on and find
        // their faces the new way themselves.
        assert_eq!(runs.lock().redo.len(), 1);
        assert!(jobs.is_under_way(|j| j.media_id == Some(unregistered) && j.state == JobState::Running));
        assert!(jobs.is_under_way(|j| j.media_id == Some(waiting) && j.state == JobState::Queued));
        assert!(start_over_now(&jobs, &runs, &ids).is_empty(), "once");

        jobs.cancel_where(|_| true);
        for stop in stops {
            let _ = stop.send(());
        }
    }

    #[test]
    fn work_dropped_unstarted_says_so() {
        let told = Cell::new(0);
        drop(Unstarted(Some(|| told.set(told.get() + 1))));
        Unstarted(Some(|| told.set(told.get() + 10))).started();
        assert_eq!(told.get(), 1, "only the work that never started");
    }

    #[test]
    fn failures_say_what_the_file_still_has() {
        assert_eq!(failed_message(1, 3, false).text, "1 clip failed to export.");
        assert_eq!(failed_message(2, 3, false).text, "2 clips failed to export.");
        assert_eq!(failed_message(1, 2, true).text, "1 clip couldn't be exported; the file has the other one.");
        assert_eq!(failed_message(2, 2 + 1, true).text, "2 clips couldn't be exported; the file has the other one.");
        assert_eq!(failed_message(1, 3, true).text, "1 clip couldn't be exported; the file has the other 2.");
        assert_eq!(failed_message(2, 42, true).text, "2 clips couldn't be exported; the file has the other 40.");
        let said = failed_message(2, 42, true);
        assert_eq!(said.key, "job.clipsFailedSomeLeft");
        assert_eq!((&said.args["count"], &said.args["rest"]), (&MsgArg::Num(2.0), &MsgArg::Num(40.0)));
    }

    #[test]
    fn an_exports_title_counts_its_clips_in_any_language() {
        assert_eq!(export_title(1, false).text, "Exporting 1 clip");
        assert_eq!(export_title(12, false).text, "Exporting 12 clips");
        assert_eq!(export_title(1, true).text, "Exporting 1 clip into one file");
        assert_eq!(export_title(12, true).text, "Exporting 12 clips into one file");
        // The UI picks the plural form of its language from the count.
        let title = export_title(3, true);
        assert_eq!((title.key.as_str(), &title.args["count"]), ("job.exportingJoined", &MsgArg::Num(3.0)));
        assert_eq!(export_title(3, false).key, "job.exporting");
    }

    #[test]
    fn on_the_cpu_wraps_the_stage_it_slows() {
        let finding = msg!("job.stage.findingShots", "Finding shots");
        let said = on_the_cpu(finding.clone());
        assert_eq!(said.text, "Finding shots · on the CPU (slower)");
        assert_eq!(said.key, "job.stage.onCpu");
        assert_eq!(said.args["stage"], MsgArg::Msg(Box::new(finding)));
    }
}

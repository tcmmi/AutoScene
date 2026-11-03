//! Media of a project: listing, adding, audio tracks, and the player's frame map.

use std::path::Path;

use as_api::{
    AnalysisStage, FrameMap, JobInfo, JobKind, JobState, MediaItem, MediaStatus, ProxyState, Relink, RelinkSummary,
    Segment, StageState,
};
use as_core::{MediaId, ProjectId};
use as_jobs::JobRunner;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use tauri::{AppHandle, State};
use tauri_specta::Event;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::paths;
use crate::state::{AppState, allow_asset, allow_asset_dir, blocking};
use crate::work;

#[tauri::command]
#[specta::specta]
pub async fn media_list(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: ProjectId,
) -> AppResult<Vec<MediaItem>> {
    let lib = state.library.clone();
    let (list, thumbs) = blocking(move || {
        let p = lib.project(project_id)?;
        Ok((p.media_list()?, p.thumbs_dir()))
    })
    .await?;
    allow_asset_dir(&app, &thumbs);
    Ok(list)
}

#[tauri::command]
#[specta::specta]
pub async fn media_get(state: State<'_, AppState>, project_id: ProjectId, media_id: MediaId) -> AppResult<MediaItem> {
    let lib = state.library.clone();
    blocking(move || Ok(lib.project(project_id)?.media(media_id)?)).await
}

/// Add videos to a project and start indexing them. Returns how many were new.
#[tauri::command]
#[specta::specta]
pub async fn media_add(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: ProjectId,
    files: Vec<String>,
) -> AppResult<u32> {
    paths::check_all(&files)?;
    let lib = state.library.clone();
    let (project, added) = blocking(move || {
        let p = lib.project(project_id)?;
        let kind = lib.summary(project_id)?.kind;
        let added = lib.add_media(&p, &files, kind)?;
        Ok((p, added))
    })
    .await?;
    for &id in &added {
        work::queue_ingest(&app, &project, id);
    }
    work::library_changed(&app, &project);
    crate::events::emit_media_changed(&app, project_id, None);
    Ok(added.len() as u32)
}

#[tauri::command]
#[specta::specta]
pub async fn media_remove(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: ProjectId,
    media_id: MediaId,
) -> AppResult<()> {
    // Its work stops first: nothing may go on writing its posters, preview
    // or faces once it is gone (and a relink of it waits).
    let _one_at_a_time = RELINKING.lock().await;
    stop_work_on(&state, &[media_id]).await?;
    let lib = state.library.clone();
    let project = blocking(move || {
        let p = lib.project(project_id)?;
        p.remove_media(media_id)?;
        // Its faces are gone: characters only it had go too.
        if let Err(e) = p.recluster() {
            log::warn!("grouping the cast after removing a media: {}", e.detail());
        }
        Ok(p)
    })
    .await?;
    work::library_changed(&app, &project);
    crate::events::emit_media_changed(&app, project_id, Some(media_id));
    work::cast_changed(&app, &project);
    Ok(())
}

/// Index a media again (after a failure, or once a missing file is back).
#[tauri::command]
#[specta::specta]
pub async fn media_retry(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: ProjectId,
    media_id: MediaId,
) -> AppResult<()> {
    // An indexing under way does it already, and the episode must not read
    // "queued" under it.
    if under_way(&state.jobs, JobKind::Ingest, media_id) {
        return Ok(());
    }
    let lib = state.library.clone();
    let project = blocking(move || {
        let p = lib.project(project_id)?;
        p.set_media_queued(media_id)?;
        Ok(p)
    })
    .await?;
    work::queue_ingest(&app, &project, media_id);
    crate::events::emit_media_changed(&app, project_id, Some(media_id));
    Ok(())
}

/// Point an episode whose file moved at `path`. The same file keeps
/// everything; another copy is indexed again (and keeps its analysis when
/// its frames are the same); another video comes back as `different`
/// unless `force`.
#[tauri::command]
#[specta::specta]
pub async fn media_relink(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: ProjectId,
    media_id: MediaId,
    path: String,
    force: bool,
) -> AppResult<Relink> {
    paths::check(&path)?;
    let lib = state.library.clone();
    let tools = state.tools()?;
    let _one_at_a_time = RELINKING.lock().await;
    let interrupted = stop_work_on(&state, &[media_id]).await?;
    let (project, relink) = blocking(move || {
        let p = lib.project(project_id)?;
        let r = p.relink(&tools, media_id, Path::new(&path), force);
        // Whatever the answer: what the relink stopped goes on.
        resume_interrupted(&p, &interrupted);
        Ok((p, r?))
    })
    .await?;
    match relink {
        Relink::Different { .. } => return Ok(relink),
        // Indexed again (the ingest queues the analysis).
        Relink::Reindex => work::queue_ingest(&app, &project, media_id),
        Relink::Same => found_as_it_was(&app, &project, media_id),
    }
    work::library_changed(&app, &project);
    crate::events::emit_media_changed(&app, project_id, Some(media_id));
    // Its faces can be shown and made into portraits again, or went with
    // an analysis the relink reset.
    work::cast_changed(&app, &project);
    Ok(relink)
}

/// How long a relink or a removal waits for the work it stops: longer than
/// a cancelled analysis may take to let go of the engine (10 s).
const STOP_WAIT: std::time::Duration = std::time::Duration::from_secs(20);

/// One relink (of a file or a folder) or removal at a time: one deciding
/// which files are missing while another is queueing work for one it found
/// would stop that work again.
static RELINKING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Cancel the jobs `of` matches and wait until they have ended and `busy`
/// is false, for up to [`STOP_WAIT`]; `still` says what is still stopping
/// when it takes longer (and nothing has been changed). Work queued for them
/// meanwhile (a restart a stopped analysis owed, a new face model's pass,
/// portraits) is cancelled too.
pub(crate) async fn stop_jobs(
    state: &AppState,
    of: impl Fn(&JobInfo) -> bool + Clone + Send + 'static,
    busy: impl Fn() -> bool,
    still: &str,
) -> AppResult<()> {
    let until = std::time::Instant::now() + STOP_WAIT;
    loop {
        // Off the async runtime: a cancelled ingest records that it was
        // (see `work::queue_ingest`).
        let (jobs, of) = (state.jobs.clone(), of.clone());
        let active = blocking(move || {
            jobs.cancel_where(&of);
            Ok(jobs.is_active(&of))
        })
        .await?;
        if !active && !busy() {
            return Ok(());
        }
        if std::time::Instant::now() > until {
            return Err(AppError::new(ErrorCode::NotReady, still));
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

/// Stop every job of `media` and wait until they have ended: nothing may go
/// on reading the old path, or record its outcome over what the relink sets
/// (or write into a removed media's files). Returns the media whose analysis
/// was under way (see `resume_interrupted`).
async fn stop_work_on(state: &AppState, media: &[MediaId]) -> AppResult<Vec<MediaId>> {
    let interrupted: Vec<MediaId> = media
        .iter()
        .copied()
        .filter(|&m| state.jobs.is_active(|j| j.kind == JobKind::Analyze && j.media_id == Some(m)))
        .collect();
    let ids = media.to_vec();
    let of = move |j: &JobInfo| j.media_id.is_some_and(|m| ids.contains(&m));
    let analysing = || media.iter().any(|m| state.analysis.lock().running.contains(m));
    let still = "The work on this episode is still stopping. Try again in a moment.";
    stop_jobs(state, of, analysing, still).await?;
    Ok(interrupted)
}

/// Analysis the relink stopped is pending again, so it carries on from where
/// it was (now, or at the next launch once its file is there).
fn resume_interrupted(project: &as_project::Project, media: &[MediaId]) {
    for &id in media {
        let Ok(m) = project.media(id) else { continue };
        let resumed = (|| {
            if m.shots == StageState::Stopped {
                project.set_shots_state(id, StageState::Pending, None)?;
            }
            if m.faces == StageState::Stopped {
                project.set_faces_state(id, StageState::Pending, None)?;
            }
            Ok::<_, as_project::ProjectError>(())
        })();
        if let Err(e) = resumed {
            log::warn!("resuming the analysis of a file found again: {}", e.detail());
        }
    }
}

/// A file found as it was: its preview and analysis go on where they were.
fn found_as_it_was(app: &AppHandle, project: &std::sync::Arc<as_project::Project>, media_id: MediaId) {
    if project.media(media_id).is_ok_and(|m| m.proxy == ProxyState::Pending) {
        work::queue_proxy(app, project, media_id);
    }
    work::queue_analysis(app, project, media_id);
}

/// Look for every missing episode of a project in `dir` and its folders.
#[tauri::command]
#[specta::specta]
pub async fn media_relink_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: ProjectId,
    dir: String,
) -> AppResult<RelinkSummary> {
    paths::check(&dir)?;
    let lib = state.library.clone();
    let tools = state.tools()?;
    let _one_at_a_time = RELINKING.lock().await;
    let missing: Vec<MediaId> = {
        let lib = state.library.clone();
        blocking(move || {
            let p = lib.project(project_id)?;
            Ok(p.media_list_uncounted()?
                .into_iter()
                .filter(|m| m.status == MediaStatus::Missing)
                .map(|m| m.id)
                .collect())
        })
        .await?
    };
    let interrupted = stop_work_on(&state, &missing).await?;
    let (project, found) = blocking(move || {
        let p = lib.project(project_id)?;
        let found = p.relink_folder(&tools, Path::new(&dir));
        resume_interrupted(&p, &interrupted);
        Ok((p, found?))
    })
    .await?;
    for &id in &found.reindex {
        work::queue_ingest(&app, &project, id);
    }
    // Those found as they were: what waited for them can run.
    for &id in &found.same {
        found_as_it_was(&app, &project, id);
    }
    if !found.same.is_empty() || !found.reindex.is_empty() {
        work::library_changed(&app, &project);
        crate::events::emit_media_changed(&app, project_id, None);
        work::cast_changed(&app, &project);
    }
    Ok(found.summary)
}

/// Where the indexing of a media is now, as a picture: the keyframe at
/// about the place it has read to, small, as a `data:` URL. `None` while it
/// isn't reading its frames (or no picture came). It is decoded only when
/// asked: the player asks every few seconds while it waits for the indexing.
#[tauri::command]
#[specta::specta]
pub async fn media_indexing_still(
    state: State<'_, AppState>,
    project_id: ProjectId,
    media_id: MediaId,
) -> AppResult<Option<String>> {
    let lib = state.library.clone();
    let tools = state.tools()?;
    let jpeg = blocking(move || Ok(lib.project(project_id)?.indexing_still(&tools, media_id))).await?;
    Ok(jpeg.map(|j| format!("data:image/jpeg;base64,{}", STANDARD.encode(j))))
}

/// Build (or rebuild) the preview of a media that needs one.
#[tauri::command]
#[specta::specta]
pub async fn media_prepare_preview(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: ProjectId,
    media_id: MediaId,
) -> AppResult<()> {
    let lib = state.library.clone();
    let (project, m) = blocking(move || {
        let p = lib.project(project_id)?;
        let m = p.media(media_id)?;
        Ok((p, m))
    })
    .await?;
    if m.status != MediaStatus::Ready || m.proxy == ProxyState::NotNeeded {
        return Err(AppError::new(ErrorCode::Invalid, "This file doesn't need a separate preview."));
    }
    // One cancelled is only stopping: a new build replaces it.
    if !under_way(&state.jobs, JobKind::Proxy, media_id) {
        work::queue_proxy(&app, &project, media_id);
    }
    Ok(())
}

/// Choose the audio track for playback and export (`None` = the file's default).
#[tauri::command]
#[specta::specta]
pub async fn media_set_audio(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: ProjectId,
    media_id: MediaId,
    ordinal: Option<u32>,
) -> AppResult<MediaItem> {
    let lib = state.library.clone();
    let (project, needs_proxy, m) = blocking(move || {
        let p = lib.project(project_id)?;
        let needs = p.set_audio(media_id, ordinal)?;
        let m = p.media(media_id)?;
        Ok((p, needs, m))
    })
    .await?;
    if needs_proxy {
        work::queue_proxy(&app, &project, media_id);
    } else {
        // The new track plays straight from the file: a preview still being
        // built for the old one is pointless.
        state.jobs.cancel_where(|j| j.kind == JobKind::Proxy && j.media_id == Some(media_id));
    }
    crate::events::emit_media_changed(&app, project_id, Some(media_id));
    Ok(m)
}

/// Everything the player needs to show frame `f` exactly. Grants the webview
/// access to the file it should play.
#[tauri::command]
#[specta::specta]
pub async fn media_frame_map(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: ProjectId,
    media_id: MediaId,
) -> AppResult<FrameMap> {
    let lib = state.library.clone();
    let result = blocking(move || {
        let p = lib.project(project_id)?;
        let map = p.frame_map(media_id);
        let media = p.media(media_id);
        let pending = matches!(media.as_ref().map(|m| m.proxy), Ok(ProxyState::Pending));
        // The player asks only for media it thinks are ready. A damaged index
        // marks its media failed (here or in an export): tell the views, so
        // they offer "Index again".
        let damaged = map.is_err() && matches!(media.map(|m| m.status), Ok(MediaStatus::Failed));
        Ok((p, map, pending, damaged))
    })
    .await?;
    let (project, map, pending, damaged) = result;
    if damaged {
        work::library_changed(&app, &project);
        crate::events::emit_media_changed(&app, project_id, Some(media_id));
    }
    // Build a missing preview on demand — unless someone stopped the last
    // build: then the player offers "Prepare preview" instead of restarting
    // it on every refresh.
    let jobs = state.jobs.jobs();
    let proxy_jobs = || jobs.iter().filter(|j| j.kind == JobKind::Proxy && j.media_id == Some(media_id));
    let running = proxy_jobs().any(|j| !j.state.is_finished());
    let stopped = proxy_jobs().max_by_key(|j| (j.created_at, j.rev)).is_some_and(|j| j.state == JobState::Cancelled);
    if pending && !running && !stopped {
        work::queue_proxy(&app, &project, media_id);
    }
    let map = map?;
    // A preview only ever lives in its project's previews folder: one the
    // database names anywhere else isn't opened to the webview.
    if map.is_proxy && !paths::is_within(Path::new(&map.src), &project.proxies_dir()) {
        log::warn!("refused a preview outside its project's previews folder: {}", paths::name(Path::new(&map.src)));
        return Err(AppError::new(ErrorCode::NotReady, "This episode's preview can't be shown. Prepare it again."));
    }
    allow_asset(&app, Path::new(&map.src));
    Ok(map)
}

/// Shots, transitions and black/flash frames of a media (empty until its
/// shots are detected).
#[tauri::command]
#[specta::specta]
pub async fn media_segments(
    state: State<'_, AppState>,
    project_id: ProjectId,
    media_id: MediaId,
) -> AppResult<Vec<Segment>> {
    let lib = state.library.clone();
    blocking(move || Ok(lib.project(project_id)?.segments(media_id)?)).await
}

/// Is a job of `kind` for `media` queued or running that nobody cancelled?
/// One that was cancelled is only stopping, so a request right after the
/// cancel goes through.
fn under_way(jobs: &JobRunner, kind: JobKind, media: MediaId) -> bool {
    jobs.is_under_way(|j| j.kind == kind && j.media_id == Some(media))
}

/// Does an analysis under way do by itself what `redo` asks? It finds every
/// stage that isn't ready yet, from that stage's checkpoints, so finding one
/// of those again would only start it over at the back of the queue; and
/// going on (`None`) is what it does. Shots found can start over while it
/// finds the characters, and characters found while it finds the people.
fn under_way_does(shots: StageState, faces: StageState, redo: Option<AnalysisStage>) -> bool {
    match redo {
        None => true,
        Some(AnalysisStage::Shots) => shots != StageState::Ready,
        Some(AnalysisStage::Faces) => faces != StageState::Ready,
    }
}

/// Is an analysis of `media` under way (waiting or running, not cancelled)
/// that does what `redo` asks by itself? Then asking for it starts nothing:
/// a menu opened just before the analysis began, a double click, a stage
/// that ended just before the list caught up.
fn analysis_does_already(
    jobs: &JobRunner,
    media: MediaId,
    shots: StageState,
    faces: StageState,
    redo: Option<AnalysisStage>,
) -> bool {
    under_way_does(shots, faces, redo) && under_way(jobs, JobKind::Analyze, media)
}

/// Run the analysis of a media again. `redo` clears results from that stage
/// on first (a running analysis of the media is stopped and restarted once
/// it has stopped); `None` continues a stopped or failed run. Nothing starts
/// over when an analysis under way does it anyway.
#[tauri::command]
#[specta::specta]
pub async fn media_analyze(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: ProjectId,
    media_id: MediaId,
    redo: Option<AnalysisStage>,
) -> AppResult<()> {
    let lib = state.library.clone();
    let (project, m) = blocking(move || {
        let p = lib.project(project_id)?;
        let m = p.media(media_id)?;
        if m.status != MediaStatus::Ready {
            return Err(AppError::new(ErrorCode::NotReady, format!("{} isn't indexed yet.", m.file_name)));
        }
        Ok((p, m))
    })
    .await?;
    if analysis_does_already(&state.jobs, media_id, m.shots, m.faces, redo) {
        return Ok(());
    }
    let analysis = |j: &JobInfo| j.kind == JobKind::Analyze && j.media_id == Some(media_id);
    match redo {
        Some(stage) => {
            // A running job restarts the analysis once it has stopped; with
            // none running, a queued one is dropped and the restart made here.
            let handed_over = work::hand_over_restart(&mut state.analysis.lock(), media_id, stage);
            state.jobs.cancel_where(analysis);
            if !handed_over {
                let app2 = app.clone();
                blocking(move || {
                    work::restart_analysis(&app2, &project, media_id, stage);
                    Ok(())
                })
                .await?;
            }
        }
        None => {
            let p = project.clone();
            blocking(move || {
                let m = p.media(media_id)?;
                if m.shots != as_api::StageState::Ready {
                    p.set_shots_state(media_id, as_api::StageState::Pending, None)?;
                } else if m.faces != as_api::StageState::Ready {
                    p.set_faces_state(media_id, as_api::StageState::Pending, None)?;
                } else if p.finds_bodies() && m.bodies != as_api::StageState::Ready {
                    p.set_bodies_state(media_id, as_api::StageState::Pending, None)?;
                }
                Ok(())
            })
            .await?;
            work::queue_analysis(&app, &project, media_id);
        }
    }
    crate::events::emit_media_changed(&app, project_id, Some(media_id));
    let _ = crate::events::AnalysisChanged { project_id, media_id }.emit(&app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use as_api::{AnalysisStage, JobKind, StageState};
    use as_core::MediaId;
    use as_jobs::{Class, JobError, JobRunner, JobSpec};

    use super::{analysis_does_already, under_way, under_way_does};

    #[test]
    fn what_an_analysis_under_way_finds_itself_is_not_started_over() {
        use AnalysisStage::{Faces, Shots};
        use StageState::*;
        // Finding shots (or waiting to): it finds them; characters can't be asked for yet.
        assert!(under_way_does(Running, Pending, Some(Shots)));
        assert!(under_way_does(Pending, Pending, Some(Shots)), "waiting to find them");
        assert!(under_way_does(Failed, Pending, Some(Shots)), "a failed stage is tried again by it");
        assert!(under_way_does(Stopped, Pending, Some(Shots)), "a stopped stage goes on in it");
        // Finding characters: shots found can start over, characters not.
        assert!(!under_way_does(Ready, Running, Some(Shots)));
        assert!(under_way_does(Ready, Running, Some(Faces)));
        assert!(under_way_does(Ready, Pending, Some(Faces)), "waiting to find them");
        assert!(under_way_does(Ready, Stopped, Some(Faces)), "a stopped stage goes on in it");
        // Finding people: shots and characters found can both start over.
        assert!(!under_way_does(Ready, Ready, Some(Shots)));
        assert!(!under_way_does(Ready, Ready, Some(Faces)));
        // Going on is what any analysis under way does.
        assert!(under_way_does(Pending, Pending, None));
        assert!(under_way_does(Ready, Ready, None));
    }

    fn job(kind: JobKind, class: Class, media: MediaId) -> JobSpec {
        JobSpec {
            kind,
            class,
            title: "Work".into(),
            subtitle: None,
            project_id: None,
            media_id: Some(media),
            export_id: None,
        }
    }

    #[test]
    fn a_request_right_after_cancel_still_goes_through() {
        use AnalysisStage::{Faces, Shots};
        use StageState::*;
        let jobs = JobRunner::new(&[(Class::Analysis, 1), (Class::Media, 2)], |_| {});
        let (started_tx, started) = mpsc::channel();
        let mut stops = Vec::new();
        // Work that, once cancelled, takes its time to let go, as the engine
        // and ffmpeg do.
        let mut slow = |kind, class, media| {
            let (stop_tx, stop) = mpsc::channel::<()>();
            stops.push(stop_tx);
            let started_tx = started_tx.clone();
            jobs.submit(job(kind, class, media), move |ctx| {
                started_tx.send(()).unwrap();
                while !ctx.cancel.is_cancelled() {
                    std::thread::sleep(Duration::from_millis(5));
                }
                let _ = stop.recv_timeout(Duration::from_secs(10));
                Err(JobError::Cancelled)
            })
        };
        let (m, waiting, other) = (MediaId::new(), MediaId::new(), MediaId::new());
        let analysing = slow(JobKind::Analyze, Class::Analysis, m);
        let queued = slow(JobKind::Analyze, Class::Analysis, waiting);
        let indexing = slow(JobKind::Ingest, Class::Media, m);
        let previewing = slow(JobKind::Proxy, Class::Media, m);
        for _ in 0..3 {
            started.recv_timeout(Duration::from_secs(5)).unwrap();
        }

        // Running: what it finds itself starts nothing; shots found start over.
        assert!(analysis_does_already(&jobs, m, Running, Pending, Some(Shots)));
        assert!(analysis_does_already(&jobs, m, Ready, Running, Some(Faces)));
        assert!(analysis_does_already(&jobs, m, Running, Pending, None));
        assert!(!analysis_does_already(&jobs, m, Ready, Running, Some(Shots)), "a stage that is ready");
        assert!(!analysis_does_already(&jobs, other, Pending, Pending, Some(Shots)), "nothing analyses it");
        // Waiting: it will find them at its place in the queue.
        assert!(analysis_does_already(&jobs, waiting, Pending, Pending, Some(Shots)));
        assert!(under_way(&jobs, JobKind::Ingest, m) && under_way(&jobs, JobKind::Proxy, m));
        assert!(!under_way(&jobs, JobKind::Ingest, other));

        // Cancelled, and still letting go: it won't do it any more.
        jobs.cancel(analysing);
        jobs.cancel(queued);
        jobs.cancel(indexing);
        jobs.cancel(previewing);
        let stopping = |id| jobs.is_active(|j| j.id == id);
        assert!(stopping(analysing) && stopping(indexing) && stopping(previewing));
        assert!(!analysis_does_already(&jobs, m, Running, Pending, Some(Shots)));
        assert!(!analysis_does_already(&jobs, m, Running, Pending, None));
        assert!(!analysis_does_already(&jobs, waiting, Pending, Pending, Some(Shots)));
        assert!(!under_way(&jobs, JobKind::Ingest, m), "Retry goes through");
        assert!(!under_way(&jobs, JobKind::Proxy, m), "Prepare preview goes through");
        for stop in stops {
            let _ = stop.send(());
        }
    }
}

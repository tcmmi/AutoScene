//! Library: projects, import scanning, settings.

use std::path::Path;

use as_api::{AppSettings, ContentKind, ImportCandidate, JobInfo, NewProject, ProjectKind, ProjectSummary};
use as_core::{ProjectId, msg};
use tauri::{AppHandle, Manager, State};
use tauri_specta::Event;

use crate::commands::media::stop_jobs;
use crate::error::AppResult;
use crate::events::{HardwareChanged, LibraryChanged};
use crate::paths;
use crate::state::{AppState, allow_asset_dir, blocking};
use crate::work;

#[tauri::command]
#[specta::specta]
pub async fn projects_list(app: AppHandle, state: State<'_, AppState>) -> AppResult<Vec<ProjectSummary>> {
    let lib = state.library.clone();
    let projects = blocking(move || Ok(lib.projects()?)).await?;
    for p in &projects {
        allow_asset_dir(&app, &Path::new(&p.dir).join("thumbs"));
    }
    Ok(projects)
}

#[tauri::command]
#[specta::specta]
pub async fn project_get(state: State<'_, AppState>, project_id: ProjectId) -> AppResult<ProjectSummary> {
    let lib = state.library.clone();
    blocking(move || Ok(lib.summary(project_id)?)).await
}

/// Create a project from video files and start indexing them. `fast_scan`
/// is what the New Project dialog's switch shows; `None` (while the dialog
/// hasn't read the defaults yet) takes its footage's default here.
#[tauri::command]
#[specta::specta]
pub async fn project_create(
    app: AppHandle,
    state: State<'_, AppState>,
    req: NewProject,
    fast_scan: Option<bool>,
) -> AppResult<ProjectSummary> {
    paths::check_all(&req.files)?;
    let lib = state.library.clone();
    let (summary, project, added) = blocking(move || Ok(lib.create_project_with(&req, fast_scan)?)).await?;
    allow_asset_dir(&app, &project.thumbs_dir());
    for id in added {
        work::queue_ingest(&app, &project, id);
    }
    let _ = LibraryChanged { project_id: Some(summary.id) }.emit(&app);
    Ok(summary)
}

#[tauri::command]
#[specta::specta]
pub async fn project_rename(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: ProjectId,
    title: String,
) -> AppResult<ProjectSummary> {
    let lib = state.library.clone();
    let summary = blocking(move || Ok(lib.rename_project(project_id, &title)?)).await?;
    let _ = LibraryChanged { project_id: Some(project_id) }.emit(&app);
    Ok(summary)
}

/// Say what a project's footage is (live action, 3D animation, mixed; a
/// project never moves to or from anime). Its cast is grouped again for it,
/// and its faces found again when it moves to or from 3D animation (a
/// faces pass under way starts over at once).
#[tauri::command]
#[specta::specta]
pub async fn project_set_content(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: ProjectId,
    content: ContentKind,
) -> AppResult<ProjectSummary> {
    let lib = state.library.clone();
    let (summary, regroup) = blocking(move || {
        let (summary, regroup) = lib.set_project_content(project_id, content)?;
        Ok((summary, regroup.then(|| lib.project(project_id)).transpose()?))
    })
    .await?;
    if let Some(project) = regroup {
        work::queue_regroup(&app, &project);
        work::queue_outdated_faces(&app, &project);
        work::restart_outdated_passes(&app, &project);
    }
    let _ = LibraryChanged { project_id: Some(project_id) }.emit(&app);
    Ok(summary)
}

/// Say whether a project's faces are found with Fast Scan (the way the
/// Scener app finds them). Faces found the other way are found again, the
/// user's decisions on them kept, and the cast is grouped again. A faces
/// pass under way starts over at once.
#[tauri::command]
#[specta::specta]
pub async fn project_set_fast_scan(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: ProjectId,
    fast_scan: bool,
) -> AppResult<ProjectSummary> {
    let lib = state.library.clone();
    let (summary, changed) = blocking(move || {
        let (summary, changed) = lib.set_project_fast_scan(project_id, fast_scan)?;
        Ok((summary, changed.then(|| lib.project(project_id)).transpose()?))
    })
    .await?;
    if let Some(project) = changed {
        work::queue_regroup(&app, &project);
        work::queue_outdated_faces(&app, &project);
        work::restart_outdated_passes(&app, &project);
    }
    let _ = LibraryChanged { project_id: Some(project_id) }.emit(&app);
    Ok(summary)
}

/// Remove a project. `delete_files` also deletes its `.autoscene` folder
/// (never the source videos or exported clips).
#[tauri::command]
#[specta::specta]
pub async fn project_delete(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: ProjectId,
    delete_files: bool,
) -> AppResult<()> {
    // Its work stops first, also what it queues while stopping: nothing may
    // go on writing into its folder or hold its database open. When that
    // takes too long, the project stays as it was.
    let of = move |j: &JobInfo| j.project_id == Some(project_id);
    let still = msg!("project.stillStopping", "The work on this project is still stopping. Try again in a moment.");
    stop_jobs(&state, of, || false, still).await?;
    let lib = state.library.clone();
    let result = blocking(move || Ok(lib.delete_project(project_id, delete_files)?)).await;
    // Even when removing the folder failed half-way, the list may have changed.
    let _ = LibraryChanged { project_id: None }.emit(&app);
    result
}

/// Expand dropped/picked files and folders into videos with parsed episode
/// numbers, in episode order. With `project_id`, marks files already in it.
#[tauri::command]
#[specta::specta]
pub async fn import_scan(
    state: State<'_, AppState>,
    paths: Vec<String>,
    kind: ProjectKind,
    project_id: Option<ProjectId>,
) -> AppResult<Vec<ImportCandidate>> {
    crate::paths::check_all(&paths)?;
    let lib = state.library.clone();
    blocking(move || {
        let project = project_id.map(|id| lib.project(id)).transpose()?;
        Ok(lib.scan(&paths, kind, project.as_deref())?)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn settings_get(state: State<'_, AppState>) -> AppResult<AppSettings> {
    let lib = state.library.clone();
    blocking(move || Ok(lib.settings()?)).await
}

/// Save the settings. Whether plugins AutoScene hasn't reviewed may run
/// stays as it is: `plugins_allow_unreviewed` changes it. So does Fast Scan's
/// default for new projects: `scan_default_set` changes it. A graphics card
/// chosen, and NVIDIA acceleration switched, apply from the next analysis.
#[tauri::command]
#[specta::specta]
pub async fn settings_update(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: AppSettings,
) -> AppResult<AppSettings> {
    let state = state.inner().clone();
    let lib = state.library.clone();
    let before = lib.finds_bodies();
    let handle = app.clone();
    let (checked_before, saved, launch_changed) = blocking(move || {
        // Only a new library folder is checked: every other change sends
        // back the one in use.
        if settings.library_root.trim() != lib.library_root()?.display().to_string() {
            paths::check(settings.library_root.trim())?;
        }
        let checked = lib.settings()?.plugin_catalog_checks;
        let before = (lib.gpu(), lib.nvidia_acceleration());
        let saved = lib.update_settings(&settings)?;
        // From the next analysis on; an idle engine stops here, off the UI
        // thread.
        let launch_changed = (&saved.gpu, saved.nvidia_acceleration) != (&before.0, before.1);
        if launch_changed {
            state.relaunch();
        }
        let locale = crate::locale::ui_locale(saved.language);
        let was = std::mem::replace(&mut *state.language.lock(), locale).resolved;
        if cfg!(target_os = "macos")
            && locale.resolved != was
            && let Err(e) = crate::mac_menu::rebuild(&handle, crate::native_text::words(locale.resolved))
        {
            log::warn!("the menu couldn't change language: {e}");
        }
        // The only place that starts or ends the status on Discord: it may
        // wait for its thread, here off the UI thread.
        if let Some(discord) = handle.try_state::<crate::discord::Presence>() {
            discord.configure(&saved.discord_rpc);
        }
        Ok((checked, saved, launch_changed))
    })
    .await?;
    if launch_changed {
        let _ = HardwareChanged.emit(&app);
    }
    // Finding people by their bodies changes every cast.
    if saved.find_bodies != before {
        work::bodies_setting_changed(&app, saved.find_bodies);
    }
    // The daily catalog checks went on: one that is due runs now.
    if saved.plugin_catalog_checks != checked_before {
        let after = saved.plugin_catalog_checks;
        tauri::async_runtime::spawn_blocking(move || {
            crate::plugins::catalog::checks_changed(&app, checked_before, after);
        });
    }
    Ok(saved)
}

//! Background jobs shown in the Activity drawer.

use as_api::{JobInfo, JobKind};
use as_core::JobId;
use tauri::{AppHandle, State};

use crate::state::AppState;
use crate::work;

/// Newest first. Live updates arrive as `job:changed` / `job:progress`.
#[tauri::command]
#[specta::specta]
pub fn jobs_list(state: State<'_, AppState>) -> Vec<JobInfo> {
    state.jobs.jobs()
}

#[tauri::command]
#[specta::specta]
pub fn job_cancel(app: AppHandle, state: State<'_, AppState>, job_id: JobId) {
    // An analysis cancelled while it waited never ran a stage that would
    // record the stop: its media is stopped where it would have begun, or
    // it would wait as pending and the next launch would queue it again.
    if let Some(JobInfo { kind: JobKind::Analyze, project_id: Some(project), media_id: Some(media), .. }) =
        state.jobs.cancel(job_id)
    {
        tauri::async_runtime::spawn_blocking(move || work::stop_unstarted_analysis(&app, project, media));
    }
}

/// Forget finished jobs.
#[tauri::command]
#[specta::specta]
pub fn jobs_clear_finished(state: State<'_, AppState>) {
    state.jobs.clear_finished();
}

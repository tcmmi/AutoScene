//! The cast: characters found across a project's media, their face tracks,
//! and the user's decisions about them.

use as_api::{
    Cast, CastEdit, FaceTrackItem, Presence, RescanAnswer, RescanRequest, RescanResults, ShotGroup, TrackFilter,
};
use as_core::{CancelToken, CharacterId, MediaId, ProjectId};
use tauri::{AppHandle, State};

use crate::error::AppResult;
use crate::state::{AppState, allow_asset_tree, blocking};
use crate::work;

#[tauri::command]
#[specta::specta]
pub async fn cast_get(app: AppHandle, state: State<'_, AppState>, project_id: ProjectId) -> AppResult<Cast> {
    let lib = state.library.clone();
    let (cast, faces) = blocking(move || {
        let p = lib.project(project_id)?;
        Ok((p.cast()?, p.faces_dir()))
    })
    .await?;
    allow_asset_tree(&app, &faces);
    Ok(cast)
}

#[tauri::command]
#[specta::specta]
pub async fn cast_tracks(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: ProjectId,
    filter: TrackFilter,
) -> AppResult<Vec<FaceTrackItem>> {
    let lib = state.library.clone();
    let (tracks, faces) = blocking(move || {
        let p = lib.project(project_id)?;
        Ok((p.cast_tracks(filter)?, p.faces_dir()))
    })
    .await?;
    allow_asset_tree(&app, &faces);
    Ok(tracks)
}

/// Read faces again and say who they are. Changes nothing: what it finds
/// waits in [`rescan_results`] for the person's answer.
#[tauri::command]
#[specta::specta]
pub async fn cast_rescan(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: ProjectId,
    request: RescanRequest,
) -> AppResult<()> {
    let lib = state.library.clone();
    blocking(move || {
        let p = lib.project(project_id)?;
        work::queue_rescan(&app, &p, request);
        Ok(())
    })
    .await
}

/// What Rescan is waiting to hear about.
#[tauri::command]
#[specta::specta]
pub async fn rescan_results(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: ProjectId,
) -> AppResult<RescanResults> {
    let lib = state.library.clone();
    let (results, faces) = blocking(move || {
        let p = lib.project(project_id)?;
        Ok((p.rescan_results()?, p.faces_dir()))
    })
    .await?;
    allow_asset_tree(&app, &faces);
    Ok(results)
}

/// The person's answer to a suggestion: a decision like any other.
#[tauri::command]
#[specta::specta]
pub async fn rescan_answer(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: ProjectId,
    answer: RescanAnswer,
) -> AppResult<()> {
    let lib = state.library.clone();
    blocking(move || {
        let p = lib.project(project_id)?;
        let edits = p.rescan_answer(&answer)?;
        for edit in &edits {
            work::cast_edited(&app, &p, edit);
        }
        // "Two people" pins faces of both without an edit of its own.
        if edits.is_empty() {
            work::cast_changed(&app, &p);
        }
        work::rescan_changed(&app, &p);
        Ok(())
    })
    .await
}

/// Apply a decision about the cast. Returns the character it created, if any.
#[tauri::command]
#[specta::specta]
pub async fn cast_edit(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: ProjectId,
    edit: CastEdit,
) -> AppResult<Option<CharacterId>> {
    let lib = state.library.clone();
    blocking(move || {
        let p = lib.project(project_id)?;
        let created = p.cast_edit(&edit)?;
        work::cast_edited(&app, &p, &edit);
        Ok(created)
    })
    .await
}

/// A face's portrait (3:4, sharp where a face is shown big), made now unless
/// it exists. `None` when it can't be made now (the media is missing or being
/// indexed): the thumbnail stands in.
#[tauri::command]
#[specta::specta]
pub async fn cast_portrait(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: ProjectId,
    track_id: u32,
) -> AppResult<Option<String>> {
    let lib = state.library.clone();
    let tools = state.tools()?;
    let (portrait, faces) = blocking(move || {
        let p = lib.project(project_id)?;
        let portrait = p.portrait(&tools, i64::from(track_id), &CancelToken::new())?;
        Ok((portrait, p.faces_dir()))
    })
    .await?;
    allow_asset_tree(&app, &faces);
    Ok(portrait.path().map(|p| p.display().to_string()))
}

/// Where characters are on screen in one media (for the timeline).
#[tauri::command]
#[specta::specta]
pub async fn media_presence(
    state: State<'_, AppState>,
    project_id: ProjectId,
    media_id: MediaId,
) -> AppResult<Vec<Presence>> {
    let lib = state.library.clone();
    blocking(move || Ok(lib.project(project_id)?.presence(media_id)?)).await
}

/// Every shot a character is seen in, with its sprite and the character's
/// clearest face there: in episode order, in groups of shots close together
/// in time (a shot far from the others is a group of one).
#[tauri::command]
#[specta::specta]
pub async fn character_shots(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: ProjectId,
    character_id: CharacterId,
) -> AppResult<Vec<ShotGroup>> {
    let lib = state.library.clone();
    let (shots, faces) = blocking(move || {
        let p = lib.project(project_id)?;
        Ok((p.character_shot_groups(character_id)?, p.faces_dir()))
    })
    .await?;
    allow_asset_tree(&app, &faces);
    Ok(shots)
}

//! Scenepacks: a character's shots, filtered and picked by hand, exported in
//! one go. The pack is made from the analysis each time (as_project::pack),
//! so the grid, the plan and the export can't disagree: with the verdicts
//! of the plugins' shot filters as they are then (plugins::filters). A
//! plugin's pack action saves the pack as the grid showed it as one file,
//! before anything is rendered (plugins::pack_actions).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use as_api::{
    ExportPlan, ExportSummary, PackChoices, PackExportRequest, PackNaming, PackPicks, PackPreview, PackSpec, PackState,
};
use as_core::{CharacterId, ProjectId, msg};
use as_export::builtin_presets;
use as_media::Caps;
use as_plugin::calls::FileFilter;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_specta::Event;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::events::{ExportChanged, PackChanged};
use crate::paths;
use crate::plugins::filters::{self, Checking};
use crate::plugins::pack_actions::{self, AskFn, PackActionStart, Saving, Shown};
use crate::state::{AppState, allow_asset_tree, blocking};
use crate::work;

/// A character's pack settings as last used (the defaults the first time).
#[tauri::command]
#[specta::specta]
pub async fn pack_get(
    state: State<'_, AppState>,
    project_id: ProjectId,
    character_id: CharacterId,
) -> AppResult<PackState> {
    let lib = state.library.clone();
    blocking(move || Ok(lib.project(project_id)?.pack_state(character_id)?)).await
}

/// Keep a character's pack settings for next time.
#[tauri::command]
#[specta::specta]
pub async fn pack_save(
    state: State<'_, AppState>,
    project_id: ProjectId,
    character_id: CharacterId,
    pack: PackState,
) -> AppResult<()> {
    let lib = state.library.clone();
    blocking(move || Ok(lib.project(project_id)?.save_pack_state(character_id, &pack)?)).await
}

/// Every shot of the character, the ones the pack takes marked, in its order.
#[tauri::command]
#[specta::specta]
pub async fn pack_preview(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: ProjectId,
    character_id: CharacterId,
    spec: PackSpec,
) -> AppResult<PackPreview> {
    let (lib, source) = (state.library.clone(), filters::Source::of(&state));
    let (preview, faces) = blocking(move || {
        let filters = source.filters(project_id, character_id, &spec)?;
        let p = lib.project(project_id)?;
        Ok((p.pack_preview(character_id, &spec, &filters)?, p.faces_dir()))
    })
    .await?;
    // The grid shows the shots' sprites and faces.
    allow_asset_tree(&app, &faces);
    Ok(preview)
}

/// Pick, drop or hand back to the filters shots of a pack (a click's
/// `picks`, in order), and say what that made of the pack `spec` makes: its
/// choices alone, as picks change nothing else. No `pack:changed`: the grid
/// that picked takes the answer.
#[tauri::command]
#[specta::specta]
pub async fn pack_override(
    state: State<'_, AppState>,
    project_id: ProjectId,
    character_id: CharacterId,
    picks: Vec<PackPicks>,
    spec: PackSpec,
) -> AppResult<PackChoices> {
    let (lib, source) = (state.library.clone(), filters::Source::of(&state));
    blocking(move || {
        let filters = source.filters(project_id, character_id, &spec)?;
        let p = lib.project(project_id)?;
        for pick in &picks {
            p.set_pack_overrides(character_id, &pick.shots, pick.include)?;
        }
        Ok(p.pack_choices(character_id, &spec, &filters)?)
    })
    .await
}

/// The names the pack's files would get and its size with each preset.
#[tauri::command]
#[specta::specta]
pub async fn pack_plan(
    state: State<'_, AppState>,
    project_id: ProjectId,
    character_id: CharacterId,
    spec: PackSpec,
    naming: PackNaming,
) -> AppResult<ExportPlan> {
    let (lib, tools, source) = (state.library.clone(), state.tools().ok(), filters::Source::of(&state));
    blocking(move || {
        let filters = source.filters(project_id, character_id, &spec)?;
        // Sized for the encoders an export would use (found once, in about a second).
        let none = Caps::default();
        let caps = tools.as_deref().map_or(&none, |t| t.caps());
        let p = lib.project(project_id)?;
        Ok(p.pack_plan(character_id, &spec, &filters, &naming, &builtin_presets(), caps)?)
    })
    .await
}

/// Ask a plugin's shot filter `filter` (`<pluginId>/<filterId>`) for its
/// verdicts on the shots of the pack `spec` makes that it hasn't judged:
/// `true` while its check waits or runs. One that failed with the settings
/// it has now starts only `again` (the person's Try again). Verdicts arrive
/// an episode at a time, with a `pack:changed` once each run is over; the
/// check shows in Activity.
#[tauri::command]
#[specta::specta]
pub async fn pack_filter_check(
    app: AppHandle,
    project_id: ProjectId,
    character_id: CharacterId,
    filter: String,
    spec: PackSpec,
    again: bool,
) -> AppResult<bool> {
    let ck = Checking::of(&app)?;
    blocking(move || filters::queue_check(&ck, project_id, character_id, &filter, &spec, again)).await
}

/// Save a character's pack, exactly as the preview with `stamp` showed it,
/// with a plugin's pack action `action` (`<pluginId>/<actionId>`): the
/// system's save dialog asks where first, then its run is queued, and the
/// file is there once it ends well. Refused, before anything is asked, as an
/// export of the pack would be: when it changed since it was shown, while a
/// filter it turns on checks shots, or when it takes nothing.
#[tauri::command]
#[specta::specta]
pub async fn pack_action_run(
    app: AppHandle,
    project_id: ProjectId,
    character_id: CharacterId,
    action: String,
    spec: PackSpec,
    naming: PackNaming,
    stamp: String,
) -> AppResult<PackActionStart> {
    let sv = Saving::of(&app, save_dialog(&app))?;
    blocking(move || {
        let shown = Shown { spec: &spec, naming: &naming, stamp: &stamp };
        pack_actions::queue_pack_action(&sv, project_id, character_id, &action, &shown)
    })
    .await
}

/// The system's save dialog over the window, as a page's `files.save` has
/// it: titled, offering a name and the kinds of file, in the folder given
/// (where the project's exports go), or where the system opens it.
fn save_dialog(app: &AppHandle) -> Arc<AskFn> {
    let app = app.clone();
    Arc::new(move |title: &str, name: &str, kinds: &[FileFilter], folder: Option<&Path>| -> Option<PathBuf> {
        let mut dialog = app.dialog().file().set_title(title).set_file_name(name);
        for k in kinds {
            let extensions: Vec<&str> = k.extensions.iter().map(String::as_str).collect();
            dialog = dialog.add_filter(&k.name, &extensions);
        }
        if let Some(folder) = folder {
            dialog = dialog.set_directory(folder);
        }
        if let Some(window) = app.get_webview_window("main") {
            dialog = dialog.set_parent(&window);
        }
        dialog.blocking_save_file()?.into_path().ok()
    })
}

/// Export a character's pack: exactly what the preview with `req.stamp`
/// showed (refused if the pack changed since). Progress arrives as `job:*`
/// events, results as `export:changed`.
#[tauri::command]
#[specta::specta]
pub async fn pack_export(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: ProjectId,
    character_id: CharacterId,
    req: PackExportRequest,
) -> AppResult<ExportSummary> {
    let preset = builtin_presets()
        .into_iter()
        .find(|p| p.id == req.preset_id)
        .ok_or_else(|| AppError::new(ErrorCode::Invalid, msg!("export.unknownPreset", "Unknown export preset.")))?;
    // The folder picked for it (the character's own goes inside, by name).
    if let Some(dir) = req.out_dir.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
        paths::check(dir)?;
    }
    let (lib, source) = (state.library.clone(), filters::Source::of(&state));
    let (project, summary) = blocking(move || {
        // Not while a filter it turns on checks shots: the pack is still changing.
        let filters = source.filters(project_id, character_id, &req.spec)?;
        if let Some(why) = filters::export_waits(&filters, &req.spec) {
            return Err(AppError::new(ErrorCode::Invalid, why));
        }
        let project = lib.project(project_id)?;
        let default_dir = lib.default_export_dir(&lib.summary(project_id)?)?;
        let (request, out_dir) = project.pack_export_request(character_id, &req, &filters, &default_dir)?;
        let summary = project.create_export(&request, preset, &out_dir)?;
        Ok((project, summary))
    })
    .await?;
    work::queue_export(&app, &project, &summary);
    let _ = ExportChanged { project_id, export_id: summary.id }.emit(&app);
    // The settings it was made with are kept.
    let _ = PackChanged { project_id, character_id }.emit(&app);
    Ok(summary)
}

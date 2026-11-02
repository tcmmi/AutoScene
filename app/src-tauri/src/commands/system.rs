use std::collections::HashMap;
use std::path::{Path, PathBuf};

use as_api::{AppInfo, MediaToolsStatus, ModelInfo, Platform};
use as_core::AppPaths;
use as_project::Library;
use tauri::{AppHandle, State};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::paths;
use crate::state::{AppState, blocking};

#[tauri::command]
#[specta::specta]
pub fn app_info(app: AppHandle, state: State<'_, AppState>) -> AppInfo {
    let paths = AppPaths::resolve();
    AppInfo {
        version: app.package_info().version.to_string(),
        platform: Platform::current(),
        arch: std::env::consts::ARCH.to_string(),
        data_dir: paths.data.display().to_string(),
        logs_dir: paths.logs().display().to_string(),
        library_root: state
            .library
            .library_root()
            .unwrap_or_else(|_| AppPaths::default_library_root())
            .display()
            .to_string(),
        extensions_dir: paths.extensions().display().to_string(),
    }
}

/// The media engine (ffmpeg) found on this machine — Settings → About.
#[tauri::command]
#[specta::specta]
pub async fn media_tools(state: State<'_, AppState>) -> AppResult<MediaToolsStatus> {
    let state = state.inner().clone();
    blocking(move || Ok(state.tools_status())).await
}

/// The AI models AutoScene downloads when an analysis first needs them:
/// what each is for, its licence, and whether it is here — Settings → About.
#[tauri::command]
#[specta::specta]
pub async fn models(state: State<'_, AppState>) -> AppResult<Vec<ModelInfo>> {
    let state = state.inner().clone();
    blocking(move || {
        Ok(as_models::manifest()
            .iter()
            .map(|m| ModelInfo {
                id: m.id.clone(),
                purpose: m.purpose.clone(),
                source: m.source.clone(),
                license: m.license.clone(),
                non_commercial: m.non_commercial,
                size_bytes: m.size as f64,
                // Listing doesn't hash: a damaged file is found when it is next used.
                installed: state.models.downloaded(&m.id).is_ok_and(|p| p.is_some()),
            })
            .collect())
    })
    .await
}

/// Show a file or folder in Explorer / Finder: one AutoScene knows (its
/// library, data and logs, a project's folder, an episode's file, an
/// export's folder or files), never just any path. Off the main thread: a
/// share that doesn't answer would freeze the window until Windows gives up.
#[tauri::command]
#[specta::specta]
pub async fn reveal_path(state: State<'_, AppState>, path: String) -> AppResult<()> {
    paths::check(&path)?;
    let lib = state.library.clone();
    blocking(move || {
        let p = PathBuf::from(&path);
        if !known(&lib, &AppPaths::resolve(), &p) {
            log::warn!("not revealing {}, which isn't AutoScene's", paths::name(&p));
            return Err(AppError::new(ErrorCode::Invalid, "AutoScene only shows its own files and folders."));
        }
        if !p.exists() {
            return Err(AppError::new(ErrorCode::NotFound, "That file or folder isn't there any more."));
        }
        tauri_plugin_opener::reveal_item_in_dir(&p).map_err(|e| {
            log::warn!("revealing {}: {e}", paths::name(&p));
            AppError::new(ErrorCode::Io, "That file or folder couldn't be shown.")
        })
    })
    .await
}

/// Whether `path` is one AutoScene keeps things in or was given: the
/// library folder, the app's data and logs, a project's folder (thumbnails,
/// previews), an export's folder and files, an episode's file. Episodes and
/// exports are those of the open projects, which after launch are all of
/// them (each is opened to resume its work).
fn known(lib: &Library, app: &AppPaths, path: &Path) -> bool {
    let folders = [lib.library_root().ok(), Some(app.data.clone()), Some(app.logs())];
    let projects = lib.projects().unwrap_or_default().into_iter().map(|p| Some(PathBuf::from(p.dir)));
    if folders.into_iter().chain(projects).flatten().any(|dir| paths::is_within(path, &dir)) {
        return true;
    }
    lib.open_projects().iter().any(|project| {
        project.exports().unwrap_or_default().iter().any(|e| paths::is_within(path, Path::new(&e.out_dir)))
            || project.media_list_uncounted().unwrap_or_default().iter().any(|m| Path::new(&m.path) == path)
    })
}

/// Put text on the clipboard. Natively, so it also works from the macOS
/// menu, where the webview's clipboard API refuses (no user gesture).
#[tauri::command]
#[specta::specta]
pub fn clipboard_write(app: AppHandle, text: String) -> AppResult<()> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    app.clipboard().write_text(text).map_err(|e| {
        log::warn!("clipboard: {e}");
        AppError::new(ErrorCode::Internal, "Couldn't copy to the clipboard.")
    })
}

/// The text on the clipboard, for the text fields' Paste (the webview's own
/// clipboard API would ask the user for permission first). `None` when the
/// clipboard holds no text (a picture, files, nothing). Off the main thread:
/// reading waits for the clipboard's owner, which on Linux can be this app.
#[tauri::command]
#[specta::specta]
pub async fn clipboard_read(app: AppHandle) -> AppResult<Option<String>> {
    blocking(move || {
        use tauri_plugin_clipboard_manager::ClipboardExt;
        Ok(app.clipboard().read_text().inspect_err(|e| log::debug!("clipboard has no text: {e}")).ok())
    })
    .await
}

/// Open the plugins folder (Settings → Plugins).
#[tauri::command]
#[specta::specta]
pub fn open_plugins_dir() -> AppResult<()> {
    let dir = AppPaths::resolve().plugins();
    std::fs::create_dir_all(&dir)?;
    tauri_plugin_opener::open_path(dir.display().to_string(), None::<&str>).map_err(|e| {
        log::warn!("opening the plugins folder: {e}");
        AppError::new(ErrorCode::Io, "The plugins folder couldn't be opened.")
    })
}

/// Open the extensions folder (backward compatibility alias).
#[tauri::command]
#[specta::specta]
pub fn open_extensions_dir() -> AppResult<()> {
    open_plugins_dir()
}

/// List plugins installed in the plugins folder.
#[tauri::command]
#[specta::specta]
pub fn plugins_list() -> Vec<as_export::InstalledPlugin> {
    let dir = AppPaths::resolve().plugins();
    let _ = std::fs::create_dir_all(&dir);
    as_export::discover_plugins(&dir)
}

/// List extensions installed (backward compatibility alias).
#[tauri::command]
#[specta::specta]
pub fn extensions_list() -> Vec<as_export::InstalledPlugin> {
    plugins_list()
}

/// Inspect an `.asp` plugin package before installing, returning metadata and cryptographic signature status.
#[tauri::command]
#[specta::specta]
pub fn plugin_inspect_asp(path: String) -> AppResult<as_export::AspInspectionResult> {
    let p = std::path::Path::new(&path);
    as_export::inspect_asp(p).map_err(|e| AppError::new(ErrorCode::Invalid, format!("Plugin inspection failed: {e}")))
}

/// Install a `.asp` (AutoScene Plugin) archive.
#[tauri::command]
#[specta::specta]
pub fn plugin_install_asp(path: String, allow_unsigned: Option<bool>) -> AppResult<as_export::InstalledPlugin> {
    let p = std::path::Path::new(&path);
    let plugins_dir = AppPaths::resolve().plugins();
    let _ = std::fs::create_dir_all(&plugins_dir);
    as_export::install_asp(p, &plugins_dir, allow_unsigned.unwrap_or(false))
        .map_err(|e| AppError::new(ErrorCode::Invalid, format!("Plugin installation failed: {e}")))
}

/// Package a plugin directory into an `.asp` file, optionally signing it with an Ed25519 private key.
#[tauri::command]
#[specta::specta]
pub fn plugin_package_asp(
    dir_path: String,
    out_file: String,
    signing_key_hex: Option<String>,
    author_identity: Option<String>,
) -> AppResult<Option<as_export::PluginSigningKeyPair>> {
    let p = std::path::Path::new(&dir_path);
    let out = std::path::Path::new(&out_file);
    as_export::package_asp_signed(p, out, signing_key_hex.as_deref(), author_identity.as_deref())
        .map_err(|e| AppError::new(ErrorCode::Invalid, format!("Plugin packaging failed: {e}")))
}

/// Generate a new Ed25519 keypair for signing `.asp` plugins.
#[tauri::command]
#[specta::specta]
pub fn plugin_generate_keypair() -> as_export::PluginSigningKeyPair {
    as_export::generate_plugin_signing_keypair()
}

/// Uninstall an installed plugin by its manifest id.
#[tauri::command]
#[specta::specta]
pub fn plugin_uninstall(id: String) -> AppResult<()> {
    let dir = AppPaths::resolve().plugins();
    let installed = as_export::discover_plugins(&dir);
    let ext = installed
        .into_iter()
        .find(|e| e.manifest.id == id)
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, "Plugin not found"))?;

    let plugin_path = std::path::Path::new(&ext.path);
    if plugin_path.is_dir() {
        std::fs::remove_dir_all(plugin_path)
            .map_err(|e| AppError::new(ErrorCode::Io, format!("Failed to delete plugin: {e}")))?;
    }
    Ok(())
}

/// Run a plugin directly to test its execution.
#[tauri::command]
#[specta::specta]
pub fn plugin_test(id: String) -> AppResult<String> {
    let dir = AppPaths::resolve().plugins();
    let installed = as_export::discover_plugins(&dir);
    let ext = installed
        .into_iter()
        .find(|e| e.manifest.id == id)
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, "Plugin not found"))?;

    let dummy_payload = as_export::PostExportPayload {
        export_id: "test-run".into(),
        project_id: "manual-test".into(),
        project_title: Some("Manual Test Run".into()),
        out_dir: ext.path.clone(),
        clip_count: 1,
        output_files: vec![],
        completed_at: as_api::now_ms() as u64,
    };

    let cancel = as_core::CancelToken::new();
    let scratch = AppPaths::resolve().cache().join("test_plugin");
    as_export::run_hook(
        &ext.entrypoint_path,
        &dummy_payload,
        &scratch,
        &cancel,
        Some(std::time::Duration::from_secs(10)),
    )
    .map_err(|e| AppError::new(ErrorCode::Invalid, format!("Plugin failed: {e}")))?;

    Ok("Plugin test ran successfully!".into())
}

/// Run an extension directly to test its execution (backward compatibility).
#[tauri::command]
#[specta::specta]
pub fn extension_test(id: String) -> AppResult<String> {
    plugin_test(id)
}

/// Run a plugin on specific clip output files.
#[tauri::command]
#[specta::specta]
pub fn plugin_run_on_files(id: String, project_id: String, files: Vec<String>) -> AppResult<String> {
    if files.is_empty() {
        return Err(AppError::new(ErrorCode::Invalid, "No files specified"));
    }

    let dir = AppPaths::resolve().plugins();
    let installed = as_export::discover_plugins(&dir);
    let ext = installed
        .into_iter()
        .find(|e| e.manifest.id == id)
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, "Plugin not found"))?;

    let out_dir = std::path::Path::new(&files[0])
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ext.path.clone());

    let payload = as_export::PostExportPayload {
        export_id: format!("clip-action-{}", as_api::now_ms()),
        project_id,
        project_title: None,
        out_dir,
        clip_count: files.len(),
        output_files: files,
        completed_at: as_api::now_ms() as u64,
    };

    let cancel = as_core::CancelToken::new();
    let scratch = AppPaths::resolve().cache().join("plugin_action");
    as_export::run_hook(&ext.entrypoint_path, &payload, &scratch, &cancel, Some(std::time::Duration::from_secs(300)))
        .map_err(|e| AppError::new(ErrorCode::Invalid, format!("Plugin failed: {e}")))?;

    Ok("Completed successfully".into())
}

/// Run an extension on specific clip output files (backward compatibility).
#[tauri::command]
#[specta::specta]
pub fn extension_run_on_files(id: String, project_id: String, files: Vec<String>) -> AppResult<String> {
    plugin_run_on_files(id, project_id, files)
}

/// Open the logs folder (Settings → About, Help menu on Windows).
#[tauri::command]
#[specta::specta]
pub fn open_logs_dir() -> AppResult<()> {
    let dir = AppPaths::resolve().logs();
    std::fs::create_dir_all(&dir)?;
    tauri_plugin_opener::open_path(dir.display().to_string(), None::<&str>).map_err(|e| {
        log::warn!("opening the logs folder: {e}");
        AppError::new(ErrorCode::Io, "The logs folder couldn't be opened.")
    })
}

/// Put the person's keys (Settings → Shortcuts) on the macOS menu, which
/// answers them there before the page sees them: `keys` maps a menu item's id
/// to its accelerator, `""` for none. Elsewhere there is no menu.
#[tauri::command]
#[specta::specta]
pub fn menu_set_keys(app: AppHandle, keys: HashMap<String, String>) -> AppResult<()> {
    #[cfg(target_os = "macos")]
    crate::mac_menu::set_keys(&app, &keys).map_err(|e| {
        log::warn!("setting the menu's keys: {e}");
        AppError::internal("The menu's keys couldn't be changed.")
    })?;
    #[cfg(not(target_os = "macos"))]
    let _ = (app, keys);
    Ok(())
}

#[cfg(test)]
mod tests {
    use as_api::{AppSettings, ContentKind, NewProject, ProjectKind};

    use super::*;

    #[test]
    fn only_paths_autoscene_knows_are_revealed() {
        let tmp = std::env::temp_dir().join(format!("autoscene-reveal-{}", std::process::id()));
        let (data, root, media) = (tmp.join("data"), tmp.join("Library"), tmp.join("Shows"));
        std::fs::create_dir_all(&media).unwrap();
        let episode = media.join("Show S01E01.mkv");
        std::fs::write(&episode, b"not really a video").unwrap();
        let app = AppPaths::at(&data);
        let lib = Library::open(&app).unwrap();
        let settings = AppSettings { library_root: root.display().to_string(), ..lib.settings().unwrap() };
        lib.update_settings(&settings).unwrap();
        let (summary, ..) = lib
            .create_project(&NewProject {
                title: "Show".into(),
                kind: ProjectKind::Series,
                content: ContentKind::Live,
                files: vec![episode.display().to_string()],
            })
            .unwrap();

        let project = PathBuf::from(&summary.dir);
        for ok in [&root, &root.join("Exports").join("Show"), &project.join("thumbs"), &episode, &app.logs(), &data] {
            assert!(known(&lib, &app, ok), "{}", ok.display());
        }
        for other in [
            media.clone(),
            media.join("Show S01E02.mkv"),
            tmp.join("Library2"),
            root.join("..").join("Shows").join("Show S01E02.mkv"),
            PathBuf::from("Library"),
        ] {
            assert!(!known(&lib, &app, &other), "{}", other.display());
        }
        drop(lib);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

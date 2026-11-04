//! AutoScene's plugin catalog (Settings → Plugins): what it offers and where
//! AutoScene stands with it, and "Check now". Each check emits
//! `catalog:changed`, and `plugins:changed` when it withdrew a plugin or
//! lifted a withdrawal (see `plugins::catalog`).

use tauri::{AppHandle, State};

use crate::error::AppResult;
use crate::plugins::catalog::{self, CatalogView};
use crate::plugins::host;
use crate::state::{AppState, blocking};

/// The catalog as AutoScene holds it. Never the network.
#[tauri::command]
#[specta::specta]
pub async fn catalog_list(state: State<'_, AppState>) -> AppResult<CatalogView> {
    let (host, lib) = (host(&state)?, state.library.clone());
    blocking(move || Ok(catalog::view(&host, as_plugin::catalog::now(), lib.settings()?.plugin_catalog_checks))).await
}

/// Checks the catalog now, also while the daily checks are off. A check
/// that fails is no error: the view says why.
#[tauri::command]
#[specta::specta]
pub async fn catalog_refresh(app: AppHandle) -> AppResult<CatalogView> {
    blocking(move || catalog::refresh_and_apply(&app)).await
}

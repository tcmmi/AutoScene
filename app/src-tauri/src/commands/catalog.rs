//! AutoScene's plugin catalog (Settings → Plugins): what it offers and where
//! AutoScene stands with it, "Check now", and getting a plugin it offers to
//! install or update it. Each check emits `catalog:changed`, and
//! `plugins:changed` when it withdrew a plugin or lifted a withdrawal (see
//! `plugins::catalog`). A get emits `catalog:progress` while the package
//! comes, and gives a review, which `plugin_install` or `plugin_discard`
//! answers as for a package picked from a file.

use as_plugin::{PluginError, PluginId};
use tauri::{AppHandle, State};

use crate::error::AppResult;
use crate::plugins::catalog::{self, CatalogView};
use crate::plugins::{PluginReview, host, review};
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

/// Gets the version of `plugin_id` the catalog offers here (newer than the
/// one installed) and checks it, for the review dialog. One at a time per
/// plugin; `catalog_cancel` ends it until its review is ready.
#[tauri::command]
#[specta::specta]
pub async fn catalog_get(app: AppHandle, state: State<'_, AppState>, plugin_id: String) -> AppResult<PluginReview> {
    let id = PluginId::parse(&plugin_id)?;
    let (host, lib) = (host(&state)?, state.library.clone());
    blocking(move || {
        // Under way before anything else: a cancel from here on ends it.
        let download = host.catalog.begin(&host.store, &id)?;
        let allow = lib.allows_unreviewed_plugins()?;
        let ticket = catalog::get(&app, &host, &download)?;
        let review = review(&ticket, &host.store, allow);
        // Cancelled while the review was made: thrown away.
        if download.end() {
            host.store.discard(ticket);
            return Err(PluginError::Cancelled.into());
        }
        host.keep(ticket);
        Ok(review)
    })
    .await
}

/// Ends the get of `plugin_id` under way (it fails as cancelled). None
/// under way is no error.
#[tauri::command]
#[specta::specta]
pub async fn catalog_cancel(state: State<'_, AppState>, plugin_id: String) -> AppResult<()> {
    let id = PluginId::parse(&plugin_id)?;
    let host = host(&state)?;
    blocking(move || {
        host.catalog.cancel(&id);
        Ok(())
    })
    .await
}

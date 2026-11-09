//! Links for plugins' sources: read for the page's feedback, and queued for
//! a project ([`crate::plugins::sources`]).

use as_core::ProjectId;
use tauri::AppHandle;

use crate::error::AppResult;
use crate::plugins::sources::{self, LinkItem, LinksQueued, Sourcing};
use crate::state::blocking;

/// Reads pasted `text` as links: each cleaned, and the plugin that takes
/// it, or why none does.
#[tauri::command]
#[specta::specta]
pub async fn source_links_read(app: AppHandle, text: String) -> AppResult<Vec<LinkItem>> {
    blocking(move || sources::read_links(&Sourcing::of(&app)?, &text)).await
}

/// Queues a download for each of `links` into the project `project_id`:
/// each video joins it as soon as it is fetched.
#[tauri::command]
#[specta::specta]
pub async fn source_links_queue(app: AppHandle, project_id: ProjectId, links: Vec<String>) -> AppResult<LinksQueued> {
    blocking(move || sources::queue_links(&Sourcing::of(&app)?, project_id, &links)).await
}

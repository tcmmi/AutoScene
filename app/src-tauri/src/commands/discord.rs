use as_api::DiscordRpcSettings;
use tauri::State;

use crate::discord::{self, DiscordLines, DiscordView, Presence};

/// What is on screen now, for the person's status on Discord. Does nothing
/// while that is off, and the names in it go no further than their switches
/// let them.
#[tauri::command]
#[specta::specta]
pub fn discord_view(view: DiscordView, presence: State<'_, Presence>) {
    presence.set_view(view);
}

/// What the status looks like with the switches `show`, for Settings: in an
/// episode, in a scenepack.
#[tauri::command]
#[specta::specta]
pub fn discord_preview(show: DiscordRpcSettings) -> Vec<DiscordLines> {
    discord::preview(&show)
}

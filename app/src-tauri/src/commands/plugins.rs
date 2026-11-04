//! Plugins (the Plugins screen): what is installed and each one's banner,
//! installing it after a review, uninstalling, turning plugins and their
//! after-export actions on or off, their settings and secrets, whether
//! plugins AutoScene hasn't reviewed may run, running their actions,
//! opening their source, and opening, closing and answering their pages.
//! Each change emits `plugins:changed`; runs report as jobs, and an open
//! page hears of its plugin's changes through `plugin-page:event`. A page's
//! frame calls nothing itself: the app's page hands its calls on with the
//! session it holds ([`plugin_page_call`]).
//!
//! The page can't say yes alone to code AutoScene hasn't reviewed: turning
//! the setting on, and installing such a plugin, each ask the person in a
//! dialog of the system's own ([`confirm`]), with what the backend checked.
//! So does a plugin's source that isn't a GitHub repository, before it
//! opens. A script in the page could call these commands; it can't answer
//! that.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use as_api::AppSettings;
use as_core::{ExportId, JobId, ProjectId};
use as_plugin::calls::FileFilter;
use as_plugin::{ActionKind, PluginError, PluginId, SettingValue, Source, Ticket, Trigger, Trust};
use parking_lot::Mutex;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::{DialogExt, FileDialogBuilder, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;
use tauri_specta::Event;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::events::PluginsChanged;
use crate::paths;
use crate::plugins::calls::{self, PageIo, PageReply};
use crate::plugins::pages::{self, PageMount};
use crate::plugins::{self, PluginInfo, PluginReview, host, info, review};
use crate::state::{AppState, blocking};

/// Every installed plugin, by id (also the broken ones).
#[tauri::command]
#[specta::specta]
pub async fn plugins_list(state: State<'_, AppState>) -> AppResult<Vec<PluginInfo>> {
    let (host, lib) = (host(&state)?, state.library.clone());
    blocking(move || {
        let allow = lib.allows_unreviewed_plugins()?;
        Ok(host.store.list()?.iter().map(|p| info(p, &host.store, allow)).collect())
    })
    .await
}

/// A plugin's banner as a `data:` URL, read and checked again as its icon
/// is: none when it has no banner, or one that is no longer the picture its
/// package listed. The list gives only its hash (`PluginInfo.bannerHash`).
#[tauri::command]
#[specta::specta]
pub async fn plugin_banner(state: State<'_, AppState>, plugin_id: String) -> AppResult<Option<String>> {
    let id = PluginId::parse(&plugin_id)?;
    let host = host(&state)?;
    blocking(move || Ok(host.store.banner_data_url(&host.store.get(&id)?))).await
}

/// Open a plugin's source in the person's browser: the `repository` its
/// own plugin.json names, read here (the page hands no link). A plain
/// GitHub repository opens at once; any other link only after the person's
/// yes in the dialog of the system's own that a page's `ui.openExternal`
/// asks in too ([`confirm`]). Whether it opened: a no is `false`, with
/// nothing more to say.
#[tauri::command]
#[specta::specta]
pub async fn plugin_open_source(app: AppHandle, state: State<'_, AppState>, plugin_id: String) -> AppResult<bool> {
    let id = PluginId::parse(&plugin_id)?;
    let host = host(&state)?;
    blocking(move || {
        let p = host.store.get(&id)?;
        let Some((name, link)) = p.manifest.as_ref().and_then(|m| Some((&m.name, m.source_link()?))) else {
            return Err(AppError::new(ErrorCode::NotFound, "Its plugin.json names no source."));
        };
        let asked = format!("Plugin “{name}” links to its source at:\n\n{}", link.url);
        if link.asks && !confirm(&app, calls::OPEN_LINK_TITLE, asked, calls::OPEN_LINK_YES) {
            return Ok(false);
        }
        app.opener().open_url(&link.url, None::<&str>).map_err(|e| {
            log::warn!("plugin {id}: opening its source: {e}");
            AppError::new(ErrorCode::Io, "AutoScene couldn't open the link.")
        })?;
        Ok(true)
    })
    .await
}

/// One change of whether plugins AutoScene hasn't reviewed may run at a
/// time: each stops the runs it should, from what was stored before it.
static ALLOWING: Mutex<()> = Mutex::new(());

/// Let plugins AutoScene hasn't reviewed be installed and run, or not
/// (`settings_update` never changes it, so a save from a copy of the
/// settings made before can't turn it back). On, it asks the person first
/// ([`confirm`]); a no leaves it off. Off, their runs stop, running or still
/// waiting. Gives the settings as they are now; once the change is stored,
/// the page hears of it even when they can't be read back.
#[tauri::command]
#[specta::specta]
pub async fn plugins_allow_unreviewed(app: AppHandle, state: State<'_, AppState>, on: bool) -> AppResult<AppSettings> {
    let (lib, plugins, jobs) = (state.library.clone(), state.plugins.clone(), state.jobs.clone());
    let asking = app.clone();
    let (changed_it, settings) = blocking(move || {
        if on && !lib.allows_unreviewed_plugins()? && !confirm(&asking, ALLOW_TITLE, ALLOW_TEXT.into(), "Allow them") {
            return Ok((false, lib.settings()));
        }
        let _one = ALLOWING.lock();
        let before = lib.allows_unreviewed_plugins()?;
        lib.set_allows_unreviewed_plugins(on)?;
        if let Some(plugins) = plugins {
            plugins.allow_changed(&jobs, before, on);
        }
        Ok((before != on, lib.settings()))
    })
    .await?;
    if changed_it {
        changed(&app, None);
    }
    Ok(settings?)
}

const ALLOW_TITLE: &str = "Allow plugins AutoScene has not reviewed?";
const ALLOW_TEXT: &str = "They run with your user rights: they can read, change and send anything you can. \
Install only plugins from people you trust.\n\n\
Turning this off again stops their runs. It doesn't undo what a plugin set up outside AutoScene.";

/// Asks the person `title`, as `message` says, with `yes` and Cancel, in a
/// dialog of the system's own over the window: the page can neither answer
/// nor change it. Blocks until they answer, so never on the main thread.
fn confirm(app: &AppHandle, title: &str, message: String, yes: &str) -> bool {
    let mut dialog = app
        .dialog()
        .message(message)
        .title(title)
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(yes.into(), "Cancel".into()));
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.parent(&window);
    }
    dialog.blocking_show()
}

/// What installing the plugin of `t` asks, from what was checked here (not
/// what the page shows): its title, its text and the yes.
fn install_question(t: &Ticket) -> (String, String, &'static str) {
    let m = &t.manifest;
    let yes = match (&t.source, &t.previous) {
        (Source::Folder { .. }, _) => "Load",
        (_, Some(_)) => "Update",
        _ => "Install",
    };
    let signed = match &t.verdict.author_key {
        Some(key) if t.verdict.trust == Trust::Pinned => format!("Signed with key {}", key.key_id),
        _ => "Not signed".to_owned(),
    };
    let place = match &t.source {
        Source::Folder { path } => format!("\nFolder: {path}"),
        Source::Package { .. } => String::new(),
    };
    let text = format!(
        "AutoScene has not reviewed it. It runs with your user rights: it can read, change and send anything you can.\n\n\
         Id: {}\nVersion: {}\nSays it is by: {}\n{signed}{place}",
        m.id, m.version, m.author.name
    );
    (format!("{yes} {}?", m.name), text, yes)
}

/// Unpack and check the plugin package at `path`, for the review dialog,
/// which answers with `plugin_install` or `plugin_discard`.
#[tauri::command]
#[specta::specta]
pub async fn plugin_inspect(state: State<'_, AppState>, path: String) -> AppResult<PluginReview> {
    paths::check(&path)?;
    let (host, lib) = (host(&state)?, state.library.clone());
    blocking(move || {
        let allow = lib.allows_unreviewed_plugins()?;
        let ticket = host.store.stage(Path::new(&path))?;
        let review = review(&ticket, &host, allow);
        host.keep(ticket);
        Ok(review)
    })
    .await
}

/// The plugin packages opened from outside AutoScene since the page last
/// asked (a double-click on one, a start with one), oldest first. Each only
/// opens the install review: `plugin_inspect` checks its path again.
#[tauri::command]
#[specta::specta]
pub async fn plugins_take_opened() -> AppResult<Vec<String>> {
    Ok(plugins::opened::take().into_iter().filter_map(|p| p.into_os_string().into_string().ok()).collect())
}

/// Check the plugin in the folder `path` to load it where it is (a
/// developer's), for the review dialog. Only while plugins AutoScene hasn't
/// reviewed are allowed: a folder is never reviewed.
#[tauri::command]
#[specta::specta]
pub async fn plugin_inspect_folder(state: State<'_, AppState>, path: String) -> AppResult<PluginReview> {
    paths::check(&path)?;
    let (host, lib) = (host(&state)?, state.library.clone());
    blocking(move || {
        if !lib.allows_unreviewed_plugins()? {
            return Err(PluginError::NotAllowed.into());
        }
        let ticket = host.store.stage_folder(Path::new(&path))?;
        let review = review(&ticket, &host, true);
        host.keep(ticket);
        Ok(review)
    })
    .await
}

/// Install the plugin the review `ticket_id` showed. One AutoScene hasn't
/// reviewed needs the setting, `accept_unreviewed` (the person typed its
/// name), and the person's yes in a dialog of the system's own; a no keeps
/// the review (`cancelled`). Refused while the version it replaces runs.
#[tauri::command]
#[specta::specta]
pub async fn plugin_install(
    app: AppHandle,
    state: State<'_, AppState>,
    ticket_id: String,
    accept_unreviewed: bool,
) -> AppResult<PluginInfo> {
    let (host, lib) = (host(&state)?, state.library.clone());
    let asking = app.clone();
    let installed = blocking(move || {
        let allow = lib.allows_unreviewed_plugins()?;
        let ticket = host.take(&ticket_id)?;
        // Asked only when the page said yes and the setting allows it: else
        // the commit refuses anyway.
        if ticket.verdict.trust != Trust::Reviewed && allow && accept_unreviewed {
            let (title, text, yes) = install_question(&ticket);
            if !confirm(&asking, &title, text, yes) {
                host.keep(ticket);
                return Err(AppError::new(ErrorCode::Cancelled, "It wasn't installed."));
            }
        }
        // Its runs wait from here until the row names the new version.
        let changing = match host.change(&ticket.manifest.id, &ticket.manifest.name, "It is being updated.") {
            Ok(changing) => changing,
            Err(busy) => {
                host.keep(ticket);
                return Err(busy.into());
            }
        };
        let p = host.store.commit(ticket, allow, accept_unreviewed)?;
        drop(changing);
        Ok(info(&p, &host.store, allow))
    })
    .await?;
    changed(&app, Some(installed.id.clone()));
    Ok(installed)
}

/// Drop the review `ticket_id` (the dialog was closed), and the files it
/// unpacked. One that is gone already is no error.
#[tauri::command]
#[specta::specta]
pub async fn plugin_discard(state: State<'_, AppState>, ticket_id: String) -> AppResult<()> {
    let host = host(&state)?;
    blocking(move || {
        if let Ok(ticket) = host.take(&ticket_id) {
            host.store.discard(ticket);
        }
        Ok(())
    })
    .await
}

/// Uninstall a plugin, with the large files it downloaded, and what it kept
/// when `delete_data`. A folder it was loaded from stays. Refused while it
/// runs; its runs still waiting go.
#[tauri::command]
#[specta::specta]
pub async fn plugin_uninstall(
    app: AppHandle,
    state: State<'_, AppState>,
    plugin_id: String,
    delete_data: bool,
) -> AppResult<()> {
    let id = PluginId::parse(&plugin_id)?;
    let (host, jobs) = (host(&state)?, state.jobs.clone());
    // A run that starts meanwhile waits, then finds it gone; its downloaded
    // files go too.
    blocking(move || Ok(host.uninstall(&jobs, &id, delete_data)?)).await?;
    changed(&app, Some(plugin_id));
    Ok(())
}

/// Turn a plugin on or off. Off, its runs stop, and those still waiting go.
#[tauri::command]
#[specta::specta]
pub async fn plugin_set_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    plugin_id: String,
    enabled: bool,
) -> AppResult<PluginInfo> {
    let id = PluginId::parse(&plugin_id)?;
    let (host, lib, jobs) = (host(&state)?, state.library.clone(), state.jobs.clone());
    let p = blocking(move || {
        let p = host.store.set_enabled(&id, enabled)?;
        if !enabled {
            host.stop(&jobs, &id);
        }
        Ok(info(&p, &host.store, lib.allows_unreviewed_plugins()?))
    })
    .await?;
    changed(&app, Some(p.id.clone()));
    Ok(p)
}

/// Turn a plugin's export action on or off for after every export.
#[tauri::command]
#[specta::specta]
pub async fn plugin_set_after_export(
    app: AppHandle,
    state: State<'_, AppState>,
    plugin_id: String,
    action_id: String,
    on: bool,
) -> AppResult<PluginInfo> {
    let id = PluginId::parse(&plugin_id)?;
    let (host, lib) = (host(&state)?, state.library.clone());
    let p = blocking(move || {
        let p = host.store.set_after_export(&id, &action_id, on)?;
        Ok(info(&p, &host.store, lib.allows_unreviewed_plugins()?))
    })
    .await?;
    changed(&app, Some(p.id.clone()));
    Ok(p)
}

/// A plugin's settings: the person's value of each, else its default.
#[tauri::command]
#[specta::specta]
pub async fn plugin_settings_get(
    state: State<'_, AppState>,
    plugin_id: String,
) -> AppResult<BTreeMap<String, SettingValue>> {
    let id = PluginId::parse(&plugin_id)?;
    let host = host(&state)?;
    blocking(move || Ok(host.store.values(&id)?)).await
}

/// Change some of a plugin's settings, each checked against what it may
/// be; gives all of them as they are now.
#[tauri::command]
#[specta::specta]
pub async fn plugin_settings_set(
    app: AppHandle,
    state: State<'_, AppState>,
    plugin_id: String,
    values: BTreeMap<String, SettingValue>,
) -> AppResult<BTreeMap<String, SettingValue>> {
    let id = PluginId::parse(&plugin_id)?;
    let host = host(&state)?;
    let values = blocking(move || {
        let values = host.store.set_values(&id, values)?;
        host.settings_changed(&id);
        Ok(values)
    })
    .await?;
    changed(&app, Some(plugin_id));
    Ok(values)
}

/// Keep `value` in the system's keychain as a plugin's secret setting
/// `key`, in place of the one before. The value never comes back: the
/// plugin's info says only which are set.
#[tauri::command]
#[specta::specta]
pub async fn plugin_secret_set(
    app: AppHandle,
    state: State<'_, AppState>,
    plugin_id: String,
    key: String,
    value: String,
) -> AppResult<PluginInfo> {
    let id = PluginId::parse(&plugin_id)?;
    let (host, lib) = (host(&state)?, state.library.clone());
    let p = blocking(move || {
        let p = host.store.secret_set(&id, &key, &value)?;
        host.settings_changed(&id);
        Ok(info(&p, &host.store, lib.allows_unreviewed_plugins()?))
    })
    .await?;
    changed(&app, Some(p.id.clone()));
    Ok(p)
}

/// Remove a plugin's secret setting `key` from the system's keychain.
#[tauri::command]
#[specta::specta]
pub async fn plugin_secret_clear(
    app: AppHandle,
    state: State<'_, AppState>,
    plugin_id: String,
    key: String,
) -> AppResult<PluginInfo> {
    let id = PluginId::parse(&plugin_id)?;
    let (host, lib) = (host(&state)?, state.library.clone());
    let p = blocking(move || {
        let p = host.store.secret_clear(&id, &key)?;
        host.settings_changed(&id);
        Ok(info(&p, &host.store, lib.allows_unreviewed_plugins()?))
    })
    .await?;
    changed(&app, Some(p.id.clone()));
    Ok(p)
}

/// Run a plugin's export action on an export (from its menu), or its
/// command (from the palette), in the project when it needs one. `None`
/// when that run waits or runs already. It reports as a job.
#[tauri::command]
#[specta::specta]
pub async fn plugin_run(
    app: AppHandle,
    plugin_id: String,
    action_id: String,
    kind: ActionKind,
    project_id: Option<ProjectId>,
    export_id: Option<ExportId>,
) -> AppResult<Option<JobId>> {
    let id = PluginId::parse(&plugin_id)?;
    let trigger = match kind {
        ActionKind::ExportAction => Trigger::Menu,
        ActionKind::Command => Trigger::Palette,
    };
    blocking(move || plugins::queue_run(&app, &id, kind, &action_id, trigger, project_id, export_id, None)).await
}

/// Open a plugin's page `page_id` for its frame, in the project
/// `project_id` when it is a project page: the session the frame loads it
/// from, and what the page may use. Only while the plugin may run.
#[tauri::command]
#[specta::specta]
pub async fn plugin_page_open(
    state: State<'_, AppState>,
    plugin_id: String,
    page_id: String,
    project_id: Option<ProjectId>,
) -> AppResult<PageMount> {
    let id = PluginId::parse(&plugin_id)?;
    let (host, lib) = (host(&state)?, state.library.clone());
    blocking(move || pages::open(&host, &lib, &id, &page_id, project_id)).await
}

/// Close a plugin page's session: its frame went away. One that is closed
/// already is no error.
#[tauri::command]
#[specta::specta]
pub async fn plugin_page_close(state: State<'_, AppState>, session: String) -> AppResult<()> {
    let host = host(&state)?;
    blocking(move || {
        host.close_page(&session);
        Ok(())
    })
    .await
}

/// A plugin page's call of `method` with `params`, which the app's page
/// hands on with the page's `session`: Rust knows the plugin from it, never
/// from the page. The params come as their JSON, so nothing reads params
/// past their method's cap. Its answer, an effect for the app's page to
/// carry out first, or the page's error; a closed page's call gets `closed`.
#[tauri::command]
#[specta::specta]
pub async fn plugin_page_call(
    app: AppHandle,
    state: State<'_, AppState>,
    session: String,
    method: String,
    params: String,
) -> AppResult<PageReply> {
    let (host, lib) = (host(&state)?, state.library.clone());
    blocking(move || Ok(calls::page_call(lib, &host, &AppIo(&app), &session, &method, &params))).await
}

/// The app's page is there for the open plugin page `session`: it says so
/// every 2 s on macOS and Linux, where a page that hangs would hang it too,
/// and is asked about once it stops ([`pages::watch`]).
#[tauri::command]
#[specta::specta]
pub async fn plugin_page_heartbeat(state: State<'_, AppState>, session: String) -> AppResult<()> {
    let host = host(&state)?;
    if host.pages.get(&session).is_some() {
        host.beats.beat(&session, Instant::now());
    }
    Ok(())
}

/// What a page's own methods reach in the app: the clipboard, the person's
/// browser after their yes, the system's file dialogs over the window, and
/// the jobs its plugin's runs are.
struct AppIo<'a>(&'a AppHandle);

impl AppIo<'_> {
    /// A file dialog titled `title` over the window that offers `filters`.
    fn file_dialog(&self, title: &str, filters: &[FileFilter]) -> FileDialogBuilder<tauri::DynRuntime> {
        let mut dialog = self.0.dialog().file().set_title(title);
        for f in filters {
            let extensions: Vec<&str> = f.extensions.iter().map(String::as_str).collect();
            dialog = dialog.add_filter(&f.name, &extensions);
        }
        if let Some(window) = self.0.get_webview_window("main") {
            dialog = dialog.set_parent(&window);
        }
        dialog
    }
}

impl PageIo for AppIo<'_> {
    fn in_front(&self) -> bool {
        self.0.get_webview_window("main").and_then(|w| w.is_focused().ok()).unwrap_or(false)
    }

    fn copy_text(&self, text: &str) -> Result<(), String> {
        self.0.clipboard().write_text(text).map_err(|e| e.to_string())
    }

    fn confirm(&self, title: &str, text: &str, yes: &str) -> bool {
        confirm(self.0, title, text.to_owned(), yes)
    }

    fn open_url(&self, url: &str) -> Result<(), String> {
        self.0.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
    }

    fn save_to(&self, title: &str, name: &str, filters: &[FileFilter]) -> Option<PathBuf> {
        self.file_dialog(title, filters).set_file_name(name).blocking_save_file()?.into_path().ok()
    }

    fn pick(&self, title: &str, filters: &[FileFilter]) -> Option<PathBuf> {
        self.file_dialog(title, filters).blocking_pick_file()?.into_path().ok()
    }

    fn queue_run(
        &self,
        id: &PluginId,
        kind: ActionKind,
        action: &str,
        project_id: Option<ProjectId>,
        export_id: Option<ExportId>,
        input: Option<serde_json::Value>,
    ) -> AppResult<Option<JobId>> {
        plugins::queue_run(self.0, id, kind, action, Trigger::Page, project_id, export_id, input)
    }
}

fn changed(app: &AppHandle, plugin_id: Option<String>) {
    let _ = PluginsChanged { plugin_id }.emit(app);
}

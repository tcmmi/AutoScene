//! AutoScene desktop shell.
//!
//! Owns the window, typed IPC (tauri-specta → `app/src/bindings.ts`), the
//! library, background jobs (indexing, previews, analysis, exports), logging
//! and updates. AI analysis runs in a separate engine process (this same
//! executable started with `--engine`) so a GPU or driver crash can never
//! take the UI down.

mod commands;
mod error;
mod events;
mod keep_awake;
#[cfg(target_os = "macos")]
mod mac_menu;
mod paths;
mod plugins;
mod state;
mod updater;
mod window;
mod work;

use as_core::AppPaths;
use as_project::Library;
use tauri::Manager;
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};
use tauri_plugin_window_state::StateFlags;
use tauri_specta::{collect_commands, collect_events};

/// Single source of truth for the IPC surface. Also used by the
/// `export-bindings` bin so CI can fail when bindings.ts is stale.
pub fn specta_builder() -> tauri_specta::Builder<tauri::DynRuntime> {
    tauri_specta::Builder::<tauri::DynRuntime>::new()
        .error_handling(tauri_specta::ErrorHandlingMode::Throw)
        .dangerously_cast_bigints_to_number()
        .commands(collect_commands![
            commands::system::app_info,
            commands::system::reveal_path,
            commands::system::clipboard_write,
            commands::system::clipboard_read,
            commands::system::open_logs_dir,
            commands::system::menu_set_keys,
            commands::system::media_tools,
            commands::system::models,
            commands::library::projects_list,
            commands::library::project_get,
            commands::library::project_create,
            commands::library::project_rename,
            commands::library::project_set_content,
            commands::library::project_set_fast_scan,
            commands::library::project_delete,
            commands::library::import_scan,
            commands::library::settings_get,
            commands::library::settings_update,
            commands::media::media_list,
            commands::media::media_get,
            commands::media::media_add,
            commands::media::media_remove,
            commands::media::media_retry,
            commands::media::media_relink,
            commands::media::media_relink_folder,
            commands::media::media_indexing_still,
            commands::media::media_prepare_preview,
            commands::media::media_set_audio,
            commands::media::media_frame_map,
            commands::media::media_segments,
            commands::media::media_analyze,
            commands::anilist::anilist_search_text,
            commands::anilist::anilist_search,
            commands::anilist::anilist_confirm,
            commands::anilist::anilist_reject,
            commands::anilist::anilist_suggest,
            commands::anilist::anilist_apply,
            commands::anilist::anilist_auto,
            commands::anilist::anilist_names,
            commands::anilist::anilist_dismiss,
            commands::anilist::anilist_accept,
            commands::cast::cast_get,
            commands::cast::cast_tracks,
            commands::cast::cast_edit,
            commands::cast::cast_portrait,
            commands::cast::media_presence,
            commands::cast::character_shots,
            commands::export::export_presets,
            commands::export::export_default_dir,
            commands::export::export_start,
            commands::export::export_plan,
            commands::export::exports_list,
            commands::export::export_save_list,
            commands::pack::pack_get,
            commands::pack::pack_save,
            commands::pack::pack_preview,
            commands::pack::pack_override,
            commands::pack::pack_plan,
            commands::pack::pack_export,
            commands::plugins::plugins_list,
            commands::plugins::plugin_banner,
            commands::plugins::plugins_allow_unreviewed,
            commands::plugins::plugin_inspect,
            commands::plugins::plugin_inspect_folder,
            commands::plugins::plugin_install,
            commands::plugins::plugin_discard,
            commands::plugins::plugin_uninstall,
            commands::plugins::plugin_set_enabled,
            commands::plugins::plugin_set_after_export,
            commands::plugins::plugin_settings_get,
            commands::plugins::plugin_settings_set,
            commands::plugins::plugin_run,
            commands::plugins::plugin_secret_set,
            commands::plugins::plugin_secret_clear,
            commands::plugins::plugin_page_open,
            commands::plugins::plugin_page_close,
            commands::plugins::plugin_page_call,
            commands::plugins::plugin_page_heartbeat,
            commands::jobs::jobs_list,
            commands::jobs::job_cancel,
            commands::jobs::jobs_clear_finished,
            updater::update_check,
            updater::update_install,
            updater::update_relaunch,
        ])
        .events(collect_events![
            events::MenuAction,
            events::JobChanged,
            events::JobProgressEvent,
            events::LibraryChanged,
            events::MediaChanged,
            events::ExportChanged,
            events::AnalysisChanged,
            events::CastChanged,
            events::AniListChanged,
            events::PortraitsChanged,
            events::PackChanged,
            events::PluginsChanged,
            events::PluginPageEvent,
            updater::UpdateProgress,
        ])
}

pub const BINDINGS_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../src/bindings.ts");

/// The types of the plugin protocol a page speaks (`sdk.js` in the kit).
/// Exported apart from the bindings: those import `@tauri-apps/api`, which
/// a page must never load.
pub const PROTOCOL_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../src/plugin-kit/protocol.ts");

pub fn export_bindings(builder: &tauri_specta::Builder<tauri::DynRuntime>) {
    builder
        .export(specta_typescript::Typescript::default().header("// @ts-nocheck\n/* eslint-disable */"), BINDINGS_PATH)
        .expect("failed to export TypeScript bindings");
    export_protocol();
}

fn export_protocol() {
    std::fs::write(PROTOCOL_PATH, protocol_ts()).expect("failed to write the plugin protocol types");
}

/// `protocol.ts`: every host method, its params and results, and what a
/// page is told about its plugin. This export has no bigint cast, so a
/// 64-bit integer in these types fails it. A JSON value of any shape is
/// `unknown` there: specta can't spell `serde_json::Value`, which holds
/// itself and an `i64`.
fn protocol_ts() -> String {
    use as_plugin::calls::*;
    let types = specta::Types::default()
        .register::<HostMethod>()
        .register::<CallCode>()
        .register::<CallError>()
        .register::<SecretsGet>()
        .register::<ProjectRef>()
        .register::<ExportRef>()
        .register::<FileRef>()
        .register::<ResolvedFile>()
        .register::<PageProject>()
        .register::<PageEpisode>()
        .register::<ProgramProject>()
        .register::<ProgramEpisode>()
        .register::<PageExportSummary>()
        .register::<PageExport>()
        .register::<PageClip>()
        .register::<NetRequest>()
        .register::<NetResponse>()
        .register::<StateKey>()
        .register::<StateSet>()
        .register::<ServiceRequestParams>()
        .register::<ActionsRun>()
        .register::<Toast>()
        .register::<ToastTone>()
        .register::<Navigate>()
        .register::<CopyText>()
        .register::<OpenExternal>()
        .register::<FileFilter>()
        .register::<SaveFile>()
        .register::<OpenFile>()
        .register::<OpenedFile>()
        .register::<as_plugin::PageSpec>()
        .register::<as_plugin::Permission>()
        .register::<as_plugin::SettingValue>();
    let json = <serde_json::Value as specta::Type>::definition(&mut specta::Types::default());
    let unknown = <specta_typescript::Unknown as specta::Type>::definition(&mut specta::Types::default());
    let types = specta_util::Remapper::new().rule(json, unknown).remap_types(types);
    let ts = specta_typescript::Typescript::default()
        .header(
            "// Generated by `npm run bindings` from crates/as-plugin/src/calls.rs. Do not edit.\n/* eslint-disable */",
        )
        .export(&types, specta_serde::Format)
        .expect("failed to export the plugin protocol types");
    let caps: Vec<String> =
        HostMethod::ALL.iter().map(|m| format!("\t\"{}\": {},", m.name(), m.max_params())).collect();
    format!(
        "{ts}\n/** The most bytes of JSON each method's params may take: past that, AutoScene reads none of them. */\n\
         export const MAX_PARAMS: Record<HostMethod, number> = {{\n{}\n}};\n",
        caps.join("\n")
    )
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let specta = specta_builder();
    #[cfg(debug_assertions)]
    export_bindings(&specta);

    let paths = AppPaths::resolve();
    let _ = std::fs::create_dir_all(paths.logs());

    let log_plugin = tauri_plugin_log::Builder::default()
        .clear_targets()
        .targets([
            Target::new(TargetKind::Stdout),
            Target::new(TargetKind::Folder { path: paths.logs(), file_name: Some("app".into()) }),
        ])
        .rotation_strategy(RotationStrategy::KeepSome(5))
        .max_file_size(8 * 1024 * 1024)
        .level(if cfg!(debug_assertions) { log::LevelFilter::Debug } else { log::LevelFilter::Info })
        // Webview/tao internals are noisy at debug.
        .level_for("tao", log::LevelFilter::Warn)
        .level_for("wry", log::LevelFilter::Warn)
        .build();

    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default()
        .runtime(tauri_runtime_wry::Wry::default())
        // Must be first: a second launch just focuses the running window.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(log_plugin)
        // Size and place come back, never the frame: the window's chrome is ours
        // (a state saved by an older, framed build would double the title bar).
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(StateFlags::all() - StateFlags::DECORATIONS)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        // Plugin pages, in frames of the main webview only. Registered here,
        // before setup: a page may be asked for before the state is there.
        .register_asynchronous_uri_scheme_protocol("asp", plugins::pages::protocol)
        // A document of the main frame starts (a reload, or one after its
        // renderer crashed): the plugin pages its frames showed are gone.
        .on_page_load(|webview, load| {
            if webview.label() == "main"
                && load.event() == tauri::webview::PageLoadEvent::Started
                && let Some(plugins) = webview.try_state::<state::AppState>().and_then(|s| s.plugins.clone())
            {
                plugins.close_all_pages();
            }
        })
        .invoke_handler(specta.invoke_handler());

    #[cfg(target_os = "macos")]
    {
        builder = builder.menu(mac_menu::build).on_menu_event(mac_menu::on_event);
    }

    builder
        .setup(move |app| {
            specta.mount_events(app);
            // The data folder's path names the user (their home folder), so
            // it's logged at debug: release logs, which people share, keep
            // info and up.
            log::info!("AutoScene {} starting", app.package_info().version);
            log::debug!("data dir {}", paths.data.display());
            let library = match Library::open(&paths) {
                Ok(l) => l,
                Err(e) => {
                    log::error!("opening the library failed: {}", e.detail());
                    app.dialog()
                        .message(format!(
                            "AutoScene couldn't open its library.

{e}

The log is in {}",
                            paths.logs().display()
                        ))
                        .title("AutoScene")
                        .kind(MessageDialogKind::Error)
                        .blocking_show();
                    std::process::exit(1);
                }
            };
            let handle = app.handle().clone();
            app.manage(state::AppState::new(&handle, library, &paths));
            // After the state: the page calls commands as soon as it loads.
            window::create_main(app)?;
            if !cfg!(debug_assertions)
                && let Some(main) = app.get_webview_window("main")
            {
                window::no_browser_menu(&main);
            }
            // Resume interrupted indexing and previews; locate ffmpeg and probe
            // the hardware off the UI thread so the first job starts warm.
            std::thread::spawn(move || {
                // Models a better one replaced (their media are found again below).
                if let Err(e) = handle.state::<state::AppState>().models.prune() {
                    log::warn!("removing unused models: {}", e.detail());
                }
                work::recover_all(&handle);
                if let Ok(tools) = handle.state::<state::AppState>().tools() {
                    let _ = tools.caps();
                }
                // What a plugin install or uninstall that never finished left.
                if let Some(plugins) = &handle.state::<state::AppState>().plugins {
                    plugins.store.remove_leftovers();
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building AutoScene")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                // Never leave ffmpeg processes running after the window closes.
                state::shutdown_work(app);
            }
        });
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_protocol_types_name_every_host_method_and_import_nothing() {
        let ts = super::protocol_ts();
        for method in as_plugin::HostMethod::ALL {
            assert!(ts.contains(&format!("\"{}\"", method.name())), "{}", method.name());
            // With its cap, which the app's page holds a page's calls to.
            assert!(ts.contains(&format!("\t\"{}\": {},\n", method.name(), method.max_params())), "{}", method.name());
        }
        // A page loads the kit alone: no Tauri, nothing else.
        assert!(!ts.lines().any(|l| l.starts_with("import ")), "{ts}");
    }
}

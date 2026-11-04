//! AutoScene desktop shell.
//!
//! Owns the window, typed IPC (tauri-specta → `app/src/bindings.ts`), the
//! library, background jobs (indexing, previews, analysis, exports), logging
//! and updates. AI analysis runs in a separate engine process (this same
//! executable started with `--engine`) so a GPU or driver crash can never
//! take the UI down.

mod accel;
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
use as_project::{Library, ProjectResult};
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
            commands::system::scan_defaults_get,
            commands::system::scan_default_set,
            commands::system::scan_notice_done,
            commands::hardware::hardware_info,
            commands::hardware::accel_install,
            commands::hardware::accel_cancel,
            commands::hardware::accel_remove,
            commands::hardware::accel_try_again,
            commands::hardware::accel_notices,
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
            commands::pack::pack_filter_check,
            commands::pack::pack_action_run,
            commands::plugins::plugins_list,
            commands::plugins::plugin_banner,
            commands::plugins::plugin_open_source,
            commands::plugins::plugins_allow_unreviewed,
            commands::plugins::plugin_inspect,
            commands::plugins::plugin_inspect_folder,
            commands::plugins::plugin_install,
            commands::plugins::plugin_discard,
            commands::plugins::plugin_uninstall,
            commands::plugins::plugin_set_enabled,
            commands::plugins::plugin_set_after_export,
            commands::plugins::plugin_set_event,
            commands::plugins::plugin_settings_get,
            commands::plugins::plugin_settings_set,
            commands::plugins::plugin_run,
            commands::catalog::catalog_list,
            commands::catalog::catalog_refresh,
            commands::catalog::catalog_get,
            commands::catalog::catalog_cancel,
            commands::plugins::plugins_take_opened,
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
            events::HardwareChanged,
            events::AccelProgress,
            events::AccelFinished,
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
            events::CatalogChanged,
            events::CatalogProgress,
            events::PluginsOpened,
            updater::UpdateProgress,
        ])
        // The name parts the pack export's templates can use: its buttons read them.
        .constant("NAME_PARTS", as_project::naming::NAME_PARTS)
        .constant("FILE_PARTS", as_project::naming::FILE_PARTS)
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
        // Must be first: a second launch just focuses the running window,
        // and hands over the plugin packages it was started with.
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            let args = argv.into_iter().skip(1).map(std::ffi::OsString::from);
            plugins::opened::offer(app, plugins::opened::packages(args, Some(std::path::Path::new(&cwd))));
            show_main(app);
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
            // The plugin packages this start was asked to open (`main` took
            // `--engine` already); the page takes them once it loads.
            let cwd = std::env::current_dir().ok();
            plugins::opened::offer(
                app.handle(),
                plugins::opened::packages(std::env::args_os().skip(1), cwd.as_deref()),
            );
            // The data folder's path names the user (their home folder), so
            // it's logged at debug: release logs, which people share, keep
            // info and up.
            log::info!("AutoScene {} starting", app.package_info().version);
            log::debug!("data dir {}", paths.data.display());
            let library = match open_library(&paths) {
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
            // Reads the plugin catalog held, before the window opens.
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
            let checks = handle.clone();
            let runtimes = paths.runtimes();
            std::thread::spawn(move || {
                // What this computer analyses on (its log line names it), so
                // the probe never meets the first click.
                handle.state::<state::AppState>().library.computer();
                // Models a better one replaced (their media are found again below).
                if let Err(e) = handle.state::<state::AppState>().models.prune() {
                    log::warn!("removing unused models: {}", e.detail());
                }
                // A uv of an older AutoScene; what it fetched for plugins stays.
                as_plugin::uv::prune(&runtimes);
                // What a removal or an update of NVIDIA's files left, and the
                // release this build pins when an older one is here.
                accel::upkeep(&handle);
                work::recover_all(&handle);
                if let Ok(tools) = handle.state::<state::AppState>().tools() {
                    let _ = tools.caps();
                }
                remove_leftovers(handle.state::<state::AppState>().plugins.as_deref(), &std::env::temp_dir());
            });
            // The plugin catalog, checked shortly after the start and then daily.
            plugins::catalog::start(checks);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building AutoScene")
        .run(|app, event| match event {
            // Never leave ffmpeg processes running after the window closes.
            tauri::RunEvent::Exit => state::shutdown_work(app),
            // Files the system asks the running app to open (macOS). Activating
            // the app doesn't bring back a minimized window: the review would
            // open where the person can't see it.
            tauri::RunEvent::Opened { urls } => {
                let found = plugins::opened::from_urls(&urls);
                if !found.is_empty() {
                    plugins::opened::offer(app, found);
                    show_main(app);
                }
            }
            _ => {}
        });
}

/// The library, knowing what this computer analyses on: new projects start
/// with Fast Scan where those graphics aren't built for AI work. Debug builds
/// take `AUTOSCENE_COMPUTER` (`for-ai`, `not-for-ai`, `integrated`,
/// `processor`, `unknown`) in its place, so the dev machine can be seen as
/// another computer.
fn open_library(paths: &AppPaths) -> ProjectResult<Library> {
    #[cfg(debug_assertions)]
    if let Ok(name) = std::env::var("AUTOSCENE_COMPUTER") {
        match stand_in(&name) {
            Some(probe) => {
                let taken = as_project::computer::choose(&probe(), &as_project::computer::Choice::Auto);
                log::warn!("AUTOSCENE_COMPUTER={name}: this computer is taken as {taken}");
                return Library::open_with(paths, probe);
            }
            None => log::warn!("AUTOSCENE_COMPUTER={name} names no computer; reading this one"),
        }
    }
    Library::open_with(paths, as_project::computer::probe)
}

/// A computer the debug override names: one of each class, as DirectML would
/// list it.
#[cfg(debug_assertions)]
fn stand_in(name: &str) -> Option<fn() -> as_project::computer::Machine> {
    use as_project::computer::{Adapter, Machine};
    fn one(vendor: u32, device: u32, name: &str, integrated: bool) -> Machine {
        Machine::Windows(Some(vec![Adapter { vendor, device, name: name.into(), integrated, luid: 0x1_0828 }]))
    }
    Some(match name {
        "for-ai" => || one(0x10DE, 0x24C9, "NVIDIA GeForce RTX 3060 Ti", false),
        "not-for-ai" => || one(0x10DE, 0x1C03, "NVIDIA GeForce GTX 1060 6GB", false),
        "integrated" => || one(0x8086, 0x9A49, "Intel Iris Xe Graphics", true),
        "processor" => || Machine::Windows(Some(Vec::new())),
        "unknown" => || Machine::Windows(None),
        _ => return None,
    })
}

/// Brings the window up, out of the Dock or taskbar, and to the front.
fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Removes what plugins left at the start: what a plugin install or
/// uninstall that never finished left, downloads from the catalog that
/// didn't and large files no installed plugin names any more (with
/// `plugins`), and the scratch folders plugin runs left in
/// `temp` (the app crashed, or a file in one was held past its removal's
/// tries), plugins or none. A run that holds its folder keeps it, also one
/// of a run that starts meanwhile.
fn remove_leftovers(plugins: Option<&plugins::PluginHost>, temp: &std::path::Path) {
    if let Some(plugins) = plugins {
        plugins.store.remove_leftovers();
        plugins.catalog.clear_downloads();
        plugins.prune_resources();
    }
    let removed = as_plugin::scratch::sweep(temp);
    if removed > 0 {
        log::info!("removed {removed} scratch folders plugin runs left");
    }
}

#[cfg(test)]
mod tests {
    use as_project::computer::{self, Choice};

    use super::*;

    #[test]
    fn leftover_scratch_folders_are_swept_at_start() {
        let temp = std::env::temp_dir().join(format!("autoscene-scratch-sweep-{}", std::process::id()));
        std::fs::remove_dir_all(&temp).ok();
        std::fs::create_dir_all(&temp).unwrap();
        // A run that crashed left its folder, with its marker and its frames.
        let left = temp.join("as-k3j9x0qa");
        let frames = left.join(as_plugin::scratch::TEMP).join("frames");
        std::fs::create_dir_all(&frames).unwrap();
        std::fs::write(left.join(as_plugin::scratch::MARKER), "").unwrap();
        std::fs::write(frames.join("0001.png"), "frame").unwrap();
        // A run that lives, and a folder of another program's.
        let live = as_plugin::scratch::Scratch::new_in(&temp).unwrap();
        let other = temp.join("as-x0qak3j9");
        std::fs::create_dir(&other).unwrap();
        std::fs::write(other.join("theirs.txt"), "theirs").unwrap();
        // Swept with the rest of what was left, also when plugins are off.
        super::remove_leftovers(None, &temp);
        assert!(!left.exists());
        assert!(live.root().join(as_plugin::scratch::MARKER).is_file());
        assert_eq!(std::fs::read_to_string(other.join("theirs.txt")).unwrap(), "theirs");
        // Its removal runs on a thread of its own: done before the test's
        // folder goes.
        let root = live.root().to_owned();
        drop(live);
        let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while std::fs::symlink_metadata(&root).is_ok() {
            assert!(std::time::Instant::now() < until, "{} is still there", root.display());
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        std::fs::remove_dir_all(&temp).unwrap();
    }

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

    #[test]
    fn the_app_library_reads_this_computer() {
        if std::env::var_os("AUTOSCENE_COMPUTER").is_some() {
            return; // a debug override names another computer
        }
        let dir = std::env::temp_dir().join(format!("as-app-computer-{}", std::process::id()));
        let lib = open_library(&AppPaths::at(&dir)).unwrap();
        // The computer the probe reads, not the unknown one a library opened
        // for tests knows (unless DXCore really can't be read).
        assert_eq!(lib.computer(), computer::choose(&computer::probe(), &Choice::Auto));
        if !cfg!(any(windows, target_os = "macos")) {
            assert_eq!(lib.computer().class, computer::Class::Processor);
        }
        drop(lib);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(debug_assertions)]
    #[test]
    fn a_debug_override_names_the_computer() {
        let class = |name| stand_in(name).map(|probe| computer::choose(&probe(), &Choice::Auto).class);
        assert_eq!(class("for-ai"), Some(computer::Class::ForAi));
        assert_eq!(class("not-for-ai"), Some(computer::Class::NotForAi));
        assert_eq!(class("integrated"), Some(computer::Class::Integrated));
        assert_eq!(class("processor"), Some(computer::Class::Processor));
        assert_eq!(class("unknown"), Some(computer::Class::Unknown));
        assert_eq!(class("laptop"), None);
        let probe = stand_in("integrated").unwrap();
        assert_eq!(computer::choose(&probe(), &Choice::Auto).name, "Intel Iris Xe Graphics");
    }
}

//! Plugin pages in the app: the `asp` scheme's handler, which answers the
//! frames of the main webview from the plugin host's open pages
//! ([`as_plugin::pages`]), the kit the app was built with, opening a page
//! ([`open`]), and asking the person about one that hangs the app's page
//! ([`watch`]).
//!
//! The main webview is the only one that frames a page, and the only one
//! the scheme answers ([`scheme_allowed`]): there is never a second
//! webview, which Tauri would let call every command. The handler runs on
//! the UI thread on Windows, so it answers from the blocking pool, where
//! files may be read, at most [`SERVING`] answers at once.
//!
//! Tauri counts an address of a scheme the app registers as the app's own
//! (`is_local_url`), so an `asp` document that reached its IPC would have
//! the main window's capability. The app can't refuse such a call itself:
//! Tauri hands a command no address, and its ACL counts every registered
//! scheme as local. So the walls are before the IPC, and differ by system.
//! On macOS and Linux a page never gets Tauri's scripts, which carry the
//! invoke key and run in the main frame only, and no plugin of the app adds
//! one to every frame (`only_the_main_webview_is_ever_built` keeps the
//! app's code so). On Windows wry adds every script to every frame, so a
//! page has Tauri's internals and the invoke key, and two walls hold: the
//! page CSP's `connect-src 'none'`, with Tauri refusing the `Origin: null`
//! of a page's opaque origin, keeps it from the IPC's own scheme; and wry
//! hears `postMessage` only from the main frame (it listens on the webview,
//! not on its frames). Only a real frame shows that the last one holds.

use std::collections::HashMap;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use as_core::{Msg, ProjectId, msg};
use as_plugin::pages::{self, AssetKit, KitSource, Served};
use as_plugin::{Grants, PageScope, PageSpec, Permission, PluginError, PluginId, Trust};
use as_project::Library;
use parking_lot::Mutex;
use serde::Serialize;
use specta::Type;
use tauri::http::header::RANGE;
use tauri::http::{HeaderValue, Request, Response, StatusCode};
use tauri::{AppHandle, DynRuntime, Manager, UriSchemeContext, UriSchemeResponder};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tokio::sync::Semaphore;

use super::{PluginHost, PluginSource, runnable, source, trust};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::state::AppState;

/// An open page, for its frame: where it loads from, and what it may use.
#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PageMount {
    /// Its session, which the page's calls name.
    pub session: String,
    /// The frame's address: `B/<session>/<entry>`.
    pub src: String,
    pub plugin: PageMountPlugin,
    pub page: PageSpec,
    /// The project it is open in (a project page's).
    pub project_id: Option<String>,
    /// What it may use, for its start.
    pub permissions: Vec<Permission>,
    pub hosts: Vec<String>,
    /// Its plugin runs a service.
    pub service: bool,
    /// The protocol its plugin declared.
    pub protocol: u32,
}

/// The plugin of an open page, as its frame's bar shows it.
#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PageMountPlugin {
    pub id: String,
    pub name: String,
    pub version: String,
    pub trust: Trust,
    pub source: PluginSource,
    /// A `data:` URL of its icon, when it has a sound one.
    pub icon: Option<String>,
}

/// Opens the page `page_id` of the plugin `id`, in the project `project_id`
/// when it is a project page (a global page is opened in none), while the
/// plugin may run. Refused while the plugin is being installed, updated or
/// uninstalled; a change that starts once it is open closes it, and so does
/// turning it off or the setting going off. Past the most pages open at
/// once, the one opened first closes, and hears why. A page of a plugin that
/// runs a service starts it, if it doesn't run, however often it crashed.
pub fn open(
    host: &Arc<PluginHost>,
    lib: &Library,
    id: &PluginId,
    page_id: &str,
    project_id: Option<ProjectId>,
) -> AppResult<PageMount> {
    if let Some(project_id) = project_id {
        lib.project(project_id)?;
    }
    // Looked at and opened in one step, the setting too: a change of the
    // plugin, its turning off or the setting going off either is stored
    // before and refuses it, or closes it after (pages close under the same
    // lock).
    let busy = host.busy.lock();
    let allow = lib.allows_unreviewed_plugins()?;
    let p = host.store.get(id)?;
    let m = runnable(&p, allow)?;
    if busy.changing.contains(id) {
        let waits = msg!("plugin.beingUpdated", "{{name}} is being updated.", name = &m.name);
        return Err(AppError::new(ErrorCode::NotReady, waits));
    }
    let page = m
        .page(page_id)
        .ok_or_else(|| PluginError::Invalid(msg!("plugin.noSuchPage", "{{name}} has no such page.", name = &m.name)))?;
    let project = match (page.scope, project_id) {
        (PageScope::Project, Some(project_id)) => Some(project_id.to_string()),
        (PageScope::Project, None) => {
            return Err(invalid(msg!("plugin.pageNeedsProject", "Open a project to see this page.")));
        }
        (PageScope::Global, Some(_)) => {
            return Err(invalid(msg!("plugin.pageNotProjects", "This page isn't a project's.")));
        }
        (PageScope::Global, None) => None,
    };
    let grants = Grants::of(&p)?;
    // Where `state.*` keeps what its pages keep (a program's run makes it
    // too). Under the lock: an uninstall that deletes it waits, and closes
    // this page first.
    std::fs::create_dir_all(host.store.data_dir(id)).map_err(PluginError::from)?;
    let (mount, closed) = host.pages.open(&p, grants, page, project)?;
    // Its silence counts from here: a page that hangs the app's page before
    // its first heartbeat is asked about too.
    host.beats.beat(&mount.token, Instant::now());
    drop(busy);
    host.tell_closed(closed, &msg!("plugin.closedBecause.tooMany", "Too many plugin pages were open."));
    if mount.grants.service() {
        host.services.forgive(id);
        host.warm_service(mount.clone());
    }
    let grants = &mount.grants;
    Ok(PageMount {
        session: mount.token.clone(),
        src: mount.src(),
        plugin: PageMountPlugin {
            id: id.to_string(),
            name: mount.name.clone(),
            version: mount.version.clone(),
            trust: trust(&p.record),
            source: source(&p.record),
            icon: host.store.icon_data_url(&p),
        },
        page: mount.page.clone(),
        project_id: mount.project.clone(),
        permissions: grants.permissions.clone(),
        hosts: grants.hosts.clone(),
        service: grants.service(),
        protocol: grants.protocol,
    })
}

fn invalid(said: Msg) -> AppError {
    AppError::new(ErrorCode::Invalid, said)
}

/// How long the app's page may go without saying it is there for an open
/// plugin page before the person is asked about it.
pub const STALLED: Duration = Duration::from_secs(5);

/// When the app's page last said it is there for each open page: as the
/// page opened, then every 2 s on macOS and Linux. There a page's frame
/// shares the app page's thread, so a page that hangs hangs the app's page
/// too, and its own watchdog with it ([`watch`]). On Windows the frame runs
/// in a process of its own, and the app's page sees a hung page itself:
/// nothing reads these there.
#[derive(Default)]
pub struct Heartbeats(Mutex<HashMap<String, Beat>>);

#[derive(Clone, Copy, Debug)]
struct Beat {
    last: Instant,
    /// The person was asked about it since.
    asked: bool,
}

impl Heartbeats {
    /// The app's page is there for the page `session` at `now`.
    pub fn beat(&self, session: &str, now: Instant) {
        self.0.lock().insert(session.to_owned(), Beat { last: now, asked: false });
    }

    /// Forgets the page `session`: it closed.
    pub fn forget(&self, session: &str) {
        self.0.lock().remove(session);
    }

    /// The app's window is out of the person's sight at `now` (minimized,
    /// hidden, or behind another window): WebKit slows the timers of a page
    /// no one sees, so the silence until now doesn't count.
    pub fn hold(&self, now: Instant) {
        for beat in self.0.lock().values_mut() {
            beat.last = beat.last.max(now);
        }
    }

    /// The pages whose app page said nothing for [`STALLED`] at `now`, each
    /// once until it says something again; those `open` no longer has are
    /// forgotten.
    pub fn stalled(&self, now: Instant, open: impl Fn(&str) -> bool) -> Vec<String> {
        let mut beats = self.0.lock();
        beats.retain(|session, _| open(session));
        let mut stalled = Vec::new();
        for (session, beat) in beats.iter_mut() {
            if !beat.asked && now.saturating_duration_since(beat.last) >= STALLED {
                beat.asked = true;
                stalled.push(session.clone());
            }
        }
        stalled
    }
}

/// Asks the person about each open page whose app page stopped saying it is
/// there ([`Heartbeats`]), in a dialog of the system's own: "Close it"
/// closes the page and sends the app's page home, which ends the frame;
/// "Wait" asks again after [`STALLED`]. Only while the app's window is in
/// front of the person: there WebKit runs the page's timers on time, and
/// the person is there to answer. Nothing to do on Windows (see
/// [`Heartbeats`]). Ends with the host.
pub fn watch(app: AppHandle, host: Weak<PluginHost>) {
    if cfg!(windows) {
        return;
    }
    let watching = std::thread::Builder::new().name("plugin-pages".into()).spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let Some(host) = host.upgrade() else { return };
            // The window answers from the app's own thread, which a hung page
            // doesn't hold: WebKit runs pages in a process of their own.
            if !in_front(&app) {
                host.beats.hold(Instant::now());
                continue;
            }
            for session in host.beats.stalled(Instant::now(), |s| host.pages.get(s).is_some()) {
                let Some(m) = host.pages.get(&session) else { continue };
                if ask_hung(&app, &m.name) {
                    host.close_page(&session);
                    go_home(&app);
                } else {
                    host.beats.beat(&session, Instant::now());
                }
            }
        }
    });
    if let Err(e) = watching {
        log::warn!("plugin pages can't be watched: {e}");
    }
}

/// Whether the app's window is in front of the person: shown, not
/// minimized, and the window they are in. One that can't say counts as in
/// front.
fn in_front(app: &AppHandle) -> bool {
    let Some(window) = app.get_webview_window("main") else { return false };
    let seen = window.is_visible().unwrap_or(true) && !window.is_minimized().unwrap_or(false);
    seen && window.is_focused().unwrap_or(true)
}

/// Asks the person whether to close the page of the plugin `name` that
/// stopped responding: true for "Close it".
fn ask_hung(app: &AppHandle, name: &str) -> bool {
    let mut dialog = app
        .dialog()
        .message("Close it to use AutoScene again, or wait for it.")
        .title(format!("Plugin “{name}”'s page stopped responding."))
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom("Close it".into(), "Wait".into()));
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.parent(&window);
    }
    dialog.blocking_show()
}

/// Sends the app's page to the library, which ends a page's frame.
fn go_home(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else { return };
    let home = window.url().map(|mut url| {
        url.set_fragment(Some("/"));
        url
    });
    if let Err(e) = home.and_then(|url| window.navigate(url)) {
        log::warn!("the app's page couldn't go home from a hung plugin page: {e}");
    }
}

/// Whether the webview `label` gets answers of the asp scheme: only the
/// main one, which frames the pages.
pub fn scheme_allowed(label: &str) -> bool {
    label == "main"
}

/// The most answers of the asp scheme made at once. Each may hold a file of
/// up to 32 MB while it is read and hashed: a page that asks for much at
/// once waits for its own answers, and never fills the pool the app's
/// commands run in.
const SERVING: usize = 4;

/// The turns at making an answer ([`SERVING`]).
static TURNS: Semaphore = Semaphore::const_new(SERVING);

/// The asp scheme's handler (registered on the builder, so it may be asked
/// before the app's state is there: it says 503 then).
pub fn protocol(ctx: UriSchemeContext<'_, DynRuntime>, request: Request<Vec<u8>>, responder: UriSchemeResponder) {
    let (app, label) = (ctx.app_handle().clone(), ctx.webview_label().to_owned());
    // Never on the UI thread, which WebView2 calls the handler on.
    tauri::async_runtime::spawn(async move {
        let answer = in_turn(move || {
            let host = app.try_state::<AppState>().and_then(|state| state.plugins.clone());
            served(&label, host.as_deref(), &request)
        });
        responder.respond(response(answer.await));
    });
}

/// The answer to `request` of the webview `label`, from the open pages of
/// `host` (none while the app's state isn't there).
fn served(label: &str, host: Option<&PluginHost>, request: &Request<Vec<u8>>) -> Served {
    if !scheme_allowed(label) {
        return pages::refusal(403, &ancestors());
    }
    let Some(host) = host else { return pages::refusal(503, &ancestors()) };
    let range = request.headers().get(RANGE).and_then(|r| r.to_str().ok());
    host.pages.serve(request.method().as_str(), request.uri().path(), range)
}

/// What `answer` gives, made on the blocking pool once one of the
/// [`SERVING`] turns is free.
async fn in_turn(answer: impl FnOnce() -> Served + Send + 'static) -> Served {
    // The turns are never closed.
    let Ok(_turn) = TURNS.acquire().await else { return pages::refusal(503, &ancestors()) };
    tauri::async_runtime::spawn_blocking(answer).await.unwrap_or_else(|e| {
        log::error!("a plugin page's answer failed: {e}");
        pages::refusal(500, &ancestors())
    })
}

/// `served` as the webview takes it. A header that can't be sent fails the
/// answer, so none goes out without its CSP.
fn response(served: Served) -> Response<Vec<u8>> {
    let mut response = Response::new(served.body);
    *response.status_mut() = StatusCode::from_u16(served.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    for (name, value) in served.headers {
        let Ok(value) = HeaderValue::from_str(&value) else {
            log::warn!("a plugin page's answer has a header {name} that can't be sent");
            let mut failed = Response::new(Vec::new());
            *failed.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
            return failed;
        };
        response.headers_mut().append(name, value);
    }
    response
}

/// The origins that may frame a plugin page: the app's own page, and in a
/// debug build also Vite's, which `tauri dev` loads it from.
pub fn ancestors() -> Vec<String> {
    let mut ancestors = vec!["http://tauri.localhost".to_owned(), "tauri://localhost".to_owned()];
    if cfg!(debug_assertions) {
        ancestors.push("http://localhost:5173".into());
    }
    ancestors
}

/// The kit the app was built with: its files under `_as/`, as the app's
/// asset resolver finds them (`tauri dev` reads them from `dist/_as/`).
pub fn app_kit(app: AppHandle) -> Arc<dyn KitSource> {
    Arc::new(AssetKit(move |name: &str| {
        let asset = app.asset_resolver().get(format!("_as/{name}"))?;
        Some((asset.bytes, asset.mime_type))
    }))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
    use std::time::Duration;

    use as_api::{ContentKind, NewProject, ProjectKind};
    use as_jobs::{Class, JobRunner};
    use serde_json::{Value, json};

    use super::super::tests::{closed, host_hearing, host_in, page_only};
    use super::*;

    #[test]
    fn only_the_main_webview_gets_asp_answers() {
        assert!(scheme_allowed("main"));
        for label in ["", "Main", "main2", "main ", "plugin", "dev.example.page"] {
            assert!(!scheme_allowed(label), "{label:?}");
        }
        // A page's address, as each webview asks for it: only the main one
        // gets it, and 503 comes before the plugins are there.
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-page-label-{}", std::process::id()));
        let (lib, host) = host_in(&tmp);
        let id = loaded(&host, &tmp, "dev.a.page");
        lib.set_allows_unreviewed_plugins(true).unwrap();
        let mount = open(&host, &lib, &id, "main", None).unwrap();
        let at = mount.src.replacen(pages::base_url(), "asp://localhost", 1);
        let request = Request::get(at).body(Vec::new()).unwrap();
        assert_eq!(served("main", Some(&host), &request).status, 200);
        assert_eq!(served("main", None, &request).status, 503);
        for label in ["", "Main", "plugin"] {
            assert_eq!(served(label, Some(&host), &request).status, 403, "{label:?}");
        }
        // What the others get: nothing, with the headers every answer has.
        let refused = served("plugin", Some(&host), &request);
        assert_eq!((refused.status, refused.body.len()), (403, 0));
        assert_eq!(refused.header("x-content-type-options"), Some("nosniff"));
        assert!(refused.header("content-security-policy").is_some_and(|c| c.starts_with("default-src 'none'")));
        let sent = response(refused);
        assert_eq!(sent.status(), StatusCode::FORBIDDEN);
        assert!(sent.headers().get("content-security-policy").is_some());

        // A media element's Range reaches the file: it gets the part it asked for.
        let clip = tmp.join("clip.mp4");
        std::fs::write(&clip, [7u8; 100]).unwrap();
        let file = host.pages.resolve(&host.pages.get(&mount.session).unwrap(), clip).unwrap();
        let at = file.replacen(pages::base_url(), "asp://localhost", 1);
        let request = Request::get(at).header(RANGE, "bytes=0-9").body(Vec::new()).unwrap();
        let part = served("main", Some(&host), &request);
        assert_eq!((part.status, part.body.len(), part.header("content-range")), (206, 10, Some("bytes 0-9/100")));
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn pages_are_framed_only_by_the_apps_own_page() {
        // http://tauri.localhost on Windows, tauri://localhost on macOS and
        // Linux, and Vite's in a debug build, which `tauri dev` loads.
        let mut own = vec!["http://tauri.localhost", "tauri://localhost"];
        if cfg!(debug_assertions) {
            own.push("http://localhost:5173");
        }
        assert_eq!(ancestors(), own);
        let config: Value = serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        assert_eq!(config["build"]["devUrl"], json!("http://localhost:5173"), "Vite's origin, as tauri dev loads it");
    }

    #[test]
    fn the_scheme_makes_at_most_four_answers_at_once() {
        let (now, most) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let asked: Vec<_> = (0..3 * SERVING)
            .map(|_| {
                let (now, most) = (now.clone(), most.clone());
                tauri::async_runtime::spawn(in_turn(move || {
                    most.fetch_max(now.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
                    thread::sleep(Duration::from_millis(30));
                    now.fetch_sub(1, Ordering::SeqCst);
                    pages::refusal(404, &[])
                }))
            })
            .collect();
        for answer in asked {
            assert_eq!(tauri::async_runtime::block_on(answer).unwrap().status, 404);
        }
        assert_eq!((SERVING, now.load(Ordering::SeqCst)), (4, 0));
        assert!((1..=SERVING).contains(&most.load(Ordering::SeqCst)), "{most:?}");
    }

    /// The Rust files of the app, as paths under `src`.
    fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                sources(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }

    #[test]
    fn only_the_main_webview_is_ever_built() {
        // A second webview would have Tauri's IPC: a page that reached it
        // could call every command. So would a frame that got Tauri's
        // scripts, which a script for all frames would give it on macOS and
        // Linux (on Windows wry adds every script to every frame, so that
        // scan guards nothing there). The names are split so this file
        // doesn't hold them.
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let builders = [
            concat!("Webview", "Builder"),
            concat!("WebviewWindow", "Builder"),
            concat!("Webview", "::builder"),
            concat!("WebviewWindow", "::builder"),
            concat!("add", "_child"),
        ];
        // How the webview's and a plugin's calls that add a script to every frame end.
        let every_frame = concat!("all", "_frames");
        let mut files = Vec::new();
        sources(&root.join("src"), &mut files);
        assert!(files.len() > 10, "{files:?}");
        for file in &files {
            let text = std::fs::read_to_string(file).unwrap();
            assert!(!text.contains(every_frame), "{} adds a script to every frame", file.display());
            if file.ends_with("window.rs") {
                continue;
            }
            for builder in builders {
                assert!(!text.contains(builder), "{} builds a webview: {builder}", file.display());
            }
        }
        // window.rs builds the main window, from its config.
        let window = std::fs::read_to_string(root.join("src/window.rs")).unwrap();
        assert!(window.contains(r#"w.label == "main""#));

        let mut configs = 0;
        for entry in std::fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_str().unwrap().to_owned();
            if !(name.starts_with("tauri") && name.ends_with(".conf.json")) {
                continue;
            }
            configs += 1;
            let config: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            for window in config["app"]["windows"].as_array().map_or(&[][..], Vec::as_slice) {
                // Tauri names a window without a label "main".
                assert_eq!(window["label"].as_str().unwrap_or("main"), "main", "{name}");
            }
        }
        assert!(configs >= 2, "the configs are next to Cargo.toml");
    }

    #[test]
    fn a_page_whose_app_page_goes_quiet_is_asked_about_once() {
        let beats = Heartbeats::default();
        let start = Instant::now();
        let at = |s: u64| start + Duration::from_secs(s);
        let open = |session: &str| session != "closed";
        beats.beat("a", at(0));
        beats.beat("b", at(3));
        beats.beat("closed", at(0));

        assert!(beats.stalled(at(4), open).is_empty());
        assert_eq!(beats.stalled(at(0) + STALLED, open), ["a"]);
        // Asked once, until it says something again.
        assert!(beats.stalled(at(7), open).is_empty());
        assert_eq!(beats.stalled(at(8), open), ["b"]);
        beats.beat("a", at(9));
        assert!(beats.stalled(at(13), open).is_empty());
        assert_eq!(beats.stalled(at(14), open), ["a"]);
        // One that closed is forgotten.
        assert!(!beats.0.lock().contains_key("closed"));
        beats.forget("a");
        assert_eq!(beats.0.lock().keys().collect::<Vec<_>>(), ["b"]);
    }

    #[test]
    fn silence_while_the_window_is_out_of_sight_doesnt_count() {
        let beats = Heartbeats::default();
        let start = Instant::now();
        let at = |s: u64| start + Duration::from_secs(s);
        let open = |_: &str| true;
        beats.beat("a", at(0));
        // Minimized from 1 s to 30 s: WebKit held its timers back.
        for s in 1..=30 {
            beats.hold(at(s));
        }
        assert!(beats.stalled(at(34), open).is_empty());
        assert_eq!(beats.stalled(at(35), open), ["a"]);
        // Holding never says something for it: asked once, still.
        beats.hold(at(36));
        assert!(beats.stalled(at(45), open).is_empty());
        // Nor takes its time back.
        beats.beat("b", at(50));
        beats.hold(at(40));
        assert_eq!(beats.stalled(at(55), open), ["b"]);
    }

    #[test]
    fn a_page_that_hangs_before_its_first_heartbeat_is_asked_about() {
        // Its frame loads as the app's page starts beating for it: a page
        // whose script loops from its start hangs it before the first beat.
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-page-beat-{}", std::process::id()));
        let (lib, host) = host_in(&tmp);
        let id = loaded(&host, &tmp, "dev.a.page");
        lib.set_allows_unreviewed_plugins(true).unwrap();
        let opened = Instant::now();
        let mount = open(&host, &lib, &id, "main", None).unwrap();
        let open_now = |s: &str| host.pages.get(s).is_some();
        assert!(host.beats.stalled(opened, open_now).is_empty());
        let later = Instant::now() + STALLED;
        assert_eq!(host.beats.stalled(later, open_now), std::slice::from_ref(&mount.session));
        assert!(host.beats.stalled(later + STALLED, open_now).is_empty(), "asked once");
        // Closed, it is forgotten.
        host.close_page(&mount.session);
        assert!(host.beats.0.lock().is_empty());
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn the_sandbox_is_exactly_scripts_and_forms() {
        let frame: Value = serde_json::from_str(include_str!("../../../src/plugin-kit/frame.json")).unwrap();
        assert_eq!(frame, json!({"sandbox": pages::SANDBOX, "allow": "", "referrerPolicy": "no-referrer"}));
        assert_eq!(pages::SANDBOX, "allow-scripts allow-forms");
        assert!(!pages::SANDBOX.contains("same-origin"));
    }

    #[test]
    fn the_app_csp_frames_only_asp() {
        let config: Value = serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        for key in ["csp", "devCsp"] {
            let csp = config["app"]["security"][key].as_str().unwrap();
            let directive = |name: &str| {
                let found = csp.split(';').map(str::trim).filter(|d| d.split(' ').next() == Some(name));
                found.collect::<Vec<_>>()
            };
            assert_eq!(directive("frame-src"), ["frame-src asp: http://asp.localhost"], "{key}");
            assert_eq!(directive("child-src"), Vec::<&str>::new(), "{key}");
            // A frame that sends itself to the app's own address can't load the app in it.
            assert_eq!(directive("frame-ancestors"), ["frame-ancestors 'none'"], "{key}");
        }
    }

    /// A page-only plugin `id` loaded from its folder in `tmp`, with a
    /// global page `main` and a project page `clips`.
    fn loaded(host: &PluginHost, tmp: &Path, id: &str) -> PluginId {
        let dir = tmp.join(id);
        std::fs::create_dir_all(dir.join("ui")).unwrap();
        for page in ["main", "clips"] {
            std::fs::write(dir.join(format!("ui/{page}.html")), b"<!doctype html>").unwrap();
        }
        let pages = json!([
            {"id": "main", "title": "Main", "entry": "ui/main.html", "rail": true},
            {"id": "clips", "title": "Clips", "entry": "ui/clips.html", "scope": "project"}
        ]);
        std::fs::write(dir.join("plugin.json"), page_only(id, pages).to_string()).unwrap();
        host.store.commit(host.store.stage_folder(&dir).unwrap(), true, true).unwrap().record.id
    }

    #[test]
    fn a_page_opens_where_it_belongs_while_its_plugin_may_run() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-pages-{}", std::process::id()));
        let (lib, host, heard) = host_hearing(&tmp);
        let id = loaded(&host, &tmp, "dev.a.page");
        let said = |r: AppResult<PageMount>| r.map(|m| m.page.id).unwrap_or_else(|e| e.message);
        let unreviewed = PluginError::NotAllowed.to_string();
        assert_eq!(said(open(&host, &lib, &id, "main", None)), unreviewed);
        lib.set_allows_unreviewed_plugins(true).unwrap();

        let mount = open(&host, &lib, &id, "main", None).unwrap();
        assert_eq!(mount.src, format!("{}/{}/ui/main.html", pages::base_url(), mount.session));
        assert_eq!((mount.plugin.trust, mount.plugin.source), (Trust::Unreviewed, PluginSource::Folder));
        assert_eq!((mount.project_id.as_deref(), mount.protocol, mount.service), (None, 2, false));
        assert!(host.pages.get(&mount.session).is_some());
        let shown = serde_json::to_value(&mount).unwrap();
        assert_eq!(shown["plugin"]["name"], json!("Page dev.a.page"));
        assert_eq!(shown["page"]["rail"], json!(true));

        // A project page opens in a project that opens, a global one in none.
        let footage = tmp.join("footage").join("Pilot.mkv");
        std::fs::create_dir_all(footage.parent().unwrap()).unwrap();
        std::fs::write(&footage, b"x").unwrap();
        let files = vec![footage.display().to_string()];
        let req = NewProject { title: "Show".into(), kind: ProjectKind::Series, content: ContentKind::Live, files };
        let (summary, project, _) = lib.create_project(&req).unwrap();
        assert_eq!(said(open(&host, &lib, &id, "clips", Some(summary.id))), "clips");
        assert_eq!(said(open(&host, &lib, &id, "clips", None)), "Open a project to see this page.");
        assert_eq!(said(open(&host, &lib, &id, "main", Some(summary.id))), "This page isn't a project's.");
        let gone = open(&host, &lib, &id, "clips", Some(ProjectId::new())).unwrap_err();
        assert_eq!(gone.code, ErrorCode::NotFound);
        assert_eq!(said(open(&host, &lib, &id, "other", None)), "Page dev.a.page has no such page.");

        // Past the most open at once, the one opened first closes, and hears why.
        for _ in host.pages.open_count(&id)..pages::MAX_SESSIONS {
            open(&host, &lib, &id, "main", None).unwrap();
        }
        assert!(closed(&heard).is_empty());
        open(&host, &lib, &id, "main", None).unwrap();
        assert_eq!(closed(&heard), [(mount.session.clone(), "Too many plugin pages were open.".to_owned())]);
        assert!(host.pages.get(&mount.session).is_none());
        // The app's page loads again: its frames went, and their pages close.
        host.close_all_pages();
        assert_eq!(host.pages.open_count(&id), 0);
        assert_eq!(closed(&heard).len(), 1 + pages::MAX_SESSIONS);

        // Not while it changes; turned off, not at all.
        let changing = host.change(&id, "Page", &Msg::plain("It is being updated.")).unwrap();
        let busy = open(&host, &lib, &id, "main", None).unwrap_err();
        assert_eq!((busy.code, busy.message.as_str()), (ErrorCode::NotReady, "Page dev.a.page is being updated."));
        drop(changing);
        host.store.set_enabled(&id, false).unwrap();
        assert_eq!(said(open(&host, &lib, &id, "main", None)), "Page dev.a.page is turned off.");
        drop((host, project, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_page_opening_as_its_plugin_goes_off_is_refused_or_closed() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-page-race-{}", std::process::id()));
        let (lib, host, heard) = host_hearing(&tmp);
        let id = loaded(&host, &tmp, "dev.a.page");
        lib.set_allows_unreviewed_plugins(true).unwrap();
        let jobs = JobRunner::new(&[(Class::Plugin, 1)], |_| {});
        let wait = Duration::from_millis(200);

        // An open that took the lock first and found the plugin on, as it
        // or the setting goes off: its page opens, and is closed after.
        let opened_meanwhile = |going_off: &(dyn Fn() + Sync)| {
            let p = host.store.get(&id).unwrap();
            let page = p.manifest.as_ref().unwrap().pages[0].clone();
            let opening = host.busy.lock();
            thread::scope(|s| {
                let off = s.spawn(going_off);
                thread::sleep(wait);
                let (mount, _) = host.pages.open(&p, Grants::of(&p).unwrap(), &page, None).unwrap();
                drop(opening);
                off.join().unwrap();
                mount.token.clone()
            })
        };
        let session = opened_meanwhile(&|| {
            host.store.set_enabled(&id, false).unwrap();
            host.stop(&jobs, &id);
        });
        assert!(host.pages.get(&session).is_none());
        assert_eq!(closed(&heard), [(session, "It was turned off.".to_owned())]);
        host.store.set_enabled(&id, true).unwrap();
        heard.lock().clear();
        let session = opened_meanwhile(&|| {
            lib.set_allows_unreviewed_plugins(false).unwrap();
            host.allow_changed(&jobs, true, false);
        });
        assert!(host.pages.get(&session).is_none());
        assert_eq!(closed(&heard), [(session, PluginError::NotAllowed.to_string())]);

        // An open that waits for the lock as the setting goes off reads the
        // setting after it, and is refused.
        lib.set_allows_unreviewed_plugins(true).unwrap();
        let held = host.busy.lock();
        thread::scope(|s| {
            let opening = s.spawn(|| open(&host, &lib, &id, "main", None));
            thread::sleep(wait);
            lib.set_allows_unreviewed_plugins(false).unwrap();
            drop(held);
            host.allow_changed(&jobs, true, false);
            let refused = opening.join().unwrap().unwrap_err();
            assert_eq!(refused.message, PluginError::NotAllowed.to_string());
        });
        assert_eq!(host.pages.open_count(&id), 0);
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}

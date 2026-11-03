//! The red team: a small Tauri app that frames plugin pages as AutoScene
//! does, and tries every known way out of one on the webview of the system
//! it runs on (WebView2, WKWebView or WebKitGTK). `cargo run -p
//! autoscene-redteam` opens a window for about a minute, writes
//! `result.json` and `summary.md` beside this crate's `Cargo.toml`, and
//! exits with 0 when every wall held, 1 when one fell or the run didn't
//! finish, and 2 when it couldn't start.
//!
//! It serves pages as the app does: with the plugin host's own [`Pages`],
//! through an `asp` handler of the app's shape, inside the app's CSP, in
//! frames with `frame.json`'s attributes (`tests/config.rs` keeps the CSP
//! and the attributes the app's). It builds its own window, as nothing in
//! the app may. Two page-only plugins are installed into a scratch folder
//! as packages: an attacker, whose files under `fixture/attacker/ui/` each
//! try one way out, and a victim, whose session the attacker aims at.
//!
//! The harness (`harness/`) plays AutoScene's side of the bridge, mounts
//! one page after another, and judges each check from what the page
//! reports and from what reached this side:
//! - a call of `redteam_pwned`: a page reached Tauri's IPC, which fails
//!   the run;
//! - a request at the wall listener: a page got something out past the
//!   CSP or the sandbox, which fails the run;
//! - what the side listeners hear, and a connection to the wall that sent
//!   nothing: what the CSP doesn't cover (link hints, WebRTC, a socket the
//!   webview opens before the CSP stops a request), measured per engine
//!   (B8.2 row 9), never a failure.
//!
//! The kit comes from `app/dist/_as` (`npm run build:kit`), or from the
//! folder `AUTOSCENE_KIT_DIR` names.

mod headers;
mod listen;
mod outcome;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use std::{env, fs, thread};

use as_plugin::pages::{self, KIT_FILES, MemoryKit, Pages};
use as_plugin::store::RECORD_VERSION;
use as_plugin::{Extra, Grants, Installed, Record, RuntimeKind, Source, Trust, manifest, package};
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{Value, json};
use tauri::http::header::RANGE;
use tauri::http::{HeaderValue, Request, Response, StatusCode};
use tauri::utils::config::Color;
use tauri::{AppHandle, RunEvent, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use listen::{Ear, Heard};

/// The origins that may frame a page: AutoScene's own page, as in a built
/// app (the app's `plugins::pages::ancestors` adds Vite's in a debug
/// build, which the red team has none of).
const ANCESTORS: [&str; 2] = ["http://tauri.localhost", "tauri://localhost"];

/// The frame's attributes, as the app's frame takes them.
const FRAME: &str = include_str!("../../src/plugin-kit/frame.json");

/// AutoScene's tokens, for the canvas a frame must leave showing (check 15).
const TOKENS: &str = include_str!("../../src/styles/tokens.css");

/// How long a run may take before it counts as stuck.
const DEADLINE: Duration = Duration::from_secs(240);

/// How long the window may take to come back once it is sent home while a
/// page is busy (check 14).
const COMING_BACK: Duration = Duration::from_secs(20);

/// One run: the pages, the two plugins, the listeners, and what reached
/// this side while each check ran.
struct Run {
    pages: Pages,
    attacker: Installed,
    victim: Installed,
    /// The folder the plugins are installed into, emptied at each start.
    scratch: PathBuf,
    heard: Arc<Heard>,
    /// The listeners' addresses: the wall's and the side one's (HTTP), and
    /// the UDP one's port.
    wall: String,
    side: String,
    udp: u16,
    /// The calls that reached Tauri from a page: the check, and what it said.
    pwned: Mutex<Vec<(String, Value)>>,
    /// The harness's own raw IPC posts that reached Tauri: the control of
    /// check 3, which shows that such a post is heard from the main frame.
    controls: AtomicUsize,
    /// How many answers of each status the scheme gave.
    served: Mutex<HashMap<u16, usize>>,
    checks: Mutex<Vec<Value>>,
    /// What the harness said about the run as a whole.
    report: Mutex<Value>,
    /// When the window was sent home (check 14), and how long it took to
    /// come back.
    sent_home: Mutex<Option<Instant>>,
    came_back: Mutex<Option<u64>>,
    /// The exit code, once the run is over.
    code: Mutex<Option<i32>>,
    started: Instant,
}

impl Run {
    fn new(started: Instant) -> Result<Self, String> {
        let kit = kit()?;
        // Beside the program, under a name it can't have: on macOS and
        // Linux the program itself is `autoscene-redteam`, without `.exe`.
        let scratch = env::current_exe()
            .ok()
            .and_then(|exe| Some(exe.parent()?.join("autoscene-redteam-data")))
            .ok_or("The red team can't tell where it runs from.")?;
        let plugins = scratch.join("plugins");
        if plugins.exists() {
            fs::remove_dir_all(&plugins).map_err(|e| format!("{}: {e}", plugins.display()))?;
        }
        let attacker = install(&plugins, "attacker")?;
        // A file in its folder that its lock doesn't list: never served (check 16).
        fs::write(attacker.dir.join("ui").join("unlisted.html"), "<!doctype html><p>unlisted</p>")
            .map_err(|e| e.to_string())?;
        let victim = install(&plugins, "victim")?;
        let heard = Arc::new(Heard::new(started));
        let listening = |e: std::io::Error| format!("A listener couldn't start: {e}");
        Ok(Self {
            pages: Pages::new(Arc::new(kit), ANCESTORS.map(String::from).to_vec()),
            attacker,
            victim,
            wall: listen::http(heard.clone(), Ear::Wall).map_err(listening)?,
            side: listen::http(heard.clone(), Ear::Side).map_err(listening)?,
            udp: listen::udp(heard.clone()).map_err(listening)?,
            heard,
            scratch: plugins,
            pwned: Mutex::default(),
            controls: AtomicUsize::new(0),
            served: Mutex::default(),
            checks: Mutex::default(),
            report: Mutex::new(json!({})),
            sent_home: Mutex::default(),
            came_back: Mutex::default(),
            code: Mutex::default(),
            started,
        })
    }

    /// The asp scheme's answer to `request` of the webview `label`, as the
    /// app's handler gives it (`plugins::pages::served` and `response`):
    /// only the main webview is answered, and an answer whose header can't
    /// be sent goes out as a 500 without it.
    fn answer(&self, label: &str, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
        let served = if label == "main" {
            let range = request.headers().get(RANGE).and_then(|r| r.to_str().ok());
            self.pages.serve(request.method().as_str(), request.uri().path(), range)
        } else {
            pages::refusal(403, &ANCESTORS.map(String::from))
        };
        *self.served.lock().entry(served.status).or_default() += 1;
        let mut response = Response::new(served.body);
        *response.status_mut() = StatusCode::from_u16(served.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        for (name, value) in served.headers {
            let Ok(value) = HeaderValue::from_str(&value) else {
                let mut failed = Response::new(Vec::new());
                *failed.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                return failed;
            };
            response.headers_mut().append(name, value);
        }
        response
    }

    /// Opens the page of the plugin `who` ("attacker" or "victim").
    fn mount(&self, who: &str) -> Result<Mounted, String> {
        let p = match who {
            "attacker" => &self.attacker,
            "victim" => &self.victim,
            _ => return Err(format!("There is no plugin {who}.")),
        };
        let m = p.manifest.as_ref().ok_or("The plugin has no manifest.")?;
        let page = m.page("main").ok_or("The plugin has no page.")?;
        let grants = Grants::of(p).map_err(|e| e.to_string())?;
        let (mount, _) = self.pages.open(p, grants, page, None).map_err(|e| e.to_string())?;
        Ok(Mounted {
            root: format!("{}/{}/", pages::base_url(), mount.token),
            src: mount.src(),
            session: mount.token.clone(),
            plugin: json!({ "id": mount.plugin, "name": mount.name, "version": mount.version }),
            page: json!({ "id": mount.page.id, "title": mount.page.title, "scope": mount.page.scope }),
        })
    }
}

/// An open page, for the harness: where its frame loads from, and what its
/// init says.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Mounted {
    session: String,
    /// `B/<session>/`: the session's files are under it.
    root: String,
    /// Its entry's address.
    src: String,
    plugin: Value,
    page: Value,
}

/// What the harness starts from.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Setup {
    platform: &'static str,
    version: String,
    /// `frame.json`: the frame's attributes.
    frame: Value,
    /// `--bg-canvas`, which the harness paints its page with.
    canvas: String,
    wall: String,
    side: String,
    udp: u16,
    /// The victim's page, open for the whole run.
    victim: Mounted,
    /// Where check 6's frame sends itself.
    targets: Vec<String>,
}

/// The kit, read from `AUTOSCENE_KIT_DIR` or `app/dist/_as`.
fn kit() -> Result<MemoryKit, String> {
    let dir = env::var_os("AUTOSCENE_KIT_DIR")
        .map_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../dist/_as"), PathBuf::from);
    let mut kit = MemoryKit::default();
    for name in KIT_FILES {
        let bytes = fs::read(dir.join(name)).map_err(|e| {
            format!("The kit's {name} isn't in {} ({e}). Build it first: cd app && npm run build:kit", dir.display())
        })?;
        kit.0.insert(name.to_owned(), bytes);
    }
    Ok(kit)
}

/// The fixture plugin `name` (`fixture/<name>`), installed into `into` as a
/// package leaves it: its files and a lock that lists each of them.
fn install(into: &Path, name: &str) -> Result<Installed, String> {
    let failed = |e: &dyn std::fmt::Display| format!("Installing the {name}: {e}");
    let dir = into.join(name);
    copy_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("fixture").join(name), &dir).map_err(|e| failed(&e))?;
    let lock = package::lock_folder(&dir).map_err(|e| failed(&e))?;
    let lock = serde_json::to_vec_pretty(&lock).map_err(|e| failed(&e))?;
    fs::write(dir.join(package::LOCK), &lock).map_err(|e| failed(&e))?;
    let bytes = fs::read(dir.join(package::MANIFEST)).map_err(|e| failed(&e))?;
    let m = manifest::parse_for_any_app(&bytes).map_err(|e| failed(&e))?.manifest;
    Ok(Installed {
        record: Record {
            v: RECORD_VERSION,
            id: m.id.clone(),
            version: m.version.clone(),
            source: Source::Package { folder: format!("{}+00000000", m.version) },
            trust: Trust::Unreviewed,
            author_key: None,
            permissions: Vec::new(),
            network_hosts: Vec::new(),
            enabled: true,
            after_export: Vec::new(),
            lock_sha256: Some(package::sha256_hex(&lock)),
            installed_at: 0,
        },
        manifest: Some(m),
        warnings: Vec::new(),
        dir,
        broken: None,
        extra: Extra::agreed(RuntimeKind::None),
    })
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let path = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &path)?;
        } else {
            fs::copy(entry.path(), path)?;
        }
    }
    Ok(())
}

/// `--bg-canvas` in AutoScene's tokens.
fn canvas() -> String {
    let line = TOKENS.lines().find_map(|l| l.trim().strip_prefix("--bg-canvas:"));
    line.unwrap_or("#09090b").trim().trim_end_matches(';').trim().to_owned()
}

fn platform() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

/// A page's call that reached Tauri: it got through a wall.
#[tauri::command]
fn redteam_pwned(from: Option<Value>, run: State<'_, Arc<Run>>) -> String {
    eprintln!("A plugin page reached Tauri: {from:?}");
    run.pwned.lock().push((run.heard.check(), from.unwrap_or(Value::Null)));
    "pwned".into()
}

/// The harness's own raw IPC post, as the control of check 3.
#[tauri::command]
fn redteam_control(run: State<'_, Arc<Run>>) {
    run.controls.fetch_add(1, Ordering::Relaxed);
}

#[tauri::command]
fn redteam_setup(app: AppHandle, run: State<'_, Arc<Run>>) -> Result<Setup, String> {
    let base = pages::base_url();
    Ok(Setup {
        platform: platform(),
        version: app.package_info().version.to_string(),
        frame: serde_json::from_str(FRAME).map_err(|e| e.to_string())?,
        canvas: canvas(),
        wall: run.wall.clone(),
        side: run.side.clone(),
        udp: run.udp,
        victim: run.mount("victim")?,
        targets: vec![
            "http://tauri.localhost/".into(),
            "tauri://localhost/".into(),
            format!("{}/navigate", run.wall),
            format!("{base}/_as/sdk.js"),
        ],
    })
}

/// The check `check` starts.
#[tauri::command]
fn redteam_begin(check: String, run: State<'_, Arc<Run>>) {
    run.heard.begin(&check);
}

#[tauri::command]
fn redteam_mount(plugin: String, run: State<'_, Arc<Run>>) -> Result<Mounted, String> {
    run.mount(&plugin)
}

#[tauri::command]
fn redteam_unmount(session: String, run: State<'_, Arc<Run>>) {
    run.pages.close(&session);
}

/// What reached this side while the check `check` ran, and how many of the
/// harness's own raw posts reached Tauri.
#[tauri::command]
fn redteam_seen(check: String, run: State<'_, Arc<Run>>) -> Value {
    let pwned: Vec<Value> =
        run.pwned.lock().iter().filter(|(c, _)| *c == check).map(|(_, from)| from.clone()).collect();
    json!({
        "pwned": pwned,
        "wall": run.heard.during(Ear::Wall, &check),
        "opened": run.heard.during(Ear::Opened, &check),
        "side": run.heard.during(Ear::Side, &check),
        "udp": run.heard.during(Ear::Udp, &check),
        "controls": run.controls.load(Ordering::Relaxed),
    })
}

/// Check 16, from this side.
#[tauri::command]
async fn redteam_headers(run: State<'_, Arc<Run>>) -> Result<Value, String> {
    Ok(headers::check(&run))
}

/// The colours of the window's pixels at `points` (physical pixels from its
/// page's top left), each as `#rrggbb`: on Windows, where check 15 reads
/// them.
#[tauri::command]
async fn redteam_pixels(window: WebviewWindow, points: Vec<(i32, i32)>) -> Result<Option<Vec<String>>, String> {
    #[cfg(windows)]
    let colours = capture::pixels(window.hwnd().map_err(|e| e.to_string())?.0, &points).map(Some);
    #[cfg(not(windows))]
    let colours = {
        let _ = (window, points);
        Ok(None)
    };
    colours
}

/// The window's own pixels, as it draws them: wherever it is, and whatever
/// covers it.
#[cfg(windows)]
mod capture {
    use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        CLR_INVALID, ClientToScreen, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetPixel, HDC, ReleaseDC, SelectObject,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;
    use windows_sys::core::BOOL;

    /// PrintWindow's flag that takes in what DirectComposition draws, as a
    /// webview does.
    const PW_RENDERFULLCONTENT: u32 = 2;

    // windows-sys has PrintWindow only under a feature the app doesn't
    // build (Win32_Storage_Xps), so it is declared here.
    #[link(name = "user32")]
    unsafe extern "system" {
        fn PrintWindow(hwnd: HWND, hdc: HDC, flags: u32) -> BOOL;
    }

    /// The colours at `points` (physical pixels from the client area's top
    /// left) of the window `hwnd`, drawn into a bitmap of its own.
    pub fn pixels(hwnd: HWND, points: &[(i32, i32)]) -> Result<Vec<String>, String> {
        let mut frame = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        let mut client = POINT { x: 0, y: 0 };
        // SAFETY: `hwnd` is the main window's, which is open while its page
        // runs a command; every handle made here is given back before the end.
        let colours: Vec<u32> = unsafe {
            if GetWindowRect(hwnd, &mut frame) == 0 || ClientToScreen(hwnd, &mut client) == 0 {
                return Err("The window's place couldn't be read.".into());
            }
            let (width, height) = (frame.right - frame.left, frame.bottom - frame.top);
            let window = GetDC(hwnd);
            let dc = CreateCompatibleDC(window);
            let bitmap = CreateCompatibleBitmap(window, width, height);
            let old = SelectObject(dc, bitmap);
            let drawn = PrintWindow(hwnd, dc, PW_RENDERFULLCONTENT) != 0;
            let (dx, dy) = (client.x - frame.left, client.y - frame.top);
            let colours = points.iter().map(|&(x, y)| if drawn { GetPixel(dc, dx + x, dy + y) } else { CLR_INVALID });
            let colours = colours.collect();
            SelectObject(dc, old);
            DeleteObject(bitmap);
            DeleteDC(dc);
            ReleaseDC(hwnd, window);
            colours
        };
        if colours.contains(&CLR_INVALID) {
            return Err("The window's pixels couldn't be read.".into());
        }
        // A COLORREF is 0x00bbggrr.
        Ok(colours
            .into_iter()
            .map(|c| format!("#{:02x}{:02x}{:02x}", c & 0xff, (c >> 8) & 0xff, (c >> 16) & 0xff))
            .collect())
    }
}

/// A check's outcome, as the harness judged it.
#[tauri::command]
fn redteam_record(check: Value, run: State<'_, Arc<Run>>) {
    run.checks.lock().push(check);
}

/// The harness is done: `report` says what it found about the run as a
/// whole. With `recover`, a busy page is open, and the window is sent home
/// while it runs (check 14): the run ends once the harness is back, or
/// after [`COMING_BACK`].
#[tauri::command]
fn redteam_done(report: Value, app: AppHandle, window: WebviewWindow, run: State<'_, Arc<Run>>) {
    let recover = report["recover"] == json!(true);
    *run.report.lock() = report;
    let run = run.inner().clone();
    thread::spawn(move || {
        if recover {
            thread::sleep(Duration::from_millis(800));
            let home = window.url().ok().map(|mut url| {
                url.set_query(Some("recovered"));
                url
            });
            *run.sent_home.lock() = Some(Instant::now());
            if let Some(home) = home
                && let Err(e) = window.navigate(home)
            {
                eprintln!("The window couldn't be sent home: {e}");
            }
            let back = Instant::now() + COMING_BACK;
            while run.came_back.lock().is_none() && Instant::now() < back {
                thread::sleep(Duration::from_millis(50));
            }
        }
        app.exit(outcome::finish(&run, None));
    });
}

/// The harness is back after the window was sent home.
#[tauri::command]
fn redteam_recovered(run: State<'_, Arc<Run>>) {
    let sent = *run.sent_home.lock();
    *run.came_back.lock() = sent.map(|at| at.elapsed().as_millis() as u64);
}

fn main() {
    let started = Instant::now();
    let run = match Run::new(started) {
        Ok(run) => Arc::new(run),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let stuck = run.clone();
    thread::spawn(move || {
        thread::sleep(DEADLINE);
        let code = outcome::finish(&stuck, Some(format!("The run didn't finish in {} s.", DEADLINE.as_secs())));
        std::process::exit(code);
    });
    let served = run.clone();
    let webview_data = run.scratch.with_file_name("webview");
    tauri::Builder::default()
        .runtime(tauri_runtime_wry::Wry::default())
        .manage(run.clone())
        // As the app registers it: on the builder, answered off the UI thread.
        .register_asynchronous_uri_scheme_protocol("asp", move |ctx, request, responder| {
            let (run, label) = (served.clone(), ctx.webview_label().to_owned());
            tauri::async_runtime::spawn_blocking(move || responder.respond(run.answer(&label, &request)));
        })
        .invoke_handler(tauri::generate_handler![
            redteam_pwned,
            redteam_control,
            redteam_setup,
            redteam_begin,
            redteam_mount,
            redteam_unmount,
            redteam_seen,
            redteam_headers,
            redteam_pixels,
            redteam_record,
            redteam_done,
            redteam_recovered,
        ])
        .setup(move |app| {
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("AutoScene red team")
                .inner_size(1120.0, 760.0)
                .center()
                .background_color(Color(9, 9, 11, 255))
                .data_directory(webview_data)
                .build()?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("the red team couldn't start")
        .run(move |_, event| {
            // The window closed before the harness was done.
            if let RunEvent::Exit = event
                && run.code.lock().is_none()
            {
                std::process::exit(outcome::finish(&run, Some("The window closed before the run ended.".into())));
            }
        });
}

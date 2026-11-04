//! WebAssembly plugins in their sandbox, with the test guest
//! (`tests/guest`, built by `cargo xtask wasm-guests`): what reaches it,
//! what it can reach, how a run ends when it misbehaves, and which
//! components are refused. Each run works in folders of its own.
//!
//! A test skips when the guest wasn't built, unless
//! `AUTOSCENE_REQUIRE_FIXTURES` is set (CI sets it).

#![cfg(feature = "wasm")]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use as_core::CancelToken;
use as_plugin::log::{MAX_LOG_BYTES, RESERVE};
use as_plugin::manifest::{RuntimeKind, WasmLimits, parse_for_any_app};
use as_plugin::package::sha256_hex;
use as_plugin::page_state;
use as_plugin::protocol::{ClipContext, ExportContext, ProjectContext};
use as_plugin::runtime::MAX_LINE_BYTES;
use as_plugin::scratch::{self, Scratch};
use as_plugin::wasm::{self, CANCEL_GRACE, Grants, Limits, Net, PROGRESS_GAP, WasmSpec};
use as_plugin::{
    ActionContext, ActionKind, AppContext, Manifest, Permission, PluginContext, PluginError, PluginId, PluginResult,
    RunMessage, SettingValue, ToolPaths, Trigger,
};
use serde_json::json;
use tempfile::TempDir;

const NAME: &str = "Test Guest";
const ID: &str = "dev.autoscene.test-guest";
/// A secret's value, which must show up nowhere but where the plugin asks.
const MARKER: &str = "s3cret-marker-7f1c";

/// A run of the test guest's `action`, in folders of its own:
/// - `data/`, its `/data`;
/// - `export/`, an export's folder with two clips and `remove-me.txt`;
/// - `project/`, the project's folder with `notes.txt`;
/// - `outside/secret.txt`, which it must never reach;
/// - `code/` and `logs/`, its compiled code and its log;
/// - `resources/`, its large files, once it has some ([`Guest::files`]).
struct Guest {
    dir: TempDir,
    component: PathBuf,
    sha256: Option<String>,
    /// Its large files' folder, its `/resources`; none when its plugin
    /// names none.
    resources: Option<PathBuf>,
    permissions: Vec<Permission>,
    secret_keys: Vec<String>,
    secrets: BTreeMap<String, String>,
    limits: Limits,
    message: RunMessage,
}

/// The test guest running `action`; none (the test skips) when it wasn't built.
fn guest(action: &str) -> Option<Guest> {
    guest_from("wasm/guest.wasm", action)
}

fn guest_from(fixture: &str, action: &str) -> Option<Guest> {
    let component = as_testkit::fixture_file(fixture)?;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for folder in ["data", "export", "project", "outside"] {
        fs::create_dir(root.join(folder)).unwrap();
    }
    fs::write(root.join("export/Clip 1.mp4"), "clip 1").unwrap();
    fs::write(root.join("export/Clip 2.mp4"), "clip 2").unwrap();
    fs::write(root.join("export/remove-me.txt"), "keep me").unwrap();
    fs::write(root.join("project/notes.txt"), "notes").unwrap();
    fs::write(root.join("outside/secret.txt"), "secret").unwrap();
    let message = message(root, action);
    Some(Guest {
        dir,
        component,
        sha256: None,
        resources: None,
        permissions: Vec::new(),
        secret_keys: vec!["token".into()],
        secrets: BTreeMap::new(),
        limits: Limits::of(WasmLimits::default()),
        message,
    })
}

fn message(root: &Path, action: &str) -> RunMessage {
    let host = |p: PathBuf| p.to_string_lossy().into_owned();
    let clip = |name: &str, path: Option<String>| ClipContext {
        name: name.into(),
        path,
        status: "done".into(),
        media_id: "m1".into(),
        source_path: host(root.join("episodes/S01E01.mkv")),
        episode: "S01E01".into(),
        start: 240,
        end: 480,
        fps: Some("24000/1001".into()),
    };
    RunMessage {
        event: None,
        protocol: 1,
        run_id: "run-1".into(),
        plugin: PluginContext {
            id: PluginId::parse(ID).unwrap(),
            version: "1.2.0".into(),
            data_dir: "?".into(),
            resources_dir: None,
            temp_dir: None,
        },
        app: AppContext { version: "1.1.0".into(), platform: "windows-x86_64".into() },
        tools: ToolPaths { ffmpeg: "ffmpeg".into(), ffprobe: "ffprobe".into(), ffmpeg_version: "9.0".into() },
        settings: BTreeMap::from([
            ("greeting".into(), SettingValue::Text("Hi".into())),
            ("alpha".into(), SettingValue::Number(2.0)),
            // A value under a secret's key never reaches the plugin as a setting.
            ("token".into(), SettingValue::Text(MARKER.into())),
        ]),
        action: ActionContext { kind: ActionKind::ExportAction, id: action.into(), trigger: Trigger::Menu },
        project: Some(ProjectContext { id: "p1".into(), title: "Breaking".into(), folder: host(root.join("project")) }),
        export: Some(ExportContext {
            status: None,
            id: "e1".into(),
            label: Some("Cut".into()),
            preset: "h264".into(),
            out_dir: host(root.join("export")),
            created_at: 1_700_000_000_000,
            merged_file: None,
            clips: vec![
                clip("Clip 1", Some(host(root.join("export/Clip 1.mp4")))),
                clip("Clip 2", Some(host(root.join("export/Clip 2.mp4")))),
                clip("Elsewhere", Some(host(root.join("outside/x.mp4")))),
            ],
        }),
        input: None,
    }
}

/// What a run reported, in order.
type Reports = Vec<(String, Option<f64>)>;

impl Guest {
    fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    fn with(mut self, permissions: &[Permission]) -> Self {
        self.permissions = permissions.to_vec();
        self
    }

    /// With the large files `files`, by name, in `resources/`.
    fn files(mut self, files: &[(&str, &[u8])]) -> Self {
        let dir = self.path("resources");
        fs::create_dir(&dir).unwrap();
        for (name, bytes) in files {
            fs::write(dir.join(name), bytes).unwrap();
        }
        self.resources = Some(dir);
        self
    }

    fn text(mut self, key: &str, value: &str) -> Self {
        self.message.settings.insert(key.into(), SettingValue::Text(value.into()));
        self
    }

    fn spec(&self) -> WasmSpec {
        WasmSpec {
            name: NAME.into(),
            component: self.component.clone(),
            component_sha256: self.sha256.clone(),
            cache: self.path("code"),
            data_dir: self.path("data"),
            resources_dir: self.resources.clone(),
            log_path: self.path("logs/plugin.log"),
            grants: Grants {
                runtime: RuntimeKind::Wasm,
                permissions: self.permissions.clone(),
                hosts: Vec::new(),
                secrets: self.secret_keys.clone(),
                protocol: 2,
            },
            limits: self.limits,
            secrets: self.secrets.clone(),
            message: self.message.clone(),
        }
    }

    fn run(&self) -> (PluginResult<Option<String>>, Reports) {
        self.run_with(&CancelToken::new())
    }

    fn run_with(&self, cancel: &CancelToken) -> (PluginResult<Option<String>>, Reports) {
        let reports = Mutex::new(Vec::new());
        let r =
            wasm::run(self.spec(), cancel, &|stage, fraction| reports.lock().unwrap().push((stage.into(), fraction)));
        (r, reports.into_inner().unwrap())
    }

    /// Compiles its component into its code folder first, so a run's time
    /// is its own.
    fn compile(&self) {
        let sha256 = sha256_hex(&fs::read(&self.component).unwrap());
        wasm::prepare(&self.component, Some(&sha256), &self.path("code"), &wasm_manifest(false)).unwrap();
    }

    /// Runs it, cancelled `after` it started: the result, and how long the
    /// run took from the cancel on. Compiled first, so Stop comes while it
    /// runs.
    fn run_cancelled(&self, after: Duration) -> (PluginResult<Option<String>>, Duration) {
        self.compile();
        let cancel = CancelToken::new();
        thread::scope(|s| {
            let run = s.spawn(|| self.run_with(&cancel).0);
            thread::sleep(after);
            let asked = Instant::now();
            cancel.cancel();
            (run.join().unwrap(), asked.elapsed())
        })
    }

    fn log(&self) -> String {
        fs::read_to_string(self.path("logs/plugin.log")).unwrap()
    }

    /// What the run returned, whole, as its log has it (the message the
    /// person sees is cut to one short line).
    fn said(&self) -> String {
        let log = self.log();
        let mut lines = log.lines().skip_while(|l| !l.starts_with("[result] "));
        let first = lines.next().expect("a result in the log").trim_start_matches("[result] ").to_owned();
        let rest = lines.map_while(|l| l.strip_prefix("  ")).map(str::to_owned);
        std::iter::once(first).chain(rest).collect::<Vec<_>>().join("\n")
    }

    /// Runs its action and gives what it returned, whole.
    fn runs(&self) -> String {
        let (r, _) = self.run();
        assert!(matches!(r, Ok(Some(_))), "{r:?}");
        self.said()
    }
}

fn wasm_manifest(network: bool) -> Manifest {
    let mut m = json!({
        "manifestVersion": 2, "id": ID, "version": "1.2.0", "name": NAME,
        "description": "Tries the sandbox.", "author": {"name": "AutoScene"},
        "engines": {"autoscene": ">=1.1.0", "protocol": 2},
        "runtime": {"type": "wasm", "component": "plugin.wasm"},
        "contributes": {"commands": [{"id": "hello", "title": "Hello"}]}
    });
    if network {
        m["permissions"] = json!(["network"]);
        m["network"] = json!({"hosts": ["discord.com"]});
    }
    parse_for_any_app(m.to_string().as_bytes()).unwrap().manifest
}

/// `wat` as a component's bytes.
fn component(wat: &str) -> Vec<u8> {
    wat::parse_str(wat).unwrap()
}

fn line<'a>(text: &'a str, start: &str) -> &'a str {
    text.lines().find(|l| l.starts_with(start)).unwrap_or_else(|| panic!("no line {start:?} in:\n{text}"))
}

fn denied(text: &str, start: &str) {
    let l = line(text, start);
    assert!(l.contains(": denied ("), "{l}");
}

fn allowed(text: &str, start: &str) {
    let l = line(text, start);
    assert!(l.ends_with(": ALLOWED"), "{l}");
}

#[test]
fn a_run_reports_progress_logs_and_its_message() {
    let Some(g) = guest("hello") else { return };
    let (r, reports) = g.run();
    let message = r.unwrap().unwrap();
    assert!(message.starts_with("hello dev.autoscene.test-guest env="), "{message}");
    assert_eq!(reports.first().unwrap(), &(format!("Getting {NAME} ready"), None));
    assert_eq!(reports.last().unwrap(), &(NAME.to_owned(), Some(1.0)));
    for report in &reports[1..] {
        assert!([("Writing".to_owned(), Some(0.5)), (NAME.to_owned(), Some(1.0))].contains(report), "{reports:?}");
    }
    let log = g.log();
    for logged in
        ["[info] hello from the guest", "[stdout] stdout line from the guest", "[stderr] stderr line from the guest"]
    {
        assert!(log.contains(logged), "{logged} not in:\n{log}");
    }
    assert_eq!(fs::read_to_string(g.path("data/hello.txt")).unwrap(), "hello");
    // WASI says it starts in /data. (The std of the pinned nightly doesn't
    // read that: it starts in /, where a relative path finds no folder. A
    // plugin names its files by /data.)
    assert_eq!(line(&g.said(), "initial-cwd="), r#"initial-cwd=Some("/data")"#);
}

#[test]
fn a_run_gets_its_context_with_paths_in_the_sandbox() {
    let Some(g) = guest("context") else { return };
    let g = g.with(&[Permission::ExportsRead, Permission::ProjectRead]);
    let said = g.runs();
    assert_eq!(line(&said, "protocol="), "protocol=2");
    assert_eq!(line(&said, "plugin="), "plugin=dev.autoscene.test-guest 1.2.0 /data");
    assert_eq!(line(&said, "action="), "action=export-action context menu");
    assert_eq!(line(&said, "project="), r#"project=p1 Breaking folder=Some("/project")"#);
    assert!(line(&said, "export=").contains(r#"out-dir=Some("/export")"#), "{said}");
    assert!(line(&said, "clip=Clip 1 ").contains(r#"path=Some("/export/Clip 1.mp4")"#), "{said}");
    assert!(line(&said, "clip=Clip 1 ").contains("240-480"), "{said}");
    // A clip outside the export's folder has no path in the sandbox.
    assert!(line(&said, "clip=Elsewhere ").contains("path=None"), "{said}");
    // Settings sorted by key, without the secret's.
    let settings: Vec<&str> = said.lines().filter(|l| l.starts_with("setting ")).collect();
    assert_eq!(settings, ["setting alpha=2", "setting greeting=Hi"]);
    assert!(!said.contains(MARKER) && !said.contains(&g.dir.path().to_string_lossy().into_owned()), "{said}");

    // Without project:read the project is named, and has no folder.
    let Some(g) = guest("context") else { return };
    let said = g.with(&[Permission::ExportsRead]).runs();
    assert_eq!(line(&said, "project="), "project=p1 Breaking folder=None");
}

#[test]
fn a_page_run_gets_its_input() {
    let Some(mut g) = guest("context") else { return };
    g.message.action = ActionContext { kind: ActionKind::Command, id: "context".into(), trigger: Trigger::Page };
    g.message.export = None;
    g.message.input = Some(json!({"words": ["one", "two"]}));
    let said = g.runs();
    assert_eq!(line(&said, "action="), "action=command context page");
    assert_eq!(line(&said, "input="), r#"input=Some("{\"words\":[\"one\",\"two\"]}")"#);
}

#[test]
fn a_plugin_sees_only_its_folders() {
    let Some(g) = guest("escape") else { return };
    let outside = g.path("outside/secret.txt");
    let paths = [outside.to_string_lossy().into_owned(), g.dir.path().to_string_lossy().into_owned()].join("\n");
    let g = g.with(&[Permission::ExportsRead, Permission::ProjectRead]).text("paths", &paths);
    let said = g.runs();
    // On Linux the test's folders are in /tmp, and their paths name a file
    // in the run's own /tmp: writing one is its own business.
    for l in said.lines().filter(|l| !l.starts_with("symlink ") && !l.starts_with("write /tmp/")) {
        assert!(l.contains(": denied ("), "{l}\n{said}");
    }
    assert_eq!(fs::read_to_string(&outside).unwrap(), "secret");
    assert!(!g.path("x").exists() && !g.path("outside/x").exists());
}

#[test]
fn links_never_lead_out_of_a_folder() {
    let Some(g) = guest("escape") else { return };
    let outside = g.path("outside");
    let link = g.path("data").join("j");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    #[cfg(windows)]
    {
        // A junction: a symlink would need developer mode.
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
    }
    let g = g.text("paths", &["/data/j/secret.txt", "/data/j", &outside.to_string_lossy()].join("\n"));
    let said = g.runs();
    denied(&said, "read /data/j/secret.txt:");
    denied(&said, "list /data/j:");
    denied(&said, "write /data/j/secret.txt:");
    // Its own links that climb out lead nowhere either.
    denied(&said, "read through /data/l:");
    denied(&said, "read /data/l/x:");
    denied(&said, "read /data/host-link:");
    assert_eq!(fs::read_to_string(outside.join("secret.txt")).unwrap(), "secret");
}

#[test]
fn the_host_never_follows_a_link_a_plugin_made_in_data() {
    let Some(g) = guest("links") else { return };
    fs::write(g.path("outside/secret.json"), r#"{"secret": 1}"#).unwrap();
    fs::write(g.path("outside/target.json"), "untouched").unwrap();
    // Its page state, and the file a write of it goes to, made links to a
    // file of the person's.
    let links = "page-state.json ../outside/secret.json\npage-state.json.tmp ../outside/target.json";
    let g = g.text("links", links);
    let said = g.runs();
    let made = line(&said, "link /data/page-state.json ").ends_with(": ALLOWED");
    // Windows makes links only in developer mode.
    #[cfg(unix)]
    assert!(made, "{said}");
    let data = g.path("data");
    let ended = CancelToken::new();
    if made {
        // Its page reads nothing through it, and writes nothing.
        assert!(page_state::keys(&data).is_err() && page_state::get(&data, "secret").is_err());
        assert!(page_state::set(&data, "k", json!(1), &ended).is_err());
        fs::remove_file(data.join(page_state::FILE)).unwrap();
    }
    page_state::set(&data, "k", json!(1), &ended).unwrap();
    assert_eq!(page_state::keys(&data).unwrap(), ["k"]);
    assert_eq!(fs::read_to_string(g.path("outside/target.json")).unwrap(), "untouched");
    assert_eq!(fs::read_to_string(g.path("outside/secret.json")).unwrap(), r#"{"secret": 1}"#);
}

#[test]
fn a_read_only_folder_stays_read_only() {
    let Some(g) = guest("escape") else { return };
    let g = g.with(&[Permission::ExportsRead, Permission::ProjectRead]);
    let said = g.runs();
    for dir in ["/export", "/project"] {
        for tried in [
            format!("write {dir}/new.txt:"),
            format!("mkdir {dir}/new:"),
            format!("delete {dir}/remove-me.txt:"),
            format!("rename {dir}/remove-me.txt to /data:"),
            format!("rename /data into {dir}:"),
            format!("hard link {dir}/remove-me.txt into /data:"),
            format!("hard link /data into {dir}:"),
            format!("truncate {dir}/remove-me.txt:"),
        ] {
            denied(&said, &tried);
        }
    }
    assert_eq!(fs::read_to_string(g.path("export/remove-me.txt")).unwrap(), "keep me");
    assert!(!g.path("export/new.txt").exists() && !g.path("project/new.txt").exists());
    assert!(!g.path("data/moved.txt").exists() && !g.path("data/linked.txt").exists());
}

#[test]
fn exports_write_lets_it_change_the_export_folder() {
    let Some(g) = guest("write-export") else { return };
    let g = g.with(&[Permission::ExportsWrite]);
    assert_eq!(g.run().0.unwrap().as_deref(), Some("written"));
    assert_eq!(fs::read_to_string(g.path("export/out.txt")).unwrap(), "out");
    assert!(!g.path("export/remove-me.txt").exists());

    // Read only, it fails at its first write.
    let Some(g) = guest("write-export") else { return };
    let (r, _) = g.with(&[Permission::ExportsRead]).run();
    let Err(PluginError::Failed { message, .. }) = r else { panic!("{r:?}") };
    assert!(message.starts_with("write /export/out.txt:"), "{message}");
}

#[test]
fn resources_are_read_only_at_slash_resources() {
    let Some(g) = guest("read-resources") else { return };
    // Weights larger than one read, and a small file beside them.
    let weights: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    let vocab: &[u8] = br#"{"a": 1}"#;
    let files = [("model.onnx", weights.as_slice()), ("vocab.json", vocab)];
    let g = g.files(&files).text("paths", "model.onnx\nvocab.json");
    let said = g.runs();
    assert_eq!(line(&said, "preopens="), "preopens=/data,/resources,/tmp", "{said}");
    assert!(line(&said, "list /resources:").ends_with(": ALLOWED"), "{said}");
    for (name, bytes) in files {
        // It reads each whole: its copy in /data hashes as the file does.
        let file = format!("/resources/{name}");
        assert_eq!(line(&said, &format!("read {file}:")), format!("read {file}: {} bytes", bytes.len()));
        assert_eq!(sha256_hex(&fs::read(g.path("data").join(name)).unwrap()), sha256_hex(bytes));
        // It changes none of them.
        for tried in ["write", "append", "truncate", "delete"] {
            denied(&said, &format!("{tried} {file}:"));
        }
        denied(&said, &format!("rename {file} to /data:"));
        denied(&said, &format!("rename /data into {file}:"));
        denied(&said, &format!("hard link {file} into /data:"));
        denied(&said, &format!("hard link /data into {file}:"));
        denied(&said, &format!("set times of {file}:"));
        assert_eq!(fs::read(g.path("resources").join(name)).unwrap(), bytes, "{name}");
        let modified = fs::metadata(g.path("resources").join(name)).unwrap().modified().unwrap();
        assert_ne!(modified, std::time::SystemTime::UNIX_EPOCH, "{name}");
    }
    // Nor adds one, nor climbs out: to a file that is there, refused for
    // where it is, not missed.
    denied(&said, "write /resources/new:");
    denied(&said, "mkdir /resources/new-dir:");
    let up = line(&said, "read /resources/../outside/secret.txt:");
    assert!(up.contains(": denied (") && !up.contains("NotFound"), "{up}");
    let mut kept: Vec<String> =
        fs::read_dir(g.path("resources")).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    kept.sort();
    assert_eq!(kept, ["model.onnx", "vocab.json"]);
    assert!(!g.path("data/moved").exists() && !g.path("data/linked").exists());
}

#[test]
fn no_resources_no_preopen() {
    let Some(g) = guest("read-resources") else { return };
    // Files lie in a folder beside its own, but its plugin names none.
    fs::create_dir(g.path("resources")).unwrap();
    fs::write(g.path("resources/model.onnx"), "weights").unwrap();
    let g = g.text("paths", "model.onnx");
    let said = g.runs();
    assert_eq!(line(&said, "preopens="), "preopens=/data,/tmp", "{said}");
    denied(&said, "list /resources:");
    denied(&said, "read /resources/model.onnx:");
    denied(&said, "write /resources/new:");
    assert!(!g.path("data/model.onnx").exists());
    assert_eq!(fs::read_to_string(g.path("resources/model.onnx")).unwrap(), "weights");
}

#[test]
fn folders_it_was_not_given_are_not_there() {
    let Some(g) = guest("escape") else { return };
    let said = g.text("paths", "/export\n/export/Clip 1.mp4\n/project/notes.txt").runs();
    denied(&said, "list /export:");
    denied(&said, "read /export/Clip 1.mp4:");
    denied(&said, "read /project/notes.txt:");
}

#[test]
fn tmp_is_the_run_s_scratch_folder() {
    let Some(g) = guest("escape") else { return };
    let g = g.text("paths", "/tmp\n/tmp/probe.txt\n/tmp/../x");
    // What it writes to /tmp lands in the run's temp folder; the marker
    // beside it is out of its reach.
    let temp = tempfile::tempdir().unwrap();
    let scratch = Scratch::new_in(temp.path()).unwrap();
    let r = wasm::run_in_scratch(g.spec(), Net::system(), &scratch, &CancelToken::new(), &|_, _| {});
    assert!(matches!(r, Ok(Some(_))), "{r:?}");
    let said = g.said();
    allowed(&said, "list /tmp:");
    allowed(&said, "write /tmp/probe.txt:");
    // A run's /tmp starts empty: nothing was there to read.
    denied(&said, "read /tmp/probe.txt:");
    assert_eq!(fs::read_to_string(scratch.path().join("probe.txt")).unwrap(), "x");
    let names: Vec<_> = fs::read_dir(scratch.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names, ["probe.txt"]);
    assert!(scratch.root().join(scratch::MARKER).is_file());
    // Nor does it lead anywhere else.
    denied(&said, "read /tmp/../x:");
    denied(&said, "write /tmp/../x:");
    let dir = scratch.root().to_owned();
    drop(scratch);
    let until = Instant::now() + Duration::from_secs(30);
    while fs::symlink_metadata(&dir).is_ok() {
        assert!(Instant::now() < until, "{} is still there", dir.display());
        thread::sleep(Duration::from_millis(20));
    }
    // Each run has a /tmp of its own, in the system's temp folder: the
    // second finds nothing the first left.
    for _ in 0..2 {
        let Some(g) = guest("escape") else { return };
        let said = g.text("paths", "/tmp/probe.txt").runs();
        denied(&said, "read /tmp/probe.txt:");
        allowed(&said, "write /tmp/probe.txt:");
    }
}

#[test]
fn a_run_whose_tmp_can_t_be_made_runs_without_one() {
    let Some(g) = guest("escape") else { return };
    let g = g.text("paths", "/tmp\n/tmp/probe.txt");
    // A system temp folder that is gone, as when TEMP names a deleted folder.
    let gone = tempfile::tempdir().unwrap();
    let (r, _) = scratch::with_temp_root(&gone.path().join("deleted"), || g.run());
    assert!(matches!(r, Ok(Some(_))), "{r:?}");
    let said = g.said();
    denied(&said, "list /tmp:");
    denied(&said, "write /tmp/probe.txt:");
    let log = g.log();
    assert!(log.contains("[host] couldn't make its temp folder, so it has no /tmp: "), "{log}");
}

#[test]
fn sockets_are_refused() {
    let Some(g) = guest("sockets") else { return };
    let said = g.runs();
    for tried in ["tcp connect:", "tcp connect to this computer:", "udp bind:", "name lookup:"] {
        denied(&said, tried);
    }
}

#[test]
fn only_its_own_variables_and_arguments_reach_it() {
    let Some(g) = guest("hello") else { return };
    // The test's own environment is there to leak (std::env::set_var is
    // unsafe, so the test uses what is set already).
    assert!(std::env::var_os("PATH").is_some());
    let said = g.runs();
    assert_eq!(line(&said, "env="), "env=AUTOSCENE_PLUGIN_ID=dev.autoscene.test-guest;AUTOSCENE_PROTOCOL=2");
    assert_eq!(line(&said, "args="), "args=dev.autoscene.test-guest");
}

#[test]
fn stop_ends_a_busy_plugin_after_the_grace() {
    let Some(g) = guest("spin") else { return };
    let (r, took) = g.run_cancelled(Duration::from_secs(1));
    assert!(matches!(r, Err(PluginError::Cancelled)), "{r:?}");
    assert!(took >= CANCEL_GRACE && took < CANCEL_GRACE + Duration::from_secs(2), "{took:?}");
}

#[test]
fn stop_ends_a_sleeping_plugin_after_the_grace() {
    let Some(g) = guest("sleep") else { return };
    let (r, took) = g.run_cancelled(Duration::from_secs(1));
    assert!(matches!(r, Err(PluginError::Cancelled)), "{r:?}");
    assert!(took >= CANCEL_GRACE && took < CANCEL_GRACE + Duration::from_secs(2), "{took:?}");
    assert!(g.log().contains("still running after cancel; ended"));
}

#[test]
fn a_plugin_that_looks_at_stop_ends_at_once() {
    let Some(g) = guest("spin-polite") else { return };
    let (r, took) = g.run_cancelled(Duration::from_secs(1));
    // Stop came before its error: the run was cancelled.
    assert!(matches!(r, Err(PluginError::Cancelled)), "{r:?}");
    assert!(took < Duration::from_millis(1500), "{took:?}");
    assert!(g.log().contains("[failed] Stopped."));
}

#[test]
fn a_result_after_stop_is_logged_and_cancelled() {
    let Some(g) = guest("spin-polite-result") else { return };
    let (r, took) = g.run_cancelled(Duration::from_secs(1));
    // It saw Stop before it returned: its result doesn't count.
    assert!(matches!(r, Err(PluginError::Cancelled)), "{r:?}");
    assert!(took < Duration::from_millis(1500), "{took:?}");
    assert!(g.log().contains("[result] stopped with a result"));
}

#[test]
fn a_run_past_its_time_limit_is_stopped() {
    for action in ["spin", "sleep"] {
        let Some(mut g) = guest(action) else { return };
        g.limits = Limits { memory: 512 << 20, time: Duration::from_secs(2) };
        // Compiled first, so the time is the run's.
        let sha256 = sha256_hex(&fs::read(&g.component).unwrap());
        wasm::prepare(&g.component, Some(&sha256), &g.path("code"), &wasm_manifest(false)).unwrap();
        g.sha256 = Some(sha256);
        let started = Instant::now();
        let (r, _) = g.run();
        let took = started.elapsed();
        let Err(e @ PluginError::TimedOut { minutes: 1, .. }) = r else { panic!("{action}: {r:?}") };
        assert_eq!(e.to_string(), "Test Guest ran for longer than 1 minute and was stopped.");
        assert!(took >= Duration::from_secs(2) && took < Duration::from_secs(3), "{action}: {took:?}");
    }
}

#[test]
fn a_plugin_cannot_take_more_memory_than_its_limit() {
    let Some(mut g) = guest("hog") else { return };
    g.limits = Limits { memory: 64 << 20, time: Duration::from_secs(60) };
    let (r, _) = g.run();
    let Err(e @ PluginError::OutOfMemory { mb: 64, .. }) = r else { panic!("{r:?}") };
    assert_eq!(e.to_string(), "Test Guest needed more than 64 MB of memory and was stopped.");

    // One that minds a refused allocation says so itself.
    let Some(mut g) = guest("hog-polite") else { return };
    g.limits = Limits { memory: 64 << 20, time: Duration::from_secs(60) };
    let (r, _) = g.run();
    let Err(PluginError::Failed { message, .. }) = r else { panic!("{r:?}") };
    assert_eq!(message, "No more memory.");
}

#[test]
fn a_crash_after_a_refused_allocation_it_minded_is_a_crash() {
    let Some(mut g) = guest("refused-then-panic") else { return };
    g.limits = Limits { memory: 64 << 20, time: Duration::from_secs(60) };
    let (r, _) = g.run();
    assert!(matches!(r, Err(PluginError::Crashed { .. })), "{r:?}");
    assert!(g.log().contains("guest panicked after 8388608 bytes"));
}

#[test]
fn a_stack_overflow_is_a_crash_not_a_host_crash() {
    let Some(g) = guest("recurse") else { return };
    let (r, _) = g.run();
    let Err(PluginError::Crashed { name, detail }) = r else { panic!("{r:?}") };
    assert_eq!(name, NAME);
    assert!(detail.contains("call stack exhausted"), "{detail}");
    assert!(g.log().contains("[host] trapped:"));
}

#[test]
fn a_panic_is_a_crash_with_its_message_in_the_log() {
    let Some(g) = guest("panic") else { return };
    let (r, _) = g.run();
    assert!(matches!(r, Err(PluginError::Crashed { .. })), "{r:?}");
    let log = g.log();
    assert!(log.lines().any(|l| l.starts_with("[stderr]") && l.contains("guest panicked on purpose")), "{log}");
}

#[test]
fn a_flood_of_output_never_ends_the_run() {
    let Some(g) = guest("flood") else { return };
    let started = Instant::now();
    let (r, reports) = g.run();
    let took = started.elapsed();
    assert_eq!(r.unwrap().as_deref(), Some("flooded"));
    let log = fs::metadata(g.path("logs/plugin.log")).unwrap().len();
    assert!(log <= MAX_LOG_BYTES + RESERVE, "{log}");
    assert!(g.log().contains("[host] more output dropped"));
    // "Getting … ready", then at most one a PROGRESS_GAP, and the last.
    let most = (took.as_millis() / PROGRESS_GAP.as_millis()) as usize + 3;
    assert!(reports.len() <= most, "{} reports in {took:?}", reports.len());
    assert_eq!(reports.last().unwrap().0, "Flooding");
}

#[test]
fn too_many_resources_end_the_run_cleanly() {
    let Some(g) = guest("resources") else { return };
    let (r, _) = g.run();
    assert!(matches!(r, Err(PluginError::Crashed { .. })), "{r:?}");
}

#[test]
fn a_huge_random_request_ends_the_run_cleanly() {
    let Some(g) = guest("random") else { return };
    let (r, _) = g.run();
    assert!(matches!(r, Err(PluginError::Crashed { .. })), "{r:?}");

    // A small one is answered.
    let Some(mut g) = guest("random") else { return };
    g.message.settings.insert("bytes".into(), SettingValue::Number(4096.0));
    assert_eq!(g.run().0.unwrap().as_deref(), Some("got 4096"));
}

#[test]
fn a_plugin_error_is_its_message() {
    let Some(g) = guest("fail") else { return };
    let (r, _) = g.run();
    let Err(PluginError::Failed { message, detail }) = r else { panic!("{r:?}") };
    assert_eq!((message.as_str(), detail.as_deref()), ("It failed on purpose.", Some("details")));
    assert!(g.log().contains("[failed] It failed on purpose. (details)"));
}

#[test]
fn huge_strings_from_a_plugin_are_cut_before_they_are_cleaned() {
    let Some(mut g) = guest("huge") else { return };
    g.limits = Limits { memory: 512 << 20, time: Duration::from_secs(60) };
    g.compile();
    let started = Instant::now();
    let (r, reports) = g.run();
    let took = started.elapsed();
    // Cleaning each 100 MB whole took seconds.
    assert!(took < Duration::from_secs(5), "{took:?}");
    let message = r.unwrap().unwrap();
    assert!(message.starts_with("a a ") && message.ends_with('…') && message.chars().count() <= 300, "{message}");
    let (stage, _) = reports.last().unwrap();
    assert!(stage.starts_with("a a ") && stage.chars().count() <= 80, "{stage}");
    let log = g.log();
    let logged = line(&log, "[info] ");
    assert!(logged.starts_with("[info] a a ") && logged.len() < 2100, "{}", logged.len());
    let result = line(&log, "[result] ");
    let most = "[result] ".len() + MAX_LINE_BYTES + " [cut]".len();
    assert!(result.ends_with(" [cut]") && result.len() <= most, "{}", result.len());
}

#[test]
fn a_run_stopped_while_its_component_is_compiled_starts_nothing() {
    let Some(g) = guest("write-export") else { return };
    let g = g.with(&[Permission::ExportsWrite]);
    let cancel = CancelToken::new();
    // Stop as the person sees "Getting … ready".
    let r = wasm::run(g.spec(), &cancel, &|stage, _| {
        if stage.starts_with("Getting") {
            cancel.cancel();
        }
    });
    assert!(matches!(r, Err(PluginError::Cancelled)), "{r:?}");
    assert!(!g.path("export/out.txt").exists());
    assert_eq!(fs::read_to_string(g.path("export/remove-me.txt")).unwrap(), "keep me");
    assert_eq!(fs::read_dir(g.path("data")).unwrap().count(), 0);
    assert!(g.log().contains("[host] cancelled before it started"));
    // Its code is kept: the next run starts at once.
    assert_eq!(fs::read_dir(g.path("code")).unwrap().count(), 1);
}

#[test]
fn a_run_cancelled_before_it_starts_starts_nothing() {
    let Some(g) = guest("hello") else { return };
    fs::remove_dir(g.path("data")).unwrap();
    let cancel = CancelToken::new();
    cancel.cancel();
    let (r, reports) = g.run_with(&cancel);
    assert!(matches!(r, Err(PluginError::Cancelled)), "{r:?}");
    assert!(reports.is_empty());
    assert!(!g.path("data").exists() && !g.path("logs").exists() && !g.path("code").exists());
}

#[test]
fn the_compile_cache_is_reused_and_rebuilt_when_stale() {
    let Some(mut g) = guest("hello") else { return };
    g.sha256 = Some(sha256_hex(&fs::read(&g.component).unwrap()));
    let getting_ready = |reports: &Reports| reports.iter().any(|(stage, _)| stage.starts_with("Getting"));
    let (r, reports) = g.run();
    assert!(r.is_ok() && getting_ready(&reports), "{r:?} {reports:?}");
    let code: Vec<PathBuf> = fs::read_dir(g.path("code")).unwrap().map(|e| e.unwrap().path()).collect();
    assert_eq!(code.len(), 1);
    assert!(code[0].file_name().unwrap().to_string_lossy().ends_with(".cwasm"));

    let (r, reports) = g.run();
    assert!(r.is_ok() && !getting_ready(&reports), "{r:?} {reports:?}");

    // Damaged code is compiled again, and the log says so.
    fs::write(&code[0], b"damaged").unwrap();
    let (r, reports) = g.run();
    assert!(r.is_ok() && getting_ready(&reports), "{r:?} {reports:?}");
    assert!(g.log().contains("cache: compiled plugin rebuilt"));
    assert!(fs::metadata(&code[0]).unwrap().len() > 7);
}

#[test]
fn a_changed_component_is_refused() {
    let Some(mut g) = guest("hello") else { return };
    g.sha256 = Some(sha256_hex(b"another component"));
    let (r, _) = g.run();
    let Err(PluginError::Invalid(why)) = r else { panic!("{r:?}") };
    assert_eq!(why, "Its files were changed. Install it again.");
    assert!(g.log().contains("[host] couldn't start: Its files were changed."));

    // Nor one that isn't a hash at all: it never names a file of code.
    let Some(mut g) = guest("hello") else { return };
    g.sha256 = Some("../../data/x".into());
    let (r, _) = g.run();
    assert!(matches!(r, Err(PluginError::Invalid(_))), "{r:?}");
}

#[test]
fn a_component_is_prepared_once_for_its_code() {
    let Some(g) = guest("hello") else { return };
    let bytes = fs::read(&g.component).unwrap();
    let code = g.path("code");
    fs::create_dir_all(&code).unwrap();
    fs::write(code.join("older.cwasm"), b"old").unwrap();
    let (checked, file) = wasm::prepare(&g.component, Some(&sha256_hex(&bytes)), &code, &wasm_manifest(false)).unwrap();
    assert!(checked.imports.iter().any(|i| i == "autoscene:plugin/host@1.0.0"), "{:?}", checked.imports);
    // The guest can fetch, and its manifest has no network: every request is refused.
    assert_eq!(
        checked.warnings,
        ["It imports wasi:http but doesn't ask for network: every request it makes is refused."]
    );
    // Beside the code there, which may be the installed version's: the
    // store keeps only the new code once the person agreed to it.
    let left = || {
        let mut left: Vec<PathBuf> = fs::read_dir(&code).unwrap().map(|e| e.unwrap().path()).collect();
        left.sort();
        left
    };
    let mut both = vec![file.clone(), code.join("older.cwasm")];
    both.sort();
    assert_eq!(left(), both);
    wasm::keep_only(&code, &file.file_name().unwrap().to_string_lossy());
    assert_eq!(left(), [file]);
    let wrong = wasm::prepare(&g.component, Some(&sha256_hex(b"other")), &code, &wasm_manifest(true));
    assert!(matches!(wrong, Err(PluginError::Invalid(_))), "{wrong:?}");
    assert!(wasm::check(&bytes, Some(&wasm_manifest(true))).unwrap().warnings.is_empty());
}

#[test]
fn a_core_module_is_refused() {
    let e = wasm::check(&component("(module)"), None).unwrap_err();
    assert_eq!(
        e.to_string(),
        "The plugin's component isn't a WebAssembly component. Build it for wasm32-wasip2 or with componentize-py."
    );
    let e = wasm::check(b"not wasm", None).unwrap_err();
    assert!(matches!(e, PluginError::Invalid(_)), "{e:?}");
}

#[test]
fn a_newer_world_minor_needs_a_newer_autoscene() {
    let e = wasm::check(&component(r#"(component (import "autoscene:plugin/host@1.1.0" (instance)))"#), None);
    assert!(matches!(e, Err(PluginError::TooNew(ref f)) if f == "world"), "{e:?}");
}

#[test]
fn a_wasi_0_3_component_needs_a_newer_autoscene() {
    let e = wasm::check(&component(r#"(component (import "wasi:http/handler@0.3.0" (instance)))"#), None);
    assert!(matches!(e, Err(PluginError::TooNew(ref f)) if f == "wasi"), "{e:?}");
}

#[test]
fn an_async_component_needs_a_newer_autoscene() {
    let e = wasm::check(&component("(component (core func (canon waitable-set.new)))"), None);
    assert!(matches!(e, Err(PluginError::TooNew(ref f)) if f == "component"), "{e:?}");
}

#[test]
fn an_import_autoscene_does_not_offer_is_refused() {
    let e = wasm::check(&component(r#"(component (import "wasi:keyvalue/store@0.2.0-draft" (instance)))"#), None);
    let e = e.unwrap_err();
    assert_eq!(e.to_string(), "The plugin uses wasi:keyvalue/store@0.2.0-draft, which AutoScene doesn't offer.");
}

#[test]
fn a_component_without_run_is_refused() {
    let e = wasm::check(&component("(component)"), None).unwrap_err();
    assert!(e.to_string().starts_with("The plugin's component doesn't fit this AutoScene: "), "{e}");
}

#[test]
fn a_run_of_another_type_is_refused() {
    let wat = r#"(component
        (core module $m (func (export "run") (param i32)))
        (core instance $i (instantiate $m))
        (func $run (param "request" u32) (canon lift (core func $i "run")))
        (export "run" (func $run))
    )"#;
    let e = wasm::check(&component(wat), None).unwrap_err();
    assert!(e.to_string().starts_with("The plugin's component doesn't fit this AutoScene: "), "{e}");
}

#[test]
fn a_guest_of_the_frozen_1_0_world_still_runs() {
    let Some(g) = guest_from("wasm/guest-frozen.wasm", "hello") else { return };
    assert!(g.runs().starts_with("hello dev.autoscene.test-guest"));
}

#[test]
fn http_is_refused_without_the_network_permission() {
    let Some(g) = guest("fetch") else { return };
    let said = g.text("urls", "https://example.com/\nhttp://example.com/").runs();
    assert_eq!(line(&said, "https://example.com/:"), "https://example.com/: error ErrorCode::HttpRequestDenied");
    assert_eq!(line(&said, "http://example.com/:"), "http://example.com/: error ErrorCode::HttpRequestDenied");
}

#[test]
fn secrets_reach_only_through_secret() {
    let Some(mut g) = guest("secret") else { return };
    g.secrets = BTreeMap::from([("token".into(), MARKER.into())]);
    assert_eq!(g.runs(), "secret token is some");
    assert!(!g.log().contains(MARKER));

    // A value for a key that isn't one of its secret settings never reaches it.
    let Some(mut g) = guest("secret") else { return };
    g.secret_keys = Vec::new();
    g.secrets = BTreeMap::from([("token".into(), MARKER.into())]);
    assert_eq!(g.runs(), "secret token is none");
    assert!(!g.log().contains(MARKER));
}

//! Plugin runs, with the stand-in plugin (`src/bin/asp_test_plugin.rs`):
//! what reaches it, what it says, how a run ends when it misbehaves, and
//! how its calls are answered. Each run works in a folder of its own.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};
use std::{env, fs};

use as_core::{AppPaths, CancelToken};
use as_db::LibraryDb;
use as_plugin::calls::{self, SecretsGet};
use as_plugin::log::MAX_LOG_BYTES;
use as_plugin::protocol::{ClipContext, ExportContext, ProjectContext};
use as_plugin::runtime::{
    self, CALL_QUEUE, CANCEL_GRACE, EXIT_GRACE, MAX_CALL_BYTES, MAX_CALLS_IN_FLIGHT, MAX_LINE_BYTES, MAX_UNREAD,
    RunSpec, TERM_GRACE,
};
use as_plugin::scratch;
use as_plugin::{
    ActionContext, ActionKind, AppContext, CallCode, CallError, Caller, CharacterContext, EpisodeContext, FaceBox,
    Grants, HostCalls, HostMethod, Launch, MemorySecrets, PluginContext, PluginError, PluginId, PluginResult,
    RunMessage, SettingValue, ShotContext, ShotsContext, Store, ToolPaths, Trigger, current_platform,
};
use semver::Version;
use serde_json::value::RawValue;
use serde_json::{Value, json};
use tempfile::TempDir;

const NAME: &str = "Test Plugin";
const ID: &str = "dev.autoscene.test-plugin";

/// A run of the stand-in plugin in `mode`, in a folder of its own.
struct Plugin {
    dir: TempDir,
    mode: &'static str,
    args: Vec<String>,
    message: RunMessage,
    protocol: u32,
    calls: Option<Arc<dyn HostCalls>>,
    /// The folder of its large files, when it names some.
    resources: Option<PathBuf>,
}

impl Plugin {
    fn new(mode: &'static str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        Self { dir, mode, args: Vec::new(), message: message(), protocol: 1, calls: None, resources: None }
    }

    /// A plugin of protocol 2 in `mode`, whose calls `answers` answers.
    fn calling<H: HostCalls + 'static>(mode: &'static str, answers: &Arc<H>) -> Self {
        Self { protocol: 2, calls: Some(answers.clone()), ..Self::new(mode) }
    }

    fn text(&self, name: &str) -> String {
        fs::read_to_string(self.data().join(name)).unwrap()
    }

    fn data(&self) -> PathBuf {
        self.dir.path().join("data")
    }

    fn log(&self) -> PathBuf {
        self.dir.path().join("logs").join(format!("{ID}.log"))
    }

    fn log_text(&self) -> String {
        fs::read_to_string(self.log()).unwrap()
    }

    fn spec(&self) -> RunSpec {
        let exe = Path::new(env!("CARGO_BIN_EXE_asp_test_plugin"));
        RunSpec {
            name: NAME.into(),
            dir: exe.parent().unwrap().to_path_buf(),
            launch: Launch::Exe {
                program: exe.file_name().unwrap().to_str().unwrap().into(),
                args: [vec![self.mode.into()], self.args.clone()].concat(),
            },
            data_dir: self.data(),
            log_path: self.log(),
            ffmpeg: self.dir.path().join("tools").join("ffmpeg"),
            ffprobe: self.dir.path().join("tools").join("ffprobe"),
            // A program: no uv runs.
            uv: None,
            resources: self.resources.clone(),
            protocol: self.protocol,
            calls: self.calls.clone(),
            verdicts: None,
            message: self.message.clone(),
        }
    }

    fn run(&self) -> PluginResult<Option<String>> {
        runtime::run(self.spec(), &CancelToken::new(), &|_, _| {})
    }

    /// Runs it, cancelled once its grandchild beats: the result, and how
    /// long the run took from the cancel on.
    fn run_cancelled(&self) -> (PluginResult<Option<String>>, Duration) {
        let beat = self.data().join("beat");
        self.run_cancelled_once(|| fs::metadata(&beat).is_ok_and(|m| m.len() > 0))
    }

    /// Runs it, cancelled once `ready`: the result, and how long the run
    /// took from the cancel on.
    fn run_cancelled_once(&self, ready: impl Fn() -> bool) -> (PluginResult<Option<String>>, Duration) {
        let cancel = CancelToken::new();
        thread::scope(|s| {
            let run = s.spawn(|| runtime::run(self.spec(), &cancel, &|_, _| {}));
            wait_for(ready);
            let asked = Instant::now();
            cancel.cancel();
            (run.join().unwrap(), asked.elapsed())
        })
    }
}

fn message() -> RunMessage {
    RunMessage {
        event: None,
        protocol: 1,
        run_id: "run-1".into(),
        plugin: PluginContext {
            id: PluginId::parse(ID).unwrap(),
            version: "1.2.0".into(),
            data_dir: "/data".into(),
            resources_dir: None,
            temp_dir: None,
        },
        app: AppContext { version: "1.0.3-BETA".into(), platform: "linux-x86_64".into() },
        tools: ToolPaths {
            ffmpeg: "/tools/ffmpeg".into(),
            ffprobe: "/tools/ffprobe".into(),
            ffmpeg_version: "9.0".into(),
        },
        settings: BTreeMap::new(),
        action: ActionContext { kind: ActionKind::ExportAction, id: "post".into(), trigger: Trigger::Menu },
        project: None,
        export: None,
        input: None,
        shots: None,
    }
}

/// AutoScene's side of a run's calls: each is answered with its method's
/// name (and `pad` bytes more), once the answers are open, and counted.
#[derive(Default)]
struct Answers {
    open: Mutex<bool>,
    opened: Condvar,
    /// Whether a call stops waiting once its plugin ended.
    heeds_end: bool,
    pad: usize,
    seen: Mutex<Vec<HostMethod>>,
    running: AtomicUsize,
    most: AtomicUsize,
    done: AtomicUsize,
    /// Whether the last call answered saw its plugin's end.
    saw_end: AtomicBool,
}

impl Answers {
    fn open() -> Arc<Self> {
        let answers = Self::closed();
        answers.let_through();
        answers
    }

    fn closed() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Closed, but a call waits only while its plugin runs.
    fn heeding() -> Arc<Self> {
        Arc::new(Self { heeds_end: true, ..Self::default() })
    }

    /// Open, each answer `pad` bytes longer.
    fn padded(pad: usize) -> Arc<Self> {
        let answers = Arc::new(Self { pad, ..Self::default() });
        answers.let_through();
        answers
    }

    fn let_through(&self) {
        *self.open.lock().unwrap() = true;
        self.opened.notify_all();
    }

    fn seen(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
}

impl HostCalls for Answers {
    fn call(&self, method: HostMethod, _: &RawValue, ended: &CancelToken) -> Result<Value, CallError> {
        self.seen.lock().unwrap().push(method);
        let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.most.fetch_max(now, Ordering::SeqCst);
        let until = Instant::now() + Duration::from_secs(20);
        let mut open = self.open.lock().unwrap();
        while !*open && !(self.heeds_end && ended.is_cancelled()) && Instant::now() < until {
            open = self.opened.wait_timeout(open, Duration::from_millis(20)).unwrap().0;
        }
        let open = *open;
        self.saw_end.store(ended.is_cancelled(), Ordering::SeqCst);
        self.running.fetch_sub(1, Ordering::SeqCst);
        self.done.fetch_add(1, Ordering::SeqCst);
        match (open, ended.is_cancelled()) {
            (true, _) if self.pad > 0 => Ok(json!({"method": method.name(), "pad": "x".repeat(self.pad)})),
            (true, _) => Ok(json!({"method": method.name()})),
            (false, true) => Err(CallError::new(CallCode::Cancelled, "The run ended.")),
            (false, false) => Err(CallError::new(CallCode::Failed, "Never let through.")),
        }
    }
}

/// A host that breaks on every call.
struct Panics;

impl HostCalls for Panics {
    fn call(&self, method: HostMethod, _: &RawValue, _: &CancelToken) -> Result<Value, CallError> {
        panic!("a host that breaks on {}", method.name());
    }
}

fn wait_for(done: impl Fn() -> bool) {
    let until = Instant::now() + Duration::from_secs(20);
    while !done() {
        assert!(Instant::now() < until, "timed out");
        thread::sleep(Duration::from_millis(20));
    }
}

/// Whether the heartbeat in `file` stopped: it grows by a byte every 50 ms
/// while its process lives.
fn stopped(file: &Path) -> bool {
    // A process the system was told to end may take a moment.
    thread::sleep(Duration::from_millis(300));
    let before = fs::metadata(file).unwrap().len();
    thread::sleep(Duration::from_millis(600));
    fs::metadata(file).unwrap().len() == before
}

#[test]
fn a_result_line_gives_the_message() {
    let p = Plugin::new("result");
    let stages = Mutex::new(Vec::new());
    let result = runtime::run(p.spec(), &CancelToken::new(), &|stage, fraction| {
        stages.lock().unwrap().push((stage.to_owned(), fraction));
    });
    assert_eq!(result.unwrap().as_deref(), Some("Posted 12 clips."));
    assert_eq!(stages.into_inner().unwrap(), [("Posting".to_owned(), Some(0.5))]);
    let log = p.log_text();
    assert!(log.starts_with("--- ") && log.lines().next().unwrap().ends_with("Z run run-1 post"), "{log}");
    assert!(log.contains("[result] Posted 12 clips.\n"), "{log}");
    // A plugin that ends well without a word has nothing to say.
    assert_eq!(Plugin::new("quiet").run().unwrap(), None);
}

#[test]
fn the_run_message_is_one_line_with_the_full_context() {
    let mut p = Plugin::new("echo");
    p.message.settings = BTreeMap::from([
        ("count".into(), SettingValue::Number(3.0)),
        ("loud".into(), SettingValue::Bool(true)),
        ("webhook".into(), SettingValue::Text("https://discord.com/api/webhooks/1/x".into())),
    ]);
    p.message.action.trigger = Trigger::AfterExport;
    p.message.project = Some(ProjectContext { id: "p1".into(), title: "Breaking Bad".into(), folder: "/bb".into() });
    p.message.export = Some(ExportContext {
        status: None,
        id: "e1".into(),
        label: Some("Jesse".into()),
        preset: "prores".into(),
        out_dir: "/out".into(),
        created_at: 1_762_000_000_000,
        merged_file: None,
        clips: vec![ClipContext {
            name: "Jesse 001.mov".into(),
            path: Some("/out/Jesse 001.mov".into()),
            status: "done".into(),
            media_id: "m1".into(),
            source_path: "/media/S01E01.mkv".into(),
            episode: "S01E01".into(),
            start: 120,
            end: 480,
            fps: Some("24000/1001".into()),
        }],
    });
    // The protocol, the data folder and FFmpeg's paths are the run's own,
    // whatever the message held ("/data", "/tools/ffmpeg").
    p.message.protocol = 7;
    assert_eq!(p.run().unwrap().as_deref(), Some("Echoed."));
    let line = fs::read_to_string(p.data().join("run.json")).unwrap();
    let json = |path: PathBuf| serde_json::to_string(&path).unwrap();
    let (data, ffmpeg, ffprobe) = (json(p.data()), json(p.spec().ffmpeg), json(p.spec().ffprobe));
    assert_eq!(
        line,
        [
            r#"{"type":"run","protocol":1,"runId":"run-1","#,
            &format!(r#""plugin":{{"id":"dev.autoscene.test-plugin","version":"1.2.0","dataDir":{data}}},"#),
            r#""app":{"version":"1.0.3-BETA","platform":"linux-x86_64"},"#,
            &format!(r#""tools":{{"ffmpeg":{ffmpeg},"ffprobe":{ffprobe},"ffmpegVersion":"9.0"}},"#),
            r#""settings":{"count":3.0,"loud":true,"webhook":"https://discord.com/api/webhooks/1/x"},"#,
            r#""action":{"kind":"exportAction","id":"post","trigger":"afterExport"},"#,
            r#""project":{"id":"p1","title":"Breaking Bad","folder":"/bb"},"#,
            r#""export":{"id":"e1","label":"Jesse","preset":"prores","outDir":"/out","createdAt":1762000000000,"#,
            r#""mergedFile":null,"clips":[{"name":"Jesse 001.mov","path":"/out/Jesse 001.mov","status":"done","#,
            r#""mediaId":"m1","sourcePath":"/media/S01E01.mkv","episode":"S01E01","start":120,"end":480,"#,
            r#""fps":"24000/1001"}]}}"#,
            "\n",
        ]
        .concat()
    );
}

#[test]
fn an_installed_plugin_runs_in_its_own_folders_with_its_settings() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = AppPaths::at(tmp.path().join("data"));
    let db = Arc::new(LibraryDb::open(&paths.library_db()).unwrap());
    let app = Version::parse("1.0.3-BETA").unwrap();
    let store = Store::open(&paths, db, app, Arc::new(MemorySecrets::default())).unwrap();
    // The stand-in plugin in a developer's folder, loaded from there.
    let work = tmp.path().join("work");
    fs::create_dir(&work).unwrap();
    let exe = Path::new(env!("CARGO_BIN_EXE_asp_test_plugin"));
    let program = exe.file_name().unwrap().to_str().unwrap();
    fs::copy(exe, work.join(program)).unwrap();
    let manifest = json!({
        "manifestVersion": 1,
        "id": ID,
        "version": "1.2.0",
        "name": NAME,
        "description": "Stands in for a plugin.",
        "author": {"name": "AutoScene"},
        "engines": {"autoscene": ">=1.0.0", "protocol": 1},
        "runtime": {"type": "process", "command": {"*": [program, "echo"]}},
        "contributes": {
            "exportActions": [{"id": "post", "title": "Post"}],
            "settings": [
                {"key": "greeting", "type": "string", "title": "Greeting", "default": "Hi"},
                {"key": "count", "type": "number", "title": "Count", "default": 3}
            ]
        }
    });
    fs::write(work.join("plugin.json"), manifest.to_string()).unwrap();
    let p = store.commit(store.stage_folder(&work).unwrap(), true, true).unwrap();
    let id = PluginId::parse(ID).unwrap();
    store.set_values(&id, BTreeMap::from([("greeting".into(), SettingValue::Text("Hey".into()))])).unwrap();

    let action = || ActionContext { kind: ActionKind::ExportAction, id: "post".into(), trigger: Trigger::AfterExport };
    let tools = tmp.path().join("tools");
    let (ffmpeg, ffprobe) = (tools.join("ffmpeg"), tools.join("ffprobe"));
    let spec = store.run_spec(&p, action(), &ffmpeg, &ffprobe, "9.0.2").unwrap();
    let (data, log) = (paths.plugin_data().join(ID), paths.plugin_logs().join(format!("{ID}.log")));
    assert_eq!((&spec.dir, &spec.data_dir, &spec.log_path), (&work, &data, &log));
    assert_eq!(runtime::run(spec, &CancelToken::new(), &|_, _| {}).unwrap().as_deref(), Some("Echoed."));

    let line: Value = serde_json::from_str(&fs::read_to_string(data.join("run.json")).unwrap()).unwrap();
    assert_eq!(line["plugin"], json!({"id": ID, "version": "1.2.0", "dataDir": data}));
    assert_eq!(line["app"], json!({"version": "1.0.3-BETA", "platform": current_platform()}));
    assert_eq!(line["tools"], json!({"ffmpeg": ffmpeg, "ffprobe": ffprobe, "ffmpegVersion": "9.0.2"}));
    // The person's value, and the default of the one left alone.
    assert_eq!(line["settings"], json!({"count": 3.0, "greeting": "Hey"}));
    assert_eq!(line["action"], json!({"kind": "exportAction", "id": "post", "trigger": "afterExport"}));
    let run_id = line["runId"].as_str().unwrap();
    let header = fs::read_to_string(&log).unwrap().lines().next().unwrap().to_owned();
    assert!(header.ends_with(&format!(" run {run_id} post")), "{header}");
    // Each run has an id of its own.
    let again = store.run_spec(&p, action(), &ffmpeg, &ffprobe, "9.0.2").unwrap();
    assert_ne!(again.message.run_id, run_id);
}

/// AutoScene's answer to `secrets.get`, as the app gives it: from the
/// store, for a key of the plugin's own.
struct SecretAnswers {
    store: Arc<Store>,
    plugin: PluginId,
    grants: Grants,
}

impl HostCalls for SecretAnswers {
    fn call(&self, method: HostMethod, params: &RawValue, _: &CancelToken) -> Result<Value, CallError> {
        calls::check(method, &Caller::Run { project: None, export: None }, &self.grants)?;
        let SecretsGet { key } = calls::params(method, params)?;
        calls::check_secret(&key, &self.grants)?;
        Ok(json!({ "value": self.store.secret(&self.plugin, &key)? }))
    }
}

#[test]
fn a_secret_from_a_call_never_reaches_the_log_or_env() {
    let marker = "SECRET-MARKER-7f3a9c";
    let tmp = tempfile::tempdir().unwrap();
    let paths = AppPaths::at(tmp.path().join("data"));
    let db = Arc::new(LibraryDb::open(&paths.library_db()).unwrap());
    let app = Version::parse("1.0.3-BETA").unwrap();
    let secrets = Arc::new(MemorySecrets::default());
    let store = Arc::new(Store::open(&paths, db, app, secrets).unwrap());
    // The stand-in plugin, of protocol 2 with a secret setting, loaded from its folder.
    let work = tmp.path().join("work");
    fs::create_dir(&work).unwrap();
    let exe = Path::new(env!("CARGO_BIN_EXE_asp_test_plugin"));
    let program = exe.file_name().unwrap().to_str().unwrap();
    fs::copy(exe, work.join(program)).unwrap();
    let manifest = json!({
        "manifestVersion": 2,
        "id": ID,
        "version": "1.2.0",
        "name": NAME,
        "description": "Stands in for a plugin.",
        "author": {"name": "AutoScene"},
        "engines": {"autoscene": ">=1.0.0", "protocol": 2},
        "runtime": {"type": "process", "command": {"*": [program, "call-secret"]}},
        "contributes": {
            "commands": [{"id": "post", "title": "Post"}],
            "settings": [{"key": "token", "type": "secret", "title": "Token"}]
        }
    });
    fs::write(work.join("plugin.json"), manifest.to_string()).unwrap();
    store.commit(store.stage_folder(&work).unwrap(), true, true).unwrap();
    let id = PluginId::parse(ID).unwrap();
    let p = store.secret_set(&id, "token", marker).unwrap();

    let action = ActionContext { kind: ActionKind::Command, id: "post".into(), trigger: Trigger::Palette };
    let tools = tmp.path().join("tools");
    let mut spec = store.run_spec(&p, action, &tools.join("ffmpeg"), &tools.join("ffprobe"), "9.0.2").unwrap();
    let grants = Grants::of(&p).unwrap();
    spec.calls = Some(Arc::new(SecretAnswers { store: store.clone(), plugin: id, grants }));
    let e = runtime::run(spec, &CancelToken::new(), &|_, _| {}).unwrap_err();
    let PluginError::Crashed { detail, .. } = &e else { panic!("{e:?}") };

    let log = fs::read_to_string(paths.plugin_logs().join(format!("{ID}.log"))).unwrap();
    let run_line = fs::read_to_string(paths.plugin_data().join(ID).join("run.json")).unwrap();
    // It got the secret: its reply was as long as the line that holds it.
    let reply = format!("{{\"type\":\"reply\",\"id\":1,\"result\":{{\"value\":\"{marker}\"}}}}\n");
    assert!(log.contains(&format!("[stderr] a reply of {} bytes\n", reply.len())), "{log}");
    assert!(log.contains("[call] secrets.get\n[reply] secrets.get ok\n"), "{log}");
    // Its environment went to stderr, so to the log, and the end of it to the error.
    assert!(log.contains(&format!("[stderr] AUTOSCENE_PLUGIN_ID={ID}\n")), "{log}");
    assert!(detail.contains("a reply of"), "{detail}");
    for (what, text) in [("log", &log), ("stderr tail", detail), ("run line", &run_line), ("error", &e.to_string())] {
        assert!(!text.contains(marker), "the secret is in the {what}: {text}");
    }
    assert!(!run_line.contains("token"), "{run_line}");
}

#[test]
fn only_allowlisted_variables_reach_the_plugin() {
    // SAFETY: the tests here read the environment only through std, which
    // holds a lock while it does.
    unsafe {
        env::set_var("AUTOSCENE_TEST_SECRET", "hunter2");
        env::set_var("AUTOSCENE_DATA_DIR", "/elsewhere");
        env::set_var("AUTOSCENE_FFMPEG_DIR", "/elsewhere");
        env::set_var("RUST_LOG", "trace");
        env::set_var("CUDA_PATH_V13_0", "/cuda");
    }
    let p = Plugin::new("env");
    assert_eq!(p.run().unwrap(), None);
    let dump: Value = serde_json::from_str(&fs::read_to_string(p.data().join("env.json")).unwrap()).unwrap();
    let vars = dump["vars"].as_object().unwrap();
    let ours = [
        "AUTOSCENE_FFMPEG",
        "AUTOSCENE_FFPROBE",
        "AUTOSCENE_PLUGIN_DATA",
        "AUTOSCENE_PLUGIN_ID",
        "AUTOSCENE_PROTOCOL",
        "PYTHONUNBUFFERED",
        "PYTHONUTF8",
        "PYTHONIOENCODING",
        "PYTHONDONTWRITEBYTECODE",
    ];
    let passed = env::vars_os().map(|(k, _)| k).filter(|k| runtime::allowed_env(k));
    let expected: BTreeSet<OsString> = passed.chain(ours.iter().map(OsString::from)).collect();
    let got: BTreeSet<OsString> = vars.keys().map(OsString::from).collect();
    assert_eq!(got, expected);
    assert!(got.contains(&OsString::from("CUDA_PATH_V13_0")));
    for name in ["AUTOSCENE_TEST_SECRET", "AUTOSCENE_DATA_DIR", "AUTOSCENE_FFMPEG_DIR", "RUST_LOG"] {
        assert!(!vars.contains_key(name), "{name}");
    }
    let data = p.data();
    let text = |name: &str| vars[name].as_str().unwrap().to_owned();
    assert_eq!(text("AUTOSCENE_PLUGIN_ID"), ID);
    assert_eq!(text("AUTOSCENE_PROTOCOL"), "1");
    assert_eq!(text("PYTHONUTF8"), "1");
    // The plugin's folder gets no bytecode of the modules it imports.
    assert_eq!(text("PYTHONDONTWRITEBYTECODE"), "1");
    assert_eq!(PathBuf::from(text("AUTOSCENE_PLUGIN_DATA")), data);
    assert_eq!(PathBuf::from(text("AUTOSCENE_FFMPEG")), p.dir.path().join("tools").join("ffmpeg"));
    // It runs in its data folder.
    let cwd = PathBuf::from(dump["cwd"].as_str().unwrap());
    assert_eq!(cwd.canonicalize().unwrap(), data.canonicalize().unwrap());
}

/// The environment `p` (in mode `env`) ran with, and its folder.
fn env_of(p: &Plugin) -> Value {
    serde_json::from_str(&fs::read_to_string(p.data().join("env.json")).unwrap()).unwrap()
}

#[test]
fn a_run_gets_its_resources_folder() {
    // In the environment, as an absolute path.
    let mut p = Plugin::new("env");
    let folder = p.dir.path().join("cache").join("plugin-resources").join(ID);
    p.resources = Some(folder.clone());
    assert_eq!(p.run().unwrap(), None);
    let vars = env_of(&p)["vars"].as_object().unwrap().clone();
    assert_eq!(PathBuf::from(vars["AUTOSCENE_RESOURCES"].as_str().unwrap()), folder);
    // And in the run line, under plugin, whatever the line held before.
    let mut p = Plugin::new("echo");
    p.resources = Some(folder.clone());
    p.message.plugin.resources_dir = Some("/elsewhere".into());
    assert_eq!(p.run().unwrap().as_deref(), Some("Echoed."));
    let line: Value = serde_json::from_str(&p.text("run.json")).unwrap();
    assert_eq!(PathBuf::from(line["plugin"]["resourcesDir"].as_str().unwrap()), folder);
    // A relative folder reaches it as an absolute one.
    let mut p = Plugin::new("env");
    p.resources = Some(PathBuf::from("plugin-resources").join(ID));
    p.run().unwrap();
    let given = PathBuf::from(env_of(&p)["vars"]["AUTOSCENE_RESOURCES"].as_str().unwrap());
    assert!(given.is_absolute() && given.ends_with(Path::new("plugin-resources").join(ID)), "{}", given.display());
}

#[test]
fn a_run_without_resources_gets_neither() {
    // A plugin that names no large files gets the line and the environment
    // it always got.
    let p = Plugin::new("env");
    p.run().unwrap();
    assert!(!env_of(&p)["vars"].as_object().unwrap().contains_key("AUTOSCENE_RESOURCES"));
    let mut p = Plugin::new("echo");
    // Whatever the line held: the run's own folder decides.
    p.message.plugin.resources_dir = Some("/elsewhere".into());
    p.run().unwrap();
    let line: Value = serde_json::from_str(&p.text("run.json")).unwrap();
    let plugin = line["plugin"].as_object().unwrap();
    let keys: BTreeSet<&str> = plugin.keys().map(String::as_str).collect();
    assert_eq!(keys, BTreeSet::from(["id", "version", "dataDir"]));
}

#[cfg(windows)]
#[test]
fn windows_plugins_get_systemroot() {
    let p = Plugin::new("env");
    p.run().unwrap();
    let dump: Value = serde_json::from_str(&fs::read_to_string(p.data().join("env.json")).unwrap()).unwrap();
    let vars = dump["vars"].as_object().unwrap();
    let get = |name: &str| vars.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str().unwrap());
    let root = env::var("SystemRoot").unwrap();
    assert_eq!(get("SystemRoot"), Some(root.as_str()));
    for name in ["windir", "PATHEXT", "TEMP", "LOCALAPPDATA", "PROCESSOR_ARCHITECTURE"] {
        assert_eq!(get(name).map(str::to_owned), env::var(name).ok(), "{name}");
    }
}

#[test]
fn a_plugin_that_floods_its_pipes_still_finishes() {
    let mut p = Plugin::new("flood");
    // A run line far past any pipe's buffer, which the plugin reads only
    // after its flood: written on the run's own thread, it would hang both.
    p.message.settings = BTreeMap::from([("big".into(), SettingValue::Text("s".repeat(1 << 20)))]);
    let (tx, rx) = mpsc::channel();
    let spec = p.spec();
    thread::spawn(move || tx.send(runtime::run(spec, &CancelToken::new(), &|_, _| {})));
    let result = rx.recv_timeout(Duration::from_secs(20)).expect("the run hung");
    assert_eq!(result.unwrap().as_deref(), Some("Flooded."));
    let log = p.log_text();
    assert!(log.len() as u64 <= MAX_LOG_BYTES + 200, "{}", log.len());
    assert_eq!(log.matches("[host] more output dropped\n").count(), 1);
    assert!(log.contains(&format!("[stderr] {}\n", "e".repeat(1023))));
    // The host's word outlasts the flood.
    let end: Vec<_> = log.lines().rev().take(3).collect();
    assert_eq!(end[2], "[host] more output dropped");
    assert_eq!(end[1], "[result] Flooded.");
    assert!(end[0].starts_with("[host] exit ") && end[0].ends_with(": 0"), "{}", end[0]);
}

#[test]
fn non_json_lines_go_to_the_log() {
    let p = Plugin::new("chatty");
    // The first result counts.
    assert_eq!(p.run().unwrap().as_deref(), Some("Said hello."));
    let log = p.log_text();
    for line in [
        "[stdout] hello from a plugin\n",
        "[stdout] inner\n  return\n",
        "[info] two\n  [host] exit code: 0\n  --- 2026-01-01T00:00:00Z run forged post\n",
        "[stdout] {\"type\":\"future\",\"x\":1}\n",
        "[stdout] {\"type\":\"progress\",\"fraction\":\"half\"}\n",
        "[warn] careful\n",
        "[info] a level from later\n",
        "[result] Said hello.\n",
        "[result] Said it twice.\n",
    ] {
        assert!(log.contains(line), "{line}: {log}");
    }
    // Only the host starts a line with `---` or `[host]`.
    assert_eq!(log.lines().filter(|l| l.starts_with("---")).count(), 1, "{log}");
    assert_eq!(log.lines().filter(|l| l.starts_with("[host]")).count(), 1, "{log}");
}

#[test]
fn verdicts_reach_their_sink_and_odd_ones_the_log() {
    let shot = |start: u64, face_height: f32| ShotContext {
        key: format!("m1:{start}"),
        media_id: "m1".into(),
        start,
        end: start + 100,
        face_height,
        faceless: false,
        face: Some(FaceBox { frame: start + 10, x: 0.4, y: 0.2, w: 0.1, h: face_height }),
    };
    let mut p = Plugin::new("filter");
    p.protocol = 2;
    p.message.action = ActionContext { kind: ActionKind::ShotFilter, id: "speaks".into(), trigger: Trigger::Pack };
    p.message.shots = Some(ShotsContext {
        character: CharacterContext { id: "c1".into(), name: "Walter".into() },
        episodes: vec![EpisodeContext {
            media_id: "m1".into(),
            path: "/media/S01E01.mkv".into(),
            fps: Some("24000/1001".into()),
            frames: Some(2400),
        }],
        shots: vec![shot(0, 0.3), shot(100, 0.1), shot(200, 0.5)],
    });
    let judged = |p: &Plugin, sink: bool| {
        let got = Arc::new(Mutex::new(Vec::new()));
        let mut spec = p.spec();
        if sink {
            let keep = got.clone();
            spec.verdicts = Some(Box::new(move |shot: &str, kept| keep.lock().unwrap().push((shot.to_owned(), kept))));
        }
        let result = runtime::run(spec, &CancelToken::new(), &|_, _| {}).unwrap();
        (result, got.lock().unwrap().clone())
    };
    // Each shot it was sent, in the order it said them; a later word counts too.
    let (result, got) = judged(&p, true);
    assert_eq!(result.as_deref(), Some("Checked 3 shots."));
    let said = |v: &[(&str, bool)]| v.iter().map(|(s, k)| (s.to_string(), *k)).collect::<Vec<_>>();
    assert_eq!(got, said(&[("m1:0", true), ("m1:100", false), ("m1:200", true), ("m1:0", false)]));
    // One it wasn't sent goes to the log, and only it.
    let log = p.log_text();
    assert!(
        log.contains(
            "[verdict] elsewhere:1 false: a shot it wasn't sent, ignored
"
        ),
        "{log}"
    );
    assert_eq!(log.matches("[verdict]").count(), 1, "{log}");

    // Without a sink, or outside a shot filter's run, every verdict is the log's.
    for kind in [ActionKind::ShotFilter, ActionKind::Command] {
        let mut q = Plugin::new("filter");
        (q.protocol, q.message.shots) = (2, p.message.shots.clone());
        q.message.action = ActionContext { kind, id: "speaks".into(), trigger: Trigger::Pack };
        let (result, got) = judged(&q, kind == ActionKind::Command);
        assert_eq!((result.as_deref(), got), (Some("Checked 3 shots."), Vec::new()));
        let log = q.log_text();
        assert!(
            log.contains(
                "[verdict] m1:100 false: not a shot filter's run, ignored
"
            ),
            "{log}"
        );
        assert_eq!(log.matches("[verdict]").count(), 5, "{log}");
    }
}

#[test]
fn a_cut_line_is_no_message() {
    let p = Plugin::new("padded");
    // Its first 64 KB read as a result, but its end is gone.
    assert_eq!(p.run().unwrap(), None);
    let log = p.log_text();
    let line = log.lines().find(|l| l.starts_with("[stdout] ")).unwrap();
    assert!(line.starts_with(r#"[stdout] {"type":"result","message":"x"}  "#) && line.ends_with(" [cut]"), "{log}");
}

#[test]
fn an_error_line_fails_the_run_with_its_message() {
    let p = Plugin::new("error");
    match p.run() {
        Err(PluginError::Failed { message, detail }) => {
            assert_eq!(message, "No webhook is set. Set one in its settings.");
            // Cleaned for the app's log, and cut.
            let detail = detail.unwrap();
            assert!(detail.starts_with("KeyError: 'webhook' [2m at main.py:12 frame frame "), "{detail}");
            assert_eq!(detail.chars().count(), 2000);
            assert!(detail.ends_with('…') && !detail.chars().any(char::is_control), "{detail}");
        }
        other => panic!("{other:?}"),
    }
    // The plugin's log has it as it came, a line for each of its lines.
    let log = p.log_text();
    let failed = "[failed] No webhook is set.\n  Set one in its settings. (KeyError: 'webhook'\n  \u{1b}[2m  at main.py:12\n  frame ";
    assert!(log.contains(failed), "{log}");
    assert!(log.contains(&format!("\n  {})\n", "frame ".repeat(400))), "{log}");
}

#[test]
fn a_crash_says_so_and_keeps_the_stderr_tail_in_the_log() {
    let p = Plugin::new("crash");
    let e = p.run().unwrap_err();
    assert_eq!(e.to_string(), format!("{NAME} stopped unexpectedly."));
    let PluginError::Crashed { name, detail } = e else { panic!("{e:?}") };
    assert_eq!(name, NAME);
    let (status, tail) = detail.split_once("; ").unwrap();
    assert!(status.contains('3'), "{detail}");
    // The last 20 lines, each cleaned for the app's log and cut.
    let mut lines: Vec<String> = (11..30).map(|i| format!("problem {i}")).collect();
    lines.push(format!("[31mred [0m over {}…", "w".repeat(482)));
    assert_eq!(tail, lines.join("\n"));
    assert_eq!(lines[19].chars().count(), 500);
    let log = p.log_text();
    for i in 0..30 {
        assert!(log.contains(&format!("[stderr] problem {i}\n")), "{log}");
    }
    assert!(log.contains(&format!("[stderr] \u{1b}[31mred\u{1b}[0m\n  over {}\n", "w".repeat(600))), "{log}");
    assert!(log.contains(&format!("[host] {status}\n")), "{log}");
}

#[test]
fn cancel_asks_first_then_kills_the_tree() {
    let p = Plugin::new("cancel");
    let (result, took) = p.run_cancelled();
    assert!(matches!(result, Err(PluginError::Cancelled)), "{result:?}");
    assert!(took < CANCEL_GRACE, "{took:?}");
    assert_eq!(fs::read_to_string(p.data().join("cancel-seen")).unwrap(), "{\"type\":\"cancel\"}\n");
    // The plugin ended on its own; what it left running did not.
    assert!(stopped(&p.data().join("beat")));
    assert!(p.log_text().contains("[host] asked to cancel\n"));
}

#[test]
fn a_plugin_ignoring_cancel_and_its_grandchild_are_killed() {
    let p = Plugin::new("ignore");
    let (result, took) = p.run_cancelled();
    assert!(matches!(result, Err(PluginError::Cancelled)), "{result:?}");
    assert!(took >= CANCEL_GRACE, "{took:?}");
    assert!(took < CANCEL_GRACE + TERM_GRACE + Duration::from_secs(5), "{took:?}");
    assert!(stopped(&p.data().join("beat")));
    assert!(p.log_text().contains("[host] still running after cancel; ended\n"));
}

#[test]
fn a_plugin_that_hangs_after_its_result_is_ended() {
    let p = Plugin::new("linger");
    let started = Instant::now();
    assert_eq!(p.run().unwrap().as_deref(), Some("Done."));
    let took = started.elapsed();
    assert!(took >= EXIT_GRACE, "{took:?}");
    assert!(took < EXIT_GRACE + TERM_GRACE + Duration::from_secs(5), "{took:?}");
    assert!(p.log_text().contains("[host] still running after its result; ended\n"));
}

#[test]
fn its_stdin_closes_once_its_result_is_in() {
    let p = Plugin::new("eof");
    let started = Instant::now();
    assert_eq!(p.run().unwrap().as_deref(), Some("Closed."));
    // It saw its stdin end, with nothing after the run line, and exited.
    assert_eq!(fs::read_to_string(p.data().join("stdin-closed")).unwrap(), "");
    assert!(started.elapsed() < EXIT_GRACE / 2, "{:?}", started.elapsed());
}

#[test]
fn a_cancel_after_the_result_keeps_it_and_ends_the_plugin_at_once() {
    let p = Plugin::new("linger");
    // The run logs the line that counts before it looks at the cancel again.
    let (result, took) =
        p.run_cancelled_once(|| fs::read_to_string(p.log()).is_ok_and(|log| log.contains("[result] Done.\n")));
    assert_eq!(result.unwrap().as_deref(), Some("Done."));
    assert!(took < EXIT_GRACE / 2, "{took:?}");
    let log = p.log_text();
    assert!(log.contains("[host] cancelled after its result; ended\n"), "{log}");
    // A plugin that said its last word isn't asked to stop.
    assert!(!log.contains("asked to cancel"), "{log}");
}

#[test]
fn a_run_cancelled_before_it_starts_starts_nothing() {
    let p = Plugin::new("result");
    let cancel = CancelToken::new();
    cancel.cancel();
    let result = runtime::run(p.spec(), &cancel, &|_, _| {});
    assert!(matches!(result, Err(PluginError::Cancelled)), "{result:?}");
    assert!(!p.data().exists() && !p.log().exists());
}

#[test]
fn an_endless_line_is_cut_at_the_limit() {
    let p = Plugin::new("long");
    assert_eq!(p.run().unwrap().as_deref(), Some("Long."));
    let log = p.log_text();
    assert!(log.contains(&format!("\n[stdout] {} [cut]\n", "x".repeat(MAX_LINE_BYTES))), "{}", log.len());
    assert!(log.contains(&format!("\n[stderr] {} [cut]\n", "z".repeat(MAX_LINE_BYTES))), "{}", log.len());
}

#[test]
fn stage_text_is_cleaned_and_capped() {
    let p = Plugin::new("stages");
    let stages = Mutex::new(Vec::new());
    let result = runtime::run(p.spec(), &CancelToken::new(), &|stage, fraction| {
        stages.lock().unwrap().push((stage.to_owned(), fraction));
    });
    let message = result.unwrap().unwrap();
    assert_eq!(message.chars().count(), 300);
    assert!(message.starts_with("word word ") && message.ends_with('…') && !message.contains("  "), "{message}");
    let long = format!("{}…", "a".repeat(79));
    assert_eq!(
        stages.into_inner().unwrap(),
        [
            ("Posting clips to Discord".to_owned(), Some(0.5)),
            (long, Some(1.0)),
            (NAME.to_owned(), Some(0.0)),
            (NAME.to_owned(), None),
        ]
    );
}

#[test]
fn a_missing_program_says_so() {
    let p = Plugin::new("result");
    let mut spec = p.spec();
    spec.launch = Launch::Exe { program: "bin/gone.exe".into(), args: Vec::new() };
    let e = runtime::run(spec, &CancelToken::new(), &|_, _| {}).unwrap_err();
    assert_eq!(e.to_string(), format!("{NAME} can't start: one of its files is missing."));
    // Nothing started, so nothing was made.
    assert!(!p.data().exists() && !p.log().exists());
}

#[test]
fn a_call_gets_its_reply_on_stdin() {
    let answers = Answers::open();
    let p = Plugin::calling("call", &answers);
    assert_eq!(p.run().unwrap().as_deref(), Some("Called."));
    assert_eq!(p.text("reply.json"), "{\"type\":\"reply\",\"id\":7,\"result\":{\"method\":\"settings.get\"}}\n");
    assert_eq!(*answers.seen.lock().unwrap(), [HostMethod::SettingsGet]);
    // The run speaks the protocol its plugin declared.
    assert!(p.text("run.json").starts_with(r#"{"type":"run","protocol":2,"#));
    // The log names the call and its answer, never what it carried.
    let log = p.log_text();
    assert!(log.contains("[call] settings.get\n[reply] settings.get ok\n"), "{log}");
    assert!(!log.contains("method\":"), "{log}");
}

#[test]
fn calls_past_four_at_a_time_wait() {
    let answers = Answers::closed();
    let mut p = Plugin::calling("call-many", &answers);
    let n = MAX_CALLS_IN_FLIGHT + CALL_QUEUE + 2;
    p.args = vec![n.to_string()];
    let busy = "[reply] settings.get busy\n";
    // Four are answered at once and 64 wait: the two past them are busy.
    let full = || {
        answers.running.load(Ordering::SeqCst) == MAX_CALLS_IN_FLIGHT
            && fs::read_to_string(p.log()).is_ok_and(|log| log.matches(busy).count() == 2)
    };
    let (result, was_full) = thread::scope(|s| {
        let run = s.spawn(|| p.run());
        let until = Instant::now() + Duration::from_secs(20);
        while !full() && Instant::now() < until {
            thread::sleep(Duration::from_millis(20));
        }
        // Let through before anything is asserted: the run waits for it.
        let was_full = full();
        answers.let_through();
        (run.join().unwrap(), was_full)
    });
    assert!(was_full, "the calls never filled up");
    assert_eq!(result.unwrap().as_deref(), Some("Called."));
    assert_eq!(answers.most.load(Ordering::SeqCst), MAX_CALLS_IN_FLIGHT);
    assert_eq!(answers.seen(), n - 2);
    let replies: Vec<Value> = p.text("replies.json").lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(replies.len(), n);
    // The busy ones come back first, before any that waited.
    for (reply, id) in replies[..2].iter().zip([n - 1, n]) {
        assert_eq!(
            *reply,
            json!({"type": "reply", "id": id, "error": {"code": "busy", "message": "Too many calls at once."}})
        );
    }
    // Then every other one, answered.
    let ids: BTreeSet<u64> = replies[2..].iter().map(|r| r["id"].as_u64().unwrap()).collect();
    assert_eq!(ids, (1..=n as u64 - 2).collect());
    assert!(replies[2..].iter().all(|r| r["result"] == json!({"method": "settings.get"})));
}

#[test]
fn a_call_after_the_result_is_dropped() {
    let answers = Answers::open();
    let p = Plugin::calling("call-late", &answers);
    assert_eq!(p.run().unwrap().as_deref(), Some("Done."));
    assert_eq!(answers.seen(), 0);
    // Its stdin closed with its result: nothing came after.
    assert_eq!(p.text("stdin-closed"), "");
    let log = p.log_text();
    assert!(log.contains("[call] settings.get after the result: dropped\n"), "{log}");
    assert!(!log.contains("[reply]"), "{log}");
}

#[test]
fn an_unknown_method_gets_an_error_reply() {
    let answers = Answers::open();
    let p = Plugin::calling("call-unknown", &answers);
    assert_eq!(p.run().unwrap().as_deref(), Some("Called."));
    assert_eq!(
        p.text("reply.json"),
        concat!(
            r#"{"type":"reply","id":"a","error":{"code":"invalid","#,
            r#""message":"AutoScene doesn't know the method \"files.delete\"."}}"#,
            "\n"
        )
    );
    assert_eq!(answers.seen(), 0);
    assert!(p.log_text().contains("[call] files.delete\n[reply] files.delete invalid\n"));
}

#[test]
fn a_protocol_1_plugin_s_call_line_goes_to_the_log() {
    let answers = Answers::open();
    // As on M1, even if the host had answers for it.
    let p = Plugin { calls: Some(answers.clone() as Arc<dyn HostCalls>), ..Plugin::new("protocol1-call") };
    assert_eq!(p.run().unwrap().as_deref(), Some("Done."));
    assert_eq!(answers.seen(), 0);
    assert_eq!(p.text("stdin-closed"), "");
    let log = p.log_text();
    assert!(log.contains("[stdout] {\"type\":\"call\",\"id\":1,\"method\":\"settings.get\",\"params\":{}}\n"), "{log}");
    assert!(!log.contains("[call]") && !log.contains("[reply]"), "{log}");
}

#[test]
fn a_slow_call_never_blocks_progress_lines() {
    // The answer waits for the progress line the plugin sends after its call.
    let answers = Answers::closed();
    let p = Plugin::calling("call", &answers);
    let stages = Mutex::new(Vec::new());
    let result = runtime::run(p.spec(), &CancelToken::new(), &|stage, _| {
        stages.lock().unwrap().push(stage.to_owned());
        if stage == "Waiting for AutoScene" {
            answers.let_through();
        }
    });
    assert_eq!(result.unwrap().as_deref(), Some("Called."));
    assert_eq!(stages.into_inner().unwrap(), ["Waiting for AutoScene"]);
    assert_eq!(p.text("reply.json"), "{\"type\":\"reply\",\"id\":7,\"result\":{\"method\":\"settings.get\"}}\n");
}

/// The replies a run of `p` wrote to `replies.json`, by their id.
fn replies(p: &Plugin) -> BTreeMap<u64, Value> {
    let lines = p.text("replies.json");
    lines.lines().map(|l| serde_json::from_str::<Value>(l).unwrap()).map(|r| (r["id"].as_u64().unwrap(), r)).collect()
}

#[test]
fn long_and_broken_calls_never_reach_the_log() {
    let answers = Answers::open();
    let p = Plugin::calling("call-long", &answers);
    assert_eq!(p.run().unwrap().as_deref(), Some("Called."));
    // A long call within its cap reached AutoScene; nothing else did.
    assert_eq!(*answers.seen.lock().unwrap(), [HostMethod::StateSet]);
    let invalid =
        |id: u64, message: &str| json!({"type": "reply", "id": id, "error": {"code": "invalid", "message": message}});
    assert_eq!(
        replies(&p),
        BTreeMap::from([
            (1, json!({"type": "reply", "id": 1, "result": {"method": "state.set"}})),
            (2, invalid(2, "A call is at most 5 MB.")),
            (4, invalid(4, "AutoScene couldn't read this call.")),
        ])
    );
    let log = p.log_text();
    for line in [
        "[call] state.set\n",
        "[reply] state.set ok\n",
        "[call] state.set too long\n[reply] state.set invalid\n",
        "[call] net.fetch too long, without an id to answer: dropped\n",
        "[call] state.get unreadable, without an id to answer: dropped\n",
        "[call] (unnamed) unreadable\n[reply] (unnamed) invalid\n",
    ] {
        assert!(log.contains(line), "{line}: {}", &log[..log.len().min(4000)]);
    }
    // Not a byte of their params.
    assert!(!log.contains("SECRET") && !log.contains("vvvv") && !log.contains("xxxx"), "{}", log.len());
    assert!(!log.contains("[stdout]"), "{}", &log[..log.len().min(4000)]);
}

#[test]
fn a_run_without_answers_refuses_its_calls() {
    let p = Plugin { protocol: 2, ..Plugin::new("call") };
    assert_eq!(p.run().unwrap().as_deref(), Some("Called."));
    assert_eq!(
        p.text("reply.json"),
        concat!(
            r#"{"type":"reply","id":7,"error":{"code":"invalid","#,
            r#""message":"AutoScene can't answer calls here."}}"#,
            "\n"
        )
    );
    assert!(p.log_text().contains("[call] settings.get\n[reply] settings.get invalid\n"));
}

#[test]
fn a_plugin_that_never_reads_its_replies_is_ended() {
    let answers = Answers::open();
    let mut p = Plugin::calling("call-deaf", &answers);
    // Far more than a pipe holds, and than it may leave unread.
    p.args = vec![(MAX_UNREAD * 5).to_string(), "60000".into()];
    let started = Instant::now();
    let e = p.run().unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(30), "{:?}", started.elapsed());
    assert!(matches!(&e, PluginError::Crashed { detail, .. } if detail.contains("unread")), "{e:?}");
    assert_eq!(e.to_string(), format!("{NAME} stopped unexpectedly."));
    assert!(p.log_text().contains("[host] its replies went unread; ended\n"));
}

#[test]
fn unread_replies_hold_its_calls_back() {
    // Each reply is 2 MB, and the plugin reads none of them.
    let reply = 2 << 20;
    let answers = Answers::padded(reply);
    let mut p = Plugin::calling("call-deaf", &answers);
    let n = 60;
    p.args = vec![n.to_string(), "2000".into()];
    assert_eq!(p.run().unwrap().as_deref(), Some("Deaf."));
    // Calls started only while the replies it hadn't read stayed under the cap.
    let seen = answers.seen();
    assert!((MAX_CALLS_IN_FLIGHT..=MAX_CALL_BYTES / reply + MAX_CALLS_IN_FLIGHT).contains(&seen), "{seen}");
    // The others were answered busy, or still waited at its result.
    let log = p.log_text();
    let ok = log.matches("[reply] settings.get ok\n").count();
    let busy = log.matches("[reply] settings.get busy\n").count();
    let dropped = log.matches("[call] settings.get still waiting at the result: dropped\n").count();
    assert_eq!((ok, ok + busy + dropped), (seen, n), "{ok} {busy} {dropped}");
}

#[test]
fn a_run_ends_only_after_its_calls() {
    // The call is still being answered when the plugin has gone.
    let answers = Answers::heeding();
    let p = Plugin::calling("call-exit", &answers);
    assert_eq!(p.run().unwrap(), None);
    assert_eq!((answers.done.load(Ordering::SeqCst), answers.running.load(Ordering::SeqCst)), (1, 0));
    // It was told the plugin had ended, and its answer went to the log.
    assert!(answers.saw_end.load(Ordering::SeqCst));
    let log = p.log_text();
    let end: Vec<_> = log.lines().rev().take(2).collect();
    assert_eq!(end[1], "[reply] settings.get cancelled", "{log}");
    assert!(end[0].starts_with("[host] exit "), "{log}");
}

#[test]
fn a_call_that_panics_is_answered_failed() {
    let mut p = Plugin::calling("call-many", &Arc::new(Panics));
    // More than may run at once: each panic still gives its place back.
    let n = MAX_CALLS_IN_FLIGHT + 2;
    p.args = vec![n.to_string()];
    assert_eq!(p.run().unwrap().as_deref(), Some("Called."));
    let failed = json!({"code": "failed", "message": "AutoScene couldn't answer this call."});
    let replies = replies(&p);
    assert_eq!(replies.len(), n);
    assert!(replies.values().all(|r| r["error"] == failed), "{replies:?}");
}

#[test]
fn a_cancel_ends_the_run_while_a_call_waits() {
    let answers = Answers::heeding();
    let p = Plugin::calling("call", &answers);
    let (result, took) = p.run_cancelled_once(|| answers.seen() == 1);
    assert!(matches!(result, Err(PluginError::Cancelled)), "{result:?}");
    assert!(took < CANCEL_GRACE, "{took:?}");
    // The cancel line reached it while its call waited.
    assert_eq!(p.text("reply.json"), "{\"type\":\"cancel\"}\n");
    assert!(answers.saw_end.load(Ordering::SeqCst));
    assert!(p.log_text().contains("[reply] settings.get cancelled\n"));
}

#[test]
fn a_call_after_the_cancel_is_refused() {
    let answers = Answers::heeding();
    let mut p = Plugin::calling("call-cancelled", &answers);
    let n = MAX_CALLS_IN_FLIGHT + 2;
    p.args = vec![n.to_string()];
    // Four being answered and two waiting their turn when the cancel comes.
    let (result, took) = p.run_cancelled_once(|| {
        answers.seen() == MAX_CALLS_IN_FLIGHT
            && fs::read_to_string(p.log()).is_ok_and(|log| log.matches("[call] settings.get\n").count() == n)
    });
    assert!(matches!(result, Err(PluginError::Cancelled)), "{result:?}");
    assert!(took < CANCEL_GRACE, "{took:?}");
    // Those being answered were told; nothing else reached AutoScene.
    assert_eq!(*answers.seen.lock().unwrap(), [HostMethod::SettingsGet; MAX_CALLS_IN_FLIGHT]);
    assert!(answers.saw_end.load(Ordering::SeqCst));
    // The cancel line came first; the two waiting and the one after it were
    // answered that the run was cancelled.
    let read = p.text("replies.json");
    assert_eq!(read.lines().next(), Some(r#"{"type":"cancel"}"#));
    let replies: BTreeMap<u64, Value> = read
        .lines()
        .skip(1)
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .map(|r| (r["id"].as_u64().unwrap(), r["error"].clone()))
        .collect();
    let cancelled = json!({"code": "cancelled", "message": "This run was cancelled."});
    for id in [n as u64 - 1, n as u64, 99] {
        assert_eq!(replies.get(&id), Some(&cancelled), "{id}: {read}");
    }
    let log = p.log_text();
    assert!(log.contains("[host] asked to cancel\n"), "{log}");
    assert!(log.contains("[call] state.set\n[reply] state.set cancelled\n"), "{log}");
}

#[test]
fn calls_still_waiting_at_the_result_are_never_answered() {
    let answers = Answers::heeding();
    let mut p = Plugin::calling("call-and-go", &answers);
    p.args = vec![(MAX_CALLS_IN_FLIGHT + 2).to_string()];
    assert_eq!(p.run().unwrap().as_deref(), Some("Gone."));
    // Four were being answered; the two waiting never started, and say so.
    assert_eq!(answers.seen(), MAX_CALLS_IN_FLIGHT);
    let log = p.log_text();
    assert_eq!(log.matches("[call] settings.get still waiting at the result: dropped\n").count(), 2, "{log}");
    assert_eq!(log.matches("[reply] settings.get cancelled\n").count(), MAX_CALLS_IN_FLIGHT, "{log}");
}

/// What a run in a `scratch` mode said of its scratch folder.
fn scratch_said(p: &Plugin) -> Value {
    serde_json::from_str(&p.text("scratch.json")).unwrap()
}

/// The temp folder a run in a `scratch` mode was given.
fn scratch_of(p: &Plugin) -> PathBuf {
    PathBuf::from(scratch_said(p)["tempDir"].as_str().expect("a scratch folder"))
}

/// The scratch folder of the run's temp folder `dir`, with its marker.
fn root_of(dir: &Path) -> PathBuf {
    assert_eq!(dir.file_name().unwrap(), scratch::TEMP, "{}", dir.display());
    dir.parent().unwrap().to_owned()
}

/// Waits until nothing is at `path` any more: a removal runs on a thread of
/// its own, and is retried for about ten seconds.
fn wait_gone(path: &Path) {
    let until = Instant::now() + Duration::from_secs(30);
    while fs::symlink_metadata(path).is_ok() {
        assert!(Instant::now() < until, "{} is still there", path.display());
        thread::sleep(Duration::from_millis(20));
    }
}

/// Runs `p` in `scratch-sleep` (protocol 2), does `during` with its temp
/// folder once it wrote its files there, and cancels it: the result, and the
/// folder.
fn run_scratch_sleep(p: &Plugin, during: impl FnOnce(&Path)) -> (PluginResult<Option<String>>, PathBuf) {
    run_scratch_sleep_in(p, &env::temp_dir(), during)
}

/// [`run_scratch_sleep`], with its scratch folder made in `temp`.
fn run_scratch_sleep_in(
    p: &Plugin,
    temp: &Path,
    during: impl FnOnce(&Path),
) -> (PluginResult<Option<String>>, PathBuf) {
    let cancel = CancelToken::new();
    thread::scope(|s| {
        let run = s.spawn(|| scratch::with_temp_root(temp, || runtime::run(p.spec(), &cancel, &|_, _| {})));
        wait_for(|| p.data().join("scratch.json").exists());
        let dir = scratch_of(p);
        during(&dir);
        cancel.cancel();
        (run.join().unwrap(), dir)
    })
}

/// A plugin of protocol 2 in `mode`, which answers no calls.
fn scratching(mode: &'static str) -> Plugin {
    Plugin { protocol: 2, ..Plugin::new(mode) }
}

#[test]
fn a_run_gets_a_scratch_folder_and_it_goes_after() {
    let p = scratching("scratch");
    assert_eq!(p.run().unwrap().as_deref(), Some("Scratched."));
    let dir = scratch_of(&p);
    // A folder of its own in the system's temp folder, named short, and in
    // it the run's temp folder.
    let root = root_of(&dir);
    assert_eq!(root.parent(), Some(env::temp_dir().as_path()));
    let name = root.file_name().unwrap().to_str().unwrap();
    assert!(name.starts_with(scratch::PREFIX) && name.len() == scratch::PREFIX.len() + scratch::NAME_CHARS, "{name}");
    // It wrote its files there, and they went with the folder once the run ended.
    wait_gone(&root);
    // Its data folder keeps what it wrote there.
    assert!(p.data().join("scratch.json").is_file());
    // Each run gets a folder of its own.
    let again = scratching("scratch");
    again.run().unwrap();
    assert_ne!(scratch_of(&again), dir);
}

#[test]
fn a_stopped_run_s_scratch_folder_goes_too() {
    let p = scratching("scratch-sleep");
    let (result, dir) = run_scratch_sleep(&p, |dir| {
        // It lives while its run does, with what the plugin wrote, and the
        // marker beside it, out of the plugin's way.
        assert_eq!(fs::read_to_string(dir.join("sub").join("b.txt")).unwrap(), "b");
        assert!(root_of(dir).join(scratch::MARKER).is_file());
        assert!(!dir.join(scratch::MARKER).exists());
    });
    assert!(matches!(result, Err(PluginError::Cancelled)), "{result:?}");
    wait_gone(&root_of(&dir));
}

#[test]
fn temp_points_into_the_scratch_folder() {
    let p = scratching("scratch");
    p.run().unwrap();
    let said = scratch_said(&p);
    let dir = said["tempDir"].as_str().unwrap();
    assert_eq!(said["runTemp"], json!(dir));
    // The program's own temp folder is it: std's, Python's tempfile, FFmpeg's
    // (Windows ends it with a separator).
    assert_eq!(Path::new(said["systemTemp"].as_str().unwrap()), Path::new(dir));
    #[cfg(windows)]
    assert_eq!((&said["TEMP"], &said["TMP"]), (&json!(dir), &json!(dir)));
    #[cfg(unix)]
    assert_eq!(said["TMPDIR"], json!(dir));
    // The run line names it beside the data folder.
    let line: Value = serde_json::from_str(&p.text("run.json")).unwrap();
    assert_eq!(line["plugin"]["dataDir"], json!(p.data()));
    assert_eq!(line["plugin"]["tempDir"], json!(dir));
}

/// Asserts that the environment `said` gives the temp folder the app has, as
/// a run of protocol 1 gets it.
fn assert_system_temp(said: &Value) {
    for name in ["TEMP", "TMP", "TMPDIR"] {
        let ours = env::var(name).ok().filter(|_| runtime::allowed_env(name.as_ref()));
        assert_eq!(said[name], json!(ours), "{name}");
    }
}

#[test]
fn a_protocol_1_run_gets_its_line_and_environment_as_before() {
    let p = Plugin::new("scratch");
    assert_eq!(p.run().unwrap().as_deref(), Some("Scratched."));
    let said = scratch_said(&p);
    assert_eq!((&said["tempDir"], &said["runTemp"]), (&Value::Null, &Value::Null));
    assert_system_temp(&said);
    assert!(!p.text("run.json").contains("tempDir"));
}

#[test]
fn a_run_whose_scratch_folder_can_t_be_made_keeps_the_system_s_temp() {
    // A system temp folder that is gone, as when TEMP names a deleted folder.
    let gone = tempfile::tempdir().unwrap();
    let p = scratching("scratch");
    let result = scratch::with_temp_root(&gone.path().join("deleted"), || p.run());
    // It runs as a run of protocol 1 would, and its log says why.
    assert_eq!(result.unwrap().as_deref(), Some("Scratched."));
    let said = scratch_said(&p);
    assert_eq!((&said["tempDir"], &said["runTemp"]), (&Value::Null, &Value::Null));
    assert_system_temp(&said);
    assert!(!p.text("run.json").contains("tempDir"));
    let log = p.log_text();
    assert!(log.contains("[host] couldn't make its temp folder, so it keeps the system's: "), "{log}");
}

#[test]
fn a_link_in_the_scratch_folder_is_removed_not_followed() {
    let victim = tempfile::tempdir().unwrap();
    let (keep, keep2) = (victim.path().join("keep.txt"), victim.path().join("keep2.txt"));
    fs::write(&keep, "keep\n").unwrap();
    fs::write(&keep2, "keep2\n").unwrap();
    let hash = |path: &Path| as_plugin::package::sha256_hex(&fs::read(path).unwrap());
    let hashes = (hash(&keep), hash(&keep2));
    let p = scratching("scratch-sleep");
    let (_, dir) = run_scratch_sleep(&p, |dir| {
        // A link to the folder at its top and deeper, a hard link to a file,
        // a link to a file (Unix: Windows makes those only in developer
        // mode), and a read-only file.
        link_dir(&dir.join("j"), victim.path());
        link_dir(&dir.join("sub").join("j2"), victim.path());
        fs::hard_link(&keep2, dir.join("hard.txt")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&keep, dir.join("f")).unwrap();
        fs::write(dir.join("ro.txt"), "ro").unwrap();
        let mut ro = fs::metadata(dir.join("ro.txt")).unwrap().permissions();
        ro.set_readonly(true);
        fs::set_permissions(dir.join("ro.txt"), ro).unwrap();
    });
    wait_gone(&root_of(&dir));
    let mut left: Vec<_> = fs::read_dir(victim.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
    left.sort();
    assert_eq!(left, ["keep.txt", "keep2.txt"]);
    assert_eq!((hash(&keep), hash(&keep2)), hashes);
}

#[test]
fn a_scratch_folder_swapped_for_a_link_removes_only_the_link() {
    // What a link could lead to: a folder with a file, and in it one named
    // as the run's temp folder, with a file too.
    let victim = tempfile::tempdir().unwrap();
    let inner = victim.path().join(scratch::TEMP);
    fs::create_dir(&inner).unwrap();
    fs::write(victim.path().join("keep.txt"), "keep").unwrap();
    fs::write(inner.join("keep.txt"), "keep").unwrap();
    let p = scratching("scratch-sleep");
    let moved: Mutex<Option<PathBuf>> = Mutex::new(None);
    let (_, dir) = run_scratch_sleep(&p, |dir| {
        let root = root_of(dir);
        let aside = root.with_file_name(format!("{}-moved", root.file_name().unwrap().to_str().unwrap()));
        let renamed = fs::rename(&root, &aside);
        // Windows keeps a folder in place while a file in it is open: the
        // run's marker holds it while the run lives and its removal runs.
        // The run's temp folder can move, and a link takes its place.
        #[cfg(windows)]
        {
            assert!(renamed.is_err(), "the scratch folder moved while its run held it");
            fs::rename(dir, dir.with_file_name("moved")).unwrap();
            link_dir(dir, victim.path());
        }
        #[cfg(unix)]
        {
            renamed.unwrap();
            std::os::unix::fs::symlink(victim.path(), &root).unwrap();
            *moved.lock().unwrap() = Some(aside);
        }
    });
    wait_gone(dir.parent().unwrap());
    assert_eq!(fs::read_to_string(victim.path().join("keep.txt")).unwrap(), "keep");
    assert_eq!(fs::read_to_string(inner.join("keep.txt")).unwrap(), "keep");
    if let Some(away) = moved.into_inner().unwrap() {
        fs::remove_dir_all(away).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn a_socket_fits_in_the_scratch_folder() {
    let p = scratching("scratch-sleep");
    let (_, dir) = run_scratch_sleep(&p, |dir| {
        // Where Python's multiprocessing puts its sockets, under TMPDIR.
        let pymp = dir.join("pymp-k3j9x0qa");
        fs::create_dir(&pymp).unwrap();
        let _listener = std::os::unix::net::UnixListener::bind(pymp.join("sock-x0qak3j9")).unwrap();
        // CPython keeps TMPDIR only while its path and the 28 bytes of
        // those two names fit (multiprocessing/util.py).
        let limit = if cfg!(target_os = "macos") { 92 } else { 108 };
        assert!(dir.as_os_str().len() + 28 < limit, "{}", dir.display());
    });
    wait_gone(&root_of(&dir));
}

#[test]
fn a_held_file_is_retried_then_left_to_the_sweep() {
    // A temp folder of its own: the sweep below takes only what this test left.
    let temp = tempfile::tempdir().unwrap();
    // A grandchild held a file in it when the plugin exited: once the run
    // ended its tree, the folder went (on Windows, after a try or two while
    // the grandchild ended).
    let p = scratching("scratch-hold");
    let result = scratch::with_temp_root(temp.path(), || p.run());
    assert_eq!(result.unwrap().as_deref(), Some("Held."));
    wait_gone(&root_of(&scratch_of(&p)));
    held_past_its_tries(temp.path());
}

/// On Windows: a file held while the run's folder is removed keeps it,
/// until the file is let go. Let go while it is retried, the folder goes;
/// held past every try, the folder stays with its marker, for the sweep of
/// the next start.
#[cfg(windows)]
fn held_past_its_tries(temp: &Path) {
    let waits: Duration = scratch::RETRY_WAITS.iter().sum();
    let p = scratching("scratch-sleep");
    let hold = Mutex::new(None);
    let (result, dir) = run_scratch_sleep_in(&p, temp, |dir| *hold.lock().unwrap() = Some(Hold::new(dir)));
    assert!(matches!(result, Err(PluginError::Cancelled)), "{result:?}");
    let root = root_of(&dir);
    // Its first try came as the run ended; the next comes half a second on.
    // Meanwhile the run's marker is held: no sweep takes the folder.
    thread::sleep(scratch::RETRY_WAITS[0] * 2);
    assert!(held(&root.join(scratch::MARKER)) && dir.join("held.bin").is_file());
    drop(hold.lock().unwrap().take());
    let until = Instant::now() + waits;
    while fs::symlink_metadata(&root).is_ok() {
        assert!(Instant::now() < until, "{} stayed after its file was let go", root.display());
        thread::sleep(Duration::from_millis(20));
    }

    let p = scratching("scratch-sleep");
    let (result, dir) = run_scratch_sleep_in(&p, temp, |dir| *hold.lock().unwrap() = Some(Hold::new(dir)));
    assert!(matches!(result, Err(PluginError::Cancelled)), "{result:?}");
    let root = root_of(&dir);
    // Its marker is held while it is tried, and let go once the tries are done.
    let until = Instant::now() + waits + Duration::from_secs(10);
    while held(&root.join(scratch::MARKER)) {
        assert!(Instant::now() < until, "{} is still being removed", root.display());
        thread::sleep(Duration::from_millis(50));
    }
    assert!(root.join(scratch::MARKER).is_file() && dir.join("held.bin").is_file());
    drop(hold);
    assert_eq!(scratch::sweep(temp), 1);
    assert!(fs::symlink_metadata(&root).is_err(), "{}", root.display());
}

/// On Linux and macOS nothing holds a folder of the person's: an open file
/// doesn't, and folders a plugin left read only (the run's temp folder too)
/// are given leave to change them, so the folder goes without the sweep.
#[cfg(unix)]
fn held_past_its_tries(temp: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let p = scratching("scratch-sleep");
    let (result, dir) = run_scratch_sleep_in(&p, temp, |dir| {
        let locked = dir.join("locked");
        fs::create_dir(&locked).unwrap();
        fs::write(locked.join("inside.bin"), "inside").unwrap();
        for dir in [locked.as_path(), dir] {
            fs::set_permissions(dir, fs::Permissions::from_mode(0o500)).unwrap();
        }
    });
    assert!(matches!(result, Err(PluginError::Cancelled)), "{result:?}");
    wait_gone(&root_of(&dir));
}

/// Whether `marker` is there and held: this process can't lock it.
#[cfg(windows)]
fn held(marker: &Path) -> bool {
    fs::File::open(marker).is_ok_and(|f| matches!(f.try_lock(), Err(fs::TryLockError::WouldBlock)))
}

/// What keeps a folder from being removed on Windows: a file in it opened
/// without leave to delete it, until it drops.
#[cfg(windows)]
struct Hold {
    _file: fs::File,
}

#[cfg(windows)]
impl Hold {
    fn new(folder: &Path) -> Self {
        use std::os::windows::fs::OpenOptionsExt;
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .share_mode(0)
            .open(folder.join("held.bin"))
            .unwrap();
        Self { _file: file }
    }
}

/// A link at `link` to the folder `target`: a junction on Windows, where a
/// symlink would need developer mode.
fn link_dir(link: &Path, target: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    {
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
    }
}

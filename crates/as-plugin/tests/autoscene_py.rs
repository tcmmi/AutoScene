//! `templates/autoscene.py`, the helper `asp init` writes beside a plugin's
//! Python program, `templates/main.py`, that program, and the Discord
//! example's program, which uses the helper: each run as AutoScene runs a
//! program, and spoken to as AutoScene speaks to it. What they write must
//! read as AutoScene reads a program's lines, and what AutoScene writes as
//! protocol 2 says. They need a Python 3.10 or later on the PATH, and are
//! skipped, with a word, without one.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use as_plugin::calls::{CallCode, CallError};
use as_plugin::protocol::{
    ActionContext, ActionKind, AppContext, Call, CallId, ClipContext, ExportContext, HostMessage, LogLevel,
    PluginContext, PluginMessage, Reply, Request, RequestContext, RunMessage, ServeMessage, ServiceMessage, ToolPaths,
    Trigger,
};
use as_plugin::{PluginId, SettingValue};
use serde_json::{Value, json};
use tempfile::TempDir;

const TEMPLATES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/templates");
/// How long a program may take to say its next line, or to end.
const WAIT: Duration = Duration::from_secs(20);

/// Starts a script as uv does, with its own folder on Python's path, which
/// `python -I` leaves off: `run.py <folder> <script>`.
const RUN: &str =
    "import runpy, sys\nsys.path.insert(0, sys.argv[1])\nrunpy.run_path(sys.argv[2], run_name='__main__')\n";

/// A program of the helper's that answers as the tests below ask it.
const PROGRAM: &str = r#"import autoscene

p = autoscene.start()
if p.mode == "run" and p.first["action"]["id"] == "post":
    hook = p.call("secrets.get", {"key": "webhook"})["value"]
    p.progress(0.5, "Posting")
    try:
        p.call("project.get")
    except autoscene.CallError as e:
        p.log(f"{e.code}: {e}", "warn")
    p.result(f"Posted {p.first['input']['words']} words to {hook}.")
elif p.mode == "run" and p.first["action"]["id"] == "wait":
    p.log("Waiting.")
    p.cancelled.wait(30)
    p.error("Cancelled.", "It was asked to stop.")
elif p.mode == "run":
    try:
        p.call("settings.get")
    except autoscene.CallError as e:
        p.error(str(e), e.code)
else:

    @p.handle("count")
    def count(params, context):
        p.emit("counted", {"page": context["page"]})
        return {"words": len(params["text"].split())}

    @p.handle("settings")
    def settings(params, context):
        return p.settings

    @p.handle("ask")
    def ask(params, context):
        return p.call("settings.get")

    @p.handle("fail")
    def fail(params, context):
        raise ValueError("No text.")

    @p.handle("nan")
    def nan(params, context):
        return float("nan")

    p.serve()
    p.log("Stopped.")
"#;

/// A Python of 3.10 or later on the PATH, if there is one. Windows may
/// have a `python3` that only offers to install one from the Store.
fn python() -> Option<&'static str> {
    ["python3", "python"].into_iter().find(|python| {
        Command::new(python)
            .args(["-I", "-c", "import sys; sys.exit(sys.version_info < (3, 10))"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    })
}

/// A folder with the helper, `run.py`, and `files`.
fn folder(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let helper = read_text(&Path::new(TEMPLATES).join("autoscene.py"));
    for (name, text) in [("autoscene.py", helper.as_str()), ("run.py", RUN)].iter().chain(files) {
        std::fs::write(dir.path().join(name), text).unwrap();
    }
    dir
}

fn read_text(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

/// A Python program, spoken to as AutoScene speaks to a plugin's program.
struct Program {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
}

impl Program {
    fn start(python: &str, dir: &Path, script: &str) -> Self {
        let mut child = Command::new(python)
            .arg("-I")
            .arg(dir.join("run.py"))
            .arg(dir)
            .arg(dir.join(script))
            .current_dir(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // Its lines come on a thread of their own, so a program that hangs
        // fails the test instead of holding it.
        let (tx, lines) = mpsc::channel();
        let stdout = child.stdout.take().unwrap();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if tx.send(line.unwrap()).is_err() {
                    break;
                }
            }
        });
        Self { stdin: child.stdin.take(), child, lines }
    }

    fn send(&mut self, message: HostMessage) {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{}", message.line()).unwrap();
        stdin.flush().unwrap();
    }

    fn reply(&mut self, call: &Call, result: Option<Value>, error: Option<CallError>) {
        self.send(HostMessage::Reply(Reply { id: call.id.clone(), result, error }));
    }

    fn line(&self) -> String {
        self.lines.recv_timeout(WAIT).unwrap_or_else(|_| panic!("the program said nothing in {WAIT:?}"))
    }

    /// Its next line, which must be a run's message.
    fn message(&self) -> PluginMessage {
        let line = self.line();
        PluginMessage::read(&line).unwrap_or_else(|| panic!("not a message: {line}"))
    }

    /// Its next line, which must be a call: its id, method and params.
    fn call(&self) -> (Call, Value) {
        let line = self.line();
        let call = Call::read(&line).unwrap_or_else(|| panic!("not a call: {line}"));
        let params = serde_json::from_str(call.params.get()).unwrap();
        (call, params)
    }

    /// Its next line, which must be a service's message.
    fn service(&self) -> ServiceMessage {
        let line = self.line();
        ServiceMessage::read(&line).unwrap_or_else(|| panic!("not a service's message: {line}"))
    }

    /// Closes its stdin, as AutoScene does once a run said its result, and
    /// waits for it to end: how it ended, after it said nothing more.
    fn end(mut self) -> ExitStatus {
        drop(self.stdin.take());
        let start = Instant::now();
        let status = loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                break status;
            }
            if start.elapsed() > WAIT {
                self.child.kill().unwrap();
                panic!("the program didn't end in {WAIT:?}");
            }
            thread::sleep(Duration::from_millis(10));
        };
        let mut stderr = String::new();
        self.child.stderr.take().unwrap().read_to_string(&mut stderr).unwrap();
        let more: Vec<String> = self.lines.try_iter().collect();
        assert_eq!(more, Vec::<String>::new(), "it said more; its stderr: {stderr}");
        status
    }
}

fn plugin() -> PluginContext {
    PluginContext {
        id: PluginId::parse("dev.example.hello").unwrap(),
        version: "0.1.0".into(),
        data_dir: ".".into(),
        resources_dir: None,
    }
}

fn app() -> AppContext {
    AppContext { version: "1.1.0".into(), platform: "linux-x86_64".into() }
}

fn tools() -> ToolPaths {
    ToolPaths { ffmpeg: "ffmpeg".into(), ffprobe: "ffprobe".into(), ffmpeg_version: "9.0".into() }
}

/// The run line of a protocol 2 program, for the action `id`.
fn run(kind: ActionKind, id: &str, trigger: Trigger) -> RunMessage {
    RunMessage {
        protocol: 2,
        run_id: "run-1".into(),
        plugin: plugin(),
        app: app(),
        tools: tools(),
        settings: BTreeMap::new(),
        action: ActionContext { kind, id: id.into(), trigger },
        project: None,
        export: None,
        input: None,
    }
}

fn serve(settings: BTreeMap<String, SettingValue>) -> HostMessage {
    let serve = ServeMessage {
        protocol: 2,
        service_id: "service-1".into(),
        plugin: plugin(),
        app: app(),
        tools: tools(),
        settings,
    };
    HostMessage::Serve(Box::new(serve))
}

fn request(id: &str, method: &str, params: Value) -> HostMessage {
    let context = RequestContext { page: "main".into(), project_id: None };
    HostMessage::Request(Request { id: id.into(), method: method.into(), params, context })
}

fn response(id: &str, result: Value) -> ServiceMessage {
    ServiceMessage::Response { id: id.into(), result, error: None }
}

/// The message of a response that failed, and its detail.
fn fault(m: ServiceMessage) -> (String, String) {
    let ServiceMessage::Response { result: Value::Null, error: Some(fault), .. } = m else { panic!("{m:?}") };
    (fault.message, fault.detail.unwrap_or_default())
}

#[test]
fn the_python_helper_speaks_protocol_2() {
    let Some(python) = python() else {
        eprintln!("skipped: the_python_helper_speaks_protocol_2 needs a Python 3.10 or later on the PATH");
        return;
    };
    let dir = folder(&[("plugin.py", PROGRAM)]);

    // A run from a page: its input, two calls (one refused), progress, a
    // log line and its result.
    let mut p = Program::start(python, dir.path(), "plugin.py");
    let mut message = run(ActionKind::Command, "post", Trigger::Page);
    message.input = Some(json!({"words": 3}));
    p.send(HostMessage::Run(Box::new(message)));
    let (call, params) = p.call();
    assert_eq!(
        (&call.id, call.method.as_str(), params),
        (&CallId::Number(1), "secrets.get", json!({"key": "webhook"}))
    );
    p.reply(&call, Some(json!({"value": "https://hooks.example"})), None);
    assert_eq!(p.message(), PluginMessage::Progress { fraction: Some(0.5), stage: Some("Posting".into()) });
    let (call, params) = p.call();
    assert_eq!((&call.id, call.method.as_str(), params), (&CallId::Number(2), "project.get", json!({})));
    let denied = CallError::new(CallCode::Denied, "project.get needs the project:read permission.");
    p.reply(&call, None, Some(denied));
    let said = "denied: project.get needs the project:read permission.".into();
    assert_eq!(p.message(), PluginMessage::Log { level: LogLevel::Warn, message: said });
    let posted = Some("Posted 3 words to https://hooks.example.".into());
    assert_eq!(p.message(), PluginMessage::Result { message: posted });
    assert!(p.end().success());

    // A run AutoScene cancels.
    let mut p = Program::start(python, dir.path(), "plugin.py");
    p.send(HostMessage::Run(Box::new(run(ActionKind::ExportAction, "wait", Trigger::Menu))));
    assert_eq!(p.message(), PluginMessage::Log { level: LogLevel::Info, message: "Waiting.".into() });
    p.send(HostMessage::Cancel);
    let cancelled = PluginMessage::Error { message: "Cancelled.".into(), detail: Some("It was asked to stop.".into()) };
    assert_eq!(p.message(), cancelled);
    assert!(p.end().success());

    // A call AutoScene ends the run before it answers: it fails at once,
    // not after its 120 seconds.
    let mut p = Program::start(python, dir.path(), "plugin.py");
    p.send(HostMessage::Run(Box::new(run(ActionKind::Command, "late", Trigger::Palette))));
    let (call, _) = p.call();
    assert_eq!(call.method, "settings.get");
    drop(p.stdin.take());
    let late =
        PluginMessage::Error { message: "AutoScene didn't answer settings.get.".into(), detail: Some("failed".into()) };
    assert_eq!(p.message(), late);
    assert!(p.end().success());

    // A service: ready, then each request answered by its id, in turn,
    // with what it emits, the settings as they are now, a call of its
    // own, and a fault for what fails; then it stops when told.
    let mut p = Program::start(python, dir.path(), "plugin.py");
    p.send(serve(BTreeMap::from([("loud".into(), SettingValue::Bool(false))])));
    assert_eq!(p.service(), ServiceMessage::Ready);
    p.send(request("r1", "count", json!({"text": "one two  three"})));
    assert_eq!(p.service(), ServiceMessage::Emit { event: "counted".into(), data: json!({"page": "main"}) });
    assert_eq!(p.service(), response("r1", json!({"words": 3})));
    p.send(HostMessage::Settings { settings: BTreeMap::from([("loud".into(), SettingValue::Bool(true))]) });
    p.send(request("r2", "settings", Value::Null));
    assert_eq!(p.service(), response("r2", json!({"loud": true})));
    p.send(request("r3", "ask", Value::Null));
    let (call, params) = p.call();
    assert_eq!((call.method.as_str(), params), ("settings.get", json!({})));
    p.reply(&call, Some(json!({"loud": true, "count": 3})), None);
    assert_eq!(p.service(), response("r3", json!({"loud": true, "count": 3})));
    p.send(request("r4", "fail", json!({})));
    let (message, detail) = fault(p.service());
    assert_eq!(message, "No text.");
    assert!(detail.contains("ValueError: No text."), "{detail}");
    p.send(request("r5", "nan", json!({})));
    assert_eq!(fault(p.service()).0, "nan gave an answer AutoScene can't read.");
    p.send(request("r6", "later", json!({})));
    assert_eq!(fault(p.service()), ("No method later.".into(), String::new()));
    p.send(HostMessage::Stop);
    assert_eq!(p.message(), PluginMessage::Log { level: LogLevel::Info, message: "Stopped.".into() });
    assert!(p.end().success());
}

#[test]
fn the_template_program_runs_and_serves() {
    let Some(python) = python() else {
        eprintln!("skipped: the_template_program_runs_and_serves needs a Python 3.10 or later on the PATH");
        return;
    };
    let dir = folder(&[("main.py", &read_text(&Path::new(TEMPLATES).join("main.py")))]);

    // Its export action, on an export that wrote two of its three clips.
    let mut p = Program::start(python, dir.path(), "main.py");
    let clip = |name: &str, path: Option<&str>| ClipContext {
        name: name.into(),
        path: path.map(Into::into),
        status: if path.is_some() { "done" } else { "failed" }.into(),
        media_id: "m1".into(),
        source_path: "/shows/s01e01.mkv".into(),
        episode: "S01E01".into(),
        start: 0,
        end: 24,
        fps: Some("24000/1001".into()),
    };
    let mut message = run(ActionKind::ExportAction, "count-clips", Trigger::Menu);
    message.export = Some(ExportContext {
        id: "e1".into(),
        label: None,
        preset: "fast".into(),
        out_dir: "/out".into(),
        created_at: 0,
        merged_file: None,
        clips: vec![clip("a", Some("/out/a.mp4")), clip("b", None), clip("c", Some("/out/c.mp4"))],
    });
    p.send(HostMessage::Run(Box::new(message)));
    assert_eq!(p.message(), PluginMessage::Progress { fraction: None, stage: Some("Counting the clips".into()) });
    assert_eq!(p.message(), PluginMessage::Result { message: Some("The export wrote 2 clips.".into()) });
    assert!(p.end().success());

    // Run without an export, it says where to run it.
    let mut p = Program::start(python, dir.path(), "main.py");
    p.send(HostMessage::Run(Box::new(run(ActionKind::Command, "count-clips", Trigger::Palette))));
    let error = PluginMessage::Error { message: "Run it from an export's menu.".into(), detail: None };
    assert_eq!(p.message(), error);
    assert_eq!(p.end().code(), Some(1));

    // Its service counts the words of what a page sends.
    let mut p = Program::start(python, dir.path(), "main.py");
    p.send(serve(BTreeMap::new()));
    assert_eq!(p.service(), ServiceMessage::Ready);
    p.send(request("r1", "count", json!({"text": "a b c d"})));
    assert_eq!(p.service(), response("r1", json!({"words": 4})));
    p.send(request("r2", "count", Value::Null));
    assert_eq!(fault(p.service()).0, "Give it some text to count.");
    p.send(HostMessage::Stop);
    assert!(p.end().success());
}

#[test]
fn the_examples_have_the_helper_asp_init_writes() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/plugins");
    for name in ["discord-notify", "hello-service"] {
        let example = examples.join(name).join("autoscene.py");
        assert_eq!(read_text(&example), read_text(&Path::new(TEMPLATES).join("autoscene.py")), "{name}");
    }
}

/// A run of the Discord example (`examples/plugins/discord-notify`) for the
/// action `id`, with the settings a person left alone.
fn discord(kind: ActionKind, id: &str) -> RunMessage {
    let mut message = run(kind, id, Trigger::Menu);
    message.settings = BTreeMap::from([
        ("listClips".into(), SettingValue::Bool(true)),
        ("username".into(), SettingValue::Text("AutoScene".into())),
    ]);
    message
}

#[test]
fn the_discord_example_reads_its_secret_and_posts_through_autoscene() {
    let Some(python) = python() else {
        eprintln!("skipped: the Discord example's test needs a Python 3.10 or later on the PATH");
        return;
    };
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/plugins/discord-notify/main.py");
    let dir = folder(&[("main.py", &read_text(&example))]);
    let hook = "https://discord.com/api/webhooks/1/abc";
    let secret = |p: &mut Program, value: Value| {
        let (call, params) = p.call();
        assert_eq!((call.method.as_str(), params), ("secrets.get", json!({"key": "webhookUrl"})));
        p.reply(&call, Some(json!({ "value": value })), None);
    };

    // Its export action: the webhook from the keychain, then one post
    // through net.fetch, with the export's clips and no mentions.
    let mut p = Program::start(python, dir.path(), "main.py");
    let mut message = discord(ActionKind::ExportAction, "post");
    let clip = |name: &str| ClipContext {
        name: name.into(),
        path: Some(format!("/out/{name}.mp4")),
        status: "done".into(),
        media_id: "m1".into(),
        source_path: "/shows/s01e01.mkv".into(),
        episode: "S01E01".into(),
        start: 0,
        end: 24,
        fps: None,
    };
    message.export = Some(ExportContext {
        id: "e1".into(),
        label: Some("Walter · scenepack".into()),
        preset: "prores-hq".into(),
        out_dir: "/out".into(),
        created_at: 0,
        merged_file: None,
        clips: vec![clip("Walter_001"), clip("Walter_002")],
    });
    p.send(HostMessage::Run(Box::new(message)));
    secret(&mut p, json!(hook));
    assert_eq!(p.message(), PluginMessage::Progress { fraction: None, stage: Some("Posting to Discord".into()) });
    let (call, params) = p.call();
    assert_eq!(call.method, "net.fetch");
    assert_eq!(
        (&params["url"], &params["method"], &params["headers"]),
        (&json!(hook), &json!("POST"), &json!([["content-type", "application/json"]]))
    );
    let body: Value = serde_json::from_str(params["body"].as_str().unwrap()).unwrap();
    let content =
        "**Walter · scenepack** is done: 2 clips with prores-hq.\n- Walter\\_001 · S01E01\n- Walter\\_002 · S01E01";
    assert_eq!(body, json!({"content": content, "allowed_mentions": {"parse": []}, "username": "AutoScene"}));
    p.reply(&call, Some(json!({"status": 204, "url": hook, "headers": [], "body": null, "bodyBase64": null})), None);
    assert_eq!(p.message(), PluginMessage::Result { message: Some("Posted the export to Discord.".into()) });
    assert!(p.end().success());

    // Its test message, to a webhook Discord no longer knows.
    let mut p = Program::start(python, dir.path(), "main.py");
    p.send(HostMessage::Run(Box::new(discord(ActionKind::Command, "test"))));
    secret(&mut p, json!(hook));
    p.message();
    let (call, _) = p.call();
    p.reply(&call, Some(json!({"status": 404, "url": hook, "headers": [], "body": "{}", "bodyBase64": null})), None);
    let unknown = "Discord doesn't know that webhook. Copy it again from the channel's settings.";
    assert_eq!(p.message(), PluginMessage::Error { message: unknown.into(), detail: Some("HTTP 404".into()) });
    assert_eq!(p.end().code(), Some(1));

    // No webhook set, or one that isn't Discord's: nothing is posted, and
    // the URL is in none of its words.
    for (value, said) in [
        (Value::Null, "Set the webhook URL in Discord Notify's settings first."),
        (
            json!("https://evil.example/api/webhooks/1/abc"),
            "That webhook URL isn't a Discord webhook. Copy it again from the channel's settings.",
        ),
    ] {
        let mut p = Program::start(python, dir.path(), "main.py");
        p.send(HostMessage::Run(Box::new(discord(ActionKind::Command, "test"))));
        secret(&mut p, value);
        assert_eq!(p.message(), PluginMessage::Error { message: said.into(), detail: None });
        assert_eq!(p.end().code(), Some(1));
    }

    // A keychain that can't be read: AutoScene's words, never a value.
    let mut p = Program::start(python, dir.path(), "main.py");
    p.send(HostMessage::Run(Box::new(discord(ActionKind::Command, "test"))));
    let (call, _) = p.call();
    let locked = "Your system's keychain can't be used: it is locked, or refused AutoScene.";
    p.reply(&call, None, Some(CallError::new(CallCode::Failed, locked)));
    assert_eq!(p.message(), PluginMessage::Error { message: locked.into(), detail: Some("failed".into()) });
    assert_eq!(p.end().code(), Some(1));
}

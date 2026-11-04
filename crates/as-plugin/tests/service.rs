//! Plugin services, with the stand-in plugin (`src/bin/asp_test_plugin.rs`)
//! in its service modes: how one starts, answers its pages' requests,
//! emits, calls AutoScene, and ends, asked to or not. Each service works
//! in a folder of its own.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use std::{fs, panic};

use as_core::CancelToken;
use as_plugin::runtime::TERM_GRACE;
use as_plugin::scratch;
use as_plugin::service::{MAX_PENDING, MAX_SERVICE_LINE, STOP_GRACE};
use as_plugin::{
    AppContext, CallCode, CallError, HostCalls, HostMethod, Launch, PluginContext, PluginError, PluginId, PluginResult,
    ProcessService, RequestContext, ServeMessage, Service, ServiceEvent, ServiceSpec, SettingValue, ToolPaths,
};
use serde_json::value::RawValue;
use serde_json::{Value, json};
use tempfile::TempDir;

const NAME: &str = "Test Plugin";
const ID: &str = "dev.autoscene.test-plugin";

/// A service of the stand-in plugin in `mode`, in a folder of its own,
/// keeping what it tells its host.
struct Served {
    dir: TempDir,
    mode: &'static str,
    ready_wait: Duration,
    calls: Arc<dyn HostCalls>,
    events: Arc<Mutex<Vec<ServiceEvent>>>,
}

impl Served {
    fn new(mode: &'static str) -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
            mode,
            ready_wait: Duration::from_secs(20),
            calls: Arc::new(Settings),
            events: Arc::default(),
        }
    }

    fn data(&self) -> PathBuf {
        self.dir.path().join("data")
    }

    fn text(&self, name: &str) -> String {
        fs::read_to_string(self.data().join(name)).unwrap()
    }

    fn log(&self) -> PathBuf {
        self.dir.path().join("logs").join(format!("{ID}.service.log"))
    }

    fn log_text(&self) -> String {
        fs::read_to_string(self.log()).unwrap()
    }

    fn events(&self) -> Vec<ServiceEvent> {
        self.events.lock().unwrap().clone()
    }

    fn spec(&self) -> ServiceSpec {
        let exe = Path::new(env!("CARGO_BIN_EXE_asp_test_plugin"));
        let plugin = PluginContext {
            id: PluginId::parse(ID).unwrap(),
            version: "1.2.0".into(),
            data_dir: "/data".into(),
            resources_dir: None,
            temp_dir: None,
        };
        ServiceSpec {
            name: NAME.into(),
            dir: exe.parent().unwrap().to_path_buf(),
            launch: Launch::Exe {
                program: exe.file_name().unwrap().to_str().unwrap().into(),
                args: vec![self.mode.into()],
            },
            data_dir: self.data(),
            log_path: self.log(),
            ffmpeg: self.dir.path().join("tools").join("ffmpeg"),
            ffprobe: self.dir.path().join("tools").join("ffprobe"),
            // A program: no uv runs.
            uv: None,
            resources: None,
            ready_wait: self.ready_wait,
            message: ServeMessage {
                protocol: 2,
                service_id: "service-1".into(),
                plugin,
                app: AppContext { version: "1.0.3-BETA".into(), platform: "linux-x86_64".into() },
                tools: ToolPaths {
                    ffmpeg: "/tools/ffmpeg".into(),
                    ffprobe: "/tools/ffprobe".into(),
                    ffmpeg_version: "9.0".into(),
                },
                settings: BTreeMap::from([("greeting".into(), SettingValue::Text("Hi".into()))]),
            },
            calls: self.calls.clone(),
        }
    }

    fn start(&self) -> PluginResult<Arc<ProcessService>> {
        let events = self.events.clone();
        ProcessService::start(self.spec(), Arc::new(move |e| events.lock().unwrap().push(e)), &CancelToken::new())
    }
}

/// AutoScene's side of a service's calls: `settings.get` gives its name.
struct Settings;

impl HostCalls for Settings {
    fn call(&self, method: HostMethod, _: &RawValue, _: &CancelToken) -> Result<Value, CallError> {
        match method {
            HostMethod::SettingsGet => Ok(json!({"greeting": "Hi from AutoScene"})),
            _ => Err(CallError::new(CallCode::Denied, "Not here.")),
        }
    }
}

/// The request of the page `main` of `method` with `params`, which waits a
/// minute at most and is never cancelled.
fn ask(s: &ProcessService, method: &str, params: Value) -> Result<Value, CallError> {
    s.request(method, params, context(), Duration::from_secs(60), &CancelToken::new())
}

fn context() -> RequestContext {
    RequestContext { page: "main".into(), project_id: Some("p1".into()) }
}

fn failed(message: &str) -> CallError {
    CallError::new(CallCode::Failed, message)
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
    thread::sleep(Duration::from_millis(300));
    let before = fs::metadata(file).unwrap().len();
    thread::sleep(Duration::from_millis(600));
    fs::metadata(file).unwrap().len() == before
}

#[test]
fn a_service_answers_requests_by_id() {
    let s = Served::new("serve");
    let service = s.start().unwrap();
    assert!(service.is_running());
    assert_eq!(s.events(), [ServiceEvent::Ready]);
    // Its first line, in its data folder, in its mode.
    let serve: Value = serde_json::from_str(&s.text("serve.json")).unwrap();
    assert_eq!((serve["type"].as_str(), serve["protocol"].as_u64()), (Some("serve"), Some(2)));
    assert_eq!(serve["serviceId"], json!("service-1"));
    assert_eq!(serve["plugin"]["dataDir"], json!(s.data()));
    assert_eq!(serve["settings"], json!({"greeting": "Hi"}));
    assert_eq!(s.text("mode"), "service");
    // Its plugin names no large files: no folder of them.
    assert!(serve["plugin"].get("resourcesDir").is_none(), "{serve}");
    assert_eq!(s.text("resources"), "");

    assert_eq!(ask(&service, "count", json!({"text": "one two three"})), Ok(json!({"words": 3})));
    // What the page asked, and from where.
    assert_eq!(
        ask(&service, "echo", json!([1, "two"])),
        Ok(json!({"params": [1, "two"], "context": {"page": "main", "projectId": "p1"}}))
    );
    // Its fault: the message for the page, cleaned, and the detail for the log.
    assert_eq!(ask(&service, "fail", Value::Null), Err(failed("No text. Give it some.")));
    // Answers out of order reach those who asked.
    let slept: Vec<_> = thread::scope(|scope| {
        let service = &service;
        let asked: Vec<_> =
            [600, 300, 0].into_iter().map(|ms| scope.spawn(move || ask(service, "slow", json!({"ms": ms})))).collect();
        asked.into_iter().map(|a| a.join().unwrap().unwrap()["slept"].clone()).collect()
    });
    assert_eq!(slept, [json!(600), json!(300), json!(0)]);
    // One the service doesn't answer in time is told so.
    let late = service.request("slow", json!({"ms": 1500}), context(), Duration::from_millis(300), &CancelToken::new());
    assert_eq!(late, Err(failed("Test Plugin didn't answer in time.")));
    // One that would wait for ever waits ten minutes at most.
    let endless = service.request("count", json!({"text": "a"}), context(), Duration::MAX, &CancelToken::new());
    assert_eq!(endless, Ok(json!({"words": 1})));

    service.stop(STOP_GRACE);
    let log = s.log_text();
    assert!(log.lines().next().unwrap().ends_with(" run service-1 serve"), "{log}");
    for line in [
        "[host] ready\n",
        "[request] r1 count\n",
        "[response] r1 ok\n",
        "[failed] r3 No text.\n  Give it some. (KeyError: 'text')\n",
        "[response] r3 failed\n",
        "[response] r7 ok, which no request waits for: dropped\n",
    ] {
        assert!(log.contains(line), "{line}: {log}");
    }
    // Never what a request or a response carried.
    assert!(!log.contains("one two") && !log.contains("words"), "{log}");
}

#[test]
fn a_service_gets_its_resources_folder() {
    let s = Served::new("serve");
    let folder = s.dir.path().join("cache").join("plugin-resources").join(ID);
    let spec = ServiceSpec { resources: Some(folder.clone()), ..s.spec() };
    let events = s.events.clone();
    let service =
        ProcessService::start(spec, Arc::new(move |e| events.lock().unwrap().push(e)), &CancelToken::new()).unwrap();
    // In its serve line and its environment.
    let serve: Value = serde_json::from_str(&s.text("serve.json")).unwrap();
    assert_eq!(PathBuf::from(serve["plugin"]["resourcesDir"].as_str().unwrap()), folder);
    assert_eq!(PathBuf::from(s.text("resources")), folder);
    service.stop(STOP_GRACE);
}

#[test]
fn a_service_s_scratch_folder_goes_when_it_stops() {
    let s = Served::new("serve");
    let service = s.start().unwrap();
    // Its serve line and its environment name the same folder, its own.
    let serve: Value = serde_json::from_str(&s.text("serve.json")).unwrap();
    let dir = PathBuf::from(serve["plugin"]["tempDir"].as_str().unwrap());
    assert_eq!(PathBuf::from(s.text("run-temp")), dir);
    assert_eq!(dir.file_name().unwrap(), scratch::TEMP);
    let root = dir.parent().unwrap();
    assert_eq!(root.file_name().unwrap().len(), scratch::PREFIX.len() + scratch::NAME_CHARS);
    // It lives while the service does, with what the service wrote there,
    // and the marker beside it.
    assert_eq!(fs::read_to_string(dir.join("served.txt")).unwrap(), "served");
    assert!(root.join(scratch::MARKER).is_file());
    service.stop(STOP_GRACE);
    // Removed on a thread of its own, once it stopped.
    let until = Instant::now() + Duration::from_secs(30);
    while fs::symlink_metadata(root).is_ok() {
        assert!(Instant::now() < until, "{} is still there", root.display());
        thread::sleep(Duration::from_millis(20));
    }
    // A second start gets a folder of its own.
    let again = s.start().unwrap();
    let serve: Value = serde_json::from_str(&s.text("serve.json")).unwrap();
    assert_ne!(PathBuf::from(serve["plugin"]["tempDir"].as_str().unwrap()), dir);
    again.stop(STOP_GRACE);
}

#[test]
fn a_slow_start_reports_progress_then_ready() {
    let s = Served::new("serve-slow-start");
    let started = Instant::now();
    let service = s.start().unwrap();
    assert!(started.elapsed() >= Duration::from_millis(300), "{:?}", started.elapsed());
    assert_eq!(
        s.events(),
        [
            ServiceEvent::Progress { stage: "Getting Python".into(), fraction: Some(0.5) },
            ServiceEvent::Progress { stage: "Installing packages".into(), fraction: None },
            ServiceEvent::Ready,
        ]
    );
    assert_eq!(ask(&service, "count", json!({"text": "ready"})), Ok(json!({"words": 1})));
    // Once it is ready, what it says of its progress goes to its log.
    wait_for(|| s.log_text().contains("[progress] Warming up\n"));
    assert!(!s.events().iter().any(|e| matches!(e, ServiceEvent::Progress { stage, .. } if stage == "Warming up")));
    service.stop(STOP_GRACE);
}

#[test]
fn a_flood_of_progress_reaches_the_host_ten_times_a_second_at_most() {
    let s = Served::new("serve-progress-flood");
    let started = Instant::now();
    let service = s.start().unwrap();
    let took = started.elapsed();
    let events = s.events();
    assert_eq!(events.last(), Some(&ServiceEvent::Ready));
    let stages: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            ServiceEvent::Progress { stage, .. } => Some(stage.as_str()),
            _ => None,
        })
        .collect();
    // The first at once, then the latest each tenth of a second: of 5000.
    assert_eq!(stages.first(), Some(&"Step 0"));
    assert!(stages.len() as u128 <= 2 + took.as_millis() / 100, "{} in {took:?}", stages.len());
    service.stop(STOP_GRACE);
}

#[test]
fn a_service_that_never_says_ready_is_ended_after_the_wait() {
    // The run mode that never answers, and leaves a grandchild behind.
    let s = Served { ready_wait: Duration::from_secs(1), ..Served::new("ignore") };
    let started = Instant::now();
    let e = s.start().err().unwrap();
    let took = started.elapsed();
    assert!(took >= Duration::from_secs(1) && took < Duration::from_secs(1) + TERM_GRACE * 3, "{took:?}");
    assert_eq!(e.to_string(), "Test Plugin didn't start in time. Its log says why.");
    assert!(stopped(&s.data().join("beat")));
    let stop = ServiceEvent::Stopped { message: "Test Plugin didn't start in time.".into(), crashed: true };
    assert_eq!(s.events(), [stop]);
    assert!(s.log_text().contains("[host] not ready in time; ended\n"));
}

#[test]
fn a_crash_fails_every_waiting_request() {
    let s = Served::new("serve-crash");
    let service = s.start().unwrap();
    // It crashes at the third request, before it answers any.
    let answers: Vec<_> = thread::scope(|scope| {
        let asked: Vec<_> = (0..3).map(|_| scope.spawn(|| ask(&service, "wait", Value::Null))).collect();
        asked.into_iter().map(|a| a.join().unwrap()).collect()
    });
    assert_eq!(answers, vec![Err(failed("Test Plugin stopped unexpectedly.")); 3]);
    wait_for(|| !service.is_running());
    assert_eq!(
        s.events().last(),
        Some(&ServiceEvent::Stopped { message: "Test Plugin stopped unexpectedly.".into(), crashed: true })
    );
    // One that comes after is told it stopped.
    assert_eq!(ask(&service, "count", Value::Null), Err(failed("Test Plugin stopped.")));
    let log = s.log_text();
    assert!(log.contains("[stderr] problem with request \"r"), "{log}");
    // The host notes how it ended, with what its stderr said last: the request
    // that reached it third, whichever of the three that was.
    assert!(log.lines().any(|l| l.starts_with("[host] ") && l.contains("; problem with request \"r")), "{log}");
}

#[test]
fn stop_asks_then_kills_the_tree() {
    // One that stops when asked ends at once.
    let s = Served::new("serve");
    let service = s.start().unwrap();
    let asked = Instant::now();
    service.stop(STOP_GRACE);
    assert!(asked.elapsed() < STOP_GRACE, "{:?}", asked.elapsed());
    assert!(!service.is_running());
    assert_eq!(s.text("stop-seen"), "{\"type\":\"stop\"}\n");
    let stopped_well = ServiceEvent::Stopped { message: "Test Plugin stopped.".into(), crashed: false };
    assert_eq!(s.events().last(), Some(&stopped_well));
    assert_eq!(ask(&service, "count", Value::Null), Err(failed("Test Plugin stopped.")));

    // One that stays is ended with its grandchild after the grace.
    let s = Served::new("serve-ignore-stop");
    let service = s.start().unwrap();
    wait_for(|| fs::metadata(s.data().join("beat")).is_ok_and(|m| m.len() > 0));
    let grace = Duration::from_millis(500);
    let asked = Instant::now();
    service.stop(grace);
    let took = asked.elapsed();
    assert!(took >= grace && took < grace + TERM_GRACE + Duration::from_secs(5), "{took:?}");
    assert!(stopped(&s.data().join("beat")));
    assert_eq!(s.events().last(), Some(&stopped_well));
    let log = s.log_text();
    assert!(
        log.contains("[host] asked to stop\n") && log.contains("[host] still running after stop; ended\n"),
        "{log}"
    );
}

#[test]
fn a_second_stop_returns_once_the_program_ended() {
    let s = Served::new("serve-ignore-stop");
    let service = s.start().unwrap();
    let beat = s.data().join("beat");
    wait_for(|| fs::metadata(&beat).is_ok_and(|m| m.len() > 0));
    thread::scope(|scope| {
        let first = scope.spawn(|| service.stop(Duration::from_secs(2)));
        thread::sleep(Duration::from_millis(300));
        // The first stop is still waiting for it: this one waits too.
        service.stop(STOP_GRACE);
        assert!(!service.is_running());
        assert!(stopped(&beat));
        first.join().unwrap();
    });
    let stopped_well = ServiceEvent::Stopped { message: "Test Plugin stopped.".into(), crashed: false };
    assert_eq!(s.events().last(), Some(&stopped_well));
}

#[test]
fn a_service_that_leaves_its_input_unread_is_ended() {
    let s = Served::new("serve-deaf");
    let service = s.start().unwrap();
    // Settings it never reads, far more than its pipe holds.
    let settings = BTreeMap::from([("greeting".into(), SettingValue::Text("x".repeat(4096)))]);
    for _ in 0..1100 {
        service.settings_changed(settings.clone());
    }
    wait_for(|| !service.is_running());
    let crashed = ServiceEvent::Stopped { message: "Test Plugin stopped unexpectedly.".into(), crashed: true };
    wait_for(|| s.events().last() == Some(&crashed));
    let log = s.log_text();
    assert!(log.contains("[host] its replies went unread; ended\n"), "{}", &log[log.len().saturating_sub(2000)..]);
}

#[test]
fn emits_with_a_bad_name_or_too_much_data_are_dropped() {
    let s = Served::new("serve");
    let service = s.start().unwrap();
    assert_eq!(ask(&service, "emits", Value::Null), Ok(Value::Null));
    let fine = ServiceEvent::Emit { event: "fine".into(), data: json!(3) };
    wait_for(|| s.events().contains(&fine));
    assert_eq!(s.events(), [ServiceEvent::Ready, fine]);
    service.stop(STOP_GRACE);
    let log = s.log_text();
    let named = "dropped: an event's name is 1 to 64 of a-z, 0-9, . _ -\n";
    assert_eq!(log.matches(named).count(), 2, "{log}");
    assert!(log.contains("[emit] big dropped: it carries more than 256 KB\n"), "{log}");
}

#[test]
fn emits_past_fifty_a_second_are_dropped() {
    let s = Served::new("serve-emit-flood");
    let service = s.start().unwrap();
    let tock = ServiceEvent::Emit { event: "tock".into(), data: Value::Null };
    wait_for(|| s.events().contains(&tock));
    let ticks: Vec<_> = s
        .events()
        .into_iter()
        .filter_map(|e| match e {
            ServiceEvent::Emit { event, data } if event == "tick" => Some(data),
            _ => None,
        })
        .collect();
    // The first fifty, in order; a second later one goes again.
    assert_eq!(ticks, (0..50).map(|i| json!(i)).collect::<Vec<_>>());
    // Each request's emit reaches its host.
    assert_eq!(ask(&service, "count", json!({"text": "a b"})), Ok(json!({"words": 2})));
    let counted = ServiceEvent::Emit { event: "counted".into(), data: json!({"words": 2}) };
    wait_for(|| s.events().contains(&counted));
    service.stop(STOP_GRACE);
    let log = s.log_text();
    assert_eq!(log.matches("[host] 150 events dropped: at most 50 a second\n").count(), 1, "{log}");
}

#[test]
fn a_service_can_call_the_host() {
    let s = Served::new("serve-call");
    let service = s.start().unwrap();
    assert_eq!(ask(&service, "call", Value::Null), Ok(json!({"greeting": "Hi from AutoScene"})));
    // Its settings changed: it hears them as they are now.
    service.settings_changed(BTreeMap::from([("greeting".into(), SettingValue::Text("Hey".into()))]));
    wait_for(|| fs::read_to_string(s.data().join("settings.json")).is_ok());
    assert_eq!(s.text("settings.json"), "{\"type\":\"settings\",\"settings\":{\"greeting\":\"Hey\"}}\n");
    service.stop(STOP_GRACE);
    let log = s.log_text();
    assert!(log.contains("[call] settings.get\n[reply] settings.get ok\n"), "{log}");
    assert!(log.contains("[settings] changed\n"), "{log}");
}

#[test]
fn requests_past_32_waiting_are_refused() {
    let s = Served::new("serve");
    let service = s.start().unwrap();
    let page_closed = CancelToken::new();
    let answers: Vec<_> = thread::scope(|scope| {
        let asked: Vec<_> = (0..MAX_PENDING)
            .map(|_| {
                scope.spawn(|| service.request("wait", Value::Null, context(), Duration::from_secs(60), &page_closed))
            })
            .collect();
        wait_for(|| fs::read_to_string(s.log()).is_ok_and(|log| log.matches("[request] ").count() == MAX_PENDING));
        let past = ask(&service, "count", Value::Null);
        assert_eq!(past, Err(CallError::new(CallCode::Busy, "Test Plugin has too many requests waiting.")));
        // The page closes: none of them waits any longer.
        page_closed.cancel();
        asked.into_iter().map(|a| a.join().unwrap()).collect()
    });
    let cancelled = Err(CallError::new(CallCode::Cancelled, "This request was cancelled."));
    assert!(answers.iter().all(|a| *a == cancelled), "{answers:?}");
    // Their places are free again.
    assert_eq!(ask(&service, "count", json!({"text": "free"})), Ok(json!({"words": 1})));
    service.stop(STOP_GRACE);
}

#[test]
fn an_endless_line_is_cut_at_1_mib() {
    let s = Served::new("serve");
    let service = s.start().unwrap();
    // Output far past the limit goes to the log, cut where a run's is.
    assert_eq!(ask(&service, "long", Value::Null), Ok(json!({"after": "long"})));
    // An answer past it is none: the request hears so at once.
    let started = Instant::now();
    let huge = ask(&service, "huge", Value::Null);
    assert_eq!(huge, Err(failed(&format!("Test Plugin's answer is larger than {} MB.", MAX_SERVICE_LINE >> 20))));
    assert!(started.elapsed() < Duration::from_secs(10), "{:?}", started.elapsed());
    // And the service goes on.
    assert_eq!(ask(&service, "count", json!({"text": "still here"})), Ok(json!({"words": 2})));
    service.stop(STOP_GRACE);
    let log = s.log_text();
    let cut = format!("\n[stdout] {} [cut]\n", "x".repeat(as_plugin::runtime::MAX_LINE_BYTES));
    assert!(log.contains(&cut), "{}", log.len());
    assert!(log.contains("[response] r2 too long\n"), "{}", &log[log.len().saturating_sub(2000)..]);
    assert!(!log.contains("hhhh"), "{}", log.len());
}

#[test]
fn a_cancelled_start_starts_nothing_or_ends_it() {
    let s = Served::new("serve-slow-start");
    // Cancelled before it starts: nothing is made.
    let cancel = CancelToken::new();
    cancel.cancel();
    let events = s.events.clone();
    let start = ProcessService::start(s.spec(), Arc::new(move |e| events.lock().unwrap().push(e)), &cancel);
    assert!(matches!(start, Err(PluginError::Cancelled)), "{:?}", start.err());
    assert!(!s.data().exists() && !s.log().exists());
    // Cancelled while it starts: it ends, and says it was asked to.
    let cancel = CancelToken::new();
    let events = s.events.clone();
    let start = thread::scope(|scope| {
        let starting =
            scope.spawn(|| ProcessService::start(s.spec(), Arc::new(move |e| events.lock().unwrap().push(e)), &cancel));
        wait_for(|| !s.events().is_empty());
        cancel.cancel();
        starting.join().unwrap_or_else(|e| panic::resume_unwind(e))
    });
    assert!(matches!(start, Err(PluginError::Cancelled)), "{:?}", start.err());
    assert_eq!(
        s.events().last(),
        Some(&ServiceEvent::Stopped { message: "Test Plugin stopped.".into(), crashed: false })
    );
}

/// The example service (`examples/plugins/hello-service`), as AutoScene
/// starts it: through AutoScene's own uv, got once into the temporary
/// folder's `autoscene-test-runtimes`, which may fetch Python first.
#[test]
#[ignore = "downloads AutoScene's uv, which may download Python"]
fn the_example_service_counts_words() {
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/plugins/hello-service");
    let runtimes = std::env::temp_dir().join("autoscene-test-runtimes");
    let uv = as_plugin::uv::install(&runtimes, &CancelToken::new(), |_, _| {}).unwrap();
    let s = Served::new("serve");
    let spec = ServiceSpec {
        dir: example.clone(),
        launch: Launch::UvScript { script: "main.py".into() },
        uv: Some(uv),
        ready_wait: as_plugin::service::READY_WAIT,
        ..s.spec()
    };
    let events = s.events.clone();
    let service =
        ProcessService::start(spec, Arc::new(move |e| events.lock().unwrap().push(e)), &CancelToken::new()).unwrap();
    assert_eq!(ask(&service, "count", json!({"text": "one two three"})), Ok(json!({"words": 3})));
    assert_eq!(ask(&service, "count", json!({})), Err(failed("Give it some text to count.")));
    let counted = ServiceEvent::Emit { event: "counted".into(), data: json!({"words": 3, "total": 3}) };
    wait_for(|| s.events().contains(&counted));
    service.stop(STOP_GRACE);
    assert_eq!(
        s.events().last(),
        Some(&ServiceEvent::Stopped { message: "Test Plugin stopped.".into(), crashed: false })
    );
    let log = s.log_text();
    assert!(log.contains("[info] Serving its page in AutoScene 1.0.3-BETA.\n"), "{log}");
    assert!(log.contains("[failed] r2 Give it some text to count. (Traceback"), "{log}");
    // Its folder stays as it is: no bytecode of the module it imports.
    assert!(!example.join("__pycache__").exists());
    // What uv fetched stays beside it, in AutoScene's runtimes.
    assert!(runtimes.join("uv-cache").is_dir());
}

//! A plugin's service (protocol 2): its program kept running while one of
//! its pages is open, which answers the pages' requests.
//!
//! A service starts as a run does ([`crate::runtime`]): by its path in the
//! plugin's folder, in the plugin's data folder, with the same environment
//! and `AUTOSCENE_MODE=service`, as the root of a tree that ends with it.
//! Each start gets a scratch folder of its own ([`crate::scratch`]), removed
//! once it stopped.
//! Its first line is a [`HostMessage::Serve`]. It has [`READY_WAIT`] to say
//! it is ready (uv may fetch Python and the script's packages first); what
//! it says of its progress until then goes to the host (its latest, at most
//! [`MAX_PROGRESS_PER_SEC`] a second), and after to its log. Then each
//! request a page makes reaches it as a
//! [`HostMessage::Request`], which it answers with a response of the same
//! id. At most [`MAX_PENDING`] wait for their answer at once, each for at
//! most [`MAX_REQUEST_TIMEOUT`]. It may emit events for its open pages, at
//! most [`MAX_EMITS_PER_SEC`] a second, and call AutoScene as a run does.
//! A line it writes is read no longer than [`MAX_SERVICE_LINE`]: a call
//! too, so a service's call is smaller than a run's may be.
//!
//! It stops when asked ([`Service::stop`]): it hears a stop line, and is
//! ended with its tree after the grace it is given; each stop returns once
//! it ended. A service that ends unasked has crashed: each request still
//! waiting fails, the log notes the end of its stderr, and the host hears
//! it ([`ServiceEvent::Stopped`]). A program never starts twice for one
//! service: whoever keeps it (the app's supervisor) starts a new one.
//!
//! The service writes its own log ([`crate::log`], as a run with the id of
//! its start), beside its plugin's run log: a run and the service may write
//! at once. The log names each request, call and response, never what they
//! carry, and moves aside when full instead of dropping what comes.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use as_core::CancelToken;
use as_media::proc::Tree;
use serde_json::Value;

use crate::calls::{CallCode, CallError, HostCalls};
use crate::error::{PluginError, PluginResult};
use crate::log::PluginLog;
use crate::manifest::{Launch, SettingValue, shown};
use crate::protocol::{self, HostMessage, PluginMessage, Request, RequestContext, ServeMessage, ServiceMessage};
use crate::runtime::{
    self, Calls, DRAIN, Load, MAX_MESSAGE_CHARS, MAX_STAGE_CHARS, Msg, TERM_GRACE, TICK, Talk, sanitize,
};
use crate::scratch::Scratch;

/// How long a service may take to say it is ready: uv may install Python
/// and the script's packages first.
pub const READY_WAIT: Duration = Duration::from_secs(120);
/// How long a request waits for its answer, unless it asks for longer…
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
/// …which is at most this.
pub const MAX_REQUEST_TIMEOUT: Duration = Duration::from_secs(600);
/// How long a service may take to stop after the stop line.
pub const STOP_GRACE: Duration = Duration::from_secs(3);
/// The requests that may wait for their answer at once.
pub const MAX_PENDING: usize = 32;
/// The longest line a service writes: a response, an event, or a call.
pub const MAX_SERVICE_LINE: usize = 1 << 20;
/// Lines read but not yet handled: 64 MB at most.
pub const SERVICE_QUEUE: usize = 64;
/// The events a service may emit in a second; those past it are dropped.
pub const MAX_EMITS_PER_SEC: u32 = 50;
/// The longest name of an event, of `[a-z0-9._-]`.
pub const MAX_EVENT_CHARS: usize = 64;
/// The most an event carries, as JSON.
pub const MAX_EVENT_DATA: usize = 256 << 10;
/// How often a starting service's progress reaches the host in a second at
/// most: what it says between waits, and only its latest goes.
pub const MAX_PROGRESS_PER_SEC: u32 = 10;

/// A service to start, as [`crate::store::Store::serve_spec`] makes it.
pub struct ServiceSpec {
    /// The plugin's name, as messages show it.
    pub name: String,
    /// The plugin's folder.
    pub dir: PathBuf,
    pub launch: Launch,
    /// The plugin's own folder for files; the service starts there.
    pub data_dir: PathBuf,
    /// The service's log file.
    pub log_path: PathBuf,
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    /// AutoScene's uv ([`crate::uv`]), which the host got for a uv script
    /// before the start: without it, the script doesn't start.
    pub uv: Option<PathBuf>,
    /// The folder of the large files its `plugin.json` names, which the
    /// host got before the start ([`crate::resources`]); none when it names
    /// none.
    pub resources: Option<PathBuf>,
    /// How long it may take to say it is ready ([`READY_WAIT`]), from the
    /// start of its program.
    pub ready_wait: Duration,
    pub message: ServeMessage,
    /// The host's answers to its calls.
    pub calls: Arc<dyn HostCalls>,
}

/// What a service tells its host.
#[derive(Clone, Debug, PartialEq)]
pub enum ServiceEvent {
    /// How far its start is, before it is ready: the stage (cleaned, at most
    /// [`MAX_STAGE_CHARS`]; the plugin's name if it named none) and how far,
    /// if known.
    Progress {
        stage: String,
        fraction: Option<f64>,
    },
    Ready,
    /// An event for the plugin's open pages.
    Emit {
        event: String,
        data: Value,
    },
    /// It ended: asked to (`crashed` false), or unasked, which counts as a
    /// crash (a start that never said it was ready too). `message` says so.
    Stopped {
        message: String,
        crashed: bool,
    },
}

/// Where a service's events go. Called on the service's own thread.
pub type OnEvent = Arc<dyn Fn(ServiceEvent) + Send + Sync>;

/// A plugin's service, to its host.
pub trait Service: Send + Sync {
    /// Asks it `method` with `params` for the page of `context`, and waits
    /// for its answer: at most `timeout` (capped at [`MAX_REQUEST_TIMEOUT`]),
    /// and only until `ended` fires (the page closed). The error is the
    /// page's: busy past [`MAX_PENDING`] waiting, cancelled once `ended`
    /// fired, else failed with a sentence that says why.
    fn request(
        &self,
        method: &str,
        params: Value,
        context: RequestContext,
        timeout: Duration,
        ended: &CancelToken,
    ) -> Result<Value, CallError>;
    /// Tells it its plugin's settings changed: each of them, as they are now.
    fn settings_changed(&self, settings: BTreeMap<String, SettingValue>);
    /// Asks it to stop, and ends it with what it started `grace` later.
    /// Returns once it ended.
    fn stop(&self, grace: Duration);
    /// Whether it still runs.
    fn is_running(&self) -> bool;
}

/// The requests of a service that wait for their answer, by id.
type Pending = Arc<Mutex<HashMap<String, SyncSender<Result<Value, CallError>>>>>;

/// A service as a process: its plugin's program, in service mode.
pub struct ProcessService {
    name: String,
    /// The queue of the thread that speaks with it.
    to_loop: SyncSender<Msg>,
    pending: Pending,
    /// How many requests it was asked.
    asked: AtomicU64,
    /// False once its program ended.
    running: Arc<AtomicBool>,
    /// The grace a stop gave it, once one was asked: no request is taken
    /// after.
    stop: Arc<Mutex<Option<Duration>>>,
    /// The thread that speaks with it, which the first stop joins…
    thread: Mutex<Option<JoinHandle<()>>>,
    /// …and each other one waits for: set once that thread ended, however.
    done: Arc<Done>,
}

impl ProcessService {
    /// Starts `spec` and waits until it says it is ready (its progress until
    /// then goes to `on_event`), it fails, `cancel` fires, or its ready wait
    /// passes. Fails as a run does when it can't start; with
    /// [`PluginError::Crashed`] when it ends before it is ready; with
    /// [`PluginError::Failed`] when it isn't ready in time, and it is ended;
    /// and with [`PluginError::Cancelled`] when `cancel` fires first (one
    /// cancelled before it starts starts nothing). Its program is gone
    /// whenever it fails.
    pub fn start(spec: ServiceSpec, on_event: OnEvent, cancel: &CancelToken) -> PluginResult<Arc<Self>> {
        if cancel.is_cancelled() {
            return Err(PluginError::Cancelled);
        }
        let ServiceSpec {
            name,
            dir,
            launch,
            data_dir,
            log_path,
            ffmpeg,
            ffprobe,
            uv,
            resources,
            ready_wait,
            mut message,
            calls,
        } = spec;
        let mut cmd = runtime::command(&name, &dir, &launch, || uv)?;
        let id = message.plugin.id.clone();
        let place = (data_dir.as_path(), resources.as_deref());
        let (data_dir, resources) = runtime::prepare(&mut cmd, place, &ffmpeg, &ffprobe, &id, message.protocol)?;
        let mut log = PluginLog::open_rolling(&log_path, &message.service_id, "serve")?;
        // Removed once it stopped, as its thread ends.
        let scratch = runtime::scratch(&mut cmd, message.protocol, &mut log, &id);
        cmd.env("AUTOSCENE_MODE", "service");
        message.plugin.data_dir = data_dir.to_string_lossy().into_owned();
        message.plugin.resources_dir = resources.map(|r| r.to_string_lossy().into_owned());
        message.plugin.temp_dir = runtime::temp_dir(scratch.as_ref());
        message.tools.ffmpeg = ffmpeg.to_string_lossy().into_owned();
        message.tools.ffprobe = ffprobe.to_string_lossy().into_owned();
        let mut tree = runtime::spawn(&mut cmd, &mut log, &id)?;

        let (tx, rx) = mpsc::sync_channel(SERVICE_QUEUE);
        let load = Arc::new(Load::default());
        let shares = load.clone();
        runtime::read_stdout(
            tree.take_stdout().expect("piped stdout"),
            tx.clone(),
            MAX_SERVICE_LINE,
            move |line, cut| service_line(line, cut, &shares),
        );
        runtime::read_stderr(tree.take_stderr().expect("piped stderr"), tx.clone());
        let to_plugin = runtime::writer(tree.take_stdin().expect("piped stdin"), load.clone());
        let calls = Calls::new(calls, tx.clone());
        let mut talk = Talk::new(log, to_plugin, load, Some(calls), MAX_SERVICE_LINE, "This service is stopping.");
        talk.send(HostMessage::Serve(Box::new(message)).line());

        let shown = sanitize(&name, MAX_STAGE_CHARS);
        let (ready_tx, ready_rx) = mpsc::channel();
        let service = Arc::new(Self {
            name,
            to_loop: tx,
            pending: Pending::default(),
            asked: AtomicU64::new(0),
            running: Arc::new(AtomicBool::new(true)),
            stop: Arc::default(),
            thread: Mutex::new(None),
            done: Arc::default(),
        });
        let serving = Serving {
            name: shown,
            tree,
            _scratch: scratch,
            rx,
            talk,
            pending: service.pending.clone(),
            running: service.running.clone(),
            stop: service.stop.clone(),
            stop_at: None,
            on_event,
            ready: Some(ready_tx),
            ready_by: Instant::now() + ready_wait,
            emits: Emits::default(),
            progress: Progress::default(),
        };
        // On an error the closure drops, and the tree with it. Its thread is
        // done once the closure drops, also when it panicked.
        let done = Finish(service.done.clone());
        let thread = thread::Builder::new().name("plugin-service".into()).spawn(move || {
            let _done = done;
            serving.run();
        })?;
        *lock(&service.thread) = Some(thread);
        loop {
            match ready_rx.recv_timeout(TICK) {
                Ok(Ok(())) => return Ok(service),
                Ok(Err(e)) => {
                    service.join();
                    return Err(e);
                }
                Err(RecvTimeoutError::Timeout) if cancel.is_cancelled() => {
                    service.stop(Duration::ZERO);
                    return Err(PluginError::Cancelled);
                }
                Err(RecvTimeoutError::Timeout) => {}
                // Its thread always says, unless it panicked.
                Err(RecvTimeoutError::Disconnected) => {
                    service.join();
                    let detail = "its thread ended without a word".to_owned();
                    return Err(PluginError::Crashed { name: service.name.clone(), detail });
                }
            }
        }
    }

    /// Waits for its thread, once its program ended or was asked to stop:
    /// the first to come joins it, and the others wait until it is done.
    fn join(&self) {
        let thread = lock(&self.thread).take();
        if let Some(thread) = thread
            && thread.join().is_err()
        {
            tracing::warn!(service = %self.name, "a plugin's service thread panicked");
        }
        self.done.wait();
    }

    /// Forgets the request `id`: its answer no longer counts.
    fn forget(&self, id: &str) {
        lock(&self.pending).remove(id);
    }

    /// Why a request that comes now isn't taken: it ended, or stops.
    fn not_taking(&self) -> Option<CallError> {
        let stopping = lock(&self.stop).is_some();
        (stopping || !self.is_running()).then(|| failed(format!("{} stopped.", self.name)))
    }
}

impl Service for ProcessService {
    fn request(
        &self,
        method: &str,
        params: Value,
        context: RequestContext,
        timeout: Duration,
        ended: &CancelToken,
    ) -> Result<Value, CallError> {
        if let Some(e) = self.not_taking() {
            return Err(e);
        }
        let id = format!("r{}", self.asked.fetch_add(1, Ordering::Relaxed) + 1);
        let (tx, rx) = mpsc::sync_channel(1);
        {
            let mut pending = lock(&self.pending);
            if pending.len() >= MAX_PENDING {
                return Err(CallError::new(CallCode::Busy, format!("{} has too many requests waiting.", self.name)));
            }
            pending.insert(id.clone(), tx);
        }
        let text = format!("{id} {}", shown(method));
        let line = HostMessage::Request(Request { id: id.clone(), method: method.to_owned(), params, context }).line();
        // Refused once its thread ended, and lost once its program did.
        let sent = self.to_loop.send(Msg::Send { line, tag: "request", text }).is_ok();
        if !sent || !self.is_running() {
            self.forget(&id);
            return Err(failed(format!("{} stopped.", self.name)));
        }
        let until = Instant::now() + timeout.min(MAX_REQUEST_TIMEOUT);
        loop {
            let left = until.saturating_duration_since(Instant::now());
            match rx.recv_timeout(left.min(TICK)) {
                Ok(answer) => return answer,
                Err(RecvTimeoutError::Timeout) if ended.is_cancelled() => {
                    self.forget(&id);
                    return Err(CallError::new(CallCode::Cancelled, "This request was cancelled."));
                }
                Err(RecvTimeoutError::Timeout) if left.is_zero() => {
                    self.forget(&id);
                    return Err(failed(format!("{} didn't answer in time.", self.name)));
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return Err(failed(format!("{} stopped.", self.name))),
            }
        }
    }

    fn settings_changed(&self, settings: BTreeMap<String, SettingValue>) {
        let line = HostMessage::Settings { settings }.line();
        // One that ended hears nothing.
        self.to_loop.send(Msg::Send { line, tag: "settings", text: "changed".into() }).ok();
    }

    fn stop(&self, grace: Duration) {
        {
            let mut stop = lock(&self.stop);
            *stop = Some(stop.map_or(grace, |g| g.min(grace)));
        }
        self.join();
    }

    fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

/// One nobody keeps any more ends at once.
impl Drop for ProcessService {
    fn drop(&mut self) {
        lock(&self.stop).get_or_insert(Duration::ZERO);
    }
}

/// Whether a service's thread ended.
#[derive(Default)]
struct Done {
    done: Mutex<bool>,
    ended: Condvar,
}

impl Done {
    fn wait(&self) {
        let mut done = lock(&self.done);
        while !*done {
            done = self.ended.wait(done).unwrap_or_else(PoisonError::into_inner);
        }
    }
}

/// Sets [`Done`] as it drops, with its thread's closure.
struct Finish(Arc<Done>);

impl Drop for Finish {
    fn drop(&mut self) {
        *lock(&self.0.done) = true;
        self.0.ended.notify_all();
    }
}

/// Its thread's part: the program, what it says, and what is asked of it.
struct Serving {
    /// The plugin's name, as a stage shows it.
    name: String,
    tree: Tree,
    /// Its scratch folder: removed as this drops, once its tree has ended.
    _scratch: Option<Scratch>,
    rx: Receiver<Msg>,
    talk: Talk,
    pending: Pending,
    running: Arc<AtomicBool>,
    stop: Arc<Mutex<Option<Duration>>>,
    /// When it was asked to stop, and when it is ended if it hasn't.
    stop_at: Option<(Instant, Instant)>,
    on_event: OnEvent,
    /// Told once it is ready, or why it never will be.
    ready: Option<mpsc::Sender<PluginResult<()>>>,
    ready_by: Instant,
    emits: Emits,
    progress: Progress,
}

/// Why the host ended a service.
#[derive(Clone, Copy, PartialEq)]
enum Ended {
    /// It ended on its own, asked to or not.
    ByItself,
    /// It was still running when its stop's grace was over.
    AfterStop,
    /// It didn't say it was ready in time.
    NotReady,
    /// It left too many replies unread.
    Unread,
}

impl Serving {
    fn run(mut self) {
        let (ended, status) = self.serve();
        // No request is taken from now: its program is gone.
        self.running.store(false, Ordering::SeqCst);
        self.talk.end_calls();
        // As a run does: what its pipes still hold, for a moment at most.
        let until = Instant::now() + DRAIN;
        while self.talk.open > 0 && Instant::now() < until {
            match self.rx.recv_timeout(until.saturating_duration_since(Instant::now())) {
                Ok(m) => self.take(m),
                Err(_) => break,
            }
        }
        let until = Instant::now() + runtime::CALL_GRACE;
        while self.talk.calls_running() > 0 {
            match self.rx.recv_timeout(until.saturating_duration_since(Instant::now())) {
                Ok(m) => self.take(m),
                Err(_) => break,
            }
        }
        self.talk.calls_left();
        self.emits.flush(&mut self.talk);
        let asked = self.stop_at.is_some();
        let detail = match &status {
            Ok(status) => self.talk.crash_detail(status),
            Err(e) => format!("couldn't end: {e}"),
        };
        // How it ended; unasked, with the end of its stderr, which the log
        // keeps also when the lines it wrote didn't fit.
        match &status {
            Ok(status) if asked => self.talk.log.note("host", &status.to_string()),
            _ => self.talk.log.note("host", &detail),
        }
        let name = self.name.clone();
        let (message, crashed) = match ended {
            _ if asked => (format!("{name} stopped."), false),
            Ended::NotReady => (format!("{name} didn't start in time."), true),
            _ => (format!("{name} stopped unexpectedly."), true),
        };
        let waiting: Vec<_> = lock(&self.pending).drain().collect();
        for (_, waiter) in waiting {
            waiter.send(Err(failed(message.clone()))).ok();
        }
        if let Some(ready) = self.ready.take() {
            let e = match ended {
                _ if asked => PluginError::Cancelled,
                Ended::NotReady => {
                    PluginError::Failed { message: format!("{message} Its log says why."), detail: Some(detail) }
                }
                _ => PluginError::Crashed { name, detail },
            };
            ready.send(Err(e)).ok();
        }
        (self.on_event)(ServiceEvent::Stopped { message, crashed });
    }

    /// Speaks with it until it ends, or the host ends it: why, and its
    /// status.
    fn serve(&mut self) -> (Ended, std::io::Result<ExitStatus>) {
        let ended = loop {
            self.stopping();
            let now = Instant::now();
            let overdue = if self.talk.unread() {
                Some((Ended::Unread, "its replies went unread"))
            } else if self.stop_at.is_some_and(|(_, by)| now >= by) {
                Some((Ended::AfterStop, "still running after stop"))
            } else if self.ready.is_some() && now >= self.ready_by {
                Some((Ended::NotReady, "not ready in time"))
            } else {
                None
            };
            if let Some((ended, why)) = overdue {
                self.talk.log.note("host", &format!("{why}; ended"));
                return (ended, self.tree.kill_tree(TERM_GRACE));
            }
            match self.rx.recv_timeout(TICK) {
                Ok(m) => self.take(m),
                Err(RecvTimeoutError::Timeout) => {}
                // Its pipes and its calls can't all be gone while it holds
                // a sender of its own; nothing to read either way.
                Err(RecvTimeoutError::Disconnected) => thread::sleep(TICK),
            }
            self.talk.next_call();
            self.report(Instant::now());
            match self.tree.has_exited() {
                Ok(false) => {}
                Ok(true) => break Ended::ByItself,
                Err(e) => {
                    self.talk.log.note("host", &format!("couldn't tell whether it runs: {e}"));
                    return (Ended::ByItself, self.tree.kill_tree(TERM_GRACE));
                }
            }
        };
        (ended, self.tree.finish())
    }

    /// A stop the host asked for: its line once, and its grace from then
    /// (a later, shorter one counts).
    fn stopping(&mut self) {
        let Some(grace) = *lock(&self.stop) else { return };
        let now = Instant::now();
        match &mut self.stop_at {
            Some((at, by)) => *by = (*by).min(*at + grace),
            None => {
                self.talk.send(HostMessage::Stop.line());
                self.talk.log.note("host", "asked to stop");
                // Nothing it calls for changes anything any more.
                self.talk.cancel_calls();
                self.stop_at = Some((now, now + grace));
            }
        }
    }

    fn take(&mut self, m: Msg) {
        let Some(m) = self.talk.take(m) else { return };
        match m {
            Msg::Plugin(PluginMessage::Progress { fraction, stage }) => {
                let stage = stage.map(|s| sanitize(&s, MAX_STAGE_CHARS)).filter(|s| !s.is_empty());
                if self.ready.is_none() {
                    return self.talk.log.line("progress", stage.as_deref().unwrap_or(""));
                }
                let fraction = fraction.filter(|f| f.is_finite()).map(|f| f.clamp(0.0, 1.0));
                let stage = stage.unwrap_or_else(|| self.name.clone());
                self.progress.held = Some(ServiceEvent::Progress { stage, fraction });
                self.report(Instant::now());
            }
            Msg::Plugin(PluginMessage::Log { level, message }) => self.talk.log.line(level.name(), &message),
            // A run's last word means nothing here.
            Msg::Plugin(PluginMessage::Result { message }) => {
                self.talk.log.line("result", message.as_deref().unwrap_or("(no message)"));
            }
            Msg::Plugin(PluginMessage::Error { message, detail }) => {
                let detail = detail.map(|d| format!(" ({d})")).unwrap_or_default();
                self.talk.log.line("failed", &format!("{message}{detail}"));
            }
            Msg::Service(ServiceMessage::Ready) => match self.ready.take() {
                Some(ready) => {
                    // What it said of its start before is over.
                    self.progress.held = None;
                    self.talk.log.note("host", "ready");
                    ready.send(Ok(())).ok();
                    (self.on_event)(ServiceEvent::Ready);
                }
                None => self.talk.log.line("ready", "again"),
            },
            Msg::Service(ServiceMessage::Response { id, result, error }) => self.respond(&id, result, error),
            Msg::Service(ServiceMessage::Emit { event, data }) => self.emit(event, data),
            // Never what it carries, in the log too: a page may get a secret.
            Msg::TooLong(id) => {
                let larger = failed(format!("{}'s answer is larger than {} MB.", self.name, MAX_SERVICE_LINE >> 20));
                // Without its id, it answers the one request that waits, if only one does.
                let id = id.or_else(|| {
                    let pending = lock(&self.pending);
                    (pending.len() == 1).then(|| pending.keys().next().cloned()).flatten()
                });
                match id {
                    Some(id) => self.answer(&id, "too long", Err(larger)),
                    None => self.talk.log.line("response", "too long, without an id: dropped"),
                }
            }
            Msg::Send { line, tag, text } => {
                self.talk.log.line(tag, &text);
                self.talk.send(line);
            }
            // A run's or a call's, which the talk took.
            _ => {}
        }
    }

    /// The latest it said of its start, once it may go at `now`.
    fn report(&mut self, now: Instant) {
        if let Some(progress) = self.progress.due(now) {
            (self.on_event)(progress);
        }
    }

    /// Its response to the request `id`: its result, or its fault, whose
    /// message goes to the page and whose detail to the log.
    fn respond(&mut self, id: &str, result: Value, error: Option<protocol::ServiceFault>) {
        let Some(fault) = error else { return self.answer(id, "ok", Ok(result)) };
        let detail = fault.detail.as_deref().map(|d| format!(" ({d})")).unwrap_or_default();
        self.talk.log.line("failed", &format!("{} {}{detail}", shown(id), fault.message));
        let message = sanitize(&fault.message, MAX_MESSAGE_CHARS);
        let message = if message.is_empty() { format!("{} couldn't do that.", self.name) } else { message };
        self.answer(id, "failed", Err(failed(message)));
    }

    /// Gives the request `id` its `answer`, said as `said` in the log, while
    /// it still waits.
    fn answer(&mut self, id: &str, said: &str, answer: Result<Value, CallError>) {
        let waiter = lock(&self.pending).remove(id);
        let id = shown(id);
        match waiter {
            Some(waiter) => {
                self.talk.log.line("response", &format!("{id} {said}"));
                waiter.send(answer).ok();
            }
            None => self.talk.log.line("response", &format!("{id} {said}, which no request waits for: dropped")),
        }
    }

    /// An event for its open pages: of a name of [`MAX_EVENT_CHARS`] of
    /// `[a-z0-9._-]`, carrying at most [`MAX_EVENT_DATA`], and at most
    /// [`MAX_EMITS_PER_SEC`] a second.
    fn emit(&mut self, event: String, data: Value) {
        let named = (1..=MAX_EVENT_CHARS).contains(&event.len())
            && event.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b));
        if !named {
            return self
                .talk
                .log
                .line("emit", &format!("{} dropped: an event's name is 1 to 64 of a-z, 0-9, . _ -", shown(&event)));
        }
        if serde_json::to_vec(&data).map_or(true, |d| d.len() > MAX_EVENT_DATA) {
            return self.talk.log.line("emit", &format!("{event} dropped: it carries more than 256 KB"));
        }
        if self.emits.take(Instant::now(), &mut self.talk) {
            (self.on_event)(ServiceEvent::Emit { event, data });
        }
    }
}

/// The events a service emitted in the second that its first one since
/// started, and those dropped.
#[derive(Default)]
struct Emits {
    since: Option<Instant>,
    sent: u32,
    dropped: u32,
}

impl Emits {
    /// Whether one more may go at `now`; past [`MAX_EMITS_PER_SEC`] in a
    /// second it is dropped, and the log says how many once a second.
    fn take(&mut self, now: Instant, talk: &mut Talk) -> bool {
        if self.since.is_none_or(|since| now.saturating_duration_since(since) >= Duration::from_secs(1)) {
            self.flush(talk);
            *self = Self { since: Some(now), ..Self::default() };
        }
        if self.sent >= MAX_EMITS_PER_SEC {
            self.dropped += 1;
            return false;
        }
        self.sent += 1;
        true
    }

    fn flush(&mut self, talk: &mut Talk) {
        if self.dropped > 0 {
            let line = format!("{} events dropped: at most {MAX_EMITS_PER_SEC} a second", self.dropped);
            talk.log.line("host", &line);
            self.dropped = 0;
        }
    }
}

/// What a starting service said of its progress and hasn't reached the
/// host yet, and when the last of it did.
#[derive(Default)]
struct Progress {
    sent: Option<Instant>,
    held: Option<ServiceEvent>,
}

impl Progress {
    /// What it holds, once the last sent is [`MAX_PROGRESS_PER_SEC`]'s share
    /// of a second back at `now`.
    fn due(&mut self, now: Instant) -> Option<ServiceEvent> {
        let every = Duration::from_secs(1) / MAX_PROGRESS_PER_SEC;
        if self.sent.is_some_and(|sent| now.saturating_duration_since(sent) < every) {
            return None;
        }
        let held = self.held.take()?;
        self.sent = Some(now);
        Some(held)
    }
}

/// What a service's stdout line is: one of its own messages, a response
/// cut at its limit, or as a run reads it (a call, a message, output).
fn service_line(line: String, was_cut: bool, load: &Arc<Load>) -> Msg {
    if !was_cut && let Some(m) = ServiceMessage::read(&line) {
        return Msg::Service(m);
    }
    if was_cut && let Some(id) = protocol::cut_response(&line) {
        return Msg::TooLong(id);
    }
    runtime::stdout_line(line, was_cut, Some(load))
}

/// A request's failure, said as `message`.
fn failed(message: String) -> CallError {
    CallError::new(CallCode::Failed, message)
}

/// `m`'s data, also when a thread that held it panicked: what it guards
/// stays whole (a map, a flag).
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cut_response_whose_id_comes_last_is_no_line_for_the_log() {
        let load = Arc::default();
        let cut = format!(r#"{{"type":"response","result":{{"token":"SECRET-abc123","pad":"{}"#, "x".repeat(100));
        assert!(matches!(service_line(cut, true, &load), Msg::TooLong(None)));
        let cut = r#"{"type":"response","id":"r1","result":{"token":"SECRET"#.to_owned();
        assert!(matches!(service_line(cut, true, &load), Msg::TooLong(Some(id)) if id == "r1"));
    }

    #[test]
    fn emits_past_the_limit_are_counted_once_a_second() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("x.service.log");
        let (to_plugin, _written) = mpsc::channel();
        let log = PluginLog::open(&log_path, "service-1", "serve").unwrap();
        let mut talk = Talk::new(log, to_plugin, Arc::default(), None, MAX_SERVICE_LINE, "This service is stopping.");
        let start = Instant::now();
        let mut emits = Emits::default();
        let taken = (0..MAX_EMITS_PER_SEC + 5).filter(|_| emits.take(start, &mut talk)).count();
        assert_eq!(taken, MAX_EMITS_PER_SEC as usize);
        // The next second takes them again, and the log says what the last dropped.
        assert!(emits.take(start + Duration::from_secs(1), &mut talk));
        drop(talk);
        let log = std::fs::read_to_string(&log_path).unwrap();
        assert_eq!(log.matches("[host] 5 events dropped: at most 50 a second\n").count(), 1, "{log}");
    }
}

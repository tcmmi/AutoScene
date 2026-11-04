//! Plugin services in the app: at most one per plugin, started when a page
//! of its plugin opens or asks it something ([`PluginHost::service`]), and
//! stopped [`IDLE_STOP`] after its plugin's last page closed ([`reap`]), on
//! any change of the plugin, when it is turned off or the setting goes off
//! for one AutoScene hasn't reviewed, and at quit ([`Services::stop_all`]).
//! Never at the app's start, and never for a run: a service is no job, so
//! it never holds up another plugin's run, nor an update of its own.
//!
//! A service starts once for all its plugin's pages, on a thread of its
//! own: one that asks while it starts waits for that start and fails with
//! it, or gives up once its own time is over while the start goes on for
//! the others. A uv script gets AutoScene's uv first ([`PluginHost`]'s
//! `uv_for`), which its pages hear; its program's time to get ready counts
//! from its start. It is claimed under the busy lock, where a change of its
//! plugin is marked and where the pages close that its plugin may no longer
//! show: a change refuses its start, and a page that closed starts none. A
//! stop waits for a start on its way, which ends its program, and for
//! another stop of it: each returns once its program ended. Once AutoScene
//! quits, none starts. What it says while it starts, its events and its end
//! reach each open page of its plugin while it is its plugin's service: an
//! older start says nothing more. One that crashes starts again when a page
//! asks it, until [`MAX_CRASHES`] crashes in [`CRASH_WINDOW`]; then it is
//! refused until a page of its plugin opens again. A running service's
//! calls hold the host, as a run's do: it stops at quit at the latest.
//!
//! A service of a plugin that uses the GPU runs beside the analysis: its
//! program is the plugin's to keep light (the guide says so), and its runs
//! still wait for the analysis.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use as_core::CancelToken;
use as_plugin::pages::Mount;
use as_plugin::service::{ProcessService, STOP_GRACE};
use as_plugin::{
    CallCode, CallError, HostCalls, Installed, OnEvent, PluginError, PluginId, PluginResult, Service, ServiceEvent,
    ServiceSpec, Trust,
};
use parking_lot::{Condvar, Mutex};
use tauri::{AppHandle, Manager};

use super::calls::AppCalls;
use super::{PluginHost, runnable};
use crate::events::{PageEvent, PluginPageEvent, ServiceStatus};
use crate::state::AppState;

/// How long a service runs on once its plugin's last page closed.
pub const IDLE_STOP: Duration = Duration::from_secs(60);
/// The crashes that count, and how many of them stop its starts.
pub const CRASH_WINDOW: Duration = Duration::from_secs(300);
pub const MAX_CRASHES: usize = 3;
/// How long a service may take to stop as the app quits.
pub const QUIT_GRACE: Duration = Duration::from_secs(1);
/// How often idle services are looked for.
const REAP_EVERY: Duration = Duration::from_secs(5);
/// How often one that waits for a start looks whether its page closed.
const WAIT_TICK: Duration = Duration::from_millis(100);

/// How a plugin's service starts: its program (the app's [`ProgramStart`]),
/// or a test's stand-in.
pub trait StartService: Send + Sync {
    /// Starts the service of `p` for `host`, a uv script through `uv`,
    /// telling `events` how it goes, until `cancel` fires: the service once
    /// it is ready.
    fn start(
        &self,
        host: &Arc<PluginHost>,
        p: &Installed,
        uv: Option<PathBuf>,
        events: OnEvent,
        cancel: &CancelToken,
    ) -> PluginResult<Arc<dyn Service>>;
}

/// The plugins' services.
pub struct Services {
    inner: Mutex<Inner>,
    /// Told whenever a start or a stop ends.
    settled: Condvar,
    /// Held while pages are told of a service, so that they hear what
    /// happened in the order it happened.
    told: Mutex<()>,
    start: Box<dyn StartService>,
}

#[derive(Default)]
struct Inner {
    slots: HashMap<PluginId, Slot>,
    /// When each plugin's service crashed, the last [`CRASH_WINDOW`].
    crashes: HashMap<PluginId, VecDeque<Instant>>,
    /// How many starts there were: each start's own number.
    starts: u64,
    /// The last start of each plugin that failed or was stopped, and why:
    /// what the pages that waited for it get.
    failed: HashMap<PluginId, (u64, CallError)>,
    /// AutoScene quits: no service starts any more.
    quitting: bool,
}

/// A plugin's service, as its pages find it.
enum Slot {
    Starting {
        start: u64,
        /// Fires when a stop comes first: the start ends its program.
        cancel: CancelToken,
        /// Its plugin's trust as its page opened.
        trust: Trust,
        /// Its plugin's settings changed after its serve line was made: it
        /// hears them as they are once it runs.
        stale: bool,
    },
    Running {
        start: u64,
        service: Arc<dyn Service>,
        trust: Trust,
        /// Since when its plugin has no page open.
        idle: Option<Instant>,
    },
    /// Being stopped: another stop of it, and a page that wants it, wait
    /// until its program ended.
    Stopping { start: u64, service: Arc<dyn Service> },
    /// It kept stopping: it starts again once a page of its plugin opens.
    Failed(String),
}

impl Slot {
    /// The start it is, while it starts, runs or stops.
    fn start(&self) -> Option<u64> {
        match self {
            Self::Starting { start, .. } | Self::Running { start, .. } | Self::Stopping { start, .. } => Some(*start),
            Self::Failed(_) => None,
        }
    }
}

/// What a page that wants its plugin's service gets.
enum Claim {
    Running(Arc<dyn Service>),
    /// Its start, the caller's to make.
    Start {
        start: u64,
        cancel: CancelToken,
    },
    /// The start of this number is on its way, or stops.
    Wait(u64),
}

impl Services {
    /// None running yet; each starts through `start`.
    pub fn new(start: Box<dyn StartService>) -> Self {
        Self { inner: Mutex::default(), settled: Condvar::new(), told: Mutex::new(()), start }
    }

    /// What a page of `id` (named `name`, of `trust`) that wants its
    /// service gets at `now`: the one that runs, the start to make, or a
    /// start or stop to wait for; refused while it kept stopping, and once
    /// AutoScene quits. One that ended gives way to a new one.
    fn claim(&self, id: &PluginId, name: &str, trust: Trust, now: Instant) -> Result<Claim, CallError> {
        let mut inner = self.inner.lock();
        if inner.quitting {
            return Err(failed("AutoScene is quitting."));
        }
        match inner.slots.get(id) {
            Some(Slot::Running { service, .. }) if service.is_running() => return Ok(Claim::Running(service.clone())),
            Some(Slot::Starting { start, .. } | Slot::Stopping { start, .. }) => return Ok(Claim::Wait(*start)),
            Some(Slot::Failed(message)) => return Err(failed(message)),
            Some(Slot::Running { .. }) | None => {}
        }
        let crashes = inner.crashes.entry(id.clone()).or_default();
        while crashes.front().is_some_and(|&at| now.saturating_duration_since(at) >= CRASH_WINDOW) {
            crashes.pop_front();
        }
        if crashes.len() >= MAX_CRASHES {
            let message = format!("{name} keeps stopping. Its log says why.");
            inner.slots.insert(id.clone(), Slot::Failed(message.clone()));
            return Err(failed(&message));
        }
        inner.starts += 1;
        let (start, cancel) = (inner.starts, CancelToken::new());
        inner.slots.insert(id.clone(), Slot::Starting { start, cancel: cancel.clone(), trust, stale: false });
        Ok(Claim::Start { start, cancel })
    }

    /// The start `start` of `id` ended with `started`: it runs from now,
    /// unless a stop came first (then it ends at once). Why it didn't start
    /// is kept for the pages that waited for it. The service that runs, and
    /// whether its plugin's settings changed as it started.
    fn settle(
        &self,
        id: &PluginId,
        start: u64,
        started: PluginResult<Arc<dyn Service>>,
    ) -> Option<(Arc<dyn Service>, bool)> {
        let ours = |slots: &HashMap<PluginId, Slot>| match slots.get(id) {
            Some(Slot::Starting { start: s, cancel, trust, stale }) if *s == start => {
                Some((cancel.is_cancelled(), *trust, *stale))
            }
            _ => None,
        };
        let error = match started {
            Ok(service) => {
                let mut inner = self.inner.lock();
                if let Some((false, trust, stale)) = ours(&inner.slots) {
                    let running = Slot::Running { start, service: service.clone(), trust, idle: None };
                    inner.slots.insert(id.clone(), running);
                    drop(inner);
                    self.settled.notify_all();
                    return Some((service, stale));
                }
                drop(inner);
                // A stop came as it got ready: it ends at once, before its
                // place goes, so the stop that waits for it waits for that.
                service.stop(Duration::ZERO);
                CallError::from(PluginError::Cancelled)
            }
            // Why, for the log: the page hears it in a sentence only (a
            // download's reason, a crash's output). A stop is no failure.
            Err(e) => {
                if !matches!(e, PluginError::Cancelled | PluginError::Closed) {
                    log::warn!("{id}'s service couldn't start: {}", e.detail());
                }
                CallError::from(&e)
            }
        };
        let mut inner = self.inner.lock();
        if ours(&inner.slots).is_some() {
            inner.slots.remove(id);
            inner.failed.insert(id.clone(), (start, error));
        }
        drop(inner);
        self.settled.notify_all();
        None
    }

    /// Why the start `start` of `id` failed or stopped, when it did.
    fn failure(&self, id: &PluginId, start: u64) -> Option<CallError> {
        let inner = self.inner.lock();
        inner.failed.get(id).filter(|(s, _)| *s == start).map(|(_, e)| e.clone())
    }

    /// Waits while the start `start` of `id` is on its way or stops, until
    /// `ended` fires or `until` passes: whether it got that far.
    fn wait_settled(&self, id: &PluginId, start: u64, ended: &CancelToken, until: Option<Instant>) -> bool {
        let mut inner = self.inner.lock();
        loop {
            let waits = matches!(
                inner.slots.get(id),
                Some(Slot::Starting { start: s, .. } | Slot::Stopping { start: s, .. }) if *s == start
            );
            if !waits {
                return true;
            }
            let left = until.map(|until| until.saturating_duration_since(Instant::now()));
            if ended.is_cancelled() || left.is_some_and(|left| left.is_zero()) {
                return false;
            }
            self.settled.wait_for(&mut inner, left.map_or(WAIT_TICK, |left| left.min(WAIT_TICK)));
        }
    }

    /// The service of the start `start` of `id` crashed at `at`: it gives
    /// way, unless a newer one took its place already. Counted either way.
    fn crashed(&self, id: &PluginId, start: u64, at: Instant) {
        let mut inner = self.inner.lock();
        inner.crashes.entry(id.clone()).or_default().push_back(at);
        if matches!(
            inner.slots.get(id),
            Some(Slot::Running { start: s, service, .. }) if *s == start && !service.is_running()
        ) {
            inner.slots.remove(id);
        }
    }

    /// The start of the service of `id`, while it starts, runs or stops.
    fn current(&self, id: &PluginId) -> Option<u64> {
        self.inner.lock().slots.get(id).and_then(Slot::start)
    }

    /// A page of `id` opened: its service may start again, however often
    /// it crashed before.
    pub fn forgive(&self, id: &PluginId) {
        let mut inner = self.inner.lock();
        inner.crashes.remove(id);
        inner.failed.remove(id);
        if matches!(inner.slots.get(id), Some(Slot::Failed(_))) {
            inner.slots.remove(id);
        }
    }

    /// Stops the service of `id`, with `grace` to end, or its start; forgets
    /// how it crashed. Returns once its program ended (a change may replace
    /// its files then), also when another stop of it came first.
    pub fn stop(&self, id: &PluginId, grace: Duration) {
        let mut inner = self.inner.lock();
        inner.crashes.remove(id);
        inner.failed.remove(id);
        let (start, service) = match inner.slots.get(id) {
            Some(Slot::Starting { start, cancel, .. }) => {
                let start = *start;
                cancel.cancel();
                // Its program is ended where it starts.
                while matches!(inner.slots.get(id), Some(Slot::Starting { start: s, .. }) if *s == start) {
                    self.settled.wait(&mut inner);
                }
                return;
            }
            Some(Slot::Running { start, service, .. }) => (*start, service.clone()),
            // Another stop's: this one waits for it, and gives it its own
            // grace when that is shorter.
            Some(Slot::Stopping { start, service }) => {
                let (start, service) = (*start, service.clone());
                drop(inner);
                service.stop(grace);
                let mut inner = self.inner.lock();
                while matches!(inner.slots.get(id), Some(Slot::Stopping { start: s, .. }) if *s == start) {
                    self.settled.wait(&mut inner);
                }
                return;
            }
            Some(Slot::Failed(_)) => {
                inner.slots.remove(id);
                return;
            }
            None => return,
        };
        inner.slots.insert(id.clone(), Slot::Stopping { start, service: service.clone() });
        drop(inner);
        self.end(id, start, &service, grace);
    }

    /// Stops `service`, the start `start` of `id`, which stops, with
    /// `grace`; its place goes once its program ended.
    fn end(&self, id: &PluginId, start: u64, service: &Arc<dyn Service>, grace: Duration) {
        service.stop(grace);
        let mut inner = self.inner.lock();
        if matches!(inner.slots.get(id), Some(Slot::Stopping { start: s, .. }) if *s == start) {
            inner.slots.remove(id);
        }
        drop(inner);
        self.settled.notify_all();
    }

    /// Stops each service (and start) whose plugin's trust `stop` picks.
    pub fn stop_where(&self, stop: impl Fn(Trust) -> bool, grace: Duration) {
        let ids: Vec<PluginId> = {
            let inner = self.inner.lock();
            let trust = |slot: &Slot| match slot {
                Slot::Starting { trust, .. } | Slot::Running { trust, .. } => Some(*trust),
                Slot::Stopping { .. } | Slot::Failed(_) => None,
            };
            inner.slots.iter().filter(|(_, slot)| trust(slot).is_some_and(&stop)).map(|(id, _)| id.clone()).collect()
        };
        for id in ids {
            self.stop(&id, grace);
        }
    }

    /// Stops every service, each with `grace`, all at once, and starts none
    /// from now: AutoScene quits.
    pub fn stop_all(&self, grace: Duration) {
        self.inner.lock().quitting = true;
        // Until none is left: a start claimed before goes once it settled.
        loop {
            let ids: Vec<PluginId> = self.inner.lock().slots.keys().cloned().collect();
            if ids.is_empty() {
                return;
            }
            std::thread::scope(|s| {
                for id in &ids {
                    s.spawn(move || self.stop(id, grace));
                }
            });
        }
    }

    /// Starts them again when a page asks: AutoScene didn't quit after all
    /// (an update's installer didn't start).
    pub fn resume(&self) {
        self.inner.lock().quitting = false;
    }

    /// The service of `id`, while it runs.
    #[cfg(test)]
    fn running(&self, id: &PluginId) -> Option<Arc<dyn Service>> {
        match self.inner.lock().slots.get(id) {
            Some(Slot::Running { service, .. }) if service.is_running() => Some(service.clone()),
            _ => None,
        }
    }

    /// The settings of the plugin `id` changed: its service to tell, while
    /// it runs. One on its way hears them once it runs.
    pub fn settings_changed(&self, id: &PluginId) -> Option<Arc<dyn Service>> {
        match self.inner.lock().slots.get_mut(id) {
            Some(Slot::Running { service, .. }) if service.is_running() => Some(service.clone()),
            Some(Slot::Starting { stale, .. }) => {
                *stale = true;
                None
            }
            _ => None,
        }
    }

    /// Stops each running service whose plugin had no page open (`open`
    /// counts them) for [`IDLE_STOP`] at `now`.
    pub fn reap_idle(&self, open: impl Fn(&PluginId) -> usize, now: Instant) {
        let running: Vec<PluginId> = {
            let inner = self.inner.lock();
            inner.slots.iter().filter(|(_, s)| matches!(s, Slot::Running { .. })).map(|(id, _)| id.clone()).collect()
        };
        // Counted without the lock: the pages have one of their own.
        let open: HashMap<PluginId, usize> = running
            .into_iter()
            .map(|id| {
                let pages = open(&id);
                (id, pages)
            })
            .collect();
        let idle: Vec<(PluginId, u64, Arc<dyn Service>)> = {
            let mut inner = self.inner.lock();
            let mut over = Vec::new();
            for (id, slot) in &mut inner.slots {
                let Slot::Running { start, service, idle, .. } = slot else { continue };
                match open.get(id) {
                    Some(0) => {
                        let since = *idle.get_or_insert(now);
                        if now.saturating_duration_since(since) >= IDLE_STOP {
                            over.push((id.clone(), *start, service.clone()));
                        }
                    }
                    Some(_) => *idle = None,
                    // It started since: it counts from the next look.
                    None => {}
                }
            }
            for (id, start, service) in &over {
                inner.slots.insert(id.clone(), Slot::Stopping { start: *start, service: service.clone() });
            }
            over
        };
        for (id, start, service) in idle {
            self.end(&id, start, &service, STOP_GRACE);
        }
    }
}

impl PluginHost {
    /// The service of the plugin of the open page `m`, which runs: started
    /// when none does, and waited for while it starts or stops, until
    /// `until` (the page gives up then; the start goes on for the others).
    /// Its start's progress, its events and its end reach each open page of
    /// its plugin. Refused once the page closed (all a change of the plugin,
    /// turning it off or the setting going off close it, under the busy lock
    /// this takes too), as busy while the plugin changes, and once AutoScene
    /// quits. The error is the page's.
    pub fn service(self: &Arc<Self>, m: &Mount, until: Option<Instant>) -> Result<Arc<dyn Service>, CallError> {
        loop {
            let claim = {
                let busy = self.busy.lock();
                if m.closed.is_cancelled() {
                    return Err(PluginError::Closed.into());
                }
                if busy.changing.contains(&m.plugin) {
                    return Err(CallError::new(CallCode::Busy, format!("{} is being updated.", m.name)));
                }
                self.services.claim(&m.plugin, &m.name, m.trust, Instant::now())?
            };
            let start = match claim {
                Claim::Running(service) => return Ok(service),
                Claim::Wait(start) => start,
                Claim::Start { start, cancel } => {
                    self.begin(&m.plugin, start, cancel);
                    start
                }
            };
            if !self.services.wait_settled(&m.plugin, start, &m.closed, until) {
                return Err(match m.closed.is_cancelled() {
                    true => PluginError::Closed.into(),
                    false => failed(&format!("{} didn't answer in time.", m.name)),
                });
            }
            // The start it waited for failed: so does this, without a start
            // of its own. After one that stopped, it looks again.
            if let Some(e) = self.services.failure(&m.plugin, start) {
                return Err(e);
            }
        }
    }

    /// Makes the start `start` of the service of `id`, until `cancel`
    /// fires, on a thread of its own, and tells its pages it starts.
    fn begin(self: &Arc<Self>, id: &PluginId, start: u64, cancel: CancelToken) {
        self.tell_service(id, start, PageEvent::ServiceState { state: ServiceStatus::Starting, message: None });
        let (host, plugin) = (self.clone(), id.clone());
        let starting = std::thread::Builder::new().name("plugin-service-start".into()).spawn(move || {
            let events = host.service_events(&plugin, start);
            // Past what every run of a plugin passes (a withdrawal, say), as
            // it is now: its page may have opened before. The setting going
            // off stops it and closes its pages itself.
            let started = host.store.get(&plugin).and_then(|p| {
                let launch = runnable(&p, true)?.launch();
                // Its pages hear that it gets uv, not how far: a stage is all they show.
                let got = std::cell::Cell::new(false);
                let getting = |stage: &str, fraction: Option<f64>| {
                    if fraction.is_none() {
                        got.set(true);
                        events(ServiceEvent::Progress { stage: stage.into(), fraction });
                    }
                };
                // One that can't start on this computer says so as it starts.
                let uv = match &launch {
                    Ok(launch) => host.uv_for(launch, &cancel, &getting)?,
                    Err(_) => None,
                };
                // uv is here: from now it starts (uv getting Python and the
                // packages, then its program), and its pages say just that.
                if got.get() {
                    host.tell_service(
                        &plugin,
                        start,
                        PageEvent::ServiceState { state: ServiceStatus::Starting, message: None },
                    );
                }
                host.services.start.start(&host, &p, uv, events, &cancel)
            });
            host.settle(&plugin, start, started);
        });
        if let Err(e) = starting {
            self.settle(id, start, Err(e.into()));
        }
    }

    /// [`Services::settle`]: a service that runs from now hears its plugin's
    /// settings as they are, when they changed as it started.
    fn settle(&self, id: &PluginId, start: u64, started: PluginResult<Arc<dyn Service>>) {
        let Some((service, true)) = self.services.settle(id, start, started) else { return };
        match self.store.values(id) {
            Ok(values) => service.settings_changed(values),
            Err(e) => log::warn!("{id}'s settings for its service: {}", e.detail()),
        }
    }

    /// Starts the service of the page `m`'s plugin that just opened, off the
    /// caller's thread: the page hears it is ready, or why it isn't.
    pub fn warm_service(self: &Arc<Self>, m: Arc<Mount>) {
        let host = self.clone();
        let warming = std::thread::Builder::new().name("plugin-service-warm".into()).spawn(move || {
            let tell = |event: PageEvent| (host.page_events)(PluginPageEvent { session: m.token.clone(), event });
            match host.service(&m, None) {
                // Told as its events are: one that ended since says so itself.
                Ok(service) => {
                    let _told = host.services.told.lock();
                    if service.is_running() {
                        tell(PageEvent::ServiceState { state: ServiceStatus::Ready, message: None });
                    }
                }
                Err(e) if matches!(e.code, CallCode::Closed | CallCode::Cancelled) => {}
                // Unless a newer start tells it more.
                Err(e) => {
                    let _told = host.services.told.lock();
                    if host.services.current(&m.plugin).is_none() {
                        tell(PageEvent::ServiceState { state: ServiceStatus::Stopped, message: Some(e.message) });
                    }
                }
            }
        });
        if let Err(e) = warming {
            log::warn!("a plugin's service couldn't start: {e}");
        }
    }

    /// Where the start `start` of the service of `id` tells how it goes: to
    /// each open page of its plugin; a crash counts too.
    fn service_events(self: &Arc<Self>, id: &PluginId, start: u64) -> OnEvent {
        let (host, id) = (Arc::downgrade(self), id.clone());
        Arc::new(move |e: ServiceEvent| {
            let Some(host) = host.upgrade() else { return };
            let event = match e {
                ServiceEvent::Progress { stage, .. } => {
                    PageEvent::ServiceState { state: ServiceStatus::Starting, message: Some(stage) }
                }
                ServiceEvent::Ready => PageEvent::ServiceState { state: ServiceStatus::Ready, message: None },
                ServiceEvent::Emit { event, data } => PageEvent::Service { name: event, data },
                ServiceEvent::Stopped { message, crashed } => {
                    let stopped = PageEvent::ServiceState { state: ServiceStatus::Stopped, message: Some(message) };
                    host.tell_service(&id, start, stopped);
                    // Also when a newer start took its place.
                    if crashed {
                        host.services.crashed(&id, start, Instant::now());
                    }
                    return;
                }
            };
            host.tell_service(&id, start, event);
        })
    }

    /// Tells each open page of `id` `event`, of the start `start` of its
    /// service, while that is its service: an older one says nothing more.
    fn tell_service(&self, id: &PluginId, start: u64, event: PageEvent) {
        let _told = self.services.told.lock();
        if self.services.current(id) != Some(start) {
            return;
        }
        for session in self.pages.tokens_of(id) {
            (self.page_events)(PluginPageEvent { session, event: event.clone() });
        }
    }
}

/// Stops the services whose plugin had no page open for [`IDLE_STOP`], on a
/// thread of its own every [`REAP_EVERY`]. Ends with the host.
pub fn reap(host: Weak<PluginHost>) {
    let reaping = std::thread::Builder::new().name("plugin-services".into()).spawn(move || {
        loop {
            std::thread::sleep(REAP_EVERY);
            let Some(host) = host.upgrade() else { return };
            host.services.reap_idle(|id| host.pages.open_count(id), Instant::now());
        }
    });
    if let Err(e) = reaping {
        log::warn!("idle plugin services can't be stopped: {e}");
    }
}

/// The app's way to start a service: its plugin's program, with the app's
/// FFmpeg, answering its calls as a run's.
pub struct ProgramStart(pub AppHandle);

impl StartService for ProgramStart {
    fn start(
        &self,
        host: &Arc<PluginHost>,
        p: &Installed,
        uv: Option<PathBuf>,
        events: OnEvent,
        cancel: &CancelToken,
    ) -> PluginResult<Arc<dyn Service>> {
        let state = self.0.state::<AppState>();
        let tools = state.tools().map_err(|e| PluginError::Invalid(e.message))?;
        let calls = AppCalls::service(state.library.clone(), host.clone(), p)?;
        let spec = serve_spec(host, p, &tools.ffmpeg, &tools.ffprobe, &tools.version, Arc::new(calls), uv)?;
        Ok(ProcessService::start(spec, events, cancel)?)
    }
}

/// The service of `p` as [`ProgramStart`] starts it: the store's
/// [`as_plugin::Store::serve_spec`], through `uv` when it is a uv script.
fn serve_spec(
    host: &PluginHost,
    p: &Installed,
    ffmpeg: &Path,
    ffprobe: &Path,
    ffmpeg_version: &str,
    calls: Arc<dyn HostCalls>,
    uv: Option<PathBuf>,
) -> PluginResult<ServiceSpec> {
    let mut spec = host.store.serve_spec(p, ffmpeg, ffprobe, ffmpeg_version, calls)?;
    spec.uv = uv;
    Ok(spec)
}

/// A start or a request refused, said as `message`.
fn failed(message: &str) -> CallError {
    CallError::new(CallCode::Failed, message)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use as_jobs::{Class, JobRunner};
    use as_plugin::RequestContext;
    use serde_json::{Value, json};

    use super::super::GETTING_UV;
    use super::super::catalog::tests::{Key, NOW, Served, index, withdrawal};
    use super::super::pages;
    use super::super::tests::{
        CATALOG_AT, FakeUv, Heard, TEST_UV, closed, host_cataloged, host_getting_uv, host_starting,
    };
    use super::*;

    /// A service that answers each request with its method, counts the
    /// settings it hears, and takes a moment to stop (a second stop returns
    /// at once).
    #[derive(Default)]
    struct Fake {
        stopping: AtomicBool,
        stopped: AtomicBool,
        settings: AtomicUsize,
    }

    impl Service for Fake {
        fn request(
            &self,
            method: &str,
            _: Value,
            _: RequestContext,
            _: Duration,
            _: &CancelToken,
        ) -> Result<Value, CallError> {
            Ok(json!({ "method": method }))
        }

        fn settings_changed(&self, _: std::collections::BTreeMap<String, as_plugin::SettingValue>) {
            self.settings.fetch_add(1, Ordering::SeqCst);
        }

        fn stop(&self, _: Duration) {
            if self.stopping.swap(true, Ordering::SeqCst) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
            self.stopped.store(true, Ordering::SeqCst);
        }

        fn is_running(&self) -> bool {
            !self.stopped.load(Ordering::SeqCst)
        }
    }

    /// Starts fakes, each `delay` after it is asked (a cancel ends the
    /// wait), or crashes each time.
    #[derive(Clone, Default)]
    struct Starter {
        delay: Duration,
        crash: bool,
        /// The starts asked for, and those that ended.
        asked: Arc<AtomicUsize>,
        ended: Arc<AtomicUsize>,
        made: Arc<Mutex<Vec<Arc<Fake>>>>,
        /// Where each start told how it went.
        events: Arc<Mutex<Vec<OnEvent>>>,
        /// The uv each start was given.
        uv: Arc<Mutex<Vec<Option<PathBuf>>>>,
    }

    impl StartService for Starter {
        fn start(
            &self,
            _: &Arc<PluginHost>,
            _: &Installed,
            uv: Option<PathBuf>,
            events: OnEvent,
            cancel: &CancelToken,
        ) -> PluginResult<Arc<dyn Service>> {
            self.asked.fetch_add(1, Ordering::SeqCst);
            self.uv.lock().push(uv);
            self.events.lock().push(events.clone());
            let until = Instant::now() + self.delay;
            while Instant::now() < until && !cancel.is_cancelled() {
                std::thread::sleep(Duration::from_millis(5));
            }
            let started = if cancel.is_cancelled() {
                Err(PluginError::Cancelled)
            } else if self.crash {
                events(ServiceEvent::Stopped { message: "Service stopped unexpectedly.".into(), crashed: true });
                Err(PluginError::Crashed { name: "Service".into(), detail: "exit code: 3".into() })
            } else {
                let fake = Arc::new(Fake::default());
                self.made.lock().push(fake.clone());
                events(ServiceEvent::Ready);
                Ok(fake as Arc<dyn Service>)
            };
            self.ended.fetch_add(1, Ordering::SeqCst);
            started
        }
    }

    /// The plugin `id` that runs a service and has the global page `main`,
    /// loaded from its folder in `tmp` (with the setting on).
    fn served(host: &PluginHost, lib: &as_project::Library, tmp: &Path, id: &str) -> PluginId {
        let dir = tmp.join(id);
        std::fs::create_dir_all(dir.join("ui")).unwrap();
        for file in ["main.py", "main.py.lock", "ui/index.html"] {
            std::fs::write(dir.join(file), b"").unwrap();
        }
        let manifest = json!({
            "manifestVersion": 2,
            "id": id,
            "version": "1.0.0",
            "name": "Service",
            "description": "Serves its page.",
            "author": {"name": "Someone"},
            "engines": {"autoscene": ">=1.0.0, <2", "protocol": 2},
            "runtime": {"type": "service", "command": {"*": ["uv", "run", "--script", "--locked", "main.py"]}},
            "contributes": {"pages": [{"id": "main", "title": "Main", "entry": "ui/index.html"}]}
        });
        std::fs::write(dir.join("plugin.json"), manifest.to_string()).unwrap();
        lib.set_allows_unreviewed_plugins(true).unwrap();
        host.store.commit(host.store.stage_folder(&dir).unwrap(), true, true).unwrap().record.id
    }

    /// A page of `id`, opened without starting its service.
    fn page_of(host: &PluginHost, id: &PluginId) -> Arc<Mount> {
        let p = host.store.get(id).unwrap();
        let page = p.manifest.as_ref().unwrap().pages[0].clone();
        host.pages.open(&p, as_plugin::Grants::of(&p).unwrap(), &page, None).unwrap().0
    }

    /// Answers no call.
    struct Refused;

    impl HostCalls for Refused {
        fn call(
            &self,
            _: as_plugin::HostMethod,
            _: &serde_json::value::RawValue,
            _: &CancelToken,
        ) -> Result<Value, CallError> {
            Err(CallError::new(CallCode::Invalid, "No calls here."))
        }
    }

    /// What `heard` told the page `session` of its service's state.
    fn states(heard: &Heard, session: &str) -> Vec<ServiceStatus> {
        let heard = heard.lock();
        let states = heard.iter().filter(|e| e.session == session).filter_map(|e| match &e.event {
            PageEvent::ServiceState { state, .. } => Some(*state),
            _ => None,
        });
        states.collect()
    }

    fn wait_for(done: impl Fn() -> bool) {
        let until = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < until, "timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Waits until no start or warm start holds `host` any more, so that it
    /// drops with its database before its folder goes.
    fn alone(host: &Arc<PluginHost>) {
        wait_for(|| Arc::strong_count(host) == 1);
    }

    /// A fake running for `id`, started for a page of `trust`.
    fn running(services: &Services, id: &str, trust: Trust) -> (PluginId, Arc<Fake>) {
        let id = PluginId::parse(id).unwrap();
        let Ok(Claim::Start { start, .. }) = services.claim(&id, "Service", trust, Instant::now()) else { panic!() };
        let fake = Arc::new(Fake::default());
        services.settle(&id, start, Ok(fake.clone() as Arc<dyn Service>)).unwrap();
        (id, fake)
    }

    #[test]
    fn a_service_starts_once_for_two_pages() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-service-once-{}", std::process::id()));
        let starter = Starter { delay: Duration::from_millis(300), ..Starter::default() };
        let (lib, host, heard) = host_starting(&tmp, starter.clone());
        let id = served(&host, &lib, &tmp, "dev.a.service");
        // Its settings show its own log.
        let shown = super::super::info(&host.store.get(&id).unwrap(), &host.store, true);
        assert!(shown.service_log_path.unwrap().ends_with("dev.a.service.service.log"));
        // Each page that opens starts it, and both find one start on its way.
        let first = pages::open(&host, &lib, &id, "main", None).unwrap();
        let second = pages::open(&host, &lib, &id, "main", None).unwrap();
        assert!(first.service && second.service);
        wait_for(|| host.services.running(&id).is_some());
        let m = host.pages.get(&second.session).unwrap();
        let service = host.service(&m, None).unwrap();
        assert_eq!(starter.asked.load(Ordering::SeqCst), 1);
        let context = RequestContext { page: "main".into(), project_id: None };
        let answer = service.request("count", Value::Null, context, Duration::from_secs(1), &CancelToken::new());
        assert_eq!(answer, Ok(json!({"method": "count"})));
        // Both pages heard it start and get ready.
        for session in [&first.session, &second.session] {
            wait_for(|| states(&heard, session).last() == Some(&ServiceStatus::Ready));
            assert_eq!(states(&heard, session).first(), Some(&ServiceStatus::Starting));
        }
        // At quit it stops, and none starts after.
        host.shutdown();
        assert!(starter.made.lock()[0].stopped.load(Ordering::SeqCst));
        assert!(host.services.running(&id).is_none());
        assert_eq!(host.service(&m, None).err().unwrap().message, "AutoScene is quitting.");
        assert_eq!(starter.asked.load(Ordering::SeqCst), 1);
        // It didn't quit after all (an update's installer didn't start): a page's ask starts it again.
        host.resume();
        host.service(&m, None).unwrap();
        assert_eq!(starter.asked.load(Ordering::SeqCst), 2);
        host.shutdown();
        alone(&host);
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_change_stops_the_service_before_it_replaces_the_files() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-service-change-{}", std::process::id()));
        let starter = Starter { delay: Duration::from_millis(50), ..Starter::default() };
        let (lib, host, _) = host_starting(&tmp, starter.clone());
        let id = served(&host, &lib, &tmp, "dev.a.service");
        let m = page_of(&host, &id);
        host.service(&m, None).unwrap();
        let fake = starter.made.lock()[0].clone();
        // Once the change may go on, its program has ended, and its page closed.
        let changing = host.change(&id, "Service", "It is being updated.").unwrap();
        assert!(fake.stopped.load(Ordering::SeqCst));
        assert!(host.services.running(&id).is_none());
        assert!(m.closed.is_cancelled());
        assert_eq!(host.service(&m, None).err().unwrap().code, CallCode::Closed);
        // A page still open meanwhile is told it is busy.
        let opened = page_of(&host, &id);
        let busy = CallError::new(CallCode::Busy, "Service is being updated.");
        assert_eq!(host.service(&opened, None).err(), Some(busy));
        drop(changing);

        // A start on its way when the change comes ends before it goes on.
        let slow = Starter { delay: Duration::from_secs(5), ..Starter::default() };
        let other = tmp.join("slow");
        let (lib2, host2, _) = host_starting(&other, slow.clone());
        let id2 = served(&host2, &lib2, &other, "dev.a.slow");
        let m2 = page_of(&host2, &id2);
        std::thread::scope(|s| {
            let starting = s.spawn(|| host2.service(&m2, None));
            wait_for(|| slow.asked.load(Ordering::SeqCst) == 1);
            let asked = Instant::now();
            let changing = host2.change(&id2, "Service", "It is being updated.").unwrap();
            assert_eq!(slow.ended.load(Ordering::SeqCst), 1, "the start ended first");
            assert!(asked.elapsed() < Duration::from_secs(3), "{:?}", asked.elapsed());
            assert_eq!(starting.join().unwrap().err().unwrap().code, CallCode::Cancelled);
            drop(changing);
        });
        alone(&host2);
        drop((host, lib, host2, lib2));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn turning_a_plugin_off_stops_its_service() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-service-off-{}", std::process::id()));
        let starter = Starter::default();
        let (lib, host, _) = host_starting(&tmp, starter.clone());
        let id = served(&host, &lib, &tmp, "dev.a.service");
        let m = page_of(&host, &id);
        host.service(&m, None).unwrap();
        let jobs = JobRunner::new(&[(Class::Plugin, 1)], |_| {});
        host.stop(&jobs, &id);
        assert!(m.closed.is_cancelled());
        assert!(starter.made.lock()[0].stopped.load(Ordering::SeqCst));
        assert!(host.services.current(&id).is_none());
        alone(&host);
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_withdrawn_plugins_service_starts_no_more() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-service-withdrawn-{}", std::process::id()));
        let starter = Starter::default();
        let (lib, host, _) = host_starting(&tmp, starter.clone());
        let id = served(&host, &lib, &tmp, "dev.a.service");
        // Its page opened before the catalog withdrew it, and its service stopped.
        let m = page_of(&host, &id);
        host.service(&m, None).unwrap();
        starter.made.lock()[0].stop(Duration::ZERO);
        let reason = "It sent export paths to a server.";
        let withdrawn =
            as_plugin::catalog::Withdrawal { id: id.clone(), versions: "*".into(), reason: reason.into(), since: 1 };
        let state = as_plugin::catalog::CatalogState { withdrawn: vec![withdrawn], ..Default::default() };
        host.store.set_catalog(Arc::new(as_plugin::catalog::Catalog::new(None, state)));
        assert_eq!(host.service(&m, None).err().unwrap().message, format!("AutoScene withdrew Service. {reason}"));
        assert_eq!(starter.asked.load(Ordering::SeqCst), 1);
        alone(&host);
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_withdrawn_plugin_s_pages_close_and_its_service_stops() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-service-withdrawn-new-{}", std::process::id()));
        let starter = Starter::default();
        let (key, site) = (Key::new(), Arc::new(Served::default()));
        let (lib, host, heard) = host_cataloged(&tmp, starter.clone(), &key, &site);
        let id = served(&host, &lib, &tmp, "dev.a.service");
        let m = page_of(&host, &id);
        host.service(&m, None).unwrap();
        // An index that withdraws nothing leaves it be.
        let jobs = JobRunner::new(&[(Class::Plugin, 1)], |_| {});
        site.index(CATALOG_AT, &key, &index(1, vec![], vec![]));
        let (_, withdrawn, _) = super::super::catalog::apply(&host, &jobs, NOW, true);
        assert!(withdrawn.is_empty() && !m.closed.is_cancelled());
        assert!(host.services.current(&id).is_some());

        // The next withdraws it: its page closes and says why, and its program ends.
        site.index(CATALOG_AT, &key, &index(2, vec![], vec![withdrawal("dev.a.service", "*")]));
        let (_, withdrawn, changed) = super::super::catalog::apply(&host, &jobs, NOW + 60, true);
        assert_eq!((withdrawn, changed), (vec![id.clone()], true));
        assert!(m.closed.is_cancelled());
        assert!(starter.made.lock()[0].stopped.load(Ordering::SeqCst));
        assert!(host.services.current(&id).is_none());
        let why = "AutoScene withdrew Service. It sent export paths to a server.";
        assert_eq!(closed(&heard), [(m.token.clone(), why.to_owned())]);
        // Nothing of it opens or starts again.
        assert_eq!(pages::open(&host, &lib, &id, "main", None).err().unwrap().message, why);
        assert_eq!(starter.asked.load(Ordering::SeqCst), 1);
        alone(&host);
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_second_stop_waits_for_the_first() {
        let services = Services::new(Box::new(Starter::default()));
        let (id, fake) = running(&services, "dev.a.stops", Trust::Reviewed);
        std::thread::scope(|s| {
            let first = s.spawn(|| services.stop(&id, STOP_GRACE));
            wait_for(|| fake.stopping.load(Ordering::SeqCst));
            // The fake's own second stop returns at once: this one still
            // waits until its program ended.
            services.stop(&id, STOP_GRACE);
            assert!(!fake.is_running());
            assert!(services.current(&id).is_none());
            first.join().unwrap();
        });
        // A page that wants it meanwhile waits for the stop, then starts anew.
        let (id, fake) = running(&services, "dev.a.again", Trust::Reviewed);
        std::thread::scope(|s| {
            s.spawn(|| services.stop(&id, STOP_GRACE));
            wait_for(|| fake.stopping.load(Ordering::SeqCst));
            let Ok(Claim::Wait(start)) = services.claim(&id, "Service", Trust::Reviewed, Instant::now()) else {
                panic!("a stopping service is waited for")
            };
            assert!(services.wait_settled(&id, start, &CancelToken::new(), None));
            assert!(!fake.is_running());
            assert!(services.failure(&id, start).is_none());
        });
    }

    /// A person who is never asked anything: a service's request needs no
    /// dialog.
    struct Nobody;

    impl super::super::calls::PageIo for Nobody {
        fn in_front(&self) -> bool {
            unreachable!()
        }
        fn copy_text(&self, _: &str) -> Result<(), String> {
            unreachable!()
        }
        fn confirm(&self, _: &str, _: &str, _: &str) -> bool {
            unreachable!()
        }
        fn open_url(&self, _: &str) -> Result<(), String> {
            unreachable!()
        }
        fn save_to(&self, _: &str, _: &str, _: &[as_plugin::calls::FileFilter]) -> Option<std::path::PathBuf> {
            unreachable!()
        }
        fn pick(&self, _: &str, _: &[as_plugin::calls::FileFilter]) -> Option<std::path::PathBuf> {
            unreachable!()
        }
        fn queue_run(
            &self,
            _: &PluginId,
            _: as_plugin::ActionKind,
            _: &str,
            _: Option<as_core::ProjectId>,
            _: Option<as_core::ExportId>,
            _: Option<Value>,
        ) -> crate::error::AppResult<Option<as_core::JobId>> {
            unreachable!()
        }
    }

    #[test]
    fn a_page_asks_its_plugins_service() {
        use super::super::calls::{PageReply, page_call};
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-service-ask-{}", std::process::id()));
        let (lib, host, _) = host_starting(&tmp, Starter::default());
        let id = served(&host, &lib, &tmp, "dev.a.service");
        let session = pages::open(&host, &lib, &id, "main", None).unwrap().session;
        let lib = Arc::new(lib);
        let ask =
            |params: Value| page_call(lib.clone(), &host, &Nobody, &session, "service.request", &params.to_string());
        assert_eq!(
            ask(json!({"method": "count", "params": {"text": "a b"}})),
            PageReply::Ok { result: json!({"method": "count"}) }
        );
        let refused = |reply: PageReply| match reply {
            PageReply::Err { error } => (error.code, error.message),
            other => panic!("{other:?}"),
        };
        let named = (CallCode::Invalid, "service.request names a method of 1 to 64 characters.".to_owned());
        assert_eq!(refused(ask(json!({"method": ""}))), named);
        assert_eq!(refused(ask(json!({"method": "m".repeat(65)}))), named);
        // A page that closed asks nothing more.
        host.close_page(&session);
        assert_eq!(refused(ask(json!({"method": "count"}))).0, CallCode::Closed);
        // One still open while its plugin changes is told it is busy, as
        // B4.2 says: it may ask again once the change is over.
        let changing = host.change(&id, "Service", "It is being updated.").unwrap();
        let opened = page_of(&host, &id);
        let count = json!({"method": "count"}).to_string();
        let busy = page_call(lib.clone(), &host, &Nobody, &opened.token, "service.request", &count);
        assert_eq!(refused(busy), (CallCode::Busy, "Service is being updated.".to_owned()));
        drop(changing);
        let again = page_call(lib.clone(), &host, &Nobody, &opened.token, "service.request", &count);
        assert_eq!(again, PageReply::Ok { result: json!({"method": "count"}) });
        host.shutdown();
        alone(&host);
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_page_gives_up_on_a_slow_start_at_its_time_and_the_start_goes_on() {
        use super::super::calls::{PageReply, page_call};
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-service-late-{}", std::process::id()));
        let starter = Starter { delay: Duration::from_millis(1500), ..Starter::default() };
        let (lib, host, _) = host_starting(&tmp, starter.clone());
        let id = served(&host, &lib, &tmp, "dev.a.service");
        let session = page_of(&host, &id).token.clone();
        let lib = Arc::new(lib);
        let ask =
            |params: Value| page_call(lib.clone(), &host, &Nobody, &session, "service.request", &params.to_string());
        let asked = Instant::now();
        let late = ask(json!({"method": "count", "timeoutMs": 300}));
        let PageReply::Err { error } = late else { panic!("{late:?}") };
        assert_eq!((error.code, error.message.as_str()), (CallCode::Failed, "Service didn't answer in time."));
        assert!(asked.elapsed() < Duration::from_millis(1200), "{:?}", asked.elapsed());
        // Its start went on: the next request finds it.
        wait_for(|| host.services.running(&id).is_some());
        assert_eq!(ask(json!({"method": "count"})), PageReply::Ok { result: json!({"method": "count"}) });
        assert_eq!(starter.asked.load(Ordering::SeqCst), 1);
        host.shutdown();
        alone(&host);
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn settings_that_change_as_it_starts_reach_it_once_it_runs() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-service-settings-{}", std::process::id()));
        let starter = Starter { delay: Duration::from_millis(300), ..Starter::default() };
        let (lib, host, _) = host_starting(&tmp, starter.clone());
        let id = served(&host, &lib, &tmp, "dev.a.service");
        let m = page_of(&host, &id);
        std::thread::scope(|s| {
            let starting = s.spawn(|| host.service(&m, None));
            wait_for(|| starter.asked.load(Ordering::SeqCst) == 1);
            host.settings_changed(&id);
            starting.join().unwrap().unwrap();
        });
        let fake = starter.made.lock()[0].clone();
        wait_for(|| fake.settings.load(Ordering::SeqCst) == 1);
        // Once it runs, a change reaches it at once.
        host.settings_changed(&id);
        assert_eq!(fake.settings.load(Ordering::SeqCst), 2);
        host.shutdown();
        alone(&host);
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn what_an_older_start_says_never_reaches_the_pages() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-service-older-{}", std::process::id()));
        let starter = Starter::default();
        let (lib, host, heard) = host_starting(&tmp, starter.clone());
        let id = served(&host, &lib, &tmp, "dev.a.service");
        let m = page_of(&host, &id);
        host.service(&m, None).unwrap();
        // It crashed, and the page's next request starts another before
        // the first says it stopped.
        starter.made.lock()[0].stopped.store(true, Ordering::SeqCst);
        host.service(&m, None).unwrap();
        assert_eq!(starter.asked.load(Ordering::SeqCst), 2);
        let first = starter.events.lock()[0].clone();
        first(ServiceEvent::Stopped { message: "Service stopped unexpectedly.".into(), crashed: true });
        first(ServiceEvent::Emit { event: "late".into(), data: Value::Null });
        let said = states(&heard, &m.token);
        assert_eq!(said.last(), Some(&ServiceStatus::Ready), "{said:?}");
        assert!(!said.contains(&ServiceStatus::Stopped));
        assert!(!heard.lock().iter().any(|e| matches!(&e.event, PageEvent::Service { .. })));
        // The newer one runs on, and the crash counted.
        assert!(host.services.running(&id).is_some());
        assert_eq!(host.services.inner.lock().crashes[&id].len(), 1);
        host.shutdown();
        alone(&host);
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn turning_the_setting_off_stops_unreviewed_services() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-service-allow-{}", std::process::id()));
        let (lib, host, _) = host_starting(&tmp, Starter::default());
        let (reviewed, kept) = running(&host.services, "dev.a.reviewed", Trust::Reviewed);
        let (pinned, signed) = running(&host.services, "dev.a.pinned", Trust::Pinned);
        let (folder, loaded) = running(&host.services, "dev.a.folder", Trust::Unreviewed);
        let jobs = JobRunner::new(&[(Class::Plugin, 1)], |_| {});
        // On, or staying on, stops nothing.
        host.allow_changed(&jobs, false, true);
        host.allow_changed(&jobs, true, true);
        assert!([&kept, &signed, &loaded].iter().all(|f| f.is_running()));
        // Off: each service of a plugin AutoScene hasn't reviewed stops.
        host.allow_changed(&jobs, true, false);
        assert!(kept.is_running() && !signed.is_running() && !loaded.is_running());
        assert!(host.services.running(&reviewed).is_some());
        assert!(host.services.running(&pinned).is_none() && host.services.running(&folder).is_none());
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn an_idle_service_stops_after_a_minute() {
        let services = Services::new(Box::new(Starter::default()));
        let (id, fake) = running(&services, "dev.a.idle", Trust::Reviewed);
        let start = Instant::now();
        let at = |s: u64| start + Duration::from_secs(s);
        let (none, one) = (|_: &PluginId| 0, |_: &PluginId| 1);
        // Its last page closed: a minute from the first look that finds none.
        services.reap_idle(none, at(0));
        services.reap_idle(none, at(59));
        assert!(fake.is_running());
        // A page opened meanwhile: it counts again from the next look.
        services.reap_idle(one, at(60));
        services.reap_idle(none, at(61));
        services.reap_idle(none, at(120));
        assert!(fake.is_running());
        services.reap_idle(none, at(121));
        assert!(!fake.is_running() && services.current(&id).is_none());
    }

    #[test]
    fn three_crashes_in_five_minutes_stop_restarts() {
        let services = Services::new(Box::new(Starter::default()));
        let id = PluginId::parse("dev.a.crashes").unwrap();
        let start = Instant::now();
        let at = |s: u64| start + Duration::from_secs(s);
        let claim =
            |s: u64| services.claim(&id, "Service", Trust::Reviewed, at(s)).map(|c| matches!(c, Claim::Start { .. }));
        // Two that count, and one past the window.
        for s in [0, 200, 290] {
            services.crashed(&id, 0, at(s));
        }
        let Ok(Claim::Start { start: n, .. }) = services.claim(&id, "Service", Trust::Reviewed, at(301)) else {
            panic!("two crashes in the window don't stop it")
        };
        // That start crashed too: the third in five minutes.
        services.crashed(&id, n, at(302));
        let crashed = PluginError::Crashed { name: "Service".into(), detail: String::new() };
        assert!(services.settle(&id, n, Err(crashed)).is_none());
        let keeps = "Service keeps stopping. Its log says why.";
        assert_eq!(claim(303).unwrap_err().message, keeps);
        // Refused from then on, however long ago they were…
        assert_eq!(claim(10_000).unwrap_err().message, keeps);
        // …until a page of it opens again.
        services.forgive(&id);
        assert!(claim(10_001).unwrap());

        // Through a page, each crash counts.
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-service-crash-{}", std::process::id()));
        let crashing = Starter { crash: true, ..Starter::default() };
        let (lib, host, heard) = host_starting(&tmp, crashing.clone());
        let id = served(&host, &lib, &tmp, "dev.a.crashes");
        let m = page_of(&host, &id);
        for _ in 0..MAX_CRASHES {
            assert_eq!(host.service(&m, None).err().unwrap().message, "Service stopped unexpectedly.");
        }
        assert_eq!(host.service(&m, None).err().unwrap().message, keeps);
        assert_eq!(crashing.asked.load(Ordering::SeqCst), MAX_CRASHES);
        // Its page heard each start, and each end.
        let said = states(&heard, &m.token);
        assert_eq!(said.iter().filter(|s| **s == ServiceStatus::Starting).count(), MAX_CRASHES);
        assert_eq!(said.iter().filter(|s| **s == ServiceStatus::Stopped).count(), MAX_CRASHES);
        // A page that opens forgives it, and starts it again.
        pages::open(&host, &lib, &id, "main", None).unwrap();
        wait_for(|| crashing.asked.load(Ordering::SeqCst) == MAX_CRASHES + 1);
        alone(&host);
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn pages_that_wait_for_a_start_that_crashes_fail_with_it() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-service-wait-{}", std::process::id()));
        let crashing = Starter { crash: true, delay: Duration::from_millis(300), ..Starter::default() };
        let (lib, host, _) = host_starting(&tmp, crashing.clone());
        let id = served(&host, &lib, &tmp, "dev.a.crashes");
        let pages = [page_of(&host, &id), page_of(&host, &id), page_of(&host, &id)];
        let said: Vec<String> = std::thread::scope(|s| {
            let asked: Vec<_> =
                pages.iter().map(|m| s.spawn(|| host.service(m, None).err().unwrap().message)).collect();
            asked.into_iter().map(|a| a.join().unwrap()).collect()
        });
        // One start, one crash: the others start none of their own.
        assert_eq!(said, vec!["Service stopped unexpectedly."; 3]);
        assert_eq!(crashing.asked.load(Ordering::SeqCst), 1);
        // The next asks again.
        host.service(&pages[0], None).err().unwrap();
        assert_eq!(crashing.asked.load(Ordering::SeqCst), 2);
        alone(&host);
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_uv_service_gets_uv_before_it_starts() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-service-uv-{}", std::process::id()));
        let (starter, uv) = (Starter::default(), FakeUv::missing(false));
        let (lib, host, heard) = host_getting_uv(&tmp, starter.clone(), uv.clone());
        let id = served(&host, &lib, &tmp, "dev.a.service");
        let page = pages::open(&host, &lib, &id, "main", None).unwrap();
        wait_for(|| host.services.running(&id).is_some());
        // Got once, and given to the start: its program starts through it.
        assert_eq!(uv.got.load(Ordering::SeqCst), 1);
        assert_eq!(*starter.uv.lock(), [Some(PathBuf::from(TEST_UV))]);
        // Its page heard it get uv as it started, then that it is ready.
        wait_for(|| states(&heard, &page.session).last() == Some(&ServiceStatus::Ready));
        let said: Vec<_> = heard
            .lock()
            .iter()
            .filter_map(|e| match &e.event {
                PageEvent::ServiceState { state: ServiceStatus::Starting, message } => Some(message.clone()),
                _ => None,
            })
            .collect();
        // Once uv is here, "Getting uv" is no longer what it does.
        assert_eq!(said, [None, Some(GETTING_UV.to_owned()), None]);
        // Its program starts through that uv.
        let p = host.store.get(&id).unwrap();
        let spec = serve_spec(
            &host,
            &p,
            Path::new("ffmpeg"),
            Path::new("ffprobe"),
            "9.0",
            Arc::new(Refused),
            Some(TEST_UV.into()),
        );
        assert_eq!(spec.unwrap().uv, Some(PathBuf::from(TEST_UV)));
        // The next start finds it here, and says nothing of it.
        heard.lock().clear();
        host.services.stop(&id, STOP_GRACE);
        let m = host.pages.get(&page.session).unwrap();
        host.service(&m, None).unwrap();
        assert_eq!(uv.got.load(Ordering::SeqCst), 1);
        assert_eq!(starter.uv.lock().len(), 2);
        let starting = heard
            .lock()
            .iter()
            .any(|e| matches!(&e.event, PageEvent::ServiceState { state: ServiceStatus::Starting, message: Some(_) }));
        assert!(!starting);
        host.shutdown();
        alone(&host);
        drop((host, lib));

        // When uv doesn't come, its program never starts, and the page hears why.
        let tmp2 = tmp.join("failing");
        let (lib, host, _) = host_getting_uv(&tmp2, starter.clone(), FakeUv::missing(true));
        let id = served(&host, &lib, &tmp2, "dev.a.service");
        let m = page_of(&host, &id);
        let e = host.service(&m, None).err().unwrap();
        assert_eq!(e.message, "AutoScene couldn't get uv. Check the internet connection, then try again.");
        assert_eq!(starter.asked.load(Ordering::SeqCst), 2);
        alone(&host);
        drop((host, lib));
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}

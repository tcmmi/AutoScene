//! Running a process plugin: one [`run`] per action, spoken to with the
//! protocol ([`crate::protocol`]).
//!
//! The program starts by its path in the plugin's folder (a uv script's
//! too, through the uv the host got: AutoScene's own, [`crate::uv`]), in
//! the plugin's data folder, with only the environment variables of
//! [`allowed_env`] and a few the run sets. A program of protocol 2 also
//! gets a scratch folder of its own as its temp folder ([`crate::scratch`]),
//! which is removed once the run has ended. It starts as the root of an
//! [`as_media::proc::Tree`], so everything it starts ends with it. Its
//! stdin is written and its stdout and stderr are read on threads of their
//! own from the start, so a plugin that floods a pipe or never reads its
//! stdin blocks only itself; lines are read no longer than
//! [`MAX_LINE_BYTES`], a call line no longer than [`MAX_CALL_LINE_BYTES`].
//! What isn't a message goes to the plugin's log ([`crate::log`]).
//!
//! A cancelled run is asked to stop with a cancel line, and ended with its
//! tree [`CANCEL_GRACE`] later. A plugin that is still running
//! [`EXIT_GRACE`] after its result or error line is ended too, and its line
//! counts. A run is never retried: what a plugin did (a post to a webhook,
//! say) would happen twice.
//!
//! A plugin of protocol 2 may call AutoScene while it runs. Each call is
//! answered on a thread of its own ([`HostCalls`]), at most
//! [`MAX_CALLS_IN_FLIGHT`] at once and [`CALL_QUEUE`] more in turn, so a
//! slow answer never holds up its progress or a cancel. What the run holds
//! for its calls (their params, kept as the text the plugin wrote until
//! the host reads them, and the replies the plugin hasn't read yet) is
//! held to [`MAX_CALL_BYTES`]: past it a call is answered busy, and those
//! waiting start once the plugin reads. The calls being answered then
//! still reply, so for a plugin that reads none it may hold the replies of
//! [`MAX_CALLS_IN_FLIGHT`] calls more; a `net.fetch` reply is the largest,
//! at most twice [`crate::net::MAX_RESPONSE_BODY`]. A plugin that leaves
//! [`MAX_UNREAD`] replies unread is ended. A reply is written while the
//! plugin's stdin is open: a call after its result line is dropped, and so
//! are those still waiting then. Once the run is cancelled, the calls
//! still waiting and each one after are answered that it was, and those
//! being answered are told. The calls still being answered when the
//! plugin ended are told too, and waited for ([`CALL_GRACE`]), so none of
//! them changes anything once its run is over. The log names each call
//! and how it was answered, never its params or its result, not even of a
//! call too long or broken to be one.
//!
//! A plugin's service ([`crate::service`]) starts, is heard and answers its
//! calls with the same pieces: its command and environment, its pipes, and
//! what a run and a service hear alike (`Talk`).

use std::collections::VecDeque;
use std::ffi::OsStr;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::time::{Duration, Instant};
use std::{env, fs, thread};

use as_core::CancelToken;
use as_media::proc;
use serde_json::Value;
use serde_json::value::RawValue;

use crate::calls::{CallCode, CallError, HostCalls, HostMethod};
use crate::error::{PluginError, PluginResult};
use crate::log::PluginLog;
use crate::manifest::{Launch, PluginId, clean, cut, shown};
use crate::protocol::{Call, CallHead, CallId, HostMessage, PluginMessage, Reply, RunMessage, ServiceMessage};
use crate::scratch::Scratch;

/// How long a plugin may take to stop after the cancel line.
pub const CANCEL_GRACE: Duration = Duration::from_secs(5);
/// How long the tree's processes may take to end once asked by the system
/// (SIGTERM, then SIGKILL; Windows ends them at once).
pub const TERM_GRACE: Duration = Duration::from_secs(2);
/// How long a plugin may run on after its result or error line.
pub const EXIT_GRACE: Duration = Duration::from_secs(10);
/// How often a run looks at its plugin while it hears nothing.
pub const TICK: Duration = Duration::from_millis(100);
pub const MAX_LINE_BYTES: usize = 64 << 10;
/// The longest call line of a plugin of protocol 2: the params of the
/// largest call a run may make (`net.fetch`), and room for the rest of it.
pub const MAX_CALL_LINE_BYTES: usize = (5 << 20) + (4 << 10);
pub const MAX_STAGE_CHARS: usize = 80;
pub const MAX_MESSAGE_CHARS: usize = 300;
/// stderr lines kept for the message of a crash.
pub const STDERR_TAIL: usize = 20;
/// Calls of a run AutoScene answers at once; the others wait their turn.
pub const MAX_CALLS_IN_FLIGHT: usize = 4;
/// Calls of a run that may wait their turn; one more is answered busy.
pub const CALL_QUEUE: usize = 64;
/// The most bytes a run holds for its calls: the lines of those read and
/// not yet answered, and the replies its plugin hasn't read. Past it, a
/// call is answered busy, and those waiting start once the plugin reads;
/// the replies of the calls being answered then still come.
pub const MAX_CALL_BYTES: usize = 16 << 20;
/// The replies a plugin may leave unread: one that makes calls and never
/// reads their answers is ended past them.
pub const MAX_UNREAD: usize = 1024;
/// How long a run waits for the calls still being answered once its
/// plugin ended.
pub const CALL_GRACE: Duration = Duration::from_secs(5);

/// The longest error detail a plugin's line hands to the app's log.
const MAX_DETAIL_CHARS: usize = 2000;
/// The longest stderr line kept in a crash's detail.
const TAIL_LINE_CHARS: usize = 500;
/// Lines read but not yet handled. A plugin that writes faster than its
/// log takes them waits on its pipe.
const QUEUE: usize = 256;
/// How long a run waits for the rest of its plugin's output once the tree
/// has ended (a process that left it may hold a pipe open).
pub(crate) const DRAIN: Duration = Duration::from_secs(2);

/// One run of a plugin.
pub struct RunSpec {
    /// The plugin's name, as messages show it.
    pub name: String,
    /// The plugin's folder.
    pub dir: PathBuf,
    pub launch: Launch,
    /// The plugin's own folder for files; the run starts there.
    pub data_dir: PathBuf,
    /// The plugin's log file.
    pub log_path: PathBuf,
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    /// AutoScene's uv ([`crate::uv`]), which the host got for a uv script:
    /// without it, the script doesn't start.
    pub uv: Option<PathBuf>,
    /// The folder of the large files its `plugin.json` names, which the
    /// host got before the run ([`crate::resources`]); none when it names
    /// none. The run line and the environment name it only then.
    pub resources: Option<PathBuf>,
    /// The protocol its plugin declared (`engines.protocol`), which the run
    /// speaks: an older plugin gets the run it always got.
    pub protocol: u32,
    /// The host's answers to its calls (protocol 2); without them, each is
    /// refused. A plugin of protocol 1 makes none: a line like a call is
    /// output for its log, as it always was.
    pub calls: Option<Arc<dyn HostCalls>>,
    pub message: RunMessage,
}

/// Runs a plugin's action to its end: the message of its result line, if
/// it gave one. `report` gets its progress: the stage (cleaned, at most
/// [`MAX_STAGE_CHARS`]; the plugin's name if it named none) and how far it
/// is, if known.
///
/// Fails with the plugin's own error line ([`PluginError::Failed`]), with
/// [`PluginError::Crashed`] when it exits with an error and said neither,
/// and with [`PluginError::Cancelled`] when `cancel` fires before it gave
/// its result (a run cancelled before it starts starts nothing). A cancel
/// after that only stops the wait for it to exit.
///
/// The run line's protocol, data folder, scratch folder and FFmpeg paths are
/// the run's own, whatever `message` held, so they match the environment's.
/// A run of protocol 2 gets a scratch folder ([`crate::scratch`]), removed
/// once its tree has ended. It returns once its calls did too, or [`CALL_GRACE`]
/// after its plugin ended.
pub fn run(spec: RunSpec, cancel: &CancelToken, report: &dyn Fn(&str, Option<f64>)) -> PluginResult<Option<String>> {
    if cancel.is_cancelled() {
        return Err(PluginError::Cancelled);
    }
    let RunSpec { name, dir, launch, data_dir, log_path, ffmpeg, ffprobe, uv, resources, protocol, calls, mut message } =
        spec;
    let mut cmd = command(&name, &dir, &launch, || uv)?;
    let id = message.plugin.id.clone();
    let (data_dir, resources) = prepare(&mut cmd, (&data_dir, resources.as_deref()), &ffmpeg, &ffprobe, &id, protocol)?;
    let mut log = PluginLog::open(&log_path, &message.run_id, &message.action.id)?;
    // Removed as it drops, at the end of the run, whatever ended it.
    let scratch = scratch(&mut cmd, protocol, &mut log, &id);
    message.protocol = protocol;
    message.plugin.data_dir = data_dir.to_string_lossy().into_owned();
    message.plugin.resources_dir = resources.map(|r| r.to_string_lossy().into_owned());
    message.plugin.temp_dir = temp_dir(scratch.as_ref());
    message.tools.ffmpeg = ffmpeg.to_string_lossy().into_owned();
    message.tools.ffprobe = ffprobe.to_string_lossy().into_owned();
    let mut tree = spawn(&mut cmd, &mut log, &id)?;

    let (tx, rx) = mpsc::sync_channel(QUEUE);
    let load = Arc::new(Load::default());
    // A plugin of protocol 1 makes no calls: a line that reads as one is its output.
    let calls = (protocol >= 2).then(|| Calls::new(calls.unwrap_or_else(|| Arc::new(NoCalls)), tx.clone()));
    let shares = calls.is_some().then(|| load.clone());
    let max = if shares.is_some() { MAX_CALL_LINE_BYTES } else { MAX_LINE_BYTES };
    read_stdout(tree.take_stdout().expect("piped stdout"), tx.clone(), max, move |line, was_cut| {
        stdout_line(line, was_cut, shares.as_ref())
    });
    read_stderr(tree.take_stderr().expect("piped stderr"), tx);
    let to_plugin = writer(tree.take_stdin().expect("piped stdin"), load.clone());

    let shown = sanitize(&name, MAX_STAGE_CHARS);
    let talk = Talk::new(log, to_plugin, load, calls, MAX_CALL_LINE_BYTES, "This run was cancelled.");
    let mut heard = Heard { name: &shown, report, outcome: None, talk };
    heard.talk.send(HostMessage::Run(Box::new(message)).line());
    let mut cancel_at: Option<Instant> = None;
    // The tree's status when the run had to end it, else None once it ended.
    let ended = loop {
        if cancel_at.is_none() && cancel.is_cancelled() {
            cancel_at = Some(Instant::now());
            if heard.outcome.is_none() && heard.talk.hears() {
                heard.talk.send(HostMessage::Cancel.line());
                heard.talk.log.note("host", "asked to cancel");
            }
            heard.talk.cancel_calls();
        }
        if heard.outcome.is_none() && heard.talk.unread() {
            heard.said(Said::Unread);
        }
        let overdue = match (cancel_at, heard.outcome.as_ref()) {
            (_, Some(Outcome { said: Said::Unread, .. })) => Some("its replies went unread"),
            // Asked to stop once it had said its last word: nothing to wait for.
            (Some(c), Some(o)) if c >= o.at => Some("cancelled after its result"),
            (Some(c), _) if c.elapsed() >= CANCEL_GRACE => Some("still running after cancel"),
            (None, Some(o)) if o.at.elapsed() >= EXIT_GRACE => Some("still running after its result"),
            _ => None,
        };
        if let Some(why) = overdue {
            heard.talk.log.note("host", &format!("{why}; ended"));
            break Some(tree.kill_tree(TERM_GRACE)?);
        }
        match rx.recv_timeout(TICK) {
            Ok(m) => heard.take(m),
            Err(RecvTimeoutError::Timeout) => {}
            // Both pipes are closed; the process may still run.
            Err(RecvTimeoutError::Disconnected) => thread::sleep(TICK),
        }
        // Calls held back while their replies waited unread start once it reads.
        heard.talk.next_call();
        if tree.has_exited()? {
            break None;
        }
    };
    let status = match ended {
        Some(status) => status,
        None => tree.finish()?,
    };
    heard.talk.end_calls();
    // The reader threads are left to end on their own: the tree's end
    // closed their pipes, unless a process that left the tree holds one.
    // That reader ends when the process closes its pipe or writes to it
    // again. The run waits for none of it past DRAIN, however fast it comes.
    let until = Instant::now() + DRAIN;
    while heard.talk.open > 0 && Instant::now() < until {
        match rx.recv_timeout(until.saturating_duration_since(Instant::now())) {
            Ok(m) => heard.take(m),
            Err(_) => break,
        }
    }
    heard.wait_calls(&rx);
    heard.talk.log.note("host", &status.to_string());
    decide(name, status, cancel_at, heard)
}

/// Readies `cmd`, a plugin's program as [`command`] made it, to start in
/// its data folder `data_dir` (made when it isn't there) with only the
/// environment variables of [`allowed_env`], those [`command`] set for its
/// program, and those AutoScene sets for every plugin, and its three pipes.
/// A plugin that names large files gets their folder `resources` as
/// `AUTOSCENE_RESOURCES`; others get no such variable. Gives the data
/// folder, and that one, as absolute paths. Python writes no bytecode
/// beside a module it imports: the plugin's folder stays as it was
/// installed.
pub(crate) fn prepare(
    cmd: &mut Command,
    (data_dir, resources): (&Path, Option<&Path>),
    ffmpeg: &Path,
    ffprobe: &Path,
    id: &PluginId,
    protocol: u32,
) -> PluginResult<(PathBuf, Option<PathBuf>)> {
    fs::create_dir_all(data_dir)?;
    let data_dir = std::path::absolute(data_dir)?;
    let resources = resources.map(std::path::absolute).transpose()?;
    let own: Vec<_> = cmd.get_envs().filter_map(|(name, value)| Some((name.to_owned(), value?.to_owned()))).collect();
    cmd.current_dir(&data_dir)
        .env_clear()
        .envs(env::vars_os().filter(|(name, _)| allowed_env(name)))
        .envs(own)
        .env("AUTOSCENE_FFMPEG", ffmpeg)
        .env("AUTOSCENE_FFPROBE", ffprobe)
        .env("AUTOSCENE_PLUGIN_DATA", &data_dir)
        .env("AUTOSCENE_PLUGIN_ID", id.as_str())
        .env("AUTOSCENE_PROTOCOL", protocol.to_string())
        .env("PYTHONUNBUFFERED", "1")
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(resources) = &resources {
        cmd.env("AUTOSCENE_RESOURCES", resources);
    }
    Ok((data_dir, resources))
}

/// A scratch folder for `cmd`, the program of the plugin `id`, of
/// `protocol`, from 2 on: its temp folder is `AUTOSCENE_RUN_TEMP`, and the
/// system's temp folder for the program (`TEMP` and `TMP` on Windows,
/// `TMPDIR` on Linux and macOS), so its temp files and those of what it
/// runs land there. A program of protocol 1 gets the environment it always
/// got, and none. So does one whose folder couldn't be made (a full drive,
/// a system temp folder that is gone), which says why in its `log`: its
/// temp files go where they went before, and the run goes on.
pub(crate) fn scratch(cmd: &mut Command, protocol: u32, log: &mut PluginLog, id: &PluginId) -> Option<Scratch> {
    if protocol < 2 {
        return None;
    }
    let scratch = Scratch::new()
        .inspect_err(|e| {
            log.note("host", &format!("couldn't make its temp folder, so it keeps the system's: {e}"));
            tracing::warn!(error = %e, plugin = %id.as_str(), "a plugin run's scratch folder couldn't be made");
        })
        .ok()?;
    let dir = scratch.path();
    cmd.env("AUTOSCENE_RUN_TEMP", dir);
    #[cfg(windows)]
    cmd.env("TEMP", dir).env("TMP", dir);
    #[cfg(unix)]
    cmd.env("TMPDIR", dir);
    Some(scratch)
}

/// The scratch folder as a run or a serve line names it.
pub(crate) fn temp_dir(scratch: Option<&Scratch>) -> Option<String> {
    scratch.map(|s| s.path().to_string_lossy().into_owned())
}

/// Starts `cmd`, the program of the plugin `id`, as the root of a tree;
/// one that can't start says why in its `log`.
pub(crate) fn spawn(cmd: &mut Command, log: &mut PluginLog, id: &PluginId) -> PluginResult<proc::Tree> {
    Ok(proc::spawn_tree(cmd).inspect_err(|e| {
        log.note("host", &format!("couldn't start: {e}"));
        tracing::warn!(error = %e, plugin = %id.as_str(), "a plugin couldn't start");
    })?)
}

/// Who wins once the tree has ended: a cancel before any result or error,
/// then the plugin's line, then its exit status.
fn decide(
    name: String,
    status: ExitStatus,
    cancel_at: Option<Instant>,
    heard: Heard<'_>,
) -> PluginResult<Option<String>> {
    if cancel_at.is_some_and(|c| heard.outcome.as_ref().is_none_or(|o| c < o.at)) {
        return Err(PluginError::Cancelled);
    }
    match heard.outcome.map(|o| o.said) {
        Some(Said::Done(message)) => Ok(message),
        Some(Said::Failed { message, detail }) => Err(PluginError::Failed { message, detail }),
        Some(Said::Unread) => {
            Err(PluginError::Crashed { name, detail: format!("it left {MAX_UNREAD} of AutoScene's replies unread") })
        }
        None if status.success() => Ok(None),
        None => Err(PluginError::Crashed { name, detail: heard.talk.crash_detail(&status) }),
    }
}

/// `text` as the app shows it: without control characters or invisible
/// ones, whitespace collapsed, at most `max_chars` (the last an ellipsis).
pub fn sanitize(text: &str, max_chars: usize) -> String {
    cut(&clean(text), max_chars)
}

/// Whether the environment variable `name` reaches plugins. Every other one
/// is left out: the app's own (`AUTOSCENE_DATA_DIR`, `RUST_LOG`) and
/// whatever secrets the person keeps in theirs. What is passed on is what
/// programs need to work at all (Windows can't start a socket without
/// `SystemRoot`; Python and uv read `PROCESSOR_ARCHITECTURE`), find their
/// files and the person's language, reach the network through its proxy
/// and see the GPU. Windows names are compared in any case.
pub fn allowed_env(name: &OsStr) -> bool {
    let Some(name) = name.to_str() else { return false };
    #[cfg(windows)]
    let (is, starts) = (
        |n: &str| name.eq_ignore_ascii_case(n),
        |p: &str| name.get(..p.len()).is_some_and(|head| head.eq_ignore_ascii_case(p)),
    );
    #[cfg(unix)]
    let (is, starts) = (|n: &str| name == n, |p: &str| name.starts_with(p));
    EVERYWHERE.iter().chain(HERE).any(|n| is(n)) || PREFIXES.iter().any(|p| starts(p))
}

const EVERYWHERE: &[&str] = &[
    "PATH",
    "LANG",
    "TZ",
    "http_proxy",
    "https_proxy",
    "no_proxy",
    "all_proxy",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "ALL_PROXY",
    "CUDA_VISIBLE_DEVICES",
];

#[cfg(windows)]
const HERE: &[&str] = &[
    "SystemRoot",
    "windir",
    "SystemDrive",
    "PATHEXT",
    "COMSPEC",
    "TEMP",
    "TMP",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "ProgramData",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramW6432",
    "CommonProgramFiles",
    "NUMBER_OF_PROCESSORS",
    "PROCESSOR_ARCHITECTURE",
    "PROCESSOR_ARCHITEW6432",
    "OS",
    "USERNAME",
    "HOMEDRIVE",
    "HOMEPATH",
];

#[cfg(unix)]
const HERE: &[&str] = &["HOME", "USER", "LOGNAME", "TMPDIR", "DISPLAY", "WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS"];

#[cfg(windows)]
const PREFIXES: &[&str] = &["CUDA_PATH"];

#[cfg(unix)]
const PREFIXES: &[&str] = &["CUDA_PATH", "LC_", "XDG_"];

/// Whether the file may be run: on Linux and macOS, whether it is marked to.
#[cfg(unix)]
fn runnable(file: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    file.metadata().is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(windows)]
fn runnable(_: &Path) -> bool {
    true
}

/// The command that starts `launch` from the plugin's folder `dir`, by its
/// absolute path: neither the PATH nor the run's folder decides what runs.
/// `uv` gives the uv the host got, for a script ([`crate::uv`]): uv keeps
/// what it fetches (the script's packages, a Python) in the folders beside
/// its own, and reads no configuration file of the person's, which could
/// change where packages come from. Its errors say what is wrong, not what
/// to do: only the host knows whether the plugin can be reinstalled.
pub(crate) fn command(
    name: &str,
    dir: &Path,
    launch: &Launch,
    uv: impl FnOnce() -> Option<PathBuf>,
) -> PluginResult<Command> {
    let entry = || -> PluginResult<PathBuf> {
        let entry = std::path::absolute(launch.entry_in(dir))?;
        if !entry.is_file() {
            return Err(PluginError::Invalid(format!("{name} can't start: one of its files is missing.")));
        }
        Ok(entry)
    };
    match launch {
        Launch::Exe { args, .. } => {
            let program = entry()?;
            if !runnable(&program) {
                return Err(PluginError::Invalid(format!("{name} can't start: its program isn't marked as runnable.")));
            }
            let mut cmd = Command::new(program);
            cmd.args(args);
            Ok(cmd)
        }
        Launch::UvScript { .. } => {
            let uv = std::path::absolute(uv().ok_or(PluginError::UvMissing)?)?;
            let mut cmd = Command::new(&uv);
            cmd.args(["run", "--script", "--locked"]).arg(entry()?);
            // `<runtimes>/uv-<version>/uv`: what it fetches goes to `<runtimes>`.
            if let Some(runtimes) = uv.parent().and_then(Path::parent) {
                cmd.env("UV_CACHE_DIR", runtimes.join(crate::uv::CACHE))
                    .env("UV_PYTHON_INSTALL_DIR", runtimes.join(crate::uv::PYTHONS));
            }
            cmd.env("UV_NO_CONFIG", "1");
            Ok(cmd)
        }
        // The store neither stages nor loads one for a run (it needs a newer AutoScene).
        Launch::Wasm { .. } => Err(PluginError::Invalid(format!("{name} runs in a sandbox, not as a program."))),
    }
}

/// What a reader thread, a call's, or the host hands a run or a service.
pub(crate) enum Msg {
    /// A message other than a call.
    Plugin(PluginMessage),
    /// A call (protocol 2), with its share of what the run holds: none when
    /// it would hold too much, and then without its params.
    Call {
        id: CallId,
        method: String,
        params: Box<RawValue>,
        share: Option<Share>,
    },
    /// A line that starts a call but is none: cut at its limit (`too_long`),
    /// or with an id, a method or JSON that doesn't fit.
    BadCall {
        head: CallHead,
        too_long: bool,
    },
    /// A stdout line that isn't a message.
    Stdout(String),
    Stderr(String),
    /// A pipe reached its end.
    Closed,
    /// The host's answer to a call.
    Answered {
        id: CallId,
        method: HostMethod,
        answer: Result<Value, CallError>,
    },
    /// A service's own message ([`crate::service`]).
    Service(ServiceMessage),
    /// A service's response cut at its limit: the request it answers, when
    /// its id came before the break.
    TooLong(Option<String>),
    /// The host's line for a service's stdin (a request, or its settings),
    /// which its log names as `[tag] text`.
    Send {
        line: String,
        tag: &'static str,
        text: String,
    },
}

/// The plugin's last word, or how the run had it end.
enum Said {
    Done(Option<String>),
    Failed {
        message: String,
        detail: Option<String>,
    },
    /// It left [`MAX_UNREAD`] replies unread, and was ended.
    Unread,
}

struct Outcome {
    said: Said,
    at: Instant,
}

/// What a run has heard from its plugin so far.
struct Heard<'a> {
    /// The plugin's name, as a stage shows it: the stage of a progress line
    /// without one.
    name: &'a str,
    report: &'a dyn Fn(&str, Option<f64>),
    /// The first result or error line; later ones go to the log only.
    outcome: Option<Outcome>,
    talk: Talk,
}

impl Heard<'_> {
    fn take(&mut self, m: Msg) {
        let Some(m) = self.talk.take(m) else { return };
        match m {
            Msg::Plugin(PluginMessage::Progress { fraction, stage }) => {
                let stage = stage.map(|s| sanitize(&s, MAX_STAGE_CHARS)).filter(|s| !s.is_empty());
                let fraction = fraction.filter(|f| f.is_finite()).map(|f| f.clamp(0.0, 1.0));
                (self.report)(stage.as_deref().unwrap_or(self.name), fraction);
            }
            Msg::Plugin(PluginMessage::Log { level, message }) => self.talk.log.line(level.name(), &message),
            Msg::Plugin(PluginMessage::Result { message }) => {
                self.log_said("result", message.as_deref().unwrap_or("(no message)"));
                let message = message.map(|m| sanitize(&m, MAX_MESSAGE_CHARS)).filter(|m| !m.is_empty());
                self.said(Said::Done(message));
            }
            Msg::Plugin(PluginMessage::Error { message, detail }) => {
                let logged = match &detail {
                    Some(detail) => format!("{message} ({detail})"),
                    None => message.clone(),
                };
                self.log_said("failed", &logged);
                let mut message = sanitize(&message, MAX_MESSAGE_CHARS);
                if message.is_empty() {
                    message = format!("{} failed.", self.name);
                }
                let detail = detail.map(|d| sanitize(&d, MAX_DETAIL_CHARS)).filter(|d| !d.is_empty());
                self.said(Said::Failed { message, detail });
            }
            // A service's alone: a run's readers never send them.
            _ => {}
        }
    }

    /// Waits for the calls still being answered, at most [`CALL_GRACE`]:
    /// a change of the plugin waits for its run, so it waits for them too.
    fn wait_calls(&mut self, rx: &Receiver<Msg>) {
        let until = Instant::now() + CALL_GRACE;
        while self.talk.calls_running() > 0 {
            match rx.recv_timeout(until.saturating_duration_since(Instant::now())) {
                Ok(m) => self.take(m),
                Err(_) => break,
            }
        }
        self.talk.calls_left();
    }

    /// Logs a result or error line: the one that counts as the host's word,
    /// so it outlasts a flood, and later ones as the plugin's output.
    fn log_said(&mut self, tag: &str, text: &str) {
        if self.outcome.is_none() {
            self.talk.log.note(tag, text);
        } else {
            self.talk.log.line(tag, text);
        }
    }

    fn said(&mut self, said: Said) {
        if self.outcome.is_none() {
            let when = if matches!(said, Said::Unread) { "at its end" } else { "at the result" };
            self.outcome = Some(Outcome { said, at: Instant::now() });
            // Its stdin closes: the run has nothing more to say, and answers
            // no call that still waits.
            self.talk.hush("after the result", when);
        }
    }
}

/// What a plugin's program hears from the host, and what of its output is
/// the same for a run and a service: its log lines, its stderr, and its
/// calls, answered as [`Talk::call`] says. The run ([`Heard`]) or the
/// service ([`crate::service`]) reads the rest.
pub(crate) struct Talk {
    pub(crate) log: PluginLog,
    /// The last [`STDERR_TAIL`] lines of its stderr, cleaned, for the
    /// detail of a crash.
    tail: VecDeque<String>,
    /// Pipes not yet at their end.
    pub(crate) open: usize,
    /// Its stdin, until it hears no more.
    to_plugin: Option<mpsc::Sender<String>>,
    /// Since when it hears no more, as a call that comes now is dropped.
    hushed: Option<&'static str>,
    /// What the host holds for it.
    load: Arc<Load>,
    /// Its calls, when it may make them.
    calls: Option<Calls>,
    /// Its longest call line, which a call cut at it is told.
    max_call: usize,
    /// What a call that comes once it was cancelled is answered.
    cancelled: &'static str,
}

impl Talk {
    /// A program that hears what `to_plugin` writes (of which `load` counts
    /// what the host holds), logs to `log`, and calls the host with `calls`
    /// in lines of at most `max_call`. Once it was cancelled, its calls are
    /// told `cancelled`.
    pub(crate) fn new(
        log: PluginLog,
        to_plugin: mpsc::Sender<String>,
        load: Arc<Load>,
        calls: Option<Calls>,
        max_call: usize,
        cancelled: &'static str,
    ) -> Self {
        Self {
            log,
            tail: VecDeque::new(),
            open: 2,
            to_plugin: Some(to_plugin),
            hushed: None,
            load,
            calls,
            max_call,
            cancelled,
        }
    }

    /// Takes what a run and a service take alike: output for its log, a
    /// pipe's end, a call and its answer. Gives back the rest.
    pub(crate) fn take(&mut self, m: Msg) -> Option<Msg> {
        match m {
            Msg::Stdout(line) => self.log.line("stdout", &line),
            Msg::Stderr(line) => {
                self.log.line("stderr", &line);
                if self.tail.len() == STDERR_TAIL {
                    self.tail.pop_front();
                }
                // Cleaned: the app's log quotes it in a crash's detail.
                self.tail.push_back(sanitize(&line, TAIL_LINE_CHARS));
            }
            Msg::Closed => self.open = self.open.saturating_sub(1),
            Msg::Call { id, method, params, share } => self.call(id, &method, params, share),
            Msg::BadCall { head, too_long } => self.bad_call(head, too_long),
            Msg::Answered { id, method, answer } => {
                if let Some(calls) = &mut self.calls {
                    calls.running -= 1;
                }
                self.reply(id, method.name(), answer);
                self.next_call();
            }
            m => return Some(m),
        }
        None
    }

    /// Whether it still hears what the host says: its stdin is open.
    pub(crate) fn hears(&self) -> bool {
        self.to_plugin.is_some()
    }

    /// Whether it left more than [`MAX_UNREAD`] lines unread.
    pub(crate) fn unread(&self) -> bool {
        self.load.lines.load(Ordering::Relaxed) > MAX_UNREAD
    }

    /// The detail of a crash that ended with `status`: it, and the tail of
    /// its stderr.
    pub(crate) fn crash_detail(&self, status: &ExitStatus) -> String {
        let tail: Vec<&str> = self.tail.iter().map(String::as_str).collect();
        match tail.is_empty() {
            true => status.to_string(),
            false => format!("{status}; {}", tail.join("\n")),
        }
    }

    /// A call line: answered at once while fewer than
    /// [`MAX_CALLS_IN_FLIGHT`] are, else in turn, and busy without its
    /// `share`. One once its stdin closed is dropped, and one after a cancel
    /// is answered that it was cancelled.
    fn call(&mut self, id: CallId, method: &str, params: Box<RawValue>, share: Option<Share>) {
        if let Some(when) = self.hushed {
            return self.log.line("call", &format!("{} {when}: dropped", shown(method)));
        }
        let Some(calls) = &mut self.calls else { return };
        let Some(m) = HostMethod::parse(method) else {
            let method = shown(method);
            self.log.line("call", &method);
            let message = format!("AutoScene doesn't know the method \"{method}\".");
            return self.reply(id, &method, Err(CallError::new(CallCode::Invalid, message)));
        };
        self.log.line("call", m.name());
        if calls.cancelled {
            let cancelled = CallError::new(CallCode::Cancelled, self.cancelled);
            return self.reply(id, m.name(), Err(cancelled));
        }
        match share {
            Some(share) if calls.waiting.len() < CALL_QUEUE => {
                calls.waiting.push_back(Waiting { id, method: m, params, share });
                self.next_call();
            }
            _ => self.reply(id, m.name(), Err(CallError::new(CallCode::Busy, "Too many calls at once."))),
        }
    }

    /// A line that starts a call but is none: answered invalid when its id
    /// reads, else dropped. Neither it nor its params reach the log.
    fn bad_call(&mut self, head: CallHead, too_long: bool) {
        let method = head.method.as_deref().map_or_else(|| "(unnamed)".to_owned(), shown);
        if let Some(when) = self.hushed {
            return self.log.line("call", &format!("{method} {when}: dropped"));
        }
        let what = if too_long { "too long" } else { "unreadable" };
        let Some(id) = head.id else {
            return self.log.line("call", &format!("{method} {what}, without an id to answer: dropped"));
        };
        self.log.line("call", &format!("{method} {what}"));
        let message = match too_long {
            true => format!("A call is at most {} MB.", self.max_call >> 20),
            false => "AutoScene couldn't read this call.".to_owned(),
        };
        self.reply(id, &method, Err(CallError::new(CallCode::Invalid, message)));
    }

    /// Starts the calls waiting their turn while there is room, and while
    /// the replies its plugin hasn't read stay under [`MAX_CALL_BYTES`].
    pub(crate) fn next_call(&mut self) {
        let Some(calls) = &mut self.calls else { return };
        while calls.running < MAX_CALLS_IN_FLIGHT
            && self.load.bytes.load(Ordering::Relaxed) <= MAX_CALL_BYTES
            && let Some(waiting) = calls.waiting.pop_front()
        {
            let method = waiting.method;
            if let Err(id) = calls.start(waiting) {
                let failed = CallError::new(CallCode::Failed, "AutoScene couldn't answer this call.");
                return self.reply(id, method.name(), Err(failed));
            }
        }
    }

    /// Once it was cancelled: the calls being answered change nothing more,
    /// and those still waiting are answered that it was, as is each one
    /// after.
    pub(crate) fn cancel_calls(&mut self) {
        let Some(calls) = &mut self.calls else { return };
        calls.cancelled = true;
        calls.ended.cancel();
        let waiting: Vec<_> = calls.waiting.drain(..).collect();
        for w in waiting {
            let cancelled = CallError::new(CallCode::Cancelled, self.cancelled);
            self.reply(w.id, w.method.name(), Err(cancelled));
        }
    }

    /// Its stdin closes: it hears nothing more, and the calls still waiting
    /// their turn are dropped, as they are `at` its end or its last word. A
    /// call that comes later is dropped as one that came `after` it.
    pub(crate) fn hush(&mut self, after: &'static str, at: &str) {
        self.to_plugin = None;
        self.hushed.get_or_insert(after);
        self.drop_waiting(at);
    }

    /// Ends its calls once its program ended: those still waiting never
    /// start, and one still being answered changes nothing more.
    pub(crate) fn end_calls(&mut self) {
        if let Some(calls) = &mut self.calls {
            calls.ended.cancel();
        }
        self.hush("after its end", "at its end");
    }

    /// Drops the calls still waiting their turn, each with a line in the log.
    fn drop_waiting(&mut self, when: &str) {
        let Some(calls) = &mut self.calls else { return };
        for waiting in calls.waiting.drain(..) {
            self.log.line("call", &format!("{} still waiting {when}: dropped", waiting.method.name()));
        }
    }

    /// How many of its calls are being answered.
    pub(crate) fn calls_running(&self) -> usize {
        self.calls.as_ref().map_or(0, |c| c.running)
    }

    /// Notes the calls still being answered once the wait for them is over.
    pub(crate) fn calls_left(&mut self) {
        match self.calls_running() {
            0 => {}
            1 => self.log.note("host", "a call still being answered; left"),
            n => self.log.note("host", &format!("{n} calls still being answered; left")),
        }
    }

    /// Logs how a call of `method` was answered, and sends the reply while
    /// the plugin's stdin is open.
    pub(crate) fn reply(&mut self, id: CallId, method: &str, answer: Result<Value, CallError>) {
        let (said, reply) = match answer {
            Ok(result) => ("ok", Reply { id, result: Some(result), error: None }),
            Err(e) => (e.code.name(), Reply { id, result: None, error: Some(e) }),
        };
        self.log.line("reply", &format!("{method} {said}"));
        self.send(HostMessage::Reply(reply).line());
    }

    /// Sends `line` to the plugin while its stdin is open.
    pub(crate) fn send(&mut self, line: String) {
        if let Some(tx) = &self.to_plugin {
            let bytes = line.len();
            self.load.queued(bytes);
            if tx.send(line).is_err() {
                self.load.written(bytes);
            }
        }
    }
}

/// What a run or a service holds for its plugin, as the threads that read
/// its lines, answer its calls and write to it count it.
#[derive(Default)]
pub(crate) struct Load {
    /// The bytes of the call lines read and not yet answered or dropped:
    /// their params, kept as the plugin wrote them, are no longer.
    calls: AtomicUsize,
    /// The lines given to the writer and not yet written to its stdin, and
    /// their bytes: replies, mostly, which it hasn't read.
    lines: AtomicUsize,
    bytes: AtomicUsize,
}

impl Load {
    /// A share for a call line of `bytes`, unless the run would hold more
    /// than [`MAX_CALL_BYTES`]. Only the stdout reader takes them.
    fn share(self: &Arc<Self>, bytes: usize) -> Option<Share> {
        let held = self.calls.load(Ordering::Relaxed) + self.bytes.load(Ordering::Relaxed);
        if held + bytes > MAX_CALL_BYTES {
            return None;
        }
        self.calls.fetch_add(bytes, Ordering::Relaxed);
        Some(Share { load: self.clone(), bytes })
    }

    fn queued(&self, bytes: usize) {
        self.lines.fetch_add(1, Ordering::Relaxed);
        self.bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    fn written(&self, bytes: usize) {
        self.lines.fetch_sub(1, Ordering::Relaxed);
        self.bytes.fetch_sub(bytes, Ordering::Relaxed);
    }
}

/// A call's part of [`Load::calls`], given back when it drops: once the call
/// was answered, refused or dropped, wherever that happened.
pub(crate) struct Share {
    load: Arc<Load>,
    bytes: usize,
}

impl Drop for Share {
    fn drop(&mut self) {
        self.load.calls.fetch_sub(self.bytes, Ordering::Relaxed);
    }
}

/// A call waiting its turn.
pub(crate) struct Waiting {
    id: CallId,
    method: HostMethod,
    params: Box<RawValue>,
    share: Share,
}

/// The calls of a run or a service to the host.
pub(crate) struct Calls {
    host: Arc<dyn HostCalls>,
    /// Its queue, which each answer comes back through.
    answers: SyncSender<Msg>,
    /// Fires once the run is cancelled (the service stopped) or its plugin
    /// ended, and once it stops for any other reason: a call being answered
    /// then changes nothing more.
    ended: CancelToken,
    /// Whether it was cancelled: no call starts any more.
    cancelled: bool,
    running: usize,
    waiting: VecDeque<Waiting>,
}

impl Calls {
    pub(crate) fn new(host: Arc<dyn HostCalls>, answers: SyncSender<Msg>) -> Self {
        Self { host, answers, ended: CancelToken::new(), cancelled: false, running: 0, waiting: VecDeque::new() }
    }

    /// Has the host answer a call on a thread of its own; gives the call's
    /// id back when no thread starts. A host that panics answers that it
    /// failed.
    fn start(&mut self, waiting: Waiting) -> Result<(), CallId> {
        let Waiting { id, method, params, share } = waiting;
        let (host, answers, ended, reply_to) =
            (self.host.clone(), self.answers.clone(), self.ended.clone(), id.clone());
        let started = thread::Builder::new().name("plugin-call".into()).spawn(move || {
            let answer = panic::catch_unwind(AssertUnwindSafe(|| host.call(method, &params, &ended)))
                .unwrap_or_else(|_| Err(CallError::new(CallCode::Failed, "AutoScene couldn't answer this call.")));
            // A run or a service that ended takes no answer.
            answers.send(Msg::Answered { id, method, answer }).ok();
            drop(share);
        });
        match started {
            Ok(_) => {
                self.running += 1;
                Ok(())
            }
            Err(e) => {
                tracing::warn!(error = %e, "a plugin's call couldn't start");
                Err(reply_to)
            }
        }
    }
}

/// However the run or the service ends (an error of its own too), the calls
/// still being answered are told.
impl Drop for Calls {
    fn drop(&mut self) {
        self.ended.cancel();
    }
}

/// The answers of a run that has none: each call is refused.
struct NoCalls;

impl HostCalls for NoCalls {
    fn call(&self, _: HostMethod, _: &RawValue, _: &CancelToken) -> Result<Value, CallError> {
        Err(CallError::new(CallCode::Invalid, "AutoScene can't answer calls here."))
    }
}

/// Writes each line it's sent to the plugin's stdin, on a thread of its own:
/// a plugin that doesn't read its stdin blocks that thread, never the run
/// or the service. `load` counts what waits. Dropping the sender closes the
/// pipe once what it holds is written; after a failed write, the rest is
/// dropped.
pub(crate) fn writer(stdin: ChildStdin, load: Arc<Load>) -> mpsc::Sender<String> {
    let (tx, rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        let mut stdin = Some(stdin);
        for mut line in rx {
            let bytes = line.len();
            line.push('\n');
            if let Some(pipe) = &mut stdin
                && pipe.write_all(line.as_bytes()).and_then(|()| pipe.flush()).is_err()
            {
                stdin = None;
            }
            load.written(bytes);
        }
    });
    tx
}

/// Reads the plugin's stdout in lines of at most `max` bytes, each of which
/// `read` says what it is, on a thread of its own: [`stdout_line`] for a
/// run.
pub(crate) fn read_stdout(
    stdout: impl Read + Send + 'static,
    tx: SyncSender<Msg>,
    max: usize,
    read: impl Fn(String, bool) -> Msg + Send + 'static,
) {
    thread::spawn(move || {
        for (line, was_cut) in capped_lines(stdout, max) {
            if tx.send(read(line, was_cut)).is_err() {
                return;
            }
        }
        tx.send(Msg::Closed).ok();
    });
}

/// What a stdout line is. With `calls` (what the run holds, for a plugin
/// that may make calls) a call line counts up to the length it was read
/// to; every other line up to [`MAX_LINE_BYTES`]. A line that starts a call
/// but is none never goes to the log as it is: its params may hold a
/// secret.
pub(crate) fn stdout_line(mut line: String, mut was_cut: bool, calls: Option<&Arc<Load>>) -> Msg {
    // A cut line is no message: its end is gone.
    if let Some(load) = calls
        && !was_cut
        && let Some(Call { id, method, params }) = Call::read(&line)
    {
        return match load.share(line.len()) {
            Some(share) => Msg::Call { id, method, params, share: Some(share) },
            None => Msg::Call { id, method, params: RawValue::NULL.to_owned(), share: None },
        };
    }
    if !was_cut
        && line.len() <= MAX_LINE_BYTES
        && let Some(m) = PluginMessage::read(&line)
    {
        return Msg::Plugin(m);
    }
    if calls.is_some()
        && let Some(head) = CallHead::read(&line)
    {
        return Msg::BadCall { head, too_long: was_cut };
    }
    if line.len() > MAX_LINE_BYTES {
        let mut end = MAX_LINE_BYTES;
        while !line.is_char_boundary(end) {
            end -= 1;
        }
        // A copy: what was read past it isn't held either.
        let mut kept = String::with_capacity(end + CUT.len());
        kept.push_str(&line[..end]);
        (line, was_cut) = (kept, true);
    }
    Msg::Stdout(marked(line, was_cut))
}

pub(crate) fn read_stderr(stderr: impl Read + Send + 'static, tx: SyncSender<Msg>) {
    thread::spawn(move || {
        for (line, was_cut) in capped_lines(stderr, MAX_LINE_BYTES) {
            if tx.send(Msg::Stderr(marked(line, was_cut))).is_err() {
                return;
            }
        }
        tx.send(Msg::Closed).ok();
    });
}

/// How a line cut at its limit ends in the log.
const CUT: &str = " [cut]";

pub(crate) fn marked(mut line: String, was_cut: bool) -> String {
    if was_cut {
        line.push_str(CUT);
    }
    line
}

/// The lines of a pipe, whatever their encoding (a stray byte must not end
/// the reader: the plugin would block on a full pipe), without their line
/// ends (`\r\n` too), and each with whether it was cut: a line keeps its
/// first `max` bytes, and the rest of it is read and dropped, so a plugin
/// that never ends a line can't fill the memory.
pub(crate) fn capped_lines(r: impl Read, max: usize) -> impl Iterator<Item = (String, bool)> {
    let mut r = BufReader::new(r);
    std::iter::from_fn(move || {
        let (mut line, mut was_cut) = (Vec::new(), false);
        loop {
            let buf = match r.fill_buf() {
                Ok(buf) => buf,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => return None,
            };
            if buf.is_empty() {
                // The end of the pipe; a last line without its newline counts.
                return (!line.is_empty() || was_cut).then(|| decoded(line, was_cut, max));
            }
            let (len, ends) = match buf.iter().position(|&b| b == b'\n') {
                Some(i) => (i, true),
                None => (buf.len(), false),
            };
            // A byte more, for the \r of a \r\n.
            let room = max + 1 - line.len();
            was_cut |= len > room;
            line.extend_from_slice(&buf[..len.min(room)]);
            r.consume(len + usize::from(ends));
            if ends {
                return Some(decoded(line, was_cut, max));
            }
        }
    })
}

pub(crate) fn decoded(mut line: Vec<u8>, mut was_cut: bool, max: usize) -> (String, bool) {
    while line.last() == Some(&b'\r') {
        line.pop();
    }
    if line.len() > max {
        line.truncate(max);
        was_cut = true;
    }
    (String::from_utf8_lossy(&line).into_owned(), was_cut)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_uv_is_a_clear_error() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("main.py"), "print()").unwrap();
        let launch = Launch::UvScript { script: "main.py".into() };
        let e = command("Hello", dir.path(), &launch, || None).unwrap_err();
        assert!(matches!(e, PluginError::UvMissing), "{e:?}");
        assert_eq!(e.to_string(), "This plugin needs uv, and AutoScene couldn't get it.");
    }

    /// AutoScene's uv in `runtimes`, as [`crate::uv`] keeps it.
    fn managed_uv(runtimes: &Path) -> PathBuf {
        crate::uv::dir(runtimes).join(if cfg!(windows) { "uv.exe" } else { "uv" })
    }

    #[test]
    fn uv_scripts_start_through_the_given_uv() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src").join("main.py"), "print()").unwrap();
        let uv = managed_uv(&dir.path().join("runtimes"));
        let launch = Launch::UvScript { script: "src/main.py".into() };
        let cmd = command("Hello", dir.path(), &launch, || Some(uv.clone())).unwrap();
        assert_eq!(cmd.get_program(), uv.as_os_str());
        let script = dir.path().join("src").join("main.py");
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(args, [OsStr::new("run"), OsStr::new("--script"), OsStr::new("--locked"), script.as_os_str()]);
    }

    #[test]
    fn uv_keeps_its_cache_and_pythons_in_autoscene() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("main.py"), "print()").unwrap();
        let runtimes = dir.path().join("runtimes");
        let launch = Launch::UvScript { script: "main.py".into() };
        let mut cmd = command("Hello", dir.path(), &launch, || Some(managed_uv(&runtimes))).unwrap();
        let id = PluginId::parse("dev.a.hello").unwrap();
        let (ffmpeg, ffprobe) = (Path::new("ffmpeg"), Path::new("ffprobe"));
        prepare(&mut cmd, (&dir.path().join("data"), None), ffmpeg, ffprobe, &id, 2).unwrap();
        let envs: std::collections::BTreeMap<_, _> =
            cmd.get_envs().filter_map(|(k, v)| Some((k.to_str()?.to_owned(), v?.to_owned()))).collect();
        // Set after every other variable was cleared: they reach uv.
        assert_eq!(envs["UV_CACHE_DIR"], runtimes.join("uv-cache").as_os_str());
        assert_eq!(envs["UV_PYTHON_INSTALL_DIR"], runtimes.join("uv-python").as_os_str());
        assert_eq!(envs["UV_NO_CONFIG"], "1");
        assert_eq!(envs["AUTOSCENE_PLUGIN_ID"], "dev.a.hello");
        // The person's own uv settings never do.
        assert!(!allowed_env(OsStr::new("UV_INDEX_URL")) && !allowed_env(OsStr::new("UV_CACHE_DIR")));
        // A program is no uv script: none of them.
        fs::create_dir(dir.path().join("bin")).unwrap();
        fs::write(dir.path().join("bin").join("tool"), "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(dir.path().join("bin").join("tool"), fs::Permissions::from_mode(0o755)).unwrap();
        }
        let launch = Launch::Exe { program: "bin/tool".into(), args: Vec::new() };
        let mut cmd = command("Hello", dir.path(), &launch, || Some(managed_uv(&runtimes))).unwrap();
        prepare(&mut cmd, (&dir.path().join("data"), None), ffmpeg, ffprobe, &id, 2).unwrap();
        assert!(!cmd.get_envs().any(|(k, _)| k.to_string_lossy().starts_with("UV_")));
    }

    #[test]
    fn programs_start_by_their_path_in_the_plugin_folder() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("bin")).unwrap();
        let program = dir.path().join("bin").join("tool.exe");
        let launch = Launch::Exe { program: "bin/tool.exe".into(), args: vec!["--quiet".into()] };
        let e = command("Hello", dir.path(), &launch, || None).unwrap_err();
        assert_eq!(e.to_string(), "Hello can't start: one of its files is missing.");
        fs::write(&program, "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let e = command("Hello", dir.path(), &launch, || None).unwrap_err();
            assert_eq!(e.to_string(), "Hello can't start: its program isn't marked as runnable.");
            fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let cmd = command("Hello", dir.path(), &launch, || None).unwrap();
        assert_eq!(cmd.get_program(), program.as_os_str());
        assert_eq!(cmd.get_args().collect::<Vec<_>>(), ["--quiet"]);
        // A script's folder doesn't count as its file.
        fs::create_dir(dir.path().join("main.py")).unwrap();
        let launch = Launch::UvScript { script: "main.py".into() };
        assert!(matches!(
            command("Hello", dir.path(), &launch, || Some(program.clone())),
            Err(PluginError::Invalid(_))
        ));
    }

    #[test]
    fn only_listed_variables_pass() {
        let allowed = |name: &str| allowed_env(OsStr::new(name));
        for name in
            ["PATH", "LANG", "TZ", "https_proxy", "HTTPS_PROXY", "CUDA_VISIBLE_DEVICES", "CUDA_PATH", "CUDA_PATH_V13_0"]
        {
            assert!(allowed(name), "{name}");
        }
        for name in [
            "AUTOSCENE_DATA_DIR",
            "AUTOSCENE_FFMPEG_DIR",
            "AUTOSCENE_TEST_SECRET",
            "RUST_LOG",
            "OPENAI_API_KEY",
            "GITHUB_TOKEN",
            "PATH2",
            "LD_PRELOAD",
            "DYLD_INSERT_LIBRARIES",
            "PYTHONPATH",
            "",
        ] {
            assert!(!allowed(name), "{name}");
        }
        #[cfg(windows)]
        {
            for name in ["Path", "SYSTEMROOT", "windir", "ProgramFiles(x86)", "PROCESSOR_ARCHITECTURE", "cuda_path_v12"]
            {
                assert!(allowed(name), "{name}");
            }
            for name in ["HOME", "LC_ALL", "XDG_RUNTIME_DIR"] {
                assert!(!allowed(name), "{name}");
            }
        }
        #[cfg(unix)]
        {
            for name in ["HOME", "LC_ALL", "XDG_RUNTIME_DIR", "DBUS_SESSION_BUS_ADDRESS", "WAYLAND_DISPLAY"] {
                assert!(allowed(name), "{name}");
            }
            for name in ["path", "SystemRoot", "Http_Proxy", "lc_all"] {
                assert!(!allowed(name), "{name}");
            }
        }
    }

    #[test]
    fn long_lines_are_cut_and_odd_bytes_kept() {
        let input = [
            b"short\r\n".as_slice(),
            &[b'x'; 40],
            b"\n",
            b"caf\xC3\xA9 \xFF\n",
            b"\n",
            // Just the limit, ended as Windows ends lines, and a byte over it.
            &[b'w'; 16],
            b"\r\n",
            &[b'v'; 17],
            b"\n",
            b"last",
        ]
        .concat();
        let lines: Vec<_> = capped_lines(input.as_slice(), 16).collect();
        assert_eq!(
            lines,
            [
                ("short".into(), false),
                ("x".repeat(16), true),
                ("café \u{FFFD}".into(), false),
                (String::new(), false),
                ("w".repeat(16), false),
                ("v".repeat(16), true),
                ("last".into(), false),
            ]
        );
        // A line that never ends is cut too, at the pipe's end.
        let endless = vec![b'y'; 100_000];
        let lines: Vec<_> = capped_lines(endless.as_slice(), 16).collect();
        assert_eq!(lines, [("y".repeat(16), true)]);
    }

    #[test]
    fn only_a_call_may_be_longer_than_a_line() {
        // Room for the params of every call a run may make, and the rest.
        for m in HostMethod::ALL.into_iter().filter(|m| m.callers().run) {
            assert!(m.max_params() + (4 << 10) <= MAX_CALL_LINE_BYTES, "{}", m.name());
        }
        let load = Arc::new(Load::default());
        let value = "v".repeat(MAX_LINE_BYTES);
        let call = serde_json::json!({"type": "call", "id": 1, "method": "state.set", "params": {"value": value}});
        let call = call.to_string();
        // A call holds its share of what the run holds while it lives, and
        // its params as they were written, no more than its line.
        let m = stdout_line(call.clone(), false, Some(&load));
        let params = serde_json::json!({"value": value}).to_string();
        assert!(
            matches!(&m, Msg::Call { share: Some(s), params: p, .. } if s.bytes == call.len() && p.get() == params)
        );
        assert_eq!(load.calls.load(Ordering::Relaxed), call.len());
        drop(m);
        assert_eq!(load.calls.load(Ordering::Relaxed), 0);
        // Past what a run holds, it comes without its params.
        load.bytes.store(MAX_CALL_BYTES - call.len() + 1, Ordering::Relaxed);
        let m = stdout_line(call.clone(), false, Some(&load));
        assert!(matches!(&m, Msg::Call { share: None, params, .. } if params.get() == "null"));
        load.bytes.store(0, Ordering::Relaxed);
        // Cut, it is a broken call, which still has its id.
        let m = stdout_line(call[..call.len() - 5].to_owned(), true, Some(&load));
        assert!(matches!(&m, Msg::BadCall { head: CallHead { id: Some(CallId::Number(1)), .. }, too_long: true }));
        // Any other line as long is cut where every line is, a message too,
        // and holds no more than it shows.
        let result = serde_json::json!({"type": "result", "message": value}).to_string();
        for line in [result, value.clone() + "w", "é".repeat(MAX_LINE_BYTES)] {
            let Msg::Stdout(shown) = stdout_line(line.clone(), false, Some(&load)) else { panic!("{}", &line[..20]) };
            assert!(shown.ends_with(CUT) && shown.capacity() <= MAX_LINE_BYTES + CUT.len(), "{}", shown.capacity());
            assert!(line.starts_with(shown.trim_end_matches(CUT)));
        }
        // A plugin of protocol 1 makes no calls: its call line is output.
        assert!(matches!(stdout_line(call, false, None), Msg::Stdout(s) if s.ends_with(CUT)));
    }

    /// A host that counts the calls it is asked.
    #[derive(Default)]
    struct Counted(AtomicUsize);

    impl HostCalls for Counted {
        fn call(&self, _: HostMethod, _: &RawValue, _: &CancelToken) -> Result<Value, CallError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(Value::Null)
        }
    }

    #[test]
    fn a_call_read_once_its_plugin_ended_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("plugin.log");
        let (tx, _rx) = mpsc::sync_channel(QUEUE);
        let (to_plugin, written) = mpsc::channel();
        let (host, load) = (Arc::new(Counted::default()), Arc::new(Load::default()));
        let report = |_: &str, _: Option<f64>| {};
        let log = PluginLog::open(&log_path, "run-1", "post").unwrap();
        let calls = Some(Calls::new(host.clone(), tx));
        let talk = Talk::new(log, to_plugin, load.clone(), calls, MAX_CALL_LINE_BYTES, "This run was cancelled.");
        let mut heard = Heard { name: "Test Plugin", report: &report, outcome: None, talk };
        let ended = heard.talk.calls.as_ref().unwrap().ended.clone();
        // Its program ended; a process it left holds its stdout and writes a call.
        heard.talk.end_calls();
        assert!(ended.is_cancelled());
        let call = r#"{"type":"call","id":1,"method":"settings.get","params":{"k":"SECRET"}}"#;
        heard.take(stdout_line(call.into(), false, Some(&load)));
        heard.talk.next_call();
        // Never asked, never answered, and nothing of it is held.
        assert_eq!(host.0.load(Ordering::SeqCst), 0);
        assert!(matches!(written.try_recv(), Err(mpsc::TryRecvError::Disconnected)));
        assert_eq!(load.calls.load(Ordering::Relaxed), 0);
        drop(heard);
        let log = fs::read_to_string(&log_path).unwrap();
        assert!(log.contains("[call] settings.get after its end: dropped\n"), "{log}");
        assert!(!log.contains("[reply]") && !log.contains("SECRET"), "{log}");
    }

    #[test]
    fn calls_are_told_their_run_ended_however_it_ends() {
        let (tx, _rx) = mpsc::sync_channel(QUEUE);
        let calls = Calls::new(Arc::new(Counted::default()), tx);
        let ended = calls.ended.clone();
        // As when the run stops on an error of its own, before its end.
        drop(calls);
        assert!(ended.is_cancelled());
    }

    #[test]
    fn shown_text_is_cleaned_and_cut() {
        assert_eq!(sanitize("  Posting\u{7}  clips\n\tto\u{202E} Discord ", 80), "Posting clips to Discord");
        assert_eq!(sanitize("abcdefghij", 5), "abcd…");
        assert_eq!(sanitize("\u{200B}\n", 5), "");
    }
}

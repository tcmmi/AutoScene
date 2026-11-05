//! WebAssembly plugins: a component (manifest runtime `wasm`) run in a
//! sandbox, on wasmtime with WASI 0.2, one store per run ([`run`]).
//!
//! A run sees what its grants give it ([`Grants`]: what the person agreed
//! to and its manifest still asks for), and nothing else:
//! - its own folder as `/data`, read and write, where it starts;
//! - the run's temp folder as `/tmp`, read and write, emptied when the run
//!   ends ([`crate::scratch`]; none when it couldn't be made, as its log
//!   says);
//! - the export's folder as `/export`: read only with `exports:read`, read
//!   and write with `exports:write`;
//! - the project's folder as `/project`, read only, with `project:read`;
//! - its secret settings, one at a time through `host.secret`;
//! - two environment variables (`AUTOSCENE_PLUGIN_ID`,
//!   `AUTOSCENE_PROTOCOL`), its id as its one argument, clocks and random.
//!
//! No other folder, no socket, no name lookup, no program and none of the
//! app's environment. A request over wasi:http goes through AutoScene's
//! guard ([`Net`]): only `https://` to the hosts its grants name, on their
//! public addresses, and none without the network permission. What it
//! writes to stdout and stderr goes to its log ([`stdio`]), and the request
//! it runs on names its folders as it sees them ([`convert`]).
//!
//! A run is held to its limits ([`Limits`]): the memory of all its linear
//! memories and tables together ([`limits`]) and its time. Stop makes
//! `host.cancelled` true; a plugin still running [`CANCEL_GRACE`] later is
//! ended, and so is one past its time limit. The engine interrupts wasm
//! code that runs ([`EPOCH_TICK`]); a plugin that waits in a call to
//! AutoScene or WASI (a sleep, a read) is dropped there: every call into a
//! component is async, so it can be left at any wait. What it hands over
//! (a log line, a stage, its result) is cut before it is cleaned
//! ([`MAX_LINE_BYTES`]), as a program's line is.
//!
//! A component is compiled once and its code kept in the plugin's code
//! folder, never one a plugin sees ([`cache`]). What it imports is checked
//! first ([`imports`]): only this world and WASI 0.2 up to the version this
//! wasmtime carries. No code of the plugin runs before its run does: its
//! review reads what it imports and compiles nothing ([`inspect`]), its
//! install compiles and links it in a process that can be ended
//! ([`Compiler`]), and neither instantiates it.

mod bindings;
mod cache;
mod convert;
mod engine;
mod imports;
mod limits;
mod net;
mod state;
mod stdio;

use std::cell::Cell;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use as_core::{CancelToken, Msg, ToMsg, msg};
use as_media::proc;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use wasmtime::component::Component;
use wasmtime::{Engine, Store, UpdateDeadline};

pub use self::cache::{keep_only, remove};
pub use self::imports::{WASI_PATCH, WORLD_MINOR, too_new};
pub use self::net::{Dial, Net, NetLimits, Resolve};
use crate::Permission;
pub use crate::calls::Grants;
use crate::error::{PluginError, PluginResult};
use crate::log::PluginLog;
use crate::manifest::{Manifest, WasmLimits, cut};
use crate::package::{MAX_COMPONENT_BYTES, is_component, sha256_hex};
use crate::protocol::RunMessage;
pub use crate::runtime::CANCEL_GRACE;
use crate::runtime::{DRAIN, MAX_LINE_BYTES, MAX_MESSAGE_CHARS, MAX_STAGE_CHARS, marked, sanitize};
use crate::scratch::Scratch;
use crate::store::changed;

use self::bindings::{Called, Pre};

/// How often the engine's epoch moves on: how soon wasm code that runs
/// notices a stop or its deadline, and hands its thread back to the run.
pub const EPOCH_TICK: Duration = Duration::from_millis(50);
/// How often a run looks for a stop and its deadline while its plugin
/// waits in a call.
const STOP_POLL: Duration = Duration::from_millis(50);
/// The least time between two progress reports of a run; its last one is
/// always reported.
pub const PROGRESS_GAP: Duration = Duration::from_millis(100);
/// The deepest stack wasm code may use (Python's components need 1 MiB).
pub const MAX_WASM_STACK: usize = 1 << 20;
/// The most resources a run may hold at once (files, streams, timers).
pub const MAX_TABLE: usize = 100_000;
/// The most random bytes a plugin may ask for at once.
pub const MAX_RANDOM: u64 = 1 << 20;
/// The longest detail of a plugin's error, or of a trap, kept for the
/// app's log (the plugin's log has all of a trap).
pub const MAX_DETAIL_CHARS: usize = 4 << 10;

/// What makes AutoScene's executable (and `asp`) a [`Compiler`]'s process.
pub const COMPILE_ARG: &str = "--compile-plugin";
/// How long a [`Compiler`] gives one component. Python's and JavaScript's
/// take about a second on 16 threads, and a 4-core laptop was guessed at 3-6
/// seconds: this leaves a slow, busy computer room to spare.
pub const COMPILE_LIMIT: Duration = Duration::from_secs(120);
/// How much memory a [`Compiler`] gives one component. A component's 64 MB
/// can ask Cranelift for 1.5 GB a second, and what runs out then is the
/// whole computer's: the app, its windows, and whatever else is open.
pub const COMPILE_MEMORY: u64 = 4 << 30;
/// How long past its [`Compiler`]'s limit its process ends by itself. The
/// limit is the [`Compiler`]'s to say: this is for when it can't any more.
const COMPILE_SLACK: Duration = Duration::from_secs(5);
/// The exit code of a compile process that ended without an answer.
const COMPILE_ENDED: i32 = 3;
const EVENTS: &str = "events";
const NO_EVENTS: &str = "no-events";
/// What Rust's standard library says on its standard error as it ends a
/// process that was refused memory.
const REFUSED_MEMORY: &str = "memory allocation of ";

pub(crate) fn compile_failed() -> Msg {
    msg!("plugin.compileFailed", "AutoScene couldn't get this plugin ready. Nothing changed.")
}

fn not_a_component() -> PluginError {
    PluginError::Invalid(msg!(
        "plugin.notAComponent",
        "The plugin's component isn't a WebAssembly component. Build it for wasm32-wasip2 or with componentize-py."
    ))
}

/// A plugin that waits for events has a component of 1.0.0, which can't be
/// told of one.
fn no_on_event() -> PluginError {
    PluginError::Invalid(msg!(
        "plugin.noOnEvent",
        "The plugin waits for events, but its component has no on-event: build it against the wit/plugin.wit that asp init writes."
    ))
}

fn too_large() -> PluginError {
    PluginError::Invalid(msg!("plugin.componentTooLarge", "The plugin's component is larger than 64 MB."))
}

/// One run of a WebAssembly plugin.
pub struct WasmSpec {
    /// The plugin's name, as messages show it.
    pub name: String,
    /// Its component in the plugin's folder.
    pub component: PathBuf,
    /// The component's SHA-256 from the installed `files.lock`; none for a
    /// loaded folder, whose component is hashed at the run.
    pub component_sha256: Option<String>,
    /// The folder its compiled code is kept in (`cache/plugin-code/<id>`).
    pub cache: PathBuf,
    /// The plugin's own folder for files, seen as `/data`.
    pub data_dir: PathBuf,
    /// The folder of the large files its `plugin.json` names
    /// ([`crate::resources`]), seen read only as `/resources`; none when it
    /// names none, and then there is no `/resources`.
    pub resources_dir: Option<PathBuf>,
    /// Where its run's scratch folder is made, seen as `/tmp`: on the disk,
    /// as nothing limits what it writes there ([`Scratch::on_disk`]).
    pub temp_root: PathBuf,
    /// The plugin's log file.
    pub log_path: PathBuf,
    pub grants: Grants,
    pub limits: Limits,
    /// The values of its secret settings, by key (never logged); only those
    /// its grants name reach it.
    pub secrets: BTreeMap<String, String>,
    /// The run, as the protocol has it; its tools are left out, and its
    /// folders are given as the plugin sees them.
    pub message: RunMessage,
}

/// What a run may use before it is stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The memory of all its linear memories and tables together, in bytes.
    pub memory: u64,
    pub time: Duration,
}

impl Limits {
    /// The limits a manifest asks for.
    pub fn of(l: WasmLimits) -> Self {
        Self { memory: u64::from(l.memory_mb) << 20, time: Duration::from_secs(u64::from(l.minutes) * 60) }
    }

    /// The memory limit in MB, as an error names it.
    fn mb(&self) -> u32 {
        u32::try_from(self.memory >> 20).unwrap_or(u32::MAX)
    }

    /// The time limit in whole minutes (at least one), as an error names it.
    fn minutes(&self) -> u32 {
        u32::try_from(self.time.as_secs().div_ceil(60).max(1)).unwrap_or(u32::MAX)
    }
}

/// What a check of a component found, for the review and for `asp`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Checked {
    /// What it imports, as `ns:pkg/interface@version`.
    pub imports: Vec<String>,
    /// What may surprise its author: a wasi:http import without the
    /// network permission, or the permission without the import.
    pub warnings: Vec<Msg>,
}

/// Runs a WebAssembly plugin's action to its end: the message it returned,
/// if any. `report` gets its progress as [`crate::runtime::run`] does, at
/// most once each [`PROGRESS_GAP`]; its first run reports
/// "Getting <name> ready" while its component is compiled.
///
/// Fails with the plugin's own error ([`PluginError::Failed`]); with
/// [`PluginError::Cancelled`] when `cancel` fires before it returned (a run
/// cancelled before it starts, or while its component is compiled, starts
/// nothing); with [`PluginError::TimedOut`] past its time limit and
/// [`PluginError::OutOfMemory`] when it needed more memory than its limit
/// and ended for that; and with [`PluginError::Crashed`] when it trapped
/// otherwise (the trap and its wasm backtrace go to its log). A run is never
/// retried.
pub fn run(spec: WasmSpec, cancel: &CancelToken, report: &dyn Fn(&str, Option<f64>)) -> PluginResult<Option<String>> {
    run_with(spec, Net::system(), cancel, report)
}

/// [`run`], with its requests sent through `net`: for tests, which answer
/// them from this computer.
#[doc(hidden)]
pub fn run_with(
    spec: WasmSpec,
    net: Net,
    cancel: &CancelToken,
    report: &dyn Fn(&str, Option<f64>),
) -> PluginResult<Option<String>> {
    if cancel.is_cancelled() {
        return Err(PluginError::Cancelled);
    }
    // Removed as it drops, once the run's store is gone.
    let scratch = Scratch::on_disk(&spec.temp_root);
    run_in_tmp(spec, net, scratch.as_ref().map(Scratch::path), cancel, report)
}

/// [`run_with`], with `scratch` as its `/tmp`, which the caller removes:
/// for tests, which look at what the plugin left there.
#[doc(hidden)]
pub fn run_in_scratch(
    spec: WasmSpec,
    net: Net,
    scratch: &Scratch,
    cancel: &CancelToken,
    report: &dyn Fn(&str, Option<f64>),
) -> PluginResult<Option<String>> {
    if cancel.is_cancelled() {
        return Err(PluginError::Cancelled);
    }
    run_in_tmp(spec, net, Ok(scratch.path()), cancel, report)
}

/// [`run_with`], with `tmp` as its `/tmp`: a run whose temp folder couldn't
/// be made goes on without one, and its log says why.
fn run_in_tmp(
    spec: WasmSpec,
    net: Net,
    tmp: Result<&Path, &std::io::Error>,
    cancel: &CancelToken,
    report: &dyn Fn(&str, Option<f64>),
) -> PluginResult<Option<String>> {
    let engine = engine::engine()?;
    fs::create_dir_all(&spec.data_dir)?;
    let log = Arc::new(Mutex::new(PluginLog::open(&spec.log_path, &spec.message.run_id, &spec.message.action.id)?));
    let tmp = tmp
        .inspect_err(|e| {
            log.lock().note("host", &format!("couldn't make its temp folder, so it has no /tmp: {e}"));
            tracing::warn!(error = %e, plugin = %spec.message.plugin.id.as_str(), "a plugin run's scratch folder couldn't be made");
        })
        .ok();
    let ran = run_in(engine, spec, net, tmp, &log, cancel, report);
    // How a run that started ended is in the log already.
    if let Err(e) = &ran
        && !matches!(
            e,
            PluginError::Cancelled
                | PluginError::Failed { .. }
                | PluginError::Crashed { .. }
                | PluginError::TimedOut { .. }
                | PluginError::OutOfMemory { .. }
        )
    {
        log.lock().note("host", &format!("couldn't start: {}", e.detail()));
    }
    ran
}

/// Whether the folders the run `spec` sees are there: its export's and its
/// project's, as its grants let it see them. [`run`] fails the same way
/// without them; a host asks first, before it gets anything for the run.
pub fn check_folders(spec: &WasmSpec) -> PluginResult<()> {
    convert::mounts(&spec.message, &spec.grants).map(drop)
}

/// What the component at `path` (its SHA-256 checked against `sha256` when
/// given) says it imports, checked against `m`: all a review shows of it.
/// This reads its bytes and compiles nothing, so it is safe on a package
/// nobody agreed to yet. Whether it compiles, links and exports `run`,
/// [`prepare`] finds out.
pub fn inspect(path: &Path, sha256: Option<&str>, m: &Manifest) -> PluginResult<Checked> {
    let bytes = read_component(path)?;
    if sha256.is_some_and(|s| s != sha256_hex(&bytes)) {
        return Err(PluginError::Invalid(changed()));
    }
    if !is_component(&bytes) {
        return Err(not_a_component());
    }
    let imports = imports::declared(&bytes).map_err(|why| {
        tracing::debug!(error = %why, "a plugin's component can't be read");
        doesnt_load(&why)
    })?;
    let warnings = imports::check(imports.iter().map(String::as_str), Some(m))?;
    Ok(Checked { imports, warnings })
}

/// Compiles the component at `path` (its SHA-256 checked against `sha256`
/// when given), checks what it imports and exports against `m`, and keeps
/// its compiled code in the folder `cache`, beside the code there: the
/// installed version's stays until the person agrees to this one, and the
/// store then keeps only this ([`keep_only`]). Gives what the check found
/// and the compiled file. A component that fails leaves no file.
///
/// This compiles here, where nothing can stop it: the app compiles with a
/// [`Compiler`] instead.
pub fn prepare(path: &Path, sha256: Option<&str>, cache: &Path, m: &Manifest) -> PluginResult<(Checked, PathBuf)> {
    compile_into(path, sha256, cache, Some(m), !m.events.is_empty())
}

/// [`prepare`], for `m` or for none, with `events` saying whether its
/// plugin waits for events.
fn compile_into(
    path: &Path,
    sha256: Option<&str>,
    cache: &Path,
    m: Option<&Manifest>,
    events: bool,
) -> PluginResult<(Checked, PathBuf)> {
    let engine = engine::engine()?;
    let bytes = read_component(path)?;
    let hash = sha256_hex(&bytes);
    if sha256.is_some_and(|s| s != hash) {
        return Err(PluginError::Invalid(changed()));
    }
    let component = loaded(engine, &bytes)?;
    let checked = checked_for(engine, &component, m, events)?;
    let file = cache::save(cache, &hash, &component)?;
    Ok((checked, file))
}

/// Compiles components in a process of its own, which the app can end:
/// Cranelift can't be stopped from outside, and a package's bytes are its
/// author's to choose. The process takes every core while it runs. It ends
/// at `limit`, at `memory`, at a cancel or with the app, whichever comes
/// first. It watches its time and the app itself ([`compile_main`]), so a
/// crashed app leaves nothing running on any system.
pub struct Compiler {
    /// The executable that takes [`COMPILE_ARG`].
    pub program: PathBuf,
    /// How long one component may take.
    pub limit: Duration,
    /// How many bytes one component may take to compile.
    pub memory: u64,
}

/// What [`compile_main`] answers on its standard output, as one line of JSON.
#[derive(Debug, Deserialize, Serialize)]
enum Compiled {
    /// The compiled file.
    Kept(PathBuf),
    /// Refused, and why ([`PluginError::Invalid`]).
    Refused(Msg),
    /// [`PluginError::TooNew`], and what needs it.
    TooNew(String),
}

impl Compiler {
    /// `program` as the compiler, with [`COMPILE_LIMIT`] and
    /// [`COMPILE_MEMORY`].
    pub fn new(program: PathBuf) -> Self {
        Self { program, limit: COMPILE_LIMIT, memory: COMPILE_MEMORY }
    }

    /// [`prepare`] in a process of its own: the file it kept. What the
    /// component imports was read for its review ([`inspect`]), so nothing
    /// comes back of that. Refused as [`prepare`] refuses, and past `limit`
    /// or `memory` the process is ended, as it is on `cancel`
    /// ([`PluginError::Cancelled`]), which leaves no compiled file.
    pub fn prepare(
        &self,
        path: &Path,
        sha256: Option<&str>,
        cache: &Path,
        m: &Manifest,
        cancel: &CancelToken,
    ) -> PluginResult<PathBuf> {
        let bytes = read_component(path)?;
        let hash = sha256_hex(&bytes);
        if sha256.is_some_and(|s| s != hash) {
            return Err(PluginError::Invalid(changed()));
        }
        drop(bytes);
        let mut cmd = Command::new(&self.program);
        cmd.arg(COMPILE_ARG).arg(path).arg(&hash).arg(cache);
        cmd.arg(if m.events.is_empty() { NO_EVENTS } else { EVENTS });
        cmd.arg((self.limit + COMPILE_SLACK).as_millis().to_string());
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut tree = proc::spawn_tree_limited(&mut cmd, self.memory)?;
        // Held until this ends: the process ends when it closes, which is
        // how it hears that the app did.
        let _alive = tree.take_stdin();
        // Read as it comes: a pipe holds so much, and a process that fills
        // it waits for it to be read.
        let answer = tree.take_stdout().map(read_all);
        let errors = tree.take_stderr().map(read_all);
        let deadline = Instant::now() + self.limit;
        while !tree.has_exited()? {
            if cancel.is_cancelled() {
                let _ = tree.kill_tree(Duration::ZERO);
                return Err(PluginError::Cancelled);
            }
            if Instant::now() >= deadline {
                let _ = tree.kill_tree(Duration::ZERO);
                tracing::warn!(plugin = %m.id, limit = ?self.limit, "a plugin's compile was stopped at its limit");
                return Err(PluginError::Invalid(msg!(
                    "plugin.compileStopped",
                    "This plugin took too long to get ready, so AutoScene stopped. Nothing changed."
                )));
            }
            thread::sleep(STOP_POLL);
        }
        let status = tree.finish()?;
        let said = |reader: Option<thread::JoinHandle<String>>| reader.and_then(|r| r.join().ok()).unwrap_or_default();
        match serde_json::from_str(&said(answer)) {
            Ok(Compiled::Kept(file)) => Ok(file),
            Ok(Compiled::Refused(why)) => Err(PluginError::Invalid(why)),
            Ok(Compiled::TooNew(what)) => Err(PluginError::TooNew(what)),
            Err(e) => {
                let errors = said(errors);
                let stderr = cut(&errors, MAX_DETAIL_CHARS);
                tracing::warn!(plugin = %m.id, %status, error = %e, %stderr, "a plugin's compile ended without an answer");
                Err(PluginError::Invalid(match errors.contains(REFUSED_MEMORY) {
                    true => msg!(
                        "plugin.compileOutOfMemory",
                        "This plugin took too much memory to get ready, so AutoScene stopped. Nothing changed."
                    ),
                    false => compile_failed(),
                }))
            }
        }
    }
}

/// All that `pipe` holds until it closes, read on a thread of its own.
fn read_all(mut pipe: impl Read + Send + 'static) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = pipe.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

/// The process of a [`Compiler`], with the arguments that follow
/// [`COMPILE_ARG`]: the component, its SHA-256, the code folder, whether it
/// waits for events, and how many milliseconds it may take. It answers on
/// its standard output, and gives the process's exit code. It ends by itself
/// past that time, and when its standard input closes, which is the app
/// ending, in whatever way: on macOS and Linux nothing else would end it.
pub fn compile_main(args: impl Iterator<Item = OsString>) -> i32 {
    let args: Vec<OsString> = args.collect();
    let limit = args.get(4).and_then(|ms| ms.to_str()?.parse().ok()).map(Duration::from_millis);
    let ([path, sha256, cache, events, _], Some(limit)) = (args.as_slice(), limit) else {
        eprintln!(
            "{COMPILE_ARG} takes a component, its SHA-256, a folder, {EVENTS} or {NO_EVENTS}, and its time limit in ms"
        );
        return 2;
    };
    watch_compile(limit);
    let (path, cache) = (Path::new(path), Path::new(cache));
    let compiled = compile_into(path, sha256.to_str(), cache, None, events == EVENTS);
    let answer = match compiled {
        Ok((_, file)) => Compiled::Kept(file),
        Err(PluginError::TooNew(what)) => Compiled::TooNew(what),
        Err(e) => Compiled::Refused(e.msg()),
    };
    match serde_json::to_string(&answer) {
        Ok(line) => {
            println!("{line}");
            0
        }
        Err(_) => 1,
    }
}

/// Ends this process at `limit`, or when its standard input closes,
/// whatever it is doing.
fn watch_compile(limit: Duration) {
    thread::spawn(move || {
        thread::sleep(limit);
        std::process::exit(COMPILE_ENDED);
    });
    thread::spawn(|| {
        // Nobody writes to it: the read ends when the other end closes.
        let _ = std::io::stdin().read(&mut [0]);
        std::process::exit(COMPILE_ENDED);
    });
}

/// The checks of [`prepare`] on `bytes`, without keeping the code: for
/// `asp`. `m` is none for a component checked on its own.
pub fn check(bytes: &[u8], m: Option<&Manifest>) -> PluginResult<Checked> {
    check_listed(bytes, m).1
}

/// [`check`], with what the component imports beside its result, also when
/// the check refused it (none when it didn't compile): for `asp`, which
/// names an import that needs a newer AutoScene ([`too_new`]).
pub fn check_listed(bytes: &[u8], m: Option<&Manifest>) -> (Vec<String>, PluginResult<Checked>) {
    let loaded = engine::engine().and_then(|engine| Ok((engine, loaded(engine, bytes)?)));
    match loaded {
        Ok((engine, component)) => (import_names(engine, &component), checked(engine, &component, m)),
        Err(e) => (Vec::new(), Err(e)),
    }
}

/// Compiles `bytes` and checks them as a plugin's component: one this
/// wasmtime can compile, that imports only what AutoScene offers (with the
/// warnings for `m`), links against it, and exports `run`. Nothing of it
/// runs.
fn compiled(engine: &Engine, bytes: &[u8], m: Option<&Manifest>) -> PluginResult<(Component, Checked)> {
    let component = loaded(engine, bytes)?;
    let checked = checked(engine, &component, m)?;
    Ok((component, checked))
}

/// `bytes` compiled, when they are a component this wasmtime can compile.
fn loaded(engine: &Engine, bytes: &[u8]) -> PluginResult<Component> {
    if bytes.len() as u64 > MAX_COMPONENT_BYTES {
        return Err(too_large());
    }
    if !is_component(bytes) {
        return Err(not_a_component());
    }
    Component::new(engine, bytes).map_err(|e| {
        let text = format!("{e:#}");
        tracing::debug!(error = %text, "a plugin's component doesn't compile");
        // wasmtime is built without the async component model: a component
        // that uses it needs an AutoScene that has it.
        if text.contains("component model async") || text.contains("requires the component model") {
            PluginError::TooNew("component".into())
        } else {
            doesnt_load(&text)
        }
    })
}

/// What `component` imports, as `ns:pkg/interface@version`.
fn import_names(engine: &Engine, component: &Component) -> Vec<String> {
    component.component_type().imports(engine).map(|(name, _)| name.to_owned()).collect()
}

/// The checks of [`compiled`] past compiling: imports, link, `run`, and
/// `on-event` when `m` waits for events.
fn checked(engine: &Engine, component: &Component, m: Option<&Manifest>) -> PluginResult<Checked> {
    checked_for(engine, component, m, m.is_some_and(|m| !m.events.is_empty()))
}

/// [`checked`], with `events` saying whether its plugin waits for events.
fn checked_for(engine: &Engine, component: &Component, m: Option<&Manifest>, events: bool) -> PluginResult<Checked> {
    let names = import_names(engine, component);
    let warnings = imports::check(names.iter().map(String::as_str), m)?;
    let pre = pre(engine, component)?;
    if events && !pre.takes_events() {
        return Err(no_on_event());
    }
    Ok(Checked { imports: names, warnings })
}

/// `component` linked against what AutoScene offers, with the exports of
/// the world it was built for found ([`Pre`]): ready to instantiate, which
/// runs nothing yet.
fn pre(engine: &Engine, component: &Component) -> PluginResult<Pre> {
    let doesnt_fit = |e: wasmtime::Error| {
        let text = format!("{e:#}");
        tracing::debug!(error = %text, "a plugin's component doesn't link");
        PluginError::Invalid(msg!(
            "plugin.componentDoesntFit",
            "The plugin's component doesn't fit this AutoScene: {{why}}",
            why = first_line(&text)
        ))
    };
    let linker = engine::linker(engine)?;
    Pre::new(engine, component, linker.instantiate_pre(component).map_err(doesnt_fit)?).map_err(doesnt_fit)
}

fn doesnt_load(why: &str) -> PluginError {
    let why = first_line(why);
    PluginError::Invalid(msg!("plugin.componentDoesntLoad", "The plugin's component doesn't load: {{why}}", why = why))
}

/// The first line of `text`, as much of it as a person can read: an
/// error can repeat a name of any length a component gives.
fn first_line(text: &str) -> String {
    cut(text.lines().next().unwrap_or_default(), MAX_DETAIL_CHARS)
}

/// Reads the component at `path`, at most [`MAX_COMPONENT_BYTES`].
fn read_component(path: &Path) -> PluginResult<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?.take(MAX_COMPONENT_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_COMPONENT_BYTES {
        return Err(too_large());
    }
    Ok(bytes)
}

/// The component of `spec`, compiled: from its code folder when it was
/// compiled before, else compiled now (reporting so) and kept there.
fn component(
    engine: &Engine,
    spec: &WasmSpec,
    log: &Mutex<PluginLog>,
    report: &dyn Fn(&str, Option<f64>),
) -> PluginResult<Component> {
    // A package's component is known by the hash its lock gives it; a
    // loaded folder's by its bytes now.
    let (bytes, sha256) = match &spec.component_sha256 {
        Some(sha256) => (None, sha256.clone()),
        None => {
            let bytes = read_component(&spec.component)?;
            let sha256 = sha256_hex(&bytes);
            (Some(bytes), sha256)
        }
    };
    // Only a hash names a file of compiled code.
    cache::file_name(&sha256)?;
    match cache::load(engine, &spec.cache, &sha256) {
        Ok(Some(component)) => return Ok(component),
        Ok(None) => {}
        Err(why) => log.lock().note("host", &format!("cache: compiled plugin rebuilt ({why})")),
    }
    let bytes = match bytes {
        Some(bytes) => bytes,
        None => read_component(&spec.component)?,
    };
    if sha256_hex(&bytes) != sha256 {
        return Err(PluginError::Invalid(changed()));
    }
    report(&format!("Getting {} ready", sanitize(&spec.name, MAX_STAGE_CHARS)), None);
    let (component, _) = compiled(engine, &bytes, None)?;
    // A run can do without its code kept: the next one compiles again.
    if let Err(e) = cache::save(&spec.cache, &sha256, &component) {
        log.lock().note("host", &format!("cache: compiled plugin not kept ({})", e.detail()));
    }
    Ok(component)
}

/// Why a run was ended before its plugin returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stop {
    /// Stop, and its grace is over.
    Cancel,
    /// Its time limit.
    Deadline,
}

/// How the call of a plugin's `run` or `on-event` ended.
enum Ended {
    Returned(Called),
    Stopped(Stop),
}

fn run_in(
    engine: &Engine,
    spec: WasmSpec,
    net: Net,
    tmp: Option<&Path>,
    log: &Arc<Mutex<PluginLog>>,
    cancel: &CancelToken,
    report: &dyn Fn(&str, Option<f64>),
) -> PluginResult<Option<String>> {
    let component = component(engine, &spec, log, report)?;
    let pre = pre(engine, &component)?;
    // Stop while it was compiled: no code of it has run yet, and none does.
    if cancel.is_cancelled() {
        log.lock().note("host", "cancelled before it started");
        return Err(PluginError::Cancelled);
    }
    let WasmSpec { name, data_dir, resources_dir, grants, limits, mut secrets, mut message, .. } = spec;
    let shown = sanitize(&name, MAX_STAGE_CHARS);
    // Its protocol is the one it declared, as for a program.
    message.protocol = grants.protocol;
    secrets.retain(|key, _| grants.secrets.contains(key));
    let mounts = convert::mounts(&message, &grants)?;
    let request = convert::request(&message, &mounts, &grants)?;
    // Never sent: the manifest's events were refused at install without it.
    if !pre.takes(&request) {
        return Err(no_on_event());
    }
    let folders = (data_dir.as_path(), resources_dir.as_deref(), tmp);
    let wasi = state::wasi(message.plugin.id.as_str(), grants.protocol, folders, &mounts, log)?;

    let deadline = Instant::now() + limits.time;
    let (progress, mut progressed) = tokio::sync::watch::channel(None);
    // Its requests go only where its grants say: nowhere without the
    // network permission, whatever hosts they name.
    let hosts: &[String] = if grants.has(Permission::Network) { &grants.hosts } else { &[] };
    let guard = net::Guard::new(Arc::new(net), hosts, cancel.clone());
    let told = guard.told();
    // How its export went, as the run line of protocol 2 has it.
    let export_status = message.export.as_ref().filter(|_| grants.protocol >= 2).and_then(|e| e.status.clone());
    let state = state::State::new(
        wasi,
        limits.memory,
        cancel.clone(),
        deadline,
        progress,
        log.clone(),
        secrets,
        export_status,
        guard,
    );
    let mut store = Store::new(engine, state);
    store.limiter(|s| &mut s.limiter);
    // Each tick of the epoch: end the run past its deadline or its grace
    // after Stop, else let the run look around (a yield) and go on.
    store.set_epoch_deadline(1);
    store.epoch_deadline_callback(|mut hook| {
        let s = hook.data_mut();
        let now = Instant::now();
        if now >= s.deadline {
            return Ok(UpdateDeadline::Interrupt);
        }
        if s.cancel.is_cancelled() {
            let seen = *s.cancel_seen.get_or_insert(now);
            if now - seen >= CANCEL_GRACE {
                return Ok(UpdateDeadline::Interrupt);
            }
        }
        Ok(UpdateDeadline::Yield(1))
    });

    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    // When the run first saw Stop while its plugin waited in a call.
    let stop_seen = Cell::new(None);
    let show = |latest: &Option<(Option<f64>, Option<String>)>| {
        if let Some((fraction, stage)) = latest {
            let stage = stage.as_deref().map(|s| sanitize(s, MAX_STAGE_CHARS)).filter(|s| !s.is_empty());
            let fraction = fraction.filter(|f| f.is_finite()).map(|f| f.clamp(0.0, 1.0));
            report(stage.as_deref().unwrap_or(&shown), fraction);
        }
    };
    let ended = rt.block_on(async {
        // Start functions run in it, within the limits.
        let call = pre.call(&mut store, &request);
        tokio::pin!(call);
        let stop = stopped(cancel, deadline, &stop_seen);
        tokio::pin!(stop);
        let (mut shown_at, mut pending): (Option<Instant>, bool) = (None, false);
        let ended = loop {
            let next = shown_at.map_or_else(Instant::now, |at| at + PROGRESS_GAP);
            tokio::select! {
                // A result that is in counts, whatever came beside it.
                biased;
                r = &mut call => break Ended::Returned(r),
                why = &mut stop => break Ended::Stopped(why),
                Ok(()) = progressed.changed() => pending = true,
                () = tokio::time::sleep_until(next.into()), if pending => {}
            }
            if pending && shown_at.is_none_or(|at| at.elapsed() >= PROGRESS_GAP) {
                show(&progressed.borrow_and_update());
                (shown_at, pending) = (Some(Instant::now()), false);
            }
        };
        // Its last report, also one it made just before it returned.
        if pending || progressed.has_changed().unwrap_or(false) {
            show(&progressed.borrow_and_update());
        }
        ended
    });
    // Stop counts as seen also when a request of the plugin told it so.
    let cancel_seen = store.data().cancel_seen.or(stop_seen.get()).or(told.get().copied());
    let hit = store.data().limiter.hit;
    drop(store);
    // A plain drop would wait for a file call that never ends.
    rt.shutdown_timeout(DRAIN);

    let cancelled = cancel_seen.is_some();
    let mut log = log.lock();
    match ended {
        Ended::Returned(Ok(Ok(said))) => {
            log.note("result", &said.as_deref().map_or_else(|| "(no message)".into(), for_log));
            if cancelled {
                return Err(PluginError::Cancelled);
            }
            Ok(said.map(|m| sanitize(head(&m).0, MAX_MESSAGE_CHARS)).filter(|m| !m.is_empty()))
        }
        Ended::Returned(Ok(Err(e))) => {
            let logged = match &e.detail {
                Some(detail) => format!("{} ({})", for_log(&e.message), for_log(detail)),
                None => for_log(&e.message),
            };
            log.note("failed", &logged);
            if cancelled {
                return Err(PluginError::Cancelled);
            }
            let mut message = sanitize(head(&e.message).0, MAX_MESSAGE_CHARS);
            if message.is_empty() {
                message = format!("{shown} failed.");
            }
            let detail = e.detail.map(|d| sanitize(head(&d).0, MAX_DETAIL_CHARS)).filter(|d| !d.is_empty());
            Err(PluginError::Failed { message, detail })
        }
        Ended::Returned(Err(trap)) => {
            let trap = format!("{trap:?}");
            log.note("host", &format!("trapped: {trap}"));
            if cancelled {
                log.note("host", "stopped after cancel");
                Err(PluginError::Cancelled)
            } else if Instant::now() >= deadline {
                log.note("host", "ran past its time limit");
                Err(PluginError::TimedOut { name, minutes: limits.minutes() })
            } else if hit {
                log.note("host", "needed more memory than its limit");
                Err(PluginError::OutOfMemory { name, mb: limits.mb() })
            } else {
                Err(PluginError::Crashed { name, detail: cut(&trap, MAX_DETAIL_CHARS) })
            }
        }
        // Its deadline within the grace after Stop: Stop came first.
        Ended::Stopped(why) if why == Stop::Cancel || cancelled => {
            log.note("host", "still running after cancel; ended");
            Err(PluginError::Cancelled)
        }
        Ended::Stopped(_) => {
            log.note("host", "ran past its time limit; ended");
            Err(PluginError::TimedOut { name, minutes: limits.minutes() })
        }
    }
}

/// How the name of code this AutoScene's engine compiled ends
/// (`-<engine>.cwasm`); none when its engine can't be made.
pub(crate) fn own_code_end() -> Option<String> {
    engine::engine_key().ok().map(|key| format!("-{key}.cwasm"))
}

/// At most [`MAX_LINE_BYTES`] of a string the plugin handed over, cut at a
/// character, and whether it was cut: as a program's line is kept before
/// it is cleaned. One may be up to wasmtime's 128 MiB, and cleaning or
/// logging that whole would take seconds in a call nothing can stop, and
/// gigabytes of the app's memory that the plugin's limit doesn't count.
pub(crate) fn head(text: &str) -> (&str, bool) {
    match text.len() > MAX_LINE_BYTES {
        true => (&text[..text.floor_char_boundary(MAX_LINE_BYTES)], true),
        false => (text, false),
    }
}

/// [`head`] of `text` for the plugin's log, marked when it was cut.
fn for_log(text: &str) -> String {
    let (head, was_cut) = head(text);
    marked(head.to_owned(), was_cut)
}

/// Ends once the run must stop while its plugin waits in a call: at its
/// deadline, or [`CANCEL_GRACE`] after `cancel` fired (noted in `seen`).
async fn stopped(cancel: &CancelToken, deadline: Instant, seen: &Cell<Option<Instant>>) -> Stop {
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Stop::Deadline;
        }
        if cancel.is_cancelled() {
            let at = seen.get().unwrap_or(now);
            seen.set(Some(at));
            if now - at >= CANCEL_GRACE {
                return Stop::Cancel;
            }
        }
        tokio::time::sleep(next_look(now, deadline, seen.get())).await;
    }
}

/// How long [`stopped`] waits from `now` before it looks again: a
/// [`STOP_POLL`], but never past the run's `deadline` nor past the end of
/// the grace after Stop was `seen`. So a run ends at most one poll after
/// Stop plus the grace, not two.
fn next_look(now: Instant, deadline: Instant, seen: Option<Instant>) -> Duration {
    let grace_ends = seen.map_or(deadline, |at| (at + CANCEL_GRACE).min(deadline));
    STOP_POLL.min(grace_ends.saturating_duration_since(now))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_looks_again_when_the_grace_ends() {
        let now = Instant::now();
        let deadline = now + Duration::from_secs(60);
        // Before Stop, and early in the grace: a poll.
        assert_eq!(next_look(now, deadline, None), STOP_POLL);
        assert_eq!(next_look(now, deadline, Some(now)), STOP_POLL);
        // Near its end: only until it ends, not a whole poll past it.
        let seen = now - CANCEL_GRACE + Duration::from_millis(20);
        assert_eq!(next_look(now, deadline, Some(seen)), Duration::from_millis(20));
        assert_eq!(next_look(now, deadline, Some(now - CANCEL_GRACE)), Duration::ZERO);
        // Nor past the deadline.
        assert_eq!(next_look(now, now + Duration::from_millis(5), None), Duration::from_millis(5));
    }

    #[test]
    fn limits_come_from_the_manifest() {
        let l = Limits::of(WasmLimits { memory_mb: 64, minutes: 2 });
        assert_eq!(l, Limits { memory: 64 << 20, time: Duration::from_secs(120) });
        assert_eq!((l.mb(), l.minutes()), (64, 2));
        // A test's limit of seconds still names a whole minute.
        let short = Limits { memory: 1 << 20, time: Duration::from_secs(2) };
        assert_eq!((short.mb(), short.minutes()), (1, 1));
    }
}

#[cfg(test)]
mod said {
    use super::*;

    #[test]
    fn a_compile_refused_in_its_process_keeps_its_key() {
        let line = serde_json::to_string(&Compiled::Refused(compile_failed())).unwrap();
        let Compiled::Refused(said) = serde_json::from_str(&line).unwrap() else { panic!("{line}") };
        assert_eq!(said, compile_failed());
    }

    #[test]
    fn a_component_that_is_refused_has_its_key() {
        let key = |e: PluginError| e.msg().key;
        assert_eq!(key(not_a_component()), "plugin.notAComponent");
        assert_eq!(key(no_on_event()), "plugin.noOnEvent");
        assert_eq!(key(too_large()), "plugin.componentTooLarge");
        let said = doesnt_load("first\nsecond");
        assert_eq!(said.msg().key, "plugin.componentDoesntLoad");
        assert_eq!(said.to_string(), "The plugin's component doesn't load: first");
    }
}

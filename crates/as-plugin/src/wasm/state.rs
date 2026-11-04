//! What a run's store holds: the plugin's WASI context, its resources, its
//! memory limiter, and AutoScene's side of the `host` interface.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use as_core::CancelToken;
use parking_lot::Mutex;
use tokio::sync::watch;
use wasmtime::component::ResourceTable;
use wasmtime_wasi::{FsPerms, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};
use wasmtime_wasi_http::{WasiHttpCtx, WasiHttpCtxView, WasiHttpView};

use super::bindings::autoscene::plugin::{host, types};
use super::convert::Mounts;
use super::limits::Limiter;
use super::net::Guard;
use super::{MAX_RANDOM, MAX_TABLE, head};
use crate::error::PluginResult;
use crate::log::PluginLog;
use crate::runtime::sanitize;

/// The longest line of a plugin's `host.log` kept.
const MAX_LOG_CHARS: usize = 2000;

/// The latest progress a plugin reported: how far, and its stage.
pub(crate) type Progress = Option<(Option<f64>, Option<String>)>;

pub(crate) struct State {
    wasi: WasiCtx,
    table: ResourceTable,
    http: WasiHttpCtx,
    guard: Guard,
    pub limiter: Limiter,
    pub cancel: CancelToken,
    /// When the run first saw Stop while the plugin ran or asked for it.
    pub cancel_seen: Option<Instant>,
    pub deadline: Instant,
    progress: watch::Sender<Progress>,
    log: Arc<Mutex<PluginLog>>,
    secrets: BTreeMap<String, String>,
}

impl State {
    pub(crate) fn new(
        wasi: WasiCtx,
        memory: u64,
        cancel: CancelToken,
        deadline: Instant,
        progress: watch::Sender<Progress>,
        log: Arc<Mutex<PluginLog>>,
        secrets: BTreeMap<String, String>,
    ) -> Self {
        let mut table = ResourceTable::new();
        table.set_max_capacity(MAX_TABLE);
        Self {
            wasi,
            table,
            http: WasiHttpCtx::new(),
            guard: Guard,
            limiter: Limiter::new(memory),
            cancel,
            cancel_seen: None,
            deadline,
            progress,
            log,
            secrets,
        }
    }
}

/// The WASI context of a run of the plugin `id`: its folders (`/data`, and
/// what `mounts` gives), its two variables and its id as its argument,
/// starting in `/data`, its output to `log`; no stdin, no socket, no name
/// lookup, and none of the app's environment or arguments.
pub(crate) fn wasi(
    id: &str,
    protocol: u32,
    data_dir: &Path,
    mounts: &Mounts,
    log: &Arc<Mutex<PluginLog>>,
) -> PluginResult<WasiCtx> {
    let mut b = WasiCtxBuilder::new();
    b.env("AUTOSCENE_PLUGIN_ID", id)
        .env("AUTOSCENE_PROTOCOL", protocol.to_string())
        .args(&[id])
        .initial_cwd("/data")
        .stdout(super::stdio::LogStream::stdout(log))
        .stderr(super::stdio::LogStream::stderr(log))
        // Off already; said here, since the builder's docs say otherwise.
        .allow_tcp(false)
        .allow_udp(false)
        .allow_ip_name_lookup(false)
        .socket_addr_check(|_, _| Box::pin(async { false }))
        .max_random_size(MAX_RANDOM);
    let open = |e: wasmtime::Error| std::io::Error::other(format!("{e:#}"));
    b.preopened_dir(data_dir, "/data", FsPerms::ReadWrite).map_err(open)?;
    if let Some(export) = &mounts.export {
        let perms = if mounts.export_write { FsPerms::ReadWrite } else { FsPerms::ReadOnly };
        b.preopened_dir(export, "/export", perms).map_err(open)?;
    }
    if let Some(project) = &mounts.project {
        b.preopened_dir(project, "/project", FsPerms::ReadOnly).map_err(open)?;
    }
    Ok(b.build())
}

impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView { ctx: &mut self.wasi, table: &mut self.table }
    }
}

impl WasiHttpView for State {
    fn http(&mut self) -> WasiHttpCtxView<'_> {
        WasiHttpCtxView { ctx: &mut self.http, table: &mut self.table, hooks: &mut self.guard }
    }
}

impl types::Host for State {}

impl host::Host for State {
    fn progress(&mut self, fraction: Option<f64>, stage: Option<String>) {
        // The run reports the latest, at most once each PROGRESS_GAP; a
        // stage is kept cut, as it waits for that.
        let stage = stage.map(|s| match head(&s) {
            (head, true) => head.to_owned(),
            (_, false) => s,
        });
        self.progress.send_replace(Some((fraction, stage)));
    }

    fn log(&mut self, level: types::LogLevel, message: String) {
        let tag = match level {
            types::LogLevel::Debug => "debug",
            types::LogLevel::Info => "info",
            types::LogLevel::Warn => "warn",
            types::LogLevel::Error => "error",
        };
        self.log.lock().line(tag, &sanitize(head(&message).0, MAX_LOG_CHARS));
    }

    fn cancelled(&mut self) -> bool {
        let cancelled = self.cancel.is_cancelled();
        if cancelled {
            // It saw Stop: whatever it returns now came after.
            self.cancel_seen.get_or_insert_with(Instant::now);
        }
        cancelled
    }

    fn secret(&mut self, key: String) -> Option<String> {
        self.secrets.get(&key).cloned()
    }
}

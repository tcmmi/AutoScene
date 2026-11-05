//! The one wasmtime engine, and what a plugin's component is linked
//! against.
//!
//! The engine is made on the first WebAssembly use, never at start: making
//! it installs wasmtime's trap handlers for the whole process (signals on
//! Unix, Mach ports on macOS, a vectored handler on Windows). An engine
//! that can't be made is kept as why, and every WebAssembly plugin then
//! says so while the app runs on.

use std::hash::{Hash, Hasher};
use std::sync::OnceLock;
use std::thread;

use wasmtime::component::{HasSelf, Linker};
use wasmtime::{Config, Engine};

use super::state::State;
use super::{EPOCH_TICK, MAX_WASM_STACK, bindings};
use as_core::msg;

use crate::error::{PluginError, PluginResult};
use crate::package::sha256_hex;

/// The engine every run shares. Its epoch moves on each [`EPOCH_TICK`],
/// from a thread of its own, for as long as the app runs.
pub(crate) fn engine() -> PluginResult<&'static Engine> {
    static ENGINE: OnceLock<Result<Engine, String>> = OnceLock::new();
    ENGINE
        .get_or_init(|| {
            let mut config = Config::new();
            // Epochs bound wall time; fuel would slow every run. A trap
            // keeps its wasm backtrace (20 frames by default) for the log.
            config.epoch_interruption(true).max_wasm_stack(MAX_WASM_STACK);
            let engine = Engine::new(&config).map_err(|e| format!("{e:#}"))?;
            let ticker = engine.clone();
            thread::Builder::new()
                .name("plugin-epoch".into())
                .spawn(move || {
                    loop {
                        thread::sleep(EPOCH_TICK);
                        ticker.increment_epoch();
                    }
                })
                .map_err(|e| e.to_string())?;
            Ok(engine)
        })
        .as_ref()
        .map_err(|why| cant_run(why.clone()))
}

fn cant_run(why: String) -> PluginError {
    PluginError::Invalid(msg!(
        "plugin.wasmCantRun",
        "WebAssembly plugins can't run on this computer: {{why}}",
        why = why
    ))
}

/// What a compiled component was compiled for: this AutoScene's version and
/// the engine's settings, as 16 hex digits. Code compiled by another
/// AutoScene is compiled again (wasmtime refuses another wasmtime's anyway).
pub(crate) fn engine_key() -> PluginResult<&'static str> {
    static KEY: OnceLock<String> = OnceLock::new();
    let engine = engine()?;
    Ok(KEY.get_or_init(|| {
        let mut bytes = Collect(env!("CARGO_PKG_VERSION").as_bytes().to_vec());
        engine.precompile_compatibility_hash().hash(&mut bytes);
        sha256_hex(&bytes.0)[..16].to_owned()
    }))
}

/// A [`Hasher`] that keeps what it is fed, for a hash of our own.
struct Collect(Vec<u8>);

impl Hasher for Collect {
    fn write(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }

    fn finish(&self) -> u64 {
        0
    }
}

/// What a plugin is linked against: WASI 0.2 (its sockets turned off by the
/// run), wasi:http (which every JavaScript component imports) and
/// AutoScene's `host`. Every call is async, so a run can drop a plugin that
/// waits in one.
pub(crate) fn linker(engine: &Engine) -> PluginResult<Linker<State>> {
    let failed = |e: wasmtime::Error| cant_run(format!("{e:#}"));
    let mut linker = Linker::new(engine);
    wasmtime_wasi::p2::add_to_linker_async(&mut linker).map_err(failed)?;
    wasmtime_wasi_http::p2::add_only_http_to_linker_async(&mut linker).map_err(failed)?;
    bindings::v1_1::Plugin::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s).map_err(failed)?;
    Ok(linker)
}

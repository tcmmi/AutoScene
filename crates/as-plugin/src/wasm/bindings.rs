//! The two worlds a component may be built for. AutoScene serves the
//! newest (`wit/`, 1.1.0): its `host` is what every plugin imports (a
//! `@1.0.0` import is served by the `@1.1.0` definition, as wasmtime links
//! a semver-compatible one), and its types are what every run takes. A
//! plugin of 1.0.0 exports only `run`; the bindings of 1.1.0 would look up
//! `on-event` too and refuse it, so it is linked through the bindings of
//! `wit-frozen/1.0.0`, which share the types and `host` of 1.1.0 (a record
//! never changes in 1.x). Which one a component gets is decided by what it
//! exports ([`Pre::new`]).

#![allow(missing_docs, clippy::all, clippy::pedantic)]

pub(crate) mod v1_1 {
    wasmtime::component::bindgen!({ world: "plugin", path: "wit", exports: { default: async } });
}

pub(crate) mod v1_0 {
    wasmtime::component::bindgen!({
        world: "plugin",
        path: "wit-frozen/1.0.0",
        exports: { default: async },
        with: {
            "autoscene:plugin/types": super::v1_1::autoscene::plugin::types,
            "autoscene:plugin/host": super::v1_1::autoscene::plugin::host,
        },
    });
}

use wasmtime::component::{Component, InstancePre};
use wasmtime::{Engine, Store};

use self::v1_1::autoscene::plugin::types::{EventRequest, RunError, RunRequest};
use super::state::State;

/// The export of 1.1.0 that a plugin of 1.0.0 doesn't have.
const ON_EVENT: &str = "on-event";

/// What a call into a plugin gives: its result, or the trap it ended with.
pub(crate) type Called = wasmtime::Result<Result<Option<String>, RunError>>;

/// A component linked against what AutoScene offers, with its exports
/// found, by the world it was built for: ready to instantiate, which runs
/// nothing yet.
pub(crate) enum Pre {
    /// `run` only.
    V1_0(v1_0::PluginPre<State>),
    /// `run` and `on-event`.
    V1_1(v1_1::PluginPre<State>),
}

/// What a run hands its plugin.
#[derive(Debug)]
pub(crate) enum Request {
    /// An action: `run`.
    Run(RunRequest),
    /// An event: `on-event`, which only a plugin of 1.1.0 has.
    Event(EventRequest),
}

/// Whether `component` exports `on-event` (the world 1.1.0); none of it runs.
fn on_event(engine: &Engine, component: &Component) -> bool {
    component.component_type().exports(engine).any(|(name, _)| name == ON_EVENT)
}

impl Pre {
    /// `pre` with the exports of the world `component` was built for found.
    pub(crate) fn new(engine: &Engine, component: &Component, pre: InstancePre<State>) -> wasmtime::Result<Self> {
        Ok(match on_event(engine, component) {
            true => Self::V1_1(v1_1::PluginPre::new(pre)?),
            false => Self::V1_0(v1_0::PluginPre::new(pre)?),
        })
    }

    /// Whether its plugin has `on-event`.
    pub(crate) fn takes_events(&self) -> bool {
        matches!(self, Self::V1_1(_))
    }

    /// Whether its plugin can be sent `request`: an event needs `on-event`.
    pub(crate) fn takes(&self, request: &Request) -> bool {
        matches!(request, Request::Run(_)) || self.takes_events()
    }

    /// Instantiates the plugin in `store` (its start functions run here)
    /// and calls the export `request` is for. One of 1.0.0 is never sent
    /// an event ([`Pre::takes`]).
    pub(crate) async fn call(&self, store: &mut Store<State>, request: &Request) -> Called {
        match (self, request) {
            (Self::V1_0(pre), Request::Run(r)) => pre.instantiate_async(&mut *store).await?.call_run(store, r).await,
            (Self::V1_1(pre), Request::Run(r)) => pre.instantiate_async(&mut *store).await?.call_run(store, r).await,
            (Self::V1_1(pre), Request::Event(e)) => {
                pre.instantiate_async(&mut *store).await?.call_on_event(store, e).await
            }
            (Self::V1_0(_), Request::Event(_)) => Err(wasmtime::format_err!("a plugin of 1.0.0 has no {ON_EVENT}")),
        }
    }
}

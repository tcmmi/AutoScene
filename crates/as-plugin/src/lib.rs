//! Plugins: programs by other people that AutoScene runs on request, after
//! an export, from the command palette or from the plugin's own pages. A
//! plugin describes itself in a `plugin.json` ([`manifest`]); AutoScene
//! draws what it adds (menu entries, commands, settings) and runs it as a
//! separate process. It comes as a
//! package ([`package`]): a zip whose `files.lock` lists and hashes each of
//! its files. Who signed that list decides the plugin's trust ([`trust`]):
//! AutoScene's review, its author's pinned key, or nobody AutoScene knows.
//! AutoScene's plugin catalog ([`catalog`]) is a signed index of the
//! versions it reviewed, by the hash of their lock, and of those it withdrew.
//! Its packages download as AutoScene's models do; what fails is said in
//! words about the plugin ([`fetch`]).
//! The installed plugins, and what the person set for them, are kept by
//! [`store`]; the values of secret settings by the system's keychain
//! ([`secrets`]).
//!
//! A run ([`runtime`]) starts the plugin's program and speaks the protocol
//! ([`protocol`]) with it over its stdin and stdout; what else it writes
//! goes to its log ([`log`]). A Python script runs through AutoScene's own
//! uv, which comes pinned, the first time one needs it ([`uv`]). The large
//! files a plugin names come the same way, at its first run, each checked
//! by its hash ([`resources`]). From protocol 2 a plugin may call AutoScene
//! for its project, its exports and its settings, keep a little state
//! ([`page_state`]), and reach the hosts it names on the internet
//! ([`net`]); each call is checked against what the plugin may use
//! ([`calls`]).
//!
//! A run of protocol 2 gets a scratch folder of its own, which AutoScene
//! empties when the run ends, and its temp files land there ([`scratch`]).
//!
//! A plugin's pages are its files under `ui/`, which AutoScene shows in a
//! sandboxed frame and serves itself, each as its package lists it
//! ([`pages`]). A plugin whose runtime is a service keeps its program running
//! while one of its pages is open, and answers their requests ([`service`]).
//!
//! A plugin may instead be a WebAssembly component (manifest version 2,
//! runtime `wasm`), which runs in a sandbox (`wasm`, with the feature of
//! that name): it sees only the folders its permissions give it, reaches
//! only the hosts it names, as `net.fetch` does, and is held to its memory
//! and time limits. This AutoScene doesn't install one yet: it reads its
//! manifest and checks its package, and says it needs a newer AutoScene
//! ([`store`]).
//!
//! What a plugin says it does (its permissions) is shown before install.
//! Its calls into AutoScene are held to it; what its program does itself is
//! not: a process plugin runs with the person's user rights. A WebAssembly
//! plugin's permissions are what its sandbox gives it.

// The compiled code of WebAssembly plugins is the one exception (wasm/cache.rs).
#![deny(unsafe_code)]

pub mod calls;
pub mod catalog;
pub mod error;
pub mod fetch;
pub mod keys;
pub mod log;
pub mod manifest;
pub mod net;
pub mod package;
pub mod page_state;
pub mod pages;
pub mod protocol;
pub mod resources;
pub mod runtime;
pub mod scratch;
pub mod secrets;
pub mod service;
pub mod store;
pub mod trust;
pub mod uv;
#[cfg(feature = "wasm")]
pub mod wasm;

pub use calls::{CallCode, CallError, Caller, Grants, HostCalls, HostMethod};
pub use catalog::{Catalog, CatalogKeys, CatalogState};
pub use error::{PluginError, PluginResult};
pub use keys::KeyRole;
pub use manifest::{
    ActionSpec, Author, CommandSpec, Engines, EventKind, EventSpec, Launch, Manifest, PackActionSpec, PackFileSpec,
    PageScope, PageSpec, Parsed, Permission, PluginId, Resource, Runtime, RuntimeChange, RuntimeKind, SelectOption,
    SettingKind, SettingSpec, SettingValue, ShotFilterSpec, SourceLink, SourceSpec, WasmLimits, current_platform,
};
pub use package::{FilesLock, LockedFile, Unpacked};
pub use protocol::{
    ActionContext, ActionKind, AppContext, CallId, CharacterContext, ClipContext, EpisodeContext, EpisodeCounts,
    EventContext, ExportContext, FaceBox, PackClipContext, PackContext, PluginContext, ProjectContext, RequestContext,
    RunMessage, ServeMessage, ServiceMessage, ShotContext, ShotsContext, SourceContext, ToolPaths, Trigger,
};
pub use resources::Resources;
pub use runtime::{FileSink, Report, RunSpec, VerdictSink};
pub use secrets::{MemorySecrets, OsKeyring, SecretStore};
pub use service::{OnEvent, ProcessService, Service, ServiceEvent, ServiceSpec};
pub use store::{Approval, Extra, Installed, Record, Source, Store, Ticket, Widens};
pub use trust::{AuthorKey, Trust, Verdict};

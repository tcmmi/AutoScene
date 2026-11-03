//! Plugins: programs by other people that AutoScene runs on request, after
//! an export or from the command palette. A plugin describes itself in a
//! `plugin.json` ([`manifest`]); AutoScene draws what it adds (menu entries,
//! commands, settings) and runs it as a separate process. It comes as a
//! package ([`package`]): a zip whose `files.lock` lists and hashes each of
//! its files. Who signed that list decides the plugin's trust ([`trust`]):
//! AutoScene's review, its author's pinned key, or nobody AutoScene knows.
//! The installed plugins, and what the person set for them, are kept by
//! [`store`]; the values of secret settings by the system's keychain
//! ([`secrets`]).
//!
//! A run ([`runtime`]) starts the plugin's program and speaks the protocol
//! ([`protocol`]) with it over its stdin and stdout; what else it writes
//! goes to its log ([`log`]). From protocol 2 a plugin may call AutoScene
//! for its project, its exports and its settings, keep a little state
//! ([`page_state`]), and reach the hosts it names on the internet
//! ([`net`]); each call is checked against what the plugin may use
//! ([`calls`]).
//!
//! What a plugin says it does (its permissions) is shown before install.
//! Its calls into AutoScene are held to it; what its program does itself is
//! not: a process plugin runs with the person's user rights.

pub mod calls;
pub mod error;
pub mod keys;
pub mod log;
pub mod manifest;
pub mod net;
pub mod package;
pub mod page_state;
pub mod protocol;
pub mod runtime;
pub mod secrets;
pub mod store;
pub mod trust;

pub use calls::{CallCode, CallError, Caller, Grants, HostCalls, HostMethod};
pub use error::{PluginError, PluginResult};
pub use manifest::{
    ActionSpec, Author, CommandSpec, Engines, Launch, Manifest, Parsed, Permission, PluginId, Runtime, RuntimeKind,
    SelectOption, SettingKind, SettingSpec, SettingValue, current_platform,
};
pub use package::{FilesLock, LockedFile, Unpacked};
pub use protocol::{
    ActionContext, ActionKind, AppContext, CallId, ClipContext, ExportContext, PluginContext, ProjectContext,
    RunMessage, ToolPaths, Trigger,
};
pub use runtime::RunSpec;
pub use secrets::{MemorySecrets, OsKeyring, SecretStore};
pub use store::{Extra, Installed, Record, Source, Store, Ticket};
pub use trust::{AuthorKey, Keys, Trust, Verdict};

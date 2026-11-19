//! AutoScene Cloud: a scan runs on a rented GPU instead of this computer.
//!
//! Three programs speak these types:
//! - the app, through its [`client`] (feature `client`);
//! - the gateway on the owner's server: accounts, money, uploads, scan
//!   sessions, the Discord bot's admin calls;
//! - the GPU worker, one per scan session, which runs the unchanged analysis
//!   engine on the uploaded file and hands its replies back line by line.
//!
//! The engine's own protocol (`as_engine::protocol`) travels through untouched
//! as JSON lines: the gateway only reads the few fields it prices and checks
//! ([`engine`]), so it never links the engine and its AI runtime.
//!
//! Money is counted in micro-euros ([`Micros`]): prices go below a cent per
//! minute. The balance lives on the gateway alone; the app only shows it.

pub mod code;
pub mod engine;
mod kind;
mod money;
mod wire;

#[cfg(feature = "client")]
pub mod client;

pub use kind::{Depth, FacesFlags, Footage, ScanKind, Switch};
pub use money::{MICROS_PER_CENT, MICROS_PER_EURO, Micros, PriceBasis, Prices, TOP_UPS_CENTS, euros};
pub use wire::*;

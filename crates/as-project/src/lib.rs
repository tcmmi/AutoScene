//! The library and its projects.
//!
//! A project is a `<Title>.autoscene` folder: `project.db` plus generated
//! files (`thumbs/`, `faces/`, `proxies/`, `tmp/`). Source media are referenced, never
//! copied or modified. Everything here is synchronous and Tauri-free; the app
//! runs the long operations (ingest, proxies, exports) on `as-jobs` workers.

mod analysis;
pub mod anilist;
mod bodies;
pub(crate) mod cast;
mod error;
pub mod fandom;
mod library;
pub mod naming;
mod pack;
mod project;
mod rescan;
pub mod scan;
pub mod tvmaze;
mod verdicts;

pub use analysis::{Pass, ShotsPlan};
/// What this computer analyses on: [`Library::open_with`] takes its probe.
pub use as_infer::computer;
pub use bodies::BodiesPlan;
pub use cast::{FACE_CHUNK, FacesPlan, PORTRAIT, Portrait, StoppedStage, face_models};
pub use error::{ProjectError, ProjectResult};
pub use library::Library;
pub use pack::{CheckShot, CutEpisode, FilterCheck, PackCut, PackSizing};
pub use project::{Project, Recovery, RelinkedFolder};
pub use rescan::{DoneRead, PAIR_READS, PairToCheck, RescanCast, RescanSource};
pub use verdicts::{PluginFilter, PluginFilters, basis};

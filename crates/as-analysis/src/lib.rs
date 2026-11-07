//! Analysis: the resumable passes over a media and what they produce —
//! frame signals fused into shots and transitions, face tracks (found in
//! full or, with Fast Scan, the way the Scener app finds them), and the
//! people around the faces (body tracks tied to them).

pub mod anime;
pub mod bodies;
pub mod body_tracker;
pub mod faces;
pub mod fast;
pub mod fusion;
pub mod link;
pub mod pipeline;
pub mod portrait;
pub mod rescan;
pub mod signals;
pub mod sprites;
pub mod tracker;
pub mod tracks;

pub use anime::{
    Anime, FastAnime, Params as AnimeParams, face_tracks as anime_face_tracks, frame_size as anime_frame_size,
};
pub use faces::{
    BodyModels, CLEAR_FACE, DETECT_BATCH, FaceChunk, FaceParams, Faces, Look, PERSON_INPUT, PeopleParams, Search,
    detect_looks, face_tracks, find_faces, frame_sizes,
};
pub use fast::{FastModels, FastParams, Finder, Spot, fast_face_tracks};
pub use fusion::{FusionParams, Segment, SegmentKind, TransitionKind, fuse};
pub use pipeline::{CHUNK, shot_signals};
pub use portrait::{crowding, portrait_region};
pub use signals::FrameSignals;
pub use sprites::ShotSprite;
pub use tracks::{FaceDet, FaceTrack};

use std::fmt;

use as_core::{Msg, ToMsg, msg};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AnalysisError {
    Media(#[from] as_media::MediaError),
    Infer(#[from] as_infer::InferError),
    Internal(String),
    /// A checkpoint couldn't be stored (the caller's error, as text).
    Store(String),
}

impl ToMsg for AnalysisError {
    fn msg(&self) -> Msg {
        match self {
            Self::Media(e) => e.msg(),
            Self::Infer(e) => e.msg(),
            Self::Internal(_) => {
                msg!("analysis.failed", "Something went wrong while analysing. Details are in the log.")
            }
            Self::Store(said) => Msg::plain(said),
        }
    }
}

impl fmt::Display for AnalysisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.msg().text)
    }
}

impl AnalysisError {
    pub fn detail(&self) -> String {
        match self {
            Self::Media(e) => e.detail(),
            Self::Infer(e) => e.detail(),
            Self::Internal(e) => format!("analysis: {e}"),
            Self::Store(e) => format!("checkpoint: {e}"),
        }
    }

    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Media(e) if e.is_cancelled())
    }
}

#[cfg(test)]
mod said {
    use super::*;

    #[test]
    fn each_error_says_its_key_or_passes_on_the_one_it_holds() {
        let said = [
            (
                AnalysisError::Internal("x".into()),
                "analysis.failed",
                "Something went wrong while analysing. Details are in the log.",
            ),
            (AnalysisError::Store("The disk is full.".into()), "", "The disk is full."),
            (AnalysisError::Media(as_media::MediaError::Cancelled), "media.cancelled", "Cancelled."),
            (
                AnalysisError::Infer(as_infer::InferError::Model("x".into())),
                "infer.model",
                "An AI model couldn't be loaded. Details are in the log.",
            ),
        ];
        for (e, key, english) in said {
            assert_eq!((e.msg().key.as_str(), e.to_string().as_str()), (key, english));
        }
    }
}

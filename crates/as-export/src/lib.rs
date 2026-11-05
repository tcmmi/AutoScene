//! Export: turning frame ranges into files editors can trust.

pub mod clip;
pub mod edl;
pub mod estimate;
pub mod lossless;
pub mod merge;
pub mod preset;

pub use clip::{ClipJob, ClipOutcome, Part, export_clip};
pub use edl::{EdlError, EdlEvent, reel_names, write_cmx3600, write_csv};
pub use estimate::{EstimateClip, estimate_bytes};
pub use lossless::{LosslessCodec, exact_format};
pub use merge::{
    JoinJob, JoinPart, LosslessShape, MergePlan, MergeShape, MergeSource, PartTiming, join_parts,
    lossless_merge_blocker, plan_lossless_merge, plan_merge,
};
pub use preset::{ExportPreset, builtin_presets};

use std::fmt;

use as_core::{Msg, ToMsg, msg};
use thiserror::Error;

pub type ExportResult<T> = Result<T, ExportError>;

#[derive(Debug, Error)]
pub enum ExportError {
    EncoderMissing(String),
    BadRange(String),
    /// Why, as a clause of the sentence that says so.
    BadPreset(Msg),
    /// A Bit-Perfect clip can't be, or didn't come out, exactly the source:
    /// why, as a clause of the sentence that says so.
    Inexact(Msg),
    /// The parts of a merged export couldn't be joined (the detail says why).
    Join(String),
    Media(#[from] as_media::MediaError),
    Io(#[from] std::io::Error),
}

impl ToMsg for ExportError {
    fn msg(&self) -> Msg {
        match self {
            Self::EncoderMissing(_) => {
                msg!("export.encoderMissing", "This export format isn't available on this computer.")
            }
            Self::BadRange(_) => msg!("export.badRange", "Those frames are outside the video."),
            Self::BadPreset(why) => {
                msg!("export.badPreset", "This export preset can't be used: {{why}}.", why = why.clone())
            }
            Self::Inexact(why) => {
                msg!("export.inexact", "This clip can't be exported bit-perfect: {{why}}.", why = why.clone())
            }
            Self::Join(_) => msg!("export.joinFailed", "The clips couldn't be joined into one file."),
            Self::Media(e) => e.msg(),
            Self::Io(e) => e.msg(),
        }
    }
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.msg().text)
    }
}

impl ExportError {
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Media(m) if m.is_cancelled())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_error_has_its_key_and_keeps_its_words() {
        let why = || msg!("export.why.needsMov", "uncompressed audio needs a MOV file");
        let said = [
            (
                ExportError::EncoderMissing("ffv1".into()),
                "export.encoderMissing",
                "This export format isn't available on this computer.",
            ),
            (ExportError::BadRange("0..2".into()), "export.badRange", "Those frames are outside the video."),
            (
                ExportError::BadPreset(why()),
                "export.badPreset",
                "This export preset can't be used: uncompressed audio needs a MOV file.",
            ),
            (
                ExportError::Inexact(why()),
                "export.inexact",
                "This clip can't be exported bit-perfect: uncompressed audio needs a MOV file.",
            ),
            (ExportError::Join("no".into()), "export.joinFailed", "The clips couldn't be joined into one file."),
            (ExportError::Media(as_media::MediaError::Cancelled), "media.cancelled", "Cancelled."),
            (ExportError::Io(std::io::ErrorKind::StorageFull.into()), "file.diskFull", "The disk is full."),
        ];
        for (e, key, english) in said {
            assert_eq!((e.msg().key.as_str(), e.to_string().as_str()), (key, english));
        }
    }
}

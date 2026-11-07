use std::fmt;

use as_core::{Msg, ToMsg, msg};
use thiserror::Error;

pub type MediaResult<T> = Result<T, MediaError>;

/// `Display` is what people read in the app, so it never names tools or
/// dumps their output; [`MediaError::detail`] has the technical story for
/// logs and bug reports.
#[derive(Debug, Error)]
pub enum MediaError {
    ToolsMissing,
    ToolsTooOld(String),
    Cancelled,
    NoVideo(String),
    Probe { path: String, message: String },
    Ffmpeg(String),
    Verify(String),
    Core(#[from] as_core::CoreError),
    Io(#[from] std::io::Error),
}

impl fmt::Display for MediaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.msg().text)
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

impl ToMsg for MediaError {
    fn msg(&self) -> Msg {
        match self {
            Self::ToolsMissing => {
                msg!("media.toolsMissing", "AutoScene's video tools are missing. Reinstall AutoScene.")
            }
            Self::ToolsTooOld(_) => {
                msg!("media.toolsTooOld", "AutoScene's video tools are out of date. Reinstall AutoScene.")
            }
            Self::Cancelled | Self::Core(as_core::CoreError::Cancelled) => msg!("media.cancelled", "Cancelled."),
            Self::NoVideo(path) => msg!("media.noVideo", "{{file}} has no video.", file = file_name(path)),
            Self::Probe { path, .. } => msg!(
                "media.unreadable",
                "{{file}} couldn't be read. It may be damaged or in a format AutoScene can't open.",
                file = file_name(path)
            ),
            Self::Ffmpeg(_) if self.is_disk_full() => msg!("file.diskFull", "The disk is full."),
            Self::Ffmpeg(_) => msg!("media.failed", "The video couldn't be processed. Details are in the log."),
            Self::Verify(_) => msg!("media.frameCheck", "The result didn't pass the frame check."),
            // A frame index that can't be built or read: the video's timing is
            // broken (or the stored index is), never something the person did.
            Self::Core(_) => {
                msg!("media.frameOrder", "The video's frames couldn't be put in order. Details are in the log.")
            }
            Self::Io(e) => e.msg(),
        }
    }
}

impl MediaError {
    /// Technical description for logs (tool output, versions, paths).
    pub fn detail(&self) -> String {
        match self {
            Self::ToolsMissing => "ffmpeg/ffprobe not found (bundled folder, AUTOSCENE_FFMPEG_DIR, PATH)".into(),
            Self::ToolsTooOld(v) => format!("ffmpeg {v} is too old; 7 or newer is required"),
            Self::Probe { path, message } => format!("ffprobe {path}: {message}"),
            Self::Ffmpeg(tail) => format!("ffmpeg failed: {tail}"),
            Self::Verify(why) => format!("verification failed: {why}"),
            Self::Core(e) => format!("frame index: {e}"),
            Self::Io(e) => format!("file: {e}"),
            e => e.to_string(),
        }
    }

    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled | Self::Core(as_core::CoreError::Cancelled))
    }

    /// ffmpeg stopped because the disk it writes to is full.
    pub fn is_disk_full(&self) -> bool {
        matches!(self, Self::Ffmpeg(tail) if tail.to_lowercase().contains("no space left on device"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_disk_says_so_in_any_case_of_the_words() {
        let full = MediaError::Ffmpeg("Error writing trailer: No space left on device".into());
        assert!(full.is_disk_full());
        assert_eq!(full.msg().key, "file.diskFull");
        assert_eq!(MediaError::Ffmpeg("Invalid data".into()).msg().key, "media.failed");
    }

    #[test]
    fn its_msg_says_what_its_display_says() {
        let keyed = [
            (MediaError::ToolsMissing, "media.toolsMissing"),
            (MediaError::ToolsTooOld("4.0".into()), "media.toolsTooOld"),
            (MediaError::Io(std::io::ErrorKind::StorageFull.into()), "file.diskFull"),
            (MediaError::Cancelled, "media.cancelled"),
            (MediaError::Core(as_core::CoreError::Cancelled), "media.cancelled"),
            (MediaError::Core(as_core::CoreError::EmptyIndex), "media.frameOrder"),
            (MediaError::NoVideo("C:/x/Pilot.mkv".into()), "media.noVideo"),
            (MediaError::Probe { path: "/x/Pilot.mkv".into(), message: "bad".into() }, "media.unreadable"),
            (MediaError::Ffmpeg("exit 1".into()), "media.failed"),
            (MediaError::Verify("frames".into()), "media.frameCheck"),
        ];
        for (e, key) in keyed {
            assert_eq!(e.msg().key, key);
        }
    }

    #[test]
    fn its_words_are_the_ones_it_always_had() {
        let said = [
            (MediaError::ToolsMissing, "AutoScene's video tools are missing. Reinstall AutoScene."),
            (MediaError::ToolsTooOld("4.0".into()), "AutoScene's video tools are out of date. Reinstall AutoScene."),
            (MediaError::Cancelled, "Cancelled."),
            (MediaError::Core(as_core::CoreError::Cancelled), "Cancelled."),
            (MediaError::NoVideo("C:/x/Pilot.mkv".into()), "Pilot.mkv has no video."),
            (
                MediaError::Probe { path: "/x/Pilot.mkv".into(), message: "bad".into() },
                "Pilot.mkv couldn't be read. It may be damaged or in a format AutoScene can't open.",
            ),
            (MediaError::Ffmpeg("exit 1".into()), "The video couldn't be processed. Details are in the log."),
            (MediaError::Verify("frames".into()), "The result didn't pass the frame check."),
            (
                MediaError::Core(as_core::CoreError::EmptyIndex),
                "The video's frames couldn't be put in order. Details are in the log.",
            ),
            (MediaError::Io(std::io::ErrorKind::StorageFull.into()), "The disk is full."),
        ];
        for (e, english) in said {
            assert_eq!(e.to_string(), english);
        }
    }
}

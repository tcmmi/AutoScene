use as_core::{Msg, ToMsg, msg};
use thiserror::Error;

pub type MediaResult<T> = Result<T, MediaError>;

/// `Display` is what people read in the app, so it never names tools or
/// dumps their output; [`MediaError::detail`] has the technical story for
/// logs and bug reports.
#[derive(Debug, Error)]
pub enum MediaError {
    #[error("AutoScene's video tools are missing. Reinstall AutoScene.")]
    ToolsMissing,
    #[error("AutoScene's video tools are out of date. Reinstall AutoScene.")]
    ToolsTooOld(String),
    #[error("Cancelled.")]
    Cancelled,
    #[error("{} has no video.", file_name(.0))]
    NoVideo(String),
    #[error("{} couldn't be read. It may be damaged or in a format AutoScene can't open.", file_name(path))]
    Probe { path: String, message: String },
    #[error("The video couldn't be processed. Details are in the log.")]
    Ffmpeg(String),
    #[error("The result didn't pass the frame check.")]
    Verify(String),
    #[error("{}", core_message(.0))]
    Core(#[from] as_core::CoreError),
    #[error("{}", as_core::io_message(.0))]
    Io(#[from] std::io::Error),
}

/// A frame index that can't be built or read: the video's timing is broken
/// (or the stored index is), never something the person did.
fn core_message(e: &as_core::CoreError) -> &'static str {
    match e {
        as_core::CoreError::Cancelled => "Cancelled.",
        _ => "The video's frames couldn't be put in order. Details are in the log.",
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// Its `Display`, with a key where the words have one so far.
impl ToMsg for MediaError {
    fn msg(&self) -> Msg {
        match self {
            Self::ToolsMissing => {
                msg!("media.toolsMissing", "AutoScene's video tools are missing. Reinstall AutoScene.")
            }
            Self::ToolsTooOld(_) => {
                msg!("media.toolsTooOld", "AutoScene's video tools are out of date. Reinstall AutoScene.")
            }
            Self::Io(e) => e.msg(),
            e => Msg::plain(e.to_string()),
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_msg_says_what_its_display_says() {
        let keyed = [
            (MediaError::ToolsMissing, "media.toolsMissing"),
            (MediaError::ToolsTooOld("4.0".into()), "media.toolsTooOld"),
            (MediaError::Io(std::io::ErrorKind::StorageFull.into()), "file.diskFull"),
            (MediaError::Ffmpeg("exit 1".into()), ""),
        ];
        for (e, key) in keyed {
            assert_eq!((e.msg().key.as_str(), e.msg().text), (key, e.to_string()));
        }
    }
}

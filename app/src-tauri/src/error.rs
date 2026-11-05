use as_core::{Msg, ToMsg};
use as_plugin::PluginError;
use as_project::ProjectError;
use serde::Serialize;
use specta::Type;

/// Error returned by every command. Serialised as `{ code, message, msg }`
/// and thrown on the TypeScript side, so UI code can branch on `code` and say
/// `msg` in its language. `message` is its English: what the log gets, and
/// what a plugin page gets from the bridge.
#[derive(Debug, Serialize, Type)]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
    pub msg: Msg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    NotFound,
    /// The request itself is wrong (bad range, empty name…); `message` says why.
    Invalid,
    /// Exists but isn't usable yet (still indexing, preview being prepared).
    NotReady,
    Cancelled,
    /// ffmpeg is missing or failed.
    Media,
    Io,
    Update,
    Internal,
}

impl AppError {
    pub fn new(code: ErrorCode, msg: Msg) -> Self {
        Self { code, message: msg.text.clone(), msg }
    }

    pub fn internal(msg: Msg) -> Self {
        Self::new(ErrorCode::Internal, msg)
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        log::warn!("file: {e}");
        let code = if e.kind() == std::io::ErrorKind::NotFound { ErrorCode::NotFound } else { ErrorCode::Io };
        Self::new(code, e.msg())
    }
}

/// People read the error's `Display`; the log gets its technical side, so
/// "Details are in the log" holds for commands as it does for jobs.
impl From<ProjectError> for AppError {
    fn from(e: ProjectError) -> Self {
        let code = match &e {
            ProjectError::NotFound(_) => ErrorCode::NotFound,
            ProjectError::Invalid(_) => ErrorCode::Invalid,
            ProjectError::NotReady(_) => ErrorCode::NotReady,
            ProjectError::Cancelled => ErrorCode::Cancelled,
            ProjectError::Media(_) | ProjectError::Export(_) => ErrorCode::Media,
            ProjectError::Io(_) => ErrorCode::Io,
            ProjectError::Db(_) => ErrorCode::Internal,
        };
        match code {
            // Expected answers (a stale id, a file still indexing): no noise.
            ErrorCode::NotFound | ErrorCode::Invalid | ErrorCode::NotReady | ErrorCode::Cancelled => {
                log::debug!("{}", e.detail());
            }
            _ => log::warn!("{}", e.detail()),
        }
        Self::new(code, e.msg())
    }
}

/// As for [`ProjectError`]. A signature that fails or an author key that
/// changed says only that to the person; the log has which and why. A
/// plugin AutoScene withdrew is news, not noise: the log has it too.
impl From<PluginError> for AppError {
    fn from(e: PluginError) -> Self {
        let code = match &e {
            // A page that was closed meanwhile.
            PluginError::NotFound(_) | PluginError::Closed => ErrorCode::NotFound,
            // The keychain may be locked, or start later (Linux's Secret Service).
            PluginError::Busy(_) | PluginError::Keychain { .. } => ErrorCode::NotReady,
            PluginError::Cancelled => ErrorCode::Cancelled,
            PluginError::Io(io) if io.kind() == std::io::ErrorKind::NotFound => ErrorCode::NotFound,
            PluginError::Io(_) => ErrorCode::Io,
            PluginError::Db(_) | PluginError::Failed { .. } | PluginError::Crashed { .. } => ErrorCode::Internal,
            PluginError::Invalid(_)
            | PluginError::TooNew(_)
            | PluginError::Platform
            | PluginError::BadSignature(_)
            | PluginError::KeyChanged { .. }
            | PluginError::NotAllowed
            | PluginError::UvMissing
            | PluginError::Denied(_)
            | PluginError::Catalog(_)
            | PluginError::Withdrawn { .. }
            // A catalog package that didn't come: the page offers to try again.
            | PluginError::Download { .. }
            // A WebAssembly plugin held to its limits.
            | PluginError::TimedOut { .. }
            | PluginError::OutOfMemory { .. } => ErrorCode::Invalid,
        };
        match (&e, code) {
            // Why a download failed is in its detail, which only the log keeps.
            (
                PluginError::BadSignature(_)
                | PluginError::KeyChanged { .. }
                | PluginError::Keychain { .. }
                | PluginError::Download { .. },
                _,
            ) => {
                log::warn!("{}", e.detail());
            }
            (PluginError::Withdrawn { .. }, _) => log::info!("{}", e.detail()),
            (_, ErrorCode::NotFound | ErrorCode::Invalid | ErrorCode::NotReady | ErrorCode::Cancelled) => {
                log::debug!("{}", e.detail());
            }
            _ => log::warn!("{}", e.detail()),
        }
        Self::new(code, e.msg())
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use std::io::{Error, ErrorKind};

    use as_media::MediaError;

    use super::*;

    #[test]
    fn plugin_errors_get_their_codes_and_keep_their_words() {
        let said = |e: PluginError| {
            let e = AppError::from(e);
            assert_eq!(e.msg.text, e.message);
            (e.code, e.message)
        };
        let invalid = [
            PluginError::Invalid("Its name is missing.".into()),
            PluginError::TooNew("engines.autoscene".into()),
            PluginError::Platform,
            PluginError::BadSignature("hash".into()),
            PluginError::KeyChanged { pinned: "A".into(), got: "B".into() },
            PluginError::NotAllowed,
            PluginError::UvMissing,
            PluginError::Denied("A run reads only the project it runs in.".into()),
            PluginError::Catalog("This catalog is older than the one AutoScene has.".into()),
            PluginError::Withdrawn { name: "Leaky".into(), reason: "It sent export paths to a server.".into() },
            PluginError::Download { what: "Poster".into(), detail: "connection refused".into() },
            PluginError::TimedOut { name: "Poster".into(), minutes: 30 },
            PluginError::OutOfMemory { name: "Poster".into(), mb: 512 },
        ];
        for e in invalid {
            let words = e.to_string();
            assert_eq!(said(e), (ErrorCode::Invalid, words));
        }
        let withdrawn =
            PluginError::Withdrawn { name: "Leaky".into(), reason: "It sent export paths to a server.".into() };
        assert_eq!(said(withdrawn).1, "AutoScene withdrew Leaky. It sent export paths to a server.");
        // What went wrong is the log's, never the message's.
        let download = PluginError::Download { what: "Poster".into(), detail: "connection refused".into() };
        assert_eq!(said(download).1, "AutoScene couldn't get Poster. Check the internet connection, then try again.");
        assert_eq!(said(PluginError::NotFound("dev.a.b".into())).0, ErrorCode::NotFound);
        assert_eq!(said(PluginError::Closed), (ErrorCode::NotFound, "This page was closed.".into()));
        assert_eq!(
            said(PluginError::Busy("Poster".into())),
            (ErrorCode::NotReady, "Poster is running. Wait for it to finish, or stop it in Activity.".into())
        );
        assert_eq!(said(PluginError::Cancelled).0, ErrorCode::Cancelled);
        assert_eq!(
            said(PluginError::Keychain { reason: "it is locked.".into(), detail: Some("Locked".into()) }),
            (ErrorCode::NotReady, "Your system's keychain can't be used: it is locked.".into())
        );
        // A file: missing, else any other trouble.
        assert_eq!(said(PluginError::Io(Error::from(ErrorKind::NotFound))).0, ErrorCode::NotFound);
        assert_eq!(said(PluginError::Io(Error::from(ErrorKind::PermissionDenied))).0, ErrorCode::Io);
        // A run's own failures are the plugin's, not the request's.
        let failed = PluginError::Failed { message: "The webhook said no.".into(), detail: None };
        assert_eq!(said(failed), (ErrorCode::Internal, "The webhook said no.".into()));
        let crashed = PluginError::Crashed { name: "Poster".into(), detail: "exit 1".into() };
        assert_eq!(said(crashed), (ErrorCode::Internal, "Poster stopped unexpectedly.".into()));
    }

    #[test]
    fn a_full_disk_has_its_key_and_keeps_its_words() {
        for e in [
            AppError::from(Error::from(ErrorKind::StorageFull)),
            AppError::from(ProjectError::Io(Error::from(ErrorKind::StorageFull))),
            AppError::from(PluginError::Io(Error::from(ErrorKind::StorageFull))),
        ] {
            assert_eq!((e.code, e.msg.key.as_str()), (ErrorCode::Io, "file.diskFull"));
            assert_eq!(e.message, "The disk is full.");
        }
    }

    #[test]
    fn an_error_of_the_media_tools_keeps_its_key_inside_a_project_error() {
        let e = AppError::from(ProjectError::Media(MediaError::ToolsMissing));
        assert_eq!((e.code, e.msg.key.as_str()), (ErrorCode::Media, "media.toolsMissing"));
        assert_eq!(e.message, MediaError::ToolsMissing.to_string());
    }

    #[test]
    fn a_project_error_reaches_the_ui_with_its_key_and_its_english() {
        let e = AppError::from(ProjectError::NotFound(as_core::msg!("episode.removed", "This episode was removed.")));
        assert_eq!((e.code, e.msg.key.as_str()), (ErrorCode::NotFound, "episode.removed"));
        assert_eq!(e.message, "This episode was removed.");
        assert_eq!(e.msg.text, e.message);

        let name = "E01";
        let said = as_core::msg!("episode.notIndexed", "{{name}} isn't indexed yet.", name = name);
        let e = AppError::from(ProjectError::NotReady(said.clone()));
        assert_eq!((e.code, &e.msg), (ErrorCode::NotReady, &said));
        assert_eq!(e.message, "E01 isn't indexed yet.");

        let e = AppError::from(ProjectError::Db(as_db::DbError::TooNew));
        assert_eq!((e.code, e.msg.key.as_str()), (ErrorCode::Internal, "db.tooNew"));
        assert!(e.message.contains("Update AutoScene"), "{e}");

        let e = AppError::from(ProjectError::Cancelled);
        assert_eq!(
            (e.code, e.msg.key.as_str(), e.message.as_str()),
            (ErrorCode::Cancelled, "job.cancelled", "cancelled")
        );
    }

    #[test]
    fn words_without_a_key_yet_keep_their_english() {
        let e = AppError::from(ProjectError::Invalid(Msg::plain("A panic's own words.")));
        assert_eq!((e.msg.key.as_str(), e.message.as_str()), ("", "A panic's own words."));
        assert_eq!(e.msg.text, e.message);
    }

    #[test]
    fn the_ui_gets_the_code_the_english_and_the_msg() {
        let e = AppError::new(ErrorCode::NotReady, as_core::msg!("a.b", "{{name}} waits.", name = "Poster"));
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "code": "not_ready",
                "message": "Poster waits.",
                "msg": { "key": "a.b", "args": { "name": "Poster" }, "text": "Poster waits." },
            })
        );
    }
}

use as_core::{Msg, ToMsg};
use thiserror::Error;

pub type PluginResult<T> = Result<T, PluginError>;

#[derive(Debug, Error)]
pub enum PluginError {
    /// What is wrong with the plugin, as a sentence for the person installing it.
    #[error("{0}")]
    Invalid(String),
    /// The plugin needs something this AutoScene doesn't have (the field that says so).
    #[error("This plugin needs a newer AutoScene.")]
    TooNew(String),
    #[error("This plugin isn't made for this computer.")]
    Platform,
    #[error("The package was changed after it was signed.")]
    BadSignature(String),
    #[error("This update is signed by a different author than the installed version.")]
    KeyChanged { pinned: String, got: String },
    #[error("Plugins that AutoScene has not reviewed are turned off. You can allow them in Plugins.")]
    NotAllowed,
    #[error("{0} isn't installed.")]
    NotFound(String),
    #[error("{0} is running. Wait for it to finish, or stop it in Activity.")]
    Busy(String),
    /// A uv script, and the host has no uv for it ([`crate::uv`]).
    #[error("This plugin needs uv, and AutoScene couldn't get it.")]
    UvMissing,
    /// The plugin's own error line; or why a `net.fetch` got no answer, with
    /// what the connection said as `detail` (which `net` logs; never the
    /// address).
    #[error("{message}")]
    Failed { message: String, detail: Option<String> },
    #[error("{name} stopped unexpectedly.")]
    Crashed { name: String, detail: String },
    /// A WebAssembly plugin's run took longer than its time limit.
    #[error("{name} ran for longer than {} and was stopped.", for_minutes(.minutes))]
    TimedOut { name: String, minutes: u32 },
    /// A WebAssembly plugin's run needed more memory than its limit.
    #[error("{name} needed more than {mb} MB of memory and was stopped.")]
    OutOfMemory { name: String, mb: u32 },
    #[error("Cancelled.")]
    Cancelled,
    /// A host method the plugin has no grant for; the sentence says which.
    #[error("{0}")]
    Denied(String),
    /// The system's keychain (where secret settings are kept) refused, or
    /// isn't there: why, in plain words, and what it said as `detail`
    /// (logged). Never a secret's value.
    #[error("Your system's keychain can't be used: {reason}")]
    Keychain { reason: String, detail: Option<String> },
    /// The page that asked was closed, or never opened.
    #[error("This page was closed.")]
    Closed,
    /// What is wrong with the catalog, as a sentence ("This catalog is older
    /// than the one AutoScene has.").
    #[error("{0}")]
    Catalog(String),
    /// The catalog withdrew this version of the plugin `name`; `reason` is
    /// the catalog's own sentence.
    #[error("AutoScene withdrew {name}. {reason}")]
    Withdrawn { name: String, reason: String },
    /// A pinned download failed: `what` names it ("Hello Export", "uv"),
    /// and `detail` says what went wrong (for the log).
    #[error("AutoScene couldn't get {what}. Check the internet connection, then try again.")]
    Download { what: String, detail: String },
    #[error("{}", .0.user_message())]
    Db(#[from] as_db::DbError),
    #[error("{}", as_core::io_message(.0))]
    Io(#[from] std::io::Error),
}

impl PluginError {
    /// Technical description for logs.
    pub fn detail(&self) -> String {
        match self {
            Self::TooNew(field) => format!("plugin needs a newer app: {field}"),
            Self::BadSignature(why) => format!("plugin signature: {why}"),
            Self::KeyChanged { pinned, got } => format!("plugin author key changed: pinned {pinned}, got {got}"),
            Self::Failed { message, detail: Some(detail) } => format!("{message}: {detail}"),
            Self::Crashed { name, detail } => format!("{name} crashed: {detail}"),
            Self::Keychain { detail: Some(detail), .. } => format!("keychain: {detail}"),
            Self::Withdrawn { name, reason } => format!("plugin withdrawn: {name}: {reason}"),
            Self::Download { what, detail } => format!("download of {what}: {detail}"),
            Self::Db(e) => format!("plugin registry: {e}"),
            Self::Io(e) => format!("plugin files: {e}"),
            e => e.to_string(),
        }
    }
}

/// Its `Display`, with a key where the words have one so far.
impl ToMsg for PluginError {
    fn msg(&self) -> Msg {
        match self {
            Self::Io(e) => e.msg(),
            e => Msg::plain(e.to_string()),
        }
    }
}

fn for_minutes(n: &u32) -> String {
    match n {
        1 => "1 minute".into(),
        n => format!("{n} minutes"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_that_stopped_a_run_are_said() {
        let late = |minutes| PluginError::TimedOut { name: "Poster".into(), minutes }.to_string();
        assert_eq!(late(30), "Poster ran for longer than 30 minutes and was stopped.");
        assert_eq!(late(1), "Poster ran for longer than 1 minute and was stopped.");
        let big = PluginError::OutOfMemory { name: "Poster".into(), mb: 512 };
        assert_eq!(big.to_string(), "Poster needed more than 512 MB of memory and was stopped.");
        assert_eq!(big.detail(), big.to_string());
    }
}

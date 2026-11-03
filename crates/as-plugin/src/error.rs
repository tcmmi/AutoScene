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
    #[error("Plugins that AutoScene has not reviewed are turned off in Settings.")]
    NotAllowed,
    #[error("{0} isn't installed.")]
    NotFound(String),
    #[error("{0} is running. Wait for it to finish, or stop it in Activity.")]
    Busy(String),
    #[error("This plugin needs uv, which isn't installed. Install it from docs.astral.sh/uv, then try again.")]
    UvMissing,
    /// The plugin's own error line; or why a `net.fetch` got no answer, with
    /// what the connection said as `detail` (which `net` logs; never the
    /// address).
    #[error("{message}")]
    Failed { message: String, detail: Option<String> },
    #[error("{name} stopped unexpectedly.")]
    Crashed { name: String, detail: String },
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
            Self::Db(e) => format!("plugin registry: {e}"),
            Self::Io(e) => format!("plugin files: {e}"),
            e => e.to_string(),
        }
    }
}

use std::fmt;

use as_core::{Msg, ToMsg, msg};
use thiserror::Error;

pub type PluginResult<T> = Result<T, PluginError>;

#[derive(Debug, Error)]
pub enum PluginError {
    /// What is wrong with a plugin, its package or a call of it, in English
    /// for its author: `asp` and the plugin read these words as they are,
    /// and the UI frames them ([`ToMsg`]).
    Manifest(String),
    /// A refusal the person reads, in the UI's language.
    Invalid(Msg),
    /// The plugin needs something this AutoScene doesn't have (the field that says so).
    TooNew(String),
    Platform,
    BadSignature(String),
    KeyChanged {
        pinned: String,
        got: String,
    },
    NotAllowed,
    NotFound(String),
    Busy(String),
    /// A uv script, and the host has no uv for it ([`crate::uv`]).
    UvMissing,
    /// The plugin's own error line; or why a `net.fetch` got no answer, with
    /// what the connection said as `detail` (which `net` logs; never the
    /// address).
    Failed {
        message: String,
        detail: Option<String>,
    },
    Crashed {
        name: String,
        detail: String,
    },
    /// A WebAssembly plugin's run took longer than its time limit.
    TimedOut {
        name: String,
        minutes: u32,
    },
    /// A WebAssembly plugin's run needed more memory than its limit.
    OutOfMemory {
        name: String,
        mb: u32,
    },
    Cancelled,
    /// A host method the plugin has no grant for; the sentence says which.
    Denied(String),
    /// The system's keychain (where secret settings are kept) refused, or
    /// isn't there: why, in plain words, and what it said as `detail`
    /// (logged). Never a secret's value.
    Keychain {
        reason: Msg,
        detail: Option<String>,
    },
    /// The page that asked was closed, or never opened.
    Closed,
    /// What is wrong with the catalog, as a sentence ("This catalog is older
    /// than the one AutoScene has.").
    Catalog(Msg),
    /// The catalog withdrew this version of the plugin `name`; `reason` is
    /// the catalog's own sentence.
    Withdrawn {
        name: String,
        reason: String,
    },
    /// A pinned download failed: `what` names it ("Hello Export", "uv"),
    /// and `detail` says what went wrong (for the log).
    Download {
        what: Msg,
        detail: String,
    },
    Db(#[from] as_db::DbError),
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
            Self::Download { what, detail } => format!("download of {}: {detail}", what.text),
            Self::Db(e) => format!("plugin registry: {e}"),
            Self::Io(e) => format!("plugin files: {e}"),
            e => e.to_string(),
        }
    }
}

impl ToMsg for PluginError {
    fn msg(&self) -> Msg {
        match self {
            // The author's English; a language other than English can frame it.
            Self::Manifest(why) | Self::Denied(why) => msg!("plugin.invalid", "{{why}}", why = why),
            Self::Invalid(said) | Self::Catalog(said) => said.clone(),
            Self::TooNew(_) => msg!("plugin.tooNew", "This plugin needs a newer AutoScene."),
            Self::Platform => msg!("plugin.wrongPlatform", "This plugin isn't made for this computer."),
            Self::BadSignature(_) => msg!("plugin.badSignature", "The package was changed after it was signed."),
            Self::KeyChanged { .. } => {
                msg!("plugin.keyChanged", "This update is signed by a different author than the installed version.")
            }
            Self::NotAllowed => msg!(
                "plugin.notAllowed",
                "Plugins that AutoScene has not reviewed are turned off. You can allow them in Plugins."
            ),
            Self::NotFound(name) => msg!("plugin.notFound", "{{name}} isn't installed.", name = name),
            Self::Busy(name) => {
                msg!("plugin.busy", "{{name}} is running. Wait for it to finish, or stop it in Activity.", name = name)
            }
            Self::UvMissing => msg!("plugin.uvMissing", "This plugin needs uv, and AutoScene couldn't get it."),
            // The plugin's own line, or `net`'s to it.
            Self::Failed { message, .. } => Msg::plain(message),
            Self::Crashed { name, .. } => msg!("plugin.crashed", "{{name}} stopped unexpectedly.", name = name),
            Self::TimedOut { name, minutes } => msg!(
                "plugin.timedOut",
                one = "{{name}} ran for longer than {{count}} minute and was stopped.",
                other = "{{name}} ran for longer than {{count}} minutes and was stopped.",
                count = *minutes,
                name = name
            ),
            Self::OutOfMemory { name, mb } => msg!(
                "plugin.outOfMemory",
                "{{name}} needed more than {{mb}} MB of memory and was stopped.",
                name = name,
                mb = *mb
            ),
            Self::Cancelled => msg!("plugin.cancelled", "Cancelled."),
            Self::Keychain { reason, .. } => {
                msg!("keychain.unusable", "Your system's keychain can't be used: {{reason}}", reason = reason.clone())
            }
            Self::Closed => msg!("plugin.closed", "This page was closed."),
            Self::Withdrawn { name, reason } => {
                msg!("plugin.withdrawn", "AutoScene withdrew {{name}}. {{reason}}", name = name, reason = reason)
            }
            Self::Download { what, .. } => msg!(
                "plugin.downloadFailed",
                "AutoScene couldn't get {{what}}. Check the internet connection, then try again.",
                what = what.clone()
            ),
            Self::Db(e) => e.user_message(),
            Self::Io(e) => e.msg(),
        }
    }
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.msg().text)
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

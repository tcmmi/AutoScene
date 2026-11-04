//! What a pinned download that failed says to the person. A catalog
//! package and AutoScene's uv ([`crate::uv`]) download as AutoScene's
//! models do (`as_models`), whose texts name "the AI model": they never
//! reach the person as they are.

use as_models::ModelError;

use crate::error::PluginError;

/// `e`, a failed download of `what` ("Hello Export", "uv"), as the person
/// reads it: cancelled, no room on the drive, a file that couldn't be
/// written here (as any file AutoScene can't write), or a download that
/// failed. A file that isn't the pinned one fails as a download too; the
/// app logs each failed download's detail at warn, so the log says which.
pub fn fetch_error(what: &str, e: ModelError) -> PluginError {
    match e {
        ModelError::Cancelled => PluginError::Cancelled,
        ModelError::NoSpace { need, free, .. } => PluginError::Invalid(format!(
            "There isn't enough free space for {what}. Free up {} MB on the drive of AutoScene's data folder and \
             try again.",
            need.saturating_sub(free).div_ceil(1_000_000)
        )),
        // Not the internet: a folder that can't be written, a file another
        // program holds, a drive whose free space can't be read.
        ModelError::Io(e) => PluginError::Io(e),
        e => PluginError::Download { what: what.to_owned(), detail: e.detail() },
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn a_failed_download_says_what_it_was_for_and_never_names_a_model() {
        let failed = [
            ModelError::Download("https://example.com/x.asp: connection refused".into()),
            ModelError::Checksum { id: "dev.a.b".into(), got: "0".repeat(64) },
            ModelError::Changed("dev.a.b: 404".into()),
            ModelError::Unknown("dev.a.b".into()),
        ];
        for e in failed {
            let detail = e.detail();
            let got = fetch_error("Hello Export", e);
            assert!(matches!(&got, PluginError::Download { what, .. } if what == "Hello Export"), "{got:?}");
            assert_eq!(
                got.to_string(),
                "AutoScene couldn't get Hello Export. Check the internet connection, then try again."
            );
            // The log keeps what went wrong.
            assert_eq!(got.detail(), format!("download of Hello Export: {detail}"));
        }
        assert!(matches!(fetch_error("uv", ModelError::Cancelled), PluginError::Cancelled));
        // A file that can't be written here is no matter of the internet.
        let denied = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "cache/plugin-downloads");
        let got = fetch_error("Hello Export", ModelError::Io(denied));
        assert!(matches!(&got, PluginError::Io(_)), "{got:?}");
        assert_eq!(got.to_string(), "AutoScene doesn't have permission to use that file or folder.");
        assert_eq!(got.detail(), "plugin files: cache/plugin-downloads");
        // Room on the drive: how much more, in MB rounded up.
        let full = ModelError::NoSpace { dir: PathBuf::from("cache"), need: 70_000_001, free: 10_000_000 };
        let got = fetch_error("uv", full);
        assert!(matches!(got, PluginError::Invalid(_)), "{got:?}");
        assert_eq!(
            got.to_string(),
            "There isn't enough free space for uv. Free up 61 MB on the drive of AutoScene's data folder and try again."
        );
        assert!(!got.to_string().contains("model"));
    }
}

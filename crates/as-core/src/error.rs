use thiserror::Error;

use crate::msg::{Msg, ToMsg};

pub type CoreResult<T> = Result<T, CoreError>;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("cancelled")]
    Cancelled,
    #[error("invalid rational '{0}'")]
    BadRational(String),
    #[error("invalid timecode '{0}'")]
    BadTimecode(String),
    #[error("frame index is empty")]
    EmptyIndex,
    #[error("frame index timestamps are not strictly increasing at frame {0}")]
    NonMonotonic(usize),
    #[error("corrupt frame index blob: {0}")]
    CorruptIndex(String),
}

/// What people read when a file or folder couldn't be read or written. The
/// system's own message (paths, OS error codes) belongs in the log, next to it.
pub fn io_msg(e: &std::io::Error) -> Msg {
    use std::io::ErrorKind as K;
    let in_use = || crate::msg!("file.inUse", "The file is open in another program. Close it there and try again.");
    // Windows reports a file another program holds open as a sharing or lock
    // violation, which std doesn't sort into a kind.
    #[cfg(windows)]
    if matches!(e.raw_os_error(), Some(32 | 33)) {
        return in_use();
    }
    match e.kind() {
        K::NotFound => crate::msg!("file.missing", "A file or folder AutoScene needs isn't there any more."),
        K::PermissionDenied => {
            crate::msg!("file.permission", "AutoScene doesn't have permission to use that file or folder.")
        }
        K::ReadOnlyFilesystem => crate::msg!("file.readOnly", "That drive is read-only."),
        K::StorageFull | K::QuotaExceeded => crate::msg!("file.diskFull", "The disk is full."),
        K::FileTooLarge => crate::msg!("file.tooBig", "The file is too big for that disk."),
        K::ResourceBusy | K::ExecutableFileBusy => in_use(),
        K::InvalidFilename => crate::msg!("file.badName", "That name can't be used for a file."),
        _ => crate::msg!("file.generic", "A file couldn't be read or written. Details are in the log."),
    }
}

/// The English of [`io_msg`], for a `Display` and the log.
pub fn io_message(e: &std::io::Error) -> String {
    io_msg(e).text
}

impl ToMsg for std::io::Error {
    fn msg(&self) -> Msg {
        io_msg(self)
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Error, ErrorKind};

    use super::{io_message, io_msg};

    #[test]
    fn file_errors_read_like_the_app_not_the_os() {
        let os = Error::from_raw_os_error(5);
        assert!(!io_message(&os).contains("os error"), "{}", io_message(&os));
        assert_eq!(io_message(&Error::from(ErrorKind::StorageFull)), "The disk is full.");
        assert!(io_message(&Error::from(ErrorKind::NotFound)).contains("isn't there"));
        assert!(io_message(&Error::from(ErrorKind::PermissionDenied)).contains("permission"));
        assert!(io_message(&Error::other("boom")).contains("Details are in the log"));
    }

    #[test]
    fn each_kind_of_file_error_has_its_key() {
        let key = |kind: ErrorKind| io_msg(&Error::from(kind)).key;
        assert_eq!(key(ErrorKind::NotFound), "file.missing");
        assert_eq!(key(ErrorKind::PermissionDenied), "file.permission");
        assert_eq!(key(ErrorKind::ReadOnlyFilesystem), "file.readOnly");
        assert_eq!(key(ErrorKind::StorageFull), "file.diskFull");
        assert_eq!(key(ErrorKind::QuotaExceeded), "file.diskFull");
        assert_eq!(key(ErrorKind::FileTooLarge), "file.tooBig");
        assert_eq!(key(ErrorKind::ResourceBusy), "file.inUse");
        assert_eq!(key(ErrorKind::ExecutableFileBusy), "file.inUse");
        assert_eq!(key(ErrorKind::InvalidFilename), "file.badName");
        assert_eq!(key(ErrorKind::Other), "file.generic");
    }

    #[cfg(windows)]
    #[test]
    fn a_file_open_elsewhere_says_so() {
        for code in [32, 33] {
            assert!(io_message(&Error::from_raw_os_error(code)).contains("open in another program"));
        }
    }
}

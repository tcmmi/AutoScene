use as_core::{Msg, ToMsg, msg};
use as_db::DbError;
use as_export::ExportError;
use as_media::MediaError;
use thiserror::Error;

pub type ProjectResult<T> = Result<T, ProjectError>;

#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("{}", .0.text)]
    NotFound(Msg),
    #[error("{}", .0.text)]
    Invalid(Msg),
    /// The thing exists but isn't usable yet (e.g. still indexing).
    #[error("{}", .0.text)]
    NotReady(Msg),
    #[error("cancelled")]
    Cancelled,
    #[error("{}", .0.user_message().text)]
    Db(#[from] DbError),
    #[error(transparent)]
    Media(MediaError),
    #[error(transparent)]
    Export(ExportError),
    #[error("{}", as_core::io_message(.0))]
    Io(#[from] std::io::Error),
}

impl ProjectError {
    /// Technical description for logs.
    pub fn detail(&self) -> String {
        match self {
            Self::Media(e) => e.detail(),
            Self::Export(ExportError::Media(e)) => e.detail(),
            Self::Export(e) => format!("{e:?}"),
            Self::Db(e) => format!("database: {e}"),
            Self::Io(e) => format!("file: {e}"),
            e => e.to_string(),
        }
    }
}

/// Its `Display`, with a key where the words have one so far.
impl ToMsg for ProjectError {
    fn msg(&self) -> Msg {
        match self {
            Self::NotFound(said) | Self::Invalid(said) | Self::NotReady(said) => said.clone(),
            Self::Cancelled => msg!("job.cancelled", "cancelled"),
            Self::Db(e) => e.user_message(),
            Self::Media(e) => e.msg(),
            Self::Io(e) => e.msg(),
            e => Msg::plain(e.to_string()),
        }
    }
}

impl From<MediaError> for ProjectError {
    fn from(e: MediaError) -> Self {
        if e.is_cancelled() { Self::Cancelled } else { Self::Media(e) }
    }
}

impl From<ExportError> for ProjectError {
    fn from(e: ExportError) -> Self {
        if e.is_cancelled() { Self::Cancelled } else { Self::Export(e) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_and_file_failures_read_like_the_app() {
        let db = ProjectError::Db(DbError::Corrupt("tracks row 7".into()));
        assert!(!db.to_string().contains("corrupt record"), "{db}");
        assert!(db.detail().contains("corrupt record: tracks row 7"), "{}", db.detail());
        assert!(ProjectError::Db(DbError::TooNew).to_string().contains("Update AutoScene"));

        let full = rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_FULL), None);
        assert_eq!(ProjectError::Db(DbError::Sqlite(full)).to_string(), "The disk is full.");
        let busy = rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY), None);
        assert!(ProjectError::Db(DbError::Sqlite(busy)).to_string().contains("Another program"));

        let io = ProjectError::Io(std::io::Error::other("The device is not ready. (os error 21)"));
        assert!(!io.to_string().contains("os error"), "{io}");
        assert!(io.detail().contains("os error 21"), "{}", io.detail());
    }

    #[test]
    fn its_msg_is_what_it_says() {
        let said = msg!("project.alreadyInProject", "That file is already in the project, as {{name}}.", name = "E01");
        for e in [
            ProjectError::NotFound(said.clone()),
            ProjectError::Invalid(said.clone()),
            ProjectError::NotReady(said.clone()),
        ] {
            assert_eq!(e.to_string(), "That file is already in the project, as E01.");
            assert_eq!(e.msg(), said);
        }
        assert_eq!(ProjectError::Cancelled.to_string(), "cancelled");
        assert_eq!(ProjectError::Cancelled.msg().key, "job.cancelled");
        assert_eq!(ProjectError::Cancelled.msg().text, "cancelled");

        let full = rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_FULL), None);
        let db = ProjectError::Db(DbError::Sqlite(full));
        assert_eq!((db.msg().key.as_str(), db.msg().text), ("db.diskFull", db.to_string()));
        let io = ProjectError::Io(std::io::Error::from(std::io::ErrorKind::StorageFull));
        assert_eq!((io.msg().key.as_str(), io.msg().text), ("file.diskFull", io.to_string()));
    }
}

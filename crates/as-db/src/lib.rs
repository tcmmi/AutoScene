//! Storage. Two SQLite databases, both in WAL mode:
//!
//! - `library.db` in the app data folder: settings and the project list (with
//!   denormalised stats so the Library grid never opens every project);
//! - `project.db` inside each `<Title>.autoscene` folder: media, frame
//!   indexes, exports — and, from M2 on, everything analysis produces.
//!
//! Schemas only ever move forward through `rusqlite_migration` (`user_version`).
//! A database written by a newer AutoScene is refused rather than damaged.

mod bodies;
mod cast;
mod library;
mod pack;
mod project;
mod rescan;

pub use bodies::{BodyEdit, BodyEditKind, BodyRow, BodySource, BodyUpdate, IdentityBody, NewBody, PackBodyRow};
pub use cast::{
    CharacterRow, CharacterShotRow, EditKind, IdentityTrack, NewTrack, OwnFace, TrackEdit, TrackRow, TrackUpdate,
};
pub use library::LibraryDb;
pub use pack::PackRow;
pub use project::{
    IngestRecord, NewExportItem, NewMedia, ProjectDb, ProjectStats, Relinked, VERDICTS_PREFIX, episode_label,
};
pub use rescan::RescanFaceRow;

use std::path::Path;

use as_core::{Msg, msg};
use parking_lot::Mutex;
use rusqlite::{Connection, Transaction};
use rusqlite_migration::Migrations;
use serde::Serialize;
use serde::de::DeserializeOwned;
use thiserror::Error;

pub type DbResult<T> = Result<T, DbError>;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("could not upgrade database: {0}")]
    Migration(#[from] rusqlite_migration::Error),
    #[error("this project was created by a newer version of AutoScene")]
    TooNew,
    #[error("corrupt record: {0}")]
    Corrupt(String),
    /// A file written as part of a transaction failed (the transaction rolled back).
    #[error("file error: {0}")]
    Io(String),
}

impl DbError {
    /// What people read when the database fails. This error's `Display`, with
    /// SQLite's own words, belongs in the log.
    pub fn user_message(&self) -> Msg {
        let generic = || msg!("db.generic", "AutoScene couldn't read or save its data. Details are in the log.");
        let upgrade =
            || msg!("db.upgradeFailed", "AutoScene couldn't update this project's data. Details are in the log.");
        let sqlite = |e: &rusqlite::Error| {
            use rusqlite::ErrorCode as C;
            let rusqlite::Error::SqliteFailure(f, _) = e else { return None };
            Some(match f.code {
                C::DiskFull => msg!("db.diskFull", "The disk is full."),
                C::ReadOnly => {
                    msg!("db.readOnly", "AutoScene can't save its data there: the drive or folder is read-only.")
                }
                C::CannotOpen => {
                    msg!("db.cannotOpen", "AutoScene couldn't open its data. Check that the drive is connected.")
                }
                C::PermissionDenied => msg!("db.permission", "AutoScene doesn't have permission to open its data."),
                C::DatabaseBusy | C::DatabaseLocked => {
                    msg!("db.busy", "Another program is using AutoScene's data. Close it and try again.")
                }
                C::DatabaseCorrupt | C::NotADatabase => {
                    msg!("db.damaged", "AutoScene's data file is damaged. Details are in the log.")
                }
                C::SystemIoFailure => {
                    msg!("db.driveFailed", "The drive couldn't be read or written. Details are in the log.")
                }
                _ => return None,
            })
        };
        match self {
            Self::TooNew => {
                msg!("db.tooNew", "This project was made by a newer version of AutoScene. Update AutoScene to open it.")
            }
            Self::Sqlite(e) => sqlite(e).unwrap_or_else(generic),
            Self::Migration(rusqlite_migration::Error::RusqliteError { err, .. }) => {
                sqlite(err).unwrap_or_else(upgrade)
            }
            Self::Migration(_) => upgrade(),
            Self::Corrupt(_) | Self::Io(_) => generic(),
        }
    }
}

/// One SQLite connection behind a mutex. Every statement here is short
/// (indexed lookups, small writes), so a single connection per database is
/// simpler and just as fast as a pool at AutoScene's scale.
struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    fn open(path: &Path, migrations: &Migrations<'static>) -> DbResult<Self> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let mut conn = Connection::open(path)?;
        // Before the pragmas: a journal_mode change can't wait on a busy
        // database — set the budget first so every pragma below honours it.
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        tracing::debug!(db = %path.display(), "opening database");
        match migrations.to_latest(&mut conn) {
            Err(rusqlite_migration::Error::MigrationDefinition(
                rusqlite_migration::MigrationDefinitionError::DatabaseTooFarAhead,
            )) => return Err(DbError::TooNew),
            r => r?,
        }
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// In memory, at schema `version` (`None`: the latest).
    #[cfg(test)]
    fn memory(migrations: &Migrations<'static>, version: Option<usize>) -> Self {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        match version {
            Some(v) => migrations.to_version(&mut conn, v).unwrap(),
            None => migrations.to_latest(&mut conn).unwrap(),
        }
        Self { conn: Mutex::new(conn) }
    }

    fn read<T>(&self, f: impl FnOnce(&Connection) -> DbResult<T>) -> DbResult<T> {
        f(&self.conn.lock())
    }

    fn write<T>(&self, f: impl FnOnce(&Transaction) -> DbResult<T>) -> DbResult<T> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let out = f(&tx)?;
        tx.commit()?;
        Ok(out)
    }
}

/// Enums are stored as their serde (= TypeScript) names, so the database,
/// the IPC payloads and the UI all spell a state the same way.
fn enum_to_sql<T: Serialize>(v: T) -> String {
    match serde_json::to_value(v) {
        Ok(serde_json::Value::String(s)) => s,
        other => panic!("enum_to_sql on a non-unit enum: {other:?}"),
    }
}

fn enum_from_sql<T: DeserializeOwned>(s: &str) -> DbResult<T> {
    serde_json::from_value(serde_json::Value::String(s.to_owned()))
        .map_err(|_| DbError::Corrupt(format!("unknown value '{s}'")))
}

fn json_to_sql<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).expect("DTOs always serialise")
}

fn json_from_sql<T: DeserializeOwned>(s: &str) -> DbResult<T> {
    serde_json::from_str(s).map_err(|e| DbError::Corrupt(e.to_string()))
}

/// A text column that says why something failed: a [`Msg`] as JSON, or the
/// English an older AutoScene wrote there, which reads the same in every language.
fn msg_from_text(text: String) -> Msg {
    serde_json::from_str(&text).unwrap_or_else(|_| Msg::plain(text))
}

fn msg_from_sql(r: &rusqlite::Row, column: usize) -> rusqlite::Result<Option<Msg>> {
    Ok(r.get::<_, Option<String>>(column)?.map(msg_from_text))
}

#[cfg(test)]
mod error_tests {
    use rusqlite::ffi;

    use super::DbError;

    fn sqlite(code: i32) -> rusqlite::Error {
        rusqlite::Error::SqliteFailure(ffi::Error::new(code), None)
    }

    #[test]
    fn each_kind_of_database_failure_has_its_key() {
        let said = |code| {
            let m = DbError::Sqlite(sqlite(code)).user_message();
            (m.key, m.text)
        };
        let of = |key: &str, text: &str| (key.to_owned(), text.to_owned());
        assert_eq!(said(ffi::SQLITE_FULL), of("db.diskFull", "The disk is full."));
        assert_eq!(
            said(ffi::SQLITE_READONLY),
            of("db.readOnly", "AutoScene can't save its data there: the drive or folder is read-only.")
        );
        assert_eq!(
            said(ffi::SQLITE_CANTOPEN),
            of("db.cannotOpen", "AutoScene couldn't open its data. Check that the drive is connected.")
        );
        assert_eq!(said(ffi::SQLITE_PERM), of("db.permission", "AutoScene doesn't have permission to open its data."));
        for busy in [ffi::SQLITE_BUSY, ffi::SQLITE_LOCKED] {
            assert_eq!(said(busy), of("db.busy", "Another program is using AutoScene's data. Close it and try again."));
        }
        for damaged in [ffi::SQLITE_CORRUPT, ffi::SQLITE_NOTADB] {
            assert_eq!(said(damaged), of("db.damaged", "AutoScene's data file is damaged. Details are in the log."));
        }
        assert_eq!(
            said(ffi::SQLITE_IOERR),
            of("db.driveFailed", "The drive couldn't be read or written. Details are in the log.")
        );
        assert_eq!(said(ffi::SQLITE_ERROR).0, "db.generic");
        assert_eq!(DbError::Corrupt("row 7".into()).user_message().key, "db.generic");
        assert_eq!(DbError::Io("gone".into()).user_message().key, "db.generic");
        assert_eq!(
            DbError::TooNew.user_message().text,
            "This project was made by a newer version of AutoScene. Update AutoScene to open it."
        );
        assert_eq!(DbError::TooNew.user_message().key, "db.tooNew");
    }

    #[test]
    fn a_failed_upgrade_says_why_when_the_drive_knows() {
        let upgrade = |err| {
            let query = String::new();
            DbError::Migration(rusqlite_migration::Error::RusqliteError { query, err }).user_message().key
        };
        assert_eq!(upgrade(sqlite(ffi::SQLITE_FULL)), "db.diskFull");
        assert_eq!(upgrade(sqlite(ffi::SQLITE_ERROR)), "db.upgradeFailed");
        let other = rusqlite_migration::Error::MigrationDefinition(
            rusqlite_migration::MigrationDefinitionError::DatabaseTooFarAhead,
        );
        assert_eq!(DbError::Migration(other).user_message().key, "db.upgradeFailed");
    }
}

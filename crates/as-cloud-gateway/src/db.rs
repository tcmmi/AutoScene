//! The gateway's database (SQLite, WAL): accounts and their one device, the
//! money ledger, holds of running scans, sessions, codes, payments.
//!
//! The ledger only ever grows: a balance is the sum of its rows, and every
//! row says why (`topup`, `code`, `charge`, `refund`, `adjust`,
//! `transfer_in`, `transfer_out`). A scan's estimate is a hold, not a row:
//! what can be spent is the balance less the holds.

use std::path::Path;

use as_cloud::admin::{LedgerRow, SessionRow};
use as_cloud::limits::{day_start, month_start, next_month, week_start};
use as_cloud::{Limits, Micros, ScanKind, Switch, Usage};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use rusqlite_migration::{M, Migrations};

pub type DbResult<T> = Result<T, DbError>;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("database: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("database upgrade: {0}")]
    Migration(#[from] rusqlite_migration::Error),
    #[error("corrupt record: {0}")]
    Corrupt(String),
}

const SCHEMA: &str = "
CREATE TABLE accounts(
    id INTEGER PRIMARY KEY,
    support_id TEXT NOT NULL UNIQUE,
    created INTEGER NOT NULL,
    beta_until INTEGER,
    blocked INTEGER NOT NULL DEFAULT 0,
    block_reason TEXT
);
-- One device per account. `hwid`: the pepper's hash of the app's hash of
-- the computer; `token`: SHA-256 of the device's bearer token (none after a
-- reset, until the device registers again).
CREATE TABLE devices(
    account_id INTEGER PRIMARY KEY REFERENCES accounts(id),
    hwid TEXT NOT NULL UNIQUE,
    token TEXT UNIQUE,
    platform TEXT,
    app_version TEXT,
    created INTEGER NOT NULL,
    last_seen INTEGER
);
CREATE TABLE ledger(
    id INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES accounts(id),
    at INTEGER NOT NULL,
    kind TEXT NOT NULL,
    amount INTEGER NOT NULL,
    ref TEXT,
    note TEXT
);
CREATE INDEX ledger_account ON ledger(account_id);
CREATE INDEX ledger_kind_at ON ledger(kind, at);
-- A payment, a code, a scan and a refund count once, whatever is retried.
CREATE UNIQUE INDEX ledger_once ON ledger(kind, ref)
    WHERE ref IS NOT NULL AND kind IN ('topup', 'code', 'charge', 'refund');
CREATE TABLE holds(
    session TEXT PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES accounts(id),
    amount INTEGER NOT NULL,
    at INTEGER NOT NULL
);
CREATE TABLE sessions(
    id TEXT PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES accounts(id),
    at INTEGER NOT NULL,
    kind TEXT NOT NULL,
    size INTEGER NOT NULL,
    sha256 TEXT NOT NULL,
    beta INTEGER NOT NULL,
    duration_ms INTEGER,
    price INTEGER,
    charged INTEGER,
    state TEXT NOT NULL,
    gpu_secs REAL,
    -- The part of its price earned so far (a restart charges it).
    share REAL NOT NULL DEFAULT 0,
    closed_at INTEGER,
    error TEXT
);
CREATE INDEX sessions_account ON sessions(account_id, at);
CREATE INDEX sessions_at ON sessions(at);
CREATE TABLE codes(
    id INTEGER PRIMARY KEY,
    hash TEXT NOT NULL UNIQUE,
    credit INTEGER,
    beta_until INTEGER,
    expires INTEGER,
    note TEXT,
    created INTEGER NOT NULL,
    created_by TEXT,
    redeemed_by INTEGER REFERENCES accounts(id),
    redeemed_at INTEGER,
    revoked_at INTEGER
);
CREATE TABLE redeem_misses(account_id INTEGER, ip TEXT, at INTEGER NOT NULL);
CREATE INDEX redeem_misses_at ON redeem_misses(at);
CREATE TABLE payments(
    checkout TEXT PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES accounts(id),
    cents INTEGER NOT NULL,
    payment_intent TEXT,
    at INTEGER NOT NULL,
    refunded_cents INTEGER NOT NULL DEFAULT 0,
    disputed INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX payments_intent ON payments(payment_intent);
CREATE INDEX payments_at ON payments(at);
CREATE TABLE switches(key TEXT PRIMARY KEY, on_ INTEGER NOT NULL);
CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE audit(
    id INTEGER PRIMARY KEY,
    at INTEGER NOT NULL,
    actor TEXT,
    action TEXT NOT NULL,
    detail TEXT
);
";

/// Limits (`as_cloud::Limits` as JSON): an admin's on an account, a code's
/// (on the code, and on the account that redeemed it, until they end). A
/// session's worth (what it was charged, or would have been on a beta pass),
/// when it was priced (the period it counts in) and launched (a scan of the
/// day's).
const LIMITS: &str = "
ALTER TABLE accounts ADD COLUMN limits TEXT;
ALTER TABLE accounts ADD COLUMN code_limits TEXT;
ALTER TABLE accounts ADD COLUMN code_limits_until INTEGER;
ALTER TABLE codes ADD COLUMN limits TEXT;
ALTER TABLE sessions ADD COLUMN worth INTEGER;
ALTER TABLE sessions ADD COLUMN priced_at INTEGER;
ALTER TABLE sessions ADD COLUMN launched_at INTEGER;
";

fn migrations() -> Migrations<'static> {
    Migrations::new(vec![M::up(SCHEMA), M::up(LIMITS)])
}

fn limits_json(limits: Option<&Limits>) -> DbResult<Option<String>> {
    limits.filter(|l| !l.is_empty()).map(serde_json::to_string).transpose().map_err(|e| DbError::Corrupt(e.to_string()))
}

fn read_limits(json: Option<String>, col: usize) -> rusqlite::Result<Option<Limits>> {
    json.map(|j| {
        serde_json::from_str(&j)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(col, rusqlite::types::Type::Text, Box::new(e)))
    })
    .transpose()
}

#[derive(Clone, Debug, PartialEq)]
pub struct AccountRow {
    pub id: i64,
    pub support_id: String,
    pub created: i64,
    pub beta_until: Option<i64>,
    pub blocked: bool,
    pub block_reason: Option<String>,
    /// An admin's limits.
    pub admin_limits: Option<Limits>,
    /// The last code's limits, until `code_limits_until` (none: for good).
    pub code_limits: Option<Limits>,
    pub code_limits_until: Option<i64>,
}

impl AccountRow {
    /// The code's limits while they last.
    pub fn code_limits_at(&self, now: i64) -> Option<&Limits> {
        self.code_limits.as_ref().filter(|_| self.code_limits_until.is_none_or(|u| u > now))
    }

    /// The limits in force at `now`: the stricter of an admin's and a code's.
    pub fn limits_at(&self, now: i64) -> Option<Limits> {
        match (self.admin_limits.as_ref(), self.code_limits_at(now)) {
            (Some(a), Some(c)) => Some(a.stricter(c)),
            (a, c) => a.or(c).cloned(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Redeem {
    Credit(Micros),
    Beta(i64),
}

/// How pricing a session went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Priced {
    Yes,
    /// The balance can't pay it.
    Short,
    /// It was closed meanwhile.
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RedeemError {
    Invalid,
    Used,
}

pub struct Db {
    conn: Connection,
}

const ACCOUNT_COLS: &str = "a.id, a.support_id, a.created, a.beta_until, a.blocked, a.block_reason, a.limits, a.code_limits, a.code_limits_until";

fn account_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<AccountRow> {
    Ok(AccountRow {
        id: r.get(0)?,
        support_id: r.get(1)?,
        created: r.get(2)?,
        beta_until: r.get(3)?,
        blocked: r.get(4)?,
        block_reason: r.get(5)?,
        admin_limits: read_limits(r.get(6)?, 6)?,
        code_limits: read_limits(r.get(7)?, 7)?,
        code_limits_until: r.get(8)?,
    })
}

impl Db {
    pub fn open(path: &Path) -> DbResult<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::init(conn)
    }

    pub fn memory() -> DbResult<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> DbResult<Self> {
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
        migrations().to_latest(&mut conn)?;
        Ok(Self { conn })
    }

    /// Copy the whole database to `to` while it is in use (a backup).
    /// Copy the database at `from` to `to` while it is in use (a backup), on
    /// a connection of its own.
    pub fn backup_file(from: &Path, to: &Path) -> DbResult<()> {
        let _ = std::fs::remove_file(to);
        let conn = Connection::open_with_flags(from, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        conn.busy_timeout(std::time::Duration::from_secs(30))?;
        conn.execute("VACUUM INTO ?1", params![to.to_string_lossy()])?;
        Ok(())
    }

    // -- accounts -----------------------------------------------------------

    /// The account of the computer `hwid` (made on its first visit) with a new
    /// device token: a reinstall finds its balance again. The old token stops.
    pub fn register(
        &mut self,
        hwid: &str,
        token_hash: &str,
        mut support_id: impl FnMut() -> String,
        platform: &str,
        app_version: &str,
        now: i64,
    ) -> DbResult<(AccountRow, bool)> {
        let tx = self.conn.transaction()?;
        let known: Option<i64> =
            tx.query_row("SELECT account_id FROM devices WHERE hwid = ?1", [hwid], |r| r.get(0)).optional()?;
        let (id, created) = match known {
            Some(id) => {
                tx.execute(
                    "UPDATE devices SET token = ?2, platform = ?3, app_version = ?4, last_seen = ?5 WHERE account_id = ?1",
                    params![id, token_hash, platform, app_version, now],
                )?;
                (id, false)
            }
            None => {
                // Support ids are short: draw again on the rare clash.
                let mut id = None;
                for _ in 0..8 {
                    let sid = support_id();
                    if tx.execute(
                        "INSERT OR IGNORE INTO accounts(support_id, created) VALUES (?1, ?2)",
                        params![sid, now],
                    )? == 1
                    {
                        id = Some(tx.last_insert_rowid());
                        break;
                    }
                }
                let id = id.ok_or_else(|| DbError::Corrupt("no free support id".into()))?;
                tx.execute(
                    "INSERT INTO devices(account_id, hwid, token, platform, app_version, created, last_seen)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                    params![id, hwid, token_hash, platform, app_version, now],
                )?;
                (id, true)
            }
        };
        let row = tx.query_row(&format!("SELECT {ACCOUNT_COLS} FROM accounts a WHERE a.id = ?1"), [id], account_row)?;
        tx.commit()?;
        Ok((row, created))
    }

    pub fn account_by_token(&self, token_hash: &str) -> DbResult<Option<AccountRow>> {
        Ok(self
            .conn
            .query_row(
                &format!(
                    "SELECT {ACCOUNT_COLS} FROM accounts a JOIN devices d ON d.account_id = a.id WHERE d.token = ?1"
                ),
                [token_hash],
                account_row,
            )
            .optional()?)
    }

    pub fn account_by_support(&self, support_id: &str) -> DbResult<Option<AccountRow>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {ACCOUNT_COLS} FROM accounts a WHERE a.support_id = ?1"),
                [support_id],
                account_row,
            )
            .optional()?)
    }

    pub fn account(&self, id: i64) -> DbResult<AccountRow> {
        Ok(self.conn.query_row(&format!("SELECT {ACCOUNT_COLS} FROM accounts a WHERE a.id = ?1"), [id], account_row)?)
    }

    /// Note the device was seen (at most once a minute: every request asks).
    pub fn touch(&self, account: i64, now: i64) -> DbResult<()> {
        self.conn.execute(
            "UPDATE devices SET last_seen = ?2 WHERE account_id = ?1 AND (last_seen IS NULL OR last_seen < ?2 - 60)",
            params![account, now],
        )?;
        Ok(())
    }

    /// Money in, less money spent.
    pub fn balance(&self, account: i64) -> DbResult<Micros> {
        balance(&self.conn, account)
    }

    /// Held by scans still running.
    pub fn held(&self, account: i64) -> DbResult<Micros> {
        held(&self.conn, account)
    }

    // -- codes --------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub fn insert_code(
        &self,
        hash: &str,
        credit: Option<Micros>,
        beta_until: Option<i64>,
        expires: Option<i64>,
        limits: Option<&Limits>,
        note: Option<&str>,
        by: Option<&str>,
        now: i64,
    ) -> DbResult<()> {
        self.conn.execute(
            "INSERT INTO codes(hash, credit, beta_until, expires, limits, note, created, created_by) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![hash, credit, beta_until, expires, limits_json(limits)?, note, now, by],
        )?;
        Ok(())
    }

    /// Wrong codes tried in the last hour: by this account, and from this address.
    pub fn misses(&self, account: i64, ip: &str, now: i64) -> DbResult<(u32, u32)> {
        let since = now - 3600;
        let by_account = self.conn.query_row(
            "SELECT COUNT(*) FROM redeem_misses WHERE account_id = ?1 AND at > ?2",
            params![account, since],
            |r| r.get(0),
        )?;
        let by_ip = self.conn.query_row(
            "SELECT COUNT(*) FROM redeem_misses WHERE ip = ?1 AND at > ?2",
            params![ip, since],
            |r| r.get(0),
        )?;
        Ok((by_account, by_ip))
    }

    pub fn add_miss(&self, account: i64, ip: &str, now: i64) -> DbResult<()> {
        self.conn.execute("DELETE FROM redeem_misses WHERE at < ?1", [now - 86_400])?;
        self.conn
            .execute("INSERT INTO redeem_misses(account_id, ip, at) VALUES (?1, ?2, ?3)", params![account, ip, now])?;
        Ok(())
    }

    #[allow(clippy::type_complexity)]
    pub fn redeem(&mut self, account: i64, hash: &str, now: i64) -> DbResult<Result<Redeem, RedeemError>> {
        let tx = self.conn.transaction()?;
        let row: Option<(i64, Option<i64>, Option<i64>, Option<i64>, Option<i64>, Option<i64>, Option<String>)> = tx
            .query_row(
                "SELECT id, credit, beta_until, expires, redeemed_by, revoked_at, limits FROM codes WHERE hash = ?1",
                [hash],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
            )
            .optional()?;
        let Some((id, credit, beta, expires, redeemed_by, revoked, limits)) = row else {
            return Ok(Err(RedeemError::Invalid));
        };
        if redeemed_by.is_some() {
            return Ok(Err(RedeemError::Used));
        }
        if revoked.is_some() || expires.is_some_and(|e| e <= now) {
            return Ok(Err(RedeemError::Invalid));
        }
        tx.execute("UPDATE codes SET redeemed_by = ?2, redeemed_at = ?3 WHERE id = ?1", params![id, account, now])?;
        let got = match (credit, beta) {
            (Some(c), _) => {
                tx.execute(
                    "INSERT INTO ledger(account_id, at, kind, amount, ref) VALUES (?1, ?2, 'code', ?3, ?4)",
                    params![account, now, c, format!("code:{id}")],
                )?;
                Redeem::Credit(c)
            }
            (None, Some(until)) => {
                tx.execute(
                    "UPDATE accounts SET beta_until = MAX(COALESCE(beta_until, 0), ?2) WHERE id = ?1",
                    params![account, until],
                )?;
                Redeem::Beta(until)
            }
            (None, None) => return Err(DbError::Corrupt(format!("code {id} is worth nothing"))),
        };
        // A code's limits replace an earlier code's (an admin's stay); a beta
        // pass's end with it.
        if let Some(limits) = limits {
            let until = if credit.is_none() { beta } else { None };
            tx.execute(
                "UPDATE accounts SET code_limits = ?2, code_limits_until = ?3 WHERE id = ?1",
                params![account, limits, until],
            )?;
        }
        tx.commit()?;
        Ok(Ok(got))
    }

    /// The account a code was redeemed by, for proof in a transfer.
    pub fn code_redeemer(&self, hash: &str) -> DbResult<Option<i64>> {
        Ok(self
            .conn
            .query_row("SELECT redeemed_by FROM codes WHERE hash = ?1", [hash], |r| r.get(0))
            .optional()?
            .flatten())
    }

    /// Set an account's admin limits; none (or empty ones) lifts them. A
    /// code's limits stay.
    pub fn set_limits(&self, account: i64, limits: Option<&Limits>) -> DbResult<()> {
        self.conn.execute("UPDATE accounts SET limits = ?2 WHERE id = ?1", params![account, limits_json(limits)?])?;
        Ok(())
    }

    /// What an account's scans were worth this day, week and month (UTC),
    /// each in the period it was priced in, and how many were launched today.
    /// A scan is worth what it was charged, on a beta pass what it would have
    /// been (GPU-time floor included).
    pub fn usage(&self, account: i64, now: i64) -> DbResult<Usage> {
        let worth = |since: i64| -> DbResult<Micros> {
            Ok(self.conn.query_row(
                "SELECT COALESCE(SUM(COALESCE(worth, CASE WHEN beta
                     THEN CAST(COALESCE(price, 0) * MIN(MAX(share, 0), 1) AS INTEGER)
                     ELSE COALESCE(charged, 0) END)), 0)
                 FROM sessions WHERE account_id = ?1 AND COALESCE(priced_at, at) >= ?2
                     AND closed_at IS NOT NULL AND price IS NOT NULL",
                params![account, since],
                |r| r.get(0),
            )?)
        };
        let day = day_start(now);
        Ok(Usage {
            day: worth(day)?,
            week: worth(week_start(now))?,
            month: worth(month_start(now))?,
            scans_today: self.conn.query_row(
                "SELECT COUNT(*) FROM sessions WHERE account_id = ?1 AND launched_at >= ?2",
                params![account, day],
                |r| r.get(0),
            )?,
            day_ends: day + 86_400,
            week_ends: week_start(now) + 7 * 86_400,
            month_ends: next_month(now),
        })
    }

    pub fn revoke_code(&self, hash: &str, now: i64) -> DbResult<bool> {
        Ok(self.conn.execute(
            "UPDATE codes SET revoked_at = ?2 WHERE hash = ?1 AND redeemed_by IS NULL AND revoked_at IS NULL",
            params![hash, now],
        )? == 1)
    }

    // -- sessions -----------------------------------------------------------

    pub fn open_session_count(&self, account: i64) -> DbResult<u32> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE account_id = ?1 AND state IN ('uploading', 'running')",
            [account],
            |r| r.get(0),
        )?)
    }

    /// Open a session holding `hold` when the account can spend it (a beta
    /// session holds nothing). `Ok(false)`: the balance is short.
    #[allow(clippy::too_many_arguments)]
    pub fn open_session(
        &mut self,
        id: &str,
        account: i64,
        kind: &ScanKind,
        size: u64,
        sha256: &str,
        beta: bool,
        hold: Micros,
        now: i64,
    ) -> DbResult<bool> {
        let tx = self.conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if !beta && available(&tx, account)? < hold {
            return Ok(false);
        }
        let kind = serde_json::to_string(kind).map_err(|e| DbError::Corrupt(e.to_string()))?;
        tx.execute(
            "INSERT INTO sessions(id, account_id, at, kind, size, sha256, beta, state) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'uploading')",
            params![id, account, now, kind, size as i64, sha256, beta],
        )?;
        if !beta {
            tx.execute(
                "INSERT INTO holds(session, account_id, amount, at) VALUES (?1, ?2, ?3, ?4)",
                params![id, account, hold, now],
            )?;
        }
        tx.commit()?;
        Ok(true)
    }

    /// The file is in and measured: hold its real price. `Ok(false)`: the
    /// balance (with this session's estimate given back) can't pay it.
    pub fn price_session(
        &mut self,
        id: &str,
        duration_ms: u64,
        price: Micros,
        beta: bool,
        now: i64,
    ) -> DbResult<Priced> {
        let tx = self.conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let open: Option<i64> = tx
            .query_row("SELECT account_id FROM sessions WHERE id = ?1 AND closed_at IS NULL", [id], |r| r.get(0))
            .optional()?;
        let Some(account) = open else { return Ok(Priced::Closed) };
        if !beta {
            let own: Micros =
                tx.query_row("SELECT COALESCE(SUM(amount), 0) FROM holds WHERE session = ?1", [id], |r| r.get(0))?;
            if available(&tx, account)? + own < price {
                return Ok(Priced::Short);
            }
            tx.execute("UPDATE holds SET amount = ?2 WHERE session = ?1", params![id, price])?;
        }
        tx.execute(
            "UPDATE sessions SET duration_ms = ?2, price = ?3, priced_at = ?4, state = 'running' WHERE id = ?1 AND closed_at IS NULL",
            params![id, duration_ms as i64, price, now],
        )?;
        tx.commit()?;
        Ok(Priced::Yes)
    }

    /// End a session: charge `charge` (never more than its price), give the
    /// rest of the hold back. Settling twice charges once.
    pub fn settle(&mut self, id: &str, charge: Micros, state: &str, error: Option<&str>, now: i64) -> DbResult<Micros> {
        self.settle_worth(id, charge, charge, state, error, now)
    }

    /// [`Self::settle`], and what the session was worth against the account's
    /// limits: its charge, or on a beta pass `worth` (what it would have
    /// been charged).
    pub fn settle_worth(
        &mut self,
        id: &str,
        charge: Micros,
        worth: Micros,
        state: &str,
        error: Option<&str>,
        now: i64,
    ) -> DbResult<Micros> {
        let tx = self.conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let (account, price, beta, done): (i64, Option<Micros>, bool, Option<i64>) =
            tx.query_row("SELECT account_id, price, beta, closed_at FROM sessions WHERE id = ?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?;
        if done.is_some() {
            let charged: Option<Micros> =
                tx.query_row("SELECT charged FROM sessions WHERE id = ?1", [id], |r| r.get(0))?;
            return Ok(charged.unwrap_or(0));
        }
        let charge = if beta { 0 } else { charge.clamp(0, price.unwrap_or(0)) };
        if charge > 0 {
            tx.execute(
                "INSERT INTO ledger(account_id, at, kind, amount, ref) VALUES (?1, ?2, 'charge', ?3, ?4)",
                params![account, now, -charge, id],
            )?;
        }
        tx.execute("DELETE FROM holds WHERE session = ?1", [id])?;
        let worth = if beta { worth.clamp(0, price.unwrap_or(0)) } else { charge };
        tx.execute(
            "UPDATE sessions SET charged = ?2, state = ?3, error = ?4, closed_at = ?5, worth = ?6 WHERE id = ?1",
            params![id, charge, state, error, now, worth],
        )?;
        tx.commit()?;
        Ok(charge)
    }

    /// Its worker started: a scan of the day's.
    pub fn mark_launched(&self, id: &str, now: i64) -> DbResult<()> {
        self.conn.execute("UPDATE sessions SET launched_at = ?2 WHERE id = ?1", params![id, now])?;
        Ok(())
    }

    pub fn set_share(&self, id: &str, share: f64) -> DbResult<()> {
        self.conn.execute("UPDATE sessions SET share = ?2 WHERE id = ?1", params![id, share])?;
        Ok(())
    }

    pub fn add_gpu_secs(&self, id: &str, secs: f64) -> DbResult<()> {
        self.conn
            .execute("UPDATE sessions SET gpu_secs = COALESCE(gpu_secs, 0) + ?2 WHERE id = ?1", params![id, secs])?;
        Ok(())
    }

    /// Sessions a restart left open: closed, charged what they had done
    /// (`share`), the rest of their holds given back.
    pub fn fail_open_sessions(&mut self, now: i64) -> DbResult<Vec<String>> {
        let ids: Vec<String> = {
            let mut st = self.conn.prepare("SELECT id FROM sessions WHERE closed_at IS NULL")?;
            st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?
        };
        for id in &ids {
            // What it had done counts: a restart is no free scan.
            let (price, share): (Option<Micros>, f64) =
                self.conn.query_row("SELECT price, share FROM sessions WHERE id = ?1", [id], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?;
            let charge = (price.unwrap_or(0) as f64 * share.clamp(0.0, 1.0)).ceil() as Micros;
            self.settle_worth(id, charge, charge, "failed", Some("the gateway restarted"), now)?;
        }
        Ok(ids)
    }

    // -- payments -----------------------------------------------------------

    /// A paid checkout, once: its money goes on the balance.
    pub fn credit_checkout(
        &mut self,
        checkout: &str,
        account: i64,
        cents: i64,
        payment_intent: Option<&str>,
        now: i64,
    ) -> DbResult<bool> {
        let tx = self.conn.transaction()?;
        if tx.execute(
            "INSERT OR IGNORE INTO payments(checkout, account_id, cents, payment_intent, at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![checkout, account, cents, payment_intent, now],
        )? == 0
        {
            return Ok(false);
        }
        tx.execute(
            "INSERT INTO ledger(account_id, at, kind, amount, ref) VALUES (?1, ?2, 'topup', ?3, ?4)",
            params![account, now, cents * as_cloud::MICROS_PER_CENT, checkout],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// Stripe says `refunded_total` cents of a payment went back: take the
    /// part not taken yet off the balance (which may go below zero).
    pub fn refund(&mut self, payment_intent: &str, refunded_total: i64, now: i64) -> DbResult<Option<(i64, i64)>> {
        let tx = self.conn.transaction()?;
        let row: Option<(String, i64, i64)> = tx
            .query_row(
                "SELECT checkout, account_id, refunded_cents FROM payments WHERE payment_intent = ?1",
                [payment_intent],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((checkout, account, before)) = row else { return Ok(None) };
        let delta = refunded_total - before;
        if delta <= 0 {
            return Ok(Some((account, 0)));
        }
        tx.execute("UPDATE payments SET refunded_cents = ?2 WHERE checkout = ?1", params![checkout, refunded_total])?;
        tx.execute(
            "INSERT INTO ledger(account_id, at, kind, amount, ref) VALUES (?1, ?2, 'refund', ?3, ?4)",
            params![account, now, -delta * as_cloud::MICROS_PER_CENT, format!("{checkout}:{refunded_total}")],
        )?;
        tx.commit()?;
        Ok(Some((account, delta)))
    }

    /// A payment is disputed: its account is blocked until an admin decides.
    pub fn dispute(&mut self, payment_intent: &str) -> DbResult<Option<i64>> {
        let tx = self.conn.transaction()?;
        let account: Option<i64> = tx
            .query_row("SELECT account_id FROM payments WHERE payment_intent = ?1", [payment_intent], |r| r.get(0))
            .optional()?;
        if let Some(a) = account {
            tx.execute("UPDATE payments SET disputed = 1 WHERE payment_intent = ?1", [payment_intent])?;
            tx.execute("UPDATE accounts SET blocked = 1, block_reason = 'payment dispute' WHERE id = ?1", [a])?;
        }
        tx.commit()?;
        Ok(account)
    }

    // -- admin --------------------------------------------------------------

    pub fn adjust(&self, account: i64, amount: Micros, note: &str, now: i64) -> DbResult<()> {
        self.conn.execute(
            "INSERT INTO ledger(account_id, at, kind, amount, note) VALUES (?1, ?2, 'adjust', ?3, ?4)",
            params![account, now, amount, note],
        )?;
        Ok(())
    }

    pub fn block(&self, account: i64, blocked: bool, reason: Option<&str>) -> DbResult<()> {
        self.conn.execute(
            "UPDATE accounts SET blocked = ?2, block_reason = ?3 WHERE id = ?1",
            params![account, blocked, if blocked { reason } else { None }],
        )?;
        Ok(())
    }

    /// The device's token stops working; the computer's next visit gets a new one.
    pub fn reset_device(&self, account: i64) -> DbResult<bool> {
        Ok(self.conn.execute("UPDATE devices SET token = NULL WHERE account_id = ?1", [account])? == 1)
    }

    /// Move `from`'s whole balance and beta pass to `to`. `Ok(None)` while
    /// `from` has a scan running.
    pub fn transfer(&mut self, from: i64, to: i64, note: &str, now: i64) -> DbResult<Option<Micros>> {
        let tx = self.conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if held(&tx, from)? != 0 {
            return Ok(None);
        }
        let amount = balance(&tx, from)?;
        let reference = format!("{from}->{to}@{now}");
        if amount != 0 {
            tx.execute(
                "INSERT INTO ledger(account_id, at, kind, amount, ref, note) VALUES (?1, ?2, 'transfer_out', ?3, ?4, ?5)",
                params![from, now, -amount, reference, note],
            )?;
            tx.execute(
                "INSERT INTO ledger(account_id, at, kind, amount, ref, note) VALUES (?1, ?2, 'transfer_in', ?3, ?4, ?5)",
                params![to, now, amount, reference, note],
            )?;
        }
        tx.execute(
            "UPDATE accounts SET beta_until = MAX(COALESCE(beta_until, 0), COALESCE((SELECT beta_until FROM accounts WHERE id = ?1), 0))
             WHERE id = ?2",
            params![from, to],
        )?;
        tx.execute("UPDATE accounts SET beta_until = NULL WHERE id = ?1", [from])?;
        // The limits go along: on the new account, the stricter of both.
        let both = |col: &str| -> DbResult<()> {
            let read = |id: i64| -> DbResult<Option<Limits>> {
                let json: Option<String> =
                    tx.query_row(&format!("SELECT {col} FROM accounts WHERE id = ?1"), [id], |r| r.get(0))?;
                Ok(read_limits(json, 0)?)
            };
            let merged = match (read(from)?, read(to)?) {
                (Some(a), Some(b)) => Some(a.stricter(&b)),
                (a, b) => a.or(b),
            };
            tx.execute(
                &format!("UPDATE accounts SET {col} = ?2 WHERE id = ?1"),
                params![to, limits_json(merged.as_ref())?],
            )?;
            Ok(())
        };
        both("limits")?;
        both("code_limits")?;
        tx.execute(
            "UPDATE accounts SET code_limits_until = (SELECT code_limits_until FROM accounts WHERE id = ?1)
             WHERE id = ?2 AND (SELECT code_limits FROM accounts WHERE id = ?1) IS NOT NULL",
            params![from, to],
        )?;
        tx.execute(
            "UPDATE accounts SET limits = NULL, code_limits = NULL, code_limits_until = NULL WHERE id = ?1",
            [from],
        )?;
        tx.commit()?;
        Ok(Some(amount))
    }

    #[allow(clippy::type_complexity)]
    pub fn device(&self, account: i64) -> DbResult<Option<(Option<i64>, Option<String>, Option<String>, bool)>> {
        Ok(self
            .conn
            .query_row(
                "SELECT last_seen, platform, app_version, token IS NOT NULL FROM devices WHERE account_id = ?1",
                [account],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?)
    }

    pub fn ledger(&self, account: i64, limit: u32) -> DbResult<Vec<LedgerRow>> {
        let mut st = self.conn.prepare(
            "SELECT at, kind, amount, ref, note FROM ledger WHERE account_id = ?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = st.query_map(params![account, limit], |r| {
            Ok(LedgerRow { at: r.get(0)?, kind: r.get(1)?, amount: r.get(2)?, reference: r.get(3)?, note: r.get(4)? })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn sessions(&self, account: i64, limit: u32) -> DbResult<Vec<SessionRow>> {
        let mut st = self.conn.prepare(
            "SELECT id, at, kind, duration_ms, charged, state FROM sessions WHERE account_id = ?1 ORDER BY at DESC LIMIT ?2",
        )?;
        let rows = st.query_map(params![account, limit], |r| {
            let kind: String = r.get(2)?;
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, kind, r.get::<_, Option<i64>>(3)?, r.get(4)?, r.get(5)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, at, kind, duration, charged, state) = row?;
            let kind = serde_json::from_str(&kind).map_err(|e| DbError::Corrupt(e.to_string()))?;
            out.push(SessionRow { id, at, kind, duration_ms: duration.map(|d| d as u64), charged, state });
        }
        Ok(out)
    }

    pub fn switches(&self) -> DbResult<Vec<(Switch, bool)>> {
        let mut out = Vec::new();
        for s in Switch::ALL {
            let on: Option<bool> =
                self.conn.query_row("SELECT on_ FROM switches WHERE key = ?1", [s.key()], |r| r.get(0)).optional()?;
            out.push((s, on.unwrap_or(true)));
        }
        Ok(out)
    }

    pub fn set_switch(&self, s: Switch, on: bool) -> DbResult<()> {
        self.conn.execute(
            "INSERT INTO switches(key, on_) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET on_ = ?2",
            params![s.key(), on],
        )?;
        Ok(())
    }

    pub fn setting(&self, key: &str) -> DbResult<Option<String>> {
        Ok(self.conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> DbResult<()> {
        self.conn.execute(
            "INSERT INTO settings(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn audit(&self, actor: Option<&str>, action: &str, detail: &str, now: i64) -> DbResult<()> {
        self.conn.execute(
            "INSERT INTO audit(at, actor, action, detail) VALUES (?1, ?2, ?3, ?4)",
            params![now, actor, action, detail],
        )?;
        Ok(())
    }

    /// The month (from `since`): money paid in and refunded (cents), charged,
    /// sessions, and GPU seconds.
    pub fn month(&self, since: i64) -> DbResult<(i64, i64, Micros, u64, f64)> {
        let paid: i64 =
            self.conn
                .query_row("SELECT COALESCE(SUM(cents), 0) FROM payments WHERE at >= ?1", [since], |r| r.get(0))?;
        let refunded: i64 = self.conn.query_row(
            "SELECT COALESCE(-SUM(amount), 0) FROM ledger WHERE kind = 'refund' AND at >= ?1",
            [since],
            |r| r.get(0),
        )?;
        let charged: Micros = self.conn.query_row(
            "SELECT COALESCE(-SUM(amount), 0) FROM ledger WHERE kind = 'charge' AND at >= ?1",
            [since],
            |r| r.get(0),
        )?;
        let (sessions, gpu): (i64, f64) = self.conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(gpu_secs), 0.0) FROM sessions WHERE at >= ?1",
            [since],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok((paid, refunded / as_cloud::MICROS_PER_CENT, charged, sessions as u64, gpu))
    }

    pub fn account_count(&self) -> DbResult<u64> {
        Ok(self.conn.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get::<_, i64>(0))? as u64)
    }
}

fn balance(conn: &Connection, account: i64) -> DbResult<Micros> {
    Ok(conn.query_row("SELECT COALESCE(SUM(amount), 0) FROM ledger WHERE account_id = ?1", [account], |r| r.get(0))?)
}

fn held(conn: &Connection, account: i64) -> DbResult<Micros> {
    Ok(conn.query_row("SELECT COALESCE(SUM(amount), 0) FROM holds WHERE account_id = ?1", [account], |r| r.get(0))?)
}

fn available(tx: &Transaction<'_>, account: i64) -> DbResult<Micros> {
    Ok(balance(tx, account)? - held(tx, account)?)
}

#[cfg(test)]
mod tests {
    use as_cloud::{Depth, Footage};

    use super::*;

    const KIND: ScanKind = ScanKind { footage: Footage::Anime, depth: Depth::Standard, people: false, scenes: false };

    fn account(db: &mut Db) -> i64 {
        db.register("hw", "tok", || "AS-TEST-0001".into(), "windows", "1.0.6", 0).unwrap().0.id
    }

    #[test]
    fn the_schema_is_valid() {
        migrations().validate().unwrap();
    }

    #[test]
    fn the_same_computer_finds_its_account_again_with_a_new_token() {
        let mut db = Db::memory().unwrap();
        let (a, created) = db.register("hw", "t1", || "AS-AAAA-0001".into(), "windows", "1", 0).unwrap();
        assert!(created);
        let (b, created) = db.register("hw", "t2", || "AS-AAAA-0002".into(), "windows", "2", 5).unwrap();
        assert!(!created);
        assert_eq!(a.id, b.id);
        assert!(db.account_by_token("t1").unwrap().is_none());
        assert_eq!(db.account_by_token("t2").unwrap().unwrap().id, a.id);
        // A clash of support ids draws again.
        let mut n = 0;
        let (c, _) = db
            .register(
                "other",
                "t3",
                || {
                    n += 1;
                    if n == 1 { "AS-AAAA-0001".into() } else { "AS-BBBB-0001".into() }
                },
                "linux",
                "1",
                0,
            )
            .unwrap();
        assert_eq!(c.support_id, "AS-BBBB-0001");
    }

    #[test]
    fn a_scan_holds_then_charges_at_most_its_price() {
        let mut db = Db::memory().unwrap();
        let a = account(&mut db);
        db.adjust(a, 1_000_000, "test", 0).unwrap();
        assert!(!db.open_session("s0", a, &KIND, 1, "x", false, 2_000_000, 0).unwrap(), "more than the balance");
        assert!(db.open_session("s1", a, &KIND, 1, "x", false, 300_000, 0).unwrap());
        assert_eq!(db.held(a).unwrap(), 300_000);
        // The real price may be higher than the estimate if the balance pays it.
        assert_eq!(db.price_session("s1", 60_000, 900_000, false, 1).unwrap(), Priced::Yes);
        assert_eq!(db.price_session("s1", 60_000, 1_100_000, false, 1).unwrap(), Priced::Short);
        assert_eq!(db.held(a).unwrap(), 900_000);
        assert_eq!(db.settle("s1", 5_000_000, "closed", None, 1).unwrap(), 900_000, "capped at the price");
        assert_eq!(db.settle("s1", 1, "closed", None, 2).unwrap(), 900_000, "settled once");
        assert_eq!(
            db.price_session("s1", 60_000, 1, false, 1).unwrap(),
            Priced::Closed,
            "a closed session stays closed"
        );
        assert_eq!((db.balance(a).unwrap(), db.held(a).unwrap()), (100_000, 0));
    }

    #[test]
    fn a_beta_pass_scans_for_nothing() {
        let mut db = Db::memory().unwrap();
        let a = account(&mut db);
        assert!(db.open_session("s", a, &KIND, 1, "x", true, 0, 0).unwrap());
        assert_eq!(db.price_session("s", 60_000, 10_000, true, 1).unwrap(), Priced::Yes);
        assert_eq!(db.settle("s", 10_000, "closed", None, 1).unwrap(), 0);
        assert_eq!(db.balance(a).unwrap(), 0);
    }

    #[test]
    fn a_code_counts_once() {
        let mut db = Db::memory().unwrap();
        let a = account(&mut db);
        db.insert_code("h1", Some(500_000), None, None, None, None, None, 0).unwrap();
        db.insert_code("h2", None, Some(1_000), Some(50), None, None, None, 0).unwrap();
        db.insert_code("h3", Some(1), None, None, None, None, None, 0).unwrap();
        assert_eq!(db.redeem(a, "h1", 1).unwrap(), Ok(Redeem::Credit(500_000)));
        assert_eq!(db.redeem(a, "h1", 2).unwrap(), Err(RedeemError::Used));
        assert_eq!(db.redeem(a, "nope", 2).unwrap(), Err(RedeemError::Invalid));
        assert_eq!(db.redeem(a, "h2", 60).unwrap(), Err(RedeemError::Invalid), "expired");
        assert!(db.revoke_code("h3", 3).unwrap());
        assert_eq!(db.redeem(a, "h3", 4).unwrap(), Err(RedeemError::Invalid), "revoked");
        assert_eq!(db.balance(a).unwrap(), 500_000);
        assert_eq!(db.code_redeemer("h1").unwrap(), Some(a));
    }

    #[test]
    fn payments_count_once_and_refunds_take_only_the_new_part() {
        let mut db = Db::memory().unwrap();
        let a = account(&mut db);
        assert!(db.credit_checkout("cs_1", a, 1000, Some("pi_1"), 0).unwrap());
        assert!(!db.credit_checkout("cs_1", a, 1000, Some("pi_1"), 0).unwrap());
        assert_eq!(db.balance(a).unwrap(), 10_000_000);
        assert_eq!(db.refund("pi_1", 300, 1).unwrap(), Some((a, 300)));
        assert_eq!(db.refund("pi_1", 300, 2).unwrap(), Some((a, 0)));
        assert_eq!(db.refund("pi_1", 1000, 3).unwrap(), Some((a, 700)));
        assert_eq!(db.balance(a).unwrap(), 0);
        assert_eq!(db.dispute("pi_1").unwrap(), Some(a));
        assert!(db.account(a).unwrap().blocked);
        assert_eq!(db.refund("pi_unknown", 1, 0).unwrap(), None);
        let (paid, refunded, ..) = db.month(0).unwrap();
        assert_eq!((paid, refunded), (1000, 1000));
    }

    #[test]
    fn a_transfer_moves_the_exact_balance_but_not_mid_scan() {
        let mut db = Db::memory().unwrap();
        let a = account(&mut db);
        let (b, _) = db.register("hw2", "tok2", || "AS-TEST-0002".into(), "windows", "1", 0).unwrap();
        db.adjust(a, 1_234_567, "test", 0).unwrap();
        db.insert_code("beta", None, Some(9_999), None, None, None, None, 0).unwrap();
        db.redeem(a, "beta", 1).unwrap().unwrap();
        assert!(db.open_session("s", a, &KIND, 1, "x", false, 1, 0).unwrap());
        assert_eq!(db.transfer(a, b.id, "lost pc", 1).unwrap(), None);
        db.settle("s", 0, "failed", None, 1).unwrap();
        assert_eq!(db.transfer(a, b.id, "lost pc", 2).unwrap(), Some(1_234_567));
        assert_eq!((db.balance(a).unwrap(), db.balance(b.id).unwrap()), (0, 1_234_567));
        assert_eq!((db.account(a).unwrap().beta_until, db.account(b.id).unwrap().beta_until), (None, Some(9_999)));
    }

    #[test]
    fn a_codes_limits_go_to_its_account_and_usage_counts_by_period() {
        let mut db = Db::memory().unwrap();
        let a = account(&mut db);
        assert_eq!(db.account(a).unwrap().limits_at(0), None);
        // A pass's limits, until the pass ends at 1000.
        let limits = Limits { per_day: Some(2_000_000), scans_per_day: Some(3), ..Limits::default() };
        db.insert_code("pass", None, Some(1_000), None, Some(&limits), None, None, 0).unwrap();
        db.redeem(a, "pass", 1).unwrap().unwrap();
        assert_eq!(db.account(a).unwrap().limits_at(999), Some(limits.clone()));
        assert_eq!(db.account(a).unwrap().limits_at(1_000), None, "they end with the pass");
        // A code without limits leaves them; an admin's stricter ones count beside them.
        db.insert_code("cash", Some(1_000_000), None, None, None, None, None, 0).unwrap();
        db.redeem(a, "cash", 2).unwrap().unwrap();
        assert_eq!(db.account(a).unwrap().limits_at(3), Some(limits.clone()));
        let admin = Limits { per_day: Some(500_000), ..Limits::default() };
        db.set_limits(a, Some(&admin)).unwrap();
        assert_eq!(db.account(a).unwrap().limits_at(3), Some(Limits { per_day: Some(500_000), ..limits.clone() }));
        // A looser code doesn't lift the admin's.
        let loose = Limits { per_day: Some(9_000_000), ..Limits::default() };
        db.insert_code("loose", Some(1), None, None, Some(&loose), None, None, 0).unwrap();
        db.redeem(a, "loose", 4).unwrap().unwrap();
        let row = db.account(a).unwrap();
        assert_eq!((row.code_limits.clone(), row.code_limits_until), (Some(loose), None), "a credit code's last");
        assert_eq!(row.limits_at(5).and_then(|l| l.per_day), Some(500_000));
        db.set_limits(a, Some(&Limits::default())).unwrap();
        assert_eq!(db.account(a).unwrap().admin_limits, None, "empty ones lift an admin's");

        // 2026-11-19 12:00 UTC (a Thursday); the day before; the 2nd of the month.
        let t = 1_795_089_600;
        let run = |db: &mut Db, id: &str, at: i64, beta: bool, price: Micros, share: f64, charge: Micros| {
            assert!(db.open_session(id, a, &KIND, 1, "x", beta, 0, at).unwrap());
            assert_eq!(db.price_session(id, 60_000, price, beta, at).unwrap(), Priced::Yes);
            db.mark_launched(id, at).unwrap();
            db.set_share(id, share).unwrap();
            let worth = (price as f64 * share) as Micros;
            db.settle_worth(id, charge, worth, "closed", None, at + 60).unwrap();
        };
        run(&mut db, "today", t - 3600, false, 300_000, 1.0, 300_000);
        run(&mut db, "beta-half", t - 1800, true, 400_000, 0.5, 0);
        run(&mut db, "yesterday", t - 86_400, false, 100_000, 1.0, 100_000);
        run(&mut db, "this-month", 1_793_491_200 + 86_400, false, 50_000, 1.0, 50_000);
        // Never priced (a bad upload): no worth, no scan.
        assert!(db.open_session("dropped", a, &KIND, 1, "x", false, 0, t - 60).unwrap());
        db.settle("dropped", 0, "failed", None, t - 50).unwrap();
        // Priced, but no GPU could start: no scan of the day's.
        assert!(db.open_session("no-gpu", a, &KIND, 1, "x", true, 0, t - 40).unwrap());
        assert_eq!(db.price_session("no-gpu", 60_000, 10_000, true, t - 40).unwrap(), Priced::Yes);
        db.settle("no-gpu", 0, "failed", None, t - 30).unwrap();
        // Opened yesterday, priced today: it counts today.
        assert!(db.open_session("midnight", a, &KIND, 1, "x", true, 0, 1_795_046_400 - 60).unwrap());
        assert_eq!(db.price_session("midnight", 60_000, 30_000, true, 1_795_046_400 + 60).unwrap(), Priced::Yes);
        db.mark_launched("midnight", 1_795_046_400 + 60).unwrap();
        db.settle_worth("midnight", 0, 30_000, "closed", None, 1_795_046_400 + 120).unwrap();
        let u = db.usage(a, t).unwrap();
        assert_eq!((u.day, u.week, u.month, u.scans_today), (530_000, 630_000, 680_000, 3));
        assert_eq!((u.day_ends, u.week_ends, u.month_ends), (1_795_132_800, 1_795_392_000, 1_796_083_200));
    }
}

//! AutoScene Cloud's gateway, on the owner's server behind a TLS proxy.
//!
//! - **The app** registers its computer, sees its balance, redeems codes,
//!   opens Stripe's payment page, and runs scan sessions: it uploads a file,
//!   the gateway measures and prices it, a GPU worker scans it, and the
//!   engine's replies come back to the app line by line.
//! - **The worker** fetches the file, takes the app's engine requests, and
//!   hands back the engine's replies.
//! - **The Discord bot** (on this machine) makes codes, looks up accounts,
//!   moves balances, flips the per-kind switches.
//!
//! Money: a session holds its estimate, then its price once the file is
//! measured here (never from the app's numbers), and is charged on close:
//! all of it when the faces pass finished, the part it reached when the app
//! stopped early, nothing when the cloud failed.

pub mod config;
pub mod crypto;
pub mod db;
pub mod dispatch;
pub mod stripe;

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use as_cloud::admin::{self, AccountView, CodeWorth};
use as_cloud::engine::{self, Event, Op};
use as_cloud::{
    Account, ApiError, CheckoutReply, CheckoutRequest, CloseReply, ErrorCode, EventsReply, JobsRequest, Micros,
    ReadyReply, RedeemReply, RedeemRequest, RegisterReply, RegisterRequest, ScanKind, SessionReply, SessionRequest,
    TOP_UPS_CENTS, UploadState, WorkerDone, WorkerEvents, WorkerJobs, WorkerLaunch, path,
};
use axum::body::{Body, Bytes};
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::Notify;

use crate::config::Config;
use crate::db::{AccountRow, Db, DbError, Priced, Redeem, RedeemError};
use crate::dispatch::{Backend, Prober};
use crate::stripe::{Action, Payments};

/// Request bodies: small JSON by default; an upload chunk; jobs (a long
/// film's index is big); a worker's batch of replies (sprites).
const BODY_LIMIT: usize = 256 << 10;
const UPLOAD_LIMIT: usize = 16 << 20;
const JOBS_LIMIT: usize = 64 << 20;
const EVENTS_LIMIT: usize = 64 << 20;
/// An upload that gets no bytes for this long ends.
const UPLOAD_STALL: i64 = 30 * 60;
/// Measuring a file may take this long at most.
const MEASURE_LIMIT: i64 = 60 * 60;
/// Biggest piece of the file a worker gets per request.
const FILE_PIECE: u64 = 64 << 20;
/// Wrong codes allowed per hour, per account and per address.
const MISSES_PER_ACCOUNT: u32 = 5;
const MISSES_PER_IP: u32 = 20;
/// Registrations per hour from one address.
const REGISTRATIONS_PER_IP: u32 = 30;
/// What a session pays, of its price, when only its shots or scenes ran.
const SHOTS_SHARE: f64 = 0.1;
/// Jobs one session may send, and their bytes (a long film's index is big).
const MAX_JOB_LINES: usize = 64;
const MAX_JOB_BYTES: usize = 256 << 20;
/// Replies waiting for the app, per session: past this the worker waits.
const MAX_BUFFERED: usize = 512 << 20;
/// Sessions open at once from one address.
const SESSIONS_PER_IP: usize = 2;
/// The least a file's length is taken to be for the estimate: a second per
/// 2 MB (16 Mbit/s), so an empty balance can't park big files.
const MIN_BYTES_PER_SEC: u64 = 2_000_000;

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// The first second of `t`'s month, in UTC.
pub fn month_start(t: i64) -> i64 {
    // Howard Hinnant's days-from-civil and back.
    let days = t.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    (days - (d - 1)) * 86_400
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct Fail(ApiError);

fn fail(code: ErrorCode, message: impl Into<String>) -> Fail {
    Fail(ApiError::new(code, message))
}

impl From<DbError> for Fail {
    fn from(e: DbError) -> Self {
        tracing::error!("{e}");
        fail(ErrorCode::Server, "the gateway's database failed")
    }
}

impl IntoResponse for Fail {
    fn into_response(self) -> Response {
        let status = match self.0.code {
            ErrorCode::BadRequest | ErrorCode::CodeInvalid => StatusCode::BAD_REQUEST,
            ErrorCode::Unauthorized => StatusCode::UNAUTHORIZED,
            ErrorCode::Blocked | ErrorCode::KindOff => StatusCode::FORBIDDEN,
            ErrorCode::NotFound => StatusCode::NOT_FOUND,
            ErrorCode::Balance => StatusCode::PAYMENT_REQUIRED,
            ErrorCode::Paused => StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            ErrorCode::CodeUsed | ErrorCode::Busy => StatusCode::CONFLICT,
            ErrorCode::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            ErrorCode::Checksum | ErrorCode::Unreadable => StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::Gone => StatusCode::GONE,
            ErrorCode::Server => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(self.0)).into_response()
    }
}

type Result<T, E = Fail> = std::result::Result<T, E>;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// A session while it is open: its upload, the jobs on their way to the
/// worker, and the replies on their way to the app.
struct Live {
    id: String,
    account: i64,
    kind: ScanKind,
    beta: bool,
    size: u64,
    sha256: String,
    file: PathBuf,
    /// The address that opened it.
    ip: String,
    st: Mutex<LiveState>,
    /// Taken while the session is settled: the first closer charges, any
    /// other reads what it charged.
    settled: Mutex<Option<Micros>>,
    jobs_ready: Notify,
    events_ready: Notify,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Uploading,
    /// The file is in and being hashed and measured: one `ready` at a time.
    Measuring,
    Running,
    /// Settled: kept a while for the worker's last calls.
    Closed,
}

struct LiveState {
    phase: Phase,
    received: u64,
    app_seen: i64,
    worker_seen: i64,
    /// When the worker was launched (0: never).
    launched_at: i64,
    /// The session ends then, however busy: a scan can't hold a GPU forever.
    deadline: i64,
    /// SHA-256 of the worker's token, once launched.
    worker_token: Option<String>,
    /// The worker said how long it ran.
    worker_reported: bool,
    price: Micros,
    /// Every job line the app sent, in order; the worker reads on from its cursor.
    jobs: Vec<Value>,
    jobs_bytes: usize,
    /// Ids of the jobs sent (a cancel must name one).
    job_ids: HashSet<u64>,
    close: bool,
    /// Replies `base..` not read by the app yet, and their size.
    base: u64,
    lines: VecDeque<String>,
    buffered: usize,
    /// Replies the worker has handed in: its cursor.
    worker_lines: u64,
    ended: bool,
    error: Option<String>,
    faces_ids: HashSet<u64>,
    faces_fraction: f64,
    faces_done: bool,
    /// A shots or scenes job made progress.
    other_progress: bool,
    /// The share last written to the database (a restart charges it).
    saved_share: f64,
    closed_at: i64,
    /// Upload writes under way: `ready` waits for them.
    writing: u32,
    /// When the last upload bytes came (an upload that stalls expires).
    last_byte: i64,
    measuring_since: i64,
    /// Settled: the app gets the replies before this one, no later ones.
    cut: Option<u64>,
}

impl LiveState {
    fn new(t: i64) -> Self {
        Self {
            phase: Phase::Uploading,
            received: 0,
            app_seen: t,
            worker_seen: t,
            launched_at: 0,
            deadline: i64::MAX,
            worker_token: None,
            worker_reported: false,
            price: 0,
            jobs: Vec::new(),
            jobs_bytes: 0,
            job_ids: HashSet::new(),
            close: false,
            base: 0,
            lines: VecDeque::new(),
            buffered: 0,
            worker_lines: 0,
            ended: false,
            error: None,
            faces_ids: HashSet::new(),
            faces_fraction: 0.0,
            faces_done: false,
            other_progress: false,
            saved_share: 0.0,
            closed_at: 0,
            writing: 0,
            last_byte: t,
            measuring_since: 0,
            cut: None,
        }
    }

    /// The part of its price the session has earned: all of it once the
    /// faces pass is done, else how far that pass got (10% when only shots
    /// or scenes moved).
    fn share(&self) -> f64 {
        let share = if self.faces_done {
            1.0
        } else {
            self.faces_fraction.max(if self.other_progress { SHOTS_SHARE } else { 0.0 })
        };
        share.clamp(0.0, 1.0)
    }

    /// What the session costs if it ends now: what it did, whoever ended it
    /// (the app can make jobs fail, so a failure is never free), and never
    /// less than the GPU time it held (`floor`), never more than its price.
    fn charge(&self, floor: Micros) -> Micros {
        let earned = (self.price as f64 * self.share()).ceil() as Micros;
        earned.max(floor).min(self.price)
    }

    fn end(&mut self, error: Option<String>) {
        if !self.ended {
            self.ended = true;
            self.error = error;
        }
    }
}

pub struct Gateway {
    pub cfg: Config,
    db: Mutex<Db>,
    live: Mutex<HashMap<String, Arc<Live>>>,
    backend: Box<dyn Backend>,
    prober: Box<dyn Prober>,
    payments: Box<dyn Payments>,
    limits: Mutex<HashMap<String, (i64, u32)>>,
    uploads: PathBuf,
}

pub type Shared = Arc<Gateway>;

impl Gateway {
    pub fn new(
        cfg: Config,
        db: Db,
        backend: Box<dyn Backend>,
        prober: Box<dyn Prober>,
        payments: Box<dyn Payments>,
    ) -> std::io::Result<Shared> {
        let uploads = cfg.data_dir.join("uploads");
        std::fs::create_dir_all(&uploads)?;
        let gw = Gateway {
            cfg,
            db: Mutex::new(db),
            live: Mutex::new(HashMap::new()),
            backend,
            prober,
            payments,
            limits: Mutex::new(HashMap::new()),
            uploads,
        };
        // A restart forgets every open session: give their holds back and
        // their files away.
        let failed = gw.db.lock().fail_open_sessions(now()).map_err(std::io::Error::other)?;
        if !failed.is_empty() {
            tracing::warn!(
                "{} sessions were open when the gateway stopped; closed at what they had done",
                failed.len()
            );
        }
        if let Ok(dir) = std::fs::read_dir(&gw.uploads) {
            for f in dir.flatten() {
                let _ = std::fs::remove_file(f.path());
            }
        }
        Ok(Arc::new(gw))
    }

    /// One more call under `key` this hour, if under `max`.
    fn allow(&self, key: &str, max: u32) -> bool {
        let t = now();
        let mut l = self.limits.lock();
        if l.len() > 100_000 {
            l.retain(|_, (start, _)| t - *start < 3600);
        }
        let e = l.entry(key.to_string()).or_insert((t, 0));
        if t - e.0 >= 3600 {
            *e = (t, 0);
        }
        e.1 += 1;
        e.1 <= max
    }

    fn month_gpu_spend(&self, db: &Db) -> Result<Micros> {
        let (.., gpu) = db.month(month_start(now()))?;
        Ok((gpu * self.cfg.guard.gpu_per_hour as f64 / 3600.0).ceil() as Micros)
    }

    fn paused(&self, db: &Db) -> Result<bool> {
        Ok(db.setting("paused")?.as_deref() == Some("1") || self.month_gpu_spend(db)? >= self.cfg.guard.stop)
    }

    fn account_view(&self, db: &Db, a: &AccountRow) -> Result<Account> {
        let paused = self.paused(db)?;
        let open =
            if paused { vec![] } else { db.switches()?.into_iter().filter(|(_, on)| *on).map(|(s, _)| s).collect() };
        Ok(Account {
            support_id: a.support_id.clone(),
            balance: db.balance(a.id)? - db.held(a.id)?,
            beta_until: a.beta_until.filter(|&u| u > now()),
            blocked: a.blocked,
            prices: self.cfg.prices,
            open,
            paused,
            top_ups: TOP_UPS_CENTS.to_vec(),
        })
    }

    fn device(&self, headers: &HeaderMap) -> Result<AccountRow> {
        let token = bearer(headers).ok_or_else(|| fail(ErrorCode::Unauthorized, "no device token"))?;
        let db = self.db.lock();
        let a = db
            .account_by_token(&crypto::sha256_hex(token.as_bytes()))?
            .ok_or_else(|| fail(ErrorCode::Unauthorized, "unknown device token"))?;
        db.touch(a.id, now())?;
        Ok(a)
    }

    fn session(&self, a: &AccountRow, id: &str) -> Result<Arc<Live>> {
        let live = self.live.lock().get(id).cloned().filter(|l| l.account == a.id);
        live.ok_or_else(|| fail(ErrorCode::Gone, "no such open session"))
    }

    fn worker_session(&self, headers: &HeaderMap, id: &str) -> Result<Arc<Live>> {
        let live = self.live.lock().get(id).cloned().ok_or_else(|| fail(ErrorCode::Gone, "no such session"))?;
        let token = bearer(headers).map(|t| crypto::sha256_hex(t.as_bytes()));
        let mut st = live.st.lock();
        if token.is_none() || st.worker_token != token {
            return Err(fail(ErrorCode::Unauthorized, "not this session's worker"));
        }
        st.worker_seen = now();
        drop(st);
        Ok(live)
    }

    /// Settle a session once (what it did, at most its price), tell the
    /// worker to stop, and delete the file. Its entry stays for the worker's
    /// last calls until the janitor clears it.
    fn finish(&self, live: &Live, state: &str) -> Result<Micros> {
        let mut settled = live.settled.lock();
        if let Some(charged) = *settled {
            return Ok(charged);
        }
        let (charge, error) = {
            let mut st = live.st.lock();
            st.close = true;
            // The replies the charge saw are the ones the app gets.
            st.cut = Some(st.worker_lines);
            let floor = if st.launched_at > 0 {
                ((now() - st.launched_at).max(0) as f64 * self.cfg.guard.gpu_per_hour as f64 / 3600.0).ceil() as Micros
            } else {
                0
            };
            (st.charge(floor), st.error.clone())
        };
        let charged = self.db.lock().settle(&live.id, charge, state, error.as_deref(), now())?;
        *settled = Some(charged);
        {
            let mut st = live.st.lock();
            st.phase = Phase::Closed;
            st.closed_at = now();
        }
        drop(settled);
        live.jobs_ready.notify_waiters();
        live.events_ready.notify_waiters();
        let _ = std::fs::remove_file(&live.file);
        Ok(charged)
    }

    /// End a session that never ran (a bad file, a short balance, no GPU):
    /// nothing was done, nothing is charged.
    fn drop_session(&self, live: &Live, error: &str) {
        live.st.lock().end(Some(error.to_string()));
        if let Err(e) = self.finish(live, "failed") {
            tracing::error!("closing {}: {:?}", live.id, e);
        }
    }
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers.get(header::AUTHORIZATION)?.to_str().ok()?.strip_prefix("Bearer ").map(str::trim).filter(|t| !t.is_empty())
}

fn client_ip(gw: &Gateway, peer: SocketAddr, headers: &HeaderMap) -> String {
    if gw.cfg.trust_proxy
        && peer.ip().is_loopback()
        && let Some(ip) = headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.rsplit(',').next())
            .map(str::trim)
            .filter(|s| !s.is_empty())
    {
        return ip.to_string();
    }
    peer.ip().to_string()
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(f).await.map_err(|e| fail(ErrorCode::Server, e.to_string()))
}

// ---------------------------------------------------------------------------
// The app
// ---------------------------------------------------------------------------

async fn register(
    State(gw): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<RegisterReply>> {
    let ip = client_ip(&gw, peer, &headers);
    if !gw.allow(&format!("register:{ip}"), REGISTRATIONS_PER_IP) {
        return Err(fail(ErrorCode::RateLimited, "too many registrations from this address"));
    }
    if req.hwid.len() != 64 || !req.hwid.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(fail(ErrorCode::BadRequest, "hwid must be a SHA-256 in hex"));
    }
    let clip = |s: &str| s.chars().take(64).collect::<String>();
    let token = crypto::token();
    let hwid = crypto::keyed(&gw.cfg.pepper, &req.hwid.to_ascii_lowercase());
    let mut db = gw.db.lock();
    let (a, created) = db.register(
        &hwid,
        &crypto::sha256_hex(token.as_bytes()),
        crypto::support_id,
        &clip(&req.platform),
        &clip(&req.app_version),
        now(),
    )?;
    if created {
        tracing::info!("new account {}", a.support_id);
    }
    let account = gw.account_view(&db, &a)?;
    Ok(Json(RegisterReply { token, account }))
}

async fn account(State(gw): State<Shared>, headers: HeaderMap) -> Result<Json<Account>> {
    let a = gw.device(&headers)?;
    let db = gw.db.lock();
    Ok(Json(gw.account_view(&db, &a)?))
}

async fn redeem(
    State(gw): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<RedeemRequest>,
) -> Result<Json<RedeemReply>> {
    let a = gw.device(&headers)?;
    if a.blocked {
        return Err(fail(ErrorCode::Blocked, "this account is blocked"));
    }
    let ip = client_ip(&gw, peer, &headers);
    let t = now();
    let mut db = gw.db.lock();
    let (by_account, by_ip) = db.misses(a.id, &ip, t)?;
    if by_account >= MISSES_PER_ACCOUNT || by_ip >= MISSES_PER_IP {
        return Err(fail(ErrorCode::RateLimited, "too many wrong codes: try again in an hour"));
    }
    let Some(code) = as_cloud::code::normalize(&req.code) else {
        db.add_miss(a.id, &ip, t)?;
        return Err(fail(ErrorCode::CodeInvalid, "that isn't a code"));
    };
    let got = db.redeem(a.id, &crypto::keyed(&gw.cfg.pepper, &code), t)?;
    let added = match got {
        Ok(Redeem::Credit(c)) => c,
        Ok(Redeem::Beta(_)) => 0,
        Err(e) => {
            db.add_miss(a.id, &ip, t)?;
            return Err(match e {
                RedeemError::Used => fail(ErrorCode::CodeUsed, "this code was already used"),
                RedeemError::Invalid => fail(ErrorCode::CodeInvalid, "this code doesn't exist or has expired"),
            });
        }
    };
    db.audit(Some(&a.support_id), "redeem", &format!("added {added}"), t)?;
    let a = db.account(a.id)?;
    Ok(Json(RedeemReply { added, account: gw.account_view(&db, &a)? }))
}

async fn checkout(
    State(gw): State<Shared>,
    headers: HeaderMap,
    Json(req): Json<CheckoutRequest>,
) -> Result<Json<CheckoutReply>> {
    let a = gw.device(&headers)?;
    if a.blocked {
        return Err(fail(ErrorCode::Blocked, "this account is blocked"));
    }
    if !TOP_UPS_CENTS.contains(&req.cents) {
        return Err(fail(ErrorCode::BadRequest, "not one of the top-ups on offer"));
    }
    if !gw.allow(&format!("checkout:{}", a.id), 30) {
        return Err(fail(ErrorCode::RateLimited, "too many payment pages"));
    }
    let gw2 = gw.clone();
    let url = blocking(move || gw2.payments.checkout(a.id, &a.support_id, req.cents)).await?.map_err(|e| {
        tracing::error!("checkout: {e}");
        fail(ErrorCode::Server, "the payment page couldn't be made")
    })?;
    Ok(Json(CheckoutReply { url }))
}

async fn open_session(
    State(gw): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<SessionRequest>,
) -> Result<Json<SessionReply>> {
    let a = gw.device(&headers)?;
    if a.blocked {
        return Err(fail(ErrorCode::Blocked, "this account is blocked"));
    }
    req.kind.check().map_err(|e| fail(ErrorCode::BadRequest, e))?;
    if req.size == 0 || req.sha256.len() != 64 || !req.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(fail(ErrorCode::BadRequest, "size and sha256 are needed"));
    }
    if req.size > gw.cfg.max_upload_bytes {
        return Err(fail(ErrorCode::TooLarge, "this file is larger than the cloud takes"));
    }
    let ip = client_ip(&gw, peer, &headers);
    let t = now();
    let beta = a.beta_until.is_some_and(|u| u > t);
    // The estimate takes the app's length, but never less than the file's
    // size suggests: a claimed zero holds nothing.
    let floor_ms = req.size.saturating_mul(1000) / MIN_BYTES_PER_SEC;
    let estimate = gw.cfg.prices.price(&req.kind, req.duration_ms.max(floor_ms));
    let id = uuid::Uuid::new_v4().simple().to_string();
    let file = gw.uploads.join(format!("{id}.bin"));
    let live = Arc::new(Live {
        id: id.clone(),
        account: a.id,
        kind: req.kind,
        beta,
        size: req.size,
        sha256: req.sha256.to_ascii_lowercase(),
        file: file.clone(),
        ip: ip.clone(),
        st: Mutex::new(LiveState::new(t)),
        settled: Mutex::new(None),
        jobs_ready: Notify::new(),
        events_ready: Notify::new(),
    });
    // Checked and taken in one step: parallel opens can't pass the limits together.
    {
        let mut all = gw.live.lock();
        let open: Vec<&Arc<Live>> = all.values().filter(|l| l.st.lock().phase != Phase::Closed).collect();
        if open.iter().filter(|l| l.ip == ip).count() >= SESSIONS_PER_IP {
            return Err(fail(ErrorCode::Busy, "too many scans open from this address"));
        }
        if open.iter().any(|l| l.account == a.id) {
            return Err(fail(ErrorCode::Busy, "another scan of this account is still open"));
        }
        let in_use: u64 = open.iter().map(|l| l.size).sum();
        if in_use + req.size > gw.cfg.max_uploads_total {
            return Err(fail(ErrorCode::Busy, "the cloud is busy: try again in a few minutes"));
        }
        all.insert(id.clone(), live);
    }
    let opened = (|| -> Result<()> {
        std::fs::File::create(&file).map_err(|e| fail(ErrorCode::Server, e.to_string()))?;
        let mut db = gw.db.lock();
        if gw.paused(&db)? {
            return Err(fail(ErrorCode::Paused, "cloud scans are paused"));
        }
        let switches = db.switches()?;
        for need in req.kind.switches() {
            if !switches.iter().any(|(s, on)| *s == need && *on) {
                return Err(fail(ErrorCode::KindOff, format!("{} scans are off", need.key())));
            }
        }
        if db.open_session_count(a.id)? > 0 {
            return Err(fail(ErrorCode::Busy, "another scan of this account is still open"));
        }
        if !db.open_session(&id, a.id, &req.kind, req.size, &req.sha256.to_ascii_lowercase(), beta, estimate, t)? {
            return Err(fail(ErrorCode::Balance, "the balance doesn't cover this scan"));
        }
        Ok(())
    })();
    if let Err(e) = opened {
        gw.live.lock().remove(&id);
        let _ = std::fs::remove_file(&file);
        return Err(e);
    }
    Ok(Json(SessionReply { id, estimate: if beta { 0 } else { estimate } }))
}

async fn upload_state(
    State(gw): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<UploadState>> {
    let a = gw.device(&headers)?;
    let live = gw.session(&a, &id)?;
    let mut st = live.st.lock();
    st.app_seen = now();
    Ok(Json(UploadState { received: st.received, size: live.size }))
}

#[derive(Deserialize)]
struct Offset {
    offset: u64,
}

async fn upload(
    State(gw): State<Shared>,
    Path(id): Path<String>,
    Query(q): Query<Offset>,
    headers: HeaderMap,
    body: Body,
) -> Result<Json<UploadState>> {
    let a = gw.device(&headers)?;
    let live = gw.session(&a, &id)?;
    let body = read_body(body, UPLOAD_LIMIT).await?;
    let received = {
        let mut st = live.st.lock();
        st.app_seen = now();
        if st.phase != Phase::Uploading {
            return Err(fail(ErrorCode::BadRequest, "the upload is finished"));
        }
        // A chunk that isn't the next one changes nothing: the reply says
        // where to go on.
        if q.offset != st.received || body.is_empty() || st.writing > 0 {
            return Ok(Json(UploadState { received: st.received, size: live.size }));
        }
        st.writing += 1;
        st.received
    };
    let wrote = write_chunk(&live, received, body).await;
    let mut st = live.st.lock();
    st.writing -= 1;
    let len = wrote?;
    if st.phase == Phase::Uploading {
        st.received = len;
        st.last_byte = now();
    }
    Ok(Json(UploadState { received: st.received, size: live.size }))
}

/// Store the next chunk of `live`'s upload at `at`; the file's new length.
async fn write_chunk(live: &Live, at: u64, body: Bytes) -> Result<u64> {
    if at + body.len() as u64 > live.size {
        return Err(fail(ErrorCode::TooLarge, "more bytes than the session announced"));
    }
    let file = live.file.clone();
    blocking(move || -> std::io::Result<u64> {
        let mut f = std::fs::OpenOptions::new().write(true).open(&file)?;
        f.seek(SeekFrom::Start(at))?;
        f.write_all(&body)?;
        let end = at + body.len() as u64;
        f.set_len(end)?;
        Ok(end)
    })
    .await?
    .map_err(|e| fail(ErrorCode::Server, format!("storing the upload: {e}")))
}

/// A request body of at most `limit` bytes, read only once the caller is
/// known to be allowed.
async fn read_body(body: Body, limit: usize) -> Result<Bytes> {
    axum::body::to_bytes(body, limit).await.map_err(|_| fail(ErrorCode::TooLarge, "the request is too large"))
}

async fn ready(State(gw): State<Shared>, Path(id): Path<String>, headers: HeaderMap) -> Result<Json<ReadyReply>> {
    let a = gw.device(&headers)?;
    let live = gw.session(&a, &id)?;
    {
        let mut st = live.st.lock();
        if st.phase != Phase::Uploading || st.received != live.size || st.writing > 0 {
            return Err(fail(ErrorCode::BadRequest, "the upload isn't complete, or is already being measured"));
        }
        st.phase = Phase::Measuring;
        st.measuring_since = now();
    }
    let (file, gw2) = (live.file.clone(), gw.clone());
    let (sha, duration) = blocking(move || {
        let sha = (|| -> std::io::Result<String> {
            use sha2::Digest;
            let mut f = std::fs::File::open(&file)?;
            let mut h = sha2::Sha256::new();
            let mut buf = vec![0u8; 1 << 20];
            loop {
                let n = f.read(&mut buf)?;
                if n == 0 {
                    break Ok(crypto::hex(&h.finalize()));
                }
                h.update(&buf[..n]);
            }
        })();
        (sha, gw2.prober.duration_ms(&file))
    })
    .await?;
    let closed = || fail(ErrorCode::Gone, "the session was closed");
    if live.st.lock().phase != Phase::Measuring {
        return Err(closed());
    }
    let sha = match sha {
        Ok(s) => s,
        Err(e) => {
            gw.drop_session(&live, "the upload couldn't be read");
            return Err(fail(ErrorCode::Server, e.to_string()));
        }
    };
    if sha != live.sha256 {
        gw.drop_session(&live, "the uploaded file isn't the one announced");
        return Err(fail(ErrorCode::Checksum, "the uploaded file isn't the one announced"));
    }
    let duration = match duration {
        Ok(d) => d,
        Err(e) => {
            tracing::info!("{id}: unreadable upload: {e}");
            gw.drop_session(&live, "the file couldn't be read as video");
            return Err(fail(ErrorCode::Unreadable, "the file couldn't be read as video"));
        }
    };
    let price = gw.cfg.prices.price(&live.kind, duration);
    // The database lock must be gone before `drop_session` takes it again.
    let priced = gw.db.lock().price_session(&id, duration, price, live.beta)?;
    match priced {
        Priced::Yes => {}
        Priced::Short => {
            gw.drop_session(&live, "the balance doesn't cover this scan");
            return Err(fail(ErrorCode::Balance, "the balance doesn't cover this scan"));
        }
        Priced::Closed => return Err(closed()),
    }
    let token = crypto::token();
    {
        let mut st = live.st.lock();
        if st.phase != Phase::Measuring {
            return Err(closed());
        }
        let t = now();
        st.phase = Phase::Running;
        st.price = if live.beta { 0 } else { price };
        st.worker_token = Some(crypto::sha256_hex(token.as_bytes()));
        st.worker_seen = t;
        st.launched_at = t;
        // Room for a slow cold start and a slow GPU, never unbounded.
        st.deadline = t + 1800 + 4 * (duration / 1000) as i64;
    }
    let launch = WorkerLaunch {
        session: id.clone(),
        base: gw.cfg.public_base.trim_end_matches('/').to_string(),
        token,
        size: live.size,
        sha256: live.sha256.clone(),
        idle_secs: gw.cfg.worker_idle_secs,
    };
    let gw2 = gw.clone();
    if let Err(e) = blocking(move || gw2.backend.launch(&launch)).await? {
        tracing::error!("{id}: launching a worker: {e}");
        live.st.lock().launched_at = 0;
        gw.drop_session(&live, "no cloud GPU could be started");
        return Err(fail(ErrorCode::Server, "no cloud GPU could be started: nothing was charged"));
    }
    Ok(Json(ReadyReply { duration_ms: duration, price: if live.beta { 0 } else { price } }))
}

async fn jobs(State(gw): State<Shared>, Path(id): Path<String>, headers: HeaderMap, body: Body) -> Result<Json<Value>> {
    let a = gw.device(&headers)?;
    let live = gw.session(&a, &id)?;
    let body = read_body(body, JOBS_LIMIT).await?;
    let req: JobsRequest = serde_json::from_slice(&body).map_err(|e| fail(ErrorCode::BadRequest, e.to_string()))?;
    let mut st = live.st.lock();
    st.app_seen = now();
    if st.phase != Phase::Running || st.ended || st.close {
        return Err(fail(ErrorCode::Gone, "the session isn't running"));
    }
    if st.jobs.len() + req.lines.len() > MAX_JOB_LINES {
        return Err(fail(ErrorCode::RateLimited, "too many jobs for one session"));
    }
    let mut accepted = Vec::with_capacity(req.lines.len());
    let mut bytes = 0usize;
    let (mut ids, mut faces) = (Vec::new(), Vec::new());
    for line in req.lines {
        let has_path = line.get("path").is_some_and(Value::is_string);
        let (Some(op), Some(id)) = (engine::request_op(&line), engine::id(&line)) else {
            return Err(fail(ErrorCode::BadRequest, "a cloud session runs shots, faces and scenes jobs only"));
        };
        let known = st.job_ids.contains(&id) || ids.contains(&id);
        match op {
            // A cancel must name a job sent before.
            Op::Cancel if known => {}
            Op::Cancel => return Err(fail(ErrorCode::BadRequest, "no such job to cancel")),
            _ if known => return Err(fail(ErrorCode::BadRequest, "a job id was used before")),
            _ if !has_path => return Err(fail(ErrorCode::BadRequest, "a job names its file")),
            Op::Shots => ids.push(id),
            Op::Faces => {
                if !live.kind.admits(&engine::faces_flags(&line)) {
                    return Err(fail(ErrorCode::BadRequest, "this isn't the scan the session was opened for"));
                }
                ids.push(id);
                faces.push(id);
            }
            Op::Scenes if live.kind.scenes => ids.push(id),
            Op::Scenes => return Err(fail(ErrorCode::BadRequest, "this session has no scenes pass")),
        }
        bytes += line.to_string().len();
        accepted.push(line);
    }
    if st.jobs_bytes + bytes > MAX_JOB_BYTES {
        return Err(fail(ErrorCode::TooLarge, "too much job data for one session"));
    }
    st.jobs_bytes += bytes;
    st.job_ids.extend(ids);
    st.faces_ids.extend(faces);
    st.jobs.extend(accepted);
    drop(st);
    live.jobs_ready.notify_waiters();
    Ok(Json(serde_json::json!({})))
}

#[derive(Deserialize)]
struct EventsQuery {
    #[serde(default)]
    after: u64,
    #[serde(default)]
    wait: u64,
}

async fn events(
    State(gw): State<Shared>,
    Path(id): Path<String>,
    Query(q): Query<EventsQuery>,
    headers: HeaderMap,
) -> Result<Json<EventsReply>> {
    let a = gw.device(&headers)?;
    let live = gw.session(&a, &id)?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(q.wait.min(30));
    loop {
        let notified = live.events_ready.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        {
            let mut st = live.st.lock();
            st.app_seen = now();
            // Lines before `after` were read: let them go.
            while st.base < q.after {
                let Some(l) = st.lines.pop_front() else { break };
                st.buffered -= l.len();
                st.base += 1;
            }
            let from = q.after.max(st.base);
            let skip = (from - st.base) as usize;
            let over = st.ended || st.phase == Phase::Closed;
            // A settled session hands out what the charge saw, no more.
            let visible = match st.cut {
                Some(cut) => (cut.saturating_sub(st.base) as usize).min(st.lines.len()),
                None => st.lines.len(),
            };
            if skip < visible || over {
                let lines: Vec<String> = st.lines.iter().take(visible).skip(skip).take(4096).cloned().collect();
                let next = from + lines.len() as u64;
                let all_sent = next >= st.base + visible as u64;
                return Ok(Json(EventsReply {
                    lines,
                    next,
                    ended: over && all_sent,
                    error: if over && all_sent { st.error.clone() } else { None },
                }));
            }
        }
        if tokio::time::timeout_at(deadline, notified).await.is_err() {
            let st = live.st.lock();
            return Ok(Json(EventsReply { lines: vec![], next: q.after.max(st.base), ended: false, error: None }));
        }
    }
}

async fn close(State(gw): State<Shared>, Path(id): Path<String>, headers: HeaderMap) -> Result<Json<CloseReply>> {
    let a = gw.device(&headers)?;
    let live = gw.session(&a, &id)?;
    let charged = gw.finish(&live, "closed")?;
    let db = gw.db.lock();
    let a = db.account(a.id)?;
    Ok(Json(CloseReply { charged, account: gw.account_view(&db, &a)? }))
}

async fn stripe_webhook(State(gw): State<Shared>, headers: HeaderMap, body: Bytes) -> Result<StatusCode> {
    let signature = headers.get("stripe-signature").and_then(|v| v.to_str().ok()).unwrap_or("");
    let secret = &gw.cfg.stripe.webhook_secret;
    if secret.is_empty() || !crypto::stripe_signed(secret, signature, &body, now(), 300) {
        return Err(fail(ErrorCode::Unauthorized, "bad signature"));
    }
    let event: Value = serde_json::from_slice(&body).map_err(|e| fail(ErrorCode::BadRequest, e.to_string()))?;
    let t = now();
    let mut db = gw.db.lock();
    // Each action counts once by itself (a checkout once, a refund up to its
    // total), so a replayed event changes nothing.
    // A refund or dispute of a payment not credited yet: Stripe sends it
    // again later, after the payment's own event.
    let unknown = || fail(ErrorCode::NotFound, "a payment not seen yet: send it again later");
    let applied = (|| -> Result<()> {
        match stripe::action(&event) {
            Action::Credit { checkout, account, cents, payment_intent } => {
                if db.credit_checkout(&checkout, account, cents, payment_intent.as_deref(), t)? {
                    db.audit(None, "topup", &format!("account {account}: {cents} cents ({checkout})"), t)?;
                }
            }
            Action::Refunded { payment_intent, total_cents } => {
                let (account, delta) = db.refund(&payment_intent, total_cents, t)?.ok_or_else(unknown)?;
                if delta > 0 {
                    db.audit(None, "refund", &format!("account {account}: {delta} cents ({payment_intent})"), t)?;
                }
            }
            Action::Disputed { payment_intent } => {
                let account = db.dispute(&payment_intent)?.ok_or_else(unknown)?;
                db.audit(None, "dispute", &format!("account {account} blocked ({payment_intent})"), t)?;
            }
            Action::Ignore => {}
        }
        Ok(())
    })();
    // An error answer makes Stripe send it again.
    applied.map(|_| StatusCode::OK)
}

// ---------------------------------------------------------------------------
// The worker
// ---------------------------------------------------------------------------

async fn worker_file(State(gw): State<Shared>, Path(id): Path<String>, headers: HeaderMap) -> Result<Response> {
    let live = gw.worker_session(&headers, &id)?;
    let range = headers.get(header::RANGE).and_then(|v| v.to_str().ok()).unwrap_or("bytes=0-");
    let (start, end) = range
        .strip_prefix("bytes=")
        .and_then(|r| r.split_once('-'))
        .and_then(|(a, b)| {
            let a: u64 = a.parse().ok()?;
            let b: u64 = if b.is_empty() { live.size.saturating_sub(1) } else { b.parse().ok()? };
            Some((a, b.min(live.size.saturating_sub(1))))
        })
        .filter(|(a, b)| a <= b && *a < live.size)
        .ok_or_else(|| fail(ErrorCode::BadRequest, "bad range"))?;
    let end = end.min(start + FILE_PIECE - 1);
    let file = live.file.clone();
    let bytes = blocking(move || -> std::io::Result<Vec<u8>> {
        let mut f = std::fs::File::open(&file)?;
        f.seek(SeekFrom::Start(start))?;
        let mut buf = vec![0u8; (end - start + 1) as usize];
        f.read_exact(&mut buf)?;
        Ok(buf)
    })
    .await?
    .map_err(|_| fail(ErrorCode::Gone, "the file is gone"))?;
    Ok((
        StatusCode::PARTIAL_CONTENT,
        [
            (header::CONTENT_RANGE, format!("bytes {start}-{end}/{}", live.size)),
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
        ],
        bytes,
    )
        .into_response())
}

#[derive(Deserialize)]
struct JobsQuery {
    #[serde(default)]
    after: u64,
    #[serde(default)]
    wait: u64,
}

/// The app's jobs from the worker's cursor on: a lost answer loses nothing,
/// the worker asks from the same place again.
async fn worker_jobs(
    State(gw): State<Shared>,
    Path(id): Path<String>,
    Query(q): Query<JobsQuery>,
    headers: HeaderMap,
) -> Result<Json<WorkerJobs>> {
    let live = gw.worker_session(&headers, &id)?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(q.wait.min(30));
    loop {
        let notified = live.jobs_ready.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        {
            let st = live.st.lock();
            let from = (q.after as usize).min(st.jobs.len());
            if from < st.jobs.len() || st.close {
                return Ok(Json(WorkerJobs {
                    lines: st.jobs[from..].to_vec(),
                    next: st.jobs.len() as u64,
                    close: st.close,
                }));
            }
        }
        if tokio::time::timeout_at(deadline, notified).await.is_err() {
            return Ok(Json(WorkerJobs { lines: vec![], next: q.after, close: false }));
        }
    }
}

/// Replies from the worker, numbered from `first`: lines it sends again
/// after a lost answer are taken once.
async fn worker_events(
    State(gw): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Result<Json<Value>> {
    let live = gw.worker_session(&headers, &id)?;
    let body = read_body(body, EVENTS_LIMIT).await?;
    let ev: WorkerEvents = serde_json::from_slice(&body).map_err(|e| fail(ErrorCode::BadRequest, e.to_string()))?;
    let save = {
        let mut st = live.st.lock();
        if st.cut.is_some() {
            return Err(fail(ErrorCode::Gone, "the session is settled"));
        }
        if ev.first > st.worker_lines {
            return Err(fail(ErrorCode::BadRequest, format!("lines from {} were expected", st.worker_lines)));
        }
        let skip = (st.worker_lines - ev.first) as usize;
        for line in ev.lines.into_iter().skip(skip) {
            match engine::event(&line) {
                Some(Event::Progress { id, fraction }) if st.faces_ids.contains(&id) => {
                    st.faces_fraction = st.faces_fraction.max(fraction);
                }
                Some(Event::Done { id }) if st.faces_ids.contains(&id) => st.faces_done = true,
                Some(Event::Progress { id, fraction }) if fraction > 0.0 && st.job_ids.contains(&id) => {
                    st.other_progress = true;
                }
                Some(Event::Done { id }) if st.job_ids.contains(&id) => st.other_progress = true,
                _ => {}
            }
            st.worker_lines += 1;
            // An app that stops reading stops its scan: what was done still
            // counts, but the replies go no further.
            if st.buffered > MAX_BUFFERED {
                if !st.close {
                    st.end(Some("the app stopped reading the replies".into()));
                    st.close = true;
                }
                continue;
            }
            st.buffered += line.len();
            st.lines.push_back(line);
        }
        let share = st.share();
        (share >= st.saved_share + 0.01 || (share == 1.0 && st.saved_share < 1.0)).then(|| {
            st.saved_share = share;
            share
        })
    };
    if let Some(share) = save {
        // A restart charges what was done, not nothing.
        gw.db.lock().set_share(&id, share)?;
    }
    live.events_ready.notify_waiters();
    live.jobs_ready.notify_waiters();
    let next = live.st.lock().worker_lines;
    Ok(Json(serde_json::json!({ "next": next })))
}

async fn worker_done(
    State(gw): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Result<Json<Value>> {
    let live = gw.worker_session(&headers, &id)?;
    let done: WorkerDone = serde_json::from_slice(&read_body(body, BODY_LIMIT).await?)
        .map_err(|e| fail(ErrorCode::BadRequest, e.to_string()))?;
    let first = {
        let mut st = live.st.lock();
        st.end(done.error.clone().or_else(|| (!done.ok).then(|| "the cloud GPU failed".to_string())));
        !std::mem::replace(&mut st.worker_reported, true)
    };
    if first {
        gw.db.lock().add_gpu_secs(&id, done.gpu_secs.clamp(0.0, 86_400.0))?;
    }
    live.events_ready.notify_waiters();
    Ok(Json(serde_json::json!({})))
}

// ---------------------------------------------------------------------------
// Admin
// ---------------------------------------------------------------------------

fn admin_auth(gw: &Gateway, headers: &HeaderMap) -> Result<Option<String>> {
    let ok = bearer(headers).is_some_and(|t| {
        use hmac::Mac;
        // Compare in constant time.
        let key = crypto::random_bytes::<32>();
        let mac = |s: &str| {
            let mut m = <hmac::Hmac<sha2::Sha256> as hmac::KeyInit>::new_from_slice(&key).expect("key");
            m.update(s.as_bytes());
            m.finalize().into_bytes()
        };
        mac(t) == mac(&gw.cfg.admin_token)
    });
    if !ok {
        return Err(fail(ErrorCode::Unauthorized, "admin token needed"));
    }
    Ok(headers.get(admin::ACTOR).and_then(|v| v.to_str().ok()).map(|s| s.chars().take(100).collect()))
}

async fn admin_codes(
    State(gw): State<Shared>,
    headers: HeaderMap,
    Json(req): Json<admin::NewCodes>,
) -> Result<Json<admin::Codes>> {
    let actor = admin_auth(&gw, &headers)?;
    let t = now();
    if req.count == 0 || req.count > 500 {
        return Err(fail(ErrorCode::BadRequest, "between 1 and 500 codes"));
    }
    let (credit, beta) = match req.worth {
        CodeWorth::Credit { cents } if (1..=100_000).contains(&cents) => {
            (Some(cents * as_cloud::MICROS_PER_CENT), None)
        }
        CodeWorth::Beta { until } if until > t => (None, Some(until)),
        _ => {
            return Err(fail(ErrorCode::BadRequest, "a credit of €0.01 to €1000, or a beta pass ending in the future"));
        }
    };
    let db = gw.db.lock();
    let mut codes = Vec::new();
    for _ in 0..req.count {
        let code = crypto::new_code();
        let canon = as_cloud::code::normalize(&code).expect("a fresh code reads");
        db.insert_code(
            &crypto::keyed(&gw.cfg.pepper, &canon),
            credit,
            beta,
            req.expires,
            req.note.as_deref(),
            actor.as_deref(),
            t,
        )?;
        codes.push(code);
    }
    db.audit(actor.as_deref(), "codes", &format!("{} × {:?}", req.count, req.worth), t)?;
    Ok(Json(admin::Codes { codes }))
}

async fn admin_revoke(
    State(gw): State<Shared>,
    headers: HeaderMap,
    Json(req): Json<admin::Revoke>,
) -> Result<Json<admin::Revoked>> {
    let actor = admin_auth(&gw, &headers)?;
    let Some(canon) = as_cloud::code::normalize(&req.code) else { return Ok(Json(admin::Revoked { revoked: false })) };
    let db = gw.db.lock();
    let revoked = db.revoke_code(&crypto::keyed(&gw.cfg.pepper, &canon), now())?;
    db.audit(actor.as_deref(), "revoke", &format!("{revoked}"), now())?;
    Ok(Json(admin::Revoked { revoked }))
}

fn by_support(db: &Db, sid: &str) -> Result<AccountRow> {
    db.account_by_support(&sid.trim().to_ascii_uppercase())?.ok_or_else(|| fail(ErrorCode::NotFound, "no such account"))
}

async fn admin_account(
    State(gw): State<Shared>,
    Path(sid): Path<String>,
    headers: HeaderMap,
) -> Result<Json<AccountView>> {
    admin_auth(&gw, &headers)?;
    let db = gw.db.lock();
    let a = by_support(&db, &sid)?;
    let device = db.device(a.id)?;
    Ok(Json(AccountView {
        support_id: a.support_id.clone(),
        created: a.created,
        last_seen: device.as_ref().and_then(|d| d.0),
        platform: device.as_ref().and_then(|d| d.1.clone()),
        app_version: device.as_ref().and_then(|d| d.2.clone()),
        balance: db.balance(a.id)?,
        held: db.held(a.id)?,
        beta_until: a.beta_until,
        blocked: a.blocked,
        block_reason: a.block_reason.clone(),
        has_device: device.as_ref().is_some_and(|d| d.3),
        ledger: db.ledger(a.id, 25)?,
        sessions: db.sessions(a.id, 10)?,
    }))
}

async fn admin_adjust(
    State(gw): State<Shared>,
    Path(sid): Path<String>,
    headers: HeaderMap,
    Json(req): Json<admin::Adjust>,
) -> Result<Json<Value>> {
    let actor = admin_auth(&gw, &headers)?;
    if req.note.trim().is_empty() || req.amount == 0 {
        return Err(fail(ErrorCode::BadRequest, "an amount and a reason"));
    }
    let db = gw.db.lock();
    let a = by_support(&db, &sid)?;
    db.adjust(a.id, req.amount, &req.note, now())?;
    db.audit(actor.as_deref(), "adjust", &format!("{}: {} ({})", a.support_id, req.amount, req.note), now())?;
    Ok(Json(serde_json::json!({})))
}

async fn admin_block(
    State(gw): State<Shared>,
    Path(sid): Path<String>,
    headers: HeaderMap,
    Json(req): Json<admin::Block>,
) -> Result<Json<Value>> {
    let actor = admin_auth(&gw, &headers)?;
    let db = gw.db.lock();
    let a = by_support(&db, &sid)?;
    db.block(a.id, req.blocked, req.reason.as_deref())?;
    db.audit(actor.as_deref(), "block", &format!("{}: {} {:?}", a.support_id, req.blocked, req.reason), now())?;
    Ok(Json(serde_json::json!({})))
}

async fn admin_reset_device(
    State(gw): State<Shared>,
    Path(sid): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let actor = admin_auth(&gw, &headers)?;
    let db = gw.db.lock();
    let a = by_support(&db, &sid)?;
    let reset = db.reset_device(a.id)?;
    db.audit(actor.as_deref(), "reset_device", &a.support_id, now())?;
    Ok(Json(serde_json::json!({ "reset": reset })))
}

async fn admin_transfer(
    State(gw): State<Shared>,
    headers: HeaderMap,
    Json(req): Json<admin::Transfer>,
) -> Result<Json<admin::Transferred>> {
    let actor = admin_auth(&gw, &headers)?;
    if req.note.trim().is_empty() {
        return Err(fail(ErrorCode::BadRequest, "say what proves it (a receipt, a code)"));
    }
    let mut db = gw.db.lock();
    let (from, to) = (by_support(&db, &req.from)?, by_support(&db, &req.to)?);
    if from.id == to.id {
        return Err(fail(ErrorCode::BadRequest, "the same account"));
    }
    let moved = db
        .transfer(from.id, to.id, &req.note, now())?
        .ok_or_else(|| fail(ErrorCode::Busy, "a scan is running on it"))?;
    db.audit(
        actor.as_deref(),
        "transfer",
        &format!("{} -> {}: {moved} ({})", from.support_id, to.support_id, req.note),
        now(),
    )?;
    Ok(Json(admin::Transferred { moved }))
}

async fn admin_switches(State(gw): State<Shared>, headers: HeaderMap) -> Result<Json<admin::Switches>> {
    admin_auth(&gw, &headers)?;
    let all = gw.db.lock().switches()?;
    Ok(Json(admin::Switches {
        on: all.iter().filter(|(_, on)| *on).map(|(s, _)| *s).collect(),
        off: all.iter().filter(|(_, on)| !*on).map(|(s, _)| *s).collect(),
    }))
}

async fn admin_set_switch(
    State(gw): State<Shared>,
    headers: HeaderMap,
    Json(req): Json<admin::SetSwitch>,
) -> Result<Json<admin::Switches>> {
    let actor = admin_auth(&gw, &headers)?;
    {
        let db = gw.db.lock();
        db.set_switch(req.switch, req.on)?;
        db.audit(actor.as_deref(), "switch", &format!("{} {}", req.switch.key(), req.on), now())?;
    }
    admin_switches(State(gw), headers).await
}

async fn admin_stats(State(gw): State<Shared>, headers: HeaderMap) -> Result<Json<admin::Stats>> {
    admin_auth(&gw, &headers)?;
    let db = gw.db.lock();
    let (paid, refunded, charged, sessions, gpu) = db.month(month_start(now()))?;
    let open = gw.live.lock().values().filter(|l| l.st.lock().phase != Phase::Closed).count() as u64;
    Ok(Json(admin::Stats {
        accounts: db.account_count()?,
        paid_in_cents: paid,
        refunded_cents: refunded,
        charged,
        sessions,
        gpu_secs: gpu,
        gpu_spend: gw.month_gpu_spend(&db)?,
        guard_alert: gw.cfg.guard.alert,
        guard_stop: gw.cfg.guard.stop,
        paused: gw.paused(&db)?,
        open_sessions: open,
    }))
}

// ---------------------------------------------------------------------------
// Routers, upkeep, serving
// ---------------------------------------------------------------------------

/// The app's, Stripe's and the worker's routes (public, behind the proxy).
pub fn public_router(gw: Shared) -> Router {
    Router::new()
        .route(path::DEVICES, post(register))
        .route(path::ACCOUNT, get(account))
        .route(path::REDEEM, post(redeem))
        .route(path::CHECKOUT, post(checkout))
        .route(path::SESSIONS, post(open_session))
        .route("/v1/sessions/{id}/upload", get(upload_state).put(upload))
        .route("/v1/sessions/{id}/ready", post(ready))
        .route("/v1/sessions/{id}/jobs", post(jobs))
        .route("/v1/sessions/{id}/events", get(events))
        .route("/v1/sessions/{id}/close", post(close))
        .route(path::STRIPE_WEBHOOK, post(stripe_webhook))
        .route("/v1/worker/{id}/file", get(worker_file))
        .route("/v1/worker/{id}/jobs", get(worker_jobs))
        .route("/v1/worker/{id}/events", post(worker_events))
        .route("/v1/worker/{id}/done", post(worker_done))
        .route("/health", get(|| async { "ok" }))
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .with_state(gw)
}

/// The Discord bot's routes: serve them on 127.0.0.1 only.
pub fn admin_router(gw: Shared) -> Router {
    Router::new()
        .route(admin::CODES, post(admin_codes))
        .route(admin::REVOKE, post(admin_revoke))
        .route("/admin/accounts/{sid}", get(admin_account))
        .route("/admin/accounts/{sid}/adjust", post(admin_adjust))
        .route("/admin/accounts/{sid}/block", post(admin_block))
        .route("/admin/accounts/{sid}/reset_device", post(admin_reset_device))
        .route(admin::TRANSFER, post(admin_transfer))
        .route(admin::SWITCHES, get(admin_switches).post(admin_set_switch))
        .route(admin::STATS, get(admin_stats))
        .with_state(gw)
}

/// One pass of upkeep: expire forgotten sessions, end overlong ones, notice
/// dead workers, count GPU time nobody reported, warn about the GPU budget,
/// keep a daily backup.
pub fn upkeep(gw: &Gateway) {
    let t = now();
    let all: Vec<Arc<Live>> = gw.live.lock().values().cloned().collect();
    let cfg = &gw.cfg;
    for live in all {
        let (phase, app_seen, worker_seen, launched_at, deadline, ended, closed_at, reported, last_byte, measuring) = {
            let st = live.st.lock();
            (
                st.phase,
                st.app_seen,
                st.worker_seen,
                st.launched_at,
                st.deadline,
                st.ended,
                st.closed_at,
                st.worker_reported,
                st.last_byte,
                st.measuring_since,
            )
        };
        match phase {
            // Asking how far an upload is doesn't keep it alive: bytes do.
            Phase::Uploading if t - last_byte > UPLOAD_STALL || t - app_seen > cfg.upload_ttl_secs as i64 => {
                let _ = gw.finish(&live, "expired");
            }
            Phase::Measuring if t - measuring > MEASURE_LIMIT => {
                gw.drop_session(&live, "measuring the file took too long");
            }
            Phase::Running if t > deadline => {
                live.st.lock().end(Some("the scan ran too long".into()));
                let _ = gw.finish(&live, "closed");
            }
            Phase::Running if t - app_seen > cfg.app_idle_secs as i64 => {
                // The app went away: priced for what was done.
                let _ = gw.finish(&live, "closed");
            }
            Phase::Running if launched_at > 0 && !ended && t - worker_seen > cfg.worker_timeout_secs as i64 => {
                live.st.lock().end(Some("the cloud GPU stopped answering".into()));
                live.events_ready.notify_waiters();
            }
            Phase::Closed => {
                let waited = t - closed_at > cfg.worker_timeout_secs as i64 + 60;
                if reported || launched_at == 0 || waited {
                    gw.live.lock().remove(&live.id);
                    // A worker that never said how long it ran: count it
                    // from its launch to its last call, for the budget.
                    if launched_at > 0 && !reported {
                        let secs = (worker_seen.max(launched_at) - launched_at) as f64;
                        let _ = gw.db.lock().add_gpu_secs(&live.id, secs);
                    }
                }
            }
            _ => {}
        }
    }
    // The GPU budget.
    {
        let db = gw.db.lock();
        if let Ok(spend) = gw.month_gpu_spend(&db) {
            let month = month_start(t).to_string();
            if spend >= cfg.guard.alert && db.setting("alerted").ok().flatten().as_deref() != Some(month.as_str()) {
                let _ = db.set_setting("alerted", &month);
                if let Some(hook) = cfg.guard.alert_webhook.clone() {
                    let text = format!(
                        "AutoScene Cloud: this month's GPU spend is about {} (alert at {}, stop at {}).",
                        as_cloud::euros(spend),
                        as_cloud::euros(cfg.guard.alert),
                        as_cloud::euros(cfg.guard.stop)
                    );
                    std::thread::spawn(move || {
                        let body = serde_json::json!({ "content": text }).to_string();
                        let _ = ureq::post(&hook).header("Content-Type", "application/json").send(body.as_bytes());
                    });
                }
            }
        }
    }
    // A backup a day, the last fortnight kept, made on a connection of its
    // own: requests go on meanwhile.
    let dir = cfg.data_dir.join("backups");
    let file = dir.join(format!("gateway-{}.db", t.div_euclid(86_400)));
    let source = cfg.data_dir.join("gateway.db");
    if source.exists() && !file.exists() && std::fs::create_dir_all(&dir).is_ok() {
        if let Err(e) = Db::backup_file(&source, &file) {
            tracing::error!("backup: {e}");
        }
        if let Ok(rd) = std::fs::read_dir(&dir) {
            let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
            files.sort();
            for old in files.iter().rev().skip(14) {
                let _ = std::fs::remove_file(old);
            }
        }
    }
}

/// Serve on the configured addresses until Ctrl-C.
pub async fn serve(gw: Shared) -> std::io::Result<()> {
    let public = tokio::net::TcpListener::bind(gw.cfg.listen).await?;
    let admin = tokio::net::TcpListener::bind(gw.cfg.admin_listen).await?;
    serve_on(gw, public, admin, async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await
}

/// Serve on listeners already bound, until `stop`.
pub async fn serve_on(
    gw: Shared,
    public: tokio::net::TcpListener,
    admin: tokio::net::TcpListener,
    stop: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    tracing::info!("listening on {:?} (admin {:?})", public.local_addr(), admin.local_addr());
    let keeper = gw.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            let gw = keeper.clone();
            let _ = tokio::task::spawn_blocking(move || upkeep(&gw)).await;
        }
    });
    let admin_app = admin_router(gw.clone());
    tokio::spawn(async move {
        if let Err(e) = axum::serve(admin, admin_app).await {
            tracing::error!("admin: {e}");
        }
    });
    axum::serve(public, public_router(gw).into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(stop)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn months_start_on_the_first_at_midnight_utc() {
        // 2026-11-19 12:00 UTC.
        assert_eq!(month_start(1_795_089_600), 1_793_491_200);
        assert_eq!(month_start(1_793_491_200), 1_793_491_200);
        // 2024-02-29 is in February.
        assert_eq!(month_start(1_709_208_000), 1_706_745_600);
    }
}

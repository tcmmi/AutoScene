//! What travels between the app, the gateway, the GPU worker and the
//! Discord bot (JSON over HTTPS).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Micros, PriceBasis, Prices, ScanKind, Switch};

/// The gateway's addresses, relative to its base URL.
pub mod path {
    /// How far the cloud is open ([`super::Info`]): asked before an account exists.
    pub const INFO: &str = "/v1/info";
    pub const DEVICES: &str = "/v1/devices";
    pub const ACCOUNT: &str = "/v1/account";
    pub const REDEEM: &str = "/v1/redeem";
    pub const CHECKOUT: &str = "/v1/checkout";
    pub const SESSIONS: &str = "/v1/sessions";
    pub const STRIPE_WEBHOOK: &str = "/v1/stripe/webhook";

    pub fn session(id: &str) -> String {
        format!("{SESSIONS}/{id}")
    }
    pub fn upload(id: &str) -> String {
        format!("{SESSIONS}/{id}/upload")
    }
    pub fn ready(id: &str) -> String {
        format!("{SESSIONS}/{id}/ready")
    }
    pub fn jobs(id: &str) -> String {
        format!("{SESSIONS}/{id}/jobs")
    }
    pub fn events(id: &str) -> String {
        format!("{SESSIONS}/{id}/events")
    }
    pub fn close(id: &str) -> String {
        format!("{SESSIONS}/{id}/close")
    }

    /// The worker's side of a session.
    pub fn worker_file(id: &str) -> String {
        format!("/v1/worker/{id}/file")
    }
    pub fn worker_jobs(id: &str) -> String {
        format!("/v1/worker/{id}/jobs")
    }
    pub fn worker_events(id: &str) -> String {
        format!("/v1/worker/{id}/events")
    }
    pub fn worker_done(id: &str) -> String {
        format!("/v1/worker/{id}/done")
    }
}

/// Why the gateway said no.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    BadRequest,
    /// No device token, or one the gateway doesn't know.
    Unauthorized,
    /// The account is blocked (a payment dispute, an admin).
    Blocked,
    NotFound,
    /// The balance doesn't cover the scan.
    Balance,
    /// The owner turned this kind of scan off.
    KindOff,
    /// Cloud scans are paused (the month's GPU budget is spent, or the owner).
    Paused,
    /// AutoScene Cloud isn't open for this ([`Stage`]): before the launch it
    /// takes no new computer, code, payment or scan; in the beta, no payment.
    Closed,
    RateLimited,
    /// A code that doesn't exist, was revoked or has expired.
    CodeInvalid,
    CodeUsed,
    TooLarge,
    /// The uploaded file isn't the one announced.
    Checksum,
    /// The file couldn't be read as video.
    Unreadable,
    /// Another scan session of this account is open.
    Busy,
    /// The session is over (closed, expired, or its worker failed).
    Gone,
    Server,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    pub code: ErrorCode,
    /// English, for logs and as a fallback; the app words each code itself.
    pub message: String,
}

impl ApiError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for ApiError {}

// ---------------------------------------------------------------------------
// The app
// ---------------------------------------------------------------------------

/// How far AutoScene Cloud is open. The owner moves it from Discord; it only
/// goes forward in normal use (pausing scans is [`Account::paused`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// Not launched: the app says "not available yet", and the gateway takes
    /// no new computer, code, payment or scan.
    #[default]
    Hidden,
    /// A closed beta: codes (beta passes, balance an admin gave) and scans,
    /// but no payments.
    Beta,
    /// Open to everyone: payments too.
    Open,
}

impl Stage {
    pub const ALL: [Stage; 3] = [Stage::Hidden, Stage::Beta, Stage::Open];

    /// The name stored and sent: `hidden`, `beta`, `open`.
    pub fn key(self) -> &'static str {
        match self {
            Stage::Hidden => "hidden",
            Stage::Beta => "beta",
            Stage::Open => "open",
        }
    }

    pub fn from_key(key: &str) -> Option<Stage> {
        Stage::ALL.into_iter().find(|s| s.key() == key)
    }
}

/// What anyone may ask before having an account.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Info {
    pub stage: Stage,
    /// What a minute of video costs now (absent from older gateways).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prices: Option<Prices>,
    /// What those prices follow, when they follow the GPU service's rates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_basis: Option<PriceBasis>,
}

/// A device asks for its account. The same hardware gets the same account
/// back (a reinstall keeps the balance) with a new token.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegisterRequest {
    /// The app's salted hash of this computer's hardware ids (hex): the raw
    /// ids never leave the computer.
    pub hwid: String,
    pub platform: String,
    pub app_version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegisterReply {
    /// The device's secret, sent as `Authorization: Bearer`; the gateway
    /// keeps only its hash.
    pub token: String,
    pub account: Account,
}

/// An account as the app shows it. People see a balance, never a key.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Account {
    /// Short and not secret: what support asks for.
    pub support_id: String,
    /// What can be spent now: the money in, less scans still running.
    pub balance: Micros,
    /// Scans are free and unlimited until then (unix seconds): a beta pass.
    pub beta_until: Option<i64>,
    pub blocked: bool,
    pub prices: Prices,
    /// The kinds of scan open now.
    pub open: Vec<Switch>,
    /// Cloud scans are paused for everyone.
    pub paused: bool,
    /// The top-ups on offer, in cents.
    pub top_ups: Vec<i64>,
    /// How far the cloud is open: payments only when it's [`Stage::Open`].
    /// A gateway that doesn't say counts as not open yet.
    #[serde(default)]
    pub stage: Stage,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RedeemRequest {
    pub code: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RedeemReply {
    /// Money the code added (0 for a beta pass).
    pub added: Micros,
    pub account: Account,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CheckoutRequest {
    /// One of the offered top-ups.
    pub cents: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CheckoutReply {
    /// Stripe's payment page, to open in the browser.
    pub url: String,
}

/// A scan session opens for one file and one kind of scan.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionRequest {
    pub kind: ScanKind,
    /// The file's size in bytes.
    pub size: u64,
    /// The file's SHA-256 (hex): the worker must scan exactly this file.
    pub sha256: String,
    /// Its length as the app measured it, for the estimate only: the price
    /// comes from the gateway's own measure.
    pub duration_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionReply {
    pub id: String,
    /// Held from the balance until the scan is priced (nothing on a beta pass).
    pub estimate: Micros,
}

/// How much of a session's file the gateway has.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UploadState {
    pub received: u64,
    pub size: u64,
}

/// The file is in: the gateway measured it and priced the scan.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReadyReply {
    pub duration_ms: u64,
    pub price: Micros,
}

/// Engine requests (`as_engine::protocol::Request`, one JSON object each)
/// for the session's worker. Their `path` is ignored: the worker scans the
/// uploaded file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JobsRequest {
    pub lines: Vec<Value>,
}

/// Engine replies since `after`, as the engine wrote them.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventsReply {
    pub lines: Vec<String>,
    /// Ask with `after=next` for the lines that follow.
    pub next: u64,
    /// No more lines will come: the worker has ended.
    pub ended: bool,
    /// Why it ended, when it ended badly.
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CloseReply {
    /// What the scan cost (0 on a beta pass, or when the cloud failed it).
    pub charged: Micros,
    pub account: Account,
}

// ---------------------------------------------------------------------------
// The GPU worker
// ---------------------------------------------------------------------------

/// What the dispatcher hands a worker: one session, its file and the
/// addresses to work through. Every call carries `token`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkerLaunch {
    pub session: String,
    /// The gateway's base URL as the worker reaches it.
    pub base: String,
    pub token: String,
    pub size: u64,
    pub sha256: String,
    /// The worker ends when no job came for this long.
    pub idle_secs: u64,
}

/// Jobs for the worker from its cursor on (`after`), and the cursor to ask
/// with next; `close` once the app closed the session.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WorkerJobs {
    pub lines: Vec<Value>,
    pub next: u64,
    pub close: bool,
}

/// Engine replies numbered from `first` (counting every line the worker
/// handed in): a batch sent again after a lost answer is taken once.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WorkerEvents {
    pub first: u64,
    pub lines: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkerDone {
    pub ok: bool,
    /// Seconds the worker ran, for the GPU budget.
    pub gpu_secs: f64,
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Admin (the Discord bot, on the gateway's own machine)
// ---------------------------------------------------------------------------

pub mod admin {
    use super::*;

    pub const CODES: &str = "/admin/codes";
    pub const REVOKE: &str = "/admin/codes/revoke";
    pub const TRANSFER: &str = "/admin/transfer";
    pub const SWITCHES: &str = "/admin/switches";
    pub const STATS: &str = "/admin/stats";
    pub const PAUSE: &str = "/admin/pause";
    pub const STAGE: &str = "/admin/stage";

    pub fn account(support_id: &str) -> String {
        format!("/admin/accounts/{support_id}")
    }
    pub fn adjust(support_id: &str) -> String {
        format!("/admin/accounts/{support_id}/adjust")
    }
    pub fn block(support_id: &str) -> String {
        format!("/admin/accounts/{support_id}/block")
    }
    pub fn reset_device(support_id: &str) -> String {
        format!("/admin/accounts/{support_id}/reset_device")
    }

    /// Who acts (a Discord user), for the audit log: header `X-Actor`.
    pub const ACTOR: &str = "x-actor";

    #[derive(Clone, Debug, Serialize, Deserialize)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum CodeWorth {
        /// Money on the balance.
        Credit { cents: i64 },
        /// Free, unlimited scans until `until` (unix seconds).
        Beta { until: i64 },
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct NewCodes {
        pub worth: CodeWorth,
        pub count: u32,
        /// Unredeemed codes stop working then (unix seconds).
        pub expires: Option<i64>,
        pub note: Option<String>,
    }

    /// The codes as people type them: shown this once, stored only hashed.
    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct Codes {
        pub codes: Vec<String>,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct Revoke {
        pub code: String,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct Revoked {
        /// False when the code was unknown or already redeemed.
        pub revoked: bool,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct Adjust {
        /// Added (or, negative, taken) from the balance.
        pub amount: Micros,
        pub note: String,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct Block {
        pub blocked: bool,
        pub reason: Option<String>,
    }

    /// Move a balance (and a beta pass) to another device's account, on
    /// proof: a lost or replaced computer.
    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct Transfer {
        pub from: String,
        pub to: String,
        pub note: String,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct Transferred {
        pub moved: Micros,
    }

    /// Pause cloud scans for everyone (no GPU yet, an outage), or go on.
    /// Accounts, codes and payments keep working.
    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct SetPause {
        pub paused: bool,
    }

    /// Move the launch stage. `open` needs Stripe's live key (or the
    /// gateway's `stripe.test_mode_ok`): test mode would sell nothing real.
    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct SetStage {
        pub stage: Stage,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct SetSwitch {
        pub switch: Switch,
        pub on: bool,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct Switches {
        pub on: Vec<Switch>,
        pub off: Vec<Switch>,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct LedgerRow {
        pub at: i64,
        pub kind: String,
        pub amount: Micros,
        pub reference: Option<String>,
        pub note: Option<String>,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct SessionRow {
        pub id: String,
        pub at: i64,
        pub kind: ScanKind,
        pub duration_ms: Option<u64>,
        pub charged: Option<Micros>,
        pub state: String,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct AccountView {
        pub support_id: String,
        pub created: i64,
        pub last_seen: Option<i64>,
        pub platform: Option<String>,
        pub app_version: Option<String>,
        /// Money in, less what was spent.
        pub balance: Micros,
        /// Held by scans still running.
        pub held: Micros,
        pub beta_until: Option<i64>,
        pub blocked: bool,
        pub block_reason: Option<String>,
        pub has_device: bool,
        pub ledger: Vec<LedgerRow>,
        pub sessions: Vec<SessionRow>,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct Stats {
        pub accounts: u64,
        /// This calendar month (UTC).
        pub paid_in_cents: i64,
        pub refunded_cents: i64,
        pub charged: Micros,
        pub sessions: u64,
        pub gpu_secs: f64,
        /// The month's GPU spend as the gateway estimates it.
        pub gpu_spend: Micros,
        pub guard_alert: Micros,
        pub guard_stop: Micros,
        pub paused: bool,
        pub open_sessions: u64,
        /// The launch stage as it acts now.
        pub stage: Stage,
        /// Stripe's key is a live one: payments are real money.
        pub live_payments: bool,
        /// What a minute of video costs now, and what that follows.
        #[serde(default)]
        pub prices: Option<Prices>,
        #[serde(default)]
        pub price_basis: Option<PriceBasis>,
    }
}

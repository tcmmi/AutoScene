//! AutoScene Cloud from the app's side (see `docs/CLOUD.md`): this
//! computer's account, and analyses that run on a cloud GPU.
//!
//! People see a balance, never a key: the device's token lives in the
//! system's keychain, and the gateway knows the computer by a salted hash of
//! its machine id (a reinstall of AutoScene finds its balance again).
//!
//! An analysis in the cloud uploads the episode, lets the gateway measure and
//! price it, then runs the usual passes on an [`Engine`] whose jobs go to the
//! session's GPU worker. The session is closed when the analysis ends,
//! however it ends: the charge is what the cloud did.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use as_cloud::client::{Client, ClientError, file_sha256};
use as_cloud::{
    Account, ApiError, Depth, ErrorCode, Footage, Over, RegisterRequest, ScanKind, SessionRequest, Stage, euros_fine,
};
use as_core::{CancelToken, Msg, msg};
use as_engine::Engine;
use as_engine::client::{Remote, RemoteIo};
use as_project::{Library, Project};
use sha2::{Digest, Sha256};

/// Analyses run in the cloud ("1").
pub const KEY_ENABLED: &str = "cloud.enabled";
/// The gateway's address, when it isn't the built-in one.
pub const KEY_GATEWAY: &str = "cloud.gateway";
/// The gateway AutoScene uses when nothing else is set.
const DEFAULT_GATEWAY: Option<&str> = Some("https://api.autoscene.io");
/// A computer without a machine id the system gives keeps one of its own.
const KEY_DEVICE: &str = "cloud.device";
/// The keychain's service for the device token.
const KEYCHAIN: &str = "AutoScene Cloud";

/// Why the cloud can't be used, in the person's words.
#[derive(Debug)]
pub enum CloudError {
    /// No gateway is set up for this build.
    Unavailable,
    /// The cloud isn't open ([`Stage::Hidden`]): nothing new is taken.
    Hidden,
    /// The closed beta: a scan needs a beta pass (or balance an admin gave),
    /// and money can't be added yet.
    NeedsPass,
    /// The account's limits (a code's or an admin's) don't allow this scan.
    Over(Over),
    Gateway(ClientError),
    Keychain(String),
    /// The person stopped it.
    Cancelled,
}

impl CloudError {
    pub fn msg(&self) -> Msg {
        match self {
            CloudError::Unavailable => msg!("cloud.unavailable", "AutoScene Cloud isn't available yet."),
            CloudError::Hidden => msg!(
                "cloud.hidden",
                "AutoScene Cloud isn't available right now. Turn off Analyse in the cloud in Settings → Cloud to analyse on this computer."
            ),
            CloudError::NeedsPass => msg!(
                "cloud.betaBalance",
                "AutoScene Cloud is in a closed beta: scans need a beta pass. Redeem yours in Settings → Cloud."
            ),
            CloudError::Over(over) => over_msg(over),
            CloudError::Keychain(_) => msg!("cloud.keychain", "AutoScene couldn't use the system's keychain."),
            CloudError::Cancelled => msg!("cloud.cancelled", "The cloud scan was stopped."),
            // The gateway says which limit, when it is one.
            CloudError::Gateway(ClientError::Api(ApiError { over: Some(over), .. })) => over_msg(over),
            CloudError::Gateway(e) => match e.code() {
                Some(ErrorCode::Balance) => {
                    msg!("cloud.balance", "Your cloud balance doesn't cover this scan. Add funds in Settings → Cloud.")
                }
                Some(ErrorCode::Blocked) => msg!("cloud.blocked", "Your cloud account is blocked. Contact support."),
                Some(ErrorCode::KindOff) => {
                    msg!("cloud.kindOff", "This kind of scan isn't offered in the cloud right now.")
                }
                Some(ErrorCode::Paused) => msg!("cloud.paused", "Cloud scans are paused right now. Try again later."),
                Some(ErrorCode::Closed) => msg!("cloud.closed", "AutoScene Cloud isn't open for this yet."),
                Some(ErrorCode::Busy) => msg!("cloud.busy", "The cloud is busy. Try again in a few minutes."),
                Some(ErrorCode::Limit) => {
                    msg!("cloud.limit", "This scan goes past your cloud account's limits. Settings → Cloud shows them.")
                }
                Some(ErrorCode::TooLarge) => msg!("cloud.tooLarge", "This file is too large for the cloud."),
                Some(ErrorCode::Unreadable) => msg!("cloud.unreadable", "The cloud couldn't read this file as video."),
                Some(ErrorCode::CodeInvalid) => msg!("cloud.codeInvalid", "That code doesn't exist or has expired."),
                Some(ErrorCode::CodeUsed) => msg!("cloud.codeUsed", "That code was already used."),
                Some(ErrorCode::RateLimited) => {
                    msg!("cloud.rateLimited", "Too many tries. Wait an hour and try again.")
                }
                Some(ErrorCode::Checksum) => msg!("cloud.checksum", "The upload arrived damaged. Try again."),
                Some(ErrorCode::Gone) => msg!("cloud.gone", "The cloud scan ended early. Try again."),
                Some(ErrorCode::Server | ErrorCode::Unknown) => {
                    msg!("cloud.server", "AutoScene Cloud had a problem. Try again later.")
                }
                _ => msg!("cloud.offline", "AutoScene Cloud couldn't be reached. Check your connection."),
            },
        }
    }

    pub fn detail(&self) -> String {
        match self {
            CloudError::Unavailable => "no gateway configured".into(),
            CloudError::Hidden => "the cloud isn't open (stage hidden)".into(),
            CloudError::NeedsPass => "beta: no pass and not enough balance".into(),
            CloudError::Over(over) => format!("over the account's limits: {over:?}"),
            CloudError::Gateway(e) => e.to_string(),
            CloudError::Keychain(e) => e.clone(),
            CloudError::Cancelled => "cancelled".into(),
        }
    }
}

/// Which of the account's limits a scan breaks, and when it allows more
/// (days, weeks and months count in UTC).
fn over_msg(over: &Over) -> Msg {
    match *over {
        Over::Kind => msg!("cloud.limitKind", "Your cloud account doesn't include this kind of scan."),
        Over::Length { max_minutes } => msg!(
            "cloud.limitLength",
            one = "This video is longer than a cloud scan may be: {{count}} minute.",
            other = "This video is longer than a cloud scan may be: {{count}} minutes.",
            count = max_minutes
        ),
        Over::Scans { per_day, .. } => msg!(
            "cloud.limitScans",
            one = "Today's cloud scan is used ({{count}} a day). More from midnight (UTC).",
            other = "Today's {{count}} cloud scans are used. More from midnight (UTC).",
            count = per_day
        ),
        Over::Day { limit, left, .. } => msg!(
            "cloud.limitDay",
            "This scan would go past today's cloud allowance: {{left}} of {{limit}} is left. It renews at midnight (UTC).",
            left = euros_fine(left),
            limit = euros_fine(limit)
        ),
        Over::Week { limit, left, .. } => msg!(
            "cloud.limitWeek",
            "This scan would go past this week's cloud allowance: {{left}} of {{limit}} is left. It renews on Monday (UTC).",
            left = euros_fine(left),
            limit = euros_fine(limit)
        ),
        Over::Month { limit, left, .. } => msg!(
            "cloud.limitMonth",
            "This scan would go past this month's cloud allowance: {{left}} of {{limit}} is left. It renews on the 1st (UTC).",
            left = euros_fine(left),
            limit = euros_fine(limit)
        ),
    }
}

impl From<ClientError> for CloudError {
    fn from(e: ClientError) -> Self {
        CloudError::Gateway(e)
    }
}

/// The gateway's address: the library's setting, the environment
/// (`AUTOSCENE_CLOUD_URL`), or the built-in one.
pub fn gateway(library: &Library) -> Option<String> {
    let set = library.shared_db().setting(KEY_GATEWAY).ok().flatten().filter(|s| !s.trim().is_empty());
    set.or_else(|| std::env::var("AUTOSCENE_CLOUD_URL").ok().filter(|s| !s.trim().is_empty()))
        .or_else(|| DEFAULT_GATEWAY.map(str::to_string))
        .map(|s| s.trim().trim_end_matches('/').to_string())
}

/// Whether analyses run in the cloud (and the cloud is there).
pub fn enabled(library: &Library) -> bool {
    gateway(library).is_some() && library.shared_db().setting(KEY_ENABLED).ok().flatten().as_deref() == Some("1")
}

pub fn set_enabled(library: &Library, on: bool) -> Result<(), String> {
    library.shared_db().set_setting(KEY_ENABLED, if on { "1" } else { "0" }).map_err(|e| e.to_string())
}

fn keychain(base: &str) -> Result<keyring::Entry, CloudError> {
    keyring::Entry::new(KEYCHAIN, base).map_err(|e| CloudError::Keychain(e.to_string()))
}

fn saved_token(base: &str) -> Result<Option<String>, CloudError> {
    match keychain(base)?.get_password() {
        Ok(t) => Ok(Some(t)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(CloudError::Keychain(e.to_string())),
    }
}

/// The machine id the system gives this computer, if it gives one.
fn machine_id() -> Option<String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let out = std::process::Command::new("reg")
            .args(["query", r"HKLM\SOFTWARE\Microsoft\Cryptography", "/v", "MachineGuid"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        return text
            .lines()
            .find(|l| l.contains("MachineGuid"))
            .and_then(|l| l.split_whitespace().last())
            .map(str::to_string);
    }
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("ioreg").args(["-rd1", "-c", "IOPlatformExpertDevice"]).output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        return text
            .lines()
            .find(|l| l.contains("IOPlatformUUID"))
            .and_then(|l| l.rsplit('"').nth(1))
            .map(str::to_string);
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        return ["/etc/machine-id", "/var/lib/dbus/machine-id"]
            .iter()
            .find_map(|p| std::fs::read_to_string(p).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
    }
    #[allow(unreachable_code)]
    None
}

/// What the gateway at `base` knows this computer by: a salted hash, never
/// the id, and a different one for every gateway (one gateway can't claim
/// the account another holds).
fn hwid(library: &Library, base: &str) -> String {
    let id = machine_id().filter(|s| !s.is_empty()).unwrap_or_else(|| {
        let db = library.shared_db();
        match db.setting(KEY_DEVICE).ok().flatten() {
            Some(own) => own,
            None => {
                let own = uuid::Uuid::new_v4().to_string();
                let _ = db.set_setting(KEY_DEVICE, &own);
                own
            }
        }
    });
    as_cloud::client::hex(&Sha256::digest(format!("autoscene-cloud-v1\0{base}\0{id}").as_bytes()))
}

/// One registration at a time: two at once would leave one token in the
/// keychain and the other in use.
static REGISTERING: Mutex<()> = Mutex::new(());

/// A client of the gateway as this computer: registered on first use (or
/// again, when the gateway forgot the token).
pub fn client(library: &Library) -> Result<Client, CloudError> {
    let base = gateway(library).ok_or(CloudError::Unavailable)?;
    let _one = REGISTERING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(token) = saved_token(&base)? {
        let c = Client::new(base.clone(), Some(token));
        match c.account() {
            Err(e) if e.code() == Some(ErrorCode::Unauthorized) => {}
            Ok(_) => return Ok(c),
            Err(e) => return Err(e.into()),
        }
    }
    let mut c = Client::new(base.clone(), None);
    // No account is made before the launch: the gateway would refuse it anyway.
    if c.info()?.stage == Stage::Hidden {
        return Err(CloudError::Hidden);
    }
    c.register(&RegisterRequest {
        hwid: hwid(library, &base),
        platform: std::env::consts::OS.into(),
        app_version: env!("CARGO_PKG_VERSION").into(),
    })?;
    if let Some(token) = c.token() {
        keychain(&base)?.set_password(token).map_err(|e| CloudError::Keychain(e.to_string()))?;
    }
    Ok(c)
}

pub fn account(library: &Library) -> Result<Account, CloudError> {
    Ok(client(library)?.account()?)
}

/// A failure worth another try: the network, or the gateway busy or failing.
fn transient(e: &ClientError) -> bool {
    matches!(e, ClientError::Net(_)) || matches!(e.code(), Some(ErrorCode::Server | ErrorCode::RateLimited))
}

/// `f`, tried again on a transient failure, waiting longer each time.
fn again<T>(mut f: impl FnMut() -> Result<T, ClientError>) -> Result<T, ClientError> {
    let mut wait = 1;
    let mut tries = 0;
    loop {
        match f() {
            Err(e) if tries < 3 && transient(&e) => {
                tries += 1;
                std::thread::sleep(Duration::from_secs(wait));
                wait *= 2;
            }
            r => return r,
        }
    }
}

/// The kind of scan `project` runs now, as a cloud session is sold.
pub fn scan_kind(project: &Project, people: bool, scenes: bool) -> Result<ScanKind, String> {
    let f = project.scan_flags().map_err(|e| e.detail())?;
    let footage = if f.anime {
        Footage::Anime
    } else if f.animated {
        Footage::Animated
    } else {
        Footage::Live
    };
    let depth = if f.best {
        Depth::Best
    } else if f.fast {
        Depth::Fast
    } else {
        Depth::Standard
    };
    // People only come with the standard scan of live action or 3D.
    let people = people && footage != Footage::Anime && depth == Depth::Standard;
    Ok(ScanKind { footage, depth, people, scenes })
}

/// How far getting a session ready is, for the job's progress.
pub enum Preparing {
    Checking(f64),
    Uploading(f64),
    Measuring,
}

/// Sessions open now, closed when AutoScene quits: a scan left open would
/// run on (and cost) after the app is gone.
static OPEN: Mutex<Vec<(Arc<Client>, String)>> = Mutex::new(Vec::new());

fn opened(client: &Arc<Client>, session: &str) {
    OPEN.lock().unwrap_or_else(|e| e.into_inner()).push((client.clone(), session.to_string()));
}

/// Whether `session` was open (and is now forgotten).
fn forget(session: &str) -> bool {
    let mut open = OPEN.lock().unwrap_or_else(|e| e.into_inner());
    let before = open.len();
    open.retain(|(_, s)| s != session);
    open.len() != before
}

/// Close every open session (AutoScene quits): each is charged what it did.
pub fn close_all() {
    let open = std::mem::take(&mut *OPEN.lock().unwrap_or_else(|e| e.into_inner()));
    for (client, session) in open {
        if let Err(e) = client.close(&session) {
            log::warn!("closing the cloud session on quit: {e}");
        }
    }
}

/// Refuse at once what the gateway would refuse after the upload: a cloud
/// that isn't open, a blocked account, paused scans, a kind switched off, a
/// balance that won't do.
fn precheck(account: &Account, kind: &ScanKind, duration_ms: u64) -> Result<(), CloudError> {
    let refuse = |code, m: &str| Err(CloudError::Gateway(ClientError::Api(ApiError::new(code, m))));
    if account.stage == Stage::Hidden {
        return Err(CloudError::Hidden);
    }
    if account.blocked {
        return refuse(ErrorCode::Blocked, "the account is blocked");
    }
    if account.paused {
        return refuse(ErrorCode::Paused, "cloud scans are paused");
    }
    if kind.switches().iter().any(|s| !account.open.contains(s)) {
        return refuse(ErrorCode::KindOff, "this kind of scan is off");
    }
    // The gateway checks again with the length it measures.
    if let (Some(limits), Some(usage)) = (&account.limits, &account.usage)
        && let Some(over) = limits.over(usage, kind, duration_ms, account.prices.price(kind, duration_ms), true)
    {
        return Err(CloudError::Over(over));
    }
    if account.beta_until.is_none() && account.balance < account.prices.price(kind, duration_ms) {
        // In the beta money can't be added: the way in is a pass.
        if account.stage == Stage::Beta {
            return Err(CloudError::NeedsPass);
        }
        return refuse(ErrorCode::Balance, "the balance doesn't cover this scan");
    }
    Ok(())
}

/// A refusal of the gateway's, said for the cloud's stage: the gateway can
/// price a file above the app's guess (its size floor, its own measure), and
/// in the beta money can't be added; a cloud closed since the check is hidden.
fn staged(stage: Stage, e: ClientError) -> CloudError {
    match e.code() {
        Some(ErrorCode::Closed) => CloudError::Hidden,
        Some(ErrorCode::Balance) if stage == Stage::Beta => CloudError::NeedsPass,
        _ => CloudError::Gateway(e),
    }
}

/// A cloud session for one analysis, and the engine whose jobs go to it.
pub struct Run {
    client: Arc<Client>,
    session: String,
    pub engine: Engine,
    pub price: as_cloud::Micros,
}

impl Run {
    /// Open a session for `file`, upload it and have it priced; the engine
    /// then runs its jobs on the session's GPU.
    pub fn begin(
        library: &Library,
        file: &std::path::Path,
        kind: ScanKind,
        duration_ms: u64,
        cancel: &CancelToken,
        mut progress: impl FnMut(Preparing),
    ) -> Result<Run, CloudError> {
        let client = client(library)?;
        let account = client.account()?;
        precheck(&account, &kind, duration_ms)?;
        // The gateway's own refusals later on, said for the stage the cloud was in.
        let said = |e: ClientError| staged(account.stage, e);
        let size = std::fs::metadata(file).map_err(ClientError::Io)?.len();
        let sha256 = file_sha256(file, |done| {
            progress(Preparing::Checking(done as f64 / size.max(1) as f64));
            !cancel.is_cancelled()
        })
        .map_err(ClientError::Io)?;
        if cancel.is_cancelled() {
            return Err(CloudError::Cancelled);
        }
        let session = client.open_session(&SessionRequest { kind, size, sha256, duration_ms }).map_err(said)?;
        let client = Arc::new(client);
        opened(&client, &session.id);
        // From here on, however `begin` ends, the session is closed (Drop).
        let mut run = Run {
            client: client.clone(),
            session: session.id.clone(),
            engine: Engine::new(Default::default()),
            price: 0,
        };
        // A broken upload resumes: the gateway says where it got to.
        let mut tries = 0;
        loop {
            match client.upload(&session.id, file, |sent, size| {
                progress(Preparing::Uploading(sent as f64 / size.max(1) as f64));
                !cancel.is_cancelled()
            }) {
                Ok(_) => break,
                Err(_) if cancel.is_cancelled() => return Err(CloudError::Cancelled),
                Err(e) if transient(&e) && tries < 5 => {
                    tries += 1;
                    std::thread::sleep(Duration::from_secs(2 * tries));
                }
                Err(e) => return Err(e.into()),
            }
        }
        progress(Preparing::Measuring);
        // Measuring a large file takes a while: a cancel doesn't wait for it.
        let (tx, rx) = mpsc::channel();
        let (c, id) = (client.clone(), session.id.clone());
        std::thread::spawn(move || drop(tx.send(c.ready(&id))));
        let ready = loop {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(r) => break r.map_err(said)?,
                Err(mpsc::RecvTimeoutError::Timeout) if !cancel.is_cancelled() => {}
                Err(mpsc::RecvTimeoutError::Timeout) => return Err(CloudError::Cancelled),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(CloudError::Gateway(ClientError::Net("measuring stopped".into())));
                }
            }
        };
        run.price = ready.price;
        run.engine = Engine::remote(Arc::new(CloudRemote {
            client,
            session: session.id,
            cursor: Arc::default(),
            ready: Arc::default(),
        }));
        Ok(run)
    }

    /// Close the session: what it cost.
    pub fn finish(self) -> Result<as_cloud::Micros, CloudError> {
        if !forget(&self.session) {
            // Closed already (AutoScene is quitting).
            return Ok(0);
        }
        Ok(again(|| self.client.close(&self.session))?.charged)
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        // However the analysis ended: the GPU stops, the scan is priced.
        if forget(&self.session)
            && let Err(e) = again(|| self.client.close(&self.session))
        {
            log::warn!("closing the cloud session: {e}");
        }
    }
}

/// A session's GPU worker as an engine. The engine client may connect to it
/// again (a job that failed, to run the next one): the replies go on from
/// where they were, and the worker's ready line is said again.
struct CloudRemote {
    client: Arc<Client>,
    session: String,
    /// Replies read so far.
    cursor: Arc<AtomicU64>,
    /// The engine's ready line, once it came.
    ready: Arc<Mutex<Option<String>>>,
}

impl Remote for CloudRemote {
    fn connect(&self, replies: mpsc::Sender<String>) -> Result<Box<dyn RemoteIo>, String> {
        if let Some(line) = self.ready.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            let _ = replies.send(line);
        }
        let ended = Arc::new(AtomicBool::new(false));
        let (client, session, done) = (self.client.clone(), self.session.clone(), ended.clone());
        let (cursor, ready) = (self.cursor.clone(), self.ready.clone());
        std::thread::spawn(move || {
            let mut after = cursor.load(Ordering::Relaxed);
            let mut failures = 0;
            while !done.load(Ordering::Relaxed) {
                match client.events(&session, after) {
                    Ok(r) => {
                        failures = 0;
                        after = r.next;
                        cursor.store(after, Ordering::Relaxed);
                        for line in r.lines {
                            let mut seen = ready.lock().unwrap_or_else(|e| e.into_inner());
                            if seen.is_none() && line.contains(r#""event":"ready""#) {
                                *seen = Some(line.clone());
                            }
                            drop(seen);
                            if replies.send(line).is_err() {
                                done.store(true, Ordering::Relaxed);
                                return;
                            }
                        }
                        if r.ended {
                            if let Some(e) = r.error {
                                log::warn!("the cloud session ended: {e}");
                            }
                            break;
                        }
                    }
                    Err(e) if transient(&e) && failures < 10 => {
                        failures += 1;
                        log::warn!("reading the cloud's replies: {e}");
                        std::thread::sleep(Duration::from_secs(failures));
                    }
                    Err(e) => {
                        log::warn!("the cloud session is gone: {e}");
                        break;
                    }
                }
            }
            done.store(true, Ordering::Relaxed);
        });
        Ok(Box::new(CloudIo { client: self.client.clone(), session: self.session.clone(), ended }))
    }

    fn start_timeout(&self) -> Duration {
        // A cold start, the worker's download and its own read of the file.
        Duration::from_secs(30 * 60)
    }
}

struct CloudIo {
    client: Arc<Client>,
    session: String,
    ended: Arc<AtomicBool>,
}

impl RemoteIo for CloudIo {
    fn send(&mut self, request: &str) -> Result<(), String> {
        let line: serde_json::Value = serde_json::from_str(request).map_err(|e| e.to_string())?;
        // The engine's own shutdown is the worker's to send.
        if line.get("op").and_then(|o| o.as_str()) == Some("shutdown") {
            return Ok(());
        }
        again(|| self.client.send_jobs(&self.session, vec![line.clone()])).map_err(|e| e.to_string())
    }

    fn alive(&mut self) -> bool {
        !self.ended.load(Ordering::Relaxed)
    }

    fn stop(&mut self) {
        self.ended.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use as_cloud::{Prices, Switch};

    use super::*;

    fn account() -> Account {
        Account {
            support_id: "AS-TEST-0001".into(),
            balance: 100_000,
            beta_until: None,
            blocked: false,
            prices: Prices::default(),
            open: Switch::ALL.to_vec(),
            paused: false,
            top_ups: vec![],
            stage: Stage::Open,
            limits: None,
            usage: None,
        }
    }

    const ANIME: ScanKind = ScanKind { footage: Footage::Anime, depth: Depth::Standard, people: false, scenes: false };

    #[test]
    fn what_the_gateway_would_refuse_is_refused_before_the_upload() {
        let code = |r: Result<(), CloudError>| match r {
            Err(CloudError::Gateway(e)) => e.code(),
            _ => None,
        };
        assert!(precheck(&account(), &ANIME, 600_000).is_ok(), "ten minutes for ten cents");
        assert_eq!(code(precheck(&account(), &ANIME, 660_000)), Some(ErrorCode::Balance));
        let beta = Account { beta_until: Some(i64::MAX), balance: 0, ..account() };
        assert!(precheck(&beta, &ANIME, 3_600_000).is_ok(), "a beta pass pays for anything");
        let off = Account { open: vec![Switch::Live], ..account() };
        assert_eq!(code(precheck(&off, &ANIME, 1)), Some(ErrorCode::KindOff));
        assert_eq!(code(precheck(&Account { blocked: true, ..account() }, &ANIME, 1)), Some(ErrorCode::Blocked));
        assert_eq!(code(precheck(&Account { paused: true, ..account() }, &ANIME, 1)), Some(ErrorCode::Paused));
    }

    #[test]
    fn the_accounts_limits_are_checked_before_the_upload() {
        use as_cloud::{Limits, Usage};
        let used = Usage { day: 40_000, scans_today: 1, ..Usage::default() };
        let limited = |limits: Limits| Account {
            beta_until: Some(i64::MAX),
            limits: Some(limits),
            usage: Some(used),
            ..account()
        };
        let over = |r: Result<(), CloudError>| match r {
            Err(CloudError::Over(o)) => Some(o),
            _ => None,
        };
        // €0.05 a day, €0.04 used: a minute of anime (€0.01) fits, two don't.
        let day = limited(Limits { per_day: Some(50_000), ..Limits::default() });
        assert!(precheck(&day, &ANIME, 60_000).is_ok());
        let two = over(precheck(&day, &ANIME, 120_000));
        assert!(matches!(two, Some(Over::Day { left: 10_000, .. })));
        assert!(CloudError::Over(two.unwrap()).msg().text.contains("€0.01 of €0.05"));
        let short = limited(Limits { max_minutes: Some(20), ..Limits::default() });
        assert_eq!(over(precheck(&short, &ANIME, 21 * 60_000)), Some(Over::Length { max_minutes: 20 }));
        let one = limited(Limits { scans_per_day: Some(1), ..Limits::default() });
        assert!(matches!(over(precheck(&one, &ANIME, 60_000)), Some(Over::Scans { per_day: 1, .. })));
        let live_only = limited(Limits { kinds: Some(vec![Switch::Live]), ..Limits::default() });
        assert_eq!(over(precheck(&live_only, &ANIME, 60_000)), Some(Over::Kind));
        // The gateway's own refusal names the limit too.
        let refused = CloudError::Gateway(ClientError::Api(ApiError {
            over: Some(Over::Length { max_minutes: 20 }),
            ..ApiError::new(ErrorCode::Limit, "no")
        }));
        assert!(refused.msg().text.contains("20 minutes"));
        let plain = CloudError::Gateway(ClientError::Api(ApiError::new(ErrorCode::Limit, "no")));
        assert!(plain.msg().text.contains("Settings → Cloud"));
        let later = CloudError::Gateway(ClientError::Api(ApiError::new(ErrorCode::Unknown, "a later code")));
        assert!(later.msg().text.contains("had a problem"));
        // Half a cent shows as such.
        let half = limited(Limits { per_day: Some(45_000), ..Limits::default() });
        let fast = ScanKind { depth: Depth::Fast, ..ANIME };
        assert!(
            CloudError::Over(over(precheck(&half, &fast, 120_000)).unwrap()).msg().text.contains("€0.005 of €0.045")
        );
        // Limits the gateway didn't send usage for are left to the gateway.
        let no_usage = Account { usage: None, ..short };
        assert!(precheck(&no_usage, &ANIME, 21 * 60_000).is_ok());
    }

    #[test]
    fn the_launch_stage_words_the_refusal() {
        // Before the launch: nothing runs in the cloud, and the person is told how to go on.
        let hidden = Account { stage: Stage::Hidden, beta_until: Some(i64::MAX), ..account() };
        assert!(matches!(precheck(&hidden, &ANIME, 1), Err(CloudError::Hidden)));
        // The beta: money can't be added, so a short balance asks for a pass.
        let beta = Account { stage: Stage::Beta, ..account() };
        assert!(precheck(&beta, &ANIME, 600_000).is_ok(), "balance an admin gave still pays");
        assert!(matches!(precheck(&beta, &ANIME, 660_000), Err(CloudError::NeedsPass)));
        let pass = Account { stage: Stage::Beta, balance: 0, beta_until: Some(i64::MAX), ..account() };
        assert!(precheck(&pass, &ANIME, 3_600_000).is_ok());
        assert!(CloudError::NeedsPass.msg().text.contains("Redeem"), "the beta never sends people to Add funds");
        // The gateway's later refusals (its size floor, its own measure, a cloud closed since).
        let api = |code| ClientError::Api(ApiError::new(code, "no"));
        assert!(matches!(staged(Stage::Beta, api(ErrorCode::Balance)), CloudError::NeedsPass));
        assert!(matches!(staged(Stage::Open, api(ErrorCode::Balance)), CloudError::Gateway(_)));
        assert!(matches!(staged(Stage::Open, api(ErrorCode::Closed)), CloudError::Hidden));
        assert!(matches!(staged(Stage::Beta, api(ErrorCode::Busy)), CloudError::Gateway(_)));
    }
}

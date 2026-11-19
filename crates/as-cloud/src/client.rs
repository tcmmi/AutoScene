//! The app's side of the gateway: blocking calls, each answering with the
//! gateway's own reply or why it failed.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};

use crate::{
    Account, ApiError, CheckoutReply, CheckoutRequest, CloseReply, ErrorCode, EventsReply, Info, JobsRequest,
    ReadyReply, RedeemReply, RedeemRequest, RegisterReply, RegisterRequest, SessionReply, SessionRequest, UploadState,
    path,
};

/// Bytes sent per upload request: a failed one costs at most this much again.
pub const UPLOAD_CHUNK: usize = 8 << 20;

/// How long the gateway may hold an events request open with nothing new.
pub const EVENTS_WAIT_SECS: u64 = 25;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// The gateway answered and said no.
    #[error("{0}")]
    Api(ApiError),
    /// The gateway couldn't be reached or answered nonsense.
    #[error("cloud: {0}")]
    Net(String),
    #[error("cloud: {0}")]
    Io(#[from] std::io::Error),
}

impl ClientError {
    pub fn code(&self) -> Option<ErrorCode> {
        match self {
            ClientError::Api(e) => Some(e.code),
            _ => None,
        }
    }
}

pub type ClientResult<T> = Result<T, ClientError>;

/// A gateway at `base` (`https://…`, no trailing slash), as one device.
pub struct Client {
    base: String,
    token: Option<String>,
    agent: ureq::Agent,
}

impl Client {
    pub fn new(base: impl Into<String>, token: Option<String>) -> Self {
        let base = base.into().trim_end_matches('/').to_string();
        let agent = ureq::Agent::config_builder()
            .https_only(base.starts_with("https://"))
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(15)))
            .user_agent(concat!("AutoScene/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        Self { base, token, agent }
    }

    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    fn url(&self, p: &str) -> String {
        format!("{}{p}", self.base)
    }

    fn auth(&self) -> String {
        format!("Bearer {}", self.token.as_deref().unwrap_or(""))
    }

    fn read<T: DeserializeOwned>(mut resp: ureq::http::Response<ureq::Body>) -> ClientResult<T> {
        let status = resp.status();
        let body =
            resp.body_mut().with_config().limit(64 << 20).read_to_vec().map_err(|e| ClientError::Net(e.to_string()))?;
        if status.is_success() {
            return serde_json::from_slice(&body).map_err(|e| ClientError::Net(format!("unreadable answer: {e}")));
        }
        match serde_json::from_slice::<ApiError>(&body) {
            Ok(e) => Err(ClientError::Api(e)),
            Err(_) => Err(ClientError::Net(format!("the gateway answered {status}"))),
        }
    }

    fn post<B: Serialize, T: DeserializeOwned>(&self, p: &str, body: &B, timeout: Duration) -> ClientResult<T> {
        let body = serde_json::to_vec(body).map_err(|e| ClientError::Net(e.to_string()))?;
        let resp = self
            .agent
            .post(&self.url(p))
            .header("Authorization", &self.auth())
            .header("Content-Type", "application/json")
            .config()
            .timeout_global(Some(timeout))
            .build()
            .send(&body[..])
            .map_err(|e| ClientError::Net(e.to_string()))?;
        Self::read(resp)
    }

    fn get<T: DeserializeOwned>(&self, p: &str, timeout: Duration) -> ClientResult<T> {
        let resp = self
            .agent
            .get(&self.url(p))
            .header("Authorization", &self.auth())
            .config()
            .timeout_global(Some(timeout))
            .build()
            .call()
            .map_err(|e| ClientError::Net(e.to_string()))?;
        Self::read(resp)
    }

    /// How far the cloud is open, asked before this computer has an account
    /// (none is made while it's hidden).
    pub fn info(&self) -> ClientResult<Info> {
        self.get(path::INFO, Duration::from_secs(15))
    }

    /// This device's account, made on its first call; keeps the token.
    pub fn register(&mut self, req: &RegisterRequest) -> ClientResult<Account> {
        let reply: RegisterReply = self.post(path::DEVICES, req, Duration::from_secs(30))?;
        self.token = Some(reply.token);
        Ok(reply.account)
    }

    pub fn account(&self) -> ClientResult<Account> {
        self.get(path::ACCOUNT, Duration::from_secs(30))
    }

    pub fn redeem(&self, code: &str) -> ClientResult<RedeemReply> {
        self.post(path::REDEEM, &RedeemRequest { code: code.to_string() }, Duration::from_secs(30))
    }

    /// Stripe's payment page for a top-up of `cents`.
    pub fn checkout(&self, cents: i64) -> ClientResult<String> {
        let r: CheckoutReply = self.post(path::CHECKOUT, &CheckoutRequest { cents }, Duration::from_secs(30))?;
        Ok(r.url)
    }

    pub fn open_session(&self, req: &SessionRequest) -> ClientResult<SessionReply> {
        self.post(path::SESSIONS, req, Duration::from_secs(30))
    }

    pub fn upload_state(&self, session: &str) -> ClientResult<UploadState> {
        self.get(&path::upload(session), Duration::from_secs(30))
    }

    /// Send `file` from where the gateway's copy ends, a chunk at a time;
    /// `progress(sent, size)` after each. A broken upload resumes from the
    /// gateway's count when called again.
    pub fn upload(
        &self,
        session: &str,
        file: &Path,
        mut progress: impl FnMut(u64, u64) -> bool,
    ) -> ClientResult<UploadState> {
        let mut state = self.upload_state(session)?;
        let mut f = File::open(file)?;
        let mut buf = vec![0u8; UPLOAD_CHUNK];
        while state.received < state.size {
            if !progress(state.received, state.size) {
                return Err(ClientError::Net("cancelled".into()));
            }
            f.seek(SeekFrom::Start(state.received))?;
            let want = UPLOAD_CHUNK.min((state.size - state.received) as usize);
            f.read_exact(&mut buf[..want])?;
            let resp = self
                .agent
                .put(&self.url(&path::upload(session)))
                .query("offset", state.received.to_string())
                .header("Authorization", &self.auth())
                .header("Content-Type", "application/octet-stream")
                .config()
                .timeout_global(Some(Duration::from_secs(300)))
                .build()
                .send(&buf[..want])
                .map_err(|e| ClientError::Net(e.to_string()))?;
            state = Self::read(resp)?;
        }
        progress(state.received, state.size);
        Ok(state)
    }

    /// The file is in: the gateway measures and prices it.
    pub fn ready(&self, session: &str) -> ClientResult<ReadyReply> {
        // Hashing and measuring a large file takes a while.
        self.post(&path::ready(session), &serde_json::json!({}), Duration::from_secs(600))
    }

    pub fn send_jobs(&self, session: &str, lines: Vec<serde_json::Value>) -> ClientResult<()> {
        let _: serde_json::Value = self.post(&path::jobs(session), &JobsRequest { lines }, Duration::from_secs(30))?;
        Ok(())
    }

    /// Engine replies after line `after`, waiting up to [`EVENTS_WAIT_SECS`]
    /// for the first.
    pub fn events(&self, session: &str, after: u64) -> ClientResult<EventsReply> {
        let p = format!("{}?after={after}&wait={EVENTS_WAIT_SECS}", path::events(session));
        self.get(&p, Duration::from_secs(EVENTS_WAIT_SECS + 30))
    }

    pub fn close(&self, session: &str) -> ClientResult<CloseReply> {
        self.post(&path::close(session), &serde_json::json!({}), Duration::from_secs(60))
    }
}

/// A file's SHA-256 (hex), as a session announces it.
pub fn file_sha256(file: &Path, mut progress: impl FnMut(u64) -> bool) -> std::io::Result<String> {
    let mut f = File::open(file)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut done = 0u64;
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        done += n as u64;
        if !progress(done) {
            return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "cancelled"));
        }
    }
    Ok(hex(&hasher.finalize()))
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

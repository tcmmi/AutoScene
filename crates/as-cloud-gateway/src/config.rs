//! The gateway's settings: a TOML file, with its secrets also readable from
//! the environment (so the file can be shown without them).

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use as_cloud::{Micros, Prices};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The app's and the worker's API (behind the TLS proxy).
    pub listen: SocketAddr,
    /// The Discord bot's API: keep it on 127.0.0.1.
    pub admin_listen: SocketAddr,
    /// The gateway's address as the GPU worker reaches it (`https://…`).
    pub public_base: String,
    /// The database and the uploads.
    pub data_dir: PathBuf,
    /// The bot's bearer token (or `AS_CLOUD_ADMIN_TOKEN`).
    #[serde(default)]
    pub admin_token: String,
    /// Keys every stored hash of a code or a computer (or `AS_CLOUD_PEPPER`):
    /// a stolen database can't be searched for them. Never change it: every
    /// code and device would stop matching.
    #[serde(default)]
    pub pepper: String,
    /// Read the client's address from `X-Forwarded-For` when the request
    /// comes from this machine (the TLS proxy).
    #[serde(default)]
    pub trust_proxy: bool,
    #[serde(default = "ffprobe")]
    pub ffprobe: PathBuf,
    #[serde(default = "max_upload")]
    pub max_upload_bytes: u64,
    /// All open uploads together.
    #[serde(default = "max_uploads_total")]
    pub max_uploads_total: u64,
    /// An uploading session nobody touched for this long is dropped.
    #[serde(default = "upload_ttl")]
    pub upload_ttl_secs: u64,
    /// A running session the app stopped asking about is closed (and priced
    /// for what it did).
    #[serde(default = "app_idle")]
    pub app_idle_secs: u64,
    /// The worker ends after this long without a job.
    #[serde(default = "worker_idle")]
    pub worker_idle_secs: u64,
    /// A worker not heard from for this long has failed.
    #[serde(default = "worker_timeout")]
    pub worker_timeout_secs: u64,
    #[serde(default)]
    pub prices: Prices,
    #[serde(default)]
    pub guard: Guard,
    #[serde(default)]
    pub stripe: Stripe,
    pub backend: Backend,
}

/// The month's GPU budget.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Guard {
    /// Tell the owner past this much (micro-euros) …
    pub alert: Micros,
    /// … and open no more sessions past this.
    pub stop: Micros,
    /// What an hour of a worker costs, in micro-euros.
    pub gpu_per_hour: Micros,
    /// A Discord webhook the alerts go to (or `AS_CLOUD_ALERT_WEBHOOK`).
    #[serde(default)]
    pub alert_webhook: Option<String>,
}

impl Default for Guard {
    fn default() -> Self {
        // €25, €75, and about $0.80 an hour.
        Self { alert: 25_000_000, stop: 75_000_000, gpu_per_hour: 740_000, alert_webhook: None }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stripe {
    /// `sk_…` (or `STRIPE_SECRET_KEY`).
    #[serde(default)]
    pub secret_key: String,
    /// `whsec_…` (or `STRIPE_WEBHOOK_SECRET`).
    #[serde(default)]
    pub webhook_secret: String,
    /// Where Stripe sends the buyer after paying, and after giving up.
    #[serde(default)]
    pub success_url: String,
    #[serde(default)]
    pub cancel_url: String,
    /// When set, every payment gets an invoice with this footer (the
    /// Kleinunternehmer note): Stripe charges for these invoices.
    #[serde(default)]
    pub invoice_footer: Option<String>,
}

/// Where workers run.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Backend {
    /// A worker program on this machine (tests, a GPU of the owner's own).
    Local {
        worker: PathBuf,
        #[serde(default)]
        args: Vec<String>,
    },
    /// A serverless GPU service: the launch is POSTed as JSON to `url`,
    /// inside `{"<wrap>": …}` when `wrap` is set.
    Http {
        url: String,
        #[serde(default)]
        authorization: Option<String>,
        #[serde(default)]
        wrap: Option<String>,
    },
    /// No workers: sessions open, but never run (a dry run of the money side).
    None,
}

fn ffprobe() -> PathBuf {
    PathBuf::from("ffprobe")
}
const fn max_upload() -> u64 {
    16 << 30
}
const fn max_uploads_total() -> u64 {
    64 << 30
}
const fn upload_ttl() -> u64 {
    6 * 3600
}
const fn app_idle() -> u64 {
    30 * 60
}
const fn worker_idle() -> u64 {
    120
}
const fn worker_timeout() -> u64 {
    300
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("reading {0}: {1}")]
    Read(PathBuf, std::io::Error),
    #[error("{0}: {1}")]
    Parse(PathBuf, toml::de::Error),
    #[error("{0}")]
    Missing(&'static str),
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|e| ConfigError::Read(path.into(), e))?;
        let mut cfg: Config = toml::from_str(&text).map_err(|e| ConfigError::Parse(path.into(), e))?;
        cfg.secrets_from_env(|k| std::env::var(k).ok());
        cfg.check()?;
        Ok(cfg)
    }

    /// Secrets the file left empty, from the environment.
    pub fn secrets_from_env(&mut self, env: impl Fn(&str) -> Option<String>) {
        let fill = |slot: &mut String, key: &str| {
            if slot.is_empty()
                && let Some(v) = env(key)
            {
                *slot = v;
            }
        };
        fill(&mut self.admin_token, "AS_CLOUD_ADMIN_TOKEN");
        fill(&mut self.pepper, "AS_CLOUD_PEPPER");
        fill(&mut self.stripe.secret_key, "STRIPE_SECRET_KEY");
        fill(&mut self.stripe.webhook_secret, "STRIPE_WEBHOOK_SECRET");
        if self.guard.alert_webhook.is_none() {
            self.guard.alert_webhook = env("AS_CLOUD_ALERT_WEBHOOK");
        }
    }

    pub fn check(&self) -> Result<(), ConfigError> {
        if self.admin_token.len() < 24 {
            return Err(ConfigError::Missing("admin_token must be set (24 characters or more)"));
        }
        if self.pepper.len() < 24 {
            return Err(ConfigError::Missing("pepper must be set (24 characters or more)"));
        }
        Ok(())
    }
}

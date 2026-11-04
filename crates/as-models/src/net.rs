//! The requests the store makes, and a small capped one for anything else
//! AutoScene fetches from the internet (an index, a signature): one agent
//! with AutoScene's user agent, redirects followed by hand so a cancel and
//! a time budget hold on every hop.

use std::time::{Duration, Instant};

use as_core::CancelToken;

use crate::{ModelError, ModelResult, Watch, pump};

/// Name lookup, connecting and the response headers each get this long.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// The most redirects a request follows (ureq's own limit).
const MAX_REDIRECTS: usize = 10;
/// Who asks, as anilist says it too.
pub const USER_AGENT: &str = concat!("AutoScene/", env!("CARGO_PKG_VERSION"), " (+https://github.com/tcmmi/AutoScene)");

/// The agent for a download's requests. It never follows a file on https to
/// a plain http address: the hash would still catch a forged file, but
/// nothing of it has to travel in the clear.
pub fn agent(url: &str) -> ureq::Agent {
    config(url).build().into()
}

/// What [`agent`] is made of: its timeouts, its user agent, the proxy the
/// environment names (`HTTPS_PROXY`, `ALL_PROXY`), and `https_only` when
/// `url` is on https. For an agent that resolves names its own way.
pub fn config(url: &str) -> ureq::config::ConfigBuilder<ureq::typestate::AgentScope> {
    ureq::Agent::config_builder()
        .https_only(url.starts_with("https://"))
        .timeout_resolve(Some(CONNECT_TIMEOUT))
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_send_request(Some(CONNECT_TIMEOUT))
        .timeout_recv_response(Some(CONNECT_TIMEOUT))
        .user_agent(USER_AGENT)
}

/// GET `url` (`range`: its Range header), following its redirects here, not
/// in ureq: ureq reads a redirect's body to its end inside `.call()`, where
/// only a global timeout bounds it, and a whole model can't have one. A
/// redirect's body is never read; its connection just closes. A cancel is
/// checked before every hop, and each keeps the agent's `https_only`.
/// `budget`: how long a small request may take in all, its body included.
pub(crate) fn get(
    agent: &ureq::Agent,
    url: &str,
    range: Option<&str>,
    budget: Option<Duration>,
    watch: Watch,
) -> ModelResult<ureq::http::Response<ureq::Body>> {
    let failed = |e: String| ModelError::Download(format!("{url}: {e}"));
    // A budget too long to reach is no budget (rather than a panic).
    let deadline = budget.and_then(|b| Instant::now().checked_add(b));
    let mut at = url::Url::parse(url).map_err(|e| failed(e.to_string()))?;
    for _ in 0..=MAX_REDIRECTS {
        watch.check()?;
        let mut req = agent.get(at.as_str());
        if let Some(range) = range {
            req = req.header("Range", range);
        }
        let resp = req
            .config()
            .max_redirects(0)
            .timeout_global(deadline.map(|d| d.saturating_duration_since(Instant::now())))
            .build()
            .call()
            .map_err(|e| failed(e.to_string()))?;
        let Some(location) = resp.headers().get("location").filter(|_| resp.status().is_redirection()) else {
            return Ok(resp);
        };
        let location = location.to_str().map_err(|_| failed("a redirect to an unreadable address".into()))?;
        at = at.join(location).map_err(|e| failed(format!("a redirect to {location}: {e}")))?;
    }
    Err(failed(format!("more than {MAX_REDIRECTS} redirects")))
}

/// The whole body of `url`, which must answer 200 within `budget` (its
/// body included) and send at most `max` bytes. Redirects are followed as
/// [`get`] does; nothing is sent beyond the request and the user agent.
/// The error says what went wrong, for logs: a caller words it for people.
pub fn get_capped(url: &str, max: u64, budget: Duration, cancel: &CancelToken) -> Result<Vec<u8>, String> {
    // A body quiet for the whole budget is over it anyway.
    let watch = Watch { cancel, stall: budget };
    let resp = get(&agent(url), url, None, Some(budget), watch).map_err(plain)?;
    if resp.status() != 200 {
        return Err(format!("{url}: the server answered {}", resp.status()));
    }
    let mut body = Vec::new();
    pump(resp, url, watch, |chunk| {
        if body.len() as u64 + chunk.len() as u64 > max {
            return Err(ModelError::Download(format!("{url}: more than {max} bytes")));
        }
        body.extend_from_slice(chunk);
        Ok(())
    })
    .map_err(plain)?;
    Ok(body)
}

/// A request's error without the words about AI models.
fn plain(e: ModelError) -> String {
    match e {
        ModelError::Download(why) => why,
        e => e.to_string(),
    }
}

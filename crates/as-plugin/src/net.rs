//! `net.fetch`: a plugin's way to the internet through AutoScene, held to
//! the hosts its `plugin.json` names and the person agreed to.
//!
//! [`fetch`] reaches only `https://` addresses on port 443 whose host is one
//! of those names exactly ([`host_allowed`]): no subdomain, no address such
//! as 127.0.0.1, no user name in it. It sends six methods and the headers a
//! plugin may set: never `host`, `user-agent` or the others that say how
//! the request travels. AutoScene sets those, so servers see which plugin
//! asks. It follows at most [`MAX_REDIRECTS`] redirects itself, each to a
//! host it may reach, and reads at most [`MAX_RESPONSE_BODY`] of the answer.
//!
//! A name may still lead to this computer or its network: its DNS decides.
//! So [`UreqTransport`] connects only to the addresses [`is_public`] allows
//! ([`PublicResolver`]), and to exactly the ones it checked: a name whose
//! answer changes between the check and the connect gains nothing. It never
//! goes through a proxy, which would resolve the name itself. A plugin
//! behind a company proxy can't reach the internet this way.
//!
//! A call whose caller is gone (a run that was cancelled, or whose program
//! ended) stops where it is: nothing more is sent, not even where a request
//! was redirected, and its answer isn't read on. What it had sent may have
//! arrived. A name lookup or a connection still being made can't be
//! stopped: the call stops waiting for it, and it ends on a thread of its
//! own at the call's timeout, sending nothing.
//!
//! The rules on names ([`public_name`], [`host_allowed`]) and addresses
//! ([`is_public`]) live here alone; whatever else lets a plugin reach the
//! network holds it to the same.

use std::fmt;
use std::io::{self, Read};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use as_core::CancelToken;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use ureq::config::{AutoHeaderValue, Config};
use ureq::http::{self, Uri};
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::time::Duration::Exact;
use ureq::unversioned::transport::{
    Buffers, ConnectionDetails, Connector, NextTimeout, RustlsConnector, TcpConnector, Transport as Connection,
};
use ureq::{Agent, AsSendBody, Body, RequestExt};
use url::{Host, Url};

use crate::calls::{NetRequest, NetResponse};
use crate::error::{PluginError, PluginResult};
use crate::manifest::shown;

/// The largest body a request sends, decoded.
pub const MAX_REQUEST_BODY: usize = 4 << 20;
/// The largest answer read; a larger one fails the call.
pub const MAX_RESPONSE_BODY: u64 = 8 << 20;
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
/// The longest a call may ask to wait, redirects included.
pub const MAX_TIMEOUT: Duration = Duration::from_secs(120);
pub const MAX_REDIRECTS: usize = 5;
pub const MAX_HEADERS: usize = 32;
/// The most bytes a request's own headers take, names and values.
pub const MAX_HEADER_BYTES: usize = 8 << 10;
/// The `net.fetch` calls one plugin may have waiting for an answer at
/// once; the app counts them.
pub const MAX_IN_FLIGHT: usize = 8;
/// The longest address a call may name.
pub const MAX_URL_BYTES: usize = 8 << 10;
/// How long [`UreqTransport`] waits for a host at a time, between looks
/// whether the request's caller is gone.
const WATCH_EVERY: Duration = Duration::from_millis(50);

const METHODS: [&str; 6] = ["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE"];

/// The headers that say what a body is, which a request that becomes a GET
/// without its body sends no more.
const BODY_HEADERS: [&str; 4] = ["content-type", "content-encoding", "content-language", "content-location"];

/// Headers a plugin doesn't set, besides `proxy-*`: how the request
/// travels is AutoScene's, and so is who asks.
const OWN_HEADERS: [&str; 10] = [
    "host",
    "connection",
    "content-length",
    "transfer-encoding",
    "te",
    "upgrade",
    "expect",
    "keep-alive",
    "trailer",
    "user-agent",
];

/// Last labels of names that never mean a host on the internet.
const LOCAL_ENDINGS: [&str; 8] = ["localhost", "local", "lan", "internal", "localdomain", "test", "invalid", "example"];

/// A request as [`fetch`] sends it, checked.
#[derive(Clone, Debug, PartialEq)]
pub struct Outgoing {
    pub method: &'static str,
    pub url: String,
    /// The plugin's headers, and `user-agent`.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// What is left of the call's time.
    pub timeout: Duration,
}

/// An answer, its body still unread: [`fetch`] reads no more of it than it
/// keeps.
pub struct Incoming {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Box<dyn Read + Send>,
}

/// Where [`fetch`] sends its requests: [`UreqTransport`], or a test's
/// stand-in. Once `ended` fires, a request on its way stops where it is,
/// and so does reading its answer.
pub trait Transport: Send + Sync {
    fn send(&self, o: &Outgoing, ended: &CancelToken) -> PluginResult<Incoming>;
}

/// Sends `req` for a plugin that may reach `hosts`, as `user_agent`,
/// through `t`, following its redirects, and gives the answer: its body as
/// text when it is text (JSON and XML among it) with no control character
/// but tabs and line breaks, else as base64. Once
/// `ended` fires (its caller is gone) it stops where it is, and fails as
/// [`PluginError::Cancelled`].
pub fn fetch(
    req: NetRequest,
    hosts: &[String],
    user_agent: &str,
    t: &dyn Transport,
    ended: &CancelToken,
) -> PluginResult<NetResponse> {
    if req.url.len() > MAX_URL_BYTES {
        return Err(invalid("net.fetch's url is longer than 8 KB."));
    }
    let parsed =
        Url::parse(&req.url).map_err(|_| invalid("net.fetch's url isn't an address such as https://discord.com/api."));
    let mut url = checked(parsed?, hosts)?;
    let method = method(req.method.as_deref())?;
    let mut headers = headers(req.headers)?;
    headers.push(("user-agent".into(), user_agent.into()));
    let body = body(req.body, req.body_base64)?;
    if !body.is_empty() && matches!(method, "GET" | "HEAD") {
        return Err(invalid("A GET or HEAD request has no body."));
    }
    let timeout = match req.timeout_ms {
        None => DEFAULT_TIMEOUT,
        Some(ms) if (1..=MAX_TIMEOUT.as_millis()).contains(&u128::from(ms)) => Duration::from_millis(ms.into()),
        Some(_) => return Err(invalid("net.fetch's timeoutMs must be from 1 to 120000.")),
    };
    let deadline = Instant::now() + timeout;
    let mut o = Outgoing { method, url: url.to_string(), headers, body, timeout };
    let mut redirects = 0;
    loop {
        let host = host_of(&url);
        // Nothing more for a caller that is gone, not even where a request
        // it sent before was redirected.
        if ended.is_cancelled() {
            return Err(PluginError::Cancelled);
        }
        o.timeout = deadline.saturating_duration_since(Instant::now());
        if o.timeout.is_zero() {
            return Err(failed(format!("{host} didn't answer in time."), None));
        }
        let incoming = t.send(&o, ended).map_err(|e| stopped(e, ended))?;
        let redirect = matches!(incoming.status, 301 | 302 | 303 | 307 | 308);
        let Some(location) = header(&incoming.headers, "location").filter(|_| redirect) else {
            return answer(url, o.method, incoming, ended);
        };
        if redirects == MAX_REDIRECTS {
            return Err(failed(format!("{host} redirected more than {MAX_REDIRECTS} times."), None));
        }
        redirects += 1;
        let next = url
            .join(location)
            .map_err(|_| failed(format!("{host} redirected to an address AutoScene can't read."), None))?;
        let next = checked(next, hosts)?;
        // A 303 asks for a GET; after a POST, browsers take a 301 or a 302
        // alike. Either way without the body, nor the headers that say what
        // it is.
        if (incoming.status == 303 && o.method != "HEAD")
            || (matches!(incoming.status, 301 | 302) && o.method == "POST")
        {
            o.method = "GET";
            o.body.clear();
            o.headers.retain(|(name, _)| !BODY_HEADERS.iter().any(|h| name.eq_ignore_ascii_case(h)));
        }
        // What signs the plugin in at one host isn't sent to another.
        if next.host_str() != url.host_str() {
            o.headers.retain(|(name, _)| !["authorization", "cookie"].iter().any(|h| name.eq_ignore_ascii_case(h)));
        }
        o.url = next.to_string();
        url = next;
    }
}

/// `e`, or [`PluginError::Cancelled`] once `ended` fired: whatever failed
/// then, the call's end stopped it.
fn stopped(e: PluginError, ended: &CancelToken) -> PluginError {
    match ended.is_cancelled() {
        true => PluginError::Cancelled,
        false => e,
    }
}

/// Whether `ip` is an address on the internet: not this computer's, its
/// network's, or one set aside (documentation, multicast, reserved). An
/// IPv6 address that carries an IPv4 one is judged by that.
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => public_v4(ip),
        IpAddr::V6(ip) => public_v6(ip),
    }
}

fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(a == 0 // "this network", the unspecified address among it
        || a == 10 // private
        || a == 127 // loopback
        || (a == 100 && (64..128).contains(&b)) // shared (carrier-grade NAT)
        || (a == 169 && b == 254) // link-local
        || (a == 172 && (16..32).contains(&b)) // private
        || (a == 192 && b == 0 && c == 0) // protocol assignments
        || (a == 192 && b == 0 && c == 2) // documentation
        || (a == 192 && b == 168) // private
        || (a == 198 && (18..20).contains(&b)) // benchmarking
        || (a == 198 && b == 51 && c == 100) // documentation
        || (a == 203 && b == 0 && c == 113) // documentation
        || a >= 224) // multicast, reserved, broadcast
}

fn public_v6(ip: Ipv6Addr) -> bool {
    let s = ip.segments();
    let [.., w, x, y, z] = ip.octets();
    if let Some(v4) = ip.to_ipv4_mapped() {
        return public_v4(v4);
    }
    // NAT64 (64:ff9b::/96) reaches the IPv4 address it carries.
    if s[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        return public_v4(Ipv4Addr::new(w, x, y, z));
    }
    // So does 6to4 (2002::/16), the one right after its prefix.
    if s[0] == 0x2002 {
        let [_, _, a, b, c, d, ..] = ip.octets();
        return public_v4(Ipv4Addr::new(a, b, c, d));
    }
    !(s[..4] == [0; 4] // unspecified, loopback, IPv4-compatible (gone), translated
        || s[..4] == [0x100, 0, 0, 0] // discard
        || (s[0] == 0x64 && s[1] == 0xff9b && s[2] == 1) // NAT64 of a local network
        || (s[0] == 0x2001 && s[1] < 0x200) // protocol assignments: Teredo, benchmarking…
        || (s[0] == 0x2001 && s[1] == 0xdb8) // documentation
        || (s[0] == 0x3fff && s[1] < 0x1000) // documentation
        || (s[0] & 0xfe00) == 0xfc00 // unique local
        || (s[0] & 0xffc0) == 0xfe80 // link-local
        || (s[0] & 0xffc0) == 0xfec0 // site-local (gone)
        || (s[0] & 0xff00) == 0xff00) // multicast
}

/// Whether `host` (a name as `network.hosts` takes it) can name a host on
/// the internet: two labels at least, the last of letters only or an
/// internationalized one in its ASCII form (`xn--p1ai`, so no address of
/// any spelling), and not a name for this computer or a local network
/// (`localhost`, `.local`, `.lan`, `.internal`, `.home.arpa`…) or one set
/// aside (`.test`, `.invalid`, `.example`). It says what a name means, for
/// clear messages; the wall is the address check.
pub fn public_name(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    let labels: Vec<&str> = host.split('.').collect();
    let last = labels[labels.len() - 1];
    let ascii_form = |l: &str| {
        let rest = l.strip_prefix("xn--").unwrap_or_default();
        !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    };
    labels.len() >= 2
        && labels.iter().all(|l| !l.is_empty())
        && (last.bytes().all(|b| b.is_ascii_alphabetic()) || ascii_form(last))
        && !LOCAL_ENDINGS.contains(&last)
        && host != "home.arpa"
        && !host.ends_with(".home.arpa")
}

/// Whether a request may go to `host`: one of `hosts` exactly, in any
/// case, with no trailing dot, and never an address.
pub fn host_allowed(host: &str, hosts: &[String]) -> bool {
    let host = host.to_ascii_lowercase();
    !host.ends_with('.') && !ip_like(&host) && hosts.contains(&host)
}

/// Whether `host` is, or reads as, an address: in brackets, an IP address,
/// or a name whose last label is a number (`127.1`, `0x7f.1`).
fn ip_like(host: &str) -> bool {
    let last = host.trim_end_matches('.').rsplit('.').next().unwrap_or_default();
    let number = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let hex = |s: &str| {
        let digits = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"));
        digits.is_some_and(|d| d.bytes().all(|b| b.is_ascii_hexdigit()))
    };
    host.starts_with('[') || host.parse::<IpAddr>().is_ok() || number(last) || hex(last)
}

/// `url`, when a request may go there: an `https://` address on port 443
/// with no user name, at one of `hosts`. Without its fragment, which is
/// the page's own.
fn checked(mut url: Url, hosts: &[String]) -> PluginResult<Url> {
    if url.scheme() != "https" {
        return Err(denied("net.fetch reaches only https:// addresses."));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(denied("net.fetch sends no user name or password in the address."));
    }
    if url.port().is_some_and(|p| p != 443) {
        return Err(denied("net.fetch reaches only port 443."));
    }
    let host = match url.host() {
        Some(Host::Domain(host)) if !ip_like(host) => host,
        _ => return Err(denied("net.fetch reaches host names, not addresses such as 127.0.0.1.")),
    };
    if !host_allowed(host, hosts) {
        let host = shown(host);
        return Err(denied(format!("net.fetch reaches only the hosts the plugin names, and {host} isn't one.")));
    }
    url.set_fragment(None);
    Ok(url)
}

/// The host of `url`, which [`checked`] passed.
fn host_of(url: &Url) -> String {
    url.host_str().map(shown).unwrap_or_default()
}

/// The method a call names, GET when it names none.
fn method(asked: Option<&str>) -> PluginResult<&'static str> {
    let Some(asked) = asked else { return Ok("GET") };
    METHODS
        .into_iter()
        .find(|m| m.eq_ignore_ascii_case(asked))
        .ok_or_else(|| invalid(format!("net.fetch sends GET, HEAD, POST, PUT, PATCH or DELETE, not {}.", shown(asked))))
}

/// The headers a call asks to send, when it may send them.
fn headers(asked: Vec<(String, String)>) -> PluginResult<Vec<(String, String)>> {
    if asked.len() > MAX_HEADERS {
        return Err(invalid(format!("net.fetch sends at most {MAX_HEADERS} headers.")));
    }
    if asked.iter().map(|(name, value)| name.len() + value.len()).sum::<usize>() > MAX_HEADER_BYTES {
        return Err(invalid("The headers of a net.fetch are larger than 8 KB."));
    }
    // RFC 9110's token characters.
    let token = |b: u8| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b);
    for (name, value) in &asked {
        if name.is_empty() || !name.bytes().all(token) {
            return Err(invalid(format!("{} isn't a header name.", shown(name))));
        }
        let name = name.to_ascii_lowercase();
        if OWN_HEADERS.contains(&name.as_str()) || name.starts_with("proxy-") {
            return Err(invalid(format!("net.fetch doesn't send a plugin's {name} header.")));
        }
        if value.chars().any(|c| c.is_ascii_control() && c != '\t') {
            return Err(invalid(format!("The {name} header has a line break or another control character in it.")));
        }
        // ureq sends this one only in ASCII, and would refuse it once
        // connected, as if the host had failed.
        if name == "authorization" && !value.is_ascii() {
            return Err(invalid("The authorization header has a character in it that isn't ASCII."));
        }
    }
    Ok(asked)
}

/// The body a call sends: its text or its base64, never both.
fn body(text: Option<String>, base64: Option<String>) -> PluginResult<Vec<u8>> {
    let body = match (text, base64) {
        (Some(_), Some(_)) => return Err(invalid("A net.fetch sends body or bodyBase64, not both.")),
        (Some(text), None) => text.into_bytes(),
        (None, Some(base64)) => STANDARD.decode(base64).map_err(|_| invalid("net.fetch's bodyBase64 isn't base64."))?,
        (None, None) => Vec::new(),
    };
    match body.len() > MAX_REQUEST_BODY {
        true => Err(invalid("The body of a net.fetch is larger than 4 MB.")),
        false => Ok(body),
    }
}

/// The answer from `url` to a request of `method`, as the call gets it,
/// read until `ended` fires.
fn answer(url: Url, method: &str, incoming: Incoming, ended: &CancelToken) -> PluginResult<NetResponse> {
    let too_large = || invalid("The answer is larger than 8 MB.");
    let length = header(&incoming.headers, "content-length").and_then(|l| l.trim().parse::<u64>().ok());
    // A HEAD's length is that of the answer it didn't send; a 1xx, a 204 or
    // a 304 has no body, whatever length it names.
    let bodiless = method == "HEAD" || matches!(incoming.status, 100..200 | 204 | 304);
    if !bodiless && length.is_some_and(|l| l > MAX_RESPONSE_BODY) {
        return Err(too_large());
    }
    let mut body = Vec::new();
    let read = Until { body: incoming.body, ended }.take(MAX_RESPONSE_BODY + 1).read_to_end(&mut body);
    read.map_err(|e| {
        if ended.is_cancelled() {
            return PluginError::Cancelled;
        }
        let host = host_of(&url);
        let message = match timed_out(&e) {
            true => format!("{host} didn't answer in time."),
            false => format!("The answer from {host} broke off."),
        };
        tracing::debug!(host, error = %e, "net.fetch's answer broke off");
        failed(message, Some(e.to_string()))
    })?;
    if body.len() as u64 > MAX_RESPONSE_BODY {
        return Err(too_large());
    }
    let text = header(&incoming.headers, "content-type").is_some_and(is_text);
    let (body, body_base64) = match String::from_utf8(body) {
        Ok(body) if text && !has_controls(&body) => (Some(body), None),
        Ok(body) => (None, Some(STANDARD.encode(body))),
        Err(e) => (None, Some(STANDARD.encode(e.into_bytes()))),
    };
    let headers = incoming.headers.into_iter().map(|(name, value)| (name.to_ascii_lowercase(), value)).collect();
    Ok(NetResponse { status: incoming.status, url: url.into(), headers, body, body_base64 })
}

/// A body, read until its call ends.
struct Until<'a> {
    body: Box<dyn Read + Send>,
    ended: &'a CancelToken,
}

impl Read for Until<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.ended.is_cancelled() {
            true => Err(io::Error::other("the call ended")),
            false => self.body.read(buf),
        }
    }
}

/// Whether `e`, which broke off reading an answer, says time ran out: by its
/// kind, or a ureq timeout inside it (as ureq's body reader hands it on).
fn timed_out(e: &io::Error) -> bool {
    let inner = e.get_ref().and_then(|inner| inner.downcast_ref::<ureq::Error>());
    e.kind() == io::ErrorKind::TimedOut || matches!(inner, Some(ureq::Error::Timeout(_)))
}

/// Whether `text` has a control character other than a tab or a line
/// break. JSON writes each as six bytes (`\u001b`), so text with them
/// comes as base64, which takes 4/3: a reply is never more than twice its
/// answer (a quote or a backslash takes two).
fn has_controls(text: &str) -> bool {
    text.bytes().any(|b| b < 0x20 && !matches!(b, b'\t' | b'\n' | b'\r'))
}

/// Whether an answer of `content_type` is text: `text/*`, JSON or XML.
fn is_text(content_type: &str) -> bool {
    let t = content_type.split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
    t.starts_with("text/")
        || ["application/json", "application/xml"].contains(&t.as_str())
        || t.ends_with("+json")
        || t.ends_with("+xml")
}

/// The first value of the header `name` (lowercase).
fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}

fn invalid(message: impl Into<String>) -> PluginError {
    PluginError::Invalid(message.into())
}

fn denied(message: impl Into<String>) -> PluginError {
    PluginError::Denied(message.into())
}

/// The internet failed the call: a sentence for the plugin, and what the
/// connection said.
fn failed(message: String, detail: Option<String>) -> PluginError {
    PluginError::Failed { message, detail }
}

/// Sends `net.fetch` requests to the internet: over TLS, never through a
/// proxy, only to public addresses ([`PublicResolver`]), following no
/// redirect ([`fetch`] follows them, checking each), and handing back an
/// answer of any status. Each request has an agent and a thread of its
/// own, whose connection gives up once the request's caller is gone
/// ([`Stoppable`]); what can't (a name lookup, a connect) is left to end
/// on that thread, and the call doesn't wait for it.
pub struct UreqTransport {
    config: Config,
    resolver: Arc<dyn Resolver>,
}

impl UreqTransport {
    pub fn new() -> Self {
        Self::with_resolver(DefaultResolver::default())
    }

    /// One whose names `inner` resolves; what it answers still passes
    /// [`PublicResolver`]. For tests, which answer what they need.
    #[doc(hidden)]
    pub fn with_resolver(inner: impl Resolver) -> Self {
        Self::resolving(Arc::new(PublicResolver::new(inner)))
    }

    /// One that connects wherever `inner` says, this computer too: for the
    /// tests that need something to listen there.
    #[cfg(test)]
    fn unchecked(inner: impl Resolver) -> Self {
        Self::resolving(Arc::new(inner))
    }

    fn resolving(resolver: Arc<dyn Resolver>) -> Self {
        let config = Agent::config_builder()
            .https_only(true)
            .max_redirects(0)
            // The default reads HTTPS_PROXY and the like, and a proxy
            // resolves the name itself, past the address check.
            .proxy(None)
            .http_status_as_error(false)
            .save_redirect_history(false)
            // fetch sends its own.
            .user_agent(AutoHeaderValue::None)
            .build();
        Self { config, resolver }
    }

    /// Sends `o` through `agent`, and gives its answer once its head is in.
    fn request(agent: &Agent, o: &Outgoing) -> Result<http::Response<Body>, ureq::Error> {
        let mut request = http::Request::builder().method(o.method).uri(&o.url);
        for (name, value) in &o.headers {
            request = request.header(name, value);
        }
        match o.body.is_empty() && matches!(o.method, "GET" | "HEAD" | "DELETE") {
            true => Self::run(agent, request.body(()), o.timeout),
            false => Self::run(agent, request.body(o.body.as_slice()), o.timeout),
        }
    }

    fn run<S: AsSendBody>(
        agent: &Agent,
        request: Result<http::Request<S>, http::Error>,
        timeout: Duration,
    ) -> Result<http::Response<Body>, ureq::Error> {
        request?.with_agent(agent).configure().timeout_global(Some(timeout)).build().run()
    }
}

impl Default for UreqTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl Transport for UreqTransport {
    fn send(&self, o: &Outgoing, ended: &CancelToken) -> PluginResult<Incoming> {
        // ureq's own connector, with each connection held to `ended` under
        // its TLS, so the handshake stops too.
        let connector =
            ().chain(TcpConnector::default()).chain(Stopping(ended.clone())).chain(RustlsConnector::default());
        let agent = Agent::with_parts(self.config.clone(), connector, Shared(self.resolver.clone()));
        let host = Url::parse(&o.url).map(|url| host_of(&url)).unwrap_or_default();
        let unreached = |detail: Option<String>| failed(format!("Couldn't reach {host}."), detail);
        // On a thread of its own, which the call stops waiting for once
        // `ended` fires: the name lookup and the connect don't stop for it.
        let (tx, rx) = mpsc::sync_channel(1);
        let o = o.clone();
        let started = thread::Builder::new().name("net-fetch".into()).spawn(move || {
            tx.send(Self::request(&agent, &o)).ok();
        });
        started.map_err(|e| unreached(Some(e.to_string())))?;
        let sent = loop {
            match rx.recv_timeout(WATCH_EVERY) {
                Ok(sent) => break sent,
                Err(RecvTimeoutError::Timeout) if ended.is_cancelled() => return Err(PluginError::Cancelled),
                Err(RecvTimeoutError::Timeout) => {}
                // Its thread panicked.
                Err(RecvTimeoutError::Disconnected) => return Err(unreached(None)),
            }
        };
        let response = sent.map_err(|e| not_reached(e, &host))?;
        let headers = response.headers().iter();
        let headers = headers.map(|(name, value)| (name.to_string(), String::from_utf8_lossy(value.as_bytes()).into()));
        Ok(Incoming {
            status: response.status().as_u16(),
            headers: headers.collect(),
            body: Box::new(response.into_body().into_reader()),
        })
    }
}

/// Holds each connection it is given to `ended` ([`Stoppable`]).
#[derive(Debug)]
struct Stopping(CancelToken);

impl<In: Connection> Connector<In> for Stopping {
    type Out = Stoppable<In>;

    fn connect(&self, _: &ConnectionDetails, chained: Option<In>) -> Result<Option<Self::Out>, ureq::Error> {
        Ok(chained.map(|inner| Stoppable { inner, ended: self.0.clone() }))
    }
}

/// A connection that gives up once `ended` fires: it sends nothing more,
/// and waits for the host a little at a time, looking in between. (Nothing
/// tells when a [`CancelToken`] fires; and ureq's `Agent::shutdown` shuts
/// the socket down, which on Windows doesn't wake a read waiting on it.) A
/// write the host stopped taking still ends only at the call's timeout,
/// on the request's thread.
#[derive(Debug)]
struct Stoppable<T> {
    inner: T,
    ended: CancelToken,
}

impl<T> Stoppable<T> {
    fn still_running(&self) -> Result<(), ureq::Error> {
        match self.ended.is_cancelled() {
            true => Err(ureq::Error::Io(io::Error::other("the call ended"))),
            false => Ok(()),
        }
    }
}

impl<T: Connection> Connection for Stoppable<T> {
    fn buffers(&mut self) -> &mut dyn Buffers {
        self.inner.buffers()
    }

    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), ureq::Error> {
        self.still_running()?;
        self.inner.transmit_output(amount, timeout)
    }

    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
        // The read's own limit, a step at a time.
        let limit = timeout.for_read();
        let started = Instant::now();
        loop {
            self.still_running()?;
            let left = limit.after.saturating_sub(started.elapsed());
            let last = left <= WATCH_EVERY;
            let step = NextTimeout { after: Exact(left.min(WATCH_EVERY)), per_read: None, ..limit };
            match self.inner.await_input(step) {
                Err(ureq::Error::Timeout(_)) if !last => {}
                read => return read,
            }
        }
    }

    fn is_open(&mut self) -> bool {
        self.inner.is_open()
    }

    fn is_tls(&self) -> bool {
        self.inner.is_tls()
    }
}

/// The resolver of [`UreqTransport`], for the agent of each request.
#[derive(Debug)]
struct Shared(Arc<dyn Resolver>);

impl Resolver for Shared {
    fn resolve(&self, uri: &Uri, config: &Config, timeout: NextTimeout) -> Result<ResolvedSocketAddrs, ureq::Error> {
        self.0.resolve(uri, config, timeout)
    }
}

/// `e`, which ended a request to `host` before its answer, as the call
/// gets it. Never with the address: it may hold what signs the plugin in.
fn not_reached(e: ureq::Error, host: &str) -> PluginError {
    use ureq::Error as E;
    let message = match &e {
        E::Io(io) if io.get_ref().is_some_and(|inner| inner.is::<NotPublic>()) => {
            return denied(format!("{host} resolves to an address on this computer or its network."));
        }
        E::HostNotFound => format!("{host} wasn't found."),
        E::Timeout(_) => format!("{host} didn't answer in time."),
        // rustls fails a handshake with an io error of this kind.
        E::Io(io) if io.kind() == io::ErrorKind::InvalidData => format!("The secure connection to {host} failed."),
        E::Tls(_) | E::Rustls(_) | E::Pem(_) => format!("The secure connection to {host} failed."),
        E::Io(_) | E::ConnectionFailed => format!("Couldn't reach {host}."),
        // These say the address.
        E::BadUri(_) | E::RequireHttpsOnly(_) => {
            return failed(format!("{host} answered in a way AutoScene can't read."), None);
        }
        _ => format!("{host} answered in a way AutoScene can't read."),
    };
    // The plugin gets the sentence; the log, what the connection said.
    tracing::debug!(host, error = %e, "net.fetch didn't get an answer");
    failed(message, Some(e.to_string()))
}

/// Whether the system's lookup failed with `e` because the name isn't
/// there. Windows says so (WSAHOST_NOT_FOUND, WSANO_DATA); elsewhere Rust
/// keeps only the system's sentence, which may be in the person's
/// language, so a failed lookup there couldn't reach the host.
fn no_such_name(e: &io::Error) -> bool {
    cfg!(windows) && matches!(e.raw_os_error(), Some(11001 | 11004))
}

/// What [`PublicResolver`] answers for a name with no public address.
#[derive(Debug)]
struct NotPublic;

impl fmt::Display for NotPublic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the name has no public address")
    }
}

impl std::error::Error for NotPublic {}

/// Resolves names as `R` does, and keeps only the addresses [`is_public`]
/// allows, in order. With none left the request fails before it connects.
#[derive(Debug, Default)]
pub struct PublicResolver<R: Resolver = DefaultResolver> {
    inner: R,
}

impl<R: Resolver> PublicResolver<R> {
    pub fn new(inner: R) -> Self {
        Self { inner }
    }
}

impl<R: Resolver> Resolver for PublicResolver<R> {
    fn resolve(&self, uri: &Uri, config: &Config, timeout: NextTimeout) -> Result<ResolvedSocketAddrs, ureq::Error> {
        // The system's lookup fails as an io error, for a name that isn't
        // there and for a network that's down alike; ureq says
        // HostNotFound only when no address is left.
        let found = self.inner.resolve(uri, config, timeout).map_err(|e| match e {
            ureq::Error::Io(e) if no_such_name(&e) => ureq::Error::HostNotFound,
            e => e,
        })?;
        let mut public = self.empty();
        for addr in found.iter().filter(|addr| is_public(addr.ip())) {
            public.push(*addr);
        }
        match public.is_empty() {
            true => Err(ureq::Error::Io(io::Error::new(io::ErrorKind::PermissionDenied, NotPublic))),
            false => Ok(public),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, ErrorKind, Write};
    use std::net::{SocketAddr, TcpListener};
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::thread;

    use parking_lot::Mutex;

    use super::*;

    const UA: &str = "AutoScene/1.1.0 (plugin dev.autoscene.poster)";

    /// [`super::fetch`] for a caller that stays.
    fn fetch(req: NetRequest, hosts: &[String], user_agent: &str, t: &dyn Transport) -> PluginResult<NetResponse> {
        super::fetch(req, hosts, user_agent, t, &CancelToken::new())
    }

    fn hosts(names: &[&str]) -> Vec<String> {
        names.iter().map(|h| h.to_string()).collect()
    }

    fn get(url: &str) -> NetRequest {
        NetRequest {
            url: url.into(),
            method: None,
            headers: Vec::new(),
            body: None,
            body_base64: None,
            timeout_ms: None,
        }
    }

    /// An answer of `status` with `headers` and `body`.
    fn reply(status: u16, headers: &[(&str, &str)], body: impl Into<Vec<u8>>) -> PluginResult<Incoming> {
        let headers = headers.iter().map(|(name, value)| (name.to_string(), value.to_string())).collect();
        Ok(Incoming { status, headers, body: Box::new(Cursor::new(body.into())) })
    }

    /// A transport that answers each request as `answer` does, and keeps
    /// what it was sent.
    struct Fake<F> {
        answer: F,
        sent: Mutex<Vec<Outgoing>>,
    }

    fn fake<F: Fn(&Outgoing) -> PluginResult<Incoming> + Send + Sync>(answer: F) -> Fake<F> {
        Fake { answer, sent: Mutex::default() }
    }

    impl<F> Fake<F> {
        fn sent(&self) -> Vec<Outgoing> {
            self.sent.lock().clone()
        }
    }

    impl<F: Fn(&Outgoing) -> PluginResult<Incoming> + Send + Sync> Transport for Fake<F> {
        fn send(&self, o: &Outgoing, _: &CancelToken) -> PluginResult<Incoming> {
            self.sent.lock().push(o.clone());
            (self.answer)(o)
        }
    }

    fn json(_: &Outgoing) -> PluginResult<Incoming> {
        reply(200, &[("content-type", "application/json")], "{}")
    }

    #[test]
    fn only_named_hosts_are_reached() {
        let t = fake(|_| reply(200, &[("Content-Type", "application/json")], r#"{"ok":true}"#));
        let named = hosts(&["discord.com"]);
        let got = fetch(get("https://Discord.com/api/webhooks?wait=true#top"), &named, UA, &t).unwrap();
        assert_eq!(got.status, 200);
        assert_eq!(
            (got.url.as_str(), got.body.as_deref()),
            ("https://discord.com/api/webhooks?wait=true", Some(r#"{"ok":true}"#))
        );
        // As a GET, without its fragment, saying who asks.
        let sent = &t.sent()[0];
        assert_eq!((sent.method, sent.url.as_str()), ("GET", "https://discord.com/api/webhooks?wait=true"));
        assert_eq!(sent.headers, [("user-agent".to_owned(), UA.to_owned())]);
        assert!(sent.timeout <= DEFAULT_TIMEOUT && sent.timeout > DEFAULT_TIMEOUT - Duration::from_secs(5));

        // Another host, a subdomain, a parent, a trailing dot: never sent.
        for url in [
            "https://evil.example.org/",
            "https://cdn.discord.com/",
            "https://com/",
            "https://discord.com./",
            "https://discord.com.evil.org/",
        ] {
            let e = fetch(get(url), &named, UA, &t).unwrap_err();
            assert!(matches!(e, PluginError::Denied(_)), "{url}: {e:?}");
        }
        assert_eq!(
            fetch(get("https://evil.example.org/x"), &named, UA, &t).unwrap_err().to_string(),
            "net.fetch reaches only the hosts the plugin names, and evil.example.org isn't one."
        );
        // Without the network permission a plugin names none.
        assert!(matches!(fetch(get("https://discord.com/"), &[], UA, &t), Err(PluginError::Denied(_))));
        assert_eq!(t.sent().len(), 1);
        assert!(host_allowed("Discord.COM", &named));
        assert!(!host_allowed("discord.com.", &named) && !host_allowed("api.discord.com", &named));
    }

    #[test]
    fn a_redirect_to_another_host_is_refused() {
        let t = fake(|o: &Outgoing| match o.url.as_str() {
            "https://discord.com/a" => {
                thread::sleep(Duration::from_millis(5));
                reply(302, &[("Location", "/b")], "")
            }
            "https://discord.com/b" => reply(301, &[("location", "https://cdn.discordapp.com/c#here")], ""),
            "https://cdn.discordapp.com/c" => reply(200, &[("content-type", "text/plain")], "here"),
            "https://discord.com/away" => reply(307, &[("location", "https://evil.example.org/")], ""),
            "https://discord.com/plain" => reply(308, &[("location", "http://discord.com/")], ""),
            "https://discord.com/loop" => reply(302, &[("location", "https://discord.com/loop")], ""),
            "https://discord.com/stay" => reply(302, &[], "moved, but not where"),
            "https://discord.com/see" => reply(303, &[("location", "/there")], ""),
            "https://discord.com/again" => reply(307, &[("location", "/there")], ""),
            "https://discord.com/there" => reply(200, &[], ""),
            _ => reply(404, &[], ""),
        });
        let named = hosts(&["discord.com", "cdn.discordapp.com"]);
        let body_headers = [
            ("Content-Type", "text/plain"),
            ("Content-Encoding", "gzip"),
            ("Content-Language", "de"),
            ("Content-Location", "/hi"),
        ];
        let headers = [("Authorization", "Bot x"), ("Cookie", "session=1")].iter().chain(&body_headers);
        let post = NetRequest {
            method: Some("post".into()),
            body: Some("hi".into()),
            headers: headers.map(|(name, value)| (name.to_string(), value.to_string())).collect(),
            ..get("https://discord.com/a")
        };
        // Followed to a host it names; the answer says from where.
        let got = fetch(post.clone(), &named, UA, &t).unwrap();
        assert_eq!((got.url.as_str(), got.body.as_deref()), ("https://cdn.discordapp.com/c", Some("here")));
        let sent = t.sent();
        let names = |o: &Outgoing| o.headers.iter().map(|(name, _)| name.clone()).collect::<Vec<_>>();
        assert_eq!((sent[0].method, sent[0].body.as_slice(), names(&sent[0]).len()), ("POST", b"hi".as_slice(), 7));
        // A 302 after a POST asks for a GET without the body, nor what says
        // what it is.
        assert_eq!((sent[1].method, sent[1].body.len()), ("GET", 0));
        assert_eq!(names(&sent[1]), ["Authorization", "Cookie", "user-agent"]);
        // What signs the plugin in stays with its host.
        assert_eq!(names(&sent[2]), ["user-agent"]);
        // Each in what is left of the call's time.
        assert!(sent[1].timeout + Duration::from_millis(5) <= sent[0].timeout, "{sent:?}");
        assert!(sent[2].timeout <= sent[1].timeout);

        // A 303 asks for a GET, but a HEAD stays one; a 307 keeps the method
        // and the body.
        for (method, at, then, body, headers) in [
            ("HEAD", "/see", "HEAD", 0, 7),
            ("PUT", "/see", "GET", 0, 3),
            ("PATCH", "/see", "GET", 0, 3),
            ("POST", "/again", "POST", 2, 7),
            ("DELETE", "/again", "DELETE", 2, 7),
        ] {
            let req = NetRequest {
                url: format!("https://discord.com{at}"),
                method: Some(method.into()),
                body: (method != "HEAD").then(|| "hi".to_owned()),
                ..post.clone()
            };
            fetch(req, &named, UA, &t).unwrap();
            let next = t.sent().pop().unwrap();
            assert_eq!((next.method, next.body.len(), names(&next).len()), (then, body, headers), "{method} {at}");
        }

        // To a host it doesn't name, or off https: refused, never sent.
        for (url, why) in [
            (
                "https://discord.com/away",
                "net.fetch reaches only the hosts the plugin names, and evil.example.org isn't one.",
            ),
            ("https://discord.com/plain", "net.fetch reaches only https:// addresses."),
        ] {
            let before = t.sent().len();
            let e = fetch(get(url), &named, UA, &t).unwrap_err();
            assert!(matches!(&e, PluginError::Denied(_)), "{e:?}");
            assert_eq!((e.to_string().as_str(), t.sent().len()), (why, before + 1));
        }
        // Five at most.
        let before = t.sent().len();
        let e = fetch(get("https://discord.com/loop"), &named, UA, &t).unwrap_err();
        assert_eq!((e.to_string().as_str(), t.sent().len()), ("discord.com redirected more than 5 times.", before + 6));
        // One that says nowhere is the answer.
        let got = fetch(get("https://discord.com/stay"), &named, UA, &t).unwrap();
        assert_eq!((got.status, got.url.as_str()), (302, "https://discord.com/stay"));
    }

    #[test]
    fn only_https_is_reached() {
        let t = fake(json);
        let named = hosts(&["discord.com"]);
        for (url, why) in [
            ("http://discord.com/", "net.fetch reaches only https:// addresses."),
            ("wss://discord.com/", "net.fetch reaches only https:// addresses."),
            ("ftp://discord.com/", "net.fetch reaches only https:// addresses."),
            ("https://discord.com:8443/", "net.fetch reaches only port 443."),
            ("https://user:secret@discord.com/", "net.fetch sends no user name or password in the address."),
            ("https://user@discord.com/", "net.fetch sends no user name or password in the address."),
        ] {
            let e = fetch(get(url), &named, UA, &t).unwrap_err();
            assert!(matches!(&e, PluginError::Denied(_)), "{url}: {e:?}");
            assert_eq!(e.to_string(), why, "{url}");
        }
        assert!(matches!(fetch(get("discord.com/api"), &named, UA, &t), Err(PluginError::Invalid(_))));
        let long = format!("https://discord.com/{}", "a".repeat(MAX_URL_BYTES));
        assert_eq!(fetch(get(&long), &named, UA, &t).unwrap_err().to_string(), "net.fetch's url is longer than 8 KB.");
        assert_eq!(t.sent().len(), 0);
        // Port 443 is https's own.
        fetch(get("https://discord.com:443/x"), &named, UA, &t).unwrap();
        assert_eq!(t.sent()[0].url, "https://discord.com/x");
    }

    #[test]
    fn ip_literal_hosts_are_refused() {
        let t = fake(json);
        // network.hosts takes these as names; net.fetch reads them as the
        // addresses they are.
        let named = hosts(&["127.0.0.1", "0x7f.1", "2130706433", "127.1", "localhost"]);
        for url in [
            "https://127.0.0.1/",
            "https://[::1]/",
            "https://[::ffff:127.0.0.1]/",
            "https://0x7f.1/",
            "https://2130706433/",
            "https://127.1/",
            "https://8.8.8.8/",
        ] {
            let e = fetch(get(url), &named, UA, &t).unwrap_err();
            assert!(matches!(&e, PluginError::Denied(_)), "{url}: {e:?}");
            assert_eq!(e.to_string(), "net.fetch reaches host names, not addresses such as 127.0.0.1.", "{url}");
        }
        assert_eq!(t.sent().len(), 0);
        for host in ["127.0.0.1", "0x7f.1", "0X7F.0x1", "2130706433", "127.1", "[::1]", "::1", "a.b.123"] {
            assert!(!host_allowed(host, &hosts(&[host])), "{host}");
        }
        // A name stays a name, also one for this computer: the address
        // check is the transport's (a_name_that_resolves_to_a_private_address_is_refused).
        assert!(fetch(get("https://localhost/"), &named, UA, &t).is_ok());
    }

    #[test]
    fn private_addresses_are_never_connected() {
        let private = [
            "0.0.0.0",
            "0.1.2.3",
            "10.0.0.1",
            "127.0.0.1",
            "127.255.255.254",
            "100.64.0.1",
            "100.127.255.255",
            "169.254.169.254",
            "172.16.0.1",
            "172.31.255.255",
            "192.0.0.8",
            "192.0.2.1",
            "192.168.1.1",
            "198.18.0.1",
            "198.19.255.255",
            "198.51.100.7",
            "203.0.113.9",
            "224.0.0.251",
            "239.255.255.250",
            "240.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "::127.0.0.1",
            "::8.8.8.8",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "::ffff:169.254.169.254",
            "::ffff:0:8.8.8.8",
            "64:ff9b::10.0.0.1",
            "64:ff9b::7f00:1",
            "64:ff9b:1::808:808",
            "100::1",
            "2001::1",
            "2001:0:4136:e378:8000:63bf:3fff:fdd2",
            "2001:2::1",
            "2001:1ff:ffff::1",
            "2001:db8::1",
            "2001:db8:ffff::1",
            "2002::1",
            "2002:7f00:1::",
            "2002:a00:1::1",
            "2002:c0a8:101::1",
            "2002:a9fe:a9fe::1",
            "3fff::1",
            "3fff:fff:ffff::1",
            "fc00::1",
            "fd12:3456:789a::1",
            "fe80::1",
            "febf::1",
            "fec0::1",
            "ff02::1",
            "ff0e::1",
        ];
        let public = [
            "1.1.1.1",
            "8.8.8.8",
            "9.255.255.255",
            "11.0.0.0",
            "100.63.255.255",
            "100.128.0.1",
            "126.255.255.255",
            "128.0.0.1",
            "162.159.135.232",
            "169.253.1.1",
            "172.15.255.255",
            "172.32.0.1",
            "192.0.1.1",
            "192.0.3.1",
            "192.169.0.1",
            "198.17.255.255",
            "198.20.0.1",
            "203.0.114.1",
            "223.255.255.255",
            "2606:4700:4700::1111",
            "2a00:1450:4001:82a::200e",
            "::ffff:8.8.8.8",
            "64:ff9b::8.8.8.8",
            "2001:200::1",
            "2001:4860:4860::8888",
            "2002:808:808::1",
            "3fff:1000::1",
        ];
        for ip in private {
            assert!(!is_public(ip.parse().unwrap()), "{ip}");
        }
        for ip in public {
            assert!(is_public(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn forbidden_headers_are_refused() {
        let t = fake(json);
        let named = hosts(&["discord.com"]);
        let with = |headers: Vec<(&str, String)>| {
            let headers = headers.into_iter().map(|(name, value)| (name.to_owned(), value)).collect();
            fetch(NetRequest { headers, ..get("https://discord.com/") }, &named, UA, &t)
        };
        let refused = |headers: Vec<(&str, String)>| match with(headers) {
            Err(PluginError::Invalid(why)) => why,
            other => panic!("{other:?}"),
        };
        for name in [
            "Host",
            "connection",
            "Content-Length",
            "transfer-encoding",
            "TE",
            "upgrade",
            "Expect",
            "keep-alive",
            "trailer",
            "User-Agent",
            "Proxy-Authorization",
            "proxy-connection",
        ] {
            let why = refused(vec![(name, "x".into())]);
            assert_eq!(why, format!("net.fetch doesn't send a plugin's {} header.", name.to_ascii_lowercase()));
        }
        // Names are tokens; values have no line breaks.
        for (name, value) in [("x y", "1"), ("", "1"), ("x:y", "1"), ("x-ü", "1")] {
            assert!(refused(vec![(name, value.into())]).ends_with("isn't a header name."), "{name}");
        }
        for value in ["1\r\nhost: evil.example.org", "a\nb", "nul\0", "del\x7f"] {
            let why = refused(vec![("x-a", value.into())]);
            assert_eq!(why, "The x-a header has a line break or another control character in it.");
        }
        // ureq would refuse it after the handshake, and blame the host.
        assert_eq!(
            refused(vec![("Authorization", "Bot grüße".into())]),
            "The authorization header has a character in it that isn't ASCII."
        );
        // 32 of them at most, in 8 KB.
        let many = |n: usize| (0..n).map(|i| ("x-n", i.to_string())).collect::<Vec<_>>();
        assert_eq!(refused(many(33)), "net.fetch sends at most 32 headers.");
        assert!(with(many(32)).is_ok());
        assert_eq!(refused(vec![("x-big", "a".repeat(8 << 10))]), "The headers of a net.fetch are larger than 8 KB.");
        // Others go as they are, beside AutoScene's user-agent.
        let own = vec![("Authorization", "Bot x".to_owned()), ("X-Note", "grüße\tgut".to_owned())];
        with(own.clone()).unwrap();
        let mut sent = own.into_iter().map(|(name, value)| (name.to_owned(), value)).collect::<Vec<_>>();
        sent.push(("user-agent".into(), UA.into()));
        assert_eq!(t.sent().last().unwrap().headers, sent);
        assert_eq!(t.sent().len(), 2);
    }

    #[test]
    fn a_body_past_its_cap_is_refused() {
        let t = fake(json);
        let named = hosts(&["discord.com"]);
        let send = |method: &str, body: Option<String>, body_base64: Option<String>| {
            let req = NetRequest { method: Some(method.into()), body, body_base64, ..get("https://discord.com/") };
            fetch(req, &named, UA, &t)
        };
        let refused = |r: PluginResult<NetResponse>| match r {
            Err(PluginError::Invalid(why)) => why,
            other => panic!("{other:?}"),
        };
        let larger = "The body of a net.fetch is larger than 4 MB.";
        send("POST", Some("a".repeat(MAX_REQUEST_BODY)), None).unwrap();
        assert_eq!(refused(send("POST", Some("a".repeat(MAX_REQUEST_BODY + 1)), None)), larger);
        let bytes = vec![0xff_u8; MAX_REQUEST_BODY];
        send("PUT", None, Some(STANDARD.encode(&bytes))).unwrap();
        assert_eq!(t.sent().last().unwrap().body, bytes);
        assert_eq!(refused(send("PUT", None, Some(STANDARD.encode([&bytes[..], b"!"].concat())))), larger);
        assert_eq!(
            refused(send("POST", Some("a".into()), Some("YQ==".into()))),
            "A net.fetch sends body or bodyBase64, not both."
        );
        assert_eq!(refused(send("POST", None, Some("not base64!".into()))), "net.fetch's bodyBase64 isn't base64.");
        assert_eq!(refused(send("GET", Some("a".into()), None)), "A GET or HEAD request has no body.");
        assert_eq!(
            refused(send("TRACE", None, None)),
            "net.fetch sends GET, HEAD, POST, PUT, PATCH or DELETE, not TRACE."
        );
        // Each method in any case; an empty body is none.
        for method in METHODS {
            send(&method.to_ascii_lowercase(), Some(String::new()), None).unwrap();
            assert_eq!(t.sent().last().unwrap().method, method);
        }
        // Up to 2 minutes, redirects and all.
        let wait = |ms: u32| fetch(NetRequest { timeout_ms: Some(ms), ..get("https://discord.com/") }, &named, UA, &t);
        assert_eq!(refused(wait(0)), "net.fetch's timeoutMs must be from 1 to 120000.");
        assert_eq!(refused(wait(120_001)), "net.fetch's timeoutMs must be from 1 to 120000.");
        wait(120_000).unwrap();
        assert!(t.sent().last().unwrap().timeout > Duration::from_secs(115));
        assert_eq!(t.sent().len(), 2 + METHODS.len() + 1);
    }

    /// Bytes without end, counting what was read of them.
    struct Endless(Arc<AtomicU64>);

    impl Read for Endless {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            buf.fill(b'a');
            self.0.fetch_add(buf.len() as u64, Ordering::Relaxed);
            Ok(buf.len())
        }
    }

    #[test]
    fn a_response_past_its_cap_fails_without_reading_it_all() {
        let read = Arc::new(AtomicU64::new(0));
        let length = Mutex::new(None::<String>);
        let t = fake(|_| {
            let mut headers = vec![("content-type".to_owned(), "text/plain".to_owned())];
            headers.extend(length.lock().clone().map(|l| ("content-length".to_owned(), l)));
            Ok(Incoming { status: 200, headers, body: Box::new(Endless(read.clone())) })
        });
        let named = hosts(&["discord.com"]);
        let e = fetch(get("https://discord.com/"), &named, UA, &t).unwrap_err();
        assert!(matches!(&e, PluginError::Invalid(_)), "{e:?}");
        assert_eq!(e.to_string(), "The answer is larger than 8 MB.");
        assert_eq!(read.load(Ordering::Relaxed), MAX_RESPONSE_BODY + 1);
        // One that says how long it is isn't read at all; for a HEAD, its
        // length is that of a body it doesn't send.
        read.store(0, Ordering::Relaxed);
        *length.lock() = Some((MAX_RESPONSE_BODY + 1).to_string());
        let e = fetch(get("https://discord.com/"), &named, UA, &t).unwrap_err();
        assert_eq!((e.to_string().as_str(), read.load(Ordering::Relaxed)), ("The answer is larger than 8 MB.", 0));
        let head = NetRequest { method: Some("HEAD".into()), ..get("https://discord.com/") };
        let t = fake(|_| reply(200, &[("content-length", "9000000")], ""));
        assert_eq!(fetch(head, &named, UA, &t).unwrap().body_base64.as_deref(), Some(""));
        // Nor for an answer that has no body: a 304 may name the length of
        // the file it didn't send again.
        for status in [204, 304] {
            let t = fake(move |_| reply(status, &[("content-length", "9000000")], ""));
            assert_eq!(fetch(get("https://discord.com/"), &named, UA, &t).unwrap().status, status);
        }
        // 8 MB is fine.
        let t = fake(|_| reply(200, &[], vec![7; MAX_RESPONSE_BODY as usize]));
        let got = fetch(get("https://discord.com/"), &named, UA, &t).unwrap();
        assert_eq!(STANDARD.decode(got.body_base64.unwrap()).unwrap().len() as u64, MAX_RESPONSE_BODY);
    }

    #[test]
    fn text_bodies_come_as_text_and_others_as_base64() {
        let t = fake(|o: &Outgoing| {
            let (kind, body): (&str, &[u8]) = match o.url.rsplit('/').next().unwrap() {
                "json" => ("application/json; charset=utf-8", br#"{"a":1}"#),
                "ld" => ("Application/LD+JSON", b"{}"),
                "atom" => ("application/atom+xml", b"<feed/>"),
                "text" => ("text/plain", "grüße".as_bytes()),
                "lines" => ("text/plain", b"a\tb\r\nc"),
                "ansi" => ("text/plain", b"\x1b[1mbold\x1b[0m"),
                "png" => ("image/png", b"\x89PNG\r\n"),
                "latin1" => ("text/plain; charset=iso-8859-1", b"gr\xfc\xdfe"),
                "plain" => ("application/octet-stream", b"plain words"),
                _ => ("", b"no type"),
            };
            reply(200, &[("Content-Type", kind), ("X-Request-Id", "7")], body)
        });
        let named = hosts(&["discord.com"]);
        let got = |path: &str| fetch(get(&format!("https://discord.com/{path}")), &named, UA, &t).unwrap();
        for (path, text) in
            [("json", r#"{"a":1}"#), ("ld", "{}"), ("atom", "<feed/>"), ("text", "grüße"), ("lines", "a\tb\r\nc")]
        {
            let got = got(path);
            assert_eq!((got.body.as_deref(), got.body_base64), (Some(text), None), "{path}");
        }
        for (path, bytes) in [
            ("png", b"\x89PNG\r\n".as_slice()),
            ("latin1", b"gr\xfc\xdfe"),
            ("plain", b"plain words"),
            ("none", b"no type"),
            // Text, but its escape characters would take six bytes each.
            ("ansi", b"\x1b[1mbold\x1b[0m"),
        ] {
            let got = got(path);
            assert_eq!(got.body, None, "{path}");
            assert_eq!(STANDARD.decode(got.body_base64.unwrap()).unwrap(), bytes, "{path}");
        }
        // Header names come lowercase, their values as they are.
        let headers = got("json").headers;
        assert_eq!(headers[0], ("content-type".to_owned(), "application/json; charset=utf-8".to_owned()));
        assert_eq!(headers[1], ("x-request-id".to_owned(), "7".to_owned()));
    }

    #[test]
    fn an_answer_as_json_is_never_more_than_twice_its_size() {
        // What the run holds for a plugin that doesn't read its replies.
        let named = hosts(&["discord.com"]);
        for byte in [b'"', b'\\', 0x01, 0x1b, b'\n', 0xff] {
            let t =
                fake(move |_| reply(200, &[("content-type", "text/plain")], vec![byte; MAX_RESPONSE_BODY as usize]));
            let got = fetch(get("https://discord.com/"), &named, UA, &t).unwrap();
            let line = serde_json::to_string(&got).unwrap();
            assert!(line.len() as u64 <= 2 * MAX_RESPONSE_BODY + 1024, "{byte:#x}: {}", line.len());
        }
    }

    #[test]
    #[ignore = "reaches example.com"]
    fn fetches_a_real_page() {
        let t = UreqTransport::new();
        let got = fetch(get("https://example.com/"), &hosts(&["example.com"]), UA, &t).unwrap();
        assert_eq!((got.status, got.url.as_str()), (200, "https://example.com/"));
        assert!(got.body.unwrap().contains("Example Domain"));
    }

    #[test]
    fn public_names_are_told_apart() {
        for host in [
            "discord.com",
            "api.example.org",
            "xn--bcher-kva.de",
            "a.b.co.uk",
            "Discord.COM",
            "my-host.io",
            "a.xn--p1ai",
            "a.XN--80ASEHDB",
        ] {
            assert!(public_name(host), "{host}");
        }
        for host in [
            "localhost",
            "x.localhost",
            "com",
            "printer.local",
            "nas.lan",
            "db.internal",
            "box.localdomain",
            "a.test",
            "a.invalid",
            "www.example",
            "router.home.arpa",
            "home.arpa",
            "127.0.0.1",
            "0x7f.1",
            "1.2.3",
            "a.b1",
            "a.xn--",
            "a.xn--p1ai.",
            "a.xn--p_1",
            "a..com",
            ".com",
            "a.com.",
            "",
        ] {
            assert!(!public_name(host), "{host}");
        }
    }

    /// A resolver that answers names as `answers` says (`*` for any), each
    /// at `port`, and keeps the names it was asked. With no answer the name
    /// isn't found, as ureq's own resolver says of a lookup with no address
    /// (a_failed_lookup_says_not_found_only_when_the_system_says_so has the
    /// system's ways).
    #[derive(Clone, Debug)]
    struct Answers {
        answers: Vec<(&'static str, IpAddr)>,
        port: u16,
        asked: Arc<Mutex<Vec<String>>>,
    }

    impl Answers {
        fn of(answers: &[(&'static str, &str)], port: u16) -> Self {
            let answers = answers.iter().map(|(name, ip)| (*name, ip.parse().unwrap())).collect();
            Self { answers, port, asked: Arc::default() }
        }

        fn none() -> Self {
            Self::of(&[], 443)
        }
    }

    impl Resolver for Answers {
        fn resolve(&self, uri: &Uri, _: &Config, _: NextTimeout) -> Result<ResolvedSocketAddrs, ureq::Error> {
            let name = uri.host().unwrap_or_default();
            self.asked.lock().push(name.to_owned());
            let mut found = self.empty();
            for (_, ip) in self.answers.iter().filter(|(n, _)| *n == "*" || *n == name) {
                found.push(SocketAddr::new(*ip, self.port));
            }
            match found.is_empty() {
                true => Err(ureq::Error::HostNotFound),
                false => Ok(found),
            }
        }
    }

    /// A resolver whose lookup fails with what it gives.
    #[derive(Debug)]
    struct Fails(fn() -> io::Error);

    impl Resolver for Fails {
        fn resolve(&self, _: &Uri, _: &Config, _: NextTimeout) -> Result<ResolvedSocketAddrs, ureq::Error> {
            Err(ureq::Error::Io((self.0)()))
        }
    }

    #[test]
    fn a_failed_lookup_says_not_found_only_when_the_system_says_so() {
        let uri: Uri = "https://discord.com/".parse().unwrap();
        let said = |error: fn() -> io::Error| {
            let e = PublicResolver::new(Fails(error)).resolve(&uri, &Config::default(), NextTimeout::default());
            not_reached(e.unwrap_err(), "discord.com").to_string()
        };
        // Windows tells a name that isn't there from the rest:
        // WSAHOST_NOT_FOUND, WSANO_DATA.
        let missing = match cfg!(windows) {
            true => "discord.com wasn't found.",
            false => "Couldn't reach discord.com.",
        };
        assert_eq!(said(|| io::Error::from_raw_os_error(11001)), missing);
        assert_eq!(said(|| io::Error::from_raw_os_error(11004)), missing);
        // Anything else may be the network: WSATRY_AGAIN, and the sentence
        // Rust keeps of a lookup that failed elsewhere.
        assert_eq!(said(|| io::Error::from_raw_os_error(11002)), "Couldn't reach discord.com.");
        let offline = || io::Error::other("failed to lookup address information: Temporary failure in name resolution");
        assert_eq!(said(offline), "Couldn't reach discord.com.");
        // No address at all is ureq's own word for it.
        assert_eq!(not_reached(ureq::Error::HostNotFound, "discord.com").to_string(), "discord.com wasn't found.");
    }

    /// Whether nothing connected to `listener`.
    fn nobody_called(listener: &TcpListener) -> bool {
        listener.set_nonblocking(true).unwrap();
        listener.accept().map(|_| ()).is_err_and(|e| e.kind() == ErrorKind::WouldBlock)
    }

    #[test]
    fn a_name_that_resolves_to_a_private_address_is_refused() {
        // Something listens where the answers lead, on IPv6 too when this
        // computer has it.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let listener_v6 = TcpListener::bind(("::1", port)).ok();
        let named = hosts(&["discord.com"]);
        for ip in ["127.0.0.1", "10.0.0.1", "::1", "::ffff:127.0.0.1"] {
            let t = UreqTransport::with_resolver(Answers::of(&[("*", ip)], port));
            let e = fetch(get("https://discord.com/"), &named, UA, &t).unwrap_err();
            assert!(matches!(&e, PluginError::Denied(_)), "{ip}: {e:?}");
            assert_eq!(e.to_string(), "discord.com resolves to an address on this computer or its network.");
        }
        assert!(nobody_called(&listener));
        assert!(listener_v6.as_ref().is_none_or(nobody_called));
    }

    #[test]
    fn a_mixed_answer_keeps_only_public_addresses() {
        let uri: Uri = "https://discord.com/api".parse().unwrap();
        let resolve = |answers: &[&'static str]| {
            let answers: Vec<(&'static str, &str)> = answers.iter().map(|ip| ("*", *ip)).collect();
            PublicResolver::new(Answers::of(&answers, 443)).resolve(&uri, &Config::default(), NextTimeout::default())
        };
        let found = resolve(&["10.0.0.1", "1.1.1.1", "::1", "2606:4700:4700::1111", "192.168.1.1", "8.8.8.8"]).unwrap();
        let found: Vec<String> = found.iter().map(SocketAddr::to_string).collect();
        assert_eq!(found, ["1.1.1.1:443", "[2606:4700:4700::1111]:443", "8.8.8.8:443"]);
        // None public: the request ends there, and says why.
        let e = not_reached(resolve(&["10.0.0.1", "fe80::1"]).unwrap_err(), "discord.com");
        assert!(matches!(&e, PluginError::Denied(_)), "{e:?}");
        // A name not found isn't refused: it just isn't there.
        let e = not_reached(resolve(&[]).unwrap_err(), "discord.com");
        assert!(matches!(&e, PluginError::Failed { .. }), "{e:?}");
        assert_eq!(e.to_string(), "discord.com wasn't found.");
    }

    #[test]
    fn the_system_proxy_is_never_used() {
        let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
        let at = format!("http://{}", proxy.local_addr().unwrap());
        // In a process of its own, whose environment names the proxy
        // before anything reads it.
        let out = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "net::tests::a_request_from_behind_a_proxy", "--ignored", "--nocapture"])
            .envs([("AUTOSCENE_TEST_PROXY", &at), ("HTTPS_PROXY", &at), ("ALL_PROXY", &at)])
            .env_remove("NO_PROXY")
            .output()
            .unwrap();
        let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(out.status.success() && said.contains("1 passed"), "{said}");
        assert!(nobody_called(&proxy));
    }

    #[test]
    fn ureq_leaves_redirects_and_statuses_to_fetch() {
        // A redirect ureq followed itself would go past host_allowed, and a
        // 404 it failed would never reach the plugin as an answer.
        let t = UreqTransport::new();
        assert_eq!(t.config.max_redirects(), 0);
        assert!(!t.config.http_status_as_error());
        assert!(t.config.https_only());
    }

    /// The other half of [`the_system_proxy_is_never_used`], which runs it.
    #[test]
    #[ignore = "run by the_system_proxy_is_never_used, with a proxy named"]
    fn a_request_from_behind_a_proxy() {
        let Ok(proxy) = std::env::var("AUTOSCENE_TEST_PROXY") else { return };
        assert_eq!(std::env::var("HTTPS_PROXY").as_deref(), Ok(proxy.as_str()));
        assert!(Config::default().proxy().is_some(), "ureq reads the proxy from here");
        let names = Answers::none();
        let t = UreqTransport::with_resolver(names.clone());
        assert!(t.config.proxy().is_none());
        let e = fetch(get("https://discord.com/"), &hosts(&["discord.com"]), UA, &t).unwrap_err();
        assert_eq!(e.to_string(), "discord.com wasn't found.");
        // It looked the host up itself; through a proxy it would have
        // looked up the proxy.
        assert_eq!(*names.asked.lock(), ["discord.com"]);
    }

    /// A transport that looks up each request's host as [`UreqTransport`]
    /// does, then answers as `answer` does.
    struct Resolving<F> {
        names: PublicResolver<Answers>,
        answer: F,
    }

    impl<F: Fn(&Outgoing) -> PluginResult<Incoming> + Send + Sync> Transport for Resolving<F> {
        fn send(&self, o: &Outgoing, _: &CancelToken) -> PluginResult<Incoming> {
            let uri: Uri = o.url.parse().unwrap();
            let host = uri.host().unwrap().to_owned();
            let found = self.names.resolve(&uri, &Config::default(), NextTimeout::default());
            found.map_err(|e| not_reached(e, &host))?;
            (self.answer)(o)
        }
    }

    #[test]
    fn a_redirect_is_resolved_again() {
        let names = Answers::of(&[("discord.com", "162.159.135.232"), ("cdn.discordapp.com", "10.0.0.1")], 443);
        let t = Resolving {
            names: PublicResolver::new(names.clone()),
            answer: |_: &Outgoing| reply(302, &[("location", "https://cdn.discordapp.com/x")], ""),
        };
        let named = hosts(&["discord.com", "cdn.discordapp.com"]);
        let e = fetch(get("https://discord.com/"), &named, UA, &t).unwrap_err();
        assert!(matches!(&e, PluginError::Denied(_)), "{e:?}");
        assert_eq!(e.to_string(), "cdn.discordapp.com resolves to an address on this computer or its network.");
        assert_eq!(*names.asked.lock(), ["discord.com", "cdn.discordapp.com"]);
    }

    /// What `f` gives, on a thread of its own: a test fails rather than
    /// waits past `secs`.
    fn within<T: Send + 'static>(secs: u64, f: impl FnOnce() -> T + Send + 'static) -> T {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || tx.send(f()));
        rx.recv_timeout(Duration::from_secs(secs)).expect("still waiting")
    }

    /// A transport to whatever listens at `port` on this computer.
    fn to_this_computer(port: u16) -> UreqTransport {
        UreqTransport::unchecked(Answers::of(&[("*", "127.0.0.1")], port))
    }

    /// Bytes without end, that end their call once `after` of them are read.
    struct EndsAfter {
        after: u64,
        read: Arc<AtomicU64>,
        ended: CancelToken,
    }

    impl Read for EndsAfter {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            buf.fill(b'a');
            if self.read.fetch_add(buf.len() as u64, Ordering::Relaxed) + buf.len() as u64 >= self.after {
                self.ended.cancel();
            }
            Ok(buf.len())
        }
    }

    #[test]
    fn a_call_stops_once_its_caller_is_gone() {
        let named = hosts(&["discord.com"]);
        let post = NetRequest {
            method: Some("POST".into()),
            body: Some("hi".into()),
            timeout_ms: Some(60_000),
            ..get("https://discord.com/")
        };
        let cancelled = |r: PluginResult<NetResponse>| matches!(r, Err(PluginError::Cancelled));
        // Ended before it starts: nothing is sent.
        let t = fake(json);
        let ended = CancelToken::new();
        ended.cancel();
        assert!(cancelled(super::fetch(post.clone(), &named, UA, &t, &ended)));
        assert_eq!(t.sent().len(), 0);

        // Ended while it waits: whatever its transport says then, it ended.
        let ended = CancelToken::new();
        let t = fake({
            let ended = ended.clone();
            move |_| {
                let until = Instant::now() + Duration::from_secs(10);
                while !ended.is_cancelled() && Instant::now() < until {
                    thread::sleep(Duration::from_millis(1));
                }
                Err(failed("Couldn't reach discord.com.".into(), None))
            }
        });
        let ending = ended.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            ending.cancel();
        });
        assert!(cancelled(super::fetch(post.clone(), &named, UA, &t, &ended)));

        // Ended while its answer is read: no more of it is.
        let (ended, read) = (CancelToken::new(), Arc::new(AtomicU64::new(0)));
        let t = fake(|_| {
            let body = EndsAfter { after: 64 << 10, read: read.clone(), ended: ended.clone() };
            Ok(Incoming { status: 200, headers: Vec::new(), body: Box::new(body) })
        });
        assert!(cancelled(super::fetch(get("https://discord.com/"), &named, UA, &t, &ended)));
        assert!(read.load(Ordering::Relaxed) < 1 << 20, "{read:?}");

        // Through ureq, to a host that never answers: the request stops
        // where it is (here, in the TLS handshake), not at its timeout.
        let silent = TcpListener::bind("127.0.0.1:0").unwrap();
        let t = to_this_computer(silent.local_addr().unwrap().port());
        let ended = CancelToken::new();
        let ending = ended.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(200));
            ending.cancel();
        });
        let started = Instant::now();
        let got = within(20, move || super::fetch(post, &named, UA, &t, &ended));
        assert!(cancelled(got), "{:?}", started.elapsed());
        assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
        assert!(!nobody_called(&silent));
    }

    /// A lookup that outlasts its call: it answers `127.0.0.1` at `port`
    /// only once `ended` fired and the test says `go`.
    #[derive(Debug)]
    struct Late {
        ended: CancelToken,
        go: Mutex<mpsc::Receiver<()>>,
        port: u16,
    }

    impl Resolver for Late {
        fn resolve(&self, _: &Uri, _: &Config, _: NextTimeout) -> Result<ResolvedSocketAddrs, ureq::Error> {
            while !self.ended.is_cancelled() {
                thread::sleep(Duration::from_millis(5));
            }
            self.go.lock().recv_timeout(Duration::from_secs(10)).ok();
            let mut found = self.empty();
            found.push(SocketAddr::from((Ipv4Addr::LOCALHOST, self.port)));
            Ok(found)
        }
    }

    #[test]
    fn a_call_ended_in_its_lookup_is_back_at_once_and_its_connection_sends_nothing() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (ended, (go, wait)) = (CancelToken::new(), mpsc::channel());
        let t = UreqTransport::unchecked(Late { ended: ended.clone(), go: Mutex::new(wait), port });
        let ending = ended.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            ending.cancel();
        });
        let post = NetRequest {
            method: Some("POST".into()),
            body: Some("hi".into()),
            timeout_ms: Some(60_000),
            ..get("https://discord.com/")
        };
        // Back while its lookup still waits, which answers only after this
        // (or in 10 s).
        let started = Instant::now();
        let got = within(20, move || super::fetch(post, &hosts(&["discord.com"]), UA, &t, &ended));
        assert!(matches!(got, Err(PluginError::Cancelled)), "{got:?}");
        assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
        // The connection it makes then carries nothing, not even the start
        // of the TLS handshake.
        go.send(()).unwrap();
        let heard = within(20, move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut heard = Vec::new();
            let _ = s.read_to_end(&mut heard);
            heard.len()
        });
        assert_eq!(heard, 0);
    }

    #[test]
    fn a_call_runs_out_of_its_time_and_says_why() {
        let named = hosts(&["discord.com"]);
        // Its time counts across redirects.
        let t = fake(|o: &Outgoing| match o.url.as_str() {
            "https://discord.com/slow" => {
                thread::sleep(Duration::from_millis(30));
                reply(302, &[("location", "/b")], "")
            }
            _ => json(o),
        });
        let slow = NetRequest { timeout_ms: Some(20), ..get("https://discord.com/slow") };
        let e = fetch(slow, &named, UA, &t).unwrap_err();
        assert_eq!((e.to_string().as_str(), t.sent().len()), ("discord.com didn't answer in time.", 1));

        // ureq gets it: a host that never answers runs out of it.
        let silent = TcpListener::bind("127.0.0.1:0").unwrap();
        let t = to_this_computer(silent.local_addr().unwrap().port());
        let quick = NetRequest { timeout_ms: Some(300), ..get("https://discord.com/") };
        let named_too = named.clone();
        let e = within(20, move || fetch(quick, &named_too, UA, &t)).unwrap_err();
        assert!(matches!(&e, PluginError::Failed { .. }), "{e:?}");
        assert_eq!(e.to_string(), "discord.com didn't answer in time.");

        // A body that stops coming, as ureq says so, and one that breaks off.
        let t = fake(|o: &Outgoing| {
            let e = match o.url.ends_with("/slow") {
                true => ureq::Error::Timeout(ureq::Timeout::Global).into_io(),
                false => io::Error::from(ErrorKind::ConnectionReset),
            };
            Ok(Incoming { status: 200, headers: Vec::new(), body: Box::new(FailsWith(Some(e))) })
        });
        let e = fetch(get("https://discord.com/slow"), &named, UA, &t).unwrap_err();
        assert_eq!(e.to_string(), "discord.com didn't answer in time.");
        let e = fetch(get("https://discord.com/reset"), &named, UA, &t).unwrap_err();
        assert_eq!(e.to_string(), "The answer from discord.com broke off.");
    }

    /// A body that fails with its error.
    struct FailsWith(Option<io::Error>);

    impl Read for FailsWith {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(self.0.take().unwrap_or_else(|| ErrorKind::Other.into()))
        }
    }

    #[test]
    fn a_host_that_does_not_speak_tls_fails_the_secure_connection() {
        let plain = TcpListener::bind("127.0.0.1:0").unwrap();
        let t = to_this_computer(plain.local_addr().unwrap().port());
        thread::spawn(move || {
            let (mut s, _) = plain.accept().unwrap();
            let mut hello = [0; 512];
            let _ = s.read(&mut hello);
            let _ = s.write_all(b"HTTP/1.1 400 Bad Request\r\ncontent-length: 0\r\n\r\n");
            let _ = io::copy(&mut s, &mut io::sink());
        });
        let named = hosts(&["discord.com"]);
        let e = within(20, move || fetch(get("https://discord.com/"), &named, UA, &t)).unwrap_err();
        assert!(matches!(&e, PluginError::Failed { .. }), "{e:?}");
        assert_eq!(e.to_string(), "The secure connection to discord.com failed.");
    }
}

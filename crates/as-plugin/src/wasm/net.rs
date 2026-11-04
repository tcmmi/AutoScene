//! A plugin's requests over wasi:http, which AutoScene sends itself: never
//! through wasmtime's own client, which resolves a name and connects with
//! no check and waits up to ten minutes.
//!
//! The rules are those of `net.fetch` ([`crate::net`]): `https://` only, on
//! port 443, to one of the hosts its grants name exactly ([`host_allowed`]),
//! never an address such as 127.0.0.1 and never a user name. Its name is
//! resolved once and only the addresses [`is_public`] allows are dialed,
//! exactly those: a name whose answer changes between the check and the
//! connect gains nothing. The certificate is checked against Mozilla's
//! roots, never the system's. No proxy is used, so a plugin behind a
//! company proxy can't reach the internet this way.
//!
//! Unlike `net.fetch`, a redirect isn't followed: the plugin gets each 3xx,
//! and its next request is checked like the first. It can't set `Host` or
//! the other headers that say how a request travels (wasmtime's default
//! filter). A run has at most [`NetLimits::in_flight`] requests at once,
//! each held to the timeouts of [`NetLimits`], shorter ones when the plugin
//! asks. Stop ends a request still being made, and an answer being read,
//! at once, and no request is started after it, so a plugin that looks at
//! its errors can end before the grace.

use std::future::{Future, poll_fn};
use std::io;
use std::net::SocketAddr;
use std::pin::{Pin, pin};
use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll, ready};
use std::time::Duration;

use as_core::CancelToken;
use http_body_util::BodyExt;
use hyper::body::{Body, Frame, Incoming, SizeHint};
use rustls::pki_types::ServerName;
use tokio::net::TcpStream;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::{Instant, Sleep, sleep, timeout};
use tokio_rustls::TlsConnector;
use wasmtime_wasi_http::io::TokioIo;
use wasmtime_wasi_http::p2::bindings::http::types::{ErrorCode, TlsAlertReceivedPayload};
use wasmtime_wasi_http::{Error, RequestOptions, WasiBody, WasiHttpHooks};

use super::STOP_POLL;
use crate::net::{host_allowed, is_public};

type Fut<T> = Pin<Box<dyn Future<Output = T> + Send>>;
/// Resolves a host name and port to its addresses.
pub type Resolve = Arc<dyn Fn(String, u16) -> Fut<io::Result<Vec<SocketAddr>>> + Send + Sync>;
/// Connects to one address.
pub type Dial = Arc<dyn Fn(SocketAddr) -> Fut<io::Result<TcpStream>> + Send + Sync>;
/// What is left of a request once its answer is in: the connection, which
/// reads the rest of the body.
type Io = Box<dyn Future<Output = Result<(), Error>> + Send>;
/// What a request of the plugin is answered with.
type Sent = Result<(http::Response<WasiBody>, Io), Error>;

/// How long a request may wait at each step, and how many a run may have
/// at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetLimits {
    /// For the name's addresses.
    pub dns: Duration,
    /// For a connection, its TLS and its HTTP handshake, each.
    pub connect: Duration,
    /// For the answer's head, from when the request is sent.
    pub first_byte: Duration,
    /// Between two parts of the answer's body.
    pub between_bytes: Duration,
    /// Requests a run may have at once, answers being read included.
    pub in_flight: usize,
}

impl Default for NetLimits {
    fn default() -> Self {
        Self {
            dns: Duration::from_secs(10),
            connect: Duration::from_secs(30),
            first_byte: Duration::from_secs(60),
            between_bytes: Duration::from_secs(60),
            in_flight: 8,
        }
    }
}

/// How the requests of WebAssembly plugins reach the internet: how names
/// are resolved and addresses dialed, which roots a certificate must chain
/// to, and the limits.
pub struct Net {
    resolve: Resolve,
    dial: Dial,
    tls: TlsConnector,
    limits: NetLimits,
}

impl Net {
    /// The system's resolver and sockets, Mozilla's roots and the default
    /// limits.
    pub fn system() -> Self {
        let resolve: Resolve = Arc::new(|host, port| {
            Box::pin(async move { Ok(tokio::net::lookup_host((host.as_str(), port)).await?.collect()) })
        });
        let dial: Dial = Arc::new(|addr| Box::pin(TcpStream::connect(addr)));
        let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
        Self::with(resolve, dial, roots, NetLimits::default())
    }

    /// Requests resolved by `resolve`, dialed by `dial`, trusting `roots`:
    /// for tests, which answer from this computer.
    #[doc(hidden)]
    pub fn with(resolve: Resolve, dial: Dial, roots: rustls::RootCertStore, limits: NetLimits) -> Self {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("ring offers the default TLS versions")
            .with_root_certificates(roots)
            .with_no_client_auth();
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        Self { resolve, dial, tls: TlsConnector::from(Arc::new(config)), limits }
    }
}

/// AutoScene's side of wasi:http for one run: where its requests may go.
pub(crate) struct Guard {
    net: Arc<Net>,
    /// The hosts it may reach; none without the network permission.
    hosts: Arc<[String]>,
    cancel: CancelToken,
    /// When it first told the plugin of Stop.
    told: Told,
    in_flight: Arc<Semaphore>,
}

/// When the plugin was first told of Stop: the run saw it then, as when
/// the plugin asks.
pub(crate) type Told = Arc<OnceLock<std::time::Instant>>;

impl Guard {
    pub(crate) fn new(net: Arc<Net>, hosts: &[String], cancel: CancelToken) -> Self {
        let in_flight = Arc::new(Semaphore::new(net.limits.in_flight));
        Self { net, hosts: hosts.into(), cancel, told: Told::default(), in_flight }
    }

    /// When it first told the plugin of Stop, once it did.
    pub(crate) fn told(&self) -> Told {
        self.told.clone()
    }

    /// Where `request` goes, when it may: its host, lowercased. Else why
    /// not, as the plugin gets it.
    fn checked(&self, request: &http::Request<WasiBody>) -> Result<String, Error> {
        let uri = request.uri();
        if uri.scheme() != Some(&http::uri::Scheme::HTTPS) {
            return Err(Error::HttpRequestDenied);
        }
        let Some(authority) = uri.authority() else { return Err(Error::HttpRequestUriInvalid) };
        // No user name in it; host_allowed refuses an address and a
        // trailing dot.
        if authority.as_str().contains('@') || !host_allowed(authority.host(), &self.hosts) {
            return Err(Error::HttpRequestDenied);
        }
        if authority.port_u16().unwrap_or(443) != 443 {
            return Err(Error::HttpRequestDenied);
        }
        Ok(authority.host().to_ascii_lowercase())
    }
}

impl WasiHttpHooks for Guard {
    fn send_request(
        &mut self,
        request: http::Request<WasiBody>,
        options: Option<RequestOptions>,
        _fut: Box<dyn Future<Output = Result<(), Error>> + Send>,
    ) -> Box<dyn Future<Output = Sent> + Send> {
        let told = self.told.clone();
        // After Stop nothing more is started, not even a lookup.
        if self.cancel.is_cancelled() {
            return Box::new(async move { Err(stop(&told)) });
        }
        // Checked as the plugin makes the request, so the one past the
        // limit is the one refused.
        let checked = self.checked(&request).and_then(|host| {
            let permit = self.in_flight.clone().try_acquire_owned().map_err(|_| Error::ConnectionLimitReached)?;
            Ok((host, permit))
        });
        let (host, permit) = match checked {
            Ok(checked) => checked,
            Err(e) => return Box::new(async move { Err(e) }),
        };
        let (net, cancel) = (self.net.clone(), self.cancel.clone());
        Box::new(async move {
            tokio::select! {
                // Stop first: a request that waited to be polled isn't
                // started once Stop is in.
                biased;
                () = stopped(&cancel) => Err(stop(&told)),
                sent = send(&net, host, request, options, permit, (&cancel, &told)) => sent,
            }
        })
    }

    fn p2_error_from_tls(&mut self, err: &io::Error) -> ErrorCode {
        tracing::debug!(error = ?err, "a plugin's request failed in TLS");
        // wasmtime tells rustls's errors apart only with its own client;
        // the rest as wasmtime does.
        match err.get_ref().and_then(|e| e.downcast_ref::<rustls::Error>()) {
            Some(rustls::Error::InvalidCertificate(_)) => ErrorCode::TlsCertificateError,
            Some(rustls::Error::AlertReceived(alert)) => ErrorCode::TlsAlertReceived(TlsAlertReceivedPayload {
                alert_id: Some(u8::from(*alert)),
                alert_message: None,
            }),
            Some(_) => ErrorCode::TlsProtocolError,
            None => match err.kind() {
                io::ErrorKind::ConnectionReset
                | io::ErrorKind::ConnectionAborted
                | io::ErrorKind::BrokenPipe
                | io::ErrorKind::UnexpectedEof => ErrorCode::ConnectionTerminated,
                _ => ErrorCode::TlsProtocolError,
            },
        }
    }
}

/// Sends `request` to `host`, which [`Guard::checked`] passed: its head
/// once it is in, and the connection that reads the rest. `permit` is held
/// until that connection ends; reading the body ends at Stop.
async fn send(
    net: &Net,
    host: String,
    mut request: http::Request<WasiBody>,
    options: Option<RequestOptions>,
    permit: OwnedSemaphorePermit,
    (cancel, told): (&CancelToken, &Told),
) -> Sent {
    let limits = net.limits;
    // The plugin may ask for less time, never more.
    let asked = |ours: Duration, theirs: Option<Duration>| theirs.map_or(ours, |t| t.min(ours));
    let options = options.unwrap_or_default();
    let connect = asked(limits.connect, options.connect_timeout);
    let first_byte = asked(limits.first_byte, options.first_byte_timeout);
    let between_bytes = asked(limits.between_bytes, options.between_bytes_timeout);

    let found = match timeout(limits.dns, (net.resolve)(host.clone(), 443)).await {
        Err(_) => return Err(Error::DnsTimeout),
        Ok(Err(e)) => {
            tracing::debug!(error = %e, "a plugin's host name didn't resolve");
            return Err(Error::DnsError { rcode: None, info_code: None });
        }
        Ok(Ok(found)) => found,
    };
    let public: Vec<SocketAddr> = found.into_iter().filter(|a| is_public(a.ip())).collect();
    if public.is_empty() {
        return Err(Error::DestinationIpProhibited);
    }
    // Each address it checked, in order; never the name again.
    let mut last = Error::DestinationIpProhibited;
    let mut stream = None;
    for addr in public {
        match timeout(connect, (net.dial)(addr)).await {
            Ok(Ok(s)) => {
                stream = Some(s);
                break;
            }
            Ok(Err(e)) => last = Error::Connect(e),
            Err(_) => last = Error::ConnectionTimeout,
        }
    }
    let Some(stream) = stream else { return Err(last) };

    let name = ServerName::try_from(host).map_err(|e| Error::Tls(io::Error::new(io::ErrorKind::InvalidInput, e)))?;
    let tls = match timeout(connect, net.tls.connect(name, stream)).await {
        Err(_) => return Err(Error::ConnectionTimeout),
        Ok(r) => r.map_err(Error::Tls)?,
    };
    let (mut sender, conn) = timeout(connect, hyper::client::conn::http1::Builder::new().handshake(TokioIo::new(tls)))
        .await
        .map_err(|_| Error::ConnectionTimeout)??;

    // The scheme and host went into the connection and the Host header;
    // the request line names only the path, as it does without a proxy.
    let path = request.uri().path_and_query().map_or("/", |p| p.as_str());
    *request.uri_mut() = http::Uri::builder().path_and_query(path).build().map_err(|_| Error::HttpRequestUriInvalid)?;

    let (cancel, told) = (cancel.clone(), told.clone());
    let answer = async move {
        let r = timeout(first_byte, sender.send_request(request)).await.map_err(|_| Error::ConnectionReadTimeout)??;
        Ok::<_, Error>(r.map(|body| Paced::new(body, between_bytes, cancel, told).boxed_unsync()))
    };
    // The connection reads and writes while the answer is awaited.
    let mut answer = pin!(answer);
    let mut conn = Some(conn);
    let response = poll_fn(|cx| match answer.as_mut().poll(cx) {
        Poll::Ready(r) => Poll::Ready(r),
        Poll::Pending => {
            let Some(c) = conn.as_mut() else { return Poll::Pending };
            let ended = ready!(Pin::new(c).poll(cx));
            conn = None;
            match ended {
                Ok(()) => answer.as_mut().poll(cx),
                Err(e) => Poll::Ready(Err(Error::from(e))),
            }
        }
    })
    .await?;
    let io: Io = Box::new(async move {
        let _permit = permit;
        match conn {
            Some(conn) => conn.await.map_err(Error::from),
            None => Ok(()),
        }
    });
    Ok((response, io))
}

/// An answer's body that fails with [`Error::ConnectionReadTimeout`] when
/// its next part is longer than `gap` in coming, and as [`stop`] once
/// `cancel` fired.
struct Paced {
    body: Incoming,
    gap: Duration,
    late: Pin<Box<Sleep>>,
    cancel: CancelToken,
    told: Told,
    /// When to look at `cancel` again while no part comes.
    look: Pin<Box<Sleep>>,
}

impl Paced {
    fn new(body: Incoming, gap: Duration, cancel: CancelToken, told: Told) -> Self {
        Self { body, gap, late: Box::pin(sleep(gap)), cancel, told, look: Box::pin(sleep(STOP_POLL)) }
    }
}

impl Body for Paced {
    type Data = <Incoming as Body>::Data;
    type Error = Error;

    fn poll_frame(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Self::Data>, Error>>> {
        let this = self.get_mut();
        if this.cancel.is_cancelled() {
            return Poll::Ready(Some(Err(stop(&this.told))));
        }
        match Pin::new(&mut this.body).poll_frame(cx) {
            Poll::Ready(Some(Ok(frame))) => {
                this.late.as_mut().reset(Instant::now() + this.gap);
                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(Error::from(e)))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => {
                while this.look.as_mut().poll(cx).is_ready() {
                    if this.cancel.is_cancelled() {
                        return Poll::Ready(Some(Err(stop(&this.told))));
                    }
                    this.look.as_mut().reset(Instant::now() + STOP_POLL);
                }
                ready!(this.late.as_mut().poll(cx));
                Poll::Ready(Some(Err(Error::ConnectionReadTimeout)))
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.body.size_hint()
    }
}

/// Ends once `cancel` fired.
async fn stopped(cancel: &CancelToken) {
    while !cancel.is_cancelled() {
        sleep(STOP_POLL).await;
    }
}

/// What a request, or the answer being read, ends with after Stop, noted
/// in `told`.
fn stop(told: &Told) -> Error {
    let _ = told.set(std::time::Instant::now());
    Error::InternalError(Some("Stopped.".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard(hosts: &[&str]) -> Guard {
        let hosts: Vec<String> = hosts.iter().map(|h| (*h).to_owned()).collect();
        let resolve: Resolve = Arc::new(|_, _| Box::pin(async { Ok(Vec::new()) }));
        let dial: Dial = Arc::new(|_| Box::pin(async { Err(io::ErrorKind::ConnectionRefused.into()) }));
        let net = Net::with(resolve, dial, rustls::RootCertStore::empty(), NetLimits::default());
        Guard::new(Arc::new(net), &hosts, CancelToken::new())
    }

    fn request(uri: &str) -> http::Request<WasiBody> {
        let body = http_body_util::Empty::new().map_err(|e: std::convert::Infallible| match e {}).boxed_unsync();
        http::Request::builder().uri(uri).body(body).unwrap()
    }

    #[test]
    fn a_request_goes_only_to_a_named_host_over_https_on_443() {
        let g = guard(&["discord.com"]);
        for uri in ["https://discord.com/api", "https://DISCORD.com/", "https://discord.com:443/"] {
            assert_eq!(g.checked(&request(uri)).unwrap(), "discord.com", "{uri}");
        }
        for uri in [
            "http://discord.com/",
            "https://discord.com:8443/",
            "https://discord.com./",
            "https://cdn.discord.com/",
            "https://user@discord.com/",
            "https://162.159.135.232/",
            "https://[::1]/",
        ] {
            assert!(matches!(g.checked(&request(uri)), Err(Error::HttpRequestDenied)), "{uri}");
        }
        // Without the network permission it names no host.
        assert!(matches!(guard(&[]).checked(&request("https://discord.com/")), Err(Error::HttpRequestDenied)));
    }
}

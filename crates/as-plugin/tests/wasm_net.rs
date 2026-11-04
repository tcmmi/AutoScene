//! The requests of WebAssembly plugins, through AutoScene's guard: where
//! they may go, which addresses are dialed, what comes back, and how
//! timeouts and Stop end them. The test guest (`tests/guest`) makes them;
//! names are resolved and addresses dialed by the test ([`Net::with`]), and
//! an answer comes from an HTTPS server on this computer whose certificate
//! the guard is made to trust. Nothing reaches the network, except the one
//! test marked `#[ignore]`.
//!
//! A test skips when the guest wasn't built, unless
//! `AUTOSCENE_REQUIRE_FIXTURES` is set (CI sets it).

#![cfg(feature = "wasm")]

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::fs;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::thread;
use std::time::{Duration, Instant};

use as_core::CancelToken;
use as_plugin::manifest::{RuntimeKind, WasmLimits};
use as_plugin::package::sha256_hex;
use as_plugin::wasm::{self, Dial, Grants, Limits, Net, NetLimits, Resolve, WasmSpec};
use as_plugin::{
    ActionContext, ActionKind, AppContext, Permission, PluginContext, PluginError, PluginId, PluginResult, RunMessage,
    SettingValue, ToolPaths, Trigger,
};
use hyper::body::{Body, Bytes, Frame};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tempfile::TempDir;
use tokio::net::{TcpListener, TcpStream};
use wasmtime_wasi_http::io::TokioIo;

const ID: &str = "dev.autoscene.test-guest";
/// The host the local server has a certificate for.
const ALLOWED: &str = "allowed.example";
/// Where names lead, as the test's resolver answers: the local server, a
/// listener that never speaks TLS, one that hangs up at once, and an
/// address no connection to ever ends. All public, so the guard dials them.
const SERVER_IP: IpAddr = IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8));
const SILENT_IP: IpAddr = IpAddr::V4(Ipv4Addr::new(9, 9, 9, 9));
const HANGUP_IP: IpAddr = IpAddr::V4(Ipv4Addr::new(4, 4, 4, 4));
const HANGING_IP: IpAddr = IpAddr::V4(Ipv4Addr::new(1, 0, 0, 1));

/// An HTTPS server on this computer for [`ALLOWED`], a listener that takes
/// connections and never answers, and one that closes each at once.
struct Server {
    rt: Option<tokio::runtime::Runtime>,
    addr: SocketAddr,
    silent: SocketAddr,
    hangup: SocketAddr,
    cert: CertificateDer<'static>,
    /// Each request it got: its `Host` header and its path.
    seen: Arc<Mutex<Vec<(String, String)>>>,
}

impl Server {
    fn start() -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let made = rcgen::generate_simple_self_signed(vec![ALLOWED.to_owned()]).unwrap();
        let cert = made.cert.der().clone();
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(made.signing_key.serialize_der()));
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![cert.clone()], key)
            .unwrap();
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        let seen = Arc::new(Mutex::new(Vec::new()));

        let bind = || rt.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
        let (listener, silent, hangup) = (bind(), bind(), bind());
        let (addr, silent_addr) = (listener.local_addr().unwrap(), silent.local_addr().unwrap());
        let hangup_addr = hangup.local_addr().unwrap();
        let seen2 = seen.clone();
        rt.spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                let (acceptor, seen) = (acceptor.clone(), seen2.clone());
                tokio::spawn(async move {
                    let Ok(tls) = acceptor.accept(tcp).await else { return };
                    let service = hyper::service::service_fn(move |req| answer(req, seen.clone()));
                    let _ =
                        hyper::server::conn::http1::Builder::new().serve_connection(TokioIo::new(tls), service).await;
                });
            }
        });
        rt.spawn(async move {
            let mut held = Vec::new();
            while let Ok((tcp, _)) = silent.accept().await {
                held.push(tcp);
            }
        });
        rt.spawn(async move {
            while let Ok((tcp, _)) = hangup.accept().await {
                drop(tcp);
            }
        });
        Self { rt: Some(rt), addr, silent: silent_addr, hangup: hangup_addr, cert, seen }
    }

    fn seen(&self) -> Vec<(String, String)> {
        self.seen.lock().unwrap().clone()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        // Its connections wait forever on purpose.
        if let Some(rt) = self.rt.take() {
            rt.shutdown_background();
        }
    }
}

/// The server's answer to `req`, by its path.
async fn answer(
    req: hyper::Request<hyper::body::Incoming>,
    seen: Arc<Mutex<Vec<(String, String)>>>,
) -> Result<hyper::Response<Parts>, Infallible> {
    let host =
        req.headers().get("host").map(|h| String::from_utf8_lossy(h.as_bytes()).into_owned()).unwrap_or_default();
    let path = req.uri().path().to_owned();
    seen.lock().unwrap().push((host, path.clone()));
    let response = hyper::Response::builder();
    Ok(match path.as_str() {
        "/redirect" => response.status(302).header("location", "https://other.example/").body(Parts::done("")),
        "/head-late" => std::future::pending().await,
        "/body-late" => response.status(200).body(Parts { stall: true, ..Parts::done("x") }),
        "/trickle" => response.status(200).body(Parts::trickle(8)),
        _ => response.status(200).body(Parts::done("hello")),
    }
    .unwrap())
}

/// A body of one part, then of `left` parts of a byte each [`TRICKLE`]
/// apart, which then ends or never does.
struct Parts {
    first: Option<Bytes>,
    left: u32,
    next: Pin<Box<tokio::time::Sleep>>,
    stall: bool,
}

/// Between two parts of a trickle: well within [`short`]'s limit.
const TRICKLE: Duration = Duration::from_millis(100);

impl Parts {
    fn done(text: &'static str) -> Self {
        Self { first: Some(Bytes::from(text)), left: 0, next: Box::pin(tokio::time::sleep(TRICKLE)), stall: false }
    }

    /// `n` bytes in all, the first at once.
    fn trickle(n: u32) -> Self {
        Self { left: n - 1, ..Self::done("x") }
    }
}

impl Body for Parts {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        let this = self.get_mut();
        if let Some(part) = this.first.take() {
            return Poll::Ready(Some(Ok(Frame::data(part))));
        }
        if this.left > 0 {
            std::task::ready!(this.next.as_mut().poll(cx));
            this.next.as_mut().reset(tokio::time::Instant::now() + TRICKLE);
            this.left -= 1;
            return Poll::Ready(Some(Ok(Frame::data(Bytes::from("x")))));
        }
        if this.stall { Poll::Pending } else { Poll::Ready(None) }
    }
}

/// What the test's resolver and dialer were asked.
#[derive(Clone, Default)]
struct Asked {
    names: Arc<Mutex<Vec<String>>>,
    dialed: Arc<Mutex<Vec<SocketAddr>>>,
}

impl Asked {
    fn names(&self) -> Vec<String> {
        self.names.lock().unwrap().clone()
    }

    fn dialed(&self) -> Vec<IpAddr> {
        self.dialed.lock().unwrap().iter().map(SocketAddr::ip).collect()
    }
}

/// A network where each name resolves as `answers` says (none: it never
/// answers), [`SERVER_IP`], [`SILENT_IP`] and [`HANGUP_IP`] lead to
/// `server`, and every
/// other address refuses, but [`HANGING_IP`], which never connects.
fn net(answers: &[(&str, Option<&[&str]>)], server: Option<&Server>, limits: NetLimits) -> (Net, Asked) {
    let asked = Asked::default();
    let answers: BTreeMap<String, Option<Vec<IpAddr>>> = answers
        .iter()
        .map(|(name, ips)| ((*name).to_owned(), ips.map(|ips| ips.iter().map(|ip| ip.parse().unwrap()).collect())))
        .collect();
    let names = asked.names.clone();
    let resolve: Resolve = Arc::new(move |name, port| {
        names.lock().unwrap().push(name.clone());
        let answer = answers.get(&name).cloned();
        Box::pin(async move {
            match answer {
                Some(Some(ips)) => Ok(ips.into_iter().map(|ip| SocketAddr::new(ip, port)).collect()),
                Some(None) => std::future::pending().await,
                None => Err(io::Error::new(io::ErrorKind::NotFound, "no such name")),
            }
        })
    });
    let to = server.map(|s| (s.addr, s.silent, s.hangup));
    let dialed = asked.dialed.clone();
    let dial: Dial = Arc::new(move |addr| {
        dialed.lock().unwrap().push(addr);
        Box::pin(async move {
            match (addr.ip(), to) {
                (SERVER_IP, Some((server, ..))) => TcpStream::connect(server).await,
                (SILENT_IP, Some((_, silent, _))) => TcpStream::connect(silent).await,
                (HANGUP_IP, Some((.., hangup))) => TcpStream::connect(hangup).await,
                (HANGING_IP, _) => std::future::pending().await,
                _ => Err(io::ErrorKind::ConnectionRefused.into()),
            }
        })
    });
    let mut roots = rustls::RootCertStore::empty();
    if let Some(server) = server {
        roots.add(server.cert.clone()).unwrap();
    }
    (Net::with(resolve, dial, roots, limits), asked)
}

/// A run of the test guest's `action` that may reach `hosts`, with `urls`
/// as its input.
struct Run {
    dir: TempDir,
    component: PathBuf,
    action: String,
    /// The network permission, unless a test takes it away.
    permissions: Vec<Permission>,
    hosts: Vec<String>,
    urls: Vec<String>,
    /// What its requests ask for as each of their timeouts.
    timeout_ms: Option<f64>,
}

/// The guest running `action`; none (the test skips) when it wasn't built.
fn run_of(action: &str, hosts: &[&str], urls: &[&str]) -> Option<Run> {
    let component = as_testkit::fixture_file("wasm/guest.wasm")?;
    Some(Run {
        dir: tempfile::tempdir().unwrap(),
        component,
        action: action.into(),
        permissions: vec![Permission::Network],
        hosts: hosts.iter().map(|h| (*h).to_owned()).collect(),
        urls: urls.iter().map(|u| (*u).to_owned()).collect(),
        timeout_ms: None,
    })
}

impl Run {
    fn spec(&self) -> WasmSpec {
        let root = self.dir.path();
        let mut settings = BTreeMap::from([("urls".into(), SettingValue::Text(self.urls.join("\n")))]);
        if let Some(ms) = self.timeout_ms {
            settings.insert("timeout-ms".into(), SettingValue::Number(ms));
        }
        let message = RunMessage {
            protocol: 2,
            run_id: "run-1".into(),
            plugin: PluginContext {
                id: PluginId::parse(ID).unwrap(),
                version: "1.2.0".into(),
                data_dir: "?".into(),
                resources_dir: None,
            },
            app: AppContext { version: "1.1.0".into(), platform: "windows-x86_64".into() },
            tools: ToolPaths { ffmpeg: "ffmpeg".into(), ffprobe: "ffprobe".into(), ffmpeg_version: "9.0".into() },
            settings,
            action: ActionContext { kind: ActionKind::Command, id: self.action.clone(), trigger: Trigger::Palette },
            project: None,
            export: None,
            input: None,
        };
        WasmSpec {
            name: "Test Guest".into(),
            component: self.component.clone(),
            component_sha256: None,
            cache: root.join("code"),
            data_dir: root.join("data"),
            log_path: root.join("logs/plugin.log"),
            grants: Grants {
                runtime: RuntimeKind::Wasm,
                permissions: self.permissions.clone(),
                hosts: self.hosts.clone(),
                secrets: Vec::new(),
                protocol: 2,
            },
            limits: Limits::of(WasmLimits::default()),
            secrets: BTreeMap::new(),
            message,
        }
    }

    /// Compiles its component into its code folder first, so a run's time
    /// is its own.
    fn compile(&self) {
        let sha256 = sha256_hex(&fs::read(&self.component).unwrap());
        let m = as_plugin::manifest::parse_for_any_app(&serde_json::to_vec(&manifest()).unwrap()).unwrap();
        wasm::prepare(&self.component, Some(&sha256), &self.dir.path().join("code"), &m.manifest).unwrap();
    }

    fn run(&self, net: Net, cancel: &CancelToken) -> PluginResult<Option<String>> {
        wasm::run_with(self.spec(), net, cancel, &|_, _| {})
    }

    /// What the guest said, whole, as its log has it.
    fn runs(&self, net: Net) -> String {
        let r = self.run(net, &CancelToken::new());
        assert!(matches!(r, Ok(Some(_))), "{r:?}");
        self.said()
    }

    /// [`Run::runs`], compiled first, when the run ends within `limit`;
    /// else Stop ends it and the test fails.
    fn runs_within(&self, net: Net, limit: Duration) -> String {
        self.compile();
        let cancel = CancelToken::new();
        let r = thread::scope(|s| {
            let (done, wait) = std::sync::mpsc::channel::<()>();
            let stop = &cancel;
            s.spawn(move || {
                if wait.recv_timeout(limit) == Err(RecvTimeoutError::Timeout) {
                    stop.cancel();
                }
            });
            let r = self.run(net, &cancel);
            drop(done);
            r
        });
        assert!(!cancel.is_cancelled(), "not done within {limit:?}: {r:?}");
        assert!(matches!(r, Ok(Some(_))), "{r:?}");
        self.said()
    }

    /// Runs, compiled first, and presses Stop once `ready` says so: how
    /// the run ended and how long after Stop.
    fn stopped_when(&self, net: Net, ready: impl Fn() -> bool) -> (PluginResult<Option<String>>, Duration) {
        self.compile();
        let cancel = CancelToken::new();
        thread::scope(|s| {
            let run = s.spawn(|| self.run(net, &cancel));
            let started = Instant::now();
            while !ready() && started.elapsed() < Duration::from_secs(10) {
                thread::sleep(Duration::from_millis(10));
            }
            let asked_at = Instant::now();
            cancel.cancel();
            (run.join().unwrap(), asked_at.elapsed())
        })
    }

    fn log(&self) -> String {
        fs::read_to_string(self.dir.path().join("logs/plugin.log")).unwrap()
    }

    /// Whether its log has `text` yet.
    fn logged(&self, text: &str) -> bool {
        fs::read_to_string(self.dir.path().join("logs/plugin.log")).is_ok_and(|log| log.contains(text))
    }

    fn said(&self) -> String {
        let log = self.log();
        let mut lines = log.lines().skip_while(|l| !l.starts_with("[result] "));
        let first = lines.next().expect("a result in the log").trim_start_matches("[result] ").to_owned();
        let rest = lines.map_while(|l| l.strip_prefix("  ")).map(str::to_owned);
        std::iter::once(first).chain(rest).collect::<Vec<_>>().join("\n")
    }
}

fn manifest() -> serde_json::Value {
    serde_json::json!({
        "manifestVersion": 2, "id": ID, "version": "1.2.0", "name": "Test Guest",
        "description": "Makes requests.", "author": {"name": "AutoScene"},
        "engines": {"autoscene": ">=1.1.0", "protocol": 2},
        "runtime": {"type": "wasm", "component": "plugin.wasm"},
        // The grants name the test's hosts; a manifest takes only public ones.
        "permissions": ["network"], "network": {"hosts": ["discord.com"]},
        "contributes": {"commands": [{"id": "fetch", "title": "Fetch"}]}
    })
}

fn line<'a>(text: &'a str, start: &str) -> &'a str {
    text.lines().find(|l| l.starts_with(start)).unwrap_or_else(|| panic!("no line {start:?} in:\n{text}"))
}

/// Limits short enough for a test to wait them out; a connection to the
/// local server still has its TLS done well within its second.
fn short() -> NetLimits {
    let wait = Duration::from_millis(300);
    NetLimits { dns: wait, connect: Duration::from_secs(1), first_byte: wait, between_bytes: wait, in_flight: 8 }
}

#[test]
fn only_https_to_named_hosts_on_443_is_sent() {
    let urls = [
        "http://discord.com/",
        "https://93.184.215.14/",
        "https://[::1]/",
        "https://other.example/",
        "https://discord.com:8443/",
        "https://discord.com./",
        "https://user@discord.com/",
    ];
    let Some(r) = run_of("fetch", &["discord.com"], &urls) else { return };
    let (n, asked) = net(&[("discord.com", Some(&["162.159.135.232"]))], None, NetLimits::default());
    let said = r.runs(n);
    for url in urls {
        assert_eq!(line(&said, &format!("{url}:")), format!("{url}: error ErrorCode::HttpRequestDenied"));
    }
    assert!(asked.names().is_empty() && asked.dialed().is_empty(), "{:?} {:?}", asked.names(), asked.dialed());

    // The name in any case is the name; it is resolved lowercased.
    let Some(r) = run_of("fetch", &["discord.com"], &["https://DISCORD.COM/"]) else { return };
    let (n, asked) = net(&[("discord.com", Some(&["162.159.135.232"]))], None, NetLimits::default());
    let said = r.runs(n);
    assert_eq!(said, "https://DISCORD.COM/: error ErrorCode::ConnectionRefused");
    assert_eq!(asked.names(), ["discord.com"]);
    assert_eq!(asked.dialed(), ["162.159.135.232".parse::<IpAddr>().unwrap()]);
}

#[test]
fn names_that_resolve_to_private_addresses_are_refused() {
    let private = ["127.0.0.1", "10.0.0.1", "100.64.0.1", "::1", "fe80::1", "::ffff:127.0.0.1"];
    for ip in private {
        let Some(r) = run_of("fetch", &[ALLOWED], &["https://allowed.example/"]) else { return };
        let (n, asked) = net(&[(ALLOWED, Some(&[ip]))], None, NetLimits::default());
        assert_eq!(r.runs(n), "https://allowed.example/: error ErrorCode::DestinationIpProhibited", "{ip}");
        assert!(asked.dialed().is_empty(), "{ip}: {:?}", asked.dialed());
    }
}

#[test]
fn a_mixed_answer_connects_only_to_a_public_address() {
    let Some(server) = guest_and_server() else { return };
    let Some(r) = run_of("fetch", &[ALLOWED], &["https://allowed.example/"]) else { return };
    let answer = ["10.0.0.1", "::1", "162.159.135.232", "192.168.1.1", "8.8.8.8"];
    let (n, asked) = net(&[(ALLOWED, Some(&answer))], Some(&server), NetLimits::default());
    // The first public address refuses; the next one answers.
    assert_eq!(r.runs(n), "https://allowed.example/: status 200, 5 bytes");
    let public: Vec<IpAddr> = ["162.159.135.232", "8.8.8.8"].iter().map(|ip| ip.parse().unwrap()).collect();
    assert_eq!(asked.dialed(), public);
}

#[test]
fn a_redirect_comes_back_to_the_plugin_and_is_not_followed() {
    let Some(server) = guest_and_server() else { return };
    let urls = ["https://allowed.example/redirect", "https://other.example/"];
    let Some(r) = run_of("fetch", &[ALLOWED], &urls) else { return };
    let (n, asked) = net(
        &[(ALLOWED, Some(&["8.8.8.8"])), ("other.example", Some(&["8.8.8.8"]))],
        Some(&server),
        NetLimits::default(),
    );
    let said = r.runs(n);
    assert_eq!(
        line(&said, "https://allowed.example/redirect:"),
        "https://allowed.example/redirect: status 302 location https://other.example/, 0 bytes"
    );
    // The plugin's own next request, where it was sent, is checked as any.
    assert_eq!(line(&said, "https://other.example/:"), "https://other.example/: error ErrorCode::HttpRequestDenied");
    assert_eq!(server.seen(), [(ALLOWED.to_owned(), "/redirect".to_owned())]);
    assert_eq!(asked.names(), [ALLOWED]);
}

#[test]
fn a_certificate_for_another_name_is_refused() {
    let Some(server) = guest_and_server() else { return };
    // The server's certificate names allowed.example only.
    let Some(r) = run_of("fetch", &["elsewhere.example"], &["https://elsewhere.example/"]) else { return };
    let (n, _) = net(&[("elsewhere.example", Some(&["8.8.8.8"]))], Some(&server), NetLimits::default());
    assert_eq!(r.runs(n), "https://elsewhere.example/: error ErrorCode::TlsCertificateError");
    assert!(server.seen().is_empty());
}

#[test]
fn a_server_that_hangs_up_in_tls_ends_the_connection() {
    let Some(server) = guest_and_server() else { return };
    let Some(r) = run_of("fetch", &["hangup.example"], &["https://hangup.example/"]) else { return };
    let (n, _) = net(&[("hangup.example", Some(&["4.4.4.4"]))], Some(&server), NetLimits::default());
    assert_eq!(r.runs(n), "https://hangup.example/: error ErrorCode::ConnectionTerminated");
}

#[test]
fn the_plugin_cannot_set_the_host_header() {
    let Some(server) = guest_and_server() else { return };
    let Some(r) = run_of("fetch-host", &[ALLOWED], &["https://allowed.example/host", "other.example"]) else {
        return;
    };
    let (n, _) = net(&[(ALLOWED, Some(&["8.8.8.8"]))], Some(&server), NetLimits::default());
    assert_eq!(r.runs(n), "https://allowed.example/host: host header refused: HeaderError::Forbidden");
    assert!(server.seen().is_empty());

    // The header the server gets is the host the request was checked for.
    let Some(r) = run_of("fetch", &[ALLOWED], &["https://allowed.example/host"]) else { return };
    let (n, _) = net(&[(ALLOWED, Some(&["8.8.8.8"]))], Some(&server), NetLimits::default());
    assert_eq!(r.runs(n), "https://allowed.example/host: status 200, 5 bytes");
    assert_eq!(server.seen(), [(ALLOWED.to_owned(), "/host".to_owned())]);
}

#[test]
fn too_many_requests_at_once_are_refused() {
    let Some(r) = run_of("fetch-many", &[ALLOWED], &["https://allowed.example/"]) else { return };
    // Each connection hangs until its timeout, so all eight are in flight.
    let (n, asked) = net(&[(ALLOWED, Some(&["1.0.0.1"]))], None, short());
    let said = r.runs(n);
    for i in 0..8 {
        assert_eq!(line(&said, &format!("{i}:")), format!("{i}: error ErrorCode::ConnectionTimeout"));
    }
    assert_eq!(line(&said, "8:"), "8: error ErrorCode::ConnectionLimitReached");
    assert_eq!(asked.dialed().len(), 8);
}

/// What a request the guest made gets once Stop is pressed.
const STOPPED: &str = "error ErrorCode::InternalError(Some(\"Stopped.\"))";

#[test]
fn a_run_may_make_many_requests_one_after_another() {
    let Some(server) = guest_and_server() else { return };
    // More than it may have at once: each gives its place back as it ends.
    let urls = ["https://allowed.example/"; 12];
    let Some(r) = run_of("fetch", &[ALLOWED], &urls) else { return };
    let (n, _) = net(&[(ALLOWED, Some(&["8.8.8.8"]))], Some(&server), NetLimits::default());
    assert_eq!(r.runs(n), ["https://allowed.example/: status 200, 5 bytes"; 12].join("\n"));
    assert_eq!(server.seen().len(), 12);
}

#[test]
fn an_answer_holds_its_place_until_it_is_let_go() {
    let Some(server) = guest_and_server() else { return };
    let urls = ["https://allowed.example/body-late", "https://allowed.example/"];
    let Some(r) = run_of("fetch-held", &[ALLOWED], &urls) else { return };
    let (n, _) = net(&[(ALLOWED, Some(&["8.8.8.8"]))], Some(&server), NetLimits::default());
    let said = r.runs(n);
    let mut expected = vec!["status 200"; 8];
    // Eight answers are in and their bodies unread: the ninth is refused
    // while the guest waits, and the tenth goes once they are let go.
    expected.extend(["error ErrorCode::ConnectionLimitReached", "status 200, 5 bytes"]);
    let expected: Vec<String> = expected.iter().enumerate().map(|(i, e)| format!("{i}: {e}")).collect();
    assert_eq!(said, expected.join("\n"));
}

#[test]
fn stop_during_a_connect_ends_the_run_at_once() {
    // The guest goes on to ask again after Stop, eleven times.
    let urls = ["https://allowed.example/"; 12];
    let Some(r) = run_of("fetch", &[ALLOWED], &urls) else { return };
    let (n, asked) = net(&[(ALLOWED, Some(&["1.0.0.1"]))], None, NetLimits::default());
    // Once the guest waits on its connection.
    let (result, took) = r.stopped_when(n, || !asked.dialed().is_empty());
    assert!(matches!(result, Err(PluginError::Cancelled)), "{result:?}");
    assert!(took < Duration::from_secs(1), "{took:?}");
    // The guest got why, and returned it; nothing it asked after was
    // looked up or dialed.
    assert_eq!(r.said(), vec![format!("https://allowed.example/: {STOPPED}"); 12].join("\n"));
    assert_eq!((asked.names().len(), asked.dialed().len()), (1, 1));
}

#[test]
fn stop_during_tls_or_the_wait_for_an_answer_ends_the_run_at_once() {
    let Some(server) = guest_and_server() else { return };
    let answers: [(&str, Option<&[&str]>); 2] = [(ALLOWED, Some(&["8.8.8.8"])), ("no-tls.example", Some(&["9.9.9.9"]))];

    // A host that takes the connection and never speaks TLS.
    let Some(r) = run_of("fetch", &["no-tls.example"], &["https://no-tls.example/"]) else { return };
    let (n, asked) = net(&answers, Some(&server), NetLimits::default());
    let (result, took) = r.stopped_when(n, || !asked.dialed().is_empty());
    assert!(matches!(result, Err(PluginError::Cancelled)) && took < Duration::from_secs(1), "{result:?} {took:?}");
    assert_eq!(r.said(), format!("https://no-tls.example/: {STOPPED}"));

    // A server that has the request and never answers.
    let Some(r) = run_of("fetch", &[ALLOWED], &["https://allowed.example/head-late"]) else { return };
    let (n, _) = net(&answers, Some(&server), NetLimits::default());
    let (result, took) = r.stopped_when(n, || !server.seen().is_empty());
    assert!(matches!(result, Err(PluginError::Cancelled)) && took < Duration::from_secs(1), "{result:?} {took:?}");
    assert_eq!(r.said(), format!("https://allowed.example/head-late: {STOPPED}"));
}

#[test]
fn stop_while_an_answer_is_read_ends_the_run_at_once() {
    let Some(server) = guest_and_server() else { return };
    let Some(r) = run_of("fetch", &[ALLOWED], &["https://allowed.example/body-late"]) else { return };
    let (n, _) = net(&[(ALLOWED, Some(&["8.8.8.8"]))], Some(&server), NetLimits::default());
    // Once its head is in and the guest reads the body, which stops after
    // a byte.
    let (result, took) = r.stopped_when(n, || r.logged("body-late: head in"));
    assert!(matches!(result, Err(PluginError::Cancelled)) && took < Duration::from_secs(1), "{result:?} {took:?}");
    // wasmtime 50 hands a body's error to a WASI 0.2 guest as text only.
    let said = r.said();
    assert!(
        said.starts_with("https://allowed.example/body-late: status 200, body error internal error: Stopped. after "),
        "{said}"
    );
}

#[test]
fn slow_servers_hit_the_timeouts() {
    let Some(server) = guest_and_server() else { return };
    let hosts = [ALLOWED, "slow-dns.example", "slow-dial.example", "no-tls.example"];
    let urls = [
        "https://slow-dns.example/",
        "https://slow-dial.example/",
        "https://no-tls.example/",
        "https://allowed.example/head-late",
        "https://allowed.example/body-late",
        "https://allowed.example/trickle",
    ];
    let Some(r) = run_of("fetch", &hosts, &urls) else { return };
    let answers: [(&str, Option<&[&str]>); 4] = [
        (ALLOWED, Some(&["8.8.8.8"])),
        ("slow-dns.example", None),
        ("slow-dial.example", Some(&["1.0.0.1"])),
        ("no-tls.example", Some(&["9.9.9.9"])),
    ];
    let (n, _) = net(&answers, Some(&server), short());
    let started = Instant::now();
    let said = r.runs(n);
    let expected = [
        "https://slow-dns.example/: error ErrorCode::DnsTimeout",
        "https://slow-dial.example/: error ErrorCode::ConnectionTimeout",
        "https://no-tls.example/: error ErrorCode::ConnectionTimeout",
        "https://allowed.example/head-late: error ErrorCode::ConnectionReadTimeout",
        // wasmtime 50 hands a body's error to a WASI 0.2 guest as text only.
        "https://allowed.example/body-late: status 200, body error connection read timeout after 1 bytes",
        // Longer than a gap in all, each part within it.
        "https://allowed.example/trickle: status 200, 8 bytes",
    ];
    assert_eq!(said, expected.join("\n"));
    // Each waited its own short limit, not the default ones.
    assert!(started.elapsed() < Duration::from_secs(10), "{:?}", started.elapsed());
}

#[test]
fn a_plugin_may_ask_for_shorter_timeouts_never_longer() {
    let Some(server) = guest_and_server() else { return };
    let hosts = [ALLOWED, "slow-dial.example"];
    let urls = ["https://slow-dial.example/", "https://allowed.example/head-late", "https://allowed.example/body-late"];
    let answers: [(&str, Option<&[&str]>); 2] =
        [(ALLOWED, Some(&["8.8.8.8"])), ("slow-dial.example", Some(&["1.0.0.1"]))];
    let expected = [
        "https://slow-dial.example/: error ErrorCode::ConnectionTimeout",
        "https://allowed.example/head-late: error ErrorCode::ConnectionReadTimeout",
        "https://allowed.example/body-late: status 200, body error connection read timeout after 1 bytes",
    ]
    .join("\n");

    // Shorter than ours: theirs (long enough for TLS to this computer).
    let Some(mut r) = run_of("fetch", &hosts, &urls) else { return };
    r.timeout_ms = Some(1000.0);
    let (n, _) = net(&answers, Some(&server), NetLimits::default());
    assert_eq!(r.runs_within(n, Duration::from_secs(10)), expected);

    // Ten minutes: still ours.
    let Some(mut r) = run_of("fetch", &hosts, &urls) else { return };
    r.timeout_ms = Some(600_000.0);
    let (n, _) = net(&answers, Some(&server), short());
    assert_eq!(r.runs_within(n, Duration::from_secs(10)), expected);
}

#[test]
fn a_lookup_has_a_limit_of_its_own() {
    let Some(r) = run_of("fetch", &["slow-dns.example"], &["https://slow-dns.example/"]) else { return };
    let (n, _) =
        net(&[("slow-dns.example", None)], None, NetLimits { dns: Duration::from_millis(300), ..NetLimits::default() });
    assert_eq!(r.runs_within(n, Duration::from_secs(10)), "https://slow-dns.example/: error ErrorCode::DnsTimeout");
}

#[test]
fn hosts_without_the_network_permission_reach_nothing() {
    // Grants made by hand: their hosts count only with the permission.
    let Some(mut r) = run_of("fetch", &[ALLOWED], &["https://allowed.example/"]) else { return };
    r.permissions = Vec::new();
    let (n, asked) = net(&[(ALLOWED, Some(&["8.8.8.8"]))], None, NetLimits::default());
    assert_eq!(r.runs(n), "https://allowed.example/: error ErrorCode::HttpRequestDenied");
    assert!(asked.names().is_empty() && asked.dialed().is_empty(), "{:?} {:?}", asked.names(), asked.dialed());
}

/// The local server, when the guest was built (none: the test skips).
fn guest_and_server() -> Option<Server> {
    as_testkit::fixture_file("wasm/guest.wasm")?;
    Some(Server::start())
}

#[test]
#[ignore = "reaches example.com over the internet"]
fn a_real_request_reaches_example_com() {
    let Some(r) = run_of("fetch", &["example.com"], &["https://example.com/"]) else { return };
    let said = r.runs(Net::system());
    assert!(said.starts_with("https://example.com/: status 200, "), "{said}");
}

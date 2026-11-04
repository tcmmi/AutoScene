//! The two WebAssembly examples in `examples/plugins`, run as AutoScene
//! runs them: `clip-list-py` (Python) and `discord-notify-wasm` (Rust).
//!
//! Their components are built outside the workspace (the CI job
//! `wasm-examples` does it, as their READMEs say), so these tests read
//! `AUTOSCENE_WASM_EXAMPLES`: a folder that holds each example's folder with
//! its component built into `plugin/plugin.wasm`. Without it they skip,
//! unless `AUTOSCENE_REQUIRE_WASM_EXAMPLES` is set.

#![cfg(feature = "wasm")]

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::fs;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, Mutex};

use as_core::CancelToken;
use as_plugin::manifest::{EventKind, RuntimeKind, WasmLimits};
use as_plugin::protocol::{ClipContext, EpisodeCounts, EventContext, ExportContext, ProjectContext};
use as_plugin::wasm::{self, Dial, Grants, Limits, Net, NetLimits, Resolve, WasmSpec};
use as_plugin::{
    ActionContext, ActionKind, AppContext, Permission, PluginContext, PluginError, PluginId, PluginResult, RunMessage,
    ToolPaths, Trigger,
};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tempfile::TempDir;
use tokio::net::{TcpListener, TcpStream};
use wasmtime_wasi_http::io::TokioIo;

/// The component of the example `name`; none (the test skips) when the
/// examples weren't built.
fn component(name: &str) -> Option<PathBuf> {
    let required = std::env::var_os("AUTOSCENE_REQUIRE_WASM_EXAMPLES").is_some_and(|v| !v.is_empty());
    let Some(dir) = std::env::var_os("AUTOSCENE_WASM_EXAMPLES").filter(|v| !v.is_empty()) else {
        assert!(!required, "AUTOSCENE_REQUIRE_WASM_EXAMPLES is set, but AUTOSCENE_WASM_EXAMPLES isn't");
        eprintln!("AUTOSCENE_WASM_EXAMPLES isn't set: build the WebAssembly examples to run this (skipping)");
        return None;
    };
    let path = Path::new(&dir).join(name).join("plugin/plugin.wasm");
    assert!(path.is_file(), "{} is missing: build the example {name} first", path.display());
    Some(path)
}

/// A run of an example's export action `action`, in folders of its own:
/// `data/` (its `/data`), `export/` (the export's folder, with two clips),
/// `code/` and `logs/`. With `event`, the run of its event `action` instead,
/// told of an analysis that ended so, in the project titled `project`.
struct Run {
    dir: TempDir,
    component: PathBuf,
    id: &'static str,
    action: &'static str,
    label: Option<String>,
    event: Option<EpisodeCounts>,
    project: Option<String>,
    permissions: Vec<Permission>,
    hosts: Vec<String>,
    secrets: BTreeMap<String, String>,
}

impl Run {
    fn of(component: PathBuf, id: &'static str, action: &'static str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        for folder in ["data", "export"] {
            fs::create_dir(dir.path().join(folder)).unwrap();
        }
        fs::write(dir.path().join("export/Walt 1.mp4"), "clip").unwrap();
        fs::write(dir.path().join("export/Jesse 1.mp4"), "clip").unwrap();
        Self {
            dir,
            component,
            id,
            action,
            label: None,
            event: None,
            project: None,
            permissions: Vec::new(),
            hosts: Vec::new(),
            secrets: BTreeMap::new(),
        }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    fn spec(&self) -> WasmSpec {
        let host = |p: PathBuf| p.to_string_lossy().into_owned();
        let clip = |name: &str, episode: &str, start: u64, end: u64| ClipContext {
            name: name.into(),
            path: Some(host(self.path(&format!("export/{name}.mp4")))),
            status: "done".into(),
            media_id: "m1".into(),
            source_path: host(self.path(&format!("episodes/{episode}.mkv"))),
            episode: episode.into(),
            start,
            end,
            fps: Some("24000/1001".into()),
        };
        let mut message = RunMessage {
            event: None,
            protocol: 2,
            run_id: "run-1".into(),
            plugin: PluginContext {
                id: PluginId::parse(self.id).unwrap(),
                version: "1.0.0".into(),
                data_dir: "?".into(),
                resources_dir: None,
                temp_dir: None,
            },
            app: AppContext { version: "1.1.0".into(), platform: "windows-x86_64".into() },
            tools: ToolPaths { ffmpeg: "ffmpeg".into(), ffprobe: "ffprobe".into(), ffmpeg_version: "9.0".into() },
            settings: BTreeMap::new(),
            action: ActionContext { kind: ActionKind::ExportAction, id: self.action.into(), trigger: Trigger::Menu },
            project: None,
            export: Some(ExportContext {
                status: None,
                id: "e1".into(),
                label: self.label.clone(),
                preset: "h264".into(),
                out_dir: host(self.path("export")),
                created_at: 1_700_000_000_000,
                merged_file: None,
                clips: vec![clip("Walt 1", "S01E01", 240, 480), clip("Jesse 1", "S01E02", 1000, 1012)],
            }),
            input: None,
        };
        if let Some(episodes) = self.event {
            message.action = ActionContext { kind: ActionKind::Event, id: self.action.into(), trigger: Trigger::Event };
            message.event = Some(EventContext { kind: EventKind::AnalysisFinished, at: 1_762_150_000_000, episodes });
            message.export = None;
            message.project = self.project.clone().map(|title| ProjectContext {
                id: "p1".into(),
                title,
                folder: host(self.path("project")),
            });
        }
        WasmSpec {
            name: self.id.into(),
            component: self.component.clone(),
            component_sha256: None,
            cache: self.path("code"),
            data_dir: self.path("data"),
            resources_dir: None,
            log_path: self.path("logs/plugin.log"),
            grants: Grants {
                runtime: RuntimeKind::Wasm,
                permissions: self.permissions.clone(),
                hosts: self.hosts.clone(),
                secrets: self.secrets.keys().cloned().collect(),
                protocol: 2,
            },
            limits: Limits::of(WasmLimits::default()),
            secrets: self.secrets.clone(),
            message,
        }
    }

    fn run(&self, net: Net) -> PluginResult<Option<String>> {
        wasm::run_with(self.spec(), net, &CancelToken::new(), &|_, _| {})
    }

    fn log(&self) -> String {
        fs::read_to_string(self.path("logs/plugin.log")).unwrap()
    }
}

/// A network where no name can be found.
fn offline() -> Net {
    let resolve: Resolve =
        Arc::new(|_, _| Box::pin(async { Err(io::Error::new(io::ErrorKind::NotFound, "no such name")) }));
    let dial: Dial = Arc::new(|_| Box::pin(async { Err(io::ErrorKind::ConnectionRefused.into()) }));
    Net::with(resolve, dial, rustls::RootCertStore::empty(), NetLimits::default())
}

#[test]
fn the_python_example_writes_its_clip_list() {
    let Some(component) = component("clip-list-py") else { return };
    let mut run = Run::of(component, "dev.autoscene.clip-list-py", "list");
    run.permissions = vec![Permission::ExportsWrite];
    // A label that can't be a file name as it is.
    run.label = Some("Cut: 1/2".into());
    assert_eq!(run.run(offline()).unwrap().as_deref(), Some("Saved the clip list."));
    let list = fs::read_to_string(run.path("export/Cut_ 1_2 clips.txt")).unwrap();
    assert_eq!(list, "Walt 1\tS01E01\t240\t479\nJesse 1\tS01E02\t1000\t1011\n");
    assert!(run.log().contains("Wrote 2 clips to Cut_ 1_2 clips.txt."), "{}", run.log());

    // Without a label, the file is named after the export's id; and only
    // with exports:write does it get the export's folder.
    run.label = None;
    run.run(offline()).unwrap();
    assert!(run.path("export/e1 clips.txt").is_file());
    run.permissions = vec![Permission::ExportsRead];
    match run.run(offline()) {
        Err(PluginError::Failed { message, .. }) => assert_eq!(message, "Couldn't save the clip list."),
        other => panic!("{other:?}"),
    }
    run.permissions.clear();
    match run.run(offline()) {
        Err(PluginError::Failed { message, .. }) => assert_eq!(message, "Clip List can't reach the export's folder."),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_discord_example_says_when_it_cant_reach_discord() {
    let Some(component) = component("discord-notify-wasm") else { return };
    let webhook = "https://discord.com/api/webhooks/1234/s3cret-token";
    let mut run = Run::of(component, "dev.autoscene.discord-notify-wasm", "post");
    run.label = Some("Cut".into());
    run.permissions = vec![Permission::Network];
    run.hosts = vec!["discord.com".into()];

    // No webhook set yet.
    match run.run(offline()) {
        Err(PluginError::Failed { message, .. }) => {
            assert_eq!(message, "Set the webhook URL in Discord Notify's settings first.");
        }
        other => panic!("{other:?}"),
    }
    // One that isn't Discord's.
    run.secrets.insert("webhook".into(), "https://example.com/api/webhooks/1".into());
    match run.run(offline()) {
        Err(PluginError::Failed { message, .. }) => assert!(
            message.starts_with("That webhook URL doesn't start with https://discord.com/api/webhooks/."),
            "{message}"
        ),
        other => panic!("{other:?}"),
    }
    // Discord's, but with a space in it.
    run.secrets.insert("webhook".into(), "https://discord.com/api/webhooks/1234/s3cret token".into());
    match run.run(offline()) {
        Err(PluginError::Failed { message, .. }) => {
            assert!(message.starts_with("That webhook URL has a space"), "{message}")
        }
        other => panic!("{other:?}"),
    }
    // Discord's, on a network where discord.com can't be found.
    run.secrets.insert("webhook".into(), webhook.into());
    match run.run(offline()) {
        Err(PluginError::Failed { message, detail }) => {
            assert_eq!(message, "Couldn't reach Discord: its address couldn't be found. Is this computer online?");
            assert!(detail.as_deref().is_some_and(|d| d.starts_with("ErrorCode::DnsError")), "{detail:?}");
        }
        other => panic!("{other:?}"),
    }
    // The secret is the plugin's to use, and never in its log.
    assert!(!run.log().contains("s3cret-token"), "{}", run.log());
}

/// What [`Discord`] got: one request.
#[derive(Debug, Clone)]
struct Post {
    method: String,
    host: String,
    path: String,
    content_type: String,
    body: String,
}

/// A Discord on this computer: an HTTPS server with a certificate for
/// discord.com, which keeps each request and answers it with `status`.
struct Discord {
    rt: Option<tokio::runtime::Runtime>,
    addr: SocketAddr,
    cert: CertificateDer<'static>,
    status: Arc<AtomicU16>,
    posts: Arc<Mutex<Vec<Post>>>,
}

impl Discord {
    fn start() -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let made = rcgen::generate_simple_self_signed(vec!["discord.com".to_owned()]).unwrap();
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
        let status = Arc::new(AtomicU16::new(204));
        let posts = Arc::new(Mutex::new(Vec::new()));

        let listener = rt.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
        let addr = listener.local_addr().unwrap();
        let (status2, posts2) = (status.clone(), posts.clone());
        rt.spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                let (acceptor, status, posts) = (acceptor.clone(), status2.clone(), posts2.clone());
                tokio::spawn(async move {
                    let Ok(tls) = acceptor.accept(tcp).await else { return };
                    let service = hyper::service::service_fn(move |req| keep(req, status.clone(), posts.clone()));
                    let _ =
                        hyper::server::conn::http1::Builder::new().serve_connection(TokioIo::new(tls), service).await;
                });
            }
        });
        Self { rt: Some(rt), addr, cert, status, posts }
    }

    /// A network where discord.com resolves to a public address that leads
    /// here, and whose only root is this server's certificate.
    fn net(&self) -> Net {
        let resolve: Resolve = Arc::new(|name, port| {
            Box::pin(async move {
                if name == "discord.com" {
                    Ok(vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::new(162, 159, 128, 233)), port)])
                } else {
                    Err(io::Error::new(io::ErrorKind::NotFound, "no such name"))
                }
            })
        });
        let to = self.addr;
        let dial: Dial = Arc::new(move |_| Box::pin(TcpStream::connect(to)));
        let mut roots = rustls::RootCertStore::empty();
        roots.add(self.cert.clone()).unwrap();
        Net::with(resolve, dial, roots, NetLimits::default())
    }

    /// Answers the next requests with `status`.
    fn answer(&self, status: u16) {
        self.status.store(status, Ordering::SeqCst);
    }

    fn posts(&self) -> Vec<Post> {
        self.posts.lock().unwrap().clone()
    }
}

impl Drop for Discord {
    fn drop(&mut self) {
        if let Some(rt) = self.rt.take() {
            rt.shutdown_background();
        }
    }
}

/// Keeps `req` in `posts` and answers it with `status`.
async fn keep(
    req: hyper::Request<hyper::body::Incoming>,
    status: Arc<AtomicU16>,
    posts: Arc<Mutex<Vec<Post>>>,
) -> Result<hyper::Response<Full<Bytes>>, Infallible> {
    let header = |name: &str| {
        req.headers().get(name).map(|h| String::from_utf8_lossy(h.as_bytes()).into_owned()).unwrap_or_default()
    };
    let (host, content_type) = (header("host"), header("content-type"));
    let (method, path) = (req.method().to_string(), req.uri().path().to_owned());
    let body = req.into_body().collect().await.map(|b| b.to_bytes()).unwrap_or_default();
    let body = String::from_utf8_lossy(&body).into_owned();
    posts.lock().unwrap().push(Post { method, host, path, content_type, body });
    Ok(hyper::Response::builder().status(status.load(Ordering::SeqCst)).body(Full::new(Bytes::new())).unwrap())
}

#[test]
fn the_discord_example_posts_to_its_webhook() {
    let Some(component) = component("discord-notify-wasm") else { return };
    let discord = Discord::start();
    let mut run = Run::of(component, "dev.autoscene.discord-notify-wasm", "post");
    // A label JSON has to escape, with a mention that mustn't ping anyone.
    run.label = Some("Cut \"1\" @everyone".into());
    run.permissions = vec![Permission::Network];
    run.hosts = vec!["discord.com".into()];
    run.secrets.insert("webhook".into(), "https://discord.com/api/webhooks/1234/s3cret-token".into());

    assert_eq!(run.run(discord.net()).unwrap().as_deref(), Some("Posted to Discord."));
    let posts = discord.posts();
    assert_eq!(posts.len(), 1, "{posts:?}");
    let post = &posts[0];
    assert_eq!(post.method, "POST");
    assert_eq!(post.host, "discord.com");
    assert_eq!(post.path, "/api/webhooks/1234/s3cret-token");
    assert_eq!(post.content_type, "application/json");
    let body: serde_json::Value = serde_json::from_str(&post.body).unwrap();
    assert_eq!(
        body,
        serde_json::json!({
            "content": "Export Cut \"1\" @everyone is done: 2 clips.",
            "allowed_mentions": { "parse": [] },
        })
    );
    assert!(run.log().contains("Posted to Discord."), "{}", run.log());

    // What Discord answers, in the person's words.
    for (status, message) in [
        (404, "Discord doesn't know that webhook. Make a new one in the channel's settings."),
        (401, "Discord doesn't know that webhook. Make a new one in the channel's settings."),
        (429, "Discord asks to wait before the next post. Try again in a minute."),
        (500, "Discord refused the post (HTTP 500)."),
    ] {
        discord.answer(status);
        match run.run(discord.net()) {
            Err(PluginError::Failed { message: got, .. }) => assert_eq!(got, message, "HTTP {status}"),
            other => panic!("HTTP {status}: {other:?}"),
        }
    }
    assert_eq!(discord.posts().len(), 5);
    // Neither a post nor a refusal writes the secret to the log.
    assert!(!run.log().contains("s3cret-token"), "{}", run.log());
}

#[test]
fn the_discord_example_posts_when_an_analysis_finishes() {
    let Some(component) = component("discord-notify-wasm") else { return };
    let discord = Discord::start();
    let mut run = Run::of(component, "dev.autoscene.discord-notify-wasm", "analysis-done");
    run.event = Some(EpisodeCounts { done: 7, failed: 1 });
    run.project = Some("Breaking \"Bad\" @here".into());
    run.permissions = vec![Permission::Network];
    run.hosts = vec!["discord.com".into()];
    run.secrets.insert("webhook".into(), "https://discord.com/api/webhooks/1234/s3cret-token".into());

    assert_eq!(run.run(discord.net()).unwrap().as_deref(), Some("Posted to Discord."));
    let contents = |posts: Vec<Post>| -> Vec<serde_json::Value> {
        posts.iter().map(|p| serde_json::from_str::<serde_json::Value>(&p.body).unwrap()).collect()
    };
    assert_eq!(
        contents(discord.posts()),
        [serde_json::json!({
            "content": "Breaking \"Bad\" @here: 7 episodes analysed, 1 failed.",
            "allowed_mentions": { "parse": [] },
        })]
    );
    let post = &discord.posts()[0];
    assert_eq!((post.method.as_str(), post.path.as_str()), ("POST", "/api/webhooks/1234/s3cret-token"));

    // One episode, none failed; and an event it doesn't have.
    run.event = Some(EpisodeCounts { done: 1, failed: 0 });
    run.run(discord.net()).unwrap();
    let last = contents(discord.posts()).pop().unwrap();
    assert_eq!(last["content"], "Breaking \"Bad\" @here: 1 episode analysed.");
    run.action = "export-done";
    match run.run(discord.net()) {
        Err(PluginError::Failed { message, .. }) => assert_eq!(message, "Discord Notify has no event export-done."),
        other => panic!("{other:?}"),
    }
    assert_eq!(discord.posts().len(), 2);
    assert!(!run.log().contains("s3cret-token"), "{}", run.log());
}

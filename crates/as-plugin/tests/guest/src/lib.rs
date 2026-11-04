//! A stand-in WebAssembly plugin for the sandbox tests (`tests/wasm.rs`,
//! and `tests/wasm_net.rs` for its requests).
//! Each action (`action.id`) does one thing a plugin may or may not do,
//! and says how it went; the settings carry its inputs: `paths`, `urls`
//! and `links` (one per line), `bytes` and `timeout-ms` (what a request
//! asks for as each of its timeouts). Built against the current world, it
//! also has `on-event`, which says what reached it (mode `event`).

#[cfg(not(feature = "frozen"))]
wit_bindgen::generate!({ world: "plugin", path: "../../wit" });
#[cfg(feature = "frozen")]
wit_bindgen::generate!({ world: "plugin", path: "../../wit-frozen/1.0.0" });

use std::fmt::Display;
use std::hint::black_box;
use std::io::Write;
use std::time::{Duration, SystemTime};
use std::{fs, thread};

use autoscene::plugin::host::{self, LogLevel};
use autoscene::plugin::types::{ActionKind, SettingValue, Trigger};
use wasip2::http::types::{ErrorCode, FutureIncomingResponse, IncomingResponse};

struct TestGuest;

fn err(message: impl Into<String>) -> RunError {
    RunError { message: message.into(), detail: None }
}

fn text(req: &RunRequest, key: &str) -> String {
    req.settings
        .iter()
        .find(|(k, _)| k == key)
        .and_then(|(_, v)| match v {
            SettingValue::Text(t) => Some(t.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

fn lines(req: &RunRequest, key: &str) -> Vec<String> {
    text(req, key).lines().filter(|l| !l.is_empty()).map(str::to_owned).collect()
}

fn number(req: &RunRequest, key: &str) -> Option<f64> {
    req.settings.iter().find(|(k, _)| k == key).and_then(|(_, v)| match v {
        SettingValue::Number(n) => Some(*n),
        _ => None,
    })
}

/// One line of a try: `ALLOWED`, or `denied (<why>)`.
fn tried<T, E: Display>(what: &str, r: Result<T, E>) -> String {
    match r {
        Ok(_) => format!("{what}: ALLOWED"),
        Err(e) => format!("{what}: denied ({e})"),
    }
}

fn hello(req: &RunRequest) -> Result<Option<String>, RunError> {
    host::progress(Some(0.5), Some("Writing"));
    host::log(LogLevel::Info, "hello from the guest");
    println!("stdout line from the guest");
    eprintln!("stderr line from the guest");
    fs::write("/data/hello.txt", "hello").map_err(|e| err(format!("write /data: {e}")))?;
    // Where WASI says the run starts, and where std starts.
    let initial = wasip2::cli::environment::initial_cwd();
    let cwd = std::env::current_dir().map(|d| d.display().to_string()).unwrap_or_default();
    let relative = tried("relative write", fs::write("relative.txt", "relative"));
    let env: Vec<String> = std::env::vars().map(|(k, v)| format!("{k}={v}")).collect();
    let args: Vec<String> = std::env::args().collect();
    host::progress(Some(1.0), None);
    Ok(Some(format!(
        "hello {}\nenv={}\nargs={}\ninitial-cwd={initial:?}\ncwd={cwd}\n{relative}",
        req.plugin.id,
        env.join(";"),
        args.join(";")
    )))
}

fn context(req: &RunRequest) -> String {
    let mut out = vec![
        format!("protocol={}", req.protocol),
        format!("run={}", req.run_id),
        format!("plugin={} {} {}", req.plugin.id, req.plugin.version, req.plugin.data_dir),
        format!("app={} {}", req.app.version, req.app.platform),
        format!(
            "action={} {} {}",
            match req.action.kind {
                ActionKind::ExportAction => "export-action",
                ActionKind::Command => "command",
            },
            req.action.id,
            match req.action.trigger {
                Trigger::Menu => "menu",
                Trigger::AfterExport => "after-export",
                Trigger::Palette => "palette",
                Trigger::Page => "page",
            }
        ),
    ];
    for (key, value) in &req.settings {
        out.push(match value {
            SettingValue::Boolean(b) => format!("setting {key}={b}"),
            SettingValue::Number(n) => format!("setting {key}={n}"),
            SettingValue::Text(t) => format!("setting {key}={}", t.replace('\n', "\\n")),
        });
    }
    if let Some(p) = &req.project {
        out.push(format!("project={} {} folder={:?}", p.id, p.title, p.folder));
    }
    if let Some(e) = &req.export {
        out.push(format!(
            "export={} {:?} {} out-dir={:?} created={} merged={:?}",
            e.id, e.label, e.preset, e.out_dir, e.created_at, e.merged_file
        ));
        for c in &e.clips {
            out.push(format!(
                "clip={} path={:?} {} {} {} {}-{} {:?}",
                c.name, c.path, c.status, c.media_id, c.episode, c.start, c.end, c.fps
            ));
        }
    }
    out.push(format!("input={:?}", req.input));
    out.join("\n")
}

/// Tries every way out of its folders it knows: one line per try.
fn escape(req: &RunRequest) -> String {
    let mut out = Vec::new();
    let fixed = ["/data/../x", "../x", "/x", "/export/../x", "/project/../x", "C:/Windows/win.ini", "/etc/passwd"];
    for p in lines(req, "paths").iter().map(String::as_str).chain(fixed) {
        out.push(tried(&format!("read {p}"), fs::read(p)));
        out.push(tried(&format!("list {p}"), fs::read_dir(p)));
    }
    for p in lines(req, "paths") {
        out.push(tried(&format!("write {p}"), fs::write(&p, "x")));
    }
    out.push(tried("write /x", fs::write("/x", "x")));
    out.push(tried("write /data/../x", fs::write("/data/../x", "x")));
    // Into and out of the folders it may only read, and between folders.
    for dir in ["/export", "/project"] {
        out.push(tried(&format!("write {dir}/new.txt"), fs::write(format!("{dir}/new.txt"), "x")));
        out.push(tried(&format!("mkdir {dir}/new"), fs::create_dir(format!("{dir}/new"))));
        out.push(tried(&format!("delete {dir}/remove-me.txt"), fs::remove_file(format!("{dir}/remove-me.txt"))));
        out.push(tried(
            &format!("rename {dir}/remove-me.txt to /data"),
            fs::rename(format!("{dir}/remove-me.txt"), "/data/moved.txt"),
        ));
        out.push(tried(
            &format!("rename /data into {dir}"),
            fs::write("/data/mine.txt", "x").and_then(|()| fs::rename("/data/mine.txt", format!("{dir}/mine.txt"))),
        ));
        out.push(tried(
            &format!("hard link {dir}/remove-me.txt into /data"),
            fs::hard_link(format!("{dir}/remove-me.txt"), "/data/linked.txt"),
        ));
        out.push(tried(
            &format!("hard link /data into {dir}"),
            fs::write("/data/mine.txt", "x")
                .and_then(|()| fs::hard_link("/data/mine.txt", format!("{dir}/linked.txt"))),
        ));
        out.push(tried(&format!("truncate {dir}/remove-me.txt"), fs::File::create(format!("{dir}/remove-me.txt"))));
    }
    // A link of its own that climbs out of /data, and one to a host path.
    out.push(tried("symlink /data/l to ../..", symlink("../..", "l")));
    out.push(tried("read through /data/l", fs::read_dir("/data/l")));
    out.push(tried("read /data/l/x", fs::read("/data/l/x")));
    for p in lines(req, "paths") {
        let _ = fs::remove_file("/data/host-link");
        out.push(tried(&format!("symlink /data/host-link to {p}"), symlink(&p, "host-link")));
        out.push(tried("read /data/host-link", fs::read("/data/host-link")));
    }
    out.join("\n")
}

/// Reads the large files `paths` names (one name per line) in
/// `/resources` and copies each into `/data`, then tries to change them: one
/// line per try, after the folders it was given (`preopens=`).
fn read_resources(req: &RunRequest) -> String {
    let dirs = wasip2::filesystem::preopens::get_directories();
    let mut folders: Vec<String> = dirs.into_iter().map(|(_, path)| path).collect();
    folders.sort();
    let mut out = vec![format!("preopens={}", folders.join(","))];
    out.push(tried("list /resources", fs::read_dir("/resources")));
    for name in lines(req, "paths") {
        let file = format!("/resources/{name}");
        let copied = fs::read(&file).and_then(|bytes| fs::write(format!("/data/{name}"), &bytes).map(|()| bytes.len()));
        out.push(match copied {
            Ok(n) => format!("read {file}: {n} bytes"),
            Err(e) => format!("read {file}: denied ({e})"),
        });
        out.push(tried(&format!("write {file}"), fs::write(&file, "x")));
        out.push(tried(&format!("append {file}"), fs::OpenOptions::new().append(true).open(&file)));
        out.push(tried(&format!("truncate {file}"), fs::File::create(&file)));
        out.push(tried(&format!("delete {file}"), fs::remove_file(&file)));
        out.push(tried(&format!("rename {file} to /data"), fs::rename(&file, "/data/moved")));
        out.push(tried(
            &format!("rename /data into {file}"),
            fs::write("/data/mine", "x").and_then(|()| fs::rename("/data/mine", &file)),
        ));
        out.push(tried(&format!("hard link {file} into /data"), fs::hard_link(&file, "/data/linked")));
        out.push(tried(
            &format!("hard link /data into {file}"),
            fs::write("/data/mine", "x").and_then(|()| fs::hard_link("/data/mine", format!("{file}.linked"))),
        ));
        out.push(tried(
            &format!("set times of {file}"),
            fs::File::open(&file).and_then(|f| f.set_modified(SystemTime::UNIX_EPOCH)),
        ));
    }
    out.push(tried("write /resources/new", fs::write("/resources/new", "x")));
    out.push(tried("mkdir /resources/new-dir", fs::create_dir("/resources/new-dir")));
    // Out of it, to a file that is there: `outside/secret.txt` lies beside
    // the folder. Its error's kind says whether the file was found.
    let up = "/resources/../outside/secret.txt";
    out.push(match fs::read(up) {
        Ok(bytes) => format!("read {up}: ALLOWED ({} bytes)", bytes.len()),
        Err(e) => format!("read {up}: denied ({:?}: {e})", e.kind()),
    });
    out.join("\n")
}

/// Makes the links each line of `links` names, `<name> <target>`, in
/// `/data`: one line per link.
fn links(req: &RunRequest) -> String {
    let made = lines(req, "links").into_iter().map(|line| {
        let (name, target) = line.split_once(' ').unwrap_or((&line, ""));
        tried(&format!("link /data/{name} to {target}"), symlink(target, name))
    });
    made.collect::<Vec<_>>().join("\n")
}

/// Makes the link `/data/<name>` to `target` (std's own is unstable).
fn symlink(target: &str, name: &str) -> Result<(), String> {
    let dirs = wasip2::filesystem::preopens::get_directories();
    let (data, _) = dirs.iter().find(|(_, path)| path == "/data").ok_or("no /data")?;
    data.symlink_at(target, name).map_err(|e| format!("{e:?}"))
}

fn sockets() -> String {
    use std::net::{TcpStream, ToSocketAddrs, UdpSocket};
    [
        tried("tcp connect", TcpStream::connect("93.184.215.14:443")),
        tried("tcp connect to this computer", TcpStream::connect("127.0.0.1:443")),
        tried("udp bind", UdpSocket::bind("0.0.0.0:0")),
        tried("name lookup", "example.com:443".to_socket_addrs()),
    ]
    .join("\n")
}

/// The options a request asks for: each timeout `ms` long, when given.
fn options(ms: Option<f64>) -> Option<wasip2::http::types::RequestOptions> {
    let ns = (ms? * 1e6) as u64;
    let options = wasip2::http::types::RequestOptions::new();
    let _ = options.set_connect_timeout(Some(ns));
    let _ = options.set_first_byte_timeout(Some(ns));
    let _ = options.set_between_bytes_timeout(Some(ns));
    Some(options)
}

fn fetch(url: &str, host_header: Option<&str>, timeout_ms: Option<f64>) -> String {
    use wasip2::http::outgoing_handler;
    use wasip2::http::types::{Fields, OutgoingRequest, Scheme};
    let (scheme, rest) = match url.split_once("://") {
        Some(("http", r)) => (Scheme::Http, r),
        Some(("https", r)) => (Scheme::Https, r),
        _ => return format!("{url}: bad url"),
    };
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let headers = Fields::new();
    if let Some(h) = host_header
        && let Err(e) = headers.set("host", &[h.as_bytes().to_vec()])
    {
        return format!("{url}: host header refused: {e:?}");
    }
    let req = OutgoingRequest::new(headers);
    let _ = req.set_scheme(Some(&scheme));
    let _ = req.set_authority(Some(authority));
    let _ = req.set_path_with_query(Some(path));
    match outgoing_handler::handle(req, options(timeout_ms)) {
        Err(code) => format!("{url}: error {code:?}"),
        Ok(fut) => {
            fut.subscribe().block();
            match fut.get() {
                Some(Ok(Ok(resp))) => {
                    // So a test knows the head is in.
                    host::log(LogLevel::Info, &format!("{url}: head in"));
                    format!("{url}: status {}{}", resp.status(), answer(&resp))
                }
                Some(Ok(Err(code))) => format!("{url}: error {code:?}"),
                other => format!("{url}: no answer {other:?}"),
            }
        }
    }
}

/// The rest of an answer: where it points, when it does (it isn't
/// followed), and how much of its body came, or how reading it failed.
fn answer(resp: &wasip2::http::types::IncomingResponse) -> String {
    use wasip2::http::types::http_error_code;
    use wasip2::io::streams::StreamError;
    let mut out = String::new();
    if let Some(to) = resp.headers().get("location").first() {
        out += &format!(" location {}", String::from_utf8_lossy(to));
    }
    let Ok(body) = resp.consume() else { return out + ", no body" };
    let Ok(stream) = body.stream() else { return out + ", no body" };
    let mut bytes = 0;
    loop {
        match stream.blocking_read(64 << 10) {
            Ok(part) => bytes += part.len(),
            Err(StreamError::Closed) => return out + &format!(", {bytes} bytes"),
            Err(StreamError::LastOperationFailed(e)) => {
                let code = http_error_code(&e).map_or_else(|| e.to_debug_string(), |code| format!("{code:?}"));
                return out + &format!(", body error {code} after {bytes} bytes");
            }
        }
    }
}

/// Starts a GET of `url` (`https://` only), without waiting for it.
fn start(url: &str) -> Result<FutureIncomingResponse, ErrorCode> {
    use wasip2::http::outgoing_handler;
    use wasip2::http::types::{Fields, OutgoingRequest, Scheme};
    let rest = url.strip_prefix("https://").unwrap_or(url);
    let (authority, path) = rest.split_once('/').map_or((rest, "/".to_owned()), |(a, p)| (a, format!("/{p}")));
    let req = OutgoingRequest::new(Fields::new());
    let _ = req.set_scheme(Some(&Scheme::Https));
    let _ = req.set_authority(Some(authority));
    let _ = req.set_path_with_query(Some(&path));
    outgoing_handler::handle(req, None)
}

/// The answer to a request [`start`] made, once it is in.
fn awaited(fut: &FutureIncomingResponse) -> Result<IncomingResponse, ErrorCode> {
    fut.subscribe().block();
    match fut.get() {
        Some(Ok(r)) => r,
        _ => Err(ErrorCode::InternalError(Some("no answer".into()))),
    }
}

fn fetch_many(url: &str) -> String {
    let mut pending = Vec::new();
    let mut out = Vec::new();
    for i in 0..9 {
        match start(url) {
            Ok(fut) => pending.push((i, fut)),
            Err(code) => out.push(format!("{i}: error {code:?}")),
        }
    }
    for (i, fut) in pending {
        fut.subscribe().block();
        out.push(match fut.get() {
            Some(Ok(Ok(resp))) => format!("{i}: status {}", resp.status()),
            Some(Ok(Err(code))) => format!("{i}: error {code:?}"),
            other => format!("{i}: no answer {other:?}"),
        });
    }
    out.join("\n")
}

/// Holds the answers of eight requests to `held` (heads in, bodies unread)
/// while it makes a ninth to `then`, then lets them go and makes a tenth,
/// as soon as it may.
fn fetch_held(held: &str, then: &str) -> String {
    let mut out = Vec::new();
    let mut answers = Vec::new();
    for i in 0..8 {
        match start(held).and_then(|fut| awaited(&fut)) {
            Ok(resp) => {
                out.push(format!("{i}: status {}", resp.status()));
                answers.push(resp);
            }
            Err(code) => out.push(format!("{i}: error {code:?}")),
        }
    }
    // Their bodies wait, and each still holds its place.
    thread::sleep(Duration::from_millis(100));
    out.push(match start(then).and_then(|fut| awaited(&fut)) {
        Ok(resp) => format!("8: status {}", resp.status()),
        Err(code) => format!("8: error {code:?}"),
    });
    drop(answers);
    // The places come back as their connections end.
    for tries in 1.. {
        match start(then).and_then(|fut| awaited(&fut)) {
            Err(ErrorCode::ConnectionLimitReached) if tries < 100 => thread::sleep(Duration::from_millis(20)),
            Ok(resp) => {
                out.push(format!("9: status {}{}", resp.status(), answer(&resp)));
                break;
            }
            Err(code) => {
                out.push(format!("9: error {code:?}"));
                break;
            }
        }
    }
    out.join("\n")
}

/// Calls itself without end, through a pointer the compiler can't see
/// through, so each call takes a frame of wasmtime's stack (and next to
/// nothing of the one in its memory).
#[inline(never)]
fn recurse(n: u64) -> u64 {
    if n == u64::MAX {
        return 0;
    }
    let next: fn(u64) -> u64 = black_box(recurse);
    next(n + 1) ^ n.rotate_left(7)
}

impl Guest for TestGuest {
    fn run(req: RunRequest) -> Result<Option<String>, RunError> {
        match req.action.id.as_str() {
            "hello" => hello(&req),
            "context" => Ok(Some(context(&req))),
            "escape" => Ok(Some(escape(&req))),
            "links" => Ok(Some(links(&req))),
            "read-resources" => Ok(Some(read_resources(&req))),
            "write-export" => {
                fs::write("/export/out.txt", "out").map_err(|e| err(format!("write /export/out.txt: {e}")))?;
                fs::remove_file("/export/remove-me.txt").map_err(|e| err(format!("delete: {e}")))?;
                Ok(Some("written".into()))
            }
            "sockets" => Ok(Some(sockets())),
            "spin" => {
                let mut x: u64 = 1;
                loop {
                    x = black_box(x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1));
                }
            }
            "spin-polite" => {
                let mut n: u64 = 0;
                loop {
                    n = black_box(n + 1);
                    if n.is_multiple_of(100_000) && host::cancelled() {
                        return Err(err("Stopped."));
                    }
                }
            }
            "spin-polite-result" => {
                let mut n: u64 = 0;
                loop {
                    n = black_box(n + 1);
                    if n.is_multiple_of(100_000) && host::cancelled() {
                        return Ok(Some("stopped with a result".into()));
                    }
                }
            }
            "sleep" => {
                thread::sleep(Duration::from_secs(60));
                Ok(Some("slept".into()))
            }
            "hog" => {
                let mut keep: Vec<Vec<u8>> = Vec::new();
                loop {
                    keep.push(black_box(vec![1u8; 16 << 20]));
                }
            }
            "hog-polite" => {
                let mut keep: Vec<Vec<u8>> = Vec::new();
                loop {
                    let mut block = Vec::new();
                    if block.try_reserve_exact(16 << 20).is_err() {
                        return Err(err("No more memory."));
                    }
                    block.resize(16 << 20, 1u8);
                    keep.push(block);
                }
            }
            "refused-then-panic" => {
                // Refused, minded, and then more memory that fits: the
                // panic after is no want of memory.
                let mut huge: Vec<u8> = Vec::new();
                if huge.try_reserve_exact(1 << 30).is_ok() {
                    return Err(err("1 GiB was allowed"));
                }
                let fits = black_box(vec![1u8; 8 << 20]);
                panic!("guest panicked after {} bytes", fits.len());
            }
            "huge" => {
                // 100 MB, under wasmtime's 128 MiB a call may carry.
                let text = "a ".repeat(50_000_000);
                host::log(LogLevel::Info, &text);
                host::progress(None, Some(&text));
                Ok(Some(text))
            }
            "recurse" => Ok(Some(recurse(0).to_string())),
            "panic" => panic!("guest panicked on purpose"),
            "flood" => {
                let line = "x".repeat(1023);
                let (mut out, mut errs) = (std::io::stdout().lock(), std::io::stderr().lock());
                for _ in 0..(50 << 10) {
                    let _ = writeln!(out, "{line}");
                    let _ = writeln!(errs, "{line}");
                }
                drop((out, errs));
                for i in 0..10_000 {
                    host::log(LogLevel::Info, &format!("log line {i}"));
                }
                for i in 0..100_000u32 {
                    host::progress(Some(f64::from(i) / 100_000.0), Some("Flooding"));
                }
                Ok(Some("flooded".into()))
            }
            "resources" => {
                let mut keep = Vec::new();
                for _ in 0..200_000 {
                    keep.push(wasip2::clocks::monotonic_clock::subscribe_duration(1_000_000_000));
                }
                Ok(Some(format!("kept {}", keep.len())))
            }
            "random" => {
                let bytes = number(&req, "bytes").unwrap_or(f64::from(64u32 << 20)) as u64;
                Ok(Some(format!("got {}", wasip2::random::random::get_random_bytes(bytes).len())))
            }
            "fail" => Err(RunError { message: "It failed on purpose.".into(), detail: Some("details".into()) }),
            "fetch" => {
                let timeout_ms = number(&req, "timeout-ms");
                let said: Vec<String> = lines(&req, "urls").iter().map(|u| fetch(u, None, timeout_ms)).collect();
                Ok(Some(said.join("\n")))
            }
            "fetch-many" => Ok(Some(fetch_many(lines(&req, "urls").first().map_or("", String::as_str)))),
            "fetch-held" => {
                let urls = lines(&req, "urls");
                let url = |i: usize| urls.get(i).map_or("", String::as_str);
                Ok(Some(fetch_held(url(0), url(1))))
            }
            "fetch-host" => {
                let urls = lines(&req, "urls");
                let other = urls.get(1).map_or("other.example", String::as_str);
                Ok(Some(fetch(urls.first().map_or("", String::as_str), Some(other), None)))
            }
            "secret" => {
                let some = host::secret("token").is_some();
                Ok(Some(format!("secret token is {}", if some { "some" } else { "none" })))
            }
            // How its export went (host.export-status, since 1.1.0).
            #[cfg(not(feature = "frozen"))]
            "export-status" => {
                Ok(Some(format!("export status {}", host::export_status().as_deref().unwrap_or("none"))))
            }
            // The world it was built against.
            "world" => Ok(Some(if cfg!(feature = "frozen") { "1.0.0" } else { "1.1.0" }.into())),
            other => Err(err(format!("unknown action {other}"))),
        }
    }

    #[cfg(not(feature = "frozen"))]
    fn on_event(req: EventRequest) -> Result<Option<String>, RunError> {
        host::progress(Some(1.0), Some("Telling"));
        host::log(LogLevel::Info, &format!("event {}", req.id));
        if req.id == "fail" {
            return Err(err("The event failed on purpose."));
        }
        let e = &req.event;
        let project = req
            .project
            .as_ref()
            .map_or_else(|| "none".into(), |p| format!("{} {} {}", p.id, p.title, p.folder.as_deref().unwrap_or("-")));
        let settings: Vec<&str> = req.settings.iter().map(|(k, _)| k.as_str()).collect();
        let lines = [
            format!("event {} {} at={} done={} failed={}", req.id, e.kind, e.at, e.episodes.done, e.episodes.failed),
            format!("project={project}"),
            format!("settings={}", settings.join(",")),
            format!(
                "protocol={} run={} plugin={} {} {}",
                req.protocol,
                req.run_id,
                req.plugin.id,
                req.plugin.data_dir,
                fs::read_dir("/data").is_ok()
            ),
        ];
        Ok(Some(lines.join("\n")))
    }
}

export!(TestGuest);

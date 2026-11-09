//! A stand-in plugin for the runtime tests (`tests/runtime.rs`), never
//! shipped. It speaks the protocol as a plugin would, and misbehaves as one
//! might; its first argument says how. Files it writes go in its current
//! folder, the run's data folder.

use std::ffi::OsStr;
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use std::{env, fs, process, thread};

use serde_json::{Value, json};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut stdin = io::stdin().lock();
    let mut first = String::new();
    let mut read_run = || {
        stdin.read_line(&mut first).expect("stdin");
    };
    match args.first().map(String::as_str).unwrap_or("") {
        // The grandchild of the cancel tests: shows it's alive.
        "heartbeat" => loop {
            let mut file = fs::OpenOptions::new().create(true).append(true).open(&args[1]).expect("heartbeat");
            file.write_all(b".").expect("heartbeat");
            thread::sleep(Duration::from_millis(50));
        },
        "result" => {
            read_run();
            send(json!({"type": "progress", "fraction": 0.5, "stage": "Posting"}));
            send(json!({"type": "result", "message": "Posted 12 clips."}));
        }
        "quiet" => read_run(),
        // A shot filter: keeps the shots with a face at least 0.2 tall, says
        // something of a shot it wasn't sent, and changes its mind on the
        // first one.
        "filter" => {
            read_run();
            let run: Value = serde_json::from_str(&first).expect("run line");
            let shots = run["shots"]["shots"].as_array().cloned().unwrap_or_default();
            for shot in &shots {
                let keep = shot["faceHeight"].as_f64().unwrap_or(0.0) >= 0.2;
                send(json!({"type": "verdict", "shot": shot["key"], "keep": keep}));
            }
            send(json!({"type": "verdict", "shot": "elsewhere:1", "keep": false}));
            send(json!({"type": "progress", "fraction": 1.0, "stage": "Listening"}));
            if let Some(shot) = shots.first() {
                send(json!({"type": "verdict", "shot": shot["key"], "keep": false}));
            }
            send(json!({"type": "result", "message": format!("Checked {} shots.", shots.len())}));
        }
        // A pack action: writes each clip of the pack, a line each, to the
        // file it is given; with "fail", it then says it failed.
        "pack" => {
            read_run();
            let run: Value = serde_json::from_str(&first).expect("run line");
            let clips = run["pack"]["clips"].as_array().cloned().unwrap_or_default();
            let lines: String = clips
                .iter()
                .map(|c| format!("{} {} {}-{}\n", c["index"], c["name"].as_str().unwrap_or(""), c["start"], c["end"]))
                .collect();
            fs::write(run["output"].as_str().expect("an output"), lines).expect("output");
            match args.get(1).map(String::as_str) {
                Some("fail") => send(json!({"type": "error", "message": "The clips don't fit a timeline."})),
                _ => send(json!({"type": "result", "message": format!("Saved {} clips.", clips.len())})),
            }
        }
        // A source: writes a part in partDir, moves it to outDir as
        // "<title>.mkv", says so, and also names a file outside outDir and
        // one in partDir; with "fail", it then says it failed; with "hang",
        // it waits for the cancel line after its first file.
        "source" => {
            read_run();
            let run: Value = serde_json::from_str(&first).expect("run line");
            let src = &run["source"];
            let (out, part) = (src["outDir"].as_str().expect("outDir"), src["partDir"].as_str().expect("partDir"));
            let link = src["link"].as_str().unwrap_or("");
            let title = link.rsplit('=').next().unwrap_or("video");
            let partial = std::path::Path::new(part).join(format!("{title}.mkv.part"));
            fs::write(&partial, b"not a real video").expect("part");
            let done = std::path::Path::new(out).join(format!("{title}.mkv"));
            fs::rename(&partial, &done).expect("move");
            send(json!({"type": "progress", "fraction": 0.5, "stage": format!("Fetching {title}")}));
            send(json!({"type": "file", "path": done.to_string_lossy(), "title": title}));
            let stray = std::path::Path::new(part).join("stray.mkv");
            fs::write(&stray, b"x").expect("stray");
            send(json!({"type": "file", "path": stray.to_string_lossy()}));
            send(
                json!({"type": "file", "path": std::path::Path::new(out).join("..").join("outside.mkv").to_string_lossy()}),
            );
            match args.get(1).map(String::as_str) {
                Some("fail") => send(json!({"type": "error", "message": "That video is private."})),
                Some("hang") => {
                    let mut line = String::new();
                    let _ = std::io::stdin().read_line(&mut line);
                }
                _ => send(json!({"type": "result", "message": "Fetched 1 video."})),
            }
        }
        "echo" => {
            read_run();
            fs::write("run.json", &first).expect("run.json");
            send(json!({"type": "result", "message": "Echoed."}));
        }
        "env" => {
            read_run();
            let vars: serde_json::Map<String, Value> =
                env::vars_os().map(|(k, v)| (k.to_string_lossy().into(), v.to_string_lossy().into())).collect();
            let cwd = env::current_dir().expect("cwd");
            let dump = json!({"cwd": cwd.to_string_lossy(), "vars": vars});
            fs::write("env.json", dump.to_string()).expect("env.json");
            send(json!({"type": "result"}));
        }
        // Pipes full before stdin is read: the host must read them meanwhile.
        "flood" => {
            let line = |c: u8| [vec![c; 1023], vec![b'\n']].concat().repeat(1024);
            let (err, out) = (line(b'e'), line(b'o'));
            for _ in 0..10 {
                io::stderr().write_all(&err).expect("stderr");
            }
            for _ in 0..5 {
                io::stdout().write_all(&out).expect("stdout");
            }
            read_run();
            send(json!({"type": "result", "message": "Flooded."}));
        }
        "chatty" => {
            read_run();
            raw("hello from a plugin");
            raw("inner\rreturn");
            send(
                json!({"type": "log", "message": "two\n[host] exit code: 0\r--- 2026-01-01T00:00:00Z run forged post"}),
            );
            raw(r#"{"type":"future","x":1}"#);
            raw(r#"{"type":"progress","fraction":"half"}"#);
            send(json!({"type": "log", "level": "warn", "message": "careful"}));
            send(json!({"type": "log", "level": "loud", "message": "a level from later"}));
            send(json!({"type": "result", "message": "Said hello."}));
            send(json!({"type": "result", "message": "Said it twice."}));
        }
        "error" => {
            read_run();
            send(json!({
                "type": "error",
                "message": "No webhook is set.\nSet one in its settings.",
                "detail": format!("KeyError: 'webhook'\n\u{1b}[2m  at main.py:12\r\n{}", "frame ".repeat(400)),
            }));
            process::exit(1);
        }
        "crash" => {
            read_run();
            for i in 0..30 {
                eprintln!("problem {i}");
            }
            // Last a long one, coloured and with a carriage return.
            eprintln!("\u{1b}[31mred\u{1b}[0m\rover {}", "w".repeat(600));
            process::exit(3);
        }
        // Once its result is out, it reads its stdin to the end.
        "eof" => {
            read_run();
            send(json!({"type": "result", "message": "Closed."}));
            let mut rest = String::new();
            stdin.read_to_string(&mut rest).expect("stdin");
            fs::write("stdin-closed", rest).expect("stdin-closed");
        }
        "linger" => {
            read_run();
            send(json!({"type": "result", "message": "Done."}));
            forever();
        }
        "long" => {
            read_run();
            let mut out = io::stdout().lock();
            out.write_all(&[vec![b'x'; 1 << 20], vec![b'\n']].concat()).expect("stdout");
            drop(out);
            send(json!({"type": "result", "message": "Long."}));
            // Never ended.
            io::stderr().write_all("z".repeat(1 << 20).as_bytes()).expect("stderr");
        }
        // A result line past the limit, whose first 64 KB would read as one.
        "padded" => {
            read_run();
            raw(&format!(r#"{{"type":"result","message":"x"}}{}"#, " ".repeat(70_000)));
        }
        "stages" => {
            read_run();
            send(json!({"type": "progress", "fraction": 0.5, "stage": "  Posting\u{7} clips\n\n to   Discord "}));
            send(json!({"type": "progress", "fraction": 7, "stage": "a".repeat(200)}));
            send(json!({"type": "progress", "fraction": -1}));
            send(json!({"type": "progress", "fraction": null, "stage": "\u{202E}\u{200B} "}));
            send(json!({"type": "result", "message": "word ".repeat(100)}));
        }
        // Asked to cancel, it says so and leaves its grandchild behind.
        "cancel" => {
            read_run();
            grandchild();
            send(json!({"type": "progress", "stage": "Waiting"}));
            loop {
                let mut line = String::new();
                if stdin.read_line(&mut line).expect("stdin") == 0 {
                    process::exit(4);
                }
                if line.contains(r#""cancel""#) {
                    fs::write("cancel-seen", line).expect("cancel-seen");
                    process::exit(0);
                }
            }
        }
        "ignore" => {
            read_run();
            grandchild();
            forever();
        }
        // Protocol 2: a call, a progress line while it waits, then its reply.
        "call" => {
            read_run();
            fs::write("run.json", &first).expect("run.json");
            send(json!({"type": "call", "id": 7, "method": "settings.get", "params": {}}));
            send(json!({"type": "progress", "stage": "Waiting for AutoScene"}));
            let mut reply = String::new();
            stdin.read_line(&mut reply).expect("stdin");
            fs::write("reply.json", reply).expect("reply.json");
            send(json!({"type": "result", "message": "Called."}));
        }
        // As many calls as its second argument says at once, then their replies.
        "call-many" => {
            read_run();
            let n: u64 = args[1].parse().expect("a count");
            for id in 1..=n {
                send(json!({"type": "call", "id": id, "method": "settings.get"}));
            }
            let mut replies = String::new();
            for _ in 0..n {
                stdin.read_line(&mut replies).expect("stdin");
            }
            fs::write("replies.json", replies).expect("replies.json");
            send(json!({"type": "result", "message": "Called."}));
        }
        // A call after its result, then it reads its stdin to the end.
        "call-late" => {
            read_run();
            send(json!({"type": "result", "message": "Done."}));
            send(json!({"type": "call", "id": 1, "method": "settings.get"}));
            let mut rest = String::new();
            stdin.read_to_string(&mut rest).expect("stdin");
            fs::write("stdin-closed", rest).expect("stdin-closed");
        }
        "call-unknown" => {
            read_run();
            send(json!({"type": "call", "id": "a", "method": "files.delete", "params": {"path": "/"}}));
            let mut reply = String::new();
            stdin.read_line(&mut reply).expect("stdin");
            fs::write("reply.json", reply).expect("reply.json");
            send(json!({"type": "result", "message": "Called."}));
        }
        // A call, then it exits at once: its answer comes after it.
        "call-exit" => {
            read_run();
            send(json!({"type": "call", "id": 1, "method": "settings.get"}));
            // Long enough for the call to start.
            thread::sleep(Duration::from_millis(300));
        }
        // As many calls as its second argument says, then its result before any reply.
        "call-and-go" => {
            read_run();
            let n: u64 = args[1].parse().expect("a count");
            for id in 1..=n {
                send(json!({"type": "call", "id": id, "method": "settings.get"}));
            }
            thread::sleep(Duration::from_millis(500));
            send(json!({"type": "result", "message": "Gone."}));
        }
        // As many calls as its second argument says, and it never reads a
        // reply: its result comes the third argument's ms later.
        "call-deaf" => {
            read_run();
            let n: u64 = args[1].parse().expect("a count");
            for id in 1..=n {
                send(json!({"type": "call", "id": id, "method": "settings.get"}));
            }
            thread::sleep(Duration::from_millis(args[2].parse().expect("ms")));
            send(json!({"type": "result", "message": "Deaf."}));
        }
        // Calls long, too long and broken, each with a secret in its params.
        "call-long" => {
            read_run();
            let secret = "SECRET-MARKER";
            // Long, within its cap: answered.
            let value = "v".repeat(200_000);
            send(json!({"type": "call", "id": 1, "method": "state.set", "params": {"key": "k", "value": value}}));
            // Past the longest call line: refused by its id, which comes first.
            let long = format!("{secret}{}", "x".repeat(6 << 20));
            send(json!({"type": "call", "id": 2, "method": "state.set", "params": {"key": "k", "value": long}}));
            // Its id after its params, which the cut takes: dropped.
            raw(&format!(r#"{{"type":"call","method":"net.fetch","params":{{"body":"{long}"}},"id":3}}"#));
            // An id no reply can carry: dropped.
            send(json!({"type": "call", "id": -1, "method": "state.get", "params": {"key": secret}}));
            // No method: refused by its id.
            send(json!({"type": "call", "id": 4, "params": {"key": secret}}));
            let mut replies = String::new();
            for _ in 0..3 {
                stdin.read_line(&mut replies).expect("stdin");
            }
            fs::write("replies.json", replies).expect("replies.json");
            send(json!({"type": "result", "message": "Called."}));
        }
        // As many calls as its second argument says; once asked to cancel,
        // one more, and every line it reads until that one's reply.
        "call-cancelled" => {
            read_run();
            let n: u64 = args[1].parse().expect("a count");
            for id in 1..=n {
                send(json!({"type": "call", "id": id, "method": "settings.get"}));
            }
            let mut read = String::new();
            while !read.contains(r#""cancel""#) {
                stdin.read_line(&mut read).expect("stdin");
            }
            send(json!({"type": "call", "id": 99, "method": "state.set", "params": {"key": "k", "value": 1}}));
            while !read.contains(r#""id":99"#) {
                stdin.read_line(&mut read).expect("stdin");
            }
            fs::write("replies.json", read).expect("replies.json");
        }
        // Reads its secret, then fails with what it has on stderr: its
        // environment and how long the reply was, never what it said.
        "call-secret" => {
            read_run();
            fs::write("run.json", &first).expect("run.json");
            send(json!({"type": "call", "id": 1, "method": "secrets.get", "params": {"key": "token"}}));
            let mut reply = String::new();
            stdin.read_line(&mut reply).expect("stdin");
            for (key, value) in env::vars_os() {
                eprintln!("{}={}", key.to_string_lossy(), value.to_string_lossy());
            }
            eprintln!("a reply of {} bytes", reply.len());
            process::exit(1);
        }
        // Service mode: see `serve`.
        mode @ ("serve"
        | "serve-slow-start"
        | "serve-progress-flood"
        | "serve-crash"
        | "serve-ignore-stop"
        | "serve-emit-flood"
        | "serve-call"
        | "serve-deaf") => {
            read_run();
            serve(mode, &mut stdin, &first);
        }
        // Its scratch folder: what its run line and its environment say of
        // it (`scratch.json`), and a few files it writes there.
        "scratch" => {
            read_run();
            scratch(&first);
            send(json!({"type": "result", "message": "Scratched."}));
        }
        // As `scratch`, then it waits for its cancel line.
        "scratch-sleep" => {
            read_run();
            scratch(&first);
            send(json!({"type": "progress", "stage": "Waiting"}));
            loop {
                let mut line = String::new();
                if stdin.read_line(&mut line).expect("stdin") == 0 {
                    process::exit(4);
                }
                if line.contains(r#""cancel""#) {
                    process::exit(0);
                }
            }
        }
        // As `scratch`, then a grandchild holds a file in its scratch folder,
        // and it exits once the file is held: the grandchild outlives it
        // until the run ends its tree.
        "scratch-hold" => {
            read_run();
            let temp = scratch(&first).expect("a scratch folder");
            holder(&temp.join("held.bin"));
            let until = Instant::now() + Duration::from_secs(20);
            while !Path::new("holding").exists() {
                assert!(Instant::now() < until, "the grandchild never held its file");
                thread::sleep(Duration::from_millis(20));
            }
            send(json!({"type": "result", "message": "Held."}));
        }
        // The grandchild of `scratch-hold`: opens the file it is given so that
        // nothing may delete it (on Windows; elsewhere an open file is no
        // hold), says so in the file it is given second, and waits.
        "hold" => {
            let mut open = fs::OpenOptions::new();
            open.read(true).write(true).create(true).truncate(false);
            #[cfg(windows)]
            std::os::windows::fs::OpenOptionsExt::share_mode(&mut open, 0);
            let _held = open.open(&args[1]).expect("held file");
            fs::write(&args[2], "").expect("holding");
            forever();
        }
        // A plugin of protocol 1 that prints a line like a call: never answered.
        "protocol1-call" => {
            read_run();
            raw(r#"{"type":"call","id":1,"method":"settings.get","params":{}}"#);
            send(json!({"type": "result", "message": "Done."}));
            let mut rest = String::new();
            stdin.read_to_string(&mut rest).expect("stdin");
            fs::write("stdin-closed", rest).expect("stdin-closed");
        }
        other => panic!("no such mode: {other}"),
    }
}

/// A service in `mode`, whose serve line was `first`: it writes that line
/// and its mode's variable, says it is ready (after its progress, with
/// `serve-slow-start`, and after 5000 lines of it with
/// `serve-progress-flood`), then answers each request by its method until
/// its stop or its stdin's end. `serve-slow-start` says one more progress
/// once ready; `serve-crash` answers none, and crashes at the third;
/// `serve-ignore-stop` leaves a grandchild and stays after its stop;
/// `serve-emit-flood` emits 200 events at once first, then one more a
/// second later; `serve-call` calls AutoScene for its `call` method;
/// `serve-deaf` never reads its stdin again, for a minute at most.
fn serve(mode: &str, stdin: &mut impl BufRead, first: &str) {
    fs::write("serve.json", first).expect("serve.json");
    fs::write("mode", env::var("AUTOSCENE_MODE").unwrap_or_default()).expect("mode");
    fs::write("resources", env::var("AUTOSCENE_RESOURCES").unwrap_or_default()).expect("resources");
    // Its scratch folder, as its environment names it, with a file in it.
    let temp = env::var("AUTOSCENE_RUN_TEMP").unwrap_or_default();
    if !temp.is_empty() {
        fs::write(Path::new(&temp).join("served.txt"), "served").expect("served.txt");
    }
    fs::write("run-temp", temp).expect("run-temp");
    if mode == "serve-slow-start" {
        send(json!({"type": "progress", "fraction": 0.5, "stage": "Getting Python"}));
        thread::sleep(Duration::from_millis(300));
        send(json!({"type": "progress", "stage": "Installing packages"}));
    }
    if mode == "serve-progress-flood" {
        for i in 0..5000 {
            send(json!({"type": "progress", "fraction": f64::from(i) / 5000.0, "stage": format!("Step {i}")}));
        }
    }
    if mode == "serve-ignore-stop" {
        grandchild();
    }
    send(json!({"type": "ready"}));
    if mode == "serve-slow-start" {
        send(json!({"type": "progress", "stage": "Warming up"}));
    }
    if mode == "serve-deaf" {
        thread::sleep(Duration::from_secs(60));
        return;
    }
    if mode == "serve-emit-flood" {
        for i in 0..200 {
            send(json!({"type": "emit", "event": "tick", "data": i}));
        }
        thread::sleep(Duration::from_millis(1100));
        send(json!({"type": "emit", "event": "tock"}));
    }
    let mut asked = 0;
    loop {
        let mut line = String::new();
        if stdin.read_line(&mut line).expect("stdin") == 0 {
            return;
        }
        let Ok(m) = serde_json::from_str::<Value>(&line) else { continue };
        match m["type"].as_str() {
            Some("stop") => {
                fs::write("stop-seen", &line).expect("stop-seen");
                if mode != "serve-ignore-stop" {
                    return;
                }
            }
            Some("settings") => fs::write("settings.json", &line).expect("settings.json"),
            Some("request") if mode == "serve-crash" => {
                asked += 1;
                if asked == 3 {
                    eprintln!("problem with request {}", m["id"]);
                    process::exit(3);
                }
            }
            Some("request") => answer(&m, stdin),
            _ => {}
        }
    }
}

/// Answers the request `m` by its method.
fn answer(m: &Value, stdin: &mut impl BufRead) {
    let id = &m["id"];
    let respond = |result: Value| send(json!({"type": "response", "id": id, "result": result}));
    match m["method"].as_str().unwrap_or("") {
        "count" => {
            let words = m["params"]["text"].as_str().unwrap_or("").split_whitespace().count();
            send(json!({"type": "emit", "event": "counted", "data": {"words": words}}));
            respond(json!({"words": words}));
        }
        "echo" => respond(json!({"params": m["params"], "context": m["context"]})),
        // Events AutoScene drops (a bad name, a long one, too much data),
        // then one it passes on.
        "emits" => {
            send(json!({"type": "emit", "event": "Bad Name", "data": 1}));
            send(json!({"type": "emit", "event": "e".repeat(65), "data": 2}));
            send(json!({"type": "emit", "event": "big", "data": "x".repeat(300 << 10)}));
            send(json!({"type": "emit", "event": "fine", "data": 3}));
            respond(Value::Null);
        }
        "fail" => send(json!({
            "type": "response",
            "id": id,
            "error": {"message": "No text.\nGive it some.", "detail": "KeyError: 'text'"},
        })),
        // Its answer comes the params' ms later.
        "slow" => {
            thread::sleep(Duration::from_millis(m["params"]["ms"].as_u64().unwrap_or(0)));
            respond(json!({"slept": m["params"]["ms"]}));
        }
        // Never answered.
        "wait" => {}
        // A line far past the limit first, then its answer.
        "long" => {
            raw(&"x".repeat(3 << 20));
            respond(json!({"after": "long"}));
        }
        // An answer past the limit.
        "huge" => respond(json!("h".repeat(1 << 20))),
        // Its settings, as AutoScene gives them to a call.
        "call" => {
            send(json!({"type": "call", "id": "c1", "method": "settings.get", "params": {}}));
            let mut reply = String::new();
            stdin.read_line(&mut reply).expect("stdin");
            let reply: Value = serde_json::from_str(&reply).expect("a reply");
            respond(reply["result"].clone());
        }
        other => send(json!({"type": "response", "id": id, "error": {"message": format!("No method {other}.")}})),
    }
}

/// Writes what the run line `first` and the environment say of the run's
/// scratch folder to `scratch.json` (with the line itself in `run.json`),
/// once it wrote files there, when it has one: one at its top, one in a
/// folder, and one in the system's temp folder as the program sees it.
/// Gives the folder.
fn scratch(first: &str) -> Option<PathBuf> {
    let line: Value = serde_json::from_str(first).expect("a run line");
    let temp = line["plugin"]["tempDir"].as_str().map(PathBuf::from);
    if let Some(dir) = &temp {
        fs::write(dir.join("a.txt"), "a").expect("a.txt");
        fs::create_dir(dir.join("sub")).expect("sub");
        fs::write(dir.join("sub").join("b.txt"), "b").expect("b.txt");
        fs::write(env::temp_dir().join("via-temp.txt"), "temp").expect("via-temp.txt");
    }
    let var = |name: &str| env::var(name).ok();
    let said = json!({
        "tempDir": line["plugin"]["tempDir"],
        "runTemp": var("AUTOSCENE_RUN_TEMP"),
        "TEMP": var("TEMP"),
        "TMP": var("TMP"),
        "TMPDIR": var("TMPDIR"),
        "systemTemp": env::temp_dir(),
    });
    fs::write("run.json", first).expect("run.json");
    // Whole once it is there: the tests wait for it.
    fs::write("scratch.json.part", said.to_string()).expect("scratch.json");
    fs::rename("scratch.json.part", "scratch.json").expect("scratch.json");
    temp
}

/// Writes `message` as its line.
fn send(message: Value) {
    raw(&message.to_string());
}

fn raw(line: &str) {
    let mut out = io::stdout().lock();
    writeln!(out, "{line}").and_then(|()| out.flush()).expect("stdout");
}

/// Starts this program again as a heartbeat, which keeps the inherited
/// stdout and stderr open. It is never waited for: it is left running for
/// the run to end.
#[allow(clippy::zombie_processes)]
fn grandchild() {
    let me = env::current_exe().expect("exe");
    process::Command::new(me).args(["heartbeat", "beat"]).stdin(process::Stdio::null()).spawn().expect("grandchild");
}

/// Starts this program again to hold `file` (mode `hold`), with no pipes of
/// the run's. It is never waited for: the run's end ends it.
#[allow(clippy::zombie_processes)]
fn holder(file: &Path) {
    let me = env::current_exe().expect("exe");
    process::Command::new(me)
        .args([OsStr::new("hold"), file.as_os_str(), OsStr::new("holding")])
        .stdin(process::Stdio::null())
        .stdout(process::Stdio::null())
        .stderr(process::Stdio::null())
        .spawn()
        .expect("grandchild");
}

fn forever() -> ! {
    loop {
        thread::sleep(Duration::from_secs(3600));
    }
}

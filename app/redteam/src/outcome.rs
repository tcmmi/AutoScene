//! The end of a run: what it found, written to `result.json` and
//! `summary.md` beside this crate's `Cargo.toml` (CI uploads the one and
//! shows the other), and its exit code.

use std::fs;
use std::path::Path;
use std::sync::atomic::Ordering;

use serde_json::{Value, json};

use crate::{Run, platform};

/// How many checks a run has (B8.2).
const CHECKS: u64 = 17;

/// Writes what `run` found, with `error` when it couldn't finish; gives the
/// exit code: 0 when every wall held, else 1. Only the first call writes:
/// a later one gives the same code.
pub fn finish(run: &Run, error: Option<String>) -> i32 {
    let mut code = run.code.lock();
    if let Some(code) = *code {
        return code;
    }
    let found = found(run, error);
    let held = found["held"] == json!(true);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let summary = summary(&found);
    println!("{summary}");
    let written = serde_json::to_string_pretty(&found).map_err(|e| e.to_string()).and_then(|json| {
        fs::write(dir.join("result.json"), json + "\n").map_err(|e| e.to_string())?;
        fs::write(dir.join("summary.md"), summary).map_err(|e| e.to_string())
    });
    if let Err(e) = &written {
        eprintln!("The red team couldn't write its result: {e}");
    }
    *code = Some(if held && written.is_ok() { 0 } else { 1 });
    code.unwrap_or(1)
}

/// What `run` found, as `result.json` holds it.
fn found(run: &Run, error: Option<String>) -> Value {
    let mut checks = run.checks.lock().clone();
    checks.sort_by_key(|c| c["n"].as_u64());
    let came_back = *run.came_back.lock();
    for check in checks.iter_mut().filter(|c| c["n"] == json!(14)) {
        check["measured"]["cameBackMs"] = json!(came_back);
    }
    let mut failed = Vec::new();
    for check in &checks {
        if check["held"] != json!(true) {
            failed.push(format!("{} {}: {}", check["n"], text(&check["name"]), notes(check)));
        }
    }
    let ran: Vec<u64> = checks.iter().filter_map(|c| c["n"].as_u64()).collect();
    let missing: Vec<u64> = (1..=CHECKS).filter(|n| !ran.contains(n)).collect();
    if !missing.is_empty() {
        failed.push(format!("These checks didn't run: {missing:?}."));
    }
    let pwned: Vec<Value> =
        run.pwned.lock().iter().map(|(check, from)| json!({ "check": check, "from": from })).collect();
    if !pwned.is_empty() {
        failed.push(format!("{} calls from a page reached Tauri.", pwned.len()));
    }
    let wall = run.heard.wall.lock().clone();
    if !wall.is_empty() {
        failed.push(format!("The wall listener heard {} requests.", wall.len()));
    }
    let report = run.report.lock().clone();
    if let Some(stopped) = report["error"].as_str() {
        failed.push(format!("The harness stopped: {stopped}"));
    }
    failed.extend(error);
    let served: serde_json::Map<String, Value> =
        run.served.lock().iter().map(|(status, n)| (status.to_string(), json!(n))).collect();
    json!({
        "held": failed.is_empty(),
        "failed": failed,
        "platform": platform(),
        "userAgent": report["userAgent"],
        "seconds": (run.started.elapsed().as_millis() as f64 / 100.0).round() / 10.0,
        "checks": checks,
        "pwned": pwned,
        "wall": wall,
        "opened": *run.heard.opened.lock(),
        "side": *run.heard.side.lock(),
        "udp": *run.heard.udp.lock(),
        "controls": run.controls.load(Ordering::Relaxed),
        "served": served,
        "strays": report["strays"],
    })
}

/// `found` as a Markdown table, one row per check, for CI's job summary.
fn summary(found: &Value) -> String {
    let failed = found["failed"].as_array().map_or(&[][..], Vec::as_slice);
    let mut out = format!("## Red team on {}\n\n`{}`\n\n", text(&found["platform"]), text(&found["userAgent"]));
    if failed.is_empty() {
        out += "Every wall held.\n\n";
    } else {
        out += "**Not every wall held:**\n";
        for f in failed {
            out += &format!("- {}\n", cell(&text(f)));
        }
        out += "\n";
    }
    out += "| # | Check | Expected | Outcome | What happened |\n| ---: | --- | --- | --- | --- |\n";
    for check in found["checks"].as_array().map_or(&[][..], Vec::as_slice) {
        let outcome = match (check["held"] == json!(true), text(&check["kind"]).as_str()) {
            (false, _) => "**fell**",
            (true, "measured") => "measured",
            (true, _) => "held",
        };
        out += &format!(
            "| {} | {} | {} | {outcome} | {} |\n",
            check["n"],
            text(&check["name"]),
            text(&check["kind"]),
            cell(&notes(check))
        );
    }
    out += &format!(
        "\nSide channels: the side listener heard {}; the UDP one {}; the wall listener had {}.\n",
        heard(&found["side"]),
        heard(&found["udp"]),
        opened(&found["opened"]),
    );
    let checks = found["checks"].as_array().map_or(&[][..], Vec::as_slice);
    if let Some(busy) = checks.iter().find(|c| c["n"] == json!(14)) {
        let came_back = &busy["measured"]["cameBackMs"];
        out += &format!(
            "A busy page (check 14): AutoScene's worst timer gap {} ms; sent home meanwhile, the window came back {}.\n",
            busy["measured"]["worstGapMs"],
            came_back.as_u64().map_or("not within 20 s".into(), |ms| format!("after {ms} ms")),
        );
    }
    out
}

/// The connections to the wall that sent no request, by the check that ran.
fn opened(hits: &Value) -> String {
    let hits = hits.as_array().map_or(&[][..], Vec::as_slice);
    if hits.is_empty() {
        return "no connection without a request".into();
    }
    let checks: Vec<String> = hits.iter().map(|h| text(&h["check"])).collect();
    let n = hits.len();
    format!("{n} connection{} without a request ({})", if n == 1 { "" } else { "s" }, checks.join(", "))
}

/// What a listener heard, in a few words.
fn heard(hits: &Value) -> String {
    let hits = hits.as_array().map_or(&[][..], Vec::as_slice);
    if hits.is_empty() {
        return "nothing".into();
    }
    let what: Vec<String> = hits.iter().map(|h| text(&h["what"])).collect();
    format!("{} ({})", hits.len(), what.join(", "))
}

fn notes(check: &Value) -> String {
    let notes = check["notes"].as_array().map_or(&[][..], Vec::as_slice);
    notes.iter().map(text).collect::<Vec<_>>().join("; ")
}

/// A string as it is; anything else as JSON.
fn text(v: &Value) -> String {
    v.as_str().map_or_else(|| v.to_string(), str::to_owned)
}

/// `s` as one cell of a Markdown table.
fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace(['\n', '\r'], " ")
}

//! Check 16: the scheme's answers, asked for through the handler the
//! webview calls ([`Run::answer`]). A page's CSP can't let the harness
//! fetch them (its `connect-src` is the app's), so this side asks: every
//! answer carries nosniff, a CSP, ACAO and no-store, a session's own pages
//! carry exactly its CSP, and what no page may have is 404.

use std::fs;

use as_plugin::Grants;
use as_plugin::pages::{self, page_csp};
use serde_json::{Value, json};
use tauri::http::header::{CONTENT_RANGE, RANGE};
use tauri::http::{Request, Response};

use crate::{ANCESTORS, Run};

/// A request of check 16: what it asks, and what the answer's status must be.
struct Ask {
    what: &'static str,
    label: &'static str,
    method: &'static str,
    path: String,
    range: Option<&'static str>,
    status: u16,
}

impl Ask {
    fn get(what: &'static str, path: String, status: u16) -> Self {
        Self { what, label: "main", method: "GET", path, range: None, status }
    }
}

/// Runs check 16 on a session of the attacker's of its own.
pub fn check(run: &Run) -> Value {
    match asks(run) {
        Ok(found) => found,
        Err(e) => json!({ "held": false, "notes": [format!("The check couldn't run: {e}")] }),
    }
}

fn asks(run: &Run) -> Result<Value, String> {
    let page = run.attacker.manifest.as_ref().and_then(|m| m.page("main")).ok_or("The attacker has no page.")?;
    let grants = Grants::of(&run.attacker).map_err(|e| e.to_string())?;
    let (m, _) = run.pages.open(&run.attacker, grants, page, None).map_err(|e| e.to_string())?;
    let t = m.token.clone();
    // A listed file changed after the session opened, and a clip it may show.
    let changed = run.attacker.dir.join("ui").join("changed.html");
    fs::write(&changed, "<!doctype html><p>changed</p>").map_err(|e| e.to_string())?;
    let clip = run.scratch.join("clip.mp4");
    fs::write(&clip, vec![7u8; 3 << 20]).map_err(|e| e.to_string())?;
    let file = run.pages.resolve(&m, clip).map_err(|e| e.to_string())?;
    let file = file.rsplit('/').next().unwrap_or_default().to_owned();

    let asks = [
        Ask::get("its page", format!("/{t}/ui/index.html"), 200),
        Ask::get("a file its lock doesn't list", format!("/{t}/ui/unlisted.html"), 404),
        Ask::get("a file changed after it opened", format!("/{t}/ui/changed.html"), 404),
        Ask::get("ui/../plugin.json", format!("/{t}/ui/../plugin.json"), 404),
        Ask::get("plugin.json", format!("/{t}/plugin.json"), 404),
        Ask::get("%2e%2e", format!("/{t}/ui/%2e%2e/plugin.json"), 404),
        Ask::get("a backslash", format!("/{t}/ui%5cindex.html"), 404),
        Ask::get("an uppercase folder", format!("/{t}/UI/index.html"), 404),
        Ask::get("an uppercase name", format!("/{t}/ui/INDEX.HTML"), 404),
        Ask::get("a bad file token", format!("/{t}/~f/{}", "0".repeat(32)), 404),
        Ask::get("/_as/../x", "/_as/../x".into(), 404),
        Ask::get("an unknown session", format!("/{}/ui/index.html", "f".repeat(32)), 404),
        Ask { range: Some("bytes=0-99"), ..Ask::get("a part of a resolved file", format!("/{t}/~f/{file}"), 206) },
        Ask { method: "POST", ..Ask::get("a POST", format!("/{t}/ui/index.html"), 405) },
        Ask { label: "plugin", ..Ask::get("another webview", format!("/{t}/ui/index.html"), 403) },
    ];
    let own_csp = page_csp(pages::base_url(), &t, &ANCESTORS.map(String::from));
    let mut notes = Vec::new();
    let mut answers = Vec::new();
    for ask in asks {
        let mut request = Request::builder().method(ask.method).uri(format!("asp://localhost{}", ask.path));
        if let Some(range) = ask.range {
            request = request.header(RANGE, range);
        }
        let request = request.body(Vec::new()).map_err(|e| e.to_string())?;
        let answer = run.answer(ask.label, &request);
        let status = answer.status().as_u16();
        answers.push(json!({ "asked": ask.what, "status": status }));
        if status != ask.status {
            notes.push(format!("{}: {status}, not {}", ask.what, ask.status));
        }
        for wrong in wrong_headers(&answer) {
            notes.push(format!("{}: {wrong}", ask.what));
        }
        if ask.status == 200 && header(&answer, "content-security-policy") != own_csp {
            notes.push(format!("{}: not its session's CSP", ask.what));
        }
        if ask.status == 206 && header(&answer, CONTENT_RANGE.as_str()) != format!("bytes 0-99/{}", 3 << 20) {
            notes.push(format!("{}: Content-Range {:?}", ask.what, header(&answer, CONTENT_RANGE.as_str())));
        }
    }
    run.pages.close(&t);
    let held = notes.is_empty();
    if held {
        notes.push(format!("{} answers, each with its status and headers", answers.len()));
    }
    Ok(json!({ "held": held, "notes": notes, "measured": { "answers": answers } }))
}

/// The headers every answer must carry that `answer` doesn't, or carries wrong.
fn wrong_headers(answer: &Response<Vec<u8>>) -> Vec<String> {
    let mut wrong = Vec::new();
    for (name, want) in
        [("x-content-type-options", "nosniff"), ("access-control-allow-origin", "*"), ("cache-control", "no-store")]
    {
        if header(answer, name) != want {
            wrong.push(format!("{name} is {:?}", header(answer, name)));
        }
    }
    if !header(answer, "content-security-policy").starts_with("default-src 'none'") {
        wrong.push("no CSP".into());
    }
    wrong
}

fn header(answer: &Response<Vec<u8>>, name: &str) -> String {
    answer.headers().get(name).and_then(|v| v.to_str().ok()).unwrap_or_default().to_owned()
}

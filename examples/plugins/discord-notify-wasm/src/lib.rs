//! Discord Notify, as a WebAssembly plugin: an example that talks to the
//! internet from AutoScene's sandbox.
//!
//! Its export action `post` tells a Discord channel that an export is
//! done, and its event `analysis-done` that a project's episodes are all
//! analysed, through the channel's webhook. The webhook's URL is the secret
//! setting `webhook`: AutoScene keeps it in the system's keychain and hands
//! it over only when the plugin asks (`host::secret`). The post goes over
//! wasi:http, which AutoScene sends itself, and only to the hosts
//! `plugin/plugin.json` names. The plugin never writes the URL to its log
//! or its messages. `docs/PLUGINS.md` describes the sandbox.

wit_bindgen::generate!({ world: "plugin", path: "wit" });

use autoscene::plugin::host::{self, LogLevel};
use autoscene::plugin::types::{EventContext, ExportContext};
use wasip2::http::outgoing_handler;
use wasip2::http::types::{ErrorCode, Fields, Method, OutgoingBody, OutgoingRequest, Scheme};

/// Where a webhook must lead: Discord's, over https.
const WEBHOOKS: &str = "https://discord.com/api/webhooks/";
/// At most what `blocking_write_and_flush` takes at once.
const CHUNK: usize = 4096;

struct Plugin;

impl Guest for Plugin {
    fn run(request: RunRequest) -> Result<Option<String>, RunError> {
        match request.action.id.as_str() {
            "post" => {
                let Some(export) = &request.export else {
                    return Err(error("Post to Discord runs on an export.", None));
                };
                send(&exported(export))
            }
            other => Err(error(&format!("Discord Notify has no action {other}."), None)),
        }
    }

    fn on_event(request: EventRequest) -> Result<Option<String>, RunError> {
        match request.id.as_str() {
            "analysis-done" => {
                let project = request.project.as_ref().map(|p| p.title.as_str());
                send(&analysed(project, &request.event))
            }
            other => Err(error(&format!("Discord Notify has no event {other}."), None)),
        }
    }
}

export!(Plugin);

fn error(message: &str, detail: Option<String>) -> RunError {
    RunError { message: message.into(), detail }
}

/// The webhook URL from the keychain, once it is one of Discord's.
fn webhook() -> Result<String, RunError> {
    let url = host::secret("webhook").map(|u| u.trim().to_owned()).unwrap_or_default();
    if url.is_empty() {
        return Err(error("Set the webhook URL in Discord Notify's settings first.", None));
    }
    // Only discord.com is in plugin.json, so a webhook of ptb., canary. or
    // discordapp.com, or of a versioned /api/v10/, is refused: the person
    // is told which form to use.
    if !url.starts_with(WEBHOOKS) {
        return Err(error(
            concat!(
                "That webhook URL doesn't start with https://discord.com/api/webhooks/. ",
                "Change a ptb., canary. or discordapp.com address to discord.com, ",
                "and leave out a version such as /v10.",
            ),
            None,
        ));
    }
    // A space, a line break or a letter outside ASCII has no place in it.
    if !url.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(error(
            "That webhook URL has a space or a letter it can't have. Copy it again from the channel's settings.",
            None,
        ));
    }
    Ok(url)
}

/// Posts `content` to the webhook from the keychain.
fn send(content: &str) -> Result<Option<String>, RunError> {
    let url = webhook()?;
    if host::cancelled() {
        return Err(error("Stopped before it posted.", None));
    }
    host::progress(None, Some("Posting to Discord"));
    post(&url, &body(content))?;
    host::log(LogLevel::Info, "Posted to Discord.");
    Ok(Some("Posted to Discord.".into()))
}

/// What to say of `export`: its name and how many clips it made.
fn exported(export: &ExportContext) -> String {
    let made = export.clips.iter().filter(|c| c.status == "done").count();
    let clips = count(made, "clip", "clips");
    match &export.label {
        Some(label) => format!("Export {label} is done: {clips}."),
        None => format!("An export is done: {clips}."),
    }
}

/// What to say when the analysis of `project`'s episodes finished: how
/// many were analysed, and how many failed.
fn analysed(project: Option<&str>, event: &EventContext) -> String {
    let episodes = event.episodes;
    let done = count(episodes.done as usize, "episode", "episodes");
    let what = match episodes.failed {
        0 => format!("{done} analysed"),
        failed => format!("{done} analysed, {failed} failed"),
    };
    match project {
        Some(title) => format!("{title}: {what}."),
        None => format!("A project: {what}."),
    }
}

/// `n` and the word for one or more.
fn count(n: usize, one: &str, more: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{n} {more}") }
}

/// The post that says `content`. Mentions in it never ping anyone.
fn body(content: &str) -> String {
    format!(r#"{{"content":{},"allowed_mentions":{{"parse":[]}}}}"#, json_string(content))
}

/// `text` as a JSON string.
fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Posts `json` to the webhook `url`, through AutoScene.
fn post(url: &str, json: &str) -> Result<(), RunError> {
    let path = &url["https://discord.com".len()..];
    let headers = Fields::new();
    headers
        .set("content-type", &[b"application/json".to_vec()])
        .map_err(|e| error("Couldn't post to Discord.", Some(format!("content-type: {e:?}"))))?;
    let request = OutgoingRequest::new(headers);
    let refused = |what: &str| error("Couldn't post to Discord.", Some(format!("{what} refused")));
    request.set_method(&Method::Post).map_err(|()| refused("method"))?;
    request.set_scheme(Some(&Scheme::Https)).map_err(|()| refused("scheme"))?;
    request.set_authority(Some("discord.com")).map_err(|()| refused("authority"))?;
    request.set_path_with_query(Some(path)).map_err(|()| refused("path"))?;
    let outgoing = request.body().map_err(|()| refused("body"))?;

    let answer = outgoing_handler::handle(request, None).map_err(unreachable)?;
    {
        let stream = outgoing.write().map_err(|()| refused("body stream"))?;
        for chunk in json.as_bytes().chunks(CHUNK) {
            stream
                .blocking_write_and_flush(chunk)
                .map_err(|e| error("Couldn't reach Discord: the connection broke off.", Some(format!("{e:?}"))))?;
        }
    }
    OutgoingBody::finish(outgoing, None).map_err(unreachable)?;

    answer.subscribe().block();
    let response = match answer.get() {
        Some(Ok(Ok(response))) => response,
        Some(Ok(Err(code))) => return Err(unreachable(code)),
        _ => return Err(error("Couldn't reach Discord: no answer came.", None)),
    };
    match response.status() {
        200..=299 => Ok(()),
        // 401 is Discord's answer to a token that isn't the webhook's.
        401 | 404 => Err(error("Discord doesn't know that webhook. Make a new one in the channel's settings.", None)),
        429 => Err(error("Discord asks to wait before the next post. Try again in a minute.", None)),
        status => Err(error(&format!("Discord refused the post (HTTP {status})."), None)),
    }
}

/// Why a request didn't reach Discord, in a person's words, with the code
/// for the log.
fn unreachable(code: ErrorCode) -> RunError {
    let why = match &code {
        ErrorCode::DnsTimeout | ErrorCode::DnsError(_) => "its address couldn't be found. Is this computer online?",
        ErrorCode::ConnectionRefused | ErrorCode::DestinationUnavailable => "it didn't answer.",
        ErrorCode::ConnectionTimeout | ErrorCode::ConnectionReadTimeout | ErrorCode::HttpResponseTimeout => {
            "it took too long to answer."
        }
        ErrorCode::TlsCertificateError | ErrorCode::TlsProtocolError | ErrorCode::TlsAlertReceived(_) => {
            "its certificate couldn't be checked."
        }
        ErrorCode::HttpRequestDenied | ErrorCode::DestinationIpProhibited => "AutoScene didn't allow the request.",
        _ => "the connection failed.",
    };
    error(&format!("Couldn't reach Discord: {why}"), Some(format!("{code:?}")))
}

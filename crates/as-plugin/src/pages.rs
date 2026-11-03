//! Plugin pages: the sessions AutoScene opens for them, and the answers of
//! the `asp` scheme that serves their files ([`Pages::serve`]).
//!
//! A page runs in a frame of the app's window, sandboxed to scripts and
//! forms ([`SANDBOX`]) and never `allow-same-origin`: a frame of the
//! window's own origin could call every command of the app. So the frame's
//! origin is opaque, and the page has no storage, cookies or dialogs of its
//! own. Its own answers say the same sandbox ([`page_csp`]), so a page is
//! sandboxed even in a frame that left the attribute out.
//!
//! Each open page is a session ([`Mount`]) under a random token, and its
//! frame loads `B/<token>/<entry>` ([`base_url`]). A session serves its
//! plugin's files under `ui/` as its package lists them: a file whose size
//! or SHA-256 on disk isn't the lock's is not served, and the bytes served
//! are the bytes hashed. A loaded folder's files are served as they are,
//! never through a link. Every answer, a refusal too, carries a CSP that
//! lets the page load only from its own session's path and the kit
//! (`B/_as/`), and nothing from the network ([`page_csp`]): `'self'` would
//! take in every plugin's files. Its type comes from a fixed table by the
//! file's extension, never from the file.
//!
//! The kit's two files come from the app ([`KitSource`]). Pictures, video
//! and sound of the person's that a page may show get a token of their own
//! ([`Pages::resolve`]). A picture comes whole, as `<img>` asks for it;
//! video and sound come in parts of at most [`RANGE_PART`], which media
//! elements ask for one after another. A page's own file comes whole, or
//! as the one part a `Range` asks for (WebKit's player asks in parts).

use std::collections::{HashMap, VecDeque};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use as_core::CancelToken;
use parking_lot::Mutex;
use uuid::Uuid;

use crate::calls::Grants;
use crate::error::{PluginError, PluginResult};
use crate::manifest::{PageSpec, PluginId, invalid};
use crate::package::{self, FilesLock, check_path, file_in, plain_file, sha256_hex};
use crate::store::{Installed, installed_lock, manifest_of};
use crate::trust::Trust;

/// The frame's sandbox: exactly this, never `allow-same-origin` (see the
/// module's docs).
pub const SANDBOX: &str = "allow-scripts allow-forms";
/// The most pages open at once: one more closes the one opened first.
pub const MAX_SESSIONS: usize = 8;
/// The most files one session resolves: one more forgets its first.
pub const MAX_FILE_TOKENS: usize = 256;
/// The largest file of a page that is served, and the largest picture a
/// page is given.
pub const MAX_SERVED_FILE: u64 = 32 << 20;
/// The most of a resolved video or sound one answer holds, about what
/// tauri's asset protocol gives for a range: a webview holds each answer
/// whole, and media elements ask for the rest.
pub const RANGE_PART: u64 = 1 << 20;
/// The kit's files, served under `/_as/`.
pub const KIT_FILES: [&str; 2] = ["sdk.js", "theme.css"];

/// What a page may not use, said to the webview on every answer: also no
/// picture-in-picture, a window over every other that shows what the page
/// draws.
const PERMISSIONS: &str = "camera=(), microphone=(), geolocation=(), display-capture=(), clipboard-read=(), \
                           clipboard-write=(), usb=(), serial=(), hid=(), bluetooth=(), payment=(), \
                           picture-in-picture=()";

/// Where the scheme's addresses start: `http://asp.localhost` on Windows
/// (and Android), whose webview takes a custom scheme only so, and
/// `asp://localhost` elsewhere. The handler sees `asp://localhost` on all.
pub fn base_url() -> &'static str {
    if cfg!(any(windows, target_os = "android")) { "http://asp.localhost" } else { "asp://localhost" }
}

/// Where the kit's files come from: the app's built files, or a test's.
pub trait KitSource: Send + Sync {
    /// The file `name`, one of [`KIT_FILES`].
    fn get(&self, name: &str) -> Option<Vec<u8>>;
}

/// A kit held in memory, by name.
#[derive(Default)]
pub struct MemoryKit(pub HashMap<String, Vec<u8>>);

impl KitSource for MemoryKit {
    fn get(&self, name: &str) -> Option<Vec<u8>> {
        self.0.get(name).cloned()
    }
}

/// A kit read through a lookup that gives a file's bytes and type: the
/// app's asset resolver, which answers a path it doesn't have with the
/// app's own `index.html`. An answer of type `text/html` counts as none.
pub struct AssetKit<F>(pub F);

impl<F> KitSource for AssetKit<F>
where
    F: Fn(&str) -> Option<(Vec<u8>, String)> + Send + Sync,
{
    fn get(&self, name: &str) -> Option<Vec<u8>> {
        let (bytes, kind) = (self.0)(name)?;
        (!kind.trim_start().to_ascii_lowercase().starts_with("text/html")).then_some(bytes)
    }
}

/// An open page: what its frame may load, and what it may use.
#[derive(Debug)]
pub struct Mount {
    /// Its session's token: 32 lowercase hex digits, random.
    pub token: String,
    pub plugin: PluginId,
    pub name: String,
    pub version: String,
    pub page: PageSpec,
    /// The project it is open in (a project page's).
    pub project: Option<String>,
    /// Its plugin's folder.
    pub dir: PathBuf,
    /// Its package's lock, read once as it opened; none for a loaded
    /// folder, whose files are served as they are.
    pub lock: Option<Arc<FilesLock>>,
    pub grants: Grants,
    /// Its plugin's trust as it opened (a loaded folder's is Unreviewed).
    pub trust: Trust,
    /// Its calls waiting for their answer.
    pub inflight: AtomicUsize,
    /// Fires as it closes: a call of it that still waits (a dialog the
    /// person hasn't closed, a write waiting its turn) then does nothing.
    pub closed: CancelToken,
    /// The files it resolved, by token, oldest first.
    files: Mutex<VecDeque<(String, ResolvedPath)>>,
    /// Its place in the order the open pages opened in.
    opened: u64,
}

impl Mount {
    /// Its frame's address: `B/<token>/<entry>`.
    pub fn src(&self) -> String {
        format!("{}/{}/{}", base_url(), self.token, self.page.entry)
    }
}

/// A file of the person's that a page may show, with its type.
#[derive(Clone, Debug)]
pub struct ResolvedPath {
    pub path: PathBuf,
    pub mime: &'static str,
}

/// The open pages, and the kit they load.
pub struct Pages {
    sessions: Mutex<HashMap<String, Arc<Mount>>>,
    /// How many pages opened so far: the next one's [`Mount::opened`].
    opened: AtomicU64,
    kit: Arc<dyn KitSource>,
    /// The origins that may frame a page: the app's own.
    ancestors: Vec<String>,
}

/// An answer of the scheme: its status, headers and body.
#[derive(Debug)]
pub struct Served {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

impl Served {
    /// The value of its header `name`, in any case.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

impl Pages {
    /// No page open yet. The kit comes from `kit`; only `ancestors` may
    /// frame a page.
    pub fn new(kit: Arc<dyn KitSource>, ancestors: Vec<String>) -> Self {
        Self { sessions: Mutex::default(), opened: AtomicU64::new(0), kit, ancestors }
    }

    /// Opens the page `page` of `p` (which the caller found may run now)
    /// in `project`, with `grants`: its session, and the tokens of those it
    /// closed. A package's lock is read here, once, and must be the one its
    /// install recorded and list the page's file; a loaded folder must hold
    /// the file, not through a link. With [`MAX_SESSIONS`] open, the one
    /// opened first closes.
    pub fn open(
        &self,
        p: &Installed,
        grants: Grants,
        page: &PageSpec,
        project: Option<String>,
    ) -> PluginResult<(Arc<Mount>, Vec<String>)> {
        let m = manifest_of(p)?;
        let lock = match &p.record.lock_sha256 {
            Some(sha256) => Some(Arc::new(installed_lock(&p.dir, sha256).map_err(invalid)?)),
            None => None,
        };
        package::check_pages(&p.dir, lock.as_deref(), std::slice::from_ref(page))?;
        let mount = Arc::new(Mount {
            token: Uuid::new_v4().simple().to_string(),
            plugin: p.record.id.clone(),
            name: m.name.clone(),
            version: p.record.version.clone(),
            page: page.clone(),
            project,
            dir: p.dir.clone(),
            // A loaded folder is never reviewed, whatever its row says.
            trust: if lock.is_some() { p.record.trust } else { Trust::Unreviewed },
            lock,
            grants,
            inflight: AtomicUsize::new(0),
            closed: CancelToken::new(),
            files: Mutex::default(),
            opened: self.opened.fetch_add(1, Ordering::Relaxed),
        });
        let mut sessions = self.sessions.lock();
        let mut closed = Vec::new();
        while sessions.len() >= MAX_SESSIONS {
            let Some(oldest) = sessions.values().min_by_key(|m| m.opened).map(|m| m.token.clone()) else { break };
            if let Some(m) = sessions.remove(&oldest) {
                m.closed.cancel();
            }
            closed.push(oldest);
        }
        sessions.insert(mount.token.clone(), mount.clone());
        Ok((mount, closed))
    }

    /// The open session `token`.
    pub fn get(&self, token: &str) -> Option<Arc<Mount>> {
        self.sessions.lock().get(token).cloned()
    }

    /// Closes the session `token`, if it is open.
    pub fn close(&self, token: &str) {
        if let Some(m) = self.sessions.lock().remove(token) {
            m.closed.cancel();
        }
    }

    /// Closes every open page of `id`; gives their tokens.
    pub fn close_plugin(&self, id: &PluginId) -> Vec<String> {
        self.close_where(|m| m.plugin == *id)
    }

    /// Closes each open page `close` picks; gives their tokens.
    pub fn close_where(&self, close: impl Fn(&Mount) -> bool) -> Vec<String> {
        let mut sessions = self.sessions.lock();
        let closing: Vec<String> = sessions.values().filter(|m| close(m)).map(|m| m.token.clone()).collect();
        for token in &closing {
            if let Some(m) = sessions.remove(token) {
                m.closed.cancel();
            }
        }
        closing
    }

    /// How many pages of `id` are open.
    pub fn open_count(&self, id: &PluginId) -> usize {
        self.sessions.lock().values().filter(|m| m.plugin == *id).count()
    }

    /// The tokens of the open pages of `id`.
    pub fn tokens_of(&self, id: &PluginId) -> Vec<String> {
        self.sessions.lock().values().filter(|m| m.plugin == *id).map(|m| m.token.clone()).collect()
    }

    /// A token of the session `m` for the picture, video or sound file at
    /// `path` (an episode, a clip or a merged file the caller found it may
    /// show): its address, `B/<session>/~f/<token>`. Past
    /// [`MAX_FILE_TOKENS`] the session forgets its first. Any other file
    /// is refused, and so is one that isn't there, a link (which is never
    /// served), and a picture larger than [`MAX_SERVED_FILE`], which comes
    /// whole.
    pub fn resolve(&self, m: &Mount, path: PathBuf) -> PluginResult<String> {
        let Some(mime) = media_type(&path) else {
            return Err(PluginError::Denied("A page can show only pictures, video and sound.".into()));
        };
        let found = fs::symlink_metadata(&path).ok();
        if found.as_ref().is_some_and(fs::Metadata::is_symlink) {
            return Err(PluginError::Denied("A page can't show a file through a link.".into()));
        }
        let Some(len) = found.filter(fs::Metadata::is_file).map(|f| f.len()) else {
            return Err(invalid("That file isn't where it was."));
        };
        if mime.starts_with("image/") && len > MAX_SERVED_FILE {
            return Err(invalid("A page can show pictures of up to 32 MB."));
        }
        let token = Uuid::new_v4().simple().to_string();
        let mut files = m.files.lock();
        if files.len() >= MAX_FILE_TOKENS {
            files.pop_front();
        }
        files.push_back((token.clone(), ResolvedPath { path, mime }));
        Ok(format!("{}/{}/~f/{token}", base_url(), m.token))
    }

    /// The answer to a request of `method` for `path` (the address's path,
    /// as the handler sees it after `asp://localhost`), with its `Range`
    /// header. Only GET and HEAD are answered; HEAD as GET, without a body.
    pub fn serve(&self, method: &str, path: &str, range: Option<&str>) -> Served {
        let path = decoded(path.split(['?', '#']).next().unwrap_or_default());
        // The session the address names, for the CSP: also on a refusal.
        let named = path.as_deref().and_then(|p| p.strip_prefix('/')?.split('/').next()).filter(|t| is_token(t));
        let csp = match named {
            Some(token) => page_csp(base_url(), token, &self.ancestors),
            None => no_page_csp(&self.ancestors),
        };
        let answer = match (method, path.as_deref()) {
            ("GET" | "HEAD", Some(path)) => self.answer(path, range),
            ("GET" | "HEAD", None) => Answer::status(404),
            _ => Answer { headers: vec![("Allow", "GET, HEAD".into())], ..Answer::status(405) },
        };
        finish(answer, csp, method == "HEAD")
    }

    fn answer(&self, path: &str, range: Option<&str>) -> Answer {
        let Some(path) = path.strip_prefix('/') else { return Answer::status(404) };
        if let Some(name) = path.strip_prefix("_as/") {
            return self.kit_file(name);
        }
        let Some((token, rest)) = path.split_once('/') else { return Answer::status(404) };
        let Some(m) = self.get(token) else { return Answer::status(404) };
        match rest.strip_prefix("~f/") {
            Some(file) => resolved_file(&m, file, range),
            None => page_file(&m, rest, range),
        }
    }

    /// The kit's file `name`.
    fn kit_file(&self, name: &str) -> Answer {
        match (KIT_FILES.contains(&name), content_type(name)) {
            (true, Some(kind)) => self.kit.get(name).map_or_else(|| Answer::status(404), |b| Answer::file(kind, b)),
            _ => Answer::status(404),
        }
    }
}

/// An answer with nothing in it, of `status`, with the headers every answer
/// gets and a CSP that allows nothing: for a request the host doesn't hand
/// to its pages (another webview asked, or plugins aren't there yet).
/// `ancestors` may frame it.
pub fn refusal(status: u16, ancestors: &[String]) -> Served {
    finish(Answer::status(status), no_page_csp(ancestors), false)
}

/// The CSP of a page of the session `token`, whose addresses start with
/// `base`, framed only by `ancestors`: its own session's path and the kit,
/// and nothing from anywhere else. It sandboxes the page as its frame does
/// ([`SANDBOX`]), whatever the frame says.
pub fn page_csp(base: &str, token: &str, ancestors: &[String]) -> String {
    let (own, kit) = (format!("{base}/{token}/"), format!("{base}/_as/"));
    format!(
        "default-src 'none'; script-src {own} {kit}; style-src {own} {kit} 'unsafe-inline'; \
         img-src {own} data: blob:; font-src {own} {kit} data:; media-src {own} blob:; \
         connect-src 'none'; frame-src 'none'; child-src 'none'; worker-src 'none'; \
         manifest-src 'none'; object-src 'none'; form-action 'none'; base-uri 'none'; \
         sandbox {SANDBOX}; frame-ancestors {}",
        frame_ancestors(ancestors)
    )
}

/// The CSP of an answer outside any session: nothing may load from it.
fn no_page_csp(ancestors: &[String]) -> String {
    format!("default-src 'none'; form-action 'none'; base-uri 'none'; frame-ancestors {}", frame_ancestors(ancestors))
}

fn frame_ancestors(ancestors: &[String]) -> String {
    if ancestors.is_empty() { "'none'".into() } else { ancestors.join(" ") }
}

/// What a request is answered with, before the headers every answer gets.
struct Answer {
    status: u16,
    kind: Option<&'static str>,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
}

impl Answer {
    fn status(status: u16) -> Self {
        Self { status, kind: None, headers: Vec::new(), body: Vec::new() }
    }

    fn file(kind: &'static str, body: Vec<u8>) -> Self {
        Self { kind: Some(kind), body, ..Self::status(200) }
    }
}

/// `answer` with the headers every answer gets, and no body for a HEAD.
fn finish(answer: Answer, csp: String, head: bool) -> Served {
    let mut headers = vec![
        ("Content-Security-Policy", csp),
        ("X-Content-Type-Options", "nosniff".into()),
        // A module, a font or a fetch of an opaque origin sends `Origin: null`.
        ("Access-Control-Allow-Origin", "*".into()),
        ("Cross-Origin-Resource-Policy", "cross-origin".into()),
        ("Cache-Control", "no-store".into()),
        ("Referrer-Policy", "no-referrer".into()),
        ("Permissions-Policy", PERMISSIONS.into()),
        ("Content-Length", answer.body.len().to_string()),
    ];
    headers.extend(answer.kind.map(|kind| ("Content-Type", kind.to_owned())));
    headers.extend(answer.headers);
    Served { status: answer.status, headers, body: if head { Vec::new() } else { answer.body } }
}

/// The file `path` of the plugin of `m`, under `ui/`: as its package lists
/// it, or as its loaded folder holds it, never through a link. Whole, or
/// the part `range` asks for, cut from the bytes that were checked.
fn page_file(m: &Mount, path: &str, range: Option<&str>) -> Answer {
    let Some(kind) = content_type(path) else { return Answer::status(404) };
    if check_path(path).is_err() || !path.starts_with("ui/") {
        return Answer::status(404);
    }
    let bytes = match &m.lock {
        Some(lock) => listed_file(m, lock, path),
        None => folder_file(&m.dir, path),
    };
    let Some(bytes) = bytes else { return Answer::status(404) };
    let len = bytes.len() as u64;
    match range.map(|r| byte_range(r, len, len)) {
        None => Answer { headers: vec![("Accept-Ranges", "bytes".into())], ..Answer::file(kind, bytes) },
        // A part of the file, so of its bytes.
        Some(Some((start, end))) => part(kind, bytes[start as usize..end as usize].to_vec(), start, len),
        Some(None) => unsatisfiable(len),
    }
}

/// The bytes of `path` in the package of `m`, while they are the ones its
/// lock lists: of its size and SHA-256.
fn listed_file(m: &Mount, lock: &FilesLock, path: &str) -> Option<Vec<u8>> {
    let listed = lock.files.iter().find(|f| f.path == path)?;
    if listed.size > MAX_SERVED_FILE {
        return None;
    }
    let mut bytes = Vec::new();
    File::open(file_in(&m.dir, path)).ok()?.take(listed.size + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 != listed.size || sha256_hex(&bytes) != listed.sha256 {
        tracing::warn!(plugin = %m.plugin, file = path, "a plugin page's file changed on disk");
        return None;
    }
    Some(bytes)
}

/// The bytes of `path` in the loaded folder `dir`, a plain file in plain
/// folders of at most [`MAX_SERVED_FILE`].
fn folder_file(dir: &Path, path: &str) -> Option<Vec<u8>> {
    if !plain_file(dir, path) {
        return None;
    }
    let mut bytes = Vec::new();
    File::open(file_in(dir, path)).ok()?.take(MAX_SERVED_FILE + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= MAX_SERVED_FILE).then_some(bytes)
}

/// The file `m` resolved as `token`, or the part of it `range` asks for. A
/// picture asked for whole comes whole, up to [`MAX_SERVED_FILE`]: `<img>`
/// never asks for a part. Else 200 for a file of up to [`RANGE_PART`] asked
/// for whole, and 206 with at most that much, from the start when no range
/// was asked for.
fn resolved_file(m: &Mount, token: &str, range: Option<&str>) -> Answer {
    let found = m.files.lock().iter().find(|(t, _)| t == token).map(|(_, r)| r.clone());
    let Some(found) = found.filter(|f| fs::symlink_metadata(&f.path).is_ok_and(|f| f.is_file())) else {
        return Answer::status(404);
    };
    let Ok(mut file) = File::open(&found.path) else { return Answer::status(404) };
    let Ok(len) = file.metadata().map(|f| f.len()) else { return Answer::status(404) };
    if range.is_none() && found.mime.starts_with("image/") {
        let mut body = Vec::new();
        let read = len <= MAX_SERVED_FILE && file.take(MAX_SERVED_FILE + 1).read_to_end(&mut body).is_ok();
        if !read || body.len() as u64 > MAX_SERVED_FILE {
            return Answer::status(404);
        }
        return Answer { headers: vec![("Accept-Ranges", "bytes".into())], ..Answer::file(found.mime, body) };
    }
    let (start, end) = match range.map(|r| byte_range(r, len, RANGE_PART)) {
        None => (0, len.min(RANGE_PART)),
        Some(Some(part)) => part,
        Some(None) => return unsatisfiable(len),
    };
    let mut body = Vec::new();
    if file.seek(SeekFrom::Start(start)).and_then(|_| file.take(end - start).read_to_end(&mut body)).is_err() {
        return Answer::status(404);
    }
    if range.is_none() && len <= RANGE_PART {
        return Answer { headers: vec![("Accept-Ranges", "bytes".into())], ..Answer::file(found.mime, body) };
    }
    part(found.mime, body, start, len)
}

/// The part `body` of a file of `len` bytes, from `start`: 206, with the
/// part it is.
fn part(kind: &'static str, body: Vec<u8>, start: u64, len: u64) -> Answer {
    let last = (start + body.len() as u64).max(1) - 1;
    let headers = vec![("Accept-Ranges", "bytes".into()), ("Content-Range", format!("bytes {start}-{last}/{len}"))];
    Answer { status: 206, headers, ..Answer::file(kind, body) }
}

/// 416, for a `Range` of a file of `len` bytes that asks for several
/// parts, for none of it, or doesn't read.
fn unsatisfiable(len: u64) -> Answer {
    Answer { headers: vec![("Content-Range", format!("bytes */{len}"))], ..Answer::status(416) }
}

/// The one range `header` asks of a file of `len` bytes (`bytes=a-b`, `a-`
/// or `-n`), as `start..end`, at most `most` long. None when it asks for
/// several, for none of the file, or doesn't read.
fn byte_range(header: &str, len: u64, most: u64) -> Option<(u64, u64)> {
    let (from, to) = header.trim().strip_prefix("bytes=")?.trim().split_once('-')?;
    let number = |s: &str| -> Option<u64> { s.bytes().all(|b| b.is_ascii_digit()).then(|| s.parse().ok())? };
    let last = len.checked_sub(1)?;
    let (start, end) = match (from.trim(), to.trim()) {
        ("", "") => return None,
        ("", n) => (len.saturating_sub(number(n).filter(|&n| n > 0)?), last),
        (a, "") => (number(a)?, last),
        (a, b) => (number(a)?, number(b)?.min(last)),
    };
    (start <= end && start < len).then(|| (start, (end + 1).min(start.saturating_add(most))))
}

/// Whether `s` is a session's token: 32 lowercase hex digits.
fn is_token(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// `path` with each `%XX` decoded, once; none when one doesn't read, or
/// what it gives isn't UTF-8.
fn decoded(path: &str) -> Option<String> {
    let mut bytes = path.bytes();
    let mut out = Vec::with_capacity(path.len());
    while let Some(b) = bytes.next() {
        if b != b'%' {
            out.push(b);
            continue;
        }
        let hex = [bytes.next()?, bytes.next()?];
        if !hex.iter().all(u8::is_ascii_hexdigit) {
            return None;
        }
        out.push(u8::from_str_radix(std::str::from_utf8(&hex).ok()?, 16).ok()?);
    }
    String::from_utf8(out).ok()
}

/// The type a file of a page is served as, by its extension; none for any
/// other, which is never served.
fn content_type(path: &str) -> Option<&'static str> {
    let name = path.rsplit('/').next()?;
    let (_, ext) = name.rsplit_once('.')?;
    Some(match ext.to_ascii_lowercase().as_str() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "txt" => "text/plain; charset=utf-8",
        "md" => "text/markdown; charset=utf-8",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "opus" => "audio/opus",
        "flac" => "audio/flac",
        _ => return None,
    })
}

/// The type of the picture, video or sound at `path`, of those a page is
/// served; none for any other file (an SVG may hold a script).
pub fn media_type(path: &Path) -> Option<&'static str> {
    let kind = content_type(path.file_name()?.to_str()?)?;
    let media = ["image/", "video/", "audio/"].iter().any(|k| kind.starts_with(k));
    (media && kind != "image/svg+xml").then_some(kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_range_is_one_part_of_the_file() {
        let (mib, byte_range) = (RANGE_PART, |r: &str, len: u64| byte_range(r, len, RANGE_PART));
        assert_eq!(byte_range("bytes=0-99", 1000), Some((0, 100)));
        assert_eq!(byte_range("bytes=900-", 1000), Some((900, 1000)));
        assert_eq!(byte_range("bytes=-100", 1000), Some((900, 1000)));
        assert_eq!(byte_range("bytes=-5000", 1000), Some((0, 1000)));
        assert_eq!(byte_range("bytes=990-2000", 1000), Some((990, 1000)));
        assert_eq!(byte_range("bytes=0-", 3 * mib), Some((0, mib)));
        assert_eq!(byte_range("bytes=10-", 3 * mib), Some((10, 10 + mib)));
        // At most what the caller gives: all a page's own file has.
        assert_eq!(super::byte_range("bytes=10-", 3 * mib, 3 * mib), Some((10, 3 * mib)));
        assert_eq!(super::byte_range("bytes=-5", 3 * mib, u64::MAX), Some((3 * mib - 5, 3 * mib)));
        for refused in [
            "bytes=0-1,5-6",
            "bytes=1000-",
            "bytes=5-4",
            "bytes=-0",
            "bytes=-",
            "bytes=a-b",
            "bytes=+1-2",
            "items=0-1",
            "0-1",
        ] {
            assert_eq!(byte_range(refused, 1000), None, "{refused}");
        }
        assert_eq!(byte_range("bytes=0-", 0), None, "an empty file has no byte to give");
    }

    #[test]
    fn a_path_is_decoded_once() {
        assert_eq!(decoded("/a%20b/%2e%2e").as_deref(), Some("/a b/.."));
        assert_eq!(decoded("/%252e").as_deref(), Some("/%2e"));
        for bad in ["/%", "/%2", "/%zz", "/%+f", "/%e9"] {
            assert_eq!(decoded(bad), None, "{bad}");
        }
    }

    #[test]
    fn only_media_files_are_resolved_types() {
        let kind = |name: &str| media_type(Path::new(name));
        assert_eq!(
            (kind("a.MP4"), kind("b.flac"), kind("c.png")),
            (Some("video/mp4"), Some("audio/flac"), Some("image/png"))
        );
        assert_eq!((kind("d.svg"), kind("e.html"), kind("f.mkv"), kind("g")), (None, None, None, None));
    }
}

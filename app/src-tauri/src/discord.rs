//! AutoScene on the person's Discord profile, over the local connection the
//! Discord app listens on: a named pipe on Windows, a socket elsewhere.
//!
//! Off by default. While it is off no thread runs and nothing is opened, so
//! nothing leaves the computer; [`Presence::configure`] alone starts and ends
//! the connection, from what `settings_update` saved, so a page can't switch
//! it on. The window only says what is on screen ([`DiscordView`]). What
//! Discord gets is decided here: fixed wording, a name only behind its own
//! switch, and the art, the link and the application fixed in this file.
//!
//! What comes back from the pipe is untrusted: frames are bounded by
//! [`MAX_FRAME`], every write and the greeting have a time limit, and nothing
//! it says is logged. A read on a live connection has none: Discord staying
//! quiet is normal, and that read only ever waits beside the updates and the
//! switch, never in their way. Nothing here logs a name.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, mpsc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use as_api::DiscordRpcSettings;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use specta::Type;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::watch;
use tokio::time::{Instant, sleep, sleep_until, timeout};

/// The Discord application whose name and art the profile shows.
const APP_ID: &str = "1567208524940058695";
/// The picture uploaded to that application, by its name there.
const ART: &str = "as-logo";
const ART_TEXT: &str = "AutoScene";
const BUTTON: &str = "Get AutoScene";
const LINK: &str = "https://github.com/tcmmi/AutoScene";

const OP_HELLO: u32 = 0;
const OP_FRAME: u32 = 1;
const OP_CLOSE: u32 = 2;
const OP_PING: u32 = 3;
const OP_PONG: u32 = 4;

/// The most one frame from Discord may claim. Its answers are a few KB.
const MAX_FRAME: usize = 64 * 1024;
/// Discord drops the whole status over a line under 2 characters or over 128
/// bytes. (The 2 comes from others' reports of its errors, not from its docs.)
const MIN_TEXT: usize = 2;
const MAX_TEXT: usize = 128;
/// Discord numbers its pipes from 0, one per running client.
const PIPES: u32 = 10;
/// A single write, and the greeting, may take this long.
const IO_LIMIT: Duration = Duration::from_secs(3);
/// Taking the status down when it is switched off or the app quits.
const BYE_LIMIT: Duration = Duration::from_secs(1);
/// How long a quit waits for the thread: past [`BYE_LIMIT`], never for good.
const END_LIMIT: Duration = Duration::from_millis(1500);
/// Discord takes 5 updates in 20 seconds: this keeps under that, the first
/// one right away and then the newest that waited.
const SEND_GAP: Duration = Duration::from_secs(5);
/// Looking for Discord again: this long at first, doubling up to
/// [`LONGEST_WAIT`]. Discord not running costs a look at 5, 15, 35 and 75 s,
/// then one a minute.
const FIRST_WAIT: Duration = Duration::from_secs(5);
const LONGEST_WAIT: Duration = Duration::from_secs(60);

/// What the person is doing, as the window sees it. Names are the person's
/// own data, and none of them is sent unless its switch is on.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DiscordView {
    pub doing: DiscordDoing,
    pub project: Option<String>,
    pub episode: Option<String>,
    pub character: Option<String>,
    /// How far the export is, while `doing` is `Export` and it is known.
    pub percent: Option<u8>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DiscordDoing {
    /// Not in a project: the library, Plugins, Settings.
    #[default]
    Idle,
    /// In a project, on none of the places below.
    Project,
    /// In the player, in a series.
    Episode,
    /// In the player, in a movie: those have files, not episodes.
    File,
    /// Making a scenepack. Only looking at a character is `Project`.
    Character,
    Export,
}

/// The two lines of the status under AutoScene's name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DiscordLines {
    pub details: String,
    pub state: Option<String>,
}

/// What a person sees for `view` with the switches `show`.
pub fn lines(view: &DiscordView, show: &DiscordRpcSettings) -> DiscordLines {
    let named = |on: bool, name: &Option<String>| name.as_deref().filter(|_| on).and_then(clean);
    if view.doing == DiscordDoing::Idle {
        return DiscordLines { details: "Between projects".into(), state: None };
    }
    let details = named(show.show_project, &view.project).unwrap_or_else(|| "Working on a project".into());
    let state = match view.doing {
        DiscordDoing::Idle | DiscordDoing::Project => None,
        DiscordDoing::Episode => {
            Some(named(show.show_episode, &view.episode).unwrap_or_else(|| "Reviewing an episode".into()))
        }
        DiscordDoing::File => {
            Some(named(show.show_episode, &view.episode).unwrap_or_else(|| "Reviewing a file".into()))
        }
        DiscordDoing::Character => {
            // Measured with its prefix: "L" is a name here, not too short.
            let name = view.character.as_deref().filter(|_| show.show_character).map(tidy);
            let line = name.filter(|name| !name.is_empty()).and_then(|name| clean(&format!("Scenepack: {name}")));
            Some(line.unwrap_or_else(|| "Making a scenepack".into()))
        }
        DiscordDoing::Export => Some(match view.percent {
            Some(percent) => format!("Exporting · {}%", percent.min(100)),
            None => "Exporting".into(),
        }),
    };
    DiscordLines { details, state }
}

/// What Settings shows for the switches `show`: in an episode, in a
/// scenepack. So a switch's effect is seen before anything is sent.
pub fn preview(show: &DiscordRpcSettings) -> Vec<DiscordLines> {
    let sample = |doing| DiscordView {
        doing,
        project: Some("Your project".into()),
        episode: Some("Episode 1".into()),
        character: Some("Your character".into()),
        percent: None,
    };
    vec![lines(&sample(DiscordDoing::Episode), show), lines(&sample(DiscordDoing::Character), show)]
}

/// `text` as Discord may show it, or `None` if too little of it is left.
fn clean(text: &str) -> Option<String> {
    let line = tidy(text);
    (line.chars().count() >= MIN_TEXT).then_some(line)
}

/// `text` without what a status shouldn't hold. Names are the person's, but
/// they come from file names and from the web: control and invisible
/// characters (bidi overrides, zero widths, blanks that pass for letters) go,
/// any run of white space is one space, and the rest is cut to whole
/// characters within Discord's limit in bytes.
fn tidy(text: &str) -> String {
    let seen: String = text.chars().filter(|c| !invisible(*c)).map(|c| if c.is_control() { ' ' } else { c }).collect();
    let words = seen.split_whitespace().map(|word| word.trim_matches(joiner)).filter(|word| !word.is_empty());
    let mut line = words.collect::<Vec<_>>().join(" ");
    if line.len() > MAX_TEXT {
        const MORE: &str = "…";
        line.truncate(line.floor_char_boundary(MAX_TEXT - MORE.len()));
        line.truncate(line.trim_end_matches(|c: char| c.is_whitespace() || joiner(c)).len());
        line.push_str(MORE);
    }
    line
}

/// Characters that show nothing themselves: they can change what is around
/// them, or are a blank that counts as a letter.
fn invisible(c: char) -> bool {
    matches!(c,
        '\u{AD}' | '\u{34F}' | '\u{61C}' | '\u{115F}' | '\u{1160}' | '\u{17B4}' | '\u{17B5}' | '\u{180E}'
        | '\u{200B}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{206F}' | '\u{2800}'
        | '\u{3164}' | '\u{FEFF}' | '\u{FFA0}' | '\u{FFF9}'..='\u{FFFB}' | '\u{E0000}'..='\u{E0FFF}')
}

/// Zero width too, but real text needs them inside a word: a family emoji is
/// one picture by them, and Persian spells with them. At a word's edge they
/// are nothing.
fn joiner(c: char) -> bool {
    matches!(c, '\u{200C}' | '\u{200D}')
}

/// The text of a frame: the whole status from `lines`, or taking it down.
fn activity(lines: Option<&DiscordLines>, started: u64, nonce: u64) -> String {
    let mut args = json!({ "pid": std::process::id() });
    if let Some(lines) = lines {
        let mut activity = json!({
            "details": lines.details,
            "timestamps": { "start": started },
            "assets": { "large_image": ART, "large_text": ART_TEXT },
            "buttons": [{ "label": BUTTON, "url": LINK }],
        });
        if let Some(state) = &lines.state {
            activity["state"] = json!(state);
        }
        args["activity"] = activity;
    }
    json!({ "cmd": "SET_ACTIVITY", "args": args, "nonce": nonce.to_string() }).to_string()
}

/// A frame as it goes down the pipe: its kind, its length, its text.
fn frame(op: u32, body: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + body.len());
    bytes.extend_from_slice(&op.to_le_bytes());
    bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
    bytes.extend_from_slice(body);
    bytes
}

/// The frames in what was read so far. Holds at most one frame and a read.
#[derive(Default)]
struct Frames(Vec<u8>);

impl Frames {
    /// The next whole frame, `Ok(None)` if more is needed, `Err` for a frame
    /// that claims more than [`MAX_FRAME`] (checked before anything is kept).
    fn pop(&mut self) -> Result<Option<(u32, Vec<u8>)>, ()> {
        let Some(head) = self.0.first_chunk::<8>() else { return Ok(None) };
        let op = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
        let len = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
        if len > MAX_FRAME {
            return Err(());
        }
        if self.0.len() < 8 + len {
            return Ok(None);
        }
        let body = self.0[8..8 + len].to_vec();
        self.0.drain(..8 + len);
        Ok(Some((op, body)))
    }
}

/// A pipe or a socket, or the tests' stand-in.
trait Pipe: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Pipe for T {}

/// A connection to Discord.
struct Wire {
    pipe: Box<dyn Pipe>,
    frames: Frames,
}

impl Wire {
    fn new(pipe: Box<dyn Pipe>) -> Self {
        Self { pipe, frames: Frames::default() }
    }

    /// Writes a frame; `false` if Discord doesn't take it in time.
    async fn write(&mut self, op: u32, body: &[u8]) -> bool {
        let bytes = frame(op, body);
        let pipe = &mut self.pipe;
        let written = async move {
            pipe.write_all(&bytes).await?;
            pipe.flush().await
        };
        matches!(timeout(IO_LIMIT, written).await, Ok(Ok(())))
    }

    /// The next frame; `None` once the pipe ended or sent one too long.
    /// Safe to drop while it waits: what was read stays in `frames`.
    async fn read(&mut self) -> Option<(u32, Vec<u8>)> {
        let mut bytes = [0; 4096];
        loop {
            if let Some(frame) = self.frames.pop().ok()? {
                return Some(frame);
            }
            match self.pipe.read(&mut bytes).await {
                Ok(0) | Err(_) => return None,
                Ok(n) => self.frames.0.extend_from_slice(&bytes[..n]),
            }
        }
    }

    /// Says who is calling; `true` once Discord answers that it is ready.
    async fn greet(&mut self) -> bool {
        #[derive(Deserialize)]
        struct Ready {
            evt: Option<String>,
        }
        let hello = json!({ "v": 1, "client_id": APP_ID }).to_string();
        if !self.write(OP_HELLO, hello.as_bytes()).await {
            return false;
        }
        match self.read().await {
            Some((OP_FRAME, body)) => {
                serde_json::from_slice::<Ready>(&body).is_ok_and(|r| r.evt.as_deref() == Some("READY"))
            }
            _ => false,
        }
    }
}

/// How the worker reaches Discord: `open` here, a stand-in in the tests.
type Dial = Arc<dyn Fn() -> Dialing + Send + Sync>;
type Dialing = Pin<Box<dyn Future<Output = Option<Wire>> + Send>>;
/// The status wanted now. Only the newest counts, so nothing queues up.
type Wanted = watch::Receiver<Option<DiscordLines>>;

/// The first pipe in `names` that answers as Discord.
async fn open(names: Vec<PathBuf>) -> Option<Wire> {
    for name in names {
        let Some(pipe) = dial(&name).await else { continue };
        if let Some(wire) = greeted(pipe).await {
            return Some(wire);
        }
    }
    None
}

/// `pipe`, if what is on it answers as Discord in time.
async fn greeted(pipe: Box<dyn Pipe>) -> Option<Wire> {
    let mut wire = Wire::new(pipe);
    timeout(IO_LIMIT, wire.greet()).await.unwrap_or(false).then_some(wire)
}

/// The names Discord listens on. No others are ever opened.
#[cfg(windows)]
fn names() -> Vec<PathBuf> {
    (0..PIPES).map(|n| PathBuf::from(format!(r"\\.\pipe\discord-ipc-{n}"))).collect()
}

/// The names Discord listens on, in the folders it uses, and for its Flatpak
/// and Snap builds in theirs. No others are ever opened.
#[cfg(unix)]
fn names() -> Vec<PathBuf> {
    let folders = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"].iter().filter_map(std::env::var_os).map(PathBuf::from);
    names_in(folders.chain([PathBuf::from("/tmp")]))
}

#[cfg(unix)]
fn names_in(folders: impl Iterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut names = Vec::new();
    for folder in folders {
        for inside in ["", "app/com.discordapp.Discord", "snap.discord"] {
            for n in 0..PIPES {
                let name = folder.join(inside).join(format!("discord-ipc-{n}"));
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
    }
    names
}

/// Opens a pipe. Tokio asks for it with the identification level only, so
/// whoever made a pipe of that name can't act as this person. Unlike a socket
/// its owner isn't checked: pipe names are machine-wide, so another account
/// logged in here could make one first and read the status.
#[cfg(windows)]
async fn dial(name: &Path) -> Option<Box<dyn Pipe>> {
    use tokio::net::windows::named_pipe::ClientOptions;
    Some(Box::new(ClientOptions::new().open(name).ok()?))
}

/// Opens a socket, if it is one and this person's: anyone can make a file of
/// that name in `/tmp`, and what is written here names their work.
#[cfg(unix)]
async fn dial(name: &Path) -> Option<Box<dyn Pipe>> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let found = std::fs::symlink_metadata(name).ok()?;
    // SAFETY: geteuid takes nothing and can't fail.
    if !found.file_type().is_socket() || found.uid() != unsafe { libc::geteuid() } {
        return None;
    }
    let socket = timeout(IO_LIMIT, tokio::net::UnixStream::connect(name)).await.ok()?.ok()?;
    Some(Box::new(socket))
}

/// Ends when nobody wants a status any more.
async fn off(mut wanted: Wanted) {
    while wanted.changed().await.is_ok() {}
}

/// How long to wait after `misses` looks that found nothing.
fn wait_after(misses: u32) -> Duration {
    FIRST_WAIT.saturating_mul(1 << misses.min(8)).min(LONGEST_WAIT)
}

#[derive(PartialEq)]
enum Ended {
    Off,
    Lost,
}

/// Keeps Discord showing what is wanted over `wire`, until one side leaves.
async fn serve(wire: &mut Wire, wanted: &mut Wanted, started: u64) -> Ended {
    let mut shown = None;
    let mut next_send = Instant::now();
    let mut nonce = 0;
    loop {
        let want = wanted.borrow().clone();
        let due = want != shown;
        tokio::select! {
            biased;
            changed = wanted.changed() => {
                if changed.is_err() {
                    // Discord takes the status down with the connection too.
                    let bye = async {
                        nonce += 1;
                        wire.write(OP_FRAME, activity(None, started, nonce).as_bytes()).await
                            && wire.write(OP_CLOSE, b"{}").await
                    };
                    let _ = timeout(BYE_LIMIT, bye).await;
                    return Ended::Off;
                }
            }
            read = wire.read() => match read {
                Some((OP_PING, body)) => {
                    if !wire.write(OP_PONG, &body).await {
                        return Ended::Lost;
                    }
                }
                Some((OP_CLOSE, _)) | None => return Ended::Lost,
                // Answers to what was sent: nothing here needs them.
                Some(_) => {}
            },
            () = sleep_until(next_send), if due => {
                nonce += 1;
                if !wire.write(OP_FRAME, activity(want.as_ref(), started, nonce).as_bytes()).await {
                    return Ended::Lost;
                }
                shown = want;
                next_send = Instant::now() + SEND_GAP;
            }
        }
    }
}

/// The worker: finds Discord, keeps it current, looks again if it goes.
/// Discord not being there is the normal case: nothing is logged for it.
async fn run(mut wanted: Wanted, started: u64, dial: Dial) {
    let mut misses = 0;
    loop {
        let wire = tokio::select! {
            biased;
            () = off(wanted.clone()) => return,
            wire = dial() => wire,
        };
        if let Some(mut wire) = wire {
            log::info!("connected to Discord");
            let since = Instant::now();
            if serve(&mut wire, &mut wanted, started).await == Ended::Off {
                return;
            }
            log::info!("the connection to Discord ended");
            // One that dropped at once doesn't start the waits over.
            if since.elapsed() >= LONGEST_WAIT {
                misses = 0;
            }
        }
        let wait = wait_after(misses);
        misses = misses.saturating_add(1);
        tokio::select! {
            biased;
            () = off(wanted.clone()) => return,
            () = sleep(wait) => {}
        }
    }
}

/// A running worker.
struct Link {
    wanted: watch::Sender<Option<DiscordLines>>,
    /// Hangs up when the thread is done.
    ended: mpsc::Receiver<()>,
}

impl Link {
    fn start(dial: Dial, started: u64) -> Option<Self> {
        let (wanted, watching) = watch::channel(None);
        let (done, ended) = mpsc::channel();
        let spawned = std::thread::Builder::new().name("discord".into()).spawn(move || {
            let _done = done;
            match tokio::runtime::Builder::new_current_thread().enable_io().enable_time().build() {
                Ok(runtime) => runtime.block_on(run(watching, started, dial)),
                Err(e) => log::warn!("starting the Discord connection: {e}"),
            }
        });
        match spawned {
            Ok(_) => Some(Self { wanted, ended }),
            Err(e) => {
                log::warn!("starting the Discord connection: {e}");
                None
            }
        }
    }

    /// Ends the worker, which takes the status down first, and waits for it.
    fn end(self) {
        drop(self.wanted);
        let _ = self.ended.recv_timeout(END_LIMIT);
    }
}

#[derive(Default)]
struct State {
    show: DiscordRpcSettings,
    view: DiscordView,
    link: Option<Link>,
}

impl State {
    fn publish(&self) {
        if let Some(link) = &self.link {
            link.wanted.send_replace(Some(lines(&self.view, &self.show)));
        }
    }
}

/// The person's status on Discord. Holds no thread while it is off.
pub struct Presence {
    state: Mutex<State>,
    dial: Dial,
    /// The status counts from here: the app's start.
    started: u64,
}

impl Default for Presence {
    fn default() -> Self {
        Self::dialing(Arc::new(|| -> Dialing { Box::pin(open(names())) }))
    }
}

impl Presence {
    fn dialing(dial: Dial) -> Self {
        let started = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        Self { state: Mutex::default(), dial, started }
    }

    /// Takes on what the person chose, starting or ending the connection. May
    /// wait up to a second and a half: not on the UI thread.
    pub fn configure(&self, show: &DiscordRpcSettings) {
        let ended = {
            let mut state = self.state.lock();
            state.show = show.clone();
            if show.enabled && state.link.is_none() {
                state.link = Link::start(self.dial.clone(), self.started);
            }
            let ended = if show.enabled { None } else { state.link.take() };
            // Next time starts from what the window says then, not from now.
            if !show.enabled {
                state.view = DiscordView::default();
            }
            state.publish();
            ended
        };
        if let Some(link) = ended {
            link.end();
        }
    }

    /// What is on screen now. Sends nothing while it is off, and turning it
    /// off forgets this.
    pub fn set_view(&self, view: DiscordView) {
        let mut state = self.state.lock();
        if state.view != view {
            state.view = view;
            state.publish();
        }
    }

    /// Ends the connection for the app's quit.
    pub fn stop(&self) {
        let link = self.state.lock().link.take();
        if let Some(link) = link {
            link.end();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde_json::Value;
    use tokio::io::{DuplexStream, duplex};

    use super::*;

    fn all_on() -> DiscordRpcSettings {
        DiscordRpcSettings { enabled: true, show_project: true, show_episode: true, show_character: true }
    }

    fn view(doing: DiscordDoing) -> DiscordView {
        DiscordView {
            doing,
            project: Some("Breaking Bad".into()),
            episode: Some("S01E01 · Pilot".into()),
            character: Some("Walter White".into()),
            percent: Some(42),
        }
    }

    fn line(details: &str, state: &str) -> DiscordLines {
        DiscordLines { details: details.into(), state: Some(state.into()) }
    }

    #[test]
    fn a_name_needs_its_own_switch() {
        let on = DiscordRpcSettings { enabled: true, ..Default::default() };
        for doing in [
            DiscordDoing::Idle,
            DiscordDoing::Project,
            DiscordDoing::Episode,
            DiscordDoing::Character,
            DiscordDoing::Export,
        ] {
            let text = format!("{:?}", lines(&view(doing), &on));
            for name in ["Breaking", "Pilot", "Walter"] {
                assert!(!text.contains(name), "{text}");
            }
        }
        assert_eq!(lines(&view(DiscordDoing::Episode), &on), line("Working on a project", "Reviewing an episode"));
        assert_eq!(lines(&view(DiscordDoing::Character), &on), line("Working on a project", "Making a scenepack"));

        // Each switch lets out its own name and no other.
        let project = DiscordRpcSettings { show_project: true, ..on.clone() };
        assert_eq!(lines(&view(DiscordDoing::Character), &project), line("Breaking Bad", "Making a scenepack"));
        let episode = DiscordRpcSettings { show_episode: true, ..on.clone() };
        assert_eq!(lines(&view(DiscordDoing::Episode), &episode), line("Working on a project", "S01E01 · Pilot"));
        let character = DiscordRpcSettings { show_character: true, ..on };
        assert_eq!(
            lines(&view(DiscordDoing::Character), &character),
            line("Working on a project", "Scenepack: Walter White")
        );
    }

    #[test]
    fn a_movie_has_files_not_episodes() {
        let on = DiscordRpcSettings { enabled: true, ..Default::default() };
        let film = view(DiscordDoing::File);
        assert_eq!(lines(&film, &on), line("Working on a project", "Reviewing a file"));
        let named = DiscordRpcSettings { show_episode: true, ..on };
        assert_eq!(lines(&film, &named), line("Working on a project", "S01E01 · Pilot"));
    }

    #[test]
    fn each_place_says_what_the_person_is_doing() {
        let show = all_on();
        let idle = DiscordLines { details: "Between projects".into(), state: None };
        assert_eq!(lines(&view(DiscordDoing::Idle), &show), idle);
        assert_eq!(
            lines(&view(DiscordDoing::Project), &show),
            DiscordLines { details: "Breaking Bad".into(), state: None }
        );
        assert_eq!(lines(&view(DiscordDoing::Export), &show), line("Breaking Bad", "Exporting · 42%"));
        let unknown = DiscordView { percent: None, ..view(DiscordDoing::Export) };
        assert_eq!(lines(&unknown, &show), line("Breaking Bad", "Exporting"));
        let over = DiscordView { percent: Some(255), ..view(DiscordDoing::Export) };
        assert_eq!(lines(&over, &show), line("Breaking Bad", "Exporting · 100%"));
    }

    #[test]
    fn settings_shows_both_places_as_the_switches_stand() {
        let off = DiscordRpcSettings { enabled: true, ..Default::default() };
        assert_eq!(
            preview(&off),
            [line("Working on a project", "Reviewing an episode"), line("Working on a project", "Making a scenepack")]
        );
        assert_eq!(
            preview(&all_on()),
            [line("Your project", "Episode 1"), line("Your project", "Scenepack: Your character")]
        );
    }

    #[test]
    fn a_hostile_name_stays_text_within_discords_limit() {
        // Line breaks, bidi overrides, zero widths and controls go.
        assert_eq!(clean("  Bad\r\n\u{202E}gnikaerB\u{200B}\t\u{0}x  ").unwrap(), "Bad gnikaerB x");
        assert_eq!(clean("a\u{2028}b").unwrap(), "a b");
        // What Discord would refuse the whole status for is no name at all.
        assert_eq!(clean("X"), None);
        assert_eq!(clean("\u{202E} \u{200B}"), None);
        // In characters: one that takes three bytes is still one.
        assert_eq!(clean("嵐"), None);
        assert_eq!(clean("嵐山").unwrap(), "嵐山");
        // Blanks that pass for letters are nothing, alone and among letters.
        for blank in ['\u{34F}', '\u{115F}', '\u{1160}', '\u{17B4}', '\u{17B5}', '\u{2800}', '\u{3164}', '\u{FFA0}'] {
            assert_eq!(clean(&format!("{blank}{blank}")), None, "{blank:?}");
            assert_eq!(clean(&format!("a{blank}b")).unwrap(), "ab", "{blank:?}");
        }
        // Joiners stay where real text needs them, and are nothing elsewhere.
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
        assert_eq!(clean(family).unwrap(), family);
        let persian = "می\u{200C}خواهم";
        assert_eq!(clean(persian).unwrap(), persian);
        assert_eq!(clean("\u{200D}\u{200C} \u{200D}"), None);
        assert_eq!(clean("\u{200D}ab\u{200C} \u{200D} cd").unwrap(), "ab cd");
        // Cut on a whole character, by bytes: an emoji is four.
        let long = clean(&"😀".repeat(200)).unwrap();
        assert!(long.len() <= MAX_TEXT, "{}", long.len());
        assert!(long.ends_with('…'));
        assert_eq!(clean(&"a".repeat(MAX_TEXT)).unwrap().len(), MAX_TEXT);
        // The cut never leaves a space before the dots.
        assert_eq!(clean(&format!("{} {}", "a".repeat(124), "b".repeat(9))).unwrap(), format!("{}…", "a".repeat(124)));
        // Nor a joiner.
        assert_eq!(
            clean(&format!("{}\u{200D}{}", "a".repeat(122), "b".repeat(9))).unwrap(),
            format!("{}…", "a".repeat(122))
        );

        // A character's line is measured with its prefix: a name of one
        // letter fits, and one with nothing to show isn't a name.
        let named = |name: &str| {
            let on = DiscordView { character: Some(name.into()), ..view(DiscordDoing::Character) };
            lines(&on, &all_on()).state.unwrap()
        };
        assert_eq!(named("L"), "Scenepack: L");
        assert_eq!(named("\u{200B}\u{3164}"), "Making a scenepack");

        // In the frame a name is a string, whatever it holds: it can't add a
        // button, a link or a picture, and a character line stays in limits.
        let evil = r#"","buttons":[{"label":"x","url":"https://evil.example"}],"details":"{project}"#;
        let show = all_on();
        let hostile = DiscordView {
            project: Some(evil.into()),
            character: Some("😀".repeat(100)),
            ..view(DiscordDoing::Character)
        };
        let said = lines(&hostile, &show);
        assert!(said.state.as_ref().unwrap().len() <= MAX_TEXT);
        let sent: Value = serde_json::from_str(&activity(Some(&said), 7, 1)).unwrap();
        let activity = &sent["args"]["activity"];
        assert_eq!(activity["details"], evil);
        assert_eq!(activity["buttons"], json!([{ "label": BUTTON, "url": LINK }]));
        assert_eq!(activity["assets"], json!({ "large_image": ART, "large_text": ART_TEXT }));
        assert_eq!(activity["timestamps"], json!({ "start": 7 }));
    }

    #[test]
    fn taking_the_status_down_sends_no_activity() {
        let sent: Value = serde_json::from_str(&activity(None, 7, 1)).unwrap();
        assert_eq!(sent["cmd"], "SET_ACTIVITY");
        assert_eq!(sent["args"]["pid"], std::process::id());
        assert!(sent["args"].get("activity").is_none());
    }

    #[test]
    fn frames_are_read_whole_and_bounded() {
        let mut frames = Frames::default();
        // Two in one read, then one in pieces.
        frames.0.extend_from_slice(&frame(OP_FRAME, b"one"));
        frames.0.extend_from_slice(&frame(OP_PING, b""));
        let last = frame(OP_FRAME, b"three");
        frames.0.extend_from_slice(&last[..5]);
        assert_eq!(frames.pop().unwrap(), Some((OP_FRAME, b"one".to_vec())));
        assert_eq!(frames.pop().unwrap(), Some((OP_PING, vec![])));
        assert_eq!(frames.pop().unwrap(), None);
        frames.0.extend_from_slice(&last[5..]);
        assert_eq!(frames.pop().unwrap(), Some((OP_FRAME, b"three".to_vec())));
        assert!(frames.0.is_empty());

        // One that claims too much is refused before it is read.
        for claimed in [MAX_FRAME as u32 + 1, u32::MAX] {
            let mut frames = Frames::default();
            frames.0.extend_from_slice(&OP_FRAME.to_le_bytes());
            frames.0.extend_from_slice(&claimed.to_le_bytes());
            assert!(frames.pop().is_err());
        }
    }

    /// A stand-in for Discord at the other end of a pipe.
    struct FakeDiscord {
        pipe: DuplexStream,
        frames: Frames,
    }

    impl FakeDiscord {
        async fn read(&mut self) -> Option<(u32, Vec<u8>)> {
            let mut bytes = [0; 4096];
            loop {
                if let Some(frame) = self.frames.pop().unwrap() {
                    return Some(frame);
                }
                match self.pipe.read(&mut bytes).await {
                    Ok(0) | Err(_) => return None,
                    Ok(n) => self.frames.0.extend_from_slice(&bytes[..n]),
                }
            }
        }

        async fn send(&mut self, op: u32, body: &[u8]) {
            self.pipe.write_all(&frame(op, body)).await.unwrap();
        }

        /// Answers the greeting as Discord does.
        async fn accept(&mut self) {
            let (op, body) = self.read().await.unwrap();
            assert_eq!(op, OP_HELLO);
            let hello: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(hello, json!({ "v": 1, "client_id": APP_ID }));
            self.send(OP_FRAME, br#"{"cmd":"DISPATCH","evt":"READY","data":{}}"#).await;
        }

        /// The next status it is told: `Some(details)`, `None` to take it down.
        async fn status(&mut self) -> Option<String> {
            let (op, body) = self.read().await.expect("a status");
            assert_eq!(op, OP_FRAME);
            let sent: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(sent["cmd"], "SET_ACTIVITY");
            sent["args"]["activity"]["details"].as_str().map(str::to_owned)
        }
    }

    /// A dial whose every call counts, and gets the next pipe of `pipes`
    /// (greeted), or nothing while there are none.
    fn dial_into(pipes: Vec<DuplexStream>) -> (Dial, Arc<AtomicUsize>) {
        let tries = Arc::new(AtomicUsize::new(0));
        let pipes = Arc::new(Mutex::new(pipes));
        let counted = tries.clone();
        let dial: Dial = Arc::new(move || -> Dialing {
            counted.fetch_add(1, Ordering::SeqCst);
            let pipe = pipes.lock().pop();
            Box::pin(async move {
                let mut wire = Wire::new(Box::new(pipe?));
                wire.greet().await.then_some(wire)
            })
        });
        (dial, tries)
    }

    fn pipe_pair() -> (DuplexStream, FakeDiscord) {
        let (ours, theirs) = duplex(1 << 16);
        (ours, FakeDiscord { pipe: theirs, frames: Frames::default() })
    }

    fn wanted(details: &str) -> Option<DiscordLines> {
        Some(DiscordLines { details: details.into(), state: None })
    }

    /// `what`, or the test fails: on a paused clock a wait for what never
    /// comes would hang it.
    async fn within<T>(what: impl Future<Output = T>) -> T {
        timeout(Duration::from_secs(600), what).await.expect("that never came")
    }

    /// A `Presence` whose worker runs here, on the test's clock.
    fn live(dial: Dial) -> (Presence, tokio::task::JoinHandle<()>) {
        let presence = Presence::dialing(dial.clone());
        let (wanted, watching) = watch::channel(None);
        let (_, ended) = mpsc::channel();
        presence.state.lock().link = Some(Link { wanted, ended });
        (presence, tokio::spawn(run(watching, 0, dial)))
    }

    #[tokio::test(start_paused = true)]
    async fn discord_not_running_soon_costs_only_a_look_a_minute() {
        let (dial, tries) = dial_into(vec![]);
        let (tx, rx) = watch::channel(wanted("Between projects"));
        let worker = tokio::spawn(run(rx, 0, dial));
        // Updates while it is gone don't start a look each.
        for _ in 0..1000 {
            tx.send_replace(wanted("Exporting"));
            tokio::task::yield_now().await;
        }
        assert_eq!(tries.load(Ordering::SeqCst), 1);
        // 5, 10, 20, 40, then a minute each.
        sleep(Duration::from_secs(76)).await;
        assert_eq!(tries.load(Ordering::SeqCst), 5);
        sleep(Duration::from_secs(600)).await;
        assert_eq!(tries.load(Ordering::SeqCst), 15);
        drop(tx);
        worker.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn updates_keep_to_discords_limit_and_the_newest_gets_through() {
        let (ours, mut discord) = pipe_pair();
        let (dial, _) = dial_into(vec![ours]);
        let (tx, rx) = watch::channel(wanted("first"));
        let worker = tokio::spawn(run(rx, 0, dial));
        discord.accept().await;
        assert_eq!(discord.status().await.as_deref(), Some("first"));
        let begun = Instant::now();

        // An export reporting four times a second, for twenty seconds.
        let sender = tokio::spawn(async move {
            for n in 0..80 {
                tx.send_replace(wanted(&format!("at {n}")));
                sleep(Duration::from_millis(250)).await;
            }
            tx.send_replace(wanted("last"));
            sleep(Duration::from_secs(10)).await;
            tx
        });
        let mut seen = Vec::new();
        while seen.last().map(String::as_str) != Some("last") {
            seen.push(discord.status().await.unwrap());
            assert!(begun.elapsed() >= SEND_GAP * seen.len() as u32 - Duration::from_millis(1));
        }
        assert!(seen.len() <= 5, "{seen:?}");
        drop(sender.await.unwrap());
        worker.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn the_same_status_is_not_sent_again() {
        let (ours, mut discord) = pipe_pair();
        let (dial, _) = dial_into(vec![ours]);
        let (tx, rx) = watch::channel(wanted("same"));
        let worker = tokio::spawn(run(rx, 0, dial));
        discord.accept().await;
        assert_eq!(discord.status().await.as_deref(), Some("same"));
        for _ in 0..20 {
            tx.send_replace(wanted("same"));
            sleep(Duration::from_secs(10)).await;
        }
        tx.send_replace(wanted("other"));
        // The very next one: nothing went between.
        assert_eq!(discord.status().await.as_deref(), Some("other"));
        drop(tx);
        worker.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn switching_off_takes_the_status_down_and_ends() {
        let (ours, mut discord) = pipe_pair();
        let (dial, _) = dial_into(vec![ours]);
        let (tx, rx) = watch::channel(wanted("shown"));
        let worker = tokio::spawn(run(rx, 0, dial));
        discord.accept().await;
        assert_eq!(discord.status().await.as_deref(), Some("shown"));
        drop(tx);
        assert_eq!(discord.status().await, None);
        assert_eq!(discord.read().await.unwrap().0, OP_CLOSE);
        worker.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn a_lost_discord_is_found_again_with_the_status_of_now() {
        let (first, mut gone) = pipe_pair();
        let (second, mut back) = pipe_pair();
        let (dial, tries) = dial_into(vec![second, first]);
        let (tx, rx) = watch::channel(wanted("before"));
        let worker = tokio::spawn(run(rx, 0, dial));
        gone.accept().await;
        assert_eq!(gone.status().await.as_deref(), Some("before"));
        tx.send_replace(wanted("meanwhile"));
        drop(gone);
        back.accept().await;
        assert_eq!(back.status().await.as_deref(), Some("meanwhile"));
        assert_eq!(tries.load(Ordering::SeqCst), 2);
        drop(tx);
        worker.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn discord_saying_goodbye_is_a_lost_connection() {
        let (first, mut leaving) = pipe_pair();
        let (second, mut back) = pipe_pair();
        let (dial, _) = dial_into(vec![second, first]);
        let (tx, rx) = watch::channel(wanted("shown"));
        let worker = tokio::spawn(run(rx, 0, dial));
        leaving.accept().await;
        assert_eq!(leaving.status().await.as_deref(), Some("shown"));
        // The pipe stays open: the close alone is what ends it.
        leaving.send(OP_CLOSE, b"{}").await;
        within(back.accept()).await;
        assert_eq!(back.status().await.as_deref(), Some("shown"));
        drop(tx);
        worker.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn only_a_connection_that_lasted_starts_the_waits_over() {
        let (pipes, discords): (Vec<_>, Vec<_>) = (0..4).map(|_| pipe_pair()).unzip();
        let (dial, _) = dial_into(pipes);
        let (tx, rx) = watch::channel(wanted("shown"));
        let worker = tokio::spawn(run(rx, 0, dial));
        // `dial_into` hands out the last pipe first.
        let mut discords = discords.into_iter().rev();
        let mut next = async || {
            let mut discord = discords.next().unwrap();
            let begun = Instant::now();
            within(discord.accept()).await;
            assert_eq!(discord.status().await.as_deref(), Some("shown"));
            (discord, begun.elapsed())
        };

        // Each one that drops at once waits longer than the last.
        drop(next().await.0);
        let (again, waited) = next().await;
        assert_eq!(waited, FIRST_WAIT);
        drop(again);
        let (lasting, waited) = next().await;
        assert_eq!(waited, FIRST_WAIT * 2);

        // One that lasted starts them over.
        sleep(LONGEST_WAIT).await;
        drop(lasting);
        let (last, waited) = next().await;
        assert_eq!(waited, FIRST_WAIT);
        drop(tx);
        worker.await.unwrap();
        drop(last);
    }

    #[tokio::test(start_paused = true)]
    async fn a_live_link_follows_the_view_and_the_switches() {
        let (ours, mut discord) = pipe_pair();
        let (dial, tries) = dial_into(vec![ours]);
        let (presence, worker) = live(dial);
        let named = DiscordRpcSettings { enabled: true, show_project: true, ..Default::default() };
        presence.configure(&named);
        presence.set_view(view(DiscordDoing::Project));
        discord.accept().await;
        assert_eq!(within(discord.status()).await.as_deref(), Some("Breaking Bad"));

        let elsewhere = DiscordView { project: Some("Better Call Saul".into()), ..view(DiscordDoing::Project) };
        presence.set_view(elsewhere);
        assert_eq!(within(discord.status()).await.as_deref(), Some("Better Call Saul"));

        // A name switched off stops going out, on the same connection.
        presence.configure(&DiscordRpcSettings { enabled: true, ..Default::default() });
        assert_eq!(within(discord.status()).await.as_deref(), Some("Working on a project"));
        assert_eq!(tries.load(Ordering::SeqCst), 1);

        presence.configure(&DiscordRpcSettings::default());
        assert_eq!(within(discord.status()).await, None);
        worker.await.unwrap();
    }

    #[test]
    fn turning_it_off_forgets_the_view() {
        let (dial, _) = dial_into(vec![]);
        let presence = Presence::dialing(dial);
        let named = DiscordRpcSettings { enabled: true, show_project: true, ..Default::default() };
        presence.configure(&named);
        presence.set_view(view(DiscordDoing::Export));
        presence.configure(&DiscordRpcSettings { enabled: false, ..named.clone() });
        assert_eq!(presence.state.lock().view, DiscordView::default());

        // On again: no name and no old percent wait for the window to catch up.
        presence.configure(&named);
        let said = presence.state.lock().link.as_ref().unwrap().wanted.borrow().clone();
        assert_eq!(said, Some(DiscordLines { details: "Between projects".into(), state: None }));
        presence.stop();
    }

    #[tokio::test(start_paused = true)]
    async fn a_write_that_sticks_is_given_up() {
        // Nobody reads the other end, and the frame is more than the pipe holds.
        let (ours, _reads_nothing) = duplex(16);
        let mut wire = Wire::new(Box::new(ours));
        let begun = Instant::now();
        assert!(!within(wire.write(OP_FRAME, &[0; 64])).await);
        assert_eq!(begun.elapsed(), IO_LIMIT);
    }

    #[tokio::test(start_paused = true)]
    async fn a_stuck_discord_holds_up_taking_the_status_down_only_briefly() {
        let (ours, theirs) = duplex(16);
        let mut discord = FakeDiscord { pipe: theirs, frames: Frames::default() };
        let (dial, _) = dial_into(vec![ours]);
        let (tx, rx) = watch::channel(wanted("shown"));
        let worker = tokio::spawn(run(rx, 0, dial));
        discord.accept().await;
        assert_eq!(discord.status().await.as_deref(), Some("shown"));
        // It reads no more, and the frame taking the status down doesn't fit.
        let begun = Instant::now();
        drop(tx);
        within(worker).await.unwrap();
        assert_eq!(begun.elapsed(), BYE_LIMIT);
    }

    #[tokio::test(start_paused = true)]
    async fn what_discord_says_is_bounded_and_answered_only_as_it_must_be() {
        let (ours, mut discord) = pipe_pair();
        let (dial, tries) = dial_into(vec![ours]);
        let (tx, rx) = watch::channel(wanted("shown"));
        let worker = tokio::spawn(run(rx, 0, dial));
        discord.accept().await;
        assert_eq!(discord.status().await.as_deref(), Some("shown"));

        // Answers and rubbish are read past, a ping gets its pong.
        discord.send(OP_FRAME, b"not json").await;
        discord.send(99, b"unknown").await;
        discord.send(OP_PING, b"are you there").await;
        assert_eq!(discord.read().await.unwrap(), (OP_PONG, b"are you there".to_vec()));

        // A frame claiming 4 GB ends the connection, not the app.
        let mut head = OP_FRAME.to_le_bytes().to_vec();
        head.extend_from_slice(&u32::MAX.to_le_bytes());
        discord.pipe.write_all(&head).await.unwrap();
        assert_eq!(discord.read().await, None);
        sleep(FIRST_WAIT + Duration::from_secs(1)).await;
        assert_eq!(tries.load(Ordering::SeqCst), 2);
        drop(tx);
        worker.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn a_pipe_that_never_answers_is_given_up() {
        let (ours, mut silent) = pipe_pair();
        // The greeting goes out, nothing comes back: it stops waiting.
        let begun = Instant::now();
        assert!(within(greeted(Box::new(ours))).await.is_none());
        assert_eq!(begun.elapsed(), IO_LIMIT);
        assert_eq!(silent.read().await.unwrap().0, OP_HELLO);

        // Something else on the name: an answer that isn't Discord's.
        for answer in [(OP_FRAME, &b"not json"[..]), (OP_FRAME, &br#"{"evt":"ERROR"}"#[..]), (OP_CLOSE, &b"{}"[..])] {
            let (ours, mut other) = pipe_pair();
            other.send(answer.0, answer.1).await;
            assert!(!Wire::new(Box::new(ours)).greet().await, "{answer:?}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn switching_off_ends_a_worker_that_is_waiting_for_discord() {
        // A greeting that hangs, and later the wait between looks.
        let dial: Dial = Arc::new(|| -> Dialing { Box::pin(std::future::pending()) });
        let (tx, rx) = watch::channel(None);
        let worker = tokio::spawn(run(rx, 0, dial));
        sleep(Duration::from_secs(1)).await;
        drop(tx);
        worker.await.unwrap();

        let (dial, _) = dial_into(vec![]);
        let (tx, rx) = watch::channel(None);
        let worker = tokio::spawn(run(rx, 0, dial));
        sleep(Duration::from_secs(1)).await;
        drop(tx);
        let begun = Instant::now();
        worker.await.unwrap();
        assert_eq!(begun.elapsed(), Duration::ZERO);
    }

    #[test]
    fn nothing_is_tried_while_it_is_off() {
        let (dial, tries) = dial_into(vec![]);
        let presence = Presence::dialing(dial);
        // As it starts, and with names switched on but not the whole.
        let off = DiscordRpcSettings { enabled: false, show_project: true, show_episode: true, show_character: true };
        presence.configure(&DiscordRpcSettings::default());
        presence.configure(&off);
        presence.set_view(view(DiscordDoing::Episode));
        presence.stop();
        std::thread::sleep(Duration::from_millis(50));
        assert!(presence.state.lock().link.is_none());
        assert_eq!(tries.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn its_thread_ends_with_its_link() {
        let (dial, _) = dial_into(vec![]);
        let Link { wanted, ended } = Link::start(dial, 0).unwrap();
        drop(wanted);
        assert_eq!(ended.recv_timeout(Duration::from_secs(10)), Err(mpsc::RecvTimeoutError::Disconnected));
    }

    /// Fails the test rather than hanging it.
    fn soon<T>(what: &str, got: Result<T, mpsc::RecvTimeoutError>) -> T {
        got.unwrap_or_else(|e| panic!("{what}: {e}"))
    }

    #[test]
    fn a_thread_that_wont_end_holds_a_quit_up_only_briefly() {
        let (dialed, in_dial) = mpsc::channel();
        let (release, stuck) = mpsc::channel::<()>();
        let stuck = Arc::new(Mutex::new(stuck));
        // Blocks its thread, so nothing there can notice the end.
        let dial: Dial = Arc::new(move || -> Dialing {
            let _ = dialed.send(());
            let _ = stuck.lock().recv();
            Box::pin(std::future::ready(None))
        });
        let link = Link::start(dial, 0).unwrap();
        soon("the dial", in_dial.recv_timeout(Duration::from_secs(10)));

        let (took, how_long) = mpsc::channel();
        std::thread::spawn(move || {
            let begun = std::time::Instant::now();
            link.end();
            took.send(begun.elapsed()).unwrap();
        });
        let elapsed = soon("the end", how_long.recv_timeout(Duration::from_secs(10)));
        assert!(elapsed >= END_LIMIT, "it didn't wait: {elapsed:?}");
        drop(release);
    }

    /// A stand-in for Discord on a thread of its own, for a `Presence` on its
    /// own thread, that tells each frame it gets and ends at the close.
    fn discord_on_a_thread(theirs: DuplexStream) -> (mpsc::Receiver<(u32, Value)>, std::thread::JoinHandle<()>) {
        let (statuses, told) = mpsc::channel();
        let discord = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            runtime.block_on(async move {
                let mut discord = FakeDiscord { pipe: theirs, frames: Frames::default() };
                discord.accept().await;
                while let Some((op, body)) = discord.read().await {
                    let sent: Value = serde_json::from_slice(&body).unwrap_or_default();
                    statuses.send((op, sent["args"]["activity"]["details"].clone())).unwrap();
                    if op == OP_CLOSE {
                        break;
                    }
                }
            });
        });
        (told, discord)
    }

    #[test]
    fn its_thread_comes_and_goes_with_the_switch() {
        let (ours, theirs) = duplex(1 << 16);
        let (told, discord) = discord_on_a_thread(theirs);
        let (dial, _) = dial_into(vec![ours]);
        let presence = Presence::dialing(dial);
        let names = DiscordRpcSettings { enabled: true, show_project: true, ..Default::default() };
        presence.set_view(view(DiscordDoing::Project));
        presence.configure(&names);
        let wait = Duration::from_secs(10);
        assert_eq!(soon("the status", told.recv_timeout(wait)), (OP_FRAME, json!("Breaking Bad")));

        presence.configure(&DiscordRpcSettings::default());
        assert!(presence.state.lock().link.is_none());
        assert_eq!(soon("taking it down", told.recv_timeout(wait)), (OP_FRAME, Value::Null));
        assert_eq!(soon("the close", told.recv_timeout(wait)).0, OP_CLOSE);
        discord.join().unwrap();
    }

    #[test]
    fn quitting_takes_the_status_down_and_ends_the_thread() {
        let (ours, theirs) = duplex(1 << 16);
        let (told, discord) = discord_on_a_thread(theirs);
        let (dial, _) = dial_into(vec![ours]);
        let presence = Presence::dialing(dial);
        presence.configure(&DiscordRpcSettings { enabled: true, ..Default::default() });
        let wait = Duration::from_secs(10);
        assert_eq!(soon("the status", told.recv_timeout(wait)), (OP_FRAME, json!("Between projects")));

        presence.stop();
        assert!(presence.state.lock().link.is_none());
        assert_eq!(soon("taking it down", told.recv_timeout(wait)), (OP_FRAME, Value::Null));
        assert_eq!(soon("the close", told.recv_timeout(wait)).0, OP_CLOSE);
        discord.join().unwrap();
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn a_real_pipe_is_greeted_and_kept_current() {
        use tokio::net::windows::named_pipe::ServerOptions;
        let name = format!(r"\\.\pipe\autoscene-discord-test-{}", std::process::id());
        let mut server = ServerOptions::new().first_pipe_instance(true).create(&name).unwrap();
        let answer = tokio::spawn(async move {
            server.connect().await.unwrap();
            let mut frames = Frames::default();
            let mut bytes = [0; 4096];
            let mut said = Vec::new();
            loop {
                while let Some((op, body)) = frames.pop().unwrap() {
                    said.push(op);
                    if op == OP_HELLO {
                        server.write_all(&frame(OP_FRAME, br#"{"evt":"READY"}"#)).await.unwrap();
                    }
                    if op == OP_FRAME {
                        let sent: Value = serde_json::from_slice(&body).unwrap();
                        return (said, sent["args"]["activity"]["details"].clone());
                    }
                }
                let n = server.read(&mut bytes).await.unwrap();
                frames.0.extend_from_slice(&bytes[..n]);
            }
        });
        // A name nothing listens on is passed over.
        let missing = PathBuf::from(format!("{name}-missing"));
        let mut wire = open(vec![missing, PathBuf::from(&name)]).await.unwrap();
        let status = wanted("Between projects");
        assert!(wire.write(OP_FRAME, activity(status.as_ref(), 0, 1).as_bytes()).await);
        assert_eq!(answer.await.unwrap(), (vec![OP_HELLO, OP_FRAME], json!("Between projects")));
    }

    #[cfg(windows)]
    #[test]
    fn only_discords_pipes_are_named() {
        let names = names();
        assert_eq!(names.len(), PIPES as usize);
        assert_eq!(names[0], Path::new(r"\\.\pipe\discord-ipc-0"));
        assert_eq!(names[9], Path::new(r"\\.\pipe\discord-ipc-9"));
    }

    #[cfg(unix)]
    #[test]
    fn only_discords_sockets_are_named() {
        let names = names_in(["/run/user/1000", "/tmp", "/tmp"].into_iter().map(PathBuf::from));
        assert_eq!(names.len(), 2 * 3 * PIPES as usize);
        assert!(names.contains(&PathBuf::from("/run/user/1000/discord-ipc-0")));
        assert!(names.contains(&PathBuf::from("/run/user/1000/app/com.discordapp.Discord/discord-ipc-0")));
        assert!(names.contains(&PathBuf::from("/tmp/snap.discord/discord-ipc-9")));
        for name in names {
            assert!(name.file_name().unwrap().to_str().unwrap().starts_with("discord-ipc-"), "{name:?}");
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn only_a_socket_is_opened() {
        let dir = std::env::temp_dir().join(format!("as-discord-socket-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // A file of that name, and a link from that name to a real socket.
        let file = dir.join("discord-ipc-0");
        std::fs::write(&file, "").unwrap();
        assert!(dial(&file).await.is_none());
        let socket = dir.join("real");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let link = dir.join("discord-ipc-1");
        std::os::unix::fs::symlink(&socket, &link).unwrap();
        assert!(dial(&link).await.is_none());
        assert!(dial(&socket).await.is_some());
        drop(listener);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn looks_wait_longer_up_to_a_minute() {
        let waits: Vec<_> = [0, 1, 2, 3, 4, 5, 1000].map(|n| wait_after(n).as_secs()).into();
        assert_eq!(waits, [5, 10, 20, 40, 60, 60, 60]);
    }
}

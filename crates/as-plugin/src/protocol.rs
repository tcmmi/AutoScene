//! The host–plugin protocol ([`crate::manifest::PROTOCOL`]): JSON lines in
//! UTF-8, one object per line, tagged by `"type"`, with camelCase fields.
//!
//! A run starts with one [`HostMessage::Run`] on the plugin's stdin: what to
//! do, with the context it needs (the export, the project, the plugin's
//! settings, where FFmpeg is). A [`HostMessage::Cancel`] may follow. The
//! plugin answers on stdout with [`PluginMessage`]s: progress and log lines,
//! then one result or error line, and exits. The first such line counts, and
//! its stdin closes once it is in. A line that isn't a message this
//! AutoScene knows (not an object, or an unknown `type`) goes to the
//! plugin's log and is never an error, so a plugin may print what it likes;
//! stderr goes to the log too.
//!
//! Version 2 adds calls: a plugin that declares it may ask AutoScene for
//! what it needs with a [`Call`] (its project, an export, its settings;
//! [`crate::calls`]), and gets a [`HostMessage::Reply`] with the same id on
//! its stdin. A plugin of version 1 gets the run line it always got, byte
//! for byte, and its call lines are output for its log.
//!
//! Version 2 also adds service mode ([`crate::service`]): a plugin whose
//! runtime is a service starts with a [`HostMessage::Serve`] line in place
//! of a run's, says it is ready ([`ServiceMessage::Ready`]), and from then
//! on answers each [`HostMessage::Request`] of its pages with a response of
//! the same id, until a [`HostMessage::Stop`]. It may emit events for its
//! open pages, and call AutoScene as a run does.
//!
//! Fields added later are `#[serde(default)]` when read and left out while
//! empty when written, so an older plugin's lines still read and an older
//! plugin's run line stays as it was.

use std::collections::BTreeMap;
use std::fmt;

use serde::de::{self, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use serde_json::value::RawValue;
use specta::Type;

use crate::calls::CallError;
use crate::manifest::{EventKind, PluginId, SettingValue, cut};

/// The longest id a call may give as a text; a call with a longer one is no
/// message.
pub const MAX_CALL_ID_CHARS: usize = 64;
/// The most of a call's method that is kept: a longer one is no method
/// AutoScene knows, and the log shows only its start anyway.
const MAX_METHOD_CHARS: usize = 64;

/// Host → plugin, on its stdin.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum HostMessage {
    /// What to do: the first line of every run, and the only one but
    /// `cancel`.
    Run(Box<RunMessage>),
    /// Stop soon. A plugin that is still running [`crate::runtime::CANCEL_GRACE`]
    /// later is ended, with everything it started.
    Cancel,
    /// The answer to a call (protocol 2).
    Reply(Reply),
    /// Service mode's first line, and its only one but those below.
    Serve(Box<ServeMessage>),
    /// A page's request to the service.
    Request(Request),
    /// The plugin's settings changed: each of them, as they are now.
    Settings { settings: BTreeMap<String, SettingValue> },
    /// End soon. A service that is still running
    /// [`crate::service::STOP_GRACE`] later is ended, with everything it
    /// started.
    Stop,
}

impl HostMessage {
    /// The message as its line, without the newline.
    pub fn line(&self) -> String {
        // Plain data: maps with string keys, and floats serde_json writes
        // as null when they aren't finite.
        serde_json::to_string(self).expect("host messages always serialise")
    }
}

/// The id of a call, which its reply carries: a number, or a text of at
/// most [`MAX_CALL_ID_CHARS`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum CallId {
    Number(u64),
    Text(String),
}

impl<'de> Deserialize<'de> for CallId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        MaybeId::deserialize(d)?.0.ok_or_else(|| de::Error::custom("a call id is a number or a short text"))
    }
}

/// What stands where a call's id goes: the id, when a reply may carry it.
/// Anything else is skipped as it is read, never kept, however much of the
/// line it takes.
struct MaybeId(Option<CallId>);

impl<'de> Deserialize<'de> for MaybeId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(IdVisitor).map(MaybeId)
    }
}

struct IdVisitor;

impl<'de> Visitor<'de> for IdVisitor {
    type Value = Option<CallId>;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a call id")
    }

    fn visit_u64<E: de::Error>(self, id: u64) -> Result<Self::Value, E> {
        Ok(Some(CallId::Number(id)))
    }

    fn visit_str<E: de::Error>(self, id: &str) -> Result<Self::Value, E> {
        Ok((id.chars().count() <= MAX_CALL_ID_CHARS).then(|| CallId::Text(id.to_owned())))
    }

    fn visit_i64<E: de::Error>(self, _: i64) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_f64<E: de::Error>(self, _: f64) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_bool<E: de::Error>(self, _: bool) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(None)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(None)
    }
}

/// What a call got: its result (`null` for an empty answer), or its error.
#[derive(Clone, Debug, Serialize)]
pub struct Reply {
    pub id: CallId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<CallError>,
}

/// A run's task and its context.
#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RunMessage {
    /// The protocol version its plugin declared, which the run speaks.
    pub protocol: u32,
    /// Unique per run; it heads the run's part of the log.
    pub run_id: String,
    pub plugin: PluginContext,
    pub app: AppContext,
    pub tools: ToolPaths,
    /// The plugin's settings, each of them, set or default.
    pub settings: BTreeMap<String, SettingValue>,
    pub action: ActionContext,
    /// The project it runs in, if any (palette commands may run without).
    pub project: Option<ProjectContext>,
    /// The export an export action runs on.
    pub export: Option<ExportContext>,
    /// What a page handed its run (protocol 2, `trigger: "page"` only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
    /// What happened, for a run of one of its events (`trigger: "event"`
    /// only); left out of every other run's line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<EventContext>,
}

/// A service's start: its context, as a run's line has it, without a task.
#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ServeMessage {
    /// The protocol its plugin declared: 2, which a service speaks.
    pub protocol: u32,
    /// Unique per start; it heads the service's part of its log.
    pub service_id: String,
    pub plugin: PluginContext,
    pub app: AppContext,
    pub tools: ToolPaths,
    /// The plugin's settings, each of them, set or default.
    pub settings: BTreeMap<String, SettingValue>,
}

/// A page's request to its plugin's service: `method` is the service's own
/// name for what it does, and its response carries the same id.
#[derive(Clone, Debug, Serialize)]
pub struct Request {
    /// `r1`, `r2`, … in the order the service is asked.
    pub id: String,
    pub method: String,
    /// `null` when the page gave none.
    pub params: Value,
    pub context: RequestContext,
}

/// Where a request comes from: the page, and the project it is open in.
#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RequestContext {
    /// The page's id in the plugin's `plugin.json`.
    pub page: String,
    pub project_id: Option<String>,
}

/// The plugin as installed.
#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginContext {
    pub id: PluginId,
    pub version: String,
    /// A folder the plugin may keep files in; runs start there.
    pub data_dir: String,
    /// The folder of the large files its `plugin.json` names, each checked
    /// by its hash ([`crate::resources`]): only when it names some, so no
    /// other plugin's line changes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources_dir: Option<String>,
    /// The run's temp folder (protocol 2), in its scratch folder: AutoScene
    /// empties it when the run ends ([`crate::scratch`]). A run of protocol
    /// 1 has none, and its line goes without it; so does a run whose folder
    /// couldn't be made, whose temp files go to the system's temp folder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temp_dir: Option<String>,
}

#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppContext {
    pub version: String,
    /// See [`crate::manifest::current_platform`].
    pub platform: String,
}

/// The app's FFmpeg, which a plugin may run itself. None for a
/// WebAssembly plugin ([`Default`]), which starts no program.
#[derive(Clone, Debug, Default, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ToolPaths {
    pub ffmpeg: String,
    pub ffprobe: String,
    pub ffmpeg_version: String,
}

/// What the person asked for, and how.
#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ActionContext {
    pub kind: ActionKind,
    /// The contribution's id in the manifest.
    pub id: String,
    pub trigger: Trigger,
}

/// Which of a plugin's contributions a run is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ActionKind {
    ExportAction,
    Command,
    /// One of its events (`contributes.events`), which AutoScene starts by
    /// itself: never from a menu, the palette or a page.
    Event,
}

/// What started a run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Trigger {
    /// The export's menu.
    Menu,
    /// An export that finished, with the action turned on to run after each.
    AfterExport,
    /// The command palette.
    Palette,
    /// A page of the plugin (`actions.run`), which starts only runs of a
    /// program that speaks protocol 2: a protocol 1 program never reads it.
    /// Its run gets what the page handed it as `input`.
    Page,
    /// What one of its events waits for happened, with the event turned on.
    /// Its run gets what happened as `event`.
    Event,
}

/// What happened, for a run of an event.
#[derive(Clone, Debug, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EventContext {
    /// What its event waits for (`on` in its `plugin.json`).
    pub kind: EventKind,
    /// When it happened (Unix milliseconds).
    pub at: i64,
    /// The episodes whose analysis ended since the project last told of
    /// one (`analysisFinished`).
    pub episodes: EpisodeCounts,
}

/// How the analyses of a project's episodes ended: those cancelled aren't
/// counted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeCounts {
    pub done: u32,
    pub failed: u32,
}

#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectContext {
    pub id: String,
    pub title: String,
    pub folder: String,
}

/// An export and its clips.
#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExportContext {
    pub id: String,
    pub label: Option<String>,
    pub preset: String,
    pub out_dir: String,
    /// Unix milliseconds.
    pub created_at: i64,
    /// The one file a merged export wrote, if it was merged.
    pub merged_file: Option<String>,
    pub clips: Vec<ClipContext>,
    /// How it went (protocol 2): `done`, `partial` (some of what it was to
    /// make is missing) or `failed` (it made nothing). Left out for a run of
    /// protocol 1, whose line stays as it was.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// One clip of an export, and the part of its episode it shows.
#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ClipContext {
    pub name: String,
    /// Where the clip was written, once it was.
    pub path: Option<String>,
    pub status: String,
    pub media_id: String,
    /// The episode's file.
    pub source_path: String,
    pub episode: String,
    /// The first frame of the episode the clip shows.
    pub start: u64,
    /// The frame after its last one.
    pub end: u64,
    /// The episode's frame rate as a fraction (`24000/1001`), if known.
    pub fps: Option<String>,
}

/// Plugin → host, on its stdout.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PluginMessage {
    /// How far the run is (`fraction`, 0 to 1, or not known) and what it
    /// does now, shown in Activity. Without a stage the plugin's name shows.
    Progress {
        #[serde(default)]
        fraction: Option<f64>,
        #[serde(default)]
        stage: Option<String>,
    },
    /// A line for the plugin's log.
    Log {
        #[serde(default)]
        level: LogLevel,
        message: String,
    },
    /// The run did its work. The message, if any, is shown to the person
    /// ("Posted 12 clips.").
    Result {
        #[serde(default)]
        message: Option<String>,
    },
    /// The run failed. The message is shown to the person; the detail goes
    /// to the logs.
    Error {
        message: String,
        #[serde(default)]
        detail: Option<String>,
    },
}

impl PluginMessage {
    /// The message a line of the plugin's stdout holds, if it holds one.
    /// Only an object is: serde would also read `["result"]` as one. A
    /// byte order mark before it is skipped, since some programs (.NET
    /// ones told to write UTF-8) write one first. A call is no message:
    /// it reads as a [`Call`].
    pub fn read(line: &str) -> Option<Self> {
        serde_json::from_str(object(line)?).ok()
    }
}

/// `line` without a byte order mark, if it may hold an object.
fn object(line: &str) -> Option<&str> {
    let line = line.strip_prefix('\u{FEFF}').unwrap_or(line);
    line.trim_start().starts_with('{').then_some(line)
}

/// Asks AutoScene for something (protocol 2), plugin to host: `method`
/// names a [`crate::calls::HostMethod`], and its reply carries the same id.
/// Its params stay the text the plugin wrote until the host reads them as
/// its method's own, once their size is checked: a call holds no more than
/// its line, whatever JSON it holds.
#[derive(Debug)]
pub struct Call {
    pub id: CallId,
    /// At most 64 characters: a longer one is no method AutoScene knows.
    pub method: String,
    /// `null` when it gave none.
    pub params: Box<RawValue>,
}

/// A call line as it is read. Not a [`PluginMessage`]: serde reads the line
/// of a tagged enum into a copy of its own first, which for 5 MB of small
/// numbers takes many times that.
#[derive(Deserialize)]
struct CallLine {
    #[serde(rename = "type")]
    kind: CallKind,
    id: CallId,
    method: String,
    #[serde(default)]
    params: Box<RawValue>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum CallKind {
    Call,
}

impl Call {
    /// The call a line of the plugin's stdout holds, if it holds one: an
    /// object, as [`PluginMessage::read`] reads one, of type `call`, with
    /// an id a reply may carry and a method.
    pub fn read(line: &str) -> Option<Self> {
        let CallLine { kind: CallKind::Call, id, method, params } = serde_json::from_str(object(line)?).ok()?;
        Some(Self { id, method: kept(method), params })
    }
}

/// `method` cut to the most of it that is kept.
fn kept(method: String) -> String {
    match method.len() > MAX_METHOD_CHARS {
        true => cut(&method, MAX_METHOD_CHARS),
        false => method,
    }
}

/// What a line that is no message shows of a call: one cut at its limit, or
/// one whose id, method or JSON doesn't fit. Its keys are read in turn until
/// the line breaks off, so its id and its method count when they came
/// first. A line starts a call when its type says so or, before any type,
/// it names a method or params.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CallHead {
    /// Its id, when it came before the break and a reply may carry it.
    pub id: Option<CallId>,
    pub method: Option<String>,
}

impl CallHead {
    pub(crate) fn read(line: &str) -> Option<Self> {
        let head = Head::read(line);
        let call = match &head.kind {
            Some(kind) => kind == "call",
            None => head.method.is_some() || head.params,
        };
        call.then_some(Self { id: head.id, method: head.method })
    }
}

/// Whether a service's line that is no message is a response cut at its
/// limit, and the id of the request it answers when that came before the
/// break. Its type may come after its id (serde_json writes its keys in
/// order): then a line that names a method or params is a call's, and one
/// with neither an id nor a result is no response. Its id may come after
/// its result too (`JSON.stringify` keeps the order the code wrote): then
/// it is a response all the same, whose request is unknown.
pub(crate) fn cut_response(line: &str) -> Option<Option<String>> {
    let head = Head::read(line);
    let response = match &head.kind {
        Some(kind) => kind == "response",
        None => head.method.is_none() && !head.params && (head.id.is_some() || head.result),
    };
    response.then_some(match head.id {
        Some(CallId::Text(id)) => Some(id),
        _ => None,
    })
}

/// The keys of a line that is no message, read in turn until it breaks off.
#[derive(Default)]
struct Head {
    kind: Option<String>,
    id: Option<CallId>,
    method: Option<String>,
    params: bool,
    /// It has a result or an error, as a response does.
    result: bool,
}

impl Head {
    fn read(line: &str) -> Self {
        let line = line.strip_prefix('\u{FEFF}').unwrap_or(line);
        let mut head = Self::default();
        // An error is where the line breaks off: what came before it counts.
        let _ = serde_json::Deserializer::from_str(line).deserialize_map(HeadVisitor(&mut head));
        head
    }
}

struct HeadVisitor<'a>(&'a mut Head);

impl<'de> Visitor<'de> for HeadVisitor<'_> {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a message")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "type" => self.0.kind = Some(map.next_value()?),
                "id" => self.0.id = map.next_value::<MaybeId>()?.0,
                "method" => self.0.method = Some(kept(map.next_value()?)),
                "params" => {
                    self.0.params = true;
                    map.next_value::<IgnoredAny>()?;
                }
                "result" | "error" => {
                    self.0.result = true;
                    map.next_value::<IgnoredAny>()?;
                }
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }
}

/// What a service says beside the messages a run says (service mode). A run
/// reads none of them: in a run's output they are lines for its log.
#[derive(Clone, Debug, PartialEq)]
pub enum ServiceMessage {
    /// It is ready for requests.
    Ready,
    /// Its answer to the request `id`: its result (`null` for an empty
    /// one), or why it couldn't do what was asked.
    Response { id: String, result: Value, error: Option<ServiceFault> },
    /// An event for the plugin's open pages.
    Emit { event: String, data: Value },
}

/// Why a service couldn't do what a page asked: a sentence for the page,
/// and what else it knows, for the log.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ServiceFault {
    pub message: String,
    #[serde(default)]
    pub detail: Option<String>,
}

/// A service's line as it is read: one object for each kind, so a
/// response's result is read once, into its value (a tagged enum reads it
/// into a copy of its own first).
#[derive(Deserialize)]
struct ServiceLine {
    #[serde(rename = "type")]
    kind: ServiceKind,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    result: Value,
    #[serde(default)]
    error: Option<ServiceFault>,
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    data: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum ServiceKind {
    Ready,
    Response,
    Emit,
}

impl ServiceMessage {
    /// The message a line of a service's stdout holds, if it holds one: an
    /// object, as [`PluginMessage::read`] reads one, whose response names
    /// its request and whose event its name.
    pub fn read(line: &str) -> Option<Self> {
        let ServiceLine { kind, id, result, error, event, data } = serde_json::from_str(object(line)?).ok()?;
        Some(match kind {
            ServiceKind::Ready => Self::Ready,
            ServiceKind::Response => Self::Response { id: id?, result, error },
            ServiceKind::Emit => Self::Emit { event: event?, data },
        })
    }
}

/// How much a log line matters. A level this AutoScene doesn't know reads
/// as `info`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LogLevel {
    Debug,
    Warn,
    Error,
    // Last: serde takes the unknown ones only into the last variant.
    #[default]
    #[serde(other)]
    Info,
}

impl LogLevel {
    pub fn name(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::calls::CallCode;

    fn run() -> RunMessage {
        RunMessage {
            protocol: 1,
            run_id: "run-1".into(),
            plugin: PluginContext {
                id: PluginId::parse("dev.autoscene.hello").unwrap(),
                version: "1.2.0".into(),
                data_dir: "/data".into(),
                resources_dir: None,
                temp_dir: None,
            },
            app: AppContext { version: "1.0.3-BETA".into(), platform: "linux-x86_64".into() },
            tools: ToolPaths { ffmpeg: "/ffmpeg".into(), ffprobe: "/ffprobe".into(), ffmpeg_version: "9.0".into() },
            settings: BTreeMap::new(),
            action: ActionContext { kind: ActionKind::Command, id: "hello".into(), trigger: Trigger::Palette },
            project: None,
            export: None,
            input: None,
            event: None,
        }
    }

    #[test]
    fn a_protocol_1_run_line_is_the_one_it_always_was() {
        assert_eq!(
            HostMessage::Run(Box::new(run())).line(),
            [
                r#"{"type":"run","protocol":1,"runId":"run-1","#,
                r#""plugin":{"id":"dev.autoscene.hello","version":"1.2.0","dataDir":"/data"},"#,
                r#""app":{"version":"1.0.3-BETA","platform":"linux-x86_64"},"#,
                r#""tools":{"ffmpeg":"/ffmpeg","ffprobe":"/ffprobe","ffmpegVersion":"9.0"},"settings":{},"#,
                r#""action":{"kind":"command","id":"hello","trigger":"palette"},"project":null,"export":null}"#,
            ]
            .concat()
        );
        // What a page hands a run of protocol 2 comes last.
        let mut message = run();
        (message.protocol, message.action.trigger) = (2, Trigger::Page);
        message.input = Some(json!({"words": 3}));
        // So does its scratch folder, after its data folder.
        message.plugin.temp_dir = Some("/tmp/as-k3j9x0qa".into());
        let line = HostMessage::Run(Box::new(message)).line();
        assert!(line.ends_with(r#""trigger":"page"},"project":null,"export":null,"input":{"words":3}}"#), "{line}");
        assert!(line.contains(r#""dataDir":"/data","tempDir":"/tmp/as-k3j9x0qa"},"#), "{line}");
    }

    /// An export of one clip, `status` as a run of its protocol gets it.
    fn export(status: Option<&str>) -> ExportContext {
        ExportContext {
            id: "e1".into(),
            label: None,
            preset: "h264-edit".into(),
            out_dir: "/out".into(),
            created_at: 7,
            merged_file: None,
            clips: vec![ClipContext {
                name: "Walter_001".into(),
                path: None,
                status: "failed".into(),
                media_id: "m1".into(),
                source_path: "/S01E01.mkv".into(),
                episode: "S01E01".into(),
                start: 0,
                end: 24,
                fps: None,
            }],
            status: status.map(str::to_owned),
        }
    }

    #[test]
    fn a_protocol_1_after_export_line_is_unchanged() {
        let mut message = run();
        message.action =
            ActionContext { kind: ActionKind::ExportAction, id: "post".into(), trigger: Trigger::AfterExport };
        message.export = Some(export(None));
        assert_eq!(
            HostMessage::Run(Box::new(message.clone())).line(),
            [
                r#"{"type":"run","protocol":1,"runId":"run-1","#,
                r#""plugin":{"id":"dev.autoscene.hello","version":"1.2.0","dataDir":"/data"},"#,
                r#""app":{"version":"1.0.3-BETA","platform":"linux-x86_64"},"#,
                r#""tools":{"ffmpeg":"/ffmpeg","ffprobe":"/ffprobe","ffmpegVersion":"9.0"},"settings":{},"#,
                r#""action":{"kind":"exportAction","id":"post","trigger":"afterExport"},"project":null,"#,
                r#""export":{"id":"e1","label":null,"preset":"h264-edit","outDir":"/out","createdAt":7,"mergedFile":null,"#,
                r#""clips":[{"name":"Walter_001","path":null,"status":"failed","mediaId":"m1","#,
                r#""sourcePath":"/S01E01.mkv","episode":"S01E01","start":0,"end":24,"fps":null}]}}"#,
            ]
            .concat()
        );
        // A run of protocol 2 is told how the export went, last.
        message.protocol = 2;
        message.export = Some(export(Some("partial")));
        let line = HostMessage::Run(Box::new(message)).line();
        assert!(line.ends_with(r#""fps":null}],"status":"partial"}}"#), "{line}");
    }

    #[test]
    fn an_event_run_line_reads_as_the_guide_shows() {
        let guide = include_str!("../../../docs/PLUGINS.md");
        let start = guide.find("A run of an event").expect("the guide shows a run of an event");
        let block = &guide[start..];
        let block = &block[block.find("```json\n").unwrap() + 8..];
        let shown: Value = serde_json::from_str(&block[..block.find("```").unwrap()]).unwrap();

        let mut message = run();
        message.action = ActionContext { kind: ActionKind::Event, id: "analysis-done".into(), trigger: Trigger::Event };
        message.project = Some(ProjectContext {
            id: "019a2b3c-4d5e-7f60-8a9b-0c1d2e3f4a5b".into(),
            title: "Breaking Bad".into(),
            folder: r"D:\AutoScene Library\Breaking Bad".into(),
        });
        message.event = Some(EventContext {
            kind: EventKind::AnalysisFinished,
            at: 1_762_150_000_000,
            episodes: EpisodeCounts { done: 7, failed: 1 },
        });
        let line: Value = serde_json::from_str(&HostMessage::Run(Box::new(message.clone())).line()).unwrap();
        let shown = shown.as_object().unwrap();
        // The guide shows what differs from the line above it.
        assert_eq!(shown.keys().collect::<Vec<_>>(), ["action", "event", "export", "project"]);
        for (key, value) in shown {
            assert_eq!(&line[key], value, "{key}");
        }
        // It comes last, and only on an event's run.
        let line = HostMessage::Run(Box::new(message.clone())).line();
        assert!(line.ends_with(r#""export":null,"event":{"kind":"analysisFinished","at":1762150000000,"episodes":{"done":7,"failed":1}}}"#), "{line}");
        message.event = None;
        assert!(!HostMessage::Run(Box::new(message)).line().contains(r#""event":"#));
    }

    #[test]
    fn a_reply_says_its_result_or_its_error() {
        let reply = |id, result, error| HostMessage::Reply(Reply { id, result, error }).line();
        assert_eq!(
            reply(CallId::Number(7), Some(json!({"projectId": "p1"})), None),
            r#"{"type":"reply","id":7,"result":{"projectId":"p1"}}"#
        );
        let denied = CallError::new(CallCode::Denied, "project.get needs the project:read permission.");
        assert_eq!(
            reply(CallId::Text("a".into()), None, Some(denied)),
            r#"{"type":"reply","id":"a","error":{"code":"denied","message":"project.get needs the project:read permission."}}"#
        );
        // An empty answer is still a result.
        assert_eq!(reply(CallId::Number(0), Some(Value::Null), None), r#"{"type":"reply","id":0,"result":null}"#);
    }

    /// What a call line reads as: its id, its method and its params' text.
    fn call(line: &str) -> Option<(CallId, String, String)> {
        Call::read(line).map(|c| (c.id, c.method, c.params.get().to_owned()))
    }

    #[test]
    fn call_ids_are_numbers_or_short_texts() {
        let with = |id: Value| call(&json!({"type": "call", "id": id, "method": "project.get"}).to_string());
        let id = |id| Some((id, "project.get".to_owned(), "null".to_owned()));
        assert_eq!(with(json!(7)), id(CallId::Number(7)));
        assert_eq!(with(json!("a")), id(CallId::Text("a".into())));
        assert_eq!(with(json!("x".repeat(MAX_CALL_ID_CHARS))), id(CallId::Text("x".repeat(MAX_CALL_ID_CHARS))));
        for other in
            [json!("x".repeat(MAX_CALL_ID_CHARS + 1)), json!(-1), json!(1.5), json!(null), json!([1]), json!({})]
        {
            assert_eq!(with(other.clone()), None, "{other}");
        }
        // Its params come as the plugin wrote them, a byte order mark before it skipped.
        assert_eq!(
            call(
                "\u{FEFF}{\"type\":\"call\",\"params\": {\"key\" : \"k\"} ,\"id\":1,\"method\":\"state.get\",\"later\":[1]}"
            ),
            Some((CallId::Number(1), "state.get".into(), r#"{"key" : "k"}"#.into()))
        );
        // A method longer than any keeps only its start.
        let long = call(&json!({"type": "call", "id": 1, "method": "m".repeat(5000)}).to_string()).unwrap();
        assert_eq!(long.1.chars().count(), 64);
        for line in [
            r#"{"type":"call","id":1}"#,
            r#"{"type":"call","id":1,"method":2}"#,
            r#"{"type":"result","id":1,"method":"state.get"}"#,
            r#"{"id":1,"method":"state.get"}"#,
            r#"[{"type":"call","id":1,"method":"state.get"}]"#,
            r#"{"type":"call","id":1,"method":"state.get","params":{"#,
        ] {
            assert_eq!(call(line), None, "{line}");
        }
    }

    #[test]
    fn a_broken_call_shows_its_id_and_method_and_nothing_else() {
        let head = CallHead::read;
        let call = |id: Option<CallId>, method: Option<&str>| Some(CallHead { id, method: method.map(str::to_owned) });
        // Cut inside its params: what came before them counts.
        let cut = r#"{"type":"call","id":1,"method":"state.set","params":{"key":"k","value":"SECRET"#;
        assert_eq!(head(cut), call(Some(CallId::Number(1)), Some("state.set")));
        let untyped = r#"{"method":"net.fetch","id":"a","params":{"body":"SECRET"#;
        assert_eq!(head(untyped), call(Some(CallId::Text("a".into())), Some("net.fetch")));
        // An id after the break, or one a reply can't carry, is none.
        let late_id = r#"{"type":"call","method":"net.fetch","params":{"body":"SECRET"#;
        assert_eq!(head(late_id), call(None, Some("net.fetch")));
        for id in [
            json!(-1),
            json!(1.5),
            json!("x".repeat(MAX_CALL_ID_CHARS + 1)),
            json!(null),
            json!(true),
            json!([1, [2]]),
            json!({"a": {"b": 1}}),
        ] {
            let line = json!({"type": "call", "id": id, "method": "state.get", "params": {}}).to_string();
            assert_eq!(head(&line), call(None, Some("state.get")), "{line}");
        }
        assert_eq!(head(r#"{"params":{"x":"#), call(None, None));
        // Of a method longer than any, only its start is kept.
        let long = head(&format!(r#"{{"type":"call","method":"{}","params":{{"#, "m".repeat(5000))).unwrap();
        assert_eq!(long.method.unwrap().chars().count(), 64);
        assert_eq!(head("\u{FEFF}{\"type\":\"call\",\"id\":3}"), call(Some(CallId::Number(3)), None));
        // Other lines aren't calls.
        for line in [r#"{"type":"result","message":"SECRET"#, r#"{"message":"x"}"#, "[1]", "call", ""] {
            assert_eq!(head(line), None, "{line}");
        }
    }

    #[test]
    fn a_service_hears_its_start_its_requests_and_its_stop() {
        let r = run();
        let serve = ServeMessage {
            protocol: 2,
            service_id: "service-1".into(),
            plugin: r.plugin,
            app: r.app,
            tools: r.tools,
            settings: BTreeMap::from([("loud".into(), SettingValue::Bool(true))]),
        };
        assert_eq!(
            HostMessage::Serve(Box::new(serve)).line(),
            [
                r#"{"type":"serve","protocol":2,"serviceId":"service-1","#,
                r#""plugin":{"id":"dev.autoscene.hello","version":"1.2.0","dataDir":"/data"},"#,
                r#""app":{"version":"1.0.3-BETA","platform":"linux-x86_64"},"#,
                r#""tools":{"ffmpeg":"/ffmpeg","ffprobe":"/ffprobe","ffmpegVersion":"9.0"},"settings":{"loud":true}}"#,
            ]
            .concat()
        );
        let context = RequestContext { page: "main".into(), project_id: None };
        let request = Request { id: "r1".into(), method: "count".into(), params: json!({}), context };
        assert_eq!(
            HostMessage::Request(request).line(),
            r#"{"type":"request","id":"r1","method":"count","params":{},"context":{"page":"main","projectId":null}}"#
        );
        let settings = BTreeMap::from([("count".into(), SettingValue::Number(3.0))]);
        assert_eq!(HostMessage::Settings { settings }.line(), r#"{"type":"settings","settings":{"count":3.0}}"#);
        assert_eq!(HostMessage::Stop.line(), r#"{"type":"stop"}"#);
    }

    #[test]
    fn a_service_says_it_is_ready_answers_and_emits() {
        let read = ServiceMessage::read;
        assert_eq!(read(r#"{"type":"ready"}"#), Some(ServiceMessage::Ready));
        assert_eq!(
            read(r#"{"type":"response","id":"r1","result":{"words":3},"later":1}"#),
            Some(ServiceMessage::Response { id: "r1".into(), result: json!({"words": 3}), error: None })
        );
        // An empty answer is still one.
        assert_eq!(
            read("\u{FEFF}{\"type\":\"response\",\"id\":\"r2\"}"),
            Some(ServiceMessage::Response { id: "r2".into(), result: Value::Null, error: None })
        );
        let fault = ServiceFault { message: "No text.".into(), detail: Some("KeyError".into()) };
        assert_eq!(
            read(r#"{"type":"response","id":"r3","error":{"message":"No text.","detail":"KeyError"}}"#),
            Some(ServiceMessage::Response { id: "r3".into(), result: Value::Null, error: Some(fault) })
        );
        assert_eq!(
            read(r#"{"type":"emit","event":"counted","data":[1]}"#),
            Some(ServiceMessage::Emit { event: "counted".into(), data: json!([1]) })
        );
        for line in [
            r#"{"type":"response","result":1}"#,
            r#"{"type":"response","id":1}"#,
            r#"{"type":"response","id":"r1","error":"No."}"#,
            r#"{"type":"emit","data":1}"#,
            r#"{"type":"result"}"#,
            r#"{"type":"call","id":1,"method":"settings.get"}"#,
            r#"["ready"]"#,
            "ready",
        ] {
            assert_eq!(read(line), None, "{line}");
        }
        // A run reads none of them as its own.
        assert_eq!(PluginMessage::read(r#"{"type":"ready"}"#), None);
    }

    #[test]
    fn a_cut_response_names_its_request() {
        let cut = format!(r#"{{"type":"response","id":"r12","result":"{}"#, "x".repeat(100));
        assert_eq!(cut_response(&cut), Some(Some("r12".to_owned())));
        // Its keys in serde_json's order: its type after the break.
        assert_eq!(cut_response(r#"{"id":"r12","result":"xxx"#), Some(Some("r12".to_owned())));
        // Its id after the break, or not text: a response all the same, of an unknown request.
        for line in [
            r#"{"type":"response","result":{"token":"xxx"#,
            r#"{"result":{"token":"xxx"#,
            r#"{"type":"response","id":12,"result":"#,
        ] {
            assert_eq!(cut_response(line), Some(None), "{line}");
        }
        // A call's, or not a response: none.
        for line in [
            r#"{"type":"emit","id":"r12","data":"#,
            r#"{"id":"c1","method":"net.fetch","params":{"body":"#,
            r#"{"id":"c1","params":{"body":"#,
            r#"{"type":"log","message":"xxx"#,
        ] {
            assert_eq!(cut_response(line), None, "{line}");
        }
    }

    #[test]
    fn host_messages_are_one_line_each() {
        assert_eq!(HostMessage::Cancel.line(), r#"{"type":"cancel"}"#);
        let line = HostMessage::Run(Box::new(run())).line();
        assert!(line.starts_with(r#"{"type":"run","protocol":1,"runId":"run-1","plugin":{"id":"dev.autoscene.hello""#));
        assert!(line.ends_with(
            r#""action":{"kind":"command","id":"hello","trigger":"palette"},"project":null,"export":null}"#
        ));
        assert!(!line.contains('\n'));
    }

    #[test]
    fn settings_travel_as_plain_json_values() {
        let mut message = run();
        message.settings = BTreeMap::from([
            ("count".into(), SettingValue::Number(3.0)),
            ("loud".into(), SettingValue::Bool(true)),
            ("name".into(), SettingValue::Text("Line\n2".into())),
            ("odd".into(), SettingValue::Number(f64::NAN)),
        ]);
        let line = HostMessage::Run(Box::new(message)).line();
        assert!(line.contains(r#""settings":{"count":3.0,"loud":true,"name":"Line\n2","odd":null}"#), "{line}");
    }

    #[test]
    fn actions_and_triggers_are_camel_case() {
        assert_eq!(serde_json::to_string(&ActionKind::ExportAction).unwrap(), r#""exportAction""#);
        assert_eq!(serde_json::to_string(&Trigger::AfterExport).unwrap(), r#""afterExport""#);
        assert_eq!(serde_json::to_string(&Trigger::Page).unwrap(), r#""page""#);
        assert_eq!(serde_json::to_string(&Trigger::Event).unwrap(), r#""event""#);
        assert_eq!(serde_json::to_string(&ActionKind::Event).unwrap(), r#""event""#);
        assert_eq!(serde_json::from_str::<Trigger>(r#""menu""#).unwrap(), Trigger::Menu);
        assert_eq!(serde_json::from_str::<ActionKind>(r#""command""#).unwrap(), ActionKind::Command);
    }

    #[test]
    fn plugin_lines_read_with_their_defaults() {
        let read = |line: &str| PluginMessage::read(line).unwrap();
        assert_eq!(read(r#"{"type":"progress"}"#), PluginMessage::Progress { fraction: None, stage: None });
        assert_eq!(
            read(r#"{"type":"progress","fraction":0.25,"stage":"Posting","later":1}"#),
            PluginMessage::Progress { fraction: Some(0.25), stage: Some("Posting".into()) }
        );
        assert_eq!(
            read(r#"{"type":"log","message":"hi"}"#),
            PluginMessage::Log { level: LogLevel::Info, message: "hi".into() }
        );
        assert_eq!(
            read(r#"{"type":"log","level":"warn","message":"hi"}"#),
            PluginMessage::Log { level: LogLevel::Warn, message: "hi".into() }
        );
        assert_eq!(
            read(r#"{"type":"log","level":"loud","message":"hi"}"#),
            PluginMessage::Log { level: LogLevel::Info, message: "hi".into() }
        );
        assert_eq!(read(r#"{"type":"result"}"#), PluginMessage::Result { message: None });
        assert_eq!(
            read(r#"{"type":"result","message":"Posted 12 clips."}"#),
            PluginMessage::Result { message: Some("Posted 12 clips.".into()) }
        );
        assert_eq!(
            read(r#"{"type":"error","message":"No webhook."}"#),
            PluginMessage::Error { message: "No webhook.".into(), detail: None }
        );
        assert_eq!(read("\u{FEFF}{\"type\":\"result\"}"), PluginMessage::Result { message: None });
    }

    #[test]
    fn other_lines_are_not_messages() {
        for line in [
            "hello",
            "",
            r#"{"type":"future","x":1}"#,
            r#"{"message":"no type"}"#,
            r#"{"type":"error"}"#,
            r#"{"type":"log"}"#,
            r#"{"type":"progress","fraction":"half"}"#,
            r#"["result"]"#,
            r#"["error","Something broke."]"#,
            "null",
            // A call reads as one, never as a message.
            r#"{"type":"call","id":1,"method":"settings.get"}"#,
        ] {
            assert_eq!(PluginMessage::read(line), None, "{line}");
        }
        assert!(PluginMessage::read(r#"  {"type":"result"}"#).is_some());
    }
}

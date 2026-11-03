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
use crate::manifest::{PluginId, SettingValue, cut};

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
}

/// The plugin as installed.
#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginContext {
    pub id: PluginId,
    pub version: String,
    /// A folder the plugin may keep files in; runs start there.
    pub data_dir: String,
}

#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppContext {
    pub version: String,
    /// See [`crate::manifest::current_platform`].
    pub platform: String,
}

/// The app's FFmpeg, which a plugin may run itself.
#[derive(Clone, Debug, Serialize, Type)]
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
    /// A page of the plugin (protocol 2). Reserved: no run starts so yet.
    Page,
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
        let line = line.strip_prefix('\u{FEFF}').unwrap_or(line);
        let mut head = Head::default();
        // An error is where the line breaks off: what came before it counts.
        let _ = serde_json::Deserializer::from_str(line).deserialize_map(HeadVisitor(&mut head));
        let call = match &head.kind {
            Some(kind) => kind == "call",
            None => head.method.is_some() || head.params,
        };
        call.then_some(Self { id: head.id, method: head.method })
    }
}

#[derive(Default)]
struct Head {
    kind: Option<String>,
    id: Option<CallId>,
    method: Option<String>,
    params: bool,
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
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(())
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
            },
            app: AppContext { version: "1.0.3-BETA".into(), platform: "linux-x86_64".into() },
            tools: ToolPaths { ffmpeg: "/ffmpeg".into(), ffprobe: "/ffprobe".into(), ffmpeg_version: "9.0".into() },
            settings: BTreeMap::new(),
            action: ActionContext { kind: ActionKind::Command, id: "hello".into(), trigger: Trigger::Palette },
            project: None,
            export: None,
            input: None,
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
        let line = HostMessage::Run(Box::new(message)).line();
        assert!(line.ends_with(r#""trigger":"page"},"project":null,"export":null,"input":{"words":3}}"#), "{line}");
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

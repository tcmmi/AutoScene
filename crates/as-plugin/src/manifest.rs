//! The manifest (`plugin.json`): what a plugin is, what it says it does, and
//! what it adds to AutoScene (export actions, palette commands, settings).
//!
//! A manifest names the versions it was written for (`manifestVersion`,
//! `engines.protocol`, `runtime.type`) and the AutoScene it needs
//! (`engines.autoscene`). A plugin that needs more than this app is refused
//! with [`PluginError::TooNew`] before the rest is read, so a field a later
//! AutoScene added never reads as a mistake. Anything else this app doesn't
//! know is judged by what it could cost: an unknown permission refuses the
//! plugin, since it may stand for more than the person agreed to, while an
//! unknown key, contribution or setting type is skipped with a warning.
//!
//! Manifest version 2 adds secret settings, kept in the system's keychain
//! ([`crate::secrets`]) for the plugin's program or pages to ask for (a
//! process plugin of protocol 1 without a page has nothing that could, so
//! they are skipped with a warning there), pages ([`PageSpec`]: an `.html`
//! file under `ui/` that AutoScene shows in a sandboxed frame,
//! [`crate::pages`]), and the runtime type `none`, of a plugin that runs no
//! program and has only pages. The runtime type `service` is reserved in it
//! for a later AutoScene. A version 1 manifest reads as it always did: its
//! pages and secret settings are skipped with a warning that names version
//! 2, and a runtime type only version 2 has is refused with that hint. In
//! version 2, a network host that names this computer or its network, or an
//! address, gets a warning: `net.fetch` never reaches it ([`crate::net`]).
//!
//! No shell reads a command: its arguments are fixed, and handed to the
//! program as they are. The program is a file in the plugin's own folder,
//! started by its path there ([`Launch::entry_in`]) and never looked up on
//! the PATH, or a Python script run through `uv run --script --locked`,
//! where the script's lock decides what it installs. Paths outside the
//! folder are refused. So are the names of the computer's shells and
//! interpreters (`python3`, `cmd`, `sh`), which would only find a file of
//! that name in the folder, and on Windows any program but an `.exe`:
//! Windows runs a `.bat` or `.cmd` through cmd.exe, and for `bin/tool` the
//! standard library starts `bin/tool.exe` if there is one. A program may
//! still be a script with a `#!` line on Linux and macOS; it is the
//! plugin's own file, like any program.

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use semver::{BuildMetadata, Comparator, Op, Prerelease, Version, VersionReq};
use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use specta::Type;

use crate::error::{PluginError, PluginResult};
use crate::net;
use crate::package::{check_path, reserved};

/// The newest manifest version this AutoScene reads.
pub const MANIFEST_VERSION: u32 = 2;
/// The newest version of the host–plugin protocol this AutoScene speaks. A
/// plugin is spoken to in the one it declares (`engines.protocol`).
pub const PROTOCOL: u32 = 2;
pub const MAX_MANIFEST_BYTES: u64 = 256 << 10;
pub const MAX_ACTIONS: usize = 8;
pub const MAX_COMMANDS: usize = 16;
pub const MAX_SETTINGS: usize = 32;
/// The most secret settings a plugin may have.
pub const MAX_SECRETS: usize = 8;
/// The most pages a plugin may add.
pub const MAX_PAGES: usize = 4;

/// The platforms a manifest can name, as `<os>-<arch>`.
pub const PLATFORMS: [&str; 6] =
    ["windows-x86_64", "windows-aarch64", "macos-aarch64", "macos-x86_64", "linux-x86_64", "linux-aarch64"];

/// The most parts a command may have (the program and its arguments).
const MAX_ARGS: usize = 16;
const MAX_ARG_CHARS: usize = 256;
const MAX_HOSTS: usize = 16;
const MAX_PLATFORMS: usize = 16;
const MAX_OPTIONS: usize = 32;
/// The longest value a select's option may have.
const MAX_VALUE_CHARS: usize = 200;
/// The longest text a text setting may hold (and its default `maxLength`).
const MAX_TEXT_SETTING: u64 = 2000;
const MAX_VERSION_CHARS: usize = 64;
const MAX_WARNINGS: usize = 20;
/// Shells, interpreters and script hosts belong to the computer, not to the
/// plugin. A command is started from the plugin's folder, so naming one
/// would only find a file of that name there; it is refused with a hint
/// instead. `python` with a version (`python3.12`, `pythonw`) counts too.
const NOT_PROGRAMS: &str = "uv uvx py pyw node deno bun perl ruby php osascript \
    sh bash zsh dash ksh csh tcsh fish env wsl cmd powershell pwsh \
    cscript wscript mshta rundll32 regsvr32 msiexec";
/// Scripts an author may expect a shell or an interpreter to run. Only
/// Windows does that by the extension (`.bat` and `.cmd` through cmd.exe);
/// elsewhere it's the file's `#!` line. The others are refused as a hint
/// toward the uv form.
const SCRIPTS: [&str; 5] = [".bat", ".cmd", ".ps1", ".sh", ".py"];
const UV_FORM: &str = r#"["uv", "run", "--script", "--locked", "<script>.py"]"#;
const TOP_KEYS: [&str; 16] = [
    "$schema",
    "manifestVersion",
    "id",
    "version",
    "name",
    "description",
    "author",
    "license",
    "repository",
    "engines",
    "platforms",
    "runtime",
    "permissions",
    "network",
    "contributes",
    "icon",
];

/// A plugin's id: a reverse domain name such as `dev.example.my-plugin`,
/// 2 to 5 parts of lowercase letters, digits and dashes (1 to 39 each, not
/// starting or ending with a dash), the first not a device name Windows
/// reserves (`nul.dev` would name the null device there). Made only of
/// those, it names the plugin's folder and its settings rows as it is.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(try_from = "String", into = "String")]
pub struct PluginId(String);

impl PluginId {
    pub fn parse(s: &str) -> PluginResult<Self> {
        let parts: Vec<&str> = s.split('.').collect();
        if (2..=5).contains(&parts.len()) && parts.iter().all(|p| is_label(p, 39)) {
            if reserved(s) {
                return Err(invalid(
                    "The plugin's id can't start with con, prn, aux, nul, com0 to com9 or lpt0 to lpt9, which Windows reserves for devices.",
                ));
            }
            return Ok(Self(s.to_owned()));
        }
        Err(invalid(
            "The plugin's id isn't valid. An id looks like dev.example.my-plugin: 2 to 5 parts of lowercase letters, digits and dashes, joined by dots.",
        ))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for PluginId {
    type Error = PluginError;

    fn try_from(s: String) -> PluginResult<Self> {
        Self::parse(&s)
    }
}

impl From<PluginId> for String {
    fn from(id: PluginId) -> Self {
        id.0
    }
}

impl fmt::Display for PluginId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What a plugin says it does. Shown before install and declared by the
/// author, not enforced: a process plugin runs with the person's user rights.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum Permission {
    /// Reads the exported clips.
    #[serde(rename = "exports:read")]
    ExportsRead,
    /// Changes or adds files beside the exported clips.
    #[serde(rename = "exports:write")]
    ExportsWrite,
    /// Reads the project: its episodes and characters.
    #[serde(rename = "project:read")]
    ProjectRead,
    /// Talks to the internet (`network.hosts` names where).
    #[serde(rename = "network")]
    Network,
}

impl Permission {
    const ALL: [Self; 4] = [Self::ExportsRead, Self::ExportsWrite, Self::ProjectRead, Self::Network];

    /// Its name in a manifest.
    pub fn name(self) -> &'static str {
        match self {
            Self::ExportsRead => "exports:read",
            Self::ExportsWrite => "exports:write",
            Self::ProjectRead => "project:read",
            Self::Network => "network",
        }
    }
}

#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    /// 1 or 2 (see [`MANIFEST_VERSION`]).
    pub manifest_version: u32,
    pub id: PluginId,
    /// A semver version (`1.2.0`).
    pub version: String,
    pub name: String,
    pub description: String,
    pub author: Author,
    pub license: Option<String>,
    /// An `https://` link.
    pub repository: Option<String>,
    pub engines: Engines,
    /// Where it runs ([`PLATFORMS`]); empty means everywhere.
    pub platforms: Vec<String>,
    pub runtime: Runtime,
    pub permissions: Vec<Permission>,
    /// Where it says it connects; empty without the network permission.
    pub network_hosts: Vec<String>,
    pub export_actions: Vec<ActionSpec>,
    pub commands: Vec<CommandSpec>,
    pub settings: Vec<SettingSpec>,
    /// Its pages (manifest version 2); whether their files are there is
    /// checked at install.
    pub pages: Vec<PageSpec>,
    /// A `.png` or `.svg` in the package (whether it's there is checked at install).
    pub icon: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Author {
    pub name: String,
    /// An `https://` link.
    pub url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Engines {
    /// The AutoScene versions it works with, as Cargo writes them (`">=1.0.0, <2"`).
    pub autoscene: String,
    pub protocol: u32,
}

/// How a plugin runs. This AutoScene installs `process` plugins and, from
/// manifest version 2, `none` ones; a `service` is reserved for a later
/// AutoScene. Also what the person agreed to at install
/// ([`crate::store::Extra`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeKind {
    /// Starts for each run and ends with it.
    Process,
    /// Also starts in service mode while one of its pages is open (protocol 2).
    Service,
    /// Runs no program: pages only.
    None,
}

#[derive(Clone, Debug, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Runtime {
    /// Its `type`.
    pub kind: RuntimeKind,
    /// The command per platform this AutoScene knows, or `*` for every
    /// other platform.
    pub command: BTreeMap<String, Vec<String>>,
    /// It uses the GPU, so it waits for the analysis to leave it free.
    pub gpu: bool,
}

/// An entry of an export's menu (and of "After every export").
#[derive(Clone, Debug, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ActionSpec {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
}

/// An entry of the command palette.
#[derive(Clone, Debug, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CommandSpec {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    /// Offered only while a project is open, and run with it.
    pub needs_project: bool,
}

/// A page a plugin adds: one of its files, which AutoScene shows full screen
/// in a sandboxed frame, from the rail (a global page) or a project's bar.
#[derive(Clone, Debug, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PageSpec {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    /// Its file: the package path of an `.html` file under `ui/`.
    pub entry: String,
    pub scope: PageScope,
    /// It has a tile in the rail (global pages only).
    pub rail: bool,
}

/// Where a page opens: on its own, or in a project, which it may then read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum PageScope {
    Global,
    Project,
}

/// A setting AutoScene shows in the plugin's settings and hands to each run.
#[derive(Clone, Debug, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SettingSpec {
    pub key: String,
    pub title: String,
    pub description: Option<String>,
    pub kind: SettingKind,
}

/// What a setting holds. A `secret` (manifest version 2) is kept in the
/// system's keychain, never as a value, and has no default.
#[derive(Clone, Debug, PartialEq, Serialize, Type)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SettingKind {
    String { default: Option<String>, placeholder: Option<String>, max_length: u32 },
    Number { default: Option<f64>, min: Option<f64>, max: Option<f64>, step: Option<f64>, integer: bool },
    Boolean { default: bool },
    Select { options: Vec<SelectOption>, default: Option<String> },
    Secret { placeholder: Option<String> },
}

#[derive(Clone, Debug, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SelectOption {
    pub value: String,
    pub label: String,
}

/// A setting's value, as it is stored and handed to a run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(untagged)]
pub enum SettingValue {
    Bool(bool),
    Number(f64),
    Text(String),
}

/// How a run starts on this computer. `program` and `script` are package
/// paths (see [`check_path`]), not names: a run starts them by their path in
/// the plugin's folder ([`Launch::entry_in`]), so the PATH never decides
/// what runs. Neither starts with a dash, which a program reads as an option.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Launch {
    /// A program in the plugin's folder (an `.exe` on Windows).
    Exe { program: String, args: Vec<String> },
    /// A Python script in the plugin's folder, through `uv run --script --locked`.
    UvScript { script: String },
}

impl Launch {
    /// The package files it starts from: the program, or the script and the
    /// lock `uv lock --script` writes beside it, which `--locked` requires.
    pub fn files(&self) -> Vec<String> {
        match self {
            Self::Exe { program, .. } => vec![program.clone()],
            Self::UvScript { script } => vec![script.clone(), format!("{script}.lock")],
        }
    }

    /// The file a run starts, inside the plugin's folder `dir`: the program,
    /// or the script uv runs (handed to uv as this path too).
    pub fn entry_in(&self, dir: &Path) -> PathBuf {
        let file = match self {
            Self::Exe { program, .. } => program,
            Self::UvScript { script } => script,
        };
        crate::package::file_in(dir, file)
    }
}

/// A manifest that can be installed, with what was skipped or changed in it.
#[derive(Clone, Debug)]
pub struct Parsed {
    pub manifest: Manifest,
    pub warnings: Vec<String>,
}

/// This computer's platform key (see [`PLATFORMS`]); `other` on any else.
pub fn current_platform() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "windows-x86_64",
        ("windows", "aarch64") => "windows-aarch64",
        ("macos", "aarch64") => "macos-aarch64",
        ("macos", "x86_64") => "macos-x86_64",
        ("linux", "x86_64") => "linux-x86_64",
        ("linux", "aarch64") => "linux-aarch64",
        _ => "other",
    }
}

impl Manifest {
    /// The command for this computer: its platform's own, else `*`. A plugin
    /// of runtime `none` has none.
    pub fn launch(&self) -> PluginResult<Launch> {
        self.launch_on(current_platform())
    }

    /// The command for the platform `here` (a key of [`PLATFORMS`]).
    fn launch_on(&self, here: &str) -> PluginResult<Launch> {
        if !self.runs_program() {
            return Err(invalid(format!("{} runs no program.", self.name)));
        }
        if !PLATFORMS.contains(&here) || (!self.platforms.is_empty() && !self.platforms.iter().any(|p| p == here)) {
            return Err(PluginError::Platform);
        }
        let commands = &self.runtime.command;
        let (key, command) =
            commands.get_key_value(here).or_else(|| commands.get_key_value("*")).ok_or(PluginError::Platform)?;
        launch_of(command, &format!("runtime.command[\"{key}\"]"), here.starts_with("windows-"))
    }

    /// How it starts on each platform of [`PLATFORMS`] it runs on (those
    /// `platforms` names, or all), or why it can't start there, as
    /// [`Manifest::launch`] would say on that computer: `asp` marks the
    /// programs to run, and checks a package for each platform. None for a
    /// plugin that runs no program.
    pub fn launches(&self) -> Vec<(&'static str, PluginResult<Launch>)> {
        if !self.runs_program() {
            return Vec::new();
        }
        let named = |p: &&str| self.platforms.is_empty() || self.platforms.iter().any(|named| named == p);
        PLATFORMS.into_iter().filter(named).map(|p| (p, self.launch_on(p))).collect()
    }

    /// Whether `app` is an AutoScene this plugin works with (see [`parse`]).
    pub fn requirement_met(&self, app: &Version) -> PluginResult<()> {
        check_requirement(&requirement(&self.engines.autoscene)?, app)
    }

    /// The keys of its secret settings.
    pub fn secret_keys(&self) -> impl Iterator<Item = &str> {
        self.settings.iter().filter(|s| matches!(s.kind, SettingKind::Secret { .. })).map(|s| s.key.as_str())
    }

    /// Whether it runs a program: all but a plugin of runtime `none`.
    pub fn runs_program(&self) -> bool {
        self.runtime.kind != RuntimeKind::None
    }

    /// Its page `id`.
    pub fn page(&self, id: &str) -> Option<&PageSpec> {
        self.pages.iter().find(|p| p.id == id)
    }
}

/// Reads a `plugin.json`. What makes the plugin unusable is an error: a
/// plugin that needs a newer AutoScene than `app` (also by
/// `engines.autoscene`) gets [`PluginError::TooNew`], one that is wrong gets
/// [`PluginError::Invalid`]. What can be left out is, with a warning. Texts
/// lose control characters and the invisible ones that turn or hide text,
/// and are cut to a length the review dialog can show. An object that names
/// a key twice is refused: JSON readers differ in which one they keep.
pub fn parse(bytes: &[u8], app: &Version) -> PluginResult<Parsed> {
    parse_for(bytes, Some(app))
}

/// Reads a `plugin.json` as [`parse`] does, for any AutoScene: what
/// `engines.autoscene` asks for must read, but isn't checked
/// ([`Manifest::requirement_met`] does that). `asp` packs and signs a plugin
/// whatever AutoScene it is made for.
pub fn parse_for_any_app(bytes: &[u8]) -> PluginResult<Parsed> {
    parse_for(bytes, None)
}

/// Reads the `plugin.json` at `path`: no more of it than [`parse`] takes and
/// a byte, so a larger one is refused without being read whole.
pub fn read_file(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn parse_for(bytes: &[u8], app: Option<&Version>) -> PluginResult<Parsed> {
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(invalid("The plugin's plugin.json is larger than 256 KB."));
    }
    // Some Windows editors start UTF-8 files with a byte order mark.
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let root: Value = serde_json::from_slice(bytes).map_err(|e| {
        invalid(format!("The plugin's plugin.json isn't valid JSON (line {}, column {}).", e.line(), e.column()))
    })?;
    if let Ok(Twice(Some(key))) = serde_json::from_slice(bytes) {
        return Err(invalid(format!("The plugin's plugin.json has \"{}\" twice in one object.", shown(&key))));
    }
    let root = object(&root, String::new())?;
    let mut w = Vec::new();

    // What says which AutoScene can read the rest comes first.
    let manifest_version = match root.get("manifestVersion").and_then(Value::as_u64) {
        Some(v) if v > u64::from(MANIFEST_VERSION) => return Err(PluginError::TooNew("manifestVersion".into())),
        Some(v @ 1..) => v as u32,
        _ => return Err(invalid(r#"The plugin's plugin.json needs "manifestVersion": 1 or 2."#)),
    };
    let engines = root.required_object("engines")?;
    let protocol = match engines.get("protocol").and_then(Value::as_u64) {
        Some(p) if p > u64::from(PROTOCOL) => return Err(PluginError::TooNew("protocol".into())),
        Some(p @ 1..) => p as u32,
        _ => return Err(invalid(r#"The plugin's engines need "protocol": 1 or 2."#)),
    };
    let runtime = root.required_object("runtime")?;
    let kind = match runtime.get("type") {
        Some(Value::String(t)) if t == "process" => RuntimeKind::Process,
        // Version 2's, which a later AutoScene runs. A version 1 manifest
        // can't name them, and this AutoScene knows that much.
        Some(Value::String(t)) if (t == "service" || t == "none") && manifest_version < 2 => {
            return Err(invalid(format!(r#"The plugin's runtime.type "{t}" needs "manifestVersion": 2."#)));
        }
        // Pages only.
        Some(Value::String(t)) if t == "none" => RuntimeKind::None,
        Some(Value::String(_)) => return Err(PluginError::TooNew("runtime".into())),
        _ => return Err(invalid(r#"The plugin's runtime needs "type": "process"."#)),
    };
    let autoscene = engines.required_text("autoscene")?;
    let req = requirement(autoscene)?;
    if let Some(app) = app {
        check_requirement(&req, app)?;
    }
    engines.warn_unknown(&["autoscene", "protocol"], &mut w);

    let id = PluginId::parse(root.required_text("id")?)?;
    let version = root.required_text("version")?;
    if version.chars().count() > MAX_VERSION_CHARS || Version::parse(version).is_err() {
        return Err(invalid("The plugin's version must be a version number such as 1.2.0."));
    }
    let name = root.required_label("name", 40, &mut w)?;
    let description = root.required_label("description", 300, &mut w)?;
    let author = {
        let a = root.required_object("author")?;
        a.warn_unknown(&["name", "url"], &mut w);
        Author { name: a.required_label("name", 60, &mut w)?, url: a.link("url", &mut w)? }
    };
    let license = root.label("license", 60, &mut w)?;
    let repository = root.link("repository", &mut w)?;

    // A platform this AutoScene doesn't know is kept: a plugin made only for
    // such platforms must not read as one made for all of them.
    let mut platforms = Vec::new();
    let mut named = root.list("platforms")?;
    // Nothing of a plugin that runs no program is made for one platform.
    if kind == RuntimeKind::None && !named.is_empty() {
        w.push("Ignored platforms: a plugin that runs no program works on every platform.".into());
        named = &[];
    }
    if named.len() > MAX_PLATFORMS {
        return Err(invalid(format!("The plugin's platforms names more than {MAX_PLATFORMS} platforms.")));
    }
    for (i, p) in named.iter().enumerate() {
        let Some(p) = p.as_str().filter(|p| is_platform(p)) else {
            return Err(invalid(format!("The plugin's platforms[{i}] isn't a platform such as windows-x86_64.")));
        };
        if !PLATFORMS.contains(&p) {
            w.push(format!("platforms names \"{p}\", which this AutoScene doesn't know."));
        }
        if !platforms.iter().any(|known| known == p) {
            platforms.push(p.to_owned());
        }
    }

    runtime.warn_unknown(&["type", "command", "gpu"], &mut w);
    let mut command = BTreeMap::new();
    let gpu = if kind == RuntimeKind::None {
        // Nothing starts, and nothing waits for the GPU.
        if let Some(key) = ["command", "gpu"].into_iter().find(|k| runtime.get(k).is_some()) {
            return Err(invalid(format!("A plugin whose runtime.type is none runs no program: remove runtime.{key}.")));
        }
        false
    } else {
        let per_platform = runtime.required_object("command")?;
        if per_platform.map.is_empty() {
            return Err(invalid("The plugin's runtime.command names no command."));
        }
        for (key, value) in per_platform.map {
            let path = format!("runtime.command[\"{}\"]", shown(key));
            // It could never run here, and a later AutoScene may allow more in it.
            if key != "*" && !PLATFORMS.contains(&key.as_str()) {
                w.push(format!("Skipped {path}, which this AutoScene doesn't know."));
                continue;
            }
            let Value::Array(parts) = value else {
                return Err(invalid(format!("The plugin's {path} must be a list.")));
            };
            let parts = parts
                .iter()
                .map(|p| p.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| invalid(format!("The plugin's {path} must list text only.")))?;
            launch_of(&parts, &path, key.starts_with("windows-"))?;
            command.insert(key.clone(), parts);
        }
        runtime.flag("gpu")?.unwrap_or(false)
    };

    let mut permissions = Vec::new();
    for p in root.list("permissions")? {
        let found = p.as_str().and_then(|p| Permission::ALL.into_iter().find(|k| k.name() == p));
        let Some(permission) = found else {
            let name = p.as_str().map_or_else(|| shown(&p.to_string()), shown);
            return Err(invalid(format!("This plugin asks for a permission this AutoScene doesn't know: {name}.")));
        };
        if !permissions.contains(&permission) {
            permissions.push(permission);
        }
    }

    let mut network_hosts = Vec::new();
    if let Some(network) = root.object("network")? {
        network.warn_unknown(&["hosts"], &mut w);
        let hosts = network.list("hosts")?;
        if hosts.len() > MAX_HOSTS {
            return Err(invalid(format!("The plugin's network.hosts names more than {MAX_HOSTS} hosts.")));
        }
        for h in hosts {
            match h.as_str() {
                Some(h) if is_host(h) => {
                    if !network_hosts.iter().any(|known| known == h) {
                        network_hosts.push(h.to_owned());
                    }
                }
                _ => {
                    let host = h.as_str().map_or_else(|| shown(&h.to_string()), shown);
                    return Err(invalid(format!(
                        "The plugin's network.hosts names {host}, which isn't a lowercase host name such as discord.com."
                    )));
                }
            }
        }
        if !network_hosts.is_empty() && !permissions.contains(&Permission::Network) {
            w.push("Ignored network.hosts: the plugin doesn't ask for the network permission.".into());
            network_hosts.clear();
        }
        // A name for this computer or its network, or an address: net.fetch
        // never connects there. A program may still, with its own sockets,
        // so version 1 named such hosts without a word, and still does.
        if manifest_version >= 2 {
            for h in network_hosts.iter().filter(|h| !net::public_name(h)) {
                w.push(format!("net.fetch can't reach {h}."));
            }
        }
    }

    let (mut export_actions, mut commands_out, mut settings) = (Vec::new(), Vec::new(), Vec::new());
    let mut pages = Vec::new();
    if let Some(c) = root.object("contributes")? {
        for kind in c.map.keys() {
            match kind.as_str() {
                "exportActions" | "commands" | "settings" => {}
                "pages" if manifest_version < 2 => {
                    w.push(r#"Skipped contributes.pages: pages need "manifestVersion": 2."#.into());
                }
                "pages" => {}
                kind => w.push(format!("Skipped contributes.{}, which this AutoScene doesn't know.", shown(kind))),
            }
        }
        let (actions, palette, specs) = (c.list("exportActions")?, c.list("commands")?, c.list("settings")?);
        if actions.len() > MAX_ACTIONS {
            return Err(invalid(format!("The plugin adds more than {MAX_ACTIONS} export actions.")));
        }
        if palette.len() > MAX_COMMANDS {
            return Err(invalid(format!("The plugin adds more than {MAX_COMMANDS} commands.")));
        }
        if specs.len() > MAX_SETTINGS {
            return Err(invalid(format!("The plugin has more than {MAX_SETTINGS} settings.")));
        }
        for (i, a) in actions.iter().enumerate() {
            let a = object(a, format!("contributes.exportActions[{i}]"))?;
            a.warn_unknown(&["id", "title", "description"], &mut w);
            export_actions.push(ActionSpec {
                id: a.contribution_id()?,
                title: a.required_label("title", 60, &mut w)?,
                description: a.label("description", 200, &mut w)?,
            });
        }
        unique(export_actions.iter().map(|a| a.id.as_str()), "contributes.exportActions", "id")?;
        for (i, p) in palette.iter().enumerate() {
            let p = object(p, format!("contributes.commands[{i}]"))?;
            p.warn_unknown(&["id", "title", "description", "needsProject"], &mut w);
            commands_out.push(CommandSpec {
                id: p.contribution_id()?,
                title: p.required_label("title", 60, &mut w)?,
                description: p.label("description", 200, &mut w)?,
                needs_project: p.flag("needsProject")?.unwrap_or(false),
            });
        }
        unique(commands_out.iter().map(|c| c.id.as_str()), "contributes.commands", "id")?;
        // A version 1 manifest's pages were skipped above, whatever they hold.
        let listed = if manifest_version >= 2 { c.list("pages")? } else { &[] };
        if listed.len() > MAX_PAGES {
            return Err(invalid(format!("The plugin adds more than {MAX_PAGES} pages.")));
        }
        for (i, p) in listed.iter().enumerate() {
            if let Some(spec) = page(&object(p, format!("contributes.pages[{i}]"))?, &mut w)? {
                pages.push(spec);
            }
        }
        unique(pages.iter().map(|p| p.id.as_str()), "contributes.pages", "id")?;
        // A secret is read with a call, which a program of protocol 1 never
        // has answered. The rule is that program's: a page asks AutoScene
        // its own way, and so does a runtime that starts none.
        let asks = kind != RuntimeKind::Process || protocol >= 2 || !pages.is_empty();
        for (i, s) in specs.iter().enumerate() {
            let s = object(s, format!("contributes.settings[{i}]"))?;
            if let Some(spec) = setting(&s, manifest_version, asks, &mut w)? {
                settings.push(spec);
            }
        }
        unique(settings.iter().map(|s| s.key.as_str()), "contributes.settings", "key")?;
        let secrets: Vec<&str> =
            settings.iter().filter(|s| matches!(s.kind, SettingKind::Secret { .. })).map(|s| s.key.as_str()).collect();
        if secrets.len() > MAX_SECRETS {
            return Err(invalid(format!("The plugin has more than {MAX_SECRETS} secret settings.")));
        }
        // Windows' keychain takes apiKey and apikey for one name.
        let mut seen = BTreeMap::new();
        for key in secrets {
            if let Some(first) = seen.insert(key.to_ascii_lowercase(), key) {
                return Err(invalid(format!(
                    r#"The plugin's secret settings "{first}" and "{key}" differ only in capitals, which a keychain doesn't tell apart."#
                )));
            }
        }
    }

    // A plugin that runs no program is its pages.
    if kind == RuntimeKind::None {
        if !export_actions.is_empty() || !commands_out.is_empty() {
            return Err(invalid("Export actions and commands need a program, and runtime.type none runs none."));
        }
        if pages.is_empty() {
            return Err(invalid("A plugin that runs no program needs a page."));
        }
    }

    let icon = match root.text("icon")? {
        Some(p) if check_path(p).is_ok() && (ends_with_any_case(p, ".png") || ends_with_any_case(p, ".svg")) => {
            Some(p.to_owned())
        }
        Some(_) => {
            w.push("Left out the icon: it must be a .png or .svg file in the plugin.".into());
            None
        }
        None => None,
    };
    root.warn_unknown(&TOP_KEYS, &mut w);

    if w.len() > MAX_WARNINGS {
        let more = w.len() - (MAX_WARNINGS - 1);
        w.truncate(MAX_WARNINGS - 1);
        w.push(format!("And {more} more warnings."));
    }
    let manifest = Manifest {
        manifest_version,
        id,
        version: version.to_owned(),
        name,
        description,
        author,
        license,
        repository,
        engines: Engines { autoscene: autoscene.to_owned(), protocol },
        platforms,
        runtime: Runtime { kind, command, gpu },
        permissions,
        network_hosts,
        export_actions,
        commands: commands_out,
        settings,
        pages,
        icon,
    };
    Ok(Parsed { manifest, warnings: w })
}

fn requirement(text: &str) -> PluginResult<VersionReq> {
    VersionReq::parse(text).map_err(|_| {
        invalid(r#"The plugin's engines.autoscene isn't a version requirement: use commas between the parts, as in ">=1.0.0, <2"."#)
    })
}

/// Refuses an `app` that doesn't meet `req`, with pre-releases cleared on
/// both sides: a beta of 1.0.3 meets `>=1.0.0`, which semver alone matches
/// only against a comparator naming a pre-release of 1.0.3 itself. A plugin
/// made for later versions needs a newer AutoScene; one made for earlier
/// versions only (`<1.0.0`) says so instead.
fn check_requirement(req: &VersionReq, app: &Version) -> PluginResult<()> {
    let app = Version { pre: Prerelease::EMPTY, build: BuildMetadata::EMPTY, ..app.clone() };
    let failed: Vec<&Comparator> = req
        .comparators
        .iter()
        .filter(|c| !Comparator { pre: Prerelease::EMPTY, ..(*c).clone() }.matches(&app))
        .collect();
    if failed.is_empty() {
        return Ok(());
    }
    // A newer AutoScene would do when each part this one fails asks for a later version.
    let later = failed.iter().all(|c| match c.op {
        Op::Less | Op::LessEq => false,
        Op::Greater | Op::GreaterEq => true,
        _ => Version::new(c.major, c.minor.unwrap_or(0), c.patch.unwrap_or(0)) > app,
    });
    if later {
        Err(PluginError::TooNew("engines.autoscene".into()))
    } else {
        Err(invalid("This plugin only works with an older AutoScene."))
    }
}

/// Checks a command and says how it starts. `path` names it in messages;
/// `windows` says it runs on Windows, where only an `.exe` starts as itself.
fn launch_of(parts: &[String], path: &str, windows: bool) -> PluginResult<Launch> {
    let refuse = |why: String| Err(invalid(format!("The plugin's {path} {why}")));
    let Some(program) = parts.first() else { return refuse("is empty.".into()) };
    if parts.len() > MAX_ARGS {
        return refuse(format!("has more than {MAX_ARGS} parts."));
    }
    if parts.iter().any(|p| p.chars().count() > MAX_ARG_CHARS) {
        return refuse(format!("has a part longer than {MAX_ARG_CHARS} characters."));
    }
    // The install review may show the command as it is.
    if parts.iter().any(|p| p.chars().any(|c| c.is_control() || invisible(c))) {
        return refuse("has a part with a control character or an invisible one in it.".into());
    }
    let dash = "names a file that starts with a dash, which a program would read as an option.";
    let lower = program.to_ascii_lowercase();
    let name = lower.strip_suffix(".exe").unwrap_or(&lower);
    if name == "uv" {
        return match parts {
            [uv, run, script, locked, file]
                if uv == "uv"
                    && run == "run"
                    && script == "--script"
                    && locked == "--locked"
                    && file.ends_with(".py") =>
            {
                if file.starts_with('-') {
                    return refuse(dash.into());
                }
                check_path(file)?;
                check_path(&format!("{file}.lock"))?;
                Ok(Launch::UvScript { script: file.clone() })
            }
            _ => refuse(format!("must run uv as {UV_FORM}, so the script's lock decides what it installs.")),
        };
    }
    if computer_program(name) {
        return refuse(format!(
            "starts {}, which belongs to the computer: start a program in the plugin's folder, or a Python script as {UV_FORM}.",
            shown(program)
        ));
    }
    check_path(program)?;
    if program.starts_with('-') {
        return refuse(dash.into());
    }
    if SCRIPTS.iter().any(|e| name.ends_with(e)) {
        return refuse(format!(
            "starts a script, which a shell or an interpreter would run: start a program, or a Python script as {UV_FORM}."
        ));
    }
    if windows && !ends_with_any_case(program, ".exe") {
        return refuse("runs on Windows, so its program must be an .exe file.".into());
    }
    Ok(Launch::Exe { program: program.clone(), args: parts[1..].to_vec() })
}

/// Whether `name` (lowercase, without `.exe`) is one of [`NOT_PROGRAMS`]
/// or a Python with a version (`python3.12`, `pythonw`).
fn computer_program(name: &str) -> bool {
    let python = name
        .strip_prefix("python")
        .is_some_and(|v| v.strip_suffix('w').unwrap_or(v).bytes().all(|b| b.is_ascii_digit() || b == b'.'));
    python || NOT_PROGRAMS.split_whitespace().any(|p| p == name)
}

/// One entry of `contributes.settings` of a manifest of `version`, or
/// `None` for a type this AutoScene skips. A secret is skipped too unless
/// the plugin `asks` for one: nothing of it could ever read it.
fn setting(o: &Obj, version: u32, asks: bool, w: &mut Vec<String>) -> PluginResult<Option<SettingSpec>> {
    let key = o.required_text("key")?;
    let mut chars = key.chars();
    let first = chars.next().is_some_and(|c| c.is_ascii_lowercase());
    if !first || key.len() > 40 || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(invalid(format!(
            "The plugin's {} must start with a lowercase letter and use only letters, digits and _ (at most 40).",
            o.at("key")
        )));
    }
    let kind = o.required_text("type")?;
    let (kind, keys): (_, &[&str]) = match kind {
        "string" => {
            let max_length = match o.whole("maxLength")? {
                None => MAX_TEXT_SETTING,
                Some(n @ 1..=MAX_TEXT_SETTING) => n,
                Some(_) => return Err(invalid(format!("The plugin's {} must be 1 to 2000.", o.at("maxLength")))),
            };
            let default = o.text("default")?.map(str::to_owned);
            if default.as_ref().is_some_and(|d| d.chars().count() as u64 > max_length) {
                return Err(invalid(format!("The plugin's {} is longer than its maxLength.", o.at("default"))));
            }
            if default.as_ref().is_some_and(|d| d.contains('\0')) {
                return Err(invalid(format!("The plugin's {} has a NUL character in it.", o.at("default"))));
            }
            let placeholder = o.label("placeholder", 200, w)?;
            (
                SettingKind::String { default, placeholder, max_length: max_length as u32 },
                &["default", "placeholder", "maxLength"],
            )
        }
        "number" => {
            let (default, min, max, step) =
                (o.number("default")?, o.number("min")?, o.number("max")?, o.number("step")?);
            let integer = o.flag("integer")?.unwrap_or(false);
            if let (Some(min), Some(max)) = (min, max)
                && min > max
            {
                return Err(invalid(format!("The plugin's {} is above its max.", o.at("min"))));
            }
            if step.is_some_and(|s| s <= 0.0) {
                return Err(invalid(format!("The plugin's {} must be above 0.", o.at("step"))));
            }
            if let Some(d) = default {
                if min.is_some_and(|min| d < min) || max.is_some_and(|max| d > max) {
                    return Err(invalid(format!("The plugin's {} lies outside its min and max.", o.at("default"))));
                }
                if integer && d.fract() != 0.0 {
                    return Err(invalid(format!("The plugin's {} must be a whole number.", o.at("default"))));
                }
            }
            // A slider stands somewhere: without a default it starts at its
            // min, which the plugin then gets too, as it is shown.
            let default = default.or_else(|| {
                let (min, max) = (min?, max?);
                let start = if integer { min.ceil() } else { min };
                (max > min && start <= max).then_some(start)
            });
            (SettingKind::Number { default, min, max, step, integer }, &["default", "min", "max", "step", "integer"])
        }
        "boolean" => (SettingKind::Boolean { default: o.flag("default")?.unwrap_or(false) }, &["default"]),
        "select" => {
            let list = o.list("options")?;
            if list.is_empty() || list.len() > MAX_OPTIONS {
                return Err(invalid(format!("The plugin's {} must list 1 to {MAX_OPTIONS} choices.", o.at("options"))));
            }
            let mut options = Vec::new();
            for (i, option) in list.iter().enumerate() {
                let option = object(option, format!("{}[{i}]", o.at("options")))?;
                option.warn_unknown(&["value", "label"], w);
                let value = option.required_text("value")?;
                if value.is_empty() {
                    return Err(invalid(format!("The plugin's {} is empty.", option.at("value"))));
                }
                if value.chars().count() > MAX_VALUE_CHARS || value.chars().any(char::is_control) {
                    return Err(invalid(format!(
                        "The plugin's {} must be at most {MAX_VALUE_CHARS} characters, without control characters.",
                        option.at("value")
                    )));
                }
                options.push(SelectOption { value: value.to_owned(), label: option.required_label("label", 60, w)? });
            }
            unique(options.iter().map(|option| option.value.as_str()), &o.at("options"), "value")?;
            let default = o.text("default")?.map(str::to_owned);
            if default.as_ref().is_some_and(|d| !options.iter().any(|option| &option.value == d)) {
                return Err(invalid(format!("The plugin's {} isn't one of its options.", o.at("default"))));
            }
            (SettingKind::Select { options, default }, &["options", "default"])
        }
        "secret" if version < 2 => {
            w.push(format!(r#"Skipped the setting "{key}": secret settings need "manifestVersion": 2."#));
            return Ok(None);
        }
        "secret" if !asks => {
            w.push(format!(r#"Skipped the setting "{key}": secret settings need "engines": {{"protocol": 2}}."#));
            return Ok(None);
        }
        "secret" => {
            // A default would sit in the plugin's files, for anyone to read.
            if o.get("default").is_some() {
                return Err(invalid("A secret setting has no default."));
            }
            // A null default counts as left out, as for any key.
            (SettingKind::Secret { placeholder: o.label("placeholder", 200, w)? }, &["placeholder", "default"])
        }
        kind => {
            w.push(format!(
                "Skipped the setting \"{key}\" of type \"{}\", which this AutoScene doesn't know.",
                shown(kind)
            ));
            return Ok(None);
        }
    };
    o.warn_unknown(&[&["key", "type", "title", "description"][..], keys].concat(), w);
    Ok(Some(SettingSpec {
        key: key.to_owned(),
        title: o.required_label("title", 60, w)?,
        description: o.label("description", 200, w)?,
        kind,
    }))
}

/// One entry of `contributes.pages` (manifest version 2), or `None` for a
/// scope this AutoScene doesn't know. Its file is checked against the
/// package at install ([`crate::package::check_pages`]).
fn page(o: &Obj, w: &mut Vec<String>) -> PluginResult<Option<PageSpec>> {
    let id = o.contribution_id()?;
    let scope = match o.text("scope")? {
        None | Some("global") => PageScope::Global,
        Some("project") => PageScope::Project,
        Some(scope) => {
            w.push(format!(r#"Skipped the page "{id}": scope "{}" needs a newer AutoScene."#, shown(scope)));
            return Ok(None);
        }
    };
    o.warn_unknown(&["id", "title", "description", "entry", "scope", "rail"], w);
    let entry = o.required_text("entry")?;
    check_path(entry)?;
    if !entry.starts_with("ui/") || !entry.ends_with(".html") {
        return Err(invalid(format!(
            "The plugin's {} must be an .html file under ui/, such as ui/index.html.",
            o.at("entry")
        )));
    }
    let rail = o.flag("rail")?.unwrap_or(false);
    if rail && scope == PageScope::Project {
        w.push(format!(r#"The page "{id}" is a project page: rail is for global pages."#));
    }
    Ok(Some(PageSpec {
        title: o.required_label("title", 40, w)?,
        description: o.label("description", 200, w)?,
        entry: entry.to_owned(),
        rail: rail && scope == PageScope::Global,
        id,
        scope,
    }))
}

/// Refuses a list in which two entries share `field`.
fn unique<'a>(values: impl Iterator<Item = &'a str>, path: &str, field: &str) -> PluginResult<()> {
    let mut seen = HashSet::new();
    for v in values {
        if !seen.insert(v) {
            return Err(invalid(format!("The plugin's {path} has the {field} \"{}\" twice.", shown(v))));
        }
    }
    Ok(())
}

/// `[a-z0-9][a-z0-9-]*`, at most `max` long.
fn is_name(s: &str, max: usize) -> bool {
    let b = s.as_bytes();
    let part = |c: &u8| c.is_ascii_lowercase() || c.is_ascii_digit();
    !b.is_empty() && b.len() <= max && part(&b[0]) && b.iter().all(|c| part(c) || *c == b'-')
}

/// A part of an id or a host name: a name that doesn't end with a dash.
fn is_label(s: &str, max: usize) -> bool {
    is_name(s, max) && !s.ends_with('-')
}

/// A host name as the network permission names it: lowercase labels of 1
/// to 63 letters, digits and dashes, joined by dots; no port, no scheme.
fn is_host(h: &str) -> bool {
    h.len() <= 253 && h.split('.').all(|label| is_label(label, 63))
}

/// A platform key such as `windows-x86_64`, also one a later AutoScene adds.
fn is_platform(p: &str) -> bool {
    !p.is_empty()
        && p.len() <= 40
        && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_".contains(&b))
}

/// Text for people: without control characters or the invisible ones (see
/// [`invisible`]), whitespace collapsed to single spaces. Joiners stay, as
/// emoji and some scripts need them, but never make a text on their own.
pub(crate) fn clean(s: &str) -> String {
    let s: String = s.chars().filter(|c| !invisible(*c)).map(|c| if c.is_control() { ' ' } else { c }).collect();
    if s.chars().all(|c| c.is_whitespace() || matches!(c, '\u{200C}' | '\u{200D}')) {
        return String::new();
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Characters that show nothing, or change how the text around them shows:
/// format characters (soft hyphen, zero-width space, bidi marks, overrides
/// and isolates, invisible operators, tags), variation selectors, and
/// letters that draw as blank (Hangul fillers, the blank Braille pattern).
fn invisible(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{034F}'
            | '\u{061C}'
            | '\u{115F}'..='\u{1160}'
            | '\u{17B4}'..='\u{17B5}'
            | '\u{180B}'..='\u{180F}'
            | '\u{200B}'
            | '\u{200E}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{2800}'
            | '\u{3164}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{FEFF}'
            | '\u{FFA0}'
            | '\u{FFF0}'..='\u{FFFB}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0000}'..='\u{E0FFF}'
    )
}

/// `s` cut to `max` characters, the last one an ellipsis.
pub(crate) fn cut(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.truncate(out.trim_end().len());
    out.push('…');
    out
}

/// Whether `s` ends in `suffix`, in any case.
fn ends_with_any_case(s: &str, suffix: &str) -> bool {
    s.len() >= suffix.len() && s.as_bytes()[s.len() - suffix.len()..].eq_ignore_ascii_case(suffix.as_bytes())
}

/// Text from a plugin as a message may quote it: clean and short.
pub(crate) fn shown(s: &str) -> String {
    cut(&clean(s), 40)
}

pub(crate) fn invalid(message: impl Into<String>) -> PluginError {
    PluginError::Invalid(message.into())
}

/// The first key an object of a JSON text names twice, at any depth.
/// serde_json keeps the last value; a person or another reader of the file
/// may go by the first and see another id or other permissions.
pub(crate) struct Twice(pub(crate) Option<String>);

impl<'de> Deserialize<'de> for Twice {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(TwiceVisitor)
    }
}

struct TwiceVisitor;

impl<'de> Visitor<'de> for TwiceVisitor {
    type Value = Twice;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON value")
    }

    fn visit_bool<E: de::Error>(self, _: bool) -> Result<Twice, E> {
        Ok(Twice(None))
    }

    fn visit_i64<E: de::Error>(self, _: i64) -> Result<Twice, E> {
        Ok(Twice(None))
    }

    fn visit_u64<E: de::Error>(self, _: u64) -> Result<Twice, E> {
        Ok(Twice(None))
    }

    fn visit_f64<E: de::Error>(self, _: f64) -> Result<Twice, E> {
        Ok(Twice(None))
    }

    fn visit_str<E: de::Error>(self, _: &str) -> Result<Twice, E> {
        Ok(Twice(None))
    }

    fn visit_unit<E: de::Error>(self) -> Result<Twice, E> {
        Ok(Twice(None))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Twice, A::Error> {
        let mut first = None;
        while let Some(Twice(inner)) = seq.next_element()? {
            first = first.or(inner);
        }
        Ok(Twice(first))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Twice, A::Error> {
        let (mut keys, mut first) = (HashSet::new(), None);
        while let Some(key) = map.next_key::<String>()? {
            let Twice(inner) = map.next_value()?;
            first = first.or(inner);
            if keys.contains(&key) {
                first = first.or(Some(key));
            } else {
                keys.insert(key);
            }
        }
        Ok(Twice(first))
    }
}

/// An object of the manifest, with where it sits for messages.
struct Obj<'a> {
    map: &'a Map<String, Value>,
    path: String,
}

fn object(v: &Value, path: String) -> PluginResult<Obj<'_>> {
    match v {
        Value::Object(map) => Ok(Obj { map, path }),
        _ if path.is_empty() => Err(invalid("The plugin's plugin.json must hold an object.")),
        _ => Err(invalid(format!("The plugin's {path} must be an object."))),
    }
}

impl<'a> Obj<'a> {
    /// The path of `key` in this object.
    fn at(&self, key: &str) -> String {
        if self.path.is_empty() { key.to_owned() } else { format!("{}.{key}", self.path) }
    }

    /// `key`'s value; `null` counts as left out.
    fn get(&self, key: &str) -> Option<&'a Value> {
        self.map.get(key).filter(|v| !v.is_null())
    }

    fn missing(&self, key: &str) -> PluginError {
        invalid(format!("The plugin's {} is missing.", self.at(key)))
    }

    fn object(&self, key: &str) -> PluginResult<Option<Obj<'a>>> {
        self.get(key).map(|v| object(v, self.at(key))).transpose()
    }

    fn required_object(&self, key: &str) -> PluginResult<Obj<'a>> {
        self.object(key)?.ok_or_else(|| self.missing(key))
    }

    fn text(&self, key: &str) -> PluginResult<Option<&'a str>> {
        match self.get(key) {
            None => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.as_str())),
            Some(_) => Err(invalid(format!("The plugin's {} must be text.", self.at(key)))),
        }
    }

    fn required_text(&self, key: &str) -> PluginResult<&'a str> {
        self.text(key)?.ok_or_else(|| self.missing(key))
    }

    /// Text for people (see [`clean`]), cut to `max` characters with a warning.
    fn label(&self, key: &str, max: usize, w: &mut Vec<String>) -> PluginResult<Option<String>> {
        let Some(text) = self.text(key)? else { return Ok(None) };
        let text = clean(text);
        if text.chars().count() > max {
            w.push(format!("Cut {} to {max} characters.", self.at(key)));
            return Ok(Some(cut(&text, max)));
        }
        Ok(Some(text))
    }

    fn required_label(&self, key: &str, max: usize, w: &mut Vec<String>) -> PluginResult<String> {
        match self.label(key, max, w)? {
            Some(text) if !text.is_empty() => Ok(text),
            Some(_) => Err(invalid(format!("The plugin's {} is empty.", self.at(key)))),
            None => Err(self.missing(key)),
        }
    }

    /// An `https://` link whose address part is a plain host, with a port
    /// at most: no `user@` before it, which would make
    /// `https://autoscene.app@evil.example` read as the first and lead to
    /// the second. Anything else is left out with a warning.
    fn link(&self, key: &str, w: &mut Vec<String>) -> PluginResult<Option<String>> {
        let Some(url) = self.text(key)? else { return Ok(None) };
        let plain = url.len() <= 2000
            && url.is_ascii()
            && !url.bytes().any(|b| b.is_ascii_whitespace() || b.is_ascii_control() || b == b'\\');
        let address = url.strip_prefix("https://").and_then(|rest| rest.split(['/', '?', '#']).next());
        let (host, port) = address.map_or(("", ""), |a| a.split_once(':').unwrap_or((a, "443")));
        let port_ok = port.bytes().all(|b| b.is_ascii_digit()) && port.parse::<u16>().is_ok_and(|p| p > 0);
        if plain && port_ok && is_host(&host.to_ascii_lowercase()) {
            return Ok(Some(url.to_owned()));
        }
        w.push(format!("Left out {}: a link must be a plain https:// address.", self.at(key)));
        Ok(None)
    }

    fn flag(&self, key: &str) -> PluginResult<Option<bool>> {
        match self.get(key) {
            None => Ok(None),
            Some(Value::Bool(b)) => Ok(Some(*b)),
            Some(_) => Err(invalid(format!("The plugin's {} must be true or false.", self.at(key)))),
        }
    }

    fn number(&self, key: &str) -> PluginResult<Option<f64>> {
        match self.get(key) {
            None => Ok(None),
            Some(v) => match v.as_f64() {
                Some(n) if n.is_finite() => Ok(Some(n)),
                _ => Err(invalid(format!("The plugin's {} must be a number.", self.at(key)))),
            },
        }
    }

    fn whole(&self, key: &str) -> PluginResult<Option<u64>> {
        match self.get(key) {
            None => Ok(None),
            Some(v) => v
                .as_u64()
                .map(Some)
                .ok_or_else(|| invalid(format!("The plugin's {} must be a whole number.", self.at(key)))),
        }
    }

    /// The list at `key`; left out, it is empty.
    fn list(&self, key: &str) -> PluginResult<&'a [Value]> {
        match self.get(key) {
            None => Ok(&[]),
            Some(Value::Array(list)) => Ok(list),
            Some(_) => Err(invalid(format!("The plugin's {} must be a list.", self.at(key)))),
        }
    }

    /// A contribution's id: `[a-z0-9][a-z0-9-]*`, at most 32 long.
    fn contribution_id(&self) -> PluginResult<String> {
        let id = self.required_text("id")?;
        if !is_name(id, 32) {
            return Err(invalid(format!(
                "The plugin's {} must use only lowercase letters, digits and dashes (at most 32).",
                self.at("id")
            )));
        }
        Ok(id.to_owned())
    }

    /// One warning for each key outside `known`.
    fn warn_unknown(&self, known: &[&str], w: &mut Vec<String>) {
        for key in self.map.keys().filter(|k| !known.contains(&k.as_str())) {
            w.push(format!("Ignored {}, which this AutoScene doesn't know.", shown(&self.at(key))));
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::package::{FilesLock, LOCK_VERSION, LockedFile, check_pages, sha256_hex};

    fn app() -> Version {
        Version::parse("1.0.3-BETA").unwrap()
    }

    fn plugin() -> Value {
        json!({
            "manifestVersion": 1,
            "id": "dev.autoscene.hello-export",
            "version": "1.2.0",
            "name": "Hello Export",
            "description": "Says hello after an export.",
            "author": {"name": "AutoScene", "url": "https://autoscene.app"},
            "engines": {"autoscene": ">=1.0.0, <2", "protocol": 1},
            "runtime": {"type": "process", "command": {"*": ["uv", "run", "--script", "--locked", "main.py"]}},
            "permissions": ["exports:read"],
            "contributes": {
                "exportActions": [{"id": "hello", "title": "Say hello"}],
                "commands": [{"id": "hello", "title": "Say hello", "needsProject": true}],
                "settings": [{"key": "greeting", "type": "string", "title": "Greeting", "default": "Hi"}]
            }
        })
    }

    fn read(v: &Value) -> PluginResult<Parsed> {
        parse(&serde_json::to_vec(v).unwrap(), &app())
    }

    fn with(edit: impl FnOnce(&mut Value)) -> PluginResult<Parsed> {
        let mut v = plugin();
        edit(&mut v);
        read(&v)
    }

    fn message<T: fmt::Debug>(r: PluginResult<T>) -> String {
        match r {
            Err(PluginError::Invalid(m)) => m,
            r => panic!("expected Invalid, got {r:?}"),
        }
    }

    fn too_new<T: fmt::Debug>(r: PluginResult<T>) -> String {
        match r {
            Err(PluginError::TooNew(field)) => field,
            r => panic!("expected TooNew, got {r:?}"),
        }
    }

    fn command(parts: Value) -> PluginResult<Launch> {
        with(|v| v["runtime"]["command"] = json!({ "*": parts })).and_then(|p| p.manifest.launch())
    }

    #[test]
    fn a_plugin_reads_with_its_contributions() {
        let p = read(&plugin()).unwrap();
        assert_eq!(p.warnings, Vec::<String>::new());
        let m = p.manifest;
        assert_eq!(m.id.as_str(), "dev.autoscene.hello-export");
        assert_eq!(m.author, Author { name: "AutoScene".into(), url: Some("https://autoscene.app".into()) });
        assert_eq!(m.permissions, [Permission::ExportsRead]);
        assert_eq!(m.export_actions[0].id, "hello");
        assert!(m.commands[0].needs_project);
        let SettingKind::String { default, max_length, .. } = &m.settings[0].kind else { panic!() };
        assert_eq!((default.as_deref(), *max_length), (Some("Hi"), 2000));
        assert_eq!(m.launch().unwrap(), Launch::UvScript { script: "main.py".into() });
    }

    #[test]
    fn ids_cannot_leave_their_folder() {
        let long = "a".repeat(40);
        let longest = "b".repeat(39);
        for id in
            ["../x", "a/b.c", "A.b", "a", ".a.b", "a..b", "a.b.", "a-.b", "-a.b", "a.b.c.d.e.f", "a b.c", r"a\b.c"]
        {
            assert!(PluginId::parse(id).is_err(), "{id}");
        }
        assert!(PluginId::parse(&format!("dev.{long}a")).is_err(), "a 41-character label");
        // Windows 10 takes a folder named so for a device.
        for id in ["con.example", "nul.dev", "com1.x", "lpt9.a", "aux.aux", "prn.x.y"] {
            assert!(message(PluginId::parse(id)).contains("Windows reserves"), "{id}");
        }
        for id in
            ["a.b", "dev.autoscene.hello-export", "1.2", "a.b.c.d.e", &format!("dev.{longest}"), "dev.nul", "com10.x"]
        {
            assert_eq!(PluginId::parse(id).unwrap().as_str(), id);
        }
        // Wherever an id comes from: the manifest, or a stored row.
        assert!(message(with(|v| v["id"] = json!("../evil"))).contains("dev.example.my-plugin"));
        assert!(serde_json::from_str::<PluginId>(r#""../x""#).is_err());
        let id: PluginId = serde_json::from_str(r#""dev.a.b""#).unwrap();
        assert_eq!(serde_json::to_string(&id).unwrap(), r#""dev.a.b""#);
    }

    #[test]
    fn a_newer_manifest_needs_a_newer_autoscene() {
        // Read before anything else, so a later field never reads as a mistake.
        assert_eq!(too_new(read(&json!({"manifestVersion": 3, "id": "?"}))), "manifestVersion");
        for version in [1, 2] {
            let wasm = with(|v| {
                v["manifestVersion"] = json!(version);
                v["runtime"] = json!({"type": "wasm", "component": "x.wasm"});
            });
            assert_eq!(too_new(wasm), "runtime");
        }
        // A service waits for a later AutoScene. In version 1, version 2's
        // runtime types are a mistake this one can name.
        let service = with(|v| {
            v["manifestVersion"] = json!(2);
            v["runtime"]["type"] = json!("service");
        });
        assert_eq!(too_new(service), "runtime");
        for kind in ["service", "none"] {
            let v1 = message(with(|v| v["runtime"]["type"] = json!(kind)));
            assert_eq!(v1, format!(r#"The plugin's runtime.type "{kind}" needs "manifestVersion": 2."#));
        }
        let later = with(|v| {
            v["engines"]["autoscene"] = json!(">=1.1.0");
            v["permissions"] = json!(["secrets"]);
        });
        assert_eq!(too_new(later), "engines.autoscene");
        for version in [json!(0), json!("1"), json!(1.5), json!(null)] {
            assert!(message(with(|v| v["manifestVersion"] = version.clone())).contains("manifestVersion"));
        }
        assert_eq!(PluginError::TooNew("runtime".into()).to_string(), "This plugin needs a newer AutoScene.");
    }

    #[test]
    fn a_newer_protocol_needs_a_newer_autoscene() {
        assert_eq!(too_new(with(|v| v["engines"]["protocol"] = json!(PROTOCOL + 1))), "protocol");
        // It names the protocols this AutoScene speaks.
        let speaks = r#"The plugin's engines need "protocol": 1 or 2."#;
        assert_eq!(PROTOCOL, 2, "{speaks}");
        assert_eq!(message(with(|v| v["engines"]["protocol"] = json!(0))), speaks);
        let left_out = with(|v| {
            v["engines"].as_object_mut().unwrap().remove("protocol");
        });
        assert_eq!(message(left_out), speaks);
        // A plugin keeps the protocol it declares, also an older one.
        assert_eq!(read(&plugin()).unwrap().manifest.engines.protocol, 1);
        assert_eq!(with(|v| v["engines"]["protocol"] = json!(2)).unwrap().manifest.engines.protocol, 2);
    }

    #[test]
    fn unknown_permissions_are_refused() {
        let m = message(with(|v| v["permissions"] = json!(["exports:read", "files:delete"])));
        assert_eq!(m, "This plugin asks for a permission this AutoScene doesn't know: files:delete.");
        assert!(message(with(|v| v["permissions"] = json!([7]))).ends_with(": 7."));
        let p = with(|v| v["permissions"] = json!(["network", "project:read", "exports:write", "network"])).unwrap();
        assert_eq!(p.manifest.permissions, [Permission::Network, Permission::ProjectRead, Permission::ExportsWrite]);
        assert_eq!(serde_json::to_string(&Permission::ExportsWrite).unwrap(), r#""exports:write""#);

        // Hosts are named with the permission, and only then kept.
        let p = with(|v| v["network"] = json!({"hosts": ["discord.com"]})).unwrap();
        assert_eq!(p.manifest.network_hosts, Vec::<String>::new());
        assert!(p.warnings[0].contains("network permission"));
        let p = with(|v| {
            v["permissions"] = json!(["network"]);
            v["network"] = json!({"hosts": ["discord.com", "api.example.org"]});
        })
        .unwrap();
        assert_eq!(p.manifest.network_hosts, ["discord.com", "api.example.org"]);
        let label = "a".repeat(64);
        for host in [
            "Discord.com",
            "https://discord.com",
            "discord.com:443",
            "",
            ".com",
            "a.",
            "a..b",
            "-a.com",
            "a-.com",
            "*.example.org",
            &format!("{label}.com"),
        ] {
            assert!(with(|v| v["network"] = json!({ "hosts": [host] })).is_err(), "{host}");
        }
        let hosts = json!(["localhost", "127.0.0.1", "xn--bcher-kva.example", format!("{}.com", &label[1..])]);
        assert!(with(|v| v["network"] = json!({ "hosts": hosts })).is_ok());
        let many: Vec<String> = (0..17).map(|i| format!("h{i}.example.org")).collect();
        assert!(with(|v| v["network"] = json!({ "hosts": many })).is_err());
    }

    #[test]
    fn unknown_contributions_and_setting_types_are_skipped_with_a_warning() {
        let p = with(|v| {
            v["homepage"] = json!("https://example.org");
            v["contributes"]["themes"] = json!([{"id": "dark"}]);
            v["contributes"]["commands"][0]["run"] = json!({"action": "hello"});
            v["contributes"]["settings"] = json!([
                {"key": "tint", "type": "color", "title": "Tint"},
                {"key": "level", "type": "number", "title": "Level", "default": 0.4, "min": 0, "max": 1},
                {"key": "mode", "type": "select", "title": "Mode", "default": "b",
                 "options": [{"value": "a", "label": "A"}, {"value": "b", "label": "B"}]},
            ]);
        })
        .unwrap();
        let keys: Vec<&str> = p.manifest.settings.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys, ["level", "mode"]);
        assert_eq!(p.manifest.commands.len(), 1);
        for said in [
            "Ignored homepage, which this AutoScene doesn't know.",
            "Skipped contributes.themes, which this AutoScene doesn't know.",
            "Ignored contributes.commands[0].run, which this AutoScene doesn't know.",
            r#"Skipped the setting "tint" of type "color", which this AutoScene doesn't know."#,
        ] {
            assert!(p.warnings.iter().any(|w| w == said), "{said} in {:?}", p.warnings);
        }
        assert_eq!(p.warnings.len(), 4);
        let many = with(|v| (0..30).for_each(|i| v[format!("extra{i}")] = json!(i))).unwrap();
        assert_eq!(many.warnings.len(), 20);
        assert_eq!(many.warnings[19], "And 11 more warnings.");

        // What's known is still checked.
        let bad = [
            json!({"key": "level", "type": "number", "title": "Level", "default": 2, "min": 0, "max": 1}),
            json!({"key": "n", "type": "number", "title": "N", "default": 0.5, "integer": true}),
            json!({"key": "mode", "type": "select", "title": "Mode", "options": []}),
            json!({"key": "mode", "type": "select", "title": "Mode", "default": "c", "options": [{"value": "a", "label": "A"}]}),
            json!({"key": "Mode", "type": "boolean", "title": "Mode"}),
            json!({"key": "s", "type": "string", "title": "S", "maxLength": 2001}),
        ];
        for setting in bad {
            assert!(with(|v| v["contributes"]["settings"] = json!([setting.clone()])).is_err(), "{setting}");
        }
        let twice = json!([{"id": "a", "title": "A"}, {"id": "a", "title": "B"}]);
        assert!(message(with(|v| v["contributes"]["exportActions"] = twice)).contains(r#"the id "a" twice"#));
    }

    #[test]
    fn a_slider_without_a_default_starts_at_its_min() {
        let default = |setting: Value| {
            let p = with(|v| v["contributes"]["settings"] = json!([setting])).unwrap();
            let SettingKind::Number { default, .. } = p.manifest.settings[0].kind else { panic!() };
            default
        };
        let n = |extra: Value| {
            let mut setting = json!({"key": "n", "type": "number", "title": "N"});
            setting.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            setting
        };
        assert_eq!(default(n(json!({"min": 2, "max": 10}))), Some(2.0));
        assert_eq!(default(n(json!({"min": 0.5, "max": 3, "integer": true}))), Some(1.0));
        assert_eq!(default(n(json!({"min": 2, "max": 10, "default": 5}))), Some(5.0));
        // Not a slider: a number field may be left empty.
        assert_eq!(default(n(json!({"min": 2}))), None);
        assert_eq!(default(n(json!({"min": 2, "max": 2}))), None);
        assert_eq!(default(n(json!({"min": 0.2, "max": 0.7, "integer": true}))), None);
    }

    /// A secret setting `key` and a plain one, with a page, in a manifest
    /// of `version` (of protocol 2).
    fn with_secret_and_pages(version: u32) -> PluginResult<Parsed> {
        with(|v| {
            v["manifestVersion"] = json!(version);
            v["engines"]["protocol"] = json!(2);
            v["contributes"]["settings"] = json!([
                {"key": "webhook", "type": "secret", "title": "Discord webhook URL", "placeholder": "https://…"},
                {"key": "loud", "type": "boolean", "title": "Loud"},
            ]);
            v["contributes"]["pages"] = json!([{"id": "main", "title": "Main", "entry": "ui/index.html"}]);
        })
    }

    #[test]
    fn pages_and_secrets_in_a_version_1_manifest_are_skipped_as_before() {
        let p = with_secret_and_pages(1).unwrap();
        assert_eq!(p.manifest.settings.len(), 1);
        assert_eq!(p.manifest.settings[0].kind, SettingKind::Boolean { default: false });
        assert_eq!(p.manifest.secret_keys().count(), 0);
        assert_eq!(p.manifest.export_actions.len(), 1);
        // The warnings name the version that has them.
        assert_eq!(
            p.warnings,
            [
                r#"Skipped contributes.pages: pages need "manifestVersion": 2."#,
                r#"Skipped the setting "webhook": secret settings need "manifestVersion": 2."#,
            ]
        );

        // Version 2 reads its secrets and its pages.
        let p = with_secret_and_pages(2).unwrap();
        assert_eq!(p.warnings, Vec::<String>::new());
        assert_eq!(p.manifest.pages.iter().map(|p| p.entry.as_str()).collect::<Vec<_>>(), ["ui/index.html"]);
        let secret = SettingKind::Secret { placeholder: Some("https://…".into()) };
        assert_eq!((&p.manifest.settings[0].key, &p.manifest.settings[0].kind), (&"webhook".to_owned(), &secret));
        assert_eq!(p.manifest.secret_keys().collect::<Vec<_>>(), ["webhook"]);
        assert_eq!(p.manifest.manifest_version, 2);
        assert_eq!(
            serde_json::to_value(&secret).unwrap(),
            json!({"type": "secret", "placeholder": "https://…"}),
            "as the settings page reads it"
        );
    }

    /// A version 2 manifest of a plugin that runs no program, with `pages`.
    fn page_only(pages: Value) -> PluginResult<Parsed> {
        with(|v| {
            v["manifestVersion"] = json!(2);
            v["runtime"] = json!({"type": "none"});
            v["contributes"] = json!({ "pages": pages });
        })
    }

    #[test]
    fn pages_need_manifest_version_2() {
        let main = json!([{"id": "main", "title": "Main", "entry": "ui/index.html"}]);
        // Version 1 skips them, whatever they hold, as AutoScene 1.0 does.
        for pages in [main.clone(), json!("everything")] {
            let p = with(|v| v["contributes"]["pages"] = pages.clone()).unwrap();
            assert_eq!(p.manifest.pages, []);
            assert_eq!(p.warnings, [r#"Skipped contributes.pages: pages need "manifestVersion": 2."#]);
        }
        let v1 = with(|v| {
            v["runtime"] = json!({"type": "none"});
            v["contributes"] = json!({ "pages": main.clone() });
        });
        assert_eq!(message(v1), r#"The plugin's runtime.type "none" needs "manifestVersion": 2."#);

        // Version 2 reads them, beside a program too.
        let p = with(|v| {
            v["manifestVersion"] = json!(2);
            v["contributes"]["pages"] = main.clone();
        })
        .unwrap();
        assert_eq!(p.warnings, Vec::<String>::new());
        let page = PageSpec {
            id: "main".into(),
            title: "Main".into(),
            description: None,
            entry: "ui/index.html".into(),
            scope: PageScope::Global,
            rail: false,
        };
        assert_eq!(p.manifest.pages, std::slice::from_ref(&page));
        assert_eq!((p.manifest.page("main"), p.manifest.page("other")), (Some(&page), None));
        assert_eq!(
            serde_json::to_value(&page).unwrap(),
            json!({"id": "main", "title": "Main", "description": null, "entry": "ui/index.html", "scope": "global", "rail": false}),
            "as the UI reads it"
        );
        // At most four, each id once.
        let pages = |n: usize| {
            json!(
                (0..n)
                    .map(|i| json!({"id": format!("p{i}"), "title": "P", "entry": "ui/index.html"}))
                    .collect::<Vec<_>>()
            )
        };
        assert_eq!(page_only(pages(4)).unwrap().manifest.pages.len(), 4);
        assert_eq!(message(page_only(pages(5))), "The plugin adds more than 4 pages.");
        let twice =
            json!([{"id": "a", "title": "A", "entry": "ui/a.html"}, {"id": "a", "title": "B", "entry": "ui/b.html"}]);
        assert_eq!(message(page_only(twice)), r#"The plugin's contributes.pages has the id "a" twice."#);
        // A scope a later AutoScene adds skips its page; a key it adds is named.
        let p = page_only(json!([
            {"id": "main", "title": "Main", "entry": "ui/index.html", "icon": "ui/icon.svg"},
            {"id": "dock", "title": "Dock", "entry": "ui/dock.html", "scope": "panel"},
        ]))
        .unwrap();
        assert_eq!(p.manifest.pages.len(), 1);
        assert_eq!(
            p.warnings,
            [
                "Ignored contributes.pages[0].icon, which this AutoScene doesn't know.",
                r#"Skipped the page "dock": scope "panel" needs a newer AutoScene."#,
            ]
        );
    }

    #[test]
    fn a_page_is_a_listed_html_file_under_ui() {
        let entry = |entry: &str| page_only(json!([{"id": "main", "title": "Main", "entry": entry}]));
        for good in ["ui/index.html", "ui/pages/main.html"] {
            assert_eq!(entry(good).unwrap().manifest.pages[0].entry, good);
        }
        let under_ui =
            "The plugin's contributes.pages[0].entry must be an .html file under ui/, such as ui/index.html.";
        for bad in ["index.html", "ui/index.htm", "ui/index.HTML", "ui/app.js", "UI/index.html", "pages/ui/index.html"]
        {
            assert_eq!(message(entry(bad)), under_ui, "{bad}");
        }
        for bad in
            ["ui/../index.html", "/ui/index.html", r"ui\index.html", "ui//index.html", "ui/./a.html", "ui/INDEX~1.HTM"]
        {
            assert!(message(entry(bad)).contains("can't be used"), "{bad}");
        }
        assert!(message(page_only(json!([{"id": "main", "title": "Main"}]))).contains("entry is missing"));

        // Its file is the plugin's: listed in its lock, or in its loaded
        // folder, as it is spelled and never through a link.
        let pages = entry("ui/index.html").unwrap().manifest.pages;
        let dir = tempfile::tempdir().unwrap();
        let lock = |paths: &[&str]| FilesLock {
            lock_version: LOCK_VERSION,
            files: paths
                .iter()
                .map(|p| LockedFile { path: (*p).into(), size: 0, sha256: sha256_hex(b""), exec: false })
                .collect(),
        };
        let said = |r: PluginResult<()>| r.map_or_else(|e| e.to_string(), |()| "ok".into());
        assert_eq!(said(check_pages(dir.path(), Some(&lock(&["plugin.json", "ui/index.html"])), &pages)), "ok");
        assert_eq!(
            said(check_pages(dir.path(), Some(&lock(&["plugin.json", "ui/Index.html"])), &pages)),
            "The plugin package is missing its page ui/index.html."
        );
        let folder = "The plugin folder is missing its page ui/index.html, or it is a link.";
        assert_eq!(said(check_pages(dir.path(), None, &pages)), folder);
        std::fs::create_dir_all(dir.path().join("ui/index.html")).unwrap();
        assert_eq!(said(check_pages(dir.path(), None, &pages)), folder, "a folder of its name");
        std::fs::remove_dir(dir.path().join("ui/index.html")).unwrap();
        std::fs::write(dir.path().join("ui/index.html"), b"<!doctype html>").unwrap();
        assert_eq!(said(check_pages(dir.path(), None, &pages)), "ok");
    }

    #[test]
    fn a_plugin_that_runs_no_program_has_a_page_and_no_actions() {
        let main = json!([{"id": "main", "title": "Main", "entry": "ui/index.html"}]);
        let m = page_only(main.clone()).unwrap().manifest;
        assert_eq!((m.runtime.kind, m.runtime.command.len(), m.runtime.gpu), (RuntimeKind::None, 0, false));
        assert!(!m.runs_program());
        assert_eq!(message(m.launch()), "Hello Export runs no program.");
        assert!(m.launches().is_empty(), "asp has no program to mark or check");
        assert_eq!(serde_json::to_value(m.runtime.kind).unwrap(), json!("none"));
        assert!(read(&plugin()).unwrap().manifest.runs_program());

        // Nothing starts, and nothing waits for the GPU.
        for (key, value) in [("command", json!({"*": ["bin/tool"]})), ("gpu", json!(false))] {
            let p = with(|v| {
                v["manifestVersion"] = json!(2);
                v["runtime"] = json!({"type": "none"});
                v["runtime"][key] = value.clone();
                v["contributes"] = json!({ "pages": main.clone() });
            });
            let said = format!("A plugin whose runtime.type is none runs no program: remove runtime.{key}.");
            assert_eq!(message(p), said);
        }
        // An export action or a command would start one.
        for (kind, list) in
            [("exportActions", json!([{"id": "a", "title": "A"}])), ("commands", json!([{"id": "c", "title": "C"}]))]
        {
            let p = with(|v| {
                v["manifestVersion"] = json!(2);
                v["runtime"] = json!({"type": "none"});
                v["contributes"] = json!({ "pages": main.clone() });
                v["contributes"][kind] = list.clone();
            });
            assert_eq!(message(p), "Export actions and commands need a program, and runtime.type none runs none.");
        }
        // Its pages are all it is: none, or none this AutoScene shows, is no plugin.
        let pageless = "A plugin that runs no program needs a page.";
        assert_eq!(message(page_only(json!([]))), pageless);
        let panel = json!([{"id": "dock", "title": "Dock", "entry": "ui/dock.html", "scope": "panel"}]);
        assert_eq!(message(page_only(panel)), pageless);
        let p = with(|v| {
            v["manifestVersion"] = json!(2);
            v["runtime"] = json!({"type": "none"});
            v["contributes"] = json!({"settings": [{"key": "loud", "type": "boolean", "title": "Loud"}]});
        });
        assert_eq!(message(p), pageless);
        // Settings it may have, for its pages.
        let p = with(|v| {
            v["manifestVersion"] = json!(2);
            v["runtime"] = json!({"type": "none"});
            v["contributes"] =
                json!({ "pages": main.clone(), "settings": [{"key": "loud", "type": "boolean", "title": "Loud"}] });
        })
        .unwrap();
        assert_eq!((p.manifest.settings.len(), p.manifest.pages.len()), (1, 1));
        // Its pages show on every platform, whatever it names: none is
        // kept, and none it doesn't know is named.
        let p = with(|v| {
            v["manifestVersion"] = json!(2);
            v["runtime"] = json!({"type": "none"});
            v["platforms"] = json!(["windows-x86_64", "plan9-mips"]);
            v["contributes"] = json!({ "pages": main.clone() });
        })
        .unwrap();
        assert_eq!(p.manifest.platforms, Vec::<String>::new());
        assert_eq!(p.warnings, ["Ignored platforms: a plugin that runs no program works on every platform."]);
    }

    #[test]
    fn rail_is_for_global_pages() {
        let p = page_only(json!([
            {"id": "home", "title": "Home", "entry": "ui/home.html", "rail": true},
            {"id": "clips", "title": "Clips", "entry": "ui/clips.html", "scope": "project", "rail": true},
            {"id": "notes", "title": "Notes", "description": "What you noted.", "entry": "ui/notes.html", "scope": "project"},
        ]))
        .unwrap();
        let pages: Vec<_> = p.manifest.pages.iter().map(|p| (p.id.as_str(), p.scope, p.rail)).collect();
        assert_eq!(
            pages,
            [
                ("home", PageScope::Global, true),
                ("clips", PageScope::Project, false),
                ("notes", PageScope::Project, false)
            ]
        );
        assert_eq!(p.warnings, [r#"The page "clips" is a project page: rail is for global pages."#]);
        assert_eq!(p.manifest.pages[2].description.as_deref(), Some("What you noted."));
        assert_eq!(serde_json::to_value(PageScope::Project).unwrap(), json!("project"));
        let said = message(page_only(json!([{"id": "home", "title": "Home", "entry": "ui/home.html", "rail": "yes"}])));
        assert_eq!(said, "The plugin's contributes.pages[0].rail must be true or false.");
        let long = "x".repeat(41);
        let p = page_only(json!([{"id": "home", "title": long, "entry": "ui/home.html"}])).unwrap();
        assert_eq!((p.manifest.pages[0].title.chars().count(), p.warnings.len()), (40, 1));
    }

    #[test]
    fn a_secret_has_no_default() {
        let secret = |extra: Value| {
            let mut s = json!({"key": "token", "type": "secret", "title": "Token"});
            s.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            with_secrets(json!([s]))
        };
        assert_eq!(message(secret(json!({"default": "hunter2"}))), "A secret setting has no default.");
        assert_eq!(message(secret(json!({"default": ""}))), "A secret setting has no default.");
        // What it doesn't know is skipped as for any setting; a maxLength is the keychain's.
        let p = secret(json!({"maxLength": 20, "default": null})).unwrap();
        assert_eq!(p.manifest.settings[0].kind, SettingKind::Secret { placeholder: None });
        assert_eq!(p.warnings, ["Ignored contributes.settings[0].maxLength, which this AutoScene doesn't know."]);
        // At most 8.
        let many = |n: usize| {
            let settings: Vec<Value> =
                (0..n).map(|i| json!({"key": format!("token{i}"), "type": "secret", "title": "Token"})).collect();
            with_secrets(json!(settings))
        };
        assert_eq!(many(8).unwrap().manifest.secret_keys().count(), 8);
        assert_eq!(message(many(9)), "The plugin has more than 8 secret settings.");
    }

    #[test]
    fn version_2_names_the_hosts_net_fetch_cant_reach() {
        let named = |version: u32| {
            with(|v| {
                v["manifestVersion"] = json!(version);
                v["permissions"] = json!(["network"]);
                v["network"] = json!({"hosts": ["discord.com", "localhost", "127.0.0.1", "nas.local", "api.example"]});
            })
            .unwrap()
        };
        let p = named(2);
        // Kept, so the review shows all the plugin names.
        assert_eq!(p.manifest.network_hosts.len(), 5);
        assert_eq!(
            p.warnings,
            [
                "net.fetch can't reach localhost.",
                "net.fetch can't reach 127.0.0.1.",
                "net.fetch can't reach nas.local.",
                "net.fetch can't reach api.example.",
            ]
        );
        // Version 1 reads as it did.
        assert_eq!(named(1).warnings, Vec::<String>::new());
    }

    /// A manifest of version 2 and protocol 2 with the settings `settings`.
    fn with_secrets(settings: Value) -> PluginResult<Parsed> {
        with(|v| {
            v["manifestVersion"] = json!(2);
            v["engines"]["protocol"] = json!(2);
            v["contributes"]["settings"] = settings;
        })
    }

    #[test]
    fn a_secret_needs_a_program_of_protocol_2() {
        // A run of protocol 1 never has its calls answered, so it could never read one.
        let token = json!([{"key": "token", "type": "secret", "title": "Token"}]);
        let p = with(|v| {
            v["manifestVersion"] = json!(2);
            v["contributes"]["settings"] = token.clone();
        })
        .unwrap();
        assert_eq!(p.manifest.secret_keys().count(), 0);
        assert_eq!(p.warnings, [r#"Skipped the setting "token": secret settings need "engines": {"protocol": 2}."#]);
        assert_eq!(with_secrets(token).unwrap().manifest.secret_keys().collect::<Vec<_>>(), ["token"]);
    }

    #[test]
    fn a_page_reads_a_secret_whatever_the_protocol() {
        // A page asks through its frame, which protocol 1 doesn't touch.
        let token = json!([{"key": "token", "type": "secret", "title": "Token"}]);
        let main = json!([{"id": "main", "title": "Main", "entry": "ui/index.html"}]);
        let page_only = with(|v| {
            v["manifestVersion"] = json!(2);
            v["runtime"] = json!({"type": "none"});
            v["contributes"] = json!({"pages": main, "settings": token});
        })
        .unwrap();
        assert_eq!(page_only.manifest.engines.protocol, 1);
        assert_eq!(page_only.manifest.secret_keys().collect::<Vec<_>>(), ["token"]);
        assert_eq!(page_only.warnings, Vec::<String>::new());
        // A program of protocol 1 keeps it for its page.
        let paged = with(|v| {
            v["manifestVersion"] = json!(2);
            v["contributes"]["pages"] = main.clone();
            v["contributes"]["settings"] = token.clone();
        })
        .unwrap();
        assert_eq!(paged.manifest.secret_keys().collect::<Vec<_>>(), ["token"]);
    }

    #[test]
    fn secret_keys_differ_in_more_than_capitals() {
        let secret = |key: &str| json!({"key": key, "type": "secret", "title": "Key"});
        // Windows' keychain takes them for one: the plugin would read the other's value.
        assert_eq!(
            message(with_secrets(json!([secret("apiKey"), secret("token"), secret("apikey")]))),
            r#"The plugin's secret settings "apiKey" and "apikey" differ only in capitals, which a keychain doesn't tell apart."#
        );
        let p = with_secrets(json!([secret("apiKey"), secret("api_key")])).unwrap();
        assert_eq!(p.manifest.secret_keys().collect::<Vec<_>>(), ["apiKey", "api_key"]);
        // A plain setting is in no keychain.
        let plain = json!({"key": "apikey", "type": "string", "title": "Key"});
        assert_eq!(with_secrets(json!([secret("apiKey"), plain])).unwrap().manifest.settings.len(), 2);
    }

    #[test]
    fn a_version_2_manifest_reads_as_too_new_to_version_1_rules() {
        // A version past this AutoScene's is refused before anything else
        // is read, as an AutoScene of version 1 refuses version 2.
        assert_eq!(MANIFEST_VERSION, 2);
        let later = json!({"manifestVersion": MANIFEST_VERSION + 1, "runtime": {"type": "hologram"}});
        assert_eq!(too_new(read(&later)), "manifestVersion");
        assert_eq!(PluginError::TooNew("manifestVersion".into()).to_string(), "This plugin needs a newer AutoScene.");
        // Version 2 reads, and says it is version 2.
        let p = with(|v| v["manifestVersion"] = json!(2)).unwrap();
        assert_eq!((p.manifest.manifest_version, p.warnings.len()), (2, 0));
    }

    /// The `plugin.json` of each example plugin.
    fn examples() -> Vec<Value> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/plugins");
        let mut found: Vec<PathBuf> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
        found.sort();
        found.iter().map(|d| serde_json::from_slice(&std::fs::read(d.join("plugin.json")).unwrap()).unwrap()).collect()
    }

    #[test]
    fn a_version_1_manifest_reads_as_it_did() {
        // What the M1 tests read, and the example plugins.
        let mut program = plugin();
        program["permissions"] = json!(["network", "project:read"]);
        program["network"] = json!({"hosts": ["discord.com"]});
        program["runtime"] = json!({"type": "process", "command": {"*": ["bin/tool.exe", "--fast"]}, "gpu": true});
        let mut fixtures = vec![plugin(), program];
        fixtures.extend(examples());
        assert!(fixtures.len() >= 4, "{}", fixtures.len());
        for v1 in fixtures {
            assert_eq!(v1["manifestVersion"], json!(1));
            let old = read(&v1).unwrap();
            assert_eq!((old.manifest.manifest_version, old.manifest.runtime.kind), (1, RuntimeKind::Process));
            // Read as version 2, nothing else changes: version 2 only adds.
            let mut v2 = v1.clone();
            v2["manifestVersion"] = json!(2);
            let new = read(&v2).unwrap();
            assert_eq!(new.warnings, old.warnings, "{}", v1["id"]);
            let as_json = |m: &Manifest| {
                let mut j = serde_json::to_value(m).unwrap();
                j.as_object_mut().unwrap().remove("manifestVersion");
                j
            };
            assert_eq!(as_json(&new.manifest), as_json(&old.manifest), "{}", v1["id"]);
            assert_eq!(serde_json::to_value(old.manifest.runtime.kind).unwrap(), json!("process"));
        }
    }

    #[test]
    fn beta_versions_meet_their_release_requirement() {
        let app = |v: &str| Version::parse(v).unwrap();
        let meets = |req: &str, version: &str| {
            let mut v = plugin();
            v["engines"]["autoscene"] = json!(req);
            parse(&serde_json::to_vec(&v).unwrap(), &app(version)).map(drop)
        };
        // "=1.0.3-BETA" needs the comparator's pre-release cleared too.
        for req in [">=1.0.0", ">=1.0.3", ">=1.0.0, <2", "^1", "1.0.*", "*", ">=1.0.3-BETA", "=1.0.3-BETA", "~1.0"] {
            assert!(meets(req, "1.0.3-BETA").is_ok(), "{req}");
        }
        assert!(meets(">=1.0.0", "1.0.3+build.7").is_ok());
        for req in [">=1.0.4", ">1.0.3", "^2", "=1.1.0"] {
            assert!(matches!(meets(req, "1.0.3-BETA"), Err(PluginError::TooNew(_))), "{req}");
        }
        for req in ["<1.0.0", "<=1.0.2", "~0.9", "=1.0.2"] {
            let m = message(meets(req, "1.0.3-BETA"));
            assert_eq!(m, "This plugin only works with an older AutoScene.", "{req}");
        }
        let m = read(&plugin()).unwrap().manifest;
        assert!(m.requirement_met(&app("1.0.3-BETA")).is_ok());
        assert!(m.requirement_met(&app("2.0.0-BETA")).is_err());
    }

    #[test]
    fn space_separated_requirements_are_refused_with_a_hint() {
        let m = message(with(|v| v["engines"]["autoscene"] = json!(">=1.1.0 <2")));
        assert!(m.contains("use commas between the parts"), "{m}");
        assert!(m.contains(r#"">=1.0.0, <2""#), "{m}");
    }

    #[test]
    fn commands_never_reach_a_shell() {
        // On Linux too, where a bare name needs no .exe.
        for name in [
            "cmd",
            "cmd.exe",
            "sh",
            "zsh",
            "dash",
            "env",
            "wsl",
            "bash",
            "python3",
            "Python3.EXE",
            "python3.12",
            "pythonw",
            "py",
            "node",
            "perl",
            "cscript",
            "mshta.exe",
            "pwsh",
            "uvx",
        ] {
            let p = with(|v| v["runtime"]["command"] = json!({"linux-x86_64": [name, "-c", "curl x | sh"]}));
            assert!(message(p).contains("belongs to the computer"), "{name}");
        }
        for parts in [
            json!(["/bin/sh", "-c", "x"]),
            json!(["/usr/local/bin/tool"]),
            json!([r"C:\Windows\System32\cmd.exe", "/C", "x"]),
            json!(["C:/tools/x.exe"]),
            json!(["../outside/tool"]),
            json!(["bin/run.sh"]),
            json!(["run.bat"]),
            json!(["bin/tool.CMD"]),
            json!(["main.py"]),
            json!(["-x.exe"]),
            json!(["bin/tool.exe", "a\0b"]),
            json!(["bin/tool.exe", "\u{202E}gpj.exe"]),
            json!(["bin/tool.exe", "x".repeat(257)]),
            json!([]),
            json!(vec!["bin/tool.exe"; 17]),
        ] {
            assert!(command(parts.clone()).is_err(), "{parts}");
        }
        let m = message(command(json!(["python3", "main.py"])));
        assert!(m.contains(UV_FORM), "{m}");
        let m = message(command(json!(["bin/tool.exe", "x".repeat(257)])));
        assert!(m.contains("longer than 256 characters"), "{m}");

        let exe = command(json!(["bin/tool.exe", "--fast", "/quiet", "x".repeat(256)])).unwrap();
        let args = vec!["--fast".into(), "/quiet".into(), "x".repeat(256)];
        assert_eq!(exe, Launch::Exe { program: "bin/tool.exe".into(), args });
        assert_eq!(exe.files(), ["bin/tool.exe"]);
        // Started by its path in the plugin's folder, never by name.
        let dir = Path::new("plugins").join("dev.a.b");
        assert_eq!(exe.entry_in(&dir), dir.join("bin").join("tool.exe"));
        // A bundled interpreter is the plugin's own, and so is a tool named like a language.
        assert!(command(json!(["python/python3.exe", "main.py"])).is_ok());
        assert!(command(json!(["pythonic.exe"])).is_ok());
    }

    #[test]
    fn windows_programs_are_exe_files() {
        // Windows runs a .bat through cmd.exe, and bin/tool.exe for bin/tool.
        let m = message(with(|v| v["runtime"]["command"] = json!({"windows-x86_64": ["bin/tool"]})));
        assert!(m.contains("must be an .exe file"), "{m}");
        let p = with(|v| {
            v["runtime"]["command"] = json!({"windows-aarch64": ["bin/tool.EXE"], "linux-x86_64": ["bin/tool"]});
        });
        assert!(p.is_ok());
        // A command for every platform is checked when it runs on Windows.
        let m = with(|v| v["runtime"]["command"] = json!({"*": ["bin/tool"]})).unwrap().manifest;
        assert!(message(m.launch_on("windows-x86_64")).contains(r#"runtime.command["*"] runs on Windows"#));
        assert_eq!(m.launch_on("macos-aarch64").unwrap().files(), ["bin/tool"]);
    }

    #[test]
    fn uv_scripts_need_their_lock() {
        let uv = command(json!(["uv", "run", "--script", "--locked", "scripts/main.py"])).unwrap();
        assert_eq!(uv, Launch::UvScript { script: "scripts/main.py".into() });
        assert_eq!(uv.files(), ["scripts/main.py", "scripts/main.py.lock"]);
        let dir = Path::new("plugins").join("dev.a.b");
        assert_eq!(uv.entry_in(&dir), dir.join("scripts").join("main.py"));
        // The lock's name must fit a package too.
        let longest = format!("{}.py", "a".repeat(195 - ".lock".len()));
        assert!(command(json!(["uv", "run", "--script", "--locked", longest])).is_ok());
        for parts in [
            json!(["uv", "run", "--script", "main.py"]),
            json!(["uv", "run", "--script", "--frozen", "main.py"]),
            json!(["uv", "run", "--locked", "--script", "main.py"]),
            json!(["uv", "run", "--script", "--locked", "main.py", "--extra"]),
            json!(["uv", "run", "--script", "--locked", "--with", "requests", "main.py"]),
            json!(["uv", "run", "--script", "--locked", "main.sh"]),
            json!(["uv", "run", "--script", "--locked", "../main.py"]),
            json!(["uv", "run", "--script", "--locked", "/tmp/main.py"]),
            json!(["uv", "run", "--script", "--locked", format!("{}.py", "a".repeat(197))]),
            json!(["uv.exe", "run", "--script", "--locked", "main.py"]),
            json!(["uv", "tool", "run", "x"]),
            json!(["uvx", "ruff"]),
        ] {
            let m = message(command(parts.clone()));
            assert!(m.contains(UV_FORM) || m.contains("can't be used"), "{parts}: {m}");
        }
        // Passed as it is, uv would read these as options (-w six, -p .py):
        // what runs would come from PyPI, not the lock.
        for script in ["-wsix.py", "-p.py"] {
            let m = message(command(json!(["uv", "run", "--script", "--locked", script])));
            assert!(m.contains("starts with a dash"), "{script}: {m}");
        }
    }

    #[test]
    fn the_command_follows_the_platform() {
        let here = current_platform();
        let p = with(|v| {
            v["runtime"]["command"] =
                json!({ here: ["bin/here.exe"], "*": ["bin/elsewhere.exe"], "plan9-mips": ["sh", "-c", "x"] });
        })
        .unwrap();
        assert_eq!(p.manifest.launch().unwrap().files(), ["bin/here.exe"]);
        // A platform this AutoScene doesn't know is skipped, whatever it holds.
        assert_eq!(p.warnings, [r#"Skipped runtime.command["plan9-mips"], which this AutoScene doesn't know."#]);
        assert_eq!(p.manifest.runtime.command.len(), 2);

        let p = with(|v| v["runtime"]["command"] = json!({"plan9-mips": ["bin/x"]})).unwrap();
        assert!(matches!(p.manifest.launch(), Err(PluginError::Platform)));
        let p = with(|v| v["platforms"] = json!(["plan9-mips"])).unwrap();
        assert_eq!(p.manifest.platforms, ["plan9-mips"]);
        assert!(matches!(p.manifest.launch(), Err(PluginError::Platform)));
        let p = with(|v| v["platforms"] = json!([here])).unwrap();
        assert!(p.manifest.launch().is_ok());
        // A computer AutoScene doesn't know runs nothing, even a plugin naming "other".
        let p = with(|v| v["platforms"] = json!(["other"])).unwrap();
        assert!(matches!(p.manifest.launch_on("other"), Err(PluginError::Platform)));
        for platforms in [json!(["Windows"]), json!(["windows\u{202E}"]), json!([""]), json!(vec!["linux-x86_64"; 17])]
        {
            assert!(message(with(|v| v["platforms"] = platforms.clone())).contains("platforms"), "{platforms}");
        }
    }

    #[test]
    fn launches_name_each_platform_it_runs_on() {
        let launches = |edit: Edit| {
            let m = with(edit).unwrap().manifest;
            m.launches().into_iter().map(|(p, l)| (p, l.map_err(|e| e.to_string()))).collect::<Vec<_>>()
        };
        // Windows falls back to "*" too, where only an .exe runs.
        let exe = "The plugin's runtime.command[\"*\"] runs on Windows, so its program must be an .exe file.";
        let tool = || Ok(Launch::Exe { program: "bin/tool".into(), args: Vec::new() });
        assert_eq!(
            launches(|v| v["runtime"]["command"] = json!({"*": ["bin/tool"]})),
            [
                ("windows-x86_64", Err(exe.into())),
                ("windows-aarch64", Err(exe.into())),
                ("macos-aarch64", tool()),
                ("macos-x86_64", tool()),
                ("linux-x86_64", tool()),
                ("linux-aarch64", tool()),
            ]
        );
        let l = launches(|v| {
            v["runtime"]["command"] = json!({"windows-x86_64": ["bin/tool.exe"], "*": ["bin/tool"]});
            v["platforms"] = json!(["windows-x86_64", "linux-x86_64", "plan9-mips"]);
        });
        let windows = Ok(Launch::Exe { program: "bin/tool.exe".into(), args: Vec::new() });
        assert_eq!(l, [("windows-x86_64", windows), ("linux-x86_64", tool())]);
        assert_eq!(launches(|v| v["platforms"] = json!(["plan9-mips"])), []);
    }

    #[test]
    fn any_app_reads_a_plugin_made_for_another() {
        let mut v = plugin();
        v["engines"]["autoscene"] = json!(">=99.0.0");
        let bytes = serde_json::to_vec(&v).unwrap();
        assert_eq!(too_new(parse(&bytes, &app())), "engines.autoscene");
        let m = parse_for_any_app(&bytes).unwrap().manifest;
        assert!(matches!(m.requirement_met(&app()), Err(PluginError::TooNew(_))));
        // What it asks for must still read.
        v["engines"]["autoscene"] = json!(">=1.1.0 <2");
        assert!(message(parse_for_any_app(&serde_json::to_vec(&v).unwrap())).contains("use commas"));
    }

    #[test]
    fn a_plugin_json_is_read_no_further_than_parse_takes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plugin.json");
        std::fs::write(&path, vec![b' '; MAX_MANIFEST_BYTES as usize + 100]).unwrap();
        assert_eq!(read_file(&path).unwrap().len() as u64, MAX_MANIFEST_BYTES + 1);
    }

    /// A change to the test plugin.
    type Edit = fn(&mut Value);

    #[test]
    fn fields_that_break_their_rule_are_refused() {
        let cases: [(Edit, &str); 22] = [
            (|v| v["version"] = json!("1.2"), "version must be a version number"),
            (|v| v["version"] = json!("banana"), "version must be a version number"),
            (|v| v["runtime"]["command"] = json!({}), "runtime.command names no command"),
            (|v| v["contributes"]["commands"][0]["id"] = json!("../x"), "commands[0].id must use only"),
            (|v| v["contributes"]["commands"][0]["id"] = json!("Hello"), "commands[0].id must use only"),
            (|v| v["contributes"]["commands"][0]["id"] = json!("a".repeat(33)), "commands[0].id must use only"),
            (
                |v| v["contributes"]["commands"] = json!([{"id": "a", "title": "A"}, {"id": "a", "title": "B"}]),
                r#"contributes.commands has the id "a" twice"#,
            ),
            (
                |v| {
                    let again = v["contributes"]["settings"][0].clone();
                    v["contributes"]["settings"].as_array_mut().unwrap().push(again);
                },
                r#"contributes.settings has the key "greeting" twice"#,
            ),
            (
                |v| v["contributes"]["exportActions"] = json!(vec![json!({"id": "a", "title": "A"}); 9]),
                "more than 8 export actions",
            ),
            (
                |v| v["contributes"]["commands"] = json!(vec![json!({"id": "a", "title": "A"}); 17]),
                "more than 16 commands",
            ),
            (
                |v| {
                    v["contributes"]["settings"] = json!(vec![json!({"key": "a", "type": "boolean", "title": "A"}); 33])
                },
                "more than 32 settings",
            ),
            (|v| v["contributes"]["settings"][0]["key"] = json!("a".repeat(41)), "must start with a lowercase letter"),
            (|v| v["contributes"]["settings"][0]["default"] = json!("x".repeat(2001)), "longer than its maxLength"),
            (
                |v| {
                    v["contributes"]["settings"][0] =
                        json!({"key": "s", "type": "string", "title": "S", "maxLength": 2, "default": "abc"})
                },
                "longer than its maxLength",
            ),
            (|v| v["contributes"]["settings"][0]["default"] = json!("a\0b"), "has a NUL character"),
            (
                |v| {
                    v["contributes"]["settings"][0] =
                        json!({"key": "n", "type": "number", "title": "N", "min": 2, "max": 1})
                },
                "min is above its max",
            ),
            (
                |v| v["contributes"]["settings"][0] = json!({"key": "n", "type": "number", "title": "N", "step": 0}),
                "step must be above 0",
            ),
            (
                |v| v["contributes"]["settings"][0] = select(json!([["a", "A"], ["a", "B"]])),
                r#"has the value "a" twice"#,
            ),
            (|v| v["contributes"]["settings"][0] = select(json!([["", "Empty"]])), "options[0].value is empty"),
            (|v| v["contributes"]["settings"][0] = select(json!([["a\0", "A"]])), "without control characters"),
            (
                |v| v["contributes"]["settings"][0] = select(json!([["x".repeat(201), "Long"]])),
                "at most 200 characters",
            ),
            (|v| v["contributes"]["settings"][0] = select(json!(vec![json!(["a", "A"]); 33])), "1 to 32 choices"),
        ];
        for (edit, said) in cases {
            let m = message(with(edit));
            assert!(m.contains(said), "{said} in {m}");
        }
        // Just within each limit.
        let p = with(|v| {
            v["contributes"]["exportActions"] =
                json!((0..8).map(|i| json!({"id": format!("a{i}"), "title": "A"})).collect::<Vec<_>>());
            v["contributes"]["commands"][0]["id"] = json!("a".repeat(32));
            v["contributes"]["settings"][0]["key"] = json!("a".repeat(40));
            v["contributes"]["settings"].as_array_mut().unwrap().push(select(json!([["x".repeat(200), "Long"]])));
        });
        assert!(p.is_ok(), "{p:?}");
    }

    /// A select setting with these `[value, label]` options.
    fn select(options: Value) -> Value {
        let options: Vec<Value> =
            options.as_array().unwrap().iter().map(|o| json!({"value": o[0], "label": o[1]})).collect();
        json!({"key": "mode", "type": "select", "title": "Mode", "options": options})
    }

    #[test]
    fn a_key_named_twice_is_refused() {
        // serde_json keeps the last one; a reader that keeps the first would see other permissions.
        let twice = br#"{"manifestVersion": 1, "permissions": ["exports:read"], "permissions": ["network"]}"#;
        assert_eq!(message(parse(twice, &app())), r#"The plugin's plugin.json has "permissions" twice in one object."#);
        let nested =
            serde_json::to_string(&plugin()).unwrap().replacen(r#""id":"hello""#, r#""id":"hello","id":"bye""#, 1);
        assert!(nested.contains(r#""id":"bye""#));
        assert!(message(parse(nested.as_bytes(), &app())).contains(r#""id" twice"#));
        // The same key in two objects is fine.
        assert!(read(&plugin()).is_ok());
    }

    #[test]
    fn a_plugin_json_saved_with_a_byte_order_mark_reads() {
        let bytes = [&b"\xEF\xBB\xBF"[..], &serde_json::to_vec(&plugin()).unwrap()].concat();
        assert_eq!(parse(&bytes, &app()).unwrap().manifest.name, "Hello Export");
    }

    #[test]
    fn texts_lose_control_characters_and_long_ones_are_cut() {
        let p = with(|v| {
            v["name"] = json!("Hello\u{202E}\u{7}  World\n");
            v["description"] = json!("x".repeat(301));
            v["repository"] = json!("http://example.org");
            v["icon"] = json!("../icon.png");
        })
        .unwrap();
        assert_eq!(p.manifest.name, "Hello World");
        assert_eq!(p.manifest.description.chars().count(), 300);
        assert!(p.manifest.description.ends_with('…'));
        assert_eq!(p.manifest.repository, None);
        assert_eq!(p.manifest.icon, None);
        assert_eq!(p.warnings.len(), 3);
        assert!(message(read(&json!([1]))).contains("must hold an object"));
        assert!(message(parse(b"{\"id\":", &app())).contains("isn't valid JSON"));
        assert!(message(parse(&vec![b' '; MAX_MANIFEST_BYTES as usize + 1], &app())).contains("larger than"));

        // Names that show nothing are empty: blank letters, format characters, joiners alone.
        for blank in
            ["\u{200B} \u{7}", "\u{3164}", "\u{115F}\u{1160}", "\u{2800}\u{FFA0}", "\u{200D}\u{AD}", "\u{E0020}"]
        {
            assert!(message(with(|v| v["name"] = json!(blank))).contains("name is empty"), "{blank:?}");
        }
        let name = |text: &str| with(|v| v["name"] = json!(text)).unwrap().manifest.name;
        assert_eq!(name("Good\u{E0020}\u{E0069}gnore"), "Goodgnore");
        assert_eq!(name("Soft\u{AD}ware\u{2062}\u{FE0F}\u{180E}"), "Software");
        assert_eq!(name("Dev \u{1F469}\u{200D}\u{1F4BB}"), "Dev \u{1F469}\u{200D}\u{1F4BB}");

        // An icon is a picture in the plugin.
        let p = with(|v| v["icon"] = json!("icon.exe")).unwrap();
        assert_eq!((p.manifest.icon, p.warnings.len()), (None, 1));
        assert_eq!(
            with(|v| v["icon"] = json!("assets/Icon.SVG")).unwrap().manifest.icon.as_deref(),
            Some("assets/Icon.SVG")
        );

        // A link leads where it reads.
        let link = |url: &str| with(|v| v["author"]["url"] = json!(url)).unwrap().manifest.author.url;
        for url in [
            "https://autoscene.app@evil.example/",
            "https://evil.example%5C@autoscene.app",
            r"https://evil.example\@autoscene.app",
            "https://",
            "https:///x",
            "https://a..b/",
            "https://example.org:/",
            "https://example.org:99999/",
            "https://exa mple.org",
        ] {
            assert_eq!(link(url), None, "{url}");
        }
        for url in ["https://autoscene.app", "https://GitHub.com/a/b?c=d#e", "https://example.org:8443/x"] {
            assert_eq!(link(url).as_deref(), Some(url));
        }
    }
}

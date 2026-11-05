//! Types that cross the IPC boundary to the React UI. Everything here derives
//! `specta::Type`, and `app/src/bindings.ts` is generated from it — the UI
//! never hand-writes a backend type.

use std::collections::BTreeMap;

use as_core::{CharacterId, ExportId, FrameCount, FrameIdx, JobId, MediaId, Msg, ProjectId, Rational};
use serde::{Deserialize, Serialize};
use specta::Type;

pub use as_export::{ClipOutcome, ExportPreset};
pub use as_media::{AudioStream, HdrKind, MediaInfo, SubtitleStream, VideoStream};

/// Static facts about the running app, shown in Settings → About and used by
/// the shell for platform-specific chrome.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub platform: Platform,
    pub arch: String,
    pub data_dir: String,
    pub logs_dir: String,
    pub library_root: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Windows,
    Macos,
    Linux,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Macos
        } else {
            Self::Linux
        }
    }
}

/// The media engine as found on this machine (Settings → About / Hardware).
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MediaToolsStatus {
    pub ready: bool,
    pub version: Option<String>,
    pub path: Option<String>,
    pub error: Option<Msg>,
}

/// An AI model AutoScene downloads the first time an analysis needs it
/// (Settings → About).
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    /// What it finds.
    pub purpose: String,
    /// Who made it, and the copy AutoScene downloads.
    pub source: String,
    /// Its licence, as its makers name it.
    pub license: String,
    /// The licence allows non-commercial use only.
    pub non_commercial: bool,
    pub size_bytes: f64,
    /// Downloaded and verified.
    pub installed: bool,
}

/// User preferences stored in `library.db`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    /// Folder that holds `<Title>.autoscene` project folders.
    pub library_root: String,
    /// Preset preselected in the export dialog.
    pub export_preset_id: String,
    /// Audio language picked automatically for new episodes (ISO 639-2 code,
    /// e.g. `eng`); empty = use each file's default track.
    #[serde(default)]
    pub audio_language: String,
    /// Also follow people by their bodies and clothes, so shots where a
    /// character's face doesn't show (turned away, too small) join their
    /// scenepack. Analysis takes longer; off by default.
    #[serde(default)]
    pub find_bodies: bool,
    /// Whether new live-action projects start with Fast Scan (smaller face
    /// models, a few frames per shot; anime keeps its character models):
    /// much faster, less sure who is who. Each project keeps what it was
    /// created with (see [`ProjectSummary::fast_scan`]). **Read only**:
    /// `settings_update` ignores it; `scan_default_set` chooses, and
    /// [`ScanDefaults`] says why.
    #[serde(default)]
    pub fast_scan: bool,
    /// Anime projects may ask AniList, animated series TVmaze, animated films
    /// Wikidata and their franchise's Fandom wiki, for their cast, to name
    /// their characters: only the search text (the project's title, or what
    /// the person confirmed), and requests for that cast and its pictures, are
    /// sent. Off by default; switching it off deletes the pictures kept.
    #[serde(default)]
    pub anilist: bool,
    /// The keyboard shortcuts as the person set them (Settings → Shortcuts).
    #[serde(default)]
    pub shortcuts: Shortcuts,
    /// Lets plugins be installed and run that AutoScene has not reviewed:
    /// unsigned ones, and ones only their author signed. They run with your
    /// user rights. Off by default. Changed only by
    /// `plugins_allow_unreviewed`: `settings_update` keeps it as it is.
    #[serde(default)]
    pub allow_unreviewed_plugins: bool,
    /// The graphics the analysis runs on: `""` Automatic (the card DirectML
    /// takes by default: the one Windows ranks fastest), `"cpu"` the
    /// processor only, or a card's key ([`GpuOption::key`]). Applies from the
    /// next analysis; a card that isn't connected leaves Automatic in its
    /// place.
    #[serde(default)]
    pub gpu: String,
    /// Checks the plugin catalog once a day for new versions and withdrawn
    /// plugins. On by default.
    #[serde(default = "yes")]
    pub plugin_catalog_checks: bool,
    /// Analysis uses NVIDIA's engine once its files are downloaded: on
    /// unless switched off. Applies from the next analysis.
    #[serde(default = "yes")]
    pub nvidia_acceleration: bool,
    /// What Discord may show. Everything off until the person turns it on.
    #[serde(default)]
    pub discord_rpc: DiscordRpcSettings,
    /// The language AutoScene speaks: Automatic until the person chooses one.
    #[serde(default)]
    pub language: UiLanguage,
}

fn yes() -> bool {
    true
}

/// What AutoScene may tell Discord (`app/src-tauri/src/discord.rs`). All off
/// by default: nothing goes to Discord until `enabled`, and a name only with
/// its own switch as well.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", default)]
pub struct DiscordRpcSettings {
    /// Shows that the person is working in AutoScene on their profile.
    pub enabled: bool,
    /// The project's title in place of "Working on a project".
    pub show_project: bool,
    /// The open episode's or file's name in place of "Reviewing an episode"
    /// or "Reviewing a file".
    pub show_episode: bool,
    /// The open character's name in place of "Making a scenepack".
    pub show_character: bool,
}

/// The language AutoScene speaks, as the person chose it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum UiLanguage {
    /// Automatic: the first language on the system's list that AutoScene
    /// speaks, English when it speaks none of them.
    #[default]
    System,
    En,
}

impl UiLanguage {
    /// What the library stores for it; Automatic stores nothing.
    pub fn code(self) -> Option<&'static str> {
        match self {
            Self::System => None,
            Self::En => Some("en"),
        }
    }

    /// The choice a stored code stands for; `None` for one this version
    /// doesn't know.
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "en" => Some(Self::En),
            _ => None,
        }
    }
}

/// A language AutoScene speaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Lang {
    En,
}

/// The language AutoScene speaks now, the choice it comes from, and what
/// Automatic would speak.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UiLocale {
    pub setting: UiLanguage,
    /// What `setting` comes to on this computer: the language in use.
    pub resolved: Lang,
    /// What Automatic comes to on this computer, whatever is chosen now: what
    /// the Automatic option is labelled with.
    pub automatic: Lang,
}

/// The keys the person chose: an editor's keys to start from, and their own
/// changes on top. The UI owns what the names mean (`app/src/lib/shortcuts.ts`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Shortcuts {
    /// `premiere`, `resolve`, or `""` for AutoScene's own keys.
    #[serde(default)]
    pub preset: String,
    /// Keys changed on top of the preset, by `<scope>.<shortcut>.<n>`
    /// (`player.frame.1` is the next frame); an empty list leaves it without a key.
    #[serde(default)]
    pub keys: BTreeMap<String, Vec<KeyBinding>>,
}

/// A key press that triggers a shortcut: `KeyboardEvent.key` in lower case
/// (a digit stands for the key that types it on a US layout) and the
/// modifiers; `mod` is Ctrl (⌘ on macOS).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct KeyBinding {
    pub key: String,
    #[serde(rename = "mod", default)]
    pub command: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub alt: bool,
}

// ---------------------------------------------------------------------------
// Fast Scan's default for new projects
// ---------------------------------------------------------------------------

/// What this computer's analysis runs on, as Fast Scan's default sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum GraphicsClass {
    /// A separate graphics card built for AI work (matrix units the runtimes
    /// use): on Automatic, new projects keep the full scan.
    ForAi,
    /// A separate graphics card without them, or a virtual adapter.
    NotForAi,
    /// Built-in graphics, every Mac's included.
    Integrated,
    /// No graphics the analysis can use: it runs on the processor.
    Processor,
    /// AutoScene couldn't tell: on Automatic, new projects keep the full scan.
    Unknown,
}

/// The person's default for new projects: follow this computer, or always
/// one scan.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ScanChoice {
    /// Automatic: Fast Scan where the graphics aren't built for AI work.
    #[default]
    Auto,
    /// Every new project starts with Fast Scan, 3D animation too.
    Fast,
    /// Every new project starts with the full scan.
    Full,
}

/// This computer, as Settings and the New Project dialog speak of it. The
/// system it runs is [`AppInfo::platform`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ThisComputer {
    pub class: GraphicsClass,
    /// The graphics as their driver names them ("Intel Iris Xe Graphics",
    /// "Apple M2"); empty for the processor and when nothing names them.
    pub name: String,
}

/// What a new project of one footage starts with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScanAdvice {
    pub content: ContentKind,
    /// Fast Scan, else the full scan.
    pub fast: bool,
}

/// The scan new projects start with, and why: what the New Project dialog,
/// Settings, Edit details and the first-start notice say about Fast Scan.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScanDefaults {
    pub computer: ThisComputer,
    pub choice: ScanChoice,
    /// One entry per footage: live action, mixed, 3D animation, anime.
    pub footage: Vec<ScanAdvice>,
    /// The first-start notice is due: the choice is Automatic, this
    /// computer's graphics get Fast Scan, and the notice isn't done for this
    /// library ("Don't show this again", or a default met in Settings).
    pub notice: bool,
}

// ---------------------------------------------------------------------------
// The graphics the analysis runs on (Settings → General)
// ---------------------------------------------------------------------------

/// A graphics card the analysis can run on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GpuOption {
    /// What [`AppSettings::gpu`] stores when it is chosen.
    pub key: String,
    /// As its driver names it, numbered among identical cards. The words are
    /// AutoScene's only for a card its driver gives no name.
    pub name: Msg,
}

/// What the analysis runs on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AnalysisDevice {
    /// A graphics card; `name` is empty when the graphics driver picks it.
    Gpu { name: String },
    /// An Apple chip: its graphics and Neural Engine.
    Chip { name: String },
    /// The processor.
    Processor,
}

/// What can be chosen here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum HardwareChoice {
    /// Nothing to choose: Linux, or Windows without a graphics card the
    /// analysis can use. It runs on the processor.
    Fixed,
    /// Automatic or the processor only: an Apple chip, or one card.
    AutoOrProcessor,
    /// Automatic, each card, or the processor only.
    Cards,
}

/// Settings → General → Graphics card: what can be chosen, what is, and
/// what the analysis runs on because of it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HardwareInfo {
    /// The graphics cards found, in the order DirectML takes them
    /// (Automatic's first).
    pub gpus: Vec<GpuOption>,
    pub choice: HardwareChoice,
    /// The option chosen: `"auto"`, `"cpu"`, or the key of the card in use
    /// (as it is named now, which a driver update may have changed).
    pub selected: String,
    /// What the analysis runs on with that choice.
    pub using: AnalysisDevice,
    /// The chosen card isn't connected: Automatic's runs in its place.
    pub missing: bool,
    /// The choice changed while an analysis runs on the one before: it
    /// applies from the next.
    pub pending: bool,
    /// The last analysis ran some steps on the processor, much slower,
    /// because the graphics couldn't take them.
    pub fell_back: bool,
    /// The ONNX Runtime's version (About).
    pub runtime: String,
    pub accel: AccelInfo,
}

/// Settings → General → NVIDIA acceleration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AccelInfo {
    pub row: AccelRow,
    /// NVIDIA's files are here (About names them).
    pub installed: bool,
    /// Some of them are, finished or not: the row offers to remove them.
    pub removable: bool,
    /// Analysis uses them.
    pub active: bool,
    /// The person stopped the update to the release this build pins, on a
    /// card that would use it: only the row can bring it now.
    pub update_stopped: bool,
    /// The bytes a download fetches.
    pub download_bytes: u64,
    /// NVIDIA's licence, which the person agrees to by downloading.
    pub licence: String,
    /// TensorRT for RTX's release (About).
    pub engine: String,
}

/// What the row shows: the first of these that applies, in this order, but
/// for [`AccelRow::Hidden`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum AccelRow {
    /// Nothing: this computer can't use it and has no files of it.
    Hidden,
    Downloading {
        done: u64,
        total: u64,
    },
    /// The last download failed; `message` says why, for the person.
    Failed {
        message: Msg,
    },
    /// The files go once the current analysis is done with them.
    Removing,
    /// It can be downloaded (or its new licence has to be agreed to).
    Available,
    /// Another card, or the processor, is chosen.
    NotUsed,
    /// It failed twice in a row, or doesn't load: analysis runs without it
    /// until the person tries again.
    Stopped,
    /// It lists no device for this card.
    NeedsDriver,
    /// Installed and usable: the switch (`AppSettings::nvidia_acceleration`).
    Ready,
}

// ---------------------------------------------------------------------------
// Library & projects
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum ProjectKind {
    Series,
    Movie,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum ContentKind {
    /// Live-action footage (faces).
    Live,
    /// 3D / CGI animation (Pixar-like films and series): faces like live
    /// action, but every character of one style looks alike to the face
    /// model, so their similarity bars are always fitted to the footage.
    Animation,
    /// Anime / 2D animation (heads, character art).
    Anime,
    Mixed,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub id: ProjectId,
    pub title: String,
    pub kind: ProjectKind,
    pub content: ContentKind,
    /// The `<Title>.autoscene` folder.
    pub dir: String,
    /// Unix milliseconds.
    pub created_at: i64,
    pub updated_at: i64,
    pub media_count: u32,
    pub ready_count: u32,
    pub duration_sec: f64,
    /// Poster image of the first episode, once indexed (720p, for artwork).
    pub poster: Option<String>,
    /// The same picture small (covers 480×270), for rail tiles and lists.
    pub poster_small: Option<String>,
    /// Its faces are found with Fast Scan (see [`AppSettings::fast_scan`]).
    #[serde(default)]
    pub fast_scan: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewProject {
    pub title: String,
    pub kind: ProjectKind,
    pub content: ContentKind,
    /// Video files (folders are expanded by `import_scan` beforehand).
    pub files: Vec<String>,
}

/// A video found by `import_scan`, with its parsed episode number.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportCandidate {
    pub path: String,
    pub file_name: String,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    /// `S01E03`, `E12`, or empty when no number was recognised.
    pub label: String,
    pub size_bytes: f64,
    /// Already part of the project (only when scanning into an existing one).
    pub duplicate: bool,
    /// Why it can't be imported: a path (its name or a folder's) that isn't
    /// valid Unicode, which media can't keep as the text they keep their
    /// path in. Such a file is listed, never added.
    pub refused: Option<Msg>,
}

// ---------------------------------------------------------------------------
// Media
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MediaStatus {
    /// Waiting for its ingest job.
    Queued,
    /// Probing and building the frame index.
    Indexing,
    Ready,
    Failed,
    /// The source file is gone or unreadable (moved drive, renamed).
    Missing,
}

/// Playback proxy state. Sources the webview plays frame-accurately as-is
/// never get one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ProxyState {
    /// Not decided yet (media not indexed).
    Unknown,
    NotNeeded,
    Pending,
    Building,
    Ready,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MediaItem {
    pub id: MediaId,
    pub path: String,
    pub file_name: String,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    /// `S01E03` / `E12` / empty.
    pub label: String,
    /// Order within the project (episode order).
    pub position: u32,
    pub size_bytes: f64,
    pub status: MediaStatus,
    pub error: Option<Msg>,
    pub info: Option<MediaInfo>,
    /// Decoded frame count (from the frame index).
    pub frames: Option<u32>,
    pub duration_sec: Option<f64>,
    /// Nominal frame rate used for timecodes.
    pub fps: Option<Rational>,
    pub vfr: bool,
    pub interlaced: bool,
    /// Audio stream ordinal used for playback and export (`None` = default track).
    pub audio_ordinal: Option<u32>,
    pub proxy: ProxyState,
    /// A still of the episode, 720p, for artwork (heroes, backdrops).
    pub poster: Option<String>,
    /// The same still small (covers 480×270), for cards, lists and tiles.
    pub poster_small: Option<String>,
    pub added_at: i64,
    /// Shot detection.
    pub shots: StageState,
    pub shots_error: Option<Msg>,
    /// Number of shots once detected.
    pub shot_count: Option<u32>,
    /// Face analysis (characters).
    pub faces: StageState,
    pub faces_error: Option<Msg>,
    /// Face tracks once found.
    pub face_count: Option<u32>,
    /// People followed by their bodies (when the setting is on).
    #[serde(default = "pending")]
    pub bodies: StageState,
    #[serde(default)]
    pub bodies_error: Option<Msg>,
}

fn pending() -> StageState {
    StageState::Pending
}

/// A stage of a media's analysis, to run again from scratch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisStage {
    /// Shots (and everything built on them: faces).
    Shots,
    /// Faces and characters only.
    Faces,
}

/// Progress of one analysis stage of a media.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum StageState {
    /// Not done yet (waiting for indexing or its job).
    Pending,
    Running,
    Ready,
    Failed,
    /// Stopped by the user; continues only when asked to.
    Stopped,
}

// ---------------------------------------------------------------------------
// Analysis
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SegmentKind {
    Shot,
    /// Cross-dissolve, wipe or another blend between two shots.
    Dissolve,
    /// From a shot to black or white.
    FadeOut,
    /// From black or white into a shot.
    FadeIn,
    /// Black (or, inside a fade, white) frames.
    Black,
    /// A brief flash (muzzle flash, lightning, anime impact frame) inside a shot.
    Flash,
}

/// Frames `[start, end)` of a media. Segments cover it without gaps; clips
/// come from `shot` segments, so they never start mid-transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub start: FrameIdx,
    pub end: FrameIdx,
    pub kind: SegmentKind,
    /// A shot under 3 frames, kept because both cuts around it are clear.
    pub tiny: bool,
}

/// Everything the player needs to map `<video>` time to source frames.
///
/// `player_time(f) = offset + rel(f)` where `rel(f) = f * step` for
/// constant-rate media, or `times[f]` otherwise (`times` has `frames + 1`
/// entries; the last is the end of the final frame).
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FrameMap {
    pub media_id: MediaId,
    /// File to play (the source itself, or its proxy).
    pub src: String,
    pub is_proxy: bool,
    pub frames: u32,
    pub fps: Rational,
    /// Timecodes use SMPTE drop-frame counting (29.97 / 59.94).
    pub drop_frame: bool,
    pub offset: f64,
    pub step: Option<f64>,
    pub times: Option<Vec<f64>>,
}

// ---------------------------------------------------------------------------
// Cast
// ---------------------------------------------------------------------------

/// How a face track got to its character.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TrackAssign {
    /// Nobody (an extra, a background face).
    None,
    /// Found by the analysis.
    Auto,
    /// Looks like the character, but not clearly: waits for a decision.
    Unsure,
    /// Decided by the user.
    Pinned,
}

/// A person the analysis found across the project's media.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Character {
    pub id: CharacterId,
    /// Given by the user; `None` until then.
    pub name: Option<String>,
    /// The name, or "Character N" (numbered in the order characters were
    /// found; a number is never reused).
    pub label: String,
    /// The N of "Character N": the order characters were found in. A number
    /// is never reused, and a named character keeps its number, so the UI can
    /// name an unnamed one in its own language.
    pub number: u32,
    pub hidden: bool,
    /// Face tracks (auto and pinned).
    pub tracks: u32,
    /// Shots the character is seen in.
    pub shots: u32,
    /// Media (episodes) the character is seen in.
    pub media: u32,
    /// Time the character is on screen: their face, or their body.
    pub screen_sec: f64,
    /// Portrait for big places (cards, heroes): 3:4 from the exact frame at
    /// full resolution once it is made, the face thumbnail until then
    /// (absolute path).
    pub cover: Option<String>,
    /// The same face as a small square thumbnail, for avatars and lists.
    pub cover_small: Option<String>,
    /// Tracks that look like the character and wait for a decision.
    pub unsure: u32,
    /// Faces of the character in media waiting for or in a new analysis:
    /// they come back when it is done.
    pub waiting: u32,
}

/// The cast of a project.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Cast {
    /// Most screen time first.
    pub characters: Vec<Character>,
    /// Tracks of nobody in particular.
    pub unassigned: u32,
    /// Tracks waiting for a decision.
    pub unsure: u32,
    /// Media whose faces are analysed / all media.
    pub media_done: u32,
    pub media_total: u32,
    /// The user's decisions about faces a later analysis didn't find again
    /// (another way of finding faces, another file): they wait, and are
    /// tried again at every analysis of their media.
    pub unplaced: u32,
}

/// An AniList series (a search result or one of a confirmed chain).
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AniListMedia {
    pub id: u32,
    /// Its best display title (English, else romaji, else native).
    pub title: String,
    /// TV, MOVIE, OVA… (AniList's format).
    pub format: Option<String>,
    pub episodes: Option<u32>,
    pub year: Option<u32>,
}

/// AniList suggests this cast character's name: their portrait matched the
/// character's look.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AniListSuggestion {
    pub character_id: CharacterId,
    pub anilist_id: u32,
    pub name: String,
    /// Cosine of the portrait to the character's look (the quality-weighted
    /// mean of its faces), 0..1.
    pub score: f32,
    /// The portrait file in the image cache (absolute path).
    pub image: Option<String>,
}

/// Two cast characters matched the same AniList one: `from` merges `into`
/// the other.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AniListMerge {
    pub from: CharacterId,
    pub into: CharacterId,
    pub anilist_id: u32,
}

/// What [`Project::anilist_suggest`] made of the confirmed series' cast.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AniListSuggestions {
    /// The cast revision the match was made on (`anilist_apply` checks it).
    pub rev: i64,
    pub names: Vec<AniListSuggestion>,
    pub merges: Vec<AniListMerge>,
    /// Whether names are given without asking (`anilist.auto`).
    pub auto: bool,
}

/// Names from AniList for a project's cast: the look-up the app does by
/// itself once the characters are found, and what it found.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AniListNames {
    /// The look-up is running.
    pub looking: bool,
    /// The series whose cast the names come from: its AniList id (the first
    /// of its chain) and title; `None` while no names wait.
    pub series_id: Option<u32>,
    pub series: Option<String>,
    /// Names waiting for the person's yes, for characters still shown and
    /// unnamed.
    pub names: Vec<AniListSuggestion>,
    /// Merges waiting for it, of characters both still shown.
    pub merges: Vec<AniListMerge>,
}

/// One face followed through one shot.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FaceTrackItem {
    pub id: u32,
    pub media_id: MediaId,
    pub start: FrameIdx,
    pub end: FrameIdx,
    /// 0..1: how clearly the face shows who it is.
    pub quality: f32,
    /// Median face height as a share of the frame height.
    pub face_height: f32,
    /// Square JPEG of the face (absolute path).
    pub thumb: String,
    /// Assigned character, or the suggestion when `assign` is `unsure`.
    pub character_id: Option<CharacterId>,
    pub assign: TrackAssign,
    /// Similarity behind an automatic assignment or suggestion.
    pub score: f32,
}

/// A shot a character is seen in.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CharacterShot {
    pub media_id: MediaId,
    pub start: FrameIdx,
    pub end: FrameIdx,
    /// JPEG of `cells` frames spread over the shot, side by side (absolute
    /// path); `None` for media analysed before shots had sprites.
    pub sprite: Option<String>,
    pub cells: u32,
    /// The character's clearest face in the shot (0 in a shot without it).
    pub track_id: u32,
    /// Its thumbnail (absolute path); the character's own in a shot
    /// without their face.
    pub face: String,
    /// The biggest the character's face is in the shot, as a share of the
    /// frame height (as big as their body suggests in a shot without it).
    pub face_height: f32,
    /// Their face doesn't show in the shot: their body does (turned away,
    /// too small), found by the face on it or by their clothes.
    #[serde(default)]
    pub faceless: bool,
    /// Their bodies there, in a shot without their face — every one, so
    /// the user's word on the shot is on all of them.
    #[serde(default)]
    pub bodies: Vec<u32>,
}

/// Shots of a character close together in time, in time order: each
/// starts at most 6 s after the one before it ends, with no fade or black
/// between. A shot far from the others is a group of one.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ShotGroup {
    pub shots: Vec<CharacterShot>,
}

/// Which tracks of the cast to list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum TrackFilter {
    /// Tracks of a character (auto and pinned).
    Character(CharacterId),
    /// Tracks suggested for a character, waiting for a decision.
    Unsure(CharacterId),
    /// Every track waiting for a decision.
    AllUnsure,
    /// Tracks of nobody.
    Unassigned,
}

/// A user decision about face tracks.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(tag = "op", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum CastEdit {
    /// These tracks are this character.
    Assign {
        tracks: Vec<u32>,
        character_id: CharacterId,
    },
    /// These tracks are someone new (returns the new character).
    NewCharacter {
        tracks: Vec<u32>,
        name: Option<String>,
    },
    /// These tracks are not this character.
    NotThem {
        tracks: Vec<u32>,
        character_id: CharacterId,
    },
    /// These tracks aren't faces at all.
    NotAFace {
        tracks: Vec<u32>,
    },
    /// Accept the suggestions of these unsure tracks.
    Confirm {
        tracks: Vec<u32>,
    },
    Rename {
        character_id: CharacterId,
        name: Option<String>,
    },
    SetHidden {
        character_id: CharacterId,
        hidden: bool,
    },
    /// Everyone of `from` is `into`.
    Merge {
        from: CharacterId,
        into: CharacterId,
    },
    SetCover {
        character_id: CharacterId,
        track: u32,
    },
    /// These bodies (people seen without their face) are this character.
    BodyAssign {
        bodies: Vec<u32>,
        character_id: CharacterId,
    },
    /// These bodies are not this character.
    BodyNotThem {
        bodies: Vec<u32>,
        character_id: CharacterId,
    },
    /// These bodies aren't people (a coat on a chair, a shadow).
    NotAPerson {
        bodies: Vec<u32>,
    },
}

impl CastEdit {
    /// The decision changes who a face is (so other faces may follow it), not
    /// just a name, a picture or what is shown.
    pub fn moves_faces(&self) -> bool {
        match self {
            Self::Assign { .. }
            | Self::NewCharacter { .. }
            | Self::NotThem { .. }
            | Self::NotAFace { .. }
            | Self::Confirm { .. }
            | Self::Merge { .. }
            | Self::BodyAssign { .. }
            | Self::BodyNotThem { .. }
            | Self::NotAPerson { .. } => true,
            Self::Rename { .. } | Self::SetHidden { .. } | Self::SetCover { .. } => false,
        }
    }
}

/// What became of a missing media pointed at a file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Relink {
    /// The same file: everything stays as it was.
    Same,
    /// Another copy of it: indexed again. Its analysis and the decisions on
    /// it stay when its frames are the same.
    Reindex,
    /// Another video (`reason` says how). Taken anyway with `force`: it is
    /// analysed again, the decisions on its faces are kept.
    Different { reason: Msg },
}

/// Missing media looked for in a folder.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RelinkSummary {
    /// Found as they were.
    pub same: u32,
    /// Found as another copy, being indexed again.
    pub reindex: u32,
    /// Still missing.
    pub missing: u32,
}

/// Where characters are on screen in one media.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Presence {
    pub character_id: CharacterId,
    /// The face track — or, with `body`, the body track.
    pub track_id: u32,
    pub start: FrameIdx,
    pub end: FrameIdx,
    /// On screen without their face showing: their body is.
    #[serde(default)]
    pub body: bool,
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ClipRequest {
    pub media_id: MediaId,
    /// First frame (inclusive).
    pub start: FrameIdx,
    /// End frame (exclusive).
    pub end: FrameIdx,
    /// Output file name without extension; generated when empty.
    pub name: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExportRequest {
    pub preset_id: String,
    /// Destination folder; the project's default export folder when `None`.
    pub out_dir: Option<String>,
    pub clips: Vec<ClipRequest>,
    /// What it is, for the export history ("Walter · scenepack").
    pub label: Option<String>,
    /// Join every clip into one file of this name (without extension), with
    /// a chapter per clip named after it; `None`: a file per clip.
    #[serde(default)]
    pub merge: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ExportStatus {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

/// The one file of an export that joins its clips.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MergedFile {
    /// File name without extension.
    pub name: String,
    /// Where it is, once it's made.
    pub path: Option<String>,
    /// Every frame of every clip was checked, and so was the joined file.
    pub verified: bool,
    pub bytes: Option<f64>,
    /// Why it couldn't be made.
    pub error: Option<Msg>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExportItem {
    pub id: i64,
    pub media_id: MediaId,
    pub start: FrameIdx,
    pub end: FrameIdx,
    pub name: String,
    pub status: ExportStatus,
    pub path: Option<String>,
    /// Every frame was checked against the source frame grid.
    pub verified: bool,
    pub encoder: Option<String>,
    pub bytes: Option<f64>,
    pub error: Option<Msg>,
}

/// What an export would make, worked out before it starts.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExportPlan {
    /// File names (without extension) in clip order, as the export names
    /// them (a name already in the destination folder gets `_2` then).
    pub names: Vec<String>,
    /// Roughly how big it would be with each preset.
    pub sizes: Vec<PresetSize>,
    /// The folder a file per clip goes into inside the destination (a
    /// scenepack's folder named after its character), as it will be named.
    pub folder: Option<String>,
    /// The one file's name (without extension) when the clips are joined
    /// into one; `names` are then its chapters.
    pub file: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PresetSize {
    pub preset_id: String,
    pub bytes: f64,
    /// Why this preset makes a file per clip of what was to be one file
    /// (Bit-Perfect can't join episodes whose pictures or sound differ).
    #[serde(default)]
    pub apart: Option<Msg>,
    /// The files' extension when it isn't the preset's container's
    /// (Bit-Perfect keeps some pictures as FFV1 in an MKV).
    #[serde(default)]
    pub ext: Option<String>,
    /// What people should know about this preset for these clips.
    #[serde(default)]
    pub note: Option<Msg>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExportSummary {
    pub id: ExportId,
    pub project_id: ProjectId,
    pub created_at: i64,
    pub preset: ExportPreset,
    pub out_dir: String,
    pub status: ExportStatus,
    pub items: Vec<ExportItem>,
    /// What it is ("Walter · scenepack"); `None` for single clips.
    pub label: Option<String>,
    /// The one file the clips are joined into; `None`: a file per clip.
    pub merged: Option<MergedFile>,
}

/// What an export's clips are saved as for other apps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ListFormat {
    /// A CMX 3600 EDL: editors (DaVinci Resolve, Premiere Pro, Avid) build
    /// the cut from the episodes with it.
    Edl,
    /// A cut list (CSV): the same timecodes, a row per clip, for reading.
    Csv,
}

/// An export's clips saved as a list (see [`ListFormat`]).
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SavedList {
    /// Where it was saved.
    pub path: String,
    /// Clips left out: their episode was removed from the project.
    pub left_out: u32,
}

// ---------------------------------------------------------------------------
// Packs
// ---------------------------------------------------------------------------

/// How big a face is in a shot, from its height in the frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ShotSize {
    ExtremeCloseUp,
    CloseUp,
    MediumCloseUp,
    Medium,
    Wide,
}

impl ShotSize {
    /// From the biggest face's height as a share of the frame height.
    pub fn of(face_height: f32) -> Self {
        match face_height {
            h if h >= 0.45 => Self::ExtremeCloseUp,
            h if h >= 0.25 => Self::CloseUp,
            h if h >= 0.15 => Self::MediumCloseUp,
            h if h >= 0.08 => Self::Medium,
            _ => Self::Wide,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PackOrder {
    /// As they are in the episodes.
    #[default]
    Episodes,
    /// The best first (clear, big, on screen the whole shot).
    Best,
}

/// Which of a character's shots go into their scenepack. (Filters saved by
/// an older version load with the defaults filled in: `as_project::pack`.)
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PackSpec {
    /// Shots shorter than this many seconds are left out.
    pub min_sec: Option<f64>,
    /// Shots longer than this many seconds are left out.
    pub max_sec: Option<f64>,
    /// Only shots of these sizes (empty: any size).
    pub sizes: Vec<ShotSize>,
    /// Only shots with no one else in them.
    pub solo: bool,
    /// Leave out flash cuts (shots under 3 frames).
    pub skip_tiny: bool,
    /// Leave out shots where the character's face doesn't show (found by
    /// their body).
    #[serde(default)]
    pub skip_faceless: bool,
    /// Only these episodes (empty: all).
    pub media: Vec<MediaId>,
    pub order: PackOrder,
    /// At most this many shots: the best ones.
    pub limit: Option<u32>,
    /// Extra frames before and after each shot (within the episode).
    pub handles: u32,
    /// Plugins' shot filters turned on (`<pluginId>/<filterId>`): each
    /// leaves out the shots its plugin says no to. One of a plugin that
    /// isn't active is ignored.
    #[serde(default)]
    pub plugin_filters: Vec<String>,
}

impl Default for PackSpec {
    fn default() -> Self {
        Self {
            min_sec: None,
            max_sec: None,
            sizes: Vec::new(),
            solo: false,
            skip_tiny: true,
            skip_faceless: false,
            media: Vec::new(),
            order: PackOrder::Episodes,
            limit: None,
            handles: 0,
            plugin_filters: Vec::new(),
        }
    }
}

/// Why a shot is left out of a pack.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PackReason {
    /// The episode's file can't be found (or isn't indexed yet).
    Missing,
    Episode,
    Tiny,
    Short,
    Long,
    Size,
    NotSolo,
    /// The character's face doesn't show in it.
    Faceless,
    /// Past the limit.
    Limit,
    /// Dropped by hand.
    Removed,
    /// A plugin's shot filter said no ([`PackClip::filter`] names which).
    Plugin,
}

/// A shot of a pack, picked or not.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PackClip {
    /// The shot (its picture, face and range): picks by hand stick to its media and first frame.
    pub shot: CharacterShot,
    /// The clip: the shot and its handles, within the episode.
    pub start: FrameIdx,
    pub end: FrameIdx,
    pub size: ShotSize,
    /// 0..1: how clearly the character's face shows in it.
    pub quality: f32,
    /// Share of the shot the character is on screen.
    pub coverage: f32,
    /// Other characters on screen in the shot.
    pub others: u32,
    /// 0..1: how good a clip it makes (what "best first" sorts by).
    pub score: f32,
    pub included: bool,
    /// What the filters alone would say.
    pub by_rules: bool,
    /// Why it's left out.
    pub reason: Option<PackReason>,
    /// Picked (`true`) or dropped (`false`) by hand.
    pub manual: Option<bool>,
    /// The plugin filter that leaves it out (an index into
    /// [`PackPreview::filters`]), picked back in by hand or not.
    pub filter: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PackTotals {
    pub clips: u32,
    pub frames: f64,
    pub seconds: f64,
}

/// A pack as it stands: every shot of the character, the ones it takes
/// marked, in the pack's order.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PackPreview {
    pub clips: Vec<PackClip>,
    pub totals: PackTotals,
    /// Names exactly the clips it takes: an export of it is refused once the
    /// pack changed, so what is exported is what was shown.
    pub stamp: String,
    /// Names its clips, in order, as they are apart from their [`PackChoice`]s.
    pub shape: String,
    /// The plugins' shot filters it can show: every one of an active
    /// plugin, and those its spec turns on whose plugin isn't (`off`).
    pub filters: Vec<PackFilter>,
    /// Shots a filter turned on hasn't judged yet (with its settings as
    /// they are): they are in the pack until it has.
    pub unchecked: u32,
}

/// A plugin's shot filter, as the scenepack builder shows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PackFilter {
    /// `<pluginId>/<filterId>`, as [`PackSpec::plugin_filters`] names it.
    pub key: String,
    pub title: String,
    pub description: Option<String>,
    /// What a shot it leaves out says.
    pub reason: String,
    /// Its plugin's name.
    pub plugin: String,
    /// The character's shots it has judged with its settings as they are,
    /// of those it can (in episodes whose files are there).
    pub checked: u32,
    pub total: u32,
    pub state: PackFilterState,
}

/// What a plugin's shot filter is doing for a character.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum PackFilterState {
    /// Nothing runs: what it judged is in the pack.
    Ready,
    /// A check of the character's shots waits or runs.
    Checking,
    /// Its last check failed or was stopped: it starts again on Try again,
    /// or once its settings change.
    Failed,
    /// Its plugin isn't active: the pack ignores it.
    Off,
    /// No plugin has it now (uninstalled, or an update took it away): the
    /// pack ignores it, and all that is left to do is turn it off.
    Gone,
}

/// Whether a shot is in its pack, and why: the part of a [`PackClip`] that
/// picking shots changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PackChoice {
    pub included: bool,
    pub by_rules: bool,
    pub reason: Option<PackReason>,
    pub manual: Option<bool>,
    /// See [`PackClip::filter`].
    pub filter: Option<u32>,
}

/// A pack after shots were picked: every clip's choice, in the pack's
/// order — the rest of the pack is its preview's, when the `shape` is.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PackChoices {
    pub choices: Vec<PackChoice>,
    pub totals: PackTotals,
    pub stamp: String,
    pub shape: String,
}

/// How a pack's files are named and where they go.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PackNaming {
    /// Each clip's name (its file's, or its chapter's in one file), with
    /// `{series}`, `{character}`, `{episode}`, `{index}`, `{tc}`, `{frame}`.
    pub template: String,
    /// Into a folder named after the character (a file per clip only).
    pub folder: bool,
    /// Every clip in one file, with a chapter each.
    pub merge: bool,
    /// The one file's name, with `{series}` and `{character}`.
    pub file: String,
}

impl Default for PackNaming {
    fn default() -> Self {
        Self {
            template: "{series} {character} {episode} {tc}".into(),
            folder: true,
            merge: true,
            file: "{series} {character} scenepack".into(),
        }
    }
}

/// A character's pack settings, as last used.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PackState {
    pub spec: PackSpec,
    pub naming: PackNaming,
}

/// A shot of a pack, by its media and first frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PackKey {
    pub media_id: MediaId,
    pub shot: FrameIdx,
}

/// Shots of a pack picked (`include: true`), dropped (`false`) or handed
/// back to the filters (`null`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PackPicks {
    pub shots: Vec<PackKey>,
    pub include: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PackExportRequest {
    pub spec: PackSpec,
    pub naming: PackNaming,
    pub preset_id: String,
    /// The folder the files (or the character's folder) go to; the
    /// project's default export folder when `None`.
    pub out_dir: Option<String>,
    /// The preview's stamp: the export is refused if the pack changed since.
    pub stamp: String,
}

// ---------------------------------------------------------------------------
// Jobs
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Ingest,
    Proxy,
    Export,
    /// AI analysis of a media: shots, then faces and characters.
    Analyze,
    /// Posters of a project made again at the current sizes.
    Posters,
    /// Sharp portraits of a project's characters, cut from the exact frames.
    Portraits,
    /// A project's characters grouped again: the grouping got better since
    /// they were grouped (the user's decisions stay).
    Regroup,
    /// A plugin's action (an export action or a palette command).
    Plugin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

impl JobState {
    pub fn is_finished(self) -> bool {
        matches!(self, Self::Done | Self::Failed | Self::Cancelled)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct JobInfo {
    pub id: JobId,
    pub kind: JobKind,
    pub title: Msg,
    pub subtitle: Option<String>,
    pub project_id: Option<ProjectId>,
    pub media_id: Option<MediaId>,
    pub export_id: Option<ExportId>,
    pub state: JobState,
    /// 0..1, `None` while indeterminate.
    pub progress: Option<f64>,
    pub stage: Option<Msg>,
    pub eta_ms: Option<i64>,
    /// While the job reads a media's frames one by one (indexing): how
    /// many so far, of about how many, and how fast.
    #[serde(default)]
    pub frames: Option<FrameCount>,
    pub error: Option<Msg>,
    /// What a finished job had to say: a plugin's result, in its own words,
    /// or the file its pack action saved.
    #[serde(default)]
    pub message: Option<Msg>,
    pub created_at: i64,
    pub finished_at: Option<i64>,
    /// Increases with every change to any job. Events can arrive out of
    /// order; the UI keeps the copy with the highest revision.
    pub rev: u64,
}

/// Throttled progress for a running job (≤ 4 Hz per job).
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct JobProgress {
    pub id: JobId,
    /// Revision of the job this progress belongs to (see [`JobInfo::rev`]).
    pub rev: u64,
    pub progress: Option<f64>,
    pub stage: Option<Msg>,
    pub eta_ms: Option<i64>,
    /// See [`JobInfo::frames`].
    #[serde(default)]
    pub frames: Option<FrameCount>,
}

/// Unix time in milliseconds.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_without_a_message_has_none_and_a_plugins_reads_back() {
        let old = format!(
            r#"{{"id":"{}","kind":"export","title":{{"key":"job.exporting","args":{{"count":2}},"text":"Exporting 2 clips"}},"subtitle":null,"projectId":null,"mediaId":null,"exportId":null,"state":"done","progress":1.0,"stage":null,"etaMs":null,"error":null,"createdAt":1,"finishedAt":2,"rev":3}}"#,
            JobId::new()
        );
        let job: JobInfo = serde_json::from_str(&old).unwrap();
        assert_eq!((job.kind, job.message), (JobKind::Export, None));
        let plugin = old
            .replace(r#""kind":"export""#, r#""kind":"plugin""#)
            .replace(r#""error":null"#, r#""error":null,"message":{"key":"","text":"Posted 12 clips."}"#);
        let job: JobInfo = serde_json::from_str(&plugin).unwrap();
        assert_eq!((job.kind, job.message), (JobKind::Plugin, Some(Msg::plain("Posted 12 clips."))));
    }
}

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use as_api::{
    AppSettings, ContentKind, ImportCandidate, NewProject, ProjectKind, ProjectSummary, ScanChoice, ScanDefaults,
    Shortcuts, now_ms,
};
use as_core::{AppPaths, CancelToken, MediaId, ProjectId};
use as_db::{LibraryDb, NewMedia, ProjectDb};
use as_infer::computer::{self, Choice, Computer, Machine};
use parking_lot::Mutex;

use crate::anilist::{AniList, Names, PictureKey};
use crate::fandom::Fandom;
use crate::naming::{episode_cmp, is_video, parse_episode, safe_component};
use crate::project::{PROJECT_DB, Project};
use crate::scan;
use crate::tvmaze::TvMaze;
use crate::{ProjectError, ProjectResult};

const KEY_LIBRARY_ROOT: &str = "library_root";
const KEY_EXPORT_PRESET: &str = "export_preset_id";
const DEFAULT_EXPORT_PRESET: &str = "prores-hq";
const KEY_AUDIO_LANGUAGE: &str = "audio_language";
const KEY_FIND_BODIES: &str = "find_bodies";
/// The scan new projects start with, as the person chose it: `fast` (On) or
/// `full` (Off); unset is Automatic, which follows this computer.
const KEY_SCAN_DEFAULT: &str = "scan_default";
/// Fast Scan's default before it followed the computer: "1" or "0", written
/// by every save of the settings. Read once, into [`KEY_SCAN_DEFAULT`].
const KEY_LEGACY_FAST_SCAN: &str = "fast_scan";
/// "done" once the first-start notice about Fast Scan is done for this
/// library: "Don't show this again", or a default met in Settings. One flag
/// for the library, whatever the graphics become: every class that gets Fast
/// Scan gets the same default, so a second notice would say nothing new.
const KEY_SCAN_NOTICE: &str = "scan_notice";
const NOTICE_DONE: &str = "done";
const KEY_ANILIST: &str = "anilist";
/// The keyboard shortcuts the person changed, as JSON.
const KEY_SHORTCUTS: &str = "shortcuts";
const KEY_ALLOW_UNREVIEWED: &str = "allow_unreviewed_plugins";
/// The graphics the analysis runs on: unset Automatic, `cpu`, or a card's
/// key (`as_infer::hardware::key`).
const KEY_GPU: &str = "gpu";
const KEY_CATALOG_CHECKS: &str = "plugin_catalog_checks";
/// `0` once NVIDIA's engine is switched off; it is on unless it is.
const KEY_NVIDIA_ACCELERATION: &str = "nvidia_acceleration";
const KEY_DISCORD_RPC: &str = "discord_rpc";
/// Most series and films people cut are watched in their original English
/// track; dubbed tracks are often flagged "default" too, so a language
/// preference beats the file's flag.
const DEFAULT_AUDIO_LANGUAGE: &str = "eng";
/// How deep folder imports look (`Show/Season 1/Disc 2/file.mkv`).
const SCAN_DEPTH: usize = 4;

/// The project list, settings, and the cache of open projects.
pub struct Library {
    db: Arc<LibraryDb>,
    open: Mutex<HashMap<ProjectId, Arc<Project>>>,
    /// One lock per project that was opened here: a project is opened once
    /// at a time, while other projects open, and open ones are handed out,
    /// meanwhile (see [`Library::project`]).
    opening: Mutex<HashMap<ProjectId, Arc<Mutex<()>>>>,
    /// Whether people are followed by their bodies too (a setting every
    /// open project reads).
    find_bodies: Arc<AtomicBool>,
    /// Whether projects may ask AniList for names (checked between images).
    anilist_on: AtomicBool,
    /// Cancelled when the opt-in goes off: whatever asks AniList stops (see
    /// [`Library::anilist_token`]).
    anilist_stop: Mutex<CancelToken>,
    anilist: AniList,
    /// Names for animated series and films, behind the same opt-in.
    tvmaze: TvMaze,
    fandom: Fandom,
    /// Pictures of projects' characters embedded this session, by file
    /// (see [`Project::look_pictures`]).
    look_pictures: Mutex<HashMap<PictureKey, Vec<f32>>>,
    /// What this computer analyses on: `probe`'s answer, read once, when it
    /// is first needed.
    machine: OnceLock<Machine>,
    probe: fn() -> Machine,
    /// The graphics the person chose ([`KEY_GPU`]).
    gpu: Mutex<String>,
    /// Whether NVIDIA's engine is switched on ([`KEY_NVIDIA_ACCELERATION`]).
    nvidia_acceleration: AtomicBool,
}

impl Library {
    /// The library at `paths`, knowing no computer: its graphics are of
    /// unknown kind, so new projects start with the full scan unless the
    /// person chose Fast Scan. For tests, examples and tools; the app opens
    /// it with [`Library::open_with`].
    pub fn open(paths: &AppPaths) -> ProjectResult<Self> {
        fn no_computer() -> Machine {
            Machine::Windows(None)
        }
        Self::open_with(paths, no_computer)
    }

    /// The library at `paths`, whose new projects follow the computer
    /// `probe` reads (`as_infer::computer::probe`), once, when first needed.
    pub fn open_with(paths: &AppPaths, probe: fn() -> Machine) -> ProjectResult<Self> {
        std::fs::create_dir_all(&paths.data)?;
        let db = Arc::new(LibraryDb::open(&paths.library_db())?);
        migrate_fast_scan(&db)?;
        let find_bodies = Arc::new(AtomicBool::new(db.setting(KEY_FIND_BODIES)?.as_deref() == Some("1")));
        let anilist_on = AtomicBool::new(db.setting(KEY_ANILIST)?.as_deref() == Some("1"));
        let gpu = Mutex::new(db.setting(KEY_GPU)?.unwrap_or_default());
        let nvidia_acceleration = AtomicBool::new(db.setting(KEY_NVIDIA_ACCELERATION)?.as_deref() != Some("0"));
        let anilist = AniList::new(paths.anilist());
        let tvmaze = TvMaze::new(paths.tvmaze());
        let fandom = Fandom::new(paths.fandom());
        Ok(Self {
            db,
            open: Mutex::default(),
            opening: Mutex::default(),
            find_bodies,
            anilist_on,
            anilist_stop: Mutex::default(),
            anilist,
            tvmaze,
            fandom,
            look_pictures: Mutex::default(),
            machine: OnceLock::new(),
            probe,
            gpu,
            nvidia_acceleration,
        })
    }

    /// This library asking `anilist` instead (recorded answers).
    #[cfg(test)]
    pub(crate) fn with_anilist(self, anilist: AniList) -> Self {
        Self { anilist, ..self }
    }

    /// This library asking `tvmaze` instead (recorded answers).
    #[cfg(test)]
    pub(crate) fn with_tvmaze(self, tvmaze: TvMaze) -> Self {
        Self { tvmaze, ..self }
    }

    /// This library asking `fandom` instead (recorded answers).
    #[cfg(test)]
    pub(crate) fn with_fandom(self, fandom: Fandom) -> Self {
        Self { fandom, ..self }
    }

    /// Whether projects may ask AniList (anime), TVmaze (animated series)
    /// and Wikidata with Fandom (animated films) for names.
    pub fn names_from_anilist(&self) -> bool {
        self.anilist_on.load(Ordering::Relaxed)
    }

    /// The token a piece of AniList work runs under: cancelled once the
    /// opt-in goes off, even when it goes on again meanwhile.
    pub fn anilist_token(&self) -> CancelToken {
        self.anilist_stop.lock().child()
    }

    pub(crate) fn anilist(&self) -> &AniList {
        &self.anilist
    }

    pub(crate) fn tvmaze(&self) -> &TvMaze {
        &self.tvmaze
    }

    pub(crate) fn fandom(&self) -> &Fandom {
        &self.fandom
    }

    pub(crate) fn look_pictures(&self) -> &Mutex<HashMap<PictureKey, Vec<f32>>> {
        &self.look_pictures
    }

    /// Whether characters are looked for by their bodies too.
    pub fn finds_bodies(&self) -> bool {
        self.find_bodies.load(Ordering::Relaxed)
    }

    /// The library's database, for the plugin registry, whose rows sit
    /// beside the settings.
    pub fn shared_db(&self) -> Arc<LibraryDb> {
        Arc::clone(&self.db)
    }

    // -- settings -----------------------------------------------------------

    pub fn settings(&self) -> ProjectResult<AppSettings> {
        Ok(AppSettings {
            library_root: self.library_root()?.display().to_string(),
            export_preset_id: self.db.setting(KEY_EXPORT_PRESET)?.unwrap_or_else(|| DEFAULT_EXPORT_PRESET.into()),
            audio_language: self.audio_language()?,
            find_bodies: self.finds_bodies(),
            fast_scan: self.fast_scan()?,
            anilist: self.names_from_anilist(),
            shortcuts: self.shortcuts()?,
            allow_unreviewed_plugins: self.allows_unreviewed_plugins()?,
            gpu: self.gpu(),
            plugin_catalog_checks: self.db.setting(KEY_CATALOG_CHECKS)?.as_deref() != Some("0"),
            nvidia_acceleration: self.nvidia_acceleration(),
            discord_rpc: self.discord_rpc()?,
        })
    }

    /// Whether analysis may use NVIDIA's engine.
    pub fn nvidia_acceleration(&self) -> bool {
        self.nvidia_acceleration.load(Ordering::Relaxed)
    }

    /// Switch NVIDIA's engine on or off. On stores nothing.
    pub fn set_nvidia_acceleration(&self, on: bool) -> ProjectResult<()> {
        if on != self.nvidia_acceleration() {
            self.db.write_settings(&[(KEY_NVIDIA_ACCELERATION, (!on).then_some("0"))])?;
            self.nvidia_acceleration.store(on, Ordering::Relaxed);
        }
        Ok(())
    }

    /// The graphics the analysis runs on, as Settings stores them: `""`
    /// Automatic, `"cpu"`, or a card's key.
    pub fn gpu(&self) -> String {
        self.gpu.lock().clone()
    }

    /// Whether plugins AutoScene has not reviewed may be installed and run.
    pub fn allows_unreviewed_plugins(&self) -> ProjectResult<bool> {
        Ok(self.db.setting(KEY_ALLOW_UNREVIEWED)?.as_deref() == Some("1"))
    }

    /// What the person lets Discord show. Off, also for what can't be read.
    pub fn discord_rpc(&self) -> ProjectResult<as_api::DiscordRpcSettings> {
        let saved = self.db.setting(KEY_DISCORD_RPC)?;
        Ok(saved.and_then(|json| serde_json::from_str(&json).ok()).unwrap_or_default())
    }

    /// Lets plugins AutoScene has not reviewed be installed and run, or
    /// not. Only here: [`Library::update_settings`] keeps it as it is, so a
    /// page saving a copy of the settings made before can't turn it back.
    pub fn set_allows_unreviewed_plugins(&self, on: bool) -> ProjectResult<()> {
        Ok(self.db.set_setting(KEY_ALLOW_UNREVIEWED, if on { "1" } else { "0" })?)
    }

    /// The keys the person chose; AutoScene's own when what is stored can't
    /// be read (an older or a newer app's).
    fn shortcuts(&self) -> ProjectResult<Shortcuts> {
        let Some(json) = self.db.setting(KEY_SHORTCUTS)? else { return Ok(Shortcuts::default()) };
        Ok(serde_json::from_str(&json).unwrap_or_else(|e| {
            tracing::warn!("ignoring the stored shortcuts: {e}");
            Shortcuts::default()
        }))
    }

    // -- Fast Scan's default for new projects ---------------------------------

    fn machine(&self) -> &Machine {
        self.machine.get_or_init(self.probe)
    }

    /// What the analysis runs on here, and whether those graphics are built
    /// for AI work. The adapters are read once; the device the person chose
    /// ([`Library::gpu`]) is applied on every call, so Fast Scan's default
    /// follows the card the analysis uses.
    pub fn computer(&self) -> Computer {
        computer::choose(self.machine(), &Choice::from_setting(&self.gpu()))
    }

    /// What the person chose for new projects: Automatic unless they chose
    /// On or Off (a value a newer app stored reads as Automatic).
    fn scan_choice(&self) -> ProjectResult<ScanChoice> {
        Ok(match self.db.setting(KEY_SCAN_DEFAULT)?.as_deref() {
            Some("fast") => ScanChoice::Fast,
            Some("full") => ScanChoice::Full,
            _ => ScanChoice::Auto,
        })
    }

    /// The scan new projects of each footage start with, and why, and
    /// whether the first-start notice is due.
    pub fn scan_defaults(&self) -> ProjectResult<ScanDefaults> {
        let notice_done = self.db.setting(KEY_SCAN_NOTICE)?.as_deref() == Some(NOTICE_DONE);
        Ok(scan::defaults(&self.computer(), self.scan_choice()?, self.finds_bodies(), notice_done))
    }

    /// Chooses the scan new projects start with: On or Off for every footage,
    /// or Automatic, which follows this computer (the choice is deleted).
    /// Only here: [`Library::update_settings`] keeps it as it is, so a page
    /// saving a copy of the settings made before can't change it. The person
    /// met the setting, so the first-start notice is done too.
    pub fn set_scan_default(&self, choice: ScanChoice) -> ProjectResult<ScanDefaults> {
        let value = match choice {
            ScanChoice::Auto => None,
            ScanChoice::Fast => Some("fast"),
            ScanChoice::Full => Some("full"),
        };
        self.db.write_settings(&[(KEY_SCAN_DEFAULT, value), (KEY_SCAN_NOTICE, Some(NOTICE_DONE))])?;
        self.scan_defaults()
    }

    /// "Don't show this again" on the first-start notice: it isn't due again
    /// in this library, whatever the graphics become.
    pub fn scan_notice_done(&self) -> ProjectResult<()> {
        Ok(self.db.set_setting(KEY_SCAN_NOTICE, NOTICE_DONE)?)
    }

    /// Whether new live-action projects find their faces with Fast Scan.
    pub fn fast_scan(&self) -> ProjectResult<bool> {
        Ok(scan::advice(&self.scan_defaults()?, ContentKind::Live))
    }

    pub fn update_settings(&self, s: &AppSettings) -> ProjectResult<AppSettings> {
        let root = PathBuf::from(s.library_root.trim());
        if !root.is_absolute() {
            return Err(ProjectError::Invalid("Choose a full folder path for the library.".into()));
        }
        // The folder in use may be on a drive that isn't there right now:
        // other settings (keys, switches) are kept all the same.
        let unchanged = root == self.library_root()?;
        if let Err(e) = std::fs::create_dir_all(&root)
            && !unchanged
        {
            return Err(ProjectError::Invalid(format!("Can't use {} as the library: {e}", root.display())));
        }
        self.db.set_setting(KEY_LIBRARY_ROOT, &root.display().to_string())?;
        if as_export::builtin_presets().iter().any(|p| p.id == s.export_preset_id) {
            self.db.set_setting(KEY_EXPORT_PRESET, &s.export_preset_id)?;
        }
        let language = as_media::lang::canonical(&s.audio_language).unwrap_or_default();
        self.db.set_setting(KEY_AUDIO_LANGUAGE, &language)?;
        self.db.set_setting(KEY_FIND_BODIES, if s.find_bodies { "1" } else { "0" })?;
        let bodies_changed = self.find_bodies.swap(s.find_bodies, Ordering::Relaxed) != s.find_bodies;
        // The switch was changed in Settings, where Fast Scan's row says why
        // it is on here: the person met the setting, so the notice is done.
        if bodies_changed && self.scan_defaults()?.notice {
            self.scan_notice_done()?;
        }
        // The graphics: Fast Scan's default follows the card chosen, so a
        // change met in Settings meets its notice too, as above.
        let gpu = s.gpu.trim().to_owned();
        if gpu != self.gpu() {
            match gpu.as_str() {
                "" => self.db.write_settings(&[(KEY_GPU, None)])?,
                chosen => self.db.set_setting(KEY_GPU, chosen)?,
            }
            *self.gpu.lock() = gpu;
            if self.scan_defaults()?.notice {
                self.scan_notice_done()?;
            }
        }
        self.set_nvidia_acceleration(s.nvidia_acceleration)?;
        self.db.set_setting(KEY_ANILIST, if s.anilist { "1" } else { "0" })?;
        self.anilist_on.store(s.anilist, Ordering::Relaxed);
        let discord = serde_json::to_string(&s.discord_rpc).map_err(|e| ProjectError::Invalid(e.to_string()))?;
        self.db.set_setting(KEY_DISCORD_RPC, &discord)?;
        // `allow_unreviewed_plugins` stays: see `set_allows_unreviewed_plugins`.
        // Fast Scan's default too: see `set_scan_default`.
        self.db.set_setting(KEY_CATALOG_CHECKS, if s.plugin_catalog_checks { "1" } else { "0" })?;
        // Keys a newer app stored, that this one can't read, stay unless
        // they were changed here.
        let unreadable =
            self.db.setting(KEY_SHORTCUTS)?.is_some_and(|json| serde_json::from_str::<Shortcuts>(&json).is_err());
        if !unreadable || s.shortcuts != Shortcuts::default() {
            let shortcuts = serde_json::to_string(&s.shortcuts).expect("shortcuts serialize");
            self.db.set_setting(KEY_SHORTCUTS, &shortcuts)?;
        }
        // Off: whatever asks for names stops, and nothing from where they
        // come from stays (also what an earlier attempt left).
        if !s.anilist {
            std::mem::take(&mut *self.anilist_stop.lock()).cancel();
            self.anilist.forget()?;
            self.tvmaze.forget()?;
            self.fandom.forget()?;
            self.look_pictures.lock().clear();
        }
        self.settings()
    }

    /// Audio language chosen automatically for new episodes (`""` = file default).
    pub fn audio_language(&self) -> ProjectResult<String> {
        Ok(self.db.setting(KEY_AUDIO_LANGUAGE)?.unwrap_or_else(|| DEFAULT_AUDIO_LANGUAGE.into()))
    }

    /// Where new project folders go.
    pub fn library_root(&self) -> ProjectResult<PathBuf> {
        Ok(self.db.setting(KEY_LIBRARY_ROOT)?.map(PathBuf::from).unwrap_or_else(AppPaths::default_library_root))
    }

    /// Default destination for a project's exports: `<library>/Exports/<Title>`.
    pub fn default_export_dir(&self, project: &ProjectSummary) -> ProjectResult<PathBuf> {
        Ok(self.library_root()?.join("Exports").join(safe_component(&project.title, " ", 80)))
    }

    // -- projects -----------------------------------------------------------

    pub fn projects(&self) -> ProjectResult<Vec<ProjectSummary>> {
        Ok(self.db.projects()?)
    }

    pub fn summary(&self, id: ProjectId) -> ProjectResult<ProjectSummary> {
        self.db.project(id)?.ok_or_else(|| ProjectError::NotFound("This project no longer exists.".into()))
    }

    /// Create the project folder and database and add `req.files` (status
    /// `queued`), with the scan its footage starts with here. Returns the new
    /// project and the media to ingest.
    pub fn create_project(&self, req: &NewProject) -> ProjectResult<(ProjectSummary, Arc<Project>, Vec<MediaId>)> {
        self.create_project_with(req, None)
    }

    /// [`Library::create_project`] with Fast Scan on or off, as the New
    /// Project dialog's switch shows it; `None` takes the footage's default.
    pub fn create_project_with(
        &self,
        req: &NewProject,
        fast: Option<bool>,
    ) -> ProjectResult<(ProjectSummary, Arc<Project>, Vec<MediaId>)> {
        let title = req.title.trim();
        if title.is_empty() {
            return Err(ProjectError::Invalid("Give the project a name.".into()));
        }
        // Before anything is made: a file that can't be imported stops it.
        let found = importable(&req.files, req.kind)?;
        let fast = match fast {
            Some(fast) => fast,
            None => scan::advice(&self.scan_defaults()?, req.content),
        };
        let root = self.library_root()?;
        std::fs::create_dir_all(&root)?;
        let dir = unique_dir(&root, &safe_component(title, " ", 80));
        std::fs::create_dir_all(&dir)?;

        let now = now_ms();
        let summary = ProjectSummary {
            id: ProjectId::new(),
            title: title.to_string(),
            kind: req.kind,
            content: req.content,
            dir: dir.display().to_string(),
            created_at: now,
            updated_at: now,
            media_count: 0,
            ready_count: 0,
            duration_sec: 0.0,
            poster: None,
            poster_small: None,
            fast_scan: fast,
        };
        let project = Arc::new(Project::create(&summary, self.find_bodies.clone())?);
        self.db.insert_project(&summary)?;
        self.open.lock().insert(summary.id, project.clone());
        let added = self.add_found(&project, found)?;
        Ok((self.refresh_stats(&project)?, project, added))
    }

    /// Open (or return the cached) project.
    ///
    /// A project is opened once at a time: `Project::open` closes out stale
    /// exports and deletes unused preview files, which a second, racing open
    /// would otherwise do to work the first instance had already started.
    /// Only that project waits for it, though: opening one can take a while
    /// (a database upgrade, its faces placed again), and every other project,
    /// open or not, is handed out meanwhile.
    pub fn project(&self, id: ProjectId) -> ProjectResult<Arc<Project>> {
        if let Some(p) = self.open.lock().get(&id) {
            return Ok(p.clone());
        }
        let one = self.opening.lock().entry(id).or_default().clone();
        let _one_at_a_time = one.lock();
        // Opened while this waited.
        if let Some(p) = self.open.lock().get(&id) {
            return Ok(p.clone());
        }
        let summary = self.summary(id)?;
        let dir = PathBuf::from(&summary.dir);
        if !dir.join(PROJECT_DB).is_file() {
            return Err(ProjectError::NotFound(format!(
                "The project folder {} is missing. Was it moved or deleted?",
                dir.display()
            )));
        }
        let project = Arc::new(Project::open(id, dir, self.find_bodies.clone())?);
        let mut open = self.open.lock();
        // Removed from the library while it opened: not kept open.
        if self.db.project(id)?.is_none() {
            return Err(ProjectError::NotFound("This project no longer exists.".into()));
        }
        open.insert(id, project.clone());
        Ok(project)
    }

    pub fn rename_project(&self, id: ProjectId, title: &str) -> ProjectResult<ProjectSummary> {
        let title = title.trim();
        if title.is_empty() {
            return Err(ProjectError::Invalid("Give the project a name.".into()));
        }
        self.db.rename_project(id, title)?;
        if let Ok(p) = self.project(id) {
            p.set_title(title)?;
        }
        self.summary(id)
    }

    /// Say what a project's footage is (see [`Project::set_content`]).
    /// Returns the summary and whether the cast must be grouped again (also
    /// when an earlier attempt already changed it).
    pub fn set_project_content(&self, id: ProjectId, content: ContentKind) -> ProjectResult<(ProjectSummary, bool)> {
        let project = self.project(id)?;
        let before = self.summary(id)?.content;
        self.db.set_project_content(id, content)?;
        if let Err(e) = project.set_content(content) {
            // The library shows what the project's cast is grouped for.
            let _ = self.db.set_project_content(id, before);
            return Err(e);
        }
        Ok((self.summary(id)?, project.grouped_by_older_version()?))
    }

    /// Say whether a project's faces are found with Fast Scan (see
    /// [`Project::set_fast_scan`]). Returns the summary and whether its
    /// faces are to be found again.
    pub fn set_project_fast_scan(&self, id: ProjectId, on: bool) -> ProjectResult<(ProjectSummary, bool)> {
        let project = self.project(id)?;
        // The library first: it is what shows, and the project's own word
        // is the one its faces follow.
        let before = self.summary(id)?.fast_scan;
        self.db.set_project_fast_scan(id, on)?;
        let changed = project.set_fast_scan(on).inspect_err(|_| {
            let _ = self.db.set_project_fast_scan(id, before);
        })?;
        Ok((self.summary(id)?, changed))
    }

    /// Remove a project from the library. With `delete_files`, also delete its
    /// `.autoscene` folder (database, posters, proxies). Source media and
    /// exported clips are never touched. Stop the project's jobs first.
    pub fn delete_project(&self, id: ProjectId, delete_files: bool) -> ProjectResult<()> {
        let summary = self.summary(id)?;
        // The row goes while the cache is locked: a lookup can't find the
        // project in between and open its database again (which would keep
        // it open for the rest of the session).
        let project = {
            let mut open = self.open.lock();
            self.db.delete_project(id)?;
            open.remove(&id)
        };
        if delete_files {
            // Whatever still holds the project (a command finishing, work
            // letting go) closes its database before the folder goes.
            if let Some(project) = project {
                let held = Arc::downgrade(&project);
                drop(project);
                let until = std::time::Instant::now() + RELEASE_WAIT;
                while held.strong_count() > 0 && std::time::Instant::now() < until {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            }
            let dir = PathBuf::from(&summary.dir);
            // Only ever delete what is recognisably this project's folder.
            if is_project_dir(&dir, id) {
                remove_dir_with_retry(&dir).map_err(|e| {
                    ProjectError::Io(std::io::Error::new(
                        e.kind(),
                        format!("The project was removed, but its folder couldn't be deleted: {e}"),
                    ))
                })?;
            }
        }
        Ok(())
    }

    /// Recompute the denormalised stats shown on the Library grid.
    pub fn refresh_stats(&self, project: &Project) -> ProjectResult<ProjectSummary> {
        self.db.update_stats(project.id, &project.db().stats()?)?;
        self.summary(project.id)
    }

    /// Show a project's current posters on the Library grid, without moving it
    /// to the front. Returns whether they changed.
    pub fn refresh_posters(&self, project: &Project) -> ProjectResult<bool> {
        let s = project.db().stats()?;
        Ok(self.db.set_posters(project.id, s.poster.as_deref(), s.poster_small.as_deref())?)
    }

    // -- import -------------------------------------------------------------

    /// Expand files and folders into importable videos, in episode order.
    /// `existing` marks candidates already in a project as duplicates.
    pub fn scan(
        &self,
        paths: &[String],
        kind: ProjectKind,
        existing: Option<&Project>,
    ) -> ProjectResult<Vec<ImportCandidate>> {
        let known = match existing {
            Some(p) => p.db().media_paths()?,
            None => HashSet::new(),
        };
        Ok(scan_paths(paths, kind, &known))
    }

    /// Add files to a project (skipping ones already there). Returns the new
    /// media ids. Nothing is added when one of them can't be imported.
    pub fn add_media(&self, project: &Project, files: &[String], kind: ProjectKind) -> ProjectResult<Vec<MediaId>> {
        self.add_found(project, importable(files, kind)?)
    }

    fn add_found(&self, project: &Project, candidates: Vec<ImportCandidate>) -> ProjectResult<Vec<MediaId>> {
        let new: Vec<NewMedia> = candidates
            .into_iter()
            .map(|c| {
                let mtime = std::fs::metadata(&c.path)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as i64);
                NewMedia {
                    id: MediaId::new(),
                    path: c.path,
                    file_name: c.file_name,
                    size_bytes: c.size_bytes as u64,
                    mtime,
                    season: c.season,
                    episode: c.episode,
                }
            })
            .collect();
        let added = project.db().add_media(&new)?;
        self.refresh_stats(project)?;
        Ok(added)
    }

    /// Projects currently open (for shutdown and recovery).
    pub fn open_projects(&self) -> Vec<Arc<Project>> {
        self.open.lock().values().cloned().collect()
    }
}

/// Fast Scan's default from before it followed the computer, read once: "1"
/// was the person's doing (it was off by default), so it stays On unless they
/// chose since; "0" was written by every save of the settings, so it says
/// nothing and goes.
fn migrate_fast_scan(db: &LibraryDb) -> ProjectResult<()> {
    let Some(legacy) = db.setting(KEY_LEGACY_FAST_SCAN)? else { return Ok(()) };
    let mut rows = vec![(KEY_LEGACY_FAST_SCAN, None)];
    if legacy == "1" && db.setting(KEY_SCAN_DEFAULT)?.is_none() {
        rows.push((KEY_SCAN_DEFAULT, Some("fast")));
    }
    Ok(db.write_settings(&rows)?)
}

fn unique_dir(root: &Path, name: &str) -> PathBuf {
    let mut dir = root.join(format!("{name}.autoscene"));
    let mut n = 2;
    while dir.exists() {
        dir = root.join(format!("{name} {n}.autoscene"));
        n += 1;
    }
    dir
}

/// Whether `dir` is recognisably project `id`'s folder, to be deleted with
/// everything in it: an `.autoscene` folder whose database says it is that
/// project. The library's record alone isn't trusted with a whole folder.
/// Where the folder is doesn't matter: the library folder can be changed,
/// and projects made before stay where they are.
fn is_project_dir(dir: &Path, id: ProjectId) -> bool {
    let db = dir.join(PROJECT_DB);
    if !db.is_file() {
        return false;
    }
    let named = dir.extension().is_some_and(|e| e.eq_ignore_ascii_case("autoscene"));
    let says = ProjectDb::stored_id(&db);
    let theirs = named && matches!(&says, Ok(Some(s)) if *s == id.to_string());
    if !theirs {
        tracing::warn!(dir = %as_core::paths::name(dir), ?says, "not deleting a folder that isn't recognisably the project's");
    }
    theirs
}

/// How long deleting a project's folder waits for others still holding the
/// project to let go of it (its jobs have stopped by then).
const RELEASE_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// Windows keeps files locked for a moment after handles close (antivirus,
/// the indexer, a just-closed SQLite WAL); retry briefly before giving up.
fn remove_dir_with_retry(dir: &Path) -> std::io::Result<()> {
    let mut last = None;
    for attempt in 0..5 {
        match std::fs::remove_dir_all(dir) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => last = Some(e),
        }
        std::thread::sleep(std::time::Duration::from_millis(100 * (attempt + 1)));
    }
    Err(last.expect("loop ran"))
}

/// Why a file whose path isn't valid Unicode can't be imported: media keep
/// their path as text, and such a path, turned into text, names no file.
const NOT_UNICODE: &str = "Its name, or the name of a folder it's in, isn't valid Unicode. Rename it to import it.";

/// The videos `files` (files and folders) hold, in episode order — none
/// when one of them can't be imported, with an error that names it.
fn importable(files: &[String], kind: ProjectKind) -> ProjectResult<Vec<ImportCandidate>> {
    let found = scan_paths(files, kind, &HashSet::new());
    let refused: Vec<&str> = found.iter().filter(|c| c.refused.is_some()).map(|c| c.file_name.as_str()).collect();
    let why = match refused[..] {
        [] => return Ok(found),
        [one] => format!(
            "{one} can't be imported: its name, or the name of a folder it's in, isn't valid Unicode. \
             Rename it and add it again."
        ),
        [first, ref rest @ ..] => format!(
            "{first} and {} more can't be imported: their names, or the names of folders they're in, aren't \
             valid Unicode. Rename them and add them again.",
            rest.len()
        ),
    };
    Err(ProjectError::Invalid(why))
}

pub(crate) fn scan_paths(paths: &[String], kind: ProjectKind, known: &HashSet<String>) -> Vec<ImportCandidate> {
    let mut files: Vec<PathBuf> = Vec::new();
    for p in paths {
        let p = PathBuf::from(p);
        if p.is_dir() {
            walk(&p, SCAN_DEPTH, &mut files);
        } else if p.is_file() && is_video(&p) {
            files.push(p);
        }
    }
    let mut seen = HashSet::new();
    let mut out: Vec<ImportCandidate> = files
        .into_iter()
        .filter(|f| seen.insert(f.clone()))
        .map(|f| {
            let file_name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let parent = f.parent().and_then(|d| d.file_name()).and_then(|d| d.to_str());
            let (season, episode) =
                if kind == ProjectKind::Series { parse_episode(&file_name, parent) } else { (None, None) };
            let path = f.display().to_string();
            ImportCandidate {
                duplicate: known.contains(&path),
                size_bytes: std::fs::metadata(&f).map(|m| m.len() as f64).unwrap_or(0.0),
                label: as_db::episode_label(season, episode),
                refused: f.to_str().is_none().then(|| NOT_UNICODE.into()),
                path,
                file_name,
                season,
                episode,
            }
        })
        .collect();
    out.sort_by(|a, b| episode_cmp((a.season, a.episode, &a.file_name), (b.season, b.episode, &b.file_name)));
    out
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().to_string();
        // Hidden files, macOS resource forks, scene-release sample folders, our own project folders.
        if name.starts_with('.') || name.eq_ignore_ascii_case("sample") || name.eq_ignore_ascii_case("samples") {
            continue;
        }
        let path = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            if depth > 0 && !name.ends_with(".autoscene") {
                walk(&path, depth - 1, out);
            }
        } else if is_video(&path) {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib() -> (tempfile::TempDir, Library) {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(&AppPaths::at(dir.path().join("data"))).unwrap();
        lib.update_settings(&AppSettings {
            library_root: dir.path().join("Library").display().to_string(),
            export_preset_id: "h264-edit".into(),
            audio_language: "German".into(),
            find_bodies: false,
            fast_scan: false,
            anilist: false,
            shortcuts: Default::default(),
            allow_unreviewed_plugins: false,
            gpu: String::new(),
            plugin_catalog_checks: true,
            nvidia_acceleration: true,
            discord_rpc: Default::default(),
        })
        .unwrap();
        assert_eq!(lib.audio_language().unwrap(), "deu", "stored as an ISO 639-2 code");
        (dir, lib)
    }

    fn touch(p: &Path) -> String {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"x").unwrap();
        p.display().to_string()
    }

    #[test]
    fn scans_folders_in_episode_order() {
        let (dir, lib) = lib();
        let show = dir.path().join("Show");
        touch(&show.join("Season 1/Show - 10.mkv"));
        touch(&show.join("Season 1/Show - 2.mkv"));
        touch(&show.join("Season 1/Show - 1.mkv"));
        touch(&show.join("Season 1/notes.txt"));
        touch(&show.join("Season 1/Sample/Show - 1 sample.mkv"));
        touch(&show.join(".hidden.mkv"));
        touch(&show.join("Season 2/Show S02E01.mkv"));
        let found = lib.scan(&[show.display().to_string()], ProjectKind::Series, None).unwrap();
        let labels: Vec<_> = found.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["S01E01", "S01E02", "S01E10", "S02E01"]);

        // Movies ignore numbers in names.
        let movie = touch(&dir.path().join("Blade Runner 2049.mkv"));
        let found = lib.scan(&[movie], ProjectKind::Movie, None).unwrap();
        assert_eq!((found[0].season, found[0].episode), (None, None));
    }

    /// `before`, a character no Unicode text holds (an unpaired surrogate on
    /// Windows, a stray byte elsewhere), then `after`.
    fn not_unicode(dir: &Path, before: &str, after: &str) -> PathBuf {
        #[cfg(windows)]
        let name = {
            use std::os::windows::ffi::OsStringExt;
            let wide: Vec<u16> = before.encode_utf16().chain([0xD800]).chain(after.encode_utf16()).collect();
            std::ffi::OsString::from_wide(&wide)
        };
        #[cfg(unix)]
        let name = {
            use std::os::unix::ffi::OsStringExt;
            std::ffi::OsString::from_vec([before.as_bytes(), b"\xff", after.as_bytes()].concat())
        };
        dir.join(name)
    }

    #[test]
    fn a_file_whose_name_isnt_unicode_is_refused_with_a_reason() {
        let (dir, lib) = lib();
        let show = dir.path().join("Show");
        let good = touch(&show.join("Show - 01.mkv"));
        let bad = not_unicode(&show, "Show", " - 02.mkv");
        if std::fs::write(&bad, b"x").is_err() {
            return; // a file system that never has such names (APFS)
        }
        // Listed with why, not as a file that is missing later.
        let found = lib.scan(&[show.display().to_string()], ProjectKind::Series, None).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].refused, None);
        assert!(found[1].refused.as_deref().is_some_and(|r| r.contains("Unicode")), "{:?}", found[1]);
        assert_eq!((found[1].episode, found[1].label.as_str()), (Some(2), "E02"), "named for the list");

        // Nothing is made or added while one is among them.
        let req = NewProject {
            title: "Show".into(),
            kind: ProjectKind::Series,
            content: ContentKind::Live,
            files: vec![show.display().to_string()],
        };
        let Err(ProjectError::Invalid(why)) = lib.create_project(&req) else { panic!("created") };
        assert!(why.starts_with("Show\u{FFFD} - 02.mkv can't") && why.contains("Rename it"), "{why}");
        assert!(lib.projects().unwrap().is_empty());
        let (_, project, added) = lib.create_project(&NewProject { files: vec![good], ..req.clone() }).unwrap();
        assert_eq!(added.len(), 1);
        std::fs::write(not_unicode(&show, "Show", " - 03.mkv"), b"x").unwrap();
        let more = lib.add_media(&project, &[show.display().to_string()], ProjectKind::Series);
        assert!(matches!(&more, Err(ProjectError::Invalid(why)) if why.contains("and 1 more")), "{more:?}");
        assert_eq!(project.media_list_uncounted().unwrap().len(), 1);
    }

    #[test]
    fn creates_renames_and_deletes_projects() {
        let (dir, lib) = lib();
        let a = touch(&dir.path().join("src/Show - 01.mkv"));
        let b = touch(&dir.path().join("src/Show - 02.mkv"));
        let req = NewProject {
            title: "Show: The Series".into(),
            kind: ProjectKind::Series,
            content: ContentKind::Anime,
            files: vec![b.clone(), a.clone(), a.clone()],
        };
        let (summary, project, added) = lib.create_project(&req).unwrap();
        assert_eq!(added.len(), 2);
        assert_eq!(summary.media_count, 2);
        assert!(summary.dir.ends_with("Show The Series.autoscene"), "{}", summary.dir);
        // A second project with the same title gets its own folder.
        let (second, ..) = lib.create_project(&NewProject { files: vec![], ..req.clone() }).unwrap();
        assert!(second.dir.ends_with("Show The Series 2.autoscene"), "{}", second.dir);

        // Re-adding the same file is a no-op; duplicates are flagged in scans.
        assert!(lib.add_media(&project, std::slice::from_ref(&a), ProjectKind::Series).unwrap().is_empty());
        let scan = lib.scan(std::slice::from_ref(&a), ProjectKind::Series, Some(&project)).unwrap();
        assert!(scan[0].duplicate);

        lib.rename_project(summary.id, "Renamed").unwrap();
        assert_eq!(lib.summary(summary.id).unwrap().title, "Renamed");
        assert_eq!(project.title().unwrap(), "Renamed");

        drop(project);
        lib.delete_project(summary.id, true).unwrap();
        assert!(!Path::new(&summary.dir).exists());
        assert!(Path::new(&a).exists(), "source media are never deleted");
        assert_eq!(lib.projects().unwrap().len(), 1);
        assert!(matches!(lib.project(summary.id), Err(ProjectError::NotFound(_))));
    }

    #[test]
    fn only_the_projects_own_folder_is_deleted_with_it() {
        let (dir, lib) = lib();
        let new = |title: &str| NewProject {
            title: title.into(),
            kind: ProjectKind::Movie,
            content: ContentKind::Live,
            files: vec![],
        };
        let (a, ..) = lib.create_project(&new("A")).unwrap();
        let (b, ..) = lib.create_project(&new("B")).unwrap();
        let (c, ..) = lib.create_project(&new("C")).unwrap();
        // What the library says a project's folder is (as a damaged or
        // edited library.db could).
        let point = |id: ProjectId, to: &Path| {
            rusqlite::Connection::open(AppPaths::at(dir.path().join("data")).library_db())
                .unwrap()
                .execute("UPDATE projects SET dir = ?2 WHERE id = ?1", [id.to_string(), to.display().to_string()])
                .unwrap();
        };
        // A folder with a project database that says it is `id`'s.
        let says = |to: &Path, id: ProjectId| {
            std::fs::create_dir_all(to).unwrap();
            ProjectDb::open(&to.join(PROJECT_DB)).unwrap().set_meta("id", &id.to_string()).unwrap();
            std::fs::write(to.join("keep.txt"), b"mine").unwrap();
        };

        // Another project's folder (a copy of B's): kept.
        let b_copy = dir.path().join("B copy.autoscene");
        says(&b_copy, b.id);
        point(a.id, &b_copy);
        lib.delete_project(a.id, true).unwrap();
        assert!(b_copy.join("keep.txt").is_file());
        assert!(matches!(lib.summary(a.id), Err(ProjectError::NotFound(_))), "the project itself is removed");

        // A folder that isn't a project folder, even with the project's database in it: kept.
        let stuff = dir.path().join("Stuff");
        says(&stuff, b.id);
        point(b.id, &stuff);
        lib.delete_project(b.id, true).unwrap();
        assert!(stuff.join("keep.txt").is_file());

        // Its own folder goes, also when the library has moved since.
        let moved =
            AppSettings { library_root: dir.path().join("Elsewhere").display().to_string(), ..lib.settings().unwrap() };
        lib.update_settings(&moved).unwrap();
        lib.delete_project(c.id, true).unwrap();
        assert!(!Path::new(&c.dir).exists());
    }

    #[test]
    fn a_project_opens_once_while_others_are_handed_out() {
        let (dir, lib) = lib();
        let new = |title: &str| NewProject {
            title: title.into(),
            kind: ProjectKind::Movie,
            content: ContentKind::Live,
            files: vec![],
        };
        let (a, ..) = lib.create_project(&new("A")).unwrap();
        let (b, ..) = lib.create_project(&new("B")).unwrap();
        let (c, ..) = lib.create_project(&new("C")).unwrap();
        drop(lib);
        // A launch: nothing is open yet.
        let lib = Library::open(&AppPaths::at(dir.path().join("data"))).unwrap();
        let open_a = lib.project(a.id).unwrap();
        let wait = std::time::Duration::from_secs(20);

        std::thread::scope(|s| {
            // B is being opened (say, its database upgraded) for as long as this is held.
            let b_opening = lib.opening.lock().entry(b.id).or_default().clone();
            let held = b_opening.lock();
            let (tx, rx) = std::sync::mpsc::channel();
            let lib = &lib;
            let tx_b = tx.clone();
            s.spawn(move || tx_b.send(("b", lib.project(b.id).unwrap())).unwrap());
            // Meanwhile an open project is handed out, and another one opens.
            let tx_a = tx.clone();
            s.spawn(move || tx_a.send(("a", lib.project(a.id).unwrap())).unwrap());
            s.spawn(move || tx.send(("c", lib.project(c.id).unwrap())).unwrap());
            let mut done: Vec<_> = (0..2).map(|_| rx.recv_timeout(wait).expect("not held up by B")).collect();
            done.sort_by_key(|(name, _)| *name);
            assert_eq!(done.iter().map(|(name, _)| *name).collect::<Vec<_>>(), ["a", "c"]);
            assert!(Arc::ptr_eq(&done[0].1, &open_a));
            // B waits for its own opening only.
            assert!(rx.recv_timeout(std::time::Duration::from_millis(200)).is_err());
            drop(held);
            let (name, open_b) = rx.recv_timeout(wait).unwrap();
            assert_eq!(name, "b");
            assert!(Arc::ptr_eq(&open_b, &lib.project(b.id).unwrap()));
        });

        // Asked for by many at once: opened once, the same for all.
        drop(lib);
        let lib = Library::open(&AppPaths::at(dir.path().join("data"))).unwrap();
        let all: Vec<Arc<Project>> = std::thread::scope(|s| {
            let asks: Vec<_> = (0..8).map(|_| s.spawn(|| lib.project(c.id).unwrap())).collect();
            asks.into_iter().map(|t| t.join().unwrap()).collect()
        });
        assert!(all.iter().all(|p| Arc::ptr_eq(p, &all[0])));
    }

    #[test]
    fn a_deleted_project_is_let_go_of_before_its_folder_goes() {
        let (_dir, lib) = lib();
        let req =
            NewProject { title: "Held".into(), kind: ProjectKind::Movie, content: ContentKind::Live, files: vec![] };
        let (summary, project, _) = lib.create_project(&req).unwrap();
        // Work still finishing with it (its database open) when it is deleted.
        let released = Arc::new(AtomicBool::new(false));
        let holder = {
            let released = released.clone();
            std::thread::spawn(move || {
                // Longer than removing the folder keeps trying.
                std::thread::sleep(std::time::Duration::from_millis(1800));
                released.store(true, Ordering::SeqCst);
                drop(project);
            })
        };
        lib.delete_project(summary.id, true).unwrap();
        assert!(released.load(Ordering::SeqCst), "the folder went while the project was still held");
        assert!(!Path::new(&summary.dir).exists());
        assert!(lib.open_projects().is_empty());
        assert!(matches!(lib.project(summary.id), Err(ProjectError::NotFound(_))));
        holder.join().unwrap();
    }

    #[test]
    fn says_what_the_footage_is_after_import() {
        let (_dir, lib) = lib();
        let new = |content| NewProject { title: "Film".into(), kind: ProjectKind::Movie, content, files: vec![] };
        let (film, ..) = lib.create_project(&new(ContentKind::Live)).unwrap();
        let project = lib.project(film.id).unwrap();
        project.db().set_meta(crate::cast::GROUPED_WITH, &as_identity::VERSION.to_string()).unwrap();

        let (summary, regroup) = lib.set_project_content(film.id, ContentKind::Animation).unwrap();
        assert!(!regroup, "no faces yet: nothing to group");
        assert_eq!(summary.content, ContentKind::Animation);
        assert_eq!(project.content().unwrap(), ContentKind::Animation);
        // The cast is grouped again for it (by the next upkeep, also after a restart).
        assert_eq!(project.db().meta(crate::cast::GROUPED_WITH).unwrap().as_deref(), Some("0"));
        // The same again changes nothing.
        assert!(!lib.set_project_content(film.id, ContentKind::Animation).unwrap().1);

        // Never to or from anime: it is analysed differently from the start.
        assert!(matches!(lib.set_project_content(film.id, ContentKind::Anime), Err(ProjectError::Invalid(_))));
        assert_eq!(lib.summary(film.id).unwrap().content, ContentKind::Animation);
        let (anime, ..) = lib.create_project(&new(ContentKind::Anime)).unwrap();
        assert!(matches!(lib.set_project_content(anime.id, ContentKind::Live), Err(ProjectError::Invalid(_))));
        assert_eq!(lib.summary(anime.id).unwrap().content, ContentKind::Anime);
        // Anime finds its characters whole: no people pass, whatever the setting.
        lib.update_settings(&AppSettings { find_bodies: true, ..lib.settings().unwrap() }).unwrap();
        assert!(lib.project(film.id).unwrap().finds_bodies());
        assert!(!lib.project(anime.id).unwrap().finds_bodies());
    }

    #[test]
    fn discord_is_off_until_chosen_and_for_what_cant_be_read() {
        let (_dir, lib) = lib();
        let off = as_api::DiscordRpcSettings::default();
        assert_eq!(lib.discord_rpc().unwrap(), off, "a fresh library");
        assert!(!off.enabled && !off.show_project && !off.show_episode && !off.show_character);

        let chosen = as_api::DiscordRpcSettings { enabled: true, show_episode: true, ..off.clone() };
        lib.update_settings(&AppSettings { discord_rpc: chosen.clone(), ..lib.settings().unwrap() }).unwrap();
        assert_eq!(lib.discord_rpc().unwrap(), chosen);
        assert_eq!(lib.settings().unwrap().discord_rpc, chosen);

        // What can't be read turns it off, never on.
        for garbage in ["not json", "", "[]", r#"{"enabled":"yes"}"#] {
            lib.db.set_setting(KEY_DISCORD_RPC, garbage).unwrap();
            assert_eq!(lib.discord_rpc().unwrap(), off, "{garbage}");
        }
        // Part of it, or fields an older build wrote: a name stays off unless saved on.
        lib.db.set_setting(KEY_DISCORD_RPC, r#"{"enabled":true,"clientId":"1","showTimer":true}"#).unwrap();
        assert_eq!(lib.discord_rpc().unwrap(), as_api::DiscordRpcSettings { enabled: true, ..off });
    }

    #[test]
    fn fast_scan_is_for_projects_made_while_it_is_on() {
        let (dir, lib) = lib();
        let new =
            |title: &str, content| NewProject { title: title.into(), kind: ProjectKind::Movie, content, files: vec![] };
        // Off by default.
        assert!(!lib.settings().unwrap().fast_scan);
        let (before, ..) = lib.create_project(&new("Before", ContentKind::Live)).unwrap();
        assert!(!before.fast_scan && !lib.project(before.id).unwrap().fast_scan());

        assert!(lib.set_scan_default(ScanChoice::Fast).unwrap().footage.iter().all(|a| a.fast));
        let (after, ..) = lib.create_project(&new("After", ContentKind::Animation)).unwrap();
        assert!(after.fast_scan && lib.project(after.id).unwrap().fast_scan());
        // Projects made before keep how they were made.
        assert!(!lib.summary(before.id).unwrap().fast_scan);
        // Anime too: its heads are found in the frames Fast Scan picks.
        let (anime, ..) = lib.create_project(&new("Anime", ContentKind::Anime)).unwrap();
        assert!(anime.fast_scan && lib.project(anime.id).unwrap().fast_scan());
        let (summary, changed) = lib.set_project_fast_scan(anime.id, false).unwrap();
        assert!(!summary.fast_scan && changed && !lib.project(anime.id).unwrap().fast_scan());

        // Each project can be switched on its own ("Edit details…").
        let (summary, changed) = lib.set_project_fast_scan(before.id, true).unwrap();
        assert!(summary.fast_scan && changed && lib.project(before.id).unwrap().fast_scan());
        assert!(!lib.set_project_fast_scan(before.id, true).unwrap().1);
        let (summary, changed) = lib.set_project_fast_scan(after.id, false).unwrap();
        assert!(!summary.fast_scan && changed);
        // The setting itself is kept.
        drop(lib);
        let reopened = Library::open(&AppPaths::at(dir.path().join("data"))).unwrap();
        assert!(reopened.settings().unwrap().fast_scan);
        assert!(reopened.summary(before.id).unwrap().fast_scan && !reopened.summary(after.id).unwrap().fast_scan);
    }

    // -- Fast Scan's default for new projects ---------------------------------

    fn built_in_graphics() -> Machine {
        Machine::Windows(Some(vec![computer::Adapter {
            vendor: 0x8086,
            device: 0x9A49,
            name: "Intel Iris Xe Graphics".into(),
            integrated: true,
            luid: 0x1_0828,
        }]))
    }

    fn rtx() -> Machine {
        Machine::Windows(Some(vec![computer::Adapter {
            vendor: 0x10DE,
            device: 0x24C9,
            name: "NVIDIA GeForce RTX 3060 Ti".into(),
            integrated: false,
            luid: 0x1_0828,
        }]))
    }

    /// `lib()`'s library, opened on the computer `probe` reads.
    fn lib_on(probe: fn() -> Machine) -> (tempfile::TempDir, Library) {
        let (dir, lib) = lib();
        drop(lib);
        let lib = Library::open_with(&AppPaths::at(dir.path().join("data")), probe).unwrap();
        (dir, lib)
    }

    fn new_project(content: ContentKind) -> NewProject {
        NewProject { title: format!("{content:?}"), kind: ProjectKind::Movie, content, files: vec![] }
    }

    fn stored(lib: &Library, key: &str) -> Option<String> {
        lib.db.setting(key).unwrap()
    }

    #[test]
    fn new_projects_follow_this_computer_on_automatic() {
        let (_dir, lib) = lib_on(built_in_graphics);
        assert_eq!(lib.computer().class, computer::Class::Integrated);
        assert_eq!(lib.scan_defaults().unwrap().choice, ScanChoice::Auto);
        assert!(lib.settings().unwrap().fast_scan, "live action starts with Fast Scan here");
        let made = |content| lib.create_project(&new_project(content)).unwrap().0.fast_scan;
        assert!(made(ContentKind::Live) && made(ContentKind::Mixed) && made(ContentKind::Anime));
        assert!(!made(ContentKind::Animation), "3D animation keeps the full scan");
        // Finding people without a visible face keeps live action on the full scan.
        lib.update_settings(&AppSettings { find_bodies: true, ..lib.settings().unwrap() }).unwrap();
        assert!(!lib.settings().unwrap().fast_scan);
        assert!(!made(ContentKind::Live) && !made(ContentKind::Mixed) && made(ContentKind::Anime));

        // Graphics built for AI work keep the full scan for every footage.
        let (_dir, lib) = lib_on(rtx);
        assert_eq!(lib.computer().name, "NVIDIA GeForce RTX 3060 Ti");
        assert!(!lib.settings().unwrap().fast_scan);
        for content in [ContentKind::Live, ContentKind::Mixed, ContentKind::Animation, ContentKind::Anime] {
            assert!(!lib.create_project(&new_project(content)).unwrap().0.fast_scan, "{content:?}");
        }
    }

    #[test]
    fn automatic_on_and_off_are_stored_and_automatic_deletes_the_key() {
        let (dir, lib) = lib_on(built_in_graphics);
        assert_eq!(stored(&lib, KEY_SCAN_DEFAULT), None);
        let off = lib.set_scan_default(ScanChoice::Full).unwrap();
        assert_eq!((off.choice, off.footage.iter().any(|a| a.fast)), (ScanChoice::Full, false));
        assert_eq!(stored(&lib, KEY_SCAN_DEFAULT).as_deref(), Some("full"));
        assert!(!lib.settings().unwrap().fast_scan);
        let on = lib.set_scan_default(ScanChoice::Fast).unwrap();
        assert_eq!((on.choice, on.footage.iter().all(|a| a.fast)), (ScanChoice::Fast, true));
        assert_eq!(stored(&lib, KEY_SCAN_DEFAULT).as_deref(), Some("fast"));
        // Kept across starts.
        drop(lib);
        let lib = Library::open_with(&AppPaths::at(dir.path().join("data")), built_in_graphics).unwrap();
        assert_eq!(lib.scan_defaults().unwrap().choice, ScanChoice::Fast);
        let auto = lib.set_scan_default(ScanChoice::Auto).unwrap();
        assert_eq!(auto, scan::defaults(&lib.computer(), ScanChoice::Auto, false, true));
        assert_eq!(stored(&lib, KEY_SCAN_DEFAULT), None, "Automatic stores nothing");
        // A choice a newer app stored, that this one can't read, is Automatic.
        lib.db.set_setting(KEY_SCAN_DEFAULT, "balanced").unwrap();
        assert_eq!(lib.scan_defaults().unwrap().choice, ScanChoice::Auto);
    }

    #[test]
    fn a_settings_copy_never_changes_the_scan_default() {
        let (_dir, lib) = lib_on(built_in_graphics);
        // A copy made before (as the export page and the shortcuts save them)
        // says Fast Scan is off: Automatic stays, and so does Fast Scan here.
        let stale = AppSettings { fast_scan: false, ..lib.settings().unwrap() };
        assert!(lib.update_settings(&stale).unwrap().fast_scan);
        assert_eq!(stored(&lib, KEY_SCAN_DEFAULT), None);
        assert_eq!(stored(&lib, KEY_LEGACY_FAST_SCAN), None, "the old key isn't written again");
        // Nor the other way round.
        lib.set_scan_default(ScanChoice::Full).unwrap();
        let stale = AppSettings { fast_scan: true, ..lib.settings().unwrap() };
        assert!(!lib.update_settings(&stale).unwrap().fast_scan);
        assert_eq!(lib.scan_defaults().unwrap().choice, ScanChoice::Full);
    }

    #[test]
    fn fast_scan_chosen_before_stays_and_the_old_default_doesnt() {
        let (dir, lib) = lib_on(built_in_graphics);
        let reopen = |lib: Library| {
            drop(lib);
            Library::open_with(&AppPaths::at(dir.path().join("data")), rtx).unwrap()
        };
        // "1" was the person's own doing: On, even where the full scan is the default.
        lib.db.set_setting(KEY_LEGACY_FAST_SCAN, "1").unwrap();
        let lib = reopen(lib);
        assert_eq!(stored(&lib, KEY_SCAN_DEFAULT).as_deref(), Some("fast"));
        assert_eq!(stored(&lib, KEY_LEGACY_FAST_SCAN), None);
        assert!(lib.settings().unwrap().fast_scan);
        let lib = reopen(lib);
        assert_eq!(lib.scan_defaults().unwrap().choice, ScanChoice::Fast, "read once, and kept");
        // "0" was written by every save, so it says nothing.
        lib.set_scan_default(ScanChoice::Auto).unwrap();
        lib.db.set_setting(KEY_LEGACY_FAST_SCAN, "0").unwrap();
        let lib = reopen(lib);
        assert_eq!((stored(&lib, KEY_SCAN_DEFAULT), stored(&lib, KEY_LEGACY_FAST_SCAN)), (None, None));
        // A choice made since wins over an old "1" (an older app saved after this one).
        lib.set_scan_default(ScanChoice::Full).unwrap();
        lib.db.set_setting(KEY_LEGACY_FAST_SCAN, "1").unwrap();
        let lib = reopen(lib);
        assert_eq!(stored(&lib, KEY_SCAN_DEFAULT).as_deref(), Some("full"));
        assert_eq!(stored(&lib, KEY_LEGACY_FAST_SCAN), None);
    }

    #[test]
    fn a_project_is_made_with_the_scan_the_dialog_sent() {
        let (_dir, lib) = lib_on(built_in_graphics);
        for content in [ContentKind::Live, ContentKind::Mixed, ContentKind::Animation, ContentKind::Anime] {
            let req = new_project(content);
            let (on, project, _) = lib.create_project_with(&req, Some(true)).unwrap();
            assert!(on.fast_scan && project.fast_scan(), "{content:?} on");
            assert!(lib.summary(on.id).unwrap().fast_scan);
            let (off, project, _) = lib.create_project_with(&req, Some(false)).unwrap();
            assert!(!off.fast_scan && !project.fast_scan(), "{content:?} off");
            // Nothing sent: the footage's default here.
            let (default, ..) = lib.create_project_with(&req, None).unwrap();
            assert_eq!(default.fast_scan, content != ContentKind::Animation, "{content:?} default");
        }
        // What the dialog sent wins over the person's default too.
        lib.set_scan_default(ScanChoice::Full).unwrap();
        assert!(lib.create_project_with(&new_project(ContentKind::Live), Some(true)).unwrap().0.fast_scan);
        assert!(!lib.create_project_with(&new_project(ContentKind::Live), None).unwrap().0.fast_scan);
    }

    #[test]
    fn a_library_opened_for_tests_knows_no_computer() {
        let (_dir, lib) = lib();
        assert_eq!(lib.computer().class, computer::Class::Unknown);
        let d = lib.scan_defaults().unwrap();
        assert_eq!(d.computer, as_api::ThisComputer { class: as_api::GraphicsClass::Unknown, name: String::new() });
        assert!(d.footage.iter().all(|a| !a.fast), "the full scan, as before");
        assert!(!lib.settings().unwrap().fast_scan);
    }

    /// The runtime AutoScene ships for Linux computes on the processor only.
    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn on_linux_new_live_action_projects_start_with_fast_scan() {
        let (_dir, lib) = lib_on(computer::probe);
        assert_eq!(lib.computer().class, computer::Class::Processor);
        assert!(lib.settings().unwrap().fast_scan);
        assert!(lib.create_project(&new_project(ContentKind::Live)).unwrap().0.fast_scan);
        assert!(!lib.create_project(&new_project(ContentKind::Animation)).unwrap().0.fast_scan);
    }

    #[test]
    fn the_machine_is_probed_once_and_chosen_on_every_call() {
        static PROBES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        fn counted() -> Machine {
            PROBES.fetch_add(1, Ordering::SeqCst);
            built_in_graphics()
        }
        let (dir, lib) = lib();
        drop(lib);
        let lib = Library::open_with(&AppPaths::at(dir.path().join("data")), counted).unwrap();
        assert_eq!(PROBES.load(Ordering::SeqCst), 0, "not read before it is needed");
        assert_eq!(lib.computer().class, computer::Class::Integrated);
        // Each answer is made afresh from what is chosen now; the adapters are read once.
        assert!(lib.scan_defaults().unwrap().footage[0].fast);
        lib.update_settings(&AppSettings { find_bodies: true, ..lib.settings().unwrap() }).unwrap();
        assert!(!lib.scan_defaults().unwrap().footage[0].fast);
        assert!(lib.set_scan_default(ScanChoice::Fast).unwrap().footage[0].fast);
        lib.create_project(&new_project(ContentKind::Live)).unwrap();
        assert_eq!(PROBES.load(Ordering::SeqCst), 1);
    }

    fn gtx() -> Machine {
        Machine::Windows(Some(vec![computer::Adapter {
            vendor: 0x10DE,
            device: 0x1C03,
            name: "NVIDIA GeForce GTX 1060 6GB".into(),
            integrated: false,
            luid: 0x1_0828,
        }]))
    }

    fn processor_only() -> Machine {
        Machine::CpuRuntime
    }

    /// A hybrid laptop: DirectML's own list starts with the RTX.
    fn hybrid() -> Machine {
        Machine::Windows(Some(vec![
            computer::Adapter {
                vendor: 0x10DE,
                device: 0x28E0,
                name: "NVIDIA GeForce RTX 4060 Laptop GPU".into(),
                integrated: false,
                luid: 0x1_0002,
            },
            computer::Adapter {
                vendor: 0x8086,
                device: 0xA7A0,
                name: "Intel Iris Xe Graphics".into(),
                integrated: true,
                luid: 0x1_0001,
            },
        ]))
    }

    fn choose_gpu(lib: &Library, gpu: &str) -> AppSettings {
        lib.update_settings(&AppSettings { gpu: gpu.into(), ..lib.settings().unwrap() }).unwrap()
    }

    #[test]
    fn the_gpu_choice_is_kept() {
        let (dir, lib) = lib_on(hybrid);
        assert_eq!((lib.gpu(), stored(&lib, KEY_GPU)), (String::new(), None), "Automatic stores nothing");
        let iris = "8086:a7a0:Intel Iris Xe Graphics";
        assert_eq!(choose_gpu(&lib, &format!(" {iris} ")).gpu, iris);
        assert_eq!(stored(&lib, KEY_GPU).as_deref(), Some(iris));
        // Saving other settings keeps it.
        lib.update_settings(&AppSettings { audio_language: "fra".into(), ..lib.settings().unwrap() }).unwrap();
        drop(lib);
        let lib = Library::open_with(&AppPaths::at(dir.path().join("data")), hybrid).unwrap();
        assert_eq!(lib.settings().unwrap().gpu, iris, "kept across starts");
        assert_eq!(choose_gpu(&lib, "cpu").gpu, "cpu");
        assert_eq!(choose_gpu(&lib, "").gpu, "");
        assert_eq!(stored(&lib, KEY_GPU), None, "Automatic deletes it");
    }

    #[test]
    fn nvidia_acceleration_is_on_unless_switched_off() {
        let (dir, lib) = lib_on(hybrid);
        // A library from before it, and a settings file without the field.
        assert_eq!(stored(&lib, KEY_NVIDIA_ACCELERATION), None);
        assert!(lib.settings().unwrap().nvidia_acceleration);
        let mut json = serde_json::to_value(lib.settings().unwrap()).unwrap();
        json.as_object_mut().unwrap().remove("nvidiaAcceleration");
        assert!(serde_json::from_value::<AppSettings>(json).unwrap().nvidia_acceleration);

        let off = AppSettings { nvidia_acceleration: false, ..lib.settings().unwrap() };
        assert!(!lib.update_settings(&off).unwrap().nvidia_acceleration);
        assert_eq!(stored(&lib, KEY_NVIDIA_ACCELERATION).as_deref(), Some("0"));
        drop(lib);
        let lib = Library::open_with(&AppPaths::at(dir.path().join("data")), hybrid).unwrap();
        assert!(!lib.nvidia_acceleration(), "kept across starts");
        // Another setting saved keeps it.
        lib.update_settings(&AppSettings { audio_language: "fra".into(), ..lib.settings().unwrap() }).unwrap();
        assert!(!lib.nvidia_acceleration());
        lib.set_nvidia_acceleration(true).unwrap();
        assert!(lib.settings().unwrap().nvidia_acceleration);
        assert_eq!(stored(&lib, KEY_NVIDIA_ACCELERATION), None, "on stores nothing");
    }

    #[test]
    fn a_chosen_card_is_classed_by_the_same_table() {
        let (_dir, lib) = lib_on(hybrid);
        // Automatic: the RTX DirectML opens, built for AI work.
        assert_eq!(lib.computer().class, computer::Class::ForAi);
        assert!(!lib.settings().unwrap().fast_scan);
        // Its built-in graphics chosen: Fast Scan on Automatic.
        choose_gpu(&lib, "8086:a7a0:Intel(R) Iris(R) Xe Graphics");
        assert_eq!((lib.computer().class, lib.computer().luid), (computer::Class::Integrated, Some(0x1_0001)));
        let d = lib.scan_defaults().unwrap();
        assert_eq!(d.computer.class, as_api::GraphicsClass::Integrated);
        assert!(lib.settings().unwrap().fast_scan);
        assert!(lib.create_project(&new_project(ContentKind::Live)).unwrap().0.fast_scan);
        // Its RTX chosen: the full scan.
        choose_gpu(&lib, "10de:28e0:NVIDIA GeForce RTX 4060 Laptop GPU");
        assert_eq!((lib.computer().class, lib.computer().luid), (computer::Class::ForAi, Some(0x1_0002)));
        assert!(!lib.create_project(&new_project(ContentKind::Live)).unwrap().0.fast_scan);
        // A card that isn't connected any more: Automatic's.
        choose_gpu(&lib, "1002:744c:AMD Radeon RX 7900 XTX");
        assert_eq!(lib.computer().luid, Some(0x1_0002));
    }

    #[test]
    fn processor_only_counts_as_the_processor_for_fast_scan() {
        let (_dir, lib) = lib_on(rtx);
        assert!(!lib.settings().unwrap().fast_scan);
        choose_gpu(&lib, "cpu");
        assert_eq!(lib.computer().class, computer::Class::Processor);
        let d = lib.scan_defaults().unwrap();
        assert_eq!(d.computer, as_api::ThisComputer { class: as_api::GraphicsClass::Processor, name: String::new() });
        assert!(lib.settings().unwrap().fast_scan, "live action starts with Fast Scan on the processor");
        assert!(!lib.create_project(&new_project(ContentKind::Animation)).unwrap().0.fast_scan);
        choose_gpu(&lib, "");
        assert!(!lib.settings().unwrap().fast_scan);
    }

    #[test]
    fn a_gpu_change_in_settings_meets_the_notice() {
        // On the RTX the notice isn't due; Processor only makes Fast Scan the
        // default, and the person chose it where Fast Scan's row says why.
        let (dir, lib) = lib_on(rtx);
        assert!(!notice_due(&lib));
        choose_gpu(&lib, "cpu");
        assert!(lib.settings().unwrap().fast_scan);
        assert!(!notice_due(&lib));
        assert_eq!(stored(&lib, KEY_SCAN_NOTICE).as_deref(), Some("done"));
        drop(lib);
        let lib = Library::open_with(&AppPaths::at(dir.path().join("data")), rtx).unwrap();
        assert!(!notice_due(&lib), "a later start doesn't show it");
        // A change that leaves the full scan the default stores nothing.
        let (_dir, lib) = lib_on(hybrid);
        choose_gpu(&lib, "10de:28e0:NVIDIA GeForce RTX 4060 Laptop GPU");
        assert_eq!(stored(&lib, KEY_SCAN_NOTICE), None);
        // Saving the same choice again isn't a change.
        let (_dir, lib) = lib_on(built_in_graphics);
        lib.update_settings(&lib.settings().unwrap()).unwrap();
        assert!(notice_due(&lib));
    }

    fn notice_due(lib: &Library) -> bool {
        lib.scan_defaults().unwrap().notice
    }

    #[test]
    fn the_notice_is_due_until_dont_show_this_again() {
        let (dir, lib) = lib_on(built_in_graphics);
        assert!(notice_due(&lib));
        // Saving other settings, or making a project, doesn't end it.
        lib.update_settings(&AppSettings { audio_language: "fra".into(), ..lib.settings().unwrap() }).unwrap();
        lib.create_project(&new_project(ContentKind::Live)).unwrap();
        assert!(notice_due(&lib), "Continue without the tick stores nothing");
        drop(lib);
        let lib = Library::open_with(&AppPaths::at(dir.path().join("data")), built_in_graphics).unwrap();
        assert!(notice_due(&lib), "still due at the next start");
        lib.scan_notice_done().unwrap();
        assert!(!notice_due(&lib));
        assert_eq!(stored(&lib, KEY_SCAN_NOTICE).as_deref(), Some("done"));
        // Fast Scan stays the default: the notice only said so.
        assert_eq!(lib.scan_defaults().unwrap().choice, ScanChoice::Auto);
        assert!(lib.settings().unwrap().fast_scan);
        drop(lib);
        let lib = Library::open_with(&AppPaths::at(dir.path().join("data")), built_in_graphics).unwrap();
        assert!(!notice_due(&lib), "kept across starts");
    }

    #[test]
    fn dont_show_again_holds_when_the_graphics_change() {
        let (dir, lib) = lib_on(built_in_graphics);
        lib.scan_notice_done().unwrap();
        drop(lib);
        // A new card without matrix units, a driver that leaves only the
        // processor, then the built-in graphics again: one flag for the library.
        for probe in [gtx as fn() -> Machine, processor_only, built_in_graphics] {
            let lib = Library::open_with(&AppPaths::at(dir.path().join("data")), probe).unwrap();
            assert!(lib.computer().class.fast_by_default());
            assert!(!notice_due(&lib), "{:?}", lib.computer().class);
        }
    }

    #[test]
    fn choosing_a_default_ends_the_notice() {
        for choice in [ScanChoice::Auto, ScanChoice::Fast, ScanChoice::Full] {
            let (_dir, lib) = lib_on(built_in_graphics);
            assert!(notice_due(&lib));
            assert!(!lib.set_scan_default(choice).unwrap().notice, "{choice:?}");
            assert_eq!(stored(&lib, KEY_SCAN_NOTICE).as_deref(), Some("done"), "{choice:?}");
            // Back to Automatic later (the toast's Undo): still not due.
            assert!(!lib.set_scan_default(ScanChoice::Auto).unwrap().notice, "{choice:?}");
        }
    }

    #[test]
    fn a_settings_change_that_leaves_fast_scan_the_default_meets_the_notice() {
        // Faceless people on: live action keeps the full scan, anime still
        // starts with Fast Scan here, and the change was made where Fast
        // Scan's row says why.
        let (_dir, lib) = lib_on(built_in_graphics);
        lib.update_settings(&AppSettings { find_bodies: true, ..lib.settings().unwrap() }).unwrap();
        assert!(!notice_due(&lib));
        assert_eq!(stored(&lib, KEY_SCAN_NOTICE).as_deref(), Some("done"));
        // Saving the same value again isn't a change.
        let (_dir, lib) = lib_on(built_in_graphics);
        lib.update_settings(&AppSettings { find_bodies: false, ..lib.settings().unwrap() }).unwrap();
        assert!(notice_due(&lib));
        assert_eq!(stored(&lib, KEY_SCAN_NOTICE), None);
        // Where the full scan is the default, the change has nothing to meet.
        let (_dir, lib) = lib_on(rtx);
        lib.update_settings(&AppSettings { find_bodies: true, ..lib.settings().unwrap() }).unwrap();
        assert_eq!(stored(&lib, KEY_SCAN_NOTICE), None);
    }

    #[test]
    fn faceless_people_on_still_brings_the_notice_for_anime() {
        let (dir, lib) = lib_on(built_in_graphics);
        // Turned on before this app version: no change is made now.
        lib.db.set_setting(KEY_FIND_BODIES, "1").unwrap();
        drop(lib);
        let lib = Library::open_with(&AppPaths::at(dir.path().join("data")), built_in_graphics).unwrap();
        let d = lib.scan_defaults().unwrap();
        assert!(!scan::advice(&d, ContentKind::Live) && scan::advice(&d, ContentKind::Anime));
        assert!(d.notice);
    }

    #[test]
    fn no_notice_where_the_full_scan_is_the_default() {
        for probe in [rtx as fn() -> Machine, || Machine::Windows(None)] {
            let (_dir, lib) = lib_on(probe);
            assert!(!lib.computer().class.fast_by_default());
            assert!(!notice_due(&lib), "{:?}", lib.computer().class);
        }
        // On or Off chosen (here by an older app's "1", which met no notice):
        // the person decided, so there is nothing to announce.
        let (dir, lib) = lib_on(built_in_graphics);
        for choice in ["fast", "full"] {
            lib.db.set_setting(KEY_SCAN_DEFAULT, choice).unwrap();
            assert!(!notice_due(&lib), "{choice}");
        }
        drop(lib);
        let lib = Library::open_with(&AppPaths::at(dir.path().join("data")), built_in_graphics).unwrap();
        lib.db.write_settings(&[(KEY_SCAN_DEFAULT, None), (KEY_LEGACY_FAST_SCAN, Some("1"))]).unwrap();
        drop(lib);
        let lib = Library::open_with(&AppPaths::at(dir.path().join("data")), built_in_graphics).unwrap();
        assert_eq!(lib.scan_defaults().unwrap().choice, ScanChoice::Fast);
        assert!(!notice_due(&lib));
    }

    #[test]
    fn keeps_the_keys_the_person_chose() {
        let (dir, lib) = lib();
        assert_eq!(lib.settings().unwrap().shortcuts, Shortcuts::default());
        let ctrl = |k: &str| as_api::KeyBinding { key: k.into(), command: true, shift: false, alt: false };
        let chosen = Shortcuts {
            preset: "premiere".into(),
            keys: [("player.export.0".to_string(), vec![ctrl("m")]), ("player.mute.0".to_string(), vec![])].into(),
        };
        let saved = lib.update_settings(&AppSettings { shortcuts: chosen.clone(), ..lib.settings().unwrap() }).unwrap();
        assert_eq!(saved.shortcuts, chosen);
        // Stored the way the UI names it.
        let stored = lib.db.setting(KEY_SHORTCUTS).unwrap().unwrap();
        assert!(stored.contains(r#"{"key":"m","mod":true,"shift":false,"alt":false}"#), "{stored}");
        drop(lib);
        let reopened = Library::open(&AppPaths::at(dir.path().join("data"))).unwrap();
        assert_eq!(reopened.settings().unwrap().shortcuts, chosen);
        // What can't be read gives AutoScene's own keys, never an error, and
        // stays while other settings change.
        reopened.db.set_setting(KEY_SHORTCUTS, "{not json").unwrap();
        assert_eq!(reopened.settings().unwrap().shortcuts, Shortcuts::default());
        reopened.update_settings(&AppSettings { find_bodies: true, ..reopened.settings().unwrap() }).unwrap();
        assert_eq!(reopened.db.setting(KEY_SHORTCUTS).unwrap().as_deref(), Some("{not json"));
        // Keys chosen here replace it.
        reopened.update_settings(&AppSettings { shortcuts: chosen.clone(), ..reopened.settings().unwrap() }).unwrap();
        assert_eq!(reopened.settings().unwrap().shortcuts, chosen);
    }

    #[test]
    fn unreviewed_plugins_are_off_by_default_and_kept() {
        let (dir, lib) = lib();
        assert!(!lib.settings().unwrap().allow_unreviewed_plugins);
        assert!(!lib.allows_unreviewed_plugins().unwrap());
        // Saving the settings never turns it on...
        let on = AppSettings { allow_unreviewed_plugins: true, ..lib.settings().unwrap() };
        assert!(!lib.update_settings(&on).unwrap().allow_unreviewed_plugins);
        lib.set_allows_unreviewed_plugins(true).unwrap();
        assert!(lib.settings().unwrap().allow_unreviewed_plugins);
        // ...or off: a copy made before it was turned on keeps it.
        let stale = AppSettings { allow_unreviewed_plugins: false, fast_scan: true, ..on };
        assert!(lib.update_settings(&stale).unwrap().allow_unreviewed_plugins);
        drop(lib);
        let reopened = Library::open(&AppPaths::at(dir.path().join("data"))).unwrap();
        assert!(reopened.allows_unreviewed_plugins().unwrap());
        reopened.set_allows_unreviewed_plugins(false).unwrap();
        assert!(!reopened.allows_unreviewed_plugins().unwrap());
        let stale = AppSettings { allow_unreviewed_plugins: true, ..reopened.settings().unwrap() };
        assert!(!reopened.update_settings(&stale).unwrap().allow_unreviewed_plugins);
        // A page that sends no answer (an older one) reads as off, and its
        // save leaves the switch as it is.
        let older: AppSettings = serde_json::from_value(serde_json::json!({
            "libraryRoot": dir.path().join("Library").display().to_string(),
            "exportPresetId": "h264-edit",
        }))
        .unwrap();
        assert!(!older.allow_unreviewed_plugins);
        reopened.set_allows_unreviewed_plugins(true).unwrap();
        assert!(reopened.update_settings(&older).unwrap().allow_unreviewed_plugins);
        assert!(reopened.allows_unreviewed_plugins().unwrap());
    }

    #[test]
    fn catalog_checks_are_on_by_default_and_kept() {
        let (dir, lib) = lib();
        assert!(lib.settings().unwrap().plugin_catalog_checks);
        // Off through the settings, and so after a restart.
        let off = AppSettings { plugin_catalog_checks: false, ..lib.settings().unwrap() };
        assert!(!lib.update_settings(&off).unwrap().plugin_catalog_checks);
        drop(lib);
        let reopened = Library::open(&AppPaths::at(dir.path().join("data"))).unwrap();
        assert!(!reopened.settings().unwrap().plugin_catalog_checks);
        let on = AppSettings { plugin_catalog_checks: true, ..reopened.settings().unwrap() };
        assert!(reopened.update_settings(&on).unwrap().plugin_catalog_checks);
        // A page that doesn't send it (an older one) reads as on.
        let older: AppSettings = serde_json::from_value(serde_json::json!({
            "libraryRoot": dir.path().join("Library").display().to_string(),
            "exportPresetId": "h264-edit",
        }))
        .unwrap();
        assert!(older.plugin_catalog_checks);
        // The row is outside what the plugin store reads as plugins.
        for prefix in ["plugin.", "plugin_settings.", "plugin_extra."] {
            assert!(!KEY_CATALOG_CHECKS.starts_with(prefix));
        }
    }

    #[test]
    fn keeps_settings_while_the_library_drive_is_away() {
        let (dir, lib) = lib();
        // The library's folder is in use, then its drive goes: a file stands
        // where the folder was, so it can't be made again.
        let root = PathBuf::from(lib.settings().unwrap().library_root);
        std::fs::remove_dir_all(&root).unwrap();
        std::fs::write(&root, b"").unwrap();
        let saved = lib.update_settings(&AppSettings { find_bodies: true, ..lib.settings().unwrap() }).unwrap();
        assert!(saved.find_bodies);
        // A new folder that can't be made is still refused.
        let blocked = dir.path().join("blocked");
        std::fs::write(&blocked, b"").unwrap();
        let elsewhere =
            AppSettings { library_root: blocked.join("Library").display().to_string(), ..lib.settings().unwrap() };
        assert!(matches!(lib.update_settings(&elsewhere), Err(ProjectError::Invalid(_))));
    }

    #[test]
    fn rejects_relative_library_roots() {
        let (_dir, lib) = lib();
        let bad = AppSettings {
            library_root: "relative/path".into(),
            export_preset_id: "h264-edit".into(),
            audio_language: "eng".into(),
            find_bodies: false,
            fast_scan: false,
            anilist: false,
            shortcuts: Default::default(),
            allow_unreviewed_plugins: false,
            gpu: String::new(),
            plugin_catalog_checks: true,
            nvidia_acceleration: true,
            discord_rpc: Default::default(),
        };
        assert!(matches!(lib.update_settings(&bad), Err(ProjectError::Invalid(_))));
        assert_eq!(lib.settings().unwrap().export_preset_id, "h264-edit");
    }

    #[test]
    fn project_db_open_is_shared() {
        let (_dir, lib) = lib();
        let (s, ..) = lib
            .create_project(&NewProject {
                title: "X".into(),
                kind: ProjectKind::Movie,
                content: ContentKind::Live,
                files: vec![],
            })
            .unwrap();
        let a = lib.project(s.id).unwrap();
        let b = lib.project(s.id).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert!(ProjectDb::open(&PathBuf::from(&s.dir).join(PROJECT_DB)).is_ok());
    }
}

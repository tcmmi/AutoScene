//! In-app updates for Windows, macOS and Linux (AppImage, .deb, .rpm).
//!
//! Wraps `tauri-plugin-updater` in three commands the webview drives:
//! `update_check`, `update_install`, `update_relaunch`. Keeping the logic in
//! Rust means the webview needs no updater capability, and in dev builds the
//! local override file below can fake an update without a release server.
//!
//! ## Release side
//!
//! `bundle.createUpdaterArtifacts` makes `tauri build` emit signed updater
//! bundles. Publish them next to a `latest.json` manifest at
//! `plugins.updater.endpoints`. Signing needs `TAURI_SIGNING_PRIVATE_KEY`; the
//! matching public key is `plugins.updater.pubkey`.
//!
//! ## Local override — `~/.autoscene/updates.json` (dev builds only)
//!
//! ```json
//! {
//!   "endpoint": "http://127.0.0.1:8000/latest.json",
//!   "simulate": { "version": "0.3.0", "notes": "What's new…", "date": "2026-10-01" }
//! }
//! ```
//!
//! Both keys are optional. `endpoint` repoints the check (staging). `simulate`
//! short-circuits it: `update_check` reports that version as available and
//! `update_install` runs the progress arc without touching the installed app.
//! A file is used rather than an env var because an app launched from Finder
//! or the Start menu inherits no shell environment. `tauri dev` and
//! `tauri build --debug` builds (installers included) read it; release builds
//! never do, so a file left behind (or planted) can't fake an update, hide
//! real ones, or point the check at another feed.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;
use tauri_specta::Event;

use crate::error::{AppError, AppResult, ErrorCode};

/// Simulated downloads pretend to be this big, over this long.
const FAKE_BYTES: u64 = 48 * 1024 * 1024;
const FAKE_STEPS: u64 = 40;
const FAKE_STEP_MS: u64 = 60;

/// How often a download tells its progress. Four times a second keeps the bar
/// moving smoothly; one event per network chunk (hundreds a second on a fast
/// line) only made the webview re-render for nothing.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

#[derive(Debug, Default, Deserialize)]
struct LocalConfig {
    endpoint: Option<String>,
    simulate: Option<Simulated>,
}

#[derive(Debug, Clone, Deserialize)]
struct Simulated {
    version: Option<String>,
    notes: Option<String>,
    date: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum UpdateState {
    Available,
    Current,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub state: UpdateState,
    /// Version running right now.
    pub current_version: String,
    /// Version on offer. Only set when `state == "available"`.
    pub version: Option<String>,
    /// Release notes, as written in the manifest.
    pub notes: Option<String>,
    /// Publish date, `YYYY-MM-DD`.
    pub date: Option<String>,
    /// True when the answer came from the local override, not a real feed.
    pub simulated: bool,
}

/// Download progress. `downloaded == 0 && total == None` after the download
/// finishes means "installing".
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[tauri_specta(event_name = "update:progress")]
pub struct UpdateProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
}

/// Whether this build reads the local override: dev builds only.
const OVERRIDE_ALLOWED: bool = cfg!(debug_assertions);

fn config_path() -> std::path::PathBuf {
    as_core::AppPaths::legacy_dot_dir().join("updates.json")
}

fn local_config() -> LocalConfig {
    read_override(&config_path(), OVERRIDE_ALLOWED)
}

/// The override at `path` when `allowed`. Missing or malformed file means
/// "no override" — never a failed check.
fn read_override(path: &std::path::Path, allowed: bool) -> LocalConfig {
    if !allowed {
        return LocalConfig::default();
    }
    let Ok(raw) = std::fs::read_to_string(path) else {
        return LocalConfig::default();
    };
    match serde_json::from_str::<LocalConfig>(&raw) {
        Ok(cfg) => {
            log::warn!("Updates follow the dev override in ~/.autoscene/updates.json");
            cfg
        }
        Err(e) => {
            log::warn!("Ignoring ~/.autoscene/updates.json: {e}");
            LocalConfig::default()
        }
    }
}

fn update_err(e: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::Update, e.to_string())
}

fn updater_for(app: &AppHandle, cfg: &LocalConfig) -> AppResult<tauri_plugin_updater::Updater> {
    // On Windows the installer starts and the plugin calls `exit(0)` without
    // a RunEvent::Exit: stop our jobs first (this replaces the plugin's
    // default hook, so run its cleanup too).
    let handle = app.clone();
    let mut builder = app.updater_builder().on_before_exit(move || {
        crate::state::shutdown_work(&handle);
        handle.cleanup_before_exit();
    });
    if let Some(endpoint) = &cfg.endpoint {
        let url = endpoint.parse().map_err(|e| update_err(format!("Bad endpoint in updates.json: {e}")))?;
        builder = builder.endpoints(vec![url]).map_err(update_err)?;
    }
    builder.build().map_err(update_err)
}

fn simulated_status(sim: &Simulated, current: String) -> UpdateStatus {
    UpdateStatus {
        state: UpdateState::Available,
        current_version: current,
        version: Some(sim.version.clone().unwrap_or_else(|| "0.3.0".into())),
        notes: Some(sim.notes.clone().unwrap_or_else(|| {
            "Simulated release notes. Set `simulate.notes` in ~/.autoscene/updates.json to change this text.".into()
        })),
        date: sim.date.clone(),
        simulated: true,
    }
}

/// Ask the configured feed whether a newer build exists. Errors are surfaced
/// so a manual check can say *why* it failed; the launch check stays quiet.
#[tauri::command]
#[specta::specta]
pub async fn update_check(app: AppHandle) -> AppResult<UpdateStatus> {
    let current = app.package_info().version.to_string();
    let cfg = local_config();
    if let Some(sim) = &cfg.simulate {
        log::info!("Update check answered from ~/.autoscene/updates.json (simulate)");
        return Ok(simulated_status(sim, current));
    }
    let status = match updater_for(&app, &cfg)?.check().await.map_err(update_err)? {
        Some(update) => UpdateStatus {
            state: UpdateState::Available,
            current_version: current,
            version: Some(update.version.clone()),
            notes: update.body.clone(),
            date: update.date.map(|d| d.date().to_string()),
            simulated: false,
        },
        None => UpdateStatus {
            state: UpdateState::Current,
            current_version: current,
            version: None,
            notes: None,
            date: None,
            simulated: false,
        },
    };
    Ok(status)
}

/// Download and install the pending update, emitting [`UpdateProgress`].
/// Re-checks the feed rather than caching a handle so it is safe to retry.
#[tauri::command]
#[specta::specta]
pub async fn update_install(app: AppHandle) -> AppResult<()> {
    let cfg = local_config();
    if cfg.simulate.is_some() {
        return simulate_install(&app).await;
    }
    let update = updater_for(&app, &cfg)?
        .check()
        .await
        .map_err(update_err)?
        .ok_or_else(|| update_err("That update is no longer available."))?;

    let progress_app = app.clone();
    let (on_chunk, on_finish) = download_progress(Instant::now, move |downloaded, total| {
        emit_progress(&progress_app, downloaded, total);
    });
    update.download_and_install(on_chunk, on_finish).await.map_err(|e| {
        // On Windows our jobs were stopped just before the installer was
        // to start; it didn't, so the app keeps running and needs them.
        crate::state::resume_work(&app);
        update_err(e)
    })
}

/// Restart into the freshly installed build. A no-op under the simulate
/// override, where nothing was installed. Returns whether a restart happened.
///
/// `request_restart` goes through RunEvent::Exit, where background jobs are
/// shut down; `restart()` on the main thread would skip it.
#[tauri::command]
#[specta::specta]
pub fn update_relaunch(app: AppHandle) -> bool {
    if local_config().simulate.is_some() {
        log::info!("Simulated update: skipping relaunch");
        return false;
    }
    app.request_restart();
    true
}

/// Adds up a download's chunks and hands the running count to `emit` at most
/// once per `interval`, and always the chunk that reaches a known total, so
/// the bar ends full. A download of unknown size has no end of its own: the
/// caller's "installing" sentinel follows it. Nothing is told before the first
/// byte, so a count can't pass for that sentinel (`0` with no total).
fn throttled(
    interval: Duration,
    now: impl Fn() -> Instant,
    mut emit: impl FnMut(u64, Option<u64>),
) -> impl FnMut(usize, Option<u64>) {
    let mut downloaded: u64 = 0;
    let mut last: Option<Instant> = None;
    let mut told_end = false;
    move |chunk, total| {
        downloaded = downloaded.saturating_add(chunk as u64);
        if downloaded == 0 {
            return;
        }
        let at = now();
        let reaches_end = !told_end && total.is_some_and(|t| downloaded >= t);
        let due = last.is_none_or(|l| at.saturating_duration_since(l) >= interval);
        if reaches_end || due {
            told_end |= reaches_end;
            last = Some(at);
            emit(downloaded, total);
        }
    }
}

/// The two callbacks of a real download: its chunks, throttled, and its end,
/// which tells the "installing" sentinel at once (never throttled away).
fn download_progress(
    now: impl Fn() -> Instant,
    emit: impl Fn(u64, Option<u64>) + Clone,
) -> (impl FnMut(usize, Option<u64>), impl FnOnce()) {
    let tell = emit.clone();
    let on_finish = move || {
        log::info!("Update download finished; installing");
        tell(0, None);
    };
    (throttled(PROGRESS_EVERY, now, emit), on_finish)
}

fn emit_progress(app: &AppHandle, downloaded: u64, total: Option<u64>) {
    let ev = UpdateProgress { downloaded, total };
    if let Err(e) = ev.emit(app) {
        log::warn!("update progress emit failed: {e}");
    }
}

async fn simulate_install(app: &AppHandle) -> AppResult<()> {
    log::info!("Simulated update: faking a {FAKE_BYTES}-byte download");
    let mut on_chunk = throttled(PROGRESS_EVERY, Instant::now, |downloaded, total| {
        emit_progress(app, downloaded, total);
    });
    for step in 1..=FAKE_STEPS {
        tokio::time::sleep(Duration::from_millis(FAKE_STEP_MS)).await;
        // Chunks that add up to exactly FAKE_BYTES, like a real download's.
        let chunk = FAKE_BYTES * step / FAKE_STEPS - FAKE_BYTES * (step - 1) / FAKE_STEPS;
        on_chunk(chunk as usize, Some(FAKE_BYTES));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use super::*;

    #[test]
    fn the_override_is_read_only_where_allowed() {
        let path = std::env::temp_dir().join(format!("autoscene-updates-{}.json", std::process::id()));
        std::fs::write(
            &path,
            r#"{ "endpoint": "https://example.test/latest.json", "simulate": { "version": "9.9.9" } }"#,
        )
        .unwrap();
        let read = read_override(&path, true);
        assert_eq!(read.endpoint.as_deref(), Some("https://example.test/latest.json"));
        assert_eq!(read.simulate.and_then(|s| s.version).as_deref(), Some("9.9.9"));
        // A release build: the real feed, whatever the file says.
        let release = read_override(&path, false);
        assert!(release.endpoint.is_none() && release.simulate.is_none());

        std::fs::write(&path, "not json").unwrap();
        assert!(read_override(&path, true).simulate.is_none());
        let _ = std::fs::remove_file(&path);
        assert!(read_override(&path, true).endpoint.is_none());
    }

    /// What a download told: the running count and its total, in order.
    type Told = Rc<RefCell<Vec<(u64, Option<u64>)>>>;

    /// A hand-turned clock and a log of what `throttled` told.
    struct Rig {
        clock: Rc<Cell<Instant>>,
        told: Told,
    }

    impl Rig {
        fn new() -> Self {
            Rig { clock: Rc::new(Cell::new(Instant::now())), told: Told::default() }
        }

        fn callbacks(&self) -> (impl FnMut(usize, Option<u64>) + use<>, impl FnOnce() + use<>) {
            let clock = self.clock.clone();
            let told = self.told.clone();
            download_progress(move || clock.get(), move |d, t| told.borrow_mut().push((d, t)))
        }

        fn throttled(&self) -> impl FnMut(usize, Option<u64>) + use<> {
            self.callbacks().0
        }

        fn advance(&self, by: Duration) {
            self.clock.set(self.clock.get() + by);
        }
    }

    #[test]
    fn update_progress_is_told_at_most_four_times_a_second_and_at_the_end() {
        // A known total: 1000 chunks of 1 KB over one second.
        let rig = Rig::new();
        let mut on_chunk = rig.throttled();
        for _ in 0..1000 {
            on_chunk(1000, Some(1_000_000));
            rig.advance(Duration::from_millis(1));
        }
        let told = rig.told.borrow().clone();
        assert!(told.len() <= 5, "told {} times in one second", told.len());
        assert_eq!(told.first(), Some(&(1000, Some(1_000_000))), "the first chunk shows at once");
        assert_eq!(told.last(), Some(&(1_000_000, Some(1_000_000))), "the bar ends full");
        assert!(told.windows(2).all(|w| w[0].0 < w[1].0), "the count only grows");

        // An unknown total: at most one event per 250 ms, and no end of its own.
        let rig = Rig::new();
        let mut on_chunk = rig.throttled();
        let mut at = Vec::new();
        for i in 0..1000u64 {
            let before = rig.told.borrow().len();
            on_chunk(1000, None);
            if rig.told.borrow().len() > before {
                at.push(i);
            }
            rig.advance(Duration::from_millis(1));
        }
        let told = rig.told.borrow().clone();
        assert!(told.len() <= 4, "told {} times in one second", told.len());
        assert!(at.windows(2).all(|w| w[1] - w[0] >= 250), "events {at:?} closer than 250 ms");
        assert!(told.iter().all(|&(_, total)| total.is_none()));
        assert!(told.last().is_some_and(|&(d, _)| d < 1_000_000), "nothing pretends the end is known");

        // Nothing before the first byte: a count of 0 with no total is the
        // "installing" sentinel, which only the finish callback tells.
        let rig = Rig::new();
        let mut on_chunk = rig.throttled();
        on_chunk(0, None);
        assert!(rig.told.borrow().is_empty());

        // The end is told even right after a throttled event, and only once.
        let rig = Rig::new();
        let mut on_chunk = rig.throttled();
        on_chunk(10, Some(20));
        on_chunk(10, Some(20));
        on_chunk(0, Some(20));
        assert_eq!(*rig.told.borrow(), vec![(10, Some(20)), (20, Some(20))]);

        // The sentinel is never throttled away: told at once, even 1 ms after
        // a chunk was told, for a download of unknown size.
        let rig = Rig::new();
        let (mut on_chunk, on_finish) = rig.callbacks();
        on_chunk(500, None);
        rig.advance(Duration::from_millis(1));
        on_chunk(500, None);
        on_finish();
        assert_eq!(*rig.told.borrow(), vec![(500, None), (0, None)]);
    }
}

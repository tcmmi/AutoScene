//! A run as a WebAssembly plugin gets it: the protocol's run message
//! ([`RunMessage`]) as the world's `run-request`, or an event's as its
//! `event-request`, with its folders named as the plugin sees them.
//!
//! Its data folder is `/data`; the export's folder `/export` and the
//! project's `/project`, each only while the run's grants give it
//! ([`mounts`]); a clip or merged file under the export's folder is
//! `/export/<name>` then, and none otherwise, as is a clip outside it. What
//! a plugin can't use is left out: FFmpeg (a component starts no program),
//! the episodes' files, and its secret settings, which reach it only
//! through `host.secret`.

use std::path::{Component, Path, PathBuf};

use super::bindings::Request;
use super::bindings::v1_1::autoscene::plugin::types as wit;
use crate::calls::Grants;
use as_core::{Msg, msg};

use crate::error::{PluginError, PluginResult};
use crate::manifest::{Permission, SettingValue};
use crate::protocol::{ActionKind, RunMessage, Trigger};

/// The folders of a run beside `/data`, on the host.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Mounts {
    /// The export's folder, as `/export`.
    pub export: Option<PathBuf>,
    /// `/export` may be changed (`exports:write`).
    pub export_write: bool,
    /// The project's folder, as `/project`, read only.
    pub project: Option<PathBuf>,
}

/// The folders `message` runs on that `grants` give the plugin: the
/// export's with `exports:read` or `exports:write`, the project's with
/// `project:read`. Each must still be a folder.
pub(crate) fn mounts(message: &RunMessage, grants: &Grants) -> PluginResult<Mounts> {
    let folder = |path: &str, gone: Msg| {
        let path = PathBuf::from(path);
        match path.is_dir() {
            true => Ok(path),
            false => Err(PluginError::Invalid(gone)),
        }
    };
    let reads_export = grants.has(Permission::ExportsRead) || grants.has(Permission::ExportsWrite);
    let export = match &message.export {
        Some(e) if reads_export => {
            Some(folder(&e.out_dir, msg!("plugin.exportFolderGone", "The export's folder is gone."))?)
        }
        _ => None,
    };
    let project = match &message.project {
        Some(p) if grants.has(Permission::ProjectRead) => {
            Some(folder(&p.folder, msg!("plugin.projectFolderGone", "The project's folder is gone."))?)
        }
        _ => None,
    };
    let export_write = export.is_some() && grants.has(Permission::ExportsWrite);
    Ok(Mounts { export, export_write, project })
}

/// The host path `host` as the plugin sees it: under `guest_root`, which
/// stands for the host folder `root`, when `host` is in it; else none.
/// Each part of it must be a plain name in UTF-8 (no `..`, no prefix), and
/// they are joined by `/` whatever the host writes.
pub(crate) fn guest_path(root: &Path, guest_root: &str, host: &str) -> Option<String> {
    let rest = Path::new(host).strip_prefix(root).ok()?;
    let mut path = guest_root.to_owned();
    for part in rest.components() {
        let Component::Normal(name) = part else { return None };
        path.push('/');
        path.push_str(name.to_str()?);
    }
    Some(path)
}

/// `message` as the world's request, its folders as `mounts` gives them:
/// an event's run as an `event-request` for `on-event`, any other as a
/// `run-request` for `run`. Its protocol is the one the plugin declared,
/// and a setting `grants` names a secret is left out. A shot filter's run
/// has no request: a component can't read the episodes it judges, and its
/// manifest can't name a filter ([`crate::manifest`]). Nor has a pack
/// action's: a component can't write the file the person chose.
pub(crate) fn request(message: &RunMessage, mounts: &Mounts, grants: &Grants) -> PluginResult<Request> {
    let settings = message
        .settings
        .iter()
        .filter(|(key, _)| !grants.secrets.contains(key))
        .map(|(key, value)| {
            let value = match value {
                SettingValue::Bool(b) => wit::SettingValue::Boolean(*b),
                SettingValue::Number(n) => wit::SettingValue::Number(*n),
                SettingValue::Text(t) => wit::SettingValue::Text(t.clone()),
            };
            (key.clone(), value)
        })
        .collect();
    let plugin = wit::PluginContext {
        id: message.plugin.id.as_str().to_owned(),
        version: message.plugin.version.clone(),
        data_dir: "/data".into(),
    };
    let app = wit::AppContext { version: message.app.version.clone(), platform: message.app.platform.clone() };
    let project = message.project.as_ref().map(|p| wit::ProjectContext {
        id: p.id.clone(),
        title: p.title.clone(),
        folder: mounts.project.as_ref().map(|_| "/project".into()),
    });
    // An event has a function of its own: the cases of the 1.0 world's
    // action-kind and trigger never grow (a 1.0 plugin wouldn't know one).
    let no_event = || PluginError::Invalid(msg!("plugin.eventSaysWhat", "An event's run says what happened."));
    let is_event = matches!(message.action.kind, ActionKind::Event) || matches!(message.action.trigger, Trigger::Event);
    if is_event {
        let (ActionKind::Event, Trigger::Event, Some(event)) =
            (message.action.kind, message.action.trigger, &message.event)
        else {
            return Err(no_event());
        };
        return Ok(Request::Event(wit::EventRequest {
            protocol: grants.protocol,
            run_id: message.run_id.clone(),
            plugin,
            app,
            settings,
            id: message.action.id.clone(),
            project,
            event: wit::EventContext {
                kind: event.kind.name().into(),
                at: event.at,
                episodes: wit::EpisodeCounts { done: event.episodes.done, failed: event.episodes.failed },
            },
        }));
    }
    // Every kind and trigger has its case in the 1.0 world; one that a
    // later protocol adds needs a newer world (or a function of its own),
    // never a case that a 1.0 plugin doesn't know.
    let no_filter = || PluginError::Invalid(msg!("plugin.wasmNoFilter", "A WebAssembly plugin can't filter shots."));
    let kind = match message.action.kind {
        ActionKind::ExportAction => wit::ActionKind::ExportAction,
        ActionKind::Command => wit::ActionKind::Command,
        ActionKind::Event => return Err(no_event()),
        ActionKind::ShotFilter => return Err(no_filter()),
        ActionKind::PackAction => {
            return Err(PluginError::Invalid(msg!(
                "plugin.wasmNoPack",
                "A WebAssembly plugin can't save a scenepack."
            )));
        }
    };
    let trigger = match message.action.trigger {
        Trigger::Menu => wit::Trigger::Menu,
        Trigger::AfterExport => wit::Trigger::AfterExport,
        Trigger::Palette => wit::Trigger::Palette,
        Trigger::Page => wit::Trigger::Page,
        Trigger::Event => return Err(no_event()),
        Trigger::Pack => return Err(no_filter()),
    };
    let in_export = |path: &str| mounts.export.as_deref().and_then(|root| guest_path(root, "/export", path));
    Ok(Request::Run(wit::RunRequest {
        protocol: grants.protocol,
        run_id: message.run_id.clone(),
        plugin,
        app,
        settings,
        action: wit::ActionContext { kind, id: message.action.id.clone(), trigger },
        project,
        export: message.export.as_ref().map(|e| wit::ExportContext {
            id: e.id.clone(),
            label: e.label.clone(),
            preset: e.preset.clone(),
            out_dir: mounts.export.as_ref().map(|_| "/export".into()),
            created_at: e.created_at,
            merged_file: e.merged_file.as_deref().and_then(in_export),
            clips: e
                .clips
                .iter()
                .map(|c| wit::ClipContext {
                    name: c.name.clone(),
                    path: c.path.as_deref().and_then(in_export),
                    status: c.status.clone(),
                    media_id: c.media_id.clone(),
                    episode: c.episode.clone(),
                    start: c.start,
                    end: c.end,
                    fps: c.fps.clone(),
                })
                .collect(),
        }),
        // A page's input is at most 64 KiB: `actions.run` takes no more.
        input: message.input.as_ref().map(|v| v.to_string()),
    }))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;

    use serde_json::json;

    use super::*;
    use crate::manifest::EventKind;
    use crate::manifest::{PluginId, RuntimeKind};
    use crate::protocol::{
        ActionContext, AppContext, ClipContext, EpisodeCounts, EventContext, ExportContext, PluginContext,
        ProjectContext, ToolPaths,
    };

    fn grants(permissions: &[Permission]) -> Grants {
        Grants {
            runtime: RuntimeKind::Wasm,
            permissions: permissions.to_vec(),
            hosts: Vec::new(),
            secrets: vec!["token".into()],
            protocol: 2,
        }
    }

    fn clip(name: &str, path: Option<String>) -> ClipContext {
        ClipContext {
            name: name.into(),
            path,
            status: "done".into(),
            media_id: "m1".into(),
            source_path: "E:/Shows/S01E01.mkv".into(),
            episode: "S01E01".into(),
            start: 100,
            end: 200,
            fps: Some("24000/1001".into()),
        }
    }

    /// A run on an export in `root/export` of a project in `root/project`.
    fn message(root: &Path) -> RunMessage {
        let out = root.join("export");
        let at = |name: &str| Some(out.join(name).to_string_lossy().into_owned());
        RunMessage {
            event: None,
            protocol: 1,
            run_id: "run-1".into(),
            plugin: PluginContext {
                id: PluginId::parse("dev.autoscene.hello").unwrap(),
                version: "1.0.0".into(),
                data_dir: root.join("data").to_string_lossy().into_owned(),
                resources_dir: None,
                temp_dir: None,
            },
            app: AppContext { version: "1.1.0".into(), platform: "windows-x86_64".into() },
            tools: ToolPaths { ffmpeg: "ffmpeg".into(), ffprobe: "ffprobe".into(), ffmpeg_version: "9.0".into() },
            settings: BTreeMap::from([
                ("zeta".into(), SettingValue::Bool(true)),
                ("alpha".into(), SettingValue::Number(2.5)),
                ("token".into(), SettingValue::Text("never".into())),
                ("mid".into(), SettingValue::Text("hi".into())),
            ]),
            action: ActionContext { kind: ActionKind::ExportAction, id: "post".into(), trigger: Trigger::Menu },
            project: Some(ProjectContext {
                id: "p1".into(),
                title: "Breaking".into(),
                folder: root.join("project").to_string_lossy().into_owned(),
            }),
            export: Some(ExportContext {
                status: None,
                id: "e1".into(),
                label: Some("Cut".into()),
                preset: "h264".into(),
                out_dir: out.to_string_lossy().into_owned(),
                created_at: 1_700_000_000_000,
                merged_file: at("All.mp4"),
                clips: vec![
                    clip("Clip 1", at("Clip 1.mp4")),
                    clip("Elsewhere", Some(root.join("outside").join("x.mp4").to_string_lossy().into_owned())),
                    clip("Not yet", None),
                ],
            }),
            input: None,
            shots: None,
            output: None,
            pack: None,
        }
    }

    /// The `run-request` of a run that isn't an event's.
    fn run(r: PluginResult<Request>) -> wit::RunRequest {
        match r.unwrap() {
            Request::Run(r) => r,
            Request::Event(_) => panic!("an event's request"),
        }
    }

    fn folders() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("export")).unwrap();
        fs::create_dir(dir.path().join("project")).unwrap();
        dir
    }

    #[test]
    fn folders_are_given_by_the_grants() {
        let dir = folders();
        let m = message(dir.path());
        assert_eq!(mounts(&m, &grants(&[])).unwrap(), Mounts::default());
        let read = mounts(&m, &grants(&[Permission::ExportsRead])).unwrap();
        assert_eq!((read.export, read.export_write, read.project), (Some(dir.path().join("export")), false, None));
        let write = mounts(&m, &grants(&[Permission::ExportsWrite])).unwrap();
        assert_eq!((write.export.is_some(), write.export_write), (true, true));
        let project = mounts(&m, &grants(&[Permission::ProjectRead])).unwrap();
        assert_eq!((project.export, project.project), (None, Some(dir.path().join("project"))));
        // A run without an export or a project gets neither folder.
        let bare = RunMessage { export: None, project: None, ..m };
        let all = [Permission::ExportsWrite, Permission::ProjectRead];
        assert_eq!(mounts(&bare, &grants(&all)).unwrap(), Mounts::default());
    }

    #[test]
    fn a_folder_that_is_gone_is_said() {
        let dir = tempfile::tempdir().unwrap();
        let m = message(dir.path());
        let e = mounts(&m, &grants(&[Permission::ExportsRead])).unwrap_err();
        assert_eq!(e.to_string(), "The export's folder is gone.");
        let e = mounts(&m, &grants(&[Permission::ProjectRead])).unwrap_err();
        assert_eq!(e.to_string(), "The project's folder is gone.");
        // Not given, not looked at.
        assert!(mounts(&m, &grants(&[])).is_ok());
    }

    #[test]
    fn host_paths_become_paths_in_the_sandbox() {
        let root = Path::new("/srv/exports/run");
        assert_eq!(guest_path(root, "/export", "/srv/exports/run/Clip 1.mp4").as_deref(), Some("/export/Clip 1.mp4"));
        assert_eq!(guest_path(root, "/export", "/srv/exports/run/a/b.mp4").as_deref(), Some("/export/a/b.mp4"));
        assert_eq!(guest_path(root, "/export", "/srv/exports/run").as_deref(), Some("/export"));
        assert_eq!(guest_path(root, "/export", "/srv/exports/other/x.mp4"), None);
        assert_eq!(guest_path(root, "/export", "/srv/exports/run/../other/x.mp4"), None);
        assert_eq!(guest_path(root, "/export", "/srv/exports/run2/x.mp4"), None);
        assert_eq!(guest_path(root, "/export", "x.mp4"), None);
    }

    #[cfg(windows)]
    #[test]
    fn windows_paths_become_paths_in_the_sandbox() {
        let root = Path::new(r"C:\Users\me\Exports\run");
        assert_eq!(
            guest_path(root, "/export", r"C:\Users\me\Exports\run\sub\Clip 1.mp4").as_deref(),
            Some("/export/sub/Clip 1.mp4")
        );
        assert_eq!(guest_path(root, "/export", r"D:\Users\me\Exports\run\x.mp4"), None);
    }

    #[test]
    fn the_request_names_only_what_the_plugin_sees() {
        let dir = folders();
        let m = message(dir.path());
        let g = grants(&[Permission::ExportsRead, Permission::ProjectRead]);
        let r = run(request(&m, &mounts(&m, &g).unwrap(), &g));
        assert_eq!((r.protocol, r.run_id.as_str(), r.plugin.data_dir.as_str()), (2, "run-1", "/data"));
        assert_eq!(r.plugin.id, "dev.autoscene.hello");
        // Sorted by key, and never a secret.
        let keys: Vec<&str> = r.settings.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["alpha", "mid", "zeta"]);
        assert!(matches!(r.settings[0].1, wit::SettingValue::Number(n) if n == 2.5));
        assert!(matches!(r.settings[2].1, wit::SettingValue::Boolean(true)));
        let p = r.project.unwrap();
        assert_eq!((p.id.as_str(), p.folder.as_deref()), ("p1", Some("/project")));
        let e = r.export.unwrap();
        assert_eq!((e.out_dir.as_deref(), e.merged_file.as_deref()), (Some("/export"), Some("/export/All.mp4")));
        let paths: Vec<Option<&str>> = e.clips.iter().map(|c| c.path.as_deref()).collect();
        assert_eq!(paths, [Some("/export/Clip 1.mp4"), None, None]);
        assert_eq!((e.clips[0].start, e.clips[0].end, e.clips[0].fps.as_deref()), (100, 200, Some("24000/1001")));
        assert!(matches!(r.action.kind, wit::ActionKind::ExportAction));
        assert!(matches!(r.action.trigger, wit::Trigger::Menu));
        assert_eq!(r.input, None);
    }

    #[test]
    fn without_its_folders_a_run_gets_their_names_only() {
        let dir = folders();
        let m = message(dir.path());
        let g = grants(&[]);
        let r = run(request(&m, &mounts(&m, &g).unwrap(), &g));
        assert_eq!(r.project.unwrap().folder, None);
        let e = r.export.unwrap();
        assert_eq!((e.out_dir, e.merged_file), (None, None));
        assert!(e.clips.iter().all(|c| c.path.is_none()));
        assert_eq!(e.clips[0].name, "Clip 1");
    }

    #[test]
    fn a_page_run_carries_its_input() {
        let dir = folders();
        let mut m = message(dir.path());
        m.action = ActionContext { kind: ActionKind::Command, id: "count".into(), trigger: Trigger::Page };
        m.input = Some(json!({"words": ["a", "b"]}));
        let g = grants(&[]);
        let r = run(request(&m, &Mounts::default(), &g));
        assert!(matches!(r.action.kind, wit::ActionKind::Command));
        assert!(matches!(r.action.trigger, wit::Trigger::Page));
        assert_eq!(r.input.as_deref(), Some(r#"{"words":["a","b"]}"#));
        for (trigger, wit) in [(Trigger::AfterExport, "Trigger::AfterExport"), (Trigger::Palette, "Trigger::Palette")] {
            m.action.trigger = trigger;
            assert_eq!(format!("{:?}", run(request(&m, &Mounts::default(), &g)).action.trigger), wit);
        }
        // A shot filter's run is never a component's, nor is a pack action's.
        m.action = ActionContext { kind: ActionKind::ShotFilter, id: "speaks".into(), trigger: Trigger::Pack };
        assert!(matches!(request(&m, &Mounts::default(), &g), Err(PluginError::Invalid(_))));
        m.action = ActionContext { kind: ActionKind::PackAction, id: "fcpxml".into(), trigger: Trigger::Menu };
        assert!(matches!(request(&m, &Mounts::default(), &g), Err(PluginError::Invalid(_))));
    }

    #[test]
    fn an_event_s_run_is_an_event_request() {
        let dir = folders();
        let mut m = message(dir.path());
        m.action = ActionContext { kind: ActionKind::Event, id: "done".into(), trigger: Trigger::Event };
        m.export = None;
        m.event = Some(EventContext {
            kind: EventKind::AnalysisFinished,
            at: 1_762_150_000_000,
            episodes: EpisodeCounts { done: 7, failed: 1 },
        });
        let g = grants(&[Permission::ProjectRead]);
        let Request::Event(e) = request(&m, &mounts(&m, &g).unwrap(), &g).unwrap() else {
            panic!("not an event's request");
        };
        assert_eq!((e.protocol, e.run_id.as_str(), e.id.as_str()), (2, "run-1", "done"));
        assert_eq!((e.plugin.id.as_str(), e.plugin.data_dir.as_str()), ("dev.autoscene.hello", "/data"));
        let keys: Vec<&str> = e.settings.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["alpha", "mid", "zeta"]);
        let p = e.project.unwrap();
        assert_eq!((p.id.as_str(), p.folder.as_deref()), ("p1", Some("/project")));
        let said = (e.event.kind.as_str(), e.event.at, e.event.episodes.done, e.event.episodes.failed);
        assert_eq!(said, ("analysisFinished", 1_762_150_000_000, 7, 1));
        // An event's run that doesn't say what happened, or a run that is
        // half an event's, has no request.
        for (kind, trigger, event) in [
            (ActionKind::Event, Trigger::Event, None),
            (ActionKind::Event, Trigger::Menu, m.event.clone()),
            (ActionKind::Command, Trigger::Event, m.event.clone()),
        ] {
            let half = RunMessage { action: ActionContext { kind, id: "done".into(), trigger }, event, ..m.clone() };
            let refused = request(&half, &Mounts::default(), &g).unwrap_err().to_string();
            assert_eq!(refused, "An event's run says what happened.");
        }
    }
}

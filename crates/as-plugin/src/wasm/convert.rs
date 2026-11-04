//! A run as a WebAssembly plugin gets it: the protocol's run message
//! ([`RunMessage`]) as the world's `run-request`, with its folders named as
//! the plugin sees them.
//!
//! Its data folder is `/data`; the export's folder `/export` and the
//! project's `/project`, each only while the run's grants give it
//! ([`mounts`]); a clip or merged file under the export's folder is
//! `/export/<name>` then, and none otherwise, as is a clip outside it. What
//! a plugin can't use is left out: FFmpeg (a component starts no program),
//! the episodes' files, and its secret settings, which reach it only
//! through `host.secret`.

use std::path::{Component, Path, PathBuf};

use super::bindings::autoscene::plugin::types as wit;
use crate::calls::Grants;
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
    let folder = |path: &str, gone: &str| {
        let path = PathBuf::from(path);
        match path.is_dir() {
            true => Ok(path),
            false => Err(PluginError::Invalid(gone.into())),
        }
    };
    let reads_export = grants.has(Permission::ExportsRead) || grants.has(Permission::ExportsWrite);
    let export = match &message.export {
        Some(e) if reads_export => Some(folder(&e.out_dir, "The export's folder is gone.")?),
        _ => None,
    };
    let project = match &message.project {
        Some(p) if grants.has(Permission::ProjectRead) => Some(folder(&p.folder, "The project's folder is gone.")?),
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

/// `message` as the world's request, its folders as `mounts` gives them.
/// Its protocol is the one the plugin declared, and a setting `grants`
/// names a secret is left out. A run of an event has no request in the 1.0
/// world: its plugin's events are skipped when its `plugin.json` is read,
/// so none is queued.
pub(crate) fn request(message: &RunMessage, mounts: &Mounts, grants: &Grants) -> PluginResult<wit::RunRequest> {
    let in_export = |path: &str| mounts.export.as_deref().and_then(|root| guest_path(root, "/export", path));
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
    // Every kind and trigger has its case in the 1.0 world; one that a
    // later protocol adds needs a newer world (or a function of its own),
    // never a case that a 1.0 plugin doesn't know.
    let no_event = || PluginError::Invalid("A WebAssembly plugin's events need a newer AutoScene.".into());
    let kind = match message.action.kind {
        ActionKind::ExportAction => wit::ActionKind::ExportAction,
        ActionKind::Command => wit::ActionKind::Command,
        ActionKind::Event => return Err(no_event()),
    };
    let trigger = match message.action.trigger {
        Trigger::Menu => wit::Trigger::Menu,
        Trigger::AfterExport => wit::Trigger::AfterExport,
        Trigger::Palette => wit::Trigger::Palette,
        Trigger::Page => wit::Trigger::Page,
        Trigger::Event => return Err(no_event()),
    };
    Ok(wit::RunRequest {
        protocol: grants.protocol,
        run_id: message.run_id.clone(),
        plugin: wit::PluginContext {
            id: message.plugin.id.as_str().to_owned(),
            version: message.plugin.version.clone(),
            data_dir: "/data".into(),
        },
        app: wit::AppContext { version: message.app.version.clone(), platform: message.app.platform.clone() },
        settings,
        action: wit::ActionContext { kind, id: message.action.id.clone(), trigger },
        project: message.project.as_ref().map(|p| wit::ProjectContext {
            id: p.id.clone(),
            title: p.title.clone(),
            folder: mounts.project.as_ref().map(|_| "/project".into()),
        }),
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
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;

    use serde_json::json;

    use super::*;
    use crate::manifest::{PluginId, RuntimeKind};
    use crate::protocol::{
        ActionContext, AppContext, ClipContext, ExportContext, PluginContext, ProjectContext, ToolPaths,
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
        let r = request(&m, &mounts(&m, &g).unwrap(), &g).unwrap();
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
        let r = request(&m, &mounts(&m, &g).unwrap(), &g).unwrap();
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
        let r = request(&m, &Mounts::default(), &g).unwrap();
        assert!(matches!(r.action.kind, wit::ActionKind::Command));
        assert!(matches!(r.action.trigger, wit::Trigger::Page));
        assert_eq!(r.input.as_deref(), Some(r#"{"words":["a","b"]}"#));
        for (trigger, wit) in [(Trigger::AfterExport, "Trigger::AfterExport"), (Trigger::Palette, "Trigger::Palette")] {
            m.action.trigger = trigger;
            assert_eq!(format!("{:?}", request(&m, &Mounts::default(), &g).unwrap().action.trigger), wit);
        }
        // An event's run has no request in the 1.0 world.
        m.action = ActionContext { kind: ActionKind::Event, id: "done".into(), trigger: Trigger::Event };
        let refused = request(&m, &Mounts::default(), &g).unwrap_err().to_string();
        assert_eq!(refused, "A WebAssembly plugin's events need a newer AutoScene.");
    }
}

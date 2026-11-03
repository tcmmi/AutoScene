//! What AutoScene answers a plugin's calls with (protocol 2): its settings
//! and its secrets, its project, the project's exports and their files, and
//! the state it keeps. [`AppCalls`] answers a run's calls, bound to the plugin, project
//! and export the run is for. Each call is checked first (its params' size,
//! then its method against the plugin's grants, then the project and the
//! export it names; see [`as_plugin::calls`]), and only then is anything
//! read. A call of a run that was cancelled, or whose program ended, writes
//! nothing.

use std::path::Path;
use std::sync::Arc;

use as_api::{ExportStatus, ExportSummary, MediaStatus};
use as_core::{CancelToken, ExportId, MediaId, ProjectId};
use as_plugin::calls::{
    self, CallCode, CallError, Caller, ExportRef, FileRef, Grants, HostCalls, HostMethod, PageExportSummary,
    ProgramEpisode, ProgramProject, ProjectRef, ResolvedFile, SecretsGet, StateKey, StateSet,
};
use as_plugin::{Installed, PluginError, PluginId, PluginResult, page_state};
use as_project::{Library, Project, ProjectError, ProjectResult};
use serde::Serialize;
use serde_json::value::RawValue;
use serde_json::{Value, json};

use super::{PluginHost, RunKey, export_context, serde_name};

/// A run's calls, bound to its plugin and the project and export it is
/// for. Its grants are the plugin's as the run started: a change of the
/// plugin stops its runs.
pub struct AppCalls {
    places: Arc<dyn Places>,
    host: Arc<PluginHost>,
    plugin: PluginId,
    grants: Grants,
    project: Option<String>,
    export: Option<String>,
}

/// Where calls find a project and its exports: the library (a test's
/// stand-in has exports no export job wrote).
trait Places: Send + Sync {
    fn project(&self, id: ProjectId) -> ProjectResult<Arc<Project>>;
    fn exports(&self, project: &Project) -> ProjectResult<Vec<ExportSummary>>;
    fn export(&self, project: &Project, id: ExportId) -> ProjectResult<ExportSummary>;
}

impl Places for Library {
    fn project(&self, id: ProjectId) -> ProjectResult<Arc<Project>> {
        Library::project(self, id)
    }

    fn exports(&self, project: &Project) -> ProjectResult<Vec<ExportSummary>> {
        project.exports()
    }

    fn export(&self, project: &Project, id: ExportId) -> ProjectResult<ExportSummary> {
        project.export(id)
    }
}

impl AppCalls {
    /// The calls of the run `key` of `p`.
    pub(super) fn run(library: Arc<Library>, host: Arc<PluginHost>, p: &Installed, key: &RunKey) -> PluginResult<Self> {
        Ok(Self {
            places: library,
            host,
            plugin: p.record.id.clone(),
            grants: Grants::of(p)?,
            project: key.project_id.map(|id| id.to_string()),
            export: key.export_id.map(|id| id.to_string()),
        })
    }

    fn caller(&self) -> Caller<'_> {
        Caller::Run { project: self.project.as_deref(), export: self.export.as_deref() }
    }

    /// The project a call that names `asked` (or none) reads.
    fn project(&self, asked: Option<&str>) -> Result<Arc<Project>, CallError> {
        let id = self.caller().project(asked)?;
        let id = ProjectId::parse(id).map_err(|_| not_found("That project doesn't exist."))?;
        self.places.project(id).map_err(project_error)
    }

    /// The export `id` of `project`, when the call may read it.
    fn export(&self, project: &Project, id: &str) -> Result<ExportSummary, CallError> {
        self.caller().export(id)?;
        let id = ExportId::parse(id).map_err(|_| not_found("That export doesn't exist."))?;
        self.places.export(project, id).map_err(project_error)
    }

    /// The path of `file`, which the call may read.
    fn path(&self, file: &FileRef) -> Result<String, CallError> {
        calls::check_file(file, &self.grants)?;
        let project = self.project(file.project_id())?;
        match file {
            FileRef::Clip { export_id, index, .. } => {
                let export = self.export(&project, export_id)?;
                let clip = export
                    .items
                    .get(*index as usize)
                    .ok_or_else(|| not_found(format!("That export has no clip {index}.")))?;
                clip.path.clone().ok_or_else(|| not_found("That clip isn't written yet."))
            }
            FileRef::Merged { export_id, .. } => {
                let merged = self.export(&project, export_id)?.merged;
                let merged = merged.ok_or_else(|| not_found("That export isn't joined into one file."))?;
                merged.path.ok_or_else(|| not_found("That export's file isn't written yet."))
            }
            FileRef::Episode { media_id, .. } => {
                let id = MediaId::parse(media_id).map_err(|_| not_found("That episode isn't in the project."))?;
                Ok(project.media(id).map_err(project_error)?.path)
            }
        }
    }
}

impl HostCalls for AppCalls {
    fn call(&self, method: HostMethod, params: &RawValue, ended: &CancelToken) -> Result<Value, CallError> {
        // Its size first: nothing else is read of params past their cap.
        calls::check_size(method, params.get().len())?;
        calls::check(method, &self.caller(), &self.grants)?;
        let (store, id) = (&self.host.store, &self.plugin);
        match method {
            HostMethod::SettingsGet => Ok(answer(store.values(id)?)),
            // Read from the keychain at each call, never kept: the value goes
            // only into this reply.
            HostMethod::SecretsGet => {
                let SecretsGet { key } = calls::params(method, params)?;
                calls::check_secret(&key, &self.grants)?;
                Ok(json!({ "value": store.secret(id, &key)? }))
            }
            HostMethod::ProjectGet => {
                let asked: ProjectRef = calls::params(method, params)?;
                let project = self.project(asked.project_id.as_deref())?;
                Ok(answer(program_project(&project).map_err(project_error)?))
            }
            HostMethod::ExportsList => {
                let asked: ProjectRef = calls::params(method, params)?;
                let project = self.project(asked.project_id.as_deref())?;
                let exports = self.places.exports(&project).map_err(project_error)?;
                Ok(answer(exports.iter().map(export_summary).collect::<Vec<_>>()))
            }
            // As an export action's run line has it, with each clip's
            // episode file: a run's program may read it anyway.
            HostMethod::ExportsGet => {
                let asked: ExportRef = calls::params(method, params)?;
                let project = self.project(asked.project_id.as_deref())?;
                let export = self.export(&project, &asked.export_id)?;
                Ok(answer(export_context(&project, &export).map_err(project_error)?))
            }
            HostMethod::FilesResolve => {
                let path = self.path(&calls::params(method, params)?)?;
                let size = std::fs::metadata(&path)
                    .ok()
                    .filter(|f| f.is_file())
                    .ok_or_else(|| not_found("That file isn't where it was."))?
                    .len();
                let mime = calls::media_type(Path::new(&path)).unwrap_or("application/octet-stream").to_owned();
                Ok(answer(ResolvedFile { url: None, path: Some(path), mime, size: size as f64 }))
            }
            HostMethod::StateGet => {
                let StateKey { key } = calls::params(method, params)?;
                Ok(json!({ "value": page_state::get(&store.data_dir(id), &key)? }))
            }
            HostMethod::StateSet => {
                let StateSet { key, value } = calls::params(method, params)?;
                still_running(ended)?;
                page_state::set(&store.data_dir(id), &key, value, ended).map_err(unless_ended)?;
                Ok(Value::Null)
            }
            HostMethod::StateDelete => {
                let StateKey { key } = calls::params(method, params)?;
                still_running(ended)?;
                page_state::delete(&store.data_dir(id), &key, ended).map_err(unless_ended)?;
                Ok(Value::Null)
            }
            HostMethod::StateKeys => Ok(answer(page_state::keys(&store.data_dir(id))?)),
            HostMethod::NetFetch => Err(later(method)),
            // A page's own: `check` refused them to a run.
            HostMethod::ServiceRequest
            | HostMethod::ActionsRun
            | HostMethod::UiToast
            | HostMethod::UiNavigate
            | HostMethod::UiCopyText
            | HostMethod::UiOpenExternal
            | HostMethod::FilesSave
            | HostMethod::FilesOpen => Err(later(method)),
        }
    }
}

/// Whether a call may still change something: not once its run was
/// cancelled or the program that made it ended. A change of the plugin (an
/// uninstall that deletes its data, say) cancels its run and waits for it,
/// and its run for what its calls write.
fn still_running(ended: &CancelToken) -> Result<(), CallError> {
    match ended.is_cancelled() {
        true => Err(run_ended()),
        false => Ok(()),
    }
}

/// The answer of a call that came, or still waited, as its run ended.
fn run_ended() -> CallError {
    CallError::new(CallCode::Cancelled, "This run has ended.")
}

/// `e` as a call gets it, said as [`still_running`] says it when its run
/// ended while the call waited its turn.
fn unless_ended(e: PluginError) -> CallError {
    match e {
        PluginError::Cancelled => run_ended(),
        e => e.into(),
    }
}

/// The answer of a method this AutoScene doesn't answer yet.
fn later(method: HostMethod) -> CallError {
    CallError::new(CallCode::Invalid, format!("{} comes in a later AutoScene.", method.name()))
}

fn not_found(message: impl Into<String>) -> CallError {
    CallError::new(CallCode::NotFound, message)
}

/// `e` as a call gets it. What a plugin can't help (the database, the
/// disk) goes to the app's log too.
fn project_error(e: ProjectError) -> CallError {
    let code = match &e {
        ProjectError::NotFound(_) => CallCode::NotFound,
        ProjectError::Invalid(_) => CallCode::Invalid,
        ProjectError::NotReady(_) => CallCode::Busy,
        ProjectError::Cancelled => CallCode::Cancelled,
        ProjectError::Media(_) | ProjectError::Export(_) | ProjectError::Io(_) | ProjectError::Db(_) => {
            log::warn!("a plugin's call: {}", e.detail());
            CallCode::Failed
        }
    };
    CallError::new(code, e.to_string())
}

/// `value` as a call's result.
fn answer(value: impl Serialize) -> Value {
    serde_json::to_value(value).expect("a call's answer is plain data")
}

/// `project` as a run gets it: with its folder and its episodes' files. An
/// episode is named by its number, else by its file.
fn program_project(project: &Project) -> ProjectResult<ProgramProject> {
    let episodes = project.media_list_uncounted()?.into_iter().map(|m| ProgramEpisode {
        id: m.id.to_string(),
        name: if m.label.is_empty() { m.file_name.clone() } else { m.label.clone() },
        path: m.path,
        frames: m.frames,
        fps: m.fps.map(|fps| fps.to_string()),
        missing: m.status == MediaStatus::Missing,
    });
    Ok(ProgramProject {
        id: project.id.to_string(),
        title: project.title()?,
        folder: project.dir.to_string_lossy().into_owned(),
        episodes: episodes.collect(),
    })
}

/// `export` as `exports.list` lists it.
fn export_summary(export: &ExportSummary) -> PageExportSummary {
    let done = export.items.iter().filter(|i| i.status == ExportStatus::Done).count();
    PageExportSummary {
        id: export.id.to_string(),
        label: export.label.clone(),
        preset: export.preset.id.clone(),
        created_at: export.created_at as f64,
        merged: export.merged.is_some(),
        clips: export.items.len() as u32,
        done: done as u32,
        status: serde_name(&export.status),
    }
}

#[cfg(test)]
mod tests {
    use as_api::{ContentKind, ExportItem, MergedFile, NewProject, ProjectKind};
    use as_plugin::{ActionKind, Permission, RuntimeKind, Trust};

    use super::super::tests::{folder, host_in, key, plugin};
    use super::super::{RunKey, calls_for};
    use super::*;

    /// A call of a run still going, and one of a run `ended` tells about.
    trait Ask {
        fn ask(&self, method: HostMethod, params: Value) -> Result<Value, CallError>;
        fn ask_ended(&self, method: HostMethod, params: Value, ended: &CancelToken) -> Result<Value, CallError>;
    }

    impl Ask for AppCalls {
        fn ask(&self, method: HostMethod, params: Value) -> Result<Value, CallError> {
            self.ask_ended(method, params, &CancelToken::new())
        }

        /// With `params` as a plugin writes them.
        fn ask_ended(&self, method: HostMethod, params: Value, ended: &CancelToken) -> Result<Value, CallError> {
            self.call(method, &serde_json::value::to_raw_value(&params).unwrap(), ended)
        }
    }

    /// The library, with exports of its own.
    struct Stand {
        library: Library,
        exports: Vec<ExportSummary>,
    }

    impl Places for Stand {
        fn project(&self, id: ProjectId) -> ProjectResult<Arc<Project>> {
            self.library.project(id)
        }

        fn exports(&self, _: &Project) -> ProjectResult<Vec<ExportSummary>> {
            Ok(self.exports.clone())
        }

        fn export(&self, _: &Project, id: ExportId) -> ProjectResult<ExportSummary> {
            let e = self.exports.iter().find(|e| e.id == id);
            e.cloned().ok_or_else(|| ProjectError::NotFound("This export no longer exists.".into()))
        }
    }

    /// A project with two episodes (one of them gone from its folder), and
    /// two exports of it in `tmp`: one with a clip written and one not yet,
    /// and one joined into a file.
    struct Place {
        stand: Arc<Stand>,
        host: Arc<PluginHost>,
        project: ProjectId,
        episodes: [MediaId; 2],
        exports: [ExportId; 2],
        files: [String; 3],
    }

    fn place(tmp: &Path) -> Place {
        let (library, host) = host_in(tmp);
        let file = |name: &str| {
            let file = tmp.join("files").join(name);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, b"clip").unwrap();
            file.display().to_string()
        };
        let files = ["Show S01E02.mkv", "Gone.mkv", "One.mp4"].map(file);
        let req = NewProject {
            title: "Show".into(),
            kind: ProjectKind::Series,
            content: ContentKind::Live,
            files: files[..2].to_vec(),
        };
        let (summary, project, _) = library.create_project(&req).unwrap();
        std::fs::remove_file(&files[1]).unwrap();
        let listed = project.media_list_uncounted().unwrap();
        let episodes = ["Show S01E02.mkv", "Gone.mkv"].map(|n| listed.iter().find(|m| m.file_name == n).unwrap().id);
        let clip = |name: &str, path: Option<&str>| ExportItem {
            id: 0,
            media_id: episodes[0],
            start: 10,
            end: 20,
            name: name.into(),
            status: if path.is_some() { ExportStatus::Done } else { ExportStatus::Queued },
            path: path.map(str::to_owned),
            verified: true,
            encoder: None,
            bytes: None,
            error: None,
        };
        let export = |items, merged| ExportSummary {
            id: ExportId::new(),
            project_id: summary.id,
            created_at: 7,
            preset: as_export::builtin_presets()[0].clone(),
            out_dir: tmp.join("files").display().to_string(),
            status: ExportStatus::Running,
            items,
            label: Some("Walter · scenepack".into()),
            merged,
        };
        let merged =
            MergedFile { name: "One".into(), path: Some(files[2].clone()), verified: true, bytes: None, error: None };
        let exports = vec![
            export(vec![clip("One", Some(&files[2])), clip("Two", None)], None),
            export(vec![clip("Three", None)], Some(merged)),
        ];
        let ids = [exports[0].id, exports[1].id];
        drop(project);
        Place { stand: Arc::new(Stand { library, exports }), host, project: summary.id, episodes, exports: ids, files }
    }

    impl Place {
        /// The calls of a run on `export` (a command's, without one), of a
        /// plugin granted `permissions`.
        fn run(&self, export: Option<ExportId>, permissions: &[Permission]) -> AppCalls {
            AppCalls {
                places: self.stand.clone(),
                host: self.host.clone(),
                plugin: PluginId::parse("dev.a.calls").unwrap(),
                grants: Grants {
                    runtime: RuntimeKind::Process,
                    permissions: permissions.to_vec(),
                    hosts: Vec::new(),
                    secrets: Vec::new(),
                    protocol: 2,
                },
                project: Some(self.project.to_string()),
                export: export.map(|e| e.to_string()),
            }
        }
    }

    fn said(r: Result<Value, CallError>) -> (CallCode, String) {
        let e = r.unwrap_err();
        (e.code, e.message)
    }

    #[test]
    fn a_run_gets_its_export_without_another_one() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-calls-export-{}", std::process::id()));
        let place = place(&tmp);
        let [mine, other] = place.exports;
        let run = place.run(Some(mine), &[Permission::ExportsRead]);
        let get = |calls: &AppCalls, export: ExportId| {
            calls.ask(HostMethod::ExportsGet, json!({"exportId": export.to_string()}))
        };

        let got = get(&run, mine).unwrap();
        assert_eq!((got["id"].clone(), got["label"].clone()), (json!(mine.to_string()), json!("Walter · scenepack")));
        let clips = got["clips"].as_array().unwrap();
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[0]["path"], json!(place.files[2]));
        assert_eq!(
            (clips[0]["episode"].clone(), clips[0]["sourcePath"].clone()),
            (json!("S01E02"), json!(place.files[0]))
        );
        // Another export of its project, or its own named as another project's: no.
        assert_eq!(said(get(&run, other)), (CallCode::Denied, "A run reads only the export it runs on.".into()));
        let elsewhere = json!({"projectId": ProjectId::new().to_string(), "exportId": mine.to_string()});
        assert_eq!(
            said(run.ask(HostMethod::ExportsGet, elsewhere)),
            (CallCode::Denied, "A run reads only the project it runs in.".into())
        );
        // A command in the project reads any of its exports, and lists them.
        let command = place.run(None, &[Permission::ExportsRead]);
        assert_eq!(get(&command, other).unwrap()["id"], json!(other.to_string()));
        assert_eq!(said(get(&command, ExportId::new())), (CallCode::NotFound, "This export no longer exists.".into()));
        let listed = run.ask(HostMethod::ExportsList, Value::Null).unwrap();
        assert_eq!(
            listed[0],
            json!({
                "id": mine.to_string(), "label": "Walter · scenepack", "preset": as_export::builtin_presets()[0].id,
                "createdAt": 7.0, "merged": false, "clips": 2, "done": 1, "status": "running"
            })
        );
        assert_eq!((listed[1]["merged"].clone(), listed[1]["done"].clone()), (json!(true), json!(0)));
        // Without exports:read it reads none; the project needs project:read.
        let blind = place.run(Some(mine), &[Permission::ProjectRead]);
        assert_eq!(
            said(get(&blind, mine)),
            (CallCode::Denied, "exports.get needs the exports:read permission.".into())
        );
        assert_eq!(said(run.ask(HostMethod::ProjectGet, json!({}))).0, CallCode::Denied);
        let project = blind.ask(HostMethod::ProjectGet, json!({})).unwrap();
        assert_eq!((project["title"].clone(), project["episodes"].as_array().unwrap().len()), (json!("Show"), 2));
        let gone = project["episodes"].as_array().unwrap().iter().find(|e| e["name"] == "Gone.mkv").unwrap();
        assert_eq!(gone["missing"], json!(true));
        // Params past their cap are refused before anything is read.
        let padded = json!({"exportId": mine.to_string(), "pad": "x".repeat(1024)});
        assert_eq!(said(run.ask(HostMethod::ExportsGet, padded)).0, CallCode::Invalid);
        drop((run, command, blind, place));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn files_resolve_gives_a_run_the_clip_path() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-calls-files-{}", std::process::id()));
        let place = place(&tmp);
        let [mine, merged] = place.exports;
        let run = place.run(None, &[Permission::ExportsRead]);
        let resolve = |calls: &AppCalls, file: Value| calls.ask(HostMethod::FilesResolve, file);
        let clip = |index: u32| json!({"kind": "clip", "exportId": mine.to_string(), "index": index});

        assert_eq!(
            resolve(&run, clip(0)).unwrap(),
            json!({"url": null, "path": place.files[2], "mime": "video/mp4", "size": 4.0})
        );
        assert_eq!(said(resolve(&run, clip(1))), (CallCode::NotFound, "That clip isn't written yet.".into()));
        assert_eq!(said(resolve(&run, clip(5))), (CallCode::NotFound, "That export has no clip 5.".into()));
        let joined = json!({"kind": "merged", "exportId": merged.to_string()});
        assert_eq!(resolve(&run, joined).unwrap()["path"], json!(place.files[2]));
        let apart = json!({"kind": "merged", "exportId": mine.to_string()});
        assert_eq!(said(resolve(&run, apart)), (CallCode::NotFound, "That export isn't joined into one file.".into()));
        // A run on the other export resolves none of this one's clips.
        let on_merged = place.run(Some(merged), &[Permission::ExportsRead]);
        assert_eq!(said(resolve(&on_merged, clip(0))).0, CallCode::Denied);

        // An episode needs project:read, and its file where it was.
        let episode = |media: MediaId| json!({"kind": "episode", "mediaId": media.to_string()});
        assert_eq!(
            said(resolve(&run, episode(place.episodes[0]))),
            (CallCode::Denied, "files.resolve needs the project:read permission for that file.".into())
        );
        let reader = place.run(None, &[Permission::ProjectRead]);
        let found = resolve(&reader, episode(place.episodes[0])).unwrap();
        assert_eq!((found["path"].clone(), found["mime"].clone()), (json!(place.files[0]), json!("video/x-matroska")));
        assert_eq!(
            said(resolve(&reader, episode(place.episodes[1]))),
            (CallCode::NotFound, "That file isn't where it was.".into())
        );
        assert_eq!(said(resolve(&reader, clip(0))).0, CallCode::Denied);
        drop((run, on_merged, reader, place));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_run_keeps_its_state_and_reads_its_settings() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-calls-state-{}", std::process::id()));
        let place = place(&tmp);
        let run = place.run(None, &[]);
        let call = |method, params| run.ask(method, params);
        assert_eq!(call(HostMethod::StateGet, json!({"key": "seen"})).unwrap(), json!({"value": null}));
        // In the data folder its run made.
        let data = place.host.store.data_dir(&run.plugin);
        std::fs::create_dir_all(&data).unwrap();
        assert_eq!(call(HostMethod::StateSet, json!({"key": "seen", "value": [1, 2]})).unwrap(), Value::Null);
        assert_eq!(call(HostMethod::StateGet, json!({"key": "seen"})).unwrap(), json!({"value": [1, 2]}));
        assert_eq!(call(HostMethod::StateKeys, Value::Null).unwrap(), json!(["seen"]));
        assert_eq!(call(HostMethod::StateDelete, json!({"key": "seen"})).unwrap(), Value::Null);
        assert_eq!(said(call(HostMethod::StateGet, json!({"key": "no way"}))).0, CallCode::Invalid);
        assert!(data.join(page_state::FILE).is_file());
        // Once its run was cancelled or its program ended, a call reads what
        // it kept and writes nothing.
        let ended = CancelToken::new();
        ended.cancel();
        let over = (CallCode::Cancelled, "This run has ended.".to_owned());
        assert_eq!(said(run.ask_ended(HostMethod::StateSet, json!({"key": "late", "value": 1}), &ended)), over);
        assert_eq!(said(run.ask_ended(HostMethod::StateDelete, json!({"key": "seen"}), &ended)), over);
        assert_eq!(run.ask_ended(HostMethod::StateKeys, Value::Null, &ended).unwrap(), json!([]));
        // Nor does a call bring back a data folder deleted with the plugin's data.
        std::fs::remove_dir_all(&data).unwrap();
        assert_eq!(said(call(HostMethod::StateSet, json!({"key": "seen", "value": 1}))).0, CallCode::Failed);
        assert!(!data.exists());
        // A plugin that isn't installed has no settings.
        assert_eq!(said(call(HostMethod::SettingsGet, Value::Null)).0, CallCode::NotFound);
        // A plugin without secret settings has none to read.
        assert_eq!(
            said(call(HostMethod::SecretsGet, json!({"key": "token"}))),
            (CallCode::Denied, "secrets.get needs a secret setting, and this plugin has none.".into())
        );
        // What comes later says so; a page's own methods aren't a run's.
        let net = place.run(None, &[Permission::Network]);
        assert_eq!(
            said(net.ask(HostMethod::NetFetch, json!({"url": "https://discord.com"}))),
            (CallCode::Invalid, "net.fetch comes in a later AutoScene.".into())
        );
        assert_eq!(
            said(call(HostMethod::UiToast, json!({"text": "Hi"}))),
            (CallCode::Denied, "A run can't call ui.toast.".into())
        );
        drop((run, net, place));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_run_reads_only_its_own_secrets() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-calls-secrets-{}", std::process::id()));
        let (library, host) = host_in(&tmp);
        let library = Arc::new(library);
        let manifest = json!({
            "manifestVersion": 2,
            "id": "dev.a.secrets",
            "version": "1.0.0",
            "name": "Poster",
            "description": "Posts clips.",
            "author": {"name": "Someone"},
            "engines": {"autoscene": ">=1.0.0", "protocol": 2},
            "runtime": {"type": "process", "command": {"*": ["uv", "run", "--script", "--locked", "main.py"]}},
            "contributes": {
                "commands": [{"id": "post", "title": "Post"}],
                "settings": [
                    {"key": "token", "type": "secret", "title": "Token"},
                    {"key": "webhook", "type": "secret", "title": "Webhook"},
                    {"key": "channel", "type": "string", "title": "Channel", "default": "clips"}
                ]
            }
        });
        let p = host.store.commit(folder(&host, &tmp, &manifest), true, true).unwrap();
        let id = p.record.id.clone();
        host.store.secret_set(&id, "token", "hunter2").unwrap();
        let post = key(&id, ActionKind::Command, "post", None, None);
        let run = calls_for(&library, &host, &p, &post, 2).unwrap().unwrap();
        let get = |key: &str| run.ask(HostMethod::SecretsGet, json!({ "key": key }));

        assert_eq!(get("token").unwrap(), json!({"value": "hunter2"}));
        // One the person hasn't set is none; it is read at each call.
        assert_eq!(get("webhook").unwrap(), json!({"value": null}));
        host.store.secret_set(&id, "webhook", "https://example.org/hook").unwrap();
        assert_eq!(get("webhook").unwrap(), json!({"value": "https://example.org/hook"}));
        // A plain setting is no secret, and settings.get gives none.
        assert_eq!(
            said(get("channel")),
            (CallCode::Denied, "secrets.get reads only the plugin's secret settings, and channel isn't one.".into())
        );
        assert_eq!(run.ask(HostMethod::SettingsGet, Value::Null).unwrap(), json!({"channel": "clips"}));
        // Removed, it is none again.
        host.store.secret_clear(&id, "token").unwrap();
        assert_eq!(get("token").unwrap(), json!({"value": null}));
        drop((run, host, library));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn only_a_run_of_protocol_2_gets_its_calls_answered_where_it_runs() {
        let tmp = std::env::temp_dir().join(format!("autoscene-plugin-calls-for-{}", std::process::id()));
        let (library, host) = host_in(&tmp);
        let library = Arc::new(library);
        // Agreed to both; its plugin.json asks for exports:read only now.
        let mut p = plugin("dev.a.calls", Trust::Reviewed, false);
        p.record.permissions = vec![Permission::ExportsRead, Permission::ProjectRead];
        let m = p.manifest.as_mut().unwrap();
        (m.permissions, m.engines.protocol) = (vec![Permission::ExportsRead], 2);
        let (project, export) = (ProjectId::new(), ExportId::new());
        let on_export: RunKey = key(&p.record.id, ActionKind::ExportAction, "post", Some(project), Some(export));

        let run = calls_for(&library, &host, &p, &on_export, 2).unwrap().unwrap();
        assert_eq!(run.plugin, p.record.id);
        assert_eq!((&run.project, &run.export), (&Some(project.to_string()), &Some(export.to_string())));
        assert_eq!(run.grants.permissions, [Permission::ExportsRead]);
        // Bound there: another project's export is refused before anything is read.
        let other = json!({"exportId": ExportId::new().to_string(), "projectId": ProjectId::new().to_string()});
        assert_eq!(said(run.ask(HostMethod::ExportsGet, other)).0, CallCode::Denied);
        let command = key(&p.record.id, ActionKind::Command, "hello", None, None);
        let command = calls_for(&library, &host, &p, &command, 2).unwrap().unwrap();
        assert_eq!((&command.project, &command.export), (&None, &None));
        std::fs::create_dir_all(host.store.data_dir(&p.record.id)).unwrap();
        assert_eq!(command.ask(HostMethod::StateSet, json!({"key": "k", "value": 1})).unwrap(), Value::Null);

        // A plugin of protocol 1 makes no calls; a broken one doesn't run.
        assert!(calls_for(&library, &host, &p, &on_export, 1).unwrap().is_none());
        p.manifest = None;
        p.broken = Some("Its files are missing. Install it again.".into());
        assert!(calls_for(&library, &host, &p, &on_export, 2).is_err());
        drop((run, command, host, library));
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}

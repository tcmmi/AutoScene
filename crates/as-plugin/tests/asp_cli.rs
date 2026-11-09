//! The asp tool, run as a plugin author runs it: keys, packages, signatures,
//! and the trust AutoScene would give a package; and as the catalog's CI and
//! its owner run it, to fill, number, certify and sign the catalog. Each
//! test works in a folder of its own.
#![cfg(feature = "cli")]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use as_core::CancelToken;
use as_plugin::PluginError;
use as_plugin::catalog::{self, index_comment};
use as_plugin::keys::CATALOG_KEYS;
use as_plugin::manifest;
use as_plugin::package::{self, AUTHOR_KEY, LOCK, SIG};
use as_plugin::trust::{self, Trust};
use as_plugin::wasm::{self, Compiler};
use as_plugin::{Launch, RuntimeKind, current_platform};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use semver::Version;
use serde_json::{Value, json};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive};

/// What a run of asp did.
struct Run {
    code: Option<i32>,
    out: String,
    err: String,
}

impl Run {
    /// Its output, once it succeeded.
    fn ok(self) -> String {
        assert_eq!(self.code, Some(0), "{}", self.err);
        self.out
    }

    /// Its warnings, once it succeeded.
    fn warnings(self) -> String {
        assert_eq!(self.code, Some(0), "{}", self.err);
        self.err
    }

    /// Its errors, once it failed with `code`.
    fn failed(self, code: i32) -> String {
        assert_eq!(self.code, Some(code), "{}{}", self.out, self.err);
        self.err
    }
}

fn asp(args: &[&str]) -> Run {
    asp_env(args, &[])
}

/// asp run with the environment variables `env` set.
fn asp_env(args: &[&str], env: &[(&str, &str)]) -> Run {
    let output = Command::new(env!("CARGO_BIN_EXE_asp")).args(args).envs(env.iter().copied()).output().unwrap();
    Run {
        code: output.status.code(),
        out: String::from_utf8(output.stdout).unwrap(),
        err: String::from_utf8(output.stderr).unwrap(),
    }
}

fn text(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// A plugin folder `dir` whose plugin.json starts `command`, with `files`.
fn folder(dir: &Path, command: Value, files: &[(&str, &[u8])]) -> PathBuf {
    let manifest = json!({
        "manifestVersion": 1,
        "id": "dev.autoscene.hello-export",
        "version": "1.2.0",
        "name": "Hello Export",
        "description": "Says hello after an export.",
        "author": {"name": "AutoScene"},
        "engines": {"autoscene": ">=1.0.0, <2", "protocol": 1},
        "runtime": {"type": "process", "command": command}
    });
    let manifest = serde_json::to_vec_pretty(&manifest).unwrap();
    for (path, bytes) in [("plugin.json", manifest.as_slice())].iter().chain(files) {
        let file = dir.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, bytes).unwrap();
    }
    dir.to_path_buf()
}

/// A plugin that runs a Python script through uv.
fn uv_plugin(dir: &Path) -> PathBuf {
    let command = json!({"*": ["uv", "run", "--script", "--locked", "main.py"]});
    let files: [(&str, &[u8]); 3] =
        [("main.py", b"print('hello')\n"), ("main.py.lock", b"version = 1\n"), ("lib/data.txt", b"hi")];
    folder(dir, command, &files)
}

/// Makes an unencrypted key pair in `dir`; gives its author.pub line.
fn keygen(dir: &Path) -> String {
    asp(&["keygen", text(dir), "--unencrypted"]).ok();
    fs::read_to_string(dir.join(AUTHOR_KEY)).unwrap().trim().to_owned()
}

/// A package's entries, by name.
fn entries(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut zip = ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut entries = BTreeMap::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        entries.insert(entry.name().unwrap().into_owned(), bytes);
    }
    entries
}

/// How a package's entries are packed, by name.
fn methods(path: &Path) -> BTreeMap<String, CompressionMethod> {
    let mut zip = ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut methods = BTreeMap::new();
    for i in 0..zip.len() {
        let entry = zip.by_index_raw(i).unwrap();
        methods.insert(entry.name().unwrap().into_owned(), entry.compression());
    }
    methods
}

/// What lies in `dir`: names, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> =
        fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

/// `platforms` as asp names them, where this computer's says so.
fn on(platforms: &[&str]) -> String {
    let named = |&p: &&str| if p == current_platform() { format!("{p} (this computer)") } else { p.to_owned() };
    platforms.iter().map(named).collect::<Vec<_>>().join(", ")
}

/// Changes the plugin.json in `dir`.
fn edit_manifest(dir: &Path, edit: impl FnOnce(&mut Value)) {
    let mut manifest: Value = serde_json::from_slice(&fs::read(dir.join("plugin.json")).unwrap()).unwrap();
    edit(&mut manifest);
    fs::write(dir.join("plugin.json"), serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
}

/// Every file below `dir`, by its path there with `/`, sorted.
fn files_in(dir: &Path) -> Vec<String> {
    let mut files = Vec::new();
    let mut folders = vec![dir.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path);
            } else {
                let parts: Vec<&str> =
                    path.strip_prefix(dir).unwrap().components().map(|c| c.as_os_str().to_str().unwrap()).collect();
                files.push(parts.join("/"));
            }
        }
    }
    files.sort();
    files
}

/// A package's files, by name, without its lock.
fn packed(path: &Path) -> Vec<String> {
    entries(path).into_keys().filter(|name| name != LOCK).collect()
}

#[test]
fn keygen_pack_sign_verify_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let place = |name: &str| tmp.path().join(name);
    let keys = place("keys");

    // A key pair; another one isn't made over it unless asked.
    let made = asp(&["keygen", text(&keys), "--unencrypted"]).ok();
    let public = fs::read_to_string(keys.join(AUTHOR_KEY)).unwrap();
    let secret = fs::read_to_string(keys.join("asp.key")).unwrap();
    let id = trust::key_id(public.trim()).unwrap();
    assert!(made.contains(&id), "{made}");
    let err = asp(&["keygen", text(&keys), "--unencrypted"]).failed(1);
    assert!(err.contains("is there already; --force replaces it."), "{err}");
    assert_eq!(fs::read_to_string(keys.join("asp.key")).unwrap(), secret);
    let err = asp(&["keygen", text(&keys), "--unencrypted", "--force"]).warnings();
    assert!(err.contains("plugins pinned to the old key"), "{err}");
    let public = fs::read_to_string(keys.join(AUTHOR_KEY)).unwrap();
    assert!(public.ends_with('\n') && !public.trim().contains('\n'), "the bare key line: {public}");
    let id = trust::key_id(public.trim()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(keys.join("asp.key")).unwrap().permissions().mode() & 0o777, 0o600);
    }

    // The same folder gives the same package, byte for byte.
    let src = uv_plugin(&place("hello"));
    let (unsigned, again) = (place("hello.asp"), place("again.asp"));
    let packed = asp(&["pack", text(&src), text(&unsigned)]).ok();
    assert_eq!(packed, format!("Packed dev.autoscene.hello-export 1.2.0 into {}, unsigned.\n", unsigned.display()));
    asp(&["pack", text(&src), text(&again)]).ok();
    assert_eq!(fs::read(&unsigned).unwrap(), fs::read(&again).unwrap());
    let names_in = |path: &Path| entries(path).into_keys().collect::<Vec<_>>();
    assert_eq!(names_in(&unsigned), ["files.lock", "lib/data.txt", "main.py", "main.py.lock", "plugin.json"]);
    assert_eq!(asp(&["verify", text(&unsigned)]).ok(), "dev.autoscene.hello-export 1.2.0: Unreviewed\n");

    // Signed into another file, which leaves the package as it was.
    let signed = place("signed.asp");
    let key = keys.join("asp.key");
    let done = asp(&["sign", text(&unsigned), "--key", text(&key), "--out", text(&signed)]).ok();
    assert!(done.ends_with(&format!(": Pinned (key {id}).\n")), "{done}");
    assert_eq!(fs::read(&unsigned).unwrap(), fs::read(&again).unwrap());
    assert_eq!(asp(&["verify", text(&signed)]).ok(), format!("dev.autoscene.hello-export 1.2.0: Pinned (key {id})\n"));

    // What AutoScene unpacks: the same files and lock, the signature and
    // the key, which judge pins.
    let into = place("unpacked");
    let unpacked = package::unpack(&signed, &into).unwrap();
    let (before, after) = (entries(&unsigned), entries(&signed));
    assert_eq!(unpacked.lock_bytes, before[LOCK]);
    assert!(before.iter().all(|(name, bytes)| after[name] == *bytes));
    assert_eq!(after.len(), before.len() + 2);
    assert_eq!(unpacked.author_key.as_deref(), Some(public.as_str()));
    let app = Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
    let m = manifest::parse(&fs::read(into.join("plugin.json")).unwrap(), &app).unwrap().manifest;
    let (sig, author) = (unpacked.signature.as_deref(), unpacked.author_key.as_deref());
    let verdict = trust::judge(&unpacked.lock_bytes, sig, author, &m, None).unwrap();
    assert_eq!((verdict.trust, verdict.author_key.unwrap().key_id), (Trust::Pinned, id.clone()));

    // Signed in place, also over a signature.
    for _ in 0..2 {
        asp(&["sign", text(&again), "--key", text(&key)]).ok();
        assert_eq!(names_in(&again), names_in(&signed));
        assert_eq!(
            asp(&["verify", text(&again)]).ok(),
            format!("dev.autoscene.hello-export 1.2.0: Pinned (key {id})\n")
        );
    }

    // asp works beside its output and leaves nothing there.
    assert_eq!(names(tmp.path()), ["again.asp", "hello", "hello.asp", "keys", "signed.asp", "unpacked"]);

    // Wrong arguments, and a package in its own folder.
    assert!(asp(&[]).failed(2).contains("usage: asp keygen"));
    assert!(asp(&["pack", text(&src)]).failed(2).contains("expected 2 path(s), got 1"));
    assert!(asp(&["sign", text(&signed)]).failed(2).contains("sign needs --key"));
    assert!(asp(&["verify", text(&signed), "--keys"]).failed(2).contains("unknown option --keys"));
    let err = asp(&["pack", text(&src), text(&src.join("hello.asp"))]).failed(1);
    assert_eq!(err, "asp: Write the package outside the plugin's folder.\n");
    assert_eq!(names(&src), ["lib", "main.py", "main.py.lock", "plugin.json"]);
    // A folder that isn't there is named.
    let missing = place("missing");
    let err = asp(&["pack", text(&missing), text(&place("missing.asp"))]).failed(1);
    assert!(err.starts_with(&format!("asp: {}: ", missing.display())), "{err}");
}

/// Links `link` to the folder `target`: a symlink on Linux and macOS, a
/// junction on Windows, which needs no administrator (its symlinks do).
fn link_folder(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    {
        // mklink takes a "/" in a path for an option.
        let (link, target): (PathBuf, PathBuf) = (link.components().collect(), target.components().collect());
        let made = Command::new("cmd").arg("/C").arg("mklink").arg("/J").arg(link).arg(target).output().unwrap();
        assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stderr));
    }
}

#[test]
fn pack_refuses_a_folder_with_a_symlink() {
    let tmp = tempfile::tempdir().unwrap();
    let src = uv_plugin(&tmp.path().join("hello"));
    let elsewhere = tmp.path().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    fs::write(elsewhere.join("secret.txt"), b"not the plugin's").unwrap();
    let out = tmp.path().join("hello.asp");

    link_folder(&elsewhere, &src.join("lib/link"));
    let err = asp(&["pack", text(&src), text(&out)]).failed(1);
    assert_eq!(err, "asp: The plugin's lib/link is a link or another special file, which a package can't hold.\n");
    assert_eq!(names(tmp.path()), ["elsewhere", "hello"]);

    // A linked file too, where links to files can be made.
    #[cfg(unix)]
    {
        fs::remove_file(src.join("lib/link")).unwrap();
        std::os::unix::fs::symlink(elsewhere.join("secret.txt"), src.join("lib/link.txt")).unwrap();
        let err = asp(&["pack", text(&src), text(&out)]).failed(1);
        assert!(err.contains("The plugin's lib/link.txt is a link"), "{err}");
        assert_eq!(names(tmp.path()), ["elsewhere", "hello"]);
    }
}

#[test]
fn pack_writes_the_exec_flag_for_listed_programs() {
    let tmp = tempfile::tempdir().unwrap();
    let command = json!({
        "windows-x86_64": ["bin/tool.exe", "--hello"],
        "linux-x86_64": ["bin/tool"],
        "macos-aarch64": ["bin/mac-tool"],
        "*": ["uv", "run", "--script", "--locked", "main.py"]
    });
    let files: [(&str, &[u8]); 5] = [
        ("bin/tool.exe", b"MZ"),
        ("bin/tool", b"\x7fELF"),
        ("bin/helper", b"#!/bin/sh\n"),
        ("main.py", b"print('hello')\n"),
        ("lib/data.txt", b"hi"),
    ];
    let src = folder(&tmp.path().join("hello"), command, &files);
    // Whether a file may run here doesn't make a program, nor keep one from
    // being one.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(src.join("bin/tool"), fs::Permissions::from_mode(0o644)).unwrap();
        fs::set_permissions(src.join("bin/helper"), fs::Permissions::from_mode(0o755)).unwrap();
    }
    let out = tmp.path().join("hello.asp");
    let err = asp(&["pack", text(&src), text(&out)]).warnings();

    let lock = package::unpack(&out, &tmp.path().join("unpacked")).unwrap().lock;
    let exec: BTreeMap<&str, bool> = lock.files.iter().map(|f| (f.path.as_str(), f.exec)).collect();
    let helper_runs = cfg!(unix);
    assert_eq!(
        exec,
        BTreeMap::from([
            ("bin/helper", helper_runs),
            ("bin/tool", true),
            ("bin/tool.exe", true),
            ("lib/data.txt", false),
            ("main.py", false),
            ("plugin.json", false),
        ])
    );
    // Each platform falls back to "*" where it has no command of its own.
    let mut warnings = Vec::new();
    if helper_runs {
        warnings.push("warning: bin/helper may run on this computer, so the package marks it to run too.".to_owned());
    }
    let missing = |file: &str| format!("The plugin package is missing {file}, which its command needs.");
    let fallback = on(&["windows-aarch64", "macos-x86_64", "linux-aarch64"]);
    warnings.push(format!("warning: It won't run on {fallback}: {}", missing("main.py.lock")));
    warnings.push(format!("warning: It won't run on {}: {}", on(&["macos-aarch64"]), missing("bin/mac-tool")));
    assert_eq!(err.lines().collect::<Vec<_>>(), warnings);

    // The zip says so too, for tools that unpack it on Linux and macOS.
    let mut zip = ZipArchive::new(fs::File::open(&out).unwrap()).unwrap();
    for i in 0..zip.len() {
        let entry = zip.by_index_raw(i).unwrap();
        let name = entry.name().unwrap().into_owned();
        let mode = if exec.get(name.as_str()) == Some(&true) { 0o755 } else { 0o644 };
        assert_eq!(entry.unix_mode().map(|m| m & 0o777), Some(mode), "{name}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &str| fs::metadata(tmp.path().join("unpacked").join(path)).unwrap().permissions().mode();
        assert_eq!((mode("bin/tool") & 0o777, mode("lib/data.txt") & 0o777), (0o755, 0o644));
    }
}

#[test]
fn pack_refuses_a_folder_with_a_secret_key() {
    let tmp = tempfile::tempdir().unwrap();
    let place = |name: &str| tmp.path().join(name);
    let src = uv_plugin(&place("hello"));
    let out = place("hello.asp");

    // keygen makes none there.
    let err = asp(&["keygen", text(&src), "--unencrypted"]).failed(1);
    assert_eq!(err, "asp: Make the key outside the plugin's folder, so it never goes into a package.\n");
    assert_eq!(names(&src), ["lib", "main.py", "main.py.lock", "plugin.json"]);

    // A key moved in, by what it holds, and anything named asp.key.
    keygen(&place("keys"));
    fs::copy(place("keys").join("asp.key"), src.join("lib/signing.txt")).unwrap();
    let err = asp(&["pack", text(&src), text(&out)]).failed(1);
    assert_eq!(
        err,
        "asp: The plugin's lib/signing.txt is a secret key, which would let anyone sign as you. \
         Move it out of the plugin's folder.\n"
    );
    fs::remove_file(src.join("lib/signing.txt")).unwrap();
    fs::write(src.join("lib/ASP.KEY"), b"not yet a key").unwrap();
    let err = asp(&["pack", text(&src), text(&out)]).failed(1);
    assert!(err.starts_with("asp: The plugin's lib/ASP.KEY is a secret key"), "{err}");
    assert_eq!(names(tmp.path()), ["hello", "keys"]);

    // A public key is no secret.
    fs::remove_file(src.join("lib/ASP.KEY")).unwrap();
    fs::copy(place("keys").join(AUTHOR_KEY), src.join("lib/author.pub")).unwrap();
    asp(&["pack", text(&src), text(&out)]).ok();
}

#[test]
fn pack_stores_a_file_deflate_would_pack_too_tightly() {
    let tmp = tempfile::tempdir().unwrap();
    let place = |name: &str| tmp.path().join(name);
    let id = trust::key_id(&keygen(&place("keys"))).unwrap();
    let key = place("keys").join("asp.key");

    // Zero-padded weights deflate over a thousand times smaller, which
    // AutoScene takes from a file of up to 1 MB only.
    let src = uv_plugin(&place("model"));
    fs::write(src.join("model.bin"), vec![0; 2 << 20]).unwrap();
    let (unsigned, again, signed) = (place("model.asp"), place("again.asp"), place("signed.asp"));
    asp(&["pack", text(&src), text(&unsigned)]).ok();
    asp(&["pack", text(&src), text(&again)]).ok();
    assert_eq!(fs::read(&unsigned).unwrap(), fs::read(&again).unwrap());
    let stored = |path: &Path| {
        let methods = methods(path).into_iter().filter(|(_, m)| *m != CompressionMethod::Deflated);
        methods.map(|(name, m)| (m == CompressionMethod::Stored).then_some(name)).collect::<Option<Vec<_>>>()
    };
    assert_eq!(stored(&unsigned), Some(vec!["model.bin".to_owned()]));
    asp(&["sign", text(&unsigned), "--key", text(&key), "--out", text(&signed)]).ok();
    assert_eq!(stored(&signed), Some(vec!["model.bin".to_owned()]));
    assert_eq!(asp(&["verify", text(&signed)]).ok(), format!("dev.autoscene.hello-export 1.2.0: Pinned (key {id})\n"));

    // Files of 1 MB may be packed as tightly as deflate likes, but not all
    // of them together: the first of two equal ones is stored.
    let src = uv_plugin(&place("halves"));
    for name in ["a.bin", "b.bin"] {
        fs::write(src.join(name), vec![0; 1 << 20]).unwrap();
    }
    let halves = place("halves.asp");
    asp(&["pack", text(&src), text(&halves)]).ok();
    assert_eq!(stored(&halves), Some(vec!["a.bin".to_owned()]));
    package::unpack(&halves, &place("unpacked")).unwrap();
}

#[test]
fn pack_and_verify_warn_as_autoscene_would() {
    let tmp = tempfile::tempdir().unwrap();
    let place = |name: &str| tmp.path().join(name);
    let files: [(&str, &[u8]); 2] = [("bin/tool", b"#!/bin/sh\n"), ("icon.svg", b"<svg/>")];
    let src = folder(&place("hello"), json!({"*": ["bin/tool"]}), &files);
    // Made for a later AutoScene, with a key this one doesn't know, and a
    // command that runs everywhere but on Windows.
    edit_manifest(&src, |v| {
        v["engines"]["autoscene"] = json!(">=99.0.0");
        v["homepage"] = json!("https://example.com");
        v["icon"] = json!("icon.svg");
    });
    let warnings = [
        "warning: Ignored homepage, which this AutoScene doesn't know.".to_owned(),
        format!(
            r#"warning: AutoScene {} won't install it: its engines.autoscene is ">=99.0.0"."#,
            env!("CARGO_PKG_VERSION")
        ),
        format!(
            r#"warning: It won't run on {}: The plugin's runtime.command["*"] runs on Windows, so its program must be an .exe file."#,
            on(&["windows-x86_64", "windows-aarch64"])
        ),
    ];
    let out = place("hello.asp");
    assert_eq!(asp(&["pack", text(&src), text(&out)]).warnings().lines().collect::<Vec<_>>(), warnings);
    let verified = asp(&["verify", text(&out)]);
    assert_eq!(verified.err.lines().collect::<Vec<_>>(), warnings);
    assert_eq!(verified.ok(), "dev.autoscene.hello-export 1.2.0: Unreviewed\n");

    // A package that runs nowhere, or lacks its icon, isn't written.
    let refused = |edit: fn(&mut Value)| {
        edit_manifest(&src, |v| {
            v["engines"]["autoscene"] = json!(">=1.0.0, <2");
            v.as_object_mut().unwrap().remove("homepage");
            edit(v);
        });
        let err = asp(&["pack", text(&src), text(&place("refused.asp"))]).failed(1);
        assert_eq!(names(tmp.path()), ["hello", "hello.asp"]);
        err
    };
    let windows = r#"The plugin's runtime.command["*"] runs on Windows, so its program must be an .exe file."#;
    assert_eq!(
        refused(|v| v["platforms"] = json!(["windows-x86_64"])),
        format!(
            "warning: It won't run on {}: {windows}\nasp: The plugin runs on none of its platforms.\n",
            on(&["windows-x86_64"])
        )
    );
    let no_command = "The plugin's platforms names it, but runtime.command has no command for it.";
    assert_eq!(
        refused(|v| {
            v["runtime"]["command"] = json!({"windows-x86_64": ["bin/tool.exe"]});
            v["platforms"] = json!(["linux-x86_64"]);
        }),
        format!(
            "warning: It won't run on {}: {no_command}\nasp: The plugin runs on none of its platforms.\n",
            on(&["linux-x86_64"])
        )
    );
    assert_eq!(
        refused(|v| v["platforms"] = json!(["plan9-mips"])),
        "warning: platforms names \"plan9-mips\", which this AutoScene doesn't know.\n\
         asp: The plugin runs on no platform this AutoScene knows.\n"
    );
    assert_eq!(
        refused(|v| {
            v["runtime"]["command"] = json!({"*": ["bin/tool"]});
            v["platforms"] = json!(["linux-x86_64"]);
            v["icon"] = json!("gone.png");
        }),
        "asp: The plugin package is missing its icon, gone.png.\n"
    );
}

/// The start of a PNG of `w`×`h`: its signature and IHDR.
fn png(w: u32, h: u32) -> Vec<u8> {
    let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    b.extend(w.to_be_bytes().into_iter().chain(h.to_be_bytes()));
    b.extend([8, 2, 0, 0, 0]);
    b
}

/// The start of a baseline JPEG of `w`×`h`.
fn jpeg(w: u16, h: u16) -> Vec<u8> {
    let mut b = b"\xFF\xD8\xFF\xC0\0\x11\x08".to_vec();
    b.extend(h.to_be_bytes().into_iter().chain(w.to_be_bytes()));
    b.extend([3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
    b
}

/// A uv plugin of manifest version 2 in `dir`, with its banner `path` of `bytes`.
fn with_banner(dir: &Path, path: &str, bytes: &[u8]) -> PathBuf {
    let src = uv_plugin(dir);
    fs::create_dir_all(src.join("art")).unwrap();
    fs::write(src.join(path), bytes).unwrap();
    edit_manifest(&src, |v| {
        v["manifestVersion"] = json!(2);
        v["banner"] = json!(path);
    });
    src
}

#[test]
fn pack_refuses_a_banner_it_doesnt_list() {
    let tmp = tempfile::tempdir().unwrap();
    let place = |name: &str| tmp.path().join(name);
    let src = with_banner(&place("hello"), "art/banner.jpg", &jpeg(1200, 600));
    let out = place("hello.asp");
    assert_eq!(asp(&["pack", text(&src), text(&out)]).warnings(), "");
    assert_eq!(asp(&["verify", text(&out)]).ok(), "dev.autoscene.hello-export 1.2.0: Unreviewed\n");
    assert!(entries(&out).contains_key("art/banner.jpg"));

    // Not in the folder (nor in its package), too large, or not what it says.
    let refused = |bytes: Option<Vec<u8>>| {
        match bytes {
            Some(bytes) => fs::write(src.join("art/banner.jpg"), bytes).unwrap(),
            None => fs::remove_file(src.join("art/banner.jpg")).unwrap(),
        }
        let err = asp(&["pack", text(&src), text(&place("refused.asp"))]).failed(1);
        assert_eq!(names(tmp.path()), ["hello", "hello.asp"]);
        err
    };
    let mut big = jpeg(1200, 600);
    big.resize((512 << 10) + 1, 0);
    assert_eq!(refused(Some(big)), "asp: The plugin's banner is larger than 512 KB.\n");
    assert_eq!(refused(Some(png(1200, 600))), "asp: The plugin's banner art/banner.jpg isn't a JPEG image.\n");
    assert_eq!(refused(None), "asp: The plugin package is missing its banner, art/banner.jpg.\n");
}

#[test]
fn pack_warns_of_a_small_or_misshapen_banner() {
    let tmp = tempfile::tempdir().unwrap();
    let place = |name: &str| tmp.path().join(name);
    let cases: [(&str, Vec<u8>, &[&str]); 4] = [
        ("art/banner.jpg", jpeg(1200, 600), &[]),
        ("art/banner.jpg", jpeg(800, 400), &["warning: The banner is 800×400. Cards show it soft below 1200×600."]),
        ("art/banner.png", png(1200, 900), &["warning: The banner is 1200×900. Cards crop it to 2:1."]),
        (
            "art/banner.png",
            png(600, 600),
            &[
                "warning: The banner is 600×600. Cards show it soft below 1200×600.",
                "warning: The banner is 600×600. Cards crop it to 2:1.",
            ],
        ),
    ];
    for (i, (path, bytes, warnings)) in cases.into_iter().enumerate() {
        let src = with_banner(&place(&format!("hello{i}")), path, &bytes);
        let out = place(&format!("hello{i}.asp"));
        assert_eq!(asp(&["pack", text(&src), text(&out)]).warnings().lines().collect::<Vec<_>>(), warnings);
        // verify says the same of the package.
        let verified = asp(&["verify", text(&out)]);
        assert_eq!(verified.err.lines().collect::<Vec<_>>(), warnings);
        verified.ok();
    }
}

#[test]
fn a_plugin_that_runs_no_program_packs_with_its_pages() {
    let tmp = tempfile::tempdir().unwrap();
    let place = |name: &str| tmp.path().join(name);
    let files: [(&str, &[u8]); 2] = [("ui/index.html", b"<!doctype html>"), ("ui/app.js", b"go()")];
    let src = folder(&place("page"), json!({"*": ["bin/tool"]}), &files);
    edit_manifest(&src, |v| {
        v["manifestVersion"] = json!(2);
        v["runtime"] = json!({"type": "none"});
        v["contributes"] = json!({"pages": [{"id": "main", "title": "Main", "entry": "ui/index.html"}]});
    });
    // It runs on every platform: it has no program to mark or to check.
    let out = place("page.asp");
    assert_eq!(asp(&["pack", text(&src), text(&out)]).warnings(), "");
    assert_eq!(asp(&["verify", text(&out)]).ok(), "dev.autoscene.hello-export 1.2.0: Unreviewed\n");
    let lock: Value = serde_json::from_slice(&entries(&out)[LOCK]).unwrap();
    let listed: Vec<(&str, &Value)> =
        lock["files"].as_array().unwrap().iter().map(|f| (f["path"].as_str().unwrap(), &f["exec"])).collect();
    assert_eq!(
        listed,
        [("plugin.json", &json!(false)), ("ui/app.js", &json!(false)), ("ui/index.html", &json!(false))]
    );

    // Its page must be in it.
    edit_manifest(&src, |v| v["contributes"]["pages"][0]["entry"] = json!("ui/gone.html"));
    let err = asp(&["pack", text(&src), text(&place("gone.asp"))]).failed(1);
    assert_eq!(err, "asp: The plugin package is missing its page ui/gone.html.\n");
    assert_eq!(names(tmp.path()), ["page", "page.asp"]);
}

#[test]
fn pack_leaves_out_the_editor_files() {
    let tmp = tempfile::tempdir().unwrap();
    let place = |name: &str| tmp.path().join(name);
    let src = uv_plugin(&place("hello"));
    // What asp init writes for an editor, and what an editor adds, at any
    // depth and in any case. A file only named like them stays.
    for path in [
        ".autoscene/plugin.schema.json",
        ".autoscene/types/sdk.d.ts",
        ".vscode/settings.json",
        "lib/.VSCode/launch.json",
        "lib/.autoscene",
        ".autoscene.json",
        // What asp init --wasm writes in a plugin's folder, for git.
        ".gitignore",
        "lib/.GITIGNORE",
    ] {
        let file = src.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, b"{}").unwrap();
    }
    let out = place("hello.asp");
    assert_eq!(asp(&["pack", text(&src), text(&out)]).warnings(), "");
    assert_eq!(packed(&out), [".autoscene.json", "lib/data.txt", "main.py", "main.py.lock", "plugin.json"]);
    asp(&["verify", text(&out)]).ok();
}

#[test]
fn pack_warns_of_pages_in_a_version_1_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    let place = |name: &str| tmp.path().join(name);
    let src = uv_plugin(&place("hello"));
    fs::create_dir(src.join("ui")).unwrap();
    fs::write(src.join("ui/index.html"), b"<!doctype html>").unwrap();
    edit_manifest(&src, |v| {
        v["contributes"] = json!({
            "pages": [{"id": "main", "title": "Main", "entry": "ui/index.html"}],
            "settings": [{"key": "token", "type": "secret", "title": "Token"}]
        });
    });
    // A version 1 manifest can't have them: AutoScene leaves them out.
    let warnings = "warning: Skipped contributes.pages: pages need \"manifestVersion\": 2.\n\
         warning: Skipped the setting \"token\": secret settings need \"manifestVersion\": 2.\n";
    let out = place("hello.asp");
    assert_eq!(asp(&["pack", text(&src), text(&out)]).warnings(), warnings);
    let verified = asp(&["verify", text(&out)]);
    assert_eq!(verified.err, warnings);
    assert_eq!(verified.ok(), "dev.autoscene.hello-export 1.2.0: Unreviewed\n");

    // Version 2 has them: its page asks for the secret.
    edit_manifest(&src, |v| v["manifestVersion"] = json!(2));
    assert_eq!(asp(&["pack", text(&src), text(&place("v2.asp"))]).warnings(), "");
}

/// `package` with `entries` added from `from` (by name), as a zip of its own.
fn grafted(package: &Path, from: &Path, names: &[&str], out: &Path) {
    let (mut entries, from) = (entries(package), entries(from));
    for name in names {
        entries.insert((*name).to_owned(), from[*name].clone());
    }
    let mut zip = zip::ZipWriter::new(fs::File::create(out).unwrap());
    for (name, bytes) in entries {
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(&bytes).unwrap();
    }
    zip.finish().unwrap();
}

/// A catalog key pair, as the catalog's CI or its owner holds one.
struct CatalogKey(minisign::KeyPair);

impl CatalogKey {
    fn new() -> Self {
        Self(minisign::KeyPair::generate_unencrypted_keypair().unwrap())
    }

    fn public(&self) -> String {
        self.0.pk.to_base64()
    }

    /// Writes `index` to `path` and its signature beside it, as the catalog's
    /// CI publishes them.
    fn publish(&self, index: &Value, path: &Path) {
        let bytes = serde_json::to_vec_pretty(index).unwrap();
        let n = |key: &str| index[key].as_u64().unwrap();
        let comment = index_comment(n("epoch"), n("seq"), n("expires"));
        let sig = minisign::sign(None, &self.0.sk, bytes.as_slice(), Some(&comment), None).unwrap();
        fs::write(path, &bytes).unwrap();
        fs::write(format!("{}.minisig", path.display()), sig.into_string()).unwrap();
    }
}

/// An index that lists the package `listed` as version 1.2.0 of Hello
/// Export, withdraws `withdrawn`, and runs out `days` from now.
fn index_of(listed: &Path, withdrawn: Value, days: i64) -> Value {
    let lock = package::sha256_hex(&entries(listed)[LOCK]);
    let now = catalog::now();
    let expires = now.checked_add_signed(days * 86_400).unwrap();
    json!({
        "catalogVersion": 1,
        "epoch": 1,
        "seq": 1,
        "published": now.min(expires) - 60,
        "expires": expires,
        "plugins": [{
            "id": "dev.autoscene.hello-export",
            "version": "1.2.0",
            "lock": lock,
            "package": {
                "urls": ["https://github.com/tcmmi/autoscene-plugins/releases/download/x/hello-export.asp"],
                "size": fs::metadata(listed).unwrap().len(),
                "sha256": package::sha256_hex(&fs::read(listed).unwrap()),
            },
            "authorKey": null,
            "name": "Hello Export",
            "author": "AutoScene",
            "engines": ">=1.0.0, <2",
            "source": {"repo": "https://github.com/tcmmi/AutoScene", "commit": "c".repeat(40), "path": "hello"},
            "approved": now.min(expires) - 60,
        }],
        "withdrawn": withdrawn,
    })
}

#[test]
fn verify_names_the_trust_a_package_would_get() {
    let tmp = tempfile::tempdir().unwrap();
    let place = |name: &str| tmp.path().join(name);
    let (author, stranger) = (keygen(&place("author")), keygen(&place("stranger")));
    let (author_id, stranger_id) = (trust::key_id(&author).unwrap(), trust::key_id(&stranger).unwrap());
    let src = uv_plugin(&place("hello"));
    let unsigned = place("unsigned.asp");
    asp(&["pack", text(&src), text(&unsigned)]).ok();
    let sign = |key: &str, out: &str| {
        let key = place(key).join("asp.key");
        asp(&["sign", text(&unsigned), "--key", text(&key), "--out", text(&place(out))]).ok();
    };
    sign("author", "by-author.asp");
    sign("stranger", "by-stranger.asp");
    let trust = |package: &str, more: &[&str]| {
        let package = place(package);
        let mut args = vec!["verify", text(&package)];
        args.extend(more);
        asp(&args).ok().trim().strip_prefix("dev.autoscene.hello-export 1.2.0: ").unwrap().to_owned()
    };
    // The catalog's two keys, and an index of each that lists the package's lock.
    let (online, offline) = (CatalogKey::new(), CatalogKey::new());
    let (by_online, by_offline) = (place("online.json"), place("offline.json"));
    online.publish(&index_of(&unsigned, json!([]), 29), &by_online);
    offline.publish(&index_of(&unsigned, json!([]), 29), &by_offline);
    let (online_key, offline_key) = (online.public(), offline.public());

    assert_eq!(trust("unsigned.asp", &[]), "Unreviewed");
    assert_eq!(trust("by-author.asp", &[]), format!("Pinned (key {author_id})"));
    assert_eq!(trust("by-stranger.asp", &[]), format!("Pinned (key {stranger_id})"));
    // Each is the lock the catalog lists, signed or not: no key pins it.
    let reviewed = ["--catalog", text(&by_online), "--online-key", &online_key];
    for package in ["unsigned.asp", "by-author.asp", "by-stranger.asp"] {
        assert_eq!(trust(package, &reviewed), "Reviewed", "{package}");
    }
    assert_eq!(trust("unsigned.asp", &["--catalog", text(&by_offline), "--offline-key", &offline_key]), "Reviewed");
    // As pasted, or read from a .pub file with Windows line ends.
    let pasted = format!(" {online_key}\r");
    assert_eq!(trust("unsigned.asp", &["--catalog", text(&by_online), "--online-key", &pasted]), "Reviewed");
    // AutoScene's own keys are always asked; until the catalog opens there
    // are none, and an index signed by another key is refused.
    let err = asp(&["verify", text(&unsigned), "--catalog", text(&by_online)]).failed(1);
    if CATALOG_KEYS.is_empty() {
        assert_eq!(err, "asp: The catalog is signed with a key AutoScene doesn't know.\n");
    }
    // A catalog that lists another lock reviews none of them.
    let other = place("other.asp");
    let other_src = uv_plugin(&place("other"));
    fs::write(other_src.join("lib/data.txt"), b"other").unwrap();
    asp(&["pack", text(&other_src), text(&other)]).ok();
    let elsewhere = place("elsewhere.json");
    online.publish(&index_of(&other, json!([]), 29), &elsewhere);
    let listed_elsewhere = ["--catalog", text(&elsewhere), "--online-key", &online_key];
    assert_eq!(trust("unsigned.asp", &listed_elsewhere), "Unreviewed");
    assert_eq!(trust("by-author.asp", &listed_elsewhere), format!("Pinned (key {author_id})"));
    // A version it withdrew fails, signed or not, though it lists the lock.
    let reason = "It sent export paths to a server.";
    let withdrawn = json!([{"id": "dev.autoscene.hello-export", "versions": "<=1.2.0", "reason": reason, "since": 1}]);
    let pulled = place("withdrawn.json");
    online.publish(&index_of(&unsigned, withdrawn, 29), &pulled);
    for package in ["unsigned.asp", "by-author.asp"] {
        let run = asp(&["verify", text(&place(package)), "--catalog", text(&pulled), "--online-key", &online_key]);
        let why = format!("AutoScene withdrew Hello Export. {reason}");
        assert_eq!(run.failed(1), format!("asp: {why} (plugin withdrawn: Hello Export: {reason})\n"));
    }
    // A catalog that ran out, or isn't there, is no catalog to go by.
    let ran_out = place("ran-out.json");
    online.publish(&index_of(&unsigned, json!([]), -1), &ran_out);
    let err = asp(&["verify", text(&unsigned), "--catalog", text(&ran_out), "--online-key", &online_key]).failed(1);
    assert!(err.starts_with("asp: The catalog ran out on "), "{err}");
    fs::remove_file(format!("{}.minisig", by_online.display())).unwrap();
    let err = asp(&["verify", text(&unsigned), "--catalog", text(&by_online), "--online-key", &online_key]).failed(1);
    assert!(err.starts_with(&format!("asp: {}.minisig: ", by_online.display())), "{err}");
    // A catalog larger than AutoScene takes is read no further.
    let large = place("large.json");
    fs::write(&large, vec![b' '; usize::try_from(catalog::MAX_INDEX_BYTES).unwrap() + 1]).unwrap();
    fs::write(
        format!("{}.minisig", large.display()),
        b"untrusted comment: x
",
    )
    .unwrap();
    let err = asp(&["verify", text(&unsigned), "--catalog", text(&large), "--online-key", &online_key]).failed(1);
    assert!(err.starts_with("asp: The catalog is too large."), "{err}");
    // A catalog key goes with a catalog.
    let err = asp(&["verify", text(&unsigned), "--online-key", &online_key]).failed(2);
    assert!(err.starts_with("asp: --online-key and --offline-key check the index --catalog names\n"), "{err}");

    // Signed by the author's key, but not for this lock: AutoScene refuses it.
    fs::write(src.join("lib/data.txt"), b"changed").unwrap();
    let changed = place("changed.asp");
    asp(&["pack", text(&src), text(&changed)]).ok();
    let forged = place("forged.asp");
    grafted(&changed, &place("by-author.asp"), &[SIG, AUTHOR_KEY], &forged);
    let err = asp(&["verify", text(&forged)]).failed(1);
    assert_eq!(
        err,
        "asp: The package was changed after it was signed. (plugin signature: signed for another plugin or version)\n"
    );

    // Neither a catalog key nor a package that isn't one.
    let not_a_key = ["verify", text(&unsigned), "--catalog", text(&by_offline), "--offline-key", "RWQ not a key"];
    let err = asp(&not_a_key).failed(1);
    assert_eq!(err, "asp: That isn't a minisign public key.\n");
    let err = asp(&["verify", text(&place("author").join("asp.key"))]).failed(1);
    assert_eq!(err, "asp: The plugin package is damaged.\n");
    let err = asp(&["verify", text(&place("missing.asp"))]).failed(1);
    assert!(err.starts_with(&format!("asp: {}: ", place("missing.asp").display())), "{err}");
}

const DAY: u64 = 86_400;
const HELLO: &str = "dev.autoscene.hello-export";
const RELEASE: &str = "https://github.com/tcmmi/autoscene-plugins/releases/download/hello-1.2.0/hello-export.asp";
const COMMIT: &str = "cccccccccccccccccccccccccccccccccccccccc";

/// The JSON file `path`.
fn json_of(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

/// Writes `v` to `path` as the catalog keeps an index: pretty, with a
/// trailing newline.
fn write_json(path: &Path, v: &Value) {
    let mut bytes = serde_json::to_vec_pretty(v).unwrap();
    bytes.push(b'\n');
    fs::write(path, bytes).unwrap();
}

/// Changes the index `path` as asp would write it (asp has no command for
/// what is changed here).
fn edit_index(path: &Path, edit: impl FnOnce(&mut catalog::Index)) {
    let mut index: catalog::Index = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    edit(&mut index);
    let mut bytes = serde_json::to_vec_pretty(&index).unwrap();
    bytes.push(b'\n');
    fs::write(path, bytes).unwrap();
}

/// A catalog as its CI keeps one: `index.json` in a folder of its own, and
/// the online key that signs it, as the Actions secret holds it.
struct CiCatalog {
    index: PathBuf,
    secret: String,
    public: String,
}

impl CiCatalog {
    /// An empty catalog in `dir`, its key made in `keys`.
    fn new(dir: &Path, keys: &Path) -> Self {
        fs::create_dir_all(dir).unwrap();
        let public = keygen(keys);
        let secret = fs::read_to_string(keys.join("asp.key")).unwrap();
        let index = dir.join("index.json");
        asp(&["catalog", "new", text(&index)]).ok();
        Self { index, secret, public }
    }

    /// `asp catalog <command> index.json <more>`.
    fn run(&self, command: &str, more: &[&str]) -> Run {
        let mut args = vec!["catalog", command, text(&self.index)];
        args.extend(more);
        asp(&args)
    }

    /// Lists `package` as built from the AutoScene repository.
    fn add(&self, package: &Path) -> Run {
        let source = ["--repo", "https://github.com/tcmmi/AutoScene", "--commit", COMMIT, "--path", "hello"];
        let mut args = vec![text(package), "--url", RELEASE];
        args.extend(source);
        args.extend(["--author", "AutoScene"]);
        self.run("add", &args)
    }

    /// Signs the index with the secret key `secret`, from the environment.
    fn sign_with(&self, secret: &str) -> Run {
        let args = ["catalog", "sign", text(&self.index), "--key-env", "AS_TEST_CATALOG_KEY"];
        asp_env(&args, &[("AS_TEST_CATALOG_KEY", secret)])
    }

    /// Bumps the index and signs it with its online key, as a push does.
    fn publish(&self) {
        self.run("bump", &[]).ok();
        self.sign_with(&self.secret).ok();
    }

    /// `asp catalog verify` with its online key and `more`.
    fn verify(&self, more: &[&str]) -> Run {
        let mut args = vec!["--online-key", self.public.as_str()];
        args.extend(more);
        self.run("verify", &args)
    }

    fn json(&self) -> Value {
        json_of(&self.index)
    }

    fn bytes(&self) -> Vec<u8> {
        fs::read(&self.index).unwrap()
    }

    /// The index and its signature copied to `path` and `<path>.minisig`.
    fn copy_to(&self, path: &Path) {
        fs::copy(&self.index, path).unwrap();
        fs::copy(format!("{}.minisig", self.index.display()), format!("{}.minisig", path.display())).unwrap();
    }
}

#[test]
fn catalog_new_add_bump_sign_verify_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let place = |name: &str| tmp.path().join(name);
    let src = uv_plugin(&place("hello"));
    let package = place("hello.asp");
    asp(&["pack", text(&src), text(&package)]).ok();
    let cat = CiCatalog::new(&place("site"), &place("online"));
    let id = trust::key_id(&cat.public).unwrap();
    let index = text(&cat.index);

    // An empty index, pretty-printed with a trailing newline, never made
    // over one that is there.
    let empty = "{\n  \"catalogVersion\": 1,\n  \"epoch\": 1,\n  \"seq\": 0,\n  \"published\": 0,\n  \"expires\": 0,\n  \
                 \"plugins\": [],\n  \"withdrawn\": []\n}\n";
    assert_eq!(String::from_utf8(cat.bytes()).unwrap(), empty);
    assert_eq!(asp(&["catalog", "new", index]).failed(1), format!("asp: {index} is there already.\n"));

    assert_eq!(cat.add(&package).ok(), format!("Listed {HELLO} 1.2.0 in {index}.\n"));
    assert!(cat.bytes().ends_with(b"}\n"));
    // Not dated yet, so AutoScene would find it ran out: it isn't signed.
    let err = cat.sign_with(&cat.secret).failed(1);
    assert_eq!(err, "asp: The catalog ran out on 1970-01-01.\n");
    assert_eq!(names(&place("site")), ["index.json"]);

    let before = catalog::now();
    let bumped = cat.run("bump", &[]).ok();
    let (published, expires) = (cat.json()["published"].as_u64().unwrap(), cat.json()["expires"].as_u64().unwrap());
    assert!((before..=catalog::now()).contains(&published), "{published}");
    assert_eq!(expires - published, 30 * DAY);
    assert!(bumped.starts_with(&format!("{index} is number 1 of epoch 1, running out on 20")), "{bumped}");
    let signed = cat.sign_with(&cat.secret).ok();
    let sig_path = format!("{index}.minisig");
    assert!(signed.ends_with(&format!(") with key {id} into {sig_path}.\n")), "{signed}");
    // The signature names the index it signs, in its trusted comment.
    let sig = fs::read_to_string(&sig_path).unwrap();
    assert!(sig.contains(&format!("\ntrusted comment: {}\n", index_comment(1, 1, expires))), "{sig}");
    assert!(sig.starts_with("untrusted comment: AutoScene plugin catalog\n"), "{sig}");

    let verified = cat.verify(&[]).ok();
    let lines: Vec<&str> = verified.lines().collect();
    assert!(lines[0].starts_with(&format!("{index}: epoch 1, number 1, published 20")), "{verified}");
    assert!(lines[0].ends_with(&format!(" ({expires}).")), "{verified}");
    let signer = format!("Signed with the online key {id}.");
    assert_eq!(lines[1..], [signer.as_str(), "1 plugin version and no withdrawals."]);
    // AutoScene's own keys are always asked; until the catalog opens there
    // are none, and they don't know this one.
    if CATALOG_KEYS.is_empty() {
        let err = cat.run("verify", &[]).failed(1);
        assert_eq!(err, "asp: The catalog is signed with a key AutoScene doesn't know.\n");
    }

    // A second version, in the next index: newer than the one before.
    let old = place("old.json");
    cat.copy_to(&old);
    edit_manifest(&src, |v| v["version"] = json!("1.3.0"));
    let newer = place("hello-1.3.0.asp");
    asp(&["pack", text(&src), text(&newer)]).ok();
    cat.add(&newer).ok();
    cat.publish();
    let verified = cat.verify(&["--previous", text(&old)]).ok();
    assert!(verified.starts_with(&format!("{index}: epoch 1, number 2, ")), "{verified}");
    assert!(verified.ends_with("\n2 plugin versions and no withdrawals.\n"), "{verified}");

    // Changed after it was signed.
    let changed = String::from_utf8(cat.bytes()).unwrap().replace("Says hello", "Says goodbye");
    fs::write(&cat.index, changed).unwrap();
    assert_eq!(cat.verify(&[]).failed(1), "asp: The catalog was changed after it was signed.\n");

    // Wrong arguments.
    assert!(cat.run("bump", &["--days", "32"]).failed(2).starts_with("asp: --days takes 1 to 31\n"));
    assert!(cat.run("bump", &["--days", "0"]).failed(2).starts_with("asp: --days takes 1 to 31\n"));
    assert!(asp(&["catalog"]).failed(2).starts_with("asp: name a catalog command\n"));
    assert!(asp(&["catalog", "publish", index]).failed(2).starts_with("asp: unknown catalog command \"publish\"\n"));
    let err = cat.run("add", &[text(&package), "--repo", "https://github.com/o/r"]).failed(2);
    assert!(err.starts_with("asp: catalog add needs --url\n"), "{err}");
    assert!(cat.run("sign", &[]).failed(2).starts_with("asp: catalog sign needs --key-env\n"));
    // A file that isn't an index.
    let err = asp(&["catalog", "bump", text(&src.join("plugin.json"))]).failed(1);
    assert_eq!(err, format!("asp: {} isn't a catalog index.\n", src.join("plugin.json").display()));
    let mut later = cat.json();
    later["catalogVersion"] = json!(2);
    write_json(&place("later.json"), &later);
    let err = asp(&["catalog", "bump", text(&place("later.json"))]).failed(1);
    assert_eq!(err, "asp: The catalog is of a later format than this asp writes. Update asp.\n");
    // A number that can't go higher.
    let mut last = cat.json();
    last["seq"] = json!(u64::MAX);
    write_json(&place("last.json"), &last);
    let err = asp(&["catalog", "bump", text(&place("last.json"))]).failed(1);
    assert_eq!(err, "asp: The catalog's number can't go higher; start a new epoch.\n");

    // asp leaves nothing beside the index.
    assert_eq!(names(&place("site")), ["index.json", "index.json.minisig"]);
}

#[test]
fn catalog_sign_reads_its_key_from_the_environment_and_writes_no_key_file() {
    let tmp = tempfile::tempdir().unwrap();
    let place = |name: &str| tmp.path().join(name);
    let package = place("hello.asp");
    asp(&["pack", text(&uv_plugin(&place("hello"))), text(&package)]).ok();
    let cat = CiCatalog::new(&place("site"), &place("online"));
    cat.add(&package).ok();
    cat.run("bump", &[]).ok();
    let index = text(&cat.index);

    // A key locked with a password, as the Actions secrets hold it.
    let pair = minisign::KeyPair::generate_encrypted_keypair(Some("a password".into())).unwrap();
    let secret = pair.sk.to_box(None).unwrap().into_string();
    let public = pair.pk.to_base64();
    let key_line = secret.lines().nth(1).unwrap().to_owned();
    let sign = |env: &[(&str, &str)], more: &[&str]| {
        let mut args = vec!["catalog", "sign", index, "--key-env", "AS_TEST_KEY"];
        args.extend(more);
        asp_env(&args, env)
    };
    let both = [("AS_TEST_KEY", secret.as_str()), ("AS_TEST_PASSWORD", "a password")];
    let run = sign(&both, &["--password-env", "AS_TEST_PASSWORD"]);
    for said in [&run.out, &run.err] {
        assert!(!said.contains(&key_line) && !said.contains("a password"), "{said}");
    }
    run.ok();
    // Nothing but the signature is written, here or beside the index.
    assert_eq!(names(&place("site")), ["index.json", "index.json.minisig"]);
    assert_eq!(names(tmp.path()), ["hello", "hello.asp", "online", "site"]);
    assert!(cat.run("verify", &["--online-key", &public]).ok().contains("Signed with the online key "));

    // A locked key needs its password, the right one, from the environment.
    let err = sign(&both, &[]).failed(1);
    assert_eq!(
        err,
        "asp: The key in $AS_TEST_KEY is locked with a password; --password-env names the variable that holds it.\n"
    );
    let wrong = [("AS_TEST_KEY", secret.as_str()), ("AS_TEST_PASSWORD", "another password")];
    let err = sign(&wrong, &["--password-env", "AS_TEST_PASSWORD"]).failed(1);
    assert_eq!(err, "asp: The key in $AS_TEST_KEY: Wrong password for that key\n");
    let err = sign(&[("AS_TEST_KEY", secret.as_str())], &["--password-env", "AS_TEST_PASSWORD"]).failed(1);
    assert_eq!(err, "asp: $AS_TEST_PASSWORD holds no password.\n");
    assert_eq!(sign(&[], &[]).failed(1), "asp: $AS_TEST_KEY holds no key.\n");
    assert_eq!(sign(&[("AS_TEST_KEY", "")], &[]).failed(1), "asp: $AS_TEST_KEY holds no key.\n");
    // One without a password signs without one; as the Actions secret may
    // hold it, with Windows line ends.
    let crlf = cat.secret.replace('\n', "\r\n");
    sign(&[("AS_TEST_KEY", crlf.as_str())], &[]).ok();
    cat.verify(&[]).ok();

    // An index AutoScene would refuse isn't signed, and the signature
    // beside it stays.
    let sig = fs::read(format!("{index}.minisig")).unwrap();
    let mut v = cat.json();
    v["plugins"][0]["package"]["urls"] = json!(["http://example.com/hello-export.asp"]);
    write_json(&cat.index, &v);
    let err = cat.sign_with(&cat.secret).failed(1);
    assert_eq!(err, format!("asp: The catalog has an entry AutoScene can't read: {HELLO} 1.2.0.\n"));
    v["plugins"][0]["package"]["urls"] = json!([RELEASE]);
    v["expires"] = json!(v["published"].as_u64().unwrap() + 32 * DAY);
    write_json(&cat.index, &v);
    assert_eq!(cat.sign_with(&cat.secret).failed(1), "asp: The catalog's dates can't be right.\n");
    assert_eq!(fs::read(format!("{index}.minisig")).unwrap(), sig);
    assert_eq!(names(&place("site")), ["index.json", "index.json.minisig"]);

    // Only the bytes asp writes are signed: AutoScene refuses an object
    // that names a key twice, which asp's reading would take; and other
    // line ends or spacing are another index than the one checked.
    cat.run("bump", &[]).ok();
    let asp_wrote = String::from_utf8(cat.bytes()).unwrap();
    let not_asps = format!(
        "asp: {index} isn't written as asp writes an index, so AutoScene might read it otherwise. Any asp catalog \
         command that changes it writes it again.\n"
    );
    let twice = asp_wrote.replacen("{\n", "{\n  \"withdrawn\": [{\"id\": \"x\"}],\n", 1);
    for other in [twice, asp_wrote.replace('\n', "\r\n"), asp_wrote.replace("  ", "\t")] {
        fs::write(&cat.index, other).unwrap();
        assert_eq!(cat.sign_with(&cat.secret).failed(1), not_asps);
    }
    assert_eq!(fs::read(format!("{index}.minisig")).unwrap(), sig);
    fs::write(&cat.index, &asp_wrote).unwrap();
    cat.sign_with(&cat.secret).ok();
}

#[test]
fn catalog_add_takes_the_entry_from_the_package() {
    let tmp = tempfile::tempdir().unwrap();
    let place = |name: &str| tmp.path().join(name);
    // A uv plugin that names its license, platforms, permissions, hosts,
    // the GPU and an icon.
    let icon = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"/>"#;
    let src = uv_plugin(&place("hello"));
    fs::write(src.join("icon.svg"), icon).unwrap();
    edit_manifest(&src, |v| {
        v["license"] = json!("MIT");
        v["platforms"] = json!(["linux-x86_64", "windows-x86_64"]);
        v["permissions"] = json!(["network", "exports:read"]);
        v["network"] = json!({"hosts": ["api.example.com"]});
        v["runtime"]["gpu"] = json!(true);
        v["icon"] = json!("icon.svg");
    });
    let package = place("hello.asp");
    asp(&["pack", text(&src), text(&package)]).ok();
    let cat = CiCatalog::new(&place("site"), &place("online"));

    // What AutoScene would refuse of the entry is refused at once, and the
    // index stays as it was.
    let empty = cat.bytes();
    let refused = format!("asp: The catalog has an entry AutoScene can't read: {HELLO} 1.2.0.\n");
    let add = |more: &[&str]| {
        let mut args = vec![text(&package), "--url", RELEASE, "--repo", "https://github.com/tcmmi/AutoScene"];
        args.extend(["--commit", COMMIT, "--author", "AutoScene"]);
        args.extend(more);
        cat.run("add", &args)
    };
    for wrong in [
        &["--url", "http://example.com/hello.asp"][..],
        &["--repo", "https://gitlab.com/tcmmi/AutoScene"],
        &["--commit", "c0ffee"],
        &["--path", "../hello"],
        &["--author", "  "],
        &["--author-key", "RWQ not a key"],
    ] {
        assert_eq!(add(wrong).failed(1), refused, "{wrong:?}");
        assert_eq!(cat.bytes(), empty, "{wrong:?}");
    }

    let before = catalog::now();
    cat.add(&package).ok();
    let after = catalog::now();
    let entry = &cat.json()["plugins"][0];
    let approved = entry["approved"].as_u64().unwrap();
    assert!((before..=after).contains(&approved), "{approved}");
    let bytes = fs::read(&package).unwrap();
    let icon_url = format!("data:image/svg+xml;base64,{}", STANDARD.encode(icon));
    assert_eq!(
        *entry,
        json!({
            "id": HELLO,
            "version": "1.2.0",
            "lock": package::sha256_hex(&entries(&package)[LOCK]),
            "package": {"urls": [RELEASE], "size": bytes.len(), "sha256": package::sha256_hex(&bytes)},
            "authorKey": null,
            "name": "Hello Export",
            "summary": "Says hello after an export.",
            "author": "AutoScene",
            "license": "MIT",
            // In AutoScene's order of platforms.
            "platforms": ["windows-x86_64", "linux-x86_64"],
            "runtime": "process",
            "gpu": true,
            "uvScript": true,
            "permissions": ["network", "exports:read"],
            "hosts": ["api.example.com"],
            "engines": ">=1.0.0, <2",
            "source": {"repo": "https://github.com/tcmmi/AutoScene", "commit": COMMIT, "path": "hello"},
            "icon": icon_url,
            "approved": approved,
        })
    );

    // A plugin that runs no program runs on every platform, as `none`. An
    // author's key goes in as given, and several addresses in their order.
    let pages = folder(&place("page"), json!({}), &[("ui/index.html", b"<!doctype html>")]);
    edit_manifest(&pages, |v| {
        v["manifestVersion"] = json!(2);
        v["version"] = json!("2.0.0");
        v["runtime"] = json!({"type": "none"});
        v["contributes"] = json!({"pages": [{"id": "main", "title": "Main", "entry": "ui/index.html"}]});
    });
    let page = place("page.asp");
    asp(&["pack", text(&pages), text(&page)]).ok();
    let author = keygen(&place("author"));
    let mirror = "https://example.com/mirror/hello-export-2.0.0.asp";
    let more = [text(&page), "--url", RELEASE, "--url", mirror, "--repo", "https://github.com/o/r", "--commit", COMMIT];
    cat.run("add", &[&more[..], &["--author", "Someone Else", "--author-key", &format!(" {author}\r")]].concat()).ok();
    let entry = &cat.json()["plugins"][1];
    let wanted = [
        ("platforms", json!([])),
        ("runtime", json!("none")),
        ("uvScript", json!(false)),
        ("gpu", json!(false)),
        ("icon", json!(null)),
        ("license", json!(null)),
        ("authorKey", json!(author)),
        ("author", json!("Someone Else")),
        ("source", json!({"repo": "https://github.com/o/r", "commit": COMMIT, "path": ""})),
    ];
    for (key, value) in wanted {
        assert_eq!(entry[key], value, "{key}");
    }
    assert_eq!(entry["package"]["urls"], json!([RELEASE, mirror]));

    // An icon larger than the catalog carries is left out, with a warning.
    let big = format!(r#"<svg xmlns="http://www.w3.org/2000/svg"><!-- {} --></svg>"#, "x".repeat(8 << 10));
    fs::write(src.join("icon.svg"), big).unwrap();
    edit_manifest(&src, |v| v["version"] = json!("1.3.0"));
    let big_icon = place("big-icon.asp");
    asp(&["pack", text(&src), text(&big_icon)]).ok();
    let warned = cat.add(&big_icon).warnings();
    assert_eq!(warned, "warning: The icon is larger than 8 KB, so the catalog lists the plugin without it.\n");
    assert_eq!(cat.json()["plugins"][2]["icon"], json!(null));

    // A PNG icon. One uv script, named for no platform, runs on all, also on
    // those a later AutoScene adds.
    let small = png(64, 64);
    fs::write(src.join("icon.png"), &small).unwrap();
    fs::remove_file(src.join("icon.svg")).unwrap();
    edit_manifest(&src, |v| {
        v["version"] = json!("1.4.0");
        v["icon"] = json!("icon.png");
        v.as_object_mut().unwrap().remove("platforms");
    });
    let png_icon = place("png-icon.asp");
    asp(&["pack", text(&src), text(&png_icon)]).ok();
    cat.add(&png_icon).ok();
    let entry = &cat.json()["plugins"][3];
    assert_eq!(entry["icon"], json!(format!("data:image/png;base64,{}", STANDARD.encode(&small))));
    assert_eq!((&entry["platforms"], &entry["uvScript"]), (&json!([]), &json!(true)));

    // A plugin with a program lists the platforms its command starts on:
    // not Windows on x86, whose program isn't in the package, nor Windows on
    // Arm, where a program must be an .exe.
    let command = json!({"windows-x86_64": ["bin/tool.exe"], "*": ["bin/tool"]});
    let tool = folder(&place("tool"), command, &[("bin/tool", b"#!/bin/sh\n")]);
    edit_manifest(&tool, |v| v["version"] = json!("4.0.0"));
    let program = place("tool.asp");
    asp(&["pack", text(&tool), text(&program)]).warnings();
    cat.add(&program).warnings();
    let entry = &cat.json()["plugins"][4];
    assert_eq!(entry["platforms"], json!(["macos-aarch64", "macos-x86_64", "linux-x86_64", "linux-aarch64"]));
    assert_eq!((&entry["runtime"], &entry["uvScript"]), (&json!("process"), &json!(false)));
}

#[test]
fn catalog_add_lists_a_webassembly_plugin_on_every_platform() {
    let Some(guest) = guest() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let place = |name: &str| tmp.path().join(name);
    let wasm = wasm_plugin(&place("wasm"), &guest, |_| {});
    let cat = CiCatalog::new(&place("site"), &place("online"));

    // A component runs wherever AutoScene does, also on a platform a later
    // AutoScene adds: it is listed as running on all.
    let everywhere = place("everywhere.asp");
    let packed = asp(&["pack", text(&wasm), text(&everywhere)]).warnings();
    assert_eq!(cat.add(&everywhere).warnings(), packed, "add checks the component as pack does");
    let entry = &cat.json()["plugins"][0];
    assert_eq!((&entry["runtime"], &entry["platforms"]), (&json!("wasm"), &json!([])));
    assert_eq!((&entry["uvScript"], &entry["gpu"]), (&json!(false), &json!(false)));

    // One that names its platforms installs only there, and is listed so.
    edit_manifest(&wasm, |v| {
        v["version"] = json!("1.3.0");
        v["platforms"] = json!(["linux-x86_64", "windows-x86_64"]);
    });
    let named = place("named.asp");
    asp(&["pack", text(&wasm), text(&named)]).warnings();
    cat.add(&named).warnings();
    let entry = &cat.json()["plugins"][1];
    assert_eq!((&entry["runtime"], &entry["platforms"]), (&json!("wasm"), &json!(["windows-x86_64", "linux-x86_64"])));
}

#[test]
fn catalog_add_refuses_a_version_already_listed() {
    let tmp = tempfile::tempdir().unwrap();
    let place = |name: &str| tmp.path().join(name);
    let src = uv_plugin(&place("hello"));
    let package = place("hello.asp");
    asp(&["pack", text(&src), text(&package)]).ok();
    let cat = CiCatalog::new(&place("site"), &place("online"));
    cat.add(&package).ok();
    let listed = cat.bytes();

    // The same package, and another one of the same version, also with
    // build metadata (which doesn't count).
    let already = format!("asp: The catalog lists {HELLO} 1.2.0 already.\n");
    assert_eq!(cat.add(&package).failed(1), already);
    for version in ["1.2.0", "1.2.0+build.7"] {
        fs::write(src.join("lib/data.txt"), version).unwrap();
        edit_manifest(&src, |v| v["version"] = json!(version));
        let again = place(&format!("{version}.asp"));
        asp(&["pack", text(&src), text(&again)]).ok();
        assert_eq!(cat.add(&again).failed(1), already, "{version}");
    }
    assert_eq!(cat.bytes(), listed);
    // A pre-release of it is another version.
    edit_manifest(&src, |v| v["version"] = json!("1.2.0-rc.1"));
    asp(&["pack", text(&src), text(&place("rc.asp"))]).ok();
    cat.add(&place("rc.asp")).ok();
    assert_eq!(cat.json()["plugins"].as_array().unwrap().len(), 2);
}

#[test]
fn catalog_verify_refuses_an_index_that_drops_an_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let place = |name: &str| tmp.path().join(name);
    let package = place("hello.asp");
    asp(&["pack", text(&uv_plugin(&place("hello"))), text(&package)]).ok();
    let cat = CiCatalog::new(&place("site"), &place("online"));
    cat.add(&package).ok();
    cat.publish();
    let old = place("old.json");
    cat.copy_to(&old);
    let kept = cat.json();

    // The next index keeps it: it passes.
    cat.publish();
    cat.verify(&["--previous", text(&old)]).ok();
    // One that leaves it out, or lists it with another lock, is refused,
    // though signed and newer.
    let refused = |edit: &dyn Fn(&mut Value)| {
        let mut v = kept.clone();
        edit(&mut v);
        write_json(&cat.index, &v);
        cat.publish();
        cat.verify(&[]).ok();
        cat.verify(&["--previous", text(&old)]).failed(1)
    };
    let err = refused(&|v| v["plugins"] = json!([]));
    assert_eq!(err, format!("asp: The catalog drops {HELLO} 1.2.0, which {} lists.\n", old.display()));
    let err = refused(&|v| v["plugins"][0]["lock"] = json!("d".repeat(64)));
    assert_eq!(
        err,
        format!("asp: The catalog lists {HELLO} 1.2.0 with another files.lock than {} does.\n", old.display())
    );
    // An old index that isn't one says so.
    let err = cat.verify(&["--previous", text(&package)]).failed(1);
    assert!(err.starts_with(&format!("asp: {}: ", package.display())), "{err}");
}

#[test]
fn catalog_verify_refuses_a_lower_number() {
    let tmp = tempfile::tempdir().unwrap();
    let place = |name: &str| tmp.path().join(name);
    let package = place("hello.asp");
    asp(&["pack", text(&uv_plugin(&place("hello"))), text(&package)]).ok();
    let cat = CiCatalog::new(&place("site"), &place("online"));
    cat.add(&package).ok();
    cat.publish();
    let (one, two) = (place("one.json"), place("two.json"));
    cat.copy_to(&one);
    cat.publish();
    cat.copy_to(&two);

    let verify = |path: &Path, previous: &Path| {
        let args = [text(path), "--previous", text(previous), "--online-key", &cat.public];
        asp(&[&["catalog", "verify"][..], &args].concat())
    };
    verify(&two, &one).ok();
    assert_eq!(verify(&one, &two).failed(1), "asp: This catalog is older than the one AutoScene has.\n");
    // The same index again is no news, and no failure.
    assert!(verify(&two, &two).ok().contains(": epoch 1, number 2, "));
    // Another index of the same number is refused.
    let withdrawn = place("withdrawn.json");
    let reason = "It sent export paths to a server.";
    fs::write(&withdrawn, json!([{"id": HELLO, "versions": "*", "reason": reason, "since": 1}]).to_string()).unwrap();
    cat.run("withdrawn", &[text(&withdrawn)]).ok();
    cat.sign_with(&cat.secret).ok();
    assert_eq!(cat.json()["seq"], json!(2));
    assert_eq!(cat.verify(&["--previous", text(&two)]).failed(1), "asp: Two catalogs carry the same number.\n");
}

#[test]
fn catalog_certify_makes_a_certificate_the_app_takes_only_from_the_offline_key() {
    let tmp = tempfile::tempdir().unwrap();
    let place = |name: &str| tmp.path().join(name);
    let package = place("hello.asp");
    asp(&["pack", text(&uv_plugin(&place("hello"))), text(&package)]).ok();
    let cat = CiCatalog::new(&place("site"), &place("online"));
    cat.add(&package).ok();
    cat.publish();
    let first = place("epoch-1.json");
    cat.copy_to(&first);
    let index = text(&cat.index);
    let (offline, next) = (keygen(&place("offline")), keygen(&place("next")));
    let (offline_key, next_secret) =
        (place("offline").join("asp.key"), fs::read_to_string(place("next").join("asp.key")).unwrap());
    let (offline_id, online_id, next_id) =
        (trust::key_id(&offline).unwrap(), trust::key_id(&cat.public).unwrap(), trust::key_id(&next).unwrap());
    let keys = ["--online-key", cat.public.as_str(), "--offline-key", offline.as_str()];

    // The owner certifies epoch 2 with the offline key: the next online key
    // signs there, and the old one is withdrawn.
    let cert = place("epoch-2.minisig");
    let certify = |epoch: &str, key: &Path, more: &[&str], out: &Path| {
        let args = ["catalog", "certify", "--epoch", epoch, "--key", text(key), "--out", text(out)];
        asp(&[&args[..], more].concat())
    };
    let made = certify("2", &offline_key, &["--online-key", &next, "--withdraw-key", &online_id.to_lowercase()], &cert);
    assert_eq!(made.ok(), format!("Certified epoch 2 with key {offline_id} into {}.\n", cert.display()));
    let certificate = fs::read_to_string(&cert).unwrap();
    let line = catalog::certificate_line(2, std::slice::from_ref(&online_id), std::slice::from_ref(&next));
    assert!(certificate.contains(&format!("\ntrusted comment: {line}\n")), "{certificate}");

    // Attached, it starts epoch 2 at number 0; again, it changes nothing.
    assert_eq!(cat.run("epoch", &[text(&cert)]).ok(), format!("{index} starts epoch 2.\n"));
    let v = cat.json();
    assert_eq!((&v["epoch"], &v["seq"], &v["epochCertificate"]), (&json!(2), &json!(0), &json!(certificate)));
    assert_eq!(cat.run("epoch", &[text(&cert)]).ok(), format!("{index} is in epoch 2 already.\n"));
    // Also with other line ends, as a checkout on Windows may give it.
    let crlf = place("epoch-2-crlf.minisig");
    fs::write(&crlf, certificate.replace('\n', "\r\n")).unwrap();
    assert_eq!(cat.run("epoch", &[text(&crlf)]).ok(), format!("{index} is in epoch 2 already.\n"));
    assert_eq!(cat.json()["epochCertificate"], json!(certificate));
    // A certificate made is never made over.
    let again = ["--online-key", next.as_str(), "--withdraw-key", online_id.as_str()];
    let err = certify("2", &offline_key, &again, &cert).failed(1);
    assert_eq!(err, format!("asp: {} is there already; --force replaces it.\n", cert.display()));
    assert_eq!(fs::read_to_string(&cert).unwrap(), certificate);
    certify("2", &offline_key, &[&again[..], &["--force"]].concat(), &cert).ok();
    cat.run("bump", &[]).ok();
    cat.sign_with(&next_secret).ok();
    let verified = asp(&[&["catalog", "verify", index, "--previous", text(&first)][..], &keys].concat()).ok();
    assert!(verified.starts_with(&format!("{index}: epoch 2, number 1, ")), "{verified}");
    assert!(verified.contains(&format!("\nSigned with the online key {next_id}.\n")), "{verified}");
    // The offline key signs an index too.
    cat.sign_with(&fs::read_to_string(&offline_key).unwrap()).ok();
    let verified = asp(&[&["catalog", "verify", index][..], &keys].concat()).ok();
    assert!(verified.contains(&format!("\nSigned with the offline key {offline_id}.\n")), "{verified}");
    // The key it withdrew signs no more.
    cat.sign_with(&cat.secret).ok();
    let err = asp(&[&["catalog", "verify", index][..], &keys].concat()).failed(1);
    assert_eq!(err, "asp: The catalog is signed with a key AutoScene withdrew.\n");

    // A certificate signed by another key than the offline one: asp can't
    // tell, AutoScene refuses it.
    let forged = place("epoch-3.minisig");
    certify("3", &place("next").join("asp.key"), &["--online-key", &next, "--previous", text(&cert)], &forged).ok();
    cat.run("epoch", &[text(&forged)]).ok();
    cat.run("bump", &[]).ok();
    cat.sign_with(&next_secret).ok();
    let err = asp(&[&["catalog", "verify", index][..], &keys].concat()).failed(1);
    assert_eq!(err, "asp: The catalog's new epoch isn't signed by AutoScene's offline key.\n");

    // A lower epoch, or another certificate of this one, is refused.
    let err = cat.run("epoch", &[text(&cert)]).failed(1);
    assert_eq!(err, "asp: The certificate is for epoch 2, and the catalog is in epoch 3 already.\n");
    let other = place("other-3.minisig");
    certify("3", &offline_key, &["--online-key", &next, "--previous", text(&cert)], &other).ok();
    let err = cat.run("epoch", &[text(&other)]).failed(1);
    assert_eq!(err, "asp: The certificate is for epoch 3, and the catalog is in epoch 3 already.\n");
    assert_eq!(
        cat.run("epoch", &[text(&place("hello").join("plugin.json"))]).failed(1),
        "asp: That isn't an epoch certificate, as asp catalog certify makes one.\n"
    );
    // An index of a later epoch isn't signed without its certificate.
    let mut v = cat.json();
    v.as_object_mut().unwrap().remove("epochCertificate");
    write_json(&cat.index, &v);
    assert_eq!(
        cat.sign_with(&next_secret).failed(1),
        "asp: An index of epoch 3 needs its epoch certificate, which asp catalog epoch attaches.\n"
    );

    // What a certificate can't say.
    let out = place("refused.minisig");
    let err = certify("4", &offline_key, &["--withdraw-key", &offline_id], &out).failed(1);
    assert_eq!(err, "asp: A certificate can't withdraw the key that signs it.\n");
    let err = certify("4", &offline_key, &["--online-key", &next, "--withdraw-key", &next_id], &out).failed(1);
    assert_eq!(err, format!("asp: The certificate both names and withdraws key {next_id}.\n"));
    let err = certify("4", &offline_key, &["--previous", text(&cert)], &out).warnings();
    assert_eq!(err, "warning: It names no online key: only the offline key signs the indexes of this epoch.\n");
    assert!(certify("4", &offline_key, &["--withdraw-key", "nope"], &out).failed(2).starts_with("asp: --withdraw-key"));
    assert!(certify("0", &offline_key, &[], &out).failed(2).starts_with("asp: --epoch takes a number from 1\n"));
    let err = certify("4", &offline_key, &["--online-key", "RWQ not a key"], &out).failed(1);
    assert_eq!(err, "asp: That isn't a minisign public key.\n");
}

#[test]
fn catalog_diff_lists_what_a_version_adds() {
    let tmp = tempfile::tempdir().unwrap();
    let place = |name: &str| tmp.path().join(name);
    let src = uv_plugin(&place("hello"));
    edit_manifest(&src, |v| v["permissions"] = json!(["exports:read"]));
    let first = place("hello-1.2.0.asp");
    asp(&["pack", text(&src), text(&first)]).ok();
    let cat = CiCatalog::new(&place("site"), &place("online"));
    let diff = |index: &Path, package: &Path| asp(&["catalog", "diff", text(index), text(package)]).ok();
    // What the package's files come to, and the package.
    let files = |package: &Path| {
        let lock: Value = serde_json::from_slice(&entries(package)[LOCK]).unwrap();
        let sizes = lock["files"].as_array().unwrap().iter().map(|f| f["size"].as_u64().unwrap());
        let (count, bytes) = (sizes.len(), sizes.sum::<u64>());
        format!("{count} files, {bytes} bytes; the package is {} bytes", fs::metadata(package).unwrap().len())
    };

    // A new plugin, also in an index that isn't there yet.
    let new = format!(
        "### `Hello Export` 1.2.0 (`{HELLO}`)\n\nA new plugin.\n\n\
         - Permissions: `exports:read`\n- Hosts: none\n- Platforms: all\n- Runtime: process\n\
         - Python script through uv: yes\n- GPU: no\n- AutoScene: `>=1.0.0, <2`\n- Files: {}\n",
        files(&first)
    );
    assert_eq!(diff(&cat.index, &first), new);
    assert_eq!(diff(&place("missing.json"), &first), new);
    cat.add(&first).ok();

    // What 1.3.0 changes against 1.2.0.
    fs::write(src.join("lib/more.txt"), b"more").unwrap();
    edit_manifest(&src, |v| {
        v["version"] = json!("1.3.0");
        v["permissions"] = json!(["network"]);
        v["network"] = json!({"hosts": ["api.example.com"]});
        v["platforms"] = json!(["linux-x86_64"]);
        v["runtime"]["gpu"] = json!(true);
        v["engines"]["autoscene"] = json!(">=1.0.0, <3");
    });
    let next = place("hello-1.3.0.asp");
    asp(&["pack", text(&src), text(&next)]).ok();
    let was = fs::metadata(&first).unwrap().len();
    assert_eq!(
        diff(&cat.index, &next),
        format!(
            "### `Hello Export` 1.3.0 (`{HELLO}`)\n\nAgainst 1.2.0, the newest version the catalog lists and hasn't \
             withdrawn:\n\n- Permissions added: `network`\n- Permissions removed: `exports:read`\n\
             - Hosts added: `api.example.com`\n- Platforms: linux-x86_64 (was all)\n- GPU: yes (was no)\n\
             - AutoScene: `>=1.0.0, <3` (was `>=1.0.0, <2`)\n- Files: {} (was {was} bytes)\n",
            files(&next)
        )
    );
    // Against the newest listed, by version, not by when it was added.
    cat.add(&next).ok();
    edit_manifest(&src, |v| v["version"] = json!("1.2.5"));
    let between = place("hello-1.2.5.asp");
    asp(&["pack", text(&src), text(&between)]).ok();
    let said = diff(&cat.index, &between);
    let as_before = "- Permissions, hosts, platforms, runtime, uv, GPU and AutoScene: as before\n- Files: ";
    assert!(
        said.contains(&format!(
            "\n\nAgainst 1.3.0, the newest version the catalog lists and hasn't withdrawn:\n\n{as_before}"
        )),
        "{said}"
    );
    // One it lists already says so.
    let said = diff(&cat.index, &first);
    assert!(
        said.contains("\n\nThe catalog lists 1.2.0 already, so `asp catalog add` refuses it.\n\nAgainst 1.3.0, "),
        "{said}"
    );
    assert!(said.contains("\n- Permissions added: `exports:read`\n- Permissions removed: `network`\n- Hosts removed: `api.example.com`\n"), "{said}");

    // A version that runs no program, against one that runs a uv script.
    fs::create_dir_all(src.join("ui")).unwrap();
    fs::write(src.join("ui/index.html"), b"<!doctype html>").unwrap();
    edit_manifest(&src, |v| {
        v["manifestVersion"] = json!(2);
        v["version"] = json!("2.0.0");
        v["runtime"] = json!({"type": "none"});
        v["contributes"] = json!({"pages": [{"id": "main", "title": "Main", "entry": "ui/index.html"}]});
    });
    let pages = place("hello-2.0.0.asp");
    asp(&["pack", text(&src), text(&pages)]).ok();
    let said = diff(&cat.index, &pages);
    assert!(said.contains("\n- Platforms: all (was linux-x86_64)\n- Runtime: none (was process)\n- Python script through uv: no (was yes)\n- GPU: no (was yes)\n"), "{said}");

    // A withdrawn version isn't what a version is shown against; when all
    // are withdrawn, it is shown in full.
    let withdraw = |versions: &str| {
        let list = place("withdrawn.json");
        fs::write(&list, json!([{"id": HELLO, "versions": versions, "reason": "It leaked.", "since": 1}]).to_string())
            .unwrap();
        cat.run("withdrawn", &[text(&list)]).ok();
    };
    withdraw("=1.3.0");
    let said = diff(&cat.index, &pages);
    assert!(
        said.contains("\n\nAgainst 1.2.0, the newest version the catalog lists and hasn't withdrawn:\n\n"),
        "{said}"
    );
    withdraw("*");
    let said = diff(&cat.index, &pages);
    assert!(said.contains("\n\nThe catalog withdrew every version of it it lists.\n\n- Permissions: "), "{said}");

    // What the author calls it is in code: no Markdown, no HTML, and no
    // version or id of its own.
    edit_manifest(&src, |v| v["name"] = json!("<b>Hi</b> 9.9.9 (`dev.x.y`)"));
    let named = place("named.asp");
    asp(&["pack", text(&src), text(&named)]).ok();
    let said = diff(&cat.index, &named);
    assert!(said.starts_with(&format!("### `<b>Hi</b> 9.9.9 ('dev.x.y')` 2.0.0 (`{HELLO}`)\n\n")), "{said}");
}

#[test]
fn catalog_keys_a_certificate_withdrew_stay_withdrawn() {
    let tmp = tempfile::tempdir().unwrap();
    let place = |name: &str| tmp.path().join(name);
    let package = place("hello.asp");
    asp(&["pack", text(&uv_plugin(&place("hello"))), text(&package)]).ok();
    let cat = CiCatalog::new(&place("site"), &place("online"));
    cat.add(&package).ok();
    cat.publish();
    let index = text(&cat.index);
    let (offline, next) = (keygen(&place("offline")), keygen(&place("next")));
    let (offline_key, next_secret) =
        (place("offline").join("asp.key"), fs::read_to_string(place("next").join("asp.key")).unwrap());
    let (first_id, next_id) = (trust::key_id(&cat.public).unwrap(), trust::key_id(&next).unwrap());
    let certify = |epoch: &str, more: &[&str], out: &Path| {
        let args = ["catalog", "certify", "--epoch", epoch, "--key", text(&offline_key), "--out", text(out)];
        asp(&[&args[..], more].concat())
    };
    let verify =
        |more: &[&str]| asp(&[&["catalog", "verify", index, "--offline-key", offline.as_str()][..], more].concat());

    // Epoch 2 withdraws the first online key and names the next.
    let two = place("epoch-2.minisig");
    certify("2", &["--withdraw-key", &first_id, "--online-key", &next], &two).ok();
    cat.run("epoch", &[text(&two)]).ok();
    cat.run("bump", &[]).ok();
    cat.sign_with(&next_secret).ok();
    let held = place("held.json");
    cat.copy_to(&held);

    // A certificate of epoch 3 that names the withdrawn key again: certify
    // refuses it after epoch 2's, and without it warns, and epoch refuses it.
    let names_it = ["--online-key", cat.public.as_str()];
    let err = certify("3", &[&names_it[..], &["--previous", text(&two)]].concat(), &place("x.minisig")).failed(1);
    let named =
        format!("asp: The certificate of epoch 3 names key {first_id}, which that of epoch 2 withdrew for good.\n");
    assert_eq!(err, named);
    let three = place("epoch-3.minisig");
    let warned = certify("3", &names_it, &three).warnings();
    assert!(warned.starts_with("warning: Without --previous, it doesn't withdraw again what earlier"), "{warned}");
    assert_eq!(cat.run("epoch", &[text(&three)]).failed(1), named);
    // Nor may it leave the withdrawal out.
    let drops = place("drops.minisig");
    certify("3", &["--online-key", &next], &drops).warnings();
    assert_eq!(
        cat.run("epoch", &[text(&drops)]).failed(1),
        format!(
            "asp: The certificate of epoch 3 doesn't withdraw key {first_id}, which that of epoch 2 withdrew. asp \
             catalog certify --previous withdraws it again.\n"
        )
    );
    let previous = ["--previous", text(&two)];
    let err = certify("2", &previous, &place("y.minisig")).failed(1);
    assert_eq!(err, "asp: --previous is the certificate of epoch 2, not of an earlier one.\n");

    // Made with asp's checks out of the way, and signed with the withdrawn
    // key: an AutoScene that took epoch 2 refuses it, and so does verify.
    // (A fresh one takes it: the next release drops that key.)
    let at_three = |cert: &Path, secret: &str| {
        let certificate = fs::read_to_string(cert).unwrap();
        edit_index(&cat.index, |i| {
            (i.epoch, i.seq, i.epoch_certificate) = (3, 1, Some(certificate));
        });
        cat.sign_with(secret).ok();
    };
    at_three(&three, &cat.secret);
    let refused = verify(&["--previous", text(&held)]).failed(1);
    assert_eq!(refused, "asp: The catalog is signed with a key AutoScene withdrew.\n");
    assert!(verify(&[]).ok().contains(&format!("\nSigned with the online key {first_id}.\n")));
    // One that leaves the withdrawal out is refused too, whoever signed it.
    at_three(&drops, &next_secret);
    assert_eq!(
        verify(&["--previous", text(&held)]).failed(1),
        format!(
            "asp: The certificate of epoch 3 doesn't withdraw key {first_id}, which that of epoch 2 withdrew. asp \
             catalog certify --previous withdraws it again.\n"
        )
    );

    // An index whose certificate is for another epoch isn't signed.
    let certificate = fs::read_to_string(&two).unwrap();
    edit_index(&cat.index, |i| i.epoch_certificate = Some(certificate));
    assert_eq!(cat.sign_with(&next_secret).failed(1), "asp: The catalog's epoch certificate is for another epoch.\n");

    // With --previous, epoch 3 withdraws the key again, and passes.
    edit_index(&cat.index, |i| (i.epoch, i.seq, i.epoch_certificate) = (2, 1, Some(fs::read_to_string(&two).unwrap())));
    let good = place("epoch-3-good.minisig");
    certify("3", &[&previous[..], &["--online-key", &next]].concat(), &good).ok();
    let line = catalog::certificate_line(3, std::slice::from_ref(&first_id), std::slice::from_ref(&next));
    assert!(fs::read_to_string(&good).unwrap().contains(&format!("\ntrusted comment: {line}\n")));
    assert_eq!(cat.run("epoch", &[text(&good)]).ok(), format!("{index} starts epoch 3.\n"));
    cat.run("bump", &[]).ok();
    cat.sign_with(&next_secret).ok();
    let verified = verify(&["--previous", text(&held)]).ok();
    assert!(verified.contains(&format!("\nSigned with the online key {next_id}.\n")), "{verified}");
}

#[test]
fn verify_says_reviewed_when_the_catalog_lists_the_package() {
    let tmp = tempfile::tempdir().unwrap();
    let place = |name: &str| tmp.path().join(name);
    let src = uv_plugin(&place("hello"));
    let (unsigned, signed) = (place("unsigned.asp"), place("signed.asp"));
    asp(&["pack", text(&src), text(&unsigned)]).ok();
    let author = keygen(&place("author"));
    asp(&["sign", text(&unsigned), "--key", text(&place("author").join("asp.key")), "--out", text(&signed)]).ok();
    fs::write(src.join("lib/data.txt"), b"other").unwrap();
    let other = place("other.asp");
    asp(&["pack", text(&src), text(&other)]).ok();
    let cat = CiCatalog::new(&place("site"), &place("online"));
    cat.add(&unsigned).ok();
    cat.publish();
    let trust = |package: &Path| {
        let args = ["verify", text(package), "--catalog", text(&cat.index), "--online-key", &cat.public];
        asp(&args).ok().trim().strip_prefix(&format!("{HELLO} 1.2.0: ")).unwrap().to_owned()
    };

    // The lock the catalog lists is Reviewed, signed or not; another lock
    // of the same version isn't.
    assert_eq!(trust(&unsigned), "Reviewed");
    assert_eq!(trust(&signed), "Reviewed");
    assert_eq!(trust(&other), "Unreviewed");
    let id = trust::key_id(&author).unwrap();
    asp(&["sign", text(&other), "--key", text(&place("author").join("asp.key"))]).ok();
    assert_eq!(trust(&other), format!("Pinned (key {id})"));

    // A withdrawal the catalog sets fails the version, though it lists it.
    let reason = "It sent export paths to a server.";
    let list = place("withdrawn.json");
    let now = catalog::now();
    fs::write(&list, json!([{"id": HELLO, "versions": "<=1.2.0", "reason": reason, "since": now}]).to_string())
        .unwrap();
    assert_eq!(cat.run("withdrawn", &[text(&list)]).ok(), format!("{} has 1 withdrawal.\n", cat.index.display()));
    cat.publish();
    assert!(cat.verify(&[]).ok().ends_with("\n1 plugin version and 1 withdrawal.\n"));
    for package in [&unsigned, &signed] {
        let args = ["verify", text(package), "--catalog", text(&cat.index), "--online-key", &cat.public];
        let why = format!("AutoScene withdrew Hello Export. {reason}");
        assert_eq!(asp(&args).failed(1), format!("asp: {why} (plugin withdrawn: Hello Export: {reason})\n"));
    }
    // Lifted, it is Reviewed again.
    fs::write(&list, b"[]").unwrap();
    cat.run("withdrawn", &[text(&list)]).ok();
    cat.publish();
    assert_eq!(trust(&unsigned), "Reviewed");

    // A withdrawal AutoScene couldn't read isn't set.
    let listed = cat.bytes();
    let refused = format!("asp: The catalog has a withdrawal AutoScene can't read: {HELLO}.\n");
    for wrong in [
        json!([{"id": HELLO, "versions": "*", "reason": " ", "since": now}]),
        json!([{"id": HELLO, "versions": "not a range", "reason": reason, "since": now}]),
        json!([{"id": HELLO, "versions": "*", "reason": reason, "since": now + 7 * DAY}]),
    ] {
        fs::write(&list, wrong.to_string()).unwrap();
        assert_eq!(cat.run("withdrawn", &[text(&list)]).failed(1), refused, "{wrong}");
    }
    fs::write(&list, json!([{"id": "Not An Id", "versions": "*", "reason": reason, "since": now}]).to_string())
        .unwrap();
    assert!(cat.run("withdrawn", &[text(&list)]).failed(1).starts_with(&format!("asp: {}: ", list.display())));
    assert_eq!(cat.bytes(), listed);
}

/// The Python and the dependencies a uv script's `# /// script` block asks
/// for, each dependency without spaces and in lowercase.
fn script_wants(script: &str) -> (String, BTreeSet<String>) {
    let block: String = script
        .lines()
        .skip_while(|l| *l != "# /// script")
        .skip(1)
        .take_while(|l| *l != "# ///")
        .map(|l| format!("{}\n", l.strip_prefix("# ").or(l.strip_prefix('#')).unwrap()))
        .collect();
    let block: toml::Table = block.parse().unwrap();
    let deps = block["dependencies"].as_array().unwrap().iter().map(|d| {
        // A marker is left out: uv writes it in an order of its own.
        let d = squeezed(d.as_str().unwrap());
        let d = d.split(';').next().unwrap().to_owned();
        match d.split_once('[') {
            Some((name, rest)) => {
                let (extras, spec) = rest.split_once(']').unwrap();
                requirement(name, extras.split(',').collect(), spec)
            }
            None => d,
        }
    });
    (block["requires-python"].as_str().unwrap().to_owned(), deps.collect())
}

/// A requirement as the two helpers compare it: its name, its extras in
/// order (none: no brackets) and its version.
fn requirement(name: &str, mut extras: Vec<&str>, spec: &str) -> String {
    extras.sort_unstable();
    let extras = if extras.is_empty() { String::new() } else { format!("[{}]", extras.join(",")) };
    squeezed(&format!("{name}{extras}{spec}"))
}

/// The Python and the dependencies the uv lock `lock` was made for, as
/// [`script_wants`] gives them: a name, its extras and its version; a
/// marker isn't compared.
fn lock_has(lock: &str) -> (String, BTreeSet<String>) {
    let lock: toml::Table = lock.parse().unwrap();
    let requirements = lock.get("manifest").and_then(|m| m.get("requirements"));
    let deps = requirements.map_or(&[][..], |r| r.as_array().unwrap()).iter().map(|r| {
        let extras: Vec<&str> =
            r.get("extras").map_or(Vec::new(), |e| e.as_array().unwrap().iter().map(|x| x.as_str().unwrap()).collect());
        requirement(r["name"].as_str().unwrap(), extras, r.get("specifier").map_or("", |s| s.as_str().unwrap()))
    });
    (lock["requires-python"].as_str().unwrap().to_owned(), deps.collect())
}

/// `requirement` without spaces, in lowercase.
fn squeezed(requirement: &str) -> String {
    requirement.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_lowercase()
}

/// The folder `AUTOSCENE_WASM_EXAMPLES` names: the examples, with each
/// WebAssembly one's component built into its `plugin/plugin.wasm` (the CI
/// job `wasm-examples` builds them). None when it isn't set.
fn built_wasm_examples() -> Option<PathBuf> {
    std::env::var_os("AUTOSCENE_WASM_EXAMPLES").filter(|v| !v.is_empty()).map(PathBuf::from)
}

#[test]
fn the_example_plugins_pack_and_parse() {
    let tmp = tempfile::tempdir().unwrap();
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/plugins");
    let built = built_wasm_examples();
    let app = Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
    // Each is checked by how it runs: an example added here is added below.
    let runtimes = [
        ("bright-shots", RuntimeKind::Process),
        ("clip-list-py", RuntimeKind::Wasm),
        ("discord-notify", RuntimeKind::Process),
        ("discord-notify-wasm", RuntimeKind::Wasm),
        ("hello-export", RuntimeKind::Process),
        ("hello-page", RuntimeKind::None),
        ("hello-service", RuntimeKind::Service),
    ];
    assert_eq!(names(&examples), runtimes.map(|(name, _)| name));
    for (name, runtime) in runtimes {
        // A WebAssembly example keeps its source beside the plugin folder
        // AutoScene loads, `plugin/`, which holds its component once built.
        let folder = match (runtime, &built) {
            (RuntimeKind::Wasm, Some(built)) => built.join(name).join("plugin"),
            (RuntimeKind::Wasm, None) => {
                // Not built here: its plugin.json, as AutoScene reads it.
                let file = examples.join(name).join("plugin/plugin.json");
                let parsed = manifest::parse(&fs::read(file).unwrap(), &app).unwrap();
                assert!(parsed.warnings.is_empty(), "{name}: {:?}", parsed.warnings);
                let m = parsed.manifest;
                assert_eq!((m.id.as_str(), m.runtime.kind), (format!("dev.autoscene.{name}").as_str(), runtime));
                assert_eq!((m.manifest_version, m.engines.protocol), (2, 2), "{name}");
                // Its component imports the world 1.1, which AutoScene
                // offers from 1.2.0 on: from that version, its engines say
                // so, as asp init writes them. Before, they can't, as this
                // AutoScene would refuse it as too new.
                if (app.major, app.minor) >= (1, 2) {
                    let older = Version::new(1, 1, 99);
                    assert!(m.requirement_met(&older).is_err(), "{name}: engines.autoscene must start at 1.2.0");
                }
                continue;
            }
            _ => examples.join(name),
        };
        // docs/PLUGINS.md packs them as they are: without a warning.
        let out = tmp.path().join(format!("{name}.asp"));
        assert_eq!(asp(&["pack", text(&folder), text(&out)]).warnings(), "", "{name}");
        let into = tmp.path().join(name);
        let lock = package::unpack(&out, &into).unwrap().lock;
        let parsed = manifest::parse(&fs::read(into.join("plugin.json")).unwrap(), &app).unwrap();
        assert!(parsed.warnings.is_empty(), "{name}: {:?}", parsed.warnings);
        let m = parsed.manifest;
        assert_eq!((m.id.as_str(), m.runtime.kind), (format!("dev.autoscene.{name}").as_str(), runtime));
        // hello-export stays the version 1 sample; the others show what
        // version 2 and its calls add.
        let versions = if name == "hello-export" { (1, 1) } else { (2, 2) };
        assert_eq!((m.manifest_version, m.engines.protocol), versions, "{name}");
        // What each shows of version 2 is still in its plugin.json.
        match name {
            "bright-shots" => {
                assert_eq!(m.shot_filters[0].settings, ["darkest"]);
                assert_eq!(m.pack_actions[0].file.name, "{character}-cut-sheet.csv");
            }
            "discord-notify" => {
                assert!(m.export_actions[0].after_failed);
                assert_eq!(m.events[0].id, "analysis-done");
            }
            _ => {}
        }

        match runtime {
            // A uv script with its lock, which starts the same way everywhere.
            RuntimeKind::Process | RuntimeKind::Service => {
                // Its lock fits the script as it is now: uv refuses every
                // run of a script whose block changed since it was locked.
                let script = fs::read_to_string(into.join("main.py")).unwrap();
                let uv_lock = fs::read_to_string(into.join("main.py.lock")).unwrap();
                assert_eq!(lock_has(&uv_lock), script_wants(&script), "{name}: lock it again");
                assert_eq!(m.launches().len(), 6, "{name}");
                for (platform, launch) in m.launches() {
                    let launch = launch.unwrap();
                    assert_eq!(launch, Launch::UvScript { script: "main.py".into() }, "{name} on {platform}");
                    package::check_launch(&lock, &launch, platform).unwrap();
                }
            }
            // Its pages, and nothing that runs.
            RuntimeKind::None => {
                assert_eq!(m.launches().len(), 0, "{name}");
                assert!(!into.join("main.py").exists(), "{name}");
                assert!(!m.pages.is_empty(), "{name}");
            }
            // One component, the same on every platform, never marked to run.
            RuntimeKind::Wasm => {
                assert_eq!(m.launches().len(), 6, "{name}");
                for (platform, launch) in m.launches() {
                    let launch = launch.unwrap();
                    assert_eq!(launch, Launch::Wasm { component: "plugin.wasm".into() }, "{name} on {platform}");
                    package::check_launch(&lock, &launch, platform).unwrap();
                }
                let listed: Vec<_> = lock.files.iter().map(|f| (f.path.as_str(), f.exec)).collect();
                assert_eq!(listed, [("plugin.json", false), ("plugin.wasm", false)], "{name}");
            }
        }
        // Each page is a file of the package, as AutoScene checks it.
        package::check_pages(&into, Some(&lock), &m.pages).unwrap();
        let sandboxed = if m.is_sandboxed() { "Runs in a sandbox.\n" } else { "" };
        assert_eq!(asp(&["verify", text(&out)]).ok(), format!("{} {}: Unreviewed\n{sandboxed}", m.id, m.version));
    }
}

/// AutoScene's own plugin Reel In (`plugins/reel-in`) packs as it is, with
/// no warning, and reads as its engines say: made for the AutoScene that
/// has sources, so it is read for that one, not this crate's version.
#[test]
fn reel_in_packs_and_parses() {
    let tmp = tempfile::tempdir().unwrap();
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/reel-in");
    let out = tmp.path().join("reel-in.asp");
    // Its one word: this crate, an AutoScene before sources, won't install it.
    let here = env!("CARGO_PKG_VERSION");
    let warned = asp(&["pack", text(&folder), text(&out)]).warnings();
    if Version::parse(here).unwrap() < Version::new(1, 0, 6) {
        let older = format!("warning: AutoScene {here} won't install it: its engines.autoscene is \">=1.0.6, <2\".
");
        assert_eq!(warned, older);
    } else {
        assert_eq!(warned, "");
    }
    let into = tmp.path().join("reel-in");
    let lock = package::unpack(&out, &into).unwrap().lock;
    let parsed = manifest::parse_for_any_app(&fs::read(into.join("plugin.json")).unwrap()).unwrap();
    assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    let m = parsed.manifest;
    assert_eq!((m.id.as_str(), m.runtime.kind), ("dev.autoscene.reel-in", RuntimeKind::Process));
    assert_eq!((m.manifest_version, m.engines.protocol), (2, 2));
    // It needs the AutoScene with sources: one without them would skip its source.
    let first = Version::parse("1.0.6").unwrap();
    assert!(m.requirement_met(&first).is_ok());
    assert!(m.requirement_met(&Version::parse("1.0.5").unwrap()).is_err());
    assert_eq!(m.sources[0].link_hosts, ["youtube.com", "youtu.be", "tiktok.com"]);
    assert_eq!(m.sources[0].settings, ["quality"]);
    // A uv script with its lock, on every platform.
    let script = fs::read_to_string(into.join("main.py")).unwrap();
    let uv_lock = fs::read_to_string(into.join("main.py.lock")).unwrap();
    assert_eq!(lock_has(&uv_lock), script_wants(&script), "lock it again");
    assert_eq!(m.launches().len(), 6);
    for (platform, launch) in m.launches() {
        let launch = launch.unwrap();
        assert_eq!(launch, Launch::UvScript { script: "main.py".into() }, "on {platform}");
        package::check_launch(&lock, &launch, platform).unwrap();
    }
    assert_eq!(asp(&["verify", text(&out)]).ok(), "dev.autoscene.reel-in 1.0.0: Unreviewed\n");
}

/// The WebAssembly examples are built against the world this AutoScene
/// serves: each `wit/plugin.wit` is the crate's, byte for byte.
#[test]
fn the_examples_carry_the_current_world() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let world = fs::read(crate_dir.join("wit/plugin.wit")).unwrap();
    let examples = crate_dir.join("../../examples/plugins");
    let with_world: Vec<String> =
        names(&examples).into_iter().filter(|name| examples.join(name).join("wit").exists()).collect();
    assert_eq!(with_world, ["clip-list-py", "discord-notify-wasm"]);
    for name in with_world {
        let copy = fs::read(examples.join(&name).join("wit/plugin.wit")).unwrap();
        assert!(copy == world, "{name}: copy crates/as-plugin/wit/plugin.wit to its wit/ again");
    }
}

/// What asp init writes for the plugin's editor, which no package holds.
const EDITOR_FILES: [&str; 2] = [".autoscene/plugin.schema.json", ".vscode/settings.json"];

#[test]
fn init_makes_a_plugin_that_packs_and_parses() {
    let tmp = tempfile::tempdir().unwrap();
    let place = |name: &str| tmp.path().join(name);
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let app = Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
    // The SDK's types, as `npm run build:kit` wrote them.
    let types: Vec<String> =
        names(&crate_dir.join("templates/types")).iter().map(|n| format!(".autoscene/types/{n}")).collect();
    let page = ["ui/app.js", "ui/index.html"];
    let program = ["autoscene.py", "main.py", "main.py.lock"];
    let forms = [
        (None, "My Page", "a page", RuntimeKind::None, page.to_vec()),
        (Some("--service"), "words", "a page and its service", RuntimeKind::Service, [&page[..], &program].concat()),
        (Some("--action"), "clips", "an export action", RuntimeKind::Process, program.to_vec()),
    ];
    for (form, folder, made, runtime, own) in forms {
        let dir = place(folder);
        let id = format!("dev.example.{}", folder.to_lowercase().replace(' ', "-"));
        let mut args = vec!["init", text(&dir)];
        args.extend(form);
        let said = asp(&args).ok();
        assert_eq!(
            said,
            format!("Made {folder} ({id}) in {}: {made}. Its README.md says what comes next.\n", dir.display())
        );

        // Its files, and the editor's: the schema and, for a page, the
        // SDK's types, as asp has them, and VS Code told where the schema is.
        let mut packs: Vec<String> = ["README.md", "plugin.json"].iter().chain(&own).map(|p| p.to_string()).collect();
        packs.sort();
        let mut files: Vec<String> = packs.iter().cloned().chain(EDITOR_FILES.map(String::from)).collect();
        if runtime != RuntimeKind::Process {
            files.extend(types.iter().cloned());
        }
        files.sort();
        assert_eq!(files_in(&dir), files, "{folder}");
        let same =
            |file: &str, original: PathBuf| assert_eq!(fs::read(dir.join(file)).unwrap(), fs::read(original).unwrap());
        same(".autoscene/plugin.schema.json", crate_dir.join("schema/plugin.schema.json"));
        for file in files.iter().filter(|f| f.starts_with(".autoscene/types/")) {
            same(file, crate_dir.join("templates/types").join(file.rsplit('/').next().unwrap()));
        }
        let vscode: Value = serde_json::from_slice(&fs::read(dir.join(".vscode/settings.json")).unwrap()).unwrap();
        assert_eq!(
            vscode,
            json!({"json.schemas": [{"fileMatch": ["/plugin.json"], "url": "./.autoscene/plugin.schema.json"}]})
        );
        // Each template filled in: no marker or placeholder is left.
        for file in &packs {
            let filled = fs::read_to_string(dir.join(file)).unwrap();
            for left in ["{{#", "{{^", "{{/", "{{id}}", "{{name}}", "{{autoscene}}", "{{dir}}", "{{package}}"] {
                assert!(!filled.contains(left), "{folder}/{file} has {left}:\n{filled}");
            }
        }
        // Each says what its form does, and only that.
        let read = |file: &str| fs::read_to_string(dir.join(file)).unwrap_or_default();
        let (readme, html, js) = (read("README.md"), read("ui/index.html"), read("ui/app.js"));
        let says = (
            html.matches("<textarea").count(),
            js.contains(r#"page.state.set("note""#),
            js.contains(r#"page.service.request("count""#),
            readme.contains("Open its page from the rail."),
            readme.contains("`main.py` is its service."),
            readme.contains("`main.py` is its program."),
        );
        let says_for = match runtime {
            RuntimeKind::None => (1, true, false, true, false, false),
            RuntimeKind::Service => (1, false, true, true, true, false),
            RuntimeKind::Process => (0, false, false, false, false, true),
            // init --wasm has tests of its own.
            RuntimeKind::Wasm => unreachable!("{folder} is a WebAssembly plugin"),
        };
        assert_eq!(says, says_for, "{folder}");
        let pack = if folder.contains(' ') {
            format!(r#""{folder}" "{folder}.asp""#)
        } else {
            format!("{folder} {folder}.asp")
        };
        assert!(readme.contains(&format!("`asp pack {pack}`")), "{readme}");

        // AutoScene reads it as it is, without a word: a page in the rail,
        // a service with its page, or a program with an export action.
        let parsed = manifest::parse(&fs::read(dir.join("plugin.json")).unwrap(), &app).unwrap();
        assert!(parsed.warnings.is_empty(), "{folder}: {:?}", parsed.warnings);
        let m = parsed.manifest;
        assert_eq!((m.id.as_str(), m.name.as_str(), m.version.as_str()), (id.as_str(), folder, "0.1.0"));
        assert_eq!((m.manifest_version, m.engines.protocol, m.runtime.kind), (2, 2, runtime));
        let pages: Vec<(&str, &str, bool)> =
            m.pages.iter().map(|p| (p.title.as_str(), p.entry.as_str(), p.rail)).collect();
        let actions: Vec<&str> = m.export_actions.iter().map(|a| a.title.as_str()).collect();
        if runtime == RuntimeKind::Process {
            assert_eq!((pages, actions), (vec![], vec!["Count the clips"]));
        } else {
            assert_eq!((pages, actions), (vec![(folder, "ui/index.html", true)], vec![]));
        }
        // Its program is a uv script, and its lock fits the script.
        for (platform, launch) in m.launches() {
            assert_eq!(launch.unwrap(), Launch::UvScript { script: "main.py".into() }, "{folder} on {platform}");
        }
        if runtime != RuntimeKind::None {
            let script = fs::read_to_string(dir.join("main.py")).unwrap();
            assert_eq!(lock_has(&fs::read_to_string(dir.join("main.py.lock")).unwrap()), script_wants(&script));
        }

        // It packs without a warning, and leaves the editor's files out.
        let out = place(&format!("{folder}.asp"));
        assert_eq!(asp(&["pack", text(&dir), text(&out)]).warnings(), "", "{folder}");
        assert_eq!(packed(&out), packs, "{folder}");
        assert_eq!(asp(&["verify", text(&out)]).ok(), format!("{id} 0.1.0: Unreviewed\n"));
    }

    // An id and a name of its own, written as each file writes text.
    let dir = place("mine");
    let name = r#"Say "hi" <now> & {{id}}"#;
    asp(&["init", text(&dir), "--service", "--id", "dev.me.words", "--name", name]).ok();
    let m = manifest::parse(&fs::read(dir.join("plugin.json")).unwrap(), &app).unwrap().manifest;
    assert_eq!((m.id.as_str(), m.name.as_str(), m.pages[0].title.as_str()), ("dev.me.words", name, name));
    let html = fs::read_to_string(dir.join("ui/index.html")).unwrap();
    assert!(html.contains(r#"<h1 class="as-display">Say &quot;hi&quot; &lt;now&gt; &amp; {{id}}</h1>"#), "{html}");
    let readme = fs::read_to_string(dir.join("README.md")).unwrap();
    assert!(readme.starts_with(&format!("# {name}\n\nAn AutoScene plugin, `dev.me.words`, made")), "{readme}");
}

#[test]
fn init_never_writes_into_a_folder_that_has_files() {
    let tmp = tempfile::tempdir().unwrap();
    let place = |name: &str| tmp.path().join(name);
    let start = "asp init makes a plugin in a new folder, or an empty one.";

    // A file in it, or only a folder, and no form writes there.
    let taken = place("taken");
    fs::create_dir(&taken).unwrap();
    fs::write(taken.join("notes.txt"), b"mine").unwrap();
    let nested = place("nested");
    fs::create_dir_all(nested.join(".vscode")).unwrap();
    for dir in [&taken, &nested] {
        for form in ["--page", "--service", "--action"] {
            let err = asp(&["init", text(dir), form]).failed(1);
            assert_eq!(err, format!("asp: {} has files in it. {start}\n", dir.display()));
        }
    }
    assert_eq!(names(&taken), ["notes.txt"]);
    assert_eq!(fs::read(taken.join("notes.txt")).unwrap(), b"mine");
    assert_eq!(files_in(&nested), Vec::<String>::new());
    let file = place("plugin.json");
    fs::write(&file, b"{}").unwrap();
    assert_eq!(asp(&["init", text(&file)]).failed(1), format!("asp: {} is a file. {start}\n", file.display()));
    assert_eq!(fs::read(&file).unwrap(), b"{}");

    // An id or a name AutoScene wouldn't take, or two forms: nothing is made.
    let new = place("new");
    let long = "n".repeat(41);
    let named = "asp: Give the plugin a name of 1 to 40 characters with --name.\n";
    let id = "asp: The plugin's id isn't valid. An id looks like dev.example.my-plugin: \
              2 to 5 parts of lowercase letters, digits and dashes, joined by dots.\n";
    for (args, code, err) in [
        (vec!["--id", "Not An Id"], 1, id),
        (vec!["--id", "nul.example"], 1, "asp: The plugin's id can't start with con, prn, aux, nul"),
        (vec!["--name", " "], 1, named),
        (vec!["--name", &long], 1, named),
        (vec!["--name", "Line\nbreak"], 1, named),
        (vec!["--page", "--action"], 2, "asp: choose one of --page, --service, --action and --wasm\n"),
    ] {
        let mut all = vec!["init", text(&new)];
        all.extend(&args);
        let said = asp(&all).failed(code);
        assert!(said.starts_with(err), "{args:?}: {said}");
        assert!(!new.exists(), "{args:?}");
    }
    // A folder whose name gives no id needs one.
    let unnamed = place("ÜÖ");
    let err = asp(&["init", text(&unnamed)]).failed(1);
    assert_eq!(err, "asp: Give the plugin an id with --id, such as dev.example.my-plugin.\n");
    assert!(!unnamed.exists());

    // An empty folder takes one.
    let empty = place("empty");
    fs::create_dir(&empty).unwrap();
    asp(&["init", text(&empty), "--action"]).ok();
    assert!(files_in(&empty).contains(&"main.py".to_owned()));
    assert_eq!(names(tmp.path()), ["empty", "nested", "plugin.json", "taken"]);
}

/// A SHA-256 of a large file.
const WEIGHTS_SHA: &str = "4b28e8dafc871cb41f00589e7df8455b244817a2fc3c67f3607ac3854ffc2a9d";
/// A revision of a Hugging Face repository.
const REV: &str = "0123456789abcdef0123456789abcdef01234567";

/// The plugin of `dir` as of manifest version 2, naming the large files `resources`.
fn with_resources(dir: &Path, resources: Value) {
    edit_manifest(dir, |v| {
        v["manifestVersion"] = json!(2);
        v["resources"] = resources;
    });
}

#[test]
fn pack_warns_of_a_branch_url() {
    let tmp = tempfile::tempdir().unwrap();
    let place = |name: &str| tmp.path().join(name);
    let src = uv_plugin(&place("hello"));
    let at = |url: &str| json!([{"name": "model.onnx", "urls": [url], "sha256": WEIGHTS_SHA, "size": 1000}]);
    // A file that follows a branch will stop matching its hash one day.
    with_resources(&src, at("https://huggingface.co/org/repo/resolve/main/model.onnx"));
    let warning = "warning: model.onnx follows a branch of Hugging Face: pin a revision, or the file will stop \
                   matching its hash.\n";
    let out = place("hello.asp");
    assert_eq!(asp(&["pack", text(&src), text(&out)]).warnings(), warning);
    let verified = asp(&["verify", text(&out)]);
    assert_eq!(verified.err, warning);
    assert_eq!(verified.ok(), "dev.autoscene.hello-export 1.2.0: Unreviewed\n");
    // A revision: not a word.
    with_resources(&src, at(&format!("https://huggingface.co/org/repo/resolve/{REV}/model.onnx")));
    assert_eq!(asp(&["pack", text(&src), text(&place("pinned.asp"))]).warnings(), "");
    // An address AutoScene would never fetch from fails the pack.
    with_resources(&src, at("http://huggingface.co/org/repo/resolve/main/model.onnx"));
    let run = asp(&["pack", text(&src), text(&place("plain.asp"))]);
    assert_ne!(run.code, Some(0));
    assert!(run.err.contains("isn't a plain https:// address of a host on the internet"), "{}", run.err);
    assert!(!place("plain.asp").exists());
}

#[test]
fn catalog_add_counts_resource_bytes_and_hosts() {
    let tmp = tempfile::tempdir().unwrap();
    let place = |name: &str| tmp.path().join(name);
    let src = uv_plugin(&place("hello"));
    let model = format!("https://huggingface.co/org/repo/resolve/{REV}/model.onnx");
    let mirror = "https://github.com/org/repo/releases/download/v1/model.onnx";
    let vocab = format!("https://HuggingFace.co/org/repo/resolve/{REV}/vocab.json");
    with_resources(
        &src,
        json!([
            {"name": "model.onnx", "urls": [model, mirror], "sha256": WEIGHTS_SHA, "size": 300_000_000u64},
            {"name": "vocab.json", "urls": [vocab], "sha256": WEIGHTS_SHA, "size": 12_000_000u64}
        ]),
    );
    let package = place("hello.asp");
    asp(&["pack", text(&src), text(&package)]).ok();
    let cat = CiCatalog::new(&place("site"), &place("online"));
    cat.add(&package).ok();
    // What its first run downloads, and from where: each host once, lowercase.
    let entry = &cat.json()["plugins"][0];
    assert_eq!(entry["resourceBytes"], json!(312_000_000u64));
    assert_eq!(entry["resourceHosts"], json!(["huggingface.co", "github.com"]));
    // AutoScene reads them back.
    cat.publish();
    cat.verify(&[]).ok();
    let index: catalog::Index = serde_json::from_value(cat.json()).unwrap();
    let e = &index.plugins[0];
    assert_eq!(
        (e.resource_bytes, e.resource_hosts.clone()),
        (312_000_000, vec!["huggingface.co".into(), "github.com".into()])
    );
}

/// Every key of the JSON `example` is one `read` has too, at the same place:
/// the docs spell no field AutoScene skips.
fn keys_read(example: &Value, read: &Value, at: &str) {
    match (example, read) {
        (Value::Object(ex), Value::Object(rd)) => {
            for (key, v) in ex {
                let Some(r) = rd.get(key) else { panic!("{at}.{key} isn't a field AutoScene reads") };
                keys_read(v, r, &format!("{at}.{key}"));
            }
        }
        (Value::Array(ex), Value::Array(rd)) => {
            assert_eq!(ex.len(), rd.len(), "{at}");
            for (i, (e, r)) in ex.iter().zip(rd).enumerate() {
                keys_read(e, r, &format!("{at}[{i}]"));
            }
        }
        _ => {}
    }
}

#[test]
fn the_catalog_example_in_the_docs_checks_out() {
    let doc = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/CATALOG.md");
    let doc = fs::read_to_string(&doc).unwrap().replace("\r\n", "\n");
    let (_, rest) = doc.split_once("<!-- index-example -->\n").expect("CATALOG.md marks its example");
    let (block, _) = rest.split_once("<!-- /index-example -->").expect("the example's end is marked");
    let json = block.trim().strip_prefix("```json\n").and_then(|b| b.strip_suffix("```")).expect("a json block");

    // It reads as an index, with no field AutoScene skips.
    let example: Value = serde_json::from_str(json).unwrap();
    let index: catalog::Index = serde_json::from_str(json).unwrap();
    keys_read(&example, &serde_json::to_value(&index).unwrap(), "index");
    assert_eq!((index.catalog_version, index.epoch), (catalog::CATALOG_VERSION, 1));
    assert_eq!((index.plugins.len(), index.withdrawn.len()), (2, 1));

    // Its dates hold as printed: `bump` below dates it anew, so they are
    // checked here.
    let published = index.published;
    assert!(
        published < index.expires && index.expires - published <= catalog::MAX_LIFETIME,
        "the example lives too long"
    );
    for e in &index.plugins {
        assert!(e.approved <= published, "{} {} is approved after the index", e.id, e.version);
    }
    for w in &index.withdrawn {
        assert!(w.since <= published, "the withdrawal of {} is dated after the index", w.id);
    }

    // Dated and signed as the catalog's CI does, AutoScene takes it: each
    // entry and withdrawal keeps its other rules.
    let tmp = tempfile::tempdir().unwrap();
    let cat = CiCatalog::new(&tmp.path().join("site"), &tmp.path().join("online"));
    write_json(&cat.index, &example);
    cat.publish();
    let verified = cat.verify(&[]).ok();
    assert!(verified.starts_with(&format!("{}: epoch 1, number 8, ", cat.index.display())), "{verified}");
    assert!(verified.ends_with("\n2 plugin versions and 1 withdrawal.\n"), "{verified}");
}

/// The test guest (`cargo xtask wasm-guests`); none (the test skips) when
/// it wasn't built.
fn guest() -> Option<Vec<u8>> {
    as_testkit::fixture_file("wasm/guest.wasm").map(|path| fs::read(path).unwrap())
}

/// A component written as text.
fn component(wat: &str) -> Vec<u8> {
    wat::parse_str(wat).unwrap()
}

/// A WebAssembly plugin folder `dir` with `component` as its plugin.wasm
/// and a command; `edit` changes its plugin.json first.
fn wasm_plugin(dir: &Path, component: &[u8], edit: impl FnOnce(&mut Value)) -> PathBuf {
    let mut manifest = json!({
        "manifestVersion": 2,
        "id": "dev.autoscene.hello-wasm",
        "version": "1.0.0",
        "name": "Hello Wasm",
        "description": "Says hello from the sandbox.",
        "author": {"name": "AutoScene"},
        "engines": {"autoscene": ">=1.0.0, <2", "protocol": 2},
        "runtime": {"type": "wasm", "component": "plugin.wasm"},
        "contributes": {"commands": [{"id": "hello", "title": "Say hello"}]}
    });
    edit(&mut manifest);
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join("plugin.json"), serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    fs::write(dir.join("plugin.wasm"), component).unwrap();
    dir.to_path_buf()
}

/// Checks what `asp init --wasm` wrote in `dir`: `own` and the editor's
/// files, each template filled in, the crate's `wit/plugin.wit` and schema,
/// and a `plugin/plugin.json` AutoScene reads without a word: a WebAssembly
/// plugin `id` named `name`, with a command. Without its component it
/// neither packs nor checks, and says why. Gives its README.
fn wasm_init_parses(dir: &Path, own: &[&str], id: &str, name: &str) -> String {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files: Vec<String> = own.iter().map(|f| f.to_string()).chain(EDITOR_FILES.map(String::from)).collect();
    files.sort();
    assert_eq!(files_in(dir), files);
    let same =
        |file: &str, original: PathBuf| assert_eq!(fs::read(dir.join(file)).unwrap(), fs::read(original).unwrap());
    same("wit/plugin.wit", crate_dir.join("wit/plugin.wit"));
    same(".autoscene/plugin.schema.json", crate_dir.join("schema/plugin.schema.json"));
    for file in own {
        let filled = fs::read_to_string(dir.join(file)).unwrap();
        assert!(!filled.contains("{{"), "{file} has a marker or placeholder left:\n{filled}");
    }
    let vscode: Value = serde_json::from_slice(&fs::read(dir.join(".vscode/settings.json")).unwrap()).unwrap();
    assert_eq!(vscode["json.schemas"][0]["fileMatch"], json!(["/plugin.json"]));

    let plugin = dir.join("plugin");
    let bytes = fs::read(plugin.join("plugin.json")).unwrap();
    // The schema it names is the one beside the source.
    let schema = serde_json::from_slice::<Value>(&bytes).unwrap()["$schema"].as_str().unwrap().to_owned();
    assert_eq!(fs::read(plugin.join(schema)).unwrap(), fs::read(crate_dir.join("schema/plugin.schema.json")).unwrap());
    let app = Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
    let parsed = manifest::parse(&bytes, &app).unwrap();
    assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    let m = parsed.manifest;
    assert_eq!((m.id.as_str(), m.name.as_str(), m.version.as_str()), (id, name, "0.1.0"));
    assert_eq!((m.manifest_version, m.engines.protocol, m.runtime.kind), (2, 2, RuntimeKind::Wasm));
    assert_eq!(m.runtime.component.as_deref(), Some("plugin.wasm"));
    let commands: Vec<(&str, &str)> = m.commands.iter().map(|c| (c.id.as_str(), c.title.as_str())).collect();
    assert_eq!(commands, [("hello", "Say hello")]);
    assert!(m.permissions.is_empty() && m.export_actions.is_empty() && m.pages.is_empty());
    // The component it builds stays out of version control.
    assert!(fs::read_to_string(plugin.join(".gitignore")).unwrap().lines().any(|l| l == "plugin.wasm"));

    let out = dir.join("plugin.asp");
    let err = asp(&["pack", text(&plugin), text(&out)]).failed(1);
    assert!(err.contains("The plugin package is missing plugin.wasm, which its runtime needs."), "{err}");
    assert!(!out.exists());
    let err = asp(&["check", text(&plugin)]).failed(1);
    let missing = plugin.join("plugin.wasm");
    assert_eq!(err, format!("asp: {} isn't there: build the plugin's component first.\n", missing.display()));
    // The folder asp init made isn't the plugin's: asp names the one that is.
    let word = |p: &Path| if text(p).contains(' ') { format!(r#""{}""#, p.display()) } else { text(p).to_owned() };
    let made =
        format!("asp: {} has no plugin.json. The plugin asp init made is in {}", dir.display(), plugin.display());
    let err = asp(&["check", text(dir)]).failed(1);
    assert_eq!(err, format!("{made}: asp check {}\n", word(&plugin)));
    let err = asp(&["pack", text(dir), text(&out)]).failed(1);
    assert_eq!(err, format!("{made}: asp pack {} {}\n", word(&plugin), word(&out)));
    assert!(!out.exists());
    fs::read_to_string(dir.join("README.md")).unwrap()
}

#[test]
fn init_writes_a_rust_plugin_that_parses() {
    let tmp = tempfile::tempdir().unwrap();
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let own = [
        ".gitignore",
        "Cargo.lock",
        "Cargo.toml",
        "README.md",
        "plugin/.gitignore",
        "plugin/plugin.json",
        "rust-toolchain.toml",
        "src/lib.rs",
        "wit/plugin.wit",
    ];
    // Its crate is named for the id's last part, with a word before a digit
    // a crate name can't start with.
    let named = ["--id", "dev.me.2d-tools", "--name", "2D Tools"];
    for (folder, args, id, name, krate) in [
        ("Hello Rust", &[][..], "dev.example.hello-rust", "Hello Rust", "hello-rust"),
        ("two", &named[..], "dev.me.2d-tools", "2D Tools", "plugin-2d-tools"),
        // Not a package its Cargo.lock holds: cargo build --locked would refuse that lock.
        ("log", &[][..], "dev.example.log", "log", "plugin-log"),
    ] {
        let dir = tmp.path().join(folder);
        let mut all = vec!["init", text(&dir), "--wasm", "rust"];
        all.extend(args);
        let said = asp(&all).ok();
        let made = "a WebAssembly plugin in Rust. Its README.md says what comes next.";
        assert_eq!(said, format!("Made {name} ({id}) in {}: {made}\n", dir.display()));
        let readme = wasm_init_parses(&dir, &own, id, name);
        let read = |file: &str| fs::read_to_string(dir.join(file)).unwrap();

        // A crate of its own, with the test guest's pin and release
        // profile (they move together), and a lock that fits it.
        let cargo: toml::Value = toml::from_str(&read("Cargo.toml")).unwrap();
        let guest: toml::Value =
            toml::from_str(&fs::read_to_string(crate_dir.join("tests/guest/Cargo.toml")).unwrap()).unwrap();
        assert_eq!(cargo["package"]["name"].as_str(), Some(krate));
        assert!(cargo.get("workspace").is_some(), "a workspace of its own");
        assert_eq!(cargo["lib"]["crate-type"], toml::Value::Array(vec!["cdylib".into()]));
        assert_eq!(cargo["dependencies"]["wit-bindgen"], guest["dependencies"]["wit-bindgen"]);
        assert_eq!(cargo["profile"], guest["profile"]);
        let lock: toml::Value = toml::from_str(&read("Cargo.lock")).unwrap();
        let packages = lock["package"].as_array().unwrap();
        let package = |name: &str| packages.iter().find(|p| p["name"].as_str() == Some(name)).unwrap();
        let mut named: Vec<&str> = packages.iter().map(|p| p["name"].as_str().unwrap()).collect();
        named.sort_unstable();
        let all = named.len();
        named.dedup();
        assert_eq!(named.len(), all, "a package name twice in its Cargo.lock");
        let deps = package(krate)["dependencies"].as_array().unwrap();
        let deps: Vec<&str> = deps.iter().map(|d| d.as_str().unwrap()).collect();
        let wants: Vec<&str> = cargo["dependencies"].as_table().unwrap().keys().map(String::as_str).collect();
        assert_eq!(deps, wants);
        let pin = cargo["dependencies"]["wit-bindgen"].as_str().unwrap().trim_start_matches('=');
        assert_eq!(package("wit-bindgen")["version"].as_str(), Some(pin));
        // A stable Rust that builds wasm32-wasip2 (1.82 or later).
        let toolchain: toml::Value = toml::from_str(&read("rust-toolchain.toml")).unwrap();
        let channel = Version::parse(toolchain["toolchain"]["channel"].as_str().unwrap()).unwrap();
        assert!(channel >= Version::new(1, 82, 0) && channel.pre.is_empty(), "{channel}");
        assert_eq!(toolchain["toolchain"]["targets"], toml::Value::Array(vec!["wasm32-wasip2".into()]));
        assert!(read("src/lib.rs").contains(r#"wit_bindgen::generate!({ world: "plugin", path: "wit" });"#));

        // The README builds, copies and packs this crate's component.
        let wasm = format!("target/wasm32-wasip2/release/{}.wasm", krate.replace('-', "_"));
        assert!(readme.starts_with(&format!("# {name}\n\nAn AutoScene plugin, `{id}`, made")), "{readme}");
        for step in [
            "`cargo build --release --target wasm32-wasip2`".to_owned(),
            format!("`Copy-Item {wasm} plugin/plugin.wasm`"),
            format!("`cp {wasm} plugin/plugin.wasm`"),
            "`asp check plugin`".to_owned(),
            format!(
                "`asp pack plugin {}`",
                if folder.contains(' ') { format!(r#""{folder}.asp""#) } else { format!("{folder}.asp") }
            ),
        ] {
            assert!(readme.contains(&step), "{step}\n{readme}");
        }
    }
}

#[test]
fn init_writes_a_python_plugin_that_parses() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("clip-words");
    let said = asp(&["init", text(&dir), "--wasm", "python"]).ok();
    let made = "a WebAssembly plugin in Python. Its README.md says what comes next.";
    assert_eq!(said, format!("Made clip-words (dev.example.clip-words) in {}: {made}\n", dir.display()));
    let own = [".gitignore", "README.md", "app.py", "plugin/.gitignore", "plugin/plugin.json", "wit/plugin.wit"];
    let readme = wasm_init_parses(&dir, &own, "dev.example.clip-words", "clip-words");
    let app = fs::read_to_string(dir.join("app.py")).unwrap();
    assert!(app.contains("class WitWorld(wit_world.WitWorld):\n    def run(self, request"), "{app}");
    assert!(app.contains(r#"if request.action.id == "hello":"#), "{app}");
    for step in [
        "`uv tool run --from componentize-py==0.25.1 componentize-py -d wit -w plugin componentize app -o plugin/plugin.wasm`",
        "A Python plugin can't use the network in this version of AutoScene.",
        "`asp check plugin`",
        "`asp pack plugin clip-words.asp`",
    ] {
        assert!(readme.contains(step), "{step}\n{readme}");
    }
}

#[test]
fn init_refuses_a_folder_that_is_not_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let place = |name: &str| tmp.path().join(name);
    let taken = place("taken");
    fs::create_dir(&taken).unwrap();
    fs::write(taken.join("notes.txt"), b"mine").unwrap();
    let start = "asp init makes a plugin in a new folder, or an empty one.";
    for language in ["rust", "python"] {
        let err = asp(&["init", text(&taken), "--wasm", language]).failed(1);
        assert_eq!(err, format!("asp: {} has files in it. {start}\n", taken.display()));
    }
    assert_eq!(files_in(&taken), ["notes.txt"]);
    assert_eq!(fs::read(taken.join("notes.txt")).unwrap(), b"mine");

    // A language asp has no template for, or two forms: nothing is made.
    let new = place("new");
    let two = "asp: choose one of --page, --service, --action and --wasm\n";
    let language = "asp: --wasm takes rust or python\n";
    for (args, err) in [
        (&["--wasm", "go"][..], language),
        (&["--wasm", "Rust"], language),
        (&["--wasm"], "asp: --wasm needs a value\n"),
        (&["--wasm", "rust", "--service"], two),
        (&["--wasm", "rust", "--wasm", "python"], two),
    ] {
        let mut all = vec!["init", text(&new)];
        all.extend(args);
        let said = asp(&all).failed(2);
        assert!(said.starts_with(err), "{args:?}: {said}");
        assert!(!new.exists(), "{args:?}");
    }

    // An empty folder takes one.
    let empty = place("empty");
    fs::create_dir(&empty).unwrap();
    asp(&["init", text(&empty), "--wasm", "python"]).ok();
    assert!(files_in(&empty).contains(&"app.py".to_owned()));
    assert_eq!(names(tmp.path()), ["empty", "taken"]);
}

/// What asp says of a component built against a newer world.
const NEWER_WORLD: &str = "asp: autoscene:plugin/host@1.2.0 needs a newer AutoScene: this one offers \
                           autoscene:plugin up to 1.1. Build it against the wit/plugin.wit that asp init \
                           writes, or wait for a newer AutoScene.\n";
/// What asp says of a component built with a newer WASI.
const NEWER_WASI: &str = "asp: wasi:cli/environment@0.2.19 needs a newer AutoScene: this one offers WASI up \
                          to 0.2.12. Build it with an older toolchain, or wait for a newer AutoScene.\n";

/// A package `out` made by hand of the plugin folder `dir`, with the
/// files.lock `edit` makes of the folder's: what someone other than asp
/// may pack, which asp pack would refuse.
fn forged(dir: &Path, out: &Path, edit: impl FnOnce(&mut as_plugin::FilesLock)) {
    let mut lock = package::lock_folder(dir).unwrap();
    edit(&mut lock);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(fs::File::create(out).unwrap());
    for f in &lock.files {
        zip.start_file(f.path.as_str(), stored).unwrap();
        zip.write_all(&fs::read(dir.join(&f.path)).unwrap()).unwrap();
    }
    zip.start_file(LOCK, stored).unwrap();
    zip.write_all(&serde_json::to_vec_pretty(&lock).unwrap()).unwrap();
    zip.finish().unwrap();
}

#[test]
fn verify_and_check_refuse_a_package_autoscene_would_refuse() {
    let tmp = tempfile::tempdir().unwrap();
    let place = |name: &str| tmp.path().join(name);
    let package = place("forged.asp");
    // A component AutoScene won't run, which asp pack would refuse.
    for (wat, said) in [
        ("(component)", "asp: The plugin's component doesn't fit this AutoScene: "),
        (
            r#"(component (import "example:thing/api@1.0.0" (instance)))"#,
            "asp: The plugin uses example:thing/api@1.0.0, which AutoScene doesn't offer.\n",
        ),
        (r#"(component (import "autoscene:plugin/host@1.2.0" (instance)))"#, NEWER_WORLD),
        (r#"(component (import "wasi:cli/environment@0.2.19" (instance)))"#, NEWER_WASI),
    ] {
        let dir = wasm_plugin(&place("wrong"), &component(wat), |_| {});
        forged(&dir, &package, |_| {});
        for command in ["verify", "check"] {
            let err = asp(&[command, text(&package)]).failed(1);
            assert!(err.starts_with(said), "{command} {wat}: {err}");
        }
    }

    // A package is checked as AutoScene installs it: a component marked to
    // run, or one its files.lock doesn't name, is refused by both alike.
    let dir = wasm_plugin(&place("hello"), &component("(component)"), |_| {});
    let marked = "The plugin's files.lock marks plugin.wasm to run. A component isn't a program: remove its exec flag.";
    let missing = "The plugin package is missing plugin.wasm, which its runtime needs.";
    for (mark, why) in [(true, marked), (false, missing)] {
        forged(&dir, &package, |lock| {
            if mark {
                lock.files.iter_mut().filter(|f| f.path == "plugin.wasm").for_each(|f| f.exec = true);
            } else {
                lock.files.retain(|f| f.path != "plugin.wasm");
            }
        });
        for command in ["verify", "check"] {
            let err = asp(&[command, text(&package)]).failed(1);
            // Said once, for every platform.
            let (warning, failure) = err.split_once('\n').unwrap();
            assert!(
                warning.starts_with("warning: It won't run on ") && warning.ends_with(&format!(": {why}")),
                "{err}"
            );
            assert!(warning.contains(&format!("{} (this computer)", current_platform())), "{err}");
            assert_eq!(failure, "asp: The plugin runs on none of its platforms.\n", "{command}");
        }
    }
}

#[test]
fn check_lists_imports_and_refuses_unknown_ones() {
    let Some(guest) = guest() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let place = |name: &str| tmp.path().join(name);

    // A component on its own: its world, its size and what it imports,
    // sorted, with the permission an import needs. Without a manifest,
    // there is nothing to compare: no warning.
    let file = place("guest.wasm");
    fs::write(&file, &guest).unwrap();
    let bare = asp(&["check", text(&file)]);
    assert_eq!((bare.code, bare.err.as_str()), (Some(0), ""));
    let kb = guest.len().div_ceil(1024);
    let head = format!("guest.wasm: a WebAssembly component of autoscene:plugin@1.1 ({kb} KB)\nimports:\n");
    assert!(bare.out.starts_with(&head), "{}", bare.out);
    let imports: Vec<&str> = bare.out.lines().skip(2).collect();
    for line in [
        "  autoscene:plugin/host@1.1.0",
        "  wasi:filesystem/preopens@0.2.12",
        r#"  wasi:http/outgoing-handler@0.2.12   needs "network""#,
        r#"  wasi:http/types@0.2.12              needs "network""#,
    ] {
        assert!(imports.contains(&line), "{line}\n{}", bare.out);
    }
    let mut sorted = imports.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(imports, sorted);

    // In a plugin's folder and in its package, against its plugin.json,
    // which asks for the network it uses: the same, without a word.
    let dir = wasm_plugin(&place("hello"), &guest, |m| {
        m["permissions"] = json!(["network"]);
        m["network"] = json!({"hosts": ["discord.com"]});
    });
    let package = place("hello.asp");
    assert_eq!(asp(&["pack", text(&dir), text(&package)]).warnings(), "");
    for path in [&dir, &package] {
        let run = asp(&["check", text(path)]);
        assert_eq!((run.code, run.err.as_str()), (Some(0), ""), "{}", path.display());
        assert_eq!(run.out.replacen("plugin.wasm:", "guest.wasm:", 1), bare.out, "{}", path.display());
    }
    let verified = asp(&["verify", text(&package)]).ok();
    assert_eq!(verified, "dev.autoscene.hello-wasm 1.0.0: Unreviewed\nRuns in a sandbox.\n");

    // What AutoScene doesn't offer, or offers only in a newer version, is
    // refused alike on its own, in a folder and when it is packed.
    for (wat, said) in [
        (
            r#"(component (import "example:thing/api@1.0.0" (instance)))"#,
            "asp: The plugin uses example:thing/api@1.0.0, which AutoScene doesn't offer.\n",
        ),
        (r#"(component (import "autoscene:plugin/host@1.2.0" (instance)))"#, NEWER_WORLD),
        (r#"(component (import "wasi:cli/environment@0.2.19" (instance)))"#, NEWER_WASI),
    ] {
        let wrong = component(wat);
        let file = place("wrong.wasm");
        fs::write(&file, &wrong).unwrap();
        assert_eq!(asp(&["check", text(&file)]).failed(1), said, "{wat}");
        let dir = wasm_plugin(&place("wrong"), &wrong, |_| {});
        assert_eq!(asp(&["check", text(&dir)]).failed(1), said, "{wat}");
        let out = place("wrong.asp");
        assert_eq!(asp(&["pack", text(&dir), text(&out)]).failed(1), said, "{wat}");
        assert!(!out.exists(), "{wat}");
    }

    // A plugin that runs no component has nothing to check.
    let process = uv_plugin(&place("uv"));
    let err = asp(&["check", text(&process)]).failed(1);
    let not = "asp: Hello Export isn't a WebAssembly plugin: asp check checks a plugin whose runtime.type is wasm.\n";
    assert_eq!(err, not);
}

#[test]
fn check_warns_about_http_without_network() {
    let Some(guest) = guest() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let place = |name: &str| tmp.path().join(name);
    let warning = "warning: It imports wasi:http but doesn't ask for network: every request it makes is refused.\n";
    let dir = wasm_plugin(&place("hello"), &guest, |_| {});

    // check, pack and verify say it alike, and still go on.
    let checked = asp(&["check", text(&dir)]);
    assert_eq!((checked.code, checked.err.as_str()), (Some(0), warning));
    assert!(checked.out.contains(r#"  wasi:http/outgoing-handler@0.2.12   needs "network""#), "{}", checked.out);
    let package = place("hello.asp");
    assert_eq!(asp(&["pack", text(&dir), text(&package)]).warnings(), warning);
    let verified = asp(&["verify", text(&package)]);
    assert_eq!((verified.code, verified.err.as_str()), (Some(0), warning));
    assert_eq!(verified.out, "dev.autoscene.hello-wasm 1.0.0: Unreviewed\nRuns in a sandbox.\n");
    let checked = asp(&["check", text(&package)]);
    assert_eq!((checked.code, checked.err.as_str()), (Some(0), warning));

    // platforms only narrows where it installs: pack says so.
    edit_manifest(&dir, |m| m["platforms"] = json!([current_platform()]));
    let platforms = "warning: platforms limits where AutoScene installs it, \
                     though a WebAssembly plugin runs the same on every platform.\n";
    assert_eq!(asp(&["pack", text(&dir), text(&package)]).warnings(), format!("{platforms}{warning}"));
}

#[test]
fn events_without_on_event_are_refused_at_pack() {
    let Some(newest) = guest() else { return };
    let Some(frozen) = as_testkit::fixture_file("wasm/guest-frozen.wasm").map(|path| fs::read(path).unwrap()) else {
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let place = |name: &str| tmp.path().join(name);
    let waits = |m: &mut Value| {
        m["contributes"]["events"] = json!([{"id": "done", "title": "Tell", "on": "analysisFinished"}]);
    };
    let warning = "warning: It imports wasi:http but doesn't ask for network: every request it makes is refused.\n";

    // A component of 1.0.0 has no on-event: check and pack refuse a plugin
    // that waits for events with it, and pack writes nothing.
    let file = place("frozen.wasm");
    fs::write(&file, &frozen).unwrap();
    let alone = asp(&["check", text(&file)]);
    assert_eq!((alone.code, alone.err.as_str()), (Some(0), ""));
    assert!(alone.out.starts_with("frozen.wasm: a WebAssembly component of autoscene:plugin@1.0 ("), "{}", alone.out);
    let said = "asp: The plugin waits for events, but its component has no on-event: \
                build it against the wit/plugin.wit that asp init writes.\n";
    let dir = wasm_plugin(&place("frozen"), &frozen, waits);
    assert_eq!(asp(&["check", text(&dir)]).failed(1), said);
    let out = place("frozen.asp");
    assert_eq!(asp(&["pack", text(&dir), text(&out)]).failed(1), said);
    assert!(!out.exists());
    // Without events it is a plugin as before.
    let dir = wasm_plugin(&place("frozen-actions"), &frozen, |_| {});
    assert_eq!(asp(&["pack", text(&dir), text(&out)]).warnings(), warning);

    // One of 1.1.0 has it, and packs.
    let dir = wasm_plugin(&place("newest"), &newest, waits);
    let checked = asp(&["check", text(&dir)]);
    assert_eq!((checked.code, checked.err.as_str()), (Some(0), warning));
    assert!(
        checked.out.starts_with("plugin.wasm: a WebAssembly component of autoscene:plugin@1.1 ("),
        "{}",
        checked.out
    );
    let package = place("newest.asp");
    assert_eq!(asp(&["pack", text(&dir), text(&package)]).warnings(), warning);
    let verified = asp(&["verify", text(&package)]);
    assert_eq!((verified.code, verified.err.as_str()), (Some(0), warning));
}

#[test]
fn pack_refuses_precompiled_code() {
    let tmp = tempfile::tempdir().unwrap();
    let place = |name: &str| tmp.path().join(name);
    let dir = wasm_plugin(&place("hello"), &component("(component)"), |_| {});
    let out = place("hello.asp");
    fs::write(&out, b"the old package").unwrap();
    // AutoScene compiles a component itself, and loads no code a package
    // brings compiled: by any name in any case, at any depth.
    for file in ["plugin.cwasm", "code/Plugin.CWASM"] {
        let path = dir.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"compiled").unwrap();
        let err = asp(&["pack", text(&dir), text(&out)]).failed(1);
        let said = "Ship the .wasm file: AutoScene compiles it itself.";
        assert_eq!(err, format!("asp: The package holds precompiled code ({file}). {said}\n"));
        fs::remove_file(&path).unwrap();
    }
    assert_eq!(fs::read(&out).unwrap(), b"the old package");
}

#[test]
fn pack_refuses_a_component_that_does_not_load() {
    let tmp = tempfile::tempdir().unwrap();
    let place = |name: &str| tmp.path().join(name);
    let out = place("hello.asp");
    let not_one = "asp: The plugin's component isn't a WebAssembly component. \
                   Build it for wasm32-wasip2 or with componentize-py.";
    for (bytes, said) in [
        (component("(module)"), not_one),
        (b"\0asm\x0d\x00\x01\x00 cut short".to_vec(), "asp: The plugin's component doesn't load: "),
        (component("(component)"), "asp: The plugin's component doesn't fit this AutoScene: "),
    ] {
        let dir = wasm_plugin(&place("hello"), &bytes, |_| {});
        let err = asp(&["pack", text(&dir), text(&out)]).failed(1);
        assert!(err.starts_with(said), "{err}");
        assert!(!out.exists(), "{said}");
        let err = asp(&["check", text(&dir)]).failed(1);
        assert!(err.starts_with(said), "{err}");
    }
}

/// A manifest of a plugin that runs `plugin.wasm`, for [`Compiler`]; `events`
/// makes it wait for one.
fn compiled_manifest(events: bool) -> manifest::Manifest {
    let mut m = json!({
        "manifestVersion": 2, "id": "dev.autoscene.hello", "version": "1.0.0", "name": "Hello",
        "description": "Says hello.", "author": {"name": "AutoScene"},
        "engines": {"autoscene": ">=1.1.0", "protocol": 2},
        "runtime": {"type": "wasm", "component": "plugin.wasm"},
        "contributes": {"commands": [{"id": "hello", "title": "Hello"}]}
    });
    if events {
        m["contributes"]["events"] = json!([{"id": "done", "title": "Tell", "on": "analysisFinished"}]);
    }
    manifest::parse_for_any_app(m.to_string().as_bytes()).unwrap().manifest
}

fn compiler() -> Compiler {
    Compiler::new(PathBuf::from(env!("CARGO_BIN_EXE_asp")))
}

/// What `compiler` makes of the component at `file`, kept in `code`.
fn compile(
    compiler: &Compiler,
    file: &Path,
    sha256: Option<&str>,
    code: &Path,
    events: bool,
) -> Result<PathBuf, PluginError> {
    compiler.prepare(file, sha256, code, &compiled_manifest(events), &CancelToken::new())
}

fn kept_in(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir).map_or(Vec::new(), |entries| entries.map(|e| e.unwrap().path()).collect())
}

/// A component that keeps Cranelift busy for a long time, and takes more
/// memory the longer it does: its `functions` have `locals` locals, each
/// updated in each of `loops` nested loops.
fn slow_component(functions: usize, locals: usize, loops: usize) -> Vec<u8> {
    fn leb(mut n: usize) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let byte = (n & 0x7f) as u8;
            n >>= 7;
            if n == 0 {
                out.push(byte);
                return out;
            }
            out.push(byte | 0x80);
        }
    }
    let section = |id: u8, body: &[u8]| [&[id][..], &leb(body.len()), body].concat();
    let updates: Vec<u8> = (0..locals)
        // local.get i, i32.const 1, i32.add, local.set i
        .flat_map(|i| [&[0x20][..], &leb(i), &[0x41, 0x01, 0x6a, 0x21], &leb(i)].concat())
        .collect();
    let mut code = [&leb(1)[..], &leb(locals), &[0x7f]].concat();
    for _ in 0..loops {
        code.extend([0x03, 0x40]);
        code.extend(&updates);
    }
    for _ in 0..loops {
        // local.get 0, br_if 0, end
        code.extend([0x20, 0x00, 0x0d, 0x00, 0x0b]);
    }
    code.push(0x0b);
    let body = [&leb(code.len())[..], &code].concat();
    let module = [
        &b"\0asm\x01\x00\x00\x00"[..],
        &section(1, &[0x01, 0x60, 0x00, 0x00]),
        &section(3, &[&leb(functions)[..], &vec![0x00; functions]].concat()),
        &section(10, &[&leb(functions)[..], &body.repeat(functions)].concat()),
    ]
    .concat();
    // The module, and an instance of it: a component that uses it.
    [&b"\0asm\x0d\x00\x01\x00"[..], &section(1, &module), &section(2, &[0x01, 0x00, 0x00, 0x00])].concat()
}

/// A [`slow_component`] of 1 MB on disk, which takes seconds to compile: its
/// file and its SHA-256.
fn slow_file(dir: &Path) -> (PathBuf, String) {
    let (file, bytes) = (dir.join("slow.wasm"), slow_component(1, 3000, 40));
    fs::write(&file, &bytes).unwrap();
    (file, package::sha256_hex(&bytes))
}

/// The compile process as a [`Compiler`] starts it, and the pipe it watches.
fn compile_process(file: &Path, sha256: &str, code: &Path, limit_ms: &str) -> (Child, ChildStdin) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_asp"))
        .arg(wasm::COMPILE_ARG)
        .args([file, Path::new(sha256), code, Path::new("no-events"), Path::new(limit_ms)])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    (child, stdin)
}

/// When `child` ended, within `limit`: its exit code and what it answered.
fn ended(mut child: Child, limit: Duration) -> (Option<i32>, String) {
    let started = Instant::now();
    while child.try_wait().unwrap().is_none() {
        assert!(started.elapsed() < limit, "it still runs after {limit:?}");
        thread::sleep(Duration::from_millis(20));
    }
    let mut said = String::new();
    child.stdout.take().unwrap().read_to_string(&mut said).unwrap();
    (child.wait().unwrap().code(), said)
}

#[test]
fn a_component_compiles_in_a_process_of_its_own_as_it_does_here() {
    let Some(guest) = as_testkit::fixture_file("wasm/guest.wasm") else { return };
    let tmp = tempfile::tempdir().unwrap();
    let (there, here) = (tmp.path().join("there"), tmp.path().join("here"));
    let sha256 = package::sha256_hex(&fs::read(&guest).unwrap());
    for events in [false, true] {
        let file = compile(&compiler(), &guest, Some(&sha256), &there, events).unwrap();
        assert_eq!(kept_in(&there), std::slice::from_ref(&file));
        // The same file under the same name, so a run finds it.
        let (_, same) = wasm::prepare(&guest, Some(&sha256), &here, &compiled_manifest(events)).unwrap();
        assert_eq!(file.file_name(), same.file_name());
        assert_eq!(fs::read(&file).unwrap(), fs::read(&same).unwrap());
    }
    // A folder has no lock: its component is taken as it is now.
    assert!(compile(&compiler(), &guest, None, &there, false).is_ok());
}

#[test]
fn a_compile_process_refuses_what_a_compile_here_refuses() {
    let tmp = tempfile::tempdir().unwrap();
    let (file, code) = (tmp.path().join("plugin.wasm"), tmp.path().join("code"));
    let m = compiled_manifest(false);
    // An error repeats this name, and more than a pipe holds of it comes
    // back all the same, at once.
    let long = "a".repeat(90_000);
    for bytes in [
        component("(component)"),
        component(r#"(component (import "autoscene:plugin/host@1.2.0" (instance)))"#),
        component(r#"(component (import "example:thing/api@1.0.0" (instance)))"#),
        component(&format!(r#"(component (import "wasi:io/{long}@0.2.0" (instance (export "f" (func)))))"#)),
        b"\0asm\x0d\x00\x01\x00\x01\x02\xff\xff".to_vec(),
        b"\0asm\x01\x00\x00\x00".to_vec(),
    ] {
        fs::write(&file, &bytes).unwrap();
        let there = compile(&compiler(), &file, None, &code, false).unwrap_err();
        let here = wasm::prepare(&file, None, &code, &m).unwrap_err();
        assert_eq!(format!("{there:?}"), format!("{here:?}"));
        assert!(there.to_string().len() < 5000, "{} bytes", there.to_string().len());
        assert_eq!(kept_in(&code), Vec::<PathBuf>::new(), "a refused component leaves no file");
    }
    let changed = compile(&compiler(), &file, Some(&package::sha256_hex(b"other")), &code, false).unwrap_err();
    assert_eq!(changed.to_string(), "Its files were changed. Install it again.");
}

#[test]
fn a_compile_process_refuses_a_plugin_that_waits_for_events_and_can_not_hear_them() {
    let Some(frozen) = as_testkit::fixture_file("wasm/guest-frozen.wasm") else { return };
    let tmp = tempfile::tempdir().unwrap();
    let code = tmp.path().join("code");
    let e = compile(&compiler(), &frozen, None, &code, true).unwrap_err();
    assert!(e.to_string().starts_with("The plugin waits for events, but its component has no on-event"), "{e}");
    assert_eq!(kept_in(&code), Vec::<PathBuf>::new());
    assert!(compile(&compiler(), &frozen, None, &code, false).is_ok());
}

#[test]
fn the_compile_process_checks_the_component_itself() {
    let tmp = tempfile::tempdir().unwrap();
    let (file, code) = (tmp.path().join("plugin.wasm"), tmp.path().join("code"));
    fs::write(&file, component("(component)")).unwrap();
    // The file changed after the app read it.
    let (child, _alive) = compile_process(&file, &package::sha256_hex(b"other"), &code, "60000");
    let (status, said) = ended(child, Duration::from_secs(30));
    assert_eq!(status, Some(0));
    assert_eq!(
        said.trim_end(),
        r#"{"Refused":{"key":"plugin.filesChanged","args":{},"text":"Its files were changed. Install it again."}}"#
    );
    assert_eq!(kept_in(&code), Vec::<PathBuf>::new());
}

#[test]
fn a_compile_past_its_limit_is_ended_and_leaves_no_code() {
    let Some(guest) = as_testkit::fixture_file("wasm/guest.wasm") else { return };
    let tmp = tempfile::tempdir().unwrap();
    let code = tmp.path().join("code");
    let stopped = Compiler { limit: Duration::ZERO, ..compiler() };
    let started = Instant::now();
    let e = compile(&stopped, &guest, None, &code, false).unwrap_err();
    assert_eq!(e.to_string(), "This plugin took too long to get ready, so AutoScene stopped. Nothing changed.");
    assert_eq!(kept_in(&code), Vec::<PathBuf>::new());
    assert!(started.elapsed() < Duration::from_secs(30), "{:?}", started.elapsed());
}

#[test]
fn a_compile_is_ended_partway_at_its_limit() {
    let tmp = tempfile::tempdir().unwrap();
    let ((file, sha256), code) = (slow_file(tmp.path()), tmp.path().join("code"));
    let stopped = Compiler { limit: Duration::from_secs(1), ..compiler() };
    let started = Instant::now();
    let e = compile(&stopped, &file, Some(&sha256), &code, false).unwrap_err();
    assert_eq!(e.to_string(), "This plugin took too long to get ready, so AutoScene stopped. Nothing changed.");
    assert!(started.elapsed() < Duration::from_secs(8), "{:?}", started.elapsed());
    assert_eq!(kept_in(&code), Vec::<PathBuf>::new());
}

#[test]
fn a_cancel_ends_a_compile_partway() {
    let tmp = tempfile::tempdir().unwrap();
    let ((file, sha256), code) = (slow_file(tmp.path()), tmp.path().join("code"));
    let cancel = CancelToken::new();
    let later = cancel.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(500));
        later.cancel();
    });
    let started = Instant::now();
    let e = compiler().prepare(&file, Some(&sha256), &code, &compiled_manifest(false), &cancel).unwrap_err();
    assert!(matches!(e, PluginError::Cancelled), "{e:?}");
    assert!(started.elapsed() < Duration::from_secs(8), "{:?}", started.elapsed());
    assert_eq!(kept_in(&code), Vec::<PathBuf>::new());
}

// RLIMIT_AS may not hold on macOS: there this would run to its time limit.
#[cfg(not(target_os = "macos"))]
#[test]
fn a_compile_past_its_memory_is_ended_in_plain_words() {
    let tmp = tempfile::tempdir().unwrap();
    let (file, code) = (tmp.path().join("heavy.wasm"), tmp.path().join("code"));
    // 58 MB that asks for more than 4 GB, within seconds. On Unix the limit
    // counts address space, so it leaves room for the compiler's threads.
    let bytes = slow_component(9, 8000, 90);
    fs::write(&file, &bytes).unwrap();
    let small = Compiler { memory: 2 << 30, ..compiler() };
    let e = compile(&small, &file, Some(&package::sha256_hex(&bytes)), &code, false).unwrap_err();
    assert_eq!(e.to_string(), "This plugin took too much memory to get ready, so AutoScene stopped. Nothing changed.");
    assert_eq!(kept_in(&code), Vec::<PathBuf>::new());
}

#[test]
fn the_compile_process_ends_by_itself_at_its_limit() {
    let tmp = tempfile::tempdir().unwrap();
    let ((file, sha256), code) = (slow_file(tmp.path()), tmp.path().join("code"));
    // Nobody ends it: the app is gone, and its pipe stays open.
    let (child, _alive) = compile_process(&file, &sha256, &code, "1000");
    let (status, said) = ended(child, Duration::from_secs(8));
    assert_eq!((status, said.as_str()), (Some(3), ""));
    assert_eq!(kept_in(&code), Vec::<PathBuf>::new());
}

#[test]
fn the_compile_process_ends_with_whoever_started_it() {
    let tmp = tempfile::tempdir().unwrap();
    let ((file, sha256), code) = (slow_file(tmp.path()), tmp.path().join("code"));
    let (child, alive) = compile_process(&file, &sha256, &code, "120000");
    thread::sleep(Duration::from_millis(500));
    drop(alive);
    let (status, said) = ended(child, Duration::from_secs(8));
    assert_eq!((status, said.as_str()), (Some(3), ""));
    assert_eq!(kept_in(&code), Vec::<PathBuf>::new());
}

#[test]
fn a_compile_process_that_says_nothing_fails_in_plain_words() {
    let Some(guest) = as_testkit::fixture_file("wasm/guest.wasm") else { return };
    let tmp = tempfile::tempdir().unwrap();
    // A program that takes the arguments for something else, and ends.
    let other = Compiler::new(PathBuf::from(env!("CARGO_BIN_EXE_asp_test_plugin")));
    let e = compile(&other, &guest, None, tmp.path(), false).unwrap_err();
    assert_eq!(e.to_string(), "AutoScene couldn't get this plugin ready. Nothing changed.");

    let missing = Compiler::new(tmp.path().join("no-such-program"));
    assert!(compile(&missing, &guest, None, tmp.path(), false).is_err());
}

#[test]
fn the_compile_process_says_what_it_takes() {
    let err = asp(&[wasm::COMPILE_ARG, "one"]).failed(2);
    let says =
        "--compile-plugin takes a component, its SHA-256, a folder, events or no-events, and its time limit in ms\n";
    assert_eq!(err, says);
    assert_eq!(asp(&[wasm::COMPILE_ARG, "a", "b", "c", "events", "soon"]).failed(2), says);
}

//! The red team frames plugin pages as the app does. These keep it so: the
//! app's CSP around its harness, `frame.json`'s attributes on its frames,
//! and the app's Tauri under both.

use std::fs;
use std::path::Path;

use as_plugin::pages::SANDBOX;
use serde_json::Value;

/// The Tauri crates both name, which the workspace pins once.
const TAURI: [&str; 3] = ["tauri", "tauri-build", "tauri-runtime-wry"];

/// A file of the red team's, or beside it (`../src-tauri/…`).
fn read(path: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap()
}

fn json(path: &str) -> Value {
    serde_json::from_str(&read(path)).unwrap()
}

fn toml(path: &str) -> toml::Table {
    read(path).parse().unwrap()
}

#[test]
fn the_harness_uses_the_apps_csp() {
    let app = json("../src-tauri/tauri.conf.json");
    let harness = json("tauri.conf.json");
    let csp = &app["app"]["security"]["csp"];
    assert!(csp.as_str().is_some_and(|c| c.contains("frame-src asp: http://asp.localhost")), "{csp}");
    assert_eq!(harness["app"]["security"]["csp"], *csp);
    assert_eq!(harness["app"]["security"]["assetProtocol"], app["app"]["security"]["assetProtocol"]);
    // A debug build takes devCsp over csp, and with a devUrl embeds no CSP
    // at all (tauri-codegen): the harness has neither, so `cargo run`
    // frames pages inside the CSP of a built app.
    assert_eq!(harness["app"]["security"].get("devCsp"), None);
    assert_eq!(harness["build"].get("devUrl"), None);
    assert_eq!(harness["build"]["frontendDist"], "harness");
}

#[test]
fn the_harness_frames_like_the_app() {
    let frame = json("../src/plugin-kit/frame.json");
    let mut keys: Vec<&str> = frame.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["allow", "referrerPolicy", "sandbox"]);
    assert_eq!(frame["sandbox"], SANDBOX);
    // The red team hands frame.json to the harness as it is…
    let main = read("src/main.rs");
    assert!(main.contains(r#"include_str!("../../src/plugin-kit/frame.json")"#));
    // …which sets each attribute from it, and none of its own.
    let harness = read("harness/harness.js");
    for (attribute, key) in [("sandbox", "sandbox"), ("allow", "allow"), ("referrerpolicy", "referrerPolicy")] {
        let from_frame = format!(r#"frame.setAttribute("{attribute}", setup.frame.{key})"#);
        assert_eq!(harness.matches(&from_frame).count(), 1, "{attribute}");
        assert_eq!(harness.matches(&format!(r#""{attribute}""#)).count(), 1, "{attribute} is set once");
    }
    for own in ["allow-", "no-referrer", ".sandbox =", ".allow =", ".referrerPolicy ="] {
        assert!(!harness.contains(own), "the harness writes {own} itself");
    }
    assert!(!read("harness/index.html").contains("<iframe"));
}

/// Whether `name` is one of Tauri's own crates, which move with its
/// version: not a plugin, nor tauri-specta.
fn is_tauri(name: &str) -> bool {
    name == "tauri" || (name.starts_with("tauri-") && !name.starts_with("tauri-plugin-") && name != "tauri-specta")
}

#[test]
fn the_redteam_uses_the_apps_tauri() {
    let root = toml("../../Cargo.toml");
    let pins = root["workspace"]["dependencies"].as_table().unwrap();
    for name in TAURI {
        let pin = pins.get(name).and_then(toml::Value::as_str).unwrap_or_default();
        assert!(pin.starts_with('='), "the workspace pins {name} exactly: {pin:?}");
    }
    let mut features = Vec::new();
    for path in ["Cargo.toml", "../src-tauri/Cargo.toml"] {
        let manifest = toml(path);
        let targets = manifest.get("target").and_then(toml::Value::as_table);
        let tables =
            [Some(&manifest)].into_iter().chain(targets.into_iter().flat_map(|t| t.values().map(|t| t.as_table())));
        let mut named = Vec::new();
        for table in tables.flatten() {
            for kind in ["dependencies", "build-dependencies", "dev-dependencies"] {
                let deps = table.get(kind).and_then(toml::Value::as_table);
                for (name, dep) in deps.into_iter().flatten().filter(|(name, _)| is_tauri(name)) {
                    let inherited = dep.get("workspace").and_then(toml::Value::as_bool) == Some(true);
                    assert!(inherited && dep.get("version").is_none(), "{path}: {name} isn't the workspace's");
                    named.push(name.as_str());
                }
            }
        }
        named.sort_unstable();
        assert_eq!(named, TAURI, "{path}");
        features.push(manifest["dependencies"]["tauri"].get("features").cloned());
    }
    // So one tauri is built, and both run on it.
    assert_eq!(features[0], features[1]);
}

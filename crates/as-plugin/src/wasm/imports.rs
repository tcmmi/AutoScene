//! What a component may import: AutoScene's world `autoscene:plugin@1.x`
//! up to the minor this AutoScene serves, and WASI 0.2 up to the patch this
//! wasmtime carries. A component built for a newer world or WASI needs a
//! newer AutoScene; anything else is a plugin AutoScene doesn't offer.

use semver::Version;
use wasmparser::{Chunk, ComponentTypeRef, Parser, Payload, TypeBounds};

use super::MAX_DETAIL_CHARS;
use as_core::{Msg, msg};

use crate::error::{PluginError, PluginResult};
use crate::manifest::{Manifest, Permission, cut};

/// The minor of `autoscene:plugin@1` this AutoScene serves.
pub const WORLD_MINOR: u64 = 1;
/// The newest WASI 0.2 patch this AutoScene offers (wasmtime 50.0.1
/// carries `wasi:*@0.2.12`).
pub const WASI_PATCH: u64 = 12;
/// The WASI packages it offers: all of WASI 0.2 (its sockets turned off in
/// each run).
const WASI: [&str; 7] = ["io", "clocks", "random", "cli", "filesystem", "sockets", "http"];
/// The interfaces of AutoScene's world.
const WORLD: [&str; 2] = ["types", "host"];
/// The most imports a component's review reads. All of WASI and AutoScene's
/// world are a few dozen; a package's 64 MB could name millions, which would
/// all be held here, before anyone agreed to anything.
const MAX_IMPORTS: usize = 4096;

/// What the component `bytes` imports, as `ns:pkg/interface@version`, read
/// from its top level: nothing is compiled, and the code in it (its core
/// modules, the components nested in it) is stepped over unread. Why not,
/// when its top level can't be read. A name given to a resource of an import
/// isn't told from one given to its own type, so it isn't listed here; a
/// compile refuses it.
pub(crate) fn declared(bytes: &[u8]) -> Result<Vec<String>, String> {
    let mut parser = Parser::new(0);
    let (mut at, mut names) = (0, Vec::new());
    loop {
        let (payload, consumed) = match parser.parse(&bytes[at..], true).map_err(|e| e.to_string())? {
            Chunk::Parsed { payload, consumed } => (payload, consumed),
            Chunk::NeedMoreData(_) => return Err("it ends early".into()),
        };
        at += consumed;
        match payload {
            Payload::ComponentImportSection(imports) => {
                for import in imports {
                    let import = import.map_err(|e| e.to_string())?;
                    // A name for a type the component defines itself, as its
                    // world's `run-request`: nobody has to provide it.
                    if !matches!(import.ty, ComponentTypeRef::Type(TypeBounds::Eq(_))) {
                        if names.len() == MAX_IMPORTS {
                            return Err("it has too many imports".into());
                        }
                        names.push(import.name.name.to_owned());
                    }
                }
            }
            // The parser is past them, and they aren't read: the next
            // section starts where they end, if the file holds that much.
            Payload::ModuleSection { unchecked_range: code, .. }
            | Payload::ComponentSection { unchecked_range: code, .. } => {
                at = usize::try_from(code.end).ok().filter(|end| *end <= bytes.len()).ok_or("it ends early")?;
            }
            Payload::End(_) => return Ok(names),
            _ => {}
        }
    }
}

/// Checks the imports `names` of a component (`ns:pkg/interface@x.y.z`
/// each). Gives what may surprise its author about the manifest `m`, when
/// given: wasi:http without the network permission (every request is
/// refused), or the permission without wasi:http.
pub(crate) fn check<'a>(names: impl Iterator<Item = &'a str>, m: Option<&Manifest>) -> PluginResult<Vec<Msg>> {
    let mut http = false;
    for name in names {
        http |= offered(name)?;
    }
    let Some(m) = m else { return Ok(Vec::new()) };
    let network = m.permissions.contains(&Permission::Network);
    Ok(match (http, network) {
        (true, false) => vec![msg!(
            "plugin.warn.httpWithoutNetwork",
            "It imports wasi:http but doesn't ask for network: every request it makes is refused."
        )],
        (false, true) => vec![msg!("plugin.warn.networkWithoutHttp", "It asks for network but can't make requests.")],
        _ => Vec::new(),
    })
}

/// Checks the import `name`: whether AutoScene offers it, and whether it is
/// wasi:http.
fn offered(name: &str) -> PluginResult<bool> {
    let not_offered = || {
        let name = cut(name, MAX_DETAIL_CHARS);
        PluginError::Invalid(msg!(
            "plugin.importNotOffered",
            "The plugin uses {{name}}, which AutoScene doesn't offer.",
            name = name
        ))
    };
    let (path, version) = name.split_once('@').ok_or_else(not_offered)?;
    let (package, interface) = path.split_once('/').ok_or_else(not_offered)?;
    let version = Version::parse(version).map_err(|_| not_offered())?;
    match package.split_once(':').ok_or_else(not_offered)? {
        ("autoscene", "plugin") => {
            if version.major != 1 || version.minor > WORLD_MINOR {
                return Err(PluginError::TooNew("world".into()));
            }
            if !WORLD.contains(&interface) || !version.pre.is_empty() {
                return Err(not_offered());
            }
            Ok(false)
        }
        ("wasi", wasi) if WASI.contains(&wasi) => {
            if version.major > 0 || version.minor > 2 || (version.minor == 2 && version.patch > WASI_PATCH) {
                return Err(PluginError::TooNew("wasi".into()));
            }
            // 0.1 and the release candidates of 0.2 are not what wasmtime links.
            if version.minor < 2 || !version.pre.is_empty() {
                return Err(not_offered());
            }
            Ok(wasi == "http")
        }
        _ => Err(not_offered()),
    }
}

/// Why the import `name` needs a newer AutoScene, for the plugin's author:
/// what this one offers, and what to do. None when it doesn't need one.
pub fn too_new(name: &str) -> Option<String> {
    let (offers, todo) = match offered(name) {
        Err(PluginError::TooNew(what)) if what == "world" => (
            format!("autoscene:plugin up to 1.{WORLD_MINOR}"),
            "Build it against the wit/plugin.wit that asp init writes",
        ),
        Err(PluginError::TooNew(_)) => (format!("WASI up to 0.2.{WASI_PATCH}"), "Build it with an older toolchain"),
        _ => return None,
    };
    Some(format!("{name} needs a newer AutoScene: this one offers {offers}. {todo}, or wait for a newer AutoScene."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::parse_for_any_app;

    /// What a Rust plugin built with std imports.
    const STD: [&str; 6] = [
        "wasi:io/streams@0.2.12",
        "wasi:cli/environment@0.2.12",
        "wasi:clocks/monotonic-clock@0.2.9",
        "wasi:filesystem/types@0.2.0",
        "wasi:random/random@0.2.12",
        "wasi:sockets/tcp@0.2.12",
    ];

    fn checked(names: &[&str]) -> PluginResult<Vec<Msg>> {
        check(names.iter().copied(), None)
    }

    fn manifest(network: bool) -> Manifest {
        let mut m = serde_json::json!({
            "manifestVersion": 2, "id": "dev.autoscene.hello", "version": "1.0.0", "name": "Hello",
            "description": "Says hello.", "author": {"name": "AutoScene"},
            "engines": {"autoscene": ">=1.1.0", "protocol": 2},
            "runtime": {"type": "wasm", "component": "plugin.wasm"},
            "contributes": {"commands": [{"id": "hello", "title": "Hello"}]}
        });
        if network {
            m["permissions"] = serde_json::json!(["network"]);
            m["network"] = serde_json::json!({"hosts": ["discord.com"]});
        }
        parse_for_any_app(m.to_string().as_bytes()).unwrap().manifest
    }

    #[test]
    fn this_world_and_wasi_0_2_are_offered() {
        let mut names = STD.to_vec();
        names.extend(["autoscene:plugin/host@1.0.0", "autoscene:plugin/types@1.0.0", "wasi:http/types@0.2.10"]);
        assert!(checked(&names).unwrap().is_empty());
        // A plugin of 1.1.0 (with on-event) as well as one of 1.0.0.
        let newest = ["autoscene:plugin/host@1.1.0", "autoscene:plugin/types@1.1.0"];
        assert!(checked(&newest).unwrap().is_empty());
    }

    #[test]
    fn a_newer_world_or_wasi_needs_a_newer_autoscene() {
        for name in ["autoscene:plugin/host@1.2.0", "autoscene:plugin/host@2.0.0", "autoscene:plugin/extra@1.2.0"] {
            assert!(matches!(checked(&[name]), Err(PluginError::TooNew(f)) if f == "world"), "{name}");
        }
        for name in ["wasi:io/streams@0.2.13", "wasi:http/handler@0.3.0", "wasi:cli/run@1.0.0"] {
            assert!(matches!(checked(&[name]), Err(PluginError::TooNew(f)) if f == "wasi"), "{name}");
        }
    }

    #[test]
    fn what_autoscene_does_not_offer_is_refused() {
        for name in [
            "wasi:keyvalue/store@0.2.0-draft",
            "wasi:io/streams@0.2.0-rc-2023-11-10",
            "wasi:io/streams@0.1.0",
            "autoscene:plugin/extra@1.0.0",
            "autoscene:plugin/host@1.0.0-rc",
            "example:thing/api@1.0.0",
            "wasi:io/streams",
            "wasi:io@0.2.0",
            "run",
        ] {
            let e = checked(&[name]).unwrap_err();
            assert_eq!(e.to_string(), format!("The plugin uses {name}, which AutoScene doesn't offer."));
        }
    }

    #[test]
    fn http_and_the_network_permission_go_together() {
        let http = ["wasi:http/outgoing-handler@0.2.12"];
        let warned = |names: &[&str], network: bool| check(names.iter().copied(), Some(&manifest(network))).unwrap();
        let said = |w: Vec<Msg>| w.into_iter().map(|m| (m.key, m.text)).collect::<Vec<_>>();
        let warning = |key: &str, english: &str| vec![(key.to_owned(), english.to_owned())];
        assert_eq!(
            said(warned(&http, false)),
            warning(
                "plugin.warn.httpWithoutNetwork",
                "It imports wasi:http but doesn't ask for network: every request it makes is refused."
            )
        );
        assert_eq!(
            said(warned(&STD, true)),
            warning("plugin.warn.networkWithoutHttp", "It asks for network but can't make requests.")
        );
        assert!(warned(&http, true).is_empty());
        // Python always imports sockets; they are refused, and nothing is said.
        assert!(warned(&STD, false).is_empty());
    }

    #[test]
    fn what_needs_a_newer_autoscene_is_named_for_its_author() {
        assert_eq!(
            too_new("wasi:cli/environment@0.2.19").as_deref(),
            Some(
                "wasi:cli/environment@0.2.19 needs a newer AutoScene: this one offers WASI up to 0.2.12. \
                 Build it with an older toolchain, or wait for a newer AutoScene."
            )
        );
        assert_eq!(
            too_new("autoscene:plugin/host@1.2.0").as_deref(),
            Some(
                "autoscene:plugin/host@1.2.0 needs a newer AutoScene: this one offers autoscene:plugin up to 1.1. \
                 Build it against the wit/plugin.wit that asp init writes, or wait for a newer AutoScene."
            )
        );
        // A newer major of either is newer too.
        assert!(too_new("wasi:cli/environment@1.0.0").is_some_and(|s| s.contains("WASI up to 0.2.12")));
        assert!(too_new("autoscene:plugin/host@2.0.0").is_some_and(|s| s.contains("autoscene:plugin up to 1.1")));
        for fine_or_unknown in ["wasi:cli/environment@0.2.12", "example:thing/api@1.0.0", "run"] {
            assert_eq!(too_new(fine_or_unknown), None, "{fine_or_unknown}");
        }
    }
}

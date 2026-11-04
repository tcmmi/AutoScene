//! What a component may import: AutoScene's world `autoscene:plugin@1.x`
//! up to the minor this AutoScene serves, and WASI 0.2 up to the patch this
//! wasmtime carries. A component built for a newer world or WASI needs a
//! newer AutoScene; anything else is a plugin AutoScene doesn't offer.

use semver::Version;

use crate::error::{PluginError, PluginResult};
use crate::manifest::{Manifest, Permission};

/// The minor of `autoscene:plugin@1` this AutoScene serves.
pub const WORLD_MINOR: u64 = 0;
/// The newest WASI 0.2 patch this AutoScene offers (wasmtime 50.0.1
/// carries `wasi:*@0.2.12`).
pub const WASI_PATCH: u64 = 12;
/// The WASI packages it offers: all of WASI 0.2 (its sockets turned off in
/// each run).
const WASI: [&str; 7] = ["io", "clocks", "random", "cli", "filesystem", "sockets", "http"];
/// The interfaces of AutoScene's world.
const WORLD: [&str; 2] = ["types", "host"];

/// Checks the imports `names` of a component (`ns:pkg/interface@x.y.z`
/// each). Gives what may surprise its author about the manifest `m`, when
/// given: wasi:http without the network permission (every request is
/// refused), or the permission without wasi:http.
pub(crate) fn check<'a>(names: impl Iterator<Item = &'a str>, m: Option<&Manifest>) -> PluginResult<Vec<String>> {
    let mut http = false;
    for name in names {
        let not_offered = || PluginError::Invalid(format!("The plugin uses {name}, which AutoScene doesn't offer."));
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
            }
            ("wasi", wasi) if WASI.contains(&wasi) => {
                if version.major > 0 || version.minor > 2 || (version.minor == 2 && version.patch > WASI_PATCH) {
                    return Err(PluginError::TooNew("wasi".into()));
                }
                // 0.1 and the release candidates of 0.2 are not what wasmtime links.
                if version.minor < 2 || !version.pre.is_empty() {
                    return Err(not_offered());
                }
                http |= wasi == "http";
            }
            _ => return Err(not_offered()),
        }
    }
    let Some(m) = m else { return Ok(Vec::new()) };
    let network = m.permissions.contains(&Permission::Network);
    Ok(match (http, network) {
        (true, false) => {
            vec!["It imports wasi:http but doesn't ask for network: every request it makes is refused.".into()]
        }
        (false, true) => vec!["It asks for network but can't make requests.".into()],
        _ => Vec::new(),
    })
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

    fn checked(names: &[&str]) -> PluginResult<Vec<String>> {
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
        assert_eq!(checked(&names).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn a_newer_world_or_wasi_needs_a_newer_autoscene() {
        for name in ["autoscene:plugin/host@1.1.0", "autoscene:plugin/host@2.0.0", "autoscene:plugin/extra@1.1.0"] {
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
        assert_eq!(
            warned(&http, false),
            ["It imports wasi:http but doesn't ask for network: every request it makes is refused."]
        );
        assert_eq!(warned(&STD, true), ["It asks for network but can't make requests."]);
        assert!(warned(&http, true).is_empty());
        // Python always imports sockets; they are refused, and nothing is said.
        assert!(warned(&STD, false).is_empty());
    }
}

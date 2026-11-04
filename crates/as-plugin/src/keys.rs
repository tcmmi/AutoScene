//! The keys AutoScene trusts for plugins, compiled into the app: those of
//! its plugin catalog, with where it is published (see [`crate::catalog`]).
//! They sign the catalog, never a package: a package is Reviewed when the
//! catalog lists its lock (see [`crate::store::verdict`]).

/// What a catalog key may do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyRole {
    /// The catalog CI's key (an Actions secret): signs indexes of the first
    /// epoch, and of each later one whose certificate names it.
    Online,
    /// The owner's key, kept offline: signs indexes, and alone signs epoch
    /// certificates.
    Offline,
}

/// The AutoScene plugin catalog's keys, each the base64 line of a minisign
/// `.pub` file. Not the updater key. Empty until the catalog opens (then one
/// of each role).
pub const CATALOG_KEYS: &[(KeyRole, &str)] = &[];

/// Where the index is published, tried in turn; the newest that checks out
/// wins. `index.json` and `index.json.minisig` are appended to each.
pub const CATALOG_URLS: &[&str] = &[
    "https://tcmmi.github.io/autoscene-plugins/",
    "https://raw.githubusercontent.com/tcmmi/autoscene-plugins/main/site/",
];

/// The epoch an index needs no certificate for.
pub const FIRST_EPOCH: u64 = 1;

/// The lowest epoch this build takes, also on its first fetch. A release
/// after a key rotation raises it, so a fresh install never takes an index
/// of an epoch whose key was withdrawn.
pub const MIN_EPOCH: u64 = 1;

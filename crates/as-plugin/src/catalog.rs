//! AutoScene's plugin catalog: a signed index of the plugins AutoScene
//! reviewed, by the SHA-256 of each version's `files.lock`, and of the
//! versions it withdrew. The index is `index.json`; its minisign signature
//! (`index.json.minisig`) carries a trusted comment that names the index's
//! epoch, number and expiry ([`index_comment`]), so it never passes for a
//! package's signature, nor a package's for its.
//!
//! [`check`] takes a fetched index only when a catalog key signed it
//! ([`crate::keys::CATALOG_KEYS`]), it hasn't run out (and lasts at most
//! [`MAX_LIFETIME`], dated at most [`MAX_CLOCK_SKEW`] ahead), and it is newer
//! than the index AutoScene holds. Order is `(epoch, seq)`, the epoch first;
//! an index of the same number must be the same bytes. So nobody can hand
//! AutoScene an older index again, nor keep it on one past its expiry. One
//! entry that breaks a rule refuses the whole index: the catalog's CI wrote
//! it, so a bad entry is a bug there.
//!
//! Two keys sign: the catalog CI's online key and the owner's offline key.
//! Either signs an index; only the offline key starts an epoch. An index
//! above [`FIRST_EPOCH`] carries the offline key's epoch certificate
//! ([`certificate_line`]), which names the online keys of its epoch, the
//! compiled one too, and may withdraw keys for good. A leaked online key
//! could sign the highest number there is, but only in its epoch: the
//! owner's next certificate leaves it out, and is newer still. That holds
//! also for an AutoScene that never saw the epoch that withdrew it.
//!
//! This module is pure: no files, no network, no clock. The caller passes
//! the time ([`now`] reads it), keeps the [`CatalogState`] and the index it
//! names, and judges plugins with the [`Catalog`] in hand: what the living
//! index approves, and what it withdrew, which holds after it ran out too.

use std::collections::HashSet;
use std::time::{Duration, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use minisign_verify::{Error as SignatureError, PublicKey, Signature};
use semver::{BuildMetadata, Comparator, Op, Prerelease, Version, VersionReq};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use as_core::{Msg, msg};

use crate::error::{PluginError, PluginResult};
use crate::keys::{CATALOG_KEYS, FIRST_EPOCH, KeyRole, MIN_EPOCH};
use crate::log::utc;
use crate::manifest::{
    MAX_PLATFORMS, MAX_RESOURCE_URLS, MAX_RESOURCES, MAX_RESOURCES_BYTES, PluginId, check_requirement, clean, cut,
    is_host, is_platform, requirement_for, shown, version_matches,
};
use crate::net::public_name;
use crate::package::{MAX_PACKAGE_BYTES, Picture, PictureFormat, check_path, picture_bytes_ok, sha256_hex};
use crate::trust::{key_id, signer_id};

/// The newest index format this AutoScene reads.
pub const CATALOG_VERSION: u32 = 1;
/// The version of [`CatalogState`] this AutoScene writes.
pub const STATE_VERSION: u32 = 1;
pub const MAX_INDEX_BYTES: u64 = 8 << 20;
pub const MAX_SIG_BYTES: u64 = 4 << 10;
pub const MAX_ENTRIES: usize = 4096;
pub const MAX_WITHDRAWN: usize = 1024;
/// The most download addresses an entry names for its package.
pub const MAX_URLS: usize = 4;
pub const MAX_URL_CHARS: usize = 2048;
/// The largest icon an entry carries, decoded.
pub const MAX_ICON_BYTES: usize = 8 << 10;
/// The longest an index may live: from when it was published to when it
/// runs out, in seconds (the catalog's CI makes it 30 days).
pub const MAX_LIFETIME: u64 = 31 * 86_400;
/// How far ahead of this computer's clock an index may be dated, in seconds:
/// so no index lives longer than [`MAX_LIFETIME`] and this from now.
pub const MAX_CLOCK_SKEW: u64 = 3 * 86_400;
pub const MAX_REASON_CHARS: usize = 200;
/// How an index's trusted comment starts ([`index_comment`]).
pub const INDEX_TAG: &str = "autoscene-catalog;v=1;";
/// How an epoch certificate's trusted comment starts ([`certificate_line`]).
pub const EPOCH_TAG: &str = "autoscene-catalog-epoch;v=1;";

/// The most permissions, and the most hosts, an entry names.
const MAX_NAMES: usize = 16;
/// The longest runtime type an entry names.
const MAX_RUNTIME_CHARS: usize = 16;

/// The keys an index is checked against, each with what it may do.
#[derive(Clone, Debug)]
pub struct CatalogKeys {
    keys: Vec<CatalogKey>,
}

#[derive(Clone, Debug)]
struct CatalogKey {
    role: KeyRole,
    key: PublicKey,
    /// Its key id, as minisign shows it.
    id: String,
}

impl CatalogKey {
    /// The minisign public key `base64` (the line a `.pub` file holds).
    fn read(role: KeyRole, base64: &str) -> PluginResult<Self> {
        let id = key_id(base64)?;
        let key = PublicKey::from_base64(base64)
            .map_err(|_| catalog(msg!("catalog.notAKey", "That isn't a minisign public key.")))?;
        Ok(Self { role, key, id })
    }
}

impl CatalogKeys {
    /// The catalog keys compiled into the app ([`CATALOG_KEYS`]).
    pub fn builtin() -> Self {
        Self::new(CATALOG_KEYS).expect("keys::CATALOG_KEYS are minisign public keys")
    }

    /// Keys of one's own (tests, `asp catalog`), each the base64 line of a
    /// minisign `.pub` file.
    pub fn new(keys: &[(KeyRole, &str)]) -> PluginResult<Self> {
        Self { keys: Vec::new() }.with(keys)
    }

    /// These keys and `extra` (the debug override's).
    pub fn with(mut self, extra: &[(KeyRole, &str)]) -> PluginResult<Self> {
        for (role, key) in extra {
            self.keys.push(CatalogKey::read(*role, key)?);
        }
        Ok(self)
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

/// The catalog's index, `index.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Index {
    pub catalog_version: u32,
    pub epoch: u64,
    /// Its number in its epoch.
    pub seq: u64,
    /// When it was published, in Unix seconds.
    pub published: u64,
    /// When it runs out, in Unix seconds.
    pub expires: u64,
    /// The offline key's signature of its epoch ([`certificate_line`]): the
    /// four lines of a minisign signature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epoch_certificate: Option<String>,
    #[serde(default)]
    pub plugins: Vec<Entry>,
    #[serde(default)]
    pub withdrawn: Vec<Withdrawal>,
}

/// A plugin version the catalog reviewed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: PluginId,
    /// Its version, strict semver.
    pub version: String,
    /// The SHA-256 of its `files.lock`, 64 lowercase hex digits: a package
    /// with that lock is the one reviewed.
    pub lock: String,
    pub package: PackageRef,
    /// The author's minisign key, as the catalog knows it.
    #[serde(default)]
    pub author_key: Option<String>,
    pub name: String,
    #[serde(default)]
    pub summary: String,
    /// The author, as the catalog checked them.
    pub author: String,
    #[serde(default)]
    pub license: Option<String>,
    /// The platforms it runs on; none names all.
    #[serde(default)]
    pub platforms: Vec<String>,
    /// Its manifest's runtime type: `process`, `service`, `none`, or one a
    /// later AutoScene runs.
    #[serde(default)]
    pub runtime: String,
    #[serde(default)]
    pub gpu: bool,
    #[serde(default)]
    pub uv_script: bool,
    /// What it says it does, as text: a later AutoScene's permissions are
    /// kept.
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub hosts: Vec<String>,
    /// The AutoScene it needs, as its manifest's `engines.autoscene` says.
    pub engines: String,
    pub source: SourceRef,
    /// Its icon, a `data:image/png;base64,` or `data:image/svg+xml;base64,`
    /// URL.
    #[serde(default)]
    pub icon: Option<String>,
    /// When the catalog approved it, in Unix seconds.
    pub approved: u64,
    /// What the large files its manifest names take, in bytes
    /// ([`crate::manifest::Resource`]): what its first run downloads.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub resource_bytes: u64,
    /// The hosts those files come from, lowercase, each once.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_hosts: Vec<String>,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// Where a reviewed package is, and what it is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageRef {
    /// Its addresses, tried in turn.
    pub urls: Vec<String>,
    pub size: u64,
    pub sha256: String,
}

/// The source a reviewed version was built from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRef {
    /// `https://github.com/<owner>/<repo>`.
    pub repo: String,
    /// 40 lowercase hex digits.
    pub commit: String,
    /// Its folder in the repository; empty or `.` for the top.
    #[serde(default)]
    pub path: String,
}

/// Versions of a plugin AutoScene withdrew: they don't run, and don't
/// install again.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Withdrawal {
    pub id: PluginId,
    /// A version requirement, `*` for all.
    pub versions: String,
    /// Why, in AutoScene's words.
    pub reason: String,
    /// When, in Unix seconds.
    pub since: u64,
}

/// What AutoScene holds of the catalog, kept beside the index it names.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogState {
    pub v: u32,
    pub epoch: u64,
    pub seq: u64,
    /// The SHA-256 of the index held; none before the first.
    pub sha256: Option<String>,
    pub published: u64,
    pub expires: u64,
    /// When the last index that checked out was fetched.
    pub fetched_at: u64,
    /// When AutoScene last tried.
    pub checked_at: u64,
    /// The ids of the keys a certificate withdrew. Only ever grows.
    pub withdrawn_keys: Vec<String>,
    /// The held index's withdrawals, kept when its file is lost.
    pub withdrawn: Vec<Withdrawal>,
    /// The id of the key that signed the index held.
    pub signed_by: Option<String>,
    /// Why the last try failed, in English: an older AutoScene reads it as text.
    pub last_error: Option<String>,
    /// The same words for the UI. None in a state kept before they had keys.
    #[serde(default)]
    pub last_error_msg: Option<Msg>,
}

/// What [`check`] made of an index.
#[derive(Debug)]
pub enum Checked {
    /// A newer index than the one held.
    Newer(Box<Accepted>),
    /// The index held, again.
    Same,
}

/// A newer index, and who signed it.
#[derive(Debug)]
pub struct Accepted {
    pub index: Index,
    pub sha256: String,
    /// The signing key's id.
    pub signed_by: String,
    pub role: KeyRole,
    /// The ids of the keys withdrawn now: the held ones and its certificate's.
    pub withdrawn_keys: Vec<String>,
}

/// The time now, in Unix seconds.
pub fn now() -> u64 {
    u64::try_from(as_api::now_ms() / 1000).unwrap_or(0)
}

/// The trusted comment an index's signature carries:
/// `autoscene-catalog;v=1;epoch=<epoch>;seq=<seq>;expires=<expires>`.
pub fn index_comment(epoch: u64, seq: u64, expires: u64) -> String {
    format!("{INDEX_TAG}epoch={epoch};seq={seq};expires={expires}")
}

/// What an epoch certificate signs, also as its trusted comment:
/// `autoscene-catalog-epoch;v=1;epoch=<epoch>;withdraw=<key ids>;online=<keys>`,
/// the key ids it withdraws and the online keys (base64) it names, each list
/// sorted, joined by commas, and `-` when it is empty.
pub fn certificate_line(epoch: u64, withdraw: &[String], online: &[String]) -> String {
    format!("{EPOCH_TAG}epoch={epoch};withdraw={};online={}", list(withdraw), list(online))
}

/// Checks a fetched index, `bytes`, and its signature `sig` against the
/// catalog `keys` and the state AutoScene `held`, at `now`. It is
/// [`Checked::Newer`] when it is signed, living and newer than the one held,
/// and [`Checked::Same`] when it is that one again. Anything else is
/// [`PluginError::Catalog`] with why. "This catalog needs a newer AutoScene."
/// comes before any signature is checked, so anyone at a catalog address can
/// say it.
pub fn check(bytes: &[u8], sig: &str, keys: &CatalogKeys, held: &CatalogState, now: u64) -> PluginResult<Checked> {
    check_from(bytes, sig, keys, held, now, MIN_EPOCH)
}

/// [`check`], with `min_epoch` the lowest epoch taken.
fn check_from(
    bytes: &[u8],
    sig: &str,
    keys: &CatalogKeys,
    held: &CatalogState,
    now: u64,
    min_epoch: u64,
) -> PluginResult<Checked> {
    let Signed { header, signed_by, role, withdrawn_keys } = signed(bytes, sig, keys, held)?;
    if header.published > header.expires || header.expires - header.published > MAX_LIFETIME {
        return Err(catalog(msg!("catalog.badDates", "The catalog's dates can't be right.")));
    }
    // A published time a little in the future is taken (the clock may be
    // behind), and the date shows. Further ahead, the index would live past
    // MAX_LIFETIME from now.
    if header.published > now.saturating_add(MAX_CLOCK_SKEW) {
        let dated = date(header.published);
        return Err(catalog(msg!(
            "catalog.notYet",
            "The catalog is dated {{date}}, which hasn't come yet. Check this computer's clock.",
            date = dated
        )));
    }
    if header.expires <= now {
        return Err(catalog(msg!("catalog.ranOut", "The catalog ran out on {{date}}.", date = date(header.expires))));
    }
    let sha256 = sha256_hex(bytes);
    // An empty state holds no number: any index from min_epoch up is newer.
    let held_number = held.sha256.as_ref().map(|_| (held.epoch, held.seq));
    let number = (header.epoch, header.seq);
    if header.epoch < min_epoch || held_number.is_some_and(|held| number < held) {
        return Err(catalog(msg!("catalog.older", "This catalog is older than the one AutoScene has.")));
    }
    if held_number == Some(number) {
        if held.sha256.as_deref() == Some(sha256.as_str()) {
            return Ok(Checked::Same);
        }
        tracing::warn!("catalog epoch {} seq {} came with other bytes ({sha256})", header.epoch, header.seq);
        return Err(catalog(msg!("catalog.sameNumber", "Two catalogs carry the same number.")));
    }
    let index = read(header, bytes)?;
    Ok(Checked::Newer(Box::new(Accepted { index, sha256, signed_by, role, withdrawn_keys })))
}

/// Reads the index AutoScene holds, `bytes`, again (at start): it must be the
/// one `held` names, signed and sound. Its time and order aren't checked: it
/// was taken already, and may have run out since.
pub fn reload(bytes: &[u8], sig: &str, keys: &CatalogKeys, held: &CatalogState) -> PluginResult<Index> {
    if held.sha256.as_deref() != Some(sha256_hex(bytes).as_str()) {
        return Err(catalog(msg!("catalog.notChecked", "The saved catalog isn't the one AutoScene checked.")));
    }
    read(signed(bytes, sig, keys, held)?.header, bytes)
}

impl CatalogState {
    /// The state once `a` is taken, at `now`.
    pub fn after(&self, a: &Accepted, now: u64) -> Self {
        let mut withdrawn_keys: Vec<String> = self.withdrawn_keys.iter().chain(&a.withdrawn_keys).cloned().collect();
        withdrawn_keys.sort_unstable();
        withdrawn_keys.dedup();
        Self {
            v: STATE_VERSION,
            epoch: a.index.epoch,
            seq: a.index.seq,
            sha256: Some(a.sha256.clone()),
            published: a.index.published,
            expires: a.index.expires,
            fetched_at: now,
            checked_at: now,
            withdrawn_keys,
            withdrawn: a.index.withdrawn.clone(),
            signed_by: Some(a.signed_by.clone()),
            last_error: None,
            last_error_msg: None,
        }
    }

    /// The state after a try at `now`, which `error` says failed.
    pub fn tried(&self, now: u64, error: Option<Msg>) -> Self {
        let last_error = error.as_ref().map(|e| e.text.clone());
        Self { checked_at: now, last_error, last_error_msg: error, ..self.clone() }
    }

    /// Why the last try failed.
    pub fn error(&self) -> Option<Msg> {
        self.last_error_msg.clone().or_else(|| self.last_error.as_deref().map(Msg::plain))
    }

    /// Whether the last try found an index only a later AutoScene reads.
    pub fn needs_newer_app(&self) -> bool {
        self.last_error.as_deref() == Some(needs_newer_app_said().text.as_str())
    }
}

/// The catalog in hand: the index held, if its file was read, and the state.
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    index: Option<Index>,
    state: CatalogState,
}

/// What the catalog offers of a plugin.
#[derive(Clone, Copy, Debug)]
pub enum Offer<'a> {
    /// The newest version this AutoScene runs on this computer.
    Here(&'a Entry),
    /// The newest version, which needs a newer AutoScene.
    NeedsNewerApp(&'a Entry),
    /// The newest version, which isn't made for this computer (or this
    /// AutoScene is too new for it).
    NotHere(&'a Entry),
}

impl Catalog {
    pub fn new(index: Option<Index>, state: CatalogState) -> Self {
        Self { index, state }
    }

    /// No catalog: nothing reviewed, nothing withdrawn.
    pub fn none() -> Self {
        Self::default()
    }

    pub fn state(&self) -> &CatalogState {
        &self.state
    }

    pub fn index(&self) -> Option<&Index> {
        self.index.as_ref()
    }

    /// Whether there is an index, and it hasn't run out at `now`.
    pub fn living(&self, now: u64) -> bool {
        self.index.as_ref().is_some_and(|i| now < i.expires)
    }

    /// The entry that approves version `version` of `id`, whose `files.lock`
    /// has the SHA-256 `lock_sha256`: of the living index, and not withdrawn.
    pub fn approval(&self, id: &PluginId, version: &str, lock_sha256: &str, now: u64) -> Option<&Entry> {
        if !self.living(now) {
            return None;
        }
        let version = Version::parse(version).ok()?;
        let entry = self
            .entries(id)
            .find(|e| e.lock == lock_sha256 && Version::parse(&e.version).is_ok_and(|v| v == version))?;
        self.withdrawal(id, &entry.version).is_none().then_some(entry)
    }

    /// The withdrawal of version `version` of `id`, also once the index ran
    /// out or its file is lost ([`withdraws`] says which versions a
    /// requirement takes). A requirement that doesn't read (in a row written
    /// by hand) withdraws every version. A version that doesn't read (a
    /// broken record's) is withdrawn by a requirement of every version.
    pub fn withdrawal(&self, id: &PluginId, version: &str) -> Option<&Withdrawal> {
        let version = Version::parse(version).ok();
        self.state.withdrawn.iter().filter(|w| &w.id == id).find(|w| {
            let Ok(req) = requirement_for(&w.versions) else {
                return true;
            };
            match &version {
                Some(version) => withdraws(&req, version),
                None => req.comparators.is_empty(),
            }
        })
    }

    /// What the catalog offers of `id` to the AutoScene `app` on `platform`,
    /// of the versions it hasn't withdrawn; none when it lists none.
    pub fn offer(&self, id: &PluginId, app: &Version, platform: &str) -> Option<Offer<'_>> {
        let mut versions: Vec<(&Entry, Version)> = self
            .entries(id)
            .filter(|e| self.withdrawal(id, &e.version).is_none())
            .filter_map(|e| Some((e, Version::parse(&e.version).ok()?)))
            .collect();
        versions.sort_by(|(_, a), (_, b)| b.cmp_precedence(a));
        let meets = |e: &Entry| requirement_for(&e.engines).is_ok_and(|req| version_matches(&req, app));
        let runs_here = |e: &Entry| e.platforms.is_empty() || e.platforms.iter().any(|p| p == platform);
        if let Some((e, _)) = versions.iter().find(|(e, _)| meets(e) && runs_here(e)) {
            return Some(Offer::Here(e));
        }
        let (newest, _) = versions.first()?;
        let needs_newer = requirement_for(&newest.engines)
            .is_ok_and(|req| matches!(check_requirement(&req, app), Err(PluginError::TooNew(_))));
        Some(if needs_newer { Offer::NeedsNewerApp(newest) } else { Offer::NotHere(newest) })
    }

    /// The ids the index lists, each once, sorted.
    pub fn ids(&self) -> Vec<&PluginId> {
        let mut ids: Vec<&PluginId> = self.index.iter().flat_map(|i| &i.plugins).map(|e| &e.id).collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    fn entries<'a>(&'a self, id: &PluginId) -> impl Iterator<Item = &'a Entry> {
        self.index.iter().flat_map(|i| &i.plugins).filter(move |e| &e.id == id)
    }
}

/// Whether `req` withdraws `v`: as semver reads it, with its pre-release
/// cleared, or by precedence alone. Semver lets a pre-release meet only a
/// comparator that names one of its own version, so it alone has
/// `>=1.2.0, <1.3.0` leave out `1.2.5-beta`, and `<1.3.0` leave out
/// `1.3.0-rc.1`, which comes before the fix. Each way leans toward
/// withdrawing.
fn withdraws(req: &VersionReq, v: &Version) -> bool {
    let cleared = Version { pre: Prerelease::EMPTY, build: BuildMetadata::EMPTY, ..v.clone() };
    let mut by_precedence = req.clone();
    if !v.pre.is_empty() {
        // Every pre-release of M.m.p meets `>=M.m.p-0`, which names one.
        let lowest = Prerelease::new("0").expect("0 is a pre-release");
        let (major, minor, patch) = (v.major, Some(v.minor), Some(v.patch));
        by_precedence.comparators.push(Comparator { op: Op::GreaterEq, major, minor, patch, pre: lowest });
    }
    by_precedence.matches(v) || req.matches(&cleared)
}

/// What an index says of itself, read before its signature is checked.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Header {
    catalog_version: u32,
    epoch: u64,
    seq: u64,
    published: u64,
    expires: u64,
    #[serde(default)]
    epoch_certificate: Option<String>,
}

/// An index's entries and withdrawals, read once its signature checked out.
#[derive(Deserialize)]
struct Body {
    #[serde(default)]
    plugins: Vec<Value>,
    #[serde(default)]
    withdrawn: Vec<Value>,
}

/// An index whose signature checked out.
struct Signed {
    header: Header,
    signed_by: String,
    role: KeyRole,
    withdrawn_keys: Vec<String>,
}

/// An epoch certificate that checked out.
struct Certificate {
    withdraw: Vec<String>,
    online: Vec<CatalogKey>,
}

/// Checks the signature of the index `bytes`: its size, that `sig` is an
/// index's signature naming this index, its epoch certificate, and that a
/// key AutoScene knows and hasn't withdrawn signed it.
fn signed(bytes: &[u8], sig: &str, keys: &CatalogKeys, held: &CatalogState) -> PluginResult<Signed> {
    if bytes.len() as u64 > MAX_INDEX_BYTES || sig.len() as u64 > MAX_SIG_BYTES {
        return Err(catalog(too_large()));
    }
    let unreadable = unreadable_signature();
    let signature = Signature::decode(sig).map_err(|_| catalog(unreadable.clone()))?;
    let named = read_comment(signature.trusted_comment())?;
    // The header is read before the signature is checked, since the
    // certificate is in it. The entries wait until it checked out.
    let header = read_header(bytes)?;
    if (header.epoch, header.seq, header.expires) != named {
        return Err(catalog(msg!("catalog.otherCatalog", "The catalog's signature names another catalog.")));
    }
    let mut withdrawn = held.withdrawn_keys.clone();
    // A certificate names every online key of its epoch: a compiled one it
    // leaves out doesn't sign there, also for an AutoScene that never took
    // the index whose certificate withdrew it. The first epoch needs none,
    // and its online keys are the compiled ones.
    let online: Vec<CatalogKey> = match &header.epoch_certificate {
        Some(text) => {
            let c = certificate(text, header.epoch, keys, &held.withdrawn_keys)?;
            withdrawn.extend(c.withdraw);
            c.online
        }
        None if header.epoch > FIRST_EPOCH => return Err(not_offline()),
        None => keys.keys.iter().filter(|k| k.role == KeyRole::Online).cloned().collect(),
    };
    withdrawn.sort_unstable();
    withdrawn.dedup();
    let offline = keys.keys.iter().filter(|k| k.role == KeyRole::Offline);
    let known = offline.chain(&online).filter(|k| !withdrawn.contains(&k.id));
    let changed = msg!("catalog.changed", "The catalog was changed after it was signed.");
    let Some(key) = signer(known, bytes, &signature, &changed, &unreadable)? else {
        // A compiled key that didn't sign was withdrawn, or left out of the epoch.
        let id = signer_id(sig);
        return Err(catalog(if withdrawn.contains(&id) || keys.keys.iter().any(|k| k.id == id) {
            msg!("catalog.keyWithdrawn", "The catalog is signed with a key AutoScene withdrew.")
        } else {
            msg!("catalog.keyUnknown", "The catalog is signed with a key AutoScene doesn't know.")
        }));
    };
    let (signed_by, role) = (key.id.clone(), key.role);
    Ok(Signed { header, signed_by, role, withdrawn_keys: withdrawn })
}

/// Checks the epoch certificate `text` of an index of `epoch`: an offline key
/// that hasn't been withdrawn before signed it, and it withdraws none.
fn certificate(text: &str, epoch: u64, keys: &CatalogKeys, withdrawn: &[String]) -> PluginResult<Certificate> {
    let unreadable = msg!("catalog.unreadableCertificate", "The catalog's epoch certificate can't be read.");
    let signature = Signature::decode(text).map_err(|_| catalog(unreadable.clone()))?;
    let line = signature.trusted_comment();
    let (named, withdraw, online) = read_certificate_line(line).ok_or_else(|| catalog(unreadable.clone()))?;
    if named != epoch {
        return Err(catalog(msg!("catalog.certificateEpoch", "The catalog's epoch certificate is for another epoch.")));
    }
    let online = online.iter().map(|key| CatalogKey::read(KeyRole::Online, key)).collect::<PluginResult<Vec<_>>>();
    let online = online.map_err(|_| catalog(unreadable.clone()))?;
    let offline = keys.keys.iter().filter(|k| k.role == KeyRole::Offline && !withdrawn.contains(&k.id));
    let changed =
        msg!("catalog.certificateChanged", "The catalog's epoch certificate was changed after it was signed.");
    if signer(offline, line.as_bytes(), &signature, &changed, &unreadable)?.is_none() {
        return Err(not_offline());
    }
    // Only a release changes the offline keys. A certificate that withdrew
    // one would leave no key to start the next epoch, or hand them to a
    // leaked one.
    if keys.keys.iter().any(|k| k.role == KeyRole::Offline && withdraw.contains(&k.id)) {
        return Err(catalog(msg!(
            "catalog.certificateWithdraws",
            "The catalog's epoch certificate withdraws AutoScene's offline key."
        )));
    }
    Ok(Certificate { withdraw, online })
}

/// The first of `keys` that signed `data`, as `signature` says. A key whose
/// id isn't the signature's moves on; one whose id is, and that doesn't
/// verify it, refuses with `changed`. Only prehashed signatures are taken:
/// any other refuses with `unreadable`.
fn signer<'k>(
    keys: impl IntoIterator<Item = &'k CatalogKey>,
    data: &[u8],
    signature: &Signature,
    changed: &Msg,
    unreadable: &Msg,
) -> PluginResult<Option<&'k CatalogKey>> {
    for key in keys {
        match key.key.verify(data, signature, false) {
            Ok(()) => return Ok(Some(key)),
            Err(SignatureError::UnexpectedKeyId) => {}
            Err(SignatureError::InvalidSignature) => {
                tracing::warn!("catalog signature by key {} doesn't verify", key.id);
                return Err(catalog(changed.clone()));
            }
            Err(e) => {
                tracing::warn!("catalog signature by key {}: {e}", key.id);
                return Err(catalog(unreadable.clone()));
            }
        }
    }
    Ok(None)
}

/// The epoch, number and expiry an index's trusted comment `tc` names, in
/// its one spelling ([`index_comment`]).
fn read_comment(tc: &str) -> PluginResult<(u64, u64, u64)> {
    let named = tc.strip_prefix(INDEX_TAG).and_then(|rest| {
        let mut parts = rest.split(';');
        let mut field = |name: &str| parts.next()?.strip_prefix(name)?.strip_prefix('=')?.parse::<u64>().ok();
        Some((field("epoch")?, field("seq")?, field("expires")?))
    });
    match named {
        // Decimal, without leading zeros or signs, and nothing after.
        Some((epoch, seq, expires)) if index_comment(epoch, seq, expires) == tc => Ok((epoch, seq, expires)),
        _ if later_index(tc) => Err(needs_newer_app()),
        // A package's signature, or an epoch certificate's, among others.
        _ => Err(catalog(msg!("catalog.notASignature", "That isn't a catalog signature."))),
    }
}

/// Whether `tc` is the trusted comment of a later index format's signature.
fn later_index(tc: &str) -> bool {
    let version = tc.strip_prefix("autoscene-catalog;v=").and_then(|rest| rest.split(';').next());
    version.and_then(|v| v.parse::<u64>().ok()).is_some_and(|v| v > u64::from(CATALOG_VERSION))
}

/// The epoch, key ids and online keys an epoch certificate's `line` names,
/// in its one spelling ([`certificate_line`]). Nothing is checked of who
/// signed it: `asp catalog` reads the catalog's own certificates with it.
pub fn read_certificate_line(line: &str) -> Option<(u64, Vec<String>, Vec<String>)> {
    let mut parts = line.strip_prefix(EPOCH_TAG)?.split(';');
    let epoch = parts.next()?.strip_prefix("epoch=")?.parse().ok()?;
    let withdraw = items(parts.next()?.strip_prefix("withdraw=")?);
    let online = items(parts.next()?.strip_prefix("online=")?);
    let ids = withdraw
        .iter()
        .all(|id| id.len() == 16 && id.bytes().all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b)));
    (ids && certificate_line(epoch, &withdraw, &online) == line).then_some((epoch, withdraw, online))
}

/// A list of [`certificate_line`]: sorted, each once, `-` when empty.
fn list(items: &[String]) -> String {
    let mut items: Vec<&str> = items.iter().map(String::as_str).collect();
    items.sort_unstable();
    items.dedup();
    if items.is_empty() { "-".into() } else { items.join(",") }
}

fn items(list: &str) -> Vec<String> {
    if list == "-" { Vec::new() } else { list.split(',').map(str::to_owned).collect() }
}

/// Reads the header of the index's JSON, an object. Its other fields are
/// skipped, not kept: nobody has signed them yet.
fn read_header(bytes: &[u8]) -> PluginResult<Header> {
    /// The index format, read first: a later one may spell the rest otherwise.
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Format {
        catalog_version: Option<u64>,
    }
    let unreadable = || catalog(unreadable_index());
    if bytes.trim_ascii_start().first() != Some(&b'{') {
        return Err(unreadable());
    }
    let format: Format = serde_json::from_slice(bytes).map_err(|_| unreadable())?;
    match format.catalog_version {
        Some(v) if v > u64::from(CATALOG_VERSION) => Err(needs_newer_app()),
        Some(1..) => serde_json::from_slice(bytes).map_err(|_| unreadable()),
        _ => Err(unreadable()),
    }
}

/// Reads the entries and withdrawals of the index `bytes`, whose header is
/// `header`: one that breaks a rule refuses it all. The texts for people are
/// cleaned and cut.
fn read(header: Header, bytes: &[u8]) -> PluginResult<Index> {
    let body: Body = serde_json::from_slice(bytes).map_err(|_| catalog(unreadable_index()))?;
    if body.plugins.len() > MAX_ENTRIES {
        return Err(catalog(msg!(
            "catalog.tooManyPlugins",
            "The catalog lists more than {{max}} plugins.",
            max = MAX_ENTRIES
        )));
    }
    if body.withdrawn.len() > MAX_WITHDRAWN {
        return Err(catalog(msg!(
            "catalog.tooManyWithdrawn",
            "The catalog withdraws more than {{max}} plugins.",
            max = MAX_WITHDRAWN
        )));
    }
    let mut plugins = Vec::with_capacity(body.plugins.len());
    // Versions of one precedence are one version: build metadata doesn't count.
    let mut listed = HashSet::new();
    for (i, v) in body.plugins.iter().enumerate() {
        let read = entry(v, header.published).filter(|(e, version)| {
            listed.insert((e.id.clone(), Version { build: BuildMetadata::EMPTY, ..version.clone() }))
        });
        let Some((e, _)) = read else {
            return Err(catalog(msg!(
                "catalog.badEntry",
                "The catalog has an entry AutoScene can't read: {{which}}.",
                which = named(v, i)
            )));
        };
        plugins.push(e);
    }
    let mut withdrawn = Vec::with_capacity(body.withdrawn.len());
    for (i, v) in body.withdrawn.iter().enumerate() {
        let Some(w) = withdrawal(v, header.published) else {
            return Err(catalog(msg!(
                "catalog.badWithdrawal",
                "The catalog has a withdrawal AutoScene can't read: {{which}}.",
                which = named(v, i)
            )));
        };
        withdrawn.push(w);
    }
    Ok(Index {
        catalog_version: header.catalog_version,
        epoch: header.epoch,
        seq: header.seq,
        published: header.published,
        expires: header.expires,
        epoch_certificate: header.epoch_certificate,
        plugins,
        withdrawn,
    })
}

/// The entry `v` of an index published at `published`, and its version,
/// when it keeps every rule; its texts cleaned and cut.
fn entry(v: &Value, published: u64) -> Option<(Entry, Version)> {
    let mut e = Entry::deserialize(v).ok()?;
    let version = Version::parse(&e.version).ok()?;
    let p = &e.package;
    let sound = lower_hex(&e.lock, 64)
        && lower_hex(&p.sha256, 64)
        && (1..=MAX_PACKAGE_BYTES).contains(&p.size)
        && (1..=MAX_URLS).contains(&p.urls.len())
        && p.urls.iter().all(|url| download_url(url))
        && e.author_key.as_deref().is_none_or(|key| key_id(key).is_ok())
        && e.permissions.len() <= MAX_NAMES
        && e.permissions.iter().all(|p| permission_name(p))
        && e.hosts.len() <= MAX_NAMES
        && e.hosts.iter().all(|h| is_host(h))
        && e.resource_bytes <= MAX_RESOURCES_BYTES
        && e.resource_hosts.len() <= MAX_RESOURCES * MAX_RESOURCE_URLS
        && e.resource_hosts.iter().all(|h| is_host(h) && public_name(h))
        && e.platforms.len() <= MAX_PLATFORMS
        && e.platforms.iter().all(|p| is_platform(p))
        && runtime_name(&e.runtime)
        && requirement_for(&e.engines).is_ok()
        && source(&e.source)
        && e.icon.as_deref().is_none_or(icon)
        && e.approved <= published;
    if !sound {
        return None;
    }
    e.name = label(&e.name, 40)?;
    e.summary = cut(&clean(&e.summary), 300);
    e.author = label(&e.author, 60)?;
    e.license = e.license.as_deref().and_then(|license| label(license, 60));
    Some((e, version))
}

/// The withdrawal `v` of an index published at `published`, when it keeps
/// every rule; its reason cleaned.
fn withdrawal(v: &Value, published: u64) -> Option<Withdrawal> {
    let mut w = Withdrawal::deserialize(v).ok()?;
    w.reason = clean(&w.reason);
    let sound = requirement_for(&w.versions).is_ok()
        && (1..=MAX_REASON_CHARS).contains(&w.reason.chars().count())
        && w.since <= published;
    sound.then_some(w)
}

/// An entry's id and version, as a message may quote them, or its place.
fn named(v: &Value, i: usize) -> String {
    let text = |key: &str| v.get(key).and_then(Value::as_str).map(shown).filter(|t| !t.is_empty());
    match (text("id"), text("version")) {
        (Some(id), Some(version)) => format!("{id} {version}"),
        (Some(id), None) => id,
        _ => format!("number {}", i + 1),
    }
}

/// `text` cleaned and cut to `max` characters, unless nothing is left.
fn label(text: &str, max: usize) -> Option<String> {
    Some(cut(&clean(text), max)).filter(|t| !t.is_empty())
}

/// `len` lowercase hex digits.
fn lower_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// An `https://` address of at most [`MAX_URL_CHARS`] characters, without
/// white space or control characters.
fn download_url(url: &str) -> bool {
    url.starts_with("https://")
        && url.chars().count() <= MAX_URL_CHARS
        && !url.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// `[a-z][a-z0-9:-]{0,31}`: a permission, also one a later AutoScene knows.
fn permission_name(p: &str) -> bool {
    let b = p.as_bytes();
    (1..=32).contains(&b.len())
        && b[0].is_ascii_lowercase()
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b":-".contains(c))
}

/// `[a-z]{0,16}`: a runtime type, also one a later AutoScene runs, or none.
fn runtime_name(r: &str) -> bool {
    r.len() <= MAX_RUNTIME_CHARS && r.bytes().all(|b| b.is_ascii_lowercase())
}

/// A repository on GitHub, a commit and a folder there.
fn source(s: &SourceRef) -> bool {
    let name = |n: &str| {
        !n.is_empty() && n != "." && n != ".." && n.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    };
    let repo = s.repo.strip_prefix("https://github.com/").and_then(|r| r.split_once('/'));
    repo.is_some_and(|(owner, repo)| name(owner) && name(repo))
        && lower_hex(&s.commit, 40)
        && (s.path.is_empty() || s.path == "." || check_path(&s.path).is_ok())
}

/// A `data:` URL of a PNG or SVG icon, at most [`MAX_ICON_BYTES`] decoded.
fn icon(url: &str) -> bool {
    let formats = [("data:image/png;base64,", PictureFormat::Png), ("data:image/svg+xml;base64,", PictureFormat::Svg)];
    let Some((format, data)) = formats.into_iter().find_map(|(head, f)| Some((f, url.strip_prefix(head)?))) else {
        return false;
    };
    // Base64 is 4 characters for each 3 bytes.
    data.len() <= MAX_ICON_BYTES.div_ceil(3) * 4
        && STANDARD
            .decode(data)
            .is_ok_and(|bytes| bytes.len() <= MAX_ICON_BYTES && picture_bytes_ok(Picture::Icon, format, &bytes))
}

/// `secs` as a UTC date: `2026-11-02`.
fn date(secs: u64) -> String {
    let at = UNIX_EPOCH.checked_add(Duration::from_secs(secs)).map(utc);
    at.and_then(|t| t.split('T').next().map(str::to_owned)).unwrap_or_else(|| secs.to_string())
}

/// What [`check`] says of an index only a later AutoScene reads, so the
/// app can tell the person to update rather than that something failed.
pub fn needs_newer_app_said() -> Msg {
    msg!("catalog.needsNewerApp", "This catalog needs a newer AutoScene.")
}

pub fn unreadable_signature() -> Msg {
    msg!("catalog.unreadableSignature", "The catalog's signature can't be read.")
}

pub fn too_large() -> Msg {
    msg!("catalog.tooLarge", "The catalog is too large.")
}

fn unreadable_index() -> Msg {
    msg!("catalog.unreadable", "The catalog can't be read.")
}

/// An index only a later AutoScene reads. Not [`PluginError::TooNew`],
/// whose sentence is about a plugin.
fn needs_newer_app() -> PluginError {
    catalog(needs_newer_app_said())
}

fn not_offline() -> PluginError {
    catalog(msg!("catalog.notOffline", "The catalog's new epoch isn't signed by AutoScene's offline key."))
}

fn catalog(message: Msg) -> PluginError {
    PluginError::Catalog(message)
}

#[cfg(test)]
mod tests {
    use minisign::KeyPair;
    use serde_json::json;

    use super::*;
    use crate::keys::CATALOG_URLS;
    use crate::manifest::{self, Manifest};
    use crate::trust::{self, trusted_comment};

    const NOW: u64 = 1_800_000_000;
    const DAY: u64 = 86_400;
    const HELLO: &str = "dev.autoscene.hello-export";
    const ENTRY_REFUSED: &str = "The catalog has an entry AutoScene can't read: ";

    /// A minisign key pair, as `asp keygen` makes one.
    struct Key(KeyPair);

    impl Key {
        fn new() -> Self {
            Self(KeyPair::generate_unencrypted_keypair().unwrap())
        }

        /// The base64 line of its `.pub` file.
        fn public(&self) -> String {
            self.0.pk.to_base64()
        }

        fn id(&self) -> String {
            key_id(&self.public()).unwrap()
        }

        /// Its signature of `data`, with the trusted comment `comment`.
        fn sign(&self, data: &[u8], comment: &str) -> String {
            minisign::sign(Some(&self.0.pk), &self.0.sk, data, Some(comment), None).unwrap().into_string()
        }

        /// The index `index` as bytes, and its signature, as the catalog's
        /// CI makes them.
        fn sign_index(&self, index: &Value) -> (Vec<u8>, String) {
            let bytes = serde_json::to_vec(index).unwrap();
            let n = |key: &str| index[key].as_u64().unwrap();
            let sig = self.sign(&bytes, &index_comment(n("epoch"), n("seq"), n("expires")));
            (bytes, sig)
        }

        /// Its certificate of `epoch`, withdrawing the keys `withdraw` and
        /// naming `online` as online keys.
        fn certify(&self, epoch: u64, withdraw: &[&Key], online: &[&Key]) -> String {
            let withdraw: Vec<String> = withdraw.iter().map(|k| k.id()).collect();
            let online: Vec<String> = online.iter().map(|k| k.public()).collect();
            let line = certificate_line(epoch, &withdraw, &online);
            self.sign(line.as_bytes(), &line)
        }
    }

    /// The catalog's two keys.
    struct Owner {
        online: Key,
        offline: Key,
    }

    impl Owner {
        fn new() -> Self {
            Self { online: Key::new(), offline: Key::new() }
        }

        fn keys(&self) -> CatalogKeys {
            CatalogKeys::new(&[(KeyRole::Online, &self.online.public()), (KeyRole::Offline, &self.offline.public())])
                .unwrap()
        }
    }

    /// An index of `epoch` and `seq`, published a day ago and running out in
    /// 29 days, that lists Hello Export 1.0.0.
    fn index(epoch: u64, seq: u64) -> Value {
        json!({
            "catalogVersion": 1,
            "epoch": epoch,
            "seq": seq,
            "published": NOW - DAY,
            "expires": NOW + 29 * DAY,
            "plugins": [entry(HELLO, "1.0.0")],
            "withdrawn": [],
        })
    }

    /// [`index`], with the epoch certificate `certificate`.
    fn index_in(epoch: u64, seq: u64, certificate: &str) -> Value {
        let mut i = index(epoch, seq);
        i["epochCertificate"] = certificate.into();
        i
    }

    /// An entry as the catalog's CI writes it.
    fn entry(id: &str, version: &str) -> Value {
        json!({
            "id": id,
            "version": version,
            "lock": lock(version),
            "package": {
                "urls": [format!("https://github.com/tcmmi/autoscene-plugins/releases/download/{id}-{version}/{id}.asp")],
                "size": 4096,
                "sha256": "b".repeat(64),
            },
            "authorKey": null,
            "name": "Hello Export",
            "summary": "Says hello after an export.",
            "author": "AutoScene",
            "license": "MIT",
            "platforms": [],
            "runtime": "process",
            "gpu": false,
            "uvScript": true,
            "permissions": ["exports:read"],
            "hosts": [],
            "engines": ">=1.0.0, <2",
            "source": {
                "repo": "https://github.com/tcmmi/autoscene",
                "commit": "c".repeat(40),
                "path": "examples/plugins/hello-export",
            },
            "icon": icon_url("image/png", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x10\0\0\0\x10"),
            "approved": NOW - 2 * DAY,
        })
    }

    /// `sig` as a legacy minisign signature (`Ed`), which signs the data
    /// itself rather than its hash.
    fn legacy(sig: &str) -> String {
        let mut lines: Vec<String> = sig.lines().map(str::to_owned).collect();
        let mut bin = STANDARD.decode(&lines[1]).unwrap();
        assert_eq!(&bin[..2], b"ED");
        bin[..2].copy_from_slice(b"Ed");
        lines[1] = STANDARD.encode(bin);
        lines.join("\n") + "\n"
    }

    fn withdrawn(id: &str, versions: &str, reason: &str) -> Value {
        json!({"id": id, "versions": versions, "reason": reason, "since": NOW - DAY})
    }

    /// The SHA-256 of a `files.lock` of version `version`, in these tests.
    fn lock(version: &str) -> String {
        sha256_hex(version.as_bytes())
    }

    fn icon_url(mime: &str, bytes: &[u8]) -> String {
        format!("data:{mime};base64,{}", STANDARD.encode(bytes))
    }

    fn id(id: &str) -> PluginId {
        PluginId::parse(id).unwrap()
    }

    /// Checks `index`, signed by `key`, against `held` now.
    fn signed_by(key: &Key, index: &Value, keys: &CatalogKeys, held: &CatalogState) -> PluginResult<Checked> {
        let (bytes, sig) = key.sign_index(index);
        check(&bytes, &sig, keys, held, NOW)
    }

    /// The index `checked` took, and what AutoScene holds after it.
    fn taken(checked: PluginResult<Checked>, held: &CatalogState) -> (Accepted, CatalogState) {
        match checked {
            Ok(Checked::Newer(a)) => {
                let state = held.after(&a, NOW);
                (*a, state)
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_failed_try_is_kept_in_english_and_with_its_key() {
        let older = CatalogState::default().tried(5, Some(needs_newer_app_said()));
        assert_eq!(older.last_error.as_deref(), Some("This catalog needs a newer AutoScene."));
        assert_eq!(older.error().unwrap().key, "catalog.needsNewerApp");
        assert!(older.needs_newer_app());
        assert_eq!(older.tried(6, None).error(), None);
        // As an AutoScene before keys wrote it: its words, plain, and what they meant.
        let mut kept = serde_json::to_value(&older).unwrap();
        kept.as_object_mut().unwrap().remove("lastErrorMsg");
        let kept: CatalogState = serde_json::from_value(kept).unwrap();
        assert_eq!(kept.error(), Some(Msg::plain("This catalog needs a newer AutoScene.")));
        assert!(kept.needs_newer_app());
    }

    /// The sentence `checked` refuses the index with.
    fn refused(checked: PluginResult<Checked>) -> String {
        match checked {
            Err(PluginError::Catalog(why)) => why.text,
            other => panic!("{other:?}"),
        }
    }

    fn hello() -> Manifest {
        let json = json!({
            "manifestVersion": 1,
            "id": HELLO,
            "version": "1.0.0",
            "name": "Hello Export",
            "description": "Says hello after an export.",
            "author": {"name": "AutoScene"},
            "engines": {"autoscene": ">=1.0.0, <2", "protocol": 1},
            "runtime": {"type": "process", "command": {"*": ["uv", "run", "--script", "--locked", "main.py"]}}
        });
        manifest::parse_for_any_app(&serde_json::to_vec(&json).unwrap()).unwrap().manifest
    }

    #[test]
    fn an_index_signed_by_either_key_is_taken() {
        let owner = Owner::new();
        let held = CatalogState::default();
        for (key, role) in [(&owner.online, KeyRole::Online), (&owner.offline, KeyRole::Offline)] {
            let (a, state) = taken(signed_by(key, &index(1, 1), &owner.keys(), &held), &held);
            assert_eq!((a.role, &a.signed_by), (role, &key.id()));
            assert_eq!(a.index.plugins[0].id, id(HELLO));
            let expected = CatalogState {
                v: STATE_VERSION,
                epoch: 1,
                seq: 1,
                sha256: Some(a.sha256.clone()),
                published: NOW - DAY,
                expires: NOW + 29 * DAY,
                fetched_at: NOW,
                checked_at: NOW,
                withdrawn_keys: Vec::new(),
                withdrawn: Vec::new(),
                signed_by: Some(key.id()),
                last_error: None,
                last_error_msg: None,
            };
            assert_eq!(state, expected);
        }
        // The comments, in the one spelling the catalog's CI writes.
        assert_eq!(index_comment(1, 7, 99), "autoscene-catalog;v=1;epoch=1;seq=7;expires=99");
        let ids = ["B".to_owned(), "A".to_owned(), "B".to_owned()];
        assert_eq!(certificate_line(2, &ids, &[]), "autoscene-catalog-epoch;v=1;epoch=2;withdraw=A,B;online=-");
    }

    #[test]
    fn an_index_signed_by_another_key_is_refused() {
        let owner = Owner::new();
        let held = CatalogState::default();
        let unknown = "The catalog is signed with a key AutoScene doesn't know.";
        assert_eq!(refused(signed_by(&Key::new(), &index(1, 1), &owner.keys(), &held)), unknown);
        // A build with no catalog keys takes no index.
        let none = CatalogKeys::new(&[]).unwrap();
        assert!(none.is_empty());
        assert_eq!(refused(signed_by(&owner.online, &index(1, 1), &none, &held)), unknown);
        // A key the debug override adds signs like a compiled one.
        let extra = Key::new();
        let keys = owner.keys().with(&[(KeyRole::Online, &extra.public())]).unwrap();
        assert!(matches!(signed_by(&extra, &index(1, 1), &keys, &held), Ok(Checked::Newer(_))));
    }

    #[test]
    fn a_package_signature_never_passes_as_an_index_signature() {
        let owner = Owner::new();
        let held = CatalogState::default();
        let (bytes, _) = owner.online.sign_index(&index(1, 1));
        let not_one = "That isn't a catalog signature.";
        let package = owner.online.sign(&bytes, &trusted_comment(&id(HELLO), "1.0.0", &sha256_hex(&bytes)));
        assert_eq!(refused(check(&bytes, &package, &owner.keys(), &held, NOW)), not_one);
        // Nor does an epoch certificate.
        let certificate = owner.offline.certify(1, &[], &[]);
        assert_eq!(refused(check(&bytes, &certificate, &owner.keys(), &held, NOW)), not_one);
        // And the other way round: an index's signature is no package's, not
        // even with its key beside it as the package's author.pub.
        let lock = br#"{"lockVersion":1,"files":[]}"#;
        let sig = owner.online.sign(lock, &index_comment(1, 1, NOW + DAY));
        let judged = trust::judge(lock, Some(&sig), Some(&owner.online.public()), &hello(), None);
        assert!(matches!(judged, Err(PluginError::BadSignature(_))), "{judged:?}");
        // Nor is an index's signature an epoch certificate.
        let unreadable = "The catalog's epoch certificate can't be read.";
        let mut i = index_in(2, 1, &owner.offline.sign(b"x", &index_comment(2, 1, NOW + 29 * DAY)));
        assert_eq!(refused(signed_by(&owner.online, &i, &owner.keys(), &held)), unreadable);
        i["epochCertificate"] = "not a signature".into();
        assert_eq!(refused(signed_by(&owner.online, &i, &owner.keys(), &held)), unreadable);
    }

    #[test]
    fn a_changed_index_fails_its_signature() {
        let owner = Owner::new();
        let held = CatalogState::default();
        let (bytes, sig) = owner.online.sign_index(&index(1, 1));
        let changed = String::from_utf8(bytes).unwrap().replace("Hello Export", "Hello Exp0rt");
        assert_eq!(
            refused(check(changed.as_bytes(), &sig, &owner.keys(), &held, NOW)),
            "The catalog was changed after it was signed."
        );
        // So is a certificate that withdraws one key more than it was signed with.
        let certificate = owner.offline.certify(2, &[], &[]);
        let forged = certificate.replace("withdraw=-", &format!("withdraw={}", owner.online.id()));
        assert_ne!(forged, certificate);
        assert_eq!(
            refused(signed_by(&owner.offline, &index_in(2, 1, &forged), &owner.keys(), &held)),
            "The catalog's epoch certificate was changed after it was signed."
        );
        // Signatures that don't read.
        let no_trusted = sig.replace("trusted comment: ", "comment: ");
        for sig in ["", "untrusted comment: x\nnot base64\n", no_trusted.as_str()] {
            assert_eq!(
                refused(check(changed.as_bytes(), sig, &owner.keys(), &held, NOW)),
                "The catalog's signature can't be read."
            );
        }
        // Only prehashed signatures: a legacy one, the index's or its
        // certificate's, doesn't read.
        let (bytes, sig) = owner.online.sign_index(&index(1, 1));
        let unreadable = "The catalog's signature can't be read.";
        assert_eq!(refused(check(&bytes, &legacy(&sig), &owner.keys(), &held, NOW)), unreadable);
        let certificate = legacy(&owner.offline.certify(2, &[], &[&owner.online]));
        assert_eq!(
            refused(signed_by(&owner.online, &index_in(2, 1, &certificate), &owner.keys(), &held)),
            "The catalog's epoch certificate can't be read."
        );
    }

    #[test]
    fn an_index_or_signature_past_its_size_is_refused() {
        let owner = Owner::new();
        let held = CatalogState::default();
        let too_large = "The catalog is too large.";
        let comment = index_comment(1, 1, NOW + 29 * DAY);
        // The index padded with white space to its largest size, then a byte more.
        let (mut bytes, _) = owner.online.sign_index(&index(1, 1));
        bytes.resize(usize::try_from(MAX_INDEX_BYTES).unwrap(), b' ');
        let sig = owner.online.sign(&bytes, &comment);
        assert!(matches!(check(&bytes, &sig, &owner.keys(), &held, NOW), Ok(Checked::Newer(_))));
        bytes.push(b' ');
        let sig = owner.online.sign(&bytes, &comment);
        assert_eq!(refused(check(&bytes, &sig, &owner.keys(), &held, NOW)), too_large);
        let (bytes, sig) = owner.online.sign_index(&index(1, 1));
        let long = format!("{sig}{}", "\n".repeat(usize::try_from(MAX_SIG_BYTES).unwrap()));
        assert_eq!(refused(check(&bytes, &long, &owner.keys(), &held, NOW)), too_large);
    }

    #[test]
    fn the_entries_are_read_only_once_the_signature_checked_out() {
        let owner = Owner::new();
        let held = CatalogState::default();
        let mut i = index(1, 1);
        i["plugins"] = 5.into();
        // Nobody AutoScene knows signed it, so its entries aren't read.
        let unknown = "The catalog is signed with a key AutoScene doesn't know.";
        assert_eq!(refused(signed_by(&Key::new(), &i, &owner.keys(), &held)), unknown);
        assert_eq!(refused(signed_by(&owner.online, &i, &owner.keys(), &held)), "The catalog can't be read.");
    }

    #[test]
    fn the_signature_and_the_body_must_name_the_same_index() {
        let owner = Owner::new();
        let held = CatalogState::default();
        let bytes = serde_json::to_vec(&index(1, 2)).unwrap();
        let expires = NOW + 29 * DAY;
        let check_as = |comment: &str| check(&bytes, &owner.online.sign(&bytes, comment), &owner.keys(), &held, NOW);
        for comment in [index_comment(2, 2, expires), index_comment(1, 3, expires), index_comment(1, 2, expires + 1)] {
            assert_eq!(refused(check_as(&comment)), "The catalog's signature names another catalog.", "{comment}");
        }
        // Its one spelling: decimal, no leading zeros or signs, nothing after the expiry.
        for comment in [
            format!("{INDEX_TAG}epoch=01;seq=2;expires={expires}"),
            format!("{INDEX_TAG}epoch=+1;seq=2;expires={expires}"),
            format!("{INDEX_TAG}seq=2;epoch=1;expires={expires}"),
            format!("{};next=1", index_comment(1, 2, expires)),
            "autoscene-catalog;v=1;epoch=1;seq=2".to_owned(),
        ] {
            assert_eq!(refused(check_as(&comment)), "That isn't a catalog signature.", "{comment}");
        }
        assert!(matches!(check_as(&index_comment(1, 2, expires)), Ok(Checked::Newer(_))));
    }

    #[test]
    fn an_older_index_is_refused() {
        let owner = Owner::new();
        let keys = owner.keys();
        let empty = CatalogState::default();
        let older = "This catalog is older than the one AutoScene has.";
        let (_, held) = taken(signed_by(&owner.online, &index(1, 5), &keys, &empty), &empty);
        assert_eq!(refused(signed_by(&owner.online, &index(1, 4), &keys, &held)), older);
        assert!(matches!(signed_by(&owner.online, &index(1, 6), &keys, &held), Ok(Checked::Newer(_))));
        // After a new epoch, the old one is older, whatever its number.
        let certificate = owner.offline.certify(2, &[], &[&owner.online]);
        let (_, epoch_2) = taken(signed_by(&owner.online, &index_in(2, 1, &certificate), &keys, &held), &held);
        assert_eq!(refused(signed_by(&owner.online, &index(1, 9), &keys, &epoch_2)), older);
        assert_eq!(refused(signed_by(&owner.offline, &index(1, u64::MAX), &keys, &epoch_2)), older);
    }

    #[test]
    fn the_same_number_with_other_bytes_is_refused() {
        let owner = Owner::new();
        let keys = owner.keys();
        let empty = CatalogState::default();
        let (_, held) = taken(signed_by(&owner.online, &index(1, 5), &keys, &empty), &empty);
        let mut other = index(1, 5);
        other["plugins"] = json!([]);
        let twice = "Two catalogs carry the same number.";
        assert_eq!(refused(signed_by(&owner.online, &other, &keys, &held)), twice);
        // Also when the other key signed it.
        assert_eq!(refused(signed_by(&owner.offline, &other, &keys, &held)), twice);
    }

    #[test]
    fn the_same_index_again_is_no_change() {
        let owner = Owner::new();
        let keys = owner.keys();
        let empty = CatalogState::default();
        let (bytes, sig) = owner.online.sign_index(&index(1, 5));
        let (_, held) = taken(check(&bytes, &sig, &keys, &empty, NOW), &empty);
        assert!(matches!(check(&bytes, &sig, &keys, &held, NOW + DAY), Ok(Checked::Same)));
        // A try changes only when it was, and what went wrong.
        let tried = held.tried(NOW + DAY, Some(Msg::plain("No answer.")));
        assert_eq!(
            tried,
            CatalogState {
                checked_at: NOW + DAY,
                last_error: Some("No answer.".into()),
                last_error_msg: Some(Msg::plain("No answer.")),
                ..held.clone()
            }
        );
        // The saved index reads again, also after it ran out; another file doesn't.
        let saved = reload(&bytes, &sig, &keys, &held).unwrap();
        assert_eq!((saved.epoch, saved.seq, saved.plugins.len()), (1, 5, 1));
        let (other, other_sig) = owner.online.sign_index(&index(1, 6));
        let lost = reload(&other, &other_sig, &keys, &held).unwrap_err();
        assert_eq!(lost.to_string(), "The saved catalog isn't the one AutoScene checked.");
        let unsigned = reload(&bytes, &other_sig, &keys, &held).unwrap_err();
        assert_eq!(unsigned.to_string(), "The catalog's signature names another catalog.");
        let none = reload(&bytes, &sig, &CatalogKeys::new(&[]).unwrap(), &held).unwrap_err();
        assert_eq!(none.to_string(), "The catalog is signed with a key AutoScene doesn't know.");
    }

    #[test]
    fn an_expired_index_is_refused_and_so_is_one_that_lives_too_long() {
        let owner = Owner::new();
        let held = CatalogState::default();
        let dated = |published: u64, expires: u64| {
            let mut i = index(1, 1);
            i["published"] = published.into();
            i["expires"] = expires.into();
            i["plugins"][0]["approved"] = published.into();
            signed_by(&owner.online, &i, &owner.keys(), &held)
        };
        assert_eq!(refused(dated(NOW - 30 * DAY, NOW)), "The catalog ran out on 2027-01-15.");
        assert!(matches!(dated(NOW - 30 * DAY, NOW + 1), Ok(Checked::Newer(_))));
        // 31 days at most, and never less than none.
        assert!(matches!(dated(NOW - DAY, NOW - DAY + MAX_LIFETIME), Ok(Checked::Newer(_))));
        assert_eq!(refused(dated(NOW - DAY, NOW - DAY + MAX_LIFETIME + 1)), "The catalog's dates can't be right.");
        assert_eq!(refused(dated(NOW + 2 * DAY, NOW + DAY)), "The catalog's dates can't be right.");
        // A publish date a little in the future is taken: the clock may be behind.
        assert!(matches!(dated(NOW + DAY, NOW + 2 * DAY), Ok(Checked::Newer(_))));
        let ahead = NOW + MAX_CLOCK_SKEW;
        assert!(matches!(dated(ahead, ahead + 30 * DAY), Ok(Checked::Newer(_))));
        // Further ahead, it would live past 31 days from now.
        let dated_ahead = "The catalog is dated 2027-01-18, which hasn't come yet. Check this computer's clock.";
        assert_eq!(refused(dated(ahead + 1, ahead + 30 * DAY)), dated_ahead);
        let years = NOW + 10 * 365 * DAY;
        assert!(refused(dated(years, years + 30 * DAY)).starts_with("The catalog is dated 2037-"));
    }

    #[test]
    fn only_the_offline_key_starts_an_epoch() {
        let owner = Owner::new();
        let keys = owner.keys();
        let held = CatalogState::default();
        let not_offline = "The catalog's new epoch isn't signed by AutoScene's offline key.";
        assert_eq!(refused(signed_by(&owner.offline, &index(2, 1), &keys, &held)), not_offline);
        let by_online = owner.online.certify(2, &[], &[]);
        assert_eq!(refused(signed_by(&owner.online, &index_in(2, 1, &by_online), &keys, &held)), not_offline);
        let by_offline = owner.offline.certify(2, &[], &[&owner.online]);
        let (a, _) = taken(signed_by(&owner.online, &index_in(2, 1, &by_offline), &keys, &held), &held);
        assert_eq!((a.index.epoch, a.role), (2, KeyRole::Online));
        assert_eq!(a.index.epoch_certificate.as_deref(), Some(by_offline.as_str()));
        // A certificate in the first epoch is checked the same way.
        let first = owner.online.certify(1, &[], &[]);
        assert_eq!(refused(signed_by(&owner.online, &index_in(1, 1, &first), &keys, &held)), not_offline);
    }

    #[test]
    fn a_fresh_install_takes_no_epoch_below_the_minimum() {
        let owner = Owner::new();
        let keys = owner.keys();
        let empty = CatalogState::default();
        let older = "This catalog is older than the one AutoScene has.";
        assert_eq!(refused(signed_by(&owner.online, &index(0, 1), &keys, &empty)), older);
        // Number 0 of the lowest epoch is new to it, as `asp catalog new` writes it.
        let (a, held) = taken(signed_by(&owner.online, &index(1, 0), &keys, &empty), &empty);
        assert_eq!((a.index.seq, held.epoch, held.seq), (0, 1, 0));
        assert!(matches!(signed_by(&owner.online, &index(1, 0), &keys, &held), Ok(Checked::Same)));
        assert!(matches!(signed_by(&owner.online, &index(1, 1), &keys, &held), Ok(Checked::Newer(_))));
        // A build whose minimum is 2 takes no epoch 1 index, not even on its
        // first fetch, nor one newer than the epoch 1 index it held before.
        let check_at_2 = |index: &Value, held: &CatalogState| {
            let (bytes, sig) = owner.online.sign_index(index);
            check_from(&bytes, &sig, &keys, held, NOW, 2)
        };
        assert_eq!(refused(check_at_2(&index(1, 9), &empty)), older);
        let (_, held) = taken(signed_by(&owner.online, &index(1, 5), &keys, &empty), &empty);
        assert_eq!(refused(check_at_2(&index(1, 6), &held)), older);
        let certificate = owner.offline.certify(2, &[], &[&owner.online]);
        assert!(matches!(check_at_2(&index_in(2, 1, &certificate), &empty), Ok(Checked::Newer(_))));
    }

    #[test]
    fn a_new_epoch_starts_its_count_again_and_withdraws_keys_for_good() {
        let owner = Owner::new();
        let keys = owner.keys();
        let empty = CatalogState::default();
        let (_, held) = taken(signed_by(&owner.online, &index(1, 50), &keys, &empty), &empty);
        // The online key leaked: the owner starts epoch 2 without it, at 1.
        let epoch_2 = owner.offline.certify(2, &[&owner.online], &[]);
        let (a, held_2) = taken(signed_by(&owner.offline, &index_in(2, 1, &epoch_2), &keys, &held), &held);
        assert_eq!(a.withdrawn_keys, [owner.online.id()]);
        assert_eq!((held_2.epoch, held_2.seq, &held_2.withdrawn_keys), (2, 1, &vec![owner.online.id()]));
        let withdrew = "The catalog is signed with a key AutoScene withdrew.";
        assert_eq!(refused(signed_by(&owner.online, &index_in(2, 2, &epoch_2), &keys, &held_2)), withdrew);
        // The highest number of the old epoch doesn't help it.
        assert_eq!(refused(signed_by(&owner.online, &index(1, u64::MAX), &keys, &held_2)), withdrew);
        // Later epochs that don't withdraw it again, or name it as an online
        // key, don't bring it back.
        let epoch_3 = owner.offline.certify(3, &[], &[&owner.online]);
        assert_eq!(refused(signed_by(&owner.online, &index_in(3, 1, &epoch_3), &keys, &held_2)), withdrew);
        let (_, held_3) = taken(signed_by(&owner.offline, &index_in(3, 1, &epoch_3), &keys, &held_2), &held_2);
        assert_eq!(held_3.withdrawn_keys, [owner.online.id()]);
        // The state never forgets a withdrawn key, whatever it takes.
        let (fresh, _) = taken(signed_by(&owner.offline, &index_in(3, 1, &epoch_3), &keys, &empty), &empty);
        assert!(fresh.withdrawn_keys.is_empty());
        assert_eq!(held_2.after(&fresh, NOW).withdrawn_keys, [owner.online.id()]);
        // A certificate that names no online key leaves the offline key alone to sign.
        let offline_only = owner.offline.certify(3, &[], &[]);
        assert_eq!(refused(signed_by(&owner.online, &index_in(3, 1, &offline_only), &keys, &empty)), withdrew);
        let by_offline = signed_by(&owner.offline, &index_in(3, 1, &offline_only), &keys, &empty);
        assert!(matches!(by_offline, Ok(Checked::Newer(_))));
        // A withdrawn offline key starts no epoch.
        let gone = CatalogState { withdrawn_keys: vec![owner.offline.id()], ..held_3 };
        let epoch_4 = owner.offline.certify(4, &[], &[]);
        assert_eq!(
            refused(signed_by(&owner.offline, &index_in(4, 1, &epoch_4), &keys, &gone)),
            "The catalog's new epoch isn't signed by AutoScene's offline key."
        );
    }

    #[test]
    fn an_autoscene_that_skipped_the_epoch_that_withdrew_a_key_never_takes_it() {
        let owner = Owner::new();
        let keys = owner.keys();
        let empty = CatalogState::default();
        let (second, third) = (Key::new(), Key::new());
        // Epoch 2 withdrew the compiled online key and named another; epoch 3
        // withdraws that one, names a third, and leaves the compiled one out.
        let epoch_2 = owner.offline.certify(2, &[&owner.online], &[&second]);
        let epoch_3 = owner.offline.certify(3, &[&second], &[&third]);
        let withdrew = "The catalog is signed with a key AutoScene withdrew.";
        // A fresh install, or one offline through epoch 2, never saw epoch 2.
        let (_, in_epoch_1) = taken(signed_by(&owner.online, &index(1, 9), &keys, &empty), &empty);
        for held in [empty.clone(), in_epoch_1] {
            let leaked = signed_by(&owner.online, &index_in(3, u64::MAX, &epoch_3), &keys, &held);
            assert_eq!(refused(leaked), withdrew);
            assert_eq!(refused(signed_by(&second, &index_in(3, u64::MAX, &epoch_3), &keys, &held)), withdrew);
            let (a, after) = taken(signed_by(&third, &index_in(3, 7, &epoch_3), &keys, &held), &held);
            assert_eq!((a.role, after.withdrawn_keys), (KeyRole::Online, vec![second.id()]));
        }
        // One that took epoch 2 holds both withdrawals.
        let (_, held_2) = taken(signed_by(&second, &index_in(2, 1, &epoch_2), &keys, &empty), &empty);
        let (_, held_3) = taken(signed_by(&third, &index_in(3, 1, &epoch_3), &keys, &held_2), &held_2);
        let mut both = vec![owner.online.id(), second.id()];
        both.sort_unstable();
        assert_eq!(held_3.withdrawn_keys, both);
    }

    #[test]
    fn a_certificate_never_withdraws_an_offline_key() {
        let owner = Owner::new();
        let keys = owner.keys();
        let empty = CatalogState::default();
        let withdraws_offline = "The catalog's epoch certificate withdraws AutoScene's offline key.";
        let own = owner.offline.certify(2, &[&owner.offline], &[&owner.online]);
        for key in [&owner.offline, &owner.online] {
            assert_eq!(refused(signed_by(key, &index_in(2, 1, &own), &keys, &empty)), withdraws_offline);
        }
        // Nor another offline key AutoScene has: only a release changes them.
        let other = Key::new();
        let keys = keys.with(&[(KeyRole::Offline, &other.public())]).unwrap();
        let shuts_out = owner.offline.certify(2, &[&other], &[]);
        assert_eq!(refused(signed_by(&owner.offline, &index_in(2, 1, &shuts_out), &keys, &empty)), withdraws_offline);
        // A certificate's withdraw list is key ids in their one spelling.
        let lower = certificate_line(2, &[owner.online.id().to_lowercase()], &[]);
        let lower = owner.offline.sign(lower.as_bytes(), &lower);
        assert_eq!(
            refused(signed_by(&owner.offline, &index_in(2, 1, &lower), &keys, &empty)),
            "The catalog's epoch certificate can't be read."
        );
    }

    #[test]
    fn a_key_the_certificate_names_signs_in_its_epoch_only() {
        let owner = Owner::new();
        let keys = owner.keys();
        let empty = CatalogState::default();
        let next = Key::new();
        let epoch_2 = owner.offline.certify(2, &[], &[&next]);
        let (a, held) = taken(signed_by(&next, &index_in(2, 1, &epoch_2), &keys, &empty), &empty);
        assert_eq!((a.role, &a.signed_by), (KeyRole::Online, &next.id()));
        let unknown = "The catalog is signed with a key AutoScene doesn't know.";
        let epoch_3 = owner.offline.certify(3, &[], &[]);
        assert_eq!(refused(signed_by(&next, &index_in(3, 1, &epoch_3), &keys, &held)), unknown);
        assert_eq!(refused(signed_by(&next, &index(1, 2), &keys, &empty)), unknown);
        // Epoch 2's certificate is no certificate of epoch 3.
        assert_eq!(
            refused(signed_by(&next, &index_in(3, 1, &epoch_2), &keys, &held)),
            "The catalog's epoch certificate is for another epoch."
        );
        // A key a certificate names can't start an epoch of its own.
        let own = next.certify(3, &[], &[&next]);
        assert_eq!(
            refused(signed_by(&next, &index_in(3, 1, &own), &keys, &held)),
            "The catalog's new epoch isn't signed by AutoScene's offline key."
        );
    }

    #[test]
    fn a_newer_catalog_version_needs_a_newer_autoscene() {
        let owner = Owner::new();
        let keys = owner.keys();
        let held = CatalogState::default();
        let mut i = index(1, 1);
        i["catalogVersion"] = 2.into();
        let newer = "This catalog needs a newer AutoScene.";
        assert_eq!(refused(signed_by(&owner.online, &i, &keys, &held)), newer);
        // Also from a key nobody knows: it is read before any signature.
        assert_eq!(refused(signed_by(&Key::new(), &i, &keys, &held)), newer);
        let bytes = serde_json::to_vec(&index(1, 1)).unwrap();
        let sig = owner.online.sign(&bytes, &format!("autoscene-catalog;v=2;epoch=1;seq=1;expires={}", NOW + 29 * DAY));
        assert_eq!(check(&bytes, &sig, &keys, &held, NOW).unwrap_err().to_string(), newer);
        // Version 0, or none, or no object at all, is no index.
        let unreadable = "The catalog can't be read.";
        for version in [json!(0), json!("1"), Value::Null] {
            i["catalogVersion"] = version;
            assert_eq!(refused(signed_by(&owner.online, &i, &keys, &held)), unreadable);
        }
        i.as_object_mut().unwrap().remove("catalogVersion");
        assert_eq!(refused(signed_by(&owner.online, &i, &keys, &held)), unreadable);
        let sig = owner.online.sign(b"[1]", &index_comment(1, 1, NOW + DAY));
        assert_eq!(refused(check(b"[1]", &sig, &keys, &held, NOW)), unreadable);
    }

    #[test]
    fn an_entry_that_breaks_its_rules_refuses_the_index() {
        let owner = Owner::new();
        let held = CatalogState::default();
        let check_with = |plugins: Value, withdrawn: Value| {
            let mut i = index(1, 1);
            i["plugins"] = plugins;
            i["withdrawn"] = withdrawn;
            signed_by(&owner.online, &i, &owner.keys(), &held)
        };
        type Break = (&'static str, fn(&mut Value));
        let breaks: &[Break] = &[
            ("a bad id", |e| e["id"] = "Dev.Hello".into()),
            ("no id", |e| e["id"] = Value::Null),
            ("bad semver", |e| e["version"] = "1.0".into()),
            ("an uppercase lock", |e| e["lock"] = "A".repeat(64).into()),
            ("a short package hash", |e| e["package"]["sha256"] = "b".repeat(63).into()),
            ("an http URL", |e| e["package"]["urls"] = json!(["http://example.com/hello.asp"])),
            ("no URL", |e| e["package"]["urls"] = json!([])),
            ("five URLs", |e| e["package"]["urls"] = json!(vec!["https://a.example/x.asp"; 5])),
            ("a URL with a space", |e| e["package"]["urls"] = json!(["https://a.example/x y.asp"])),
            ("a URL past its length", |e| {
                e["package"]["urls"] = json!([format!("https://a.example/{}", "x".repeat(MAX_URL_CHARS))]);
            }),
            ("an empty package", |e| e["package"]["size"] = 0.into()),
            ("a package past the limit", |e| e["package"]["size"] = (MAX_PACKAGE_BYTES + 1).into()),
            ("an author key that isn't one", |e| e["authorKey"] = "RWQ".into()),
            ("a name that is blank", |e| e["name"] = "\u{200B} \u{2800}".into()),
            ("no author", |e| e["author"] = "".into()),
            ("a permission that can't be one", |e| e["permissions"] = json!(["Exports:Read"])),
            ("17 permissions", |e| e["permissions"] = json!(vec!["exports:read"; 17])),
            ("a permission past 32 characters", |e| e["permissions"] = json!(["a".repeat(33)])),
            ("17 hosts", |e| e["hosts"] = json!(vec!["discord.com"; 17])),
            ("a platform that can't be one", |e| e["platforms"] = json!(["Windows x64"])),
            ("17 platforms", |e| e["platforms"] = json!(vec!["windows-x86_64"; 17])),
            ("a runtime past 16 characters", |e| e["runtime"] = "a".repeat(17).into()),
            ("a runtime with a hidden character", |e| e["runtime"] = "process\u{202E}".into()),
            ("a URL with a control character", |e| e["package"]["urls"] = json!(["https://a.example/x\u{7}.asp"])),
            ("a host that can't be one", |e| e["hosts"] = json!(["https://discord.com"])),
            ("resources past 8 GiB", |e| e["resourceBytes"] = (MAX_RESOURCES_BYTES + 1).into()),
            ("49 resource hosts", |e| e["resourceHosts"] = json!(vec!["huggingface.co"; 49])),
            ("a resource host on this computer", |e| e["resourceHosts"] = json!(["localhost"])),
            ("a resource host that is an address", |e| e["resourceHosts"] = json!(["127.0.0.1"])),
            ("unreadable engines", |e| e["engines"] = ">=1.0.0 <2".into()),
            ("a source outside github.com", |e| e["source"]["repo"] = "https://gitlab.com/tcmmi/autoscene".into()),
            ("a source a folder up", |e| e["source"]["repo"] = "https://github.com/tcmmi/..".into()),
            ("a source deeper", |e| e["source"]["repo"] = "https://github.com/tcmmi/autoscene/tree".into()),
            ("a short commit", |e| e["source"]["commit"] = "c".repeat(39).into()),
            ("a path out of the repository", |e| e["source"]["path"] = "../x".into()),
            ("an icon that is no PNG or SVG", |e| e["icon"] = icon_url("image/jpeg", b"\xFF\xD8\xFF\xE0").into()),
            ("an icon that isn't its format", |e| e["icon"] = icon_url("image/png", b"GIF89a").into()),
            ("an icon that isn't base64", |e| e["icon"] = "data:image/png;base64,***".into()),
            ("an icon past 8 KB", |e| {
                let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
                png.resize(MAX_ICON_BYTES + 1, 0);
                e["icon"] = icon_url("image/png", &png).into();
            }),
            ("an approval after the index", |e| e["approved"] = NOW.into()),
            ("no engines", |e| _ = e.as_object_mut().unwrap().remove("engines")),
        ];
        for (what, broken) in breaks {
            let mut e = entry(HELLO, "1.0.0");
            broken(&mut e);
            let why = refused(check_with(json!([e]), json!([])));
            assert!(why.starts_with(ENTRY_REFUSED), "{what}: {why}");
        }
        // The sentence names the entry.
        let mut e = entry(HELLO, "1.0.0");
        e["lock"] = "x".into();
        assert_eq!(refused(check_with(json!([e]), json!([]))), format!("{ENTRY_REFUSED}{HELLO} 1.0.0."));
        assert_eq!(refused(check_with(json!([{}]), json!([]))), format!("{ENTRY_REFUSED}number 1."));
        // An id and version listed twice, also with other build metadata.
        for again in ["1.0.0", "1.0.0+build.2"] {
            let twice = json!([entry(HELLO, "1.0.0"), entry(HELLO, again)]);
            assert_eq!(refused(check_with(twice, json!([]))), format!("{ENTRY_REFUSED}{HELLO} {again}."), "{again}");
        }
        let many = Value::Array(vec![json!({}); MAX_ENTRIES + 1]);
        assert_eq!(refused(check_with(many, json!([]))), "The catalog lists more than 4096 plugins.");
        // And a withdrawal that breaks its rules.
        for (what, w) in [
            ("a bad id", withdrawn("Dev.Hello", "*", "It deletes exports.")),
            ("unreadable versions", withdrawn(HELLO, ">=1 <2", "It deletes exports.")),
            ("no reason", withdrawn(HELLO, "*", " \u{200B}")),
            ("a reason past 200 characters", withdrawn(HELLO, "*", &"x".repeat(MAX_REASON_CHARS + 1))),
            ("a withdrawal after the index", json!({"id": HELLO, "versions": "*", "reason": "No.", "since": NOW})),
        ] {
            let why = refused(check_with(json!([entry(HELLO, "1.0.0")]), json!([w])));
            assert!(why.starts_with("The catalog has a withdrawal AutoScene can't read: "), "{what}: {why}");
        }
        let many = Value::Array(vec![json!({}); MAX_WITHDRAWN + 1]);
        assert_eq!(refused(check_with(json!([]), many)), "The catalog withdraws more than 1024 plugins.");
        // What passes: a pre-release, an SVG icon, an author key, a host, the repository's top.
        let mut e = entry(HELLO, "2.0.0-beta.1");
        e["icon"] = icon_url("image/svg+xml", b"<svg xmlns='http://www.w3.org/2000/svg'/>").into();
        e["authorKey"] = owner.online.public().into();
        e["source"]["path"] = ".".into();
        e["hosts"] = json!(["discord.com"]);
        e["resourceBytes"] = MAX_RESOURCES_BYTES.into();
        e["resourceHosts"] = json!(["huggingface.co", "cas-bridge.xethub.hf.co"]);
        e["permissions"] = json!(["a".repeat(32)]);
        e["platforms"] = json!(["windows-x86_64", "freebsd-x86_64"]);
        assert!(matches!(check_with(json!([e]), json!([])), Ok(Checked::Newer(_))));
    }

    #[test]
    fn unknown_fields_are_ignored_and_unknown_permissions_kept() {
        let owner = Owner::new();
        let held = CatalogState::default();
        let mut i = index(1, 1);
        i["mirrors"] = json!(["https://example.com/"]);
        i["plugins"][0]["screenshots"] = json!(["shot.png"]);
        i["plugins"][0]["permissions"] = json!(["exports:read", "camera", "clips:write"]);
        i["plugins"][0]["runtime"] = "wasm".into();
        let mut w = withdrawn("dev.a.b", "*", "It sends exports away.");
        w["severity"] = "high".into();
        i["withdrawn"] = json!([w]);
        let (a, state) = taken(signed_by(&owner.online, &i, &owner.keys(), &held), &held);
        let e = &a.index.plugins[0];
        assert_eq!(e.permissions, ["exports:read", "camera", "clips:write"]);
        assert_eq!(e.runtime, "wasm");
        let reason = "It sends exports away.".to_owned();
        assert_eq!(state.withdrawn, [Withdrawal { id: id("dev.a.b"), versions: "*".into(), reason, since: NOW - DAY }]);
        // What an entry may leave out.
        let mut i = index(1, 2);
        let optional = ["authorKey", "summary", "license", "platforms", "runtime", "gpu", "uvScript", "permissions"];
        for key in optional.into_iter().chain(["hosts", "icon"]) {
            i["plugins"][0].as_object_mut().unwrap().remove(key);
        }
        i["plugins"][0]["source"].as_object_mut().unwrap().remove("path");
        i.as_object_mut().unwrap().remove("withdrawn");
        let (a, _) = taken(signed_by(&owner.online, &i, &owner.keys(), &held), &held);
        let e = &a.index.plugins[0];
        assert_eq!((e.author_key.as_deref(), e.summary.as_str(), e.license.as_deref()), (None, "", None));
        assert!(e.platforms.is_empty() && e.permissions.is_empty() && e.hosts.is_empty() && e.icon.is_none());
        assert!(e.resource_bytes == 0 && e.resource_hosts.is_empty());
        assert_eq!((e.runtime.as_str(), e.gpu, e.uv_script, e.source.path.as_str()), ("", false, false, ""));
        assert!(a.index.withdrawn.is_empty());
    }

    #[test]
    fn withdrawals_match_versions_with_and_without_pre_releases() {
        let hello = id(HELLO);
        let held = |w: &[Value]| {
            let withdrawn = w.iter().map(|w| serde_json::from_value(w.clone()).unwrap()).collect();
            Catalog::new(None, CatalogState { withdrawn, ..CatalogState::default() })
        };
        let c =
            held(&[withdrawn(HELLO, ">=1.2.0, <1.3.0", "It deletes exports."), withdrawn(HELLO, "=2.0.0-rc.1", "No.")]);
        let out = |v: &str| c.withdrawal(&hello, v).is_some();
        assert!(out("1.2.0") && out("1.2.9"));
        // With its pre-release cleared: semver alone leaves it out.
        assert!(out("1.2.5-beta.2"));
        // As semver reads it.
        assert!(out("2.0.0-rc.1"));
        assert!(!out("1.1.9") && !out("1.3.0") && !out("2.0.0") && !out("2.0.0-rc.2"));
        // By precedence: 1.3.0's pre-releases come before the fix.
        assert!(out("1.3.0-beta"));
        let before = held(&[withdrawn(HELLO, "<1.3.0", "It deletes exports.")]);
        let before = |v: &str| before.withdrawal(&hello, v).is_some();
        assert!(before("1.3.0-rc.1") && before("1.2.9") && before("0.1.0-alpha"));
        assert!(!before("1.3.0") && !before("1.3.1-rc.1") && !before(""));
        assert_eq!(c.withdrawal(&hello, "1.2.3").map(|w| w.reason.as_str()), Some("It deletes exports."));
        assert!(c.withdrawal(&id("dev.a.b"), "1.2.0").is_none());
        // `*` is every version, pre-releases too.
        let all = held(&[withdrawn(HELLO, "*", "No.")]);
        assert!(all.withdrawal(&hello, "0.1.0-alpha").is_some() && all.withdrawal(&hello, "9.0.0").is_some());
        // And a version that doesn't read, as a broken record has.
        assert!(all.withdrawal(&hello, "").is_some());
        // A requirement that doesn't read (a row written by hand) withdraws.
        let unreadable = held(&[withdrawn(HELLO, ">=1 <2", "No.")]);
        assert!(unreadable.withdrawal(&hello, "3.0.0").is_some() && unreadable.withdrawal(&hello, "").is_some());
    }

    #[test]
    fn approval_needs_a_living_index_but_withdrawal_does_not() {
        let owner = Owner::new();
        let empty = CatalogState::default();
        let mut i = index(1, 1);
        i["plugins"] = json!([entry(HELLO, "1.0.0"), entry(HELLO, "1.1.0")]);
        i["withdrawn"] = json!([withdrawn(HELLO, "=1.1.0", "It deletes exports.")]);
        let (a, state) = taken(signed_by(&owner.online, &i, &owner.keys(), &empty), &empty);
        let c = Catalog::new(Some(a.index), state.clone());
        let hello = id(HELLO);
        assert!(c.living(NOW));
        assert_eq!(c.approval(&hello, "1.0.0", &lock("1.0.0"), NOW).map(|e| e.version.as_str()), Some("1.0.0"));
        // Another lock, version or plugin isn't what the catalog reviewed.
        assert!(c.approval(&hello, "1.0.0", &lock("1.1.0"), NOW).is_none());
        assert!(c.approval(&hello, "1.0.1", &lock("1.0.0"), NOW).is_none());
        assert!(c.approval(&hello, "1.0.0+other", &lock("1.0.0"), NOW).is_none());
        assert!(c.approval(&id("dev.a.b"), "1.0.0", &lock("1.0.0"), NOW).is_none());
        // A withdrawn version is approved no more.
        assert!(c.approval(&hello, "1.1.0", &lock("1.1.0"), NOW).is_none());
        assert_eq!(c.withdrawal(&hello, "1.1.0").map(|w| w.reason.as_str()), Some("It deletes exports."));
        // Once the index ran out, nothing is approved, and the withdrawal holds.
        let expiry = c.index().unwrap().expires;
        assert!(!c.living(expiry));
        assert!(c.approval(&hello, "1.0.0", &lock("1.0.0"), expiry).is_none());
        assert!(c.withdrawal(&hello, "1.1.0").is_some());
        // So it does when the index's file is lost.
        let lost = Catalog::new(None, state);
        assert!(!lost.living(NOW) && lost.approval(&hello, "1.0.0", &lock("1.0.0"), NOW).is_none());
        assert!(lost.withdrawal(&hello, "1.1.0").is_some());
        assert!(Catalog::none().withdrawal(&hello, "1.1.0").is_none() && Catalog::none().ids().is_empty());
    }

    #[test]
    fn the_newest_version_for_this_app_and_computer_is_offered() {
        let owner = Owner::new();
        let empty = CatalogState::default();
        let made = |id: &str, version: &str, engines: &str, platforms: Value| {
            let mut e = entry(id, version);
            e["engines"] = engines.into();
            e["platforms"] = platforms;
            e
        };
        let mut i = index(1, 1);
        i["plugins"] = json!([
            made(HELLO, "1.0.0", ">=1.0.0, <2", json!([])),
            made(HELLO, "2.0.0", ">=2.0.0", json!([])),
            made(HELLO, "1.2.0", ">=1.0.0, <2", json!([])),
            made(HELLO, "1.1.0", ">=1.0.0, <2", json!(["linux-x86_64"])),
            made("dev.a.later", "3.0.0", ">=3.0.0", json!([])),
            made("dev.a.mac", "1.0.0", ">=1.0.0", json!(["macos-aarch64"])),
            made("dev.a.old", "0.9.0", "<1.0.0", json!([])),
            made("dev.a.gone", "1.0.0", ">=1.0.0", json!([])),
            made("dev.a.win", "1.0.0", ">=1.0.0", json!(["windows"])),
        ]);
        i["withdrawn"] = json!([withdrawn(HELLO, "=1.2.0", "No."), withdrawn("dev.a.gone", "*", "No.")]);
        let (a, state) = taken(signed_by(&owner.online, &i, &owner.keys(), &empty), &empty);
        let c = Catalog::new(Some(a.index), state);
        let beta = Version::parse("1.0.3-BETA").unwrap();
        let offered = |of: &str, app: &Version, platform: &str| match c.offer(&id(of), app, platform) {
            Some(Offer::Here(e)) => format!("here {}", e.version),
            Some(Offer::NeedsNewerApp(e)) => format!("newer app {}", e.version),
            Some(Offer::NotHere(e)) => format!("not here {}", e.version),
            None => "none".into(),
        };
        // 2.0.0 needs a newer AutoScene, 1.2.0 is withdrawn, 1.1.0 is for Linux.
        assert_eq!(offered(HELLO, &beta, "windows-x86_64"), "here 1.0.0");
        assert_eq!(offered(HELLO, &beta, "linux-x86_64"), "here 1.1.0");
        assert_eq!(offered(HELLO, &Version::new(2, 1, 0), "windows-x86_64"), "here 2.0.0");
        assert_eq!(offered("dev.a.later", &beta, "windows-x86_64"), "newer app 3.0.0");
        assert_eq!(offered("dev.a.mac", &beta, "windows-x86_64"), "not here 1.0.0");
        assert_eq!(offered("dev.a.mac", &beta, "macos-aarch64"), "here 1.0.0");
        assert_eq!(offered("dev.a.old", &beta, "windows-x86_64"), "not here 0.9.0");
        assert_eq!(offered("dev.a.gone", &beta, "windows-x86_64"), "none");
        assert_eq!(offered("dev.a.unknown", &beta, "windows-x86_64"), "none");
        // A platform is named whole.
        assert_eq!(offered("dev.a.win", &beta, "windows-x86_64"), "not here 1.0.0");
        let ids: Vec<&str> = c.ids().into_iter().map(PluginId::as_str).collect();
        assert_eq!(ids, ["dev.a.gone", "dev.a.later", "dev.a.mac", "dev.a.old", "dev.a.win", HELLO]);
    }

    #[test]
    fn texts_from_the_catalog_are_cleaned_and_cut() {
        let owner = Owner::new();
        let held = CatalogState::default();
        let mut i = index(1, 1);
        let e = &mut i["plugins"][0];
        e["name"] = "  Hello\u{200B}\t\u{202E}Export\n ".into();
        e["summary"] = "Says\u{202E} hello. ".repeat(40).into();
        e["author"] = "Auto\u{7}Scene".into();
        e["license"] = " \u{FEFF} ".into();
        i["withdrawn"] = json!([withdrawn(HELLO, "<1.0.0", "It deleted\n\texports.\u{200F}")]);
        let (a, state) = taken(signed_by(&owner.online, &i, &owner.keys(), &held), &held);
        let e = &a.index.plugins[0];
        assert_eq!((e.name.as_str(), e.author.as_str(), e.license.as_deref()), ("Hello Export", "Auto Scene", None));
        assert_eq!(e.summary.chars().count(), 300);
        assert!(e.summary.starts_with("Says hello. Says hello.") && e.summary.ends_with('…'), "{}", e.summary);
        assert_eq!(state.withdrawn[0].reason, "It deleted exports.");
        let mut i = index(1, 2);
        i["plugins"][0]["name"] = "x".repeat(50).into();
        i["plugins"][0]["author"] = "y".repeat(70).into();
        i["plugins"][0]["license"] = "z".repeat(70).into();
        let (a, _) = taken(signed_by(&owner.online, &i, &owner.keys(), &held), &held);
        let e = &a.index.plugins[0];
        assert_eq!(e.name, format!("{}…", "x".repeat(39)));
        assert_eq!(e.author, format!("{}…", "y".repeat(59)));
        assert_eq!(e.license, Some(format!("{}…", "z".repeat(59))));
    }

    #[test]
    fn the_catalog_keys_are_minisign_keys_one_of_each_role() {
        // Each compiled key reads (builtin panics otherwise).
        assert_eq!(CatalogKeys::builtin().is_empty(), CATALOG_KEYS.is_empty());
        // Empty until the catalog opens; then one online and one offline key.
        if !CATALOG_KEYS.is_empty() {
            let roles: Vec<KeyRole> = CATALOG_KEYS.iter().map(|(role, _)| *role).collect();
            assert_eq!(roles.len(), 2);
            assert!(roles.contains(&KeyRole::Online) && roles.contains(&KeyRole::Offline));
            assert_ne!(key_id(CATALOG_KEYS[0].1).unwrap(), key_id(CATALOG_KEYS[1].1).unwrap());
        }
        let urls = [
            "https://tcmmi.github.io/autoscene-plugins/",
            "https://raw.githubusercontent.com/tcmmi/autoscene-plugins/main/site/",
        ];
        assert_eq!(CATALOG_URLS, urls);
        assert_eq!((FIRST_EPOCH, MIN_EPOCH), (1, 1));
        let refused = CatalogKeys::new(&[(KeyRole::Online, "RWQ")]).unwrap_err();
        assert_eq!(refused.to_string(), "That isn't a minisign public key.");
    }
}

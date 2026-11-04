//! The large files plugins name in their `plugin.json` ([`Resource`]): a
//! model's weights, say, too large for a package. AutoScene gets each the
//! first time a run (or a service's start) of its plugin needs it, as it
//! gets its own models ([`as_models`]): from its addresses in turn, resumed
//! after an interruption, and kept only with its pinned size and SHA-256,
//! which the plugin's lock covers like its code. A plugin's files are kept
//! in a folder of its own, `<plugin-resources>/<id>/`
//! ([`as_core::AppPaths::plugin_resources`]), each with as-models' marker
//! beside it, and its program gets that folder (`AUTOSCENE_RESOURCES`, and
//! `plugin.resourcesDir` in its run line).
//!
//! One [`Resources`] lives as long as the app, and its clones share what it
//! hashed: a file is hashed the first time it is asked for in a process,
//! and again only once its size or time changed (8 GiB take about a minute
//! from a cold disk, so [`Resources::needs_check`] tells a caller to say
//! so first, and a cancel ends the hash). Two plugins never share a file.
//!
//! The requests go out as AutoScene's own models' do, not as `net.fetch`'s
//! ([`crate::net`]): through the proxy the environment names, as uv and the
//! models do, so a file never fails behind a company proxy that lets those
//! through. A name AutoScene resolves itself resolves only to public
//! addresses ([`PublicResolver`]), after each redirect too: every name
//! without a proxy, and with one, a host its `NO_PROXY` lets AutoScene
//! reach directly; only the proxy's own name resolves as the system says
//! ([`ProxyOrPublic`]). A name the proxy is asked for, the proxy resolves.
//! Either way the manifest's address is plain https on port 443 of a name
//! on the internet, every hop stays on https (at most 10, to any host:
//! Hugging Face sends its files to its CDN), nothing of the person's
//! travels with it, and the body counts only by its size, where it stops,
//! and its hash.
//!
//! What no installed version names goes at the app's start, and so do the
//! folders of plugins that are gone ([`Resources::prune`]); an uninstall
//! removes its plugin's folder ([`Resources::remove`]).

use std::collections::HashMap;
use std::fs::{self, File, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use as_core::CancelToken;
use as_models::{ModelStore, Packed, Pinned};
use ureq::Agent;
use ureq::config::Config;
use ureq::http::Uri;
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{DefaultConnector, NextTimeout};

use crate::error::PluginResult;
use crate::fetch::fetch_error;
use crate::manifest::{PluginId, RESOURCE_SIDECARS, Resource};
use crate::net::PublicResolver;

/// The plugins' large files, in their folder of the app's cache.
#[derive(Clone, Debug)]
pub struct Resources {
    root: PathBuf,
    /// The store of each plugin's folder, which remembers what it hashed:
    /// shared by the clones.
    stores: Arc<Mutex<HashMap<PluginId, ModelStore>>>,
}

impl Resources {
    /// The files kept in `root`, a folder per plugin.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into(), stores: Arc::default() }
    }

    /// The folder of `id`'s files.
    pub fn dir(&self, id: &PluginId) -> PathBuf {
        self.root.join(id.as_str())
    }

    fn stores(&self) -> MutexGuard<'_, HashMap<PluginId, ModelStore>> {
        self.stores.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The store of `id`'s folder.
    fn store(&self, id: &PluginId) -> ModelStore {
        self.stores().entry(id.clone()).or_insert_with(|| ModelStore::new(self.dir(id))).clone()
    }

    /// The ones of `rs`, `id`'s files, not kept yet: a file counts with its
    /// marker, its size and its hash, which is taken the first time this
    /// process asks (and again once the file changed), until `cancel`
    /// fires. One that no longer matches loses its marker, and is got again.
    pub fn missing<'a>(
        &self,
        id: &PluginId,
        rs: &'a [Resource],
        cancel: &CancelToken,
    ) -> PluginResult<Vec<&'a Resource>> {
        let store = self.store(id);
        let mut missing = Vec::new();
        for r in rs {
            match store.installed_pinned_until(&pinned(id, r), cancel) {
                Ok(Some(_)) => {}
                Ok(None) => missing.push(r),
                Err(e) => return Err(fetch_error(&r.name, e)),
            }
        }
        Ok(missing)
    }

    /// Whether [`Resources::missing`] would hash one of `rs`, `id`'s files,
    /// now: one is kept, and this process hasn't hashed it as it is. Read
    /// from sizes and times alone, so a caller can say it checks the files
    /// before it does.
    pub fn needs_check(&self, id: &PluginId, rs: &[Resource]) -> bool {
        let store = self.store(id);
        rs.iter().any(|r| store.would_hash(&pinned(id, r)))
    }

    /// What `id`'s files of `rs` take on the disk now ([`kept_bytes_in`]).
    pub fn kept_bytes(&self, id: &PluginId, rs: &[Resource]) -> u64 {
        kept_bytes_in(&self.dir(id), rs)
    }

    /// What getting `id`'s files of `rs` would download now: those without
    /// their pinned size and their mark. Read from sizes and marks alone, it
    /// hashes nothing (a file whose bytes changed in place counts as kept
    /// here, and is got again at the run, which hashes it).
    pub fn to_get(&self, id: &PluginId, rs: &[Resource]) -> u64 {
        let dir = self.dir(id);
        let kept = |r: &Resource| {
            let sized = fs::metadata(dir.join(&r.name)).is_ok_and(|m| m.is_file() && m.len() == r.bytes());
            let marked =
                fs::read_to_string(dir.join(format!("{}.verified", r.name))).is_ok_and(|s| s.trim() == r.sha256);
            sized && marked
        };
        rs.iter().filter(|r| !kept(r)).map(Resource::bytes).sum()
    }

    /// Gets the files of `rs` that aren't kept yet, for the plugin `id`
    /// named `name`: `progress(done, total)` over all of them as their
    /// bytes come; `cancel` ends it, and keeps what came for a resume.
    /// Nothing comes when the drive hasn't room for all of them. Waits
    /// while another fetch of a file runs, in this process or another.
    /// Gives the plugin's folder. What fails says so in words about the
    /// plugin's files.
    pub fn fetch(
        &self,
        id: &PluginId,
        name: &str,
        rs: &[Resource],
        cancel: &CancelToken,
        progress: impl FnMut(u64, u64),
    ) -> PluginResult<PathBuf> {
        self.fetch_via(&agent(), id, name, rs, cancel, progress)
    }

    /// [`Resources::fetch`], each request through `agent`: the tests' plain
    /// http one, to a server of their own.
    pub(crate) fn fetch_via(
        &self,
        agent: &Agent,
        id: &PluginId,
        name: &str,
        rs: &[Resource],
        cancel: &CancelToken,
        mut progress: impl FnMut(u64, u64),
    ) -> PluginResult<PathBuf> {
        let what = format!("{name}'s files");
        let dir = self.dir(id);
        fs::create_dir_all(&dir)?;
        let missing = self.missing(id, rs, cancel)?;
        let total: u64 = missing.iter().map(|r| r.bytes()).sum();
        let store = self.store(id);
        // Room for all of them, before the first byte of the first.
        let pins: Vec<Pinned> = missing.iter().map(|r| pinned(id, r)).collect();
        store.room_for_pinned(&pins).map_err(|e| fetch_error(&what, e))?;
        let mut before = 0;
        for r in missing {
            let got = store.fetch_pinned_via(&pinned(id, r), agent, cancel, |done, _| progress(before + done, total));
            got.map_err(|e| fetch_error(&what, e))?;
            before += r.bytes();
        }
        Ok(dir)
    }

    /// Removes, as much as it can, what no installed plugin names: the
    /// folders of plugins not in `installed`, and in the others the files
    /// their version no longer names, with what as-models keeps beside
    /// them. `installed` lists each plugin with the files its version
    /// names, or none when its `plugin.json` can't be read now: its folder
    /// stays as it is. A folder whose file is being fetched (its lock is
    /// held) stays too.
    pub fn prune(&self, installed: &[(PluginId, Option<Vec<Resource>>)]) {
        self.stores().retain(|id, _| installed.iter().any(|(i, _)| i == id));
        let Ok(entries) = fs::read_dir(&self.root) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let found = name.to_str().and_then(|n| installed.iter().find(|(id, _)| id.as_str() == n));
            match found {
                Some((_, None)) => {}
                Some((_, Some(rs))) => prune_files(&path, rs),
                None if held(&path) => {
                    tracing::info!(folder = %as_core::paths::name(&path), "a plugin's files are being fetched; kept");
                }
                None => match remove(&path) {
                    Ok(()) => {
                        tracing::info!(folder = %as_core::paths::name(&path), "removed the files of a plugin that is gone")
                    }
                    Err(e) => {
                        tracing::info!(folder = %as_core::paths::name(&path), error = %e, "the files of a plugin that is gone stay until the next start")
                    }
                },
            }
        }
    }

    /// Removes from `id`'s folder the files `rs`, those its version names
    /// now, doesn't name, with what as-models keeps beside them: before a
    /// run that sees the folder, so a version updated to in this process
    /// leaves it nothing of the one before, not even a part of a file. Not
    /// while a fetch holds a lock of the folder: what stays goes before the
    /// next run, or at the next start's [`Resources::prune`].
    pub fn prune_unnamed(&self, id: &PluginId, rs: &[Resource]) {
        let dir = self.dir(id);
        if !held(&dir) {
            prune_files(&dir, rs);
        }
    }

    /// Removes `id`'s files (it was uninstalled), as much as it can: what
    /// stays goes at the next start's [`Resources::prune`].
    pub fn remove(&self, id: &PluginId) {
        self.stores().remove(id);
        let dir = self.dir(id);
        match remove(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => tracing::warn!(plugin = %id, error = %e, "its downloaded files stay until the next start"),
        }
    }
}

/// What the files of `rs` in the folder `dir` take on the disk now, by
/// their sizes alone: never a hash, so it is cheap enough for every listing.
/// A part of one that is still coming doesn't count.
pub fn kept_bytes_in(dir: &Path, rs: &[Resource]) -> u64 {
    rs.iter().filter_map(|r| fs::metadata(dir.join(&r.name)).ok()).filter(|m| m.is_file()).map(|m| m.len()).sum()
}

/// `r` of the plugin `id` as the file the store fetches: kept by its name
/// in the plugin's folder, as it travels, never runnable.
fn pinned(id: &PluginId, r: &Resource) -> Pinned {
    Pinned {
        id: format!("{id}/{}", r.name),
        file: r.name.clone(),
        urls: r.urls.clone(),
        sha256: r.sha256.clone(),
        size: r.bytes(),
        packed: Packed::File,
        exec: false,
    }
}

/// Removes the files in `dir` none of `rs` names, with their markers,
/// locks and parts (a resource is one file, never an archive's member:
/// an `.archive.part` is nobody's), and a part longer than the file it is
/// of, which no fetch would resume: it goes now, not at the next fetch.
fn prune_files(dir: &Path, rs: &[Resource]) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let file = RESOURCE_SIDECARS.iter().find_map(|end| name.strip_suffix(end)).unwrap_or(name);
        let what = match rs.iter().find(|r| r.name == file) {
            Some(r) if name.ends_with(".part") && entry.metadata().is_ok_and(|md| md.len() > r.bytes()) => {
                "part longer than its file"
            }
            Some(_) => continue,
            None => "file it no longer names",
        };
        match remove(&entry.path()) {
            Ok(()) => tracing::info!(file = name, "removed a plugin's {what}"),
            Err(e) => tracing::info!(file = name, error = %e, "a plugin's {what} stays until the next start"),
        }
    }
}

/// Removes the file or folder at `path`; a link goes, never what it leads to.
fn remove(path: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(path)?.is_dir() {
        true => fs::remove_dir_all(path),
        false => fs::remove_file(path),
    }
}

/// Whether a fetch holds a lock of the folder `dir`.
fn held(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else { return false };
    entries.flatten().filter(|e| e.file_name().to_str().is_some_and(|n| n.ends_with(".lock"))).any(|e| {
        File::options().write(true).open(e.path()).is_ok_and(|f| matches!(f.try_lock(), Err(TryLockError::WouldBlock)))
    })
}

/// The agent the files come through: as-models' ([`as_models::net::agent`]:
/// its timeouts, its user agent, https only, the proxy the environment
/// names), whose names, without a proxy, resolve only to public addresses.
pub fn agent() -> Agent {
    agent_from(as_models::net::config("https://").build())
}

/// The agent of `config`, whose names resolve to public addresses only, as
/// `net.fetch`'s do, but for its proxy's own ([`ProxyOrPublic`]).
fn agent_from(config: Config) -> Agent {
    let proxy = config.proxy().map(|p| p.uri().clone());
    let public = PublicResolver::new(DefaultResolver::default());
    Agent::with_parts(
        config,
        DefaultConnector::new(),
        ProxyOrPublic { proxy, direct: DefaultResolver::default(), public },
    )
}

/// Resolves the proxy's own address (the one ureq connects to: its `uri`)
/// as the system does, and every other name to public addresses only. ureq
/// resolves a name itself for a host the proxy's `NO_PROXY` names (and
/// connects to it directly), and for a SOCKS4 proxy: those connections
/// must not reach a private address either. A name the proxy is asked for,
/// ureq never resolves.
#[derive(Debug)]
struct ProxyOrPublic {
    proxy: Option<Uri>,
    direct: DefaultResolver,
    public: PublicResolver,
}

impl Resolver for ProxyOrPublic {
    fn resolve(&self, uri: &Uri, config: &Config, timeout: NextTimeout) -> Result<ResolvedSocketAddrs, ureq::Error> {
        match self.proxy.as_ref() == Some(uri) {
            true => self.direct.resolve(uri, config, timeout),
            false => self.public.resolve(uri, config, timeout),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, ErrorKind, Write};
    use std::net::TcpListener;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, SystemTime};

    use sha2::{Digest, Sha256};

    use super::*;
    use crate::error::PluginError;

    fn id() -> PluginId {
        PluginId::parse("dev.example.big-model").unwrap()
    }

    fn sha(bytes: &[u8]) -> String {
        Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Bytes that look like a model's.
    fn weights(len: u32, seed: u32) -> Vec<u8> {
        (0..len).map(|i| (i.wrapping_add(seed).wrapping_mul(2_654_435_761) >> 13) as u8).collect()
    }

    /// The resource `name` of `bytes`, from `urls`.
    fn resource(name: &str, urls: &[&str], bytes: &[u8]) -> Resource {
        Resource {
            name: name.into(),
            urls: urls.iter().map(|u| (*u).to_owned()).collect(),
            sha256: sha(bytes),
            size: bytes.len() as f64,
        }
    }

    /// The range a request asks for, within `len` bytes; none for all.
    fn read_range(reader: &mut impl BufRead, len: u64) -> Option<(u64, u64)> {
        let mut range = None;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim().is_empty() {
                return range;
            }
            if let Some(r) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                let (a, b) = r.trim().split_once('-').unwrap();
                let b: u64 = if b.is_empty() { len - 1 } else { b.parse().unwrap() };
                range = Some((a.parse().unwrap(), b.min(len - 1)));
            }
        }
    }

    /// Serves `data` on 127.0.0.1, its byte ranges too, counting the bytes
    /// it sent: its address. `/gone/…` answers 404. One that `stalls` sends
    /// half of each answer, then nothing more.
    fn serve_as(data: Vec<u8>, stalls: bool) -> (String, Arc<AtomicU64>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/model.onnx", listener.local_addr().unwrap());
        let sent = Arc::new(AtomicU64::new(0));
        let counted = sent.clone();
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { return };
                let mut reader = BufReader::new(conn.try_clone().unwrap());
                let mut request = String::new();
                let _ = reader.read_line(&mut request);
                let range = read_range(&mut reader, data.len() as u64);
                if request.split(' ').nth(1).is_some_and(|path| path.starts_with("/gone/")) {
                    let _ = conn.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    continue;
                }
                let (status, body, extra) = match range {
                    Some((a, b)) => (
                        "206 Partial Content",
                        &data[a as usize..=b as usize],
                        format!("Content-Range: bytes {a}-{b}/{}\r\n", data.len()),
                    ),
                    None => ("200 OK", &data[..], String::new()),
                };
                let body = if stalls { &body[..body.len() / 2] } else { body };
                counted.fetch_add(body.len() as u64, Ordering::SeqCst);
                let len = if stalls { body.len() * 2 } else { body.len() };
                let head = format!("HTTP/1.1 {status}\r\nContent-Length: {len}\r\n{extra}Connection: close\r\n\r\n");
                let _ = conn.write_all(head.as_bytes()).and_then(|()| conn.write_all(body));
                if stalls {
                    held.push(conn);
                }
            }
        });
        (url, sent)
    }

    fn serve(data: Vec<u8>) -> (String, Arc<AtomicU64>) {
        serve_as(data, false)
    }

    /// The tests' agent: plain http, to their own servers.
    fn plain() -> Agent {
        as_models::net::agent("http://127.0.0.1/")
    }

    /// The names in `dir`, sorted.
    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> =
            fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort_unstable();
        names
    }

    fn fetch(store: &Resources, rs: &[Resource]) -> PluginResult<PathBuf> {
        store.fetch_via(&plain(), &id(), "Big Model", rs, &CancelToken::new(), |_, _| {})
    }

    #[test]
    fn a_resource_is_got_once_and_kept_by_its_hash() {
        let (one, two) = (weights(300_000, 1), weights(100_000, 2));
        let ((a, sent_a), (b, sent_b)) = (serve(one.clone()), serve(two.clone()));
        let rs = [resource("model.onnx", &[&a], &one), resource("vocab.bin", &[&b], &two)];
        let tmp = tempfile::tempdir().unwrap();
        let store = Resources::new(tmp.path().join("plugin-resources"));
        assert_eq!(store.missing(&id(), &rs, &CancelToken::new()).unwrap(), [&rs[0], &rs[1]]);
        assert_eq!((store.kept_bytes(&id(), &rs), store.to_get(&id(), &rs)), (0, 400_000));
        // Both, with their progress counted over the two.
        let mut seen = Vec::new();
        let dir =
            store.fetch_via(&plain(), &id(), "Big Model", &rs, &CancelToken::new(), |d, t| seen.push((d, t))).unwrap();
        assert_eq!(dir, store.dir(&id()));
        assert_eq!(dir, tmp.path().join("plugin-resources").join("dev.example.big-model"));
        assert_eq!(fs::read(dir.join("model.onnx")).unwrap(), one);
        assert_eq!(fs::read(dir.join("vocab.bin")).unwrap(), two);
        let total = 400_000;
        assert!(seen.iter().all(|&(_, t)| t == total), "{seen:?}");
        assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0), "progress only grows");
        assert_eq!(seen.last(), Some(&(total, total)));
        let kept = [
            "model.onnx",
            "model.onnx.lock",
            "model.onnx.verified",
            "vocab.bin",
            "vocab.bin.lock",
            "vocab.bin.verified",
        ];
        assert_eq!(names(&dir), kept);
        assert_eq!(fs::read_to_string(dir.join("model.onnx.verified")).unwrap(), rs[0].sha256);
        // Kept: nothing is missing, and nothing travels again.
        assert!(store.missing(&id(), &rs, &CancelToken::new()).unwrap().is_empty());
        assert_eq!(fetch(&store, &rs).unwrap(), dir);
        assert_eq!((sent_a.load(Ordering::SeqCst), sent_b.load(Ordering::SeqCst)), (300_000, 100_000));
        assert_eq!((store.kept_bytes(&id(), &rs), store.to_get(&id(), &rs)), (total, 0));
        // Another plugin's files are its own.
        let other = PluginId::parse("dev.example.other").unwrap();
        assert_eq!(store.missing(&other, &rs, &CancelToken::new()).unwrap().len(), 2);
    }

    #[test]
    fn a_resource_unlike_its_pin_is_never_kept() {
        let pinned = weights(100_000, 1);
        // The address now serves another file of the same size.
        let (url, _) = serve(weights(100_000, 9));
        let rs = [resource("model.onnx", &[&url], &pinned)];
        let tmp = tempfile::tempdir().unwrap();
        let store = Resources::new(tmp.path());
        let e = fetch(&store, &rs).unwrap_err();
        assert!(matches!(&e, PluginError::Download { what, .. } if what == "Big Model's files"), "{e:?}");
        assert_eq!(
            e.to_string(),
            "AutoScene couldn't get Big Model's files. Check the internet connection, then try again."
        );
        assert!(e.detail().contains("dev.example.big-model/model.onnx"), "{}", e.detail());
        assert_eq!(names(&store.dir(&id())), ["model.onnx.lock"], "nothing of it stays");
        assert_eq!(store.missing(&id(), &rs, &CancelToken::new()).unwrap().len(), 1);
    }

    #[test]
    fn a_host_that_sends_more_than_the_pin_is_stopped_and_nothing_stays() {
        let pinned = weights(100_000, 1);
        let (url, _) = serve(weights(100_001, 1));
        let rs = [resource("model.onnx", &[&url], &pinned)];
        let tmp = tempfile::tempdir().unwrap();
        let store = Resources::new(tmp.path());
        let e = fetch(&store, &rs).unwrap_err();
        assert!(matches!(&e, PluginError::Download { what, .. } if what == "Big Model's files"), "{e:?}");
        assert!(e.detail().contains("more than 100000 bytes"), "{}", e.detail());
        assert_eq!(names(&store.dir(&id())), ["model.onnx.lock"], "nothing of it stays");
    }

    #[test]
    fn a_part_longer_than_its_file_goes_without_waiting_for_a_fetch() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Resources::new(tmp.path());
        let rs = vec![resource("model.onnx", &[], b"four"), resource("vocab.bin", &[], b"four")];
        let dir = store.dir(&id());
        fs::create_dir_all(&dir).unwrap();
        let put = || {
            fs::write(dir.join("model.onnx.part"), b"longer").unwrap();
            fs::write(dir.join("vocab.bin.part"), b"four").unwrap();
        };
        put();
        store.prune(&[(id(), Some(rs.clone()))]);
        assert_eq!(names(&dir), ["vocab.bin.part"], "a part that fits stays for a resume");
        put();
        store.prune_unnamed(&id(), &rs);
        assert_eq!(names(&dir), ["vocab.bin.part"]);
    }

    #[test]
    fn a_changed_pin_gets_the_file_again() {
        let (old, new) = (weights(100_000, 1), weights(120_000, 2));
        let ((a, _), (b, sent)) = (serve(old.clone()), serve(new.clone()));
        let tmp = tempfile::tempdir().unwrap();
        let store = Resources::new(tmp.path());
        fetch(&store, &[resource("model.onnx", &[&a], &old)]).unwrap();
        // An update names the same file with another hash.
        let rs = [resource("model.onnx", &[&b], &new)];
        assert_eq!(store.missing(&id(), &rs, &CancelToken::new()).unwrap(), [&rs[0]]);
        let dir = fetch(&store, &rs).unwrap();
        assert_eq!(fs::read(dir.join("model.onnx")).unwrap(), new);
        assert_eq!(fs::read_to_string(dir.join("model.onnx.verified")).unwrap(), rs[0].sha256);
        assert_eq!(sent.load(Ordering::SeqCst), 120_000);
    }

    #[test]
    fn a_changed_pin_after_a_cancel_gets_the_file_whole() {
        let (old, new) = (weights(100_000, 1), weights(100_000, 2));
        let (url, sent) = serve(new.clone());
        let tmp = tempfile::tempdir().unwrap();
        let store = Resources::new(tmp.path());
        // A cancelled fetch of the old pin left half of it.
        fs::create_dir_all(store.dir(&id())).unwrap();
        fs::write(store.dir(&id()).join("model.onnx.part"), &old[..50_000]).unwrap();
        // The new pin, one address: resumed, it fails the hash; then it
        // comes whole from the same address.
        let dir = fetch(&store, &[resource("model.onnx", &[&url], &new)]).unwrap();
        assert_eq!(fs::read(dir.join("model.onnx")).unwrap(), new);
        assert_eq!(sent.load(Ordering::SeqCst), 50_000 + 100_000);
        assert_eq!(names(&dir), ["model.onnx", "model.onnx.lock", "model.onnx.verified"]);
    }

    #[test]
    fn the_next_url_is_tried_when_one_fails() {
        let file = weights(50_000, 1);
        let (good, _) = serve(file.clone());
        let (wrong, _) = serve(weights(50_000, 2));
        let gone = good.replace("/model.onnx", "/gone/model.onnx");
        let tmp = tempfile::tempdir().unwrap();
        let store = Resources::new(tmp.path());
        // A 404, a file that isn't the pinned one, then the file.
        let rs = [resource("model.onnx", &[&gone, &wrong, &good], &file)];
        assert_eq!(fs::read(fetch(&store, &rs).unwrap().join("model.onnx")).unwrap(), file);
        // All failing: the last one's error, in words about the plugin.
        let other = Resources::new(tmp.path().join("other"));
        let e = fetch(&other, &[resource("model.onnx", &[&wrong, &gone], &file)]).unwrap_err();
        assert!(matches!(&e, PluginError::Download { .. }), "{e:?}");
        assert!(e.detail().contains("/gone/model.onnx"), "{}", e.detail());
    }

    #[test]
    fn cancel_keeps_the_part_for_a_resume() {
        let file = weights(200_000, 1);
        let (stalling, _) = serve_as(file.clone(), true);
        let tmp = tempfile::tempdir().unwrap();
        let store = Resources::new(tmp.path());
        let rs = [resource("model.onnx", &[&stalling], &file)];
        let cancel = CancelToken::new();
        let later = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            later.cancel();
        });
        let e = store.fetch_via(&plain(), &id(), "Big Model", &rs, &cancel, |_, _| {}).unwrap_err();
        assert!(matches!(e, PluginError::Cancelled), "{e:?}");
        let part = store.dir(&id()).join("model.onnx.part");
        let had = fs::metadata(&part).unwrap().len();
        assert_eq!(had, 100_000, "what came stays");
        // The next fetch asks only for the rest.
        let (url, sent) = serve(file.clone());
        let rs = [resource("model.onnx", &[&url], &file)];
        let mut first = None;
        let dir = store
            .fetch_via(&plain(), &id(), "Big Model", &rs, &CancelToken::new(), |d, _| {
                first.get_or_insert(d);
            })
            .unwrap();
        assert_eq!(fs::read(dir.join("model.onnx")).unwrap(), file);
        assert_eq!(sent.load(Ordering::SeqCst), 200_000 - had);
        assert_eq!(first, Some(had), "its progress starts where the part ends");
        // Cancelled before it starts, nothing is asked.
        let cancelled = CancelToken::new();
        cancelled.cancel();
        let fresh = Resources::new(tmp.path().join("fresh"));
        let e = fresh.fetch_via(&plain(), &id(), "Big Model", &rs, &cancelled, |_, _| {}).unwrap_err();
        assert!(matches!(e, PluginError::Cancelled), "{e:?}");
        assert_eq!(sent.load(Ordering::SeqCst), 200_000 - had);
    }

    #[test]
    fn files_no_plugin_names_are_pruned() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Resources::new(tmp.path());
        let put = |path: &str| {
            let path = tmp.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"x").unwrap();
        };
        let kept = [
            "dev.a.one/model.onnx",
            "dev.a.one/model.onnx.verified",
            "dev.a.one/model.onnx.lock",
            "dev.a.one/vocab.bin.part",
            "dev.a.broken/whatever.bin",
            "dev.a.busy/model.onnx.lock",
        ];
        let gone = [
            "dev.a.one/old.onnx",
            "dev.a.one/old.onnx.verified",
            "dev.a.one/old.onnx.lock",
            "dev.a.one/old.onnx.part",
            "dev.a.one/model.onnx.archive.part",
            "dev.a.gone/model.onnx",
            "dev.a.gone/model.onnx.verified",
        ];
        for path in kept.iter().chain(&gone) {
            put(path);
        }
        // A plugin that isn't installed any more, whose file a fetch holds
        // (one installed after the list was made): kept.
        let busy = File::options().write(true).open(tmp.path().join("dev.a.busy/model.onnx.lock")).unwrap();
        busy.lock().unwrap();
        let one = PluginId::parse("dev.a.one").unwrap();
        let broken = PluginId::parse("dev.a.broken").unwrap();
        let rs = vec![resource("model.onnx", &["https://example.com/m"], b"x"), resource("vocab.bin", &[], b"v")];
        store.prune(&[(one, Some(rs)), (broken, None)]);
        for path in kept {
            assert!(tmp.path().join(path).is_file(), "{path} stays");
        }
        for path in gone {
            assert!(!tmp.path().join(path).exists(), "{path} goes");
        }
        assert!(!tmp.path().join("dev.a.gone").exists());
        drop(busy);
        store.prune(&[]);
        assert!(names(tmp.path()).is_empty(), "with none installed, nothing stays");
        // No folder yet: nothing to do.
        Resources::new(tmp.path().join("missing")).prune(&[]);
    }

    #[test]
    fn before_a_run_its_folder_keeps_only_what_its_version_names() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Resources::new(tmp.path());
        let dir = store.dir(&id());
        fs::create_dir_all(&dir).unwrap();
        let kept = ["model.onnx", "model.onnx.verified", "model.onnx.lock"];
        // What the version before named: a file, its marker, and a part of one.
        let gone = ["old.onnx", "old.onnx.verified", "old.onnx.part"];
        for name in kept.iter().chain(&gone) {
            fs::write(dir.join(name), b"x").unwrap();
        }
        let rs = [resource("model.onnx", &["https://example.com/m"], b"x")];
        // A fetch holds its lock: nothing goes yet.
        let busy = File::options().write(true).open(dir.join("model.onnx.lock")).unwrap();
        busy.lock().unwrap();
        store.prune_unnamed(&id(), &rs);
        assert_eq!(names(&dir).len(), kept.len() + gone.len());
        drop(busy);
        store.prune_unnamed(&id(), &rs);
        let mut want = kept.to_vec();
        want.sort();
        assert_eq!(names(&dir), want);
        // No folder yet: nothing to do.
        Resources::new(tmp.path().join("missing")).prune_unnamed(&id(), &rs);
    }

    #[test]
    fn uninstall_removes_the_plugins_folder() {
        let file = weights(10_000, 1);
        let (url, _) = serve(file.clone());
        let tmp = tempfile::tempdir().unwrap();
        let store = Resources::new(tmp.path());
        let rs = [resource("model.onnx", &[&url], &file)];
        fetch(&store, &rs).unwrap();
        store.remove(&id());
        assert!(!store.dir(&id()).exists());
        assert_eq!(store.missing(&id(), &rs, &CancelToken::new()).unwrap().len(), 1);
        // Gone already: nothing to say.
        store.remove(&id());
    }

    #[test]
    fn a_second_check_in_one_process_hashes_nothing() {
        let file = weights(100_000, 1);
        let (url, _) = serve(file.clone());
        let tmp = tempfile::tempdir().unwrap();
        let store = Resources::new(tmp.path());
        let rs = [resource("model.onnx", &[&url], &file)];
        let dir = fetch(&store, &rs).unwrap();
        let path = dir.join("model.onnx");
        // Damaged in place, its size and time kept: a clone of the store,
        // which shares what it hashed, trusts it…
        let when = fs::metadata(&path).unwrap().modified().unwrap();
        let mut damaged = file.clone();
        damaged[500] ^= 1;
        fs::write(&path, &damaged).unwrap();
        File::options().write(true).open(&path).unwrap().set_modified(when).unwrap();
        assert!(store.clone().missing(&id(), &rs, &CancelToken::new()).unwrap().is_empty());
        // What a review would download reads sizes and marks alone.
        assert_eq!(Resources::new(tmp.path()).to_get(&id(), &rs), 0);
        // …a new one hashes it, finds it damaged, and takes its marker away.
        assert_eq!(Resources::new(tmp.path()).missing(&id(), &rs, &CancelToken::new()).unwrap(), [&rs[0]]);
        assert!(!dir.join("model.onnx.verified").exists());
        // A file that changed is hashed again by any.
        fs::write(&path, &file).unwrap();
        fs::write(dir.join("model.onnx.verified"), &rs[0].sha256).unwrap();
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(SystemTime::now() + Duration::from_secs(5))
            .unwrap();
        assert!(store.missing(&id(), &rs, &CancelToken::new()).unwrap().is_empty());
    }

    #[test]
    fn kept_bytes_reads_sizes_only() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Resources::new(tmp.path());
        let dir = store.dir(&id());
        fs::create_dir_all(&dir).unwrap();
        let rs =
            [resource("a.bin", &[], &[1; 1000]), resource("b.bin", &[], &[2; 500]), resource("c.bin", &[], &[3; 200])];
        // a.bin with other bytes of its size, no marker: on the disk, so it
        // counts; a hash would have said otherwise.
        fs::write(dir.join("a.bin"), [9; 1000]).unwrap();
        // b.bin half there, as a part: not a file yet.
        fs::write(dir.join("b.bin.part"), [2; 250]).unwrap();
        // c.bin of another size: what it takes.
        fs::write(dir.join("c.bin"), [3; 10]).unwrap();
        // A file no resource names isn't the plugin's.
        fs::write(dir.join("d.bin"), [4; 100]).unwrap();
        assert_eq!(store.kept_bytes(&id(), &rs), 1010);
        assert!(!dir.join("a.bin.verified").exists(), "nothing was hashed or marked");
        assert_eq!(store.kept_bytes(&PluginId::parse("dev.a.none").unwrap(), &rs), 0);
    }

    #[test]
    fn to_get_counts_a_file_without_its_size_or_its_mark() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Resources::new(tmp.path());
        let dir = store.dir(&id());
        fs::create_dir_all(&dir).unwrap();
        let rs = [
            resource("sized.bin", &[], &[1; 1000]),
            resource("marked.bin", &[], &[2; 500]),
            resource("kept.bin", &[], &[3; 200]),
        ];
        // Its size, no mark: still to get.
        fs::write(dir.join("sized.bin"), [1; 1000]).unwrap();
        // Its mark, another size: still to get.
        fs::write(dir.join("marked.bin"), [2; 499]).unwrap();
        fs::write(dir.join("marked.bin.verified"), &rs[1].sha256).unwrap();
        // Both: kept.
        fs::write(dir.join("kept.bin"), [3; 200]).unwrap();
        fs::write(dir.join("kept.bin.verified"), &rs[2].sha256).unwrap();
        assert_eq!(store.to_get(&id(), &rs), 1500);
        // A mark of another hash isn't its mark.
        fs::write(dir.join("kept.bin.verified"), &rs[0].sha256).unwrap();
        assert_eq!(store.to_get(&id(), &rs), 1700);
    }

    #[test]
    fn a_check_of_kept_files_is_told_first_and_ends_at_a_cancel() {
        let file = weights(3 << 20, 1);
        let (url, _) = serve(file.clone());
        let tmp = tempfile::tempdir().unwrap();
        let rs = [resource("model.onnx", &[&url], &file)];
        let store = Resources::new(tmp.path());
        // Nothing kept: nothing to check.
        assert!(!store.needs_check(&id(), &rs));
        fetch(&store, &rs).unwrap();
        // Just fetched, it was hashed: nothing to check in this process.
        assert!(!store.needs_check(&id(), &rs));
        // The app's next start hashes it first, which a cancel ends.
        let next = Resources::new(tmp.path());
        assert!(next.needs_check(&id(), &rs));
        let cancelled = CancelToken::new();
        cancelled.cancel();
        let e = next.missing(&id(), &rs, &cancelled).unwrap_err();
        assert!(matches!(e, PluginError::Cancelled), "{e:?}");
        let e = next.fetch_via(&plain(), &id(), "Big Model", &rs, &cancelled, |_, _| {}).unwrap_err();
        assert!(matches!(e, PluginError::Cancelled), "{e:?}");
        assert!(next.dir(&id()).join("model.onnx.verified").exists(), "a cancel says nothing of the file");
        assert!(next.missing(&id(), &rs, &CancelToken::new()).unwrap().is_empty());
        assert!(!next.needs_check(&id(), &rs));
    }

    #[test]
    fn no_file_comes_without_room_for_all_of_them() {
        let small = weights(10_000, 1);
        let (url, sent) = serve(small.clone());
        let tmp = tempfile::tempdir().unwrap();
        let store = Resources::new(tmp.path());
        // The second can't fit on any drive: the first doesn't come either.
        let mut huge = resource("huge.bin", &[&url], &small);
        huge.size = (1u64 << 50) as f64;
        let rs = [resource("small.bin", &[&url], &small), huge];
        let e = fetch(&store, &rs).unwrap_err();
        assert!(e.to_string().starts_with("There isn't enough free space for Big Model's files. Free up "), "{e}");
        let mb: u64 = e.to_string().split("Free up ").nth(1).unwrap().split(' ').next().unwrap().parse().unwrap();
        assert!(mb > 1_000_000_000, "the whole shortfall: {mb} MB");
        assert_eq!(sent.load(Ordering::SeqCst), 0, "nothing travels");
    }

    /// Whether nothing connected to `listener`.
    fn nobody_called(listener: &TcpListener) -> bool {
        listener.set_nonblocking(true).unwrap();
        listener.accept().map(|_| ()).is_err_and(|e| e.kind() == ErrorKind::WouldBlock)
    }

    #[test]
    fn agent_resolves_publicly_without_a_proxy() {
        // Something listens on this computer, where `localhost` leads.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let listener_v6 = TcpListener::bind(("::1", port)).ok();
        let config = as_models::net::config("https://").proxy(None).build();
        assert!(config.https_only());
        let agent = agent_from(config);
        let e = agent.get(format!("https://localhost:{port}/model.onnx")).call().unwrap_err();
        assert!(e.to_string().contains("no public address"), "{e}");
        // Through a store too, which says so in its words.
        let file = weights(1000, 1);
        let rs = [resource("model.onnx", &[&format!("https://localhost:{port}/model.onnx")], &file)];
        let tmp = tempfile::tempdir().unwrap();
        let e = Resources::new(tmp.path())
            .fetch_via(&agent, &id(), "Big Model", &rs, &CancelToken::new(), |_, _| {})
            .unwrap_err();
        assert!(e.detail().contains("no public address"), "{}", e.detail());
        assert!(nobody_called(&listener));
        assert!(listener_v6.as_ref().is_none_or(nobody_called));
    }

    /// The first line a connection to `listener` sent within 30 seconds:
    /// none when nothing connected, so a test fails rather than hangs.
    fn first_line(listener: TcpListener) -> Option<String> {
        listener.set_nonblocking(true).unwrap();
        let until = std::time::Instant::now() + Duration::from_secs(30);
        let conn = loop {
            match listener.accept() {
                Ok((conn, _)) => break conn,
                Err(e) if e.kind() == ErrorKind::WouldBlock && std::time::Instant::now() < until => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(_) => return None,
            }
        };
        conn.set_nonblocking(false).unwrap();
        conn.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let mut line = String::new();
        BufReader::new(conn).read_line(&mut line).ok()?;
        Some(line)
    }

    #[test]
    fn agent_uses_the_environment_s_proxy() {
        // A proxy on this computer: the address check would refuse it.
        let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
        let at = format!("http://{}", proxy.local_addr().unwrap());
        let asked = std::thread::spawn(move || first_line(proxy));
        // Something on this computer that NO_PROXY names: reached directly,
        // its name would lead here.
        let direct = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = direct.local_addr().unwrap().port();
        let direct_v6 = TcpListener::bind(("::1", port)).ok();
        // In a process of its own, whose environment names the proxy before
        // anything reads it.
        let out = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "resources::tests::a_fetch_from_behind_a_proxy", "--ignored", "--nocapture"])
            .envs([("AUTOSCENE_TEST_PROXY", &at), ("HTTPS_PROXY", &at), ("ALL_PROXY", &at)])
            .env("AUTOSCENE_TEST_DIRECT", port.to_string())
            // Read before no_proxy, which on Windows is the same variable.
            .env("NO_PROXY", "localhost")
            .output()
            .unwrap();
        let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(out.status.success() && said.contains("1 passed"), "{said}");
        // The proxy was asked for the host: it resolves the name.
        assert_eq!(asked.join().unwrap().as_deref().map(str::trim_end), Some("CONNECT huggingface.co:443 HTTP/1.1"));
        // What NO_PROXY names goes directly, and so only to a public address.
        assert!(nobody_called(&direct));
        assert!(direct_v6.as_ref().is_none_or(nobody_called));
    }

    /// The other half of [`agent_uses_the_environment_s_proxy`], which runs it.
    #[test]
    #[ignore = "run by agent_uses_the_environment_s_proxy, with a proxy named"]
    fn a_fetch_from_behind_a_proxy() {
        let Ok(proxy) = std::env::var("AUTOSCENE_TEST_PROXY") else { return };
        let agent = agent();
        let named = agent.config().proxy().map(|p| format!("http://{}:{}", p.host(), p.port()));
        assert_eq!(named.as_deref(), Some(proxy.as_str()));
        assert!(agent.config().https_only());
        // The proxy hangs up: the request fails, after it asked the proxy.
        let r = agent.get("https://huggingface.co/org/repo/resolve/main/model.onnx").call();
        assert!(r.is_err());
        // A host NO_PROXY names isn't asked of the proxy: its name resolves
        // here, to public addresses only.
        let port = std::env::var("AUTOSCENE_TEST_DIRECT").unwrap();
        let e = agent.get(format!("https://localhost:{port}/model.onnx")).call().unwrap_err();
        assert!(e.to_string().contains("no public address"), "{e}");
    }
}

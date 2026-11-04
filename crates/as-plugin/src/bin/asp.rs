//! `asp`: makes and checks AutoScene plugin packages (`.asp`).
//!
//! ```text
//! asp keygen <dir> [--force] [--unencrypted]
//! asp pack <plugin-folder> <out.asp>
//! asp sign <in.asp> --key <asp.key> [--out <out.asp>]
//! asp verify <in.asp> [--catalog <index.json> [--online-key <RW…>]… [--offline-key <RW…>]…]
//! asp init <dir> [--page | --service | --action] [--id <id>] [--name <name>]
//! ```
//!
//! - `init` makes a plugin to start from in `<dir>`, a new or empty folder,
//!   from the templates in `crates/as-plugin/templates`: a page in the rail
//!   (`--page`, the default), a page and the service it asks (`--service`),
//!   or a program with an export action (`--action`). Each gets
//!   `plugin.json`'s schema in `.autoscene/` and a `.vscode/settings.json`
//!   that checks `plugin.json` against it; a page also gets the SDK's types
//!   in `.autoscene/types/`. `pack` leaves both folders out. Its id is
//!   `--id`, or `dev.example.` and the folder's name; its name is `--name`,
//!   or the folder's.
//! - `keygen` makes an author's minisign key pair in `<dir>`: `asp.key`, the
//!   secret key, locked with a password it asks for (`--unencrypted` leaves
//!   that out, for CI), and `author.pub`, the public key's base64 line. It
//!   replaces neither unless `--force`, and makes none in a plugin's folder.
//! - `pack` lists the folder in a `files.lock` ([`package::lock_folder`],
//!   which refuses links) and zips the two. It refuses a folder that holds
//!   a secret key. The programs `plugin.json` starts are marked to run, and
//!   so, with a warning, is any other file that may run here (Linux and
//!   macOS). The same folder gives the same package, byte for byte, on one
//!   system.
//! - `sign` signs a package's lock with `asp.key`, for its plugin and
//!   version ([`trust::trusted_comment`]), and adds the signature and
//!   `author.pub` (in place, or as `--out`).
//! - `verify` checks a package as AutoScene installs it, and names the trust
//!   it would get there: Reviewed when the catalog `--catalog` lists its
//!   lock, Pinned to its author's key, or Unreviewed. That catalog is an
//!   `index.json` with its `index.json.minisig` beside it, taken as a fresh
//!   AutoScene takes it: signed by one of AutoScene's catalog keys (or a key
//!   given with `--online-key` or `--offline-key`) and living. A version it
//!   withdrew fails.
//!
//! `pack` and `verify` check each platform the plugin runs on as AutoScene
//! does there, warn of one it can't run on, and fail when it runs on none.
//! They check the icon and the banner as AutoScene does, and warn of a
//! banner that cards would show soft or crop. A plugin made for another
//! AutoScene than the one asp comes with gets a warning, not an error. A
//! package is unpacked as AutoScene would before it takes the place of the
//! output, so one AutoScene would refuse is never written. Errors exit with
//! 1, wrong arguments with 2.

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use as_plugin::catalog::{self, Catalog, CatalogKeys, CatalogState, Checked};
use as_plugin::manifest::{self, Parsed};
use as_plugin::package::{self, AUTHOR_KEY, LOCK, MANIFEST, MAX_BANNER, PictureFormat, SIG, file_in};
use as_plugin::store;
use as_plugin::trust::{self, Trust, Verdict};
use as_plugin::{FilesLock, KeyRole, Launch, LockedFile, Manifest, PluginError, Unpacked, current_platform};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use minisign::{KeyPair, PublicKey, SecretKey};
use semver::Version;
use tempfile::{NamedTempFile, TempDir};
use zip::result::ZipError;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, System, ZipArchive, ZipWriter};

const USAGE: &str = "\
usage: asp keygen <dir> [--force] [--unencrypted]
       asp pack <plugin-folder> <out.asp>
       asp sign <in.asp> --key <asp.key> [--out <out.asp>]
       asp verify <in.asp> [--catalog <index.json> [--online-key <RW…>]… [--offline-key <RW…>]…]
       asp init <dir> [--page | --service | --action] [--id <id>] [--name <name>]";

/// The secret key `keygen` writes beside `author.pub`.
const SECRET_KEY: &str = "asp.key";
/// The largest file `pack` looks into for a secret key. A key file is a
/// comment line and a line of about 210 characters.
const MAX_KEY_FILE: u64 = 4 << 10;
/// The schema of `plugin.json`, which `init` writes for the plugin's editor.
const SCHEMA: &str = include_str!("../../schema/plugin.schema.json");
/// The SDK's types (`npm run build:kit` writes them), which `init` writes
/// beside the schema for a page: each file there, by its path in the plugin.
const TYPES: [(&str, &str); 6] = [
    (".autoscene/types/bridge-types.d.ts", include_str!("../../templates/types/bridge-types.d.ts")),
    (".autoscene/types/global.d.ts", include_str!("../../templates/types/global.d.ts")),
    (".autoscene/types/keys.d.ts", include_str!("../../templates/types/keys.d.ts")),
    (".autoscene/types/mock.d.ts", include_str!("../../templates/types/mock.d.ts")),
    (".autoscene/types/protocol.d.ts", include_str!("../../templates/types/protocol.d.ts")),
    (".autoscene/types/sdk.d.ts", include_str!("../../templates/types/sdk.d.ts")),
];

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::Usage(why)) => {
            eprintln!("asp: {why}\n{USAGE}");
            ExitCode::from(2)
        }
        Err(Failure::Failed(why)) => {
            eprintln!("asp: {why}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[OsString]) -> Result<(), Failure> {
    let Some((command, rest)) = args.split_first() else { return Err(usage("name a command")) };
    match command.to_str().unwrap_or_default() {
        "keygen" => {
            let a = Args::read(rest, 1, &["--force", "--unencrypted"], &[])?;
            keygen(&a.paths[0], a.has("--force"), a.has("--unencrypted"))
        }
        "pack" => {
            let a = Args::read(rest, 2, &[], &[])?;
            pack(&a.paths[0], &a.paths[1])
        }
        "sign" => {
            let a = Args::read(rest, 1, &[], &["--key", "--out"])?;
            let key = a.value("--key").ok_or_else(|| usage("sign needs --key <asp.key>"))?;
            let out = a.value("--out").map_or_else(|| a.paths[0].clone(), PathBuf::from);
            sign(&a.paths[0], Path::new(key), &out)
        }
        "verify" => {
            let a = Args::read(rest, 1, &[], &["--catalog", "--online-key", "--offline-key"])?;
            let given = [(KeyRole::Online, "--online-key"), (KeyRole::Offline, "--offline-key")];
            let keys: Vec<(KeyRole, &OsString)> =
                given.iter().flat_map(|&(role, option)| a.values(option).into_iter().map(move |k| (role, k))).collect();
            let index = a.value("--catalog").map(Path::new);
            if index.is_none() && !keys.is_empty() {
                return Err(usage("--online-key and --offline-key check the index --catalog names"));
            }
            verify(&a.paths[0], index, &keys)
        }
        "init" => {
            let a = Args::read(rest, 1, &["--page", "--service", "--action"], &["--id", "--name"])?;
            let forms = [("--page", Form::Page), ("--service", Form::Service), ("--action", Form::Action)];
            let chosen: Vec<Form> = forms.into_iter().filter(|(s, _)| a.has(s)).map(|(_, form)| form).collect();
            let form = match chosen[..] {
                [] => Form::Page,
                [form] => form,
                _ => return Err(usage("choose one of --page, --service and --action")),
            };
            init(&a.paths[0], form, a.value("--id"), a.value("--name"))
        }
        "help" | "--help" | "-h" => {
            println!("{USAGE}");
            Ok(())
        }
        other => Err(usage(format!("unknown command \"{other}\""))),
    }
}

/// Makes a key pair in `dir`: `asp.key` and `author.pub`.
fn keygen(dir: &Path, force: bool, unencrypted: bool) -> Result<(), Failure> {
    if fs::symlink_metadata(dir.join(MANIFEST)).is_ok() {
        return Err(failed("Make the key outside the plugin's folder, so it never goes into a package."));
    }
    let (secret, public) = (dir.join(SECRET_KEY), dir.join(AUTHOR_KEY));
    let there: Vec<&PathBuf> = [&secret, &public].into_iter().filter(|p| fs::symlink_metadata(p).is_ok()).collect();
    if let Some(path) = there.first()
        && !force
    {
        return Err(failed(format!("{} is there already; --force replaces it.", path.display())));
    }
    fs::create_dir_all(dir).map_err(at(dir))?;
    let (pair, comment) = if unencrypted {
        (KeyPair::generate_unencrypted_keypair()?, "asp secret key, not encrypted")
    } else {
        let pair = KeyPair::generate_encrypted_keypair(None)?;
        // minisign takes an empty password, which opens the key to anyone.
        if SecretKey::from_box(pair.sk.to_box(None)?, Some(String::new())).is_ok() {
            return Err(failed("Give the key a password, or make it with --unencrypted."));
        }
        (pair, "asp secret key, encrypted")
    };
    let id = trust::key_id(&pair.pk.to_base64())?;
    // Both are written beside their place first. author.pub takes its place
    // first, and gets its old line back when asp.key can't take its own, so
    // a failure never leaves a pair that doesn't match.
    let new_secret = key_file(dir, pair.sk.to_box(Some(comment))?.into_string().as_bytes(), true)?;
    let new_public = key_file(dir, format!("{}\n", pair.pk.to_base64()).as_bytes(), false)?;
    let old_public = fs::read(&public).ok();
    new_public.persist(&public).map_err(|e| at(&public)(e.error))?;
    if let Err(e) = new_secret.persist(&secret) {
        let _ = match old_public {
            Some(old) => fs::write(&public, old),
            None => fs::remove_file(&public),
        };
        return Err(at(&secret)(e.error));
    }
    if !there.is_empty() {
        eprintln!("warning: plugins pinned to the old key won't take updates signed with this one.");
    }
    println!("Made key {id}: {} (keep it secret, and keep a copy) and {}.", secret.display(), public.display());
    Ok(())
}

/// A temp file in `dir` that holds a key file's `bytes`, to take its place.
/// Only its owner may read a `secret` one, from the start (Linux and macOS).
fn key_file(dir: &Path, bytes: &[u8], secret: bool) -> Result<NamedTempFile, Failure> {
    let mut builder = tempfile::Builder::new();
    builder.prefix(".asp-").suffix(".part");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(if secret { 0o600 } else { 0o644 }));
    }
    #[cfg(not(unix))]
    let _ = secret;
    let mut file = builder.tempfile_in(dir).map_err(at(dir))?;
    file.write_all(bytes).map_err(at(dir))?;
    Ok(file)
}

/// Packs the plugin folder `dir` into the package `out`, unsigned.
fn pack(dir: &Path, out: &Path) -> Result<(), Failure> {
    // asp writes beside the package as it works, and a package in its own
    // folder would land in its next version.
    if let (Ok(dir), Ok(beside)) = (dir.canonicalize(), folder_of(out).canonicalize())
        && beside.starts_with(dir)
    {
        return Err(failed("Write the package outside the plugin's folder."));
    }
    let mut lock = package::lock_folder(dir).map_err(plugin_at(dir))?;
    refuse_secret_keys(dir, &lock)?;
    let path = dir.join(MANIFEST);
    let parsed = read_manifest(&manifest::read_file(&path).map_err(at(&path))?)?;
    warn_all(&parsed.warnings);
    let m = &parsed.manifest;
    let launches = m.launches();
    let programs: Vec<&str> = launches
        .iter()
        .filter_map(|(_, launch)| match launch {
            Ok(Launch::Exe { program, .. }) => Some(program.as_str()),
            _ => None,
        })
        .collect();
    for f in &mut lock.files {
        if programs.contains(&f.path.as_str()) {
            f.exec = true;
        } else if f.exec {
            warn(&format!("{} may run on this computer, so the package marks it to run too.", f.path));
        }
    }
    check_platforms(m, &lock)?;
    package::check_pages(dir, Some(&lock), &m.pages).map_err(plugin_at(dir))?;
    package::check_pictures(dir, &lock, m).map_err(plugin_at(dir))?;
    warn_of_banner(dir, m)?;
    let mut lock_bytes = serde_json::to_vec_pretty(&lock).expect("a files.lock is JSON");
    lock_bytes.push(b'\n');
    save(out, dir, &lock, &lock_bytes, &[])?;
    println!("Packed {} {} into {}, unsigned.", m.id, m.version, out.display());
    Ok(())
}

/// Refuses the plugin folder `dir` when one of its files (those of `lock`)
/// is a secret key, as `keygen` or minisign writes one, or is named
/// `asp.key`: its package would let anyone sign as its author.
fn refuse_secret_keys(dir: &Path, lock: &FilesLock) -> Result<(), Failure> {
    for f in &lock.files {
        let named = f.path.rsplit('/').next().is_some_and(|name| name.eq_ignore_ascii_case(SECRET_KEY));
        let path = file_in(dir, &f.path);
        if named || (f.size <= MAX_KEY_FILE && secret_key_lock(&fs::read(&path).map_err(at(&path))?).is_some()) {
            return Err(failed(format!(
                "The plugin's {} is a secret key, which would let anyone sign as you. Move it out of the plugin's folder.",
                f.path
            )));
        }
    }
    Ok(())
}

/// Checks each platform the plugin `m` runs on ([`Manifest::launches`]) as
/// AutoScene checks it at install there: its command, and the files that
/// starts in `lock` ([`package::check_launch`]). Warns of each platform it
/// can't run on, and fails when that is all of them. A plugin that runs no
/// program (only pages) runs on every platform.
fn check_platforms(m: &Manifest, lock: &FilesLock) -> Result<(), Failure> {
    if !m.runs_program() {
        return Ok(());
    }
    // Why it can't run, with the platforms that holds for.
    let mut cant: Vec<(String, Vec<String>)> = Vec::new();
    let mut runs = false;
    for (platform, launch) in m.launches() {
        let why = match launch.and_then(|launch| package::check_launch(lock, &launch, platform)) {
            Ok(()) => {
                runs = true;
                continue;
            }
            // A plugin that doesn't name its platforms runs where it has a command.
            Err(PluginError::Platform) if m.platforms.is_empty() => continue,
            Err(PluginError::Platform) => {
                "The plugin's platforms names it, but runtime.command has no command for it.".into()
            }
            Err(e) => e.to_string(),
        };
        let here = if platform == current_platform() { " (this computer)" } else { "" };
        let platform = format!("{platform}{here}");
        match cant.iter_mut().find(|(known, _)| *known == why) {
            Some((_, platforms)) => platforms.push(platform),
            None => cant.push((why, vec![platform])),
        }
    }
    for (why, platforms) in &cant {
        warn(&format!("It won't run on {}: {why}", platforms.join(", ")));
    }
    match (runs, cant.is_empty()) {
        (true, _) => Ok(()),
        (false, true) => Err(failed("The plugin runs on no platform this AutoScene knows.")),
        (false, false) => Err(failed("The plugin runs on none of its platforms.")),
    }
}

/// Warns of a banner that cards show soft (smaller than 960×480) or crop
/// (its sides more than 10% off 2:1), as its header says. Never refuses
/// one: AutoScene shows it either way.
fn warn_of_banner(dir: &Path, m: &Manifest) -> Result<(), Failure> {
    let Some(banner) = &m.banner else { return Ok(()) };
    let path = file_in(dir, banner);
    let mut bytes = Vec::new();
    File::open(&path).and_then(|f| f.take(MAX_BANNER).read_to_end(&mut bytes)).map_err(at(&path))?;
    let Some((w, h)) = PictureFormat::of(banner).and_then(|f| package::picture_size(f, &bytes)) else {
        return Ok(());
    };
    if w < 960 || h < 480 {
        warn(&format!("The banner is {w}×{h}. Cards show it soft below 1200×600."));
    }
    if (f64::from(w) / f64::from(h) / 2.0 - 1.0).abs() > 0.1 {
        warn(&format!("The banner is {w}×{h}. Cards crop it to 2:1."));
    }
    Ok(())
}

/// Signs the package `input` with the secret key file `key`, and writes it
/// with the signature and the author's key to `out` (which may be `input`).
fn sign(input: &Path, key: &Path, out: &Path) -> Result<(), Failure> {
    let secret = secret_key(key)?;
    let public = PublicKey::from_secret_key(&secret)?;
    let scratch = scratch_beside(out)?;
    let dir = scratch.path().join("plugin");
    let (unpacked, parsed) = open(input, &dir)?;
    let m = &parsed.manifest;
    let lock = &unpacked.lock_bytes;
    let comment = trust::trusted_comment(&m.id, &m.version, &package::sha256_hex(lock));
    let signature = minisign::sign(Some(&public), &secret, lock.as_slice(), Some(&comment), None)?.into_string();
    let author_key = format!("{}\n", public.to_base64());
    // As AutoScene would judge it, before it is written.
    let verdict = trust::judge(lock, Some(&signature), Some(&author_key), m, None)?;
    let beside = [(SIG, signature.as_bytes()), (AUTHOR_KEY, author_key.as_bytes())];
    save(out, &dir, &unpacked.lock, lock, &beside)?;
    println!("Signed {} {} into {}: {}.", m.id, m.version, out.display(), trust_text(&verdict));
    Ok(())
}

/// Checks the package `path` and says the trust AutoScene would give it by
/// the catalog `index` (none: no catalog), checked against AutoScene's
/// catalog keys and the `keys` given.
fn verify(path: &Path, index: Option<&Path>, keys: &[(KeyRole, &OsString)]) -> Result<(), Failure> {
    let catalog = match index {
        Some(index) => read_catalog(index, keys)?,
        None => Catalog::none(),
    };
    let scratch = tempfile::tempdir()?;
    let dir = scratch.path().join("plugin");
    let (unpacked, parsed) = open(path, &dir)?;
    warn_all(&parsed.warnings);
    let m = &parsed.manifest;
    check_platforms(m, &unpacked.lock)?;
    package::check_pages(&dir, Some(&unpacked.lock), &m.pages)?;
    package::check_pictures(&dir, &unpacked.lock, m)?;
    warn_of_banner(&dir, m)?;
    let (sig, author_key) = (unpacked.signature.as_deref(), unpacked.author_key.as_deref());
    let lock = Some(unpacked.lock_bytes.as_slice());
    let (verdict, _) = store::verdict(m, lock, sig, author_key, None, &catalog, catalog::now())?;
    println!("{} {}: {}", m.id, m.version, trust_text(&verdict));
    Ok(())
}

/// The catalog of the index `path` (an `index.json`, with its
/// `index.json.minisig` beside it), as an AutoScene that holds none takes
/// it: signed by one of AutoScene's catalog keys or of `keys`, and living.
fn read_catalog(path: &Path, keys: &[(KeyRole, &OsString)]) -> Result<Catalog, Failure> {
    // A key pasted, or read from a .pub file, may end in a space or a CR.
    let keys = keys
        .iter()
        .map(|&(role, k)| {
            Ok((role, k.to_str().map(str::trim).ok_or_else(|| failed("That isn't a minisign public key."))?))
        })
        .collect::<Result<Vec<(KeyRole, &str)>, Failure>>()?;
    let keys = CatalogKeys::builtin().with(&keys)?;
    let mut sig_path = path.as_os_str().to_owned();
    sig_path.push(".minisig");
    let sig_path = PathBuf::from(sig_path);
    // Read no further than the check takes: it refuses a larger file.
    let bytes = read_at_most(path, catalog::MAX_INDEX_BYTES)?;
    let sig = String::from_utf8_lossy(&read_at_most(&sig_path, catalog::MAX_SIG_BYTES)?).into_owned();
    let (held, now) = (CatalogState::default(), catalog::now());
    let Checked::Newer(accepted) = catalog::check(&bytes, &sig, &keys, &held, now)? else {
        unreachable!("an empty state holds no index to be the same as");
    };
    let state = held.after(&accepted, now);
    Ok(Catalog::new(Some(accepted.index), state))
}

/// The file `path`, to a byte more than `max`.
fn read_at_most(path: &Path, max: u64) -> Result<Vec<u8>, Failure> {
    let mut bytes = Vec::new();
    File::open(path).and_then(|f| f.take(max + 1).read_to_end(&mut bytes)).map_err(at(path))?;
    Ok(bytes)
}

/// Reviewed, Pinned (key <id>) or Unreviewed.
fn trust_text(verdict: &Verdict) -> String {
    match (verdict.trust, &verdict.author_key) {
        (Trust::Reviewed, _) => "Reviewed".into(),
        (Trust::Pinned, Some(key)) => format!("Pinned (key {})", key.key_id),
        (Trust::Pinned, None) => "Pinned".into(),
        (Trust::Unreviewed, _) => "Unreviewed".into(),
    }
}

/// What `init` makes.
#[derive(Clone, Copy)]
enum Form {
    /// A plugin that runs no program: one page, in the rail.
    Page,
    /// A page, and the service it asks (protocol 2).
    Service,
    /// A program of protocol 2 with one export action.
    Action,
}

impl Form {
    fn what(self) -> &'static str {
        match self {
            Self::Page => "a page",
            Self::Service => "a page and its service",
            Self::Action => "an export action",
        }
    }

    /// The template blocks it keeps (see [`fill`]).
    fn blocks(self) -> &'static [&'static str] {
        match self {
            Self::Page => &["page"],
            Self::Service => &["page", "program", "service"],
            Self::Action => &["program", "action"],
        }
    }

    /// Its files, by their path in the plugin: the templates [`fill`]
    /// makes them of, and those it gets as they are.
    fn files(self) -> Vec<(&'static str, Template)> {
        let manifest = match self {
            Self::Page => include_str!("../../templates/plugin.page.json"),
            Self::Service => include_str!("../../templates/plugin.service.json"),
            Self::Action => include_str!("../../templates/plugin.action.json"),
        };
        let mut files = vec![
            (MANIFEST, Template::Fill(manifest)),
            ("README.md", Template::Fill(include_str!("../../templates/README.md"))),
            (".autoscene/plugin.schema.json", Template::Copy(SCHEMA)),
            (".vscode/settings.json", Template::Copy(include_str!("../../templates/vscode-settings.json"))),
        ];
        let blocks = self.blocks();
        if blocks.contains(&"page") {
            files.push(("ui/index.html", Template::Fill(include_str!("../../templates/ui/index.html"))));
            files.push(("ui/app.js", Template::Fill(include_str!("../../templates/ui/app.js"))));
        }
        if blocks.contains(&"program") {
            files.push(("main.py", Template::Copy(include_str!("../../templates/main.py"))));
            files.push(("main.py.lock", Template::Copy(include_str!("../../templates/main.py.lock"))));
            files.push(("autoscene.py", Template::Copy(include_str!("../../templates/autoscene.py"))));
        }
        if blocks.contains(&"page") {
            files.extend(TYPES.map(|(path, text)| (path, Template::Copy(text))));
        }
        files
    }
}

/// A file of a plugin `init` makes.
#[derive(Clone, Copy)]
enum Template {
    /// A template, which [`fill`] makes the file of.
    Fill(&'static str),
    /// The file as it is.
    Copy(&'static str),
}

/// Makes a plugin of `form` in `dir`, a new or empty folder: its id is
/// `id`, or `dev.example.` and the folder's name in lowercase with dashes,
/// and its name is `name`, or the folder's. It needs at least the minor
/// version of the AutoScene asp comes with. Nothing is written when one of
/// them is wrong, and no file that is there is ever written over.
fn init(dir: &Path, form: Form, id: Option<&OsString>, name: Option<&OsString>) -> Result<(), Failure> {
    let start = "asp init makes a plugin in a new folder, or an empty one.";
    if let Ok(meta) = fs::metadata(dir) {
        if !meta.is_dir() {
            return Err(failed(format!("{} is a file. {start}", dir.display())));
        }
        if fs::read_dir(dir).map_err(at(dir))?.next().is_some() {
            return Err(failed(format!("{} has files in it. {start}", dir.display())));
        }
    }
    // `.` and `..` name a folder too.
    let full = fs::canonicalize(dir).or_else(|_| std::path::absolute(dir)).map_err(at(dir))?;
    let folder = full.file_name().map(|n| n.to_string_lossy().into_owned());
    let name = match name {
        Some(name) => name.to_str().ok_or_else(|| failed("The name must be text."))?.trim().to_owned(),
        None => folder.clone().unwrap_or_default(),
    };
    if name.is_empty() || name.chars().count() > 40 || name.chars().any(char::is_control) {
        return Err(failed("Give the plugin a name of 1 to 40 characters with --name."));
    }
    let id = match id {
        Some(id) => id.to_str().ok_or_else(|| failed("The id must be text."))?.to_owned(),
        None => format!("dev.example.{}", folder.as_deref().map(id_part).unwrap_or_default()),
    };
    if id.ends_with('.') {
        return Err(failed("Give the plugin an id with --id, such as dev.example.my-plugin."));
    }
    let id = manifest::PluginId::parse(&id)?;
    let app = app();
    let autoscene = format!(">={}.{}.0, <{}", app.major, app.minor, app.major + 1);
    let dir_name = folder.as_deref().unwrap_or("<plugin-folder>");

    let files: Vec<(&str, String)> = form
        .files()
        .into_iter()
        .map(|(path, template)| match template {
            Template::Copy(text) => (path, text.to_owned()),
            Template::Fill(template) => {
                // Each value as text of the file's kind.
                let text = |s: &str| match path.rsplit('.').next() {
                    Some("json") => json_text(s),
                    Some("html") => html_text(s),
                    _ => s.to_owned(),
                };
                let values = [
                    ("id", text(id.as_str())),
                    ("name", text(&name)),
                    ("autoscene", text(&autoscene)),
                    ("dir", text(&shell_word(dir_name))),
                    ("package", text(&shell_word(&format!("{dir_name}.asp")))),
                ];
                let filled = fill(template, form.blocks(), &values);
                (path, filled)
            }
        })
        .collect();
    let (_, plugin_json) = files.iter().find(|(path, _)| *path == MANIFEST).expect("every form has a plugin.json");
    warn_all(&read_manifest(plugin_json.as_bytes())?.warnings);

    for (path, text) in &files {
        let file = file_in(dir, path);
        let parent = folder_of(&file);
        fs::create_dir_all(parent).map_err(at(parent))?;
        // A file that came since the folder was looked at stays as it is.
        let mut out = File::options().write(true).create_new(true).open(&file).map_err(at(&file))?;
        out.write_all(text.as_bytes()).map_err(at(&file))?;
    }
    println!("Made {name} ({id}) in {}: {}. Its README.md says what comes next.", dir.display(), form.what());
    Ok(())
}

/// The part of a default id a folder's name gives: lowercase letters and
/// digits, a dash for each run of anything else, at most 39 characters.
fn id_part(folder: &str) -> String {
    let mut part = String::new();
    for c in folder.chars() {
        match c.to_ascii_lowercase() {
            c @ ('a'..='z' | '0'..='9') => part.push(c),
            _ if !part.is_empty() && !part.ends_with('-') => part.push('-'),
            _ => {}
        }
    }
    part.truncate(39);
    part.trim_end_matches('-').to_owned()
}

/// A template as a plugin's file: the lines between `{{#block}}` and
/// `{{/block}}` are kept for the `blocks` the plugin has, those between
/// `{{^block}}` and `{{/block}}` for the others, and each `{{key}}` of
/// `values` takes its value (once: a value is never read for keys). A
/// marker sits on a line of its own (in a comment, so the template stays a
/// file of its kind), which goes.
fn fill(template: &str, blocks: &[&str], values: &[(&str, String)]) -> String {
    let mut kept = String::new();
    // Whether each block that is open keeps its lines.
    let mut open: Vec<bool> = Vec::new();
    for line in template.split_inclusive('\n') {
        match marker(line) {
            Some(('#', block)) => open.push(blocks.contains(&block)),
            Some(('^', block)) => open.push(!blocks.contains(&block)),
            Some(_) => {
                open.pop();
            }
            None if open.iter().all(|&keep| keep) => kept.push_str(line),
            None => {}
        }
    }
    let mut out = String::new();
    let mut rest = kept.as_str();
    while let Some((before, after)) = rest.split_once("{{") {
        let value = |(key, after)| Some((values.iter().find(|(k, _)| *k == key)?.1.as_str(), after));
        let (text, next) = after.split_once("}}").and_then(value).unwrap_or(("{{", after));
        out.push_str(before);
        out.push_str(text);
        rest = next;
    }
    out.push_str(rest);
    out
}

/// The block marker `line` holds, if it holds one: `#`, `^` or `/`, and the
/// block's name.
fn marker(line: &str) -> Option<(char, &str)> {
    let (_, rest) = line.split_once("{{")?;
    let (inside, _) = rest.split_once("}}")?;
    let kind = inside.chars().next().filter(|c| "#^/".contains(*c))?;
    Some((kind, &inside[1..]))
}

/// `s` as it goes inside a JSON string.
fn json_text(s: &str) -> String {
    let quoted = serde_json::to_string(s).expect("text is JSON");
    quoted[1..quoted.len() - 1].to_owned()
}

/// `s` as HTML text.
fn html_text(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

/// `s` as one word of a command line, in quotes when a shell would split it.
fn shell_word(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)) {
        s.to_owned()
    } else {
        format!("\"{s}\"")
    }
}

/// Unpacks the package `path` into the new folder `into` and reads its
/// manifest.
fn open(path: &Path, into: &Path) -> Result<(Unpacked, Parsed), Failure> {
    let unpacked = package::unpack(path, into).map_err(plugin_at(path))?;
    let parsed = read_manifest(&manifest::read_file(&into.join(MANIFEST))?)?;
    Ok((unpacked, parsed))
}

/// Reads a `plugin.json` for any AutoScene, since making and checking a
/// package don't depend on which, with a warning when the one asp comes
/// with wouldn't install it.
fn read_manifest(bytes: &[u8]) -> Result<Parsed, Failure> {
    let mut parsed = manifest::parse_for_any_app(bytes)?;
    let (m, app) = (&parsed.manifest, app());
    if m.requirement_met(&app).is_err() {
        let wants = &m.engines.autoscene;
        parsed.warnings.push(format!("AutoScene {app} won't install it: its engines.autoscene is \"{wants}\"."));
    }
    Ok(parsed)
}

/// Writes the package `out`: the files of `lock` from the folder `dir`, the
/// lock, and `beside` (a signature and the author's key). The same files
/// give the same bytes: they go in the lock's order, then the lock and
/// `beside`, dated 1980 and made on Unix with the mode the lock gives, each
/// deflated but those [`to_store`] picks. The package is unpacked as
/// AutoScene would before it takes the place of `out`, which stays as it
/// was when that fails.
fn save(out: &Path, dir: &Path, lock: &FilesLock, lock_bytes: &[u8], beside: &[(&str, &[u8])]) -> Result<(), Failure> {
    let mut file = write_zip(out, dir, lock, lock_bytes, beside, &HashSet::new())?;
    let stored = to_store(file.path(), lock)?;
    if !stored.is_empty() {
        file = write_zip(out, dir, lock, lock_bytes, beside, &stored)?;
    }
    let scratch = scratch_beside(out)?;
    package::unpack(file.path(), &scratch.path().join("plugin"))?;
    file.persist(out).map_err(|e| at(out)(e.error))?;
    Ok(())
}

/// Writes a package (see [`save`]) into a temp file beside `out`, with the
/// `stored` files stored and the others deflated.
fn write_zip(
    out: &Path,
    dir: &Path,
    lock: &FilesLock,
    lock_bytes: &[u8],
    beside: &[(&str, &[u8])],
    stored: &HashSet<&str>,
) -> Result<NamedTempFile, Failure> {
    let options = |exec: bool, method: CompressionMethod| {
        SimpleFileOptions::default()
            .compression_method(method)
            .last_modified_time(DateTime::default())
            .system(System::Unix)
            .unix_permissions(if exec { 0o755 } else { 0o644 })
    };
    let file = tempfile::Builder::new().prefix(".asp-").suffix(".part").tempfile_in(folder_of(out)).map_err(at(out))?;
    let mut zip = ZipWriter::new(BufWriter::new(file.as_file()));
    for f in &lock.files {
        let path = file_in(dir, &f.path);
        let method =
            if stored.contains(f.path.as_str()) { CompressionMethod::Stored } else { CompressionMethod::Deflated };
        zip.start_file(f.path.as_str(), options(f.exec, method))?;
        io::copy(&mut File::open(&path).map_err(at(&path))?, &mut zip).map_err(at(&path))?;
    }
    for (name, bytes) in [(LOCK, lock_bytes)].into_iter().chain(beside.iter().copied()) {
        zip.start_file(name, options(false, CompressionMethod::Deflated))?;
        zip.write_all(bytes)?;
    }
    zip.finish()?.into_inner().map_err(|e| at(out)(e.into_error()))?;
    Ok(file)
}

/// The files of `lock` that the package `zip`, all deflated, must store
/// instead for AutoScene to take it ([`package::packed_too_tightly`]): each
/// that deflate packs too tightly (zero-padded model weights, say), then,
/// while all of them together are still packed too tightly, those deflate
/// shrinks most, in the lock's order among equals. The pick depends on the
/// files alone, so the same files still give the same package.
fn to_store<'a>(zip: &Path, lock: &'a FilesLock) -> Result<HashSet<&'a str>, Failure> {
    let mut archive = ZipArchive::new(File::open(zip).map_err(at(zip))?)?;
    let mut packed = HashMap::new();
    for i in 0..archive.len() {
        let entry = archive.by_index_raw(i)?;
        packed.insert(entry.name()?.into_owned(), entry.compressed_size());
    }
    let deflated = |f: &LockedFile| packed.get(&f.path).copied().unwrap_or(f.size);
    let tight = |f: &LockedFile| package::packed_too_tightly(f.size, deflated(f));
    let mut stored: HashSet<&str> = lock.files.iter().filter(|f| tight(f)).map(|f| f.path.as_str()).collect();
    let size: u64 = lock.files.iter().map(|f| f.size).sum();
    let mut all_packed: u64 = lock.files.iter().map(|f| if tight(f) { f.size } else { deflated(f) }).sum();
    let mut rest: Vec<&LockedFile> = lock.files.iter().filter(|f| !tight(f)).collect();
    rest.sort_by_key(|f| Reverse(f.size.saturating_sub(deflated(f))));
    for f in rest {
        if !package::packed_too_tightly(size, all_packed) {
            break;
        }
        stored.insert(f.path.as_str());
        all_packed = all_packed - deflated(f) + f.size;
    }
    Ok(stored)
}

/// A folder to work in beside `out`, on its drive, which has room for it.
fn scratch_beside(out: &Path) -> Result<TempDir, Failure> {
    tempfile::Builder::new().prefix(".asp-").tempdir_in(folder_of(out)).map_err(at(out))
}

/// The folder `path` lies in.
fn folder_of(path: &Path) -> &Path {
    path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."))
}

/// Reads the secret key file `path` (as `keygen` or minisign writes it),
/// asking for its password when it is locked with one.
fn secret_key(path: &Path) -> Result<SecretKey, Failure> {
    let text = fs::read_to_string(path).map_err(at(path))?;
    let key = if secret_key_lock(text.as_bytes()) == Some(*b"Sc") {
        SecretKey::from_box(text.into(), None)
    } else {
        SecretKey::from_unencrypted_box(text.into())
    };
    key.map_err(|e| failed(format!("{}: {e}", path.display())))
}

/// How the file `bytes` locks the minisign secret key it is, if it is one:
/// `Sc` with a password (scrypt), two zero bytes not at all. Its second line
/// is the key, 158 bytes that start with `Ed`, how it is locked, and `B2`.
fn secret_key_lock(bytes: &[u8]) -> Option<[u8; 2]> {
    let line = bytes.split(|&b| b == b'\n').nth(1)?;
    let raw = STANDARD.decode(line.trim_ascii()).ok()?;
    (raw.len() == 158 && raw.starts_with(b"Ed") && raw[4..6] == *b"B2").then(|| [raw[2], raw[3]])
}

/// The AutoScene a manifest is read for: the one asp comes with.
fn app() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("the workspace version is a version")
}

fn warn(warning: &str) {
    eprintln!("warning: {warning}");
}

fn warn_all(warnings: &[String]) {
    warnings.iter().for_each(|w| warn(w));
}

/// A command's arguments: its paths in order, its switches, and its options
/// with their values (an option may come more than once).
struct Args<'a> {
    paths: Vec<PathBuf>,
    switches: Vec<&'static str>,
    options: Vec<(&'static str, &'a OsString)>,
}

impl<'a> Args<'a> {
    /// Reads `args`: `paths` paths, and any of `switches` and `options`
    /// (each followed by its value).
    fn read(
        args: &'a [OsString],
        paths: usize,
        switches: &[&'static str],
        options: &[&'static str],
    ) -> Result<Self, Failure> {
        let mut read = Self { paths: Vec::new(), switches: Vec::new(), options: Vec::new() };
        let mut it = args.iter();
        while let Some(arg) = it.next() {
            let text = arg.to_str().unwrap_or_default();
            if let Some(&switch) = switches.iter().find(|&&s| s == text) {
                read.switches.push(switch);
            } else if let Some(&option) = options.iter().find(|&&o| o == text) {
                let value = it.next().ok_or_else(|| usage(format!("{option} needs a value")))?;
                read.options.push((option, value));
            } else if text.starts_with("--") {
                return Err(usage(format!("unknown option {text}")));
            } else {
                read.paths.push(PathBuf::from(arg));
            }
        }
        if read.paths.len() != paths {
            return Err(usage(format!("expected {paths} path(s), got {}", read.paths.len())));
        }
        Ok(read)
    }

    fn has(&self, switch: &str) -> bool {
        self.switches.contains(&switch)
    }

    /// The option's last value.
    fn value(&self, option: &str) -> Option<&'a OsString> {
        self.values(option).pop()
    }

    fn values(&self, option: &str) -> Vec<&'a OsString> {
        self.options.iter().filter(|(o, _)| *o == option).map(|(_, v)| *v).collect()
    }
}

/// Why asp stopped: wrong arguments (with the usage), or a failure.
enum Failure {
    Usage(String),
    Failed(String),
}

fn usage(why: impl Into<String>) -> Failure {
    Failure::Usage(why.into())
}

fn failed(why: impl Into<String>) -> Failure {
    Failure::Failed(why.into())
}

/// A file error at `path`, in the system's words.
fn at(path: &Path) -> impl FnOnce(io::Error) -> Failure + '_ {
    move |e| failed(format!("{}: {e}", path.display()))
}

/// A plugin error, with `path` when it is a file error.
fn plugin_at(path: &Path) -> impl FnOnce(PluginError) -> Failure + '_ {
    move |e| match e {
        PluginError::Io(e) => at(path)(e),
        e => e.into(),
    }
}

impl From<PluginError> for Failure {
    /// The sentence AutoScene would show, and what lies behind it.
    fn from(e: PluginError) -> Self {
        let (message, detail) = match e {
            PluginError::Io(e) => return failed(e.to_string()),
            e => (e.to_string(), e.detail()),
        };
        failed(if detail == message { message } else { format!("{message} ({detail})") })
    }
}

impl From<io::Error> for Failure {
    fn from(e: io::Error) -> Self {
        failed(e.to_string())
    }
}

impl From<ZipError> for Failure {
    fn from(e: ZipError) -> Self {
        failed(format!("The package couldn't be written: {e}"))
    }
}

impl From<minisign::PError> for Failure {
    fn from(e: minisign::PError) -> Self {
        failed(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_locked_key_is_told_from_an_open_one() {
        let text = |pair: KeyPair| pair.sk.to_box(Some("asp secret key")).unwrap().into_string();
        let locked = text(KeyPair::generate_encrypted_keypair(Some("a password".into())).unwrap());
        let open = text(KeyPair::generate_unencrypted_keypair().unwrap());
        assert_eq!(secret_key_lock(locked.as_bytes()), Some(*b"Sc"));
        assert_eq!(secret_key_lock(open.replace('\n', "\r\n").as_bytes()), Some([0, 0]));
        let public = KeyPair::generate_unencrypted_keypair().unwrap().pk.to_box().unwrap().into_string();
        for not_one in [public.as_str(), "", "untrusted comment: x\nRWQ=\n", &open[..open.len() / 2]] {
            assert_eq!(secret_key_lock(not_one.as_bytes()), None, "{not_one}");
        }
    }

    #[test]
    fn a_package_autoscene_would_refuse_is_never_written() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("plugin");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join(MANIFEST), b"{}").unwrap();
        fs::write(dir.join("data.txt"), b"before").unwrap();
        let lock = package::lock_folder(&dir).unwrap();
        // The file changes after it was listed: its lock no longer fits.
        fs::write(dir.join("data.txt"), b"after, and longer").unwrap();
        let out = tmp.path().join("plugin.asp");
        fs::write(&out, b"the old package").unwrap();
        let Err(Failure::Failed(why)) = save(&out, &dir, &lock, &serde_json::to_vec(&lock).unwrap(), &[]) else {
            panic!("saved a package whose data.txt isn't the size its lock says")
        };
        assert_eq!(why, "The plugin package's data.txt isn't the size its list says.");
        assert_eq!(fs::read(&out).unwrap(), b"the old package");
        let mut left: Vec<_> = fs::read_dir(tmp.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        left.sort();
        assert_eq!(left, ["plugin", "plugin.asp"]);
    }
}

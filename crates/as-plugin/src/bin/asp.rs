//! `asp`: makes and checks AutoScene plugin packages (`.asp`).
//!
//! ```text
//! asp keygen <dir> [--force] [--unencrypted]
//! asp pack <plugin-folder> <out.asp>
//! asp sign <in.asp> --key <asp.key> [--out <out.asp>]
//! asp verify <in.asp> [--catalog <index.json> [--online-key <RW…>]… [--offline-key <RW…>]…]
//! asp init <dir> [--page | --service | --action | --wasm <rust|python>] [--id <id>] [--name <name>]
//! asp check <plugin.wasm | plugin-folder | in.asp>
//! asp catalog new <index.json>
//! asp catalog add <index.json> <pkg.asp> --url <https://…>… --repo <https://github.com/o/r> --commit <40 hex>
//!                 [--path <dir>] --author <name> [--author-key <RW…>]
//! asp catalog withdrawn <index.json> <withdrawn.json>
//! asp catalog certify --epoch <n> --key <offline.key> [--withdraw-key <ID>]… [--online-key <RW…>]…
//!                     [--previous <epoch.minisig>] --out <epoch.minisig> [--force]
//! asp catalog epoch <index.json> <epoch.minisig>
//! asp catalog bump <index.json> [--days 30]
//! asp catalog sign <index.json> --key-env <NAME> [--password-env <NAME>]
//! asp catalog verify <index.json> [--previous <old.json>] [--online-key <RW…>]… [--offline-key <RW…>]…
//! asp catalog diff <index.json> <pkg.asp>
//! ```
//!
//! - `init` makes a plugin to start from in `<dir>`, a new or empty folder,
//!   from the templates in `crates/as-plugin/templates`: a page in the rail
//!   (`--page`, the default), a page and the service it asks (`--service`),
//!   a program with an export action (`--action`), or a WebAssembly plugin
//!   in Rust or Python with a command (`--wasm`), whose source sits beside
//!   the plugin's folder `plugin/` with `wit/plugin.wit`. Each gets
//!   `plugin.json`'s schema in `.autoscene/` and a `.vscode/settings.json`
//!   that checks `plugin.json` against it; a page also gets the SDK's types
//!   in `.autoscene/types/`. `pack` leaves both folders out. Its id is
//!   `--id`, or `dev.example.` and the folder's name; its name is `--name`,
//!   or the folder's.
//! - `check` checks a WebAssembly component as AutoScene does before it
//!   runs it, and lists what it imports: a plugin folder's or a package's,
//!   against its `plugin.json` (a package's also against its `files.lock`,
//!   as AutoScene installs it), or a `.wasm` file on its own. An import
//!   that needs a newer AutoScene is named, with what this one offers.
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
//! - `catalog` keeps AutoScene's plugin catalog ([`catalog`]) as its CI
//!   does: `new` starts an empty index (epoch 1, number 0), `add` lists a
//!   package with what it says of itself, `withdrawn` sets the withdrawals,
//!   `epoch` attaches the epoch certificate `certify` signs with the offline
//!   key on the owner's computer (asking for the key's password; it
//!   withdraws again the keys the `--previous` one withdrew), `bump`
//!   numbers and dates the index, `sign` signs it with a key it reads from
//!   the environment and writes nowhere, `verify` checks it as an AutoScene
//!   that holds no catalog (or the `--previous` one) takes it, and `diff`
//!   says in Markdown what a package changes against the newest version
//!   listed and not withdrawn. An index is written pretty-printed with a
//!   trailing newline;
//!   those bytes are what `sign` signs, and it signs no others. Entries and
//!   withdrawals are checked as AutoScene checks them, and `sign` refuses an
//!   index AutoScene would refuse for its form. A key a certificate withdrew
//!   is withdrawn for good: each later certificate withdraws it again.
//!
//! `pack` and `verify` check each platform the plugin runs on as AutoScene
//! does there, warn of one it can't run on, and fail when it runs on none.
//! They check the icon and the banner as AutoScene does, and warn of a
//! banner that cards would show soft or crop. They check a WebAssembly
//! plugin's component as `check` does (`catalog add` too), and `verify`
//! says it runs in a sandbox. A plugin made for another
//! AutoScene than the one asp comes with gets a warning, not an error. A
//! package is unpacked as AutoScene would before it takes the place of the
//! output, so one AutoScene would refuse is never written. Errors exit with
//! 1, wrong arguments with 2.

use std::cmp::Reverse;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use as_plugin::catalog::{
    self, Accepted, Catalog, CatalogKeys, CatalogState, Checked, Entry, Index, MAX_LIFETIME, PackageRef, SourceRef,
};
use as_plugin::keys::FIRST_EPOCH;
use as_plugin::manifest::{self, Parsed};
use as_plugin::package::{
    self, AUTHOR_KEY, LOCK, MANIFEST, MAX_BANNER, MAX_COMPONENT_BYTES, PictureFormat, SIG, file_in,
};
use as_plugin::store;
use as_plugin::trust::{self, Trust, Verdict};
use as_plugin::wasm;
use as_plugin::{
    FilesLock, KeyRole, Launch, LockedFile, Manifest, PluginError, RuntimeKind, Unpacked, current_platform,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use minisign::{KeyPair, PublicKey, SecretKey, SecretKeyBox};
use semver::Version;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::{NamedTempFile, TempDir};
use zip::result::ZipError;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, System, ZipArchive, ZipWriter};

const USAGE: &str = "\
usage: asp keygen <dir> [--force] [--unencrypted]
       asp pack <plugin-folder> <out.asp>
       asp sign <in.asp> --key <asp.key> [--out <out.asp>]
       asp verify <in.asp> [--catalog <index.json> [--online-key <RW…>]… [--offline-key <RW…>]…]
       asp init <dir> [--page | --service | --action | --wasm <rust|python>] [--id <id>] [--name <name>]
       asp check <plugin.wasm | plugin-folder | in.asp>
       asp catalog new <index.json>
       asp catalog add <index.json> <pkg.asp> --url <https://…>… --repo <https://github.com/o/r>
                       --commit <40 hex> [--path <dir>] --author <name> [--author-key <RW…>]
       asp catalog withdrawn <index.json> <withdrawn.json>
       asp catalog certify --epoch <n> --key <offline.key> [--withdraw-key <ID>]… [--online-key <RW…>]…
                           [--previous <epoch.minisig>] --out <epoch.minisig> [--force]
       asp catalog epoch <index.json> <epoch.minisig>
       asp catalog bump <index.json> [--days 30]
       asp catalog sign <index.json> --key-env <NAME> [--password-env <NAME>]
       asp catalog verify <index.json> [--previous <old.json>] [--online-key <RW…>]… [--offline-key <RW…>]…
       asp catalog diff <index.json> <pkg.asp>";

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
/// A day, in seconds.
const DAY: u64 = 86_400;
/// The world a WebAssembly plugin is built against, which `init --wasm`
/// writes beside its source.
const WIT: &str = include_str!("../../wit/plugin.wit");
/// The template of a Rust WebAssembly plugin's `Cargo.lock`: its crate,
/// `{{crate}}`, and the packages it depends on.
const RUST_LOCK: &str = include_str!("../../templates/wasm-rust/Cargo.lock");
/// What `check` says of each import that does nothing without a permission.
const NEEDS: [(&str, &str); 1] = [("wasi:http/", "network")];

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
            let keys = a.catalog_keys();
            let index = a.value("--catalog").map(Path::new);
            if index.is_none() && !keys.is_empty() {
                return Err(usage("--online-key and --offline-key check the index --catalog names"));
            }
            verify(&a.paths[0], index, &keys)
        }
        "init" => {
            let a = Args::read(rest, 1, &["--page", "--service", "--action"], &["--id", "--name", "--wasm"])?;
            let forms = [("--page", Form::Page), ("--service", Form::Service), ("--action", Form::Action)];
            let mut chosen: Vec<Form> = forms.into_iter().filter(|(s, _)| a.has(s)).map(|(_, form)| form).collect();
            for language in a.values("--wasm") {
                chosen.push(match language.to_str() {
                    Some("rust") => Form::WasmRust,
                    Some("python") => Form::WasmPython,
                    _ => return Err(usage("--wasm takes rust or python")),
                });
            }
            let form = match chosen[..] {
                [] => Form::Page,
                [form] => form,
                _ => return Err(usage("choose one of --page, --service, --action and --wasm")),
            };
            init(&a.paths[0], form, a.value("--id"), a.value("--name"))
        }
        "catalog" => catalog_command(rest),
        "check" => {
            let a = Args::read(rest, 1, &[], &[])?;
            check(&a.paths[0])
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

/// Marks the programs the manifest `m` starts to run in `lock`, and its
/// component not to: a component isn't a program, whatever the folder says
/// of it. Gives the other files the folder marks to run, which stay so.
fn mark_to_run(m: &Manifest, lock: &mut FilesLock) -> Vec<String> {
    let (mut programs, mut components) = (Vec::new(), Vec::new());
    for (_, launch) in m.launches() {
        match launch {
            Ok(Launch::Exe { program, .. }) => programs.push(program),
            Ok(Launch::Wasm { component }) => components.push(component),
            _ => {}
        }
    }
    let mut others = Vec::new();
    for f in &mut lock.files {
        if programs.contains(&f.path) {
            f.exec = true;
        } else if components.contains(&f.path) {
            f.exec = false;
        } else if f.exec {
            others.push(f.path.clone());
        }
    }
    others
}

/// Packs the plugin folder `dir` into the package `out`, unsigned.
fn pack(dir: &Path, out: &Path) -> Result<(), Failure> {
    if let Some(plugin) = made_beside(dir) {
        return Err(failed(format!(
            "{} has no plugin.json. The plugin asp init made is in {}: asp pack {} {}",
            dir.display(),
            plugin.display(),
            path_word(&plugin),
            path_word(out)
        )));
    }
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
    if m.is_sandboxed() && !m.platforms.is_empty() {
        warn(
            "platforms limits where AutoScene installs it, though a WebAssembly plugin runs the same on every platform.",
        );
    }
    for marked in mark_to_run(m, &mut lock) {
        warn(&format!("{marked} may run on this computer, so the package marks it to run too."));
    }
    check_platforms(m, &lock)?;
    if m.is_sandboxed() {
        warn_all(&check_component(dir, m)?.checked.warnings);
    }
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
/// program (only pages) runs on every platform. A WebAssembly plugin has
/// one launch, its component, on each platform it names (or all), so what
/// is wrong with the component is said once, for all of them.
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
    let (unpacked, parsed) = unpack_checked(path, &dir)?;
    let m = &parsed.manifest;
    let (sig, author_key) = (unpacked.signature.as_deref(), unpacked.author_key.as_deref());
    let lock = Some(unpacked.lock_bytes.as_slice());
    let (verdict, _) = store::verdict(m, lock, sig, author_key, None, &catalog, catalog::now())?;
    println!("{} {}: {}", m.id, m.version, trust_text(&verdict));
    if m.is_sandboxed() {
        println!("Runs in a sandbox.");
    }
    Ok(())
}

/// The catalog of the index `path` (an `index.json`, with its
/// `index.json.minisig` beside it), as an AutoScene that holds none takes
/// it: signed by one of AutoScene's catalog keys or of `keys`, and living.
fn read_catalog(path: &Path, keys: &[(KeyRole, &OsString)]) -> Result<Catalog, Failure> {
    let keys = catalog_keys(keys)?;
    let (bytes, sig) = read_signed(path)?;
    let now = catalog::now();
    let accepted = take_fresh(&bytes, &sig, &keys, now)?;
    let state = CatalogState::default().after(&accepted, now);
    Ok(Catalog::new(Some(accepted.index), state))
}

/// AutoScene's catalog keys and `keys`, each a minisign public key's base64
/// line.
fn catalog_keys(keys: &[(KeyRole, &OsString)]) -> Result<CatalogKeys, Failure> {
    // A key pasted, or read from a .pub file, may end in a space or a CR.
    let keys = keys
        .iter()
        .map(|&(role, k)| {
            Ok((role, k.to_str().map(str::trim).ok_or_else(|| failed("That isn't a minisign public key."))?))
        })
        .collect::<Result<Vec<(KeyRole, &str)>, Failure>>()?;
    Ok(CatalogKeys::builtin().with(&keys)?)
}

/// The index `path` and the signature beside it (`<path>.minisig`), each
/// read no further than [`catalog::check`] takes: it refuses a larger file.
fn read_signed(path: &Path) -> Result<(Vec<u8>, String), Failure> {
    let bytes = read_at_most(path, catalog::MAX_INDEX_BYTES)?;
    let sig = String::from_utf8_lossy(&read_at_most(&sig_beside(path), catalog::MAX_SIG_BYTES)?).into_owned();
    Ok((bytes, sig))
}

/// The index `bytes`, signed with `sig`, as an AutoScene that holds no
/// catalog takes it at `now`.
fn take_fresh(bytes: &[u8], sig: &str, keys: &CatalogKeys, now: u64) -> Result<Accepted, Failure> {
    match catalog::check(bytes, sig, keys, &CatalogState::default(), now)? {
        Checked::Newer(accepted) => Ok(*accepted),
        Checked::Same => unreachable!("an empty state holds no index to be the same as"),
    }
}

/// `<path>.minisig`, where an index's signature lies.
fn sig_beside(path: &Path) -> PathBuf {
    let mut sig_path = path.as_os_str().to_owned();
    sig_path.push(".minisig");
    PathBuf::from(sig_path)
}

/// The file `path`, to a byte more than `max`.
fn read_at_most(path: &Path, max: u64) -> Result<Vec<u8>, Failure> {
    let mut bytes = Vec::new();
    File::open(path).and_then(|f| f.take(max + 1).read_to_end(&mut bytes)).map_err(at(path))?;
    Ok(bytes)
}

/// Runs `asp catalog <command> …`.
fn catalog_command(args: &[OsString]) -> Result<(), Failure> {
    let Some((command, rest)) = args.split_first() else { return Err(usage("name a catalog command")) };
    match command.to_str().unwrap_or_default() {
        "new" => {
            let a = Args::read(rest, 1, &[], &[])?;
            catalog_new(&a.paths[0])
        }
        "add" => {
            let a = Args::read(rest, 2, &[], &["--url", "--repo", "--commit", "--path", "--author", "--author-key"])?;
            let urls = a.texts("--url")?;
            if urls.is_empty() {
                return Err(usage("catalog add needs --url"));
            }
            let source = SourceRef {
                repo: a.needed("--repo", "catalog add")?.to_owned(),
                commit: a.needed("--commit", "catalog add")?.to_owned(),
                path: a.text("--path")?.unwrap_or_default().to_owned(),
            };
            let author = a.needed("--author", "catalog add")?;
            // A key pasted, or read from a .pub file, may end in a space or a CR.
            let author_key = a.text("--author-key")?.map(str::trim);
            catalog_add(&a.paths[0], &a.paths[1], &urls, source, author, author_key)
        }
        "withdrawn" => {
            let a = Args::read(rest, 2, &[], &[])?;
            catalog_withdrawn(&a.paths[0], &a.paths[1])
        }
        "certify" => {
            let options = ["--epoch", "--key", "--withdraw-key", "--online-key", "--previous", "--out"];
            let a = Args::read(rest, 0, &["--force"], &options)?;
            let epoch = a.needed("--epoch", "catalog certify")?;
            let epoch = epoch.parse().ok().filter(|&e| e >= FIRST_EPOCH);
            let epoch = epoch.ok_or_else(|| usage(format!("--epoch takes a number from {FIRST_EPOCH}")))?;
            let key = a.value("--key").ok_or_else(|| usage("catalog certify needs --key <offline.key>"))?;
            let out = a.value("--out").ok_or_else(|| usage("catalog certify needs --out <epoch.minisig>"))?;
            let (withdraw, online) = (a.texts("--withdraw-key")?, a.texts("--online-key")?);
            let previous = a.value("--previous").map(Path::new);
            certify(epoch, Path::new(key), &withdraw, &online, previous, Path::new(out), a.has("--force"))
        }
        "epoch" => {
            let a = Args::read(rest, 2, &[], &[])?;
            catalog_epoch(&a.paths[0], &a.paths[1])
        }
        "bump" => {
            let a = Args::read(rest, 1, &[], &["--days"])?;
            let most = MAX_LIFETIME / DAY;
            let days = match a.text("--days")? {
                Some(days) => days.parse().ok().filter(|d| (1..=most).contains(d)),
                None => Some(30),
            };
            bump(&a.paths[0], days.ok_or_else(|| usage(format!("--days takes 1 to {most}")))?)
        }
        "sign" => {
            let a = Args::read(rest, 1, &[], &["--key-env", "--password-env"])?;
            sign_index(&a.paths[0], a.needed("--key-env", "catalog sign")?, a.text("--password-env")?)
        }
        "verify" => {
            let a = Args::read(rest, 1, &[], &["--previous", "--online-key", "--offline-key"])?;
            verify_index(&a.paths[0], a.value("--previous").map(Path::new), &a.catalog_keys())
        }
        "diff" => {
            let a = Args::read(rest, 2, &[], &[])?;
            catalog_diff(&a.paths[0], &a.paths[1])
        }
        other => Err(usage(format!("unknown catalog command \"{other}\""))),
    }
}

/// Writes an empty index to `path`, which mustn't be there: epoch 1,
/// number 0, published and running out at 0, so `bump` dates the first.
fn catalog_new(path: &Path) -> Result<(), Failure> {
    if fs::symlink_metadata(path).is_ok() {
        return Err(failed(format!("{} is there already.", path.display())));
    }
    let index = Index {
        catalog_version: catalog::CATALOG_VERSION,
        epoch: FIRST_EPOCH,
        seq: 0,
        published: 0,
        expires: 0,
        epoch_certificate: None,
        plugins: Vec::new(),
        withdrawn: Vec::new(),
    };
    let file = key_file(folder_of(path), &index_bytes(&index), false)?;
    file.persist_noclobber(path).map_err(|e| at(path)(e.error))?;
    println!("Made an empty catalog in {}.", path.display());
    Ok(())
}

/// Lists the package `pkg` in the index `path`, with what the package says
/// of itself, where it is (`urls`), the source it was built from, and its
/// author (and their key) as the catalog checked them; approved now. A
/// version the index lists already is refused.
fn catalog_add(
    path: &Path,
    pkg: &Path,
    urls: &[&str],
    source: SourceRef,
    author: &str,
    author_key: Option<&str>,
) -> Result<(), Failure> {
    let mut index = read_index(path)?;
    let listing = Listing::of(pkg)?;
    let m = &listing.manifest;
    if let Some(e) = index.plugins.iter().find(|e| e.id == m.id && same_version(&e.version, &m.version)) {
        return Err(failed(format!("The catalog lists {} {} already.", e.id, e.version)));
    }
    let now = catalog::now();
    let entry = Entry {
        id: m.id.clone(),
        version: m.version.clone(),
        lock: listing.lock.clone(),
        package: PackageRef {
            urls: urls.iter().map(|&url| url.to_owned()).collect(),
            size: listing.size,
            sha256: listing.sha256.clone(),
        },
        author_key: author_key.map(str::to_owned),
        name: m.name.clone(),
        // The manifest cuts its description to 300 characters already.
        summary: m.description.clone(),
        author: author.to_owned(),
        license: m.license.clone(),
        platforms: listing.platforms.clone(),
        runtime: listing.runtime.to_owned(),
        gpu: m.runtime.gpu,
        uv_script: listing.uv_script,
        permissions: listing.permissions(),
        hosts: m.network_hosts.clone(),
        engines: m.engines.autoscene.clone(),
        source,
        icon: listing.icon.clone(),
        approved: now,
        // What its first run downloads, and from where.
        resource_bytes: m.resource_bytes(),
        resource_hosts: m.resource_hosts(),
    };
    // Checked as AutoScene checks an entry of an index published now.
    let alone = Index { plugins: vec![entry], ..probe(index.epoch, now) };
    let Some(entry) = as_autoscene_reads(&alone, now)?.plugins.pop() else {
        unreachable!("an index AutoScene took keeps its entries")
    };
    let named = format!("{} {}", entry.id, entry.version);
    index.plugins.push(entry);
    write_file(path, &index_bytes(&index))?;
    println!("Listed {named} in {}.", path.display());
    Ok(())
}

/// Sets the withdrawals of the index `path` to those of the JSON file `list`
/// (`[{id, versions, reason, since}]`), each checked as AutoScene checks it.
fn catalog_withdrawn(path: &Path, list: &Path) -> Result<(), Failure> {
    let mut index = read_index(path)?;
    let bytes = read_at_most(list, catalog::MAX_INDEX_BYTES)?;
    let withdrawn = serde_json::from_slice(&bytes).map_err(|e| failed(format!("{}: {e}", list.display())))?;
    let now = catalog::now();
    let alone = Index { withdrawn, ..probe(index.epoch, now) };
    index.withdrawn = as_autoscene_reads(&alone, now)?.withdrawn;
    write_file(path, &index_bytes(&index))?;
    println!("{} has {}.", path.display(), count(index.withdrawn.len(), "withdrawal", "withdrawals"));
    Ok(())
}

/// Signs the certificate of epoch `epoch` with the offline key `key` (asking
/// for its password), into `out`, which `force` lets it replace: it names the
/// online keys of the epoch, `online`, and withdraws the keys whose ids are
/// `withdraw` for good, and those the certificate `previous` withdrew.
fn certify(
    epoch: u64,
    key: &Path,
    withdraw: &[&str],
    online: &[&str],
    previous: Option<&Path>,
    out: &Path,
    force: bool,
) -> Result<(), Failure> {
    let mut withdraw = withdraw
        .iter()
        .map(|id| {
            let id = id.trim().to_ascii_uppercase();
            if id.len() == 16 && id.bytes().all(|b| b.is_ascii_hexdigit()) {
                Ok(id)
            } else {
                Err(usage(format!("--withdraw-key takes a key id, 16 hex digits as minisign shows it, not \"{id}\"")))
            }
        })
        .collect::<Result<Vec<String>, Failure>>()?;
    let online: Vec<String> = online.iter().map(|k| k.trim().to_owned()).collect();
    for key in &online {
        // As AutoScene reads the keys a certificate names.
        CatalogKeys::new(&[(KeyRole::Online, key.as_str())])?;
    }
    match previous.map(read_certificate).transpose()? {
        Some((_, previous)) if previous.epoch >= epoch => {
            return Err(failed(format!(
                "--previous is the certificate of epoch {}, not of an earlier one.",
                previous.epoch
            )));
        }
        Some((_, previous)) => {
            Certificate { epoch, withdraw: Vec::new(), online: online.clone() }.names_none_of(&previous)?;
            withdraw.extend(previous.withdraw);
            withdraw.sort_unstable();
            withdraw.dedup();
        }
        None => {}
    }
    for key in &online {
        if withdraw.contains(&trust::key_id(key)?) {
            return Err(failed(format!("The certificate both names and withdraws key {}.", trust::key_id(key)?)));
        }
    }
    if !force && fs::symlink_metadata(out).is_ok() {
        return Err(failed(format!("{} is there already; --force replaces it.", out.display())));
    }
    let secret = secret_key(key)?;
    let public = PublicKey::from_secret_key(&secret)?;
    let id = trust::key_id(&public.to_base64())?;
    if withdraw.contains(&id) {
        return Err(failed("A certificate can't withdraw the key that signs it."));
    }
    if previous.is_none() && epoch > FIRST_EPOCH + 1 {
        warn(
            "Without --previous, it doesn't withdraw again what earlier certificates withdrew, and asp catalog \
             epoch refuses it after one that withdrew a key.",
        );
    }
    if online.is_empty() {
        warn("It names no online key: only the offline key signs the indexes of this epoch.");
    }
    let line = catalog::certificate_line(epoch, &withdraw, &online);
    let untrusted = Some("AutoScene plugin catalog epoch certificate");
    let signature = minisign::sign(Some(&public), &secret, line.as_bytes(), Some(&line), untrusted)?;
    write_file(out, signature.into_string().as_bytes())?;
    println!("Certified epoch {epoch} with key {id} into {}.", out.display());
    Ok(())
}

/// Attaches the epoch certificate `cert` to the index `path`. Its epoch
/// starts at number 0, so the next `bump` gives its first index. A lower
/// epoch is refused, and so is another certificate of the same one; the
/// same certificate again changes nothing.
fn catalog_epoch(path: &Path, cert: &Path) -> Result<(), Failure> {
    let mut index = read_index(path)?;
    let (text, certificate) = read_certificate(cert)?;
    let epoch = certificate.epoch;
    let held = index.epoch_certificate.as_deref();
    // The same certificate, also with other line ends.
    if epoch == index.epoch && held.is_some_and(|held| signed_lines(held) == signed_lines(&text)) {
        println!("{} is in epoch {epoch} already.", path.display());
        return Ok(());
    }
    if epoch <= index.epoch {
        return Err(failed(format!(
            "The certificate is for epoch {epoch}, and the catalog is in epoch {} already.",
            index.epoch
        )));
    }
    if let Some(held) = held {
        certificate.keeps_withdrawals_of(&Certificate::read(held).map_err(|_| unreadable_certificate(path))?)?;
    }
    index.epoch = epoch;
    index.seq = 0;
    index.epoch_certificate = Some(text);
    write_file(path, &index_bytes(&index))?;
    println!("{} starts epoch {epoch}.", path.display());
    Ok(())
}

/// Numbers the index `path` one higher and dates it: published now, and
/// running out in `days`.
fn bump(path: &Path, days: u64) -> Result<(), Failure> {
    let mut index = read_index(path)?;
    let now = catalog::now();
    index.seq =
        index.seq.checked_add(1).ok_or_else(|| failed("The catalog's number can't go higher; start a new epoch."))?;
    index.published = now;
    index.expires = now + days * DAY;
    write_file(path, &index_bytes(&index))?;
    let (seq, epoch, expires) = (index.seq, index.epoch, date(index.expires));
    println!("{} is number {seq} of epoch {epoch}, running out on {expires}.", path.display());
    Ok(())
}

/// Signs the index `path`, as its bytes are, into `<path>.minisig` with the
/// secret key the environment variable `key_env` holds, opened with the
/// password `password_env` holds (none: the key has none). The key comes
/// from the environment alone, and is never written. An index AutoScene
/// would refuse for its form, its dates included, isn't signed, nor one asp
/// didn't write as it writes one.
fn sign_index(path: &Path, key_env: &str, password_env: Option<&str>) -> Result<(), Failure> {
    let bytes = read_at_most(path, catalog::MAX_INDEX_BYTES)?;
    let index = parse_index(path, &bytes)?;
    match &index.epoch_certificate {
        Some(text) if Certificate::read(text).map_err(|_| unreadable_certificate(path))?.epoch != index.epoch => {
            return Err(failed("The catalog's epoch certificate is for another epoch."));
        }
        None if index.epoch > FIRST_EPOCH => {
            return Err(failed(format!(
                "An index of epoch {} needs its epoch certificate, which asp catalog epoch attaches.",
                index.epoch
            )));
        }
        _ => {}
    }
    as_autoscene_reads(&index, catalog::now())?;
    // Other bytes (edited by hand, a key named twice, other line ends) might
    // read otherwise in AutoScene than they did here.
    if bytes != index_bytes(&index) {
        return Err(failed(format!(
            "{} isn't written as asp writes an index, so AutoScene might read it otherwise. Any asp catalog \
             command that changes it writes it again.",
            path.display()
        )));
    }
    let secret = secret_key_from_env(key_env, password_env)?;
    let public = PublicKey::from_secret_key(&secret)?;
    let id = trust::key_id(&public.to_base64())?;
    let comment = catalog::index_comment(index.epoch, index.seq, index.expires);
    let untrusted = Some("AutoScene plugin catalog");
    let signature = minisign::sign(Some(&public), &secret, bytes.as_slice(), Some(&comment), untrusted)?;
    let sig_path = sig_beside(path);
    write_file(&sig_path, signature.into_string().as_bytes())?;
    let (epoch, seq, expires) = (index.epoch, index.seq, date(index.expires));
    println!(
        "Signed {} (epoch {epoch}, number {seq}, running out on {expires}) with key {id} into {}.",
        path.display(),
        sig_path.display()
    );
    Ok(())
}

/// The secret key the environment variable `key_env` holds, as minisign
/// writes one, opened with the password the variable `password_env` holds
/// (none: the key has no password).
fn secret_key_from_env(key_env: &str, password_env: Option<&str>) -> Result<SecretKey, Failure> {
    let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    let text = var(key_env).ok_or_else(|| failed(format!("${key_env} holds no key.")))?;
    let boxed = SecretKeyBox::from_string(&text)?;
    let key = match password_env {
        Some(name) => {
            let password = var(name).ok_or_else(|| failed(format!("${name} holds no password.")))?;
            boxed.into_secret_key(Some(password))
        }
        None if secret_key_lock(text.as_bytes()) == Some(*b"Sc") => {
            return Err(failed(format!(
                "The key in ${key_env} is locked with a password; --password-env names the variable that holds it."
            )));
        }
        None => boxed.into_unencrypted_secret_key(),
    };
    key.map_err(|e| failed(format!("The key in ${key_env}: {e}")))
}

/// Checks the index `path`, with its `<path>.minisig`, as AutoScene takes
/// it now, against AutoScene's catalog keys and `keys`: as an AutoScene
/// that holds no catalog, or the `previous` index, of which it must keep
/// every version. Says what it is and who signed it.
fn verify_index(path: &Path, previous: Option<&Path>, keys: &[(KeyRole, &OsString)]) -> Result<(), Failure> {
    let keys = catalog_keys(keys)?;
    let (bytes, sig) = read_signed(path)?;
    let old = match previous {
        Some(previous) => {
            let old_bytes = read_at_most(previous, catalog::MAX_INDEX_BYTES)?;
            let old = parse_index(previous, &old_bytes)?;
            let certificate = old.epoch_certificate.as_deref().map(Certificate::read).transpose();
            let certificate = certificate.map_err(|_| unreadable_certificate(previous))?;
            Some((previous, old, package::sha256_hex(&old_bytes), certificate))
        }
        None => None,
    };
    // What an AutoScene holds once it took the previous index, the keys its
    // certificate withdrew among them. Those earlier ones withdrew, it holds
    // too: each certificate withdraws them again.
    let held = old.as_ref().map_or_else(CatalogState::default, |(_, old, sha256, certificate)| CatalogState {
        v: catalog::STATE_VERSION,
        epoch: old.epoch,
        seq: old.seq,
        sha256: Some(sha256.clone()),
        published: old.published,
        expires: old.expires,
        withdrawn_keys: certificate.as_ref().map(|c| c.withdraw.clone()).unwrap_or_default(),
        withdrawn: old.withdrawn.clone(),
        ..CatalogState::default()
    });
    let now = catalog::now();
    let accepted = match catalog::check(&bytes, &sig, &keys, &held, now)? {
        Checked::Newer(accepted) => *accepted,
        // The previous index again, which AutoScene takes as no news.
        Checked::Same => take_fresh(&bytes, &sig, &keys, now)?,
    };
    if let Some((previous, old, _, held)) = &old {
        keeps_every_version(&accepted.index, old, previous)?;
        if let (Some(held), Some(text)) = (held, &accepted.index.epoch_certificate)
            && accepted.index.epoch > old.epoch
        {
            Certificate::read(text).map_err(|_| unreadable_certificate(path))?.keeps_withdrawals_of(held)?;
        }
    }
    let i = &accepted.index;
    let role = match accepted.role {
        KeyRole::Online => "online",
        KeyRole::Offline => "offline",
    };
    println!(
        "{}: epoch {}, number {}, published {} ({}), running out on {} ({}).",
        path.display(),
        i.epoch,
        i.seq,
        date(i.published),
        i.published,
        date(i.expires),
        i.expires
    );
    println!("Signed with the {role} key {}.", accepted.signed_by);
    let (plugins, withdrawn) = (i.plugins.len(), i.withdrawn.len());
    println!(
        "{} and {}.",
        count(plugins, "plugin version", "plugin versions"),
        count(withdrawn, "withdrawal", "withdrawals")
    );
    Ok(())
}

/// Refuses the index `new` when it drops a version the index `old` (read
/// from `path`) lists, or lists it with another lock: the catalog only adds.
fn keeps_every_version(new: &Index, old: &Index, path: &Path) -> Result<(), Failure> {
    for e in &old.plugins {
        match new.plugins.iter().find(|n| n.id == e.id && same_version(&n.version, &e.version)) {
            None => {
                return Err(failed(format!(
                    "The catalog drops {} {}, which {} lists.",
                    e.id,
                    e.version,
                    path.display()
                )));
            }
            Some(n) if n.lock != e.lock => {
                return Err(failed(format!(
                    "The catalog lists {} {} with another files.lock than {} does.",
                    e.id,
                    e.version,
                    path.display()
                )));
            }
            Some(_) => {}
        }
    }
    Ok(())
}

/// Says in Markdown, for the catalog CI's job summary, what the package
/// `pkg` changes against the newest version of its plugin the index `path`
/// lists and hasn't withdrawn, or all it is when that lists none. An index
/// that isn't there yet lists none.
fn catalog_diff(path: &Path, pkg: &Path) -> Result<(), Failure> {
    let index = match fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        _ => Some(read_index(path)?),
    };
    let listing = Listing::of(pkg)?;
    let m = &listing.manifest;
    let theirs: Vec<&Entry> = index.iter().flat_map(|i| &i.plugins).filter(|e| e.id == m.id).collect();
    // Withdrawn as AutoScene withdraws a version.
    let withdrawn = index.as_ref().map(|i| i.withdrawn.clone()).unwrap_or_default();
    let withdrawn = Catalog::new(None, CatalogState { withdrawn, ..CatalogState::default() });
    let newest = theirs
        .iter()
        .filter(|e| withdrawn.withdrawal(&e.id, &e.version).is_none())
        .filter_map(|&e| Some((e, Version::parse(&e.version).ok()?)))
        .max_by(|(_, a), (_, b)| a.cmp_precedence(b))
        .map(|(e, _)| e);
    let listed = theirs.iter().any(|e| same_version(&e.version, &m.version));
    print!("{}", diff_text(&listing, newest, listed, !theirs.is_empty()));
    Ok(())
}

/// [`catalog_diff`]'s Markdown for `listing`, against the entry `newest`;
/// `listed` when the catalog lists its version already, and `known` when it
/// lists any (withdrawn, when `newest` is none).
fn diff_text(listing: &Listing, newest: Option<&Entry>, listed: bool, known: bool) -> String {
    let m = &listing.manifest;
    let permissions = listing.permissions();
    let yes = |b: bool| if b { "yes" } else { "no" };
    let files = format!(
        "{}, {} bytes; the package is {} bytes",
        count(listing.files, "file", "files"),
        listing.unpacked,
        listing.size
    );
    // The author names the plugin: in code, it says nothing in Markdown or
    // HTML, and can't pass for the version or the id.
    let mut out = format!("### `{}` {} (`{}`)\n\n", m.name.replace('`', "'"), m.version, m.id);
    let mut lines = Vec::new();
    match newest {
        None => {
            out.push_str(if known {
                "The catalog withdrew every version of it it lists.\n\n"
            } else {
                "A new plugin.\n\n"
            });
            lines.push(format!("Permissions: {}", names(&permissions)));
            lines.push(format!("Hosts: {}", names(&m.network_hosts)));
            lines.push(format!("Platforms: {}", platforms(&listing.platforms)));
            lines.push(format!("Runtime: {}", listing.runtime));
            lines.push(format!("Python script through uv: {}", yes(listing.uv_script)));
            lines.push(format!("GPU: {}", yes(m.runtime.gpu)));
            lines.push(format!("AutoScene: `{}`", m.engines.autoscene));
            lines.push(format!("Files: {files}"));
        }
        Some(e) => {
            if listed {
                out.push_str(&format!("The catalog lists {} already, so `asp catalog add` refuses it.\n\n", m.version));
            }
            out.push_str(&format!(
                "Against {}, the newest version the catalog lists and hasn't withdrawn:\n\n",
                e.version
            ));
            for (what, now, was) in
                [("Permissions", &permissions, &e.permissions), ("Hosts", &m.network_hosts, &e.hosts)]
            {
                let added: Vec<String> = now.iter().filter(|p| !was.contains(p)).cloned().collect();
                let removed: Vec<String> = was.iter().filter(|p| !now.contains(p)).cloned().collect();
                if !added.is_empty() {
                    lines.push(format!("{what} added: {}", names(&added)));
                }
                if !removed.is_empty() {
                    lines.push(format!("{what} removed: {}", names(&removed)));
                }
            }
            if listing.platforms.iter().collect::<BTreeSet<_>>() != e.platforms.iter().collect::<BTreeSet<_>>() {
                lines.push(format!("Platforms: {} (was {})", platforms(&listing.platforms), platforms(&e.platforms)));
            }
            if listing.runtime != e.runtime {
                lines.push(format!("Runtime: {} (was {})", listing.runtime, e.runtime));
            }
            if listing.uv_script != e.uv_script {
                lines.push(format!("Python script through uv: {} (was {})", yes(listing.uv_script), yes(e.uv_script)));
            }
            if m.runtime.gpu != e.gpu {
                lines.push(format!("GPU: {} (was {})", yes(m.runtime.gpu), yes(e.gpu)));
            }
            if m.engines.autoscene != e.engines {
                lines.push(format!("AutoScene: `{}` (was `{}`)", m.engines.autoscene, e.engines));
            }
            if lines.is_empty() {
                lines.push("Permissions, hosts, platforms, runtime, uv, GPU and AutoScene: as before".into());
            }
            lines.push(format!("Files: {files} (was {} bytes)", e.package.size));
        }
    }
    for line in lines {
        out.push_str(&format!("- {line}\n"));
    }
    out
}

/// `items` in Markdown: `a`, `b`; "none" for none.
fn names(items: &[String]) -> String {
    if items.is_empty() { "none".into() } else { items.iter().map(|i| format!("`{i}`")).collect::<Vec<_>>().join(", ") }
}

/// An entry's platforms: "all" for none.
fn platforms(platforms: &[String]) -> String {
    if platforms.is_empty() { "all".into() } else { platforms.join(", ") }
}

/// `n` things: "no files", "1 file", "2 files".
fn count(n: usize, one: &str, many: &str) -> String {
    match n {
        0 => format!("no {many}"),
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    }
}

/// What a package says of itself, as the catalog lists it.
struct Listing {
    manifest: Manifest,
    /// The SHA-256 of its `files.lock`.
    lock: String,
    /// How many files it has, and their bytes.
    files: usize,
    unpacked: u64,
    /// The package's size and SHA-256.
    size: u64,
    sha256: String,
    /// The platforms where its command starts; none for one that runs on
    /// all: one that runs no program, one uv script everywhere, or a
    /// WebAssembly component that names no platform.
    platforms: Vec<String>,
    /// Its runtime type, as its manifest names it.
    runtime: &'static str,
    /// Whether it runs a Python script through uv.
    uv_script: bool,
    /// Its icon as a `data:` URL, when that is small enough.
    icon: Option<String>,
}

impl Listing {
    /// The package `path`, unpacked and checked as `verify` does.
    fn of(path: &Path) -> Result<Self, Failure> {
        let scratch = tempfile::tempdir()?;
        let dir = scratch.path().join("plugin");
        let (unpacked, parsed) = unpack_checked(path, &dir)?;
        let m = parsed.manifest;
        let (mut platforms, mut uv_script) = (Vec::new(), false);
        for (platform, launch) in m.launches() {
            if let Ok(launch) = launch
                && package::check_launch(&unpacked.lock, &launch, platform).is_ok()
            {
                uv_script |= matches!(launch, Launch::UvScript { .. });
                platforms.push(platform.to_owned());
            }
        }
        // One uv script for every platform runs wherever uv does, and a
        // WebAssembly component wherever AutoScene does, also on a platform
        // a later AutoScene adds: either is listed as running on all.
        let everywhere = m.platforms.is_empty() && m.runtime.command.keys().all(|key| key == "*");
        if (uv_script || m.is_sandboxed()) && everywhere && platforms.len() == manifest::PLATFORMS.len() {
            platforms.clear();
        }
        let runtime = match m.runtime.kind {
            RuntimeKind::Process => "process",
            RuntimeKind::Service => "service",
            RuntimeKind::None => "none",
            RuntimeKind::Wasm => "wasm",
        };
        let icon = m.icon.as_deref().map(|icon| icon_url(&dir, icon)).transpose()?.flatten();
        let (size, sha256) = size_and_sha256(path)?;
        Ok(Self {
            lock: package::sha256_hex(&unpacked.lock_bytes),
            files: unpacked.lock.files.len(),
            unpacked: unpacked.lock.files.iter().map(|f| f.size).sum(),
            size,
            sha256,
            platforms,
            runtime,
            uv_script,
            icon,
            manifest: m,
        })
    }

    /// Its permissions, as its manifest names them.
    fn permissions(&self) -> Vec<String> {
        self.manifest.permissions.iter().map(|p| p.name().to_owned()).collect()
    }
}

/// The icon `path` of the plugin in `dir` as a `data:` URL, when it is at
/// most [`catalog::MAX_ICON_BYTES`]; a larger one is left out, with a
/// warning.
fn icon_url(dir: &Path, path: &str) -> Result<Option<String>, Failure> {
    let kind = match PictureFormat::of(path) {
        Some(PictureFormat::Png) => "png",
        Some(PictureFormat::Svg) => "svg+xml",
        _ => return Ok(None),
    };
    let file = file_in(dir, path);
    let bytes = fs::read(&file).map_err(at(&file))?;
    if bytes.len() > catalog::MAX_ICON_BYTES {
        let most = catalog::MAX_ICON_BYTES >> 10;
        warn(&format!("The icon is larger than {most} KB, so the catalog lists the plugin without it."));
        return Ok(None);
    }
    Ok(Some(format!("data:image/{kind};base64,{}", STANDARD.encode(bytes))))
}

/// The size and the SHA-256 of the file `path`, read once.
fn size_and_sha256(path: &Path) -> Result<(u64, String), Failure> {
    let (mut file, mut hasher) = (File::open(path).map_err(at(path))?, Sha256::new());
    let (mut buf, mut size) = (vec![0; 1 << 16], 0);
    loop {
        let n = file.read(&mut buf).map_err(at(path))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        size += n as u64;
    }
    Ok((size, hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()))
}

/// An index of `epoch` with nothing in it, published `now` and running out a
/// day later: what an entry or a withdrawal is checked in.
fn probe(epoch: u64, now: u64) -> Index {
    Index {
        catalog_version: catalog::CATALOG_VERSION,
        epoch,
        seq: 1,
        published: now,
        expires: now + DAY,
        epoch_certificate: None,
        plugins: Vec::new(),
        withdrawn: Vec::new(),
    }
}

/// `index` as AutoScene reads it at `now` ([`catalog::check`]): its dates,
/// entries and withdrawals checked, and their texts cleaned and cut. The
/// check runs on a copy signed with keys made for it, which carries their
/// epoch certificate: whether the index's own certificate and signature
/// hold, only AutoScene's keys tell, and `catalog verify` asks them.
fn as_autoscene_reads(index: &Index, now: u64) -> Result<Index, Failure> {
    let (online, offline) = (KeyPair::generate_unencrypted_keypair()?, KeyPair::generate_unencrypted_keypair()?);
    let (online_key, offline_key) = (online.pk.to_base64(), offline.pk.to_base64());
    let line = catalog::certificate_line(index.epoch, &[], std::slice::from_ref(&online_key));
    let certificate = minisign::sign(None, &offline.sk, line.as_bytes(), Some(&line), None)?.into_string();
    let copy = Index { epoch_certificate: Some(certificate), ..index.clone() };
    let bytes = index_bytes(&copy);
    let comment = catalog::index_comment(copy.epoch, copy.seq, copy.expires);
    let sig = minisign::sign(None, &online.sk, bytes.as_slice(), Some(&comment), None)?.into_string();
    let keys = CatalogKeys::new(&[(KeyRole::Online, &online_key), (KeyRole::Offline, &offline_key)])?;
    Ok(take_fresh(&bytes, &sig, &keys, now)?.index)
}

/// The index `path`, to change it or to compare with it. Its signature isn't
/// checked: it is the catalog CI's own file.
fn read_index(path: &Path) -> Result<Index, Failure> {
    parse_index(path, &read_at_most(path, catalog::MAX_INDEX_BYTES)?)
}

/// The index `bytes`, read from `path`. One of a later format is refused:
/// this asp would drop or change what it doesn't know.
fn parse_index(path: &Path, bytes: &[u8]) -> Result<Index, Failure> {
    if bytes.len() as u64 > catalog::MAX_INDEX_BYTES {
        return Err(failed("The catalog is too large."));
    }
    let unreadable = |e: serde_json::Error| failed(format!("{}: {e}", path.display()));
    let v: Value = serde_json::from_slice(bytes).map_err(unreadable)?;
    match v.get("catalogVersion").and_then(Value::as_u64) {
        Some(n) if n == u64::from(catalog::CATALOG_VERSION) => serde_json::from_value(v).map_err(unreadable),
        Some(n) if n > u64::from(catalog::CATALOG_VERSION) => {
            Err(failed("The catalog is of a later format than this asp writes. Update asp."))
        }
        _ => Err(failed(format!("{} isn't a catalog index.", path.display()))),
    }
}

/// An index as the catalog keeps it: pretty-printed, with a trailing
/// newline. These bytes are what `sign` signs.
fn index_bytes(index: &Index) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(index).expect("an index is JSON");
    bytes.push(b'\n');
    bytes
}

/// Writes `bytes` to `path` beside it first, then in its place.
fn write_file(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
    key_file(folder_of(path), bytes, false)?.persist(path).map_err(|e| at(path)(e.error))?;
    Ok(())
}

/// An epoch certificate, as the catalog keeps it: the epoch it starts, the
/// ids of the keys it withdraws, and the online keys it names. Who signed it
/// isn't checked here: only AutoScene's keys tell, and `catalog verify` asks
/// them.
struct Certificate {
    epoch: u64,
    withdraw: Vec<String>,
    online: Vec<String>,
}

impl Certificate {
    /// The certificate `text`, when it is one, spelled as
    /// [`catalog::certificate_line`] spells it.
    fn read(text: &str) -> Result<Self, Failure> {
        let signature = minisign_verify::Signature::decode(text).map_err(|_| not_a_certificate())?;
        let line = catalog::read_certificate_line(signature.trusted_comment());
        let (epoch, withdraw, online) = line.ok_or_else(not_a_certificate)?;
        Ok(Self { epoch, withdraw, online })
    }

    /// Refuses this certificate, of a later epoch than `earlier`, when it
    /// names a key `earlier` withdrew or leaves one out of its withdrawals:
    /// a key is withdrawn for good, also for an AutoScene that never took an
    /// index of `earlier`'s epoch.
    fn keeps_withdrawals_of(&self, earlier: &Self) -> Result<(), Failure> {
        self.names_none_of(earlier)?;
        match earlier.withdraw.iter().find(|&id| !self.withdraw.contains(id)) {
            Some(id) => Err(failed(format!(
                "The certificate of epoch {} doesn't withdraw key {id}, which that of epoch {} withdrew. \
                 asp catalog certify --previous withdraws it again.",
                self.epoch, earlier.epoch
            ))),
            None => Ok(()),
        }
    }

    /// Refuses this certificate when it names a key `earlier` withdrew.
    fn names_none_of(&self, earlier: &Self) -> Result<(), Failure> {
        for key in &self.online {
            let id = trust::key_id(key)?;
            if earlier.withdraw.contains(&id) {
                return Err(failed(format!(
                    "The certificate of epoch {} names key {id}, which that of epoch {} withdrew for good.",
                    self.epoch, earlier.epoch
                )));
            }
        }
        Ok(())
    }
}

/// The epoch certificate in the file `path`, as its text and read.
fn read_certificate(path: &Path) -> Result<(String, Certificate), Failure> {
    let bytes = read_at_most(path, catalog::MAX_SIG_BYTES)?;
    if bytes.len() as u64 > catalog::MAX_SIG_BYTES {
        return Err(failed("The epoch certificate is too large."));
    }
    let text = String::from_utf8(bytes).map_err(|_| not_a_certificate())?;
    let certificate = Certificate::read(&text)?;
    Ok((text, certificate))
}

/// What a minisign signature says, without its untrusted comment and line
/// ends: the lines its signer wrote, as minisign reads them.
fn signed_lines(text: &str) -> Vec<&str> {
    text.lines().skip(1).take(3).collect()
}

fn not_a_certificate() -> Failure {
    failed("That isn't an epoch certificate, as asp catalog certify makes one.")
}

/// The epoch certificate of the index `path` doesn't read.
fn unreadable_certificate(path: &Path) -> Failure {
    failed(format!("{}: its epoch certificate can't be read.", path.display()))
}

/// Whether `a` and `b` are one version as the catalog counts them: of one
/// precedence (build metadata doesn't count).
fn same_version(a: &str, b: &str) -> bool {
    match (Version::parse(a), Version::parse(b)) {
        (Ok(a), Ok(b)) => a.cmp_precedence(&b).is_eq(),
        _ => a == b,
    }
}

/// `secs` (Unix time) as a UTC date: `2026-11-04`.
fn date(secs: u64) -> String {
    // Howard Hinnant's days_from_civil, backwards.
    let days = i64::try_from(secs / DAY).unwrap_or(i64::MAX / 2);
    let z = days + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Checks the WebAssembly component at `path` (a plugin folder, a package
/// or a `.wasm` file) as AutoScene does before it runs it, and says what it
/// imports. A folder's or a package's is checked against its `plugin.json`.
fn check(path: &Path) -> Result<(), Failure> {
    let folder = path.is_dir();
    let package = !folder && path.extension().is_some_and(|e| e.eq_ignore_ascii_case("asp"));
    if !folder && !package {
        let shown = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        show(&component_at(path, shown, None)?);
        return Ok(());
    }
    let scratch = tempfile::tempdir()?;
    let (dir, parsed, lock) = if package {
        let dir = scratch.path().join("plugin");
        let (unpacked, parsed) = open(path, &dir)?;
        (dir, parsed, Some(unpacked.lock))
    } else {
        if let Some(plugin) = made_beside(path) {
            return Err(failed(format!(
                "{} has no plugin.json. The plugin asp init made is in {}: asp check {}",
                path.display(),
                plugin.display(),
                path_word(&plugin)
            )));
        }
        let file = path.join(MANIFEST);
        (path.to_path_buf(), read_manifest(&manifest::read_file(&file).map_err(at(&file))?)?, None)
    };
    warn_all(&parsed.warnings);
    let m = &parsed.manifest;
    if !m.is_sandboxed() {
        return Err(failed(format!(
            "{} isn't a WebAssembly plugin: asp check checks a plugin whose runtime.type is wasm.",
            m.name
        )));
    }
    // A package is checked as AutoScene installs it: its files.lock names
    // the component, which isn't marked to run.
    if let Some(lock) = &lock {
        check_platforms(m, lock)?;
    }
    show(&check_component(&dir, m)?);
    Ok(())
}

/// The plugin folder `plugin/` in `dir`, when `dir` has no `plugin.json`
/// and that folder has one: `asp init --wasm` puts a plugin's source beside
/// it, so the folder asp init made is easy to name in its place.
fn made_beside(dir: &Path) -> Option<PathBuf> {
    let plugin = dir.join("plugin");
    (!dir.join(MANIFEST).exists() && plugin.join(MANIFEST).is_file()).then_some(plugin)
}

/// A component `check` looked at.
struct Component {
    /// Its path in the plugin, or its file name.
    shown: String,
    size: u64,
    checked: wasm::Checked,
}

/// The component of the WebAssembly plugin `m` in its folder `dir`, checked
/// against `m`.
fn check_component(dir: &Path, m: &Manifest) -> Result<Component, Failure> {
    let Some(shown) = m.runtime.component.clone() else {
        return Err(failed("The plugin's runtime names no component."));
    };
    let path = file_in(dir, &shown);
    if !path.is_file() {
        return Err(failed(format!("{} isn't there: build the plugin's component first.", path.display())));
    }
    component_at(&path, shown, Some(m))
}

/// The component at `path`, checked as AutoScene checks it before a run
/// ([`wasm::check`]): against the manifest `m`, when given. An import that
/// needs a newer AutoScene is named, with what this one offers. Nothing of
/// it runs.
fn component_at(path: &Path, shown: String, m: Option<&Manifest>) -> Result<Component, Failure> {
    let mut bytes = Vec::new();
    File::open(path).map_err(at(path))?.take(MAX_COMPONENT_BYTES + 1).read_to_end(&mut bytes).map_err(at(path))?;
    let checked = match wasm::check_listed(&bytes, m) {
        (_, Ok(checked)) => checked,
        // Which import needs a newer AutoScene, and what this one offers.
        (imports, Err(e @ PluginError::TooNew(_))) => {
            return Err(imports.iter().find_map(|name| wasm::too_new(name)).map_or_else(|| e.into(), failed));
        }
        (_, Err(e)) => return Err(e.into()),
    };
    Ok(Component { shown, size: bytes.len() as u64, checked })
}

/// Says what `check` found: the world and size of the component, what it
/// imports (with the permission an import needs to do anything), and the
/// warnings.
fn show(c: &Component) {
    let mut imports: Vec<&str> = c.checked.imports.iter().map(String::as_str).collect();
    imports.sort_unstable();
    // The newest minor of the world it imports; a component that imports
    // none of it still fits 1.0, whose run it exports.
    let minor = imports
        .iter()
        .filter_map(|name| name.strip_prefix("autoscene:plugin/")?.split_once('@'))
        .filter_map(|(_, version)| Version::parse(version).ok())
        .map(|v| v.minor)
        .max()
        .unwrap_or(0);
    println!("{}: a WebAssembly component of autoscene:plugin@1.{minor} ({})", c.shown, size_text(c.size));
    println!("{}", if imports.is_empty() { "imports: none" } else { "imports:" });
    let needs = |name: &str| NEEDS.iter().find(|(prefix, _)| name.starts_with(prefix)).map(|(_, p)| *p);
    let width = imports.iter().filter(|n| needs(n).is_some()).map(|n| n.chars().count()).max().unwrap_or(0);
    for name in imports {
        match needs(name) {
            Some(permission) => println!("  {name:<width$}   needs \"{permission}\""),
            None => println!("  {name}"),
        }
    }
    warn_all(&c.checked.warnings);
}

/// A file's size as `check` gives it: whole KB (rounded up) below 1 MB,
/// else MB rounded to one decimal (1 KB = 1024 bytes).
fn size_text(bytes: u64) -> String {
    if bytes < 1 << 20 {
        format!("{} KB", bytes.div_ceil(1 << 10))
    } else {
        let tenths = (bytes * 10 + (1 << 19)) >> 20;
        format!("{}.{} MB", tenths / 10, tenths % 10)
    }
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
    /// A WebAssembly component in Rust with one command, its source beside
    /// the plugin's folder `plugin/`.
    WasmRust,
    /// A WebAssembly component in Python (componentize-py) with one
    /// command, its source beside the plugin's folder `plugin/`.
    WasmPython,
}

impl Form {
    fn what(self) -> &'static str {
        match self {
            Self::Page => "a page",
            Self::Service => "a page and its service",
            Self::Action => "an export action",
            Self::WasmRust => "a WebAssembly plugin in Rust",
            Self::WasmPython => "a WebAssembly plugin in Python",
        }
    }

    /// The path of its `plugin.json`: a WebAssembly plugin's source sits
    /// beside its folder, which is what AutoScene loads and asp packs.
    fn manifest(self) -> &'static str {
        match self {
            Self::WasmRust | Self::WasmPython => "plugin/plugin.json",
            _ => MANIFEST,
        }
    }

    /// The template blocks it keeps (see [`fill`]).
    fn blocks(self) -> &'static [&'static str] {
        match self {
            Self::Page => &["page"],
            Self::Service => &["page", "program", "service"],
            Self::Action => &["program", "action"],
            Self::WasmRust | Self::WasmPython => &[],
        }
    }

    /// Its files, by their path in the plugin: the templates [`fill`]
    /// makes them of, and those it gets as they are.
    fn files(self) -> Vec<(&'static str, Template)> {
        let editor = [
            (".autoscene/plugin.schema.json", Template::Copy(SCHEMA)),
            (".vscode/settings.json", Template::Copy(include_str!("../../templates/vscode-settings.json"))),
        ];
        let manifest = match self {
            Self::Page => include_str!("../../templates/plugin.page.json"),
            Self::Service => include_str!("../../templates/plugin.service.json"),
            Self::Action => include_str!("../../templates/plugin.action.json"),
            Self::WasmRust => {
                return [
                    ("plugin/plugin.json", Template::Fill(include_str!("../../templates/wasm-rust/plugin.json"))),
                    ("plugin/.gitignore", Template::Copy(include_str!("../../templates/wasm-rust/plugin-gitignore"))),
                    ("README.md", Template::Fill(include_str!("../../templates/wasm-rust/README.md"))),
                    ("Cargo.toml", Template::Fill(include_str!("../../templates/wasm-rust/Cargo.toml"))),
                    ("Cargo.lock", Template::Fill(RUST_LOCK)),
                    (
                        "rust-toolchain.toml",
                        Template::Copy(include_str!("../../templates/wasm-rust/rust-toolchain.toml")),
                    ),
                    ("src/lib.rs", Template::Copy(include_str!("../../templates/wasm-rust/src/lib.rs"))),
                    (".gitignore", Template::Copy(include_str!("../../templates/wasm-rust/gitignore"))),
                    ("wit/plugin.wit", Template::Copy(WIT)),
                ]
                .into_iter()
                .chain(editor)
                .collect();
            }
            Self::WasmPython => {
                return [
                    ("plugin/plugin.json", Template::Fill(include_str!("../../templates/wasm-python/plugin.json"))),
                    ("plugin/.gitignore", Template::Copy(include_str!("../../templates/wasm-python/plugin-gitignore"))),
                    ("README.md", Template::Fill(include_str!("../../templates/wasm-python/README.md"))),
                    ("app.py", Template::Copy(include_str!("../../templates/wasm-python/app.py"))),
                    (".gitignore", Template::Copy(include_str!("../../templates/wasm-python/gitignore"))),
                    ("wit/plugin.wit", Template::Copy(WIT)),
                ]
                .into_iter()
                .chain(editor)
                .collect();
            }
        };
        let mut files = vec![
            (MANIFEST, Template::Fill(manifest)),
            ("README.md", Template::Fill(include_str!("../../templates/README.md"))),
        ];
        files.extend(editor);
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
    // A WebAssembly plugin's crate, named for the id's last part, and the
    // file cargo builds of it.
    let crate_name = crate_name(id.as_str().rsplit('.').next().unwrap_or_default());
    let wasm_file = format!("{}.wasm", crate_name.replace('-', "_"));

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
                    ("crate", text(&crate_name)),
                    ("wasm", text(&wasm_file)),
                ];
                let filled = fill(template, form.blocks(), &values);
                (path, filled)
            }
        })
        .collect();
    let (_, plugin_json) =
        files.iter().find(|(path, _)| *path == form.manifest()).expect("every form has a plugin.json");
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

/// The name of a Rust WebAssembly plugin's crate for `last`, its id's last
/// part: `last`, or `plugin-` and `last` when a crate name can't start like
/// it or a package in the template's `Cargo.lock` has its name (cargo
/// reads `-` and `_` alike there). Another crate of that name would make
/// the lock wrong, and `cargo build --locked` would refuse it.
fn crate_name(last: &str) -> String {
    let same = |a: &str, b: &str| a.replace('-', "_") == b.replace('-', "_");
    let taken = RUST_LOCK
        .lines()
        .filter_map(|line| line.strip_prefix("name = \"")?.strip_suffix('"'))
        .any(|package| same(package, last));
    if last.starts_with(|c: char| c.is_ascii_lowercase()) && !taken {
        last.to_owned()
    } else {
        format!("plugin-{last}")
    }
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

/// Unpacks the package `path` into the new folder `into`, reads its
/// manifest and checks it as AutoScene would install it, with the warnings
/// `pack` gives.
fn unpack_checked(path: &Path, into: &Path) -> Result<(Unpacked, Parsed), Failure> {
    let (unpacked, parsed) = open(path, into)?;
    warn_all(&parsed.warnings);
    let m = &parsed.manifest;
    check_platforms(m, &unpacked.lock)?;
    if m.is_sandboxed() {
        warn_all(&check_component(into, m)?.checked.warnings);
    }
    package::check_pages(into, Some(&unpacked.lock), &m.pages)?;
    package::check_pictures(into, &unpacked.lock, m)?;
    warn_of_banner(into, m)?;
    Ok((unpacked, parsed))
}

/// `path` as one word of a command line: in double quotes when it holds
/// a space.
fn path_word(path: &Path) -> String {
    let shown = path.display().to_string();
    if shown.contains(char::is_whitespace) { format!("\"{shown}\"") } else { shown }
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

    /// The option's last value, as text.
    fn text(&self, option: &str) -> Result<Option<&'a str>, Failure> {
        self.value(option).map(|v| as_text(option, v)).transpose()
    }

    /// The option's values, as text.
    fn texts(&self, option: &str) -> Result<Vec<&'a str>, Failure> {
        self.values(option).into_iter().map(|v| as_text(option, v)).collect()
    }

    /// The option's last value, which the command needs.
    fn needed(&self, option: &str, command: &str) -> Result<&'a str, Failure> {
        self.text(option)?.ok_or_else(|| usage(format!("{command} needs {option}")))
    }

    /// The catalog keys `--online-key` and `--offline-key` give.
    fn catalog_keys(&self) -> Vec<(KeyRole, &'a OsString)> {
        let given = [(KeyRole::Online, "--online-key"), (KeyRole::Offline, "--offline-key")];
        given.iter().flat_map(|&(role, option)| self.values(option).into_iter().map(move |k| (role, k))).collect()
    }
}

/// The value `v` of `option`, as text.
fn as_text<'a>(option: &str, v: &'a OsString) -> Result<&'a str, Failure> {
    v.to_str().ok_or_else(|| usage(format!("{option} takes text")))
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
    fn a_date_is_the_day_in_utc() {
        let days = [
            (0, "1970-01-01"),
            (86_399, "1970-01-01"),
            (951_782_400, "2000-02-29"),
            (1_800_000_000, "2027-01-15"),
            (4_107_542_399, "2100-02-28"),
            (4_107_542_400, "2100-03-01"),
        ];
        for (secs, day) in days {
            assert_eq!(date(secs), day, "{secs}");
        }
    }

    #[test]
    fn a_component_is_never_marked_to_run() {
        let wasm = br#"{"manifestVersion": 2, "id": "dev.autoscene.hello-wasm", "version": "1.0.0",
            "name": "Hello Wasm", "description": "Says hello.", "author": {"name": "AutoScene"},
            "engines": {"autoscene": ">=1.0.0", "protocol": 2},
            "runtime": {"type": "wasm", "component": "plugin.wasm"}}"#;
        let m = manifest::parse_for_any_app(wasm).unwrap().manifest;
        let file = |path: &str| LockedFile { path: path.into(), size: 8, sha256: "0".repeat(64), exec: true };
        let mut lock = FilesLock { lock_version: 1, files: vec![file("plugin.wasm"), file("tool.sh")] };
        // The folder marks both to run (Linux and macOS): the component is cleared, the other kept.
        assert_eq!(mark_to_run(&m, &mut lock), ["tool.sh"]);
        assert_eq!(lock.files.iter().map(|f| f.exec).collect::<Vec<_>>(), [false, true]);
        assert!(check_platforms(&m, &lock).is_ok());
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

    #[test]
    fn a_crate_is_never_named_like_a_package_in_its_lock() {
        for (last, named) in [
            ("hello-rust", "hello-rust"),
            ("2d-tools", "plugin-2d-tools"),
            // Packages in the template's Cargo.lock, with - and _ alike.
            ("log", "plugin-log"),
            ("serde", "plugin-serde"),
            ("serde-json", "plugin-serde-json"),
            ("wit-bindgen", "plugin-wit-bindgen"),
            ("wit_bindgen", "plugin-wit_bindgen"),
            // Only a whole name counts.
            ("logger", "logger"),
            ("serde-tools", "serde-tools"),
        ] {
            assert_eq!(crate_name(last), named, "{last}");
        }
        // The template names its own crate only as a placeholder.
        assert!(RUST_LOCK.contains("name = \"{{crate}}\"\n"));
        assert!(RUST_LOCK.contains("name = \"wit-bindgen\"\n"));
    }

    #[test]
    fn a_size_is_rounded_to_what_check_shows() {
        for (bytes, shown) in [
            (0, "0 KB"),
            (1, "1 KB"),
            (1024, "1 KB"),
            (1025, "2 KB"),
            ((1 << 20) - 1, "1024 KB"),
            (1 << 20, "1.0 MB"),
            // 1.05 MB is 1,101,004.8 bytes.
            (1_101_004, "1.0 MB"),
            (1_101_005, "1.1 MB"),
            // What the Python template builds: 17.49 MB.
            (18_335_930, "17.5 MB"),
            ((10 << 20) - 1, "10.0 MB"),
            (MAX_COMPONENT_BYTES, "64.0 MB"),
        ] {
            assert_eq!(size_text(bytes), shown, "{bytes}");
        }
    }
}

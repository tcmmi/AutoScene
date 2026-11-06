//! `cargo xtask pin-accel <version> [--work DIR]`: pins the NVIDIA files
//! AutoScene downloads for "NVIDIA acceleration" (TensorRT for RTX, as ONNX
//! Runtime's plugin). It
//!
//! 1. reads the wheel's address, size and digest from the Python Package Index,
//! 2. downloads the wheel and compares its SHA-256 with the Index's digest,
//! 3. unpacks the plugin's libraries and NVIDIA's acknowledgements, refusing
//!    a wheel that gives one of them anything but a plain name: the digest
//!    only says the Index holds this wheel, and until step 4 nothing says
//!    who made it,
//! 4. refuses unless every library carries a valid Authenticode signature by
//!    "NVIDIA Corporation", the only proof of origin there is: the Index
//!    shows no attestation for this project,
//!
//! and writes `crates/as-infer/accel.toml`. A new version also has to be
//! written into `as_infer::accel::VERSION`, and a test says so.
//!
//! It needs `curl` and Windows' PowerShell, so only a developer on Windows
//! runs it, never CI. `--work` keeps the wheel and its files in DIR, a new or
//! empty folder.

use std::fs;
use std::io::{Cursor, Read};
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

const PROJECT: &str = "onnxruntime-ep-nv-tensorrt-rtx-cu13";
/// The folder of the wheel the plugin's files are in.
const PACKAGE: &str = "onnxruntime_ep_nv_tensorrt_rtx/";
/// The library whose version is TensorRT for RTX's.
const ENGINE_LIBRARY: &str = "tensorrt_rtx_";
/// NVIDIA's third-party notices, which About links to.
const NOTICES: &str = "TRTRTX_Acknowledgements.txt";
/// The one signer the libraries may carry, as their certificate names it.
const SIGNER: &str = "NVIDIA Corporation";
/// How PowerShell is told which file to look at: in its environment, so no
/// name is ever read as part of a script.
const FILE_VARIABLE: &str = "PIN_ACCEL_FILE";
/// The terms the person agrees to in Settings.
const LICENCE: &str = "https://docs.nvidia.com/deeplearning/tensorrt-rtx/latest/reference/sla.html";

/// A file of the pack as the wheel holds it.
struct Pin {
    name: String,
    member: String,
    /// Bytes it takes in the wheel.
    packed: u64,
    size: u64,
    sha256: String,
}

/// The folder to work in, there and empty: `chosen`, whose files aren't this
/// tool's to delete, else `temporary`, whose files are.
fn work_folder<'a>(chosen: Option<&'a Path>, temporary: &'a Path) -> Result<&'a Path> {
    let work = match chosen {
        Some(dir) if fs::read_dir(dir).is_ok_and(|mut files| files.next().is_some()) => {
            bail!("{} isn't empty: --work takes a new or empty folder", dir.display())
        }
        Some(dir) => dir,
        None => {
            let _ = fs::remove_dir_all(temporary);
            temporary
        }
    };
    fs::create_dir_all(work)?;
    Ok(work)
}

/// What Windows says of a file's Authenticode signature.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Signature {
    /// `Valid`, `NotSigned`, `HashMismatch`, …
    status: String,
    /// The signing certificate's subject; none on an unsigned file.
    subject: Option<String>,
}

pub fn pin(version: &str, work: Option<&Path>, root: &Path) -> Result<()> {
    if version.is_empty() || !version.chars().all(|c| c.is_ascii_digit() || c == '.') {
        bail!("'{version}' isn't a release like 0.5.0");
    }
    let temporary = root.join("target/pin-accel");
    let work = work_folder(work, &temporary)?;
    let meta = pypi(version)?;
    let (url, size, digest) = wheel(&meta)?;

    let path = work.join("wheel.whl");
    println!("downloading {url}");
    run(Command::new("curl")
        .args(["--fail", "--location", "--silent", "--show-error", "--output"])
        .arg(&path)
        .arg(&url))?;
    let bytes = fs::read(&path)?;
    if bytes.len() as u64 != size {
        bail!("the wheel is {} bytes, the Index says {size}", bytes.len());
    }
    let got = hex(&Sha256::digest(&bytes));
    if got != digest {
        bail!("the wheel's checksum {got} doesn't match the Index's digest {digest}");
    }

    let pins = unpack(&bytes, work)?;
    let libraries: Vec<&Pin> = pins.iter().filter(|p| is_library(&p.name)).collect();
    if libraries.is_empty() {
        bail!("the wheel holds no library under {PACKAGE}");
    }
    for p in &libraries {
        let signature = signature(&work.join(&p.name))?;
        check_signer(&p.name, &signature)?;
        println!("{}: signed by {SIGNER}", p.name);
    }
    let engine = libraries
        .iter()
        .find(|p| p.name.starts_with(ENGINE_LIBRARY))
        .with_context(|| format!("the wheel has no {ENGINE_LIBRARY}*.dll"))?;
    let engine = file_version(&work.join(&engine.name))?;

    let file = root.join("crates/as-infer/accel.toml");
    fs::write(&file, toml(version, &engine, &url, size, &digest, &pins))?;
    if work == temporary {
        let _ = fs::remove_dir_all(work);
    }
    println!("wrote {}", file.display());
    Ok(())
}

fn is_library(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".dll")
}

/// The Index's record of `version` of the project.
fn pypi(version: &str) -> Result<Value> {
    let url = format!("https://pypi.org/pypi/{PROJECT}/{version}/json");
    let out = Command::new("curl")
        .args(["--fail", "--location", "--silent", "--show-error"])
        .arg(&url)
        .output()
        .context("spawning curl")?;
    if !out.status.success() {
        bail!("{url}: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(serde_json::from_slice(&out.stdout)?)
}

/// The Windows wheel's address, size and SHA-256 in the Index's record.
fn wheel(meta: &Value) -> Result<(String, u64, String)> {
    let urls = meta["urls"].as_array().context("the Index lists no files")?;
    let file = urls
        .iter()
        .find(|u| u["filename"].as_str().is_some_and(|f| f.ends_with("-win_amd64.whl")))
        .context("the release has no Windows wheel")?;
    let url = file["url"].as_str().context("a file without an address")?.to_string();
    let size = file["size"].as_u64().context("a file without a size")?;
    let sha256 = file["digests"]["sha256"].as_str().context("a file without a digest")?.to_string();
    Ok((url, size, sha256))
}

/// Whether `name`, as a wheel has it, is a plain file name: nothing a file
/// system reads as a way out of the folder (`zip` hands the name over raw, a
/// `\` and all), and nothing a shell could read as more than a name.
fn plain_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Put the plugin's libraries and NVIDIA's notices from the wheel `bytes`
/// into `dir`, and say what each is. Nothing in them runs, and none of them
/// lands anywhere else: a wheel that names one otherwise than plainly is
/// refused before it is written.
fn unpack(bytes: &[u8], dir: &Path) -> Result<Vec<Pin>> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))?;
    let mut pins = Vec::new();
    for i in 0..zip.len() {
        let mut file = zip.by_index(i)?;
        let member = file.name()?.to_string();
        let Some(name) = member.strip_prefix(PACKAGE) else { continue };
        if !file.is_file() || name.contains('/') || !(is_library(name) || name == NOTICES) {
            continue;
        }
        if !plain_name(name) {
            bail!("the wheel names a file {name:?}: only letters, digits, '.', '_' and '-' are taken");
        }
        let name = name.to_string();
        let packed = file.compressed_size();
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;
        fs::write(dir.join(&name), &data)?;
        pins.push(Pin { packed, size: data.len() as u64, sha256: hex(&Sha256::digest(&data)), name, member });
    }
    if !pins.iter().any(|p| p.name == NOTICES) {
        bail!("the wheel has no {NOTICES}");
    }
    // The largest first: a drive without room says so before much has travelled.
    pins.sort_by_key(|p| std::cmp::Reverse(p.size));
    Ok(pins)
}

/// What Windows says of the signature of `file`.
fn signature(file: &Path) -> Result<Signature> {
    let script = "$ErrorActionPreference = 'Stop'; \
                  $s = Get-AuthenticodeSignature -LiteralPath $env:PIN_ACCEL_FILE; \
                  [pscustomobject]@{ Status = [string]$s.Status; Subject = $s.SignerCertificate.Subject } \
                  | ConvertTo-Json -Compress";
    let out = powershell(script, file)?;
    serde_json::from_str(&out).with_context(|| format!("{}: PowerShell said {out:?}", file.display()))
}

/// Refuse a library whose signature isn't valid or isn't NVIDIA's.
fn check_signer(name: &str, signature: &Signature) -> Result<()> {
    if signature.status != "Valid" {
        bail!("{name}: its signature is {}, not Valid", signature.status);
    }
    let subject = signature.subject.as_deref().unwrap_or_default();
    match common_name(subject) {
        Some(SIGNER) => Ok(()),
        _ => bail!("{name} is signed by {subject:?}, not {SIGNER:?}"),
    }
}

/// The `CN=` of a certificate subject (`CN=NVIDIA Corporation, OU=…`, or
/// `CN="Some, Name", …` when it holds a comma).
fn common_name(subject: &str) -> Option<&str> {
    let value = subject.strip_prefix("CN=")?;
    if let Some(quoted) = value.strip_prefix('"') {
        return quoted.split('"').next();
    }
    value.split(',').next().map(str::trim)
}

/// The release of the library `file`.
fn file_version(file: &Path) -> Result<String> {
    let out = powershell("(Get-Item -LiteralPath $env:PIN_ACCEL_FILE).VersionInfo.ProductVersion", file)?;
    release(&out).with_context(|| format!("{} names no version, but {out:?}", file.display()))
}

/// The first three parts of a product version (`1.7.1.116` is release 1.7.1).
fn release(version: &str) -> Option<String> {
    let parts: Vec<&str> = version.trim().split('.').take(3).collect();
    let numbers = parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    numbers.then(|| parts.join("."))
}

/// What `script` prints about `file`, which it finds in [`FILE_VARIABLE`].
/// The path stays out of the script's text: PowerShell ends a quoted string
/// at more than the `'` one could escape (U+2019 too). Started from PowerShell
/// 7, this one would inherit its module folders and fail to load its own
/// Get-AuthenticodeSignature, so it finds its modules for itself.
fn powershell(script: &str, file: &Path) -> Result<String> {
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env(FILE_VARIABLE, file)
        .env_remove("PSModulePath")
        .output()
        .context("spawning PowerShell (pin-accel runs on Windows only)")?;
    if !out.status.success() {
        bail!("PowerShell: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn toml(version: &str, engine: &str, url: &str, size: u64, sha256: &str, pins: &[Pin]) -> String {
    let mut toml = format!(
        "# Written by `cargo xtask pin-accel {version}`. Don't edit by hand.\n\
         version = \"{version}\"\n\
         engine = \"{engine}\"\n\
         licence = \"{LICENCE}\"\n\
         url = \"{url}\"\n\
         size = {size}\n\
         sha256 = \"{sha256}\"\n"
    );
    for p in pins {
        toml.push_str(&format!(
            "\n[[file]]\nname = \"{}\"\nmember = \"{}\"\npacked = {}\nsize = {}\nsha256 = \"{}\"\n",
            p.name, p.member, p.packed, p.size, p.sha256
        ));
    }
    toml
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn run(cmd: &mut Command) -> Result<()> {
    let out = cmd.output().with_context(|| format!("spawning {:?}", cmd.get_program()))?;
    if !out.status.success() {
        bail!("{:?} failed: {}", cmd.get_program(), String::from_utf8_lossy(&out.stderr));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    const NVIDIA: &str = "CN=NVIDIA Corporation, OU=2-J, O=NVIDIA Corporation, L=Santa Clara, S=California, C=US";

    #[test]
    fn a_work_folder_of_the_persons_is_never_emptied() {
        let dir = tempfile::tempdir().unwrap();
        let (mine, temporary) = (dir.path().join("mine"), dir.path().join("temporary"));
        // New, then empty: fine.
        assert_eq!(work_folder(Some(&mine), &temporary).unwrap(), mine);
        assert_eq!(work_folder(Some(&mine), &temporary).unwrap(), mine);
        // With a file in it: refused, and the file stays.
        fs::write(mine.join("thesis.docx"), "years of work").unwrap();
        let e = work_folder(Some(&mine), &temporary).unwrap_err();
        assert!(e.to_string().contains("isn't empty"), "{e}");
        assert!(mine.join("thesis.docx").is_file());
        // The tool's own folder starts empty.
        fs::create_dir_all(&temporary).unwrap();
        fs::write(temporary.join("wheel.whl"), "last run").unwrap();
        assert_eq!(work_folder(None, &temporary).unwrap(), temporary);
        assert!(!temporary.join("wheel.whl").exists());
    }

    fn signed(status: &str, subject: Option<&str>) -> Signature {
        Signature { status: status.into(), subject: subject.map(str::to_string) }
    }

    #[test]
    fn only_a_valid_signature_by_nvidia_passes() {
        assert!(check_signer("a.dll", &signed("Valid", Some(NVIDIA))).is_ok());
        let refused = |s: Signature| check_signer("a.dll", &s).unwrap_err().to_string();
        assert!(refused(signed("NotSigned", None)).contains("NotSigned"));
        // A signature that no longer matches the file, by NVIDIA or not.
        assert!(refused(signed("HashMismatch", Some(NVIDIA))).contains("HashMismatch"));
        assert!(refused(signed("Valid", None)).contains("not \"NVIDIA Corporation\""));
        for other in [
            "CN=Microsoft Windows, O=Microsoft Corporation, C=US",
            // The name only in another part of the subject, or as part of a longer one.
            "CN=Someone, O=NVIDIA Corporation, C=US",
            "CN=NVIDIA Corporation Ltd, O=Someone, C=US",
            "O=NVIDIA Corporation, CN=NVIDIA Corporation",
            "CN=\"NVIDIA Corporation, Inc\", O=Someone",
        ] {
            assert!(refused(signed("Valid", Some(other))).contains("is signed by"), "{other}");
        }
    }

    /// Windows' own answer, on files that exist on every Windows.
    #[cfg(windows)]
    #[test]
    fn windows_is_asked_and_a_file_of_another_signer_or_none_is_refused() {
        let system = std::env::var_os("SystemRoot").expect("Windows has a SystemRoot");
        let theirs = Path::new(&system).join("System32/ntdll.dll");
        let s = signature(&theirs).unwrap();
        assert_eq!(s.status, "Valid", "{s:?}");
        assert!(check_signer("ntdll.dll", &s).unwrap_err().to_string().contains("Microsoft"));

        let dir = tempfile::tempdir().unwrap();
        let plain = dir.path().join("it's unsigned.dll");
        fs::write(&plain, b"MZ not a library").unwrap();
        assert!(check_signer("it's unsigned.dll", &signature(&plain).unwrap()).is_err());
    }

    /// A path is a path, whatever it holds: PowerShell ends a string at
    /// U+2019 as it does at `'`, and what follows it here would print.
    #[cfg(windows)]
    #[test]
    fn a_path_is_never_read_as_part_of_a_script() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("x\u{2019}; Write-Output ran; \u{2019}.dll");
        fs::write(&file, b"MZ not a library").unwrap();
        // Windows' answer alone: anything printed beside it isn't one.
        assert_ne!(signature(&file).unwrap().status, "Valid");
    }

    /// A wheel of the notices and of a library named `name`.
    fn wheel_with(name: &str) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (file, data) in [(NOTICES, b"thanks".as_slice()), (name, b"code")] {
            zip.start_file(format!("{PACKAGE}{file}"), stored).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn a_library_with_more_than_a_plain_name_is_refused_before_it_is_written() {
        for name in [
            // Would be read as code when its signature is looked at.
            "x\u{2019}; Start-Process calc; \u{2019}.dll",
            "x'; Start-Process calc; '.dll",
            // Has no '/', and leaves the folder on Windows.
            r"..\..\x.dll",
            r"sub\x.dll",
            // Another stream of a file, a name that isn't ASCII, and a space.
            "x.dll:hidden.dll",
            "caf\u{e9}.dll",
            " x.dll",
        ] {
            let top = tempfile::tempdir().unwrap();
            let dir = top.path().join("a").join("b");
            fs::create_dir_all(&dir).unwrap();
            let e = unpack(&wheel_with(name), &dir).err().unwrap_or_else(|| panic!("{name:?} was taken"));
            assert!(e.to_string().contains("names a file"), "{name:?}: {e}");
            // The notices came first; nothing else is anywhere.
            for (folder, held) in [(&dir, NOTICES), (&top.path().join("a"), "b"), (&top.path().to_owned(), "a")] {
                let names: Vec<_> = fs::read_dir(folder).unwrap().map(|e| e.unwrap().file_name()).collect();
                assert_eq!(names, [held], "{name:?}");
            }
        }
        // A file that isn't one of the pack's is left in the wheel, whatever its name.
        let dir = tempfile::tempdir().unwrap();
        assert!(unpack(&wheel_with(r"..\..\x.py"), dir.path()).is_ok());
        // A library named plainly is taken.
        let pins = unpack(&wheel_with("nvinfer_10-rtx.1.dll"), dir.path()).unwrap();
        assert!(pins.iter().any(|p| p.name == "nvinfer_10-rtx.1.dll"));
    }

    #[test]
    fn the_wheel_gives_up_the_libraries_and_the_notices_and_nothing_else() {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in [
            ("onnxruntime_ep_nv_tensorrt_rtx/__init__.py", b"# no".as_slice()),
            ("onnxruntime_ep_nv_tensorrt_rtx/small.dll", b"small"),
            ("onnxruntime_ep_nv_tensorrt_rtx/big.dll", b"a bigger library"),
            ("onnxruntime_ep_nv_tensorrt_rtx/TRTRTX_Acknowledgements.txt", b"thanks"),
            ("elsewhere/other.dll", b"not ours"),
            ("onnxruntime_ep_nv_tensorrt_rtx/sub/deep.dll", b"not at the top"),
        ] {
            zip.start_file(name, deflated).unwrap();
            zip.write_all(data).unwrap();
        }
        let wheel = zip.finish().unwrap().into_inner();
        let dir = tempfile::tempdir().unwrap();
        let pins = unpack(&wheel, dir.path()).unwrap();
        let names: Vec<&str> = pins.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["big.dll", "TRTRTX_Acknowledgements.txt", "small.dll"], "the largest first");
        assert_eq!(fs::read(dir.path().join("small.dll")).unwrap(), b"small");
        assert_eq!(pins[2].member, "onnxruntime_ep_nv_tensorrt_rtx/small.dll");
        assert_eq!(pins[2].sha256, hex(&Sha256::digest(b"small")));
        assert_eq!(pins[2].size, 5);
        assert!(pins[2].packed > 0);

        // No notices: About couldn't link them.
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("onnxruntime_ep_nv_tensorrt_rtx/a.dll", deflated).unwrap();
        zip.write_all(b"a").unwrap();
        let bare = zip.finish().unwrap().into_inner();
        assert!(unpack(&bare, dir.path()).is_err());
    }

    #[test]
    fn a_library_names_its_release() {
        assert_eq!(release("1.7.1.116\r\n").as_deref(), Some("1.7.1"));
        assert_eq!(release("1.7.1").as_deref(), Some("1.7.1"));
        assert_eq!(release(""), None);
        assert_eq!(release("1.7"), None);
        assert_eq!(release("1.7.x"), None);
    }

    #[test]
    fn the_indexs_record_names_the_windows_wheel() {
        let meta = serde_json::json!({ "urls": [
            { "filename": "x-0.5.0-py3-none-manylinux_2_28_x86_64.whl", "url": "linux", "size": 1, "digests": { "sha256": "l" } },
            { "filename": "x-0.5.0-py3-none-win_amd64.whl", "url": "windows", "size": 2, "digests": { "sha256": "w" } },
        ]});
        assert_eq!(wheel(&meta).unwrap(), ("windows".to_string(), 2, "w".to_string()));
        assert!(wheel(&serde_json::json!({ "urls": [] })).is_err());
    }
}

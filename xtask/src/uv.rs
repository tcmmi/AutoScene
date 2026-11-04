//! `cargo xtask pin-uv <version>`: pins the uv AutoScene downloads for
//! uv-script plugins. For each of the six platforms it
//!
//! 1. reads the release asset's size, digest and URL from the GitHub API,
//! 2. downloads the archive and compares its SHA-256 with GitHub's digest,
//! 3. checks its build attestation with `gh attestation verify`,
//! 4. unpacks only the uv binary and records its size and SHA-256,
//!
//! and writes `crates/as-plugin/uv.toml`. It needs `gh` (logged in) and
//! `curl`, reads from the network only, and is never run by CI. It reads
//! the archives itself, so it runs alike on Windows, macOS and Linux.

use std::fs;
use std::io::{Cursor, Read};
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// The platforms AutoScene ships on: (platform, release asset, binary inside the archive).
const PLATFORMS: &[(&str, &str, &str)] = &[
    ("windows-x86_64", "uv-x86_64-pc-windows-msvc.zip", "uv.exe"),
    ("windows-aarch64", "uv-aarch64-pc-windows-msvc.zip", "uv.exe"),
    ("macos-aarch64", "uv-aarch64-apple-darwin.tar.gz", "uv-aarch64-apple-darwin/uv"),
    ("macos-x86_64", "uv-x86_64-apple-darwin.tar.gz", "uv-x86_64-apple-darwin/uv"),
    ("linux-x86_64", "uv-x86_64-unknown-linux-gnu.tar.gz", "uv-x86_64-unknown-linux-gnu/uv"),
    ("linux-aarch64", "uv-aarch64-unknown-linux-gnu.tar.gz", "uv-aarch64-unknown-linux-gnu/uv"),
];

struct Pin {
    platform: &'static str,
    url: String,
    archive_size: u64,
    archive_sha256: String,
    member: &'static str,
    size: u64,
    sha256: String,
}

pub fn pin(version: &str, root: &Path) -> Result<()> {
    if version.is_empty() || !version.chars().all(|c| c.is_ascii_digit() || c == '.') {
        bail!("'{version}' isn't a uv version like 0.13.7");
    }
    let release = gh_json(&format!("repos/astral-sh/uv/releases/tags/{version}"))?;
    let assets = release["assets"].as_array().context("the release lists no assets")?;
    let work = root.join("target/pin-uv");
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work)?;

    let mut pins = Vec::new();
    for (platform, name, member) in PLATFORMS {
        let asset =
            assets.iter().find(|a| a["name"] == *name).with_context(|| format!("uv {version} has no asset {name}"))?;
        let url = asset["browser_download_url"].as_str().context("an asset without a URL")?.to_string();
        let size = asset["size"].as_u64().context("an asset without a size")?;
        let digest = asset["digest"]
            .as_str()
            .and_then(|d| d.strip_prefix("sha256:"))
            .with_context(|| format!("{name}: GitHub gives no sha256 digest"))?
            .to_string();

        println!("{platform}: downloading {url}");
        let dir = work.join(platform);
        fs::create_dir_all(&dir)?;
        let archive = dir.join(name);
        run(Command::new("curl")
            .args(["--fail", "--location", "--silent", "--show-error", "--output"])
            .arg(&archive)
            .arg(&url))?;
        let bytes = fs::read(&archive)?;
        if bytes.len() as u64 != size {
            bail!("{name}: {} bytes downloaded, GitHub says {size}", bytes.len());
        }
        let got = hex(&Sha256::digest(&bytes));
        if got != digest {
            bail!("{name}: checksum {got} doesn't match GitHub's digest {digest}");
        }

        println!("{platform}: verifying the attestation");
        run(Command::new("gh")
            .args(["attestation", "verify"])
            .arg(&archive)
            .args(["--repo", "astral-sh/uv"])
            .args(["--signer-workflow", "astral-sh/uv/.github/workflows/release.yml"])
            .args(["--source-ref", "refs/heads/main"]))
        .with_context(|| format!("{name}: attestation"))?;

        // Only the one member is read, into memory: nothing comes out onto the disk, and nothing runs.
        let binary = member_of(name, &bytes, member).with_context(|| format!("{name}: no member {member}"))?;
        if binary.is_empty() {
            bail!("{name}: {member} is empty");
        }
        pins.push(Pin {
            platform,
            url,
            archive_size: size,
            archive_sha256: digest,
            member,
            size: binary.len() as u64,
            sha256: hex(&Sha256::digest(&binary)),
        });
    }

    let mut toml =
        format!("# Written by `cargo xtask pin-uv {version}`. Don't edit by hand.\nversion = \"{version}\"\n");
    for p in &pins {
        toml.push_str(&format!(
            "\n[[pin]]\nplatform = \"{}\"\nurl = \"{}\"\narchive_size = {}\narchive_sha256 = \"{}\"\nmember = \"{}\"\nsize = {}\nsha256 = \"{}\"\n",
            p.platform, p.url, p.archive_size, p.archive_sha256, p.member, p.size, p.sha256
        ));
    }
    let path = root.join("crates/as-plugin/uv.toml");
    fs::write(&path, toml)?;
    let _ = fs::remove_dir_all(&work);
    println!("wrote {}", path.display());
    Ok(())
}

/// The bytes of the regular file `member` (its exact path) of the archive
/// `name` (a `.zip` or a `.tar.gz`), read from `bytes`.
fn member_of(name: &str, bytes: &[u8], member: &str) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    if name.ends_with(".zip") {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes))?;
        let mut file = zip.by_name(member)?;
        if !file.is_file() {
            bail!("{member} isn't a file");
        }
        file.read_to_end(&mut out)?;
        return Ok(out);
    }
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
    for entry in tar.entries()? {
        let mut entry = entry?;
        if entry.header().entry_type().is_file() && *entry.path_bytes() == *member.as_bytes() {
            entry.read_to_end(&mut out)?;
            return Ok(out);
        }
    }
    bail!("{member} isn't in the archive")
}

fn gh_json(endpoint: &str) -> Result<Value> {
    let out =
        Command::new("gh").args(["api", endpoint]).output().context("spawning gh (is it installed and logged in?)")?;
    if !out.status.success() {
        bail!("gh api {endpoint} failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(serde_json::from_slice(&out.stdout)?)
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

    #[test]
    fn uv_comes_out_of_either_archive_without_a_tar_program() {
        let uv = b"the uv binary".as_slice();
        // A Windows zip, deflated, with another file before it.
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in [("uvx.exe", b"not uv".as_slice()), ("uv.exe", uv)] {
            zip.start_file(name, deflated).unwrap();
            zip.write_all(data).unwrap();
        }
        let zip = zip.finish().unwrap().into_inner();
        assert_eq!(member_of("uv-x86_64-pc-windows-msvc.zip", &zip, "uv.exe").unwrap(), uv);
        assert!(member_of("uv-x86_64-pc-windows-msvc.zip", &zip, "uvw.exe").is_err());

        // A tar.gz with uv in its folder, after a folder entry and another file.
        let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        let mut folder = tar::Header::new_gnu();
        folder.set_entry_type(tar::EntryType::Directory);
        folder.set_size(0);
        tar.append_data(&mut folder, "uv-x86_64-unknown-linux-gnu/", std::io::empty()).unwrap();
        for (name, data) in
            [("uv-x86_64-unknown-linux-gnu/uvx", b"not uv".as_slice()), ("uv-x86_64-unknown-linux-gnu/uv", uv)]
        {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o755);
            tar.append_data(&mut header, name, data).unwrap();
        }
        let gz = tar.into_inner().unwrap().finish().unwrap();
        let name = "uv-x86_64-unknown-linux-gnu.tar.gz";
        assert_eq!(member_of(name, &gz, "uv-x86_64-unknown-linux-gnu/uv").unwrap(), uv);
        assert!(member_of(name, &gz, "uv-x86_64-unknown-linux-gnu/").is_err());
        assert!(member_of(name, &gz, "uv").is_err());
    }
}

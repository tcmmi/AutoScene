//! Extension manifest schema, validation, and host loader.
//!
//! Parses `extension.json` manifests, validates capability permissions,
//! prevents directory traversal (Zip Slip), and discovers installed
//! extensions in the local AutoScene extensions directory.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;
use thiserror::Error;
use zip::write::SimpleFileOptions;

/// Result type for plugin operations.
pub type PluginResult<T> = Result<T, PluginError>;
pub type ExtensionResult<T> = PluginResult<T>;

/// Errors encountered while loading, validating, or packaging plugins.
#[derive(Debug, Error)]
pub enum PluginError {
    #[error("Manifest file not found: {0}")]
    NotFound(String),
    #[error("Invalid plugin manifest: {0}")]
    InvalidManifest(String),
    #[error("Insecure path traversal detected: {0}")]
    InsecurePath(String),
    #[error("Package archive error: {0}")]
    Archive(String),
    #[error("Security integrity violation: {0}")]
    Security(String),
    #[error("Cryptographic signature error: {0}")]
    Signature(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type ExtensionError = PluginError;

/// Declared capability permissions an extension can request.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub enum ExtensionCapability {
    #[serde(rename = "filesystem:read-project", alias = "filesystem-read-project")]
    FilesystemReadProject,
    #[serde(rename = "filesystem:write-export", alias = "filesystem-write-export")]
    FilesystemWriteExport,
    #[serde(rename = "network:webhook", alias = "network-webhook")]
    NetworkWebhook,
    #[serde(rename = "gpu")]
    Gpu,
}

/// Extension type and lifecycle target.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum ExtensionKind {
    Exporter,
    PostProcessor,
    Webhook,
    Custom,
}

/// Button injected into the TopBar navigation bar.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NavButtonContribution {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub tooltip: Option<String>,
    #[serde(default)]
    pub when: Option<String>,
}

/// Button or entry injected into the Left Navigation Rail.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RailButtonContribution {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub tooltip: Option<String>,
}

/// Command registered into the Command Palette.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CommandContribution {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
}

/// Context menu item contribution across pages.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MenuContribution {
    pub target: String,
    pub label: String,
    #[serde(default)]
    pub icon: Option<String>,
}

/// Declarative setting field for extension configuration.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SettingFieldContribution {
    pub id: String,
    pub label: String,
    #[serde(rename = "type")]
    pub field_type: String,
    #[serde(default)]
    pub default_value: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub options: Option<Vec<String>>,
}

/// Container for declarative UI contribution points.
#[derive(Clone, Debug, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionContributions {
    #[serde(default)]
    pub nav_buttons: Vec<NavButtonContribution>,
    #[serde(default)]
    pub rail_buttons: Vec<RailButtonContribution>,
    #[serde(default)]
    pub commands: Vec<CommandContribution>,
    #[serde(default)]
    pub menus: Vec<MenuContribution>,
    #[serde(default)]
    pub settings: Vec<SettingFieldContribution>,
}

/// The `extension.json` manifest structure.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    #[serde(default)]
    pub author_avatar: Option<String>,
    #[serde(default)]
    pub author_url: Option<String>,
    #[serde(default)]
    pub author_github: Option<String>,
    pub kind: ExtensionKind,
    pub entrypoint: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub banner: Option<String>,
    #[serde(default)]
    pub accent_color: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
    pub capabilities: Vec<ExtensionCapability>,
    #[serde(default)]
    pub triggers: Vec<String>,
    #[serde(default)]
    pub contributes: Option<ExtensionContributions>,
}

impl ExtensionManifest {
    /// Validates basic security boundaries and manifest sanity.
    pub fn validate(&self) -> ExtensionResult<()> {
        if self.id.trim().is_empty() {
            return Err(ExtensionError::InvalidManifest("id cannot be empty".into()));
        }
        if self.entrypoint.contains("..") || self.entrypoint.starts_with('/') || self.entrypoint.starts_with('\\') {
            return Err(ExtensionError::InsecurePath(format!(
                "entrypoint '{}' contains invalid traversal or absolute path",
                self.entrypoint
            )));
        }
        if let Some(icon) = &self.icon
            && (icon.contains("..") || icon.starts_with('/') || icon.starts_with('\\'))
        {
            return Err(ExtensionError::InsecurePath(format!("icon '{icon}' contains invalid traversal")));
        }
        if let Some(banner) = &self.banner
            && (banner.contains("..") || banner.starts_with('/') || banner.starts_with('\\'))
        {
            return Err(ExtensionError::InsecurePath(format!("banner '{banner}' contains invalid traversal")));
        }
        if let Some(avatar) = &self.author_avatar
            && !avatar.starts_with("http://")
            && !avatar.starts_with("https://")
            && (avatar.contains("..") || avatar.starts_with('/') || avatar.starts_with('\\'))
        {
            return Err(ExtensionError::InsecurePath(format!("authorAvatar '{avatar}' contains invalid traversal")));
        }
        Ok(())
    }
}

pub type PluginCapability = ExtensionCapability;
pub type PluginKind = ExtensionKind;
pub type PluginManifest = ExtensionManifest;
pub type InstalledPlugin = InstalledExtension;

/// Status of the cryptographic signature on an `.asp` plugin package.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum PluginSignatureStatus {
    /// Cryptographically verified with AutoScene's official master key.
    Official,
    /// Cryptographically verified with a community/author developer key.
    CommunitySigned,
    /// No digital signature present (unverified/unsigned archive).
    Unsigned,
    /// Signature is present but verification failed, or file digests were tampered with.
    Tampered,
}

/// Metadata and cryptographic verification details of an `.asp` package.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginSignatureInfo {
    pub status: PluginSignatureStatus,
    pub public_key: Option<String>,
    pub key_fingerprint: Option<String>,
    pub author_identity: Option<String>,
    pub signed_at: Option<String>,
    pub error_message: Option<String>,
}

/// Result of inspecting an `.asp` file before deciding to install it.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AspInspectionResult {
    pub manifest: PluginManifest,
    pub signature: PluginSignatureInfo,
    pub file_count: usize,
    pub total_uncompressed_bytes: u64,
}

/// Key pair generated for plugin authors to sign their packages.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginSigningKeyPair {
    pub private_key_hex: String,
    pub public_key_hex: String,
    pub fingerprint: String,
}

/// Integrity manifest stored inside `META-INF/MANIFEST.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AspIntegrityManifest {
    pub version: u32,
    pub plugin_id: String,
    pub plugin_version: String,
    pub created_at: String,
    pub files: BTreeMap<String, String>,
}

/// Cryptographic signature block stored inside `META-INF/SIGNATURE.sig`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AspSignatureBlock {
    pub algorithm: String,
    pub public_key_hex: String,
    pub signature_hex: String,
    pub author_identity: Option<String>,
    pub signed_at: String,
}

/// Generates a new Ed25519 keypair for signing `.asp` plugins.
pub fn generate_plugin_signing_keypair() -> PluginSigningKeyPair {
    use rand_core::OsRng;
    let signing_key = SigningKey::generate(&mut OsRng);
    let verifying_key = signing_key.verifying_key();
    let priv_hex = hex::encode(signing_key.to_bytes());
    let pub_hex = hex::encode(verifying_key.to_bytes());

    let mut hasher = Sha256::new();
    hasher.update(verifying_key.as_bytes());
    let fingerprint = format!("SHA256:{}", hex::encode(hasher.finalize()));

    PluginSigningKeyPair { private_key_hex: priv_hex, public_key_hex: pub_hex, fingerprint }
}

/// Built-in official AutoScene verifying public key (hex-encoded 32-byte Ed25519).
pub const AUTOSCENE_OFFICIAL_PUBLIC_KEY: &str = "a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0";

/// Discovered plugin/extension installed in the system.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InstalledExtension {
    pub manifest: ExtensionManifest,
    pub path: String,
    pub entrypoint_path: String,
    #[serde(default)]
    pub icon_path: Option<String>,
    #[serde(default)]
    pub banner_path: Option<String>,
    #[serde(default)]
    pub author_avatar_path: Option<String>,
    #[serde(default)]
    pub signature_status: Option<PluginSignatureStatus>,
}

/// Scans a directory for installed plugins containing `plugin.json` or `extension.json`.
pub fn discover_extensions(plugins_dir: &Path) -> Vec<InstalledPlugin> {
    let mut installed = Vec::new();
    let Ok(entries) = fs::read_dir(plugins_dir) else {
        return installed;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        // Support both plugin.json and extension.json
        let manifest_path = if path.join("plugin.json").is_file() {
            path.join("plugin.json")
        } else if path.join("extension.json").is_file() {
            path.join("extension.json")
        } else {
            continue;
        };

        let Ok(data) = fs::read(&manifest_path) else {
            continue;
        };

        let Ok(manifest) = serde_json::from_slice::<ExtensionManifest>(&data) else {
            continue;
        };

        if manifest.validate().is_err() {
            continue;
        }

        let entrypoint_path = path.join(&manifest.entrypoint);
        let icon_path = manifest
            .icon
            .as_ref()
            .map(|i| path.join(i))
            .filter(|p| p.is_file())
            .map(|p| p.to_string_lossy().to_string());
        let banner_path = manifest
            .banner
            .as_ref()
            .map(|b| path.join(b))
            .filter(|p| p.is_file())
            .map(|p| p.to_string_lossy().to_string());
        let author_avatar_path = manifest
            .author_avatar
            .as_ref()
            .filter(|a| !a.starts_with("http://") && !a.starts_with("https://"))
            .map(|a| path.join(a))
            .filter(|p| p.is_file())
            .map(|p| p.to_string_lossy().to_string());

        installed.push(InstalledExtension {
            path: path.to_string_lossy().to_string(),
            entrypoint_path: entrypoint_path.to_string_lossy().to_string(),
            icon_path,
            banner_path,
            author_avatar_path,
            signature_status: None,
            manifest,
        });
    }

    installed
}

pub use discover_extensions as discover_plugins;

/// Packages a plugin directory into a `.asp` (AutoScene Plugin) archive, optionally signing it.
pub fn package_asp_signed(
    plugin_dir: &Path,
    out_asp: &Path,
    signing_key_hex: Option<&str>,
    author_identity: Option<&str>,
) -> PluginResult<Option<PluginSigningKeyPair>> {
    let manifest_path = if plugin_dir.join("plugin.json").is_file() {
        plugin_dir.join("plugin.json")
    } else if plugin_dir.join("extension.json").is_file() {
        plugin_dir.join("extension.json")
    } else {
        return Err(PluginError::NotFound("plugin.json manifest not found".into()));
    };

    let data = fs::read(&manifest_path)?;
    let manifest = serde_json::from_slice::<PluginManifest>(&data)?;
    manifest.validate()?;

    // Step 1: Collect files and compute SHA-256 hashes (skipping any existing META-INF)
    let mut file_map: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut hash_map: BTreeMap<String, String> = BTreeMap::new();

    fn collect_files(
        dir: &Path,
        base: &Path,
        files: &mut BTreeMap<String, Vec<u8>>,
        hashes: &mut BTreeMap<String, String>,
    ) -> PluginResult<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let rel = path.strip_prefix(base).map_err(|e| PluginError::Archive(e.to_string()))?;
            let rel_str = rel.to_string_lossy().replace('\\', "/");

            if rel_str.starts_with("META-INF/") || rel_str == "META-INF" {
                continue;
            }

            if path.is_file() {
                let bytes = fs::read(&path)?;
                let mut hasher = Sha256::new();
                hasher.update(&bytes);
                let digest = hex::encode(hasher.finalize());
                hashes.insert(rel_str.clone(), digest);
                files.insert(rel_str, bytes);
            } else if path.is_dir() {
                collect_files(&path, base, files, hashes)?;
            }
        }
        Ok(())
    }

    collect_files(plugin_dir, plugin_dir, &mut file_map, &mut hash_map)?;

    // Step 2: If a signing key is provided, generate META-INF/MANIFEST.json and META-INF/SIGNATURE.sig
    let mut key_pair_info = None;
    if let Some(key_hex) = signing_key_hex {
        let key_bytes =
            hex::decode(key_hex.trim()).map_err(|e| PluginError::Signature(format!("Invalid signing key hex: {e}")))?;
        let signing_key_bytes: [u8; 32] =
            key_bytes.try_into().map_err(|_| PluginError::Signature("Private key must be exactly 32 bytes".into()))?;
        let signing_key = SigningKey::from_bytes(&signing_key_bytes);
        let verifying_key = signing_key.verifying_key();

        let mut pub_hasher = Sha256::new();
        pub_hasher.update(verifying_key.as_bytes());
        let fingerprint = format!("SHA256:{}", hex::encode(pub_hasher.finalize()));

        key_pair_info = Some(PluginSigningKeyPair {
            private_key_hex: hex::encode(signing_key.to_bytes()),
            public_key_hex: hex::encode(verifying_key.to_bytes()),
            fingerprint,
        });

        let integrity_manifest = AspIntegrityManifest {
            version: 1,
            plugin_id: manifest.id.clone(),
            plugin_version: manifest.version.clone(),
            created_at: chrono_like_now(),
            files: hash_map,
        };

        let manifest_bytes = serde_json::to_vec_pretty(&integrity_manifest)?;
        let signature = signing_key.sign(&manifest_bytes);

        let sig_block = AspSignatureBlock {
            algorithm: "ed25519".into(),
            public_key_hex: hex::encode(verifying_key.to_bytes()),
            signature_hex: hex::encode(signature.to_bytes()),
            author_identity: author_identity.map(|s| s.to_string()),
            signed_at: chrono_like_now(),
        };

        let sig_bytes = serde_json::to_vec_pretty(&sig_block)?;

        file_map.insert("META-INF/MANIFEST.json".into(), manifest_bytes);
        file_map.insert("META-INF/SIGNATURE.sig".into(), sig_bytes);
    }

    // Step 3: Write out ZIP archive
    let file = File::create(out_asp)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    for (name, content) in file_map {
        zip.start_file(name, options).map_err(|e| PluginError::Archive(e.to_string()))?;
        zip.write_all(&content)?;
    }

    zip.finish().map_err(|e| PluginError::Archive(e.to_string()))?;
    Ok(key_pair_info)
}

/// Convenience helper: packages without signing.
pub fn package_asp(plugin_dir: &Path, out_asp: &Path) -> PluginResult<()> {
    package_asp_signed(plugin_dir, out_asp, None, None).map(|_| ())
}

fn chrono_like_now() -> String {
    "2026-11-02T14:15:00Z".into()
}

/// Inspects an `.asp` package without extracting it, verifying its cryptographic signature and integrity.
pub fn inspect_asp(asp_path: &Path) -> PluginResult<AspInspectionResult> {
    let file = File::open(asp_path)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| PluginError::Archive(e.to_string()))?;

    let mut manifest_data: Option<Vec<u8>> = None;
    let mut meta_manifest_data: Option<Vec<u8>> = None;
    let mut meta_signature_data: Option<Vec<u8>> = None;
    let mut file_contents: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut total_uncompressed_bytes: u64 = 0;
    let mut file_count = 0;

    for i in 0..archive.len() {
        let mut f = archive.by_index(i).map_err(|e| PluginError::Archive(e.to_string()))?;
        if f.name().ends_with('/') {
            continue;
        }

        // Verify path traversal
        if f.enclosed_name().is_none() {
            return Err(PluginError::InsecurePath("Zip Slip detected in archive".into()));
        }

        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        total_uncompressed_bytes += buf.len() as u64;
        file_count += 1;

        let name = f.name().replace('\\', "/");
        if name == "plugin.json" || name == "extension.json" {
            manifest_data = Some(buf.clone());
        } else if name == "META-INF/MANIFEST.json" {
            meta_manifest_data = Some(buf.clone());
        } else if name == "META-INF/SIGNATURE.sig" {
            meta_signature_data = Some(buf.clone());
        }

        file_contents.insert(name, buf);
    }

    let Some(m_bytes) = manifest_data else {
        return Err(PluginError::NotFound("Archive does not contain plugin.json or extension.json".into()));
    };

    let manifest = serde_json::from_slice::<PluginManifest>(&m_bytes)?;
    manifest.validate()?;

    // Evaluate signature status
    let signature_info = match (meta_manifest_data, meta_signature_data) {
        (Some(manifest_bytes), Some(sig_bytes)) => verify_asp_signature(&manifest_bytes, &sig_bytes, &file_contents),
        _ => PluginSignatureInfo {
            status: PluginSignatureStatus::Unsigned,
            public_key: None,
            key_fingerprint: None,
            author_identity: None,
            signed_at: None,
            error_message: Some("No cryptographic signature found (unsigned plugin)".into()),
        },
    };

    Ok(AspInspectionResult { manifest, signature: signature_info, file_count, total_uncompressed_bytes })
}

fn verify_asp_signature(
    manifest_bytes: &[u8],
    sig_bytes: &[u8],
    files: &BTreeMap<String, Vec<u8>>,
) -> PluginSignatureInfo {
    let integrity_manifest: AspIntegrityManifest = match serde_json::from_slice(manifest_bytes) {
        Ok(m) => m,
        Err(e) => {
            return PluginSignatureInfo {
                status: PluginSignatureStatus::Tampered,
                public_key: None,
                key_fingerprint: None,
                author_identity: None,
                signed_at: None,
                error_message: Some(format!("Corrupt integrity manifest: {e}")),
            };
        }
    };

    let sig_block: AspSignatureBlock = match serde_json::from_slice(sig_bytes) {
        Ok(s) => s,
        Err(e) => {
            return PluginSignatureInfo {
                status: PluginSignatureStatus::Tampered,
                public_key: None,
                key_fingerprint: None,
                author_identity: None,
                signed_at: None,
                error_message: Some(format!("Corrupt signature block: {e}")),
            };
        }
    };

    // 1. Verify file digests against files map
    for (filename, expected_digest) in &integrity_manifest.files {
        match files.get(filename) {
            Some(content) => {
                let mut hasher = Sha256::new();
                hasher.update(content);
                let actual_digest = hex::encode(hasher.finalize());
                if &actual_digest != expected_digest {
                    return PluginSignatureInfo {
                        status: PluginSignatureStatus::Tampered,
                        public_key: Some(sig_block.public_key_hex),
                        key_fingerprint: None,
                        author_identity: sig_block.author_identity,
                        signed_at: Some(sig_block.signed_at),
                        error_message: Some(format!("File digest mismatch on '{filename}'. File has been altered!")),
                    };
                }
            }
            None => {
                return PluginSignatureInfo {
                    status: PluginSignatureStatus::Tampered,
                    public_key: Some(sig_block.public_key_hex),
                    key_fingerprint: None,
                    author_identity: sig_block.author_identity,
                    signed_at: Some(sig_block.signed_at),
                    error_message: Some(format!("File '{filename}' declared in manifest is missing!")),
                };
            }
        }
    }

    // 2. Parse verifying key
    let pub_bytes = match hex::decode(&sig_block.public_key_hex) {
        Ok(b) => b,
        Err(_) => {
            return PluginSignatureInfo {
                status: PluginSignatureStatus::Tampered,
                public_key: Some(sig_block.public_key_hex),
                key_fingerprint: None,
                author_identity: sig_block.author_identity,
                signed_at: Some(sig_block.signed_at),
                error_message: Some("Invalid public key encoding".into()),
            };
        }
    };

    let verifying_key_bytes: [u8; 32] = match pub_bytes.try_into() {
        Ok(b) => b,
        Err(_) => {
            return PluginSignatureInfo {
                status: PluginSignatureStatus::Tampered,
                public_key: Some(sig_block.public_key_hex),
                key_fingerprint: None,
                author_identity: sig_block.author_identity,
                signed_at: Some(sig_block.signed_at),
                error_message: Some("Public key is not 32 bytes".into()),
            };
        }
    };

    let verifying_key = match VerifyingKey::from_bytes(&verifying_key_bytes) {
        Ok(k) => k,
        Err(e) => {
            return PluginSignatureInfo {
                status: PluginSignatureStatus::Tampered,
                public_key: Some(sig_block.public_key_hex),
                key_fingerprint: None,
                author_identity: sig_block.author_identity,
                signed_at: Some(sig_block.signed_at),
                error_message: Some(format!("Invalid public key: {e}")),
            };
        }
    };

    // 3. Parse and verify signature over manifest_bytes
    let sig_raw = match hex::decode(&sig_block.signature_hex) {
        Ok(b) => b,
        Err(_) => {
            return PluginSignatureInfo {
                status: PluginSignatureStatus::Tampered,
                public_key: Some(sig_block.public_key_hex),
                key_fingerprint: None,
                author_identity: sig_block.author_identity,
                signed_at: Some(sig_block.signed_at),
                error_message: Some("Invalid signature hex encoding".into()),
            };
        }
    };

    let sig_bytes_64: [u8; 64] = match sig_raw.try_into() {
        Ok(b) => b,
        Err(_) => {
            return PluginSignatureInfo {
                status: PluginSignatureStatus::Tampered,
                public_key: Some(sig_block.public_key_hex),
                key_fingerprint: None,
                author_identity: sig_block.author_identity,
                signed_at: Some(sig_block.signed_at),
                error_message: Some("Signature is not 64 bytes".into()),
            };
        }
    };

    let signature = Signature::from_bytes(&sig_bytes_64);
    if let Err(e) = verifying_key.verify(manifest_bytes, &signature) {
        return PluginSignatureInfo {
            status: PluginSignatureStatus::Tampered,
            public_key: Some(sig_block.public_key_hex),
            key_fingerprint: None,
            author_identity: sig_block.author_identity,
            signed_at: Some(sig_block.signed_at),
            error_message: Some(format!("Cryptographic signature verification failed: {e}")),
        };
    }

    let mut fp_hasher = Sha256::new();
    fp_hasher.update(verifying_key.as_bytes());
    let fingerprint = format!("SHA256:{}", hex::encode(fp_hasher.finalize()));

    let status = if sig_block.public_key_hex == AUTOSCENE_OFFICIAL_PUBLIC_KEY {
        PluginSignatureStatus::Official
    } else {
        PluginSignatureStatus::CommunitySigned
    };

    PluginSignatureInfo {
        status,
        public_key: Some(sig_block.public_key_hex),
        key_fingerprint: Some(fingerprint),
        author_identity: sig_block.author_identity,
        signed_at: Some(sig_block.signed_at),
        error_message: None,
    }
}

/// Installs a `.asp` (AutoScene Plugin) archive by unpacking it into the plugins folder.
/// Automatically verifies archive integrity and cryptographic signature.
pub fn install_asp(asp_path: &Path, plugins_dir: &Path, allow_unsigned: bool) -> PluginResult<InstalledPlugin> {
    let inspection = inspect_asp(asp_path)?;

    if inspection.signature.status == PluginSignatureStatus::Tampered {
        return Err(PluginError::Security(format!(
            "Plugin package verification failed: {}",
            inspection.signature.error_message.unwrap_or_else(|| "Tampered files detected".into())
        )));
    }

    if inspection.signature.status == PluginSignatureStatus::Unsigned && !allow_unsigned {
        return Err(PluginError::Security(
            "Installation blocked: Plugin is not signed. Explicit confirmation required.".into(),
        ));
    }

    let file = File::open(asp_path)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| PluginError::Archive(e.to_string()))?;

    let manifest = inspection.manifest;
    let target_dir = plugins_dir.join(&manifest.id);
    if target_dir.exists() {
        fs::remove_dir_all(&target_dir)?;
    }
    fs::create_dir_all(&target_dir)?;

    // Extract files safely
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| PluginError::Archive(e.to_string()))?;
        let name = file.name().replace('\\', "/");

        // Do not extract META-INF verification files to plugins runtime folder
        if name.starts_with("META-INF/") {
            continue;
        }

        let outpath = match file.enclosed_name() {
            Some(path) => target_dir.join(path),
            None => return Err(PluginError::InsecurePath("Zip Slip detected".into())),
        };

        if file.name().ends_with('/') {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(p) = outpath.parent()
                && !p.exists()
            {
                fs::create_dir_all(p)?;
            }
            let mut outfile = File::create(&outpath)?;
            std::io::copy(&mut file, &mut outfile)?;
        }

        // Restore execute permissions on Unix for scripts and binaries
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(mode) = file.unix_mode() {
                if mode & 0o111 != 0 {
                    let _ = fs::set_permissions(&outpath, fs::Permissions::from_mode(0o755));
                }
            } else if outpath.extension().is_some_and(|ext| ext == "sh" || ext == "py") {
                let _ = fs::set_permissions(&outpath, fs::Permissions::from_mode(0o755));
            }
        }
    }

    let entrypoint_path = target_dir.join(&manifest.entrypoint);
    let icon_path = manifest
        .icon
        .as_ref()
        .map(|i| target_dir.join(i))
        .filter(|p| p.is_file())
        .map(|p| p.to_string_lossy().to_string());
    let banner_path = manifest
        .banner
        .as_ref()
        .map(|b| target_dir.join(b))
        .filter(|p| p.is_file())
        .map(|p| p.to_string_lossy().to_string());
    let author_avatar_path = manifest
        .author_avatar
        .as_ref()
        .filter(|a| !a.starts_with("http://") && !a.starts_with("https://"))
        .map(|a| target_dir.join(a))
        .filter(|p| p.is_file())
        .map(|p| p.to_string_lossy().to_string());

    Ok(InstalledPlugin {
        path: target_dir.to_string_lossy().to_string(),
        entrypoint_path: entrypoint_path.to_string_lossy().to_string(),
        icon_path,
        banner_path,
        author_avatar_path,
        signature_status: Some(inspection.signature.status),
        manifest,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_json_deserialization() {
        let json = r#"{
          "id": "autoscene.sample.export-logger",
          "name": "Export Summary Logger",
          "version": "1.0.0",
          "description": "Logs export details, clip counts, and output files to a summary log file.",
          "author": "AutoScene Team",
          "kind": "post-processor",
          "entrypoint": "run.sh",
          "capabilities": [
            "filesystem:write-export"
          ],
          "triggers": [
            "post-export"
          ]
        }"#;

        let res = serde_json::from_str::<ExtensionManifest>(json);
        match &res {
            Ok(m) => println!("Success: {:?}", m),
            Err(e) => println!("Error: {:?}", e),
        }
        assert!(res.is_ok(), "Failed to deserialize sample: {:?}", res.err());
    }

    #[test]
    fn validates_clean_manifest() {
        let manifest = ExtensionManifest {
            id: "com.example.discord-notify".into(),
            name: "Discord Notifier".into(),
            version: "1.0.0".into(),
            description: "Sends notifications".into(),
            author: "Tester".into(),
            author_avatar: None,
            author_url: None,
            author_github: None,
            kind: ExtensionKind::Webhook,
            entrypoint: "notify.sh".into(),
            icon: Some("icon.png".into()),
            banner: Some("banner.jpg".into()),
            accent_color: Some("#ff0000".into()),
            homepage: Some("https://example.com".into()),
            repository: Some("https://github.com/example/repo".into()),
            capabilities: vec![ExtensionCapability::NetworkWebhook],
            triggers: vec!["post-export".into()],
            contributes: None,
        };

        assert!(manifest.validate().is_ok());
    }

    #[test]
    fn rejects_path_traversal_in_entrypoint() {
        let manifest = ExtensionManifest {
            id: "com.example.malicious".into(),
            name: "Malicious".into(),
            version: "1.0.0".into(),
            description: "Bad".into(),
            author: "Attacker".into(),
            author_avatar: None,
            author_url: None,
            author_github: None,
            kind: ExtensionKind::Custom,
            entrypoint: "../../etc/passwd".into(),
            icon: None,
            banner: None,
            accent_color: None,
            homepage: None,
            repository: None,
            capabilities: vec![],
            triggers: vec![],
            contributes: None,
        };

        assert!(manifest.validate().is_err());
    }

    #[test]
    fn discovers_valid_extensions_in_folder() {
        let temp = tempfile::tempdir().unwrap();
        let ext_dir = temp.path().join("my-ext");
        fs::create_dir_all(&ext_dir).unwrap();

        let manifest = ExtensionManifest {
            id: "my-ext".into(),
            name: "My Ext".into(),
            version: "0.1.0".into(),
            description: "Test".into(),
            author: "Dev".into(),
            author_avatar: None,
            author_url: None,
            author_github: None,
            kind: ExtensionKind::Exporter,
            entrypoint: "main.py".into(),
            icon: None,
            banner: None,
            accent_color: None,
            homepage: None,
            repository: None,
            capabilities: vec![ExtensionCapability::FilesystemWriteExport],
            triggers: vec![],
            contributes: None,
        };

        let manifest_bytes = serde_json::to_vec_pretty(&manifest).unwrap();
        fs::write(ext_dir.join("extension.json"), manifest_bytes).unwrap();
        fs::write(ext_dir.join("main.py"), "print('hello')").unwrap();

        let found = discover_extensions(temp.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].manifest.id, "my-ext");
        assert_eq!(found[0].manifest.kind, ExtensionKind::Exporter);
    }

    #[test]
    fn packages_and_installs_asp_archive() {
        let temp = tempfile::tempdir().unwrap();
        let src_dir = temp.path().join("source-plugin");
        fs::create_dir_all(&src_dir).unwrap();

        let manifest = PluginManifest {
            id: "test.plugin.asp".into(),
            name: "Test ASP Plugin".into(),
            version: "1.0.0".into(),
            description: "Test".into(),
            author: "Tester".into(),
            author_avatar: None,
            author_url: None,
            author_github: None,
            kind: PluginKind::PostProcessor,
            entrypoint: "run.sh".into(),
            icon: None,
            banner: None,
            accent_color: None,
            homepage: None,
            repository: None,
            capabilities: vec![PluginCapability::FilesystemWriteExport],
            triggers: vec!["post-export".into()],
            contributes: None,
        };

        fs::write(src_dir.join("plugin.json"), serde_json::to_vec(&manifest).unwrap()).unwrap();
        fs::write(src_dir.join("run.sh"), "#!/bin/sh\necho ok").unwrap();

        let asp_file = temp.path().join("test.plugin.asp");
        package_asp(&src_dir, &asp_file).expect("package_asp succeeds");
        assert!(asp_file.is_file());

        let target_dir = temp.path().join("installed_plugins");
        fs::create_dir_all(&target_dir).unwrap();

        let installed = install_asp(&asp_file, &target_dir, true).expect("install_asp succeeds");
        assert_eq!(installed.manifest.id, "test.plugin.asp");
        assert!(Path::new(&installed.entrypoint_path).is_file());
    }

    #[test]
    fn signs_and_verifies_asp_package_cryptographically() {
        let temp = tempfile::tempdir().unwrap();
        let src_dir = temp.path().join("secure-plugin");
        fs::create_dir_all(&src_dir).unwrap();

        let manifest = PluginManifest {
            id: "secure.plugin.verified".into(),
            name: "Secure Verified Plugin".into(),
            version: "1.0.0".into(),
            description: "Signed plugin test".into(),
            author: "Security Author".into(),
            author_avatar: None,
            author_url: None,
            author_github: None,
            kind: PluginKind::PostProcessor,
            entrypoint: "main.py".into(),
            icon: None,
            banner: None,
            accent_color: None,
            homepage: None,
            repository: None,
            capabilities: vec![],
            triggers: vec![],
            contributes: None,
        };

        fs::write(src_dir.join("plugin.json"), serde_json::to_vec(&manifest).unwrap()).unwrap();
        fs::write(src_dir.join("main.py"), "print('secure code')").unwrap();

        // 1. Generate keypair and sign
        let keypair = generate_plugin_signing_keypair();
        let asp_path = temp.path().join("secure.plugin.asp");

        let key_info = package_asp_signed(
            &src_dir,
            &asp_path,
            Some(&keypair.private_key_hex),
            Some("Security Author <sec@example.com>"),
        )
        .expect("package signed succeeds");

        assert!(key_info.is_some());
        assert_eq!(key_info.unwrap().public_key_hex, keypair.public_key_hex);

        // 2. Inspect package
        let inspection = inspect_asp(&asp_path).expect("inspect_asp succeeds");
        assert_eq!(inspection.signature.status, PluginSignatureStatus::CommunitySigned);
        assert_eq!(inspection.signature.public_key, Some(keypair.public_key_hex));
        assert!(inspection.signature.key_fingerprint.is_some());
        assert_eq!(inspection.signature.author_identity, Some("Security Author <sec@example.com>".into()));

        // 3. Install package
        let target_dir = temp.path().join("installed");
        let installed = install_asp(&asp_path, &target_dir, false).expect("install_asp succeeds");
        assert_eq!(installed.manifest.id, "secure.plugin.verified");
        assert_eq!(installed.signature_status, Some(PluginSignatureStatus::CommunitySigned));
    }

    #[test]
    fn detects_tampered_payload_in_asp_package() {
        let temp = tempfile::tempdir().unwrap();
        let src_dir = temp.path().join("tamper-source");
        fs::create_dir_all(&src_dir).unwrap();

        let manifest = PluginManifest {
            id: "tampered.plugin".into(),
            name: "Tampered".into(),
            version: "1.0.0".into(),
            description: "Test".into(),
            author: "Tester".into(),
            author_avatar: None,
            author_url: None,
            author_github: None,
            kind: PluginKind::Custom,
            entrypoint: "main.py".into(),
            icon: None,
            banner: None,
            accent_color: None,
            homepage: None,
            repository: None,
            capabilities: vec![],
            triggers: vec![],
            contributes: None,
        };

        fs::write(src_dir.join("plugin.json"), serde_json::to_vec(&manifest).unwrap()).unwrap();
        fs::write(src_dir.join("main.py"), "print('original')").unwrap();

        let keypair = generate_plugin_signing_keypair();
        let asp_path = temp.path().join("tamper.asp");
        package_asp_signed(&src_dir, &asp_path, Some(&keypair.private_key_hex), None).unwrap();

        // Tamper with main.py inside the archive
        // We unpack to a folder, modify main.py, and zip again keeping the old signature
        let extract_dir = temp.path().join("tamper-work");
        fs::create_dir_all(&extract_dir).unwrap();
        {
            let mut zip = zip::ZipArchive::new(File::open(&asp_path).unwrap()).unwrap();
            zip.extract(&extract_dir).unwrap();
        }

        // Alter main.py without updating signature
        fs::write(extract_dir.join("main.py"), "print('injected malicious virus')").unwrap();

        // Repack into tampered.asp
        let tampered_asp = temp.path().join("tampered_corrupt.asp");
        {
            let file = File::create(&tampered_asp).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            let opts = SimpleFileOptions::default();

            fn add_all(dir: &Path, base: &Path, writer: &mut zip::ZipWriter<File>, opts: SimpleFileOptions) {
                for entry in fs::read_dir(dir).unwrap() {
                    let entry = entry.unwrap();
                    let path = entry.path();
                    if path.is_file() {
                        let rel = path.strip_prefix(base).unwrap().to_str().unwrap().replace('\\', "/");
                        writer.start_file(rel, opts).unwrap();
                        let content = fs::read(&path).unwrap();
                        writer.write_all(&content).unwrap();
                    } else if path.is_dir() {
                        add_all(&path, base, writer, opts);
                    }
                }
            }

            add_all(&extract_dir, &extract_dir, &mut writer, opts);
            writer.finish().unwrap();
        }

        // Inspect and install must detect tampering
        let inspection = inspect_asp(&tampered_asp).expect("inspection completes");
        assert_eq!(inspection.signature.status, PluginSignatureStatus::Tampered);
        assert!(inspection.signature.error_message.unwrap().contains("digest mismatch"));

        let target_dir = temp.path().join("installed");
        let install_res = install_asp(&tampered_asp, &target_dir, true);
        assert!(install_res.is_err());
    }
}

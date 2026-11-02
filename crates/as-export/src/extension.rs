//! Extension manifest schema, validation, and host loader.
//!
//! Parses `extension.json` manifests, validates capability permissions,
//! prevents directory traversal (Zip Slip), and discovers installed
//! extensions in the local AutoScene extensions directory.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use specta::Type;
use thiserror::Error;

/// Result type for extension operations.
pub type ExtensionResult<T> = Result<T, ExtensionError>;

/// Errors encountered while loading or validating extensions.
#[derive(Debug, Error)]
pub enum ExtensionError {
    #[error("Manifest file not found: {0}")]
    NotFound(String),
    #[error("Invalid extension manifest: {0}")]
    InvalidManifest(String),
    #[error("Insecure path traversal detected: {0}")]
    InsecurePath(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

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
        if let Some(icon) = &self.icon {
            if icon.contains("..") || icon.starts_with('/') || icon.starts_with('\\') {
                return Err(ExtensionError::InsecurePath(format!("icon '{icon}' contains invalid traversal")));
            }
        }
        if let Some(banner) = &self.banner {
            if banner.contains("..") || banner.starts_with('/') || banner.starts_with('\\') {
                return Err(ExtensionError::InsecurePath(format!("banner '{banner}' contains invalid traversal")));
            }
        }
        if let Some(avatar) = &self.author_avatar {
            if !avatar.starts_with("http://") && !avatar.starts_with("https://") {
                if avatar.contains("..") || avatar.starts_with('/') || avatar.starts_with('\\') {
                    return Err(ExtensionError::InsecurePath(format!("authorAvatar '{avatar}' contains invalid traversal")));
                }
            }
        }
        Ok(())
    }
}

/// Discovered extension installed in the system.
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
}

/// Scans a directory for installed extensions containing `extension.json`.
pub fn discover_extensions(extensions_dir: &Path) -> Vec<InstalledExtension> {
    let mut installed = Vec::new();
    let Ok(entries) = fs::read_dir(extensions_dir) else {
        return installed;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let manifest_path = path.join("extension.json");
        if !manifest_path.is_file() {
            continue;
        }

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
            manifest,
        });
    }

    installed
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
}

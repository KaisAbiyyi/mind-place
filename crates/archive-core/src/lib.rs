use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

mod generic;
mod legacy;
mod store;
pub use generic::{GenericImportReceipt, MAX_GENERIC_IMPORT_BYTES};
pub use store::{conversation_id_from_url, markdown, MAX_CAPTURE_BYTES};

pub const NATIVE_HOST_NAME: &str = "com.mindplace.capture";

pub fn native_host_manifest(host: &Path) -> Result<serde_json::Value> {
    if !host.is_absolute() || !host.is_file() {
        return Err(ArchiveError::Invalid(
            "Build or install the native capture host before browser setup".into(),
        ));
    }
    store::no_link(host)?;
    Ok(serde_json::json!({
        "name":NATIVE_HOST_NAME,"description":"Mind Place manual conversation capture","path":host.display().to_string(),"type":"stdio",
        "allowed_origins":[format!("chrome-extension://{}/",extension_id())]
    }))
}

pub fn extension_id() -> String {
    use base64::{engine::general_purpose::STANDARD, Engine};
    use sha2::{Digest, Sha256};
    let key = STANDARD
        .decode(include_str!("../../../extension/public-key.txt").trim())
        .expect("valid bundled extension public key");
    Sha256::digest(key)[..16]
        .iter()
        .flat_map(|byte| [(b'a' + (byte >> 4)) as char, (b'a' + (byte & 15)) as char])
        .collect()
}

pub fn data_root() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("MIND_PLACE_DATA_DIR") {
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return Err(ArchiveError::Invalid(
                "MIND_PLACE_DATA_DIR must be an absolute path".into(),
            ));
        }
        return Ok(path);
    }
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/share")));
    base.map(|p| p.join("Mind Place").join("archive-v1"))
        .ok_or_else(|| {
            ArchiveError::Invalid("Local application data directory is unavailable".into())
        })
}

pub type Result<T> = std::result::Result<T, ArchiveError>;

#[derive(Debug, thiserror::Error)]
pub enum ArchiveError {
    #[error("{0}")]
    Invalid(String),
    #[error("Archive storage error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid archive JSON: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Capture {
    pub schema_version: u32,
    pub provider: String,
    pub conversation_id: String,
    pub source_url: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_updated_at: Option<String>,
    pub captured_at: String,
    pub capture_method: String,
    pub completeness: String,
    #[serde(default)]
    pub warnings: Vec<String>,
    pub messages: Vec<Message>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub id: String,
    pub role: String,
    pub text: String,
    pub format: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authored_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Attachment {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversationSummary {
    pub id: String,
    pub conversation_id: String,
    pub provider: String,
    pub title: String,
    pub source_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_source: Option<String>,
    pub captured_at: String,
    pub message_count: usize,
    pub snapshot_count: usize,
    pub completeness: String,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Conversation {
    pub summary: ConversationSummary,
    pub capture: Capture,
    pub snapshot_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SaveReceipt {
    pub ok: bool,
    pub id: String,
    pub snapshot_id: String,
    pub message_count: usize,
    pub unchanged: bool,
    pub completeness: String,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnapshotSummary {
    pub snapshot_id: String,
    pub captured_at: String,
    pub message_count: usize,
    pub completeness: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExportResult {
    pub path: String,
    pub markdown: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AssetResult {
    pub mime_type: String,
    pub data_url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ImportReceipt {
    pub imported: usize,
    pub skipped: usize,
    pub failed: usize,
    pub errors: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct ArchiveStore {
    root: PathBuf,
}

impl ArchiveStore {
    /// Legacy plaintext storage must not be used in an MP-9B encrypted vault.
    /// This is also enforced at write time for already-open desktop handles,
    /// while the independent native capture host reopens on every save.
    pub(crate) fn ensure_not_encrypted_vault(&self) -> Result<()> {
        for parent in [Some(self.root.as_path()), self.root.parent()]
            .into_iter()
            .flatten()
        {
            let marker = parent.join("vault-v1.json");
            match std::fs::symlink_metadata(marker) {
                Ok(_) => {
                    return Err(ArchiveError::Invalid(
                        "Encrypted vault detected: legacy plaintext archive access is disabled"
                            .into(),
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(ArchiveError::Io(error)),
            }
        }
        Ok(())
    }

    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let store = Self {
            root: root.as_ref().to_owned(),
        };
        store.ensure_not_encrypted_vault()?;
        store.initialize()?;
        Ok(store)
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[cfg(test)]
mod encrypted_vault_boundary_tests {
    use super::*;

    #[test]
    fn plaintext_archive_open_refuses_marked_vault_without_creating_directories() {
        let fixture = tempfile::tempdir().unwrap();
        let vault = fixture.path().join("vault");
        std::fs::create_dir(&vault).unwrap();
        std::fs::write(vault.join("vault-v1.json"), b"synthetic-marker").unwrap();

        let legacy_archive = vault.join("archive");
        assert!(matches!(
            ArchiveStore::open(&legacy_archive),
            Err(ArchiveError::Invalid(_))
        ));
        assert!(!legacy_archive.exists());
    }

    #[test]
    fn already_open_plaintext_handle_refuses_vault_marker_before_writing() {
        let fixture = tempfile::tempdir().unwrap();
        let vault = fixture.path().join("vault");
        let legacy_archive = vault.join("archive");
        let store = ArchiveStore::open(&legacy_archive).unwrap();
        std::fs::write(vault.join("vault-v1.json"), b"synthetic-marker").unwrap();

        assert!(matches!(store.lock(), Err(ArchiveError::Invalid(_))));
        assert!(!legacy_archive.join(".archive.lock").exists());
    }
}

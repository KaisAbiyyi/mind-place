//! Owner-session encrypted capture writer/reader. No legacy raw archive files.
//! This adapter is independent from production routing until authenticated
//! native-host IPC and derived search indexing are fully ported and qualified.
use crate::{documents::DocumentIndex, IndexError, IndexStore, Result};
use mind_place_archive::{
    markdown, validate_capture, Capture, Conversation, ConversationSummary, ExportResult,
    SaveReceipt, SnapshotSummary, MAX_CAPTURE_BYTES,
};
use mind_place_security::{
    vault::{ContentKind, CryptoVault},
    vault_fence::VaultWriterFence,
    vault_storage::ImmutableVaultStorage,
    SecurityGate, UnlockLease,
};
use mind_place_state::{CipherKey, StateStore};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use sha2::{Digest, Sha256};
use std::{
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
    sync::Arc,
};
use zeroize::Zeroize;

const APPLICATION_ID: i64 = 0x4d505641; // MPVA (never legacy MPIX).
const SCHEMA_VERSION: i64 = 1;
const SCHEMA: &str = "
CREATE TABLE vault_captures(
    archive_id TEXT NOT NULL,
    snapshot_id TEXT NOT NULL,
    identity TEXT NOT NULL,
    provider TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    title TEXT NOT NULL,
    source_url TEXT NOT NULL,
    captured_at TEXT NOT NULL,
    message_count INTEGER NOT NULL,
    completeness TEXT NOT NULL,
    warnings_json TEXT NOT NULL,
    PRIMARY KEY(archive_id,snapshot_id),
    UNIQUE(identity)
);
CREATE INDEX vault_capture_recent ON vault_captures(archive_id,captured_at DESC);
";

pub struct VaultCaptureStore {
    root: PathBuf,
    vault: CryptoVault,
    gate: SecurityGate,
    lease: UnlockLease,
    cipher: Arc<CipherKey>,
}

/// A read may open a writable SQLCipher pager for WAL configuration. Keep
/// its entire lifetime inside the same writer fence as backup and capture.
struct VaultConnection {
    db: Connection,
    _writer: VaultWriterFence,
}
impl Deref for VaultConnection {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        &self.db
    }
}
impl DerefMut for VaultConnection {
    fn deref_mut(&mut self) -> &mut Connection {
        &mut self.db
    }
}

impl VaultCaptureStore {
    pub fn open(
        root: impl AsRef<Path>,
        vault: CryptoVault,
        gate: &SecurityGate,
        lease: UnlockLease,
    ) -> Result<Self> {
        gate.validate_lease(lease)
            .map_err(|_| IndexError::Revoked)?;
        let root = root.as_ref().to_owned();
        ImmutableVaultStorage::new(&root, &vault, gate, lease)
            .map_err(|_| IndexError::InvalidEvidence)?;
        let derived = vault
            .derived_key(gate, lease, b"archive-index/sqlcipher/v1")
            .map_err(|_| IndexError::Revoked)?;
        let mut raw = [0u8; 32];
        raw.copy_from_slice(derived.as_ref());
        let cipher = CipherKey::new(raw, gate, lease);
        raw.zeroize();
        let store = Self {
            root,
            vault,
            gate: gate.clone(),
            lease,
            cipher,
        };
        {
            let _fence =
                VaultWriterFence::writer(&store.root).map_err(|_| IndexError::InvalidEvidence)?;
            let db = store.connection()?;
            let current: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
            let app: i64 = db.query_row("PRAGMA application_id", [], |r| r.get(0))?;
            if current == 0 && app == 0 {
                db.execute_batch(SCHEMA)?;
                db.pragma_update(None, "application_id", APPLICATION_ID)?;
                db.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            } else if current != SCHEMA_VERSION || app != APPLICATION_ID {
                return Err(IndexError::InvalidIndex);
            }
            Self::integrity(&db)?;
        }
        Ok(store)
    }

    /// All desktop readers/writers must use vault-derived keys. A legacy
    /// StateStore/IndexStore open on a vault path is never an allowed fallback.
    fn derived_sqlcipher_key(&self, domain: &[u8]) -> Result<[u8; 32]> {
        self.gate
            .validate_lease(self.lease)
            .map_err(|_| IndexError::Revoked)?;
        let key = self
            .vault
            .derived_key(&self.gate, self.lease, domain)
            .map_err(|_| IndexError::Revoked)?;
        let mut raw = [0u8; 32];
        raw.copy_from_slice(key.as_ref());
        Ok(raw)
    }

    pub fn open_state_store(&self) -> Result<StateStore> {
        Ok(StateStore::open_encrypted(
            self.root.join("state.sqlite3"),
            self.derived_sqlcipher_key(b"state/sqlcipher/v1")?,
            &self.gate,
            self.lease,
        )?)
    }

    pub fn open_index_store(&self) -> Result<IndexStore> {
        IndexStore::open_encrypted(
            self.root.join("index.sqlite3"),
            self.derived_sqlcipher_key(b"index/sqlcipher/v1")?,
            &self.gate,
            self.lease,
        )
    }

    pub fn open_document_index(&self) -> Result<DocumentIndex> {
        DocumentIndex::open_encrypted(
            self.root.join("document-index.sqlite3"),
            self.derived_sqlcipher_key(b"document-index/sqlcipher/v1")?,
            &self.gate,
            self.lease,
        )
    }

    fn db_path(&self) -> PathBuf {
        self.root.join("archive-index.sqlite3")
    }

    fn connection(&self) -> Result<VaultConnection> {
        self.gate
            .validate_lease(self.lease)
            .map_err(|_| IndexError::Revoked)?;
        let writer =
            VaultWriterFence::writer(&self.root).map_err(|_| IndexError::InvalidEvidence)?;
        let path = self.db_path();
        let db = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        self.cipher.unlock(&db)?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "synchronous", "FULL")?;
        Ok(VaultConnection {
            db,
            _writer: writer,
        })
    }

    fn integrity(db: &Connection) -> Result<()> {
        let _: i64 = db.query_row("SELECT count(*) FROM vault_captures", [], |r| r.get(0))?;
        Ok(())
    }

    fn storage(&self) -> Result<ImmutableVaultStorage<'_>> {
        ImmutableVaultStorage::new(&self.root, &self.vault, &self.gate, self.lease)
            .map_err(|_| IndexError::InvalidEvidence)
    }

    fn identity(archive_id: &str, snapshot_id: &str) -> String {
        format!("capture/{archive_id}/{snapshot_id}")
    }

    pub fn save(&self, mut capture: Capture) -> Result<SaveReceipt> {
        self.gate
            .validate_lease(self.lease)
            .map_err(|_| IndexError::Revoked)?;
        validate_capture(&mut capture)?;
        if capture.provider == "generic" || capture.capture_method == "legacy-import" {
            return Err(IndexError::InvalidEvidence);
        }
        let id = format!(
            "{:x}",
            Sha256::digest(format!("{}:{}", capture.provider, capture.conversation_id).as_bytes())
        );
        // Observation timestamps do not mint a new immutable revision.
        let mut normalized = capture.clone();
        normalized.captured_at.clear();
        let snapshot_id = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&normalized).map_err(|_| IndexError::InvalidEvidence)?
            )
        );
        let identity = Self::identity(&id, &snapshot_id);
        let _writer =
            VaultWriterFence::writer(&self.root).map_err(|_| IndexError::InvalidEvidence)?;
        let mut db = self.connection()?;
        if db
            .query_row(
                "SELECT 1 FROM vault_captures WHERE archive_id=?1 AND snapshot_id=?2",
                params![id, snapshot_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .is_some()
        {
            return Ok(SaveReceipt {
                ok: true,
                id,
                snapshot_id,
                message_count: capture.messages.len(),
                unchanged: true,
                completeness: capture.completeness,
                warnings: capture.warnings,
            });
        }
        let payload = serde_json::to_vec(&capture).map_err(|_| IndexError::InvalidEvidence)?;
        if payload.len() > MAX_CAPTURE_BYTES {
            return Err(IndexError::InvalidEvidence);
        }
        self.storage()?
            .write_immutable(
                &self.gate,
                self.lease,
                ContentKind::Archive,
                identity.as_bytes(),
                &payload,
            )
            .map_err(|_| IndexError::InvalidEvidence)?;
        let tx = db.transaction()?;
        tx.execute(
            "INSERT INTO vault_captures(
                archive_id,snapshot_id,identity,provider,conversation_id,title,source_url,
                captured_at,message_count,completeness,warnings_json
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                id,
                snapshot_id,
                identity,
                capture.provider,
                capture.conversation_id,
                capture.title,
                capture.source_url,
                capture.captured_at,
                capture.messages.len() as i64,
                capture.completeness,
                serde_json::to_string(&capture.warnings)
                    .map_err(|_| IndexError::InvalidEvidence)?
            ],
        )?;
        self.gate
            .validate_lease(self.lease)
            .map_err(|_| IndexError::Revoked)?;
        tx.commit()?;
        Ok(SaveReceipt {
            ok: true,
            id,
            snapshot_id,
            message_count: capture.messages.len(),
            unchanged: false,
            completeness: capture.completeness,
            warnings: capture.warnings,
        })
    }

    pub fn read(&self, archive_id: &str, revision: Option<&str>) -> Result<Conversation> {
        let _writer =
            VaultWriterFence::writer(&self.root).map_err(|_| IndexError::InvalidEvidence)?;
        let db = self.connection()?;
        let mut statement = db.prepare(
            "SELECT snapshot_id,identity,provider,conversation_id,title,source_url,
                    captured_at,message_count,completeness,warnings_json
             FROM vault_captures
             WHERE archive_id=?1 AND (?2 IS NULL OR snapshot_id=?2)
             ORDER BY captured_at DESC LIMIT 1",
        )?;
        let row = statement
            .query_row(params![archive_id, revision], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, String>(8)?,
                    r.get::<_, String>(9)?,
                ))
            })
            .optional()?
            .ok_or(IndexError::InvalidEvidence)?;
        let snapshot_count: i64 = db.query_row(
            "SELECT count(*) FROM vault_captures WHERE archive_id=?1",
            [archive_id],
            |r| r.get(0),
        )?;
        let bytes = self
            .storage()?
            .read(
                &self.gate,
                self.lease,
                ContentKind::Archive,
                row.1.as_bytes(),
            )
            .map_err(|_| IndexError::InvalidEvidence)?;
        let mut capture: Capture =
            serde_json::from_slice(bytes.as_slice()).map_err(|_| IndexError::InvalidEvidence)?;
        validate_capture(&mut capture)?;
        if capture.provider != row.2
            || capture.conversation_id != row.3
            || capture.title != row.4
            || capture.source_url != row.5
            || capture.captured_at != row.6
            || capture.messages.len() as i64 != row.7
            || capture.completeness != row.8
            || Self::identity(archive_id, &row.0) != row.1
        {
            return Err(IndexError::InvalidEvidence);
        }
        self.gate
            .validate_lease(self.lease)
            .map_err(|_| IndexError::Revoked)?;
        let warnings: Vec<String> =
            serde_json::from_str(&row.9).map_err(|_| IndexError::InvalidEvidence)?;
        if capture.warnings != warnings {
            return Err(IndexError::InvalidEvidence);
        }
        let summary = ConversationSummary {
            id: archive_id.to_owned(),
            conversation_id: row.3,
            provider: row.2,
            title: row.4,
            source_url: row.5,
            conversation_date: capture.conversation_created_at.clone(),
            date_source: capture
                .conversation_created_at
                .as_ref()
                .map(|_| "provider".to_owned()),
            captured_at: row.6,
            message_count: capture.messages.len(),
            snapshot_count: snapshot_count as usize,
            completeness: capture.completeness.clone(),
            warnings,
        };
        Ok(Conversation {
            summary,
            capture,
            snapshot_id: row.0,
        })
    }

    /// A vault export stays in owner memory. The caller can explicitly copy or
    /// save it using the desktop UI; never silently create a plaintext file.
    /// Captures referencing media that was not stored in the vault cannot be
    /// represented by a complete self-contained Markdown export.
    pub fn export_markdown(
        &self,
        archive_id: &str,
        revision: Option<&str>,
    ) -> Result<ExportResult> {
        let conversation = self.read(archive_id, revision)?;
        if conversation.capture.messages.iter().any(|message| {
            message
                .attachments
                .iter()
                .any(|attachment| attachment.asset_path.is_some())
        }) {
            return Err(IndexError::InvalidEvidence);
        }
        let rendered = markdown(&conversation.capture);
        self.gate
            .validate_lease(self.lease)
            .map_err(|_| IndexError::Revoked)?;
        Ok(ExportResult {
            path: String::new(),
            markdown: rendered,
        })
    }

    pub fn list(&self, query: &str) -> Result<Vec<ConversationSummary>> {
        if query.len() > 1024 {
            return Err(IndexError::InvalidQuery);
        }
        // The catalog itself is SQLCipher encrypted. Never decrypt every
        // potentially 32 MiB raw capture merely to populate the sidebar.
        let _writer =
            VaultWriterFence::writer(&self.root).map_err(|_| IndexError::InvalidEvidence)?;
        let db = self.connection()?;
        let mut statement = db.prepare(
            "SELECT c.archive_id,c.conversation_id,c.provider,c.title,c.source_url,
                    c.captured_at,c.message_count,c.completeness,c.warnings_json,
                    (SELECT count(*) FROM vault_captures WHERE archive_id=c.archive_id)
             FROM vault_captures c
             WHERE c.rowid = (
               SELECT rowid FROM vault_captures x WHERE x.archive_id=c.archive_id
               ORDER BY x.captured_at DESC LIMIT 1
             )
             ORDER BY c.captured_at DESC LIMIT 10000",
        )?;
        let needle = query.to_lowercase();
        let rows = statement.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, String>(8)?,
                r.get::<_, i64>(9)?,
            ))
        })?;
        let mut results = Vec::new();
        for row in rows {
            let row = row?;
            if !needle.is_empty()
                && !row.3.to_lowercase().contains(&needle)
                && !row.2.contains(&needle)
                && !row.1.contains(&needle)
            {
                continue;
            }
            let warnings: Vec<String> =
                serde_json::from_str(&row.8).map_err(|_| IndexError::InvalidEvidence)?;
            results.push(ConversationSummary {
                id: row.0,
                conversation_id: row.1,
                provider: row.2,
                title: row.3,
                source_url: row.4,
                captured_at: row.5,
                message_count: row.6 as usize,
                completeness: row.7,
                warnings,
                snapshot_count: row.9 as usize,
                conversation_date: None,
                date_source: None,
            });
        }
        self.gate
            .validate_lease(self.lease)
            .map_err(|_| IndexError::Revoked)?;
        Ok(results)
    }

    /// Explicit owner-requested online portable backup. Never runs in the
    /// native capture host or a remote connector; the desktop owns the lease.
    pub fn online_backup(
        &self,
        destination: &Path,
        generation: u64,
    ) -> Result<mind_place_security::vault_backup::BackupMetadata> {
        crate::vault_online_backup::VaultOnlineBackup::create(
            &self.gate,
            self.lease,
            &self.vault,
            &self.root,
            destination,
            generation,
        )
    }

    pub fn snapshots(&self, archive_id: &str) -> Result<Vec<SnapshotSummary>> {
        let _writer =
            VaultWriterFence::writer(&self.root).map_err(|_| IndexError::InvalidEvidence)?;
        let db = self.connection()?;
        let mut statement = db.prepare(
            "SELECT snapshot_id,captured_at,message_count,completeness
             FROM vault_captures WHERE archive_id=?1 ORDER BY captured_at DESC",
        )?;
        let rows = statement.query_map([archive_id], |row| {
            Ok(SnapshotSummary {
                snapshot_id: row.get(0)?,
                captured_at: row.get(1)?,
                message_count: row.get::<_, i64>(2)? as usize,
                completeness: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

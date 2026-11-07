//! Owner-session encrypted capture writer/reader. No legacy raw archive files.
//! This adapter is independent from production routing until authenticated
//! native-host IPC and derived search indexing are fully ported and qualified.
use crate::{documents::DocumentIndex, IndexError, IndexStore, Result};
use mind_place_archive::{
    markdown, validate_capture, AssetResult, Capture, Conversation, ConversationSummary,
    ExportResult, SaveReceipt, SnapshotSummary, MAX_CAPTURE_BYTES,
};
use mind_place_security::{
    vault::{ContentKind, CryptoVault},
    vault_fence::VaultWriterFence,
    vault_storage::ImmutableVaultStorage,
    SecurityError, SecurityGate, UnlockLease,
};
use mind_place_state::{canonical_source_id, CipherKey, SourceDescriptor, SourceKey, StateStore};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use sha2::{Digest, Sha256};
use std::{
    fs,
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
    sync::Arc,
};
use zeroize::Zeroize;

const APPLICATION_ID: i64 = 0x4d505641; // MPVA (never legacy MPIX).
const SCHEMA_VERSION: i64 = 2;
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
CREATE TABLE vault_current_snapshots(
    archive_id TEXT NOT NULL PRIMARY KEY,
    snapshot_id TEXT NOT NULL
);
";

pub struct VaultCaptureStore {
    root: PathBuf,
    vault: Arc<CryptoVault>,
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
        let vault = Arc::new(vault);
        ImmutableVaultStorage::new(&root, &vault, gate, lease)
            .map_err(|_| IndexError::InvalidEvidence)?;
        let catalog_exists = root.join("archive-index.sqlite3").exists();
        if !catalog_exists && Self::archive_objects_present(&root)? {
            return Err(IndexError::InvalidIndex);
        }
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
            let mut db = store.connection()?;
            Self::ensure_schema(&mut db, &store.root)?;
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
        DocumentIndex::open_encrypted_with_objects(
            self.root.join("document-index.sqlite3"),
            self.derived_sqlcipher_key(b"document-index/sqlcipher/v1")?,
            &self.gate,
            self.lease,
            self.vault.clone(),
            &self.root,
        )
    }

    fn db_path(&self) -> PathBuf {
        self.root.join("archive-index.sqlite3")
    }

    pub(crate) fn archive_objects_present(root: &Path) -> Result<bool> {
        let archive = root.join("archive");
        let metadata = match fs::symlink_metadata(&archive) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(IndexError::InvalidEvidence);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(IndexError::InvalidEvidence);
            }
        }
        let mut pending = vec![archive];
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(directory)? {
                let path = entry?.path();
                let metadata = fs::symlink_metadata(&path)?;
                if metadata.file_type().is_symlink() {
                    return Err(IndexError::InvalidEvidence);
                }
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if metadata.file_attributes() & 0x400 != 0 {
                        return Err(IndexError::InvalidEvidence);
                    }
                }
                if metadata.is_file() {
                    return Ok(true);
                }
                if metadata.is_dir() {
                    pending.push(path);
                } else {
                    return Err(IndexError::InvalidEvidence);
                }
            }
        }
        Ok(false)
    }

    fn ensure_schema(db: &mut Connection, root: &Path) -> Result<()> {
        let current: i64 = db.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        let app: i64 = db.query_row("PRAGMA application_id", [], |row| row.get(0))?;
        if current == 0 && app == 0 {
            if Self::archive_objects_present(root)? {
                return Err(IndexError::InvalidIndex);
            }
            let tx = db.transaction()?;
            tx.execute_batch(SCHEMA)?;
            tx.pragma_update(None, "application_id", APPLICATION_ID)?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            tx.commit()?;
        } else if current == 1 && app == APPLICATION_ID {
            let tx = db.transaction()?;
            tx.execute_batch(
                "CREATE TABLE vault_current_snapshots(
                    archive_id TEXT NOT NULL PRIMARY KEY,
                    snapshot_id TEXT NOT NULL
                 );",
            )?;
            tx.execute(
                "INSERT INTO vault_current_snapshots(archive_id,snapshot_id)
                 SELECT c.archive_id,c.snapshot_id
                 FROM vault_captures c
                 WHERE c.rowid = (
                    SELECT rowid FROM vault_captures x
                    WHERE x.archive_id=c.archive_id
                    ORDER BY x.captured_at DESC,x.snapshot_id DESC LIMIT 1
                 )",
                [],
            )?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            tx.commit()?;
        } else if current != SCHEMA_VERSION || app != APPLICATION_ID {
            return Err(IndexError::InvalidIndex);
        }
        Ok(())
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
        let missing: i64 = db.query_row(
            "SELECT count(*) FROM (
                SELECT DISTINCT archive_id FROM vault_captures
             ) archives
             LEFT JOIN vault_current_snapshots current
               ON current.archive_id=archives.archive_id
             WHERE current.archive_id IS NULL",
            [],
            |row| row.get(0),
        )?;
        let orphaned: i64 = db.query_row(
            "SELECT count(*) FROM vault_current_snapshots current
             WHERE NOT EXISTS (
                SELECT 1 FROM vault_captures capture
                WHERE capture.archive_id=current.archive_id
                  AND capture.snapshot_id=current.snapshot_id
             )",
            [],
            |row| row.get(0),
        )?;
        if missing != 0 || orphaned != 0 {
            return Err(IndexError::InvalidIndex);
        }
        Ok(())
    }

    fn storage(&self) -> Result<ImmutableVaultStorage<'_>> {
        ImmutableVaultStorage::new(&self.root, &self.vault, &self.gate, self.lease)
            .map_err(|_| IndexError::InvalidEvidence)
    }

    fn identity(archive_id: &str, snapshot_id: &str) -> String {
        format!("capture/{archive_id}/{snapshot_id}")
    }

    /// Map an archive conversation back to its versioned state authority.
    /// Unindexed legacy import types have no provider canonical identity here;
    /// migration/unsupported-provider policy is a separate release gate.
    fn capture_redacted(
        &self,
        authority: &StateStore,
        provider: &str,
        conversation_id: &str,
    ) -> Result<bool> {
        // Unknown/legacy provider types must never bypass source authority.
        // Generic imports have their own explicit, canonical namespace.
        let namespace =
            crate::namespace_for_provider(provider).ok_or(IndexError::InvalidEvidence)?;
        let source_id = canonical_source_id(&SourceDescriptor {
            provider,
            source_namespace: namespace,
            source_kind: "conversation",
            key: SourceKey::External(conversation_id),
        })?;
        Ok(authority.source_is_redacted(&source_id)?)
    }

    fn require_visible(
        &self,
        authority: &StateStore,
        provider: &str,
        conversation_id: &str,
    ) -> Result<()> {
        if self.capture_redacted(authority, provider, conversation_id)? {
            return Err(IndexError::Revoked);
        }
        Ok(())
    }

    pub fn save(&self, capture: Capture) -> Result<SaveReceipt> {
        self.save_internal(capture, false)
    }

    /// Only an explicit owner-consented offline migration may carry legacy
    /// import provenance into the encrypted archive. Ordinary native capture
    /// remains unable to impersonate that source type or a generic import.
    pub fn import_legacy_consented(
        &self,
        _consent: &mind_place_security::vault_migration::ExplicitMigrationConsent,
        capture: Capture,
    ) -> Result<SaveReceipt> {
        self.save_internal(capture, true)
    }

    fn save_internal(&self, capture: Capture, migration: bool) -> Result<SaveReceipt> {
        self.save_internal_with_checkpoint(capture, migration, || {})
    }

    // Test checkpoint is invoked under the exact production privacy fence,
    // immediately after authority visibility and before ciphertext publication.
    fn save_internal_with_checkpoint(
        &self,
        mut capture: Capture,
        migration: bool,
        after_visibility_check: impl FnOnce(),
    ) -> Result<SaveReceipt> {
        self.gate
            .validate_lease(self.lease)
            .map_err(|_| IndexError::Revoked)?;
        validate_capture(&mut capture)?;
        if !migration
            && (capture.provider == "generic" || capture.capture_method == "legacy-import")
        {
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
        // Lock order: privacy exclusive -> nested shared writer leases.
        // The writer fence alone is SHARED and cannot serialize redaction.
        // Keep this lock across the visibility check, immutable publication,
        // and catalog commit; redaction takes the same exclusive privacy lock
        // BEFORE it opens a writable state connection.
        let _privacy =
            VaultWriterFence::privacy(&self.root).map_err(|_| IndexError::InvalidEvidence)?;
        let authority = self.open_state_store()?;
        let _writer =
            VaultWriterFence::writer(&self.root).map_err(|_| IndexError::InvalidEvidence)?;
        // Ingestion itself establishes canonical authority, including a generic
        // migration source. Otherwise a captured conversation could never be
        // redacted if another subsystem had not registered it beforehand.
        let namespace =
            crate::namespace_for_provider(&capture.provider).ok_or(IndexError::InvalidEvidence)?;
        authority.register_source(&SourceDescriptor {
            provider: &capture.provider,
            source_namespace: namespace,
            source_kind: "conversation",
            key: SourceKey::External(&capture.conversation_id),
        })?;
        self.require_visible(&authority, &capture.provider, &capture.conversation_id)?;
        after_visibility_check();
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
            if !migration {
                let tx = db.transaction()?;
                tx.execute(
                    "INSERT INTO vault_current_snapshots(archive_id,snapshot_id)
                     VALUES(?1,?2)
                     ON CONFLICT(archive_id) DO UPDATE SET snapshot_id=excluded.snapshot_id",
                    params![id, snapshot_id],
                )?;
                self.gate
                    .validate_lease(self.lease)
                    .map_err(|_| IndexError::Revoked)?;
                tx.commit()?;
            }
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
        let storage = self.storage()?;
        let previous_current_identity = if migration {
            db.query_row(
                "SELECT capture.identity
                 FROM vault_current_snapshots current
                 JOIN vault_captures capture
                   ON capture.archive_id=current.archive_id
                  AND capture.snapshot_id=current.snapshot_id
                 WHERE current.archive_id=?1",
                [&id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        } else {
            None
        };
        if let Err(error) = storage.write_immutable(
            &self.gate,
            self.lease,
            ContentKind::Archive,
            identity.as_bytes(),
            &payload,
        ) {
            let collision = match &error {
                SecurityError::Crypto => true,
                SecurityError::Io(error) => error.kind() == std::io::ErrorKind::AlreadyExists,
                _ => false,
            };
            if !collision {
                return Err(IndexError::InvalidEvidence);
            }
            // A crash can publish immutable evidence before its catalog commit.
            // Adopt only authenticated, identical normalized content; retain the
            // original observation timestamp and never replace the raw object.
            let previous = storage
                .read(
                    &self.gate,
                    self.lease,
                    ContentKind::Archive,
                    identity.as_bytes(),
                )
                .map_err(|_| IndexError::InvalidEvidence)?;
            let mut original: Capture =
                serde_json::from_slice(&previous).map_err(|_| IndexError::InvalidEvidence)?;
            validate_capture(&mut original)?;
            let mut original_normalized = original.clone();
            original_normalized.captured_at.clear();
            if serde_json::to_vec(&original_normalized).map_err(|_| IndexError::InvalidEvidence)?
                != serde_json::to_vec(&normalized).map_err(|_| IndexError::InvalidEvidence)?
            {
                return Err(IndexError::InvalidEvidence);
            }
            capture = original;
        }
        // Evaluate the legacy selection policy against the exact immutable
        // object adopted above, not a retry payload with a different timestamp.
        let preserve_current = if let Some(previous_identity) = previous_current_identity {
            let previous = storage
                .read(
                    &self.gate,
                    self.lease,
                    ContentKind::Archive,
                    previous_identity.as_bytes(),
                )
                .map_err(|_| IndexError::InvalidEvidence)?;
            let mut prior: Capture =
                serde_json::from_slice(&previous).map_err(|_| IndexError::InvalidEvidence)?;
            validate_capture(&mut prior)?;
            let prior_time = chrono::DateTime::parse_from_rfc3339(&prior.captured_at)
                .map_err(|_| IndexError::InvalidEvidence)?;
            let incoming_time = chrono::DateTime::parse_from_rfc3339(&capture.captured_at)
                .map_err(|_| IndexError::InvalidEvidence)?;
            matches!(
                prior.capture_method.as_str(),
                "manual-dom" | "automatic-dom"
            ) || prior_time >= incoming_time
        } else {
            false
        };
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
        if !preserve_current {
            tx.execute(
                "INSERT INTO vault_current_snapshots(archive_id,snapshot_id)
                 VALUES(?1,?2)
                 ON CONFLICT(archive_id) DO UPDATE SET snapshot_id=excluded.snapshot_id",
                params![id, snapshot_id],
            )?;
        }
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
        let authority = self.open_state_store()?;
        let _writer =
            VaultWriterFence::writer(&self.root).map_err(|_| IndexError::InvalidEvidence)?;
        let db = self.connection()?;
        let mut statement = db.prepare(
            "SELECT snapshot_id,identity,provider,conversation_id,title,source_url,
                    captured_at,message_count,completeness,warnings_json
             FROM vault_captures
             WHERE archive_id=?1
               AND (
                 (?2 IS NOT NULL AND snapshot_id=?2)
                 OR (?2 IS NULL AND snapshot_id=(
                    SELECT snapshot_id FROM vault_current_snapshots WHERE archive_id=?1
                 ))
               )
             LIMIT 1",
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
        // Direct archive-id + revision fetch cannot bypass logical redaction.
        self.require_visible(&authority, &row.2, &row.3)?;
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

    pub(crate) fn select_current_snapshot(
        &self,
        archive_id: &str,
        snapshot_id: &str,
    ) -> Result<()> {
        let valid = |value: &str| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        };
        if !valid(archive_id) || !valid(snapshot_id) {
            return Err(IndexError::InvalidEvidence);
        }
        let _writer =
            VaultWriterFence::writer(&self.root).map_err(|_| IndexError::InvalidEvidence)?;
        let mut db = self.connection()?;
        let exists: Option<i64> = db
            .query_row(
                "SELECT 1 FROM vault_captures WHERE archive_id=?1 AND snapshot_id=?2",
                params![archive_id, snapshot_id],
                |row| row.get(0),
            )
            .optional()?;
        if exists.is_none() {
            return Err(IndexError::InvalidEvidence);
        }
        let tx = db.transaction()?;
        tx.execute(
            "INSERT INTO vault_current_snapshots(archive_id,snapshot_id)
             VALUES(?1,?2)
             ON CONFLICT(archive_id) DO UPDATE SET snapshot_id=excluded.snapshot_id",
            params![archive_id, snapshot_id],
        )?;
        self.gate
            .validate_lease(self.lease)
            .map_err(|_| IndexError::Revoked)?;
        tx.commit()?;
        Ok(())
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
        let authority = self.open_state_store()?;
        let conversation = self.read(archive_id, revision)?;
        let _writer =
            VaultWriterFence::writer(&self.root).map_err(|_| IndexError::InvalidEvidence)?;
        self.require_visible(
            &authority,
            &conversation.capture.provider,
            &conversation.capture.conversation_id,
        )?;
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

    pub fn asset(&self, archive_id: &str, name: &str) -> Result<AssetResult> {
        self.gate
            .validate_lease(self.lease)
            .map_err(|_| IndexError::Revoked)?;
        let valid_hash = |value: &str| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        };
        let (hash, extension) = name.split_once('.').ok_or(IndexError::InvalidEvidence)?;
        if !valid_hash(archive_id)
            || !valid_hash(hash)
            || !matches!(extension, "png" | "jpg" | "gif" | "webp")
        {
            return Err(IndexError::InvalidEvidence);
        }
        let authority = self.open_state_store()?;
        let conversation = self.read(archive_id, None)?;
        let _writer =
            VaultWriterFence::writer(&self.root).map_err(|_| IndexError::InvalidEvidence)?;
        self.require_visible(
            &authority,
            &conversation.capture.provider,
            &conversation.capture.conversation_id,
        )?;
        let storage = self.storage()?;
        let inventory = crate::vault_legacy_objects::RawLegacyInventory::read(
            &self.root,
            &self.vault,
            &storage,
            &self.gate,
            self.lease,
        )
        .map_err(|_| IndexError::InvalidEvidence)?
        .ok_or(IndexError::InvalidEvidence)?;
        let relative = format!("archive-v1/conversations/{archive_id}/assets/{name}");
        let bytes = inventory
            .read_file(&relative, &storage, &self.gate, self.lease)
            .map_err(|_| IndexError::InvalidEvidence)?;
        let image = mind_place_archive::verified_image_asset(name, &bytes)?;
        self.gate
            .validate_lease(self.lease)
            .map_err(|_| IndexError::Revoked)?;
        Ok(image)
    }

    /// Sidebar search remains capped; the FTS reindexer instead requests a
    /// complete cursor-paged inventory from one stable SQLCipher read snapshot.
    pub fn list(&self, query: &str) -> Result<Vec<ConversationSummary>> {
        self.list_catalog(query, false)
    }

    pub(crate) fn list_complete_for_index(&self) -> Result<Vec<ConversationSummary>> {
        self.list_catalog("", true)
    }

    fn list_catalog(&self, query: &str, complete: bool) -> Result<Vec<ConversationSummary>> {
        if query.len() > 1024 {
            return Err(IndexError::InvalidQuery);
        }
        let authority = self.open_state_store()?;
        let _writer =
            VaultWriterFence::writer(&self.root).map_err(|_| IndexError::InvalidEvidence)?;
        let mut db = self.connection()?;
        // One transaction prevents page drift while a concurrent capture appends
        // or updates a conversation between keyset pages.
        let tx = db.transaction()?;
        let result = Self::catalog_rows(&tx, query, complete)?;
        // Hide even historical catalogue titles and metadata for a redacted
        // source; the complete reindex inventory drops these derived rows.
        let mut filtered = Vec::with_capacity(result.len());
        for summary in result {
            if !self.capture_redacted(&authority, &summary.provider, &summary.conversation_id)? {
                filtered.push(summary);
            }
        }
        self.gate
            .validate_lease(self.lease)
            .map_err(|_| IndexError::Revoked)?;
        tx.commit()?;
        Ok(filtered)
    }

    fn catalog_rows(
        db: &Connection,
        query: &str,
        complete: bool,
    ) -> Result<Vec<ConversationSummary>> {
        const PAGE_SIZE: i64 = 512;
        const SIDEBAR_LIMIT: i64 = 10_000;
        let page_size = if complete { PAGE_SIZE } else { SIDEBAR_LIMIT };
        let needle = query.to_lowercase();
        let mut cursor: Option<(String, String)> = None;
        let mut results = Vec::new();
        loop {
            let mut statement = db.prepare(
                "SELECT c.archive_id,c.conversation_id,c.provider,c.title,c.source_url,
                        c.captured_at,c.message_count,c.completeness,c.warnings_json,
                        (SELECT count(*) FROM vault_captures WHERE archive_id=c.archive_id)
                 FROM vault_captures c
                 JOIN vault_current_snapshots current
                   ON current.archive_id=c.archive_id
                  AND current.snapshot_id=c.snapshot_id
                 WHERE (?1 IS NULL OR c.captured_at < ?1
                      OR (c.captured_at = ?1 AND c.archive_id < ?2))
                 ORDER BY c.captured_at DESC,c.archive_id DESC LIMIT ?3",
            )?;
            let rows = statement.query_map(
                params![
                    cursor.as_ref().map(|pair| pair.0.as_str()),
                    cursor.as_ref().map(|pair| pair.1.as_str()),
                    page_size
                ],
                |r| {
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
                },
            )?;
            let mut scanned = 0i64;
            for row in rows {
                let row = row?;
                scanned += 1;
                cursor = Some((row.5.clone(), row.0.clone()));
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
            if !complete || scanned < page_size {
                break;
            }
        }
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

    /// Backup/restore validation must walk every immutable revision, including
    /// non-current captures, without recreating or trusting a missing catalog.
    pub(crate) fn verify_backup_catalog(
        root: &Path,
        vault: &CryptoVault,
        gate: &SecurityGate,
        lease: UnlockLease,
    ) -> Result<()> {
        let path = root.join("archive-index.sqlite3");
        if !path.exists() {
            return if Self::archive_objects_present(root)? {
                Err(IndexError::InvalidEvidence)
            } else {
                Ok(())
            };
        }
        let derived = vault
            .derived_key(gate, lease, b"archive-index/sqlcipher/v1")
            .map_err(|_| IndexError::Revoked)?;
        let mut raw = [0u8; 32];
        raw.copy_from_slice(derived.as_ref());
        let key = CipherKey::new(raw, gate, lease);
        raw.zeroize();
        // This is an unpublished snapshot. A read-only WAL reader leaves
        // empty WAL/SHM sidecars behind; a writable pager closes/checkpoints
        // them before the snapshot's quiescent packaging/publication step.
        let mut db = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        key.unlock(&db)?;
        Self::ensure_schema(&mut db, root).map_err(|_| IndexError::InvalidEvidence)?;
        Self::integrity(&db).map_err(|_| IndexError::InvalidEvidence)?;
        let storage = ImmutableVaultStorage::new(root, vault, gate, lease)
            .map_err(|_| IndexError::InvalidEvidence)?;
        let mut statement = db.prepare("SELECT archive_id,snapshot_id,identity,provider,conversation_id,title,source_url,captured_at,message_count,completeness,warnings_json FROM vault_captures ORDER BY archive_id,snapshot_id")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            gate.validate_lease(lease)
                .map_err(|_| IndexError::Revoked)?;
            let id: String = row.get(0)?;
            let snapshot: String = row.get(1)?;
            let identity: String = row.get(2)?;
            if identity != Self::identity(&id, &snapshot) {
                return Err(IndexError::InvalidEvidence);
            }
            let bytes = storage
                .read(gate, lease, ContentKind::Archive, identity.as_bytes())
                .map_err(|_| IndexError::InvalidEvidence)?;
            let mut capture: Capture =
                serde_json::from_slice(&bytes).map_err(|_| IndexError::InvalidEvidence)?;
            validate_capture(&mut capture)?;
            let expected_id = format!(
                "{:x}",
                Sha256::digest(
                    format!("{}:{}", capture.provider, capture.conversation_id).as_bytes()
                )
            );
            let mut normalized = capture.clone();
            normalized.captured_at.clear();
            let expected_snapshot = format!(
                "{:x}",
                Sha256::digest(
                    serde_json::to_vec(&normalized).map_err(|_| IndexError::InvalidEvidence)?
                )
            );
            let warnings: Vec<String> = serde_json::from_str(&row.get::<_, String>(10)?)
                .map_err(|_| IndexError::InvalidEvidence)?;
            if id != expected_id
                || snapshot != expected_snapshot
                || capture.provider != row.get::<_, String>(3)?
                || capture.conversation_id != row.get::<_, String>(4)?
                || capture.title != row.get::<_, String>(5)?
                || capture.source_url != row.get::<_, String>(6)?
                || capture.captured_at != row.get::<_, String>(7)?
                || capture.messages.len() as i64 != row.get::<_, i64>(8)?
                || capture.completeness != row.get::<_, String>(9)?
                || capture.warnings != warnings
            {
                return Err(IndexError::InvalidEvidence);
            }
        }
        Ok(())
    }

    pub fn snapshots(&self, archive_id: &str) -> Result<Vec<SnapshotSummary>> {
        let authority = self.open_state_store()?;
        let _writer =
            VaultWriterFence::writer(&self.root).map_err(|_| IndexError::InvalidEvidence)?;
        let db = self.connection()?;
        let identity: Option<(String, String)> = db
            .query_row(
                "SELECT provider,conversation_id FROM vault_captures
             WHERE archive_id=?1 LIMIT 1",
                [archive_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((provider, conversation_id)) = identity {
            self.require_visible(&authority, &provider, &conversation_id)?;
        }
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

#[cfg(test)]
mod vault_catalog_pagination_tests {
    use super::*;
    #[test]
    fn full_inventory_exceeds_sidebar_limit_without_dropping_old_fts_inputs() {
        let mut db = Connection::open_in_memory().expect("synthetic in-memory catalog");
        db.execute_batch(SCHEMA).expect("create catalog schema");
        let tx = db.transaction().expect("begin");
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO vault_captures(archive_id,snapshot_id,identity,provider,
                  conversation_id,title,source_url,captured_at,message_count,completeness,
                  warnings_json) VALUES (?1,?2,?3,'chatgpt',?4,'synthetic','',
                  ?5,1,'complete','[]')",
                )
                .expect("insert");
            for i in 0..10_021usize {
                let id = format!("synthetic-{i:05}");
                let order = format!("2026-01-01T{i:05}");
                stmt.execute(params![
                    id,
                    "snapshot-0",
                    format!("identity-{i:05}"),
                    format!("conversation-{i:05}"),
                    order
                ])
                .expect("insert synthetic row");
            }
        }
        tx.execute(
            "INSERT INTO vault_current_snapshots(archive_id,snapshot_id)
             SELECT archive_id,snapshot_id FROM vault_captures",
            [],
        )
        .expect("populate current pointers");
        tx.commit().expect("commit");
        let sidebar = VaultCaptureStore::catalog_rows(&db, "", false).expect("sidebar");
        assert_eq!(sidebar.len(), 10_000);
        let full = VaultCaptureStore::catalog_rows(&db, "", true).expect("FTS inventory");
        assert_eq!(full.len(), 10_021);
        assert_eq!(full.first().unwrap().id, "synthetic-10020");
        assert_eq!(full.last().unwrap().id, "synthetic-00000");
        let unique: std::collections::HashSet<_> = full.iter().map(|item| &item.id).collect();
        assert_eq!(unique.len(), full.len());
    }
    #[test]
    fn schema_v1_upgrade_backfills_the_latest_current_revision() {
        let home = tempfile::tempdir().unwrap();
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE vault_captures(
                archive_id TEXT NOT NULL,
                snapshot_id TEXT NOT NULL,
                captured_at TEXT NOT NULL
             );
             INSERT INTO vault_captures VALUES
                ('synthetic-archive','older','2026-10-28T08:00:00Z'),
                ('synthetic-archive','newer','2026-10-28T09:00:00Z');",
        )
        .unwrap();
        db.pragma_update(None, "application_id", APPLICATION_ID)
            .unwrap();
        db.pragma_update(None, "user_version", 1).unwrap();

        VaultCaptureStore::ensure_schema(&mut db, home.path()).unwrap();

        let version: i64 = db
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        let current: String = db
            .query_row(
                "SELECT snapshot_id FROM vault_current_snapshots WHERE archive_id=?1",
                ["synthetic-archive"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        assert_eq!(current, "newer");
    }

    #[cfg(feature = "sqlcipher-preview")]
    #[test]
    fn lost_catalog_with_raw_inventory_fails_closed_for_open_and_backup() {
        use crate::vault_legacy_objects::RawLegacyInventory;
        use mind_place_security::vault::CryptoVault;

        let home = tempfile::tempdir().unwrap();
        let gate = SecurityGate::open(home.path().join("security.json")).unwrap();
        gate.initialize(b"synthetic-lost-catalog-raw-map").unwrap();
        let lease = gate.require_unlocked().unwrap();
        let (vault, recovery, manifest) = CryptoVault::provision(&gate, lease).unwrap();
        let root = home.path().join("vault");
        manifest.persist_new(&root).unwrap();
        let store = VaultCaptureStore::open(&root, vault, &gate, lease).unwrap();

        let vault = CryptoVault::restore_with_recovery(&gate, lease, &manifest, &recovery).unwrap();
        let storage = ImmutableVaultStorage::new(&root, &vault, &gate, lease).unwrap();
        RawLegacyInventory {
            version: 1,
            files: Vec::new(),
        }
        .persist(&storage, &gate, lease)
        .unwrap();
        drop(storage);
        drop(store);
        fs::remove_file(root.join("archive-index.sqlite3")).unwrap();

        let vault = CryptoVault::restore_with_recovery(&gate, lease, &manifest, &recovery).unwrap();
        assert!(VaultCaptureStore::open(&root, vault, &gate, lease).is_err());
        let vault = CryptoVault::restore_with_recovery(&gate, lease, &manifest, &recovery).unwrap();
        assert!(VaultCaptureStore::verify_backup_catalog(&root, &vault, &gate, lease).is_err());
        assert!(!root.join("archive-index.sqlite3").exists());
    }
}

#[cfg(test)]
mod privacy_capture_race_tests {
    use super::*;
    use mind_place_security::vault::CryptoVault;
    use mind_place_state::ClientCapability;
    use std::{
        sync::{mpsc, Arc},
        thread,
        time::Duration,
    };

    fn synthetic_capture(title: &str) -> Capture {
        serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "provider": "chatgpt",
            "conversation_id": "123e4567-e89b-12d3-a456-426614174099",
            "source_url": "https://chatgpt.com/c/123e4567-e89b-12d3-a456-426614174099",
            "title": title,
            "captured_at": "2026-11-07T10:00:00Z",
            "capture_method": "manual-dom",
            "completeness": "partial",
            "warnings": [],
            "messages": [{
                "id": "synthetic-message",
                "role": "user",
                "text": "synthetic privacy fence evidence",
                "format": "plain",
                "attachments": []
            }]
        }))
        .unwrap()
    }

    fn fixture() -> (
        tempfile::TempDir,
        SecurityGate,
        UnlockLease,
        mind_place_security::vault::VaultManifest,
        mind_place_security::vault::RecoverySecret,
        Arc<VaultCaptureStore>,
        String,
        String,
    ) {
        let home = tempfile::tempdir().unwrap();
        let gate = SecurityGate::open(home.path().join("security.json")).unwrap();
        gate.initialize(b"synthetic-redaction-race-password")
            .unwrap();
        let lease = gate.require_unlocked().unwrap();
        let (vault, recovery, manifest) = CryptoVault::provision(&gate, lease).unwrap();
        let root = home.path().join("vault");
        manifest.persist_new(&root).unwrap();
        let archive = Arc::new(VaultCaptureStore::open(&root, vault, &gate, lease).unwrap());
        let state = archive.open_state_store().unwrap();
        let admin = "a".repeat(64);
        state
            .issue_client(
                "client:v1:owner",
                "Synthetic owner",
                &admin,
                &[ClientCapability::Admin],
            )
            .unwrap();
        let source = state
            .register_source(&SourceDescriptor {
                provider: "chatgpt",
                source_namespace: crate::namespace_for_provider("chatgpt").unwrap(),
                source_kind: "conversation",
                key: SourceKey::External("123e4567-e89b-12d3-a456-426614174099"),
            })
            .unwrap();
        (
            home, gate, lease, manifest, recovery, archive, admin, source,
        )
    }

    #[test]
    fn redaction_waits_for_checked_capture_publication_then_denies_later_writes() {
        let (_home, _gate, _lease, _manifest, _recovery, archive, admin, source) = fixture();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (resume_tx, resume_rx) = mpsc::channel();
        let producer = Arc::clone(&archive);
        let capturing = thread::spawn(move || {
            producer.save_internal_with_checkpoint(synthetic_capture("first"), false, || {
                entered_tx.send(()).unwrap();
                resume_rx.recv_timeout(Duration::from_secs(30)).unwrap();
            })
        });
        entered_rx.recv_timeout(Duration::from_secs(30)).unwrap();

        let state = archive.open_state_store().unwrap();
        let (began_tx, began_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let redactor = thread::spawn(move || {
            began_tx.send(()).unwrap();
            done_tx
                .send(state.redact_source(&admin, &source, "Synthetic owner redaction"))
                .unwrap();
        });
        began_rx.recv_timeout(Duration::from_secs(30)).unwrap();
        // The mutation cannot commit while capture is between its visibility
        // check and immutable publication. The old shared-only fence failed here.
        assert!(matches!(
            done_rx.recv_timeout(Duration::from_millis(150)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        resume_tx.send(()).unwrap();
        let saved = capturing.join().unwrap().unwrap();
        redactor.join().unwrap();
        assert!(done_rx
            .recv_timeout(Duration::from_secs(30))
            .unwrap()
            .is_ok());

        assert!(archive.read(&saved.id, Some(&saved.snapshot_id)).is_err());
        assert!(archive.save(synthetic_capture("new revision")).is_err());
        let state = archive.open_state_store().unwrap();
        let source_id = state.register_source(&SourceDescriptor {
            provider: "chatgpt",
            source_namespace: crate::namespace_for_provider("chatgpt").unwrap(),
            source_kind: "conversation",
            key: SourceKey::External("123e4567-e89b-12d3-a456-426614174099"),
        });
        assert!(
            source_id.is_err(),
            "redacted source cannot re-enter ingestion"
        );
    }

    #[test]
    fn raw_capture_and_consented_generic_import_register_redactable_authority() {
        let (_home, _gate, _lease, _manifest, _recovery, archive, admin, _) = fixture();
        let raw_id = "123e4567-e89b-12d3-a456-426614174098";
        let mut raw = synthetic_capture("unregistered browser source");
        raw.conversation_id = raw_id.to_owned();
        raw.source_url = format!("https://chatgpt.com/c/{raw_id}");
        let saved = archive.save(raw.clone()).unwrap();
        let state = archive.open_state_store().unwrap();
        let raw_source = canonical_source_id(&SourceDescriptor {
            provider: "chatgpt",
            source_namespace: crate::namespace_for_provider("chatgpt").unwrap(),
            source_kind: "conversation",
            key: SourceKey::External(raw_id),
        })
        .unwrap();
        assert!(state.source(&raw_source).unwrap().is_some());
        state
            .redact_source(&admin, &raw_source, "Synthetic capture")
            .unwrap();
        assert!(archive.read(&saved.id, None).is_err());
        assert!(archive.save(raw).is_err());

        let generic_key = "f".repeat(64);
        let mut generic = synthetic_capture("consented generic import");
        generic.provider = "generic".to_owned();
        generic.conversation_id = generic_key.clone();
        generic.source_url = format!("mindplace-import://generic/{generic_key}");
        generic.capture_method = "generic-import".to_owned();
        let imported = archive.save_internal(generic.clone(), true).unwrap();
        let generic_source = canonical_source_id(&SourceDescriptor {
            provider: "generic",
            source_namespace: crate::namespace_for_provider("generic").unwrap(),
            source_kind: "conversation",
            key: SourceKey::External(&generic_key),
        })
        .unwrap();
        assert!(state.source(&generic_source).unwrap().is_some());
        state
            .redact_source(&admin, &generic_source, "Synthetic generic import")
            .unwrap();
        assert!(archive.read(&imported.id, None).is_err());
        assert!(archive.save_internal(generic, true).is_err());
    }

    #[test]
    fn interrupted_capture_releases_privacy_lock_and_restart_preserves_denial() {
        let (home, gate, lease, manifest, recovery, archive, admin, source) = fixture();
        let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            archive
                .save_internal_with_checkpoint(synthetic_capture("interrupted"), false, || {
                    panic!("synthetic interruption before immutable publication")
                })
                .unwrap();
        }));
        assert!(interrupted.is_err());
        let authority = archive.open_state_store().unwrap();
        authority
            .redact_source(&admin, &source, "After interrupted capture")
            .unwrap();
        assert!(archive
            .save(synthetic_capture("after interruption"))
            .is_err());
        drop(archive);

        let vault = CryptoVault::restore_with_recovery(&gate, lease, &manifest, &recovery).unwrap();
        let reopened =
            VaultCaptureStore::open(home.path().join("vault"), vault, &gate, lease).unwrap();
        assert!(reopened
            .open_state_store()
            .unwrap()
            .source_is_redacted(&source)
            .unwrap());
        assert!(reopened.save(synthetic_capture("after restart")).is_err());
        assert!(!home.path().join("vault").join("archive").exists());
    }
}

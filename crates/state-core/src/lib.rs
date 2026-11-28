use fs2::FileExt;
use mind_place_security::vault_fence::VaultWriterFence;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use sha2::{Digest, Sha256};
#[cfg(any(unix, feature = "sqlcipher-preview"))]
use std::fs::File;
use std::{
    fs::{self, OpenOptions},
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
mod cipher;
mod client_read_grants;
mod clients;
mod curated;
mod curated_export;
mod document_objects;
mod document_roots;
mod document_scanner;
mod identity;
mod memory;
mod memory_inspection;
mod memory_ranking;
mod memory_writes;
mod model_writes;
mod ntfs;
mod private_scopes;
mod source_forget;
mod source_redaction;
mod topics;
#[cfg(feature = "sqlcipher-preview")]
pub mod vault_bootstrap;
pub use client_read_grants::{ClientReadScope, MAX_READ_GRANTS_PER_CLIENT};
pub use clients::{ClientAuditEvent, ClientCapability, ClientRecord};
pub use curated::{
    curated_edge_id, curated_node_id_for_key, AttrValue, CnfRecord, CuratedEdge, CuratedEdgeRel,
    CuratedEdgeStatus, CuratedIntegrityReport, CuratedKey, CuratedNode, CuratedNodeKind,
    CuratedNodeRevision, CuratedNodeStatus, CuratedRebuildReceipt, CurationGeneration,
    CurationProfile, EvidenceSpan, GenerationStatus, NewCurationGeneration,
    PublishedGenerationReceipt, RecordedCuratedOp, MAX_CNF_RECORD_BYTES, MAX_EVIDENCE_QUOTE_BYTES,
    MAX_EVIDENCE_SPANS_PER_RECORD, MAX_NODE_ALIASES, MAX_NODE_ATTRS, MAX_NODE_BODY_BYTES,
    MAX_NODE_SUMMARY_CHARS, MAX_NODE_TITLE_CHARS, MAX_RECORDS_PER_APPEND, MAX_RECORDS_PER_SHARD,
    MAX_SHARDS_PER_GENERATION,
};
pub use curated_export::TopicMarkdownExport;
pub use document_objects::{
    DocumentMetadata, DocumentObjectStore, DocumentPreserveReceipt, DocumentVersion,
    MAX_DOCUMENT_OBJECT_BYTES,
};
pub use document_roots::{DocumentRoot, VirtualDocumentPath, VIRTUAL_ROOT_PREFIX};
pub use document_scanner::{DocumentScanner, ScanPolicy, ScanReport, WatchEvent};
pub use identity::{canonical_source_id, SourceDescriptor, SourceKey};
pub use memory::{ClaimEvidence, ClaimRevision, ClaimState, ClaimTransition};
pub use memory_inspection::{MemoryInspectRelation, MemoryInspectionDetail};
pub use memory_ranking::{
    serialize_ranked_context, ClaimRelation, ClaimRelationResolution, RankedClaim, RankingMode,
    RankingPolicy, RelationKind,
};
pub use memory_writes::{MemoryWrite, MemoryWriteReceipt};
pub use model_writes::{
    quarantine_scope_id, CredentialTier, ModelClaimProposal, QUARANTINE_SCOPE_PREFIX,
};
pub use private_scopes::PrivateScope;
pub use source_forget::{SourceForgetPreview, SourceForgetProgress};
pub use source_redaction::SourceRedactionReceipt;
pub use topics::{
    valid_cnf_slug, AssertionSource, MembershipStatus, NewTopic, NewTopicMembership, Topic,
    TopicMemberKind, TopicMemberRole, TopicMembership, TopicWorkStatus,
};

pub const APPLICATION_ID: i64 = 0x4D504C43;
pub const SCHEMA_VERSION: i64 = 10;
const MIN_SQLITE_NUMBER: i32 = 3_053_002;
const MIGRATION_V1: &str = include_str!("schema_v1.sql");
const MIGRATION_V2: &str = include_str!("schema_v2.sql");
const MIGRATION_V3: &str = include_str!("schema_v3.sql");
const MIGRATION_V4: &str = include_str!("schema_v4.sql");
const MIGRATION_V5: &str = include_str!("schema_v5.sql");
const MIGRATION_V6: &str = include_str!("schema_v6.sql");
const MIGRATION_V7: &str = include_str!("schema_v7.sql");
const MIGRATION_V8: &str = include_str!("schema_v8.sql");
const MIGRATION_V9: &str = include_str!("schema_v9.sql");
const MIGRATION_V10: &str = include_str!("schema_v10.sql");
pub use cipher::CipherKey;
pub type Result<T> = std::result::Result<T, StateError>;

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("document access denied")]
    DocumentDenied,
    #[error("memory mutation denied")]
    MemoryDenied,
    #[error("document object integrity failure")]
    ObjectCorrupt,
    #[error("document resource limit exceeded")]
    DocumentLimit,
    #[error("document changed during reading")]
    DocumentChanged,
    #[error("state I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("state SQLite: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("unsafe state path")]
    UnsafePath,
    #[error("refusing to adopt foreign SQLite file")]
    ForeignDatabase,
    #[error("unsupported future state schema {0}")]
    FutureSchema(i64),
    #[error("invalid authoritative state or migration checksum")]
    Corrupt,
    #[error("another state migration is running")]
    Busy,
    #[error("bundled SQLite version is below the supported floor")]
    SqliteTooOld,
    #[error("invalid durable identifier or value")]
    InvalidValue,
    #[error("source redaction requires the encrypted vault")]
    VaultRequired,
    #[error("curated revision conflict")]
    RevisionConflict,
    #[error("curated record limit exceeded")]
    CuratedLimit,
    #[error("encrypted authoritative state schema v{current} requires explicit backed-up migration to v{target}")]
    MigrationRequired { current: i64, target: i64 },
}
impl StateError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::DocumentDenied => "MP_DOCUMENT_DENIED",
            Self::MemoryDenied => "MP_MEMORY_DENIED",
            Self::ObjectCorrupt => "MP_DOCUMENT_OBJECT_CORRUPT",
            Self::DocumentLimit => "MP_DOCUMENT_LIMIT",
            Self::DocumentChanged => "MP_DOCUMENT_CHANGED",
            Self::Io(_) => "MP_STATE_IO",
            Self::Sql(_) => "MP_STATE_SQL",
            Self::UnsafePath => "MP_STATE_UNSAFE_PATH",
            Self::ForeignDatabase => "MP_STATE_FOREIGN_DATABASE",
            Self::FutureSchema(_) => "MP_STATE_FUTURE_SCHEMA",
            Self::Corrupt => "MP_STATE_CORRUPT",
            Self::Busy => "MP_STATE_BUSY",
            Self::SqliteTooOld => "MP_STATE_SQLITE_TOO_OLD",
            Self::InvalidValue => "MP_STATE_INVALID_VALUE",
            Self::VaultRequired => "MP_PRIVACY_REQUIRES_VAULT",
            Self::RevisionConflict => "MP_CURATED_REVISION_CONFLICT",
            Self::CuratedLimit => "MP_CURATED_LIMIT",
            Self::MigrationRequired { .. } => "MP_STATE_MIGRATION_REQUIRED",
        }
    }
}
#[derive(Clone, Debug)]
pub struct StateStore {
    path: PathBuf,
    cipher_key: Option<std::sync::Arc<CipherKey>>,
}

/// Holds the encrypted vault's shared writer lease for the entire SQLite
/// operation/transaction. No extra file lock on legacy plaintext state.
struct FencedStateConnection {
    db: Connection,
    _writer: Option<VaultWriterFence>,
}
impl Deref for FencedStateConnection {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        &self.db
    }
}
impl DerefMut for FencedStateConnection {
    fn deref_mut(&mut self) -> &mut Connection {
        &mut self.db
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    pub id: String,
    pub name: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct MemoryClaim {
    pub id: String,
    pub statement: String,
    pub asserted_by: String,
    pub epistemic_origin: String,
    pub status: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceRoot {
    pub id: String,
    pub canonical_path: String,
    pub approval_state: String,
}

/// Authority-owned stable identity mapping. Derived indexes may cache this ID
/// but must not substitute rowids or unregistered, transient search IDs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceRecord {
    pub id: String,
    pub provider: String,
    pub source_namespace: String,
    pub source_kind: String,
    pub key_type: String,
    pub source_key: String,
}

fn source_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<SourceRecord> {
    Ok(SourceRecord {
        id: row.get(0)?,
        provider: row.get(1)?,
        source_namespace: row.get(2)?,
        source_kind: row.get(3)?,
        key_type: row.get(4)?,
        source_key: row.get(5)?,
    })
}

fn verify_source_record(source: &SourceRecord) -> Result<()> {
    let key = match source.key_type.as_str() {
        "external" => SourceKey::External(&source.source_key),
        "raw_sha256" => SourceKey::RawSha256(&source.source_key),
        _ => return Err(StateError::Corrupt),
    };
    let actual = canonical_source_id(&SourceDescriptor {
        provider: &source.provider,
        source_namespace: &source.source_namespace,
        source_kind: &source.source_kind,
        key,
    })
    .map_err(|_| StateError::Corrupt)?;
    if actual != source.id {
        return Err(StateError::Corrupt);
    }
    Ok(())
}

impl StateStore {
    /// Call only after authorized unlock. Never wipe, recreate, or auto-repair
    /// an existing unknown/corrupt/future-schema database.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_internal(path, None)
    }
    /// Encrypted vaults only. Caller must validate unlock epoch before and after usage.
    pub fn open_encrypted(
        path: impl AsRef<Path>,
        raw_key: [u8; 32],
        gate: &mind_place_security::SecurityGate,
        lease: mind_place_security::UnlockLease,
    ) -> Result<Self> {
        Self::open_internal(path, Some(CipherKey::new(raw_key, gate, lease)))
    }
    fn open_internal(
        path: impl AsRef<Path>,
        cipher_key: Option<std::sync::Arc<CipherKey>>,
    ) -> Result<Self> {
        if rusqlite::version_number() < MIN_SQLITE_NUMBER {
            return Err(StateError::SqliteTooOld);
        }
        let path = path.as_ref().to_owned();
        let parent = path.parent().ok_or(StateError::UnsafePath)?;
        safe_path(parent)?;
        // A recognized encrypted-vault root must never fall back to plaintext
        // SQLite, even when its database has not yet been created.
        if cipher_key.is_none() && parent.join("vault-v1.json").exists() {
            return Err(StateError::Corrupt);
        }
        fs::create_dir_all(parent)?;
        safe_path(parent)?;
        safe_path(&path)?;
        // A keyed vault initializer may create/migrate schema. Participate in
        // the same cross-process writer fence as immutable objects and backups.
        let _writer_guard = if cipher_key.is_some() && parent.join("vault-v1.json").exists() {
            Some(VaultWriterFence::writer(parent).map_err(|_| StateError::Corrupt)?)
        } else {
            None
        };
        let lock_path = parent.join(".state-schema.lock");
        safe_path(&lock_path)?;
        let lock = OpenOptions::new()
            .create(true)
            .write(true)
            .open(lock_path)?;
        lock.try_lock_exclusive().map_err(|_| StateError::Busy)?;

        let existed = fs::symlink_metadata(&path).is_ok();
        if existed {
            // Read-only preflight prevents touching unknown/future/corrupt DBs.
            let ro = Connection::open_with_flags(
                &path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
            )?;
            if let Some(key) = cipher_key.as_ref() {
                key.unlock(&ro)?;
            }
            identity(&ro)?;
            let version = user_version(&ro)?;
            if version > SCHEMA_VERSION {
                return Err(StateError::FutureSchema(version));
            }
            if version == 0 {
                health(&ro)?;
            } else {
                validate_version(&ro, version)?;
            }
        }
        let mut db = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        if let Some(key) = cipher_key.as_ref() {
            key.unlock(&db)?;
        }
        db.busy_timeout(Duration::from_secs(5))?;
        db.pragma_update(None, "foreign_keys", "ON")?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "synchronous", "FULL")?;
        if !existed {
            db.pragma_update(None, "application_id", APPLICATION_ID)?;
        }
        let previous_version = user_version(&db)?;
        if previous_version < SCHEMA_VERSION {
            if existed && cipher_key.is_some() {
                // Encrypted schema migrations require separately consented staging
                // (`migrate_encrypted_with_backup`); ordinary snapshots would leak plaintext.
                if previous_version == 0 {
                    return Err(StateError::Corrupt);
                }
                return Err(StateError::MigrationRequired {
                    current: previous_version,
                    target: SCHEMA_VERSION,
                });
            }
            if existed {
                backup(&db, parent, previous_version)?;
            }
            // Every pending schema step commits as one transaction. A failed
            // rebuild leaves the entire original state intact.
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            apply_pending_migrations_tx(&tx, previous_version)?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            tx.commit()?;
            validate(&db)?;
        } else if !existed {
            validate(&db)?;
        }
        drop(db);
        FileExt::unlock(&lock)?;
        Ok(Self { path, cipher_key })
    }

    /// Explicitly migrates an existing SQLCipher-encrypted state database forward
    /// to `SCHEMA_VERSION`, first writing and verifying an encrypted backup
    /// outside the vault directory so plaintext never touches disk and vault
    /// backup inventories stay intact.
    #[cfg(feature = "sqlcipher-preview")]
    pub fn migrate_encrypted_with_backup(
        path: impl AsRef<Path>,
        raw_key: [u8; 32],
        gate: &mind_place_security::SecurityGate,
        lease: mind_place_security::UnlockLease,
        backup_destination: impl AsRef<Path>,
    ) -> Result<Self> {
        use rusqlite::backup::Backup;
        use std::io::Read;

        if rusqlite::version_number() < MIN_SQLITE_NUMBER {
            return Err(StateError::SqliteTooOld);
        }
        let path = path.as_ref().to_owned();
        let parent = path.parent().ok_or(StateError::UnsafePath)?;
        let backup_destination = backup_destination.as_ref();
        let backup_parent = backup_destination.parent().ok_or(StateError::UnsafePath)?;
        safe_path(parent)?;
        safe_path(&path)?;
        safe_path(backup_parent)?;
        safe_path(backup_destination)?;
        if backup_destination.starts_with(parent)
            || parent.starts_with(backup_destination)
            || !backup_parent.is_dir()
        {
            return Err(StateError::UnsafePath);
        }
        let key = CipherKey::new(raw_key, gate, lease);
        let _writer_guard = if parent.join("vault-v1.json").exists() {
            Some(VaultWriterFence::writer(parent).map_err(|_| StateError::Corrupt)?)
        } else {
            None
        };
        let lock_path = parent.join(".state-schema.lock");
        safe_path(&lock_path)?;
        let lock = OpenOptions::new()
            .create(true)
            .write(true)
            .open(lock_path)?;
        lock.try_lock_exclusive().map_err(|_| StateError::Busy)?;

        if fs::symlink_metadata(&path).is_err() {
            return Err(StateError::InvalidValue);
        }
        let mut db = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        key.unlock(&db)?;
        db.busy_timeout(Duration::from_secs(5))?;
        db.pragma_update(None, "foreign_keys", "ON")?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "synchronous", "FULL")?;
        identity(&db)?;
        let previous_version = user_version(&db)?;
        if previous_version > SCHEMA_VERSION {
            return Err(StateError::FutureSchema(previous_version));
        }
        if previous_version == 0 {
            health(&db)?;
        } else {
            validate_version(&db, previous_version)?;
        }
        if previous_version < SCHEMA_VERSION {
            let placeholder = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(backup_destination)?;
            placeholder.sync_all()?;
            drop(placeholder);
            let mut target = Connection::open_with_flags(
                backup_destination,
                OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
            )?;
            key.unlock(&target)?;
            {
                let b = Backup::new(&db, &mut target)?;
                b.run_to_completion(64, Duration::from_millis(50), None)?;
            }
            key.unlock(&target)?;
            if previous_version == 0 {
                identity(&target)?;
                health(&target)?;
            } else {
                validate_version(&target, previous_version)?;
            }
            drop(target);
            let mut first = [0u8; 16];
            File::open(backup_destination)?.read_exact(&mut first)?;
            if &first == b"SQLite format 3\0" {
                return Err(StateError::Corrupt);
            }
            OpenOptions::new()
                .read(true)
                .write(true)
                .open(backup_destination)?
                .sync_all()?;

            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            apply_pending_migrations_tx(&tx, previous_version)?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            tx.commit()?;
        }
        validate(&db)?;
        drop(db);
        FileExt::unlock(&lock)?;
        Ok(Self {
            path,
            cipher_key: Some(key),
        })
    }
    /// Create a consistent, still-encrypted snapshot of the authoritative state,
    /// including committed WAL content, using SQLite's Online Backup API.
    ///
    /// This is a single-database primitive, NOT an atomic cross-database/object
    /// vault backup. A higher-level owner-authorized coordinator must fence all
    /// writers, capture every database/object at the same generation, and then
    /// package the verified results. It never reads or mutates legacy plaintext.
    #[cfg(feature = "sqlcipher-preview")]
    pub fn create_encrypted_online_snapshot(&self, destination: impl AsRef<Path>) -> Result<()> {
        use rusqlite::backup::Backup;
        use std::io::Read;

        let key = self.cipher_key.as_ref().ok_or(StateError::Corrupt)?;
        let destination = destination.as_ref();
        let parent = destination.parent().ok_or(StateError::UnsafePath)?;
        safe_path(parent)?;
        safe_path(destination)?;
        if destination == self.path || !parent.is_dir() {
            return Err(StateError::UnsafePath);
        }

        // Exclusive creation ensures no existing owner backup can be overwritten.
        // On failure, the staged output remains for explicit owner disposition.
        // Its destination pager is keyed *before* any SQLite pages are copied.
        let source = self.read_connection()?;
        let placeholder = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)?;
        placeholder.sync_all()?;
        drop(placeholder);
        let mut target = Connection::open_with_flags(
            destination,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        key.unlock(&target)?;
        {
            let backup = Backup::new(&source, &mut target)?;
            backup.run_to_completion(64, Duration::from_millis(50), None)?;
        }
        // Verify real schema and active keyed codec, not just an opaque header.
        key.unlock(&target)?;
        validate(&target)?;
        let health: String = target.query_row("PRAGMA quick_check(1)", [], |row| row.get(0))?;
        if health != "ok" {
            return Err(StateError::Corrupt);
        }
        drop(target);
        drop(source);
        let mut first = [0u8; 16];
        File::open(destination)?.read_exact(&mut first)?;
        if &first == b"SQLite format 3\0" {
            return Err(StateError::Corrupt);
        }
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
    fn connection(&self) -> Result<FencedStateConnection> {
        safe_path(&self.path)?;
        let parent = self.path.parent().ok_or(StateError::UnsafePath)?;
        let writer = if self.cipher_key.is_some() && parent.join("vault-v1.json").exists() {
            Some(VaultWriterFence::writer(parent).map_err(|_| StateError::Corrupt)?)
        } else {
            None
        };
        let db = Connection::open_with_flags(
            &self.path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        if let Some(key) = self.cipher_key.as_ref() {
            key.unlock(&db)?;
        }
        db.busy_timeout(Duration::from_secs(5))?;
        db.pragma_update(None, "foreign_keys", "ON")?;
        db.pragma_update(None, "synchronous", "FULL")?;
        identity(&db)?;
        if user_version(&db)? != SCHEMA_VERSION {
            return Err(StateError::Corrupt);
        }
        Ok(FencedStateConnection {
            db,
            _writer: writer,
        })
    }
    /// Fresh authority snapshot for hot read-only checks. Do not configure a
    /// writable WAL connection (and its close/checkpoint work) for each poll.
    fn read_connection(&self) -> Result<Connection> {
        safe_path(&self.path)?;
        let db = Connection::open_with_flags(
            &self.path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        if let Some(key) = self.cipher_key.as_ref() {
            key.unlock(&db)?;
        }
        db.busy_timeout(Duration::from_secs(5))?;
        identity(&db)?;
        if user_version(&db)? != SCHEMA_VERSION {
            return Err(StateError::Corrupt);
        }
        Ok(db)
    }
    /// Idempotent, durable registration; no derived/index identifier is an input.
    pub fn register_source(&self, source: &SourceDescriptor<'_>) -> Result<String> {
        let id = canonical_source_id(source)?;
        let (key_type, key) = source.key.parts();
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        source_redaction::assert_source_not_redacted(&tx, &id)?;
        tx.execute(
            "INSERT INTO source_identities(id,provider,source_namespace,source_kind,key_type,source_key)
             VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT DO NOTHING",
            params![
                id,
                source.provider,
                source.source_namespace,
                source.source_kind,
                key_type,
                key
            ],
        )?;
        let saved = tx
            .query_row(
                "SELECT id,provider,source_namespace,source_kind,key_type,source_key
                 FROM source_identities WHERE id=?1",
                [&id],
                source_record,
            )
            .optional()?
            .ok_or(StateError::Corrupt)?;
        verify_source_record(&saved)?;
        if saved.provider != source.provider
            || saved.source_namespace != source.source_namespace
            || saved.source_kind != source.source_kind
            || saved.key_type != key_type
            || saved.source_key != key
        {
            return Err(StateError::Corrupt);
        }
        tx.commit()?;
        Ok(id)
    }

    /// Resolve authority identity independently from index.sqlite3.
    pub fn source(&self, id: &str) -> Result<Option<SourceRecord>> {
        valid_id(id)?;
        let db = self.read_connection()?;
        let saved = db
            .query_row(
                "SELECT id,provider,source_namespace,source_kind,key_type,source_key
                 FROM source_identities WHERE id=?1",
                [id],
                source_record,
            )
            .optional()?;
        let Some(record) = saved else {
            // Unknown or non-canonical IDs (e.g. a derived index ID) resolve
            // to None, as before MP-9C; they can never carry a tombstone.
            return Ok(None);
        };
        verify_source_record(&record)?;
        // A registered (hence canonical) but redacted source is hidden.
        if source_redaction::source_is_redacted(&db, &record.id)? {
            return Ok(None);
        }
        Ok(Some(record))
    }

    /// Evidence must name an authority-registered canonical source. The FK
    /// rejects disposable rowids, nonexistent sources and forged index IDs.
    pub fn add_claim_source_evidence(
        &self,
        claim_id: &str,
        source_id: &str,
        evidence_kind: &str,
    ) -> Result<()> {
        valid_id(claim_id)?;
        valid_id(source_id)?;
        valid_text(evidence_kind)?;
        if evidence_kind.len() > 64 || !source_id.starts_with("src:v1:") {
            return Err(StateError::InvalidValue);
        }
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        source_redaction::assert_source_not_redacted(&tx, source_id)?;
        let source = tx
            .query_row(
                "SELECT id,provider,source_namespace,source_kind,key_type,source_key
             FROM source_identities WHERE id=?1",
                [source_id],
                source_record,
            )
            .optional()?
            .ok_or(StateError::InvalidValue)?;
        verify_source_record(&source)?;
        tx.execute(
            "INSERT INTO claim_evidence(claim_id,source_ref,evidence_kind) VALUES(?1,?2,?3)",
            params![claim_id, source_id, evidence_kind],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn create_project(&self, id: &str, name: &str) -> Result<()> {
        valid_id(id)?;
        valid_text(name)?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        topics::create_legacy_project_tx(&tx, id, name)?;
        tx.commit()?;
        Ok(())
    }
    pub fn get_project(&self, id: &str) -> Result<Option<Project>> {
        valid_id(id)?;
        let db = self.read_connection()?;
        if let Some(project) = db
            .query_row("SELECT id,name FROM projects WHERE id=?1", [id], |r| {
                Ok(Project {
                    id: r.get(0)?,
                    name: r.get(1)?,
                })
            })
            .optional()?
        {
            return Ok(Some(project));
        }
        Ok(db
            .query_row(
                "SELECT id,name FROM topics WHERE id=?1 AND is_project=1",
                [id],
                |r| {
                    Ok(Project {
                        id: r.get(0)?,
                        name: r.get(1)?,
                    })
                },
            )
            .optional()?)
    }
    pub fn create_memory_claim(&self, claim: &MemoryClaim) -> Result<()> {
        valid_id(&claim.id)?;
        valid_text(&claim.statement)?;
        if !matches!(
            claim.asserted_by.as_str(),
            "user" | "model" | "import" | "deterministic"
        ) || !matches!(
            claim.epistemic_origin.as_str(),
            "explicit" | "observed" | "inferred" | "summarized"
        ) || !matches!(
            claim.status.as_str(),
            "active" | "superseded" | "disputed" | "uncertain" | "retracted" | "historical"
        ) {
            return Err(StateError::InvalidValue);
        }
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let clock = memory::next_claim_clock(&tx, 0)?;
        tx.execute("INSERT INTO memory_claims(id,statement,asserted_by,epistemic_origin,status) VALUES(?1,?2,?3,?4,?5)",
            params![claim.id,claim.statement,claim.asserted_by,claim.epistemic_origin,claim.status])?;
        tx.execute("INSERT INTO memory_claim_events(claim_id,revision,event_type,payload_json,recorded_at_ms) VALUES(?1,1,'created',?2,?3)",
            params![claim.id,serde_json::to_string(claim).map_err(|_|StateError::InvalidValue)?,clock])?;
        tx.commit()?;
        Ok(())
    }
    pub fn add_pending_root(&self, id: &str, canonical_path: &str) -> Result<()> {
        valid_id(id)?;
        valid_text(canonical_path)?;
        // Staging a root is not permission to read it.
        self.connection()?.execute(
            "INSERT INTO source_roots(id,canonical_path,approval_state) VALUES(?1,?2,'pending')",
            params![id, canonical_path],
        )?;
        Ok(())
    }
    pub fn revoke_root(&self, id: &str) -> Result<()> {
        valid_id(id)?;
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        let changed=tx.execute("UPDATE source_roots SET approval_state='revoked',revoked_at=unixepoch() WHERE id=?1 AND approval_state!='revoked'",[id])?;
        if changed == 0 {
            return Err(StateError::InvalidValue);
        }
        tx.execute("INSERT INTO root_tombstones(root_id) VALUES(?1)", [id])?;
        tx.commit()?;
        Ok(())
    }
    pub fn root(&self, id: &str) -> Result<Option<SourceRoot>> {
        valid_id(id)?;
        Ok(self
            .connection()?
            .query_row(
                "SELECT id,canonical_path,approval_state FROM source_roots WHERE id=?1",
                [id],
                |r| {
                    Ok(SourceRoot {
                        id: r.get(0)?,
                        canonical_path: r.get(1)?,
                        approval_state: r.get(2)?,
                    })
                },
            )
            .optional()?)
    }
}
fn valid_id(s: &str) -> Result<()> {
    if s.is_empty()
        || s.len() > 256
        || !s.is_ascii()
        || !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'-' | b'_' | b'.'))
    {
        return Err(StateError::InvalidValue);
    }
    Ok(())
}
fn valid_text(s: &str) -> Result<()> {
    if s.trim().is_empty() || s.len() > 65536 || s.contains('\0') {
        return Err(StateError::InvalidValue);
    }
    Ok(())
}
fn user_version(db: &Connection) -> Result<i64> {
    Ok(db.query_row("PRAGMA user_version", [], |r| r.get(0))?)
}
fn identity(db: &Connection) -> Result<()> {
    let id: i64 = db.query_row("PRAGMA application_id", [], |r| r.get(0))?;
    if id != APPLICATION_ID {
        return Err(StateError::ForeignDatabase);
    }
    Ok(())
}
fn health(db: &Connection) -> Result<()> {
    let result: String = db.query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?;
    if result != "ok" {
        return Err(StateError::Corrupt);
    }
    Ok(())
}
fn checksum(sql: &str) -> String {
    format!("{:x}", Sha256::digest(sql.as_bytes()))
}
fn apply_pending_migrations_tx(
    tx: &rusqlite::Transaction<'_>,
    previous_version: i64,
) -> Result<()> {
    for (step, sql) in [
        (1, MIGRATION_V1),
        (2, MIGRATION_V2),
        (3, MIGRATION_V3),
        (4, MIGRATION_V4),
        (5, MIGRATION_V5),
        (6, MIGRATION_V6),
        (7, MIGRATION_V7),
        (8, MIGRATION_V8),
        (9, MIGRATION_V9),
        (10, MIGRATION_V10),
    ] {
        if previous_version < step {
            tx.execute_batch(sql)?;
            tx.execute(
                "INSERT INTO schema_migrations(version,checksum) VALUES(?1,?2)",
                params![step, checksum(sql)],
            )?;
            if step == 10 {
                topics::backfill_v10_topics_and_projects(tx)?;
            }
        }
    }
    Ok(())
}

fn validate(db: &Connection) -> Result<()> {
    validate_version(db, SCHEMA_VERSION)
}
fn validate_version(db: &Connection, version: i64) -> Result<()> {
    identity(db)?;
    health(db)?;
    if !(1..=SCHEMA_VERSION).contains(&version) || user_version(db)? != version {
        return Err(StateError::Corrupt);
    }
    for (step, migration) in [
        (1, MIGRATION_V1),
        (2, MIGRATION_V2),
        (3, MIGRATION_V3),
        (4, MIGRATION_V4),
        (5, MIGRATION_V5),
        (6, MIGRATION_V6),
        (7, MIGRATION_V7),
        (8, MIGRATION_V8),
        (9, MIGRATION_V9),
        (10, MIGRATION_V10),
    ] {
        if step > version {
            break;
        }
        let saved: Option<String> = db
            .query_row(
                "SELECT checksum FROM schema_migrations WHERE version=?1",
                [step],
                |r| r.get(0),
            )
            .optional()
            .map_err(|_| StateError::Corrupt)?;
        if saved.as_deref() != Some(checksum(migration).as_str()) {
            return Err(StateError::Corrupt);
        }
    }
    let bad: Option<i64> = db
        .query_row("SELECT 1 FROM pragma_foreign_key_check LIMIT 1", [], |r| {
            r.get(0)
        })
        .optional()
        .map_err(|_| StateError::Corrupt)?;
    if bad.is_some() {
        return Err(StateError::Corrupt);
    }
    if version >= 2 {
        let mut statement = db
            .prepare(
                "SELECT id,provider,source_namespace,source_kind,key_type,source_key
                 FROM source_identities",
            )
            .map_err(|_| StateError::Corrupt)?;
        let records = statement
            .query_map([], source_record)
            .map_err(|_| StateError::Corrupt)?;
        for record in records {
            verify_source_record(&record.map_err(|_| StateError::Corrupt)?)?;
        }
    }
    if version >= 10 {
        curated::verify_curated_invariants_connection(db).map_err(|_| StateError::Corrupt)?;
    }
    Ok(())
}
fn backup(db: &Connection, parent: &Path, version: i64) -> Result<()> {
    let folder = parent.join("state-backups");
    safe_path(&folder)?;
    fs::create_dir_all(&folder)?;
    safe_path(&folder)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| StateError::Corrupt)?
        .as_nanos();
    let dest = folder.join(format!(
        "state-v{version}-{nonce}-{}.sqlite3",
        std::process::id()
    ));
    safe_path(&dest)?;
    // Never copy SQLite main db alone while WAL is in use.
    db.execute("VACUUM INTO ?1", [dest.to_string_lossy().to_string()])?;
    let ro = Connection::open_with_flags(
        &dest,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    identity(&ro)?;
    health(&ro)?;
    drop(ro);
    // Windows FlushFileBuffers requires a write-capable file handle.
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(&dest)?
        .sync_all()?;
    #[cfg(unix)]
    {
        File::open(&folder)?.sync_all()?;
    }
    Ok(())
}
fn safe_path(path: &Path) -> Result<()> {
    for p in path.ancestors() {
        match fs::symlink_metadata(p) {
            Ok(md) => {
                if md.file_type().is_symlink() {
                    return Err(StateError::UnsafePath);
                }
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if md.file_attributes() & 0x400 != 0 {
                        return Err(StateError::UnsafePath);
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(StateError::Io(e)),
        }
    }
    Ok(())
}

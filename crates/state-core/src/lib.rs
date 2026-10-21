use fs2::FileExt;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
mod clients;
mod document_roots;
mod identity;
pub use document_roots::{DocumentRoot, VirtualDocumentPath, VIRTUAL_ROOT_PREFIX};
pub use clients::{ClientAuditEvent, ClientCapability, ClientRecord};
pub use identity::{canonical_source_id, SourceDescriptor, SourceKey};

pub const APPLICATION_ID: i64 = 0x4D504C43;
pub const SCHEMA_VERSION: i64 = 4;
const MIN_SQLITE_NUMBER: i32 = 3_053_002;
const MIGRATION_V1: &str = include_str!("schema_v1.sql");
const MIGRATION_V2: &str = include_str!("schema_v2.sql");
const MIGRATION_V3: &str = include_str!("schema_v3.sql");
const MIGRATION_V4: &str = include_str!("schema_v4.sql");
pub type Result<T> = std::result::Result<T, StateError>;

#[derive(Debug, thiserror::Error)]
pub enum StateError {
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
}
impl StateError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io(_) => "MP_STATE_IO",
            Self::Sql(_) => "MP_STATE_SQL",
            Self::UnsafePath => "MP_STATE_UNSAFE_PATH",
            Self::ForeignDatabase => "MP_STATE_FOREIGN_DATABASE",
            Self::FutureSchema(_) => "MP_STATE_FUTURE_SCHEMA",
            Self::Corrupt => "MP_STATE_CORRUPT",
            Self::Busy => "MP_STATE_BUSY",
            Self::SqliteTooOld => "MP_STATE_SQLITE_TOO_OLD",
            Self::InvalidValue => "MP_STATE_INVALID_VALUE",
        }
    }
}
#[derive(Clone, Debug)]
pub struct StateStore {
    path: PathBuf,
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
        if rusqlite::version_number() < MIN_SQLITE_NUMBER {
            return Err(StateError::SqliteTooOld);
        }
        let path = path.as_ref().to_owned();
        let parent = path.parent().ok_or(StateError::UnsafePath)?;
        safe_path(parent)?;
        fs::create_dir_all(parent)?;
        safe_path(parent)?;
        safe_path(&path)?;
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
        db.busy_timeout(Duration::from_secs(5))?;
        db.pragma_update(None, "foreign_keys", "ON")?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "synchronous", "FULL")?;
        if !existed {
            db.pragma_update(None, "application_id", APPLICATION_ID)?;
        }
        let previous_version = user_version(&db)?;
        if previous_version < SCHEMA_VERSION {
            if existed {
                backup(&db, parent, previous_version)?;
            }
            // Every pending schema step commits as one transaction. A failed
            // v2 rebuild leaves the entire original v0/v1 state intact.
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            if previous_version < 1 {
                tx.execute_batch(MIGRATION_V1)?;
                tx.execute(
                    "INSERT INTO schema_migrations(version,checksum) VALUES(1,?1)",
                    [checksum(MIGRATION_V1)],
                )?;
            }
            if previous_version < 2 {
                tx.execute_batch(MIGRATION_V2)?;
                tx.execute(
                    "INSERT INTO schema_migrations(version,checksum) VALUES(2,?1)",
                    [checksum(MIGRATION_V2)],
                )?;
            }
            if previous_version < 3 {
                tx.execute_batch(MIGRATION_V3)?;
                tx.execute(
                    "INSERT INTO schema_migrations(version,checksum) VALUES(3,?1)",
                    [checksum(MIGRATION_V3)],
                )?;
            }
            if previous_version < 4 {
                tx.execute_batch(MIGRATION_V4)?;
                tx.execute(
                    "INSERT INTO schema_migrations(version,checksum) VALUES(4,?1)",
                    [checksum(MIGRATION_V4)],
                )?;
            }
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            tx.commit()?;
        }
        validate(&db)?;
        drop(db);
        FileExt::unlock(&lock)?;
        Ok(Self { path })
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    fn connection(&self) -> Result<Connection> {
        safe_path(&self.path)?;
        let db = Connection::open_with_flags(
            &self.path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        db.busy_timeout(Duration::from_secs(5))?;
        db.pragma_update(None, "foreign_keys", "ON")?;
        db.pragma_update(None, "synchronous", "FULL")?;
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
        let saved = self
            .connection()?
            .query_row(
                "SELECT id,provider,source_namespace,source_kind,key_type,source_key
                 FROM source_identities WHERE id=?1",
                [id],
                source_record,
            )
            .optional()?;
        if let Some(ref record) = saved {
            verify_source_record(record)?;
        }
        Ok(saved)
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
        let source = self.source(source_id)?;
        if source.is_none() {
            return Err(StateError::InvalidValue);
        }
        self.connection()?.execute(
            "INSERT INTO claim_evidence(claim_id,source_ref,evidence_kind) VALUES(?1,?2,?3)",
            params![claim_id, source_id, evidence_kind],
        )?;
        Ok(())
    }

    pub fn create_project(&self, id: &str, name: &str) -> Result<()> {
        valid_id(id)?;
        valid_text(name)?;
        self.connection()?.execute(
            "INSERT INTO projects(id,name) VALUES(?1,?2)",
            params![id, name],
        )?;
        Ok(())
    }
    pub fn get_project(&self, id: &str) -> Result<Option<Project>> {
        valid_id(id)?;
        Ok(self
            .connection()?
            .query_row("SELECT id,name FROM projects WHERE id=?1", [id], |r| {
                Ok(Project {
                    id: r.get(0)?,
                    name: r.get(1)?,
                })
            })
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
        let tx = db.transaction()?;
        tx.execute("INSERT INTO memory_claims(id,statement,asserted_by,epistemic_origin,status) VALUES(?1,?2,?3,?4,?5)",
            params![claim.id,claim.statement,claim.asserted_by,claim.epistemic_origin,claim.status])?;
        tx.execute("INSERT INTO memory_claim_events(claim_id,revision,event_type,payload_json) VALUES(?1,1,'created',?2)",
            params![claim.id,serde_json::to_string(claim).map_err(|_|StateError::InvalidValue)?])?;
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
fn validate(db: &Connection) -> Result<()> {
    validate_version(db, SCHEMA_VERSION)
}
fn validate_version(db: &Connection, version: i64) -> Result<()> {
    identity(db)?;
    health(db)?;
    if !(1..=SCHEMA_VERSION).contains(&version) || user_version(db)? != version {
        return Err(StateError::Corrupt);
    }
    for (step, migration) in [(1, MIGRATION_V1), (2, MIGRATION_V2), (3, MIGRATION_V3), (4, MIGRATION_V4)] {
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

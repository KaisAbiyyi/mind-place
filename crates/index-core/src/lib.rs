//! Rebuildable, non-authoritative archive search projection.
//! Every durable source ID is registered in state.sqlite3, never assigned by this DB.
use mind_place_archive::{ArchiveStore, Capture};
use mind_place_state::{CipherKey, SourceDescriptor, SourceKey, StateStore};
use rusqlite::{
    params, Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

const APPLICATION_ID: i64 = 0x4D504958; // MPIX, deliberately distinct from state.
const SCHEMA_VERSION: i64 = 4;
const NAMESPACE: &str = "mind-place-chatgpt-archive-v1";

// Preserve historical ChatGPT source identity bytes; each newer provider has
// its own explicit local archive namespace.
pub(crate) fn namespace_for_provider(provider: &str) -> Option<&'static str> {
    match provider {
        "chatgpt" => Some(NAMESPACE),
        "gemini" => Some("mind-place-gemini-archive-v1"),
        "claude" => Some("mind-place-claude-archive-v1"),
        _ => None,
    }
}
const MAX_CHUNK_BYTES: usize = 4096;
const SCHEMA: &str = include_str!("schema_v1.sql");
const GRAPH_SCHEMA: &str = include_str!("schema_v2.sql");
const GRAPH_GENERATION_SCHEMA: &str = include_str!("schema_v3.sql");
const GRAPHIFY_SCHEMA: &str = include_str!("schema_v4.sql");

pub mod context;
pub mod documents;
pub mod fetch;
pub mod graph;
pub mod graph_generation;
pub mod graph_maintenance;
pub mod graph_query;
pub mod graphify;
pub mod inspection;
pub use context::{
    ContextBudget, ContextExcerpt, ContextGroup, ContextPack, ContextPreset, ContextRequest,
};
pub mod retrieval;
pub use fetch::{
    FetchBudget, FetchCursor, FetchGranularity, FetchRequest, FetchResponse, FetchSegment,
};
pub use graph::{
    GraphEdge, GraphEvidence, GraphKind, GraphNode, GraphNodeRef, GraphOrigin, GraphProjection,
    GraphProjectionStats, GraphRelation,
};
pub use graph_generation::{GraphActivation, GraphGeneration, GraphInputHashes};
pub use graph_maintenance::{GraphBatchReceipt, GraphMaintenanceBatch, GraphMaintenanceJob};
pub use graph_query::{
    GraphCommunities, GraphCommunity, GraphDirection, GraphNeighborhood, GraphNeighborhoodRequest,
    GraphQueryEdge, GraphQueryNode,
};
pub use graphify::{
    graphify_prompt, inferred_edge, qualify_graphify_batch, submit_graphify_batch, GraphifyProfile,
    GraphifyPromptInput, GraphifyQualification,
};
pub use inspection::{ContextInspection, FetchInspection, RetrievalTrace, SearchInspection};
pub use retrieval::{
    MatchMode, QueryTerm, RetrievalFilters, RetrievalHit, RetrievalRequest, RetrievalResponse,
    ScoreContribution, WeightedQuery,
};

pub type Result<T> = std::result::Result<T, IndexError>;

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("index I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("index SQLite: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("archive: {0}")]
    Archive(#[from] mind_place_archive::ArchiveError),
    #[error("authority: {0}")]
    State(#[from] mind_place_state::StateError),
    #[error("unsafe index path")]
    UnsafePath,
    #[error("not a Mind Place derived index")]
    ForeignDatabase,
    #[error("unknown or damaged derived index; remove the disposable file to rebuild")]
    InvalidIndex,
    #[error("archive identity/integrity mismatch; raw evidence was not changed")]
    InvalidEvidence,
    #[error("index operation cancelled by security boundary")]
    Revoked,
    #[error("invalid index query")]
    InvalidQuery,
    #[error("stale derived graph generation; retry against the current fence")]
    StaleGeneration,
}

impl IndexError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io(_) => "MP_INDEX_IO",
            Self::Sql(_) => "MP_INDEX_SQL",
            Self::Archive(_) => "MP_INDEX_ARCHIVE",
            Self::State(_) => "MP_INDEX_AUTHORITY",
            Self::UnsafePath => "MP_INDEX_UNSAFE_PATH",
            Self::ForeignDatabase => "MP_INDEX_FOREIGN_DATABASE",
            Self::InvalidIndex => "MP_INDEX_INVALID",
            Self::InvalidEvidence => "MP_INDEX_INVALID_EVIDENCE",
            Self::Revoked => "MP_INDEX_REVOKED",
            Self::InvalidQuery => "MP_INDEX_INVALID_QUERY",
            Self::StaleGeneration => "MP_INDEX_STALE_GENERATION",
        }
    }
}

#[derive(Clone, Debug)]
pub struct IndexStore {
    path: PathBuf,
    cipher_key: Option<std::sync::Arc<CipherKey>>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct IndexStats {
    pub sources: i64,
    pub revisions: i64,
    pub chunks: i64,
    pub generation: i64,
    pub inserted_revisions: i64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct SearchHit {
    pub source_id: String,
    pub archive_id: String,
    pub snapshot_id: String,
    pub message_id: String,
    pub part: i64,
    pub kind: String,
    pub role: String,
    pub title: String,
    pub snippet: String,
    pub score: f64,
}

fn safe_path(path: &Path) -> Result<()> {
    for part in path.ancestors() {
        match fs::symlink_metadata(part) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    return Err(IndexError::UnsafePath);
                }
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if meta.file_attributes() & 0x400 != 0 {
                        return Err(IndexError::UnsafePath);
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

fn configure(db: &Connection) -> Result<()> {
    db.busy_timeout(Duration::from_secs(5))?;
    db.pragma_update(None, "foreign_keys", "ON")?;
    db.pragma_update(None, "journal_mode", "WAL")?;
    db.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(())
}

fn expected_schema_hash(version: i64) -> String {
    let mut hash = Sha256::new();
    hash.update(SCHEMA.as_bytes());
    if version >= 2 {
        hash.update(GRAPH_SCHEMA.as_bytes());
    }
    if version >= 3 {
        hash.update(GRAPH_GENERATION_SCHEMA.as_bytes());
    }
    if version >= 4 {
        hash.update(GRAPHIFY_SCHEMA.as_bytes());
    }
    format!("{:x}", hash.finalize())
}

fn verify(db: &Connection, check_integrity: bool) -> Result<()> {
    let app_id: i64 = db.query_row("PRAGMA application_id", [], |r| r.get(0))?;
    if app_id != APPLICATION_ID {
        return Err(IndexError::ForeignDatabase);
    }
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if !(1..=SCHEMA_VERSION).contains(&version) {
        return Err(IndexError::InvalidIndex);
    }
    if check_integrity {
        let health: String = db.query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?;
        if health != "ok" {
            return Err(IndexError::InvalidIndex);
        }
    }
    let schema_hash: String = db
        .query_row(
            "SELECT value FROM index_meta WHERE key='schema_hash'",
            [],
            |r| r.get(0),
        )
        .map_err(|_| IndexError::InvalidIndex)?;
    if schema_hash != expected_schema_hash(version) {
        return Err(IndexError::InvalidIndex);
    }
    Ok(())
}

impl IndexStore {
    /// Open after unlock only. Refuses an existing foreign/unrecognized file.
    /// Missing index files are safe to initialize because they contain no authority.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_internal(path, None)
    }
    /// New encrypted vaults only; never modifies an existing plaintext index.
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
        let path = path.as_ref().to_owned();
        let parent = path.parent().ok_or(IndexError::UnsafePath)?;
        safe_path(parent)?;
        // Never materialize a cleartext search projection inside a vault.
        if cipher_key.is_none() && parent.join("vault-v1.json").exists() {
            return Err(IndexError::ForeignDatabase);
        }
        fs::create_dir_all(parent)?;
        safe_path(&path)?;
        let existed = path.exists();
        if existed {
            let ro = Connection::open_with_flags(
                &path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
            )?;
            if let Some(key) = cipher_key.as_ref() {
                key.unlock(&ro)?;
            }
            verify(&ro, true)?;
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
        configure(&db)?;
        if !existed {
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(SCHEMA)?;
            tx.execute_batch(GRAPH_SCHEMA)?;
            tx.execute_batch(GRAPH_GENERATION_SCHEMA)?;
            tx.execute_batch(GRAPHIFY_SCHEMA)?;
            tx.pragma_update(None, "application_id", APPLICATION_ID)?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            tx.execute(
                "INSERT INTO index_meta(key,value) VALUES('schema_hash',?1)",
                [expected_schema_hash(SCHEMA_VERSION)],
            )?;
            tx.commit()?;
        } else {
            let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
            if version < SCHEMA_VERSION {
                // Preserve existing FTS + MP-6A graph data. Both derived-schema
                // upgrades commit together; neither touches archives or state.
                let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
                if version == 1 {
                    tx.execute_batch(GRAPH_SCHEMA)?;
                }
                if version < 3 {
                    tx.execute_batch(GRAPH_GENERATION_SCHEMA)?;
                }
                tx.execute_batch(GRAPHIFY_SCHEMA)?;
                tx.execute(
                    "UPDATE index_meta SET value=?1 WHERE key='schema_hash'",
                    [expected_schema_hash(SCHEMA_VERSION)],
                )?;
                tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
                tx.commit()?;
            }
        }
        verify(&db, false)?;
        Ok(Self { path, cipher_key })
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
        if let Some(key) = self.cipher_key.as_ref() {
            key.unlock(&db)?;
        }
        configure(&db)?;
        // Full integrity scans are done on open, not per hot search or transaction.
        verify(&db, false)?;
        Ok(db)
    }

    pub fn stats(&self) -> Result<IndexStats> {
        let db = self.connection()?;
        stats(&db, 0)
    }

    pub fn sync(&self, archive: &ArchiveStore, authority: &StateStore) -> Result<IndexStats> {
        self.sync_with_check(archive, authority, || Ok(()))
    }

    /// Check the unlock lease before every source and before committing.
    /// An interrupted scan leaves the previous search generation untouched.
    pub fn sync_with_check(
        &self,
        archive: &ArchiveStore,
        authority: &StateStore,
        mut check: impl FnMut() -> Result<()>,
    ) -> Result<IndexStats> {
        self.index(archive, authority, false, &mut check)
    }

    pub fn rebuild(&self, archive: &ArchiveStore, authority: &StateStore) -> Result<IndexStats> {
        self.rebuild_with_check(archive, authority, || Ok(()))
    }

    pub fn rebuild_with_check(
        &self,
        archive: &ArchiveStore,
        authority: &StateStore,
        mut check: impl FnMut() -> Result<()>,
    ) -> Result<IndexStats> {
        self.index(archive, authority, true, &mut check)
    }

    fn index(
        &self,
        archive: &ArchiveStore,
        authority: &StateStore,
        rebuild: bool,
        check: &mut impl FnMut() -> Result<()>,
    ) -> Result<IndexStats> {
        check()?;
        // Acquire/validate the full conversation list before starting a mutation.
        let conversations = archive.list("")?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if rebuild {
            tx.execute("DELETE FROM revisions", [])?;
        }
        let generation: i64 = tx.query_row(
            "SELECT CAST(value AS INTEGER) FROM index_meta WHERE key='generation'",
            [],
            |r| r.get(0),
        )?;
        let generation = generation.checked_add(1).ok_or(IndexError::InvalidIndex)?;
        let mut inserted = 0;
        for summary in conversations {
            check()?;
            let selected = archive.read(&summary.id, None)?;
            let namespace =
                namespace_for_provider(&summary.provider).ok_or(IndexError::InvalidEvidence)?;
            if selected.capture.provider != summary.provider
                || selected.capture.conversation_id != summary.conversation_id
                || format!(
                    "{:x}",
                    Sha256::digest(
                        format!("{}:{}", summary.provider, summary.conversation_id).as_bytes()
                    )
                ) != summary.id
            {
                return Err(IndexError::InvalidEvidence);
            }
            // An explicit provider-local namespace avoids cross-provider identity
            // collisions without pretending an unobserved account ID exists.
            let source_id = authority.register_source(&SourceDescriptor {
                provider: &summary.provider,
                source_namespace: namespace,
                source_kind: "conversation",
                key: SourceKey::External(&summary.conversation_id),
            })?;
            let snapshots = archive.snapshots(&summary.id)?;
            if !snapshots
                .iter()
                .any(|r| r.snapshot_id == selected.snapshot_id)
            {
                return Err(IndexError::InvalidEvidence);
            }
            for revision in snapshots {
                check()?;
                let current = i64::from(revision.snapshot_id == selected.snapshot_id);
                let known: bool = tx
                    .query_row(
                        "SELECT 1 FROM revisions WHERE source_id=?1 AND snapshot_id=?2",
                        params![source_id, revision.snapshot_id],
                        |_| Ok(()),
                    )
                    .optional()?
                    .is_some();
                if known {
                    tx.execute(
                        "UPDATE revisions SET seen_generation=?3,is_current=?4
                         WHERE source_id=?1 AND snapshot_id=?2",
                        params![source_id, revision.snapshot_id, generation, current],
                    )?;
                    continue;
                }
                let snapshot = if current == 1 {
                    selected.clone()
                } else {
                    archive.read(&summary.id, Some(&revision.snapshot_id))?
                };
                verify_snapshot(&snapshot.capture, &revision.snapshot_id)?;
                if snapshot.capture.conversation_id != summary.conversation_id
                    || snapshot.capture.provider != summary.provider
                {
                    return Err(IndexError::InvalidEvidence);
                }
                tx.execute(
                    "INSERT INTO revisions(source_id,archive_id,snapshot_id,provider,
                     conversation_id,title,source_url,captured_at,completeness,is_current,seen_generation)
                     VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                    params![
                        source_id, summary.id, revision.snapshot_id,
                        snapshot.capture.provider, snapshot.capture.conversation_id,
                        snapshot.capture.title, snapshot.capture.source_url,
                        snapshot.capture.captured_at, snapshot.capture.completeness,
                        current, generation
                    ],
                )?;
                insert_capture(&tx, &source_id, &revision.snapshot_id, &snapshot.capture)?;
                inserted += 1;
            }
        }
        // Prune only derived rows. The archive and authoritative identities remain intact.
        tx.execute(
            "DELETE FROM revisions WHERE seen_generation != ?1",
            [generation],
        )?;
        graph::refresh_archive_graph(&tx)?;
        tx.execute(
            "UPDATE index_meta SET value=?1 WHERE key='generation'",
            [generation.to_string()],
        )?;
        check()?;
        tx.commit()?;
        stats(&db, inserted)
    }

    /// Provisional Phase 1D lexical lookup. Phase 2 will own ranking/filters.
    pub fn search(
        &self,
        query: &str,
        include_history: bool,
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        if query.trim().is_empty() || query.len() > 1024 || limit == 0 || limit > 100 {
            return Err(IndexError::InvalidQuery);
        }
        let db = self.connection()?;
        let mut statement = db.prepare(
            "SELECT c.source_id,r.archive_id,c.snapshot_id,c.message_id,c.part,
                    c.kind,c.role,r.title,
                    snippet(chunks_fts,0,'[',']','…',18),
                    bm25(chunks_fts)
             FROM chunks_fts
             JOIN chunks c ON c.id=chunks_fts.rowid
             JOIN revisions r ON r.source_id=c.source_id AND r.snapshot_id=c.snapshot_id
             WHERE chunks_fts MATCH ?1 AND (?2=1 OR r.is_current=1)
             ORDER BY bm25(chunks_fts), c.source_id, c.snapshot_id, c.id
             LIMIT ?3",
        )?;
        let rows = statement.query_map(
            params![query, i64::from(include_history), limit as i64],
            |row| {
                Ok(SearchHit {
                    source_id: row.get(0)?,
                    archive_id: row.get(1)?,
                    snapshot_id: row.get(2)?,
                    message_id: row.get(3)?,
                    part: row.get(4)?,
                    kind: row.get(5)?,
                    role: row.get(6)?,
                    title: row.get(7)?,
                    snippet: row.get(8)?,
                    score: row.get(9)?,
                })
            },
        )?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }
}

fn stats(db: &Connection, inserted_revisions: i64) -> Result<IndexStats> {
    let count = |sql| db.query_row(sql, [], |row| row.get::<_, i64>(0));
    Ok(IndexStats {
        sources: count("SELECT COUNT(DISTINCT source_id) FROM revisions")?,
        revisions: count("SELECT COUNT(*) FROM revisions")?,
        chunks: count("SELECT COUNT(*) FROM chunks")?,
        generation: count("SELECT CAST(value AS INTEGER) FROM index_meta WHERE key='generation'")?,
        inserted_revisions,
    })
}

fn verify_snapshot(capture: &Capture, snapshot_id: &str) -> Result<()> {
    // ArchiveStore::save computes the revision identity over this exact
    // normalized capture with observation time removed.
    let mut revision = capture.clone();
    revision.captured_at.clear();
    let bytes = serde_json::to_vec(&revision).map_err(|_| IndexError::InvalidEvidence)?;
    if format!("{:x}", Sha256::digest(&bytes)) != snapshot_id {
        return Err(IndexError::InvalidEvidence);
    }
    Ok(())
}

fn insert_capture(
    tx: &Transaction<'_>,
    source_id: &str,
    snapshot_id: &str,
    capture: &Capture,
) -> Result<()> {
    insert_parts(
        tx,
        source_id,
        snapshot_id,
        "@title",
        "title",
        "title",
        None,
        &capture.title,
    )?;
    for message in &capture.messages {
        insert_parts(
            tx,
            source_id,
            snapshot_id,
            &message.id,
            "message",
            &message.role,
            message.model.as_deref(),
            &message.text,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn insert_parts(
    tx: &Transaction<'_>,
    source_id: &str,
    snapshot_id: &str,
    message_id: &str,
    kind: &str,
    role: &str,
    model: Option<&str>,
    text: &str,
) -> Result<()> {
    let mut offset = 0;
    let mut part = 0i64;
    while offset < text.len() {
        let mut end = (offset + MAX_CHUNK_BYTES).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        if end == offset {
            return Err(IndexError::InvalidEvidence);
        }
        let chunk = &text[offset..end];
        if !chunk.trim().is_empty() {
            tx.execute(
                "INSERT INTO chunks(source_id,snapshot_id,message_id,part,kind,role,model,body)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    source_id,
                    snapshot_id,
                    message_id,
                    part,
                    kind,
                    role,
                    model,
                    chunk
                ],
            )?;
        }
        offset = end;
        part += 1;
    }
    Ok(())
}

//! MP-4F: rebuildable document-only FTS projection. Never an authority source.
//! Only explicit owner-triggered rescans create generations; searches revalidate
//! current authoritative heads AND live source bytes before disclosing snippets.
use crate::{configure, safe_path, IndexError, Result};
use mind_place_documents::{extract_with_check, DocumentFormat, ExtractionError, ExtractionLimits};
use mind_place_state::{
    DocumentObjectStore, DocumentScanner, ScanPolicy, StateError, StateStore, VirtualDocumentPath,
};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

const APPLICATION_ID: i64 = 0x4D504446; // MPDF; never confused with conversation index.
const MAX_RESULTS: usize = 25;
const MAX_CANDIDATES: i64 = 100;

const SCHEMA: &str = "
CREATE VIRTUAL TABLE document_fts USING fts5(
  root_id UNINDEXED, uri UNINDEXED, version_id UNINDEXED,
  section UNINDEXED, content, tokenize='unicode61'
);
CREATE TABLE document_meta(root_id TEXT PRIMARY KEY, generation INTEGER NOT NULL);
";

#[derive(Clone, Debug)]
pub struct DocumentIndex {
    path: PathBuf,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct DocumentIndexReport {
    pub root_id: String,
    pub scanned_files: usize,
    pub indexed_files: usize,
    pub skipped_files: usize,
    pub indexed_chunks: usize,
    pub generation: i64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct DocumentHit {
    pub uri: String,
    pub version_id: String,
    pub section: String,
    pub snippet: String,
    pub score: f64,
}

fn lease(check: &mut impl FnMut() -> Result<()>) -> std::result::Result<(), StateError> {
    check().map_err(|_| StateError::DocumentDenied)
}

fn format_for_uri(uri: &str) -> Option<DocumentFormat> {
    let path = VirtualDocumentPath::parse(uri).ok()?;
    let filename = path.segments().last()?;
    let ext = filename.rsplit_once('.')?.1;
    DocumentFormat::from_extension(ext)
}

/// Phrase-quote the entire user input. Never interpolate raw input into FTS SQL.
fn literal_query(input: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed.is_empty()
        || trimmed.len() > 256
        || trimmed
            .chars()
            .any(|c| c.is_control() && !c.is_whitespace())
    {
        return Err(IndexError::InvalidQuery);
    }
    Ok(format!("\"{}\"", trimmed.replace('"', "\"\"")))
}

impl DocumentIndex {
    /// Separate from the v1 conversation index: rebuildable without a schema migration.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_owned();
        let parent = path.parent().ok_or(IndexError::UnsafePath)?;
        safe_path(parent)?;
        fs::create_dir_all(parent)?;
        safe_path(&path)?;
        let existed = path.exists();
        let db = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        configure(&db)?;
        if !existed {
            db.execute_batch(SCHEMA)?;
            db.pragma_update(None, "application_id", APPLICATION_ID)?;
            db.pragma_update(None, "user_version", 1)?;
        }
        let id: i64 = db.query_row("PRAGMA application_id", [], |r| r.get(0))?;
        let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if id != APPLICATION_ID || version != 1 {
            return Err(IndexError::InvalidIndex);
        }
        let health: String = db.query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?;
        if health != "ok" {
            return Err(IndexError::InvalidIndex);
        }
        // Detect damaged or altered projections before using their derived text.
        let _: i64 = db
            .query_row("SELECT COUNT(*) FROM document_meta", [], |r| r.get(0))
            .map_err(|_| IndexError::InvalidIndex)?;
        Ok(Self { path })
    }

    fn connection(&self) -> Result<Connection> {
        safe_path(&self.path)?;
        let db = Connection::open_with_flags(
            &self.path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        configure(&db)?;
        let id: i64 = db.query_row("PRAGMA application_id", [], |r| r.get(0))?;
        if id != APPLICATION_ID {
            return Err(IndexError::InvalidIndex);
        }
        Ok(db)
    }

    /// All-or-nothing generation for one explicitly approved root.
    /// Never reads a private corpus until the owner has selected this command.
    pub fn refresh_root(
        &self,
        state: &StateStore,
        root_id: &str,
        mut check: impl FnMut() -> Result<()>,
    ) -> Result<DocumentIndexReport> {
        check()?;
        let mut scanner = DocumentScanner::new(state.clone(), ScanPolicy::default())?;
        let scan = scanner.rescan(root_id, || lease(&mut check))?;
        if !scan.complete {
            return Err(IndexError::InvalidEvidence);
        }
        let uris = scanner
            .current_uris(root_id, || lease(&mut check))?
            .ok_or(IndexError::InvalidEvidence)?;
        let objects = DocumentObjectStore::open(state.clone())?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("DELETE FROM document_fts WHERE root_id=?1", [root_id])?;
        let previous: Option<i64> = tx
            .query_row(
                "SELECT generation FROM document_meta WHERE root_id=?1",
                [root_id],
                |r| r.get(0),
            )
            .optional()?;
        let generation = previous
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(IndexError::InvalidIndex)?;
        let mut report = DocumentIndexReport {
            root_id: root_id.to_owned(),
            scanned_files: uris.len(),
            indexed_files: 0,
            skipped_files: 0,
            indexed_chunks: 0,
            generation,
        };
        for uri in uris {
            check()?;
            let Some(format) = format_for_uri(&uri) else {
                report.skipped_files += 1;
                continue;
            };
            let Some(head) = objects.current_version(&uri, || lease(&mut check))? else {
                return Err(IndexError::InvalidEvidence);
            };
            if head.byte_length > ExtractionLimits::default().max_input_bytes as u64 {
                report.skipped_files += 1;
                continue;
            }
            let bytes = objects.read_version(&uri, &head.id, || lease(&mut check))?;
            // A source can mutate after the scan; a version alone never proves
            // the indexed bytes are still the live approved file.
            let live =
                state.read_document_bytes(&uri, ExtractionLimits::default().max_input_bytes)?;
            if Sha256::digest(&live).as_slice() != Sha256::digest(&bytes).as_slice() {
                return Err(IndexError::InvalidEvidence);
            }
            let extracted = extract_with_check(format, &bytes, ExtractionLimits::default(), || {
                check().is_ok() && state.require_approved_document_uri(&uri).is_ok()
            });
            let extracted = match extracted {
                Ok(value) => value,
                Err(ExtractionError::Revoked) => return Err(IndexError::Revoked),
                Err(
                    ExtractionError::Invalid
                    | ExtractionError::Unsupported
                    | ExtractionError::Limit(_),
                ) => {
                    report.skipped_files += 1;
                    continue;
                }
            };
            if objects
                .current_version(&uri, || lease(&mut check))?
                .map(|v| v.id)
                != Some(head.id.clone())
            {
                return Err(IndexError::InvalidEvidence);
            }
            for chunk in extracted.chunks {
                check()?;
                tx.execute(
                    "INSERT INTO document_fts(root_id,uri,version_id,section,content)
                     VALUES(?1,?2,?3,?4,?5)",
                    params![root_id, uri, head.id, chunk.section, chunk.text],
                )?;
                report.indexed_chunks += 1;
            }
            report.indexed_files += 1;
        }
        check()?;
        state
            .document_root(root_id)?
            .filter(|r| r.is_approved())
            .ok_or(IndexError::Revoked)?;
        // Root permission + native identity must remain live at the commit boundary.
        scanner
            .current_uris(root_id, || lease(&mut check))?
            .ok_or(IndexError::Revoked)?;
        tx.execute(
            "INSERT INTO document_meta(root_id,generation) VALUES(?1,?2)
             ON CONFLICT(root_id) DO UPDATE SET generation=excluded.generation",
            params![root_id, generation],
        )?;
        check()?;
        tx.commit()?;
        check()?;
        Ok(report)
    }

    /// A candidate is untrusted derived text: require current head, owner grant,
    /// and current native file hash before returning it to the UI.
    pub fn search(
        &self,
        state: &StateStore,
        query: &str,
        limit: usize,
        mut check: impl FnMut() -> Result<()>,
    ) -> Result<Vec<DocumentHit>> {
        if limit == 0 || limit > MAX_RESULTS {
            return Err(IndexError::InvalidQuery);
        }
        let expression = literal_query(query)?;
        check()?;
        let db = self.connection()?;
        let mut statement = db.prepare(
            "SELECT uri,version_id,section,content,
                    snippet(document_fts,4,'[',']','…',12),
                    bm25(document_fts)
             FROM document_fts WHERE document_fts MATCH ?1
             ORDER BY bm25(document_fts), uri, version_id LIMIT ?2",
        )?;
        let rows = statement.query_map(params![expression, MAX_CANDIDATES], |r| {
            Ok((
                DocumentHit {
                    uri: r.get(0)?,
                    version_id: r.get(1)?,
                    section: r.get(2)?,
                    snippet: r.get(4)?,
                    score: r.get(5)?,
                },
                r.get::<_, String>(3)?,
            ))
        })?;
        let candidates: Vec<_> = rows.collect::<std::result::Result<_, _>>()?;
        drop(statement);
        drop(db);
        let objects = DocumentObjectStore::open(state.clone())?;
        let mut hits = Vec::new();
        // Bound verification effort even if 100 candidates match.
        let mut verified_bytes = 0usize;
        for (candidate, indexed_chunk) in candidates {
            check()?;
            let Some(head) = (match objects.current_version(&candidate.uri, || lease(&mut check)) {
                Ok(head) => head,
                Err(StateError::InvalidValue | StateError::DocumentDenied) => None,
                Err(error) => return Err(error.into()),
            }) else {
                continue;
            };
            if head.id != candidate.version_id {
                continue;
            }
            if head.byte_length > ExtractionLimits::default().max_input_bytes as u64 {
                continue;
            }
            verified_bytes = verified_bytes.saturating_add(head.byte_length as usize);
            if verified_bytes > 64 * 1024 * 1024 {
                break;
            }
            // Prove the indexed immutable revision still matches authorized source bytes.
            let fresh = match state
                .read_document_bytes(&candidate.uri, ExtractionLimits::default().max_input_bytes)
            {
                Ok(bytes) => bytes,
                Err(
                    StateError::InvalidValue
                    | StateError::DocumentDenied
                    | StateError::DocumentChanged,
                ) => continue,
                Err(StateError::Io(_)) => continue, // Renamed/deleted files are not results.
                Err(error) => return Err(error.into()),
            };
            if fresh.len() as u64 != head.byte_length
                || format!("{:x}", Sha256::digest(&fresh)) != head.content_hash
            {
                continue;
            }
            // Derived FTS5 rows can be corrupted or edited. Re-extract from
            // the verified live bytes and require a byte-for-byte original chunk
            // before disclosing the FTS-generated snippet.
            let Some(format) = format_for_uri(&candidate.uri) else {
                continue;
            };
            let verified = extract_with_check(format, &fresh, ExtractionLimits::default(), || {
                check().is_ok() && state.require_approved_document_uri(&candidate.uri).is_ok()
            });
            let verified = match verified {
                Ok(value) => value,
                Err(ExtractionError::Revoked) => return Err(IndexError::Revoked),
                Err(_) => continue,
            };
            if !verified
                .chunks
                .iter()
                .any(|chunk| chunk.section == candidate.section && chunk.text == indexed_chunk)
            {
                continue;
            }
            check()?;
            if objects
                .current_version(&candidate.uri, || lease(&mut check))?
                .map(|v| v.id)
                != Some(candidate.version_id.clone())
            {
                continue;
            }
            hits.push(candidate);
            if hits.len() == limit {
                break;
            }
        }
        check()?;
        Ok(hits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn literal_fts5_does_not_accept_operators_or_empty_queries() {
        assert_eq!(literal_query("a OR b").unwrap(), "\"a OR b\"");
        assert_eq!(literal_query("x\"y").unwrap(), "\"x\"\"y\"");
        assert!(literal_query("").is_err());
        assert!(literal_query(&"x".repeat(257)).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn windows_owner_document_index_search_mutation_and_revocation() {
        let dir = tempfile::tempdir().unwrap();
        let state = StateStore::open(dir.path().join("authority/state.sqlite3")).unwrap();
        let folder = dir.path().join("approved");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("notes.md"), "alpha uniquephrase beta").unwrap();
        let root = state.register_document_root(&folder).unwrap();
        state.approve_document_root(&root.id).unwrap();
        let index = DocumentIndex::open(dir.path().join("document-index.sqlite3")).unwrap();
        let first = index.refresh_root(&state, &root.id, || Ok(())).unwrap();
        assert_eq!(first.indexed_files, 1);
        assert!(first.indexed_chunks > 0);
        assert_eq!(index.search(&state, "uniquephrase", 10, || Ok(())).unwrap().len(), 1);

        // A current file edit must suppress stale projections even prior to rescan.
        std::fs::write(folder.join("notes.md"), "completely changed").unwrap();
        assert!(index.search(&state, "uniquephrase", 10, || Ok(())).unwrap().is_empty());
        let second = index.refresh_root(&state, &root.id, || Ok(())).unwrap();
        assert_eq!(second.generation, first.generation + 1);
        assert!(index.search(&state, "uniquephrase", 10, || Ok(())).unwrap().is_empty());
        assert_eq!(index.search(&state, "changed", 10, || Ok(())).unwrap().len(), 1);
        state.revoke_document_root(&root.id).unwrap();
        assert!(index.search(&state, "changed", 10, || Ok(())).unwrap().is_empty());
    }

    #[test]
    fn unapproved_derived_fts_candidates_never_disclose() {
        let tmp = tempfile::tempdir().unwrap();
        let state = StateStore::open(tmp.path().join("state.sqlite3")).unwrap();
        let index = DocumentIndex::open(tmp.path().join("document-index.sqlite3")).unwrap();
        let db = index.connection().unwrap();
        let root = format!("root:v1:{}", "f".repeat(64));
        let uri = format!("mpdoc://root/{root}/private.md");
        db.execute(
            "INSERT INTO document_fts(root_id,uri,version_id,section,content)
             VALUES(?1,?2,?3,?4,?5)",
            params![root, uri, "docver:v1:fake", "secret", "confidential evidence"],
        ).unwrap();
        drop(db);
        assert!(index.search(&state, "confidential", 20, || Ok(())).unwrap().is_empty());
    }

    #[test]
    fn derived_index_is_distinct_and_queryable() {
        let tmp = tempfile::tempdir().unwrap();
        let idx = DocumentIndex::open(tmp.path().join("document-index.sqlite3")).unwrap();
        let db = idx.connection().unwrap();
        db.execute(
            "INSERT INTO document_fts(root_id,uri,version_id,section,content)
                     VALUES('root','mpdoc://root/x/a.md','v','title','a bounded hello example')",
            [],
        )
        .unwrap();
        let count: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM document_fts WHERE document_fts MATCH ?1",
                [literal_query("bounded hello").unwrap()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        assert!(DocumentIndex::open(tmp.path().join("document-index.sqlite3")).is_ok());
    }
}

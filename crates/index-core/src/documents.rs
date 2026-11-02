//! MP-4F: rebuildable document-only FTS projection. Never an authority source.
//! Only explicit owner-triggered rescans create generations; searches revalidate
//! current authoritative heads AND live source bytes before disclosing snippets.
use crate::{configure, safe_path, IndexError, Result};
use mind_place_documents::{
    extract_with_check, DocumentFormat, ExtractionError, ExtractionLimits, EXTRACTOR_REVISION,
};
use mind_place_security::{
    vault::CryptoVault, vault_fence::VaultWriterFence, SecurityGate, UnlockLease,
};
use mind_place_state::{
    CipherKey, DocumentObjectStore, DocumentScanner, ScanPolicy, StateError, StateStore,
    VirtualDocumentPath,
};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
    sync::Arc,
};

const APPLICATION_ID: i64 = 0x4D504446; // MPDF; never confused with conversation index.
const SCHEMA_VERSION: i64 = 2;
const MAX_RESULTS: usize = 25;
const MAX_CANDIDATES: i64 = 100;

const SCHEMA: &str = "
CREATE VIRTUAL TABLE document_fts USING fts5(
  root_id UNINDEXED, uri UNINDEXED, version_id UNINDEXED,
  section UNINDEXED, content, extractor_revision UNINDEXED,
  extractor_warnings_json UNINDEXED, tokenize='unicode61'
);
CREATE TABLE document_meta(root_id TEXT PRIMARY KEY, generation INTEGER NOT NULL);
";

#[derive(Clone, Debug)]
pub struct DocumentIndex {
    path: PathBuf,
    cipher_key: Option<std::sync::Arc<CipherKey>>,
    vault_documents: Option<VaultDocumentBinding>,
}
#[derive(Clone)]
struct VaultDocumentBinding {
    root: PathBuf,
    vault: Arc<CryptoVault>,
    gate: SecurityGate,
    lease: UnlockLease,
}
impl std::fmt::Debug for VaultDocumentBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("VaultDocumentBinding([redacted])")
    }
}

struct FencedDocumentConnection {
    db: Connection,
    _writer: Option<VaultWriterFence>,
}
impl Deref for FencedDocumentConnection {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        &self.db
    }
}
impl DerefMut for FencedDocumentConnection {
    fn deref_mut(&mut self) -> &mut Connection {
        &mut self.db
    }
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

/// Bounded, freshly verified owner-only extracted source preview.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct DocumentPreview {
    pub uri: String,
    pub version_id: String,
    pub text: String,
    pub truncated: bool,
    pub extractor_revision: String,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct DocumentHit {
    pub uri: String,
    pub version_id: String,
    pub section: String,
    pub snippet: String,
    pub score: f64,
    pub extractor_revision: String,
    pub warnings: Vec<String>,
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

fn verify_document_database(db: &Connection, check_integrity: bool) -> Result<()> {
    let id: i64 = db.query_row("PRAGMA application_id", [], |r| r.get(0))?;
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if id != APPLICATION_ID || version != SCHEMA_VERSION {
        return Err(IndexError::InvalidIndex);
    }
    if check_integrity {
        let health: String = db.query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?;
        if health != "ok" {
            return Err(IndexError::InvalidIndex);
        }
    }
    // Detect damaged or altered projections before using their derived text.
    let _: i64 = db
        .query_row("SELECT COUNT(*) FROM document_meta", [], |r| r.get(0))
        .map_err(|_| IndexError::InvalidIndex)?;
    Ok(())
}

impl DocumentIndex {
    /// Separate from the v1 conversation index: rebuildable without a schema migration.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_internal(path, None)
    }
    pub fn open_encrypted(
        path: impl AsRef<Path>,
        raw_key: [u8; 32],
        gate: &mind_place_security::SecurityGate,
        lease: mind_place_security::UnlockLease,
    ) -> Result<Self> {
        Self::open_internal(path, Some(CipherKey::new(raw_key, gate, lease)))
    }
    pub fn open_encrypted_with_objects(
        path: impl AsRef<Path>,
        raw_key: [u8; 32],
        gate: &SecurityGate,
        lease: UnlockLease,
        vault: Arc<CryptoVault>,
        root: impl AsRef<Path>,
    ) -> Result<Self> {
        let root = root.as_ref();
        if path.as_ref().parent() != Some(root) {
            return Err(IndexError::UnsafePath);
        }
        let mut index = Self::open_encrypted(path, raw_key, gate, lease)?;
        index.vault_documents = Some(VaultDocumentBinding {
            root: root.to_owned(),
            vault,
            gate: gate.clone(),
            lease,
        });
        Ok(index)
    }

    fn objects(&self, state: &StateStore) -> Result<DocumentObjectStore> {
        match &self.vault_documents {
            Some(binding) => Ok(DocumentObjectStore::open_encrypted(
                state.clone(),
                &binding.root,
                binding.vault.clone(),
                &binding.gate,
                binding.lease,
            )?),
            None => Ok(DocumentObjectStore::open(state.clone())?),
        }
    }

    fn scanner(&self, state: &StateStore) -> Result<DocumentScanner> {
        let policy = ScanPolicy::default();
        match &self.vault_documents {
            Some(binding) => Ok(DocumentScanner::new_encrypted(
                state.clone(),
                policy,
                &binding.root,
                binding.vault.clone(),
                &binding.gate,
                binding.lease,
            )?),
            None => Ok(DocumentScanner::new(state.clone(), policy)?),
        }
    }

    fn open_internal(
        path: impl AsRef<Path>,
        cipher_key: Option<std::sync::Arc<CipherKey>>,
    ) -> Result<Self> {
        let path = path.as_ref().to_owned();
        let parent = path.parent().ok_or(IndexError::UnsafePath)?;
        safe_path(parent)?;
        // Reject a legacy plaintext open path once the vault marker exists.
        if cipher_key.is_none() && parent.join("vault-v1.json").exists() {
            return Err(IndexError::ForeignDatabase);
        }
        fs::create_dir_all(parent)?;
        safe_path(&path)?;
        let _writer_guard = if cipher_key.is_some() && parent.join("vault-v1.json").exists() {
            Some(VaultWriterFence::writer(parent).map_err(|_| IndexError::InvalidIndex)?)
        } else {
            None
        };
        let existed = path.exists();
        let mut legacy_v1 = false;
        if existed {
            // Reject foreign/corrupt DB before any mutable open or migration.
            let ro = Connection::open_with_flags(
                &path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
            )?;
            if let Some(key) = cipher_key.as_ref() {
                key.unlock(&ro)?;
            }
            let id: i64 = ro.query_row("PRAGMA application_id", [], |r| r.get(0))?;
            let version: i64 = ro.query_row("PRAGMA user_version", [], |r| r.get(0))?;
            if id != APPLICATION_ID || !matches!(version, 1 | SCHEMA_VERSION) {
                return Err(IndexError::InvalidIndex);
            }
            if version == SCHEMA_VERSION {
                verify_document_database(&ro, true)?;
            } else {
                let health: String = ro.query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?;
                if health != "ok" {
                    return Err(IndexError::InvalidIndex);
                }
                // Only a known disposable v1 projection may be recreated.
                let _: i64 = ro
                    .query_row("SELECT COUNT(*) FROM document_meta", [], |r| r.get(0))
                    .map_err(|_| IndexError::InvalidIndex)?;
                legacy_v1 = true;
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
        configure(&db)?;
        if !existed {
            db.execute_batch(SCHEMA)?;
            db.pragma_update(None, "application_id", APPLICATION_ID)?;
            db.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        } else if legacy_v1 {
            // The old text projection was never raw evidence. Atomically rebuild
            // its schema without touching document objects or root authority.
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch("DROP TABLE document_fts; DROP TABLE document_meta;")?;
            tx.execute_batch(SCHEMA)?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            tx.commit()?;
        }
        verify_document_database(&db, false)?;
        Ok(Self {
            path,
            cipher_key,
            vault_documents: None,
        })
    }

    fn connection(&self) -> Result<FencedDocumentConnection> {
        safe_path(&self.path)?;
        let parent = self.path.parent().ok_or(IndexError::UnsafePath)?;
        let writer = if self.cipher_key.is_some() && parent.join("vault-v1.json").exists() {
            Some(VaultWriterFence::writer(parent).map_err(|_| IndexError::InvalidIndex)?)
        } else {
            None
        };
        let ro = Connection::open_with_flags(
            &self.path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        if let Some(key) = self.cipher_key.as_ref() {
            key.unlock(&ro)?;
        }
        verify_document_database(&ro, false)?;
        drop(ro);
        let db = Connection::open_with_flags(
            &self.path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        if let Some(key) = self.cipher_key.as_ref() {
            key.unlock(&db)?;
        }
        configure(&db)?;
        verify_document_database(&db, false)?;
        Ok(FencedDocumentConnection {
            db,
            _writer: writer,
        })
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
        // One approved-root generation spans scanner object/state publication
        // and the FTS transaction below. A per-connection fence alone permits
        // backup to cut between these two parts of the same generation.
        let parent = self.path.parent().ok_or(IndexError::UnsafePath)?;
        let _generation = if self.cipher_key.is_some() && parent.join("vault-v1.json").exists() {
            Some(VaultWriterFence::writer(parent).map_err(|_| IndexError::InvalidEvidence)?)
        } else {
            None
        };
        let mut scanner = self.scanner(state)?;
        let scan = scanner.rescan(root_id, || lease(&mut check))?;
        if !scan.complete {
            return Err(IndexError::InvalidEvidence);
        }
        let uris = scanner
            .current_uris(root_id, || lease(&mut check))?
            .ok_or(IndexError::InvalidEvidence)?;
        let objects = self.objects(state)?;
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
            let warnings_json = serde_json::to_string(&extracted.warnings)
                .map_err(|_| IndexError::InvalidEvidence)?;
            for chunk in extracted.chunks {
                check()?;
                tx.execute(
                    "INSERT INTO document_fts(root_id,uri,version_id,section,content,extractor_revision,extractor_warnings_json)
                     VALUES(?1,?2,?3,?4,?5,?6,?7)",
                    params![root_id, uri, head.id, chunk.section, chunk.text, EXTRACTOR_REVISION, warnings_json],
                )?;
                report.indexed_chunks += 1;
            }
            report.indexed_files += 1;
        }
        check()?;
        // Reconcile the latest inventory before the final authority fence.
        // Do not invoke the potentially blocking caller lease inside the fence.
        scanner
            .current_uris(root_id, || lease(&mut check))?
            .ok_or(IndexError::Revoked)?;
        tx.execute(
            "INSERT INTO document_meta(root_id,generation) VALUES(?1,?2)
             ON CONFLICT(root_id) DO UPDATE SET generation=excluded.generation",
            params![root_id, generation],
        )?;
        check()?;
        // The grant must still be live at publication. A concurrent revoke
        // cannot finish between this authority check and the FTS commit.
        state.with_approved_document_roots(&[root_id.to_owned()], || {
            tx.commit()?;
            Ok::<(), IndexError>(())
        })?;
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
                    bm25(document_fts),extractor_revision,extractor_warnings_json
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
                    extractor_revision: r.get(6)?,
                    warnings: serde_json::from_str(&r.get::<_, String>(7)?).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            7,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?,
                },
                r.get::<_, String>(3)?,
            ))
        })?;
        let candidates: Vec<_> = rows.collect::<std::result::Result<_, _>>()?;
        drop(statement);
        drop(db);
        let objects = self.objects(state)?;
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
            if candidate.extractor_revision != EXTRACTOR_REVISION
                || candidate.warnings != verified.warnings
            {
                continue;
            }
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
        if hits.is_empty() {
            return Ok(hits);
        }
        // A revoke can complete after a candidate's live-byte verification.
        // Revalidate EVERY root in the response while holding the shared
        // authority writer fence; fail closed even for a mixed-root result.
        let roots = hits
            .iter()
            .map(|hit| VirtualDocumentPath::parse(&hit.uri).map(|uri| uri.root_id().to_owned()))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        state.with_approved_document_roots(&roots, || Ok::<_, IndexError>(hits))
    }

    /// Preview an indexed version only. Never accepts an absolute native path,
    /// stale version, unapproved URI or unverified index/source bytes.
    pub fn preview(
        &self,
        state: &StateStore,
        uri: &str,
        version_id: &str,
        mut check: impl FnMut() -> Result<()>,
    ) -> Result<DocumentPreview> {
        const MAX_PREVIEW_CHARS: usize = 4096;
        check()?;
        let path = VirtualDocumentPath::parse(uri)?;
        let objects = self.objects(state)?;
        let head = objects
            .current_version(uri, || lease(&mut check))?
            .ok_or(IndexError::Revoked)?;
        if head.id != version_id
            || head.byte_length > ExtractionLimits::default().max_input_bytes as u64
        {
            return Err(IndexError::Revoked);
        }
        // Only documents in the published owner-triggered index can be previewed.
        // A head alone is not evidence of successful extraction or indexing.
        let db = self.connection()?;
        let indexed: i64 = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM document_fts WHERE uri=?1 AND version_id=?2)",
            params![uri, version_id],
            |row| row.get(0),
        )?;
        if indexed == 0 {
            return Err(IndexError::InvalidEvidence);
        }
        drop(db);
        // Re-read the current authorized file through NTFS handle containment,
        // not a cached FTS row or an arbitrary path supplied by the browser.
        let bytes = state.read_document_bytes(uri, ExtractionLimits::default().max_input_bytes)?;
        if bytes.len() as u64 != head.byte_length
            || format!("{:x}", Sha256::digest(&bytes)) != head.content_hash
        {
            return Err(IndexError::InvalidEvidence);
        }
        let format = format_for_uri(uri).ok_or(IndexError::InvalidQuery)?;
        let extracted = extract_with_check(format, &bytes, ExtractionLimits::default(), || {
            check().is_ok() && state.require_approved_document_uri(uri).is_ok()
        })
        .map_err(|error| match error {
            ExtractionError::Revoked => IndexError::Revoked,
            _ => IndexError::InvalidEvidence,
        })?;
        check()?;
        if objects
            .current_version(uri, || lease(&mut check))?
            .map(|version| version.id)
            != Some(head.id.clone())
        {
            return Err(IndexError::Revoked);
        }
        // Bounded UTF-8 output; never return unbounded original source bytes.
        let mut chars = extracted.text.chars();
        let text: String = chars.by_ref().take(MAX_PREVIEW_CHARS).collect();
        let truncated = chars.next().is_some();
        let preview = DocumentPreview {
            uri: uri.to_owned(),
            version_id: head.id,
            text,
            truncated,
            extractor_revision: EXTRACTOR_REVISION.to_owned(),
            warnings: extracted.warnings,
        };
        check()?;
        // Final disclosure fence serializes against a completed owner revoke.
        state.with_approved_document_roots(&[path.root_id().to_owned()], || {
            check()?;
            Ok::<_, IndexError>(preview)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    use std::sync::{Arc, Mutex};

    #[cfg(windows)]
    fn approved_fixture() -> (tempfile::TempDir, StateStore, DocumentIndex, String) {
        let dir = tempfile::tempdir().unwrap();
        let state = StateStore::open(dir.path().join("authority/state.sqlite3")).unwrap();
        let folder = dir.path().join("approved");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("notes.md"), "safefixture uniquephrase text").unwrap();
        let root = state.register_document_root(&folder).unwrap();
        state.approve_document_root(&root.id).unwrap();
        let index = DocumentIndex::open(dir.path().join("document-index.sqlite3")).unwrap();
        (dir, state, index, root.id)
    }

    #[cfg(windows)]
    #[test]
    fn preview_revalidates_current_source_revision_and_permission() {
        let (dir, state, index, root_id) = approved_fixture();
        index.refresh_root(&state, &root_id, || Ok(())).unwrap();
        let hits = index.search(&state, "uniquephrase", 1, || Ok(())).unwrap();
        let hit = &hits[0];
        let preview = index
            .preview(&state, &hit.uri, &hit.version_id, || Ok(()))
            .unwrap();
        assert!(preview.text.contains("uniquephrase"));
        assert!(!preview.truncated);
        assert_eq!(preview.version_id, hit.version_id);
        assert!(index
            .preview(&state, &hit.uri, "docver:v1:stale", || Ok(()))
            .is_err());

        // Changing the authorized live bytes invalidates the saved head hash.
        std::fs::write(dir.path().join("approved").join("notes.md"), "changed").unwrap();
        assert!(index
            .preview(&state, &hit.uri, &hit.version_id, || Ok(()))
            .is_err());

        state.revoke_document_root(&root_id).unwrap();
        assert!(index
            .preview(&state, &hit.uri, &hit.version_id, || Ok(()))
            .is_err());
    }

    #[cfg(windows)]
    #[test]
    fn revoke_on_second_thread_at_final_search_lease_never_discloses() {
        let (_dir, state, index, root_id) = approved_fixture();
        index.refresh_root(&state, &root_id, || Ok(())).unwrap();
        let mut total = 0;
        assert_eq!(
            index
                .search(&state, "uniquephrase", 10, || {
                    total += 1;
                    Ok(())
                })
                .unwrap()
                .len(),
            1
        );
        let mut checkpoint = 0;
        let mut revoked = false;
        let result = index.search(&state, "uniquephrase", 10, || {
            checkpoint += 1;
            if checkpoint == total {
                let db = state.clone();
                let id = root_id.clone();
                std::thread::spawn(move || db.revoke_document_root(&id).unwrap())
                    .join()
                    .unwrap();
                revoked = true;
            }
            Ok(())
        });
        assert!(revoked, "the final search boundary was not exercised");
        assert!(
            !matches!(result, Ok(ref hits) if !hits.is_empty()),
            "completed root revoke disclosed a hit"
        );
        assert!(index
            .search(&state, "uniquephrase", 10, || Ok(()))
            .unwrap()
            .is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn revoke_on_second_thread_at_final_refresh_lease_cannot_commit_generation() {
        let (_dir, state, index, root_id) = approved_fixture();
        let mut total = 0;
        let first = index
            .refresh_root(&state, &root_id, || {
                total += 1;
                Ok(())
            })
            .unwrap();
        let mut checkpoint = 0;
        let mut revoked = false;
        let result = index.refresh_root(&state, &root_id, || {
            checkpoint += 1;
            if checkpoint == total {
                let db = state.clone();
                let id = root_id.clone();
                std::thread::spawn(move || db.revoke_document_root(&id).unwrap())
                    .join()
                    .unwrap();
                revoked = true;
            }
            Ok(())
        });
        assert!(revoked, "the final commit boundary was not exercised");
        assert!(
            result.is_err(),
            "revocation must abort projection publication"
        );
        let generation: i64 = index
            .connection()
            .unwrap()
            .query_row(
                "SELECT generation FROM document_meta WHERE root_id=?1",
                [&root_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(generation, first.generation);
    }

    #[cfg(windows)]
    #[test]
    fn final_disclosure_fence_denies_mixed_root_response_after_revoke() {
        let (dir, state, index, first_id) = approved_fixture();
        let second_folder = dir.path().join("second");
        std::fs::create_dir(&second_folder).unwrap();
        std::fs::write(second_folder.join("notes.md"), "another uniquephrase").unwrap();
        let second = state.register_document_root(&second_folder).unwrap();
        state.approve_document_root(&second.id).unwrap();
        index.refresh_root(&state, &first_id, || Ok(())).unwrap();
        index.refresh_root(&state, &second.id, || Ok(())).unwrap();
        assert_eq!(
            index
                .search(&state, "uniquephrase", 10, || Ok(()))
                .unwrap()
                .len(),
            2
        );
        let last_guard = Arc::new(Mutex::new(0usize));
        let mut total = 0;
        index
            .search(&state, "uniquephrase", 10, || {
                total += 1;
                Ok(())
            })
            .unwrap();
        let counter = last_guard.clone();
        let result = index.search(&state, "uniquephrase", 10, || {
            let mut calls = counter.lock().unwrap();
            *calls += 1;
            if *calls == total {
                let db = state.clone();
                let id = first_id.clone();
                std::thread::spawn(move || db.revoke_document_root(&id).unwrap())
                    .join()
                    .unwrap();
            }
            Ok(())
        });
        assert!(
            !matches!(result, Ok(ref hits) if hits.iter().any(|h| h.uri.contains(&first_id))),
            "mixed result included a revoked root"
        );
        let remaining = index.search(&state, "uniquephrase", 10, || Ok(())).unwrap();
        assert_eq!(remaining.len(), 1);
        assert!(remaining[0].uri.contains(&second.id));
    }

    #[test]
    fn legacy_document_projection_rebuilds_without_touching_source_data() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("document-index.sqlite3");
        let db = Connection::open(&path).unwrap();
        db.execute_batch(
            "CREATE VIRTUAL TABLE document_fts USING fts5(
                root_id UNINDEXED, uri UNINDEXED, version_id UNINDEXED,
                section UNINDEXED, content, tokenize='unicode61'
            );
            CREATE TABLE document_meta(root_id TEXT PRIMARY KEY, generation INTEGER NOT NULL);
            INSERT INTO document_meta VALUES('legacy', 1);",
        )
        .unwrap();
        db.pragma_update(None, "application_id", APPLICATION_ID)
            .unwrap();
        db.pragma_update(None, "user_version", 1).unwrap();
        drop(db);
        let idx = DocumentIndex::open(&path).unwrap();
        let connection = idx.connection().unwrap();
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        let old_roots: i64 = connection
            .query_row("SELECT COUNT(*) FROM document_meta", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        assert_eq!(old_roots, 0);
        assert!(DocumentIndex::open(path).is_ok());
    }

    #[test]
    fn refuses_foreign_sqlite_without_changing_journal_or_identity() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("document-index.sqlite3");
        let foreign = Connection::open(&path).unwrap();
        foreign
            .pragma_update(None, "application_id", 123_i64)
            .unwrap();
        foreign.pragma_update(None, "user_version", 5_i64).unwrap();
        foreign
            .execute_batch("CREATE TABLE private_evidence(secret TEXT);")
            .unwrap();
        drop(foreign);
        assert!(matches!(
            DocumentIndex::open(&path),
            Err(IndexError::InvalidIndex)
        ));
        let ro = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let id: i64 = ro
            .query_row("PRAGMA application_id", [], |r| r.get(0))
            .unwrap();
        let version: i64 = ro
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        let journal: String = ro
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!((id, version, journal.as_str()), (123, 5, "delete"));
    }

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
        let initial = index.search(&state, "uniquephrase", 10, || Ok(())).unwrap();
        assert_eq!(initial.len(), 1);
        assert_eq!(initial[0].extractor_revision, EXTRACTOR_REVISION);
        assert!(initial[0].warnings.is_empty());
        // Malformed or forged parser provenance must never be returned.
        let conn = index.connection().unwrap();
        conn.execute("UPDATE document_fts SET extractor_revision='forged'", [])
            .unwrap();
        assert!(index
            .search(&state, "uniquephrase", 10, || Ok(()))
            .unwrap()
            .is_empty());
        conn.execute(
            "UPDATE document_fts SET extractor_revision=?1, extractor_warnings_json='[\"forged\"]'",
            [EXTRACTOR_REVISION],
        )
        .unwrap();
        assert!(index
            .search(&state, "uniquephrase", 10, || Ok(()))
            .unwrap()
            .is_empty());
        drop(conn);

        // A current file edit must suppress stale projections even prior to rescan.
        std::fs::write(folder.join("notes.md"), "completely changed").unwrap();
        assert!(index
            .search(&state, "uniquephrase", 10, || Ok(()))
            .unwrap()
            .is_empty());
        let second = index.refresh_root(&state, &root.id, || Ok(())).unwrap();
        assert_eq!(second.generation, first.generation + 1);
        assert!(index
            .search(&state, "uniquephrase", 10, || Ok(()))
            .unwrap()
            .is_empty());
        assert_eq!(
            index
                .search(&state, "changed", 10, || Ok(()))
                .unwrap()
                .len(),
            1
        );
        state.revoke_document_root(&root.id).unwrap();
        assert!(index
            .search(&state, "changed", 10, || Ok(()))
            .unwrap()
            .is_empty());
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
            "INSERT INTO document_fts(root_id,uri,version_id,section,content,extractor_revision,extractor_warnings_json)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                root, uri, "docver:v1:fake", "secret",
                "confidential evidence", EXTRACTOR_REVISION, "[]"
            ],
        )
        .unwrap();
        drop(db);
        assert!(index
            .search(&state, "confidential", 20, || Ok(()))
            .unwrap()
            .is_empty());
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

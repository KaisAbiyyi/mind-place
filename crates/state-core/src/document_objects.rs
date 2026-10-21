//! Version 1 raw document bytes. Internal trusted API: callers authorize their
//! source handles before supplying a reader and revalidate unlock/source state
//! in the guard. Guards must only validate: never mutate StateStore or acquire
//! writer transactions, because they also run inside an authority transaction.
//! Virtual URIs never become source filesystem paths here.
use crate::{safe_path, Result, StateError, StateStore, VirtualDocumentPath};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub const MAX_DOCUMENT_OBJECT_BYTES: u64 = 64 * 1024 * 1024;
const BUFFER_BYTES: usize = 64 * 1024;

/// Opaque metadata observed on an authorized source handle. Equality is an
/// admission optimization, not proof of byte equality or filesystem authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentMetadata {
    pub file_identity: String,
    pub byte_length: u64,
    pub modified_ns: u64,
    pub changed_ns: u64,
}
impl DocumentMetadata {
    fn encode(&self, limit: u64) -> Result<String> {
        if self.byte_length > limit {
            return Err(StateError::DocumentLimit);
        }
        if self.file_identity.is_empty()
            || self.file_identity.len() > 256
            || self.file_identity.chars().any(char::is_control)
        {
            return Err(StateError::InvalidValue);
        }
        serde_json::to_string(self).map_err(|_| StateError::InvalidValue)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DocumentVersion {
    pub id: String,
    pub uri: String,
    pub content_hash: String,
    pub byte_length: u64,
}
#[derive(Clone, Debug, Serialize)]
pub struct DocumentPreserveReceipt {
    pub version: DocumentVersion,
    pub new_version: bool,
    pub reused_object: bool,
}
#[derive(Clone, Debug)]
pub struct DocumentObjectStore {
    state: StateStore,
    objects: PathBuf,
    limit: u64,
}

fn valid_hash(hash: &str) -> Result<()> {
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(StateError::ObjectCorrupt);
    }
    Ok(())
}
fn version_id(uri: &str, hash: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"mind-place/document-version/v1\0");
    for part in [uri, hash] {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part.as_bytes());
    }
    format!("docver:v1:{:x}", digest.finalize())
}
fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DocumentVersion> {
    Ok(DocumentVersion {
        id: row.get(0)?,
        uri: row.get(1)?,
        content_hash: row.get(2)?,
        byte_length: u64::from(row.get::<_, u32>(3)?),
    })
}
fn validate_version(version: &DocumentVersion) -> Result<()> {
    valid_hash(&version.content_hash)?;
    VirtualDocumentPath::parse(&version.uri).map_err(|_| StateError::ObjectCorrupt)?;
    if version.id != version_id(&version.uri, &version.content_hash)
        || version.byte_length > MAX_DOCUMENT_OBJECT_BYTES
    {
        return Err(StateError::ObjectCorrupt);
    }
    Ok(())
}
fn sync_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    // Windows file sync + atomic no-clobber publication is exercised by cloud
    // Windows CI. Power-loss directory-entry durability needs native qualification.
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
fn ensure_dir(path: &Path) -> Result<()> {
    safe_path(path)?;
    if !path.exists() {
        let parent = path.parent().ok_or(StateError::UnsafePath)?;
        ensure_dir(parent)?;
        match fs::create_dir(path) {
            Ok(()) => {
                sync_dir(parent)?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e.into()),
        }
    }
    safe_path(path)?;
    if !fs::metadata(path)?.is_dir() {
        return Err(StateError::UnsafePath);
    }
    Ok(())
}
/// Recheck under the same immediate SQLite transaction that registers evidence.
/// Revocation cannot interleave with the authority commit.
fn active_in_transaction(db: &Connection, root: &str) -> Result<()> {
    let active: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM document_root_registry d
         JOIN source_roots r ON r.id=d.root_id
         JOIN document_root_authorizations a ON a.root_id=d.root_id
         WHERE d.root_id=?1 AND r.approval_state='approved'
         AND NOT EXISTS(SELECT 1 FROM root_tombstones t WHERE t.root_id=d.root_id))",
        [root],
        |r| r.get(0),
    )?;
    if !active {
        return Err(StateError::DocumentDenied);
    }
    Ok(())
}

impl DocumentObjectStore {
    /// No I/O or disclosure on construction. Operations require an unlock guard.
    pub fn open(state: StateStore) -> Result<Self> {
        Self::with_limit(state, MAX_DOCUMENT_OBJECT_BYTES)
    }
    pub fn with_limit(state: StateStore, limit: u64) -> Result<Self> {
        if limit == 0 || limit > MAX_DOCUMENT_OBJECT_BYTES {
            return Err(StateError::DocumentLimit);
        }
        let objects = state
            .path()
            .parent()
            .ok_or(StateError::UnsafePath)?
            .join("document-objects/v1/sha256");
        Ok(Self {
            state,
            objects,
            limit,
        })
    }
    fn authorize(&self, uri: &str, guard: &mut impl FnMut() -> Result<()>) -> Result<String> {
        guard()?;
        let (_, path) = self
            .state
            .require_approved_document_uri(uri)
            .map_err(|e| match e {
                StateError::InvalidValue => StateError::DocumentDenied,
                other => other,
            })?;
        Ok(path.root_id().to_owned())
    }
    fn object_path(&self, hash: &str) -> Result<PathBuf> {
        valid_hash(hash)?;
        Ok(self.objects.join(&hash[..2]).join(hash))
    }
    fn check_file(&self, path: &Path, size: u64) -> Result<()> {
        safe_path(path)?;
        let meta = fs::metadata(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StateError::ObjectCorrupt
            } else {
                e.into()
            }
        })?;
        if !meta.is_file() || meta.len() != size {
            return Err(StateError::ObjectCorrupt);
        }
        Ok(())
    }
    /// Hash and size verification is mandatory before byte disclosure or reuse.
    fn verified_bytes(
        &self,
        hash: &str,
        size: u64,
        guard: &mut impl FnMut() -> Result<()>,
    ) -> Result<Vec<u8>> {
        if size > self.limit {
            return Err(StateError::DocumentLimit);
        }
        let path = self.object_path(hash)?;
        self.check_file(&path, size)?;
        let mut file = File::open(&path)?;
        let mut bytes = Vec::new();
        let mut digest = Sha256::new();
        let mut buf = [0; BUFFER_BYTES];
        loop {
            guard()?;
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            if bytes.len() as u64 + n as u64 > size {
                return Err(StateError::ObjectCorrupt);
            }
            digest.update(&buf[..n]);
            bytes.extend_from_slice(&buf[..n]);
        }
        if bytes.len() as u64 != size || format!("{:x}", digest.finalize()) != hash {
            return Err(StateError::ObjectCorrupt);
        }
        Ok(bytes)
    }
    /// Always hashes the supplied reader, even when metadata is unchanged.
    /// Guard must revalidate the unlock lease and, before committing, any source
    /// handle stability requirements. No partially read evidence is registered.
    pub fn preserve(
        &self,
        uri: &str,
        metadata: &DocumentMetadata,
        mut reader: impl Read,
        mut guard: impl FnMut() -> Result<()>,
    ) -> Result<DocumentPreserveReceipt> {
        let root = self.authorize(uri, &mut guard)?;
        let signature = metadata.encode(self.limit)?;
        ensure_dir(&self.objects)?;
        let mut temp = tempfile::NamedTempFile::new_in(&self.objects)?;
        let mut digest = Sha256::new();
        let mut length = 0u64;
        let mut buffer = [0; BUFFER_BYTES];
        loop {
            guard()?;
            let n = reader.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            length += n as u64;
            if length > self.limit {
                return Err(StateError::DocumentLimit);
            }
            if length > metadata.byte_length {
                return Err(StateError::DocumentChanged);
            }
            digest.update(&buffer[..n]);
            temp.write_all(&buffer[..n])?;
        }
        if length != metadata.byte_length {
            return Err(StateError::DocumentChanged);
        }
        self.authorize(uri, &mut guard)?;
        temp.as_file().sync_all()?;
        let hash = format!("{:x}", digest.finalize());
        let path = self.object_path(&hash)?;
        ensure_dir(path.parent().ok_or(StateError::UnsafePath)?)?;
        safe_path(&path)?;
        // Hold the authority transaction during publication, never during source
        // reads. Nonmutating guards fence lock/source changes across SQLite waits
        // and publication. Evidence may be orphaned on rollback; never GC.
        let mut db = self.state.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        guard()?;
        active_in_transaction(&tx, &root)?;
        let reused_object = match temp.persist_noclobber(&path) {
            Ok(file) => {
                file.sync_all()?;
                false
            }
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                // Reuse verification is bounded; the nonmutating guard before
                // commit revalidates the session/source after this work.
                self.verified_bytes(&hash, length, &mut || Ok(()))?;
                true
            }
            Err(error) => return Err(error.error.into()),
        };
        sync_dir(path.parent().ok_or(StateError::UnsafePath)?)?;
        tx.execute(
            "INSERT INTO document_objects(content_hash,format_version,byte_length)
            VALUES(?1,1,?2) ON CONFLICT(content_hash) DO NOTHING",
            params![hash, length as i64],
        )?;
        let saved: (i64, i64) = tx.query_row(
            "SELECT byte_length,format_version FROM document_objects WHERE content_hash=?1",
            [&hash],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if saved != (length as i64, 1) {
            return Err(StateError::ObjectCorrupt);
        }
        tx.execute(
            "INSERT INTO documents(uri,root_id) VALUES(?1,?2) ON CONFLICT(uri) DO NOTHING",
            params![uri, root],
        )?;
        let saved_root: String =
            tx.query_row("SELECT root_id FROM documents WHERE uri=?1", [uri], |r| {
                r.get(0)
            })?;
        if saved_root != root {
            return Err(StateError::ObjectCorrupt);
        }
        let id = version_id(uri, &hash);
        let new_version = tx.execute(
            "INSERT INTO document_versions(id,uri,content_hash,first_metadata_json)
            VALUES(?1,?2,?3,?4) ON CONFLICT(uri,content_hash) DO NOTHING",
            params![id, uri, hash, signature],
        )? == 1;
        let saved_id: String = tx.query_row(
            "SELECT id FROM document_versions WHERE uri=?1 AND content_hash=?2",
            params![uri, hash],
            |r| r.get(0),
        )?;
        if saved_id != id {
            return Err(StateError::ObjectCorrupt);
        }
        tx.execute("INSERT INTO document_heads(uri,version_id,metadata_json) VALUES(?1,?2,?3)
            ON CONFLICT(uri) DO UPDATE SET version_id=excluded.version_id,metadata_json=excluded.metadata_json,last_seen=unixepoch()",params![uri,id,signature])?;
        guard()?;
        active_in_transaction(&tx, &root)?;
        tx.commit()?;
        self.authorize(uri, &mut guard)?;
        Ok(DocumentPreserveReceipt {
            version: DocumentVersion {
                id,
                uri: uri.to_owned(),
                content_hash: hash,
                byte_length: length,
            },
            new_version,
            reused_object,
        })
    }
    /// Advisory no-content-read probe. Corrupt/missing object metadata is an
    /// error, never an unchanged success. Use preserve/read_version to hash.
    pub fn unchanged(
        &self,
        uri: &str,
        metadata: &DocumentMetadata,
        mut guard: impl FnMut() -> Result<()>,
    ) -> Result<Option<DocumentVersion>> {
        self.authorize(uri, &mut guard)?;
        let signature = metadata.encode(self.limit)?;
        let db = self.state.connection()?;
        let saved = db
            .query_row(
                "SELECT v.id,v.uri,v.content_hash,o.byte_length,h.metadata_json
            FROM document_heads h JOIN document_versions v ON v.id=h.version_id AND v.uri=h.uri
            JOIN document_objects o ON o.content_hash=v.content_hash
            WHERE h.uri=?1",
                [uri],
                |r| Ok((read_row(r)?, r.get::<_, String>(4)?)),
            )
            .optional()?;
        let result = match saved {
            Some((v, stored)) => {
                validate_version(&v)?;
                if stored == signature {
                    if v.byte_length != metadata.byte_length {
                        return Err(StateError::ObjectCorrupt);
                    }
                    self.check_file(&self.object_path(&v.content_hash)?, v.byte_length)?;
                    Some(v)
                } else {
                    None
                }
            }
            None => None,
        };
        self.authorize(uri, &mut guard)?;
        Ok(result)
    }
    /// Bounded, stable-ID pagination. No object-hash lookup API is exposed.
    pub fn versions(
        &self,
        uri: &str,
        after: Option<&str>,
        limit: usize,
        mut guard: impl FnMut() -> Result<()>,
    ) -> Result<Vec<DocumentVersion>> {
        self.authorize(uri, &mut guard)?;
        if limit == 0 || limit > 1000 {
            return Err(StateError::DocumentLimit);
        }
        if let Some(id) = after {
            crate::valid_id(id)?;
        }
        let db = self.state.connection()?;
        let mut stmt = db.prepare(
            "SELECT v.id,v.uri,v.content_hash,o.byte_length
            FROM document_versions v JOIN document_objects o ON o.content_hash=v.content_hash
            WHERE v.uri=?1 AND v.id>?2 ORDER BY v.id LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![uri, after.unwrap_or(""), limit as i64], read_row)?;
        let mut versions = Vec::new();
        for row in rows {
            let v = row?;
            validate_version(&v)?;
            versions.push(v);
        }
        self.authorize(uri, &mut guard)?;
        Ok(versions)
    }
    pub fn read_version(
        &self,
        uri: &str,
        id: &str,
        mut guard: impl FnMut() -> Result<()>,
    ) -> Result<Vec<u8>> {
        self.authorize(uri, &mut guard)?;
        crate::valid_id(id)?;
        let db = self.state.connection()?;
        let version = db
            .query_row(
                "SELECT v.id,v.uri,v.content_hash,o.byte_length
            FROM document_versions v JOIN document_objects o ON o.content_hash=v.content_hash
            WHERE v.uri=?1 AND v.id=?2",
                params![uri, id],
                read_row,
            )
            .optional()?
            .ok_or(StateError::InvalidValue)?;
        validate_version(&version)?;
        let bytes = self.verified_bytes(&version.content_hash, version.byte_length, &mut guard)?;
        self.authorize(uri, &mut guard)?;
        Ok(bytes)
    }
}

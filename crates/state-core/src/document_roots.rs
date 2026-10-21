//! MP-4A document roots are logical identities, never filesystem capabilities.
//! Phase 4B must revalidate an authorized directory handle before any file open.
use crate::{safe_path, Result, StateError, StateStore};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

pub const VIRTUAL_ROOT_PREFIX: &str = "mpdoc://root/";
const MAX_URI_BYTES: usize = 4096;
const MAX_COMPONENTS: usize = 128;
const MAX_COMPONENT_BYTES: usize = 255;
const ROOT_ID_PREFIX: &str = "root:v1:";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DocumentRoot {
    pub id: String,
    pub canonical_path: String,
    pub approval_state: String,
    pub virtual_prefix: String,
}

impl DocumentRoot {
    pub fn is_approved(&self) -> bool {
        self.approval_state == "approved"
    }
}

/// A logical address only. Never pass the relative path to File::open directly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VirtualDocumentPath {
    pub root_id: String,
    pub segments: Vec<String>,
}

impl VirtualDocumentPath {
    pub fn from_segments(root_id: &str, segments: &[String]) -> Result<Self> {
        validate_root_id(root_id)?;
        if segments.is_empty() || segments.len() > MAX_COMPONENTS {
            return Err(StateError::InvalidValue);
        }
        for segment in segments {
            validate_segment(segment)?;
        }
        let path = Self {
            root_id: root_id.to_owned(),
            segments: segments.to_vec(),
        };
        if path.to_uri().len() > MAX_URI_BYTES {
            return Err(StateError::InvalidValue);
        }
        Ok(path)
    }

    pub fn parse(uri: &str) -> Result<Self> {
        if uri.len() > MAX_URI_BYTES || !uri.starts_with(VIRTUAL_ROOT_PREFIX) {
            return Err(StateError::InvalidValue);
        }
        let mut pieces = uri[VIRTUAL_ROOT_PREFIX.len()..].split('/');
        let root_id = pieces.next().ok_or(StateError::InvalidValue)?;
        validate_root_id(root_id)?;
        let mut segments = Vec::new();
        for encoded in pieces {
            if encoded.is_empty() || segments.len() == MAX_COMPONENTS {
                return Err(StateError::InvalidValue);
            }
            let decoded = decode_segment(encoded)?;
            if encode_segment(&decoded) != encoded {
                // Noncanonical encodings and double decoding are forbidden.
                return Err(StateError::InvalidValue);
            }
            segments.push(decoded);
        }
        Self::from_segments(root_id, &segments)
    }

    pub fn to_uri(&self) -> String {
        let mut uri = format!("{VIRTUAL_ROOT_PREFIX}{}", self.root_id);
        for segment in &self.segments {
            uri.push('/');
            uri.push_str(&encode_segment(segment));
        }
        uri
    }
}

fn validate_root_id(id: &str) -> Result<()> {
    let Some(hex) = id.strip_prefix(ROOT_ID_PREFIX) else {
        return Err(StateError::InvalidValue);
    };
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) {
        return Err(StateError::InvalidValue);
    }
    Ok(())
}

fn root_digest(path: &str) -> String {
    // Length framing reserves an independent ID domain from archive source IDs.
    let mut hasher = Sha256::new();
    hasher.update(b"mind-place/document-root/v1\0");
    hasher.update((path.len() as u64).to_be_bytes());
    hasher.update(path.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn validate_segment(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > MAX_COMPONENT_BYTES ||
        value == "." || value == ".." ||
        value.ends_with('.') || value.ends_with(' ') ||
        value.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '%'))
    {
        return Err(StateError::InvalidValue);
    }
    let stem = value.split('.').next().unwrap_or("").to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$") ||
        (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) &&
            stem.as_bytes()[3].is_ascii_digit() && stem.as_bytes()[3] != b'0')
    {
        return Err(StateError::InvalidValue);
    }
    Ok(())
}

fn encode_segment(value: &str) -> String {
    let mut s = String::new();
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-') {
            s.push(char::from(*byte));
        } else {
            s.push('%');
            s.push(char::from(HEX[(byte >> 4) as usize]));
            s.push(char::from(HEX[(byte & 15) as usize]));
        }
    }
    s
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn decode_segment(raw: &str) -> Result<String> {
    let mut bytes = Vec::with_capacity(raw.len());
    let input = raw.as_bytes();
    let mut index = 0;
    while index < input.len() {
        if input[index] == b'%' {
            if index + 2 >= input.len() {
                return Err(StateError::InvalidValue);
            }
            let hi = hex_digit(input[index + 1]).ok_or(StateError::InvalidValue)?;
            let lo = hex_digit(input[index + 2]).ok_or(StateError::InvalidValue)?;
            bytes.push((hi << 4) | lo);
            index += 3;
        } else {
            if !input[index].is_ascii() {
                return Err(StateError::InvalidValue);
            }
            bytes.push(input[index]);
            index += 1;
        }
    }
    let decoded = String::from_utf8(bytes).map_err(|_| StateError::InvalidValue)?;
    validate_segment(&decoded)?;
    Ok(decoded)
}

fn row_root(row: &rusqlite::Row<'_>) -> rusqlite::Result<DocumentRoot> {
    let id: String = row.get(0)?;
    Ok(DocumentRoot {
        virtual_prefix: format!("{VIRTUAL_ROOT_PREFIX}{id}"),
        id,
        canonical_path: row.get(1)?,
        approval_state: row.get(2)?,
    })
}

fn validate_binding(root: &DocumentRoot, digest: &str) -> Result<()> {
    validate_root_id(&root.id).map_err(|_| StateError::Corrupt)?;
    let expected = root_digest(&root.canonical_path);
    if root.id != format!("{ROOT_ID_PREFIX}{expected}") || digest != expected {
        return Err(StateError::Corrupt);
    }
    Ok(())
}

impl StateStore {
    /// Explicitly stage an existing folder. This only examines path metadata.
    /// Staging never approves, scans, indexes, reads, or creates a filesystem grant.
    /// Caller (Tauri) must require a current authenticated unlock lease.
    pub fn register_document_root(&self, requested: &Path) -> Result<DocumentRoot> {
        if !requested.is_absolute() {
            return Err(StateError::InvalidValue);
        }
        safe_path(requested)?;
        let canonical = fs::canonicalize(requested)?;
        safe_path(&canonical)?;
        if !fs::metadata(&canonical)?.is_dir() {
            return Err(StateError::InvalidValue);
        }
        let path = canonical.to_str().ok_or(StateError::InvalidValue)?;
        if path.len() > 4096 || path.contains('\0') {
            return Err(StateError::InvalidValue);
        }
        let digest = root_digest(path);
        let id = format!("{ROOT_ID_PREFIX}{digest}");
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO source_roots(id,canonical_path,approval_state) VALUES(?1,?2,'pending') ON CONFLICT(id) DO NOTHING",
            params![id, path],
        )?;
        let saved: (String, String) = tx.query_row(
            "SELECT canonical_path,approval_state FROM source_roots WHERE id=?1",
            [&id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        // Terminal revocation cannot be silently undone by restaging.
        if saved.0 != path || saved.1 == "revoked" {
            return Err(StateError::InvalidValue);
        }
        tx.execute(
            "INSERT INTO document_root_registry(root_id,identity_version,path_digest) VALUES(?1,1,?2) ON CONFLICT(root_id) DO NOTHING",
            params![id, digest],
        )?;
        let stored_digest: String = tx.query_row(
            "SELECT path_digest FROM document_root_registry WHERE root_id=?1",
            [&id],
            |row| row.get(0),
        )?;
        if stored_digest != digest {
            return Err(StateError::Corrupt);
        }
        tx.commit()?;
        self.document_root(&id)?.ok_or(StateError::Corrupt)
    }

    /// Only explicitly enrolled root identities exist in the document namespace.
    pub fn document_root(&self, id: &str) -> Result<Option<DocumentRoot>> {
        validate_root_id(id)?;
        let db = self.connection()?;
        let found = db.query_row(
            "SELECT r.id,r.canonical_path,r.approval_state,d.path_digest
             FROM document_root_registry d JOIN source_roots r ON r.id=d.root_id
             WHERE d.root_id=?1",
            [id],
            |row| Ok((row_root(row)?, row.get::<_, String>(3)?)),
        ).optional()?;
        match found {
            Some((root, digest)) => {
                validate_binding(&root, &digest)?;
                Ok(Some(root))
            }
            None => Ok(None),
        }
    }

    /// Owner-only registry inventory; never exposed to a read-only MCP client.
    pub fn list_document_roots(&self) -> Result<Vec<DocumentRoot>> {
        let db = self.connection()?;
        let mut statement = db.prepare(
            "SELECT r.id,r.canonical_path,r.approval_state,d.path_digest
             FROM document_root_registry d JOIN source_roots r ON r.id=d.root_id ORDER BY r.id"
        )?;
        let rows = statement.query_map([], |row| Ok((row_root(row)?, row.get::<_, String>(3)?)))?;
        let mut roots = Vec::new();
        for row in rows {
            let (root, digest) = row?;
            validate_binding(&root, &digest)?;
            roots.push(root);
        }
        Ok(roots)
    }

    /// Strict registry lookup, not a path resolver/open primitive. MP-4B must
    /// validate current authorization and a filesystem HANDLE before reading.
    pub fn require_approved_document_uri(&self, uri: &str) -> Result<(DocumentRoot, VirtualDocumentPath)> {
        let virtual_path = VirtualDocumentPath::parse(uri)?;
        let root = self.document_root(&virtual_path.root_id)?.ok_or(StateError::InvalidValue)?;
        if !root.is_approved() {
            return Err(StateError::InvalidValue);
        }
        Ok((root, virtual_path))
    }
}

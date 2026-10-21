//! MP-4C bounded, opt-in document scanner and conservative watch reconciliation.
//!
//! This is an owner-unlock-only core API; there is no background scan on startup
//! and no MCP exposure. Watch notifications are hints, never completeness proof.
//! After restart, disconnect, overflow, rename, or revocation, a full rescan is
//! required before claiming an inventory current. All bytes come from 4B's
//! handle-relative NTFS open and enter 4D's immutable versioned object store.
//! Real NTFS watcher-loss/race qualification is intentionally a 4L1 gate.
use crate::{
    DocumentMetadata, DocumentObjectStore, Result, StateError, StateStore, VirtualDocumentPath,
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

const MAX_SCAN_ENTRIES: usize = 10_000;
const MAX_DEPTH: usize = 24;
const MAX_READ_BYTES: u64 = 32 * 1024 * 1024;
const REPARSE_POINT: u32 = 0x400;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanPolicy {
    pub max_files: usize,
    pub max_depth: usize,
    pub max_bytes_per_file: u64,
    pub extensions: BTreeSet<String>,
    pub excluded_dirs: BTreeSet<String>,
}
impl Default for ScanPolicy {
    fn default() -> Self {
        Self {
            max_files: MAX_SCAN_ENTRIES,
            max_depth: MAX_DEPTH,
            max_bytes_per_file: MAX_READ_BYTES,
            extensions: [
                "md", "markdown", "txt", "json", "jsonl", "pdf", "docx", "pptx",
                "xlsx", "rs", "ts", "tsx", "js", "jsx", "py", "go", "toml",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            excluded_dirs: [".git", ".cache", ".next", "node_modules", "target"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
        }
    }
}
impl ScanPolicy {
    fn validate(&self) -> Result<()> {
        if self.max_files == 0
            || self.max_files > MAX_SCAN_ENTRIES
            || self.max_depth == 0
            || self.max_depth > MAX_DEPTH
            || self.max_bytes_per_file == 0
            || self.max_bytes_per_file > MAX_READ_BYTES
            || self.extensions.len() > 128
            || self.extensions.iter().any(|v| {
                v.is_empty() || v.len() > 20 || !v.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            })
        {
            return Err(StateError::InvalidValue);
        }
        Ok(())
    }
    fn included(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|v| v.to_str())
            .map(|v| self.extensions.contains(&v.to_ascii_lowercase()))
            .unwrap_or(false)
    }
    fn excluded_dir(&self, name: &str) -> bool {
        self.excluded_dirs.contains(&name.to_ascii_lowercase()) || name.starts_with('.')
    }
}

/// Watchers are best-effort hints, not evidence that a folder is fully indexed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchEvent {
    Changed,
    Deleted,
    Renamed,
    Overflow,
    Disconnected,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ScanReport {
    pub root_id: String,
    pub visited_files: usize,
    pub new_versions: usize,
    pub unchanged_versions: usize,
    pub removed_paths: usize,
    pub excluded_paths: usize,
    pub complete: bool,
}

/// In-memory inventory only. It is never authority: on restart it starts dirty.
/// Tombstoned/removed paths never delete immutable bytes or evidence; MP-4F
/// must use a fresh inventory + live root check before disclosing search hits.
pub struct DocumentScanner {
    state: StateStore,
    objects: DocumentObjectStore,
    policy: ScanPolicy,
    known: BTreeMap<String, BTreeSet<String>>,
    dirty: BTreeSet<String>,
}

impl DocumentScanner {
    pub fn new(state: StateStore, policy: ScanPolicy) -> Result<Self> {
        policy.validate()?;
        let objects = DocumentObjectStore::open(state.clone())?;
        Ok(Self {
            state,
            objects,
            policy,
            known: BTreeMap::new(),
            dirty: BTreeSet::new(),
        })
    }

    /// Invalidation always requests a full reconciliation, even for one file.
    /// This deliberately avoids trusting lost, reordered or duplicate events.
    pub fn watch_hint(&mut self, root_id: &str, _event: WatchEvent) -> Result<()> {
        self.state.document_root(root_id)?.ok_or(StateError::DocumentDenied)?;
        self.dirty.insert(root_id.to_owned());
        Ok(())
    }
    pub fn needs_rescan(&self, root_id: &str) -> bool {
        self.dirty.contains(root_id) || !self.known.contains_key(root_id)
    }
    /// Clears the in-memory view immediately on revocation/lock notification.
    /// Durable root authority remains controlled solely by 4B.
    pub fn invalidate_root(&mut self, root_id: &str) {
        self.known.remove(root_id);
        self.dirty.insert(root_id.to_owned());
    }
    /// Never return a cached inventory without rechecking live root authority.
    pub fn current_uris(
        &self,
        root_id: &str,
        mut unlock_guard: impl FnMut() -> Result<()>,
    ) -> Result<Option<Vec<String>>> {
        unlock_guard()?;
        if self.needs_rescan(root_id) {
            return Ok(None);
        }
        let root = self.state.document_root(root_id)?.ok_or(StateError::DocumentDenied)?;
        if !root.is_approved() || self.state.authorized_binding(root_id).is_err() {
            return Err(StateError::DocumentDenied);
        }
        let result = self.known.get(root_id).map(|v| v.iter().cloned().collect());
        unlock_guard()?;
        self.state.authorized_binding(root_id).map_err(|_| StateError::DocumentDenied)?;
        Ok(result)
    }

    /// Complete bounded filesystem inventory. Directory listing is metadata-only,
    /// with no-follow filters; every file read uses NTFS handle-relative opens.
    /// Native enumeration/rename races still require MP-4L1 owner qualification.
    /// On overflow or scan failure, no "complete" inventory is published.
    pub fn rescan(
        &mut self,
        root_id: &str,
        mut unlock_guard: impl FnMut() -> Result<()>,
    ) -> Result<ScanReport> {
        self.dirty.insert(root_id.to_owned());
        unlock_guard()?;
        let root = self.state.document_root(root_id)?.ok_or(StateError::DocumentDenied)?;
        if !root.is_approved() {
            self.known.remove(root_id);
            return Err(StateError::DocumentDenied);
        }
        let expected = self.state.authorized_binding(root_id).map_err(|_| StateError::DocumentDenied)?;
        let root_path = Path::new(&root.canonical_path);
        if crate::ntfs::root_identity(root_path)? != expected {
            self.known.remove(root_id);
            return Err(StateError::DocumentDenied);
        }
        let mut discovered = BTreeSet::new();
        let mut excluded = 0usize;
        self.walk(root_path, &[], &mut discovered, &mut excluded, &mut unlock_guard)?;
        let mut report = ScanReport {
            root_id: root_id.to_owned(),
            visited_files: discovered.len(),
            new_versions: 0,
            unchanged_versions: 0,
            removed_paths: 0,
            excluded_paths: excluded,
            complete: false,
        };
        let mut next = BTreeSet::new();
        for segments in discovered {
            unlock_guard()?;
            let path = VirtualDocumentPath::from_segments(root_id, &segments)?;
            let uri = path.to_uri();
            let mut file = match crate::ntfs::open_document(root_path, expected, &segments) {
                Ok(f) => f,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    // Concurrent removal: keep dirty and force a subsequent pass.
                    return Err(StateError::DocumentChanged);
                }
                Err(e) => return Err(e.into()),
            };
            let attributes = file.metadata()?;
            if attributes.len() > self.policy.max_bytes_per_file {
                report.excluded_paths += 1;
                continue;
            }
            let metadata = DocumentMetadata {
                file_identity: crate::ntfs::document_file_identity(&file)?,
                byte_length: attributes.len(),
                modified_ns: attributes.modified().ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .and_then(|d| u64::try_from(d.as_nanos()).ok())
                    .unwrap_or(0),
                changed_ns: 0, // NTFS change-time requires separate native qualification.
            };
            let source = self.state.clone();
            let root_id = root_id.to_owned();
            let mut check = || {
                unlock_guard()?;
                source.authorized_binding(&root_id)
                    .map_err(|_| StateError::DocumentDenied)?;
                if crate::ntfs::root_identity(root_path)? != expected {
                    return Err(StateError::DocumentDenied);
                }
                Ok(())
            };
            let receipt = self.objects.preserve(&uri, &metadata, &mut file, &mut check)?;
            if receipt.new_version {
                report.new_versions += 1;
            } else {
                report.unchanged_versions += 1;
            }
            next.insert(uri);
        }
        unlock_guard()?;
        if crate::ntfs::root_identity(root_path)? != expected
            || self.state.authorized_binding(root_id).ok() != Some(expected)
        {
            self.known.remove(root_id);
            return Err(StateError::DocumentDenied);
        }
        report.removed_paths = self.known.get(root_id)
            .map(|old| old.difference(&next).count()).unwrap_or(0);
        report.complete = true;
        self.known.insert(root_id.to_owned(), next);
        self.dirty.remove(root_id);
        Ok(report)
    }

    fn walk(
        &self,
        directory: &Path,
        segments: &[String],
        found: &mut BTreeSet<Vec<String>>,
        excluded: &mut usize,
        unlock_guard: &mut impl FnMut() -> Result<()>,
    ) -> Result<()> {
        unlock_guard()?;
        if segments.len() > self.policy.max_depth {
            return Err(StateError::DocumentLimit);
        }
        // No-follow metadata checks avoid intentionally traversing reparse
        // points. All source bytes are still opened only through 4B handles.
        let md = fs::symlink_metadata(directory)?;
        if !md.is_dir() || md.file_type().is_symlink() || is_reparse(&md) {
            return Err(StateError::DocumentDenied);
        }
        let mut entries = fs::read_dir(directory)?.collect::<io::Result<Vec<_>>>()?;
        if entries.len() > self.policy.max_files || found.len() + entries.len() > self.policy.max_files {
            return Err(StateError::DocumentLimit);
        }
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            unlock_guard()?;
            let name = entry.file_name().into_string().map_err(|_| StateError::InvalidValue)?;
            let mut child = segments.to_vec();
            child.push(name.clone());
            let md = fs::symlink_metadata(entry.path())?;
            if md.file_type().is_symlink() || is_reparse(&md) {
                *excluded += 1;
            } else if md.is_dir() {
                if self.policy.excluded_dir(&name) {
                    *excluded += 1;
                } else {
                    self.walk(&entry.path(), &child, found, excluded, unlock_guard)?;
                }
            } else if md.is_file() && self.policy.included(Path::new(&name)) {
                VirtualDocumentPath::from_segments("_", &child).ok(); // validation below uses real root.
                found.insert(child);
                if found.len() > self.policy.max_files {
                    return Err(StateError::DocumentLimit);
                }
            } else {
                *excluded += 1;
            }
        }
        Ok(())
    }
}
#[cfg(windows)]
fn is_reparse(meta: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    meta.file_attributes() & REPARSE_POINT != 0
}
#[cfg(not(windows))]
fn is_reparse(_: &fs::Metadata) -> bool {
    let _ = REPARSE_POINT;
    false
}

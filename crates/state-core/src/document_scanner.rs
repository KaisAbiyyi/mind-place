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
    io,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

const MAX_SCAN_ENTRIES: usize = 10_000;
const MAX_DEPTH: usize = 24;
const MAX_READ_BYTES: u64 = 32 * 1024 * 1024;
const MAX_SCAN_TOTAL_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanPolicy {
    pub max_files: usize,
    pub max_depth: usize,
    pub max_bytes_per_file: u64,
    pub max_total_bytes: u64,
    pub extensions: BTreeSet<String>,
    pub excluded_dirs: BTreeSet<String>,
}
impl Default for ScanPolicy {
    fn default() -> Self {
        Self {
            max_files: MAX_SCAN_ENTRIES,
            max_depth: MAX_DEPTH,
            max_bytes_per_file: MAX_READ_BYTES,
            max_total_bytes: 512 * 1024 * 1024,
            extensions: [
                "md", "markdown", "txt", "json", "jsonl", "pdf", "docx", "pptx", "xlsx", "rs",
                "ts", "tsx", "js", "jsx", "py", "go", "toml",
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
            || self.max_total_bytes == 0
            || self.max_total_bytes > MAX_SCAN_TOTAL_BYTES
            || self.extensions.len() > 128
            || self.extensions.iter().any(|v| {
                v.is_empty()
                    || v.len() > 20
                    || !v
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
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
        self.state
            .document_root(root_id)?
            .ok_or(StateError::DocumentDenied)?;
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
        let root = self
            .state
            .document_root(root_id)?
            .ok_or(StateError::DocumentDenied)?;
        if !root.is_approved() {
            return Err(StateError::DocumentDenied);
        }
        let binding = self
            .state
            .authorized_binding(root_id)
            .map_err(|_| StateError::DocumentDenied)?;
        if crate::ntfs::root_identity(Path::new(&root.canonical_path))? != binding {
            return Err(StateError::DocumentDenied);
        }
        let result = self.known.get(root_id).map(|v| v.iter().cloned().collect());
        unlock_guard()?;
        self.state
            .authorized_binding(root_id)
            .map_err(|_| StateError::DocumentDenied)?;
        Ok(result)
    }

    /// Complete bounded filesystem inventory. Directory names and file bytes
    /// are both read from NTFS root-identity-verified relative handles.
    /// Native rename/event-loss stress still requires MP-4L1 qualification.
    /// On overflow or scan failure, no "complete" inventory is published.
    pub fn rescan(
        &mut self,
        root_id: &str,
        mut unlock_guard: impl FnMut() -> Result<()>,
    ) -> Result<ScanReport> {
        self.dirty.insert(root_id.to_owned());
        unlock_guard()?;
        let root = self
            .state
            .document_root(root_id)?
            .ok_or(StateError::DocumentDenied)?;
        if !root.is_approved() {
            self.known.remove(root_id);
            return Err(StateError::DocumentDenied);
        }
        let expected = self
            .state
            .authorized_binding(root_id)
            .map_err(|_| StateError::DocumentDenied)?;
        let root_path = Path::new(&root.canonical_path);
        if crate::ntfs::root_identity(root_path)? != expected {
            self.known.remove(root_id);
            return Err(StateError::DocumentDenied);
        }
        let mut discovered = BTreeSet::new();
        let mut excluded = 0usize;
        let mut visited_entries = 0usize;
        self.walk(
            root_path,
            expected,
            &[],
            &mut discovered,
            &mut excluded,
            &mut visited_entries,
            &mut unlock_guard,
        )?;
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
        let mut total_bytes = 0u64;
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
            total_bytes = total_bytes
                .checked_add(attributes.len())
                .ok_or(StateError::DocumentLimit)?;
            if total_bytes > self.policy.max_total_bytes {
                return Err(StateError::DocumentLimit);
            }
            let metadata = DocumentMetadata {
                file_identity: crate::ntfs::document_file_identity(&file)?,
                byte_length: attributes.len(),
                modified_ns: attributes
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .and_then(|d| u64::try_from(d.as_nanos()).ok())
                    .unwrap_or(0),
                changed_ns: 0, // NTFS change-time requires separate native qualification.
            };
            let source = self.state.clone();
            let root_id = root_id.to_owned();
            let probe = file.try_clone()?;
            let original_modified = attributes.modified().ok();
            let original_identity = metadata.file_identity.clone();
            let original_size = metadata.byte_length;
            let mut check = || {
                unlock_guard()?;
                source
                    .authorized_binding(&root_id)
                    .map_err(|_| StateError::DocumentDenied)?;
                if crate::ntfs::root_identity(root_path)? != expected {
                    return Err(StateError::DocumentDenied);
                }
                let live = probe.metadata()?;
                if live.len() != original_size
                    || live.modified().ok() != original_modified
                    || crate::ntfs::document_file_identity(&probe)? != original_identity
                {
                    return Err(StateError::DocumentChanged);
                }
                Ok(())
            };
            let receipt = self
                .objects
                .preserve(&uri, &metadata, &mut file, &mut check)?;
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
        report.removed_paths = self
            .known
            .get(root_id)
            .map(|old| old.difference(&next).count())
            .unwrap_or(0);
        report.complete = true;
        self.known.insert(root_id.to_owned(), next);
        self.dirty.remove(root_id);
        Ok(report)
    }

    fn walk(
        &self,
        root_path: &Path,
        expected: (u64, u64),
        segments: &[String],
        found: &mut BTreeSet<Vec<String>>,
        excluded: &mut usize,
        visited_entries: &mut usize,
        unlock_guard: &mut impl FnMut() -> Result<()>,
    ) -> Result<()> {
        unlock_guard()?;
        if segments.len() > self.policy.max_depth {
            return Err(StateError::DocumentLimit);
        }
        // Every listing is bound to an NT directory handle under the original
        // approved root identity. No path-based metadata/read_dir traversal.
        let entries = crate::ntfs::directory_entries(root_path, expected, segments)?;
        *visited_entries = visited_entries
            .checked_add(entries.len())
            .ok_or(StateError::DocumentLimit)?;
        if *visited_entries > self.policy.max_files {
            return Err(StateError::DocumentLimit);
        }
        for entry in entries {
            unlock_guard()?;
            let mut child = segments.to_vec();
            child.push(entry.name.clone());
            if entry.is_reparse {
                *excluded += 1;
            } else if entry.is_directory {
                if self.policy.excluded_dir(&entry.name) {
                    *excluded += 1;
                } else {
                    self.walk(
                        root_path,
                        expected,
                        &child,
                        found,
                        excluded,
                        visited_entries,
                        unlock_guard,
                    )?;
                }
            } else if self.policy.included(Path::new(&entry.name)) {
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

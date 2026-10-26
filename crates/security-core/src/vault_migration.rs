//! MP-9B legacy migration preflight: read-only evidence and change detection.
//! Never copy, encrypt, relocate, delete, or rewrite owner data here.
//! Approval, isolated conversion and atomic rollback are separate gates.
use crate::{validate_path, Result, SecurityError};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

const MAX_FILES: usize = 100_000;
const MAX_SINGLE_FILE: u64 = 8 * 1024 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024 * 1024;
const MAX_DEPTH: usize = 12;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyFileReceipt {
    pub relative: PathBuf,
    pub bytes: u64,
    pub sha256: String,
}

/// Intentionally non-serializable: a path inventory must never be exposed via
/// remote MCP, logged, or treated as proof of explicit migration consent.
pub struct LegacyMigrationPreflight {
    source: PathBuf,
    destination: PathBuf,
    files: Vec<LegacyFileReceipt>,
    total_bytes: u64,
}

impl LegacyMigrationPreflight {
    /// The caller selects an app-owned, pre-approved legacy root. The
    /// destination must be a distinct, non-existent directory.
    pub fn inspect(source: &Path, destination: &Path) -> Result<Self> {
        if !source.is_absolute() || !destination.is_absolute() {
            return Err(SecurityError::UnsafePath);
        }
        check(source)?;
        check(destination)?;
        if destination.exists() {
            return Err(SecurityError::AlreadyConfigured);
        }
        let canonical_source = source.canonicalize()?;
        if !canonical_source.is_dir() {
            return Err(SecurityError::InvalidConfig);
        }
        let destination_parent = destination.parent().ok_or(SecurityError::UnsafePath)?;
        let canonical_parent = destination_parent.canonicalize()?;
        // Existing sources inside the intended destination or the destination
        // itself must not be adopted. Destination may be a new child of the
        // app-owned root, but only explicitly whitelisted legacy sources scan.
        if canonical_source == destination
            || canonical_parent == destination
            || (canonical_parent.starts_with(&canonical_source)
                && canonical_parent != canonical_source)
        {
            return Err(SecurityError::UnsafePath);
        }
        let mut plan = Self {
            source: canonical_source,
            destination: destination.to_owned(),
            files: Vec::new(),
            total_bytes: 0,
        };
        plan.files = plan.inventory()?;
        plan.total_bytes = plan.files.iter().map(|item| item.bytes).sum();
        if plan.files.is_empty() {
            return Err(SecurityError::InvalidConfig);
        }
        Ok(plan)
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// Call immediately before any *separately authorized* conversion.
    /// New, missing or changed source files invalidate the original approval.
    /// No file is modified even when verification fails.
    pub fn verify_unchanged(&self) -> Result<()> {
        check(&self.source)?;
        check(&self.destination)?;
        if self.destination.exists() {
            return Err(SecurityError::AlreadyConfigured);
        }
        if self.inventory()? != self.files {
            return Err(SecurityError::Crypto);
        }
        Ok(())
    }

    fn inventory(&self) -> Result<Vec<LegacyFileReceipt>> {
        let mut receipts = Vec::new();
        let mut accumulated = 0u64;
        // No unknown files or WAL sidecars may be omitted from a proposed
        // migration. Old state WAL can hold committed records not in the DB.
        for entry in fs::read_dir(&self.source)? {
            let entry = entry?;
            let path = entry.path();
            check(&path)?;
            let name = entry.file_name();
            let name = name.to_str().ok_or(SecurityError::InvalidConfig)?;
            let metadata = fs::symlink_metadata(&path)?;
            match name {
                "state.sqlite3" if metadata.is_file() => {}
                "archive-v1" | "document-objects" if metadata.is_dir() => {}
                // Derived indexes are rebuildable, but an active WAL means
                // concurrent work and must block the migration preflight.
                "index.sqlite3" | "document-index.sqlite3" if metadata.is_file() => {}
                "security-v1.json" if metadata.is_file() => {}
                _ => return Err(SecurityError::InvalidConfig),
            }
        }
        // Derived FTS and security verifiers are excluded deliberately:
        // these must be rebuilt/reinitialized rather than accepted as keys.
        let state = self.source.join("state.sqlite3");
        if state.exists() {
            check(&state)?;
            let mut header = [0u8; 16];
            File::open(&state)?.read_exact(&mut header)?;
            if &header != b"SQLite format 3\0" {
                return Err(SecurityError::Crypto);
            }
            collect_file(&self.source, &state, &mut receipts, &mut accumulated)?;
        }
        for folder in ["archive-v1", "document-objects"] {
            let directory = self.source.join(folder);
            if directory.exists() {
                scan(&self.source, &directory, 0, &mut receipts, &mut accumulated)?;
            }
        }
        receipts.sort_by(|a, b| a.relative.cmp(&b.relative));
        Ok(receipts)
    }
}

fn check(path: &Path) -> Result<()> {
    for ancestor in path.ancestors() {
        if !ancestor.as_os_str().is_empty() {
            validate_path(ancestor)?;
        }
    }
    Ok(())
}

fn scan(
    root: &Path,
    directory: &Path,
    depth: usize,
    out: &mut Vec<LegacyFileReceipt>,
    accumulated: &mut u64,
) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err(SecurityError::InvalidConfig);
    }
    check(directory)?;
    if !fs::symlink_metadata(directory)?.is_dir() {
        return Err(SecurityError::UnsafePath);
    }
    for entry in fs::read_dir(directory)? {
        if out.len() >= MAX_FILES {
            return Err(SecurityError::InvalidConfig);
        }
        let child = entry?.path();
        check(&child)?;
        let metadata = fs::symlink_metadata(&child)?;
        if metadata.is_dir() {
            scan(root, &child, depth + 1, out, accumulated)?;
        } else if metadata.is_file() {
            collect_file(root, &child, out, accumulated)?;
        } else {
            return Err(SecurityError::UnsafePath);
        }
    }
    Ok(())
}

fn collect_file(
    root: &Path,
    path: &Path,
    out: &mut Vec<LegacyFileReceipt>,
    accumulated: &mut u64,
) -> Result<()> {
    check(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() > MAX_SINGLE_FILE || out.len() >= MAX_FILES {
        return Err(SecurityError::InvalidConfig);
    }
    let mut sha = Sha256::new();
    let mut file = File::open(path)?;
    let mut size = 0u64;
    let mut block = [0u8; 65536];
    loop {
        let amount = file.read(&mut block)?;
        if amount == 0 {
            break;
        }
        size = size
            .checked_add(amount as u64)
            .ok_or(SecurityError::Crypto)?;
        if size > MAX_SINGLE_FILE {
            return Err(SecurityError::InvalidConfig);
        }
        sha.update(&block[..amount]);
    }
    let next_total = accumulated.checked_add(size).ok_or(SecurityError::Crypto)?;
    if next_total > MAX_TOTAL_BYTES {
        return Err(SecurityError::InvalidConfig);
    }
    *accumulated = next_total;
    let relative = path
        .strip_prefix(root)
        .map_err(|_| SecurityError::UnsafePath)?;
    out.push(LegacyFileReceipt {
        relative: relative.to_owned(),
        bytes: size,
        sha256: format!("{:x}", sha.finalize()),
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_preflight_detects_changes_without_modifying_source() {
        let home = tempfile::tempdir().unwrap();
        let legacy = home.path().join("legacy");
        fs::create_dir_all(legacy.join("archive-v1/conversations")).unwrap();
        fs::create_dir_all(legacy.join("document-objects")).unwrap();
        fs::write(legacy.join("state.sqlite3"), b"SQLite format 3\0synthetic").unwrap();
        fs::write(
            legacy.join("archive-v1/conversations/snapshot.jsonl"),
            b"synthetic-chat",
        )
        .unwrap();
        fs::write(legacy.join("document-objects/blob"), b"synthetic-object").unwrap();
        let output = home.path().join("vault-new");
        let preflight = LegacyMigrationPreflight::inspect(&legacy, &output).unwrap();
        assert_eq!(preflight.file_count(), 3);
        assert!(preflight.total_bytes() > 0);
        preflight.verify_unchanged().unwrap();
        assert!(!output.exists());
        fs::write(
            legacy.join("archive-v1/conversations/snapshot.jsonl"),
            b"changed",
        )
        .unwrap();
        assert!(preflight.verify_unchanged().is_err());
        assert!(!output.exists());
        assert_eq!(
            fs::read(legacy.join("archive-v1/conversations/snapshot.jsonl")).unwrap(),
            b"changed"
        );
    }

    #[test]
    fn migration_preflight_rejects_ciphertext_unsafe_paths_and_existing_destination() {
        let home = tempfile::tempdir().unwrap();
        let legacy = home.path().join("legacy");
        fs::create_dir(&legacy).unwrap();
        let state = legacy.join("state.sqlite3");
        fs::write(&state, b"encrypted-fake").unwrap();
        let destination = home.path().join("vault-new");
        assert!(LegacyMigrationPreflight::inspect(&legacy, &destination).is_err());
        fs::write(&state, b"SQLite format 3\0synthetic").unwrap();
        fs::create_dir(&destination).unwrap();
        assert!(LegacyMigrationPreflight::inspect(&legacy, &destination).is_err());
    }

    #[test]
    fn unexpected_legacy_files_and_live_wal_are_never_silently_skipped() {
        let home = tempfile::tempdir().unwrap();
        let source = home.path().join("legacy");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("state.sqlite3"), b"SQLite format 3\0synthetic").unwrap();
        let output = home.path().join("new-vault");
        let plan = LegacyMigrationPreflight::inspect(&source, &output).unwrap();
        fs::write(source.join("state.sqlite3-wal"), b"committed rows").unwrap();
        assert!(plan.verify_unchanged().is_err());
        assert!(LegacyMigrationPreflight::inspect(&source, &output).is_err());
        fs::remove_file(source.join("state.sqlite3-wal")).unwrap();
        fs::write(source.join("owner-unknown.json"), b"sensitive").unwrap();
        assert!(LegacyMigrationPreflight::inspect(&source, &output).is_err());
        assert_eq!(
            fs::read(source.join("owner-unknown.json")).unwrap(),
            b"sensitive"
        );
        // Production stores the new encrypted-vault-v1 folder directly under
        // the existing app-owned legacy root. This is a safe, distinct child;
        // deeper unapproved nesting must still be refused.
        assert!(LegacyMigrationPreflight::inspect(&source, &source.join("vault")).is_err());
        fs::remove_file(source.join("owner-unknown.json")).unwrap();
        let nested = source.join("archive-v1/subdir/encrypted-vault-v1");
        assert!(LegacyMigrationPreflight::inspect(&source, &nested).is_err());
        let app_child = LegacyMigrationPreflight::inspect(&source, &source.join("vault")).unwrap();
        app_child.verify_unchanged().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_archive_is_never_inventoried() {
        use std::os::unix::fs::symlink;
        let home = tempfile::tempdir().unwrap();
        let legacy = home.path().join("legacy");
        fs::create_dir(&legacy).unwrap();
        fs::write(legacy.join("state.sqlite3"), b"SQLite format 3\0synthetic").unwrap();
        let outside = home.path().join("outside");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, legacy.join("archive-v1")).unwrap();
        assert!(LegacyMigrationPreflight::inspect(&legacy, &home.path().join("vault")).is_err());
    }
}

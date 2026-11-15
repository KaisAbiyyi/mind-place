//! MP-9B read-only preflight and authenticated owner-consented migration journal.
//! Conversion uses isolated staging; legacy source bytes are never rewritten.
//! Existing destinations require authenticated resume, never ordinary adoption.
use crate::vault::{ContentKind, CryptoVault};
use crate::{validate_path, Result, SecurityError, SecurityGate, UnlockLease};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;

use crate::vault_fs as durable;

const MAX_FILES: usize = 100_000;
const MAX_SINGLE_FILE: u64 = 8 * 1024 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024 * 1024;
const MAX_DEPTH: usize = 12;

/// The legacy ArchiveStore uses this same OS lock for every capture/import.
/// Exposed narrowly for the Windows offline owner maintenance runtime.
pub fn lock_legacy_archive(file: &File) -> Result<()> {
    FileExt::try_lock_exclusive(file)?;
    Ok(())
}

/// Shared no-link boundary for the explicit offline maintenance runtime.
pub fn validate_owner_migration_path(path: &Path) -> Result<()> {
    check(path)
}

const SQLITE_SHM_REGION_BYTES: usize = 32_768;
const SQLITE_WAL_INDEX_VERSION: u32 = 3_007_000;
const SQLITE_WAL_READMARK_NOT_USED: u32 = u32::MAX;
const SQLITE_SHM_CKPT_INFO_END: usize = 136;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EmptySidecarKind {
    Wal,
    Shm,
}

fn check_safe_regular_file(path: &Path) -> Result<()> {
    check(path)?;
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err(SecurityError::UnsafePath);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return Err(SecurityError::UnsafePath);
        }
    }
    Ok(())
}

/// Validates that `path` is a safe zero-byte SQLite WAL file without deleting
/// or mutating it. Any non-empty or non-regular WAL fails closed.
pub fn validate_empty_sqlite_wal_sidecar(path: &Path) -> Result<()> {
    check_safe_regular_file(path)?;
    if fs::symlink_metadata(path)?.len() != 0 {
        return Err(SecurityError::Crypto);
    }
    let mut file = File::open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() != 0 {
        return Err(SecurityError::Crypto);
    }
    let mut probe = [0u8; 1];
    if file.read(&mut probe)? != 0 {
        return Err(SecurityError::Crypto);
    }
    Ok(())
}

fn sqlite_wal_header_checksum(header_prefix: &[u8]) -> [u32; 2] {
    let mut s0: u32 = 0;
    let mut s1: u32 = 0;
    for chunk in header_prefix.chunks_exact(8) {
        let x0 = u32::from_ne_bytes(chunk[0..4].try_into().expect("4-byte chunk"));
        let x1 = u32::from_ne_bytes(chunk[4..8].try_into().expect("4-byte chunk"));
        s0 = s0.wrapping_add(x0).wrapping_add(s1);
        s1 = s1.wrapping_add(x1).wrapping_add(s0);
    }
    [s0, s1]
}

fn validate_empty_sqlite_shm_bytes(bytes: &[u8]) -> Result<()> {
    if bytes.len() != SQLITE_SHM_REGION_BYTES || bytes[..48] != bytes[48..96] {
        return Err(SecurityError::Crypto);
    }
    let version = u32::from_ne_bytes(bytes[0..4].try_into().map_err(|_| SecurityError::Crypto)?);
    let unused = u32::from_ne_bytes(bytes[4..8].try_into().map_err(|_| SecurityError::Crypto)?);
    let is_init = bytes[12];
    let big_end_cksum = bytes[13];
    let sz_page = u16::from_ne_bytes(
        bytes[14..16]
            .try_into()
            .map_err(|_| SecurityError::Crypto)?,
    );
    let mx_frame = u32::from_ne_bytes(
        bytes[16..20]
            .try_into()
            .map_err(|_| SecurityError::Crypto)?,
    );
    let n_page = u32::from_ne_bytes(
        bytes[20..24]
            .try_into()
            .map_err(|_| SecurityError::Crypto)?,
    );
    let valid_page_size = sz_page == 0
        || sz_page == 1
        || (sz_page >= 512 && sz_page <= 32_768 && sz_page.is_power_of_two());
    if version != SQLITE_WAL_INDEX_VERSION
        || unused != 0
        || is_init != 1
        || big_end_cksum > 1
        || !valid_page_size
        || mx_frame != 0
        || (sz_page == 0 && n_page != 0)
        || bytes[24..32] != [0u8; 8]
    {
        return Err(SecurityError::Crypto);
    }
    let expected_cksum = sqlite_wal_header_checksum(&bytes[0..40]);
    let actual_cksum = [
        u32::from_ne_bytes(
            bytes[40..44]
                .try_into()
                .map_err(|_| SecurityError::Crypto)?,
        ),
        u32::from_ne_bytes(
            bytes[44..48]
                .try_into()
                .map_err(|_| SecurityError::Crypto)?,
        ),
    ];
    if actual_cksum != expected_cksum {
        return Err(SecurityError::Crypto);
    }
    let n_backfill = u32::from_ne_bytes(
        bytes[96..100]
            .try_into()
            .map_err(|_| SecurityError::Crypto)?,
    );
    let mark0 = u32::from_ne_bytes(
        bytes[100..104]
            .try_into()
            .map_err(|_| SecurityError::Crypto)?,
    );
    if n_backfill != 0 || mark0 != 0 {
        return Err(SecurityError::Crypto);
    }
    for offset in [104usize, 108, 112, 116] {
        let mark = u32::from_ne_bytes(
            bytes[offset..offset + 4]
                .try_into()
                .map_err(|_| SecurityError::Crypto)?,
        );
        if mark != 0 && mark != SQLITE_WAL_READMARK_NOT_USED {
            return Err(SecurityError::Crypto);
        }
    }
    let n_backfill_attempted = u32::from_ne_bytes(
        bytes[128..132]
            .try_into()
            .map_err(|_| SecurityError::Crypto)?,
    );
    let not_used0 = u32::from_ne_bytes(
        bytes[132..136]
            .try_into()
            .map_err(|_| SecurityError::Crypto)?,
    );
    if bytes[120..128] != [0u8; 8] || n_backfill_attempted != 0 || not_used0 != 0 {
        return Err(SecurityError::Crypto);
    }
    if bytes[SQLITE_SHM_CKPT_INFO_END..]
        .iter()
        .any(|&byte| byte != 0)
    {
        return Err(SecurityError::Crypto);
    }
    Ok(())
}

/// Validates that `path` is a benign 32 KiB empty-WAL SQLite `-shm` index
/// left after read-only WAL inspection (zero frames, valid checksum, zero tail).
pub fn validate_empty_sqlite_shm_sidecar(path: &Path) -> Result<()> {
    check_safe_regular_file(path)?;
    if fs::symlink_metadata(path)?.len() != SQLITE_SHM_REGION_BYTES as u64 {
        return Err(SecurityError::Crypto);
    }
    let file = File::open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() != SQLITE_SHM_REGION_BYTES as u64 {
        return Err(SecurityError::Crypto);
    }
    let mut bytes = Vec::with_capacity(SQLITE_SHM_REGION_BYTES);
    file.take((SQLITE_SHM_REGION_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    validate_empty_sqlite_shm_bytes(&bytes)
}

fn benign_empty_sidecar_kind(
    relative: &Path,
    has_state_db: bool,
    has_archive_db: bool,
) -> Option<EmptySidecarKind> {
    if relative.components().count() != 1 {
        return None;
    }
    match relative.to_str()? {
        "state.sqlite3-wal" if has_state_db => Some(EmptySidecarKind::Wal),
        "state.sqlite3-shm" if has_state_db => Some(EmptySidecarKind::Shm),
        "archive-index.sqlite3-wal" if has_archive_db => Some(EmptySidecarKind::Wal),
        "archive-index.sqlite3-shm" if has_archive_db => Some(EmptySidecarKind::Shm),
        _ => None,
    }
}

fn digest_scanned_files(mut files: Vec<LegacyFileReceipt>) -> Result<String> {
    files.sort_by(|a, b| a.relative.cmp(&b.relative));
    let mut digest = Sha256::new();
    digest.update(b"mp9b-migration-output-v2\0");
    for file in files {
        let path = file.relative.to_str().ok_or(SecurityError::InvalidConfig)?;
        digest.update((path.len() as u64).to_be_bytes());
        digest.update(path.as_bytes());
        digest.update(file.bytes.to_be_bytes());
        digest.update(file.sha256.as_bytes());
    }
    Ok(format!("{:x}", digest.finalize()))
}

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
    /// Reconstruct an explicitly authorized migration after a process restart.
    /// Existing destinations are admitted only through the bound journal.
    pub fn resume_owner_consented(
        source: &Path,
        destination: &Path,
        _consent: &ExplicitMigrationConsent,
        gate: &SecurityGate,
        lease: UnlockLease,
        vault: &CryptoVault,
    ) -> Result<Self> {
        gate.validate_lease(lease)?;
        let plan = Self::inspect_paths(source, destination, true)?;
        let _operation = plan.operation_lock()?;
        let journal = plan.read_journal(gate, lease, vault)?;
        if plan.destination.exists() {
            plan.verify_published_destination_for_resume(&journal)?;
        } else if journal.phase == "published" || journal.phase == "rolled-back" {
            return Err(SecurityError::InvalidConfig);
        }
        Ok(plan)
    }

    /// The caller selects an app-owned, pre-approved legacy root. The
    /// destination must be a distinct, non-existent directory.
    pub fn inspect(source: &Path, destination: &Path) -> Result<Self> {
        Self::inspect_paths(source, destination, false)
    }

    /// Locate a recovery-wrapped manifest without adopting an existing target.
    /// The caller must still authenticate resume_owner_consented before acting.
    pub fn recovery_authorization_root(source: &Path, destination: &Path) -> Result<PathBuf> {
        Ok(Self::inspect_paths(source, destination, true)?.authorization_root())
    }

    fn inspect_paths(source: &Path, destination: &Path, recovering: bool) -> Result<Self> {
        if !source.is_absolute() || !destination.is_absolute() {
            return Err(SecurityError::UnsafePath);
        }
        check(source)?;
        check(destination)?;
        if destination.exists() && !recovering {
            return Err(SecurityError::AlreadyConfigured);
        }
        let canonical_source = source.canonicalize()?;
        if !canonical_source.is_dir() || canonical_source.parent().is_none() {
            return Err(SecurityError::InvalidConfig);
        }
        let destination_parent = destination.parent().ok_or(SecurityError::UnsafePath)?;
        let canonical_parent = destination_parent.canonicalize()?;
        let destination =
            canonical_parent.join(destination.file_name().ok_or(SecurityError::UnsafePath)?);
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
            destination,
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

    /// Original caller-selected root; never serialize into remote interfaces.
    pub fn source_root(&self) -> &Path {
        &self.source
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
            // Only this exact target is outside the legacy authority. During
            // recovery it is admitted only after the bound journal authenticates.
            if path == self.destination && metadata.is_dir() {
                continue;
            }
            match name {
                "state.sqlite3" if metadata.is_file() => {}
                "archive-v1" | "document-objects" if metadata.is_dir() => {}
                // Derived indexes are rebuildable, but an active WAL means
                // concurrent work and must block the migration preflight.
                "index.sqlite3" | "document-index.sqlite3" if metadata.is_file() => {}
                "security-v1.json" if metadata.is_file() => {}
                ".state-schema.lock" if metadata.is_file() && metadata.len() == 0 => {}
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
    // Windows byte-range locks also reject a zero-length EOF probe through
    // another handle. This exact empty sentinel has no source payload.
    let empty_archive_lock = metadata.len() == 0 && path.ends_with("archive-v1/.archive.lock");
    while !empty_archive_lock {
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

const JOURNAL_VERSION: u32 = 2;
const CONSENT_WORDS: &str = "ENCRYPT MY LEGACY VAULT";

/// UI must obtain explicit typed consent for the owner's data.
pub struct ExplicitMigrationConsent(());
impl ExplicitMigrationConsent {
    pub fn from_owner_typed_confirmation(input: &str) -> Result<Self> {
        if input != CONSENT_WORDS {
            return Err(SecurityError::InvalidCredentials);
        }
        Ok(Self(()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DurableMigrationJournal {
    version: u32,
    inventory_hash: String,
    phase: String,
    output_hash: Option<String>,
}

/// Safe interruption/progress boundaries. Returning an error preserves a
/// resumable journal/staging; observers never receive corpus contents or keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrationCheckpoint {
    ConversionComplete,
    Verified,
    Published,
}

impl LegacyMigrationPreflight {
    fn digest_inventory(&self) -> Result<String> {
        let mut h = Sha256::new();
        h.update(b"mp9b-legacy-source-inventory-v1\0");
        for r in &self.files {
            let relative = r.relative.to_str().ok_or(SecurityError::InvalidConfig)?;
            h.update(relative.as_bytes());
            h.update([0]);
            h.update(r.bytes.to_be_bytes());
            h.update(r.sha256.as_bytes());
            h.update([0]);
        }
        Ok(format!("{:x}", h.finalize()))
    }

    fn journal_path(&self) -> PathBuf {
        self.artifact_path("mp9b-migration-journal")
    }

    fn staging_path(&self) -> PathBuf {
        self.artifact_path("mp9b-migration-staging")
    }

    /// Wrapped manifest only, never a plaintext key. Owner runtime persists it
    /// after recovery confirmation so an interrupted conversion can reopen.
    pub fn authorization_root(&self) -> PathBuf {
        self.artifact_path("mp9b-migration-authorization")
    }

    fn artifact_path(&self, extension: &str) -> PathBuf {
        if self.destination.parent() != Some(self.source.as_path()) {
            return self.destination.with_extension(extension);
        }
        // The app vault is a child of the old root. Keep every working artifact
        // outside that root, using a length-framed canonical path identity.
        let mut hash = Sha256::new();
        hash.update(b"mp9b-migration-artifacts-v1\0");
        for path in [&self.source, &self.destination] {
            let bytes = path.as_os_str().as_encoded_bytes();
            hash.update((bytes.len() as u64).to_be_bytes());
            hash.update(bytes);
        }
        self.source
            .parent()
            .expect("a child destination has a parent")
            .join(format!(".mp9b-{:x}", hash.finalize()))
            .with_extension(extension)
    }

    fn journal_identity(&self) -> Result<Vec<u8>> {
        // Canonical path pair is authenticated but never stored in cleartext.
        let paths = serde_json::to_vec(&(&self.source, &self.destination))
            .map_err(|_| SecurityError::InvalidConfig)?;
        Ok(Sha256::digest(paths).to_vec())
    }

    fn operation_lock(&self) -> Result<File> {
        let path = self.artifact_path("mp9b-migration-lock");
        check(&path)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        if !file.metadata()?.is_file() || file.metadata()?.len() != 0 {
            return Err(SecurityError::InvalidConfig);
        }
        FileExt::try_lock_exclusive(&file)?;
        Ok(file)
    }

    fn output_digest(&self, directory: &Path) -> Result<String> {
        check(directory)?;
        if !directory.is_dir() {
            return Err(SecurityError::UnsafePath);
        }
        let mut files = Vec::new();
        scan(directory, directory, 0, &mut files, &mut 0)?;
        digest_scanned_files(files)
    }

    fn verify_published_destination_for_resume(
        &self,
        journal: &DurableMigrationJournal,
    ) -> Result<()> {
        if !matches!(journal.phase.as_str(), "converted" | "published")
            || self.staging_path().exists()
        {
            return Err(SecurityError::Crypto);
        }
        let expected_hash = journal
            .output_hash
            .as_deref()
            .ok_or(SecurityError::Crypto)?;
        check(&self.destination)?;
        if !fs::symlink_metadata(&self.destination)?.is_dir() {
            return Err(SecurityError::UnsafePath);
        }
        // Confirm no writer, schema migration, or privacy reader holds
        // destination fences before admitting benign empty SQLite read sidecars.
        crate::vault_fence::ensure_existing_vault_fences_idle(&self.destination)?;
        let mut files = Vec::new();
        scan(&self.destination, &self.destination, 0, &mut files, &mut 0)?;
        let has_state_db = files
            .iter()
            .any(|file| file.relative == Path::new("state.sqlite3"));
        let has_archive_db = files
            .iter()
            .any(|file| file.relative == Path::new("archive-index.sqlite3"));
        let mut core_files = Vec::with_capacity(files.len());
        let mut sidecars = Vec::new();
        for file in files {
            if let Some(kind) =
                benign_empty_sidecar_kind(&file.relative, has_state_db, has_archive_db)
            {
                sidecars.push((file.relative, kind));
            } else {
                core_files.push(file);
            }
        }
        if digest_scanned_files(core_files)? != expected_hash {
            return Err(SecurityError::Crypto);
        }
        for (relative, kind) in sidecars {
            let path = self.destination.join(relative);
            match kind {
                EmptySidecarKind::Wal => validate_empty_sqlite_wal_sidecar(&path)?,
                EmptySidecarKind::Shm => validate_empty_sqlite_shm_sidecar(&path)?,
            }
        }
        Ok(())
    }

    fn read_journal(
        &self,
        gate: &SecurityGate,
        lease: UnlockLease,
        vault: &CryptoVault,
    ) -> Result<DurableMigrationJournal> {
        let path = self.journal_path();
        check(&path)?;
        let file = File::open(path)?;
        if file.metadata()?.len() > 4096 {
            return Err(SecurityError::InvalidConfig);
        }
        let mut bytes = Vec::new();
        file.take(4097).read_to_end(&mut bytes)?;
        if bytes.len() > 4096 {
            return Err(SecurityError::InvalidConfig);
        }
        let clear = vault.unseal(
            gate,
            lease,
            ContentKind::MigrationJournal,
            &self.journal_identity()?,
            &bytes,
        )?;
        let journal: DurableMigrationJournal =
            serde_json::from_slice(&clear).map_err(|_| SecurityError::InvalidConfig)?;
        if journal.version != JOURNAL_VERSION
            || journal.inventory_hash != self.digest_inventory()?
            || !matches!(
                journal.phase.as_str(),
                "prepared" | "converted" | "published" | "rolled-back"
            )
        {
            return Err(SecurityError::Crypto);
        }
        if (journal.phase == "prepared" && journal.output_hash.is_some())
            || (matches!(journal.phase.as_str(), "converted" | "published")
                && !journal.output_hash.as_ref().is_some_and(|h| {
                    h.len() == 64
                        && h.bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                }))
        {
            return Err(SecurityError::Crypto);
        }
        Ok(journal)
    }

    fn write_journal(
        &self,
        journal: &DurableMigrationJournal,
        replace: bool,
        gate: &SecurityGate,
        lease: UnlockLease,
        vault: &CryptoVault,
    ) -> Result<()> {
        let path = self.journal_path();
        check(&path)?;
        let parent = path.parent().ok_or(SecurityError::UnsafePath)?;
        let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
        let clear =
            Zeroizing::new(serde_json::to_vec(journal).map_err(|_| SecurityError::InvalidConfig)?);
        let sealed = vault.seal(
            gate,
            lease,
            ContentKind::MigrationJournal,
            &self.journal_identity()?,
            &clear,
        )?;
        tmp.write_all(&sealed)?;
        tmp.as_file().sync_all()?;
        // Close every file handle before the Windows durable rename.
        let tmp = tmp.into_temp_path();
        durable::rename(&tmp, &path, replace)?;
        Ok(())
    }

    /// Begin a durable operation without changing even one legacy byte.
    pub fn begin_owner_consented(
        &self,
        _consent: &ExplicitMigrationConsent,
        gate: &SecurityGate,
        lease: UnlockLease,
        vault: &CryptoVault,
    ) -> Result<()> {
        gate.validate_lease(lease)?;
        let _operation = self.operation_lock()?;
        self.verify_unchanged()?;
        check(&self.staging_path())?;
        if self.staging_path().exists() || self.journal_path().exists() {
            return Err(SecurityError::AlreadyConfigured);
        }
        self.write_journal(
            &DurableMigrationJournal {
                version: JOURNAL_VERSION,
                inventory_hash: self.digest_inventory()?,
                phase: "prepared".into(),
                output_hash: None,
            },
            false,
            gate,
            lease,
            vault,
        )
    }

    /// Converter must encrypt all authoritative domains and be resumable.
    /// Verifier must independently decrypt/read back identity and integrity;
    /// a failure leaves the durable journal/staging for explicit recovery.
    pub fn execute_or_resume(
        &self,
        _consent: &ExplicitMigrationConsent,
        gate: &SecurityGate,
        lease: UnlockLease,
        vault: &CryptoVault,
        convert: impl FnMut(&Path, &Path) -> Result<()>,
        verify: impl FnMut(&Path) -> Result<()>,
    ) -> Result<()> {
        self.execute_or_resume_with_checkpoint(
            _consent,
            gate,
            lease,
            vault,
            convert,
            verify,
            |_| Ok(()),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn execute_or_resume_with_checkpoint(
        &self,
        _consent: &ExplicitMigrationConsent,
        gate: &SecurityGate,
        lease: UnlockLease,
        vault: &CryptoVault,
        mut convert: impl FnMut(&Path, &Path) -> Result<()>,
        mut verify: impl FnMut(&Path) -> Result<()>,
        mut checkpoint: impl FnMut(MigrationCheckpoint) -> Result<()>,
    ) -> Result<()> {
        gate.validate_lease(lease)?;
        let _operation = self.operation_lock()?;
        let mut journal = self.read_journal(gate, lease, vault)?;
        if journal.phase == "rolled-back" {
            return Err(SecurityError::InvalidConfig);
        }
        let staging = self.staging_path();
        check(&staging)?;
        if self.destination.exists() {
            self.verify_published_destination_for_resume(&journal)?;
            verify(&self.destination)?;
            self.verify_source_only()?;
            gate.validate_lease(lease)?;
            if journal.output_hash.as_deref()
                != Some(self.output_digest(&self.destination)?.as_str())
            {
                return Err(SecurityError::Crypto);
            }
            journal.phase = "published".into();
            self.write_journal(&journal, true, gate, lease, vault)?;
            return Ok(());
        }
        self.verify_unchanged()?;
        if journal.phase == "published" {
            return Err(SecurityError::Crypto);
        }
        if journal.phase == "prepared" {
            if !staging.exists() {
                fs::create_dir(&staging)?;
            }
            check(&staging)?;
            convert(&self.source, &staging)?;
            gate.validate_lease(lease)?;
            self.verify_unchanged()?;
            checkpoint(MigrationCheckpoint::ConversionComplete)?;
        }
        if !staging.is_dir() {
            return Err(SecurityError::Crypto);
        }
        if journal.phase == "converted"
            && journal.output_hash.as_deref() != Some(self.output_digest(&staging)?.as_str())
        {
            return Err(SecurityError::Crypto);
        }
        verify(&staging)?;
        gate.validate_lease(lease)?;
        self.verify_unchanged()?;
        if !staging.join("vault-v1.json").is_file() {
            return Err(SecurityError::Crypto);
        }
        let mut first = [0u8; 16];
        File::open(staging.join("state.sqlite3"))?.read_exact(&mut first)?;
        if &first == b"SQLite format 3\0" {
            return Err(SecurityError::Crypto);
        }
        if journal.phase == "prepared" {
            journal.output_hash = Some(self.output_digest(&staging)?);
            journal.phase = "converted".into();
            self.write_journal(&journal, true, gate, lease, vault)?;
        } else if journal.output_hash.as_deref() != Some(self.output_digest(&staging)?.as_str()) {
            return Err(SecurityError::Crypto);
        }
        checkpoint(MigrationCheckpoint::Verified)?;
        gate.validate_lease(lease)?;
        self.verify_unchanged()?;
        if journal.output_hash.as_deref() != Some(self.output_digest(&staging)?.as_str()) {
            return Err(SecurityError::Crypto);
        }
        durable::rename(&staging, &self.destination, false)?;
        checkpoint(MigrationCheckpoint::Published)?;
        self.verify_source_only()?;
        journal.phase = "published".into();
        self.write_journal(&journal, true, gate, lease, vault)?;
        Ok(())
    }

    fn verify_source_only(&self) -> Result<()> {
        if self.inventory()? != self.files {
            return Err(SecurityError::Crypto);
        }
        Ok(())
    }

    /// No plaintext deletion: quarantine encrypted staging for owner disposition.
    pub fn rollback_to_legacy(
        &self,
        _consent: &ExplicitMigrationConsent,
        gate: &SecurityGate,
        lease: UnlockLease,
        vault: &CryptoVault,
    ) -> Result<()> {
        gate.validate_lease(lease)?;
        let _operation = self.operation_lock()?;
        let mut journal = self.read_journal(gate, lease, vault)?;
        if journal.phase == "published" || self.destination.exists() {
            return Err(SecurityError::AlreadyConfigured);
        }
        self.verify_unchanged()?;
        let staging = self.staging_path();
        check(&staging)?;
        if staging.exists() {
            if !staging.is_dir() {
                return Err(SecurityError::UnsafePath);
            }
            let quarantined = self.artifact_path("mp9b-migration-rollback");
            check(&quarantined)?;
            if quarantined.exists() {
                return Err(SecurityError::AlreadyConfigured);
            }
            durable::rename(&staging, &quarantined, false)?;
        }
        journal.phase = "rolled-back".into();
        self.write_journal(&journal, true, gate, lease, vault)
    }
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
        fs::write(source.join(".state-schema.lock"), b"").unwrap();
        assert!(LegacyMigrationPreflight::inspect(&source, &output).is_ok());
        fs::write(source.join(".state-schema.lock"), b"active-writer").unwrap();
        assert!(LegacyMigrationPreflight::inspect(&source, &output).is_err());
        fs::write(source.join(".state-schema.lock"), b"").unwrap();
        let nested = source.join("archive-v1/subdir/encrypted-vault-v1");
        assert!(LegacyMigrationPreflight::inspect(&source, &nested).is_err());
        let app_child = LegacyMigrationPreflight::inspect(&source, &source.join("vault")).unwrap();
        app_child.verify_unchanged().unwrap();
    }

    fn synthetic_journal_fixture() -> (
        tempfile::TempDir,
        LegacyMigrationPreflight,
        SecurityGate,
        UnlockLease,
        CryptoVault,
    ) {
        let home = tempfile::tempdir().unwrap();
        let source = home.path().join("legacy");
        fs::create_dir(&source).unwrap();
        fs::write(
            source.join("state.sqlite3"),
            b"SQLite format 3\0synthetic-data",
        )
        .unwrap();
        let dest = home.path().join("new-vault");
        let plan = LegacyMigrationPreflight::inspect(&source, &dest).unwrap();
        let gate = SecurityGate::open(home.path().join("gate.json")).unwrap();
        gate.initialize(b"synthetic-confirmed-migration-secret")
            .unwrap();
        let lease = gate.require_unlocked().unwrap();
        let (vault, _, _) = CryptoVault::provision(&gate, lease).unwrap();
        (home, plan, gate, lease, vault)
    }

    #[test]
    fn app_child_publication_recovers_without_inventorying_it_as_legacy() {
        let (_home, original, gate, lease, vault) = synthetic_journal_fixture();
        let destination = original.source.join("encrypted-vault-v1");
        let plan = LegacyMigrationPreflight::inspect(&original.source, &destination).unwrap();
        let consent =
            ExplicitMigrationConsent::from_owner_typed_confirmation("ENCRYPT MY LEGACY VAULT")
                .unwrap();
        plan.begin_owner_consented(&consent, &gate, lease, &vault)
            .unwrap();
        assert!(!plan.journal_path().starts_with(&plan.source));
        assert!(!plan.staging_path().starts_with(&plan.source));
        let stopped = plan.execute_or_resume_with_checkpoint(
            &consent,
            &gate,
            lease,
            &vault,
            |_, stage| {
                fs::write(stage.join("vault-v1.json"), b"synthetic-manifest")?;
                fs::write(stage.join("state.sqlite3"), [7u8; 32])?;
                Ok(())
            },
            |_| Ok(()),
            |phase| {
                if phase == MigrationCheckpoint::Published {
                    return Err(SecurityError::Locked);
                }
                Ok(())
            },
        );
        assert!(stopped.is_err());
        assert!(destination.exists());
        assert!(LegacyMigrationPreflight::inspect(&plan.source, &destination).is_err());
        let fresh = LegacyMigrationPreflight::resume_owner_consented(
            &plan.source,
            &destination,
            &consent,
            &gate,
            lease,
            &vault,
        )
        .unwrap();
        fresh
            .execute_or_resume(
                &consent,
                &gate,
                lease,
                &vault,
                |_, _| panic!("published output must not be converted again"),
                |_| Ok(()),
            )
            .unwrap();
        assert_eq!(
            fs::read(plan.source.join("state.sqlite3")).unwrap(),
            b"SQLite format 3\0synthetic-data"
        );
    }

    fn fake_encrypted_conversion(_source: &Path, staging: &Path) -> Result<()> {
        fs::write(staging.join("vault-v1.json"), b"synthetic-marker-only")?;
        fs::write(staging.join("state.sqlite3"), b"synthetic-encrypted-header")?;
        Ok(())
    }

    fn fake_crypto_verify(staging: &Path) -> Result<()> {
        if fs::read(staging.join("state.sqlite3"))? != b"synthetic-encrypted-header" {
            return Err(SecurityError::Crypto);
        }
        Ok(())
    }

    #[test]
    fn consented_journal_recovers_interrupted_conversion_without_mutating_source() {
        let (_home, plan, gate, lease, vault) = synthetic_journal_fixture();
        let source_before = fs::read(plan.source.join("state.sqlite3")).unwrap();
        assert!(ExplicitMigrationConsent::from_owner_typed_confirmation("skip").is_err());
        let consent =
            ExplicitMigrationConsent::from_owner_typed_confirmation("ENCRYPT MY LEGACY VAULT")
                .unwrap();
        plan.begin_owner_consented(&consent, &gate, lease, &vault)
            .unwrap();
        let err = plan.execute_or_resume(
            &consent,
            &gate,
            lease,
            &vault,
            |source, stage| {
                fake_encrypted_conversion(source, stage)?;
                Err(SecurityError::Crypto)
            },
            fake_crypto_verify,
        );
        assert!(err.is_err());
        assert!(!plan.destination.exists());
        assert!(plan.staging_path().exists());
        // Simulate a completely new process constructing its own plan.
        let recovered = LegacyMigrationPreflight::inspect(&plan.source, &plan.destination).unwrap();
        recovered
            .execute_or_resume(
                &consent,
                &gate,
                lease,
                &vault,
                fake_encrypted_conversion,
                fake_crypto_verify,
            )
            .unwrap();
        assert_eq!(
            fs::read(recovered.source.join("state.sqlite3")).unwrap(),
            source_before
        );
        assert!(recovered.destination.join("vault-v1.json").is_file());
        assert!(!recovered.staging_path().exists());
    }

    #[test]
    fn migration_journal_rejects_source_tamper_revocation_and_rollback_is_non_destructive() {
        let (_home, plan, gate, lease, vault) = synthetic_journal_fixture();
        let consent =
            ExplicitMigrationConsent::from_owner_typed_confirmation("ENCRYPT MY LEGACY VAULT")
                .unwrap();
        plan.begin_owner_consented(&consent, &gate, lease, &vault)
            .unwrap();
        fs::write(
            plan.source.join("state.sqlite3"),
            b"SQLite format 3\0changed",
        )
        .unwrap();
        assert!(plan
            .execute_or_resume(
                &consent,
                &gate,
                lease,
                &vault,
                fake_encrypted_conversion,
                fake_crypto_verify,
            )
            .is_err());
        fs::write(
            plan.source.join("state.sqlite3"),
            b"SQLite format 3\0synthetic-data",
        )
        .unwrap();
        plan.execute_or_resume(
            &consent,
            &gate,
            lease,
            &vault,
            |source, stage| {
                fake_encrypted_conversion(source, stage)?;
                Err(SecurityError::Crypto)
            },
            fake_crypto_verify,
        )
        .unwrap_err();
        plan.rollback_to_legacy(&consent, &gate, lease, &vault)
            .unwrap();
        assert!(!plan.destination.exists());
        assert!(plan
            .destination
            .with_extension("mp9b-migration-rollback")
            .exists());
        assert!(fs::read(plan.source.join("state.sqlite3"))
            .unwrap()
            .starts_with(b"SQLite format 3\0"));
        assert!(plan
            .execute_or_resume(
                &consent,
                &gate,
                lease,
                &vault,
                fake_encrypted_conversion,
                fake_crypto_verify,
            )
            .is_err());
        gate.lock();
        assert!(plan
            .rollback_to_legacy(&consent, &gate, lease, &vault)
            .is_err());
    }

    #[test]
    fn post_publish_restart_uses_authenticated_recovery_entry_point() {
        let (_home, plan, gate, lease, vault) = synthetic_journal_fixture();
        let consent =
            ExplicitMigrationConsent::from_owner_typed_confirmation(CONSENT_WORDS).unwrap();
        plan.begin_owner_consented(&consent, &gate, lease, &vault)
            .unwrap();
        plan.execute_or_resume(
            &consent,
            &gate,
            lease,
            &vault,
            fake_encrypted_conversion,
            fake_crypto_verify,
        )
        .unwrap();
        // Persist the last pre-publication journal, simulating a restart after
        // rename and before the final journal transition.
        let mut journal = plan.read_journal(&gate, lease, &vault).unwrap();
        journal.phase = "converted".into();
        plan.write_journal(&journal, true, &gate, lease, &vault)
            .unwrap();
        let source = plan.source.clone();
        let destination = plan.destination.clone();
        drop(plan);
        assert!(LegacyMigrationPreflight::inspect(&source, &destination).is_err());
        let recovered = LegacyMigrationPreflight::resume_owner_consented(
            &source,
            &destination,
            &consent,
            &gate,
            lease,
            &vault,
        )
        .unwrap();
        recovered
            .execute_or_resume(
                &consent,
                &gate,
                lease,
                &vault,
                fake_encrypted_conversion,
                fake_crypto_verify,
            )
            .unwrap();
    }

    #[test]
    fn recovery_rejects_wrong_vault_relocated_journal_and_ciphertext_tamper() {
        let (home, plan, gate, lease, vault) = synthetic_journal_fixture();
        let consent =
            ExplicitMigrationConsent::from_owner_typed_confirmation(CONSENT_WORDS).unwrap();
        plan.begin_owner_consented(&consent, &gate, lease, &vault)
            .unwrap();
        let bytes = fs::read(plan.journal_path()).unwrap();
        assert!(!bytes
            .windows(b"inventory_hash".len())
            .any(|w| w == b"inventory_hash"));
        let (foreign, _, _) = CryptoVault::provision(&gate, lease).unwrap();
        assert!(LegacyMigrationPreflight::resume_owner_consented(
            &plan.source,
            &plan.destination,
            &consent,
            &gate,
            lease,
            &foreign
        )
        .is_err());
        let other =
            LegacyMigrationPreflight::inspect(&plan.source, &home.path().join("unrelated-target"))
                .unwrap();
        fs::write(other.journal_path(), &bytes).unwrap();
        assert!(LegacyMigrationPreflight::resume_owner_consented(
            &other.source,
            &other.destination,
            &consent,
            &gate,
            lease,
            &vault
        )
        .is_err());
        let mut tampered = bytes.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        fs::write(plan.journal_path(), tampered).unwrap();
        assert!(LegacyMigrationPreflight::resume_owner_consented(
            &plan.source,
            &plan.destination,
            &consent,
            &gate,
            lease,
            &vault
        )
        .is_err());
        fs::write(plan.journal_path(), bytes).unwrap();
        fs::create_dir(&plan.destination).unwrap();
        fs::write(plan.destination.join("unrelated-owner-file"), b"keep").unwrap();
        assert!(LegacyMigrationPreflight::resume_owner_consented(
            &plan.source,
            &plan.destination,
            &consent,
            &gate,
            lease,
            &vault
        )
        .is_err());
        assert_eq!(
            fs::read(plan.destination.join("unrelated-owner-file")).unwrap(),
            b"keep"
        );
    }

    #[test]
    fn changed_staging_after_verification_is_not_published() {
        let (_home, plan, gate, lease, vault) = synthetic_journal_fixture();
        let consent =
            ExplicitMigrationConsent::from_owner_typed_confirmation(CONSENT_WORDS).unwrap();
        plan.begin_owner_consented(&consent, &gate, lease, &vault)
            .unwrap();
        let result = plan.execute_or_resume_with_checkpoint(
            &consent,
            &gate,
            lease,
            &vault,
            fake_encrypted_conversion,
            fake_crypto_verify,
            |phase| {
                if phase == MigrationCheckpoint::Verified {
                    fs::write(
                        plan.staging_path().join("state.sqlite3"),
                        b"tampered encrypted output",
                    )?;
                }
                Ok(())
            },
        );
        assert!(result.is_err());
        assert!(!plan.destination.exists());
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

    fn set_shm_header_checksum(bytes: &mut [u8]) {
        let [s0, s1] = sqlite_wal_header_checksum(&bytes[0..40]);
        bytes[40..44].copy_from_slice(&s0.to_ne_bytes());
        bytes[44..48].copy_from_slice(&s1.to_ne_bytes());
        let header = bytes[..48].to_vec();
        bytes[48..96].copy_from_slice(&header);
    }

    fn synthetic_empty_sqlite_shm() -> Vec<u8> {
        let mut bytes = vec![0u8; SQLITE_SHM_REGION_BYTES];
        bytes[0..4].copy_from_slice(&SQLITE_WAL_INDEX_VERSION.to_ne_bytes());
        bytes[12] = 1;
        set_shm_header_checksum(&mut bytes);
        for offset in [104usize, 108, 112, 116] {
            bytes[offset..offset + 4].copy_from_slice(&SQLITE_WAL_READMARK_NOT_USED.to_ne_bytes());
        }
        bytes
    }

    fn published_synthetic_vault_fixture(
        with_archive_index: bool,
    ) -> (
        tempfile::TempDir,
        PathBuf,
        PathBuf,
        SecurityGate,
        UnlockLease,
        CryptoVault,
        ExplicitMigrationConsent,
    ) {
        let (home, plan, gate, lease, vault) = synthetic_journal_fixture();
        let consent =
            ExplicitMigrationConsent::from_owner_typed_confirmation(CONSENT_WORDS).unwrap();
        plan.begin_owner_consented(&consent, &gate, lease, &vault)
            .unwrap();
        plan.execute_or_resume(
            &consent,
            &gate,
            lease,
            &vault,
            |_source, staging| {
                fs::write(staging.join("vault-v1.json"), b"synthetic-marker-only")?;
                fs::write(staging.join("state.sqlite3"), b"synthetic-encrypted-header")?;
                if with_archive_index {
                    fs::write(
                        staging.join("archive-index.sqlite3"),
                        b"synthetic-encrypted-archive-index",
                    )?;
                }
                Ok(())
            },
            fake_crypto_verify,
        )
        .unwrap();
        (
            home,
            plan.source,
            plan.destination,
            gate,
            lease,
            vault,
            consent,
        )
    }

    #[test]
    fn resume_admits_verified_empty_read_sidecars_and_requires_quiescence() {
        let (_home, source, destination, gate, lease, vault, consent) =
            published_synthetic_vault_fixture(true);
        let shm = synthetic_empty_sqlite_shm();
        for name in [
            "state.sqlite3-wal",
            "state.sqlite3-shm",
            "archive-index.sqlite3-wal",
            "archive-index.sqlite3-shm",
        ] {
            let contents: &[u8] = if name.ends_with("-wal") { &[] } else { &shm };
            fs::write(destination.join(name), contents).unwrap();
        }
        let recovered = LegacyMigrationPreflight::resume_owner_consented(
            &source,
            &destination,
            &consent,
            &gate,
            lease,
            &vault,
        )
        .unwrap();
        // Post-verify strict digest must still reject unquiesced sidecars.
        assert!(recovered
            .execute_or_resume(
                &consent,
                &gate,
                lease,
                &vault,
                |_, _| panic!("published output must not convert again"),
                fake_crypto_verify,
            )
            .is_err());

        for _ in 0..2 {
            for name in [
                "state.sqlite3-wal",
                "state.sqlite3-shm",
                "archive-index.sqlite3-wal",
                "archive-index.sqlite3-shm",
            ] {
                let contents: &[u8] = if name.ends_with("-wal") { &[] } else { &shm };
                fs::write(destination.join(name), contents).unwrap();
            }
            let plan = LegacyMigrationPreflight::resume_owner_consented(
                &source,
                &destination,
                &consent,
                &gate,
                lease,
                &vault,
            )
            .unwrap();
            plan.execute_or_resume(
                &consent,
                &gate,
                lease,
                &vault,
                |_, _| panic!("published output must not convert again"),
                |dir| {
                    fake_crypto_verify(dir)?;
                    for name in [
                        "state.sqlite3-wal",
                        "state.sqlite3-shm",
                        "archive-index.sqlite3-wal",
                        "archive-index.sqlite3-shm",
                    ] {
                        let path = dir.join(name);
                        if path.exists() {
                            fs::remove_file(path)?;
                        }
                    }
                    Ok(())
                },
            )
            .unwrap();
        }
    }

    #[test]
    fn resume_rejects_dirty_forged_unknown_and_contended_sidecars_before_verify() {
        use crate::vault_fence::VaultWriterFence;
        use std::cell::Cell;

        let assert_fails_before_verify = |source: &Path,
                                          destination: &Path,
                                          consent: &ExplicitMigrationConsent,
                                          gate: &SecurityGate,
                                          lease: UnlockLease,
                                          vault: &CryptoVault,
                                          pre_plan: &LegacyMigrationPreflight,
                                          label: &str| {
            assert!(
                LegacyMigrationPreflight::resume_owner_consented(
                    source,
                    destination,
                    consent,
                    gate,
                    lease,
                    vault,
                )
                .is_err(),
                "{label}: resume_owner_consented unexpectedly succeeded"
            );
            let verify_called = Cell::new(false);
            assert!(
                pre_plan
                    .execute_or_resume(
                        consent,
                        gate,
                        lease,
                        vault,
                        |_, _| panic!("{label}: convert must not run"),
                        |_| {
                            verify_called.set(true);
                            Ok(())
                        },
                    )
                    .is_err(),
                "{label}: execute_or_resume unexpectedly succeeded"
            );
            assert!(
                !verify_called.get(),
                "{label}: verify closure ran on unauthenticated state"
            );
        };

        let (_home, source, destination, gate, lease, vault, consent) =
            published_synthetic_vault_fixture(true);
        let pre_plan = LegacyMigrationPreflight::resume_owner_consented(
            &source,
            &destination,
            &consent,
            &gate,
            lease,
            &vault,
        )
        .unwrap();

        // 1. Non-empty WAL files.
        for wal_name in ["state.sqlite3-wal", "archive-index.sqlite3-wal"] {
            let wal = destination.join(wal_name);
            fs::write(&wal, b"uncheckpointed-wal-bytes").unwrap();
            assert_fails_before_verify(
                &source,
                &destination,
                &consent,
                &gate,
                lease,
                &vault,
                &pre_plan,
                wal_name,
            );
            assert_eq!(fs::read(&wal).unwrap(), b"uncheckpointed-wal-bytes");
            fs::remove_file(wal).unwrap();
        }

        // 2. Malformed, forged, or dirty -shm variants.
        let shm_path = destination.join("state.sqlite3-shm");
        let bad_size_variants = vec![
            ("shm-empty", vec![]),
            ("shm-short", vec![0u8; SQLITE_SHM_REGION_BYTES - 1]),
            ("shm-long", vec![0u8; SQLITE_SHM_REGION_BYTES + 1]),
        ];
        for (label, bytes) in bad_size_variants {
            fs::write(&shm_path, &bytes).unwrap();
            assert_fails_before_verify(
                &source,
                &destination,
                &consent,
                &gate,
                lease,
                &vault,
                &pre_plan,
                label,
            );
            fs::remove_file(&shm_path).unwrap();
        }

        let mut broken_mirror = synthetic_empty_sqlite_shm();
        broken_mirror[48] ^= 1;
        fs::write(&shm_path, &broken_mirror).unwrap();
        assert_fails_before_verify(
            &source,
            &destination,
            &consent,
            &gate,
            lease,
            &vault,
            &pre_plan,
            "shm-broken-mirror",
        );

        let mut bad_cksum = synthetic_empty_sqlite_shm();
        bad_cksum[40] ^= 1;
        bad_cksum[48 + 40] ^= 1;
        fs::write(&shm_path, &bad_cksum).unwrap();
        assert_fails_before_verify(
            &source,
            &destination,
            &consent,
            &gate,
            lease,
            &vault,
            &pre_plan,
            "shm-bad-checksum",
        );

        let mut dirty_mx_frame = synthetic_empty_sqlite_shm();
        dirty_mx_frame[16..20].copy_from_slice(&1u32.to_ne_bytes());
        set_shm_header_checksum(&mut dirty_mx_frame);
        fs::write(&shm_path, &dirty_mx_frame).unwrap();
        assert_fails_before_verify(
            &source,
            &destination,
            &consent,
            &gate,
            lease,
            &vault,
            &pre_plan,
            "shm-mxframe-nonzero",
        );

        let mut dirty_backfill = synthetic_empty_sqlite_shm();
        dirty_backfill[96..100].copy_from_slice(&1u32.to_ne_bytes());
        fs::write(&shm_path, &dirty_backfill).unwrap();
        assert_fails_before_verify(
            &source,
            &destination,
            &consent,
            &gate,
            lease,
            &vault,
            &pre_plan,
            "shm-nbackfill-nonzero",
        );

        let mut dirty_readmark = synthetic_empty_sqlite_shm();
        dirty_readmark[104..108].copy_from_slice(&2u32.to_ne_bytes());
        fs::write(&shm_path, &dirty_readmark).unwrap();
        assert_fails_before_verify(
            &source,
            &destination,
            &consent,
            &gate,
            lease,
            &vault,
            &pre_plan,
            "shm-readmark-nonzero",
        );

        let mut dirty_lock = synthetic_empty_sqlite_shm();
        dirty_lock[120] = 1;
        fs::write(&shm_path, &dirty_lock).unwrap();
        assert_fails_before_verify(
            &source,
            &destination,
            &consent,
            &gate,
            lease,
            &vault,
            &pre_plan,
            "shm-alock-nonzero",
        );

        let mut smuggled_tail = synthetic_empty_sqlite_shm();
        smuggled_tail[200] = 0x42;
        fs::write(&shm_path, &smuggled_tail).unwrap();
        assert_fails_before_verify(
            &source,
            &destination,
            &consent,
            &gate,
            lease,
            &vault,
            &pre_plan,
            "shm-nonzero-tail",
        );
        fs::remove_file(&shm_path).unwrap();

        // 3. Disallowed sidecars and nested filenames.
        for unknown in ["state.sqlite3-journal", "index.sqlite3-wal"] {
            let p = destination.join(unknown);
            fs::write(&p, b"").unwrap();
            assert_fails_before_verify(
                &source,
                &destination,
                &consent,
                &gate,
                lease,
                &vault,
                &pre_plan,
                unknown,
            );
            fs::remove_file(p).unwrap();
        }
        fs::create_dir(destination.join("nested")).unwrap();
        fs::write(destination.join("nested/state.sqlite3-wal"), b"").unwrap();
        assert_fails_before_verify(
            &source,
            &destination,
            &consent,
            &gate,
            lease,
            &vault,
            &pre_plan,
            "nested-wal",
        );
        fs::remove_dir_all(destination.join("nested")).unwrap();

        // 4. Contended destination writer, schema, and privacy fences.
        let writer = VaultWriterFence::writer(&destination).unwrap();
        assert_fails_before_verify(
            &source,
            &destination,
            &consent,
            &gate,
            lease,
            &vault,
            &pre_plan,
            "active-writer-fence",
        );
        drop(writer);

        let privacy = VaultWriterFence::privacy_reader(&destination).unwrap();
        assert_fails_before_verify(
            &source,
            &destination,
            &consent,
            &gate,
            lease,
            &vault,
            &pre_plan,
            "active-privacy-fence",
        );
        drop(privacy);

        let schema_lock_path = destination.join(".state-schema.lock");
        let schema_lock = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&schema_lock_path)
            .unwrap();
        FileExt::try_lock_exclusive(&schema_lock).unwrap();
        assert_fails_before_verify(
            &source,
            &destination,
            &consent,
            &gate,
            lease,
            &vault,
            &pre_plan,
            "active-schema-lock",
        );
        drop(schema_lock);
        fs::remove_file(&schema_lock_path).unwrap();

        // 5. Non-empty forged lock sentinel must fail.
        fs::write(destination.join(".vault-writer.lock"), b"forged").unwrap();
        assert_fails_before_verify(
            &source,
            &destination,
            &consent,
            &gate,
            lease,
            &vault,
            &pre_plan,
            "nonempty-writer-lock",
        );
        fs::write(destination.join(".vault-writer.lock"), b"").unwrap();

        // 6. Tampered core file even with valid empty sidecars must fail.
        fs::write(destination.join("state.sqlite3-wal"), b"").unwrap();
        fs::write(&shm_path, synthetic_empty_sqlite_shm()).unwrap();
        let mut core = fs::read(destination.join("state.sqlite3")).unwrap();
        core[0] ^= 1;
        fs::write(destination.join("state.sqlite3"), core).unwrap();
        assert_fails_before_verify(
            &source,
            &destination,
            &consent,
            &gate,
            lease,
            &vault,
            &pre_plan,
            "tampered-core-with-valid-sidecars",
        );

        // 7. Archive sidecar without matching archive-index.sqlite3 base DB must fail.
        let (_home2, src2, dst2, gate2, lease2, vault2, consent2) =
            published_synthetic_vault_fixture(false);
        let plan2 = LegacyMigrationPreflight::resume_owner_consented(
            &src2, &dst2, &consent2, &gate2, lease2, &vault2,
        )
        .unwrap();
        fs::write(dst2.join("archive-index.sqlite3-wal"), b"").unwrap();
        assert_fails_before_verify(
            &src2,
            &dst2,
            &consent2,
            &gate2,
            lease2,
            &vault2,
            &plan2,
            "orphan-archive-wal-without-base-db",
        );
    }
}

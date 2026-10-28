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

#[path = "vault_migration_fs.rs"]
mod durable;

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
            if !matches!(journal.phase.as_str(), "converted" | "published")
                || plan.staging_path().exists()
                || journal.output_hash.as_deref()
                    != Some(plan.output_digest(&plan.destination)?.as_str())
            {
                return Err(SecurityError::Crypto);
            }
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
        if !canonical_source.is_dir() {
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
        self.destination.with_extension("mp9b-migration-journal")
    }

    fn staging_path(&self) -> PathBuf {
        self.destination.with_extension("mp9b-migration-staging")
    }

    fn journal_identity(&self) -> Result<Vec<u8>> {
        // Canonical path pair is authenticated but never stored in cleartext.
        let paths = serde_json::to_vec(&(&self.source, &self.destination))
            .map_err(|_| SecurityError::InvalidConfig)?;
        Ok(Sha256::digest(paths).to_vec())
    }

    fn operation_lock(&self) -> Result<File> {
        if self.destination.parent() == Some(self.source.as_path()) {
            return Err(SecurityError::UnsafePath);
        }
        let path = self.destination.with_extension("mp9b-migration-lock");
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
        // Journal and staging must live OUTSIDE the legacy inventory.
        if self.destination.parent() == Some(self.source.as_path()) {
            return Err(SecurityError::UnsafePath);
        }
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
            if (journal.phase != "converted" && journal.phase != "published")
                || staging.exists()
                || journal.output_hash.as_deref()
                    != Some(self.output_digest(&self.destination)?.as_str())
            {
                return Err(SecurityError::Crypto);
            }
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
            let quarantined = self.destination.with_extension("mp9b-migration-rollback");
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
}

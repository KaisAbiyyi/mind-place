//! MP-9B cooperative online vault snapshot.
//! Holds one exclusive cross-process writer fence while capturing all keyed
//! SQLite WAL-visible pages and every immutable encrypted object. This is
//! NOT a substitute for auditing every future writer and testing native IPC.
use crate::{
    documents::DocumentIndex, vault_archive::VaultCaptureStore,
    vault_legacy_objects::RawLegacyInventory, IndexError, IndexStore, Result,
};
use mind_place_security::{
    vault::{CryptoVault, VaultManifest},
    vault_backup::{BackupMetadata, PortableBackup},
    vault_fence::VaultWriterFence,
    vault_storage::ImmutableVaultStorage,
    SecurityGate, UnlockLease,
};
use mind_place_state::{CipherKey, StateStore};
use rusqlite::{backup::Backup, Connection, OpenFlags};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
    time::Duration,
};
use zeroize::Zeroize;

const DB_DOMAINS: [(&str, &[u8]); 4] = [
    ("state.sqlite3", b"state/sqlcipher/v1"),
    ("index.sqlite3", b"index/sqlcipher/v1"),
    ("document-index.sqlite3", b"document-index/sqlcipher/v1"),
    ("archive-index.sqlite3", b"archive-index/sqlcipher/v1"),
];

const MAX_OBJECT_TREE_ENTRIES: usize = 200_000;
const MAX_OBJECT_TREE_DIRECTORIES: usize = 100_000;
const MAX_OBJECT_TREE_FILES: usize = 100_000;
const MAX_OBJECT_TREE_DEPTH: usize = 12;

#[derive(Clone, Copy)]
struct ObjectTreeLimits {
    max_entries: usize,
    max_directories: usize,
    max_files: usize,
    max_depth: usize,
}

const PRODUCTION_OBJECT_TREE_LIMITS: ObjectTreeLimits = ObjectTreeLimits {
    max_entries: MAX_OBJECT_TREE_ENTRIES,
    max_directories: MAX_OBJECT_TREE_DIRECTORIES,
    max_files: MAX_OBJECT_TREE_FILES,
    max_depth: MAX_OBJECT_TREE_DEPTH,
};

struct ObjectTreeBudget {
    limits: ObjectTreeLimits,
    entries: usize,
    directories: usize,
    files: usize,
}

impl ObjectTreeBudget {
    fn new(limits: ObjectTreeLimits) -> Self {
        Self {
            limits,
            entries: 0,
            directories: 0,
            files: 0,
        }
    }

    fn production() -> Self {
        Self::new(PRODUCTION_OBJECT_TREE_LIMITS)
    }

    fn record_entry(&mut self) -> Result<()> {
        self.entries = self
            .entries
            .checked_add(1)
            .ok_or(IndexError::InvalidEvidence)?;
        if self.entries > self.limits.max_entries {
            return Err(IndexError::InvalidEvidence);
        }
        Ok(())
    }

    fn record_directory(&mut self, depth: usize) -> Result<()> {
        self.directories = self
            .directories
            .checked_add(1)
            .ok_or(IndexError::InvalidEvidence)?;
        if self.directories > self.limits.max_directories || depth > self.limits.max_depth {
            return Err(IndexError::InvalidEvidence);
        }
        Ok(())
    }

    fn record_file(&mut self) -> Result<()> {
        self.files = self
            .files
            .checked_add(1)
            .ok_or(IndexError::InvalidEvidence)?;
        if self.files > self.limits.max_files {
            return Err(IndexError::InvalidEvidence);
        }
        Ok(())
    }
}

pub struct VaultOnlineBackup;

impl VaultOnlineBackup {
    /// The caller must be the unlocked local owner and choose an empty,
    /// non-overlapping destination. The source remains immutable throughout.
    pub fn create(
        gate: &SecurityGate,
        lease: UnlockLease,
        vault: &CryptoVault,
        source: &Path,
        destination: &Path,
        generation: u64,
    ) -> Result<BackupMetadata> {
        gate.validate_lease(lease)
            .map_err(|_| IndexError::Revoked)?;
        let resolved = mind_place_security::vault_fs::checked_new_destination(source, destination)
            .map_err(|_| IndexError::UnsafePath)?;
        let destination = resolved.as_path();
        if destination.starts_with(source)
            || source.starts_with(destination)
            || destination.exists()
        {
            return Err(IndexError::UnsafePath);
        }
        let parent = destination.parent().ok_or(IndexError::UnsafePath)?;
        if !parent.is_dir() {
            return Err(IndexError::UnsafePath);
        }
        let manifest = VaultManifest::read(source).map_err(|_| IndexError::InvalidEvidence)?;
        if manifest.vault_id_b64
            != base64::Engine::encode(&base64::engine::general_purpose::STANDARD, vault.id())
        {
            return Err(IndexError::InvalidEvidence);
        }
        // All supported encrypted state/index/immutable object writers take
        // shared lock; no source write can advance during this snapshot cut.
        let _fence = VaultWriterFence::snapshot(source).map_err(|_| IndexError::InvalidEvidence)?;
        // Detach the scratch path from TempDir immediately. A panic or any
        // returned error must leave the encrypted snapshot for owner disposition.
        let scratch_parent_path = tempfile::Builder::new()
            .prefix(".mp9b-encrypted-online-")
            .tempdir_in(parent)?
            .keep();
        let result = (|| -> Result<BackupMetadata> {
            let scratch = scratch_parent_path.join("vault");
            manifest
                .persist_new(&scratch)
                .map_err(|_| IndexError::InvalidEvidence)?;

            let mut have_state = false;
            let mut object_budget = ObjectTreeBudget::production();
            for entry in fs::read_dir(source)? {
                gate.validate_lease(lease)
                    .map_err(|_| IndexError::Revoked)?;
                let entry = entry?;
                let path = entry.path();
                guarded(&path)?;
                let name = entry.file_name();
                let name = name.to_str().ok_or(IndexError::UnsafePath)?;
                let meta = fs::symlink_metadata(&path)?;
                if DB_DOMAINS.iter().any(|(n, _)| *n == name) && meta.is_file() {
                    let domain = DB_DOMAINS.iter().find(|(n, _)| *n == name).unwrap().1;
                    snapshot_sqlcipher(gate, lease, vault, domain, &path, &scratch.join(name))?;
                    if name == "state.sqlite3" {
                        have_state = true;
                    }
                } else if ["archive", "document-objects", "backups"].contains(&name)
                    && meta.is_dir()
                {
                    object_budget.record_entry()?;
                    object_budget.record_directory(0)?;
                    copy_ciphertext_tree(&path, &scratch.join(name), 0, &mut object_budget)?;
                } else if ["vault-v1.json", ".state-schema.lock", ".vault-writer.lock"]
                    .contains(&name)
                    && meta.is_file()
                    && (name == "vault-v1.json" || meta.len() == 0)
                {
                    // New manifest already persisted. Lock and schema sentinels
                    // have no authoritative content to capture.
                } else if DB_DOMAINS.iter().any(|(n, _)| {
                    ["-wal", "-shm", "-journal"]
                        .iter()
                        .any(|suffix| name == format!("{n}{suffix}"))
                }) && meta.is_file()
                {
                    // SQLCipher Online Backup includes committed WAL content.
                    // Neither WAL nor SHM may be copied as independent files.
                } else {
                    return Err(IndexError::InvalidEvidence);
                }
            }
            if !have_state {
                return Err(IndexError::InvalidEvidence);
            }
            verify_coherent_snapshot(gate, lease, vault, &scratch)?;
            gate.validate_lease(lease)
                .map_err(|_| IndexError::Revoked)?;
            // Package a fully quiesced scratch root. Existing portable restore
            // authenticates the manifest and verifies each file digest.
            let metadata =
                PortableBackup::create(gate, lease, vault, &scratch, destination, generation)
                    .map_err(|_| IndexError::InvalidEvidence)?;
            gate.validate_lease(lease)
                .map_err(|_| IndexError::Revoked)?;
            Ok(metadata)
        })();
        match result {
            Ok(metadata) => {
                // The package is fully published and verified; only this
                // successful path may dispose of its temporary duplicate.
                let _ = fs::remove_dir_all(&scratch_parent_path);
                Ok(metadata)
            }
            Err(error) => {
                // Failed encrypted scratch snapshots remain for explicit
                // owner disposition; never auto-clean them on error or panic.
                Err(error)
            }
        }
    }

    /// Owner-authorized new-destination restore: authenticate the portable
    /// snapshot, then verify the keyed SQLCipher databases *before* publishing.
    /// A failed validation leaves only an explicitly named partial root.
    pub fn restore(
        gate: &SecurityGate,
        lease: UnlockLease,
        backup_root: &Path,
        destination: &Path,
        recovery: &mind_place_security::vault::RecoverySecret,
    ) -> Result<BackupMetadata> {
        gate.validate_lease(lease)
            .map_err(|_| IndexError::Revoked)?;
        let resolved =
            mind_place_security::vault_fs::checked_new_destination(backup_root, destination)
                .map_err(|_| IndexError::UnsafePath)?;
        let destination = resolved.as_path();
        if destination.exists()
            || backup_root.starts_with(destination)
            || destination.starts_with(backup_root)
        {
            return Err(IndexError::UnsafePath);
        }
        let parent = destination.parent().ok_or(IndexError::UnsafePath)?;
        guarded(parent)?;
        if !parent.is_dir() {
            return Err(IndexError::UnsafePath);
        }
        let staged = destination.with_extension("mp9b-verified-restore-partial");
        guarded(&staged)?;
        if staged.exists() {
            return Err(IndexError::InvalidEvidence);
        }
        // Portable restore already enforces authenticated manifest, full file
        // inventory, ciphertext hashes, and a non-overwriting staged write.
        let meta = PortableBackup::restore(gate, lease, backup_root, &staged, recovery)
            .map_err(|_| IndexError::InvalidEvidence)?;
        let manifest = VaultManifest::read(&staged).map_err(|_| IndexError::InvalidEvidence)?;
        let vault = CryptoVault::restore_with_recovery(gate, lease, &manifest, recovery)
            .map_err(|_| IndexError::InvalidEvidence)?;
        let mut found_state = false;
        for (filename, domain) in DB_DOMAINS {
            let path = staged.join(filename);
            if !path.exists() {
                continue;
            }
            if filename == "state.sqlite3" {
                found_state = true;
            }
            guarded(&path)?;
            let mut header = [0u8; 16];
            File::open(&path)?.read_exact(&mut header)?;
            if &header == b"SQLite format 3\0" {
                return Err(IndexError::InvalidEvidence);
            }
            let key_bytes = vault
                .derived_key(gate, lease, domain)
                .map_err(|_| IndexError::Revoked)?;
            let mut raw = [0u8; 32];
            raw.copy_from_slice(key_bytes.as_ref());
            let key = CipherKey::new(raw, gate, lease);
            raw.zeroize();
            let db = Connection::open_with_flags(
                &path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
            )?;
            key.unlock(&db)?;
            let health: String = db.query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?;
            if health != "ok" {
                return Err(IndexError::InvalidEvidence);
            }
        }
        if !found_state {
            return Err(IndexError::InvalidEvidence);
        }
        verify_coherent_snapshot(gate, lease, &vault, &staged)?;
        gate.validate_lease(lease)
            .map_err(|_| IndexError::Revoked)?;
        // All SQLite handles above have closed before Windows directory rename.
        mind_place_security::vault_fs::publish_new_directory(&staged, destination)
            .map_err(|_| IndexError::InvalidEvidence)?;
        Ok(meta)
    }
}

fn snapshot_sqlcipher(
    gate: &SecurityGate,
    lease: UnlockLease,
    vault: &CryptoVault,
    domain: &[u8],
    source: &Path,
    destination: &Path,
) -> Result<()> {
    guarded(source)?;
    guarded(destination)?;
    let derived = vault
        .derived_key(gate, lease, domain)
        .map_err(|_| IndexError::Revoked)?;
    let mut raw = [0u8; 32];
    raw.copy_from_slice(derived.as_ref());
    let key = CipherKey::new(raw, gate, lease);
    raw.zeroize();
    let input = Connection::open_with_flags(
        source,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    key.unlock(&input)?;
    let reserved = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    reserved.sync_all()?;
    drop(reserved);
    let mut output = Connection::open_with_flags(
        destination,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    key.unlock(&output)?;
    {
        let copy = Backup::new(&input, &mut output)?;
        copy.run_to_completion(128, Duration::from_millis(50), None)?;
    }
    key.unlock(&output)?;
    let health: String = output.query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?;
    if health != "ok" {
        return Err(IndexError::InvalidIndex);
    }
    drop(output);
    drop(input);
    let mut header = [0u8; 16];
    let mut durable = OpenOptions::new()
        .read(true)
        .write(true)
        .open(destination)?;
    durable.read_exact(&mut header)?;
    if &header == b"SQLite format 3\0" {
        return Err(IndexError::InvalidIndex);
    }
    durable.sync_all()?;
    Ok(())
}

fn verify_coherent_snapshot(
    gate: &SecurityGate,
    lease: UnlockLease,
    vault: &CryptoVault,
    root: &Path,
) -> Result<()> {
    let _validation = VaultWriterFence::writer(root).map_err(|_| IndexError::InvalidEvidence)?;
    let storage = ImmutableVaultStorage::new(root, vault, gate, lease)
        .map_err(|_| IndexError::InvalidEvidence)?;
    let mut expected_documents = HashSet::new();
    for (filename, domain) in DB_DOMAINS {
        let path = root.join(filename);
        if !path.exists() {
            continue;
        }
        let derived = vault
            .derived_key(gate, lease, domain)
            .map_err(|_| IndexError::Revoked)?;
        let mut raw = [0u8; 32];
        raw.copy_from_slice(derived.as_ref());
        // These product constructors validate actual application/schema and
        // migration identity, not merely SQLite's physical page consistency.
        let valid = match filename {
            "state.sqlite3" => StateStore::open_encrypted(&path, raw, gate, lease)
                .map(drop)
                .map_err(IndexError::from),
            "index.sqlite3" => IndexStore::open_encrypted(&path, raw, gate, lease).map(drop),
            "document-index.sqlite3" => {
                DocumentIndex::open_encrypted(&path, raw, gate, lease).map(drop)
            }
            _ => Ok(()),
        };
        let key = CipherKey::new(raw, gate, lease);
        raw.zeroize();
        valid?;
        let db = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        key.unlock(&db)?;
        let health: String = db.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        if health != "ok"
            || db
                .prepare("PRAGMA foreign_key_check")?
                .query([])?
                .next()?
                .is_some()
        {
            return Err(IndexError::InvalidEvidence);
        }
        // FTS external-content correctness is distinct from SQLite page checks.
        let tables = db.prepare("SELECT name FROM sqlite_master WHERE type='table' AND lower(sql) LIKE '%using fts5%' ORDER BY name")?
            .query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        for name in tables {
            let quoted = name.replace('"', "\"\"");
            db.execute_batch(&format!(
                "INSERT INTO \"{quoted}\"(\"{quoted}\", rank) VALUES('integrity-check', 1)"
            ))?;
        }
        if filename == "state.sqlite3" {
            let mut query = db.prepare(
                "SELECT content_hash,byte_length FROM document_objects ORDER BY content_hash",
            )?;
            let mut rows = query.query([])?;
            while let Some(row) = rows.next()? {
                let hash: String = row.get(0)?;
                let length: i64 = row.get(1)?;
                if hash.len() != 64
                    || !hash
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    || length < 0
                {
                    return Err(IndexError::InvalidEvidence);
                }
                let identity = format!("document/{hash}");
                let bytes = storage
                    .read(
                        gate,
                        lease,
                        mind_place_security::vault::ContentKind::DocumentObject,
                        identity.as_bytes(),
                    )
                    .map_err(|_| IndexError::InvalidEvidence)?;
                if bytes.len() as i64 != length || format!("{:x}", Sha256::digest(&bytes)) != hash {
                    return Err(IndexError::InvalidEvidence);
                }
                expected_documents.insert(
                    vault
                        .storage_name(
                            gate,
                            lease,
                            mind_place_security::vault::ContentKind::DocumentObject,
                            identity.as_bytes(),
                        )
                        .map_err(|_| IndexError::InvalidEvidence)?,
                );
            }
        }
    }
    let raw = RawLegacyInventory::read(root, vault, &storage, gate, lease)
        .map_err(|_| IndexError::InvalidEvidence)?;
    if root.join("archive").exists()
        && !root.join("archive-index.sqlite3").exists()
        && raw.is_none()
    {
        return Err(IndexError::InvalidEvidence);
    }
    let mut expected_archive = VaultCaptureStore::verify_backup_catalog(root, vault, gate, lease)?;
    if let Some(raw) = raw {
        raw.verify_all(&storage, gate, lease)
            .map_err(|_| IndexError::InvalidEvidence)?;
        let (archive, documents) = raw
            .referenced_storage_names(vault, gate, lease)
            .map_err(|_| IndexError::InvalidEvidence)?;
        expected_archive.extend(archive);
        expected_documents.extend(documents);
        expected_archive.insert(
            vault
                .storage_name(
                    gate,
                    lease,
                    mind_place_security::vault::ContentKind::Archive,
                    crate::vault_legacy_objects::RAW_MAP_ID,
                )
                .map_err(|_| IndexError::InvalidEvidence)?,
        );
    }
    // A catalog-to-object check alone misses unreferenced, potentially
    // attacker-added ciphertext. Require exact inverse correspondence too.
    let mut budget = ObjectTreeBudget::production();
    verify_exact_object_tree(&root.join("archive"), &expected_archive, &mut budget)?;
    verify_exact_object_tree(
        &root.join("document-objects"),
        &expected_documents,
        &mut budget,
    )?;
    gate.validate_lease(lease)
        .map_err(|_| IndexError::Revoked)?;
    Ok(())
}

/// Reject orphaned or unexpected immutable objects, not only missing references.
/// Both roots consume the same checked total-entry/file/directory budget.
fn verify_exact_object_tree(
    root: &Path,
    expected: &HashSet<String>,
    budget: &mut ObjectTreeBudget,
) -> Result<()> {
    let metadata = match fs::symlink_metadata(root) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return if expected.is_empty() {
                Ok(())
            } else {
                Err(IndexError::InvalidEvidence)
            };
        }
        Err(err) => return Err(err.into()),
    };
    guarded(root)?;
    if !metadata.is_dir() {
        return Err(IndexError::InvalidEvidence);
    }
    budget.record_entry()?;
    budget.record_directory(0)?;
    let mut found = HashSet::new();
    for shard in fs::read_dir(root)? {
        let shard = shard?;
        budget.record_entry()?;
        let path = shard.path();
        guarded(&path)?;
        if !fs::symlink_metadata(&path)?.is_dir() {
            return Err(IndexError::InvalidEvidence);
        }
        budget.record_directory(1)?;
        let shard_name = shard
            .file_name()
            .to_str()
            .ok_or(IndexError::InvalidEvidence)?
            .to_owned();
        if shard_name.len() != 2
            || !shard_name
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(IndexError::InvalidEvidence);
        }
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            budget.record_entry()?;
            let object_path = entry.path();
            guarded(&object_path)?;
            if !fs::symlink_metadata(&object_path)?.is_file() {
                return Err(IndexError::InvalidEvidence);
            }
            budget.record_file()?;
            let name = entry
                .file_name()
                .to_str()
                .ok_or(IndexError::InvalidEvidence)?
                .to_owned();
            if name.len() != 64
                || !name.starts_with(&shard_name)
                || !expected.contains(&name)
                || !found.insert(name)
            {
                return Err(IndexError::InvalidEvidence);
            }
        }
    }
    if found.len() != expected.len() {
        return Err(IndexError::InvalidEvidence);
    }
    Ok(())
}

fn copy_ciphertext_tree(
    root: &Path,
    target: &Path,
    depth: usize,
    budget: &mut ObjectTreeBudget,
) -> Result<()> {
    if depth > budget.limits.max_depth {
        return Err(IndexError::InvalidEvidence);
    }
    guarded(root)?;
    fs::create_dir(target)?;
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        budget.record_entry()?;
        let source = entry.path();
        guarded(&source)?;
        let file_name = entry.file_name();
        let output = target.join(file_name);
        let meta = fs::symlink_metadata(&source)?;
        if meta.is_dir() {
            budget.record_directory(depth + 1)?;
            copy_ciphertext_tree(&source, &output, depth + 1, budget)?;
        } else if meta.is_file() {
            budget.record_file()?;
            if meta.len() > 8 * 1024 * 1024 * 1024 {
                return Err(IndexError::InvalidEvidence);
            }
            let mut first = [0u8; 5];
            File::open(&source)?.read_exact(&mut first)?;
            if &first != b"MPOS\x01" {
                return Err(IndexError::InvalidEvidence);
            }
            let mut input = File::open(&source)?;
            let mut target_file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(output)?;
            let length = std::io::copy(&mut input, &mut target_file)?;
            if length != meta.len() {
                return Err(IndexError::InvalidEvidence);
            }
            target_file.flush()?;
            target_file.sync_all()?;
        } else {
            return Err(IndexError::UnsafePath);
        }
    }
    Ok(())
}

fn guarded(path: &Path) -> Result<()> {
    for item in path.ancestors() {
        if item.as_os_str().is_empty() {
            continue;
        }
        if let Ok(meta) = fs::symlink_metadata(item) {
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
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ciphertext_copy_rejects_empty_directory_fanout_with_injected_budget() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        fs::create_dir(&source).unwrap();
        for name in ["empty-a", "empty-b", "empty-c"] {
            fs::create_dir(source.join(name)).unwrap();
        }
        let limits = ObjectTreeLimits {
            max_entries: 2,
            max_directories: 10,
            max_files: 10,
            max_depth: 12,
        };
        let mut budget = ObjectTreeBudget::new(limits);
        budget.record_entry().unwrap();
        budget.record_directory(0).unwrap();
        assert!(copy_ciphertext_tree(&source, &target, 0, &mut budget).is_err());
        assert!(target.is_dir());
        assert!(source.join("empty-a").is_dir());
        assert!(source.join("empty-b").is_dir());
        assert!(source.join("empty-c").is_dir());
    }
}

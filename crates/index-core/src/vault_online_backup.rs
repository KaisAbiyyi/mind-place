//! MP-9B cooperative online vault snapshot.
//! Holds one exclusive cross-process writer fence while capturing all keyed
//! SQLite WAL-visible pages and every immutable encrypted object. This is
//! NOT a substitute for auditing every future writer and testing native IPC.
use crate::{IndexError, Result};
use mind_place_security::{
    vault::{CryptoVault, VaultManifest},
    vault_backup::{BackupMetadata, PortableBackup},
    vault_fence::VaultWriterFence,
    SecurityGate, UnlockLease,
};
use mind_place_state::CipherKey;
use rusqlite::{backup::Backup, Connection, OpenFlags};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use zeroize::Zeroize;

const DB_DOMAINS: [(&str, &[u8]); 4] = [
    ("state.sqlite3", b"state/sqlcipher/v1"),
    ("index.sqlite3", b"index/sqlcipher/v1"),
    ("document-index.sqlite3", b"document-index/sqlcipher/v1"),
    ("archive-index.sqlite3", b"archive-index/sqlcipher/v1"),
];

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
        let scratch_parent = tempfile::Builder::new()
            .prefix(".mp9b-encrypted-online-")
            .tempdir_in(parent)?;
        let scratch = scratch_parent.path().join("vault");
        manifest
            .persist_new(&scratch)
            .map_err(|_| IndexError::InvalidEvidence)?;

        let mut have_state = false;
        let mut count = 0usize;
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
            } else if ["archive", "document-objects", "backups"].contains(&name) && meta.is_dir() {
                copy_ciphertext_tree(&path, &scratch.join(name), 0, &mut count)?;
            } else if ["vault-v1.json", ".state-schema.lock", ".vault-writer.lock"].contains(&name)
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
        gate.validate_lease(lease).map_err(|_| IndexError::Revoked)?;
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
            let key_bytes = vault.derived_key(gate, lease, domain)
                .map_err(|_| IndexError::Revoked)?;
            let mut raw = [0u8; 32];
            raw.copy_from_slice(key_bytes.as_ref());
            let key = CipherKey::new(raw, gate, lease);
            raw.zeroize();
            let db = Connection::open_with_flags(
                &path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
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
        gate.validate_lease(lease).map_err(|_| IndexError::Revoked)?;
        // All SQLite handles above have closed before Windows directory rename.
        fs::rename(&staged, destination)?;
        #[cfg(unix)]
        File::open(parent)?.sync_all()?;
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
    File::open(destination)?.read_exact(&mut header)?;
    if &header == b"SQLite format 3\0" {
        return Err(IndexError::InvalidIndex);
    }
    Ok(())
}

fn copy_ciphertext_tree(root: &Path, target: &Path, depth: usize, count: &mut usize) -> Result<()> {
    if depth > 12 || *count > 100_000 {
        return Err(IndexError::InvalidEvidence);
    }
    guarded(root)?;
    fs::create_dir(target)?;
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let source = entry.path();
        guarded(&source)?;
        let file_name = entry.file_name();
        let output = target.join(file_name);
        let meta = fs::symlink_metadata(&source)?;
        if meta.is_dir() {
            copy_ciphertext_tree(&source, &output, depth + 1, count)?;
        } else if meta.is_file() {
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
            *count += 1;
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

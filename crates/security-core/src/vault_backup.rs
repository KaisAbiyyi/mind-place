//! Owner-triggered, generation-tagged portable encrypted backup (no automatic
//! retention, deletion, root reauthorization or legacy migration).
//! Source state must have already been quiesced and checkpointed by caller.
use crate::vault::{ContentKind, CryptoVault, VaultManifest};
use crate::{Result, SecurityError, SecurityGate, UnlockLease};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

const VERSION: u32 = 1;
const MAX_FILES: usize = 100_000;
const MAX_MANIFEST: usize = 24 * 1024 * 1024;
const MAX_FILE_SIZE: u64 = 8 * 1024 * 1024 * 1024;
const MAX_TOTAL_SIZE: u64 = 512 * 1024 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileReceipt {
    path: String,
    length: u64,
    sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackageManifest {
    version: u32,
    vault_id_b64: String,
    generation: u64,
    files: Vec<FileReceipt>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupMetadata {
    pub version: u32,
    pub vault_id_b64: String,
    pub generation: u64,
    pub count: usize,
}
pub struct PortableBackup;
impl PortableBackup {
    /// Caller chooses destination and retention; NEVER reads external approved roots.
    /// State must be a consistent encrypted SQLCipher snapshot (no open WAL).
    pub fn create(
        gate: &SecurityGate,
        lease: UnlockLease,
        vault: &CryptoVault,
        vault_root: &Path,
        destination: &Path,
        generation: u64,
    ) -> Result<BackupMetadata> {
        gate.validate_lease(lease)?;
        safe(vault_root)?;
        safe(destination)?;
        if destination.starts_with(vault_root) || vault_root.starts_with(destination) {
            return Err(SecurityError::UnsafePath);
        }
        let original = VaultManifest::read(vault_root)?;
        if original.vault_id_b64 != STANDARD.encode(vault.id()) {
            return Err(SecurityError::Crypto);
        }
        if destination.exists() {
            return Err(SecurityError::AlreadyConfigured);
        }
        // Conservative offline inventory: refuse unexpected app-owned files
        // rather than silently produce an incomplete "full" backup.
        let files = inventory(vault_root)?;
        if files.len() > MAX_FILES {
            return Err(SecurityError::InvalidConfig);
        }
        let staging = destination.with_extension("mp9b-partial");
        if staging.exists() {
            return Err(SecurityError::AlreadyConfigured);
        }
        let parent = destination.parent().ok_or(SecurityError::UnsafePath)?;
        safe(parent)?;
        fs::create_dir(&staging)?;
        // On errors the staging folder is intentionally left for explicit
        // owner disposition; never auto-delete snapshots on catch/rollback.
        let expected_files = files.clone();
        let mut receipts = Vec::with_capacity(files.len());
        let mut cumulative = 0u64;
        for rel in files {
            gate.validate_lease(lease)?;
            valid_relative(&rel)?;
            let source = vault_root.join(&rel);
            safe(&source)?;
            let meta = fs::metadata(&source)?;
            if !meta.is_file() || meta.len() > MAX_FILE_SIZE {
                return Err(SecurityError::InvalidConfig);
            }
            cumulative = cumulative
                .checked_add(meta.len())
                .ok_or(SecurityError::InvalidConfig)?;
            if cumulative > MAX_TOTAL_SIZE {
                return Err(SecurityError::InvalidConfig);
            }
            if !matches!(
                rel.as_path(),
                p if p == Path::new("state.sqlite3")
                    || p == Path::new("index.sqlite3")
                    || p == Path::new("document-index.sqlite3")
            ) && !starts_with(&source, b"MPOS\x01")?
            {
                return Err(SecurityError::Crypto);
            }
            let target = staging.join("payload").join(&rel);
            fs::create_dir_all(target.parent().ok_or(SecurityError::UnsafePath)?)?;
            let mut input = File::open(&source)?;
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)?;
            let (size, digest) = copy_digest(&mut input, &mut output)?;
            output.sync_all()?;
            if size != meta.len() {
                return Err(SecurityError::Crypto);
            }
            let relative = rel
                .to_str()
                .ok_or(SecurityError::InvalidConfig)?
                .replace('\\', "/");
            receipts.push(FileReceipt {
                path: relative,
                length: size,
                sha256: digest,
            });
        }
        // Detect source changes or newly added immutable objects before publish.
        // This is a fail-closed offline verification, NOT an online snapshot
        // coordinator. Writers must still be stopped by the caller.
        verify_quiescent_sources(vault_root, &expected_files, &receipts)?;
        gate.validate_lease(lease)?;
        let manifest = PackageManifest {
            version: VERSION,
            vault_id_b64: STANDARD.encode(vault.id()),
            generation,
            files: receipts,
        };
        let body = serde_json::to_vec(&manifest).map_err(|_| SecurityError::Crypto)?;
        if body.len() > MAX_MANIFEST {
            return Err(SecurityError::InvalidConfig);
        }
        let ciphertext = vault.seal(
            gate,
            lease,
            ContentKind::Backup,
            &generation.to_be_bytes(),
            &body,
        )?;
        let mut encrypted_manifest = File::create_new(staging.join("manifest.enc"))?;
        encrypted_manifest.write_all(&ciphertext)?;
        encrypted_manifest.sync_all()?;
        let meta = BackupMetadata {
            version: VERSION,
            vault_id_b64: manifest.vault_id_b64,
            generation,
            count: manifest.files.len(),
        };
        let mut metadata_file = File::create_new(staging.join("bundle.json"))?;
        metadata_file.write_all(&serde_json::to_vec(&meta).map_err(|_| SecurityError::Crypto)?)?;
        metadata_file.sync_all()?;
        fs::write(
            staging.join("vault-v1.json"),
            serde_json::to_vec(&original).map_err(|_| SecurityError::Crypto)?,
        )?;
        // Windows denies renaming directories containing files whose open
        // handles do not share DELETE. Finish and close *all* staging writers
        // before publishing the complete snapshot via atomic rename.
        drop(metadata_file);
        drop(encrypted_manifest);
        gate.validate_lease(lease)?;
        fs::rename(&staging, destination)?;
        sync_parent(parent)?;
        Ok(meta)
    }

    /// Recovery is independent of the original Windows user/machine DPAPI blob.
    /// Only a new output path is accepted; no overwrite or external root grant.
    pub fn restore(
        gate: &SecurityGate,
        lease: UnlockLease,
        backup_root: &Path,
        destination: &Path,
        recovery: &crate::vault::RecoverySecret,
    ) -> Result<BackupMetadata> {
        gate.validate_lease(lease)?;
        safe(backup_root)?;
        safe(destination)?;
        if backup_root.starts_with(destination) || destination.starts_with(backup_root) {
            return Err(SecurityError::UnsafePath);
        }
        if destination.exists() {
            return Err(SecurityError::AlreadyConfigured);
        }
        let original = VaultManifest::read(backup_root)?;
        let vault = CryptoVault::restore_with_recovery(gate, lease, &original, recovery)?;
        let meta: BackupMetadata =
            serde_json::from_slice(&bounded(&backup_root.join("bundle.json"), 4096)?)
                .map_err(|_| SecurityError::InvalidConfig)?;
        if meta.version != VERSION || meta.vault_id_b64 != STANDARD.encode(vault.id()) {
            return Err(SecurityError::Crypto);
        }
        let bytes = bounded(&backup_root.join("manifest.enc"), MAX_MANIFEST + 100)?;
        let clear = vault.unseal(
            gate,
            lease,
            ContentKind::Backup,
            &meta.generation.to_be_bytes(),
            &bytes,
        )?;
        let manifest: PackageManifest =
            serde_json::from_slice(&clear).map_err(|_| SecurityError::Crypto)?;
        if manifest.version != VERSION
            || manifest.vault_id_b64 != meta.vault_id_b64
            || manifest.generation != meta.generation
            || manifest.files.len() != meta.count
            || manifest.files.len() > MAX_FILES
        {
            return Err(SecurityError::Crypto);
        }
        // Verify everything before creating even the staging destination.
        let mut seen = std::collections::HashSet::new();
        let mut total = 0u64;
        for receipt in &manifest.files {
            gate.validate_lease(lease)?;
            let relative = Path::new(&receipt.path);
            valid_relative(relative)?;
            if !allowed(relative)
                || !seen.insert(receipt.path.clone())
                || receipt.length > MAX_FILE_SIZE
            {
                return Err(SecurityError::Crypto);
            }
            total = total
                .checked_add(receipt.length)
                .ok_or(SecurityError::Crypto)?;
            if total > MAX_TOTAL_SIZE {
                return Err(SecurityError::Crypto);
            }
            let source = backup_root.join("payload").join(relative);
            safe(&source)?;
            let mut input = File::open(source)?;
            let (length, digest) = digest_file(&mut input)?;
            if length != receipt.length || digest != receipt.sha256 {
                return Err(SecurityError::Crypto);
            }
        }
        if !seen.contains("state.sqlite3") {
            return Err(SecurityError::Crypto);
        }
        let staging = destination.with_extension("mp9b-restore-partial");
        if staging.exists() {
            return Err(SecurityError::AlreadyConfigured);
        }
        fs::create_dir(&staging)?;
        let mut config = File::create_new(staging.join("vault-v1.json"))?;
        config.write_all(&serde_json::to_vec(&original).map_err(|_| SecurityError::Crypto)?)?;
        config.sync_all()?;
        for receipt in &manifest.files {
            gate.validate_lease(lease)?;
            let relative = Path::new(&receipt.path);
            let path = staging.join(relative);
            fs::create_dir_all(path.parent().ok_or(SecurityError::UnsafePath)?)?;
            let mut out = File::create_new(&path)?;
            let mut input = File::open(backup_root.join("payload").join(relative))?;
            let (length, digest) = copy_digest(&mut input, &mut out)?;
            out.sync_all()?;
            if length != receipt.length || digest != receipt.sha256 {
                return Err(SecurityError::Crypto);
            }
        }
        // Close the restored manifest handle before directory publication.
        // Otherwise Windows reports ERROR_ACCESS_DENIED on the rename.
        drop(config);
        gate.validate_lease(lease)?;
        fs::rename(&staging, destination)?;
        sync_parent(destination.parent().ok_or(SecurityError::UnsafePath)?)?;
        Ok(meta)
    }
}
fn allowed(relative: &Path) -> bool {
    let mut components = relative.components();
    match components.next() {
        Some(Component::Normal(first))
            if first == "state.sqlite3"
                || first == "index.sqlite3"
                || first == "document-index.sqlite3" =>
        {
            components.next().is_none()
        }
        Some(Component::Normal(first))
            if first == "archive" || first == "document-objects" || first == "backups" =>
        {
            components.next().is_some()
        }
        _ => false,
    }
}
/// This rejects obvious archive/index changes during an offline backup.
/// It is not a replacement for an application-wide writer/quiescence gate.
fn verify_quiescent_sources(
    root: &Path,
    expected: &[PathBuf],
    receipts: &[FileReceipt],
) -> Result<()> {
    let current = inventory(root)?;
    if current != expected || expected.len() != receipts.len() {
        return Err(SecurityError::Crypto);
    }
    for (relative, receipt) in expected.iter().zip(receipts) {
        if relative.to_string_lossy().replace('\\', "/") != receipt.path {
            return Err(SecurityError::Crypto);
        }
        let source = root.join(relative);
        safe(&source)?;
        let mut stream = File::open(source)?;
        let (len, digest) = digest_file(&mut stream)?;
        if len != receipt.length || digest != receipt.sha256 {
            return Err(SecurityError::Crypto);
        }
    }
    Ok(())
}
/// Dedicated-vault allowlist. New root layouts MUST update the backup contract
/// before rollout; otherwise partial "complete backups" would silently omit
/// state. Only zero-length schema locks are excluded.
fn inventory(root: &Path) -> Result<Vec<PathBuf>> {
    safe(root)?;
    let mut files = Vec::new();
    let mut has_state = false;
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        safe(&path)?;
        let name = entry.file_name();
        let name = name.to_str().ok_or(SecurityError::InvalidConfig)?;
        let meta = fs::symlink_metadata(&path)?;
        match name {
            "vault-v1.json" if meta.is_file() => {}
            ".state-schema.lock" if meta.is_file() && meta.len() == 0 => {}
            "state.sqlite3" | "index.sqlite3" | "document-index.sqlite3" if meta.is_file() => {
                for suffix in ["-wal", "-shm", "-journal"] {
                    if root.join(format!("{name}{suffix}")).exists() {
                        return Err(SecurityError::InvalidConfig);
                    }
                }
                if starts_with(&path, b"SQLite format 3\0")? {
                    return Err(SecurityError::Crypto);
                }
                if name == "state.sqlite3" {
                    has_state = true;
                }
                files.push(PathBuf::from(name));
            }
            "archive" | "document-objects" | "backups" if meta.is_dir() => {
                gather(root, &path, &mut files)?;
            }
            // Unknown/unmigrated state, WAL, plaintext cache, directory, or
            // unclassified app-owned file makes the backup unsafe/incomplete.
            _ => return Err(SecurityError::InvalidConfig),
        }
        if files.len() > MAX_FILES {
            return Err(SecurityError::InvalidConfig);
        }
    }
    if !has_state {
        return Err(SecurityError::InvalidConfig);
    }
    files.sort();
    Ok(files)
}
fn valid_relative(path: &Path) -> Result<()> {
    if !allowed(path)
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(SecurityError::UnsafePath);
    }
    Ok(())
}
fn safe(path: &Path) -> Result<()> {
    for component in path.ancestors() {
        if !component.as_os_str().is_empty() {
            super::validate_path(component)?;
        }
    }
    Ok(())
}
fn gather(root: &Path, folder: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    safe(folder)?;
    for item in fs::read_dir(folder)? {
        if files.len() >= MAX_FILES {
            return Err(SecurityError::InvalidConfig);
        }
        let path = item?.path();
        safe(&path)?;
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.is_dir() {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| SecurityError::UnsafePath)?;
            if relative.components().count() > 5 {
                return Err(SecurityError::InvalidConfig);
            }
            gather(root, &path, files)?;
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| SecurityError::UnsafePath)?
                .to_owned();
            valid_relative(&relative)?;
            files.push(relative);
        } else {
            return Err(SecurityError::UnsafePath);
        }
    }
    Ok(())
}
fn starts_with(path: &Path, needle: &[u8]) -> Result<bool> {
    let mut file = File::open(path)?;
    let mut prefix = vec![0u8; needle.len()];
    Ok(file.read(&mut prefix)? == needle.len() && prefix == needle)
}
fn bounded(path: &Path, max: usize) -> Result<Vec<u8>> {
    safe(path)?;
    let mut file = File::open(path)?;
    if file.metadata()?.len() > max as u64 {
        return Err(SecurityError::InvalidConfig);
    }
    let mut out = Vec::new();
    file.take((max + 1) as u64).read_to_end(&mut out)?;
    if out.len() > max {
        return Err(SecurityError::InvalidConfig);
    }
    Ok(out)
}
fn digest_file(src: &mut File) -> Result<(u64, String)> {
    let mut sha = Sha256::new();
    let mut len = 0u64;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let amount = src.read(&mut buf)?;
        if amount == 0 {
            break;
        }
        len = len
            .checked_add(amount as u64)
            .ok_or(SecurityError::Crypto)?;
        sha.update(&buf[..amount]);
    }
    Ok((len, format!("{:x}", sha.finalize())))
}
fn copy_digest(src: &mut File, dst: &mut File) -> Result<(u64, String)> {
    let mut sha = Sha256::new();
    let mut len = 0u64;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let amount = src.read(&mut buf)?;
        if amount == 0 {
            break;
        }
        len = len
            .checked_add(amount as u64)
            .ok_or(SecurityError::Crypto)?;
        dst.write_all(&buf[..amount])?;
        sha.update(&buf[..amount]);
    }
    Ok((len, format!("{:x}", sha.finalize())))
}
fn sync_parent(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault_storage::ImmutableVaultStorage;
    fn fixture() -> (
        tempfile::TempDir,
        SecurityGate,
        UnlockLease,
        CryptoVault,
        crate::vault::RecoverySecret,
    ) {
        let temp = tempfile::tempdir().unwrap();
        let gate = SecurityGate::open(temp.path().join("security.json")).unwrap();
        gate.initialize(b"synthetic-mp9b-recovery-secret").unwrap();
        let lease = gate.require_unlocked().unwrap();
        let (vault, recovery, manifest) = CryptoVault::provision(&gate, lease).unwrap();
        let root = temp.path().join("vault");
        manifest.persist_new(&root).unwrap();
        // Synthetic encrypted-looking state header; production must validate
        // SQLCipher identity/FTS with StateStore before calling create.
        fs::write(
            root.join("state.sqlite3"),
            [b"SQLCIPHER_FAKE_TEST_FIXTURE".as_slice(), &[0x53; 4096]].concat(),
        )
        .unwrap();
        (temp, gate, lease, vault, recovery)
    }
    #[test]
    fn portable_backup_restore_and_tamper_are_fail_closed() {
        let (temp, gate, lease, vault, recovery) = fixture();
        let root = temp.path().join("vault");
        let store = ImmutableVaultStorage::new(&root, &vault, &gate, lease).unwrap();
        store
            .write_immutable(
                &gate,
                lease,
                ContentKind::Archive,
                b"chatgpt:rev:1",
                b"synthetic conversation",
            )
            .unwrap();
        store
            .write_immutable(
                &gate,
                lease,
                ContentKind::DocumentObject,
                b"document:hash:1",
                b"synthetic document",
            )
            .unwrap();
        store
            .write_immutable(
                &gate,
                lease,
                ContentKind::Backup,
                b"prior-generation",
                b"synthetic prior backup object",
            )
            .unwrap();
        let backup = temp.path().join("backup");
        let meta = PortableBackup::create(&gate, lease, &vault, &root, &backup, 7).unwrap();
        assert_eq!(meta.count, 4);
        let restore = temp.path().join("restored");
        let restored = PortableBackup::restore(&gate, lease, &backup, &restore, &recovery).unwrap();
        assert_eq!(restored.count, 4);
        assert_eq!(
            fs::read(restore.join("state.sqlite3")).unwrap(),
            fs::read(root.join("state.sqlite3")).unwrap()
        );
        assert!(PortableBackup::restore(&gate, lease, &backup, &restore, &recovery).is_err());
        let state_in_backup = backup.join("payload/state.sqlite3");
        let mut data = fs::read(&state_in_backup).unwrap();
        data[12] ^= 1;
        fs::write(state_in_backup, data).unwrap();
        assert!(PortableBackup::restore(
            &gate,
            lease,
            &backup,
            &temp.path().join("tampered-restore"),
            &recovery
        )
        .is_err());
    }
    #[test]
    fn backup_covers_encrypted_derived_databases_and_rejects_active_wal() {
        let (temp, gate, lease, vault, recovery) = fixture();
        let root = temp.path().join("vault");
        for name in ["index.sqlite3", "document-index.sqlite3"] {
            fs::write(
                root.join(name),
                [b"ENCRYPTED_INDEX_FIXTURE".as_slice(), &[0x27; 128]].concat(),
            )
            .unwrap();
        }
        let backup = temp.path().join("complete");
        let receipt = PortableBackup::create(&gate, lease, &vault, &root, &backup, 91).unwrap();
        assert_eq!(receipt.count, 3);
        let restored = temp.path().join("complete-restored");
        PortableBackup::restore(&gate, lease, &backup, &restored, &recovery).unwrap();
        for name in ["state.sqlite3", "index.sqlite3", "document-index.sqlite3"] {
            assert_eq!(
                fs::read(root.join(name)).unwrap(),
                fs::read(restored.join(name)).unwrap()
            );
        }
        fs::write(root.join("index.sqlite3-wal"), b"pending page").unwrap();
        let blocked = temp.path().join("unquiesced");
        assert!(PortableBackup::create(&gate, lease, &vault, &root, &blocked, 92).is_err());
        assert!(!blocked.exists());
        fs::remove_file(root.join("index.sqlite3-wal")).unwrap();
        fs::write(
            root.join("document-index.sqlite3"),
            b"SQLite format 3\0plaintext",
        )
        .unwrap();
        let plaintext = temp.path().join("plaintext-index");
        assert!(PortableBackup::create(&gate, lease, &vault, &root, &plaintext, 93).is_err());
        assert!(!plaintext.exists());
    }
    #[test]
    fn quiescent_validator_detects_added_removed_or_changed_files() {
        let (temp, gate, lease, vault, _) = fixture();
        let root = temp.path().join("vault");
        let backup = temp.path().join("initial");
        PortableBackup::create(&gate, lease, &vault, &root, &backup, 17).unwrap();
        let original = fs::read(root.join("state.sqlite3")).unwrap();
        let receipt = FileReceipt {
            path: "state.sqlite3".into(),
            length: original.len() as u64,
            sha256: format!("{:x}", Sha256::digest(&original)),
        };
        let paths = vec![PathBuf::from("state.sqlite3")];
        verify_quiescent_sources(
            &root,
            &paths,
            &[FileReceipt {
                path: receipt.path.clone(),
                length: receipt.length,
                sha256: receipt.sha256.clone(),
            }],
        )
        .unwrap();
        fs::write(root.join("state.sqlite3"), b"modified").unwrap();
        assert!(verify_quiescent_sources(&root, &paths, &[receipt]).is_err());
        fs::write(root.join("state.sqlite3"), original).unwrap();
        fs::write(root.join("index.sqlite3"), b"ENCRYPTED").unwrap();
        let receipts = vec![FileReceipt {
            path: "state.sqlite3".into(),
            length: fs::metadata(root.join("state.sqlite3")).unwrap().len(),
            sha256: format!(
                "{:x}",
                Sha256::digest(fs::read(root.join("state.sqlite3")).unwrap())
            ),
        }];
        assert!(verify_quiescent_sources(&root, &paths, &receipts).is_err());
    }
    #[test]
    fn refuse_unclassified_vault_state_and_nonempty_schema_lock() {
        let (temp, gate, lease, vault, _) = fixture();
        let root = temp.path().join("vault");
        fs::write(
            root.join("unmigrated-preferences.json"),
            b"{\"private\":true}",
        )
        .unwrap();
        let destination = temp.path().join("incomplete-backup");
        assert!(PortableBackup::create(&gate, lease, &vault, &root, &destination, 30).is_err());
        assert!(!destination.exists());
        fs::remove_file(root.join("unmigrated-preferences.json")).unwrap();
        fs::write(root.join(".state-schema.lock"), b"private").unwrap();
        assert!(PortableBackup::create(&gate, lease, &vault, &root, &destination, 31).is_err());
        fs::write(root.join(".state-schema.lock"), b"").unwrap();
        PortableBackup::create(&gate, lease, &vault, &root, &destination, 32).unwrap();
    }

    #[test]
    fn never_mistake_legacy_plaintext_for_an_encrypted_state() {
        let (temp, gate, lease, vault, _) = fixture();
        let root = temp.path().join("vault");
        fs::write(
            root.join("state.sqlite3"),
            b"SQLite format 3\0not-encrypted",
        )
        .unwrap();
        let backup = temp.path().join("backup");
        assert!(PortableBackup::create(&gate, lease, &vault, &root, &backup, 1).is_err());
        assert!(!backup.exists());
    }
}

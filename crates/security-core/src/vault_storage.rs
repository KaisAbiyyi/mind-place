//! Durable immutable AEAD storage for new vaults. All temporary files contain
//! ciphertext only. Existing plaintext archives are NEVER adopted or migrated.
use crate::vault::{ContentKind, CryptoVault, VaultManifest};
use crate::vault_fence::VaultWriterFence;
use crate::{Result, SecurityError, SecurityGate, UnlockLease};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use zeroize::{Zeroize, Zeroizing};

const CHUNK: usize = 1024 * 1024;
const MAX_TOTAL: usize = 64 * CHUNK;
const HEADER_LEN: usize = 49;
const MAGIC: &[u8; 5] = b"MPOS\x01";
const OVERHEAD_PER_CHUNK: usize = 5 + 12 + 16 + 4;

pub struct ImmutableVaultStorage<'a> {
    root: PathBuf,
    vault: &'a CryptoVault,
}
impl<'a> ImmutableVaultStorage<'a> {
    /// Verify the standalone manifest before storing any encrypted bytes.
    pub fn new(
        root: impl AsRef<Path>,
        vault: &'a CryptoVault,
        gate: &SecurityGate,
        lease: UnlockLease,
    ) -> Result<Self> {
        gate.validate_lease(lease)?;
        let root = root.as_ref();
        safe_ancestors(root)?;
        let manifest = VaultManifest::read(root)?;
        let id = STANDARD.encode(vault.id());
        if manifest.vault_id_b64 != id {
            return Err(SecurityError::Crypto);
        }
        Ok(Self {
            root: root.to_owned(),
            vault,
        })
    }

    /// A keyed plaintext commitment prevents cross-vault equality leaks that
    /// would occur if the immutable file header contained a bare SHA-256.
    fn commitment(
        &self,
        gate: &SecurityGate,
        lease: UnlockLease,
        input: &[u8],
    ) -> Result<[u8; 32]> {
        let key = self
            .vault
            .derived_key(gate, lease, b"object-content-commitment/v1")?;
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key.as_ref())
            .map_err(|_| SecurityError::Crypto)?;
        mac.update(input);
        let digest = mac.finalize().into_bytes();
        let mut out = [0u8; 32];
        out.copy_from_slice(&digest);
        gate.validate_lease(lease)?;
        Ok(out)
    }
    fn location(
        &self,
        gate: &SecurityGate,
        lease: UnlockLease,
        kind: ContentKind,
        identity: &[u8],
    ) -> Result<PathBuf> {
        let category = match kind {
            ContentKind::Archive => "archive",
            ContentKind::DocumentObject => "document-objects",
            ContentKind::Backup => "backups",
        };
        let name = self.vault.storage_name(gate, lease, kind, identity)?;
        Ok(self.root.join(category).join(&name[..2]).join(name))
    }

    pub fn write_immutable(
        &self,
        gate: &SecurityGate,
        lease: UnlockLease,
        kind: ContentKind,
        identity: &[u8],
        payload: &[u8],
    ) -> Result<PathBuf> {
        gate.validate_lease(lease)?;
        // Cooperating native/desktop writers must hold the same shared fence
        // through immutable object publication and directory sync.
        let _writer_fence = VaultWriterFence::writer(&self.root)?;
        if payload.len() > MAX_TOTAL {
            return Err(SecurityError::InvalidConfig);
        }
        let path = self.location(gate, lease, kind, identity)?;
        let parent = path.parent().ok_or(SecurityError::UnsafePath)?;
        safe_ancestors(parent)?;
        // Never use a name derived from untrusted input to escape the vault.
        fs::create_dir_all(parent)?;
        safe_ancestors(parent)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        let chunks = (payload.len().saturating_add(CHUNK - 1) / CHUNK).max(1);
        let mut header = [0u8; HEADER_LEN];
        header[0..5].copy_from_slice(MAGIC);
        header[5..13].copy_from_slice(&(payload.len() as u64).to_be_bytes());
        header[13..17].copy_from_slice(&(chunks as u32).to_be_bytes());
        header[17..49].copy_from_slice(&self.commitment(gate, lease, payload)?);
        file.write_all(&header)?;
        for i in 0..chunks {
            gate.validate_lease(lease)?;
            let offset = i * CHUNK;
            let end = payload.len().min(offset + CHUNK);
            let mut aad_id = Vec::with_capacity(identity.len() + HEADER_LEN + 16);
            aad_id.extend_from_slice(b"chunk/");
            aad_id.extend_from_slice(identity);
            aad_id.extend_from_slice(&header);
            aad_id.extend_from_slice(&(i as u32).to_be_bytes());
            let encrypted = self
                .vault
                .seal(gate, lease, kind, &aad_id, &payload[offset..end])?;
            file.write_all(&(encrypted.len() as u32).to_be_bytes())?;
            file.write_all(&encrypted)?;
        }
        file.as_file().sync_all()?;
        gate.validate_lease(lease)?;
        // Never replace an immutable evidence object, even after a crash.
        if path.exists() {
            let previous = self.read(gate, lease, kind, identity)?;
            if previous.as_slice() == payload {
                return Ok(path);
            }
            return Err(SecurityError::Crypto);
        }
        file.persist_noclobber(&path).map_err(|e| e.error)?;
        sync_parent(parent)?;
        gate.validate_lease(lease)?;
        Ok(path)
    }

    pub fn read(
        &self,
        gate: &SecurityGate,
        lease: UnlockLease,
        kind: ContentKind,
        identity: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>> {
        gate.validate_lease(lease)?;
        let path = self.location(gate, lease, kind, identity)?;
        safe_ancestors(&path)?;
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.is_file()
            || metadata.len() > (MAX_TOTAL + 64 * OVERHEAD_PER_CHUNK + HEADER_LEN) as u64
        {
            return Err(SecurityError::Crypto);
        }
        let mut file = File::open(&path)?;
        let mut header = [0u8; HEADER_LEN];
        file.read_exact(&mut header)?;
        if &header[0..5] != MAGIC {
            return Err(SecurityError::Crypto);
        }
        let total = u64::from_be_bytes(
            header[5..13]
                .try_into()
                .map_err(|_| SecurityError::Crypto)?,
        ) as usize;
        let chunks = u32::from_be_bytes(
            header[13..17]
                .try_into()
                .map_err(|_| SecurityError::Crypto)?,
        ) as usize;
        let expected_chunks = (total.saturating_add(CHUNK - 1) / CHUNK).max(1);
        if total > MAX_TOTAL || chunks != expected_chunks {
            return Err(SecurityError::Crypto);
        }
        let mut assembled = Zeroizing::new(Vec::with_capacity(total));
        for i in 0..chunks {
            gate.validate_lease(lease)?;
            let mut len = [0u8; 4];
            file.read_exact(&mut len)?;
            let encrypted_len = u32::from_be_bytes(len) as usize;
            if !(33..=CHUNK + OVERHEAD_PER_CHUNK).contains(&encrypted_len) {
                return Err(SecurityError::Crypto);
            }
            let mut encrypted = vec![0u8; encrypted_len];
            file.read_exact(&mut encrypted)?;
            let mut aad_id = Vec::with_capacity(identity.len() + HEADER_LEN + 16);
            aad_id.extend_from_slice(b"chunk/");
            aad_id.extend_from_slice(identity);
            aad_id.extend_from_slice(&header);
            aad_id.extend_from_slice(&(i as u32).to_be_bytes());
            let plaintext = self.vault.unseal(gate, lease, kind, &aad_id, &encrypted)?;
            let expected_len = (total - assembled.len()).min(CHUNK);
            if plaintext.len() != expected_len {
                return Err(SecurityError::Crypto);
            }
            assembled.extend_from_slice(&plaintext);
            encrypted.zeroize();
        }
        let mut trailing = [0u8; 1];
        if file.read(&mut trailing)? != 0
            || assembled.len() != total
            || self
                .commitment(gate, lease, assembled.as_slice())?
                .as_slice()
                != &header[17..49]
        {
            return Err(SecurityError::Crypto);
        }
        gate.validate_lease(lease)?;
        Ok(assembled)
    }
}
fn safe_ancestors(path: &Path) -> Result<()> {
    for component in path.ancestors() {
        if !component.as_os_str().is_empty() {
            super::validate_path(component)?;
        }
    }
    Ok(())
}
fn sync_parent(parent: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = parent;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::Digest;
    fn fixture() -> (tempfile::TempDir, SecurityGate, UnlockLease, CryptoVault) {
        let dir = tempfile::tempdir().unwrap();
        let security = SecurityGate::open(dir.path().join("security.json")).unwrap();
        security
            .initialize(b"synthetic fixture unlock secret")
            .unwrap();
        let lease = security.require_unlocked().unwrap();
        let (vault, _, manifest) = CryptoVault::provision(&security, lease).unwrap();
        manifest.persist_new(&dir.path().join("vault")).unwrap();
        (dir, security, lease, vault)
    }
    #[test]
    fn encrypted_archive_and_document_readback_no_plaintext() {
        let (dir, gate, lease, vault) = fixture();
        let store =
            ImmutableVaultStorage::new(dir.path().join("vault"), &vault, &gate, lease).unwrap();
        let content = vec![0x54; 2 * CHUNK + 17];
        let p = store
            .write_immutable(
                &gate,
                lease,
                ContentKind::DocumentObject,
                b"source-1",
                &content,
            )
            .unwrap();
        let bytes = fs::read(&p).unwrap();
        // Public SHA256 of plaintext must never be exposed in ciphertext headers.
        assert_ne!(&bytes[17..49], sha2::Sha256::digest(&content).as_slice());
        assert!(!bytes
            .windows(24)
            .any(|part| part.iter().all(|b| *b == 0x54)));
        let restored = store
            .read(&gate, lease, ContentKind::DocumentObject, b"source-1")
            .unwrap();
        assert_eq!(restored.as_slice(), &content);
        assert_eq!(
            store
                .write_immutable(
                    &gate,
                    lease,
                    ContentKind::DocumentObject,
                    b"source-1",
                    &content
                )
                .unwrap(),
            p
        );
        assert!(store
            .read(&gate, lease, ContentKind::Archive, b"source-1")
            .is_err());
    }
    #[test]
    fn corruption_and_stale_lease_fail_closed() {
        let (dir, gate, lease, vault) = fixture();
        let store =
            ImmutableVaultStorage::new(dir.path().join("vault"), &vault, &gate, lease).unwrap();
        let p = store
            .write_immutable(
                &gate,
                lease,
                ContentKind::Archive,
                b"version-3",
                b"synthetic text",
            )
            .unwrap();
        let mut corrupted = fs::read(&p).unwrap();
        *corrupted.last_mut().unwrap() ^= 0x55;
        fs::write(&p, corrupted).unwrap();
        assert!(store
            .read(&gate, lease, ContentKind::Archive, b"version-3")
            .is_err());
        gate.lock();
        assert!(matches!(
            store.read(&gate, lease, ContentKind::Archive, b"version-3"),
            Err(SecurityError::Locked)
        ));
    }
    #[test]
    fn cross_vault_and_different_source_are_rejected() {
        let (dir, gate, lease, vault) = fixture();
        let store =
            ImmutableVaultStorage::new(dir.path().join("vault"), &vault, &gate, lease).unwrap();
        store
            .write_immutable(&gate, lease, ContentKind::Archive, b"source-A", b"A")
            .unwrap();
        assert!(store
            .read(&gate, lease, ContentKind::Archive, b"source-B")
            .is_err());
        let (another, _, _) = CryptoVault::provision(&gate, lease).unwrap();
        assert!(
            ImmutableVaultStorage::new(dir.path().join("vault"), &another, &gate, lease).is_err()
        );
    }
}

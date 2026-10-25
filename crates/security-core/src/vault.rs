//! MP-9B isolated vault primitive. No automatic legacy migration or app-data access.
//! The caller must carry a live SecurityGate lease for every disclosure.
use super::{Result, SecurityError, SecurityGate, UnlockLease};
use aes_gcm::{
    aead::{Aead, Payload},
    Aes256Gcm, KeyInit, Nonce,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
};
use zeroize::{Zeroize, Zeroizing};

const FORMAT_VERSION: u32 = 1;
const ENVELOPE_PREFIX: &[u8; 5] = b"MPVE\x01";
const MAX_MANIFEST_BYTES: u64 = 16 * 1024;
const MAX_PAYLOAD_BYTES: usize = 32 * 1024 * 1024;
const MAX_ID_BYTES: usize = 4096;

/// Independently generated 256-bit offline recovery secret; NOT a PIN or verifier.
/// A caller must explicitly display/export this once and verify secure retention.
pub struct RecoverySecret(Zeroizing<[u8; 32]>);
impl RecoverySecret {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
    pub fn from_exported_bytes(bytes: [u8; 32]) -> Self {
        Self(Zeroizing::new(bytes))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultManifest {
    pub format_version: u32,
    pub vault_id_b64: String,
    pub recovery_nonce_b64: String,
    pub recovery_wrapped_dek_b64: String,
    /// User/machine-scoped convenience only, not a portable recovery mechanism.
    pub windows_dpapi_wrapped_dek_b64: Option<String>,
}
pub struct CryptoVault {
    vault_id: [u8; 16],
    dek: Zeroizing<[u8; 32]>,
}
#[derive(Clone, Copy)]
pub enum ContentKind {
    Archive,
    DocumentObject,
    Backup,
}
impl ContentKind {
    fn label(self) -> &'static [u8] {
        match self {
            Self::Archive => b"archive",
            Self::DocumentObject => b"document-object",
            Self::Backup => b"portable-backup",
        }
    }
}
fn random<const N: usize>() -> Result<[u8; N]> {
    let mut value = [0u8; N];
    getrandom::fill(&mut value).map_err(|_| SecurityError::Crypto)?;
    Ok(value)
}
fn fixed<const N: usize>(value: &str) -> Result<[u8; N]> {
    let decoded = STANDARD
        .decode(value)
        .map_err(|_| SecurityError::InvalidConfig)?;
    decoded.try_into().map_err(|_| SecurityError::InvalidConfig)
}
fn guarded_path(path: &Path) -> Result<()> {
    for part in path.ancestors() {
        if !part.as_os_str().is_empty() {
            super::validate_path(part)?;
        }
    }
    Ok(())
}
fn cipher(key: &[u8; 32]) -> Result<Aes256Gcm> {
    Aes256Gcm::new_from_slice(key).map_err(|_| SecurityError::Crypto)
}
fn associated(vault_id: &[u8; 16], domain: &[u8], identity: &[u8]) -> Result<Vec<u8>> {
    if identity.len() > MAX_ID_BYTES {
        return Err(SecurityError::InvalidConfig);
    }
    let mut aad = Vec::with_capacity(40 + domain.len() + identity.len());
    aad.extend_from_slice(b"mind-place/vault/1");
    aad.extend_from_slice(vault_id);
    aad.extend_from_slice(&(domain.len() as u32).to_be_bytes());
    aad.extend_from_slice(domain);
    aad.extend_from_slice(&(identity.len() as u32).to_be_bytes());
    aad.extend_from_slice(identity);
    Ok(aad)
}
impl CryptoVault {
    /// New, isolated vault only. The legacy SecurityGate verifier is *never* key material.
    pub fn provision(
        gate: &SecurityGate,
        lease: UnlockLease,
    ) -> Result<(Self, RecoverySecret, VaultManifest)> {
        gate.validate_lease(lease)?;
        let vault_id = random::<16>()?;
        let dek = Zeroizing::new(random::<32>()?);
        let recovery = RecoverySecret(Zeroizing::new(random::<32>()?));
        let nonce = random::<12>()?;
        let aad = associated(&vault_id, b"recovery-dek", b"v1")?;
        let encrypted = cipher(&recovery.0)?
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: dek.as_ref(),
                    aad: &aad,
                },
            )
            .map_err(|_| SecurityError::Crypto)?;
        let dpapi_wrapped = dpapi::protect(&dek, &vault_id)?;
        gate.validate_lease(lease)?;
        let manifest = VaultManifest {
            format_version: FORMAT_VERSION,
            vault_id_b64: STANDARD.encode(vault_id),
            recovery_nonce_b64: STANDARD.encode(nonce),
            recovery_wrapped_dek_b64: STANDARD.encode(encrypted),
            windows_dpapi_wrapped_dek_b64: dpapi_wrapped.map(|bytes| STANDARD.encode(bytes)),
        };
        Ok((Self { vault_id, dek }, recovery, manifest))
    }

    pub fn restore_with_recovery(
        gate: &SecurityGate,
        lease: UnlockLease,
        manifest: &VaultManifest,
        recovery: &RecoverySecret,
    ) -> Result<Self> {
        gate.validate_lease(lease)?;
        let vault_id = manifest.validate()?;
        let nonce = fixed::<12>(&manifest.recovery_nonce_b64)?;
        let encrypted = STANDARD
            .decode(&manifest.recovery_wrapped_dek_b64)
            .map_err(|_| SecurityError::InvalidConfig)?;
        let aad = associated(&vault_id, b"recovery-dek", b"v1")?;
        let mut plain = cipher(&recovery.0)?
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &encrypted,
                    aad: &aad,
                },
            )
            .map_err(|_| SecurityError::Crypto)?;
        if plain.len() != 32 {
            plain.zeroize();
            return Err(SecurityError::Crypto);
        }
        let mut dek = Zeroizing::new([0u8; 32]);
        dek.copy_from_slice(&plain);
        plain.zeroize();
        gate.validate_lease(lease)?;
        Ok(Self { vault_id, dek })
    }

    /// Convenience unlock is unavailable outside Windows; recovery remains portable.
    pub fn restore_with_dpapi(
        gate: &SecurityGate,
        lease: UnlockLease,
        manifest: &VaultManifest,
    ) -> Result<Self> {
        gate.validate_lease(lease)?;
        let vault_id = manifest.validate()?;
        let wrapped = manifest
            .windows_dpapi_wrapped_dek_b64
            .as_deref()
            .ok_or(SecurityError::Crypto)?;
        let decoded = STANDARD
            .decode(wrapped)
            .map_err(|_| SecurityError::InvalidConfig)?;
        let mut clear = dpapi::unprotect(&decoded, &vault_id)?.ok_or(SecurityError::Crypto)?;
        if clear.len() != 32 {
            clear.zeroize();
            return Err(SecurityError::Crypto);
        }
        let mut dek = Zeroizing::new([0u8; 32]);
        dek.copy_from_slice(&clear);
        clear.zeroize();
        gate.validate_lease(lease)?;
        Ok(Self { vault_id, dek })
    }

    pub fn id(&self) -> [u8; 16] {
        self.vault_id
    }

    /// Envelopes are versioned and AEAD-bound to vault, kind and canonical identity.
    /// Nonces are randomized using the system CSPRNG for each immutable write.
    pub fn seal(
        &self,
        gate: &SecurityGate,
        lease: UnlockLease,
        kind: ContentKind,
        identity: &[u8],
        payload: &[u8],
    ) -> Result<Vec<u8>> {
        gate.validate_lease(lease)?;
        if payload.len() > MAX_PAYLOAD_BYTES {
            return Err(SecurityError::InvalidConfig);
        }
        let aad = associated(&self.vault_id, kind.label(), identity)?;
        let nonce = random::<12>()?;
        let encrypted = cipher(&self.dek)?
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: payload,
                    aad: &aad,
                },
            )
            .map_err(|_| SecurityError::Crypto)?;
        gate.validate_lease(lease)?;
        let mut data = Vec::with_capacity(ENVELOPE_PREFIX.len() + nonce.len() + encrypted.len());
        data.extend_from_slice(ENVELOPE_PREFIX);
        data.extend_from_slice(&nonce);
        data.extend_from_slice(&encrypted);
        Ok(data)
    }

    pub fn unseal(
        &self,
        gate: &SecurityGate,
        lease: UnlockLease,
        kind: ContentKind,
        identity: &[u8],
        envelope: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>> {
        gate.validate_lease(lease)?;
        if envelope.len() < 5 + 12 + 16
            || envelope.len() > MAX_PAYLOAD_BYTES + 5 + 12 + 16
            || &envelope[..5] != ENVELOPE_PREFIX
        {
            return Err(SecurityError::Crypto);
        }
        let aad = associated(&self.vault_id, kind.label(), identity)?;
        let plain = cipher(&self.dek)?
            .decrypt(
                Nonce::from_slice(&envelope[5..17]),
                Payload {
                    msg: &envelope[17..],
                    aad: &aad,
                },
            )
            .map_err(|_| SecurityError::Crypto)?;
        gate.validate_lease(lease)?;
        Ok(Zeroizing::new(plain))
    }
}
impl VaultManifest {
    pub fn validate(&self) -> Result<[u8; 16]> {
        if self.format_version != FORMAT_VERSION {
            return Err(SecurityError::UnsupportedSchema);
        }
        let id = fixed::<16>(&self.vault_id_b64)?;
        fixed::<12>(&self.recovery_nonce_b64)?;
        let wrapped = STANDARD
            .decode(&self.recovery_wrapped_dek_b64)
            .map_err(|_| SecurityError::InvalidConfig)?;
        if wrapped.len() != 48 {
            return Err(SecurityError::InvalidConfig);
        }
        if let Some(dpapi) = &self.windows_dpapi_wrapped_dek_b64 {
            if dpapi.len() > 8192
                || STANDARD
                    .decode(dpapi)
                    .map_err(|_| SecurityError::InvalidConfig)?
                    .is_empty()
            {
                return Err(SecurityError::InvalidConfig);
            }
        }
        Ok(id)
    }
    /// Create only, not an update/migration. Existing directories are rejected.
    /// A crash can leave an incomplete empty vault directory, never overwrite legacy bytes.
    pub fn persist_new(&self, root: &Path) -> Result<()> {
        self.validate()?;
        guarded_path(root)?;
        let parent = root.parent().ok_or(SecurityError::UnsafePath)?;
        guarded_path(parent)?;
        let json = serde_json::to_vec(self).map_err(|_| SecurityError::InvalidConfig)?;
        if json.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(SecurityError::InvalidConfig);
        }
        fs::create_dir(root)?;
        let path = root.join("vault-v1.json");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        file.write_all(&json)?;
        file.sync_all()?;
        Ok(())
    }
    pub fn read(root: &Path) -> Result<Self> {
        guarded_path(root)?;
        let path = root.join("vault-v1.json");
        guarded_path(&path)?;
        let file = OpenOptions::new().read(true).open(&path)?;
        let mut json = Vec::new();
        file.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut json)?;
        if json.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(SecurityError::InvalidConfig);
        }
        let manifest: Self =
            serde_json::from_slice(&json).map_err(|_| SecurityError::InvalidConfig)?;
        manifest.validate()?;
        Ok(manifest)
    }
}
#[cfg(windows)]
mod dpapi {
    use super::{Result, SecurityError};
    use std::{ffi::c_void, ptr};
    #[repr(C)]
    struct Blob {
        len: u32,
        data: *mut u8,
    }
    #[link(name = "Crypt32")]
    extern "system" {
        fn CryptProtectData(
            input: *const Blob,
            description: *const u16,
            entropy: *const Blob,
            reserved: *const c_void,
            prompt: *const c_void,
            flags: u32,
            output: *mut Blob,
        ) -> i32;
        fn CryptUnprotectData(
            input: *const Blob,
            description: *mut *mut u16,
            entropy: *const Blob,
            reserved: *const c_void,
            prompt: *const c_void,
            flags: u32,
            output: *mut Blob,
        ) -> i32;
    }
    #[link(name = "Kernel32")]
    extern "system" {
        fn LocalFree(ptr: *mut c_void) -> *mut c_void;
    }
    fn process(input: &[u8], entropy: &[u8; 16], protect: bool) -> Result<Vec<u8>> {
        let src = Blob {
            len: input.len().try_into().map_err(|_| SecurityError::Crypto)?,
            data: input.as_ptr() as *mut u8,
        };
        let extra = Blob {
            len: 16,
            data: entropy.as_ptr() as *mut u8,
        };
        let mut out = Blob {
            len: 0,
            data: ptr::null_mut(),
        };
        // UI forbidden: background unlock must never cause a credential prompt.
        let ok = unsafe {
            if protect {
                CryptProtectData(
                    &src,
                    ptr::null(),
                    &extra,
                    ptr::null(),
                    ptr::null(),
                    1,
                    &mut out,
                )
            } else {
                CryptUnprotectData(
                    &src,
                    ptr::null_mut(),
                    &extra,
                    ptr::null(),
                    ptr::null(),
                    1,
                    &mut out,
                )
            }
        };
        if ok == 0 {
            return Err(SecurityError::Crypto);
        }
        if out.data.is_null() || out.len == 0 {
            return Err(SecurityError::Crypto);
        }
        let result = unsafe {
            let mut data = std::slice::from_raw_parts(out.data, out.len as usize).to_vec();
            ptr::write_bytes(out.data, 0, out.len as usize);
            LocalFree(out.data.cast());
            std::mem::take(&mut data)
        };
        Ok(result)
    }
    pub fn protect(key: &[u8; 32], entropy: &[u8; 16]) -> Result<Option<Vec<u8>>> {
        process(key, entropy, true).map(Some)
    }
    pub fn unprotect(blob: &[u8], entropy: &[u8; 16]) -> Result<Option<Vec<u8>>> {
        process(blob, entropy, false).map(Some)
    }
}
#[cfg(not(windows))]
mod dpapi {
    use super::Result;
    pub fn protect(_: &[u8; 32], _: &[u8; 16]) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }
    pub fn unprotect(_: &[u8], _: &[u8; 16]) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, SecurityGate, UnlockLease) {
        let dir = tempfile::tempdir().unwrap();
        let gate = SecurityGate::open(dir.path().join("verifier.json")).unwrap();
        gate.initialize(b"fixture-only-unlock-secret").unwrap();
        let lease = gate.require_unlocked().unwrap();
        (dir, gate, lease)
    }
    #[test]
    fn recovery_roundtrip_and_authentication_boundaries() {
        let (_, gate, lease) = fixture();
        let (vault, recovery, manifest) = CryptoVault::provision(&gate, lease).unwrap();
        let reopened =
            CryptoVault::restore_with_recovery(&gate, lease, &manifest, &recovery).unwrap();
        let bytes = vault
            .seal(
                &gate,
                lease,
                ContentKind::Archive,
                b"snapshot-v1",
                b"raw synthetic archive",
            )
            .unwrap();
        assert_eq!(
            &*reopened
                .unseal(&gate, lease, ContentKind::Archive, b"snapshot-v1", &bytes)
                .unwrap(),
            b"raw synthetic archive"
        );
        assert!(reopened
            .unseal(
                &gate,
                lease,
                ContentKind::DocumentObject,
                b"snapshot-v1",
                &bytes
            )
            .is_err());
        assert!(reopened
            .unseal(&gate, lease, ContentKind::Archive, b"wrong-id", &bytes)
            .is_err());
        let mut tampered = bytes.clone();
        *tampered.last_mut().unwrap() ^= 1;
        assert!(reopened
            .unseal(
                &gate,
                lease,
                ContentKind::Archive,
                b"snapshot-v1",
                &tampered
            )
            .is_err());
        let bad = RecoverySecret::from_exported_bytes([9u8; 32]);
        assert!(CryptoVault::restore_with_recovery(&gate, lease, &manifest, &bad).is_err());
        let other = CryptoVault::provision(&gate, lease).unwrap().0;
        assert!(other
            .unseal(&gate, lease, ContentKind::Archive, b"snapshot-v1", &bytes)
            .is_err());
    }
    #[test]
    fn lease_revocation_blocks_plaintext_and_new_ciphertext() {
        let (_, gate, lease) = fixture();
        let (vault, _, _) = CryptoVault::provision(&gate, lease).unwrap();
        let sealed = vault
            .seal(&gate, lease, ContentKind::Backup, b"b1", b"synthetic")
            .unwrap();
        gate.lock();
        assert!(matches!(
            vault.unseal(&gate, lease, ContentKind::Backup, b"b1", &sealed),
            Err(SecurityError::Locked)
        ));
        assert!(matches!(
            vault.seal(&gate, lease, ContentKind::Backup, b"b1", b"synthetic"),
            Err(SecurityError::Locked)
        ));
    }
    #[test]
    fn legacy_vault_paths_are_not_adopted_or_modified() {
        let (dir, gate, lease) = fixture();
        let (_, _, manifest) = CryptoVault::provision(&gate, lease).unwrap();
        let existing = dir.path().join("legacy");
        fs::create_dir(&existing).unwrap();
        fs::write(existing.join("archive.jsonl"), b"leave me alone").unwrap();
        assert!(manifest.persist_new(&existing).is_err());
        assert_eq!(
            fs::read(existing.join("archive.jsonl")).unwrap(),
            b"leave me alone"
        );
        let fresh = dir.path().join("new-vault");
        manifest.persist_new(&fresh).unwrap();
        assert!(manifest.persist_new(&fresh).is_err());
        assert_eq!(
            VaultManifest::read(&fresh).unwrap().vault_id_b64,
            manifest.vault_id_b64
        );
    }
    #[test]
    fn malformed_metadata_and_replay_are_rejected() {
        let (_, gate, lease) = fixture();
        let (vault, recovery, mut manifest) = CryptoVault::provision(&gate, lease).unwrap();
        manifest.format_version = 2;
        assert!(matches!(
            CryptoVault::restore_with_recovery(&gate, lease, &manifest, &recovery),
            Err(SecurityError::UnsupportedSchema)
        ));
        let sealed = vault
            .seal(
                &gate,
                lease,
                ContentKind::Backup,
                b"generation-1",
                b"backup",
            )
            .unwrap();
        assert!(vault
            .unseal(&gate, lease, ContentKind::Backup, b"generation-2", &sealed)
            .is_err());
        let mut malformed = sealed.clone();
        malformed[0] = 0;
        assert!(vault
            .unseal(
                &gate,
                lease,
                ContentKind::Backup,
                b"generation-1",
                &malformed
            )
            .is_err());
    }
    #[cfg(unix)]
    #[test]
    fn ancestor_symlink_is_rejected() {
        use std::os::unix::fs::symlink;
        let (dir, gate, lease) = fixture();
        let (_, _, manifest) = CryptoVault::provision(&gate, lease).unwrap();
        let actual = dir.path().join("actual");
        fs::create_dir(&actual).unwrap();
        let alias = dir.path().join("alias");
        symlink(&actual, &alias).unwrap();
        assert!(matches!(
            manifest.persist_new(&alias.join("new-vault")),
            Err(SecurityError::UnsafePath)
        ));
    }
    #[cfg(windows)]
    #[test]
    fn dpapi_user_context_roundtrip() {
        let (_, gate, lease) = fixture();
        let (vault, _, manifest) = CryptoVault::provision(&gate, lease).unwrap();
        assert!(manifest.windows_dpapi_wrapped_dek_b64.is_some());
        let unlocked = CryptoVault::restore_with_dpapi(&gate, lease, &manifest).unwrap();
        assert_eq!(unlocked.id(), vault.id());
    }
}

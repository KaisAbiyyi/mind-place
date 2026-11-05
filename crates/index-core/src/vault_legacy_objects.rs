//! Immutable encrypted mapping needed to recover original raw file identities
//! without retaining access to the plaintext legacy directory or a derived FTS.
use mind_place_security::{
    vault::{ContentKind, CryptoVault},
    vault_storage::ImmutableVaultStorage,
    Result, SecurityError, SecurityGate, UnlockLease,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, path::Path};
use zeroize::Zeroizing;

pub(crate) const RAW_MAP_ID: &[u8] = b"legacy-migration/raw-map/v1";

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawLegacyFile {
    pub relative: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawLegacyInventory {
    pub version: u32,
    pub files: Vec<RawLegacyFile>,
}

fn hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl RawLegacyFile {
    fn binding(&self) -> Result<(ContentKind, String)> {
        let parts: Vec<_> = self.relative.split('/').collect();
        if self.relative.len() > 32768
            || parts
                .iter()
                .any(|p| p.is_empty() || *p == "." || *p == ".." || p.contains('\0'))
            || !hex64(&self.sha256)
            || self.bytes > 64 * 1024 * 1024
        {
            return Err(SecurityError::Crypto);
        }
        match parts.as_slice() {
            ["archive-v1", _, ..] => Ok((
                ContentKind::Archive,
                format!("legacy-file/v1/{}", self.relative),
            )),
            ["document-objects", "v1", "sha256", prefix, hash]
                if hex64(hash) && *prefix == &hash[..2] && *hash == self.sha256 =>
            {
                Ok((ContentKind::DocumentObject, format!("document/{hash}")))
            }
            _ => Err(SecurityError::Crypto),
        }
    }
}

impl RawLegacyInventory {
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 || self.files.len() > 100_000 {
            return Err(SecurityError::Crypto);
        }
        let mut names = BTreeSet::new();
        let mut total = 0u64;
        for file in &self.files {
            file.binding()?;
            total = total.checked_add(file.bytes).ok_or(SecurityError::Crypto)?;
            if total > 512 * 1024 * 1024 * 1024 {
                return Err(SecurityError::Crypto);
            }
            if !names.insert(&file.relative) {
                return Err(SecurityError::Crypto);
            }
        }
        Ok(())
    }

    pub fn persist(
        &self,
        storage: &ImmutableVaultStorage<'_>,
        gate: &SecurityGate,
        lease: UnlockLease,
    ) -> Result<()> {
        self.validate()?;
        let bytes = Zeroizing::new(serde_json::to_vec(self).map_err(|_| SecurityError::Crypto)?);
        storage.write_immutable(gate, lease, ContentKind::Archive, RAW_MAP_ID, &bytes)?;
        Ok(())
    }

    pub fn read(
        root: &Path,
        vault: &CryptoVault,
        storage: &ImmutableVaultStorage<'_>,
        gate: &SecurityGate,
        lease: UnlockLease,
    ) -> Result<Option<Self>> {
        let name = vault.storage_name(gate, lease, ContentKind::Archive, RAW_MAP_ID)?;
        if !root
            .join("archive")
            .join(&name[..2])
            .join(name)
            .try_exists()?
        {
            return Ok(None);
        }
        let bytes = storage.read(gate, lease, ContentKind::Archive, RAW_MAP_ID)?;
        let inventory: Self = serde_json::from_slice(&bytes).map_err(|_| SecurityError::Crypto)?;
        inventory.validate()?;
        Ok(Some(inventory))
    }

    pub fn read_file(
        &self,
        relative: &str,
        storage: &ImmutableVaultStorage<'_>,
        gate: &SecurityGate,
        lease: UnlockLease,
    ) -> Result<Zeroizing<Vec<u8>>> {
        let entry = self
            .files
            .iter()
            .find(|file| file.relative == relative)
            .ok_or(SecurityError::Crypto)?;
        let (kind, identity) = entry.binding()?;
        let bytes = storage.read(gate, lease, kind, identity.as_bytes())?;
        if bytes.len() as u64 != entry.bytes
            || format!("{:x}", Sha256::digest(&bytes)) != entry.sha256
        {
            return Err(SecurityError::Crypto);
        }
        Ok(bytes)
    }

    pub fn verify_all(
        &self,
        storage: &ImmutableVaultStorage<'_>,
        gate: &SecurityGate,
        lease: UnlockLease,
    ) -> Result<()> {
        for file in &self.files {
            self.read_file(&file.relative, storage, gate, lease)?;
        }
        Ok(())
    }
}

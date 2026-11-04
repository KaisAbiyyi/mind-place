//! MP-9B: isolated, explicit-recovery-confirmed new SQLCipher state bootstrap.
//! Preview-only. No legacy adoption, no frontend/native-host activation here.
//! Production must encrypt *every* archive/index/object I/O path before rollout.
use crate::{Result, StateError, StateStore};
use mind_place_security::{
    vault::{CryptoVault, RecoverySecret, VaultManifest},
    vault_first_run::FirstRunJournal,
    SecurityGate, UnlockLease,
};
use std::{
    fs,
    path::{Path, PathBuf},
};
use zeroize::{Zeroize, Zeroizing};

/// Staged only in memory. Not persisted until the owner types the independent
/// recovery code back; an unlocked PIN alone never encrypts/decrypts the DEK.
pub struct PreparedFirstRunVault {
    target: PathBuf,
    vault: CryptoVault,
    recovery: RecoverySecret,
    manifest: VaultManifest,
}

impl PreparedFirstRunVault {
    pub fn prepare(root: &Path, gate: &SecurityGate, lease: UnlockLease) -> Result<Self> {
        gate.validate_lease(lease)
            .map_err(|_| StateError::Corrupt)?;
        if !root.is_absolute() || root.exists() {
            return Err(StateError::UnsafePath);
        }
        let parent = root.parent().ok_or(StateError::UnsafePath)?;
        if !parent.is_dir() {
            return Err(StateError::UnsafePath);
        }
        // The legacy plaintext runtime must be migrated with separate consent,
        // never silently treated as a first-run encrypted installation.
        for name in [
            "state.sqlite3",
            "index.sqlite3",
            "archive-v1",
            "document-objects",
        ] {
            if parent.join(name).exists() {
                return Err(StateError::Corrupt);
            }
        }
        let partial = root.with_extension("mp9b-init-partial");
        if partial.exists() {
            // Preserve interrupted staging for explicit owner disposition.
            return Err(StateError::Corrupt);
        }
        let (vault, recovery, manifest) =
            CryptoVault::provision(gate, lease).map_err(|_| StateError::Corrupt)?;
        Ok(Self {
            target: root.to_owned(),
            vault,
            recovery,
            manifest,
        })
    }

    /// Display only in first-run UI. Do not log, persist, cache or auto-copy.
    pub fn recovery_code(&self) -> Zeroizing<String> {
        self.recovery.export_code()
    }

    /// Requires evidence the owner preserved the independent code. A wrong
    /// code, stale lease, existing target, or legacy root cannot publish a vault.
    /// On a crash, encrypted-only partial staging remains for explicit recovery.
    pub fn confirm_and_provision(
        &self,
        entered_code: &str,
        gate: &SecurityGate,
        lease: UnlockLease,
    ) -> Result<StateStore> {
        gate.validate_lease(lease)
            .map_err(|_| StateError::Corrupt)?;
        let recovered =
            RecoverySecret::from_export_code(entered_code).map_err(|_| StateError::Corrupt)?;
        let confirmed = CryptoVault::restore_with_recovery(gate, lease, &self.manifest, &recovered)
            .map_err(|_| StateError::Corrupt)?;
        if confirmed.id() != self.vault.id() {
            return Err(StateError::Corrupt);
        }
        if self.target.exists() {
            return Err(StateError::Corrupt);
        }
        let partial = self.target.with_extension("mp9b-init-partial");
        if partial.exists() {
            return Err(StateError::Corrupt);
        }
        let parent = self.target.parent().ok_or(StateError::UnsafePath)?;
        for name in [
            "state.sqlite3",
            "index.sqlite3",
            "archive-v1",
            "document-objects",
        ] {
            if parent.join(name).exists() {
                return Err(StateError::Corrupt);
            }
        }
        FirstRunJournal::mark_committing(&self.target).map_err(|_| StateError::Corrupt)?;
        self.manifest
            .persist_new(&partial)
            .map_err(|_| StateError::Corrupt)?;
        let derived = self
            .vault
            .derived_key(gate, lease, b"state/sqlcipher/v1")
            .map_err(|_| StateError::Corrupt)?;
        let mut raw = [0u8; 32];
        raw.copy_from_slice(derived.as_ref());
        let staged = StateStore::open_encrypted(partial.join("state.sqlite3"), raw, gate, lease);
        raw.zeroize();
        staged?;
        gate.validate_lease(lease)
            .map_err(|_| StateError::Corrupt)?;
        fs::rename(&partial, &self.target)?;
        // Reopen from the published destination and verify correct keyed state.
        let mut raw = [0u8; 32];
        raw.copy_from_slice(derived.as_ref());
        let final_store =
            StateStore::open_encrypted(self.target.join("state.sqlite3"), raw, gate, lease);
        raw.zeroize();
        let final_store = final_store?;
        FirstRunJournal::mark_complete(&self.target).map_err(|_| StateError::Corrupt)?;
        Ok(final_store)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, SecurityGate, UnlockLease) {
        let home = tempfile::tempdir().unwrap();
        let gate = SecurityGate::open(home.path().join("security.json")).unwrap();
        gate.initialize(b"synthetic-first-run-password").unwrap();
        let lease = gate.require_unlocked().unwrap();
        (home, gate, lease)
    }

    #[test]
    fn confirmation_required_before_any_ciphertext_published() {
        let (home, gate, lease) = fixture();
        let vault_root = home.path().join("new-vault");
        FirstRunJournal::ensure_start(&vault_root, b"synthetic-first-run-password", false).unwrap();
        let pending = PreparedFirstRunVault::prepare(&vault_root, &gate, lease).unwrap();
        let code = pending.recovery_code();
        assert!(!vault_root.exists());
        assert!(pending
            .confirm_and_provision("incorrect", &gate, lease)
            .is_err());
        assert!(!vault_root.exists());

        let pending = PreparedFirstRunVault::prepare(&vault_root, &gate, lease).unwrap();
        let right_code = pending.recovery_code();
        let state = pending
            .confirm_and_provision(&right_code, &gate, lease)
            .unwrap();
        assert!(vault_root.join("vault-v1.json").exists());
        let bytes = fs::read(state.path()).unwrap();
        assert!(!bytes.starts_with(b"SQLite format 3\0"));
        assert!(StateStore::open(state.path()).is_err());
        assert!(crate::DocumentObjectStore::open(state.clone()).is_err());
        assert!(!vault_root.join("document-objects").exists());
        // A live state transaction in this vault participates in the shared
        // writer fence, so a portable backup cannot run concurrently.
        let connection = state.connection().unwrap();
        let competing = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(vault_root.join(".vault-writer.lock"))
            .unwrap();
        assert!(fs2::FileExt::try_lock_exclusive(&competing).is_err());
        drop(connection);
        fs2::FileExt::try_lock_exclusive(&competing).unwrap();
        fs2::FileExt::unlock(&competing).unwrap();
        assert!(!code.is_empty());
    }

    #[test]
    fn interrupted_first_run_requires_original_secret_and_new_confirmation() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("encrypted-vault-v1");
        let secret = b"synthetic-first-run-password";
        FirstRunJournal::ensure_start(&root, secret, false).unwrap();
        let original = SecurityGate::open(home.path().join("security.json")).unwrap();
        original.initialize(secret).unwrap();
        let lease = original.require_unlocked().unwrap();
        let abandoned = PreparedFirstRunVault::prepare(&root, &original, lease).unwrap();
        let original_code = abandoned.recovery_code();
        assert!(!root.exists());
        drop(abandoned);
        drop(original);
        let restarted = SecurityGate::open(home.path().join("security.json")).unwrap();
        assert!(restarted.unlock(b"not the original password").is_err());
        assert!(restarted.require_unlocked().is_err());
        restarted.unlock(secret).unwrap();
        let lease = restarted.require_unlocked().unwrap();
        assert!(FirstRunJournal::can_resume(&root, secret).unwrap());
        FirstRunJournal::ensure_start(&root, secret, true).unwrap();
        let resumed = PreparedFirstRunVault::prepare(&root, &restarted, lease).unwrap();
        let new_code = resumed.recovery_code();
        assert_ne!(original_code.as_str(), new_code.as_str());
        assert!(resumed
            .confirm_and_provision(original_code.as_str(), &restarted, lease)
            .is_err());
        assert!(!root.exists());
        let encrypted = resumed
            .confirm_and_provision(new_code.as_str(), &restarted, lease)
            .unwrap();
        assert!(!fs::read(encrypted.path())
            .unwrap()
            .starts_with(b"SQLite format 3\0"));
        restarted.lock();
        assert!(FirstRunJournal::can_resume(&root, secret).is_err());
        fs::remove_file(root.join("vault-v1.json")).unwrap();
        assert!(FirstRunJournal::can_resume(&root, secret).is_err());
        assert!(FirstRunJournal::ensure_start(&root, secret, true).is_err());
    }

    #[test]
    fn repeated_abort_and_committing_boundary_never_replaces_keys() {
        let (home, gate, lease) = fixture();
        let root = home.path().join("encrypted-vault-v1");
        let secret = b"synthetic-first-run-password";
        FirstRunJournal::ensure_start(&root, secret, false).unwrap();
        for _ in 0..3 {
            let pending = PreparedFirstRunVault::prepare(&root, &gate, lease).unwrap();
            drop(pending);
            assert!(FirstRunJournal::can_resume(&root, secret).unwrap());
            FirstRunJournal::ensure_start(&root, secret, true).unwrap();
        }
        FirstRunJournal::mark_committing(&root).unwrap();
        assert!(FirstRunJournal::can_resume(&root, secret).is_err());
        assert!(PreparedFirstRunVault::prepare(&root, &gate, lease).is_ok());
        let prepared = PreparedFirstRunVault::prepare(&root, &gate, lease).unwrap();
        let code = prepared.recovery_code();
        assert!(prepared
            .confirm_and_provision(code.as_str(), &gate, lease)
            .is_err());
        assert!(!root.exists());
        fs::create_dir(root.with_extension("mp9b-init-partial")).unwrap();
        assert!(FirstRunJournal::can_resume(&root, secret).is_err());
    }

    #[test]
    fn legacy_plaintext_or_revoked_epoch_cannot_be_adopted() {
        let (home, gate, lease) = fixture();
        let state = home.path().join("state.sqlite3");
        StateStore::open(&state).unwrap();
        assert!(PreparedFirstRunVault::prepare(&home.path().join("vault"), &gate, lease).is_err());
        fs::remove_file(state).unwrap();
        let pending =
            PreparedFirstRunVault::prepare(&home.path().join("vault"), &gate, lease).unwrap();
        let code = pending.recovery_code();
        gate.lock();
        assert!(pending.confirm_and_provision(&code, &gate, lease).is_err());
        assert!(!home.path().join("vault").exists());
    }
}

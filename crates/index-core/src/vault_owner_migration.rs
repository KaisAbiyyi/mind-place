//! Explicit Windows owner maintenance runtime. Never called by app startup,
//! MCP, capture or a folder association. All desktop/daemon instances must exit.
use crate::vault_legacy_migration::LegacyEncryptedConversion;
use mind_place_security::{
    vault::{CryptoVault, RecoverySecret, VaultManifest},
    vault_migration::{ExplicitMigrationConsent, LegacyMigrationPreflight},
    Result, SecurityError, SecurityGate, UnlockLease,
};
use std::{
    fs::File,
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;

/// Keeps Windows sharing barriers and the legacy capture lock alive throughout
/// owner confirmation, conversion, verification and publication. No source writes.
pub struct OfflineLegacyRoot {
    root: PathBuf,
    _handles: Vec<File>,
}

impl OfflineLegacyRoot {
    #[cfg(windows)]
    pub fn acquire(root: &Path, gate: &SecurityGate, lease: UnlockLease) -> Result<Self> {
        use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};
        gate.validate_lease(lease)?;
        if !root.is_absolute() {
            return Err(SecurityError::UnsafePath);
        }
        mind_place_security::vault_migration::validate_owner_migration_path(root)?;
        let root = root.canonicalize()?;
        let mut handles = Vec::new();
        // FILE_SHARE_READ excludes every existing/future writer and replacement
        // handle. Converter readers use SQLite immutable read-only connections.
        for name in ["state.sqlite3", "index.sqlite3", "document-index.sqlite3"] {
            let path = root.join(name);
            mind_place_security::vault_migration::validate_owner_migration_path(&path)?;
            if path.exists() {
                let file = OpenOptions::new().read(true).share_mode(1).open(path)?;
                if !file.metadata()?.is_file() {
                    return Err(SecurityError::UnsafePath);
                }
                handles.push(file);
            } else if name == "state.sqlite3" {
                return Err(SecurityError::InvalidConfig);
            }
        }
        let archive = root.join("archive-v1");
        mind_place_security::vault_migration::validate_owner_migration_path(&archive)?;
        if archive.exists() {
            let lock = archive.join(".archive.lock");
            mind_place_security::vault_migration::validate_owner_migration_path(&lock)?;
            // Never create/repair legacy data as part of an offline preflight.
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .share_mode(3)
                .open(lock)?;
            if !file.metadata()?.is_file() || file.metadata()?.len() != 0 {
                return Err(SecurityError::InvalidConfig);
            }
            mind_place_security::vault_migration::lock_legacy_archive(&file)?;
            handles.push(file);
        }
        Ok(Self {
            root,
            _handles: handles,
        })
    }

    #[cfg(not(windows))]
    pub fn acquire(_root: &Path, gate: &SecurityGate, lease: UnlockLease) -> Result<Self> {
        gate.validate_lease(lease)?;
        Err(SecurityError::InvalidConfig)
    }

    fn target(&self) -> PathBuf {
        self.root.join("encrypted-vault-v1")
    }

    pub fn prepare(
        &self,
        gate: &SecurityGate,
        lease: UnlockLease,
    ) -> Result<PreparedOwnerMigration> {
        gate.validate_lease(lease)?;
        let plan = LegacyMigrationPreflight::inspect(&self.root, &self.target())?;
        if plan.authorization_root().exists() {
            return Err(SecurityError::AlreadyConfigured);
        }
        let (_, recovery, manifest) = CryptoVault::provision(gate, lease)?;
        Ok(PreparedOwnerMigration {
            recovery,
            manifest,
            plan,
        })
    }

    /// Recovery code and explicit consent are required again after any restart.
    pub fn resume(
        &self,
        gate: &SecurityGate,
        lease: UnlockLease,
        code: &str,
        consent: &ExplicitMigrationConsent,
        rollback: bool,
    ) -> Result<()> {
        gate.validate_lease(lease)?;
        let auth =
            LegacyMigrationPreflight::recovery_authorization_root(&self.root, &self.target())?;
        let manifest = VaultManifest::read(&auth)?;
        let recovery = RecoverySecret::from_export_code(code)?;
        let crypto = CryptoVault::restore_with_recovery(gate, lease, &manifest, &recovery)?;
        let conversion = LegacyEncryptedConversion {
            gate,
            lease,
            consent,
            manifest: &manifest,
            recovery: &recovery,
        };
        let plan = if self.target().exists() {
            conversion.resume_owner_consented(&self.root, &self.target())?
        } else {
            let plan = LegacyMigrationPreflight::inspect(&self.root, &self.target())?;
            // Crash after wrapped authorization but before journal: safe to begin
            // only when no prior working artifacts exist. begin checks both.
            match conversion.resume_owner_consented(&self.root, &self.target()) {
                Ok(existing) => existing,
                Err(SecurityError::Io(ref error))
                    if error.kind() == std::io::ErrorKind::NotFound =>
                {
                    conversion.begin_owner_consented(&plan)?;
                    plan
                }
                Err(error) => return Err(error),
            }
        };
        drop(crypto);
        if rollback {
            conversion.rollback_to_legacy(&plan)
        } else {
            conversion.execute_or_resume(&plan)
        }
    }
}

pub struct PreparedOwnerMigration {
    recovery: RecoverySecret,
    manifest: VaultManifest,
    plan: LegacyMigrationPreflight,
}
impl PreparedOwnerMigration {
    /// Owner display only. Never send to diagnostics, clipboard or command args.
    pub fn recovery_code(&self) -> Zeroizing<String> {
        self.recovery.export_code()
    }

    pub fn confirm(
        &self,
        offline: &OfflineLegacyRoot,
        gate: &SecurityGate,
        lease: UnlockLease,
        code: &str,
        consent: &ExplicitMigrationConsent,
    ) -> Result<()> {
        let recovered = RecoverySecret::from_export_code(code)?;
        CryptoVault::restore_with_recovery(gate, lease, &self.manifest, &recovered)?;
        let plan = &self.plan;
        if plan.source_root() != offline.root {
            return Err(SecurityError::UnsafePath);
        }
        plan.verify_unchanged()?;
        // Only the encrypted/wrapped manifest is made durable, after confirmation.
        self.manifest.persist_new(&plan.authorization_root())?;
        let conversion = LegacyEncryptedConversion {
            gate,
            lease,
            consent,
            manifest: &self.manifest,
            recovery: &recovered,
        };
        conversion.begin_owner_consented(plan)?;
        conversion.execute_or_resume(plan)
    }
}

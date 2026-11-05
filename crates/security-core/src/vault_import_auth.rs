use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

use crate::{
    read_config, validate_config, validate_path, validate_secret, verify_secret, Result,
    SecurityError, SecurityGate, UnlockLease,
};

#[derive(Clone, Debug)]
pub struct ImportAuthorizationCapability {
    token: Zeroizing<[u8; 32]>,
    job_id: String,
    provider: String,
    source_root: PathBuf,
    source_manifest_hash: [u8; 32],
    lease_epoch: u64,
    created_at: Instant,
    ttl: Duration,
}

impl ImportAuthorizationCapability {
    pub fn token(&self) -> &[u8; 32] {
        &self.token
    }

    pub fn job_id(&self) -> &str {
        &self.job_id
    }

    pub fn provider(&self) -> &str {
        &self.provider
    }

    pub fn source_root(&self) -> &Path {
        &self.source_root
    }

    pub fn manifest_hash(&self) -> &[u8; 32] {
        &self.source_manifest_hash
    }

    pub fn lease_epoch(&self) -> u64 {
        self.lease_epoch
    }
}

pub fn validate_source_directory(source_root: &Path) -> Result<()> {
    if !source_root.is_absolute() {
        return Err(SecurityError::UnsafePath);
    }

    // Verify all existing ancestors to prevent symlink or reparse-point substitution
    for ancestor in source_root.ancestors() {
        if ancestor.exists() {
            validate_path(ancestor)?;
        }
    }

    let meta = fs::symlink_metadata(source_root)?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
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

impl SecurityGate {
    pub fn verify_owner_credential(&self, credential: &[u8]) -> Result<()> {
        validate_secret(credential)?;
        let config = read_config(&self.inner.config_path)?;
        validate_config(&config)?;
        verify_secret(credential, &config.password_hash)
    }

    pub fn authorize_import_session(
        &self,
        lease: UnlockLease,
        owner_auth_credential: &[u8],
        provider: &str,
        source_root: &Path,
        current_source_manifest_hash: [u8; 32],
        ttl: Duration,
    ) -> Result<ImportAuthorizationCapability> {
        self.validate_lease(lease)?;
        self.verify_owner_credential(owner_auth_credential)?;
        validate_source_directory(source_root)?;

        let mut token = [0u8; 32];
        getrandom::fill(&mut token).map_err(|_| SecurityError::Crypto)?;
        let suffix: String = token[..6].iter().map(|b| format!("{b:02x}")).collect();
        let job_id = format!("import-{provider}-{suffix}");

        Ok(ImportAuthorizationCapability {
            token: Zeroizing::new(token),
            job_id,
            provider: provider.to_owned(),
            source_root: source_root.to_owned(),
            source_manifest_hash: current_source_manifest_hash,
            lease_epoch: lease.epoch(),
            created_at: Instant::now(),
            ttl: std::cmp::min(ttl, Duration::from_secs(3600)),
        })
    }

    pub fn verify_import_capability(
        &self,
        lease: UnlockLease,
        cap: &ImportAuthorizationCapability,
        independently_computed_manifest_hash: &[u8; 32],
    ) -> Result<()> {
        self.validate_lease(lease)?;
        if cap.lease_epoch != lease.epoch() {
            return Err(SecurityError::Locked);
        }
        if cap.created_at.elapsed() > cap.ttl {
            return Err(SecurityError::Locked);
        }
        if &cap.source_manifest_hash != independently_computed_manifest_hash {
            return Err(SecurityError::InvalidConfig);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_import_session_authorization_and_verification() {
        let fixture = tempdir().unwrap();
        let config_path = fixture.path().join("security-config.json");
        let gate = SecurityGate::open(&config_path).unwrap();
        let secret = b"correct-horse-battery-staple-99";
        gate.initialize(secret).unwrap();

        let lease = gate.require_unlocked().unwrap();
        let source_dir = fixture.path().join("test_import_source");
        fs::create_dir(&source_dir).unwrap();

        let manifest_hash = [42u8; 32];

        // 1. Wrong owner credential is rejected
        let bad_auth = gate.authorize_import_session(
            lease,
            b"wrong-password-here",
            "gemini",
            &source_dir,
            manifest_hash,
            Duration::from_secs(1800),
        );
        assert!(matches!(bad_auth, Err(SecurityError::InvalidCredentials)));

        // 2. Correct owner credential issues capability
        let cap = gate
            .authorize_import_session(
                lease,
                secret,
                "gemini",
                &source_dir,
                manifest_hash,
                Duration::from_secs(1800),
            )
            .unwrap();
        assert_eq!(cap.provider(), "gemini");
        assert_eq!(cap.source_root(), source_dir.as_path());
        assert_eq!(cap.manifest_hash(), &manifest_hash);

        // 3. Verification succeeds with identical independently computed hash
        assert!(gate
            .verify_import_capability(lease, &cap, &manifest_hash)
            .is_ok());

        // 4. Verification fails if manifest hash differs (tamper detection)
        let tampered_hash = [99u8; 32];
        assert!(matches!(
            gate.verify_import_capability(lease, &cap, &tampered_hash),
            Err(SecurityError::InvalidConfig)
        ));

        // 5. Verification fails when vault locks
        gate.lock();
        assert!(matches!(
            gate.verify_import_capability(lease, &cap, &manifest_hash),
            Err(SecurityError::Locked)
        ));
    }

    #[test]
    fn test_source_directory_validation_rejects_unsafe_paths() {
        let fixture = tempdir().unwrap();

        // Non-existent directory
        let missing = fixture.path().join("does_not_exist");
        assert!(validate_source_directory(&missing).is_err());

        // Relative path
        let relative = PathBuf::from("relative/path");
        assert!(validate_source_directory(&relative).is_err());

        // Valid directory
        let valid = fixture.path().join("valid_source");
        fs::create_dir(&valid).unwrap();
        assert!(validate_source_directory(&valid).is_ok());
    }
}

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

use crate::{
    duration_ms_ceil, failure_delay, read_config, remaining_block, validate_config, validate_path,
    validate_secret, verify_secret, ActiveImportRecord, Result, SecurityError, SecurityGate,
    UnlockLease,
};

#[derive(Clone, Debug)]
pub struct ImportAuthorizationCapability {
    issuing_gate_instance_id: [u8; 16],
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
    pub fn issuing_gate_instance_id(&self) -> [u8; 16] {
        self.issuing_gate_instance_id
    }

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

    pub fn created_at(&self) -> Instant {
        self.created_at
    }

    pub fn ttl(&self) -> Duration {
        self.ttl
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
        let mut state = self.lock_state();
        if !state.configured {
            return Err(SecurityError::NotConfigured);
        }
        if let Some(remaining) = remaining_block(&state) {
            return Err(SecurityError::RateLimited {
                retry_after_ms: duration_ms_ceil(remaining),
            });
        }

        let config = read_config(&self.inner.config_path)?;
        validate_config(&config)?;
        if verify_secret(credential, &config.password_hash).is_err() {
            // A failed import reauthentication cannot revoke an authorized owner session,
            // but it increments failed attempts and enforces backoff across subsequent attempts.
            state.failed_attempts = state.failed_attempts.saturating_add(1);
            let delay = failure_delay(state.failed_attempts);
            state.blocked_until = (!delay.is_zero()).then(|| Instant::now() + delay);
            return Err(SecurityError::InvalidCredentials);
        }

        // On correct verification: reset failure backoff without modifying active lease or epoch
        state.failed_attempts = 0;
        state.blocked_until = None;
        Ok(())
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

        let canonical_root =
            fs::canonicalize(source_root).unwrap_or_else(|_| source_root.to_path_buf());

        let mut token = [0u8; 32];
        getrandom::fill(&mut token).map_err(|_| SecurityError::Crypto)?;
        let suffix: String = token[..6].iter().map(|b| format!("{b:02x}")).collect();
        let job_id = format!("import-{provider}-{suffix}");
        let cap_ttl = std::cmp::min(ttl, Duration::from_secs(3600));
        let now = Instant::now();

        let record = ActiveImportRecord {
            token,
            job_id: job_id.clone(),
            provider: provider.to_owned(),
            canonical_source_root: canonical_root.clone(),
            manifest_hash: current_source_manifest_hash,
            lease_epoch: lease.epoch(),
            created_at: now,
            ttl: cap_ttl,
            revoked: false,
            usage_count: 0,
            last_verified_at: None,
        };

        {
            let mut state = self.lock_state();
            if state.locked || state.epoch != lease.epoch() {
                return Err(SecurityError::Locked);
            }
            state.active_imports.insert(token, record);
        }

        Ok(ImportAuthorizationCapability {
            issuing_gate_instance_id: self.inner.instance_id,
            token: Zeroizing::new(token),
            job_id,
            provider: provider.to_owned(),
            source_root: canonical_root,
            source_manifest_hash: current_source_manifest_hash,
            lease_epoch: lease.epoch(),
            created_at: now,
            ttl: cap_ttl,
        })
    }

    pub fn verify_import_capability(
        &self,
        lease: UnlockLease,
        cap: &ImportAuthorizationCapability,
        independently_computed_manifest_hash: &[u8; 32],
    ) -> Result<()> {
        self.verify_import_capability_bound(
            lease,
            cap,
            cap.provider(),
            cap.job_id(),
            cap.source_root(),
            independently_computed_manifest_hash,
        )
    }

    pub fn verify_import_capability_bound(
        &self,
        lease: UnlockLease,
        cap: &ImportAuthorizationCapability,
        expected_provider: &str,
        expected_job_id: &str,
        expected_source_root: &Path,
        independently_computed_manifest_hash: &[u8; 32],
    ) -> Result<()> {
        self.with_verified_import_capability_bound(
            lease,
            cap,
            expected_provider,
            expected_job_id,
            expected_source_root,
            independently_computed_manifest_hash,
            || (),
        )
    }

    /// Run a bounded import write while its capability is valid. The action must
    /// not call back into this gate. Lock and revocation serialize with the action.
    #[allow(clippy::too_many_arguments)]
    pub fn with_verified_import_capability_bound<T>(
        &self,
        lease: UnlockLease,
        cap: &ImportAuthorizationCapability,
        expected_provider: &str,
        expected_job_id: &str,
        expected_source_root: &Path,
        independently_computed_manifest_hash: &[u8; 32],
        action: impl FnOnce() -> T,
    ) -> Result<T> {
        let mut state = self.lock_state();
        if !state.configured {
            return Err(SecurityError::Locked);
        }

        // 1. Issuing gate instance binding: reject foreign gate capability
        if cap.issuing_gate_instance_id != self.inner.instance_id {
            return Err(SecurityError::InvalidCredentials);
        }

        // 2. Epoch and unlock state check
        let current_epoch = state.epoch;
        if state.locked || current_epoch != lease.epoch() {
            return Err(SecurityError::Locked);
        }
        if cap.lease_epoch != current_epoch {
            return Err(SecurityError::Locked);
        }

        // 3. TTL check on capability
        if cap.created_at.elapsed() > cap.ttl {
            return Err(SecurityError::Locked);
        }

        // 4. Look up tracked active import in gate registry
        let record = state
            .active_imports
            .get_mut(cap.token.as_ref())
            .ok_or(SecurityError::InvalidCredentials)?;

        // 5. Revocation check
        if record.revoked {
            return Err(SecurityError::Locked);
        }

        // 6. Epoch check on record
        if record.lease_epoch != current_epoch {
            return Err(SecurityError::Locked);
        }

        // 7. TTL check on record
        if record.created_at.elapsed() > record.ttl {
            return Err(SecurityError::Locked);
        }

        // 8. Expected provider binding
        if record.provider != expected_provider || cap.provider != expected_provider {
            return Err(SecurityError::InvalidConfig);
        }

        // 9. Expected job ID binding
        if record.job_id != expected_job_id || cap.job_id != expected_job_id {
            return Err(SecurityError::InvalidConfig);
        }

        // 10. Canonical source root binding
        let canonical_expected_root = fs::canonicalize(expected_source_root)
            .unwrap_or_else(|_| expected_source_root.to_path_buf());
        if record.canonical_source_root != canonical_expected_root
            || cap.source_root != canonical_expected_root
        {
            return Err(SecurityError::InvalidConfig);
        }

        // 11. Source manifest hash binding (must match independently computed hash)
        if &record.manifest_hash != independently_computed_manifest_hash
            || &cap.source_manifest_hash != independently_computed_manifest_hash
        {
            return Err(SecurityError::InvalidConfig);
        }

        // 12. Replay & usage tracking: record verification step
        record.usage_count = record.usage_count.saturating_add(1);
        record.last_verified_at = Some(Instant::now());

        // Keep the gate locked through the write: lock/revoke cannot race publication.
        Ok(action())
    }

    pub fn revoke_import_capability(&self, token: &[u8; 32]) -> Result<()> {
        let mut state = self.lock_state();
        if let Some(record) = state.active_imports.get_mut(token) {
            record.revoked = true;
            Ok(())
        } else {
            Err(SecurityError::InvalidCredentials)
        }
    }

    pub fn revoke_import_job(&self, job_id: &str) -> Result<()> {
        let mut state = self.lock_state();
        let mut found = false;
        for record in state.active_imports.values_mut() {
            if record.job_id == job_id {
                record.revoked = true;
                found = true;
            }
        }
        if found {
            Ok(())
        } else {
            Err(SecurityError::InvalidCredentials)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn import_action_is_fenced_and_revoked_actions_never_run() {
        let home = tempdir().unwrap();
        let gate = SecurityGate::open(home.path().join("security.json")).unwrap();
        gate.initialize(b"synthetic-owner-credential").unwrap();
        let lease = gate.require_unlocked().unwrap();
        let cap = gate
            .authorize_import_session(
                lease,
                b"synthetic-owner-credential",
                "chatgpt",
                home.path(),
                [5; 32],
                Duration::from_secs(60),
            )
            .unwrap();
        let mut calls = 0;
        gate.with_verified_import_capability_bound(
            lease,
            &cap,
            "chatgpt",
            cap.job_id(),
            home.path(),
            &[5; 32],
            || {
                assert!(matches!(
                    gate.inner.state.try_lock(),
                    Err(std::sync::TryLockError::WouldBlock)
                ));
                calls += 1;
            },
        )
        .unwrap();
        gate.revoke_import_capability(cap.token()).unwrap();
        assert!(gate
            .with_verified_import_capability_bound(
                lease,
                &cap,
                "chatgpt",
                cap.job_id(),
                home.path(),
                &[5; 32],
                || calls += 1,
            )
            .is_err());
        assert_eq!(calls, 1);
    }

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
        let canonical_expected = fs::canonicalize(&source_dir).unwrap();
        assert_eq!(cap.source_root(), canonical_expected.as_path());
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
    fn test_cross_gate_capability_rejected() {
        let fixture = tempdir().unwrap();
        let gate_a = SecurityGate::open(fixture.path().join("gate-a.json")).unwrap();
        let gate_b = SecurityGate::open(fixture.path().join("gate-b.json")).unwrap();
        let secret = b"correct-horse-battery-staple-99";
        gate_a.initialize(secret).unwrap();
        gate_b.initialize(secret).unwrap();

        let lease_a = gate_a.require_unlocked().unwrap();
        let lease_b = gate_b.require_unlocked().unwrap();
        let source_dir = fixture.path().join("test_import_source");
        fs::create_dir(&source_dir).unwrap();
        let manifest_hash = [7u8; 32];

        let cap_a = gate_a
            .authorize_import_session(
                lease_a,
                secret,
                "gemini",
                &source_dir,
                manifest_hash,
                Duration::from_secs(1800),
            )
            .unwrap();

        // Gate B must reject capability issued by Gate A
        let err = gate_b
            .verify_import_capability(lease_b, &cap_a, &manifest_hash)
            .unwrap_err();
        assert!(matches!(err, SecurityError::InvalidCredentials));
    }

    #[test]
    fn test_import_auth_enforces_rate_limit_and_preserves_valid_lease() {
        let fixture = tempdir().unwrap();
        let gate = SecurityGate::open(fixture.path().join("gate.json")).unwrap();
        let secret = b"correct-horse-battery-staple-99";
        gate.initialize(secret).unwrap();
        let lease = gate.require_unlocked().unwrap();

        let source_dir = fixture.path().join("test_import_source");
        fs::create_dir(&source_dir).unwrap();
        let manifest_hash = [7u8; 32];

        // 3 failed unlock attempts trigger rate limiting
        for _ in 0..3 {
            assert!(gate.unlock(b"wrong-password-value-2026").is_err());
        }
        assert!(gate.status().retry_after_ms > 0);

        // Import reauth must enforce active rate limit
        let res = gate.authorize_import_session(
            lease,
            secret,
            "gemini",
            &source_dir,
            manifest_hash,
            Duration::from_secs(300),
        );
        assert!(matches!(res, Err(SecurityError::RateLimited { .. })));

        // Existing owner lease remains valid and gate remains unlocked
        assert!(gate.validate_lease(lease).is_ok());
    }

    #[test]
    fn test_context_mismatch_and_revocation() {
        let fixture = tempdir().unwrap();
        let gate = SecurityGate::open(fixture.path().join("gate.json")).unwrap();
        let secret = b"correct-horse-battery-staple-99";
        gate.initialize(secret).unwrap();
        let lease = gate.require_unlocked().unwrap();

        let source_dir = fixture.path().join("test_import_source");
        fs::create_dir(&source_dir).unwrap();
        let other_dir = fixture.path().join("other_source");
        fs::create_dir(&other_dir).unwrap();
        let manifest_hash = [7u8; 32];

        let cap = gate
            .authorize_import_session(
                lease,
                secret,
                "gemini",
                &source_dir,
                manifest_hash,
                Duration::from_secs(300),
            )
            .unwrap();

        // 1. Provider mismatch rejected
        assert!(matches!(
            gate.verify_import_capability_bound(
                lease,
                &cap,
                "chatgpt",
                cap.job_id(),
                &source_dir,
                &manifest_hash
            ),
            Err(SecurityError::InvalidConfig)
        ));

        // 2. Job ID mismatch rejected
        assert!(matches!(
            gate.verify_import_capability_bound(
                lease,
                &cap,
                "gemini",
                "foreign-job-id",
                &source_dir,
                &manifest_hash
            ),
            Err(SecurityError::InvalidConfig)
        ));

        // 3. Source root mismatch rejected
        assert!(matches!(
            gate.verify_import_capability_bound(
                lease,
                &cap,
                "gemini",
                cap.job_id(),
                &other_dir,
                &manifest_hash
            ),
            Err(SecurityError::InvalidConfig)
        ));

        // 4. Repeated check for same multi-step job succeeds (replay tracking)
        assert!(gate
            .verify_import_capability(lease, &cap, &manifest_hash)
            .is_ok());
        assert!(gate
            .verify_import_capability(lease, &cap, &manifest_hash)
            .is_ok());

        // 5. Revocation immediately blocks further use
        gate.revoke_import_capability(cap.token()).unwrap();
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

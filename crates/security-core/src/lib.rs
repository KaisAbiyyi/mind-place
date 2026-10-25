pub mod vault;
pub mod vault_backup;
pub mod vault_storage;

use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Algorithm, Argon2, Params, Version,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

const SCHEMA_VERSION: u32 = 1;
const MAX_CONFIG_BYTES: usize = 16 * 1024;
const MIN_SECRET_BYTES: usize = 8;
const MAX_SECRET_BYTES: usize = 256;
const ARGON2_MEMORY_KIB: u32 = 65_536;
const ARGON2_ITERATIONS: u32 = 3;
const ARGON2_PARALLELISM: u32 = 1;
const FAILURE_DELAY_START: u32 = 3;
const MAX_FAILURE_DELAY_SECS: u64 = 60;

pub type Result<T> = std::result::Result<T, SecurityError>;

#[derive(Debug, thiserror::Error)]
pub enum SecurityError {
    #[error("Mind Place security state is unavailable")]
    Io(#[from] std::io::Error),
    #[error("Mind Place security state is invalid")]
    InvalidConfig,
    #[error("Mind Place security state uses an unsupported schema")]
    UnsupportedSchema,
    #[error("Mind Place security state is already configured")]
    AlreadyConfigured,
    #[error("Mind Place security state is not configured")]
    NotConfigured,
    #[error("Unlock secret must contain 8-256 UTF-8 bytes and cannot be whitespace-only")]
    WeakSecret,
    #[error("Unlock secret is incorrect")]
    InvalidCredentials,
    #[error("Unlock is temporarily rate limited")]
    RateLimited { retry_after_ms: u64 },
    #[error("Mind Place is locked")]
    Locked,
    #[error("Security path cannot be a symbolic link or reparse point")]
    UnsafePath,
    #[error("Cryptographic operation failed")]
    Crypto,
}

impl SecurityError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io(_) => "MP_SECURITY_IO",
            Self::InvalidConfig => "MP_SECURITY_INVALID_CONFIG",
            Self::UnsupportedSchema => "MP_SECURITY_UNSUPPORTED_SCHEMA",
            Self::AlreadyConfigured => "MP_SECURITY_ALREADY_CONFIGURED",
            Self::NotConfigured => "MP_SECURITY_NOT_CONFIGURED",
            Self::WeakSecret => "MP_SECURITY_WEAK_SECRET",
            Self::InvalidCredentials => "MP_SECURITY_INVALID_CREDENTIALS",
            Self::RateLimited { .. } => "MP_SECURITY_RATE_LIMITED",
            Self::Locked => "MP_LOCKED",
            Self::UnsafePath => "MP_SECURITY_UNSAFE_PATH",
            Self::Crypto => "MP_SECURITY_CRYPTO",
        }
    }

    pub fn retry_after_ms(&self) -> Option<u64> {
        match self {
            Self::RateLimited { retry_after_ms } => Some(*retry_after_ms),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct SecurityStatus {
    pub configured: bool,
    pub locked: bool,
    pub epoch: u64,
    pub retry_after_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnlockLease {
    epoch: u64,
}

impl UnlockLease {
    pub fn epoch(self) -> u64 {
        self.epoch
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct KdfConfig {
    algorithm: String,
    version: u32,
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredSecurityConfig {
    schema_version: u32,
    kdf: KdfConfig,
    password_hash: String,
}

#[derive(Debug)]
struct RuntimeState {
    configured: bool,
    locked: bool,
    epoch: u64,
    failed_attempts: u32,
    blocked_until: Option<Instant>,
}

#[derive(Debug)]
struct Inner {
    config_path: PathBuf,
    state: Mutex<RuntimeState>,
}

#[derive(Clone, Debug)]
pub struct SecurityGate {
    inner: Arc<Inner>,
}

impl SecurityGate {
    pub fn open(config_path: impl AsRef<Path>) -> Result<Self> {
        let config_path = config_path.as_ref().to_owned();
        validate_path(&config_path)?;
        let configured = if config_path.exists() {
            let config = read_config(&config_path)?;
            validate_config(&config)?;
            true
        } else {
            false
        };
        Ok(Self {
            inner: Arc::new(Inner {
                config_path,
                state: Mutex::new(RuntimeState {
                    configured,
                    locked: true,
                    epoch: 0,
                    failed_attempts: 0,
                    blocked_until: None,
                }),
            }),
        })
    }

    pub fn status(&self) -> SecurityStatus {
        let state = self.lock_state();
        SecurityStatus {
            configured: state.configured,
            locked: state.locked,
            epoch: state.epoch,
            retry_after_ms: retry_after_ms(&state),
        }
    }

    pub fn initialize(&self, secret: &[u8]) -> Result<SecurityStatus> {
        validate_secret(secret)?;
        let mut state = self.lock_state();
        if state.configured || self.inner.config_path.exists() {
            return Err(SecurityError::AlreadyConfigured);
        }

        let hash = hash_secret(secret)?;
        let config = StoredSecurityConfig {
            schema_version: SCHEMA_VERSION,
            kdf: KdfConfig {
                algorithm: "argon2id".into(),
                version: 19,
                memory_kib: ARGON2_MEMORY_KIB,
                iterations: ARGON2_ITERATIONS,
                parallelism: ARGON2_PARALLELISM,
            },
            password_hash: hash,
        };
        write_config_once(&self.inner.config_path, &config)?;
        state.configured = true;
        state.locked = false;
        state.failed_attempts = 0;
        state.blocked_until = None;
        advance_epoch(&mut state);
        Ok(status_from(&state))
    }

    pub fn unlock(&self, secret: &[u8]) -> Result<SecurityStatus> {
        validate_secret(secret)?;
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
        if verify_secret(secret, &config.password_hash).is_err() {
            state.locked = true;
            state.failed_attempts = state.failed_attempts.saturating_add(1);
            let delay = failure_delay(state.failed_attempts);
            state.blocked_until = (!delay.is_zero()).then(|| Instant::now() + delay);
            return Err(SecurityError::InvalidCredentials);
        }

        state.locked = false;
        state.failed_attempts = 0;
        state.blocked_until = None;
        advance_epoch(&mut state);
        Ok(status_from(&state))
    }

    pub fn lock(&self) -> SecurityStatus {
        let mut state = self.lock_state();
        state.locked = true;
        advance_epoch(&mut state);
        status_from(&state)
    }

    pub fn require_unlocked(&self) -> Result<UnlockLease> {
        let state = self.lock_state();
        if !state.configured || state.locked {
            return Err(SecurityError::Locked);
        }
        Ok(UnlockLease { epoch: state.epoch })
    }

    pub fn validate_lease(&self, lease: UnlockLease) -> Result<()> {
        let state = self.lock_state();
        if state.locked || !state.configured || state.epoch != lease.epoch {
            return Err(SecurityError::Locked);
        }
        Ok(())
    }

    pub fn config_path(&self) -> &Path {
        &self.inner.config_path
    }

    fn lock_state(&self) -> MutexGuard<'_, RuntimeState> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn status_from(state: &RuntimeState) -> SecurityStatus {
    SecurityStatus {
        configured: state.configured,
        locked: state.locked,
        epoch: state.epoch,
        retry_after_ms: retry_after_ms(state),
    }
}

fn advance_epoch(state: &mut RuntimeState) {
    state.epoch = state.epoch.wrapping_add(1);
    if state.epoch == 0 {
        state.epoch = 1;
    }
}

fn validate_secret(secret: &[u8]) -> Result<()> {
    if secret.len() < MIN_SECRET_BYTES
        || secret.len() > MAX_SECRET_BYTES
        || secret.iter().all(u8::is_ascii_whitespace)
    {
        return Err(SecurityError::WeakSecret);
    }
    Ok(())
}

fn argon2() -> Result<Argon2<'static>> {
    let params = Params::new(
        ARGON2_MEMORY_KIB,
        ARGON2_ITERATIONS,
        ARGON2_PARALLELISM,
        None,
    )
    .map_err(|_| SecurityError::Crypto)?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

fn hash_secret(secret: &[u8]) -> Result<String> {
    let mut salt_bytes = [0u8; 16];
    getrandom::fill(&mut salt_bytes).map_err(|_| SecurityError::Crypto)?;
    let salt = SaltString::encode_b64(&salt_bytes).map_err(|_| SecurityError::Crypto)?;
    argon2()?
        .hash_password(secret, &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| SecurityError::Crypto)
}

fn verify_secret(secret: &[u8], encoded: &str) -> Result<()> {
    let parsed = PasswordHash::new(encoded).map_err(|_| SecurityError::InvalidConfig)?;
    Argon2::default()
        .verify_password(secret, &parsed)
        .map_err(|_| SecurityError::InvalidCredentials)
}

fn validate_config(config: &StoredSecurityConfig) -> Result<()> {
    if config.schema_version != SCHEMA_VERSION {
        return Err(SecurityError::UnsupportedSchema);
    }
    if config.kdf.algorithm != "argon2id"
        || config.kdf.version != 19
        || config.kdf.memory_kib < 19_456
        || config.kdf.iterations < 2
        || config.kdf.parallelism == 0
    {
        return Err(SecurityError::InvalidConfig);
    }
    let parsed =
        PasswordHash::new(&config.password_hash).map_err(|_| SecurityError::InvalidConfig)?;
    if parsed.algorithm.as_str() != "argon2id" {
        return Err(SecurityError::InvalidConfig);
    }
    Ok(())
}

fn remaining_block(state: &RuntimeState) -> Option<Duration> {
    state
        .blocked_until
        .and_then(|deadline| deadline.checked_duration_since(Instant::now()))
}

fn retry_after_ms(state: &RuntimeState) -> u64 {
    remaining_block(state).map(duration_ms_ceil).unwrap_or(0)
}

fn duration_ms_ceil(duration: Duration) -> u64 {
    duration.as_millis().saturating_add(1).min(u64::MAX as u128) as u64
}

fn failure_delay(failed_attempts: u32) -> Duration {
    if failed_attempts < FAILURE_DELAY_START {
        return Duration::ZERO;
    }
    let exponent = failed_attempts.saturating_sub(FAILURE_DELAY_START).min(6);
    Duration::from_secs((1u64 << exponent).min(MAX_FAILURE_DELAY_SECS))
}

fn read_config(path: &Path) -> Result<StoredSecurityConfig> {
    validate_path(path)?;
    let file = File::open(path)?;
    if file.metadata()?.len() > MAX_CONFIG_BYTES as u64 {
        return Err(SecurityError::InvalidConfig);
    }
    let mut bytes = Vec::new();
    file.take(MAX_CONFIG_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err(SecurityError::InvalidConfig);
    }
    serde_json::from_slice(&bytes).map_err(|_| SecurityError::InvalidConfig)
}

fn write_config_once(path: &Path, config: &StoredSecurityConfig) -> Result<()> {
    let parent = path.parent().ok_or(SecurityError::UnsafePath)?;
    validate_path(parent)?;
    fs::create_dir_all(parent)?;
    validate_path(parent)?;
    validate_path(path)?;

    let bytes = Zeroizing::new(
        serde_json::to_vec_pretty(config).map_err(|_| SecurityError::InvalidConfig)?,
    );
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(&bytes)?;
    file.as_file().sync_all()?;
    file.persist_noclobber(path).map_err(|error| error.error)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}

fn validate_path(path: &Path) -> Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() {
            return Err(SecurityError::UnsafePath);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(SecurityError::UnsafePath);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate() -> (tempfile::TempDir, SecurityGate) {
        let dir = tempfile::tempdir().unwrap();
        let gate = SecurityGate::open(dir.path().join("security-v1.json")).unwrap();
        (dir, gate)
    }

    #[test]
    fn starts_locked_and_unconfigured() {
        let (_dir, gate) = gate();
        assert_eq!(
            gate.status(),
            SecurityStatus {
                configured: false,
                locked: true,
                epoch: 0,
                retry_after_ms: 0,
            }
        );
        assert!(matches!(
            gate.require_unlocked(),
            Err(SecurityError::Locked)
        ));
    }

    #[test]
    fn initialize_lock_unlock_and_restart_are_fail_closed() {
        let (dir, gate) = gate();
        let status = gate.initialize(b"correct horse battery staple").unwrap();
        assert!(status.configured);
        assert!(!status.locked);
        let lease = gate.require_unlocked().unwrap();
        assert!(gate.validate_lease(lease).is_ok());

        gate.lock();
        assert!(matches!(
            gate.validate_lease(lease),
            Err(SecurityError::Locked)
        ));
        assert!(gate.unlock(b"correct horse battery staple").is_ok());

        let restarted = SecurityGate::open(dir.path().join("security-v1.json")).unwrap();
        assert!(restarted.status().configured);
        assert!(restarted.status().locked);
        assert!(matches!(
            restarted.require_unlocked(),
            Err(SecurityError::Locked)
        ));
    }

    #[test]
    fn weak_secret_is_rejected_without_creating_config() {
        let (_dir, gate) = gate();
        assert!(matches!(
            gate.initialize(b"1234567"),
            Err(SecurityError::WeakSecret)
        ));
        assert!(!gate.config_path().exists());
    }

    #[test]
    fn wrong_secret_triggers_backoff_without_unlocking() {
        let (_dir, gate) = gate();
        gate.initialize(b"correct horse battery staple").unwrap();
        gate.lock();
        for _ in 0..3 {
            assert!(matches!(
                gate.unlock(b"definitely wrong"),
                Err(SecurityError::InvalidCredentials)
            ));
        }
        assert!(gate.status().locked);
        assert!(gate.status().retry_after_ms > 0);
        gate.lock();
        assert!(gate.status().retry_after_ms > 0);
        assert!(matches!(
            gate.unlock(b"correct horse battery staple"),
            Err(SecurityError::RateLimited { .. })
        ));
    }

    #[test]
    fn malformed_or_downgraded_config_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("security-v1.json");
        fs::write(
            &path,
            br#"{"schema_version":1,"kdf":{"algorithm":"argon2id","version":19,"memory_kib":1,"iterations":1,"parallelism":1},"password_hash":"bad"}"#,
        )
        .unwrap();
        assert!(matches!(
            SecurityGate::open(path),
            Err(SecurityError::InvalidConfig)
        ));
    }
}

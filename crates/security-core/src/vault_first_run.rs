//! Durable owner-local intent for a new encrypted vault. No credentials, DEK,
//! or recovery code are stored here. An Argon2id-derived proof authenticates
//! the intent without introducing a cheap offline password verifier.
//!
//! Intent is published BEFORE the security verifier. The irreversible
//! "committing" tombstone is published BEFORE any vault bytes. Never remove
//! these markers to make a missing established vault look like a new install.
use crate::{Result, SecurityError};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD, Engine};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::Path,
};
use zeroize::Zeroizing;

const INTENT: &str = ".mp9b-first-run-intent-v1.json";
const COMMITTING: &str = ".mp9b-first-run-committing-v1";
const COMPLETE: &str = ".mp9b-first-run-complete-v1";
const DOMAIN: &[u8] = b"mind-place/first-run-intent/root/v1";
const MAX_INTENT_BYTES: u64 = 4096;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    format_version: u32,
    salt_b64: String,
    proof_b64: String,
}

pub struct FirstRunJournal;

impl FirstRunJournal {
    /// Called before initialize() on a new profile, or after authenticated
    /// unlock() on a profile that has not completed its first-run ceremony.
    pub fn ensure_start(root: &Path, secret: &[u8], configured: bool) -> Result<()> {
        validate_empty_root(root)?;
        let parent = root.parent().ok_or(SecurityError::UnsafePath)?;
        if parent.join(COMMITTING).exists() || parent.join(COMPLETE).exists() {
            return Err(SecurityError::InvalidConfig);
        }
        let path = parent.join(INTENT);
        safe_path(&path)?;
        if path.exists() {
            verify_intent(&path, root, secret)
        } else if configured {
            // A historical verifier-only profile cannot be distinguished from
            // an established vault with its directory deleted. Fail closed.
            Err(SecurityError::InvalidConfig)
        } else {
            let mut salt = [0u8; 16];
            getrandom::fill(&mut salt).map_err(|_| SecurityError::Crypto)?;
            let proof = proof(secret, root, &salt)?;
            let value = Intent {
                format_version: 1,
                salt_b64: STANDARD.encode(salt),
                proof_b64: STANDARD.encode(proof),
            };
            let json = serde_json::to_vec(&value).map_err(|_| SecurityError::InvalidConfig)?;
            write_once(&path, &json)
        }
    }

    /// Only return true following successful verifier authentication.
    /// Never interpret a missing intent or a missing/corrupt established
    /// manifest as authorization to create replacement vault keys.
    pub fn can_resume(root: &Path, secret: &[u8]) -> Result<bool> {
        validate_empty_root(root)?;
        let parent = root.parent().ok_or(SecurityError::UnsafePath)?;
        if parent.join(COMMITTING).exists() || parent.join(COMPLETE).exists() {
            return Err(SecurityError::InvalidConfig);
        }
        let path = parent.join(INTENT);
        safe_path(&path)?;
        if !path.exists() {
            return Ok(false);
        }
        verify_intent(&path, root, secret)?;
        Ok(true)
    }

    /// An independently confirmed recovery code is required by the caller
    /// before this irreversible boundary. Crash after this point fails closed.
    pub fn mark_committing(root: &Path) -> Result<()> {
        validate_empty_root(root)?;
        let parent = root.parent().ok_or(SecurityError::UnsafePath)?;
        if !parent.join(INTENT).is_file()
            || parent.join(COMMITTING).exists()
            || parent.join(COMPLETE).exists()
        {
            return Err(SecurityError::InvalidConfig);
        }
        write_once(&parent.join(COMMITTING), b"mp9b-first-run-committing-v1\n")
    }

    /// Durable completion marker persists even if the vault is later lost.
    pub fn mark_complete(root: &Path) -> Result<()> {
        safe_path(root)?;
        let parent = root.parent().ok_or(SecurityError::UnsafePath)?;
        if !parent.join(COMMITTING).is_file() || !root.join("vault-v1.json").is_file() {
            return Err(SecurityError::InvalidConfig);
        }
        write_once(&parent.join(COMPLETE), b"mp9b-first-run-complete-v1\n")
    }
}

fn validate_empty_root(root: &Path) -> Result<()> {
    if !root.is_absolute() {
        return Err(SecurityError::UnsafePath);
    }
    safe_path(root)?;
    let parent = root.parent().ok_or(SecurityError::UnsafePath)?;
    if !parent.is_dir() || root.exists() || root.with_extension("mp9b-init-partial").exists() {
        return Err(SecurityError::InvalidConfig);
    }
    for name in [
        "state.sqlite3",
        "index.sqlite3",
        "archive-v1",
        "document-objects",
    ] {
        if parent.join(name).exists() {
            return Err(SecurityError::InvalidConfig);
        }
    }
    Ok(())
}

fn safe_path(path: &Path) -> Result<()> {
    for ancestor in path.ancestors() {
        if !ancestor.as_os_str().is_empty() {
            super::validate_path(ancestor)?;
        }
    }
    Ok(())
}

fn proof(secret: &[u8], root: &Path, salt: &[u8]) -> Result<[u8; 32]> {
    let params = Params::new(65_536, 3, 1, Some(32)).map_err(|_| SecurityError::Crypto)?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = Zeroizing::new([0u8; 32]);
    argon
        .hash_password_into(secret, salt, &mut key[..])
        .map_err(|_| SecurityError::Crypto)?;
    let mut mac = <Hmac<sha2::Sha256> as Mac>::new_from_slice(key.as_ref())
        .map_err(|_| SecurityError::Crypto)?;
    mac.update(DOMAIN);
    mac.update(root.to_string_lossy().as_bytes());
    let bytes = mac.finalize().into_bytes();
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn verify_intent(path: &Path, root: &Path, secret: &[u8]) -> Result<()> {
    safe_path(path)?;
    let file = File::open(path)?;
    let mut json = Vec::new();
    file.take(MAX_INTENT_BYTES + 1).read_to_end(&mut json)?;
    if json.len() as u64 > MAX_INTENT_BYTES {
        return Err(SecurityError::InvalidConfig);
    }
    let intent: Intent = serde_json::from_slice(&json).map_err(|_| SecurityError::InvalidConfig)?;
    if intent.format_version != 1 {
        return Err(SecurityError::UnsupportedSchema);
    }
    let salt = STANDARD
        .decode(intent.salt_b64)
        .map_err(|_| SecurityError::InvalidConfig)?;
    let tag = STANDARD
        .decode(intent.proof_b64)
        .map_err(|_| SecurityError::InvalidConfig)?;
    if salt.len() != 16 || tag.len() != 32 {
        return Err(SecurityError::InvalidConfig);
    }
    let expected = proof(secret, root, &salt)?;
    // HMAC compares equal-length tags in constant time.
    let mut mac = <Hmac<sha2::Sha256> as Mac>::new_from_slice(&[0u8; 32])
        .map_err(|_| SecurityError::Crypto)?;
    mac.update(&expected);
    let known = mac.finalize().into_bytes();
    let mut verify = <Hmac<sha2::Sha256> as Mac>::new_from_slice(&[0u8; 32])
        .map_err(|_| SecurityError::Crypto)?;
    verify.update(&tag);
    if verify.verify_slice(&known).is_err() {
        return Err(SecurityError::InvalidConfig);
    }
    Ok(())
}

fn write_once(path: &Path, content: &[u8]) -> Result<()> {
    safe_path(path)?;
    let parent = path.parent().ok_or(SecurityError::UnsafePath)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(content)?;
    file.as_file().sync_all()?;
    file.persist_noclobber(path).map_err(|error| error.error)?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authenticated_intent_restart_and_commit_fence() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("encrypted-vault-v1");
        let pass = b"synthetic correct first run secret";
        FirstRunJournal::ensure_start(&root, pass, false).unwrap();
        assert!(FirstRunJournal::can_resume(&root, pass).unwrap());
        assert!(FirstRunJournal::can_resume(&root, b"wrong synthetic secret").is_err());
        FirstRunJournal::ensure_start(&root, pass, true).unwrap();
        FirstRunJournal::mark_committing(&root).unwrap();
        assert!(FirstRunJournal::can_resume(&root, pass).is_err());
        assert!(FirstRunJournal::ensure_start(&root, pass, true).is_err());
        assert!(FirstRunJournal::mark_committing(&root).is_err());
        let marker = fs::read(home.path().join(INTENT)).unwrap();
        assert!(!marker.windows(pass.len()).any(|w| w == pass));
    }

    #[test]
    fn unmarked_or_legacy_or_tampered_root_fails_closed() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("encrypted-vault-v1");
        assert!(!FirstRunJournal::can_resume(&root, b"correct test secret").unwrap());
        assert!(FirstRunJournal::ensure_start(&root, b"correct test secret", true).is_err());
        FirstRunJournal::ensure_start(&root, b"correct test secret", false).unwrap();
        let intent = home.path().join(INTENT);
        fs::write(&intent, b"{}").unwrap();
        assert!(FirstRunJournal::can_resume(&root, b"correct test secret").is_err());
        fs::write(home.path().join("index.sqlite3"), b"synthetic").unwrap();
        assert!(FirstRunJournal::can_resume(&root, b"correct test secret").is_err());
    }
}

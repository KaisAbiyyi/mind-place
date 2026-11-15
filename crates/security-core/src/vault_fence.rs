//! Cross-process advisory writer fence for the MP-9B encrypted vault.
//!
//! Writers must hold a shared lock through ciphertext publication. A portable
//! snapshot takes the exclusive lock for its entire inventory/copy/verify/publish
//! cycle. Encrypted state/index connections, immutable storage, capture commit
//! and complete approved-root document generations participate. MCP/IPC use
//! those adapters. Plaintext legacy runtime is outside this protocol; migration
//! requires explicit offline owner maintenance. Future writers must join it.
use crate::{Result, SecurityError};
use fs2::FileExt;
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::ErrorKind,
    marker::PhantomData,
    path::{Path, PathBuf},
};

thread_local! {
    /// Privacy disclosure fences held by THIS thread, per lock file. A
    /// disclosure that spans a whole command (query -> response) nests the
    /// archive's own read fence; re-entry is counted here instead of asking
    /// the OS for a second shared lock, so nesting never depends on platform
    /// lock fairness and can never queue behind a waiting redaction.
    /// The OS lock file lives here, so it is released only when the LAST
    /// guard on this thread drops, whatever order nested guards drop in.
    static HELD_PRIVACY_READERS: RefCell<HashMap<PathBuf, (usize, File)>> = RefCell::new(HashMap::new());
}

/// Shared privacy disclosure guard. Thread-bound (`!Send`): it must be
/// released on the thread that acquired it, because re-entry is tracked per
/// thread. While any guard for a vault is alive on any thread, a source
/// redaction (exclusive `privacy`) for that vault cannot commit.
pub struct PrivacyReadGuard {
    path: PathBuf,
    _thread_bound: PhantomData<*const ()>,
}

impl Drop for PrivacyReadGuard {
    fn drop(&mut self) {
        let released = HELD_PRIVACY_READERS.with(|held| {
            let mut held = held.borrow_mut();
            let last = match held.get_mut(&self.path) {
                Some((count, _)) => {
                    *count -= 1;
                    *count == 0
                }
                None => false,
            };
            if last {
                held.remove(&self.path)
            } else {
                None
            }
        });
        // Dropping the File of the last guard releases the shared OS lock.
        drop(released);
    }
}

fn holds_privacy_reader(path: &Path) -> bool {
    HELD_PRIVACY_READERS.with(|held| held.borrow().contains_key(path))
}

pub const WRITER_LOCK_NAME: &str = ".vault-writer.lock";
/// Independent privacy serialization: capture and source redaction acquire
/// this EXCLUSIVE lock before any nested shared writer/SQLite connection.
// Advisory sentinel lives outside the vault and, for the fixed app child,
// outside the legacy profile too. It contains no authority or user data.
pub const PRIVACY_LOCK_NAME: &str = ".vault-privacy.lock";

pub struct VaultWriterFence {
    _file: File,
}

impl VaultWriterFence {
    pub fn writer(vault_root: &Path) -> Result<Self> {
        let file = open_lock(vault_root, WRITER_LOCK_NAME)?;
        FileExt::lock_shared(&file)?;
        Ok(Self { _file: file })
    }

    pub fn snapshot(vault_root: &Path) -> Result<Self> {
        let file = open_lock(vault_root, WRITER_LOCK_NAME)?;
        FileExt::lock_exclusive(&file)?;
        Ok(Self { _file: file })
    }

    /// Shared disclosure fence. A source-redaction commit holds the matching
    /// EXCLUSIVE fence, so a reader which checked visibility cannot overlap a
    /// committed tombstone. Acquire before opening state or writer connections.
    /// Re-entrant on the same thread (see `PrivacyReadGuard`).
    pub fn privacy_reader(vault_root: &Path) -> Result<PrivacyReadGuard> {
        let path = privacy_path(vault_root)?;
        let nested = HELD_PRIVACY_READERS.with(|held| {
            let mut held = held.borrow_mut();
            match held.get_mut(&path) {
                Some((count, _)) => {
                    *count += 1;
                    true
                }
                None => false,
            }
        });
        if !nested {
            let file = privacy_file(vault_root)?;
            FileExt::lock_shared(&file)?;
            HELD_PRIVACY_READERS.with(|held| held.borrow_mut().insert(path.clone(), (1, file)));
        }
        Ok(PrivacyReadGuard {
            path,
            _thread_bound: PhantomData,
        })
    }

    /// Exclusive source mutation fence; always acquire before shared writer,
    /// SQLCipher and immutable storage locks (never upgrade a shared lock).
    /// A thread that still holds a disclosure guard for this vault would
    /// deadlock on itself; that is refused instead.
    pub fn privacy(vault_root: &Path) -> Result<Self> {
        if holds_privacy_reader(&privacy_path(vault_root)?) {
            return Err(SecurityError::InvalidConfig);
        }
        let file = privacy_file(vault_root)?;
        FileExt::lock_exclusive(&file)?;
        Ok(Self { _file: file })
    }
}

fn privacy_path(vault_root: &Path) -> Result<PathBuf> {
    for ancestor in vault_root.ancestors() {
        if !ancestor.as_os_str().is_empty() {
            crate::validate_path(ancestor)?;
        }
    }
    if !vault_root.is_dir() {
        return Err(SecurityError::InvalidConfig);
    }
    // Canonical identity is shared by nested guards and separate processes.
    let root = vault_root.canonicalize()?;
    let parent = root.parent().ok_or(SecurityError::InvalidConfig)?;
    let name = root.file_name().and_then(|name| name.to_str());
    let app_child = name.is_some_and(|name| {
        if cfg!(windows) {
            name.eq_ignore_ascii_case("encrypted-vault-v1")
        } else {
            name == "encrypted-vault-v1"
        }
    });
    if app_child {
        // Offline migration publishes this fixed child beneath its immutable
        // legacy source. A sibling sentinel there would contaminate the strict
        // source inventory on the first post-publication disclosure. Keep it
        // outside the ENTIRE profile, like migration's existing work artifacts.
        // Never delete/adopt an old in-profile sentinel or relax an inventory.
        let outside = parent.parent().ok_or(SecurityError::InvalidConfig)?;
        let identity = Sha256::digest(root.as_os_str().as_encoded_bytes());
        return Ok(outside.join(format!(".mp9c-{identity:x}.privacy.lock")));
    }
    Ok(parent.join(PRIVACY_LOCK_NAME))
}

fn privacy_file(vault_root: &Path) -> Result<File> {
    let path = privacy_path(vault_root)?;
    let parent = path.parent().ok_or(SecurityError::InvalidConfig)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(SecurityError::InvalidConfig)?;
    // Outside the portable-backup inventory: coordination is never authority.
    open_lock(parent, name)
}

fn open_lock(vault_root: &Path, name: &str) -> Result<File> {
    for ancestor in vault_root.ancestors() {
        if !ancestor.as_os_str().is_empty() {
            crate::validate_path(ancestor)?;
        }
    }
    if !vault_root.is_dir() {
        return Err(SecurityError::InvalidConfig);
    }
    let path = vault_root.join(name);
    crate::validate_path(&path)?;
    let file = match OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            crate::validate_path(&path)?;
            OpenOptions::new().read(true).write(true).open(&path)?
        }
        Err(error) => return Err(SecurityError::Io(error)),
    };
    let meta = fs::symlink_metadata(&path)?;
    if !meta.is_file() || meta.len() != 0 {
        return Err(SecurityError::InvalidConfig);
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_child_disclosure_does_not_change_the_strict_legacy_inventory() {
        use crate::vault_migration::LegacyMigrationPreflight;
        let home = tempfile::tempdir().unwrap();
        let legacy = home.path().join("app");
        fs::create_dir(&legacy).unwrap();
        fs::write(legacy.join("state.sqlite3"), b"SQLite format 3\0synthetic").unwrap();
        let root = legacy.join("encrypted-vault-v1");
        let before: Vec<_> = fs::read_dir(&legacy)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        LegacyMigrationPreflight::inspect(&legacy, &root).unwrap();
        fs::create_dir(&root).unwrap();
        let read = VaultWriterFence::privacy_reader(&root).unwrap();
        drop(read);
        let recovery = LegacyMigrationPreflight::recovery_authorization_root(&legacy, &root);
        let mut after: Vec<_> = fs::read_dir(&legacy)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name != "encrypted-vault-v1")
            .collect();
        after.sort();
        eprintln!(
            "synthetic legacy entries: {after:?}; recovery accepted: {}",
            recovery.is_ok()
        );
        assert!(
            recovery.is_ok(),
            "a disclosure contaminated the legacy inventory"
        );
        assert_eq!(
            after, before,
            "privacy coordination must not modify the source root"
        );
        assert_eq!(
            fs::read(legacy.join("state.sqlite3")).unwrap(),
            b"SQLite format 3\0synthetic"
        );
    }

    #[test]
    fn app_child_nested_aliases_keep_one_fence_until_the_last_guard_drops() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("app/encrypted-vault-v1");
        fs::create_dir_all(&root).unwrap();
        let outer = VaultWriterFence::privacy_reader(&root).unwrap();
        let inner = VaultWriterFence::privacy_reader(&root.join(".")).unwrap();
        assert_eq!(outer.path, inner.path);
        assert!(!outer.path.starts_with(root.parent().unwrap()));
        let competing = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&outer.path)
            .unwrap();
        assert!(FileExt::try_lock_exclusive(&competing).is_err());
        assert!(VaultWriterFence::privacy(&root.join(".")).is_err());
        drop(outer);
        assert!(FileExt::try_lock_exclusive(&competing).is_err());
        drop(inner);
        FileExt::try_lock_exclusive(&competing).unwrap();
        FileExt::unlock(&competing).unwrap();
        let _exclusive = VaultWriterFence::privacy(&root).unwrap();
    }

    #[test]
    fn app_child_rejects_nonempty_or_directory_sentinel_without_repair() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("app/encrypted-vault-v1");
        fs::create_dir_all(&root).unwrap();
        let path = privacy_path(&root).unwrap();
        fs::write(&path, b"synthetic-unknown").unwrap();
        assert!(VaultWriterFence::privacy_reader(&root).is_err());
        assert!(VaultWriterFence::privacy(&root).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"synthetic-unknown");

        let other = home.path().join("other/encrypted-vault-v1");
        fs::create_dir_all(&other).unwrap();
        let directory = privacy_path(&other).unwrap();
        fs::create_dir(&directory).unwrap();
        assert!(VaultWriterFence::privacy_reader(&other).is_err());
        assert!(VaultWriterFence::privacy(&other).is_err());
        assert!(directory.is_dir());
    }

    #[test]
    fn old_in_profile_sentinel_is_still_rejected_by_migration_and_never_removed() {
        use crate::vault_migration::LegacyMigrationPreflight;
        let home = tempfile::tempdir().unwrap();
        let legacy = home.path().join("app");
        let root = legacy.join("encrypted-vault-v1");
        fs::create_dir_all(&root).unwrap();
        fs::write(legacy.join("state.sqlite3"), b"SQLite format 3\0synthetic").unwrap();
        let old = legacy.join(PRIVACY_LOCK_NAME);
        fs::write(&old, b"").unwrap();
        let _reader = VaultWriterFence::privacy_reader(&root).unwrap();
        assert!(matches!(
            LegacyMigrationPreflight::recovery_authorization_root(&legacy, &root),
            Err(SecurityError::InvalidConfig)
        ));
        assert_eq!(fs::read(&old).unwrap(), b"");
    }

    #[test]
    fn app_child_privacy_process_probe() {
        let Some(root) = std::env::var_os("MP9C_SYNTHETIC_FENCE_PROBE") else {
            return;
        };
        let file = privacy_file(Path::new(&root)).unwrap();
        assert!(
            FileExt::try_lock_exclusive(&file).is_err(),
            "another process bypassed the reader"
        );
    }

    #[test]
    fn app_child_privacy_reader_excludes_a_separate_process() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("app/encrypted-vault-v1");
        fs::create_dir_all(&root).unwrap();
        let _reader = VaultWriterFence::privacy_reader(&root).unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "vault_fence::tests::app_child_privacy_process_probe",
                "--nocapture",
            ])
            .env("MP9C_SYNTHETIC_FENCE_PROBE", &root)
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[test]
    fn multiple_writers_block_snapshot_until_both_release() {
        let home = tempfile::tempdir().unwrap();
        let one = VaultWriterFence::writer(home.path()).unwrap();
        let two = VaultWriterFence::writer(home.path()).unwrap();
        let competing = OpenOptions::new()
            .read(true)
            .write(true)
            .open(home.path().join(WRITER_LOCK_NAME))
            .unwrap();
        assert!(FileExt::try_lock_exclusive(&competing).is_err());
        drop(one);
        assert!(FileExt::try_lock_exclusive(&competing).is_err());
        drop(two);
        FileExt::try_lock_exclusive(&competing).unwrap();
        FileExt::unlock(&competing).unwrap();
        let _snapshot = VaultWriterFence::snapshot(home.path()).unwrap();
    }

    #[test]
    fn privacy_exclusive_is_separate_from_shared_writer_and_snapshot_lock() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("vault");
        fs::create_dir(&root).unwrap();
        let _writer = VaultWriterFence::writer(&root).unwrap();
        let _privacy = VaultWriterFence::privacy(&root).unwrap();
        let competing = OpenOptions::new()
            .read(true)
            .write(true)
            .open(home.path().join(PRIVACY_LOCK_NAME))
            .unwrap();
        assert!(FileExt::try_lock_exclusive(&competing).is_err());
        // The shared writer and privacy locks can coexist, avoiding a
        // self-deadlock in nested encrypted SQLite/object writes.
    }

    #[test]
    fn shared_disclosure_fence_blocks_exclusive_redaction_transition() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("vault");
        fs::create_dir(&root).unwrap();
        let _read1 = VaultWriterFence::privacy_reader(&root).unwrap();
        let _read2 = VaultWriterFence::privacy_reader(&root).unwrap();
        let competing = OpenOptions::new()
            .read(true)
            .write(true)
            .open(home.path().join(PRIVACY_LOCK_NAME))
            .unwrap();
        assert!(FileExt::try_lock_exclusive(&competing).is_err());
        drop(_read1);
        assert!(FileExt::try_lock_exclusive(&competing).is_err());
        drop(_read2);
        FileExt::try_lock_exclusive(&competing).unwrap();
        FileExt::unlock(&competing).unwrap();
        let _privacy = VaultWriterFence::privacy(&root).unwrap();
    }

    #[test]
    fn nested_disclosure_guard_is_reentrant_and_blocks_redaction_until_outermost_drop() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("vault");
        fs::create_dir(&root).unwrap();
        let outer = VaultWriterFence::privacy_reader(&root).unwrap();
        let inner = VaultWriterFence::privacy_reader(&root).unwrap();
        let competing = OpenOptions::new()
            .read(true)
            .write(true)
            .open(home.path().join(PRIVACY_LOCK_NAME))
            .unwrap();
        // Same-thread exclusive acquisition is refused, not self-deadlocked.
        assert!(VaultWriterFence::privacy(&root).is_err());
        drop(inner);
        assert!(FileExt::try_lock_exclusive(&competing).is_err());
        drop(outer);
        FileExt::try_lock_exclusive(&competing).unwrap();
        FileExt::unlock(&competing).unwrap();
        let _privacy = VaultWriterFence::privacy(&root).unwrap();
    }

    #[test]
    fn redaction_on_another_thread_waits_for_the_outermost_disclosure_guard() {
        use std::{sync::mpsc, thread, time::Duration};
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("vault");
        fs::create_dir(&root).unwrap();
        let outer = VaultWriterFence::privacy_reader(&root).unwrap();
        let inner = VaultWriterFence::privacy_reader(&root).unwrap();
        let (tx, rx) = mpsc::channel();
        let redactor_root = root.clone();
        let redactor = thread::spawn(move || {
            let _exclusive = VaultWriterFence::privacy(&redactor_root).unwrap();
            tx.send(()).unwrap();
        });
        assert!(rx.recv_timeout(Duration::from_millis(150)).is_err());
        drop(inner);
        assert!(rx.recv_timeout(Duration::from_millis(150)).is_err());
        drop(outer);
        rx.recv_timeout(Duration::from_secs(10)).unwrap();
        redactor.join().unwrap();
    }

    #[test]
    fn refuse_nonzero_or_nonfile_lock_before_writing() {
        let home = tempfile::tempdir().unwrap();
        let lock = home.path().join(WRITER_LOCK_NAME);
        fs::write(&lock, b"tampered").unwrap();
        assert!(VaultWriterFence::writer(home.path()).is_err());
        assert!(VaultWriterFence::snapshot(home.path()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_lock_is_not_accepted() {
        use std::os::unix::fs::symlink;
        let home = tempfile::tempdir().unwrap();
        let secret = home.path().join("outside");
        fs::write(&secret, b"do not touch").unwrap();
        symlink(&secret, home.path().join(WRITER_LOCK_NAME)).unwrap();
        assert!(VaultWriterFence::snapshot(home.path()).is_err());
        assert_eq!(fs::read(secret).unwrap(), b"do not touch");
    }
}

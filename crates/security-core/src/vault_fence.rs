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
use std::{
    fs::{self, File, OpenOptions},
    io::ErrorKind,
    path::Path,
};

pub const WRITER_LOCK_NAME: &str = ".vault-writer.lock";

pub struct VaultWriterFence {
    _file: File,
}

impl VaultWriterFence {
    pub fn writer(vault_root: &Path) -> Result<Self> {
        let file = open_lock(vault_root)?;
        FileExt::lock_shared(&file)?;
        Ok(Self { _file: file })
    }

    pub fn snapshot(vault_root: &Path) -> Result<Self> {
        let file = open_lock(vault_root)?;
        FileExt::lock_exclusive(&file)?;
        Ok(Self { _file: file })
    }
}

fn open_lock(vault_root: &Path) -> Result<File> {
    for ancestor in vault_root.ancestors() {
        if !ancestor.as_os_str().is_empty() {
            crate::validate_path(ancestor)?;
        }
    }
    if !vault_root.is_dir() {
        return Err(SecurityError::InvalidConfig);
    }
    let path = vault_root.join(WRITER_LOCK_NAME);
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

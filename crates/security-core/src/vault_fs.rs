//! Shared durable vault publication without replacing an unrelated destination.
#[cfg(not(windows))]
use std::fs;
use std::{io, path::Path};

pub(crate) fn rename(source: &Path, destination: &Path, replace: bool) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        extern "system" {
            fn MoveFileExW(source: *const u16, destination: *const u16, flags: u32) -> i32;
        }
        let encode = |path: &Path| -> io::Result<Vec<u16>> {
            let mut value: Vec<u16> = path.as_os_str().encode_wide().collect();
            if value.contains(&0) {
                return Err(io::ErrorKind::InvalidInput.into());
            }
            value.push(0);
            Ok(value)
        };
        let source = encode(source)?;
        let target = encode(destination)?;
        // WRITE_THROUGH; REPLACE_EXISTING only for an authenticated journal
        // transition, never for a staging directory or rollback quarantine.
        if unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), 8 | u32::from(replace)) } == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    #[cfg(target_os = "linux")]
    {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};
        extern "C" {
            fn renameat2(
                old_dir: i32,
                old: *const std::ffi::c_char,
                new_dir: i32,
                new: *const std::ffi::c_char,
                flags: u32,
            ) -> i32;
        }
        let old =
            CString::new(source.as_os_str().as_bytes()).map_err(|_| io::ErrorKind::InvalidInput)?;
        let new = CString::new(destination.as_os_str().as_bytes())
            .map_err(|_| io::ErrorKind::InvalidInput)?;
        if unsafe { renameat2(-100, old.as_ptr(), -100, new.as_ptr(), u32::from(!replace)) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        if !replace && destination.exists() {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        fs::rename(source, destination)?;
    }
    #[cfg(unix)]
    fs::File::open(destination.parent().ok_or(io::ErrorKind::InvalidInput)?)?.sync_all()?;
    Ok(())
}

fn guarded(path: &Path) -> crate::Result<()> {
    for part in path.ancestors() {
        if !part.as_os_str().is_empty() {
            crate::validate_path(part)?;
        }
    }
    Ok(())
}

/// Resolve aliases before checking overlap or creating any staging directory.
pub fn checked_new_destination(
    source: &Path,
    destination: &Path,
) -> crate::Result<std::path::PathBuf> {
    use crate::SecurityError;
    if !source.is_absolute() || !destination.is_absolute() {
        return Err(SecurityError::UnsafePath);
    }
    guarded(source)?;
    guarded(destination)?;
    let source = source.canonicalize()?;
    let parent = destination.parent().ok_or(SecurityError::UnsafePath)?;
    let target = parent
        .canonicalize()?
        .join(destination.file_name().ok_or(SecurityError::UnsafePath)?);
    if !source.is_dir() || target.starts_with(&source) || source.starts_with(&target) {
        return Err(SecurityError::UnsafePath);
    }
    if target.exists() {
        return Err(SecurityError::AlreadyConfigured);
    }
    Ok(target)
}

pub fn publish_new_directory(staged: &Path, destination: &Path) -> crate::Result<()> {
    guarded(staged)?;
    guarded(destination)?;
    if !staged.is_dir() {
        return Err(crate::SecurityError::UnsafePath);
    }
    rename(staged, destination, false)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn publication_never_replaces_existing_empty_directory_and_aliases_do_not_overlap() {
        let home = tempfile::tempdir().unwrap();
        let source = home.path().join("source");
        let stage = home.path().join("stage");
        let target = home.path().join("target");
        std::fs::create_dir(&source).unwrap();
        std::fs::create_dir(&stage).unwrap();
        std::fs::create_dir(&target).unwrap();
        std::fs::write(stage.join("kept"), b"synthetic").unwrap();
        assert!(publish_new_directory(&stage, &target).is_err());
        assert!(stage.join("kept").exists());
        assert!(std::fs::read_dir(&target).unwrap().next().is_none());
        assert!(checked_new_destination(&source, &source.join("../source/new")).is_err());
        assert!(checked_new_destination(&source, Path::new("relative")).is_err());
    }
}

//! Atomic migration publication without replacing an unrelated destination.
#[cfg(not(windows))]
use std::fs;
use std::{io, path::Path};

pub(super) fn rename(source: &Path, destination: &Path, replace: bool) -> io::Result<()> {
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

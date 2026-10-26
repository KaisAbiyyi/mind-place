//! Windows owner-only DACL for the short-lived native capture IPC capability.
//! Never inherit an app-data directory's possibly broader ACL. Fail closed on
//! non-NTFS filesystems or if the OS cannot attest the published descriptor.
use std::{
    ffi::c_void,
    io,
    iter,
    mem::size_of,
    os::windows::{ffi::OsStrExt, io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle}},
    path::Path,
};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{
        Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW,
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            SDDL_REVISION_1,
        },
        DACL_SECURITY_INFORMATION, GetFileSecurityW, GetTokenInformation,
        PROTECTED_DACL_SECURITY_INFORMATION, SetFileSecurityW, TOKEN_QUERY, TOKEN_USER,
        TokenUser,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

struct LocalAllocation(*mut c_void);
impl Drop for LocalAllocation {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { LocalFree(self.0) };
        }
    }
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(iter::once(0)).collect()
}

fn current_sid() -> io::Result<String> {
    unsafe {
        let mut raw = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) == 0 {
            return Err(io::Error::last_os_error());
        }
        // OwnedHandle closes the token even when a later API fails.
        let token = OwnedHandle::from_raw_handle(raw as RawHandle);
        let mut required = 0u32;
        GetTokenInformation(
            token.as_raw_handle() as _,
            TokenUser,
            std::ptr::null_mut(),
            0,
            &mut required,
        );
        if required == 0 || required > 4096 {
            return Err(io::Error::last_os_error());
        }
        let mut buffer = vec![0usize; (required as usize).div_ceil(size_of::<usize>())];
        if GetTokenInformation(
            token.as_raw_handle() as _,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            required,
            &mut required,
        ) == 0 {
            return Err(io::Error::last_os_error());
        }
        let token_user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let mut sid = std::ptr::null_mut();
        if ConvertSidToStringSidW(token_user.User.Sid, &mut sid) == 0 {
            return Err(io::Error::last_os_error());
        }
        let allocation = LocalAllocation(sid.cast());
        let len = (0..256).take_while(|&index| *sid.add(index) != 0).count();
        if len == 256 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "SID too long"));
        }
        let value = String::from_utf16(std::slice::from_raw_parts(sid, len))
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid SID"))?;
        drop(allocation);
        Ok(value)
    }
}

fn set_dacl(path: &Path, sddl: &str) -> io::Result<()> {
    let encoded = sddl.encode_utf16().chain(iter::once(0)).collect::<Vec<_>>();
    let mut descriptor = std::ptr::null_mut();
    unsafe {
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            encoded.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        ) == 0 {
            return Err(io::Error::last_os_error());
        }
        let owned = LocalAllocation(descriptor);
        let result = SetFileSecurityW(
            wide(path).as_ptr(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor,
        );
        drop(owned);
        if result == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

fn get_dacl(path: &Path) -> io::Result<String> {
    unsafe {
        let name = wide(path);
        let mut required = 0u32;
        GetFileSecurityW(
            name.as_ptr(),
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            0,
            &mut required,
        );
        if required == 0 || required > 65536 {
            return Err(io::Error::last_os_error());
        }
        let mut data = vec![0usize; (required as usize).div_ceil(size_of::<usize>())];
        if GetFileSecurityW(
            name.as_ptr(),
            DACL_SECURITY_INFORMATION,
            data.as_mut_ptr().cast(),
            required,
            &mut required,
        ) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut sddl = std::ptr::null_mut();
        if ConvertSecurityDescriptorToStringSecurityDescriptorW(
            data.as_mut_ptr().cast(),
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &mut sddl,
            std::ptr::null_mut(),
        ) == 0 {
            return Err(io::Error::last_os_error());
        }
        let owned = LocalAllocation(sddl.cast());
        let len = (0..4096).take_while(|&index| *sddl.add(index) != 0).count();
        if len == 4096 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "DACL too long"));
        }
        let result = String::from_utf16(std::slice::from_raw_parts(sddl, len))
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid DACL"))?;
        drop(owned);
        Ok(result)
    }
}

/// Protect a descriptor *before* rename/publication. Both the owner SID and
/// LocalSystem need full control; inherited/everyone/group grants are excluded.
pub(super) fn protect(path: &Path) -> io::Result<()> {
    let sid = current_sid()?;
    set_dacl(path, &format!("D:P(A;;FA;;;SY)(A;;FA;;;{sid})"))?;
    verify(path)
}

/// Called by BOTH desktop and native host; weak or altered DACLs are refused.
/// The formatted DACL must have exactly two full-control allow ACEs and no
/// deny, inherited, callback, group, Everyone or app-package ACEs.
pub(super) fn verify(path: &Path) -> io::Result<()> {
    let sddl = get_dacl(path)?;
    let current = current_sid()?;
    if !sddl.starts_with("D:P") {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "IPC DACL inheritance"));
    }
    let mut principals = Vec::new();
    for entry in sddl.split('(').skip(1) {
        let entry = entry
            .strip_suffix(')')
            .ok_or_else(|| io::Error::new(io::ErrorKind::PermissionDenied, "IPC DACL shape"))?;
        let fields: Vec<_> = entry.split(';').collect();
        if fields.len() != 6 || fields[0] != "A" || !fields[1].is_empty()
            || !matches!(fields[2], "FA" | "0x1f01ff")
            || !fields[3].is_empty() || !fields[4].is_empty()
        {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "IPC DACL ACE"));
        }
        principals.push(fields[5]);
    }
    let owner = principals.iter().filter(|&&sid| sid == current).count();
    let system = principals.iter().filter(|&&sid| sid == "SY" || sid == "S-1-5-18").count();
    if principals.len() != 2 || owner != 1 || system != 1 {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "IPC DACL principals"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipc_descriptor_is_owner_only_and_rejects_everyone() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("capture-ipc-v1.json");
        std::fs::write(&path, b"synthetic-secret").unwrap();
        protect(&path).unwrap();
        verify(&path).unwrap();
        set_dacl(&path, "D:P(A;;FA;;;WD)").unwrap();
        assert!(verify(&path).is_err());
        // Restore private permissions before the tempfile is disposed.
        protect(&path).unwrap();
    }
}

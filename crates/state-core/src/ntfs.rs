//! Capability boundary: NTFS handle-relative no-reparse traversal.
use std::{fs::File, io, path::Path};

#[cfg(windows)]
mod platform {
    use super::*;
    use std::{
        ffi::c_void,
        fs::OpenOptions,
        os::windows::{
            fs::OpenOptionsExt,
            io::{AsRawHandle, FromRawHandle},
        },
        ptr,
    };

    const REPARSE: u32 = 0x400;
    const DIRECTORY: u32 = 0x10;
    const OPEN_REPARSE: u32 = 0x00200000;
    const BACKUP_SEMANTICS: u32 = 0x02000000;

    #[repr(C)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    #[repr(C)]
    struct FileInfo {
        attributes: u32,
        created: FileTime,
        accessed: FileTime,
        written: FileTime,
        volume: u32,
        size_high: u32,
        size_low: u32,
        links: u32,
        index_high: u32,
        index_low: u32,
    }
    #[repr(C)]
    struct UnicodeString {
        length: u16,
        maximum_length: u16,
        buffer: *mut u16,
    }
    #[repr(C)]
    struct ObjectAttributes {
        length: u32,
        root: isize,
        name: *mut UnicodeString,
        attributes: u32,
        security_descriptor: *mut c_void,
        quality_of_service: *mut c_void,
    }
    #[repr(C)]
    struct IoStatusBlock {
        status: usize,
        information: usize,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetFileInformationByHandle(handle: isize, info: *mut FileInfo) -> i32;
    }
    #[link(name = "ntdll")]
    extern "system" {
        fn NtCreateFile(
            handle: *mut isize,
            access: u32,
            attributes: *mut ObjectAttributes,
            status: *mut IoStatusBlock,
            allocation: *mut i64,
            file_attributes: u32,
            share: u32,
            disposition: u32,
            options: u32,
            ea: *mut c_void,
            ea_len: u32,
        ) -> i32;
        fn RtlNtStatusToDosError(status: i32) -> u32;
    }

    fn identity(file: &File, directory: bool) -> io::Result<(u64, u64)> {
        let mut data = std::mem::MaybeUninit::<FileInfo>::uninit();
        if unsafe { GetFileInformationByHandle(file.as_raw_handle() as isize, data.as_mut_ptr()) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        let data = unsafe { data.assume_init() };
        if data.attributes & REPARSE != 0 || (data.attributes & DIRECTORY != 0) != directory {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "reparse point or wrong entry kind",
            ));
        }
        // A hardlink may reference a file residing outside the approved namespace.
        if !directory && data.links != 1 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "hardlinked file",
            ));
        }
        Ok((
            data.volume as u64,
            ((data.index_high as u64) << 32) | data.index_low as u64,
        ))
    }
    fn root(path: &Path) -> io::Result<File> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(BACKUP_SEMANTICS | OPEN_REPARSE)
            .open(path)?;
        identity(&file, true)?;
        Ok(file)
    }
    pub(crate) fn root_identity(path: &Path) -> io::Result<(u64, u64)> {
        identity(&root(path)?, true)
    }
    pub(crate) fn document_file_identity(file: &File) -> io::Result<String> {
        let (volume, index) = identity(file, false)?;
        Ok(format!("ntfs:{volume}:{index}"))
    }
    fn child(parent: &File, segment: &str, directory: bool) -> io::Result<File> {
        // NT RootDirectory binds each child to a verified parent HANDLE, not
        // an attacker-swappable absolute string. OBJ_DONT_REPARSE fails closed.
        let mut units: Vec<u16> = segment.encode_utf16().collect();
        let bytes = units
            .len()
            .checked_mul(2)
            .filter(|n| *n <= u16::MAX as usize)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "component too long"))?
            as u16;
        let mut name = UnicodeString {
            length: bytes,
            maximum_length: bytes,
            buffer: units.as_mut_ptr(),
        };
        let mut attrs = ObjectAttributes {
            length: std::mem::size_of::<ObjectAttributes>() as u32,
            root: parent.as_raw_handle() as isize,
            name: &mut name,
            attributes: 0x40 | 0x1000, // OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE
            security_descriptor: ptr::null_mut(),
            quality_of_service: ptr::null_mut(),
        };
        let mut handle = 0isize;
        let mut status = IoStatusBlock {
            status: 0,
            information: 0,
        };
        let options = OPEN_REPARSE | 0x20 | if directory { 0x1 } else { 0x40 };
        let rc = unsafe {
            NtCreateFile(
                &mut handle,
                1 | 0x80 | 0x00100000,
                &mut attrs,
                &mut status,
                ptr::null_mut(),
                0,
                7,
                1,
                options,
                ptr::null_mut(),
                0,
            )
        };
        if rc < 0 {
            return Err(io::Error::from_raw_os_error(
                unsafe { RtlNtStatusToDosError(rc) } as i32,
            ));
        }
        let file = unsafe { File::from_raw_handle(handle as *mut c_void) };
        identity(&file, directory)?;
        Ok(file)
    }
    pub(crate) fn open_document(
        path: &Path,
        expected: (u64, u64),
        segments: &[String],
    ) -> io::Result<File> {
        let mut file = root(path)?;
        if identity(&file, true)? != expected {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "approved root replaced",
            ));
        }
        for (index, segment) in segments.iter().enumerate() {
            file = child(&file, segment, index + 1 < segments.len())?;
        }
        Ok(file)
    }
}
#[cfg(not(windows))]
mod platform {
    use super::*;
    pub(crate) fn root_identity(path: &Path) -> io::Result<(u64, u64)> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let md = path.metadata()?;
            return Ok((md.dev(), md.ino()));
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            Err(io::Error::new(io::ErrorKind::Unsupported, "NTFS only"))
        }
    }
    pub(crate) fn document_file_identity(_: &File) -> io::Result<String> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "NTFS only"))
    }
    pub(crate) fn open_document(_: &Path, _: (u64, u64), _: &[String]) -> io::Result<File> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "NTFS handle containment is Windows-only",
        ))
    }
}
pub(crate) use platform::{document_file_identity, open_document, root_identity};

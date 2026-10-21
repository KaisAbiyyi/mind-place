//! Capability boundary: NTFS handle-relative no-reparse traversal.
use std::{fs::File, io, path::Path};

#[derive(Debug)]
pub(crate) struct DirectoryEntry {
    pub(crate) name: String,
    pub(crate) is_directory: bool,
    pub(crate) is_reparse: bool,
}

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
        fn NtQueryDirectoryFile(
            handle: isize,
            event: isize,
            apc_routine: *mut c_void,
            apc_context: *mut c_void,
            status: *mut IoStatusBlock,
            file_information: *mut c_void,
            length: u32,
            file_information_class: u32,
            return_single_entry: u8,
            file_name: *mut UnicodeString,
            restart_scan: u8,
        ) -> i32;
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

    /// NT handle-bound directory enumeration. No path-based read_dir, even
    /// temporarily: a junction swap could otherwise disclose outside names.
    pub(crate) fn directory_entries(
        path: &Path,
        expected: (u64, u64),
        segments: &[String],
    ) -> io::Result<Vec<DirectoryEntry>> {
        let mut dir = root(path)?;
        if identity(&dir, true)? != expected {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "approved root replaced",
            ));
        }
        for segment in segments {
            dir = child(&dir, segment, true)?;
        }
        let mut names = Vec::new();
        let mut buf = vec![0u8; 64 * 1024];
        let mut restart = 1u8;
        loop {
            let mut status = IoStatusBlock {
                status: 0,
                information: 0,
            };
            let rc = unsafe {
                NtQueryDirectoryFile(
                    dir.as_raw_handle() as isize,
                    0,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    &mut status,
                    buf.as_mut_ptr().cast(),
                    buf.len() as u32,
                    1, // FileDirectoryInformation: 64-byte header + UTF-16 name
                    1,
                    ptr::null_mut(),
                    restart,
                )
            };
            restart = 0;
            if rc as u32 == 0x80000006 {
                // STATUS_NO_MORE_FILES
                break;
            }
            if rc != 0 {
                return Err(io::Error::from_raw_os_error(
                    unsafe { RtlNtStatusToDosError(rc) } as i32,
                ));
            }
            if status.information < 64 || status.information > buf.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "malformed directory record",
                ));
            }
            let attribute = u32::from_le_bytes(buf[56..60].try_into().unwrap());
            let size = u32::from_le_bytes(buf[60..64].try_into().unwrap()) as usize;
            if size == 0 || size % 2 != 0 || 64 + size > status.information {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "malformed directory name",
                ));
            }
            let name = String::from_utf16(
                &buf[64..64 + size]
                    .chunks_exact(2)
                    .map(|unit| u16::from_le_bytes([unit[0], unit[1]]))
                    .collect::<Vec<_>>(),
            )
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid UTF-16 entry"))?;
            if name == "." || name == ".." {
                continue;
            }
            names.push(DirectoryEntry {
                name,
                is_directory: attribute & DIRECTORY != 0,
                is_reparse: attribute & REPARSE != 0,
            });
            if names.len() > 10_000 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "too many directory entries",
                ));
            }
        }
        names.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(names)
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
    pub(crate) fn directory_entries(
        _: &Path,
        _: (u64, u64),
        _: &[String],
    ) -> io::Result<Vec<DirectoryEntry>> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "NTFS only"))
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
pub(crate) use platform::{
    directory_entries, document_file_identity, open_document, root_identity,
};

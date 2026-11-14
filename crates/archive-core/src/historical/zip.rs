//! Strict, read-only zip reader for import sources (MP-IMPORT-APP-01).
//!
//! Only what a provider export needs: stored and deflate members, zip64 sizes and offsets, a
//! single disk. Everything else is a typed error, never a silent skip. Nothing is extracted:
//! members are streamed through [`member_reader`], which enforces the declared size and CRC-32.
//!
//! Safety rules (see the design note `docs/implementation-plan/2026-11-14-mp-import-app-01-design.md`):
//! member names are identifiers only (validated, never joined to a disk path), exact duplicate
//! names are refused, local entries may not overlap each other or the central directory, the
//! declared total and the per-member ratio are bounded, and the central directory must end
//! exactly where the end records begin (no appended data). The members must also tile the file:
//! the first local header is at byte zero, each next one starts where the previous member (plus
//! its data descriptor, if it declares one and the descriptor agrees with the directory) ends,
//! and the last one ends where the central directory begins. Bytes before, between or after the
//! members (a self-extractor stub, a polyglot prefix with rebased offsets, hidden padding) are
//! `E_ZIP_MALFORMED`, so every byte of the archive is either a measured member or a record this
//! reader parsed.

use super::manifest::{check_component, coded, ManifestLimits};
use crate::{ArchiveError, Result};
use flate2::read::DeflateDecoder;
use flate2::Crc;
use std::collections::HashSet;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};

const SIG_LOCAL: u32 = 0x0403_4b50;
const SIG_CENTRAL: u32 = 0x0201_4b50;
const SIG_EOCD: u32 = 0x0605_4b50;
const SIG_ZIP64_EOCD: u32 = 0x0606_4b50;
const SIG_ZIP64_LOCATOR: u32 = 0x0706_4b50;
const SIG_DESCRIPTOR: u32 = 0x0807_4b50;
const EOCD_LEN: u64 = 22;
const MAX_COMMENT: u64 = 0xFFFF;
const LOCAL_LEN: u64 = 30;
const CENTRAL_LEN: usize = 46;
/// Members at most this large are exempt from the ratio rule (tiny files compress oddly).
const RATIO_EXEMPT_BYTES: u64 = 1024 * 1024;
/// Decompressed bytes always allowed on top of `archive length * max ratio`.
const AGGREGATE_ALLOWANCE_BYTES: u64 = 64 * 1024 * 1024;
const METHOD_STORED: u16 = 0;
const METHOD_DEFLATE: u16 = 8;

/// One central directory entry, cross-checked against its local header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ZipEntry {
    pub name: String,
    pub is_dir: bool,
    pub method: u16,
    pub crc32: u32,
    pub compressed: u64,
    pub uncompressed: u64,
    pub local_offset: u64,
    pub data_offset: u64,
}

fn malformed(detail: &str) -> ArchiveError {
    coded("E_ZIP_MALFORMED", detail)
}

fn le16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}
fn le32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn le64(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}

fn read_at(file: &mut File, offset: u64, out: &mut [u8]) -> Result<()> {
    file.seek(SeekFrom::Start(offset))
        .map_err(|_| coded("E_SOURCE_IO", "Import source file cannot be read"))?;
    file.read_exact(out).map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            malformed("Zip archive is truncated")
        } else {
            coded("E_SOURCE_IO", "Import source file cannot be read")
        }
    })
}

/// Reads `n` bytes from a stream that must contain them.
fn take_exact(reader: &mut impl Read, n: usize) -> Result<Vec<u8>> {
    let mut out = vec![0u8; n];
    reader.read_exact(&mut out).map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            malformed("Zip central directory is truncated")
        } else {
            coded("E_SOURCE_IO", "Import source file cannot be read")
        }
    })?;
    Ok(out)
}

/// Validates a member name and returns it without a trailing `/` for directories.
fn check_member_name(raw: &[u8], limits: &ManifestLimits) -> Result<(String, bool)> {
    let name = std::str::from_utf8(raw)
        .map_err(|_| coded("E_SOURCE_NAME", "Zip member name is not UTF-8"))?;
    if name.len() > limits.max_path_bytes {
        return Err(coded("E_SOURCE_LIMIT", "Import source path is too long"));
    }
    let (body, is_dir) = match name.strip_suffix('/') {
        Some(body) => (body, true),
        None => (name, false),
    };
    if body.is_empty() || body.starts_with('/') {
        return Err(coded("E_SOURCE_NAME", "Zip member name is not allowed"));
    }
    let components: Vec<&str> = body.split('/').collect();
    if components.len() > limits.max_depth {
        return Err(coded("E_SOURCE_LIMIT", "Import source nesting is too deep"));
    }
    // A drive prefix (`C:`) would make the name absolute on Windows.
    let first = components[0].as_bytes();
    if first.len() >= 2 && first[1] == b':' && first[0].is_ascii_alphabetic() {
        return Err(coded("E_SOURCE_NAME", "Zip member name is not allowed"));
    }
    for component in components {
        check_component(component)?;
    }
    // Bidirectional controls would let a name display differently from what it is when the
    // owner reviews the candidate members.
    if body.chars().any(|c| {
        matches!(
            c,
            '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
        )
    }) {
        return Err(coded("E_SOURCE_NAME", "Zip member name is not allowed"));
    }
    Ok((body.to_owned(), is_dir))
}

struct EndRecords {
    entries: u64,
    cd_offset: u64,
    cd_size: u64,
    /// Where the central directory must end (the zip64 record, or the classic end record).
    cd_end: u64,
}

fn read_end_records(file: &mut File, file_len: u64) -> Result<EndRecords> {
    if file_len < EOCD_LEN {
        return Err(malformed("Not a zip archive"));
    }
    let tail_len = file_len.min(EOCD_LEN + MAX_COMMENT);
    let tail_start = file_len - tail_len;
    let mut tail = vec![0u8; tail_len as usize];
    read_at(file, tail_start, &mut tail)?;
    // The end record must end exactly at the end of the file (its comment fills the rest).
    let mut found = None;
    for at in (0..=tail.len() - EOCD_LEN as usize).rev() {
        if le32(&tail, at) == SIG_EOCD
            && at + EOCD_LEN as usize + le16(&tail, at + 20) as usize == tail.len()
        {
            found = Some(at);
            break;
        }
    }
    let at = found.ok_or_else(|| malformed("Not a zip archive"))?;
    let eocd_pos = tail_start + at as u64;
    let rec = &tail[at..at + EOCD_LEN as usize];
    let disk = le16(rec, 4);
    let cd_disk = le16(rec, 6);
    let disk_entries = le16(rec, 8);
    let entries = le16(rec, 10);
    let cd_size = le32(rec, 12);
    let cd_offset = le32(rec, 16);

    let needs_zip64 = entries == 0xFFFF || cd_size == 0xFFFF_FFFF || cd_offset == 0xFFFF_FFFF;
    let mut locator = [0u8; 20];
    let has_locator = eocd_pos >= 20 && {
        read_at(file, eocd_pos - 20, &mut locator)?;
        le32(&locator, 0) == SIG_ZIP64_LOCATOR
    };
    if !has_locator {
        if needs_zip64 {
            return Err(malformed("Zip64 end record is missing"));
        }
        if disk != 0 || cd_disk != 0 || disk_entries != entries {
            return Err(coded(
                "E_ZIP_MULTIDISK",
                "Split or multi-disk zip archives are not supported",
            ));
        }
        return Ok(EndRecords {
            entries: entries as u64,
            cd_offset: cd_offset as u64,
            cd_size: cd_size as u64,
            cd_end: eocd_pos,
        });
    }
    let locator_pos = eocd_pos - 20;
    if le32(&locator, 4) != 0 || le32(&locator, 16) != 1 {
        return Err(coded(
            "E_ZIP_MULTIDISK",
            "Split or multi-disk zip archives are not supported",
        ));
    }
    let z64_pos = le64(&locator, 8);
    if z64_pos.checked_add(56).is_none_or(|end| end > locator_pos) {
        return Err(malformed("Zip64 end record is out of range"));
    }
    let mut z64 = [0u8; 56];
    read_at(file, z64_pos, &mut z64)?;
    if le32(&z64, 0) != SIG_ZIP64_EOCD {
        return Err(malformed("Zip64 end record is missing"));
    }
    // The record (plus any extensible data) must end exactly at the locator.
    if z64_pos
        .checked_add(12)
        .and_then(|v| v.checked_add(le64(&z64, 4)))
        != Some(locator_pos)
    {
        return Err(malformed("Zip64 end record is inconsistent"));
    }
    let z_disk = le32(&z64, 16);
    let z_cd_disk = le32(&z64, 20);
    let z_disk_entries = le64(&z64, 24);
    let z_entries = le64(&z64, 32);
    if z_disk != 0 || z_cd_disk != 0 || z_disk_entries != z_entries {
        return Err(coded(
            "E_ZIP_MULTIDISK",
            "Split or multi-disk zip archives are not supported",
        ));
    }
    // Classic fields that are not saturated must agree with the zip64 record.
    let z_cd_size = le64(&z64, 40);
    let z_cd_offset = le64(&z64, 48);
    if (entries != 0xFFFF && entries as u64 != z_entries)
        || (cd_size != 0xFFFF_FFFF && cd_size as u64 != z_cd_size)
        || (cd_offset != 0xFFFF_FFFF && cd_offset as u64 != z_cd_offset)
    {
        return Err(malformed("Zip64 end record is inconsistent"));
    }
    Ok(EndRecords {
        entries: z_entries,
        cd_offset: z_cd_offset,
        cd_size: z_cd_size,
        cd_end: z64_pos,
    })
}

/// Parses and cross-checks the whole directory. Returns every entry (directories included),
/// sorted by local header offset.
pub(crate) fn read_directory(
    file: &mut File,
    file_len: u64,
    limits: &ManifestLimits,
) -> Result<Vec<ZipEntry>> {
    let end = read_end_records(file, file_len)?;
    if end.cd_offset.checked_add(end.cd_size) != Some(end.cd_end) {
        return Err(malformed(
            "Zip central directory is not where the end record says",
        ));
    }
    if end.entries > limits.max_entries as u64 {
        return Err(coded(
            "E_SOURCE_LIMIT",
            "Import source has too many files or folders",
        ));
    }
    file.seek(SeekFrom::Start(end.cd_offset))
        .map_err(|_| coded("E_SOURCE_IO", "Import source file cannot be read"))?;
    let mut cd = BufReader::new(Read::by_ref(file).take(end.cd_size));
    let mut entries = Vec::with_capacity(end.entries.min(4096) as usize);
    let mut names = HashSet::new();
    let mut declared_total = 0u64;
    let mut consumed = 0u64;
    for _ in 0..end.entries {
        let fixed = take_exact(&mut cd, CENTRAL_LEN)?;
        if le32(&fixed, 0) != SIG_CENTRAL {
            return Err(malformed("Zip central directory entry is invalid"));
        }
        let flags = le16(&fixed, 8);
        let method = le16(&fixed, 10);
        let crc32 = le32(&fixed, 16);
        let mut compressed = le32(&fixed, 20) as u64;
        let mut uncompressed = le32(&fixed, 24) as u64;
        let name_len = le16(&fixed, 28) as usize;
        let extra_len = le16(&fixed, 30) as usize;
        let comment_len = le16(&fixed, 32) as usize;
        let disk_start = le16(&fixed, 34);
        let mut local_offset = le32(&fixed, 42) as u64;
        consumed += (CENTRAL_LEN + name_len + extra_len + comment_len) as u64;
        if name_len > limits.max_path_bytes + 1 {
            return Err(coded("E_SOURCE_LIMIT", "Import source path is too long"));
        }
        let raw_name = take_exact(&mut cd, name_len)?;
        let extra = take_exact(&mut cd, extra_len)?;
        let _comment = take_exact(&mut cd, comment_len)?;

        // Zip64 extended information: present only for the saturated fields, in this order.
        let mut need = [
            uncompressed == 0xFFFF_FFFF,
            compressed == 0xFFFF_FFFF,
            local_offset == 0xFFFF_FFFF,
            disk_start == 0xFFFF,
        ];
        let mut disk = disk_start as u32;
        let mut at = 0usize;
        while at + 4 <= extra.len() {
            let id = le16(&extra, at);
            let size = le16(&extra, at + 2) as usize;
            let body = extra
                .get(at + 4..at + 4 + size)
                .ok_or_else(|| malformed("Zip extra field is truncated"))?;
            if id == 0x0001 {
                let mut pos = 0usize;
                let next64 = |pos: &mut usize| -> Result<u64> {
                    let value = body
                        .get(*pos..*pos + 8)
                        .map(|b| le64(b, 0))
                        .ok_or_else(|| malformed("Zip64 extra field is truncated"))?;
                    *pos += 8;
                    Ok(value)
                };
                if need[0] {
                    uncompressed = next64(&mut pos)?;
                    need[0] = false;
                }
                if need[1] {
                    compressed = next64(&mut pos)?;
                    need[1] = false;
                }
                if need[2] {
                    local_offset = next64(&mut pos)?;
                    need[2] = false;
                }
                if need[3] {
                    disk = body
                        .get(pos..pos + 4)
                        .map(|b| le32(b, 0))
                        .ok_or_else(|| malformed("Zip64 extra field is truncated"))?;
                    need[3] = false;
                }
            }
            at += 4 + size;
        }
        if need.iter().any(|missing| *missing) {
            return Err(malformed("Zip64 extra field is missing"));
        }
        if disk != 0 {
            return Err(coded(
                "E_ZIP_MULTIDISK",
                "Split or multi-disk zip archives are not supported",
            ));
        }
        // Bit 0: encrypted, bit 6: strong encryption, bit 13: encrypted central directory.
        if flags & (1 | 1 << 6 | 1 << 13) != 0 {
            return Err(coded(
                "E_ZIP_ENCRYPTED",
                "Encrypted zip archives are not supported",
            ));
        }
        if method != METHOD_STORED && method != METHOD_DEFLATE {
            return Err(coded(
                "E_ZIP_METHOD",
                "Zip compression method is not supported",
            ));
        }
        let (name, is_dir) = check_member_name(&raw_name, limits)?;
        if is_dir && uncompressed != 0 {
            return Err(malformed("Zip folder entry has content"));
        }
        if method == METHOD_STORED && compressed != uncompressed {
            return Err(coded("E_ZIP_CORRUPT", "Zip member sizes disagree"));
        }
        if uncompressed > RATIO_EXEMPT_BYTES
            && uncompressed > compressed.saturating_mul(limits.max_zip_ratio)
        {
            return Err(coded(
                "E_ZIP_BOMB",
                "Zip member expands far more than an export would",
            ));
        }
        declared_total = declared_total.saturating_add(uncompressed);
        if declared_total > limits.max_total_bytes {
            return Err(coded("E_SOURCE_LIMIT", "Import source is too large"));
        }
        // Many small members are exempt from the per-member rule; the archive as a whole may
        // still not expand beyond the same ratio (plus a small allowance).
        if declared_total
            > file_len
                .saturating_mul(limits.max_zip_ratio)
                .saturating_add(AGGREGATE_ALLOWANCE_BYTES)
        {
            return Err(coded(
                "E_ZIP_BOMB",
                "Zip archive expands far more than an export would",
            ));
        }
        if !is_dir && !names.insert(name.clone()) {
            return Err(coded(
                "E_ZIP_DUPLICATE",
                "Zip archive lists the same member twice",
            ));
        }
        entries.push(ZipEntry {
            name,
            is_dir,
            method,
            crc32,
            compressed,
            uncompressed,
            local_offset,
            data_offset: 0,
        });
    }
    if consumed != end.cd_size {
        return Err(malformed("Zip central directory size is inconsistent"));
    }
    drop(cd);

    entries.sort_by_key(|entry| entry.local_offset);
    // Members tile the file exactly: see the module documentation.
    let mut expected = 0u64;
    for entry in &mut entries {
        if entry.local_offset < expected {
            return Err(coded("E_ZIP_BOMB", "Zip members overlap"));
        }
        if entry.local_offset > expected {
            return Err(malformed("Zip archive has bytes outside its members"));
        }
        let (data_offset, has_descriptor) = local_data_offset(file, entry)?;
        entry.data_offset = data_offset;
        let data_end = entry
            .data_offset
            .checked_add(entry.compressed)
            .ok_or_else(|| malformed("Zip member is out of range"))?;
        if data_end > end.cd_offset {
            return Err(coded(
                "E_ZIP_BOMB",
                "Zip member overlaps the central directory",
            ));
        }
        expected = data_end;
        if has_descriptor {
            expected += descriptor_len(file, entry, data_end, end.cd_offset)?;
        }
    }
    if expected != end.cd_offset {
        return Err(malformed("Zip archive has bytes outside its members"));
    }
    Ok(entries)
}

/// Length of the data descriptor that follows a member whose local header sets bit 3. Its CRC-32
/// and sizes (32 or 64 bit, with or without the optional signature) must equal the central
/// directory's, so it cannot carry anything else.
fn descriptor_len(file: &mut File, entry: &ZipEntry, at: u64, limit: u64) -> Result<u64> {
    let mut buffer = [0u8; 24];
    let available = limit.saturating_sub(at).min(buffer.len() as u64) as usize;
    read_at(file, at, &mut buffer[..available])?;
    let bytes = &buffer[..available];
    let signed = available >= 4 && le32(bytes, 0) == SIG_DESCRIPTOR;
    let skips: &[usize] = if signed { &[4, 0] } else { &[0] };
    for &skip in skips {
        for wide in [false, true] {
            let len = skip + 4 + if wide { 16 } else { 8 };
            if available < len || le32(bytes, skip) != entry.crc32 {
                continue;
            }
            let (compressed, uncompressed) = if wide {
                (le64(bytes, skip + 4), le64(bytes, skip + 12))
            } else {
                (le32(bytes, skip + 4) as u64, le32(bytes, skip + 8) as u64)
            };
            if compressed == entry.compressed && uncompressed == entry.uncompressed {
                return Ok(len as u64);
            }
        }
    }
    Err(coded(
        "E_ZIP_CORRUPT",
        "Zip data descriptor disagrees with the directory",
    ))
}

/// Reads the local header of `entry`, requires it to agree with the central directory, and
/// returns where the member data starts and whether a data descriptor follows the data.
pub(crate) fn local_data_offset(file: &mut File, entry: &ZipEntry) -> Result<(u64, bool)> {
    let mut header = [0u8; LOCAL_LEN as usize];
    read_at(file, entry.local_offset, &mut header)?;
    if le32(&header, 0) != SIG_LOCAL {
        return Err(malformed("Zip local header is invalid"));
    }
    let flags = le16(&header, 6);
    let method = le16(&header, 8);
    let name_len = le16(&header, 26) as u64;
    let extra_len = le16(&header, 28) as u64;
    if flags & 1 != 0 || method != entry.method {
        return Err(coded(
            "E_ZIP_CORRUPT",
            "Zip local header disagrees with the directory",
        ));
    }
    let mut raw_name = vec![0u8; name_len as usize];
    read_at(file, entry.local_offset + LOCAL_LEN, &mut raw_name)?;
    let local_name = std::str::from_utf8(&raw_name).unwrap_or_default();
    let expected = if entry.is_dir {
        format!("{}/", entry.name)
    } else {
        entry.name.clone()
    };
    if local_name != expected {
        return Err(coded(
            "E_ZIP_CORRUPT",
            "Zip local header disagrees with the directory",
        ));
    }
    Ok((
        entry.local_offset + LOCAL_LEN + name_len + extra_len,
        flags & (1 << 3) != 0,
    ))
}

/// Streams one member's decompressed bytes. Errors while reading are `io::Error`s that wrap a
/// coded [`ArchiveError`] (see `archive_error_from_io`).
pub(crate) fn member_reader(mut file: File, entry: &ZipEntry) -> Result<Box<dyn Read>> {
    file.seek(SeekFrom::Start(entry.data_offset))
        .map_err(|_| coded("E_SOURCE_IO", "Import source file cannot be read"))?;
    let raw = file.take(entry.compressed);
    let inner: Box<dyn Read> = match entry.method {
        METHOD_STORED => Box::new(raw),
        METHOD_DEFLATE => Box::new(DeflateDecoder::new(raw)),
        _ => {
            return Err(coded(
                "E_ZIP_METHOD",
                "Zip compression method is not supported",
            ))
        }
    };
    Ok(Box::new(CheckedMember {
        inner,
        remaining: entry.uncompressed,
        crc: Crc::new(),
        expected_crc: entry.crc32,
        finished: false,
        failed: false,
    }))
}

fn corrupt(detail: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, coded("E_ZIP_CORRUPT", detail))
}

/// Enforces the declared decompressed size exactly and the CRC-32 at the end of the member.
struct CheckedMember {
    inner: Box<dyn Read>,
    remaining: u64,
    crc: Crc,
    expected_crc: u32,
    finished: bool,
    /// Sticky: once a read failed, every later read fails too (never a late `Ok(0)`).
    failed: bool,
}

impl Read for CheckedMember {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.failed {
            return Err(corrupt("Zip member data is corrupt"));
        }
        let result = self.read_checked(out);
        if matches!(&result, Err(error) if error.kind() != io::ErrorKind::Interrupted) {
            self.failed = true;
        }
        result
    }
}

impl CheckedMember {
    fn read_checked(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.finished || out.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            // Declared size reached: the stream must have nothing more.
            let mut probe = [0u8; 1];
            let extra = loop {
                match self.inner.read(&mut probe) {
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    other => break other,
                }
            };
            return match extra {
                Ok(0) => {
                    if self.crc.sum() != self.expected_crc {
                        return Err(corrupt("Zip member checksum does not match"));
                    }
                    self.finished = true;
                    Ok(0)
                }
                Ok(_) => Err(corrupt("Zip member is larger than declared")),
                Err(error) => Err(wrap_inflate_error(error)),
            };
        }
        let want = out
            .len()
            .min(self.remaining.min(usize::MAX as u64) as usize);
        let read = self
            .inner
            .read(&mut out[..want])
            .map_err(wrap_inflate_error)?;
        if read == 0 {
            return Err(corrupt("Zip member is smaller than declared"));
        }
        self.crc.update(&out[..read]);
        self.remaining -= read as u64;
        Ok(read)
    }
}

fn wrap_inflate_error(error: io::Error) -> io::Error {
    match error.kind() {
        io::ErrorKind::InvalidData | io::ErrorKind::InvalidInput | io::ErrorKind::UnexpectedEof => {
            corrupt("Zip member data is corrupt")
        }
        _ => error,
    }
}

/// Recovers the coded error carried by an `io::Error` from [`member_reader`]; any other I/O
/// failure becomes `E_SOURCE_IO`.
pub(crate) fn archive_error_from_io(error: &io::Error) -> ArchiveError {
    if let Some(ArchiveError::Invalid(message)) = error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<ArchiveError>())
    {
        return ArchiveError::Invalid(message.clone());
    }
    coded("E_SOURCE_IO", "Import source file cannot be read")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_member_read_error_is_sticky() {
        // Declares 4 bytes but the stream carries 6: the overrun is an error, and so is every
        // later read (a caller that keeps reading must never see a clean end of file).
        let mut member = CheckedMember {
            inner: Box::new(io::Cursor::new(b"abcdef".to_vec())),
            remaining: 4,
            crc: Crc::new(),
            expected_crc: 0,
            finished: false,
            failed: false,
        };
        let mut buffer = [0u8; 16];
        assert_eq!(member.read(&mut buffer).unwrap(), 4);
        assert!(member.read(&mut buffer).is_err());
        for _ in 0..3 {
            assert!(member.read(&mut buffer).is_err());
        }
    }
}

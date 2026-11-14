//! Independent, deterministic measurement of an import source folder (MP-IMPORT-RAW-01, P1-2).
//!
//! The manifest lists every regular file under the source root with its size and SHA-256 and
//! reduces the list to one domain-separated hash. The fields are private: a manifest can only be
//! produced by [`SourceManifest::measure`] reading the folder, never assembled from a caller's
//! claim. The owner flow measures first, pins `sha256()` in the security capability, and the
//! import re-measures and compares, so a swapped file cannot ride on a hash the caller supplied.
//!
//! A source is a folder or a single zip file (MP-IMPORT-APP-01). For a zip the entries are its
//! members: decompressed size and the SHA-256 of the decompressed bytes, so raw-evidence offsets
//! mean the same thing for both kinds. The zip manifest hash also binds the archive length and
//! the SHA-256 of the archive bytes.
//!
//! Errors carry stable codes (`E_SOURCE_*`, `E_ZIP_*`) and never include file content.

use super::zip::{self, ZipEntry};
use crate::{ArchiveError, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Domain separator of the manifest hash.
pub const MANIFEST_DOMAIN: &[u8] = b"mind-place/import-source-manifest/v1\0";
/// Domain separator of the manifest hash of a zip source.
pub const ZIP_MANIFEST_DOMAIN: &[u8] = b"mind-place/import-source-manifest/zip/v1\0";

pub(crate) fn coded(code: &str, detail: &str) -> ArchiveError {
    ArchiveError::Invalid(format!("{code}: {detail}"))
}

/// Hard limits applied while the source folder is walked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ManifestLimits {
    /// Most directory entries (files and directories) visited.
    pub max_entries: usize,
    pub max_depth: usize,
    pub max_path_bytes: usize,
    /// Most bytes hashed across all files (for a zip: decompressed bytes).
    pub max_total_bytes: u64,
    /// Most decompressed bytes per compressed byte for a zip member larger than 1 MiB.
    pub max_zip_ratio: u64,
}

impl Default for ManifestLimits {
    fn default() -> Self {
        Self {
            max_entries: 100_000,
            max_depth: 32,
            max_path_bytes: 512,
            max_total_bytes: 64 * 1024 * 1024 * 1024,
            max_zip_ratio: 200,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEntry {
    /// Relative path, `/`-separated, UTF-8, no `.`/`..`/empty components.
    pub path: String,
    pub size: u64,
    /// Lowercase hex SHA-256 of the file content.
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ZipSource {
    archive_bytes: u64,
    archive_modified: Option<SystemTime>,
    archive_sha256: [u8; 32],
    /// File members, sorted by name (the same order as `entries`).
    members: Vec<ZipEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SourceKind {
    Folder,
    Zip(ZipSource),
}

/// Polled while a source is measured: between entries and about once per MiB read or
/// decompressed. An `Err` stops the measurement and is returned unchanged, so a caller can stop
/// a long scan (cancel, lock, deadline) instead of waiting for it to finish.
pub type Interrupt<'a> = &'a dyn Fn() -> Result<()>;

/// How many bytes are hashed between two [`Interrupt`] polls.
const INTERRUPT_EVERY_BYTES: u64 = 1024 * 1024;

fn never_interrupt() -> Result<()> {
    Ok(())
}

/// A measured source folder or zip file. Constructed only by [`SourceManifest::measure`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceManifest {
    root: PathBuf,
    kind: SourceKind,
    entries: Vec<ManifestEntry>,
    sha256: [u8; 32],
    total_bytes: u64,
}

impl SourceManifest {
    /// Measures a source folder (walks it and hashes every regular file) or a zip file (hashes
    /// the archive and the decompressed bytes of every member). Symlinks, reparse points,
    /// devices, non-UTF-8 or unsafe names, unsupported zip features and anything beyond
    /// `limits` are typed errors, not skipped.
    pub fn measure(source_root: &Path, limits: ManifestLimits) -> Result<Self> {
        Self::measure_interruptible(source_root, limits, &never_interrupt)
    }

    /// [`Self::measure`] that polls `interrupt` between entries and about once per MiB of
    /// content, so a long scan of a large folder or archive can be stopped.
    pub fn measure_interruptible(
        source_root: &Path,
        limits: ManifestLimits,
        interrupt: Interrupt<'_>,
    ) -> Result<Self> {
        mind_place_security::vault_import_auth::validate_import_source(source_root).map_err(
            |_| {
                coded(
                    "E_SOURCE_ROOT",
                    "Import source is not a safe folder or file",
                )
            },
        )?;
        let meta = fs::symlink_metadata(source_root)
            .map_err(|_| coded("E_SOURCE_ROOT", "Import source cannot be inspected"))?;
        if meta.is_file() {
            Self::measure_zip(source_root, limits, true, interrupt)
        } else {
            Self::walk(source_root, limits, true, interrupt)
        }
    }

    /// Re-reads the source without hashing member content and requires the same set of
    /// entries and sizes (for a zip also the same archive length and modification time).
    /// Cheaper than [`Self::verify_unchanged`]; content binding comes from hashing the bytes
    /// while they are parsed.
    pub fn verify_layout_unchanged(&self, limits: ManifestLimits) -> Result<()> {
        let again = match &self.kind {
            SourceKind::Folder => Self::walk(&self.root, limits, false, &never_interrupt)?,
            SourceKind::Zip(_) => Self::measure_zip(&self.root, limits, false, &never_interrupt)?,
        };
        let same_archive = match (&self.kind, &again.kind) {
            (SourceKind::Folder, SourceKind::Folder) => true,
            (SourceKind::Zip(a), SourceKind::Zip(b)) => {
                a.archive_bytes == b.archive_bytes && a.archive_modified == b.archive_modified
            }
            _ => false,
        };
        let same = same_archive
            && again.entries.len() == self.entries.len()
            && again
                .entries
                .iter()
                .zip(&self.entries)
                .all(|(a, b)| a.path == b.path && a.size == b.size);
        if !same {
            return Err(coded(
                "E_SOURCE_CHANGED",
                "Import source changed since it was measured",
            ));
        }
        Ok(())
    }

    fn walk(
        source_root: &Path,
        limits: ManifestLimits,
        hash_files: bool,
        interrupt: Interrupt<'_>,
    ) -> Result<Self> {
        mind_place_security::vault_import_auth::validate_source_directory(source_root).map_err(
            |_| {
                coded(
                    "E_SOURCE_ROOT",
                    "Import source folder is not a safe directory",
                )
            },
        )?;
        let root = fs::canonicalize(source_root)
            .map_err(|_| coded("E_SOURCE_ROOT", "Import source folder cannot be resolved"))?;
        if !fs::metadata(&root).is_ok_and(|meta| meta.is_dir()) {
            return Err(coded(
                "E_SOURCE_ROOT",
                "Import source folder is not a safe directory",
            ));
        }

        let mut entries = Vec::new();
        let mut visited = 0usize;
        let mut total_bytes = 0u64;
        let mut pending: Vec<(PathBuf, Vec<String>)> = vec![(root.clone(), Vec::new())];

        while let Some((dir, prefix)) = pending.pop() {
            if prefix.len() >= limits.max_depth {
                return Err(coded("E_SOURCE_LIMIT", "Import source nesting is too deep"));
            }
            let listing = fs::read_dir(&dir)
                .map_err(|_| coded("E_SOURCE_IO", "Import source folder cannot be listed"))?;
            for entry in listing {
                let entry = entry
                    .map_err(|_| coded("E_SOURCE_IO", "Import source folder cannot be listed"))?;
                interrupt()?;
                visited += 1;
                if visited > limits.max_entries {
                    return Err(coded(
                        "E_SOURCE_LIMIT",
                        "Import source has too many files or folders",
                    ));
                }
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| coded("E_SOURCE_NAME", "Import source name is not UTF-8"))?;
                check_component(&name)?;
                let path = entry.path();
                let meta = fs::symlink_metadata(&path)
                    .map_err(|_| coded("E_SOURCE_IO", "Import source entry cannot be inspected"))?;
                if is_link(&meta) {
                    return Err(coded(
                        "E_SOURCE_LINK",
                        "Import source contains a link or reparse point",
                    ));
                }
                let mut rel = prefix.clone();
                rel.push(name);
                if meta.is_dir() {
                    pending.push((path, rel));
                } else if meta.is_file() {
                    let joined = rel.join("/");
                    if joined.len() > limits.max_path_bytes {
                        return Err(coded("E_SOURCE_LIMIT", "Import source path is too long"));
                    }
                    if total_bytes.saturating_add(meta.len()) > limits.max_total_bytes {
                        return Err(coded("E_SOURCE_LIMIT", "Import source is too large"));
                    }
                    let (size, sha256) = if hash_files {
                        let (size, digest) = hash_regular_file(&path, &meta, interrupt)?;
                        (size, hex(&digest))
                    } else {
                        (meta.len(), String::new())
                    };
                    total_bytes += size;
                    entries.push(ManifestEntry {
                        path: joined,
                        size,
                        sha256,
                    });
                } else {
                    return Err(coded(
                        "E_SOURCE_SPECIAL",
                        "Import source contains a non-regular file",
                    ));
                }
            }
        }
        entries.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
        let sha256 = manifest_digest(&entries);
        Ok(Self {
            root,
            kind: SourceKind::Folder,
            entries,
            sha256,
            total_bytes,
        })
    }

    /// Measures a zip file. With `hash`, every member is decompressed and hashed; without it
    /// only the directory is read (layout re-checks).
    fn measure_zip(
        source: &Path,
        limits: ManifestLimits,
        hash: bool,
        interrupt: Interrupt<'_>,
    ) -> Result<Self> {
        let root = fs::canonicalize(source)
            .map_err(|_| coded("E_SOURCE_ROOT", "Import source file cannot be resolved"))?;
        let (mut file, before) = open_regular_no_follow(&root)?;
        let archive_sha256 = if hash {
            hash_open_file(&mut file, &before, interrupt)?.1
        } else {
            [0u8; 32]
        };
        let directory = zip::read_directory(&mut file, before.len(), &limits)?;
        let mut members: Vec<ZipEntry> = directory.into_iter().filter(|e| !e.is_dir).collect();
        members.sort_by(|a, b| a.name.as_bytes().cmp(b.name.as_bytes()));
        let mut entries = Vec::with_capacity(members.len());
        let mut total_bytes = 0u64;
        for member in &members {
            interrupt()?;
            let sha256 = if hash {
                let handle = file
                    .try_clone()
                    .map_err(|_| coded("E_SOURCE_IO", "Import source file cannot be read"))?;
                let reader = HashingReader::new(zip::member_reader(handle, member)?);
                let (bytes, digest) = reader
                    .finish_to_eof_with(interrupt)
                    .map_err(|error| zip::archive_error_from_io(&error))?;
                if bytes != member.uncompressed {
                    return Err(coded("E_ZIP_CORRUPT", "Zip member sizes disagree"));
                }
                hex(&digest)
            } else {
                String::new()
            };
            total_bytes += member.uncompressed;
            entries.push(ManifestEntry {
                path: member.name.clone(),
                size: member.uncompressed,
                sha256,
            });
        }
        let after = file
            .metadata()
            .map_err(|_| coded("E_SOURCE_IO", "Import source file cannot be inspected"))?;
        if after.len() != before.len() || after.modified().ok() != before.modified().ok() {
            return Err(coded(
                "E_SOURCE_CHANGING",
                "Import source file changed while it was read",
            ));
        }
        let sha256 = zip_manifest_digest(before.len(), &archive_sha256, &entries);
        Ok(Self {
            root,
            kind: SourceKind::Zip(ZipSource {
                archive_bytes: before.len(),
                archive_modified: before.modified().ok(),
                archive_sha256,
                members,
            }),
            entries,
            sha256,
            total_bytes,
        })
    }

    /// Opens one measured entry for streaming: the file itself for a folder, the decompressed
    /// member (size and CRC-32 enforced) for a zip. The caller still compares the bytes it reads
    /// with the entry hash.
    pub(crate) fn open_member(&self, path: &str) -> Result<Box<dyn Read>> {
        let entry = self
            .entry(path)
            .ok_or_else(|| coded("E_MEMBER_UNKNOWN", "An import member is not measured"))?;
        match &self.kind {
            SourceKind::Folder => {
                let (file, meta) = open_regular_no_follow(&self.root.join(path))?;
                if meta.len() != entry.size {
                    return Err(coded(
                        "E_SOURCE_MISMATCH",
                        "An import member differs from its measured size",
                    ));
                }
                Ok(Box::new(file))
            }
            SourceKind::Zip(source) => {
                let index = source
                    .members
                    .binary_search_by(|m| m.name.as_bytes().cmp(path.as_bytes()))
                    .map_err(|_| coded("E_MEMBER_UNKNOWN", "An import member is not measured"))?;
                let member = &source.members[index];
                let (mut file, meta) = open_regular_no_follow(&self.root)?;
                if meta.len() != source.archive_bytes {
                    return Err(coded(
                        "E_SOURCE_MISMATCH",
                        "The import archive differs from its measured size",
                    ));
                }
                if zip::local_data_offset(&mut file, member)?.0 != member.data_offset {
                    return Err(coded(
                        "E_SOURCE_MISMATCH",
                        "The import archive differs from what was measured",
                    ));
                }
                zip::member_reader(file, member)
            }
        }
    }

    /// `"folder"` or `"zip"`.
    pub fn source_kind(&self) -> &'static str {
        match self.kind {
            SourceKind::Folder => "folder",
            SourceKind::Zip(_) => "zip",
        }
    }

    /// Bytes of the source as stored: the zip file length, or the folder's total.
    pub fn stored_bytes(&self) -> u64 {
        match &self.kind {
            SourceKind::Folder => self.total_bytes,
            SourceKind::Zip(source) => source.archive_bytes,
        }
    }

    /// Lowercase hex SHA-256 of the zip archive bytes (`None` for a folder).
    pub fn archive_sha256_hex(&self) -> Option<String> {
        match &self.kind {
            SourceKind::Folder => None,
            SourceKind::Zip(source) => Some(hex(&source.archive_sha256)),
        }
    }

    /// Entries that match the documented export layout of `provider` (unverified against real
    /// exports; see the MP-IMPORT-APP-01 design note, section 3).
    pub fn candidate_members(&self, provider: &str) -> Vec<&ManifestEntry> {
        self.entries
            .iter()
            .filter(|entry| entry_matches_provider_layout(&entry.path, provider))
            .collect()
    }

    /// Measures again and requires the same hash: the folder is unchanged since this manifest.
    pub fn verify_unchanged(&self, limits: ManifestLimits) -> Result<()> {
        let again = Self::measure(&self.root, limits)?;
        if again.sha256 != self.sha256 {
            return Err(coded(
                "E_SOURCE_CHANGED",
                "Import source changed since it was measured",
            ));
        }
        Ok(())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn entries(&self) -> &[ManifestEntry] {
        &self.entries
    }
    pub fn entry(&self, path: &str) -> Option<&ManifestEntry> {
        self.entries
            .binary_search_by(|entry| entry.path.as_bytes().cmp(path.as_bytes()))
            .ok()
            .map(|index| &self.entries[index])
    }
    pub fn sha256(&self) -> &[u8; 32] {
        &self.sha256
    }
    pub fn sha256_hex(&self) -> String {
        hex(&self.sha256)
    }
    pub fn total_files(&self) -> usize {
        self.entries.len()
    }
    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }
}

/// ChatGPT: `conversations.json` or `conversations-*.json` at any depth. Gemini: an `.html` file
/// with a path component that names Gemini (case-insensitive), at any depth.
pub fn entry_matches_provider_layout(path: &str, provider: &str) -> bool {
    let file_name = path.rsplit('/').next().unwrap_or(path);
    let lower = file_name.to_ascii_lowercase();
    match provider {
        "chatgpt" => {
            lower == "conversations.json"
                || (lower.starts_with("conversations-") && lower.ends_with(".json"))
        }
        "gemini" => {
            lower.ends_with(".html")
                && path
                    .split('/')
                    .any(|component| component.to_lowercase().contains("gemini"))
        }
        _ => false,
    }
}

/// The file extension a member of `provider` must have when the owner picks it explicitly.
pub fn member_extension_allowed(path: &str, provider: &str) -> bool {
    let lower = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    match provider {
        "chatgpt" => lower.ends_with(".json"),
        "gemini" => lower.ends_with(".html") || lower.ends_with(".htm"),
        _ => false,
    }
}

pub(crate) fn check_component(name: &str) -> Result<()> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.len() > 255
        || name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\'))
    {
        return Err(coded("E_SOURCE_NAME", "Import source name is not allowed"));
    }
    Ok(())
}

fn is_link(meta: &fs::Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    false
}

/// Opens a regular file without following a link and proves the handle is the file that was
/// inspected (`lstat`). Returns the handle and its metadata at open time.
pub(crate) fn open_regular_no_follow(path: &Path) -> Result<(File, fs::Metadata)> {
    let inspected = fs::symlink_metadata(path)
        .map_err(|_| coded("E_SOURCE_IO", "Import source file cannot be inspected"))?;
    if is_link(&inspected) {
        return Err(coded(
            "E_SOURCE_LINK",
            "Import source contains a link or reparse point",
        ));
    }
    if !inspected.is_file() {
        return Err(coded(
            "E_SOURCE_SPECIAL",
            "Import source contains a non-regular file",
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_OPEN_REPARSE_POINT: never follow a reparse point swapped in after the check.
        options.custom_flags(0x0020_0000);
    }
    let file: File = options
        .open(path)
        .map_err(|_| coded("E_SOURCE_IO", "Import source file cannot be opened"))?;
    let before = file
        .metadata()
        .map_err(|_| coded("E_SOURCE_IO", "Import source file cannot be inspected"))?;
    if !before.is_file() || is_link(&before) {
        return Err(coded(
            "E_SOURCE_LINK",
            "Import source file changed type while it was read",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before.dev() != inspected.dev() || before.ino() != inspected.ino() {
            return Err(coded(
                "E_SOURCE_CHANGING",
                "Import source file was replaced while it was read",
            ));
        }
    }
    if before.len() != inspected.len() {
        return Err(coded(
            "E_SOURCE_CHANGING",
            "Import source file changed while it was read",
        ));
    }
    Ok((file, before))
}

/// Hashes a regular file in fixed chunks. The size and modification time must not change while
/// it is read.
fn hash_regular_file(
    path: &Path,
    _inspected: &fs::Metadata,
    interrupt: Interrupt<'_>,
) -> Result<(u64, [u8; 32])> {
    let (mut file, before) = open_regular_no_follow(path)?;
    hash_open_file(&mut file, &before, interrupt)
}

fn hash_open_file(
    file: &mut File,
    before: &fs::Metadata,
    interrupt: Interrupt<'_>,
) -> Result<(u64, [u8; 32])> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut read_total = 0u64;
    let mut since_poll = 0u64;
    loop {
        let read = match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(coded("E_SOURCE_IO", "Import source file cannot be read")),
        };
        hasher.update(&buffer[..read]);
        read_total += read as u64;
        since_poll += read as u64;
        if since_poll >= INTERRUPT_EVERY_BYTES {
            since_poll = 0;
            interrupt()?;
        }
        if read_total > before.len() {
            return Err(coded(
                "E_SOURCE_CHANGING",
                "Import source file grew while it was read",
            ));
        }
    }
    let after = file
        .metadata()
        .map_err(|_| coded("E_SOURCE_IO", "Import source file cannot be inspected"))?;
    if read_total != before.len()
        || after.len() != before.len()
        || after.modified().ok() != before.modified().ok()
    {
        return Err(coded(
            "E_SOURCE_CHANGING",
            "Import source file changed while it was read",
        ));
    }
    Ok((read_total, hasher.finalize().into()))
}

/// `sha256(zip_domain || u64be(archive_len) || archive_sha256 || u64be(count) || entries as in
/// [`manifest_digest`])`.
fn zip_manifest_digest(
    archive_len: u64,
    archive_sha256: &[u8; 32],
    entries: &[ManifestEntry],
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(ZIP_MANIFEST_DOMAIN);
    hasher.update(archive_len.to_be_bytes());
    hasher.update(archive_sha256);
    hasher.update((entries.len() as u64).to_be_bytes());
    for entry in entries {
        hasher.update((entry.path.len() as u32).to_be_bytes());
        hasher.update(entry.path.as_bytes());
        hasher.update(entry.size.to_be_bytes());
        hasher.update(unhex32(&entry.sha256).unwrap_or([0u8; 32]));
    }
    hasher.finalize().into()
}

/// `sha256(domain || u64be(count) || for each entry sorted by path bytes:
/// u32be(len(path)) || path || u64be(size) || sha256(content))`.
pub(crate) fn manifest_digest(entries: &[ManifestEntry]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(MANIFEST_DOMAIN);
    hasher.update((entries.len() as u64).to_be_bytes());
    for entry in entries {
        hasher.update((entry.path.len() as u32).to_be_bytes());
        hasher.update(entry.path.as_bytes());
        hasher.update(entry.size.to_be_bytes());
        hasher.update(unhex32(&entry.sha256).unwrap_or([0u8; 32]));
    }
    hasher.finalize().into()
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) fn unhex32(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 || !text.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f')) {
        return None;
    }
    let mut out = [0u8; 32];
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// Hashes everything read through it. After a parser has consumed a member, `finish_to_eof`
/// proves the parser saw the whole file and yields the byte count and digest to compare with
/// the manifest entry.
pub(crate) struct HashingReader<R> {
    inner: R,
    hasher: Sha256,
    bytes: u64,
}

impl<R: Read> HashingReader<R> {
    pub(crate) fn new(inner: R) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
            bytes: 0,
        }
    }

    /// Reads whatever the parser left unread (bounded by the stream itself) and returns the
    /// total byte count and the SHA-256 of the whole stream.
    pub(crate) fn finish_to_eof(self) -> std::io::Result<(u64, [u8; 32])> {
        self.finish_to_eof_with(&never_interrupt)
    }

    /// [`Self::finish_to_eof`] that polls `interrupt` about once per MiB read. An interrupt
    /// error comes back as an `io::Error` wrapping the coded error (see
    /// `zip::archive_error_from_io`).
    pub(crate) fn finish_to_eof_with(
        mut self,
        interrupt: Interrupt<'_>,
    ) -> std::io::Result<(u64, [u8; 32])> {
        let mut buffer = vec![0u8; 64 * 1024];
        let mut since_poll = 0u64;
        loop {
            match self.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    since_poll += read as u64;
                    if since_poll >= INTERRUPT_EVERY_BYTES {
                        since_poll = 0;
                        interrupt().map_err(|error| std::io::Error::other(error))?;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        }
        Ok((self.bytes, self.hasher.finalize().into()))
    }
}

impl<R: Read> Read for HashingReader<R> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(out)?;
        self.hasher.update(&out[..read]);
        self.bytes += read as u64;
        Ok(read)
    }
}

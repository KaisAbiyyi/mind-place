//! Independent, deterministic measurement of an import source folder (MP-IMPORT-RAW-01, P1-2).
//!
//! The manifest lists every regular file under the source root with its size and SHA-256 and
//! reduces the list to one domain-separated hash. The fields are private: a manifest can only be
//! produced by [`SourceManifest::measure`] reading the folder, never assembled from a caller's
//! claim. The owner flow measures first, pins `sha256()` in the security capability, and the
//! import re-measures and compares, so a swapped file cannot ride on a hash the caller supplied.
//!
//! Errors carry stable codes (`E_SOURCE_*`) and never include file content.

use crate::{ArchiveError, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};

/// Domain separator of the manifest hash.
pub const MANIFEST_DOMAIN: &[u8] = b"mind-place/import-source-manifest/v1\0";

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
    /// Most bytes hashed across all files.
    pub max_total_bytes: u64,
}

impl Default for ManifestLimits {
    fn default() -> Self {
        Self {
            max_entries: 100_000,
            max_depth: 32,
            max_path_bytes: 512,
            max_total_bytes: 64 * 1024 * 1024 * 1024,
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

/// A measured source folder. Constructed only by [`SourceManifest::measure`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceManifest {
    root: PathBuf,
    entries: Vec<ManifestEntry>,
    sha256: [u8; 32],
    total_bytes: u64,
}

impl SourceManifest {
    /// Walks `source_root` and hashes every regular file. Symlinks, reparse points, devices,
    /// non-UTF-8 or unsafe names, and anything beyond `limits` are typed errors, not skipped.
    pub fn measure(source_root: &Path, limits: ManifestLimits) -> Result<Self> {
        Self::walk(source_root, limits, true)
    }

    /// Walks the folder again without hashing and requires the same set of files and sizes.
    /// Cheaper than [`Self::verify_unchanged`]; content binding comes from hashing the bytes
    /// while they are parsed.
    pub fn verify_layout_unchanged(&self, limits: ManifestLimits) -> Result<()> {
        let again = Self::walk(&self.root, limits, false)?;
        let same = again.entries.len() == self.entries.len()
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

    fn walk(source_root: &Path, limits: ManifestLimits, hash_files: bool) -> Result<Self> {
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
                        let (size, digest) = hash_regular_file(&path, &meta)?;
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
            entries,
            sha256,
            total_bytes,
        })
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

fn check_component(name: &str) -> Result<()> {
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
fn hash_regular_file(path: &Path, _inspected: &fs::Metadata) -> Result<(u64, [u8; 32])> {
    let (mut file, before) = open_regular_no_follow(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut read_total = 0u64;
    loop {
        let read = match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(coded("E_SOURCE_IO", "Import source file cannot be read")),
        };
        hasher.update(&buffer[..read]);
        read_total += read as u64;
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
    pub(crate) fn finish_to_eof(mut self) -> std::io::Result<(u64, [u8; 32])> {
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            match self.read(&mut buffer) {
                Ok(0) => break,
                Ok(_) => {}
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

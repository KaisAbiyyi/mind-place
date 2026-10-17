use crate::{
    conversation_id_from_url, ArchiveError, ArchiveStore, Attachment, Capture, ImportReceipt,
    Message, Result,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::Value;
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::fs::File;
use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, OpenOptions},
    io::Write,
    path::{Component, Path, PathBuf},
};

const MAX_LEGACY_SESSIONS: usize = 2048;
const MAX_SESSION_ENTRIES: usize = 50_000;
const MAX_SESSION_DEPTH: usize = 128;
const MAX_SESSION_PATH_BYTES: usize = 4096;
const MAX_SESSION_MANIFEST_BYTES: usize = 16 * 1024 * 1024;
const MAX_SESSION_BYTES: u64 = 256 * 1024 * 1024;
const MAX_TOTAL_IMPORT_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_LEGACY_FILE_BYTES: usize = 32 * 1024 * 1024;
const MAX_META_BYTES: usize = 1024 * 1024;
const MAX_OVERFLOW_BYTES: usize = 8 * 1024 * 1024;
const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_CAPTURE_TEXT_BYTES: usize = 8 * 1024 * 1024;
const MAX_CAPTURE_IMAGE_BYTES: usize = 6 * 1024 * 1024;
const MAX_CAPTURE_ATTACHMENTS: usize = 500;
const MAX_MESSAGES: usize = 10_000;
const MAX_CANDIDATE_MESSAGES: usize = 25_000;
const MAX_NATIVE_IMAGES: usize = 10_000;
const MAX_WARNINGS: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EntryKind {
    Directory,
    File,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TreeEntry {
    relative: PathBuf,
    kind: EntryKind,
    bytes: u64,
}

#[derive(Clone, Debug)]
struct TreeManifest {
    entries: Vec<TreeEntry>,
    digest: String,
    bytes: u64,
}

#[derive(Default)]
struct ScanBudget {
    entries: usize,
    bytes: u64,
    path_bytes: usize,
}

#[derive(Clone, Debug)]
struct LegacyAssetRef {
    id: String,
    mime_type: Option<String>,
    name: Option<String>,
}

#[derive(Clone, Debug)]
struct LegacyUpload {
    id: String,
    name: String,
    mime_type: String,
    preview: Option<String>,
}

#[derive(Clone, Debug)]
struct LegacyMessage {
    kind: String,
    message_id: String,
    seq: u64,
    position: u64,
    time: Option<String>,
    model: Option<String>,
    stored_text: StoredText,
    assets: Vec<LegacyAssetRef>,
    uploads: Vec<LegacyUpload>,
    source_priority: u8,
}

#[derive(Clone, Debug)]
struct StoredText {
    text: String,
    truncated: bool,
    asset_id: Option<String>,
}

#[derive(Clone, Debug)]
struct LegacyImage {
    message_id: String,
    provider_asset_id: String,
    asset: Option<LegacyAssetRef>,
    preview_error: Option<String>,
    source_priority: u8,
    seq: u64,
}

impl ArchiveStore {
    /// Imports known legacy Mind Place session folders. The supplied path is the trusted old profile's
    /// `sessions` directory; it is never interpreted as a path from archive content.
    pub fn import_legacy(&self, sessions: &Path) -> Result<ImportReceipt> {
        validate_source_root(sessions)?;

        let mut receipt = ImportReceipt::default();
        let mut session_dirs = Vec::new();
        let mut over_limit = false;
        for entry in fs::read_dir(sessions)? {
            let entry = entry?;
            if session_dirs.len() >= MAX_LEGACY_SESSIONS {
                over_limit = true;
                break;
            }
            session_dirs.push(entry.path());
        }
        session_dirs.sort_by(|left, right| path_name_bytes(left).cmp(&path_name_bytes(right)));
        if over_limit {
            receipt.failed += 1;
            receipt.errors.push(format!(
                "Legacy session scan reached the {MAX_LEGACY_SESSIONS}-session limit; remaining entries were not scanned."
            ));
        }

        let mut total_scanned_bytes = 0u64;
        for source in session_dirs {
            let session_label = source
                .file_name()
                .map(|name| safe_session_name(name))
                .unwrap_or_else(|| "session-unknown".to_owned());
            match self.import_legacy_session(&source, &session_label, &mut total_scanned_bytes) {
                Ok((outcome, issue)) => {
                    match outcome {
                        SessionOutcome::Imported => receipt.imported += 1,
                        SessionOutcome::Skipped => receipt.skipped += 1,
                        SessionOutcome::Failed => receipt.failed += 1,
                    }
                    if let Some(issue) = issue {
                        push_receipt_error(&mut receipt, format!("{session_label}: {issue}"));
                    }
                }
                Err(error) => {
                    receipt.failed += 1;
                    push_receipt_error(&mut receipt, format!("{session_label}: {error}"));
                }
            }
        }

        self.publish_import_receipt(&receipt)?;
        Ok(receipt)
    }

    fn import_legacy_session(
        &self,
        source: &Path,
        session_label: &str,
        total_scanned_bytes: &mut u64,
    ) -> Result<(SessionOutcome, Option<String>)> {
        no_link_ancestors(source)?;
        crate::store::no_link(source)?;
        if !fs::symlink_metadata(source)?.is_dir() {
            return Err(invalid("Legacy session entry is not a directory"));
        }

        let remaining_import_bytes = MAX_TOTAL_IMPORT_BYTES.saturating_sub(*total_scanned_bytes);
        let manifest = scan_tree(source, remaining_import_bytes)?;
        *total_scanned_bytes = total_scanned_bytes.saturating_add(manifest.bytes);

        let (raw_written, raw_path) = self.publish_raw_session(source, session_label, &manifest)?;
        let mut warnings = Vec::new();
        let capture = match build_capture(&raw_path, &manifest, &mut warnings) {
            Ok(Some(capture)) => capture,
            Ok(None) => {
                let reason = warnings.first().cloned().unwrap_or_else(|| {
                    "No readable user or assistant messages were found.".to_owned()
                });
                let issue = format!(
                    "{}; original source copied to {}",
                    reason,
                    raw_path.display()
                );
                return Ok((SessionOutcome::Skipped, Some(issue)));
            }
            Err(error) => {
                return Ok((
                    SessionOutcome::Failed,
                    Some(format!(
                        "transcript conversion failed after raw copy to {}: {error}",
                        raw_path.display()
                    )),
                ));
            }
        };

        match self.save(capture) {
            Ok(saved) => {
                let outcome = if raw_written || !saved.unchanged {
                    SessionOutcome::Imported
                } else {
                    SessionOutcome::Skipped
                };
                let issue = if warnings.is_empty() {
                    None
                } else {
                    Some(clip_utf8(
                        &format!("{} warning(s): {}", warnings.len(), warnings.join("; ")),
                        4096,
                    ))
                };
                Ok((outcome, issue))
            }
            Err(error) => Ok((
                SessionOutcome::Failed,
                Some(format!(
                    "archive save failed after raw copy to {}: {error}",
                    raw_path.display()
                )),
            )),
        }
    }

    fn publish_raw_session(
        &self,
        source: &Path,
        session_label: &str,
        manifest: &TreeManifest,
    ) -> Result<(bool, PathBuf)> {
        let legacy_root = self.root().join("legacy");
        crate::store::checked_dir(&legacy_root)?;
        let session_root = legacy_root.join(session_label);
        let raw_path = session_root.join(&manifest.digest);
        crate::store::no_link(&session_root)?;
        crate::store::no_link(&raw_path)?;

        if raw_path.exists() {
            if !fs::symlink_metadata(&raw_path)?.is_dir() {
                return Err(invalid(
                    "Existing legacy source revision is not a directory",
                ));
            }
            let current = scan_tree(source, manifest.bytes)?;
            if current.digest != manifest.digest || current.entries != manifest.entries {
                return Err(invalid(
                    "Legacy session changed while it was being imported",
                ));
            }
            return Ok((false, raw_path));
        }

        let stage = tempfile::Builder::new()
            .prefix(".legacy-stage-")
            .tempdir_in(&legacy_root)?;
        let copied_digest = copy_tree(source, stage.path(), &manifest.entries)?;
        if copied_digest != manifest.digest {
            return Err(invalid(
                "Legacy source changed while its raw copy was staged",
            ));
        }
        let after_copy = scan_tree(source, manifest.bytes)?;
        if after_copy.digest != manifest.digest || after_copy.entries != manifest.entries {
            return Err(invalid(
                "Legacy session changed while its raw copy was staged",
            ));
        }

        let _lock = self.lock()?;
        crate::store::checked_dir(&session_root)?;
        crate::store::no_link(&raw_path)?;
        if raw_path.exists() {
            if !fs::symlink_metadata(&raw_path)?.is_dir() {
                return Err(invalid(
                    "Existing legacy source revision is not a directory",
                ));
            }
            return Ok((false, raw_path));
        }
        fs::rename(stage.path(), &raw_path)?;
        #[cfg(unix)]
        File::open(&session_root)?.sync_all()?;
        Ok((true, raw_path))
    }

    fn publish_import_receipt(&self, receipt: &ImportReceipt) -> Result<()> {
        let _lock = self.lock()?;
        let root = self.root().join("legacy");
        crate::store::checked_dir(&root)?;
        let bytes = serde_json::to_vec(receipt)?;
        crate::store::atomic_write(&root.join("import-receipt.json"), &bytes, false)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SessionOutcome {
    Imported,
    Skipped,
    Failed,
}

fn invalid(message: impl Into<String>) -> ArchiveError {
    ArchiveError::Invalid(message.into())
}

fn push_receipt_error(receipt: &mut ImportReceipt, text: String) {
    const MAX_RECEIPT_ERRORS: usize = MAX_LEGACY_SESSIONS + 1;
    if receipt.errors.len() < MAX_RECEIPT_ERRORS {
        receipt.errors.push(clip_utf8(&text, 4096));
    }
}

fn validate_source_root(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        return Err(invalid("Legacy sessions path must be absolute"));
    }
    for component in path.components() {
        if matches!(component, Component::ParentDir) {
            return Err(invalid(
                "Legacy sessions path cannot contain parent traversal",
            ));
        }
    }
    no_link_ancestors(path)?;
    crate::store::no_link(path)?;
    if !fs::symlink_metadata(path)?.is_dir() {
        return Err(invalid("Legacy sessions path is not a directory"));
    }
    Ok(())
}

fn no_link_ancestors(path: &Path) -> Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => return Err(invalid("Path cannot contain parent traversal")),
            other => current.push(other.as_os_str()),
        }
        if current.exists() {
            crate::store::no_link(&current)?;
        }
    }
    Ok(())
}

fn path_name_bytes(path: &Path) -> Vec<u8> {
    path.file_name()
        .map(|name| name.as_encoded_bytes().to_vec())
        .unwrap_or_default()
}

fn safe_session_name(name: &std::ffi::OsStr) -> String {
    let bytes = name.as_encoded_bytes();
    if !bytes.is_empty()
        && bytes.len() <= 128
        && bytes != b"."
        && bytes != b".."
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return String::from_utf8(bytes.to_vec()).expect("safe session name is ASCII");
    }
    let hash = hex(&Sha256::digest(bytes));
    format!("session-{}", &hash[..24])
}

fn scan_tree(source: &Path, byte_limit: u64) -> Result<TreeManifest> {
    let mut entries = Vec::new();
    let mut budget = ScanBudget::default();
    let mut hasher = Sha256::new();
    scan_directory(
        source,
        Path::new(""),
        &mut entries,
        &mut budget,
        &mut hasher,
        byte_limit.min(MAX_SESSION_BYTES),
        0,
    )?;
    Ok(TreeManifest {
        entries,
        digest: hex(&hasher.finalize()),
        bytes: budget.bytes,
    })
}

fn scan_directory(
    source: &Path,
    relative: &Path,
    entries: &mut Vec<TreeEntry>,
    budget: &mut ScanBudget,
    hasher: &mut Sha256,
    byte_limit: u64,
    depth: usize,
) -> Result<()> {
    if depth > MAX_SESSION_DEPTH {
        return Err(invalid(
            "Legacy session exceeds its 128-directory nesting limit",
        ));
    }
    let path = source.join(relative);
    crate::store::no_link(&path)?;
    let metadata = fs::symlink_metadata(&path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(invalid(
            "Legacy source contains a link or non-directory path",
        ));
    }

    if !relative.as_os_str().is_empty() {
        note_tree_entry(hasher, b'D', relative, 0);
        push_tree_entry(
            entries,
            budget,
            TreeEntry {
                relative: relative.to_owned(),
                kind: EntryKind::Directory,
                bytes: 0,
            },
        )?;
    }

    let mut children = Vec::new();
    for entry in fs::read_dir(&path)? {
        let entry = entry?;
        children.push(entry.file_name());
        if children.len() > MAX_SESSION_ENTRIES {
            return Err(invalid("Legacy session exceeds its entry-count limit"));
        }
    }
    children.sort_by(|left, right| left.as_encoded_bytes().cmp(right.as_encoded_bytes()));
    for name in children {
        if name.as_encoded_bytes().is_empty() {
            return Err(invalid("Legacy source contains an empty path component"));
        }
        #[cfg(windows)]
        if name.as_encoded_bytes().contains(&b':') {
            return Err(invalid(
                "Legacy source contains a Windows alternate-stream path",
            ));
        }
        let child_relative = relative.join(&name);
        validate_relative(&child_relative)?;
        let child = source.join(&child_relative);
        crate::store::no_link(&child)?;
        let child_metadata = fs::symlink_metadata(&child)?;
        if child_metadata.is_dir() {
            scan_directory(
                source,
                &child_relative,
                entries,
                budget,
                hasher,
                byte_limit,
                depth + 1,
            )?;
        } else if child_metadata.is_file() && !child_metadata.file_type().is_symlink() {
            let size = child_metadata.len();
            if size > MAX_LEGACY_FILE_BYTES as u64 {
                return Err(invalid("Legacy file exceeds the 32 MiB per-file limit"));
            }
            if size > byte_limit.saturating_sub(budget.bytes) {
                return Err(invalid(if byte_limit < MAX_SESSION_BYTES {
                    "Legacy import exceeded its 1 GiB total source-byte limit"
                } else {
                    "Legacy session exceeds its 256 MiB source-byte limit"
                }));
            }
            let bytes = crate::store::bounded_read(&child, MAX_LEGACY_FILE_BYTES)?;
            if bytes.len() as u64 != size {
                return Err(invalid("Legacy source changed while a file was read"));
            }
            note_tree_entry(hasher, b'F', &child_relative, size);
            hasher.update(&bytes);
            push_tree_entry(
                entries,
                budget,
                TreeEntry {
                    relative: child_relative,
                    kind: EntryKind::File,
                    bytes: size,
                },
            )?;
        } else {
            return Err(invalid("Legacy source contains a link or special file"));
        }
    }
    Ok(())
}

fn push_tree_entry(
    entries: &mut Vec<TreeEntry>,
    budget: &mut ScanBudget,
    entry: TreeEntry,
) -> Result<()> {
    budget.entries += 1;
    if budget.entries > MAX_SESSION_ENTRIES {
        return Err(invalid("Legacy session exceeds its 50,000-entry limit"));
    }
    budget.path_bytes = budget
        .path_bytes
        .saturating_add(entry.relative.as_os_str().as_encoded_bytes().len());
    if budget.path_bytes > MAX_SESSION_MANIFEST_BYTES {
        return Err(invalid(
            "Legacy session exceeds its 16 MiB path-manifest limit",
        ));
    }
    budget.bytes = budget.bytes.saturating_add(entry.bytes);
    if budget.bytes > MAX_SESSION_BYTES {
        return Err(invalid(
            "Legacy session exceeds its 256 MiB source-byte limit",
        ));
    }
    entries.push(entry);
    Ok(())
}

fn note_tree_entry(hasher: &mut Sha256, kind: u8, path: &Path, size: u64) {
    let bytes = path.as_os_str().as_encoded_bytes();
    hasher.update([kind]);
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
    hasher.update(size.to_be_bytes());
}

fn validate_relative(path: &Path) -> Result<()> {
    if path.as_os_str().as_encoded_bytes().len() > MAX_SESSION_PATH_BYTES {
        return Err(invalid(
            "Legacy source path exceeds the 4 KiB relative-path limit",
        ));
    }
    for component in path.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(invalid("Legacy source contains a traversing path"));
        }
    }
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path, entries: &[TreeEntry]) -> Result<String> {
    let mut hasher = Sha256::new();
    for entry in entries {
        validate_relative(&entry.relative)?;
        let target = destination.join(&entry.relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        if entry.kind == EntryKind::Directory {
            fs::create_dir_all(&target)?;
            note_tree_entry(&mut hasher, b'D', &entry.relative, 0);
            continue;
        }

        let input = source.join(&entry.relative);
        crate::store::no_link(&input)?;
        let bytes = crate::store::bounded_read(&input, MAX_LEGACY_FILE_BYTES)?;
        if bytes.len() as u64 != entry.bytes {
            return Err(invalid(
                "Legacy source changed while its raw copy was staged",
            ));
        }
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        output.write_all(&bytes)?;
        output.sync_all()?;
        note_tree_entry(&mut hasher, b'F', &entry.relative, entry.bytes);
        hasher.update(&bytes);
    }
    Ok(hex(&hasher.finalize()))
}

fn build_capture(
    source: &Path,
    manifest: &TreeManifest,
    warnings: &mut Vec<String>,
) -> Result<Option<Capture>> {
    let meta_path = source.join("meta.json");
    crate::store::no_link(&meta_path)?;
    let meta_bytes = crate::store::bounded_read(&meta_path, MAX_META_BYTES)?;
    let meta: Value = serde_json::from_slice(&meta_bytes)
        .map_err(|e| invalid(format!("Invalid legacy meta.json: {e}")))?;

    let conversation_id = meta
        .get("conversationId")
        .and_then(Value::as_str)
        .unwrap_or("");
    if conversation_id.is_empty() {
        push_warning(
            warnings,
            "Legacy session has no valid ChatGPT conversation identity; raw evidence was retained.",
        );
        return Ok(None);
    }
    let source_url = format!("https://chatgpt.com/c/{conversation_id}");
    let conversation_id = match conversation_id_from_url(&source_url) {
        Ok(id) => id,
        Err(_) => {
            push_warning(warnings, "Legacy session has a null or invalid ChatGPT conversationId; raw evidence was retained without inventing a provider ID.");
            return Ok(None);
        }
    };

    push_warning(warnings, "Legacy recordings contain observed local content only; full ChatGPT history is not proven.");
    let (mut messages, images) = collect_messages(source, warnings)?;
    if messages.is_empty() {
        push_warning(warnings, "No readable legacy user or assistant messages were found; raw events remain available in the preserved session tree.");
        return Ok(None);
    }
    messages.sort_by(|left, right| {
        left.position
            .cmp(&right.position)
            .then_with(|| left.seq.cmp(&right.seq))
            .then_with(|| {
                let left_role = if left.kind == "user_message" { 0 } else { 1 };
                let right_role = if right.kind == "user_message" { 0 } else { 1 };
                left_role.cmp(&right_role)
            })
            .then_with(|| left.message_id.cmp(&right.message_id))
    });

    if messages.len() > MAX_MESSAGES {
        push_warning(warnings, "Only the first 10,000 ordered messages were converted; the complete source remains in the raw archive.");
        messages.truncate(MAX_MESSAGES);
    }

    let mut image_map: BTreeMap<String, Vec<LegacyImage>> = BTreeMap::new();
    for image in images {
        image_map
            .entry(image.message_id.clone())
            .or_default()
            .push(image);
    }

    let mut text_budget = MAX_CAPTURE_TEXT_BYTES;
    let mut image_budget = MAX_CAPTURE_IMAGE_BYTES;
    let mut attachment_budget = MAX_CAPTURE_ATTACHMENTS;
    let mut converted = Vec::with_capacity(messages.len());
    let mut found_image_message_ids = HashSet::new();
    for mut message in messages {
        let mut text = resolve_stored_text(source, &message, warnings);
        if text.len() > 4 * 1024 * 1024 {
            text = clip_utf8(&text, 4 * 1024 * 1024);
            push_warning(warnings, &format!("Message {} exceeded the 4 MiB display limit; its full text remains in the raw archive.", short_id(&message.message_id)));
        }
        if text.len() > text_budget {
            text = clip_utf8(&text, text_budget);
            text_budget = 0;
            push_warning(warnings, "Imported transcript text reached the archive display budget; remaining message text is preserved in the raw source copy.");
        } else {
            text_budget -= text.len();
        }

        let role = if message.kind == "user_message" {
            "user"
        } else {
            "assistant"
        };
        let mut attachments = Vec::new();
        let mut seen_asset_ids = HashSet::new();
        let has_native_images = role == "assistant" && image_map.contains_key(&message.message_id);
        if has_native_images {
            found_image_message_ids.insert(message.message_id.clone());
        }

        if attachment_budget > attachments.len() {
            for asset in message.assets.drain(..) {
                if attachments.len() >= 100 || attachments.len() >= attachment_budget {
                    push_warning(warnings, "A message attachment limit was reached; excess references remain in the raw session.");
                    break;
                }
                if !seen_asset_ids.insert(asset.id.clone()) {
                    continue;
                }
                attachments.push(asset_attachment(source, asset, &mut image_budget, warnings));
            }
            for upload in message.uploads.drain(..) {
                if attachments.len() >= 100 || attachments.len() >= attachment_budget {
                    push_warning(warnings, "A message attachment limit was reached; excess references remain in the raw session.");
                    break;
                }
                if !seen_asset_ids.insert(format!("upload:{}", upload.id)) {
                    continue;
                }
                attachments.push(upload_attachment(upload, &mut image_budget, warnings));
            }
            if let Some(images) = image_map.get(&message.message_id) {
                for image in images {
                    if attachments.len() >= 100 || attachments.len() >= attachment_budget {
                        push_warning(warnings, "A message attachment limit was reached; excess references remain in the raw session.");
                        break;
                    }
                    let asset = image.asset.clone().unwrap_or_else(|| LegacyAssetRef {
                        id: image.provider_asset_id.clone(),
                        mime_type: None,
                        name: Some("Generated image".to_owned()),
                    });
                    if !seen_asset_ids.insert(asset.id.clone()) {
                        continue;
                    }
                    let mut attachment =
                        asset_attachment(source, asset, &mut image_budget, warnings);
                    attachment.id = stable_attachment_id("native-image", &image.provider_asset_id);
                    attachment.name = image
                        .asset
                        .as_ref()
                        .and_then(|a| a.name.as_deref())
                        .map(safe_text)
                        .filter(|name| !name.is_empty())
                        .unwrap_or_else(|| "Generated image".to_owned());
                    if image.asset.is_none() {
                        attachment.unavailable_reason = Some(format!(
                            "Legacy native image had no retained local asset{}; original event remains in the raw session.",
                            image.preview_error.as_deref().map(|e| format!(" ({e})")).unwrap_or_default()
                        ));
                    }
                    attachments.push(attachment);
                }
            }
        } else if !message.assets.is_empty() || !message.uploads.is_empty() || has_native_images {
            push_warning(warnings, "The imported transcript reached its 500-attachment limit; remaining references are preserved in the raw session.");
        }

        if attachments.len() > 100 {
            attachments.truncate(100);
            push_warning(warnings, "A message had more than 100 attachment references; excess references remain in the raw session.");
        }
        if attachments.len() > attachment_budget {
            attachments.truncate(attachment_budget);
            push_warning(warnings, "The imported transcript reached its 500-attachment limit; remaining references are preserved in the raw session.");
        }
        attachment_budget = attachment_budget.saturating_sub(attachments.len());

        converted.push(Message {
            id: stable_message_id(role, &message.message_id),
            role: role.to_owned(),
            text,
            format: if role == "assistant" {
                "markdown"
            } else {
                "plain"
            }
            .to_owned(),
            authored_at: message.time.take(),
            model: message.model.take().map(|model| clip_utf8(&model, 128)),
            attachments,
        });
    }

    for message_id in image_map.keys() {
        if !found_image_message_ids.contains(message_id) {
            push_warning(warnings, &format!("Legacy native image for message {} was not attached because no matching transcript message was found; raw event and asset remain preserved.", short_id(message_id)));
        }
    }

    let captured_at = meta
        .get("updatedAt")
        .and_then(milliseconds_to_rfc3339)
        .or_else(|| {
            converted
                .iter()
                .filter_map(|m| m.authored_at.as_ref())
                .max()
                .cloned()
        })
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
    let title = meta
        .get("title")
        .and_then(Value::as_str)
        .map(safe_text)
        .filter(|title| !title.trim().is_empty())
        .map(|title| clip_utf8(title.trim(), 1024))
        .unwrap_or_else(|| {
            push_warning(
                warnings,
                "Legacy session title was missing; a generic title was used.",
            );
            "Imported ChatGPT conversation".to_owned()
        });

    // The scan manifest is deliberately kept live through conversion so callers cannot
    // accidentally pass a manifest from another tree in a future refactor.
    let _source_digest = &manifest.digest;
    Ok(Some(Capture {
        schema_version: 1,
        provider: "chatgpt".to_owned(),
        conversation_id,
        source_url,
        title,
        conversation_created_at: None,
        conversation_updated_at: None,
        captured_at,
        capture_method: "legacy-import".to_owned(),
        completeness: "partial".to_owned(),
        warnings: warnings.clone(),
        messages: converted,
    }))
}

fn collect_messages(
    source: &Path,
    warnings: &mut Vec<String>,
) -> Result<(Vec<LegacyMessage>, Vec<LegacyImage>)> {
    let mut candidates: BTreeMap<String, LegacyMessage> = BTreeMap::new();
    let mut images: BTreeMap<(String, String), LegacyImage> = BTreeMap::new();
    let journal = source.join("events.jsonl");
    if journal.exists() {
        match read_source_file(&journal, MAX_LEGACY_FILE_BYTES) {
            Ok(bytes) => {
                for (index, line) in bytes.split(|byte| *byte == b'\n').enumerate() {
                    if line.iter().all(u8::is_ascii_whitespace) {
                        continue;
                    }
                    match serde_json::from_slice::<Value>(line) {
                        Ok(event) => add_event_candidate(&mut candidates, &mut images, &event, 0, warnings),
                        Err(_) => push_warning(warnings, &format!("Unreadable legacy event line {} remains available in the raw session.", index + 1)),
                    }
                }
            }
            Err(error) => push_warning(warnings, &format!("Legacy event journal could not be read for conversion: {error}; raw bytes were retained.")),
        }
    }

    let canonical = source.join("messages.json");
    if canonical.exists() {
        match read_source_file(&canonical, MAX_LEGACY_FILE_BYTES).and_then(|bytes| {
            serde_json::from_slice::<Value>(&bytes).map_err(|error| invalid(format!("Invalid messages.json: {error}")))
        }) {
            Ok(Value::Object(map)) => {
                for (key, event) in map {
                    if !event.is_object() {
                        continue;
                    }
                    let kind = event.get("kind").and_then(Value::as_str).unwrap_or("");
                    match kind {
                        "user_message" | "assistant_message" => {
                            let message_id = event.get("messageId").and_then(Value::as_str).unwrap_or("");
                            if key != format!("{kind}\0{message_id}") {
                                push_warning(warnings, "A canonical legacy message had a mismatched identity key and was not converted; raw evidence remains preserved.");
                                continue;
                            }
                            add_event_candidate(&mut candidates, &mut images, &event, 1, warnings);
                        }
                        "native_image" => {
                            if native_image_key(&event).as_deref() == Some(key.as_str()) {
                                if let Some(image) = parse_native_image(&event, 1, warnings) {
                                    insert_image(&mut images, image, warnings);
                                } else {
                                    push_warning(warnings, "A canonical native image event had incomplete identity fields; raw evidence remains preserved.");
                                }
                            } else {
                                push_warning(warnings, "A canonical native image event had a mismatched identity key; raw evidence remains preserved.");
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(_) => push_warning(warnings, "Legacy messages.json was not an object; its bytes remain in the raw archive."),
            Err(error) => push_warning(warnings, &format!("Legacy messages.json could not be parsed: {error}; raw bytes remain in the archive.")),
        }
    }

    let shards = source.join("messages");
    if shards.exists() {
        crate::store::no_link(&shards)?;
        let mut paths = Vec::new();
        for entry in fs::read_dir(&shards)? {
            let entry = entry?;
            paths.push(entry.path());
            if paths.len() > MAX_SESSION_ENTRIES {
                return Err(invalid(
                    "Legacy canonical message shard count exceeds its limit",
                ));
            }
        }
        paths.sort_by(|left, right| path_name_bytes(left).cmp(&path_name_bytes(right)));
        for path in paths {
            if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                continue;
            }
            let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            match read_source_file(&path, MAX_LEGACY_FILE_BYTES).and_then(|bytes| {
                serde_json::from_slice::<Value>(&bytes).map_err(|error| invalid(format!("Invalid canonical message shard: {error}")))
            }) {
                Ok(event) => {
                    let kind = event.get("kind").and_then(Value::as_str).unwrap_or("");
                    if matches!(kind, "user_message" | "assistant_message") {
                        let Some(message_id) = event.get("messageId").and_then(Value::as_str) else {
                            push_warning(warnings, "A canonical message shard had no messageId and was not converted; raw evidence remains preserved.");
                            continue;
                        };
                        let key = format!("{kind}\0{message_id}");
                        if format!("{}.json", hex(&Sha256::digest(key.as_bytes()))) != file_name {
                            push_warning(warnings, "A canonical message shard had an invalid filename identity and was not converted.");
                            continue;
                        }
                        add_event_candidate(&mut candidates, &mut images, &event, 2, warnings);
                    } else if kind == "native_image" {
                        if let Some(key) = native_image_key(&event) {
                            if format!("{}.json", hex(&Sha256::digest(key.as_bytes()))) == file_name {
                                if let Some(image) = parse_native_image(&event, 2, warnings) {
                                    insert_image(&mut images, image, warnings);
                                } else {
                                    push_warning(warnings, "A canonical native image shard had incomplete identity fields; raw evidence remains preserved.");
                                }
                            } else {
                                push_warning(warnings, "A canonical native image shard had a mismatched filename identity; raw evidence remains preserved.");
                            }
                        } else {
                            push_warning(warnings, "A canonical native image shard had no identity fields; raw evidence remains preserved.");
                        }
                    }
                }
                Err(error) => push_warning(warnings, &format!("A canonical message shard could not be converted: {error}; raw bytes remain available.")),
            }
        }
    }

    let mut out = candidates.into_values().collect::<Vec<_>>();
    out.sort_by(|left, right| {
        left.position
            .cmp(&right.position)
            .then(left.seq.cmp(&right.seq))
    });
    Ok((out, images.into_values().collect()))
}

fn add_event_candidate(
    candidates: &mut BTreeMap<String, LegacyMessage>,
    images: &mut BTreeMap<(String, String), LegacyImage>,
    event: &Value,
    source_priority: u8,
    warnings: &mut Vec<String>,
) {
    match event.get("kind").and_then(Value::as_str).unwrap_or("") {
        "user_message" | "assistant_message" => {
            let kind = event.get("kind").and_then(Value::as_str).unwrap_or("");
            let Some(message_id) = event
                .get("messageId")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
            else {
                push_warning(warnings, "A legacy message without messageId was not converted; its raw event remains preserved.");
                return;
            };
            let Some(stored_text) = parse_stored_text(event.get("message")) else {
                push_warning(warnings, &format!("Legacy message {} had no readable StoredText; its raw event remains preserved.", short_id(message_id)));
                return;
            };
            let seq = number_u64(event.get("seq")).unwrap_or(0);
            let position = number_u64(event.get("origin")).unwrap_or(seq);
            let timestamp = event.get("authoredAt").and_then(milliseconds_to_rfc3339);
            let model = if kind == "assistant_message" {
                event
                    .get("resolvedModel")
                    .and_then(Value::as_str)
                    .or_else(|| event.get("model").and_then(Value::as_str))
                    .map(safe_text)
                    .filter(|model| !model.is_empty())
            } else {
                None
            };
            let mut candidate = LegacyMessage {
                kind: kind.to_owned(),
                message_id: message_id.to_owned(),
                seq,
                position,
                time: timestamp,
                model,
                stored_text,
                assets: parse_asset_refs(event.get("assets"), message_id, warnings),
                uploads: parse_uploads(event.get("attachments"), message_id, warnings),
                source_priority,
            };
            let key = format!("{kind}\0{message_id}");
            if !candidates.contains_key(&key) && candidates.len() >= MAX_CANDIDATE_MESSAGES {
                push_warning(warnings, "More than 25,000 legacy message identities were present; excess messages remain in the raw session.");
                return;
            }
            let replace = candidates.get(&key).is_none_or(|old| {
                candidate.source_priority > old.source_priority
                    || (candidate.source_priority == old.source_priority
                        && candidate.seq >= old.seq)
            });
            if replace {
                if let Some(old) = candidates.remove(&key) {
                    merge_missing_message_metadata(&mut candidate, old);
                }
                candidates.insert(key, candidate);
            } else if let Some(existing) = candidates.get_mut(&key) {
                merge_missing_message_metadata(existing, candidate);
            }
        }
        "native_image" => {
            if let Some(image) = parse_native_image(event, source_priority, warnings) {
                insert_image(images, image, warnings);
            }
        }
        _ => {}
    }
}

fn merge_missing_message_metadata(selected: &mut LegacyMessage, fallback: LegacyMessage) {
    let same_text = selected.stored_text.text == fallback.stored_text.text
        && selected.stored_text.truncated == fallback.stored_text.truncated;
    if selected.position == 0 {
        selected.position = fallback.position;
    }
    if selected.seq == 0 {
        selected.seq = fallback.seq;
    }
    if same_text {
        if selected.time.is_none() {
            selected.time = fallback.time;
        }
        if selected.model.is_none() {
            selected.model = fallback.model;
        }
        for asset in fallback.assets {
            if !selected.assets.iter().any(|current| current.id == asset.id) {
                selected.assets.push(asset);
            }
        }
        for upload in fallback.uploads {
            if !selected
                .uploads
                .iter()
                .any(|current| current.id == upload.id)
            {
                selected.uploads.push(upload);
            }
        }
    }
}

fn insert_image(
    images: &mut BTreeMap<(String, String), LegacyImage>,
    image: LegacyImage,
    warnings: &mut Vec<String>,
) {
    let key = (image.message_id.clone(), image.provider_asset_id.clone());
    if let Some(existing) = images.get_mut(&key) {
        if image.source_priority > existing.source_priority
            || (image.source_priority == existing.source_priority && image.seq >= existing.seq)
        {
            *existing = image;
        }
    } else if images.len() < MAX_NATIVE_IMAGES {
        images.insert(key, image);
    } else {
        push_warning(warnings, "Native image event count exceeded 10,000; remaining media events remain in the raw session.");
    }
}

fn parse_native_image(
    event: &Value,
    source_priority: u8,
    warnings: &mut Vec<String>,
) -> Option<LegacyImage> {
    let message_id = event
        .get("messageId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty());
    let provider_asset_id = event
        .get("providerAssetId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty());
    let (Some(message_id), Some(provider_asset_id)) = (message_id, provider_asset_id) else {
        let message_id = message_id.unwrap_or("unknown");
        push_warning(
            warnings,
            &format!(
                "Legacy native image for message {} had incomplete identity fields; raw evidence remains preserved.",
                short_id(message_id)
            ),
        );
        return None;
    };
    let asset = parse_asset_ref(event.get("asset"));
    if asset.is_none() {
        push_warning(
            warnings,
            &format!(
                "Legacy native image for message {} had no readable local asset reference; raw evidence remains preserved.",
                short_id(message_id)
            ),
        );
    }
    Some(LegacyImage {
        message_id: message_id.to_owned(),
        provider_asset_id: provider_asset_id.to_owned(),
        asset,
        preview_error: event
            .get("previewError")
            .and_then(Value::as_str)
            .map(safe_text),
        source_priority,
        seq: number_u64(event.get("seq")).unwrap_or(0),
    })
}

fn native_image_key(event: &Value) -> Option<String> {
    Some(format!(
        "native_image\0{}\0{}",
        event.get("messageId")?.as_str()?,
        event.get("providerAssetId")?.as_str()?
    ))
}

fn parse_asset_refs(
    value: Option<&Value>,
    message_id: &str,
    warnings: &mut Vec<String>,
) -> Vec<LegacyAssetRef> {
    let Some(value) = value else {
        return Vec::new();
    };
    let Some(entries) = value.as_array() else {
        push_warning(
            warnings,
            &format!(
                "Asset references for legacy message {} were malformed; raw evidence remains preserved.",
                short_id(message_id)
            ),
        );
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| match parse_asset_ref(Some(entry)) {
            Some(asset) => Some(asset),
            None => {
                push_warning(
                    warnings,
                    &format!(
                        "A malformed asset reference for legacy message {} was not attached; raw evidence remains preserved.",
                        short_id(message_id)
                    ),
                );
                None
            }
        })
        .collect()
}

fn parse_asset_ref(value: Option<&Value>) -> Option<LegacyAssetRef> {
    let value = value?;
    let id = value.get("id")?.as_str()?.to_owned();
    Some(LegacyAssetRef {
        id,
        mime_type: value
            .get("mimeType")
            .or_else(|| value.get("mime_type"))
            .and_then(Value::as_str)
            .map(safe_text),
        name: value.get("name").and_then(Value::as_str).map(safe_text),
    })
}

fn parse_uploads(
    value: Option<&Value>,
    message_id: &str,
    warnings: &mut Vec<String>,
) -> Vec<LegacyUpload> {
    let Some(value) = value else {
        return Vec::new();
    };
    let Some(entries) = value.as_array() else {
        push_warning(
            warnings,
            &format!(
                "Uploaded file references for legacy message {} were malformed; raw evidence remains preserved.",
                short_id(message_id)
            ),
        );
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let Some(id) = entry.get("id").and_then(Value::as_str).filter(|id| !id.is_empty()) else {
                push_warning(
                    warnings,
                    &format!(
                        "A malformed upload reference for legacy message {} was not attached; raw evidence remains preserved.",
                        short_id(message_id)
                    ),
                );
                return None;
            };
            Some(LegacyUpload {
                id: id.to_owned(),
                name: entry
                    .get("name")
                    .and_then(Value::as_str)
                    .map(safe_text)
                    .unwrap_or_else(|| "Uploaded file".to_owned()),
                mime_type: entry
                    .get("mimeType")
                    .and_then(Value::as_str)
                    .map(safe_text)
                    .unwrap_or_else(|| "application/octet-stream".to_owned()),
                preview: entry
                    .get("preview")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            })
        })
        .collect()
}

fn parse_stored_text(value: Option<&Value>) -> Option<StoredText> {
    let value = value?;
    if let Some(text) = value.as_str() {
        return Some(StoredText {
            text: text.to_owned(),
            truncated: false,
            asset_id: None,
        });
    }
    Some(StoredText {
        text: value.get("text")?.as_str()?.to_owned(),
        truncated: value
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        asset_id: value
            .get("assetId")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

fn resolve_stored_text(
    source: &Path,
    message: &LegacyMessage,
    warnings: &mut Vec<String>,
) -> String {
    if !message.stored_text.truncated {
        return message.stored_text.text.clone();
    }
    let Some(asset_id) = message.stored_text.asset_id.as_deref() else {
        push_warning(warnings, &format!("Truncated message {} has no overflow asset reference; its inline prefix is retained.", short_id(&message.message_id)));
        return message.stored_text.text.clone();
    };
    if !safe_asset_id(asset_id) || !asset_id.ends_with(".txt") {
        push_warning(warnings, &format!("Truncated message {} had an unsafe overflow asset reference; its inline prefix is retained.", short_id(&message.message_id)));
        return message.stored_text.text.clone();
    }
    let path = source.join("assets").join(asset_id);
    match read_source_file(&path, MAX_OVERFLOW_BYTES) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) => {
                push_warning(warnings, &format!("Overflow text for message {} was not UTF-8; its inline prefix is retained.", short_id(&message.message_id)));
                message.stored_text.text.clone()
            }
        },
        Err(_) => {
            push_warning(warnings, &format!("Overflow text for message {} was unavailable or exceeded 8 MiB; its inline prefix is retained.", short_id(&message.message_id)));
            message.stored_text.text.clone()
        }
    }
}

fn asset_attachment(
    source: &Path,
    asset: LegacyAssetRef,
    remaining: &mut usize,
    warnings: &mut Vec<String>,
) -> Attachment {
    let safe_id = safe_asset_id(&asset.id);
    let name = asset
        .name
        .as_deref()
        .map(safe_text)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| asset.id.clone());
    let mut attachment = Attachment {
        id: stable_attachment_id("asset", &asset.id),
        name: clip_utf8(&name, 1024),
        mime_type: asset
            .mime_type
            .as_deref()
            .map(|m| clip_utf8(m, 128))
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| "application/octet-stream".to_owned()),
        source_url: None,
        data_url: None,
        asset_path: None,
        unavailable_reason: None,
    };
    if !safe_id {
        attachment.unavailable_reason = Some("Legacy asset reference was not a safe local filename; original reference remains in the raw session.".to_owned());
        push_warning(
            warnings,
            "An unsafe legacy asset reference was not opened; raw source evidence was preserved.",
        );
        return attachment;
    }

    let path = source.join("assets").join(&asset.id);
    if *remaining == 0 {
        attachment.unavailable_reason = Some("Local image was not embedded because the import image budget was reached; original bytes remain in the raw session.".to_owned());
        push_warning(warnings, "Some local images exceeded the 6 MiB aggregate import budget; they remain in the raw session with visible references.");
        return attachment;
    }
    let source_size = crate::store::no_link(&path)
        .and_then(|()| fs::symlink_metadata(&path).map_err(ArchiveError::from))
        .ok()
        .filter(|metadata| metadata.is_file())
        .map(|metadata| metadata.len());
    if source_size.is_some_and(|size| size > *remaining as u64) {
        attachment.unavailable_reason = Some("Local image was not embedded because it would exceed the remaining import image budget; original bytes remain in the raw session.".to_owned());
        push_warning(warnings, "Some local images exceeded the 6 MiB aggregate import budget; they remain in the raw session with visible references.");
        return attachment;
    }
    match read_source_file(&path, MAX_IMAGE_BYTES) {
        Ok(bytes) => match image_kind(&bytes) {
            Some((mime, _)) if bytes.len() <= *remaining => {
                attachment.mime_type = mime.to_owned();
                attachment.data_url =
                    Some(format!("data:{mime};base64,{}", STANDARD.encode(&bytes)));
                *remaining -= bytes.len();
            }
            Some((_, _)) => {
                attachment.unavailable_reason = Some("Local image was not embedded because the import image budget was reached; original bytes remain in the raw session.".to_owned());
                push_warning(warnings, "Some local images exceeded the 6 MiB aggregate import budget; they remain in the raw session with visible references.");
            }
            None => {
                attachment.unavailable_reason = Some("Legacy asset is not a supported local image; original bytes remain in the raw session.".to_owned());
            }
        },
        Err(_) => {
            attachment.unavailable_reason = Some("Legacy asset bytes were missing or exceeded 8 MiB; original reference remains in the raw session.".to_owned());
            push_warning(warnings, "A referenced legacy media asset was missing or exceeded 8 MiB; the transcript shows an unavailable reference and raw evidence is preserved.");
        }
    }
    attachment
}

fn upload_attachment(
    upload: LegacyUpload,
    remaining: &mut usize,
    warnings: &mut Vec<String>,
) -> Attachment {
    let mut attachment = Attachment {
        id: stable_attachment_id("upload", &upload.id),
        name: clip_utf8(&upload.name, 1024),
        mime_type: clip_utf8(&upload.mime_type, 128),
        source_url: None,
        data_url: None,
        asset_path: None,
        unavailable_reason: Some("Legacy upload metadata was recorded, but original file bytes were not retained locally.".to_owned()),
    };
    if let Some(preview) = upload.preview.as_deref() {
        let estimated_bytes = base64_data_length(preview);
        if *remaining == 0 || estimated_bytes.is_some_and(|size| size > *remaining) {
            push_warning(
                warnings,
                "A legacy upload preview was omitted because the import image budget was reached.",
            );
        } else if let Some((mime, bytes)) = decode_safe_image_data_url(preview) {
            if bytes.len() <= *remaining {
                attachment.mime_type = mime.to_owned();
                attachment.data_url =
                    Some(format!("data:{mime};base64,{}", STANDARD.encode(&bytes)));
                attachment.unavailable_reason = None;
                *remaining -= bytes.len();
            } else {
                push_warning(warnings, "A legacy upload preview was omitted because the import image budget was reached.");
            }
        }
    }
    attachment
}

fn base64_data_length(value: &str) -> Option<usize> {
    if value.len() > MAX_IMAGE_BYTES * 4 / 3 + 128 {
        return None;
    }
    let (header, payload) = value.split_once(',')?;
    if !header.starts_with("data:image/") || !header.ends_with(";base64") {
        return None;
    }
    let padding = payload
        .bytes()
        .rev()
        .take_while(|byte| *byte == b'=')
        .count()
        .min(2);
    Some((payload.len().saturating_mul(3) / 4).saturating_sub(padding))
}

fn decode_safe_image_data_url(value: &str) -> Option<(&'static str, Vec<u8>)> {
    if value.len() > MAX_IMAGE_BYTES * 4 / 3 + 128 {
        return None;
    }
    let (header, payload) = value.split_once(',')?;
    let bytes = STANDARD.decode(payload).ok()?;
    if bytes.len() > MAX_IMAGE_BYTES {
        return None;
    }
    let (mime, _) = image_kind(&bytes)?;
    (header == format!("data:{mime};base64")).then_some((mime, bytes))
}

fn image_kind(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(("image/png", "png"))
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some(("image/jpeg", "jpg"))
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(("image/gif", "gif"))
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some(("image/webp", "webp"))
    } else {
        None
    }
}

fn safe_asset_id(asset_id: &str) -> bool {
    let Some((hash, extension)) = asset_id.rsplit_once('.') else {
        return false;
    };
    (8..=64).contains(&hash.len())
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && matches!(extension, "png" | "jpg" | "gif" | "webp" | "txt" | "bin")
}

fn read_source_file(path: &Path, max_bytes: usize) -> Result<Vec<u8>> {
    no_link_ancestors(path)?;
    crate::store::no_link(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(invalid("Legacy source path is not a regular file"));
    }
    crate::store::bounded_read(path, max_bytes)
}

fn stable_message_id(role: &str, provider_id: &str) -> String {
    let candidate = format!("legacy:{role}:{provider_id}");
    if candidate.len() <= 256 && is_safe_identity(&candidate) {
        candidate
    } else {
        format!(
            "legacy:{role}:{}",
            hex(&Sha256::digest(provider_id.as_bytes()))
        )
    }
}

fn stable_attachment_id(kind: &str, provider_id: &str) -> String {
    let candidate = format!("legacy-{kind}:{provider_id}");
    if candidate.len() <= 256 && is_safe_identity(&candidate) {
        candidate
    } else {
        format!(
            "legacy-{kind}:{}",
            hex(&Sha256::digest(provider_id.as_bytes()))
        )
    }
}

fn is_safe_identity(value: &str) -> bool {
    value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'@')
    })
}

fn milliseconds_to_rfc3339(value: &Value) -> Option<String> {
    let millis = value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|v| i64::try_from(v).ok()))?;
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(millis)
        .map(|date| date.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

fn number_u64(value: Option<&Value>) -> Option<u64> {
    value?.as_u64()
}

fn safe_text(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_control() || *ch == '\t')
        .collect()
}

fn short_id(value: &str) -> String {
    clip_utf8(value, 80)
}

fn clip_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

fn push_warning(warnings: &mut Vec<String>, warning: &str) {
    if warnings.iter().any(|existing| existing == warning) {
        return;
    }
    if warnings.len() < MAX_WARNINGS - 1 {
        warnings.push(clip_utf8(warning, 2048));
    } else if warnings.len() == MAX_WARNINGS - 1 {
        warnings.push("Additional legacy import warnings were omitted; full details remain in the preserved raw session.".to_owned());
    }
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

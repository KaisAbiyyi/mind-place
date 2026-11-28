//! Claude.ai official data export (MP-R1-WP01).
//!
//! The export is a `.zip` (or extracted folder) with `conversations.json` (a top-level array of
//! conversations) and projects, either as `projects.json` (a top-level array, the earlier layout)
//! or, in the split export observed in 2026 (one `<category>-NNN.zip` per category, listed by a
//! `manifest-*.json`), as `projects/<id>.json` files holding one project object each. Other
//! categories of the split export (memories, frames/artifacts, claude_code_cloud, light_metadata
//! with the `users.json` account file) are not import members. Every rule here fails towards
//! "raw evidence only":
//!
//! - the exact record bytes are always kept as raw evidence;
//! - a projection is made only when message order is either unproved (no parent links: `partial`)
//!   or proved linear; a fork, cycle or out-of-order link keeps the record raw-only, because the
//!   active branch cannot be chosen without guessing;
//! - nothing missing is invented: no model (the export does not name one), no message time that is
//!   not in the record, no file body (only file metadata), no hidden reasoning shown as text.

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io::Read;

use crate::importers::chatgpt_export::{clip, record_completeness_gaps, ConversationIdTracker};
use crate::importers::{
    stream_bounded_top_level_array_spanned, BoundedArrayLimits, RawEvidenceEnvelope,
    ReconciledItem, RecordSpan,
};
use crate::{Attachment, Capture, Message};

pub const PROVIDER: &str = "claude";
/// Envelope domain of one `conversations.json` record.
pub const CONVERSATION_DOMAIN: &str = "claude-export-conversation";
/// Envelope domain of one `projects.json` record.
pub const PROJECT_DOMAIN: &str = "claude-export-project";
/// Largest accepted record of either member, enforced while streaming.
pub const MAX_CLAUDE_RECORD_BYTES: usize = 64 * 1024 * 1024; // 64 MiB
pub const MAX_CLAUDE_SEPARATOR_BYTES: usize = 1024 * 1024; // 1 MiB
/// Most records `parse_conversations_json` collects in memory (tests and small tools only).
pub const MAX_ITEMS_IN_MEMORY: usize = 50_000;
pub const MAX_COLLECTED_RAW_BYTES: usize = 256 * 1024 * 1024; // 256 MiB
/// Longest identifier kept as the evidence `source_identifier`.
const MAX_SOURCE_IDENTIFIER_BYTES: usize = 256;
/// Most attachments shown per message (the archive contract allows 100).
const MAX_ATTACHMENTS_PER_MESSAGE: usize = 100;
/// Parent of the first message in exports that use a sentinel instead of `null`.
const ROOT_PARENT_SENTINEL: &str = "00000000-0000-4000-8000-000000000000";
/// The standard nil UUID, accepted as a root marker too.
const NIL_UUID: &str = "00000000-0000-0000-0000-000000000000";

const STREAM_LIMITS: BoundedArrayLimits = BoundedArrayLimits {
    max_record_bytes: MAX_CLAUDE_RECORD_BYTES,
    max_separator_bytes: MAX_CLAUDE_SEPARATOR_BYTES,
};

type BoxError = Box<dyn std::error::Error>;

pub struct ClaudeExportParser;

impl ClaudeExportParser {
    /// Streams `conversations.json` with a caller-owned duplicate tracker, so duplicate
    /// conversation ids are disambiguated across every member of one export. Each item comes with
    /// the span of the exact source bytes it was cut from.
    pub fn stream_conversations_json_tracked<
        R: Read,
        F: FnMut(RecordSpan, ReconciledItem) -> Result<(), BoxError>,
    >(
        reader: R,
        limits: BoundedArrayLimits,
        seen_ids: &mut ConversationIdTracker,
        mut callback: F,
    ) -> Result<usize, BoxError> {
        stream_bounded_top_level_array_spanned(reader, limits, |offset, record| {
            let span = RecordSpan::of(offset, record);
            let item = Self::reconcile_conversation_raw(record.to_vec(), seen_ids)?;
            callback(span, item)
        })
    }

    /// Streams `projects.json`. A project is not a conversation: every record is preserved as
    /// raw evidence only.
    pub fn stream_projects_json_spanned<
        R: Read,
        F: FnMut(RecordSpan, ReconciledItem) -> Result<(), BoxError>,
    >(
        reader: R,
        limits: BoundedArrayLimits,
        mut callback: F,
    ) -> Result<usize, BoxError> {
        stream_bounded_top_level_array_spanned(reader, limits, |offset, record| {
            let span = RecordSpan::of(offset, record);
            callback(span, Self::reconcile_project_raw(record.to_vec()))
        })
    }

    /// Reads one `projects/<id>.json` file of the split (2026) export: the whole file is one
    /// project object, preserved byte for byte as raw evidence only. The read is capped at
    /// `max_bytes` plus one byte, so an oversized file is refused before it is buffered whole.
    pub fn read_project_file_spanned<
        R: Read,
        F: FnMut(RecordSpan, ReconciledItem) -> Result<(), BoxError>,
    >(
        reader: R,
        max_bytes: usize,
        mut callback: F,
    ) -> Result<usize, BoxError> {
        let mut raw = Vec::new();
        reader.take(max_bytes as u64 + 1).read_to_end(&mut raw)?;
        if raw.len() > max_bytes {
            return Err(format!("Import record exceeds the {max_bytes} byte limit").into());
        }
        let span = RecordSpan::of(0, &raw);
        callback(span, Self::reconcile_project_raw(raw))?;
        Ok(1)
    }

    /// In-memory convenience for small inputs (tests, tools). Bounded by count and bytes.
    pub fn parse_conversations_json<R: Read>(reader: R) -> Result<Vec<ReconciledItem>, BoxError> {
        let mut tracker = ConversationIdTracker::default();
        let mut items = Vec::new();
        let mut collected = 0usize;
        Self::stream_conversations_json_tracked(reader, STREAM_LIMITS, &mut tracker, |_, item| {
            collected = collected.saturating_add(raw_len(&item));
            if items.len() >= MAX_ITEMS_IN_MEMORY || collected > MAX_COLLECTED_RAW_BYTES {
                return Err("Claude export exceeds in-memory collection limit; stream it".into());
            }
            items.push(item);
            Ok(())
        })?;
        Ok(items)
    }

    /// In-memory convenience for `projects.json`. Bounded by count and bytes.
    pub fn parse_projects_json<R: Read>(reader: R) -> Result<Vec<ReconciledItem>, BoxError> {
        let mut items = Vec::new();
        let mut collected = 0usize;
        Self::stream_projects_json_spanned(reader, STREAM_LIMITS, |_, item| {
            collected = collected.saturating_add(raw_len(&item));
            if items.len() >= MAX_ITEMS_IN_MEMORY || collected > MAX_COLLECTED_RAW_BYTES {
                return Err("Claude export exceeds in-memory collection limit; stream it".into());
            }
            items.push(item);
            Ok(())
        })?;
        Ok(items)
    }

    fn reconcile_project_raw(raw: Vec<u8>) -> ReconciledItem {
        let sha = format!("{:x}", Sha256::digest(&raw));
        let parsed: Option<Value> = serde_json::from_slice(&raw).ok();
        let (identifier, metadata) = match parsed.as_ref().and_then(Value::as_object) {
            Some(project) => (
                bounded_identifier(project.get("uuid")),
                serde_json::json!({
                    "record": "project",
                    "doc_count": project.get("docs").and_then(Value::as_array).map_or(0, Vec::len),
                    "has_prompt_template": project
                        .get("prompt_template")
                        .and_then(Value::as_str)
                        .is_some_and(|text| !text.is_empty()),
                }),
            ),
            None => (
                String::new(),
                serde_json::json!({"record": "project", "parse_failed": true}),
            ),
        };
        ReconciledItem::PreservedRawOnly {
            evidence: RawEvidenceEnvelope {
                provider: PROVIDER.into(),
                source_identifier: identifier,
                envelope_domain: PROJECT_DOMAIN.into(),
                payload_bytes: raw,
                payload_sha256: sha,
                metadata_json: metadata.to_string(),
            },
            disposition_reason: if parsed.as_ref().is_some_and(Value::is_object) {
                "Claude project record preserved as raw evidence; projects are not conversations"
                    .into()
            } else {
                "Claude project record is not a JSON object; raw record preserved".into()
            },
            assets: vec![],
        }
    }

    pub(crate) fn reconcile_conversation_raw(
        raw: Vec<u8>,
        seen_ids: &mut ConversationIdTracker,
    ) -> Result<ReconciledItem, BoxError> {
        let sha = format!("{:x}", Sha256::digest(&raw));
        let value: Value = match serde_json::from_slice::<Value>(&raw) {
            Ok(value) if value.is_object() => value,
            _ => {
                // The bytes are in hand: keep them. The reason never echoes record content.
                return Ok(raw_only(
                    raw,
                    sha,
                    String::new(),
                    serde_json::json!({"parse_failed": true}),
                    "Claude record is not a valid conversation object; raw record preserved",
                ));
            }
        };
        let conversation = value.as_object().expect("checked object");
        let id = conversation
            .get("uuid")
            .and_then(Value::as_str)
            .unwrap_or("");
        let messages = conversation.get("chat_messages").and_then(Value::as_array);
        // Any present, non-null link counts: a link of the wrong type makes the chain `Broken`
        // (raw-only) instead of being mistaken for "no links".
        let has_parent_links = messages.is_some_and(|list| {
            list.iter().any(|m| {
                m.get("parent_message_uuid")
                    .is_some_and(|link| !link.is_null())
            })
        });
        let project_uuid = conversation
            .get("project_uuid")
            .and_then(Value::as_str)
            .filter(|p| crate::store::uuid(p))
            .map(str::to_ascii_lowercase);
        let metadata = serde_json::json!({
            "message_count": messages.map_or(0, Vec::len),
            "has_parent_links": has_parent_links,
            "project_uuid": project_uuid,
        });
        let identifier = bounded_identifier(conversation.get("uuid"));
        let refuse = |raw: Vec<u8>, sha: String, reason: &str| {
            Ok(raw_only(
                raw,
                sha,
                identifier.clone(),
                metadata.clone(),
                reason,
            ))
        };

        if !crate::store::uuid(id) {
            return refuse(
                raw,
                sha,
                "Claude conversation uuid is missing or not a UUID; raw record preserved",
            );
        }
        let Some(messages) = messages.filter(|list| !list.is_empty()) else {
            return refuse(
                raw,
                sha,
                "Claude conversation has no chat_messages; raw record preserved",
            );
        };

        let mut warnings = Vec::new();
        let message_ids = match message_identities(messages) {
            Some(ids) => ids,
            None => return refuse(
                raw,
                sha,
                "A Claude message has a missing, oversized or duplicate uuid; raw record preserved",
            ),
        };
        // Order is proved only by a chain that starts at an explicit root.
        let mut order_proved = has_parent_links;
        if has_parent_links {
            match linear_chain(messages, &message_ids) {
                Chain::Rooted => {}
                Chain::Unrooted => {
                    order_proved = false;
                    warnings.push(
                        "The first Claude message points to a parent that is not in the export; earlier history may be missing"
                            .to_string(),
                    );
                }
                Chain::Broken => {
                    return refuse(
                        raw,
                        sha,
                        "Claude message parent links do not form one linear chain; the active branch cannot be chosen without guessing. Raw record preserved",
                    );
                }
            }
        } else {
            warnings.push(
                "Claude export carries no message parent links; branch structure is not proved"
                    .to_string(),
            );
        }

        let mut projected = Vec::new();
        let mut gaps = Vec::new();
        for (message, id) in messages.iter().zip(&message_ids) {
            match convert_message(message, id) {
                Converted::Represented(message, gap) => {
                    projected.push(message);
                    gaps.extend(gap);
                }
                Converted::Omitted(reason) => gaps.push(reason),
            }
        }
        if projected.is_empty() {
            return refuse(
                raw,
                sha,
                "Claude conversation has no message with representable text; raw record preserved",
            );
        }
        let completeness = if order_proved && gaps.is_empty() {
            "complete"
        } else {
            "partial"
        };
        record_completeness_gaps(&mut warnings, gaps);

        let title: String = conversation
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .unwrap_or("Claude Export Conversation")
            .chars()
            .take(120)
            .collect();
        let created = provider_time(conversation.get("created_at"));
        let updated = provider_time(conversation.get("updated_at"));
        let captured_at = match created.as_ref().or(updated.as_ref()) {
            Some(time) => time.clone(),
            None => {
                warnings.push(
                    "Missing conversation created_at and updated_at; using import processing time for capture timestamp".into(),
                );
                time::OffsetDateTime::now_utc()
                    .format(&time::format_description::well_known::Rfc3339)
                    .expect("valid UTC timestamp")
            }
        };

        let count = seen_ids.observe(id)?;
        let (conversation_id, source_url) = if count == 1 {
            let id = id.to_ascii_lowercase();
            let url = format!("https://claude.ai/chat/{id}");
            (id, url)
        } else {
            let disambiguated = crate::importers::disambiguated_uuid(id, count);
            warnings.push(
                "Duplicate conversation UUID encountered in export; disambiguated identity assigned"
                    .into(),
            );
            let url = format!("mindplace-import://claude-export/{disambiguated}");
            (disambiguated, url)
        };

        let mut capture = Capture {
            schema_version: 1,
            provider: PROVIDER.into(),
            conversation_id,
            source_url,
            title,
            conversation_created_at: created,
            conversation_updated_at: updated,
            captured_at,
            capture_method: "export-import".into(),
            completeness: completeness.into(),
            warnings,
            messages: projected,
        };
        let raw_evidence = RawEvidenceEnvelope {
            provider: PROVIDER.into(),
            source_identifier: identifier.clone(),
            envelope_domain: CONVERSATION_DOMAIN.into(),
            payload_bytes: raw,
            payload_sha256: sha,
            metadata_json: metadata.to_string(),
        };
        // The projection must satisfy the same contract as its write boundary.
        if let Err(error) = crate::validate_historical_capture(&mut capture, PROVIDER) {
            return Ok(ReconciledItem::PreservedRawOnly {
                evidence: raw_evidence,
                disposition_reason: format!(
                    "Claude projection rejected: {error}; raw record preserved"
                ),
                assets: vec![],
            });
        }
        Ok(ReconciledItem::Conversation {
            capture,
            raw_evidence,
            assets: vec![],
        })
    }
}

fn raw_len(item: &ReconciledItem) -> usize {
    match item {
        ReconciledItem::Conversation { raw_evidence, .. } => raw_evidence.payload_bytes.len(),
        ReconciledItem::PreservedRawOnly { evidence, .. } => evidence.payload_bytes.len(),
    }
}

fn raw_only(
    raw: Vec<u8>,
    sha: String,
    identifier: String,
    metadata: Value,
    reason: &str,
) -> ReconciledItem {
    ReconciledItem::PreservedRawOnly {
        evidence: RawEvidenceEnvelope {
            provider: PROVIDER.into(),
            source_identifier: identifier,
            envelope_domain: CONVERSATION_DOMAIN.into(),
            payload_bytes: raw,
            payload_sha256: sha,
            metadata_json: metadata.to_string(),
        },
        disposition_reason: reason.into(),
        assets: vec![],
    }
}

fn bounded_identifier(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .filter(|id| id.len() <= MAX_SOURCE_IDENTIFIER_BYTES)
        .unwrap_or("")
        .to_owned()
}

/// A provider timestamp is kept verbatim when it is a valid RFC 3339 value, otherwise dropped.
fn provider_time(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|time| crate::parse_rfc3339(time).is_some())
        .map(str::to_owned)
}

/// Every message needs its own non-empty, bounded, unique uuid. `None` refuses the record.
fn message_identities(messages: &[Value]) -> Option<Vec<String>> {
    let mut seen = HashSet::new();
    let mut ids = Vec::with_capacity(messages.len());
    for message in messages {
        let id = message.get("uuid").and_then(Value::as_str)?;
        if id.is_empty() || id.len() > 256 || !seen.insert(id.to_ascii_lowercase()) {
            return None;
        }
        ids.push(id.to_owned());
    }
    Some(ids)
}

/// What the parent links of one conversation prove.
#[derive(Debug, PartialEq, Eq)]
enum Chain {
    /// One chain in array order from an explicit root (absent, null, the root sentinel or the
    /// nil UUID): order and completeness of the history are proved.
    Rooted,
    /// One chain in array order, but the first message points to a parent that is not in the
    /// conversation (for example a truncated export): order is proved, the prefix is not.
    Unrooted,
    /// A fork, cycle, out-of-order link or non-string link: nothing is proved.
    Broken,
}

/// Parent links prove order only when they form exactly one chain in array order: every later
/// message's parent is the message just before it, and the first message's parent is not a
/// message of this conversation.
fn linear_chain(messages: &[Value], ids: &[String]) -> Chain {
    let known: HashSet<String> = ids.iter().map(|id| id.to_ascii_lowercase()).collect();
    let mut rooted = true;
    for (index, message) in messages.iter().enumerate() {
        let parent = match message.get("parent_message_uuid") {
            None | Some(Value::Null) => None,
            Some(Value::String(parent)) => Some(parent.to_ascii_lowercase()),
            Some(_) => return Chain::Broken,
        };
        if index == 0 {
            match parent {
                None => {}
                Some(parent) if parent == ROOT_PARENT_SENTINEL || parent == NIL_UUID => {}
                Some(parent) if known.contains(&parent) => return Chain::Broken,
                Some(_) => rooted = false,
            }
        } else if parent.as_deref() != Some(ids[index - 1].to_ascii_lowercase().as_str()) {
            return Chain::Broken;
        }
    }
    if rooted {
        Chain::Rooted
    } else {
        Chain::Unrooted
    }
}

enum Converted {
    /// Shown; the reason (if any) why the projection is not a faithful copy of the message.
    Represented(Message, Option<String>),
    /// Not shown: nothing representable. Always a completeness gap.
    Omitted(String),
}

fn convert_message(raw: &Value, id: &str) -> Converted {
    let mut gap: Option<String> = None;
    let role = match raw.get("sender").and_then(Value::as_str) {
        Some("human") => "user",
        Some("assistant") => "assistant",
        other => {
            gap.get_or_insert_with(|| {
                format!(
                    "Sender '{}' is not a known role and was shown as tool",
                    clip(other.unwrap_or("missing"))
                )
            });
            "tool"
        }
    };

    let mut text = String::new();
    let content = raw.get("content").and_then(Value::as_array);
    if raw
        .get("content")
        .is_some_and(|c| !c.is_null() && !c.is_array())
    {
        // A shape nobody has verified: never claim it was read.
        gap.get_or_insert_with(|| "Message content has an unexpected shape".to_string());
    }
    let mut text_blocks = 0usize;
    if let Some(blocks) = content {
        for block in blocks {
            let kind = block.get("type").and_then(Value::as_str);
            match (kind, block.get("text")) {
                (Some("text"), Some(Value::String(part))) => {
                    if !text.is_empty() && !part.is_empty() {
                        text.push_str("\n\n");
                    }
                    text.push_str(part);
                    text_blocks += 1;
                }
                (Some("text"), _) => {
                    gap.get_or_insert_with(|| "A text block has no readable text".to_string());
                }
                (Some(other), _) => {
                    gap.get_or_insert_with(|| {
                        format!(
                            "Content block type '{}' is not shown in the projection",
                            clip(other)
                        )
                    });
                }
                (None, _) => {
                    gap.get_or_insert_with(|| "A content block has no type".to_string());
                }
            }
        }
    }
    // The top-level `text` is used only when `content` is absent, null or an empty array. With
    // blocks present (for example only `thinking` or `tool_use`), or with `content` in a shape
    // nobody has verified, `text` is not proved to be the shown answer, so it is not projected;
    // the content is a completeness gap and stays in raw evidence.
    let has_blocks = raw
        .get("content")
        .is_some_and(|c| !c.is_null() && c.as_array().is_none_or(|blocks| !blocks.is_empty()));
    if text_blocks == 0 && !has_blocks {
        match raw.get("text") {
            Some(Value::String(plain)) => text.push_str(plain),
            _ => {
                gap.get_or_insert_with(|| {
                    "A message has no content; its absence is not proved to be genuine emptiness"
                        .to_string()
                });
            }
        }
    }

    let authored_at = match raw.get("created_at") {
        None | Some(Value::Null) => None,
        value => {
            let time = provider_time(value);
            if time.is_none() {
                gap.get_or_insert_with(|| "A message timestamp is not valid RFC 3339".to_string());
            }
            time
        }
    };

    let mut attachments = Vec::new();
    for (list_key, from_files) in [("attachments", false), ("files", true)] {
        let list = match raw.get(list_key) {
            None | Some(Value::Null) => continue,
            Some(Value::Array(list)) => list,
            Some(_) => {
                gap.get_or_insert_with(|| "A file list has an unexpected shape".to_string());
                continue;
            }
        };
        for (index, entry) in list.iter().enumerate() {
            if attachments.len() >= MAX_ATTACHMENTS_PER_MESSAGE {
                gap.get_or_insert_with(|| {
                    format!("More than {MAX_ATTACHMENTS_PER_MESSAGE} files on one message; the rest stay in raw evidence")
                });
                break;
            }
            match entry.as_object() {
                Some(file) => attachments.push(attachment(file, id, list_key, index, from_files)),
                None => {
                    gap.get_or_insert_with(|| "A file entry is not an object".to_string());
                }
            }
        }
    }

    if text.trim().is_empty() && attachments.is_empty() {
        return Converted::Omitted(gap.unwrap_or_else(|| {
            "A message has empty text and no files; it is not shown".to_string()
        }));
    }
    let format = if text.contains("```") || text.contains("**") || text.contains('#') {
        "markdown"
    } else {
        "plain"
    };
    Converted::Represented(
        Message {
            id: id.to_owned(),
            role: role.into(),
            text,
            format: format.into(),
            authored_at,
            // The Claude export does not name the model; never guess one.
            model: None,
            attachments,
        },
        gap,
    )
}

/// File metadata only. The export carries no file body (at most extracted text, which stays in
/// raw evidence), so the attachment is always marked unavailable.
fn attachment(
    file: &Map<String, Value>,
    message_id: &str,
    list_key: &str,
    index: usize,
    from_files: bool,
) -> Attachment {
    let name: String = file
        .get("file_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("attachment")
        .chars()
        .take(255)
        .collect();
    let id = file
        .get("file_uuid")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty() && id.len() <= 128)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{}:{list_key}:{index}", clip(message_id)));
    let declared = file.get("file_type").and_then(Value::as_str).unwrap_or("");
    let has_extracted = file
        .get("extracted_content")
        .and_then(Value::as_str)
        .is_some_and(|text| !text.is_empty());
    Attachment {
        id,
        mime_type: mime_for(declared, &name),
        name,
        source_url: None,
        data_url: None,
        asset_path: None,
        unavailable_reason: Some(if has_extracted && !from_files {
            "File body is not in the export; its extracted text is kept in raw evidence".into()
        } else {
            "File body is not in the export; file details are kept in raw evidence".into()
        }),
    }
}

fn mime_for(declared: &str, name: &str) -> String {
    let declared = declared.trim().to_ascii_lowercase();
    if declared.contains('/')
        && declared.len() <= 128
        && declared
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'+' | b'-'))
    {
        return declared;
    }
    let extension = if declared.is_empty() {
        name.rsplit_once('.')
            .map(|(_, ext)| ext.to_ascii_lowercase())
            .unwrap_or_default()
    } else {
        declared
    };
    match extension.as_str() {
        "txt" | "text" => "text/plain",
        "md" | "markdown" => "text/markdown",
        "csv" => "text/csv",
        "json" => "application/json",
        "pdf" => "application/pdf",
        "html" | "htm" => "text/html",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
    .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const CONV: &str = "1b2c3d4e-0000-4000-8000-00000000c001";
    const M1: &str = "1b2c3d4e-0000-4000-8000-0000000000a1";
    const M2: &str = "1b2c3d4e-0000-4000-8000-0000000000a2";
    const M3: &str = "1b2c3d4e-0000-4000-8000-0000000000a3";

    fn message(id: &str, sender: &str, parent: Option<&str>, content: Value) -> Value {
        let mut m = json!({
            "uuid": id, "sender": sender, "text": "",
            "created_at": "2026-01-02T03:04:05.123456Z", "updated_at": "2026-01-02T03:04:05Z",
            "content": content, "attachments": [], "files": []
        });
        if let Some(parent) = parent {
            m["parent_message_uuid"] = parent.into();
        }
        m
    }

    fn text(t: &str) -> Value {
        json!([{"type": "text", "text": t, "citations": []}])
    }

    fn conversation(messages: Vec<Value>) -> Value {
        json!({
            "uuid": CONV, "name": "Synthetic chat", "summary": "",
            "created_at": "2026-01-02T03:04:00Z", "updated_at": "2026-01-02T03:10:00Z",
            "account": {"uuid": "1b2c3d4e-0000-4000-8000-0000000000ff"},
            "chat_messages": messages
        })
    }

    fn items(records: &[Value]) -> Vec<ReconciledItem> {
        ClaudeExportParser::parse_conversations_json(
            serde_json::to_vec(&Value::Array(records.to_vec()))
                .unwrap()
                .as_slice(),
        )
        .unwrap()
    }

    fn projected(record: Value) -> Capture {
        match items(&[record]).remove(0) {
            ReconciledItem::Conversation { capture, .. } => capture,
            other => panic!("expected projection, got {other:?}"),
        }
    }

    fn refused(record: Value) -> String {
        match items(&[record]).remove(0) {
            ReconciledItem::PreservedRawOnly {
                disposition_reason,
                evidence,
                ..
            } => {
                assert_eq!(evidence.provider, "claude");
                assert_eq!(evidence.envelope_domain, CONVERSATION_DOMAIN);
                disposition_reason
            }
            other => panic!("expected raw-only, got {other:?}"),
        }
    }

    fn linked() -> Value {
        conversation(vec![
            message(M1, "human", Some(ROOT_PARENT_SENTINEL), text("Question?")),
            message(M2, "assistant", Some(M1), text("**Answer.**")),
        ])
    }

    #[test]
    fn linked_linear_conversation_is_complete_with_official_url_and_no_model() {
        let capture = projected(linked());
        assert_eq!(capture.provider, "claude");
        assert_eq!(capture.capture_method, "export-import");
        assert_eq!(capture.conversation_id, CONV);
        assert_eq!(capture.source_url, format!("https://claude.ai/chat/{CONV}"));
        assert_eq!(capture.completeness, "complete", "{:?}", capture.warnings);
        assert_eq!(capture.title, "Synthetic chat");
        assert_eq!(capture.captured_at, "2026-01-02T03:04:00Z");
        let roles: Vec<_> = capture.messages.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(roles, ["user", "assistant"]);
        assert_eq!(capture.messages[1].format, "markdown");
        assert!(capture.messages.iter().all(|m| m.model.is_none()));
        assert_eq!(
            capture.messages[0].authored_at.as_deref(),
            Some("2026-01-02T03:04:05.123456Z")
        );
    }

    #[test]
    fn raw_bytes_are_preserved_exactly_including_unknown_fields() {
        let record = r#"{"uuid":"1b2c3d4e-0000-4000-8000-00000000c001","name":"x","chat_messages":[{"uuid":"m1","sender":"human","text":"hi"}],"future_field":{"canary":"must-preserve"}}"#;
        let parsed =
            ClaudeExportParser::parse_conversations_json(format!("[ {record} ]").as_bytes())
                .unwrap();
        let ReconciledItem::Conversation { raw_evidence, .. } = &parsed[0] else {
            panic!("expected projection");
        };
        assert_eq!(raw_evidence.payload_bytes, record.as_bytes());
        assert_eq!(
            raw_evidence.payload_sha256,
            format!("{:x}", Sha256::digest(record.as_bytes()))
        );
        assert_eq!(raw_evidence.source_identifier, CONV);
    }

    #[test]
    fn without_parent_links_order_is_unproved_and_the_projection_is_partial() {
        let capture = projected(conversation(vec![
            message(M1, "human", None, text("a")),
            message(M2, "assistant", None, text("b")),
        ]));
        assert_eq!(capture.completeness, "partial");
        assert!(capture
            .warnings
            .iter()
            .any(|w| w.contains("no message parent links")));
    }

    #[test]
    fn forks_cycles_and_out_of_order_links_stay_raw_only() {
        for (label, messages) in [
            (
                "fork",
                vec![
                    message(M1, "human", None, text("a")),
                    message(M2, "assistant", Some(M1), text("b")),
                    message(M3, "assistant", Some(M1), text("c")),
                ],
            ),
            (
                "out of order",
                vec![
                    message(M1, "human", Some(M2), text("a")),
                    message(M2, "assistant", None, text("b")),
                ],
            ),
            (
                "cycle",
                vec![
                    message(M1, "human", Some(M2), text("a")),
                    message(M2, "assistant", Some(M1), text("b")),
                ],
            ),
            (
                "partial links",
                vec![
                    message(M1, "human", None, text("a")),
                    message(M2, "assistant", None, text("b")),
                    message(M3, "human", Some(M2), text("c")),
                ],
            ),
        ] {
            let reason = refused(conversation(messages));
            assert!(reason.contains("one linear chain"), "{label}: {reason}");
        }
    }

    #[test]
    fn a_first_parent_outside_the_export_is_not_a_proved_root() {
        for parent in ["1b2c3d4e-0000-4000-8000-0000000000ff", "", "not-a-uuid"] {
            let capture = projected(conversation(vec![
                message(M1, "human", Some(parent), text("a")),
                message(M2, "assistant", Some(M1), text("b")),
            ]));
            assert_eq!(capture.completeness, "partial", "parent {parent:?}");
            assert!(
                capture
                    .warnings
                    .iter()
                    .any(|w| w.contains("earlier history may be missing")),
                "{:?}",
                capture.warnings
            );
        }
        for root in [ROOT_PARENT_SENTINEL, NIL_UUID] {
            let capture = projected(conversation(vec![
                message(M1, "human", Some(root), text("a")),
                message(M2, "assistant", Some(M1), text("b")),
            ]));
            assert_eq!(capture.completeness, "complete", "root {root}");
        }
        let mut null_root = message(M1, "human", None, text("a"));
        null_root["parent_message_uuid"] = Value::Null;
        let capture = projected(conversation(vec![
            null_root,
            message(M2, "assistant", Some(M1), text("b")),
        ]));
        assert_eq!(capture.completeness, "complete");
    }

    #[test]
    fn unexpected_content_or_file_list_shapes_are_gaps_never_complete() {
        for (field, value) in [
            ("content", json!({"type": "text", "text": "x"})),
            ("content", json!("plain")),
            ("attachments", json!({"file_name": "a.txt"})),
            ("files", json!("a.png")),
        ] {
            let mut odd = message(M2, "assistant", Some(M1), text("answer"));
            odd[field] = value;
            odd["text"] = "answer".into();
            let capture = projected(conversation(vec![
                message(M1, "human", Some(ROOT_PARENT_SENTINEL), text("q")),
                odd,
            ]));
            assert_eq!(capture.completeness, "partial", "{field}");
            assert!(
                capture
                    .warnings
                    .iter()
                    .any(|w| w.contains("unexpected shape")),
                "{field}: {:?}",
                capture.warnings
            );
            if field == "content" {
                // An unverified `content` shape never lets the top-level `text` stand in for it:
                // the message has nothing representable and is left out of the projection.
                assert_eq!(capture.messages.len(), 1, "{field}");
                assert!(
                    capture.messages.iter().all(|m| m.text != "answer"),
                    "{field}"
                );
            }
        }
    }

    #[test]
    fn non_string_parent_links_are_refused_not_read_as_missing() {
        for link in [json!(7), json!({"uuid": M1}), json!(true)] {
            let mut second = message(M2, "assistant", None, text("b"));
            second["parent_message_uuid"] = link.clone();
            let reason = refused(conversation(vec![
                message(M1, "human", None, text("a")),
                second,
            ]));
            assert!(reason.contains("one linear chain"), "{link}: {reason}");
        }
    }

    #[test]
    fn identity_problems_stay_raw_only() {
        let mut bad_id = linked();
        bad_id["uuid"] = "not-a-uuid".into();
        assert!(refused(bad_id).contains("not a UUID"));
        let mut empty = linked();
        empty["chat_messages"] = json!([]);
        assert!(refused(empty).contains("no chat_messages"));
        let duplicate = conversation(vec![
            message(M1, "human", None, text("a")),
            message(M1, "assistant", None, text("b")),
        ]);
        assert!(refused(duplicate).contains("duplicate uuid"));
        assert!(refused(json!([1, 2])).contains("not a valid conversation object"));
    }

    #[test]
    fn thinking_and_tool_blocks_are_not_shown_and_lower_completeness() {
        let capture = projected(conversation(vec![
            message(M1, "human", None, text("q")),
            message(
                M2,
                "assistant",
                Some(M1),
                json!([
                    {"type": "thinking", "thinking": "hidden reasoning canary"},
                    {"type": "tool_use", "name": "web_search", "input": {"q": "x"}},
                    {"type": "tool_result", "content": [{"type": "text", "text": "r"}]},
                    {"type": "text", "text": "visible answer"}
                ]),
            ),
        ]));
        assert_eq!(capture.completeness, "partial");
        assert_eq!(capture.messages[1].text, "visible answer");
        assert!(!capture
            .messages
            .iter()
            .any(|m| m.text.contains("hidden reasoning")));
        assert!(capture
            .warnings
            .iter()
            .any(|w| w.starts_with("Completeness not proved: Content block type 'thinking'")));
    }

    #[test]
    fn top_level_text_is_not_shown_when_only_non_text_blocks_exist() {
        let mut only_tools = message(
            M2,
            "assistant",
            Some(M1),
            json!([{"type": "thinking", "thinking": "t"}, {"type": "tool_use", "name": "x", "input": {}}]),
        );
        only_tools["text"] = "placeholder canary".into();
        let capture = projected(conversation(vec![
            message(M1, "human", Some(ROOT_PARENT_SENTINEL), text("q")),
            only_tools,
        ]));
        assert_eq!(capture.completeness, "partial");
        assert_eq!(
            capture.messages.len(),
            1,
            "the tools-only message is not shown"
        );
        assert!(!serde_json::to_string(&capture)
            .unwrap()
            .contains("placeholder canary"));
    }

    #[test]
    fn legacy_text_field_is_used_when_content_is_absent() {
        let mut first = message(M1, "human", Some(ROOT_PARENT_SENTINEL), Value::Null);
        first.as_object_mut().unwrap().remove("content");
        first["text"] = "plain question".into();
        let capture = projected(conversation(vec![
            first,
            message(M2, "assistant", Some(M1), text("a")),
        ]));
        assert_eq!(capture.messages[0].text, "plain question");
        assert_eq!(capture.completeness, "complete");
    }

    #[test]
    fn attachments_are_metadata_only_and_never_carry_bodies() {
        let mut first = message(M1, "human", Some(ROOT_PARENT_SENTINEL), text("see file"));
        first["attachments"] = json!([{
            "file_name": "notes.txt", "file_size": 12, "file_type": "txt",
            "extracted_content": "secret extracted canary"
        }]);
        first["files"] = json!([{"file_name": "photo.png", "file_uuid": "f-123"}]);
        let capture = projected(conversation(vec![
            first,
            message(M2, "assistant", Some(M1), text("ok")),
        ]));
        let files = &capture.messages[0].attachments;
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].name, "notes.txt");
        assert_eq!(files[0].mime_type, "text/plain");
        assert_eq!(files[1].id, "f-123");
        assert_eq!(files[1].mime_type, "image/png");
        for file in files {
            assert!(file.data_url.is_none() && file.asset_path.is_none());
            assert!(file.unavailable_reason.is_some());
        }
        let shown = serde_json::to_string(&capture).unwrap();
        assert!(!shown.contains("secret extracted canary"));
        assert_eq!(capture.completeness, "complete");
    }

    #[test]
    fn unknown_sender_invalid_message_time_and_missing_content_are_gaps() {
        let mut odd = message(M2, "system", Some(M1), text("x"));
        odd["created_at"] = "yesterday".into();
        let capture = projected(conversation(vec![
            message(M1, "human", Some(ROOT_PARENT_SENTINEL), text("q")),
            odd,
        ]));
        assert_eq!(capture.completeness, "partial");
        assert_eq!(capture.messages[1].role, "tool");
        assert!(capture.messages[1].authored_at.is_none());
        let mut none = message(M2, "assistant", Some(M1), Value::Null);
        none.as_object_mut().unwrap().remove("content");
        none.as_object_mut().unwrap().remove("text");
        let capture = projected(conversation(vec![
            message(M1, "human", Some(ROOT_PARENT_SENTINEL), text("q")),
            none,
        ]));
        assert_eq!(capture.completeness, "partial");
        assert_eq!(capture.messages.len(), 1);
        assert!(capture
            .warnings
            .iter()
            .any(|w| w.contains("A message has no content")));
    }

    #[test]
    fn missing_conversation_times_are_not_invented() {
        let mut record = linked();
        record.as_object_mut().unwrap().remove("created_at");
        record["updated_at"] = "not a time".into();
        let capture = projected(record);
        assert!(capture.conversation_created_at.is_none());
        assert!(capture.conversation_updated_at.is_none());
        assert!(capture
            .warnings
            .iter()
            .any(|w| w.contains("Missing conversation created_at")));
    }

    #[test]
    fn duplicate_conversations_get_distinct_identities() {
        let parsed = items(&[linked(), linked()]);
        let ids: Vec<_> = parsed
            .iter()
            .map(|item| match item {
                ReconciledItem::Conversation { capture, .. } => {
                    (capture.conversation_id.clone(), capture.source_url.clone())
                }
                other => panic!("{other:?}"),
            })
            .collect();
        assert_ne!(ids[0].0, ids[1].0);
        assert!(ids[1].1.starts_with("mindplace-import://claude-export/"));
    }

    #[test]
    fn projects_are_always_raw_only_with_their_exact_bytes() {
        let record = r#"{"uuid":"1b2c3d4e-0000-4000-8000-0000000000b1","name":"P","docs":[{"filename":"a.md","content":"doc canary"}],"prompt_template":"be brief"}"#;
        let parsed =
            ClaudeExportParser::parse_projects_json(format!("[{record}, 7]").as_bytes()).unwrap();
        assert_eq!(parsed.len(), 2);
        for (index, item) in parsed.iter().enumerate() {
            let ReconciledItem::PreservedRawOnly {
                evidence,
                disposition_reason,
                ..
            } = item
            else {
                panic!("projects are never projected");
            };
            assert_eq!(evidence.envelope_domain, PROJECT_DOMAIN);
            assert!(!evidence.metadata_json.contains("canary"));
            if index == 0 {
                assert_eq!(evidence.payload_bytes, record.as_bytes());
                assert!(disposition_reason.contains("projects are not conversations"));
                assert!(evidence.metadata_json.contains("\"doc_count\":1"));
            } else {
                assert!(disposition_reason.contains("not a JSON object"));
            }
        }
    }

    #[test]
    fn oversized_records_are_refused_while_streaming() {
        let limits = BoundedArrayLimits {
            max_record_bytes: 64,
            max_separator_bytes: 16,
        };
        let big = serde_json::to_vec(&json!([linked()])).unwrap();
        let mut tracker = ConversationIdTracker::default();
        let error = ClaudeExportParser::stream_conversations_json_tracked(
            big.as_slice(),
            limits,
            &mut tracker,
            |_, _| Ok(()),
        )
        .unwrap_err();
        assert!(error.to_string().contains("byte limit"));
    }

    #[test]
    fn mime_types_come_from_declared_type_or_extension() {
        assert_eq!(mime_for("application/pdf", "x"), "application/pdf");
        assert_eq!(mime_for("pdf", "x"), "application/pdf");
        assert_eq!(mime_for("", "Report.MD"), "text/markdown");
        assert_eq!(mime_for("weird type!", "x"), "application/octet-stream");
        assert_eq!(mime_for("", "no-extension"), "application/octet-stream");
    }
}

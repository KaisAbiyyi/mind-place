use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::Read;

use crate::importers::{
    stream_bounded_top_level_array_spanned, BoundedArrayLimits, RawEvidenceEnvelope,
    ReconciledItem, RecordSpan,
};
use crate::{Attachment, Capture, Message};

/// Largest accepted `conversations.json` record. Enforced while the stream is read, before
/// the record is buffered or parsed.
pub const MAX_CHATGPT_RECORD_BYTES: usize = 64 * 1024 * 1024; // 64 MiB
/// Longest run of whitespace tolerated between records of the export array.
pub const MAX_CHATGPT_SEPARATOR_BYTES: usize = 1024 * 1024; // 1 MiB
/// Most distinct conversation IDs tracked for duplicate disambiguation in one stream.
/// Reaching it aborts the stream: uniqueness can no longer be guaranteed.
pub const MAX_TRACKED_CONVERSATION_IDS: usize = 250_000;
/// Longest conversation ID that is tracked and projected; longer IDs stay raw-only.
pub const MAX_CONVERSATION_ID_BYTES: usize = 256;
/// Most records `parse_conversations_json` will collect in memory.
pub const MAX_ITEMS_IN_MEMORY: usize = 50_000;
/// Most raw evidence bytes `parse_conversations_json` will hold in memory at once.
pub const MAX_COLLECTED_RAW_BYTES: usize = 256 * 1024 * 1024; // 256 MiB
/// Most distinct completeness warnings recorded for one conversation.
const MAX_COMPLETENESS_WARNINGS: usize = 4;

const STREAM_LIMITS: BoundedArrayLimits = BoundedArrayLimits {
    max_record_bytes: MAX_CHATGPT_RECORD_BYTES,
    max_separator_bytes: MAX_CHATGPT_SEPARATOR_BYTES,
};

/// Tracks how many times each conversation ID occurred (case-insensitively, because the stored
/// identity is lowercased), with a hard bound on memory. One tracker should span every member of
/// an export so duplicates across `conversations-*.json` shards are disambiguated too.
pub struct ConversationIdTracker {
    seen: HashMap<String, usize>,
    limit: usize,
}

impl Default for ConversationIdTracker {
    fn default() -> Self {
        Self::with_limit(MAX_TRACKED_CONVERSATION_IDS)
    }
}

impl ConversationIdTracker {
    pub fn with_limit(limit: usize) -> Self {
        Self {
            seen: HashMap::new(),
            limit,
        }
    }

    fn observe(&mut self, id: &str) -> Result<usize, Box<dyn std::error::Error>> {
        let id = &id.to_ascii_lowercase();
        if let Some(count) = self.seen.get_mut(id) {
            *count += 1;
            return Ok(*count);
        }
        if self.seen.len() >= self.limit {
            return Err(format!(
                "ChatGPT export exceeds {} distinct conversation IDs",
                self.limit
            )
            .into());
        }
        self.seen.insert(id.to_owned(), 1);
        Ok(1)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChatGptExportConversation {
    pub id: String,
    pub title: Option<String>,
    pub create_time: Option<f64>,
    pub update_time: Option<f64>,
    pub current_node: Option<String>,
    #[serde(default)]
    pub mapping: HashMap<String, ChatGptNode>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChatGptNode {
    pub id: String,
    pub message: Option<ChatGptMessage>,
    pub parent: Option<String>,
    #[serde(default)]
    pub children: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChatGptMessage {
    pub id: String,
    pub author: ChatGptAuthor,
    pub create_time: Option<f64>,
    pub update_time: Option<f64>,
    pub content: Option<ChatGptContent>,
    pub status: Option<String>,
    pub end_turn: Option<bool>,
    pub weight: Option<f64>,
    pub metadata: Option<serde_json::Value>,
    pub recipient: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChatGptAuthor {
    pub role: String,
    pub name: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChatGptContent {
    pub content_type: String,
    pub parts: Option<Vec<serde_json::Value>>,
    pub text: Option<String>,
}

pub struct ChatGptExportParser;

impl ChatGptExportParser {
    pub fn stream_conversations_json<
        R: Read,
        F: FnMut(ReconciledItem) -> Result<(), Box<dyn std::error::Error>>,
    >(
        reader: R,
        mut callback: F,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        Self::stream_conversations_json_spanned(reader, STREAM_LIMITS, |_span, item| callback(item))
    }

    /// Streams every record together with the span (offset, length, SHA-256) of the exact source
    /// bytes it was cut from, under caller-supplied limits. The span is measured by the stream
    /// layer from the sliced bytes, so it is independent of what the parser reports.
    pub fn stream_conversations_json_spanned<
        R: Read,
        F: FnMut(RecordSpan, ReconciledItem) -> Result<(), Box<dyn std::error::Error>>,
    >(
        reader: R,
        limits: BoundedArrayLimits,
        callback: F,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let mut seen_ids = ConversationIdTracker::default();
        Self::stream_conversations_json_tracked(reader, limits, &mut seen_ids, callback)
    }

    /// As [`Self::stream_conversations_json_spanned`] with a caller-owned tracker, so duplicate
    /// conversation IDs are disambiguated across several members of one export. A record that is
    /// not a valid conversation object is kept as raw-only with a generic reason.
    pub fn stream_conversations_json_tracked<
        R: Read,
        F: FnMut(RecordSpan, ReconciledItem) -> Result<(), Box<dyn std::error::Error>>,
    >(
        reader: R,
        limits: BoundedArrayLimits,
        seen_ids: &mut ConversationIdTracker,
        mut callback: F,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        stream_bounded_top_level_array_spanned(reader, limits, |offset, record| {
            let span = RecordSpan::of(offset, record);
            let item = Self::reconcile_conversation_raw(record.to_vec(), seen_ids)?;
            callback(span, item)
        })
    }

    pub fn parse_conversations_json<R: Read>(
        reader: R,
    ) -> Result<Vec<ReconciledItem>, Box<dyn std::error::Error>> {
        let mut items = Vec::new();
        let mut collected_bytes = 0usize;
        Self::stream_conversations_json(reader, |item| {
            let raw_len = match &item {
                ReconciledItem::Conversation { raw_evidence, .. } => {
                    raw_evidence.payload_bytes.len()
                }
                ReconciledItem::PreservedRawOnly { evidence, .. } => evidence.payload_bytes.len(),
            };
            collected_bytes = collected_bytes.saturating_add(raw_len);
            if items.len() >= MAX_ITEMS_IN_MEMORY || collected_bytes > MAX_COLLECTED_RAW_BYTES {
                return Err(
                    "ChatGPT export exceeds in-memory collection limit; use stream_conversations_json".into(),
                );
            }
            items.push(item);
            Ok(())
        })?;
        Ok(items)
    }

    pub fn reconcile_conversation(
        conv: ChatGptExportConversation,
    ) -> Result<ReconciledItem, Box<dyn std::error::Error>> {
        let raw_bytes = serde_json::to_vec(&conv)?;
        if raw_bytes.len() > MAX_CHATGPT_RECORD_BYTES {
            return Err(
                format!("Import record exceeds the {MAX_CHATGPT_RECORD_BYTES} byte limit").into(),
            );
        }
        let mut seen_ids = ConversationIdTracker::default();
        Self::reconcile_conversation_raw(raw_bytes, &mut seen_ids)
    }

    fn reconcile_conversation_raw(
        raw_bytes: Vec<u8>,
        seen_ids: &mut ConversationIdTracker,
    ) -> Result<ReconciledItem, Box<dyn std::error::Error>> {
        let raw_sha256 = format!("{:x}", Sha256::digest(&raw_bytes));
        let conv: ChatGptExportConversation = match serde_json::from_slice(&raw_bytes) {
            Ok(conv) => conv,
            Err(_) => {
                // The bytes are in hand: keep them. The reason never echoes record content.
                return Ok(ReconciledItem::PreservedRawOnly {
                    evidence: RawEvidenceEnvelope {
                        provider: "chatgpt".into(),
                        source_identifier: String::new(),
                        envelope_domain: "chatgpt-export-dag".into(),
                        payload_bytes: raw_bytes,
                        payload_sha256: raw_sha256,
                        metadata_json: serde_json::json!({"parse_failed": true}).to_string(),
                    },
                    disposition_reason:
                        "ChatGPT record is not a valid conversation object; raw record preserved"
                            .into(),
                    assets: vec![],
                });
            }
        };

        let raw_evidence = RawEvidenceEnvelope {
            provider: "chatgpt".into(),
            source_identifier: conv.id.clone(),
            envelope_domain: "chatgpt-export-dag".into(),
            payload_bytes: raw_bytes,
            payload_sha256: raw_sha256,
            metadata_json: serde_json::json!({
                "original_node_count": conv.mapping.len(),
                "has_current_node": conv.current_node.is_some(),
            })
            .to_string(),
        };

        // An unbounded or empty identity is never tracked or projected; the raw record stays.
        if conv.id.is_empty() || conv.id.len() > MAX_CONVERSATION_ID_BYTES {
            return Ok(ReconciledItem::PreservedRawOnly {
                evidence: raw_evidence,
                disposition_reason: format!(
                    "ChatGPT conversation id is empty or longer than {MAX_CONVERSATION_ID_BYTES} bytes; raw record preserved"
                ),
                assets: vec![],
            });
        }

        let mut warnings = Vec::new();

        // 1. Authoritative branch projection
        let branch_result = Self::project_authoritative_branch(&conv, &mut warnings);

        let (messages, completeness) = match branch_result {
            BranchProjection::CompleteActiveBranch(msgs) => (msgs, "complete".to_string()),
            BranchProjection::PartialUnverified(msgs) => (msgs, "partial".to_string()),
            BranchProjection::BranchingRefused(reason) => {
                return Ok(ReconciledItem::PreservedRawOnly {
                    evidence: raw_evidence,
                    disposition_reason: reason,
                    assets: vec![],
                });
            }
            BranchProjection::Empty => {
                return Ok(ReconciledItem::PreservedRawOnly {
                    evidence: raw_evidence,
                    disposition_reason:
                        "ChatGPT export conversation contained zero valid message nodes".into(),
                    assets: vec![],
                });
            }
        };

        let title = conv
            .title
            .as_deref()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .unwrap_or("ChatGPT Export Conversation")
            .chars()
            .take(120)
            .collect();

        // 2. Honest timestamp handling
        let conversation_created_at = conv.create_time.and_then(timestamp_to_rfc3339);
        let conversation_updated_at = conv.update_time.and_then(timestamp_to_rfc3339);

        let captured_at = if let Some(ref ts) = conversation_created_at {
            ts.clone()
        } else if let Some(ref ts) = conversation_updated_at {
            ts.clone()
        } else {
            warnings.push(
                "Missing conversation create_time and update_time; using import processing time for capture timestamp".into(),
            );
            chrono::Utc::now().to_rfc3339()
        };

        // 3. Collision-resistant duplicate conversation ID handling
        let count = seen_ids.observe(&conv.id)?;
        let (conversation_id, source_url) = if count == 1 {
            (
                conv.id.to_ascii_lowercase(),
                format!("https://chatgpt.com/c/{}", conv.id),
            )
        } else {
            // Generate deterministic UUIDv4 for duplicate occurrence to prevent collision
            let hash = Sha256::digest(format!("{}:dup:{}", conv.id, count).as_bytes());
            let mut uuid_bytes = [0u8; 16];
            uuid_bytes.copy_from_slice(&hash[..16]);
            uuid_bytes[6] = (uuid_bytes[6] & 0x0f) | 0x40; // Version 4
            uuid_bytes[8] = (uuid_bytes[8] & 0x3f) | 0x80; // Variant 1
            let disambiguated = format!(
                "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
                uuid_bytes[0], uuid_bytes[1], uuid_bytes[2], uuid_bytes[3],
                uuid_bytes[4], uuid_bytes[5],
                uuid_bytes[6], uuid_bytes[7],
                uuid_bytes[8], uuid_bytes[9],
                uuid_bytes[10], uuid_bytes[11], uuid_bytes[12], uuid_bytes[13], uuid_bytes[14], uuid_bytes[15]
            );
            warnings.push(
                "Duplicate conversation UUID encountered in export; disambiguated identity assigned"
                    .into(),
            );
            (
                disambiguated.clone(),
                format!("mindplace-import://chatgpt-export/{disambiguated}"),
            )
        };

        let mut capture = Capture {
            schema_version: 1,
            provider: "chatgpt".into(),
            conversation_id,
            source_url,
            title,
            conversation_created_at,
            conversation_updated_at,
            captured_at,
            capture_method: "export-import".into(),
            completeness,
            warnings,
            messages,
        };

        // A reader projection must satisfy the same contract as its write boundary.
        // Conflicting message IDs (and other invalid metadata) remain lossless raw evidence.
        if let Err(error) = crate::validate_historical_capture(&mut capture, "chatgpt") {
            return Ok(ReconciledItem::PreservedRawOnly {
                evidence: raw_evidence,
                disposition_reason: format!(
                    "ChatGPT projection rejected: {error}; raw DAG preserved"
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

    fn project_authoritative_branch(
        conv: &ChatGptExportConversation,
        warnings: &mut Vec<String>,
    ) -> BranchProjection {
        if conv.mapping.is_empty() {
            return BranchProjection::Empty;
        }

        if !Self::valid_graph(conv) {
            return BranchProjection::BranchingRefused(
                "Inconsistent ChatGPT DAG: node identities, reciprocal parent/child links or root connectivity invalid; raw DAG preserved".into(),
            );
        }

        // Case A: valid current_node chain
        if let Some(curr_id) = &conv.current_node {
            if conv.mapping.contains_key(curr_id) {
                let mut path_nodes = Vec::new();
                let mut visited = HashSet::new();
                let mut curr = Some(curr_id.clone());
                let mut cyclic = false;
                let mut orphaned = false;

                while let Some(node_id) = curr {
                    if !visited.insert(node_id.clone()) {
                        cyclic = true;
                        break;
                    }
                    if let Some(node) = conv.mapping.get(&node_id) {
                        if let Some(msg) = &node.message {
                            path_nodes.push(msg);
                        }
                        curr = node.parent.clone();
                    } else {
                        orphaned = true;
                        break;
                    }
                }

                if !cyclic && !orphaned && !path_nodes.is_empty() {
                    path_nodes.reverse(); // Chronological root -> leaf
                    let mut projected = ProjectedBranch::default();
                    for msg in path_nodes {
                        projected.push(Self::convert_message(msg));
                    }

                    if visited.len() < conv.mapping.len() {
                        warnings.push(
                            "Conversation contains alternate branches preserved in raw evidence"
                                .into(),
                        );
                    }

                    // Complete only when every node on the active branch is either represented or
                    // provably empty, and every content part is represented; otherwise partial.
                    let ProjectedBranch {
                        messages: ordered_messages,
                        gaps,
                        omitted_empty,
                    } = projected;
                    note_omitted_empty(warnings, omitted_empty);
                    if gaps.is_empty() {
                        return BranchProjection::CompleteActiveBranch(ordered_messages);
                    }
                    record_completeness_gaps(warnings, gaps);
                    return BranchProjection::PartialUnverified(ordered_messages);
                }
            }
        }

        // Case B: missing or invalid current_node
        // Check if graph is strictly linear (single root, every node has <= 1 child)
        let roots: Vec<_> = conv
            .mapping
            .values()
            .filter(|n| n.parent.is_none())
            .collect();

        let is_strictly_linear =
            roots.len() == 1 && conv.mapping.values().all(|n| n.children.len() <= 1);

        if is_strictly_linear {
            let mut curr = Some(roots[0]);
            let mut visited = HashSet::new();
            let mut projected = ProjectedBranch::default();

            while let Some(node) = curr {
                if !visited.insert(node.id.clone()) {
                    break;
                }
                if let Some(msg) = &node.message {
                    projected.push(Self::convert_message(msg));
                }
                curr = node
                    .children
                    .first()
                    .and_then(|c_id| conv.mapping.get(c_id));
            }

            let ProjectedBranch {
                messages: ordered_messages,
                gaps,
                omitted_empty,
            } = projected;
            if ordered_messages.is_empty() {
                return BranchProjection::Empty;
            }

            warnings.push("current_node pointer missing from export; projection unverified".into());
            note_omitted_empty(warnings, omitted_empty);
            record_completeness_gaps(warnings, gaps);
            return BranchProjection::PartialUnverified(ordered_messages);
        }

        // Multiple roots or branching children without authoritative current_node
        BranchProjection::BranchingRefused(
            "Missing or invalid current_node pointer in branching conversation DAG; active branch cannot be authoritatively determined without guessing. Raw DAG preserved.".into(),
        )
    }

    fn valid_graph(conv: &ChatGptExportConversation) -> bool {
        let mut roots = Vec::new();
        let mut linked_children = HashSet::new();
        for (id, node) in &conv.mapping {
            if id != &node.id {
                return false;
            }
            if let Some(parent_id) = &node.parent {
                if !conv.mapping.contains_key(parent_id) {
                    return false;
                }
            } else {
                roots.push(id);
            }
            for child_id in &node.children {
                if !linked_children.insert(child_id)
                    || !conv
                        .mapping
                        .get(child_id)
                        .is_some_and(|child| child.parent.as_ref() == Some(id))
                {
                    return false;
                }
            }
        }
        if roots.len() != 1 {
            return false;
        }
        if conv
            .mapping
            .iter()
            .any(|(id, node)| node.parent.is_some() && !linked_children.contains(id))
        {
            return false;
        }
        let mut pending = roots;
        let mut visited = HashSet::new();
        while let Some(id) = pending.pop() {
            if !visited.insert(id) {
                return false;
            }
            pending.extend(&conv.mapping[id].children);
        }
        visited.len() == conv.mapping.len()
    }

    /// Projects one export message. A represented message carries the reason, if any, why the
    /// projection is not a faithful representation of every content part. A node that is not
    /// represented is never silent: it is either provably empty (counted and noted) or a
    /// completeness gap. The raw record always stays authoritative; this only decides whether
    /// the capture may be `complete`.
    fn convert_message(raw: &ChatGptMessage) -> Converted {
        let mut gap: Option<String> = None;
        let role = match raw.author.role.as_str() {
            "user" => "user",
            "assistant" => "assistant",
            "system" => "system",
            "tool" => "tool",
            other => {
                gap.get_or_insert_with(|| {
                    format!(
                        "Author role '{}' is not a known role and was shown as tool",
                        clip(other)
                    )
                });
                "tool"
            }
        };

        let mut text = String::new();
        let mut attachments = Vec::new();

        if let Some(content) = &raw.content {
            if !matches!(content.content_type.as_str(), "text" | "multimodal_text") {
                gap.get_or_insert_with(|| {
                    format!(
                        "Content type '{}' is not fully represented in the projection",
                        clip(&content.content_type)
                    )
                });
            }
            if content.parts.is_some() && content.text.is_some() {
                gap.get_or_insert_with(|| {
                    "Message has both parts and text; only parts are projected".to_string()
                });
            }
            if let Some(parts) = &content.parts {
                for part in parts {
                    match part {
                        serde_json::Value::String(s) => {
                            text.push_str(s);
                        }
                        serde_json::Value::Object(map) => {
                            // Multimodal image/asset attachment pointer
                            if map.get("content_type")
                                == Some(&serde_json::json!("image_asset_pointer"))
                                || map.contains_key("asset_pointer")
                            {
                                let pointer = map.get("asset_pointer").and_then(|v| v.as_str());
                                if pointer.is_none() {
                                    gap.get_or_insert_with(|| {
                                        "Asset part has no readable asset pointer".to_string()
                                    });
                                }
                                let pointer = pointer.unwrap_or("unknown-asset");
                                attachments.push(Attachment {
                                    id: pointer.to_string(),
                                    name: "image_asset".into(),
                                    mime_type: "image/*".into(),
                                    source_url: Some(pointer.to_string()),
                                    data_url: None,
                                    asset_path: None,
                                    unavailable_reason: Some(
                                        "External asset pointer preserved without fetching remote media"
                                            .into(),
                                    ),
                                });
                            } else if let Some(serde_json::Value::String(s)) = map.get("text") {
                                gap.get_or_insert_with(|| {
                                    "A structured content part was reduced to its text".to_string()
                                });
                                text.push_str(s);
                            } else {
                                gap.get_or_insert_with(|| {
                                    "A structured content part was projected as raw JSON text"
                                        .to_string()
                                });
                                text.push_str(&part.to_string());
                            }
                        }
                        _ => {
                            gap.get_or_insert_with(|| {
                                "A non-text content part was projected as raw JSON text".to_string()
                            });
                            text.push_str(&part.to_string());
                        }
                    }
                }
            } else if let Some(t) = &content.text {
                text.push_str(t);
            }
        } else {
            // Absent content cannot be told apart from content the export dropped.
            gap.get_or_insert_with(|| {
                "A message has no content; its absence is not proved to be genuine emptiness"
                    .to_string()
            });
        }

        if text.trim().is_empty() && attachments.is_empty() && role == "system" {
            // Not represented. With a known content shape and nothing in it the node carries
            // no content (provably empty); anything else is an explicit completeness gap.
            return match gap {
                None => Converted::OmittedEmpty,
                Some(reason) => Converted::Omitted(reason),
            };
        }

        let format = if text.contains("```") || text.contains("**") || text.contains('#') {
            "markdown".to_string()
        } else {
            "plain".to_string()
        };

        Converted::Represented(
            Message {
                id: raw.id.clone(),
                role: role.to_string(),
                text,
                format,
                authored_at: raw.create_time.and_then(timestamp_to_rfc3339),
                model: raw
                    .metadata
                    .as_ref()
                    .and_then(|m| m.get("model_slug"))
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string()),
                attachments,
            },
            gap,
        )
    }
}

/// Outcome of projecting one export message.
enum Converted {
    /// Shown in the projection; the reason (if any) the projection is not faithful.
    Represented(Message, Option<String>),
    /// Not shown, and provably empty: known content shape, no text, no parts, no assets.
    OmittedEmpty,
    /// Not shown, and not provably empty: a completeness gap.
    Omitted(String),
}

/// Messages of the active branch plus everything that stopped a node from being faithfully shown.
#[derive(Default)]
struct ProjectedBranch {
    messages: Vec<Message>,
    gaps: Vec<String>,
    omitted_empty: usize,
}

impl ProjectedBranch {
    fn push(&mut self, converted: Converted) {
        match converted {
            Converted::Represented(message, gap) => {
                self.messages.push(message);
                self.gaps.extend(gap);
            }
            Converted::OmittedEmpty => self.omitted_empty += 1,
            Converted::Omitted(reason) => self.gaps.push(reason),
        }
    }
}

/// Provably empty nodes do not lower completeness, but they are never dropped silently.
fn note_omitted_empty(warnings: &mut Vec<String>, count: usize) {
    if count > 0 {
        warnings.push(format!(
            "{count} provably empty system message node(s) with no text, parts or assets were omitted from the projection"
        ));
    }
}

fn clip(value: &str) -> String {
    value.chars().take(48).collect()
}

/// Records why a projection is partial, without letting a hostile export flood the warnings.
fn record_completeness_gaps(warnings: &mut Vec<String>, gaps: Vec<String>) {
    let mut distinct: Vec<String> = Vec::new();
    for gap in gaps {
        if !distinct.contains(&gap) {
            distinct.push(gap);
        }
    }
    let total = distinct.len();
    for gap in distinct.into_iter().take(MAX_COMPLETENESS_WARNINGS) {
        warnings.push(format!("Completeness not proved: {gap}"));
    }
    if total > MAX_COMPLETENESS_WARNINGS {
        warnings.push(format!(
            "Completeness not proved: {} further distinct gaps omitted",
            total - MAX_COMPLETENESS_WARNINGS
        ));
    }
}

enum BranchProjection {
    CompleteActiveBranch(Vec<Message>),
    PartialUnverified(Vec<Message>),
    BranchingRefused(String),
    Empty,
}

fn timestamp_to_rfc3339(ts: f64) -> Option<String> {
    let secs = ts as i64;
    let nsecs = ((ts - secs as f64) * 1_000_000_000.0) as u32;
    chrono::DateTime::from_timestamp(secs, nsecs).map(|dt| dt.to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validate_historical_capture;

    #[test]
    fn test_chatgpt_linear_conversation() {
        let json = br#"[
            {
                "id": "12345678-1234-4123-8123-123456789abc",
                "title": "Quantum Computing",
                "create_time": 1700000000.0,
                "update_time": 1700000010.0,
                "current_node": "node-2",
                "mapping": {
                    "node-1": {
                        "id": "node-1",
                        "parent": null,
                        "children": ["node-2"],
                        "message": {
                            "id": "msg-1",
                            "author": {"role": "user"},
                            "create_time": 1700000000.0,
                            "content": {"content_type": "text", "parts": ["What is superposition?"]}
                        }
                    },
                    "node-2": {
                        "id": "node-2",
                        "parent": "node-1",
                        "children": [],
                        "message": {
                            "id": "msg-2",
                            "author": {"role": "assistant"},
                            "create_time": 1700000005.0,
                            "content": {"content_type": "text", "parts": ["Superposition is a principle of quantum mechanics..."]},
                            "metadata": {"model_slug": "gpt-4"}
                        }
                    }
                }
            }
        ]"#;

        let items = ChatGptExportParser::parse_conversations_json(&json[..]).unwrap();
        assert_eq!(items.len(), 1);

        match &items[0] {
            ReconciledItem::Conversation {
                capture,
                raw_evidence,
                ..
            } => {
                assert_eq!(capture.provider, "chatgpt");
                assert_eq!(
                    capture.conversation_id,
                    "12345678-1234-4123-8123-123456789abc"
                );
                assert_eq!(
                    capture.source_url,
                    "https://chatgpt.com/c/12345678-1234-4123-8123-123456789abc"
                );
                assert_eq!(capture.title, "Quantum Computing");
                assert_eq!(capture.capture_method, "export-import");
                assert_eq!(capture.completeness, "complete");
                assert_eq!(capture.messages.len(), 2);
                assert_eq!(capture.messages[0].role, "user");
                assert_eq!(capture.messages[0].text, "What is superposition?");
                assert_eq!(capture.messages[1].role, "assistant");
                assert_eq!(capture.messages[1].model.as_deref(), Some("gpt-4"));

                let mut cap = capture.clone();
                assert!(validate_historical_capture(&mut cap, "chatgpt").is_ok());

                assert_eq!(raw_evidence.provider, "chatgpt");
                assert_eq!(raw_evidence.envelope_domain, "chatgpt-export-dag");
            }
            _ => panic!("Expected Conversation variant"),
        }
    }

    #[test]
    fn test_chatgpt_branching_dag_projects_active_branch_only() {
        let json = br#"[
            {
                "id": "abcdef12-3456-7890-abcd-ef1234567890",
                "title": "Branching Story",
                "create_time": 1700000000.0,
                "current_node": "node-branch-b",
                "mapping": {
                    "node-root": {
                        "id": "node-root",
                        "parent": null,
                        "children": ["node-branch-a", "node-branch-b"],
                        "message": {
                            "id": "msg-root",
                            "author": {"role": "user"},
                            "content": {"content_type": "text", "parts": ["Write a story"]}
                        }
                    },
                    "node-branch-a": {
                        "id": "node-branch-a",
                        "parent": "node-root",
                        "children": [],
                        "message": {
                            "id": "msg-a",
                            "author": {"role": "assistant"},
                            "content": {"content_type": "text", "parts": ["Alternative A: Once upon a time in space..."]}
                        }
                    },
                    "node-branch-b": {
                        "id": "node-branch-b",
                        "parent": "node-root",
                        "children": [],
                        "message": {
                            "id": "msg-b",
                            "author": {"role": "assistant"},
                            "content": {"content_type": "text", "parts": ["Alternative B: Once upon a time in the ocean..."]}
                        }
                    }
                }
            }
        ]"#;

        let items = ChatGptExportParser::parse_conversations_json(&json[..]).unwrap();
        assert_eq!(items.len(), 1);

        match &items[0] {
            ReconciledItem::Conversation {
                capture,
                raw_evidence,
                ..
            } => {
                assert_eq!(capture.messages.len(), 2);
                assert_eq!(capture.messages[0].id, "msg-root");
                assert_eq!(capture.messages[1].id, "msg-b");
                assert!(capture.messages[1].text.contains("Alternative B"));

                let raw_str = String::from_utf8(raw_evidence.payload_bytes.clone()).unwrap();
                assert!(raw_str.contains("Alternative A"));
                assert!(raw_str.contains("Alternative B"));
            }
            _ => panic!("Expected Conversation variant"),
        }
    }

    #[test]
    fn test_chatgpt_cyclic_mapping_does_not_infinite_loop() {
        let json = br#"[
            {
                "id": "11112222-3333-4444-5555-666677778888",
                "title": "Cyclic Graph",
                "create_time": 1700000000.0,
                "current_node": "cycle-1",
                "mapping": {
                    "cycle-1": {
                        "id": "cycle-1",
                        "parent": "cycle-2",
                        "children": ["cycle-2"],
                        "message": {
                            "id": "msg-c1",
                            "author": {"role": "user"},
                            "content": {"content_type": "text", "parts": ["Looping"]}
                        }
                    },
                    "cycle-2": {
                        "id": "cycle-2",
                        "parent": "cycle-1",
                        "children": ["cycle-1"],
                        "message": {
                            "id": "msg-c2",
                            "author": {"role": "assistant"},
                            "content": {"content_type": "text", "parts": ["Infinite"]}
                        }
                    }
                }
            }
        ]"#;

        let items = ChatGptExportParser::parse_conversations_json(&json[..]).unwrap();
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn test_chatgpt_raw_exact_preservation_and_unknown_fields() {
        let orig = r#"{"id":"12345678-1234-4123-8123-123456789abc", "title":"Fixture","mapping":{"root":{"id":"root","parent":null,"children":[],"message":{"id":"m","author":{"role":"user"},"content":{"content_type":"text","parts":["hi"]}}}},"current_node":"root","unexpected_external":{"canary":"must-preserve"}}"#;
        let items =
            ChatGptExportParser::parse_conversations_json(format!("[{orig}]").as_bytes()).unwrap();
        let raw = match &items[0] {
            ReconciledItem::Conversation { raw_evidence, .. } => raw_evidence,
            _ => panic!("not conversation"),
        };
        assert_eq!(raw.payload_bytes, orig.as_bytes());
        assert!(String::from_utf8_lossy(&raw.payload_bytes).contains("must-preserve"));
    }

    #[test]
    fn test_chatgpt_missing_current_node_does_not_claim_complete() {
        let orig = r#"{"id":"12345678-1234-4123-8123-123456789abc","title":"Fixture","mapping":{"root":{"id":"root","parent":null,"children":["child"],"message":{"id":"m","author":{"role":"user"},"content":{"content_type":"text","parts":["hi"]}}},"child":{"id":"child","parent":"root","children":[],"message":{"id":"m2","author":{"role":"assistant"},"content":{"content_type":"text","parts":["reply"]}}}}}"#;
        let items =
            ChatGptExportParser::parse_conversations_json(format!("[{orig}]").as_bytes()).unwrap();
        match &items[0] {
            ReconciledItem::Conversation { capture, .. } => {
                assert_ne!(capture.completeness, "complete");
                assert_eq!(capture.completeness, "partial");
                assert!(capture
                    .warnings
                    .iter()
                    .any(|w| w.contains("current_node pointer missing")));
            }
            ReconciledItem::PreservedRawOnly { .. } => {}
        }
    }

    #[test]
    fn test_chatgpt_duplicate_conversation_ids_reconciled() {
        let x = r#"{"id":"12345678-1234-4123-8123-123456789abc","title":"Fixture","mapping":{"root":{"id":"root","parent":null,"children":[],"message":{"id":"m","author":{"role":"user"},"content":{"content_type":"text","parts":["hi"]}}}},"current_node":"root"}"#;
        let items =
            ChatGptExportParser::parse_conversations_json(format!("[{x},{x}]").as_bytes()).unwrap();
        assert_eq!(items.len(), 2);
        let ids: Vec<_> = items
            .iter()
            .map(|x| match x {
                ReconciledItem::Conversation { capture, .. } => capture.conversation_id.clone(),
                _ => panic!(),
            })
            .collect();
        assert_ne!(ids[0], ids[1]);
    }

    #[test]
    fn test_chatgpt_multimodal_attachment_projection() {
        let orig = r#"{"id":"12345678-1234-4123-8123-123456789abc","mapping":{"root":{"id":"root","parent":null,"children":[],"message":{"id":"m","author":{"role":"user"},"content":{"content_type":"multimodal_text","parts":[{"content_type":"image_asset_pointer","asset_pointer":"file-service://id"}]}}}},"current_node":"root"}"#;
        let items =
            ChatGptExportParser::parse_conversations_json(format!("[{orig}]").as_bytes()).unwrap();
        let c = match &items[0] {
            ReconciledItem::Conversation { capture, .. } => capture,
            _ => panic!(),
        };
        assert!(!c.messages[0].attachments.is_empty());
        assert_eq!(
            c.messages[0].attachments[0].source_url.as_deref(),
            Some("file-service://id")
        );
    }

    #[test]
    fn test_chatgpt_branching_without_current_node_preserves_raw_only() {
        let json = br#"[
            {
                "id": "12345678-1234-4123-8123-123456789abc",
                "title": "Branching Without Current Node",
                "create_time": 1700000000.0,
                "current_node": null,
                "mapping": {
                    "root": {
                        "id": "root",
                        "parent": null,
                        "children": ["branch-a", "branch-b"],
                        "message": {
                            "id": "m-root",
                            "author": {"role": "user"},
                            "content": {"content_type": "text", "parts": ["hello"]}
                        }
                    },
                    "branch-a": {
                        "id": "branch-a",
                        "parent": "root",
                        "children": [],
                        "message": {
                            "id": "m-a",
                            "author": {"role": "assistant"},
                            "content": {"content_type": "text", "parts": ["reply a"]}
                        }
                    },
                    "branch-b": {
                        "id": "branch-b",
                        "parent": "root",
                        "children": [],
                        "message": {
                            "id": "m-b",
                            "author": {"role": "assistant"},
                            "content": {"content_type": "text", "parts": ["reply b"]}
                        }
                    }
                }
            }
        ]"#;

        let items = ChatGptExportParser::parse_conversations_json(&json[..]).unwrap();
        assert_eq!(items.len(), 1);
        match &items[0] {
            ReconciledItem::PreservedRawOnly {
                disposition_reason, ..
            } => {
                assert!(disposition_reason.contains("active branch cannot be authoritatively"));
            }
            _ => panic!("Expected PreservedRawOnly variant for unresolvable branching DAG"),
        }
    }

    #[test]
    fn test_chatgpt_missing_timestamps_honestly_none() {
        let json = br#"[
            {
                "id": "12345678-1234-4123-8123-123456789abc",
                "title": "No Timestamps",
                "create_time": null,
                "update_time": null,
                "current_node": "root",
                "mapping": {
                    "root": {
                        "id": "root",
                        "parent": null,
                        "children": [],
                        "message": {
                            "id": "m-root",
                            "author": {"role": "user"},
                            "content": {"content_type": "text", "parts": ["timeless"]}
                        }
                    }
                }
            }
        ]"#;

        let items = ChatGptExportParser::parse_conversations_json(&json[..]).unwrap();
        assert_eq!(items.len(), 1);
        match &items[0] {
            ReconciledItem::Conversation { capture, .. } => {
                assert_eq!(capture.conversation_created_at, None);
                assert_eq!(capture.conversation_updated_at, None);
                assert!(capture
                    .warnings
                    .iter()
                    .any(|w| w.contains("Missing conversation create_time")));
            }
            _ => panic!("Expected Conversation variant"),
        }
    }

    fn two_node_conversation(id: &str, role: &str, content: serde_json::Value) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!([{
            "id": id,
            "title": "Synthetic",
            "create_time": 1700000000.0,
            "current_node": "n2",
            "mapping": {
                "n1": {"id":"n1","parent":null,"children":["n2"],"message":{
                    "id":"m1","author":{"role":"user"},"create_time":1700000001.0,
                    "content":{"content_type":"text","parts":["question"]}}},
                "n2": {"id":"n2","parent":"n1","children":[],"message":{
                    "id":"m2","author":{"role":role},"create_time":1700000002.0,
                    "content":content}}
            }
        }]))
        .unwrap()
    }

    fn projected(bytes: &[u8]) -> Capture {
        match ChatGptExportParser::parse_conversations_json(bytes)
            .unwrap()
            .remove(0)
        {
            ReconciledItem::Conversation { capture, .. } => capture,
            other => panic!("expected a conversation, got {other:?}"),
        }
    }

    const ID: &str = "12345678-1234-4123-8123-123456789abc";

    #[test]
    fn fully_represented_text_and_image_parts_stay_complete() {
        let text = projected(&two_node_conversation(
            ID,
            "assistant",
            serde_json::json!({"content_type":"text","parts":["answer"]}),
        ));
        assert_eq!(text.completeness, "complete");
        let multimodal = projected(&two_node_conversation(
            ID,
            "assistant",
            serde_json::json!({"content_type":"multimodal_text","parts":[
                "see", {"content_type":"image_asset_pointer","asset_pointer":"file-service://file-1"}]}),
        ));
        assert_eq!(multimodal.completeness, "complete");
        assert_eq!(multimodal.messages[1].attachments.len(), 1);
    }

    #[test]
    fn any_unrepresented_content_part_downgrades_the_whole_conversation_to_partial() {
        let cases = [
            (
                "unknown content type",
                "assistant",
                serde_json::json!({"content_type":"thoughts","parts":["hidden"]}),
            ),
            (
                "code content",
                "assistant",
                serde_json::json!({"content_type":"code","text":"print(1)","language":"python"}),
            ),
            (
                "non-text object part",
                "assistant",
                serde_json::json!({"content_type":"multimodal_text","parts":[{"content_type":"audio_transcription","text":"hi","direction":"in"}]}),
            ),
            (
                "numeric part",
                "assistant",
                serde_json::json!({"content_type":"text","parts":["a", 42]}),
            ),
            (
                "opaque object part",
                "assistant",
                serde_json::json!({"content_type":"multimodal_text","parts":[{"kind":"table","rows":[1]}]}),
            ),
            (
                "parts and text both present",
                "assistant",
                serde_json::json!({"content_type":"text","parts":["a"],"text":"b"}),
            ),
            (
                "asset without a pointer",
                "assistant",
                serde_json::json!({"content_type":"multimodal_text","parts":[{"content_type":"image_asset_pointer"}]}),
            ),
            (
                "unknown author role",
                "browser",
                serde_json::json!({"content_type":"text","parts":["a"]}),
            ),
        ];
        for (label, role, content) in cases {
            let capture = projected(&two_node_conversation(ID, role, content));
            assert_eq!(capture.completeness, "partial", "{label}");
            assert!(
                capture
                    .warnings
                    .iter()
                    .any(|w| w.starts_with("Completeness not proved:")),
                "{label}: {:?}",
                capture.warnings
            );
            let mut copy = capture.clone();
            assert!(
                validate_historical_capture(&mut copy, "chatgpt").is_ok(),
                "{label}"
            );
        }
    }

    #[test]
    fn completeness_warnings_stay_bounded_under_hostile_part_variety() {
        let parts: Vec<serde_json::Value> = (0..200)
            .map(|index| serde_json::json!({ format!("k{index}"): index }))
            .collect();
        let capture = projected(&two_node_conversation(
            ID,
            "assistant",
            serde_json::json!({"content_type":"multimodal_text","parts":parts}),
        ));
        assert_eq!(capture.completeness, "partial");
        assert!(capture.warnings.len() <= 8, "{:?}", capture.warnings);
    }

    #[test]
    fn message_times_are_the_messages_own_times_only() {
        let capture = projected(&two_node_conversation(
            ID,
            "assistant",
            serde_json::json!({"content_type":"text","parts":["answer"]}),
        ));
        assert_eq!(
            capture.messages[0].authored_at.as_deref(),
            Some("2023-11-14T22:13:21+00:00")
        );
        assert_eq!(
            capture.messages[1].authored_at.as_deref(),
            Some("2023-11-14T22:13:22+00:00")
        );
    }

    #[test]
    fn oversized_record_is_rejected_while_streaming_without_draining_the_source() {
        struct Endless {
            served: usize,
        }
        impl Read for Endless {
            fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
                let prefix: &[u8] = if self.served == 0 {
                    b"[{\"id\":\""
                } else {
                    b""
                };
                let mut written = 0;
                for byte in prefix.iter().copied().chain(std::iter::repeat(b'a')) {
                    if written == out.len() {
                        break;
                    }
                    out[written] = byte;
                    written += 1;
                }
                self.served += written;
                Ok(written)
            }
        }
        let mut source = Endless { served: 0 };
        let result = ChatGptExportParser::stream_conversations_json(&mut source, |_| Ok(()));
        assert!(result.unwrap_err().to_string().contains("byte limit"));
        assert!(
            source.served <= MAX_CHATGPT_RECORD_BYTES + 64 * 1024 + 16,
            "stream was read for {} bytes",
            source.served
        );
    }

    #[test]
    fn oversized_whitespace_stream_between_records_is_rejected() {
        struct Spaces {
            served: usize,
        }
        impl Read for Spaces {
            fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
                let prefix: &[u8] = if self.served == 0 { b"[" } else { b"" };
                let mut written = 0;
                for byte in prefix.iter().copied().chain(std::iter::repeat(b' ')) {
                    if written == out.len() {
                        break;
                    }
                    out[written] = byte;
                    written += 1;
                }
                self.served += written;
                Ok(written)
            }
        }
        let mut source = Spaces { served: 0 };
        let result = ChatGptExportParser::stream_conversations_json(&mut source, |_| Ok(()));
        assert!(result.is_err());
        assert!(source.served <= MAX_CHATGPT_SEPARATOR_BYTES + 64 * 1024 + 16);
    }

    #[test]
    fn conversation_id_tracking_is_bounded() {
        let mut tracker = ConversationIdTracker::with_limit(3);
        assert_eq!(tracker.observe("a").unwrap(), 1);
        assert_eq!(tracker.observe("b").unwrap(), 1);
        assert_eq!(tracker.observe("a").unwrap(), 2);
        assert_eq!(tracker.observe("c").unwrap(), 1);
        // The limit counts distinct IDs; repeats of known IDs stay allowed, new IDs fail.
        assert_eq!(tracker.observe("c").unwrap(), 2);
        assert!(tracker.observe("d").is_err());
        assert_eq!(tracker.seen.len(), 3);
        assert_eq!(MAX_TRACKED_CONVERSATION_IDS, 250_000);
    }

    #[test]
    fn exceeding_the_tracked_id_bound_aborts_projection_of_a_new_conversation() {
        let mut tracker = ConversationIdTracker::with_limit(1);
        let first = two_node_conversation(
            ID,
            "assistant",
            serde_json::json!({"content_type":"text","parts":["a"]}),
        );
        let second = two_node_conversation(
            "87654321-1234-4123-8123-123456789abc",
            "assistant",
            serde_json::json!({"content_type":"text","parts":["a"]}),
        );
        let mut one = |bytes: &[u8]| {
            let record = &bytes[1..bytes.len() - 1];
            ChatGptExportParser::reconcile_conversation_raw(record.to_vec(), &mut tracker)
        };
        assert!(one(&first).is_ok());
        assert!(one(&second)
            .unwrap_err()
            .to_string()
            .contains("distinct conversation IDs"));
    }

    #[test]
    fn overlong_or_empty_conversation_ids_are_preserved_raw_only_and_never_tracked() {
        for id in [String::new(), "x".repeat(MAX_CONVERSATION_ID_BYTES + 1)] {
            let items = ChatGptExportParser::parse_conversations_json(
                &two_node_conversation(
                    &id,
                    "assistant",
                    serde_json::json!({"content_type":"text","parts":["a"]}),
                )[..],
            )
            .unwrap();
            match &items[0] {
                ReconciledItem::PreservedRawOnly {
                    disposition_reason, ..
                } => {
                    assert!(disposition_reason.contains("conversation id"));
                }
                other => panic!("expected raw-only, got {other:?}"),
            }
        }
    }

    #[test]
    fn in_memory_collection_is_bounded_by_bytes_as_well_as_count() {
        assert!(MAX_COLLECTED_RAW_BYTES >= MAX_CHATGPT_RECORD_BYTES);
        assert!(MAX_ITEMS_IN_MEMORY > 0);
    }

    /// system node (given content) -> user -> assistant, current_node on the assistant.
    fn with_system_node(system_content: serde_json::Value, current: bool) -> Vec<u8> {
        let mut conversation = serde_json::json!({
            "id": ID, "title": "Synthetic", "create_time": 1700000000.0,
            "mapping": {
                "s": {"id":"s","parent":null,"children":["u"],"message":{
                    "id":"ms","author":{"role":"system"},"content": system_content}},
                "u": {"id":"u","parent":"s","children":["a"],"message":{
                    "id":"mu","author":{"role":"user"},"content":{"content_type":"text","parts":["q"]}}},
                "a": {"id":"a","parent":"u","children":[],"message":{
                    "id":"ma","author":{"role":"assistant"},"content":{"content_type":"text","parts":["r"]}}}
            }
        });
        if current {
            conversation["current_node"] = "a".into();
        }
        serde_json::to_vec(&serde_json::json!([conversation])).unwrap()
    }

    fn warned(capture: &Capture, needle: &str) -> bool {
        capture.warnings.iter().any(|w| w.contains(needle))
    }

    #[test]
    fn provably_empty_system_nodes_are_omitted_with_a_note_and_stay_complete() {
        for content in [
            serde_json::json!({"content_type":"text","parts":[""]}),
            serde_json::json!({"content_type":"text","parts":[]}),
            serde_json::json!({"content_type":"multimodal_text","parts":["", "  "]}),
        ] {
            let capture = projected(&with_system_node(content.clone(), true));
            assert_eq!(capture.completeness, "complete", "{content}");
            assert_eq!(
                capture
                    .messages
                    .iter()
                    .map(|m| m.role.as_str())
                    .collect::<Vec<_>>(),
                ["user", "assistant"]
            );
            assert!(
                warned(&capture, "1 provably empty system message node(s)"),
                "never silent: {:?}",
                capture.warnings
            );
            assert!(!warned(&capture, "Completeness not proved"));
        }
    }

    #[test]
    fn a_system_node_with_null_or_unrepresentable_content_is_a_completeness_gap() {
        // `content: null` is absence, not proven emptiness.
        let null = projected(&with_system_node(serde_json::Value::Null, true));
        assert_eq!(null.completeness, "partial");
        assert!(
            warned(&null, "Completeness not proved: A message has no content"),
            "{:?}",
            null.warnings
        );
        assert_eq!(
            null.messages.len(),
            2,
            "the node is still not shown, but it is accounted for"
        );
        // A content type the projection cannot represent, even with no text, is a gap too.
        let thoughts = projected(&with_system_node(
            serde_json::json!({"content_type":"thoughts","thoughts":[]}),
            true,
        ));
        assert_eq!(thoughts.completeness, "partial");
        assert!(warned(&thoughts, "Content type 'thoughts'"));
        // Text that is present is represented (not omitted) and stays complete.
        let present = projected(&with_system_node(
            serde_json::json!({"content_type":"text","parts":["be brief"]}),
            true,
        ));
        assert_eq!(present.completeness, "complete");
        assert_eq!(present.messages.len(), 3);
        assert!(!warned(&present, "provably empty"));
    }

    #[test]
    fn a_non_system_message_without_content_is_a_gap_not_silent_emptiness() {
        let mut conversation: serde_json::Value = serde_json::from_slice(&with_system_node(
            serde_json::json!({"content_type":"text","parts":["x"]}),
            true,
        ))
        .unwrap();
        conversation[0]["mapping"]["a"]["message"]["content"] = serde_json::Value::Null;
        let capture = projected(&serde_json::to_vec(&conversation).unwrap());
        assert_eq!(capture.completeness, "partial");
        assert!(warned(&capture, "A message has no content"));
    }

    #[test]
    fn omitted_empty_nodes_are_also_noted_when_current_node_is_missing() {
        let capture = projected(&with_system_node(
            serde_json::json!({"content_type":"text","parts":[""]}),
            false,
        ));
        // Without current_node the projection is unverified regardless, and the omission is noted.
        assert_eq!(capture.completeness, "partial");
        assert!(warned(&capture, "provably empty system message node(s)"));
        assert!(warned(&capture, "current_node pointer missing"));
    }

    #[test]
    fn several_omitted_empty_nodes_are_counted_once() {
        let mut conversation: serde_json::Value = serde_json::from_slice(&with_system_node(
            serde_json::json!({"content_type":"text","parts":[""]}),
            true,
        ))
        .unwrap();
        // Insert a second empty system node between the first and the user message.
        conversation[0]["mapping"]["s2"] = serde_json::json!({"id":"s2","parent":"s","children":["u"],"message":{
            "id":"ms2","author":{"role":"system"},"content":{"content_type":"text","parts":[""]}}});
        conversation[0]["mapping"]["s"]["children"] = serde_json::json!(["s2"]);
        conversation[0]["mapping"]["u"]["parent"] = "s2".into();
        let capture = projected(&serde_json::to_vec(&conversation).unwrap());
        assert_eq!(capture.completeness, "complete");
        assert!(
            warned(&capture, "2 provably empty system message node(s)"),
            "{:?}",
            capture.warnings
        );
    }
}

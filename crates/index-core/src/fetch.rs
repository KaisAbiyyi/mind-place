//! Phase 2B: provenance-checked, bounded progressive archive fetch.
//! All fetches are resolved from a stable indexed source+snapshot, never from
//! arbitrary filesystem paths or transient FTS row IDs.
use crate::{verify_snapshot, IndexError, IndexStore, Result};
use mind_place_archive::ArchiveStore;
use mind_place_state::{canonical_source_id, SourceDescriptor, SourceKey};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::time::{Duration, Instant};

const MAX_RESPONSE_SEGMENTS: usize = 100;
const MAX_TEXT_BYTES: usize = 256 * 1024;
const MAX_ESTIMATED_TOKENS: usize = 65_536;
const MAX_LATENCY_MS: u64 = 30_000;
const MAX_WINDOW: usize = 20;

fn default_bytes() -> usize {
    16 * 1024
}
fn default_tokens() -> usize {
    4096
}
fn default_latency() -> u64 {
    2000
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FetchGranularity {
    #[default]
    Message,
    Window,
    Revision,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct FetchBudget {
    /// UTF-8 text bytes only; excludes JSON envelope and metadata.
    pub max_bytes: usize,
    /// Approximate token proxy: ceil(total UTF-8 text bytes / 4), not a
    /// model-tokenizer guarantee. The byte cap remains authoritative.
    pub max_estimated_tokens: usize,
    /// Cooperative deadline. A blocking archive read cannot be preempted.
    pub max_latency_ms: u64,
}
impl Default for FetchBudget {
    fn default() -> Self {
        Self {
            max_bytes: default_bytes(),
            max_estimated_tokens: default_tokens(),
            max_latency_ms: default_latency(),
        }
    }
}
impl FetchBudget {
    fn text_cap(&self) -> Result<usize> {
        if !(4..=MAX_TEXT_BYTES).contains(&self.max_bytes)
            || !(1..=MAX_ESTIMATED_TOKENS).contains(&self.max_estimated_tokens)
            || !(1..=MAX_LATENCY_MS).contains(&self.max_latency_ms)
        {
            return Err(IndexError::InvalidQuery);
        }
        Ok(self
            .max_bytes
            .min(self.max_estimated_tokens.saturating_mul(4)))
    }
}

/// Next position is relative to the same immutable revision and selector.
/// Callers must resend the original source/snapshot/selection and budget.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FetchCursor {
    pub message_index: usize,
    pub byte_offset: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct FetchRequest {
    pub source_id: String,
    pub archive_id: String,
    pub snapshot_id: String,
    pub granularity: FetchGranularity,
    /// Required for message/window; ignored only if absent for revision.
    pub message_id: Option<String>,
    /// Neighbor message count on each side in window mode.
    pub before: usize,
    pub after: usize,
    pub budget: FetchBudget,
    pub cursor: Option<FetchCursor>,
}
impl Default for FetchRequest {
    fn default() -> Self {
        Self {
            source_id: String::new(),
            archive_id: String::new(),
            snapshot_id: String::new(),
            granularity: FetchGranularity::Message,
            message_id: None,
            before: 0,
            after: 0,
            budget: FetchBudget::default(),
            cursor: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct FetchSegment {
    pub message_index: usize,
    pub message_id: String,
    pub role: String,
    pub model: Option<String>,
    pub authored_at: Option<String>,
    pub byte_start: usize,
    pub byte_end: usize,
    pub message_total_bytes: usize,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct FetchResponse {
    pub source_id: String,
    pub archive_id: String,
    pub snapshot_id: String,
    pub conversation_id: String,
    pub provider: String,
    pub source_url: String,
    pub title: String,
    pub captured_at: String,
    pub completeness: String,
    pub index_generation: i64,
    /// Current as of index generation, not necessarily the present archive head.
    pub indexed_is_current: bool,
    pub selected_start: usize,
    pub selected_end_exclusive: usize,
    pub segments: Vec<FetchSegment>,
    pub text_bytes: usize,
    pub estimated_tokens: usize,
    pub truncated: bool,
    pub truncation_reason: Option<String>,
    pub next_cursor: Option<FetchCursor>,
}

impl IndexStore {
    pub fn fetch(&self, archive: &ArchiveStore, request: &FetchRequest) -> Result<FetchResponse> {
        self.fetch_with_check(archive, request, || Ok(()))
    }

    /// Caller must validate authorization before entry and after return.
    /// This callback also aborts with no payload on a revoked security lease.
    pub fn fetch_with_check(
        &self,
        archive: &ArchiveStore,
        request: &FetchRequest,
        mut check: impl FnMut() -> Result<()>,
    ) -> Result<FetchResponse> {
        let started = Instant::now();
        let cap = request.budget.text_cap()?;
        if request.source_id.len() != 71
            || !request.source_id.starts_with("src:v1:")
            || !is_hex(&request.source_id[7..])
            || !is_hex64(&request.archive_id)
            || !is_hex64(&request.snapshot_id)
            || request.before > MAX_WINDOW
            || request.after > MAX_WINDOW
            || (request.granularity != FetchGranularity::Window
                && (request.before != 0 || request.after != 0))
            || (request.granularity == FetchGranularity::Revision && request.message_id.is_some())
            || (request.granularity != FetchGranularity::Revision
                && request
                    .message_id
                    .as_ref()
                    .is_none_or(|id| id.is_empty() || id.len() > 256))
        {
            return Err(IndexError::InvalidQuery);
        }
        check()?;
        let db = self.connection()?;
        let indexed: Option<(String, String, String, String, String, String, i64)> = db
            .query_row(
                "SELECT provider,conversation_id,title,source_url,captured_at,completeness,is_current
                 FROM revisions WHERE source_id=?1 AND archive_id=?2 AND snapshot_id=?3",
                params![request.source_id, request.archive_id, request.snapshot_id],
                |row| Ok((
                    row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?,
                    row.get(4)?, row.get(5)?, row.get(6)?,
                )),
            )
            .optional()?;
        let (provider, conversation_id, title, source_url, captured_at, completeness, is_current) =
            indexed.ok_or(IndexError::InvalidQuery)?;
        let namespace =
            super::namespace_for_provider(&provider).ok_or(IndexError::InvalidEvidence)?;
        let canonical = canonical_source_id(&SourceDescriptor {
            provider: &provider,
            source_namespace: namespace,
            source_kind: "conversation",
            key: SourceKey::External(&conversation_id),
        })?;
        if canonical != request.source_id {
            return Err(IndexError::InvalidEvidence);
        }
        let expected_archive = format!(
            "{:x}",
            sha2::Sha256::digest(format!("{provider}:{conversation_id}").as_bytes())
        );
        if expected_archive != request.archive_id {
            return Err(IndexError::InvalidEvidence);
        }
        let generation: i64 = db.query_row(
            "SELECT CAST(value AS INTEGER) FROM index_meta WHERE key='generation'",
            [],
            |row| row.get(0),
        )?;
        check()?;
        let conversation = archive.read(&request.archive_id, Some(&request.snapshot_id))?;
        check()?;
        // A filename is not proof that its contents match the requested revision.
        verify_snapshot(&conversation.capture, &request.snapshot_id)?;
        let capture = &conversation.capture;
        if conversation.snapshot_id != request.snapshot_id
            || capture.provider != provider
            || capture.conversation_id != conversation_id
            || capture.title != title
            || capture.source_url != source_url
            || capture.captured_at != captured_at
            || capture.completeness != completeness
        {
            return Err(IndexError::InvalidEvidence);
        }
        let messages = &capture.messages;
        let (start, end) = match request.granularity {
            FetchGranularity::Revision => (0, messages.len()),
            FetchGranularity::Message | FetchGranularity::Window => {
                let target = request.message_id.as_deref().unwrap_or_default();
                let at = messages
                    .iter()
                    .position(|message| message.id == target)
                    .ok_or(IndexError::InvalidQuery)?;
                if request.granularity == FetchGranularity::Message {
                    (at, at + 1)
                } else {
                    (
                        at.saturating_sub(request.before),
                        at.saturating_add(request.after + 1).min(messages.len()),
                    )
                }
            }
        };
        let mut cursor = request.cursor.unwrap_or(FetchCursor {
            message_index: start,
            byte_offset: 0,
        });
        if cursor.message_index < start
            || cursor.message_index > end
            || (cursor.message_index == end && cursor.byte_offset != 0)
            || (cursor.message_index < end
                && !messages[cursor.message_index]
                    .text
                    .is_char_boundary(cursor.byte_offset))
        {
            return Err(IndexError::InvalidQuery);
        }
        let mut segments = Vec::new();
        let mut used = 0usize;
        let mut reason = None;
        while cursor.message_index < end {
            check()?;
            if started.elapsed() >= Duration::from_millis(request.budget.max_latency_ms) {
                reason = Some("latency_budget".to_string());
                break;
            }
            if segments.len() == MAX_RESPONSE_SEGMENTS {
                reason = Some("segment_limit".to_string());
                break;
            }
            if used >= cap {
                reason = Some(
                    if cap == request.budget.max_bytes {
                        "byte_budget"
                    } else {
                        "token_estimate_budget"
                    }
                    .to_string(),
                );
                break;
            }
            let message = &messages[cursor.message_index];
            let remaining = &message.text[cursor.byte_offset..];
            let available = cap - used;
            let mut take = remaining.len().min(available);
            while take > 0 && !remaining.is_char_boundary(take) {
                take -= 1;
            }
            // With a minimum of four remaining bytes every Unicode scalar can
            // fit, but a nearly exhausted request may have 1-3 bytes left.
            if take == 0 && !remaining.is_empty() {
                reason = Some("utf8_boundary".to_string());
                break;
            }
            let end_byte = cursor.byte_offset + take;
            segments.push(FetchSegment {
                message_index: cursor.message_index,
                message_id: message.id.clone(),
                role: message.role.clone(),
                model: message.model.clone(),
                authored_at: message.authored_at.clone(),
                byte_start: cursor.byte_offset,
                byte_end: end_byte,
                message_total_bytes: message.text.len(),
                text: remaining[..take].to_string(),
            });
            used += take;
            if end_byte == message.text.len() {
                cursor.message_index += 1;
                cursor.byte_offset = 0;
            } else {
                cursor.byte_offset = end_byte;
            }
        }
        check()?;
        let truncated = cursor.message_index < end;
        Ok(FetchResponse {
            source_id: request.source_id.clone(),
            archive_id: request.archive_id.clone(),
            snapshot_id: request.snapshot_id.clone(),
            conversation_id,
            provider,
            source_url,
            title,
            captured_at,
            completeness,
            index_generation: generation,
            indexed_is_current: is_current == 1,
            selected_start: start,
            selected_end_exclusive: end,
            segments,
            text_bytes: used,
            estimated_tokens: used.div_ceil(4),
            truncated,
            truncation_reason: if truncated { reason } else { None },
            next_cursor: truncated.then_some(cursor),
        })
    }
}

fn is_hex(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
fn is_hex64(value: &str) -> bool {
    value.len() == 64 && is_hex(value)
}

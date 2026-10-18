//! Phase 2C: evidence-backed context packs over Phase 2A search and Phase 2B fetch.
//! A pack is a bounded projection, never a new authority or a memory truth claim.
use crate::{
    FetchBudget, FetchCursor, FetchGranularity, FetchRequest, IndexError, IndexStore, QueryTerm,
    Result, RetrievalFilters, RetrievalHit, RetrievalRequest,
};
use chrono::DateTime;
use mind_place_archive::ArchiveStore;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    time::{Duration, Instant},
};

const MAX_CONTEXT_BYTES: usize = 64 * 1024;
const MAX_CONTEXT_TOKENS: usize = 16_384;
const MAX_CONTEXT_MS: u64 = 30_000;
const MAX_SOURCES: usize = 12;
const MAX_EXCERPTS: usize = 40;
const MAX_PER_SOURCE: usize = 6;
const SEARCH_POOL: usize = 100;

fn default_sources() -> usize {
    6
}
fn default_excerpts() -> usize {
    16
}
fn default_per_source() -> usize {
    3
}
fn default_bytes() -> usize {
    16 * 1024
}
fn default_tokens() -> usize {
    4096
}
fn default_latency() -> u64 {
    3000
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContextPreset {
    Fast,
    #[default]
    Balanced,
    SourceFirst,
    Challenge,
    Historical,
}
impl ContextPreset {
    fn neighbors(self) -> usize {
        match self {
            Self::Fast | Self::Challenge => 0,
            Self::Balanced | Self::Historical => 1,
            Self::SourceFirst => 2,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct ContextBudget {
    /// Bounds emitted evidence text, not JSON/envelope metadata.
    pub max_bytes: usize,
    /// Estimate only: ceil(total emitted UTF-8 bytes / 4).
    pub max_estimated_tokens: usize,
    /// Cooperative latency budget (archive I/O is not preemptible).
    pub max_latency_ms: u64,
}
impl Default for ContextBudget {
    fn default() -> Self {
        Self {
            max_bytes: default_bytes(),
            max_estimated_tokens: default_tokens(),
            max_latency_ms: default_latency(),
        }
    }
}
impl ContextBudget {
    fn validate(&self) -> Result<usize> {
        if !(4..=MAX_CONTEXT_BYTES).contains(&self.max_bytes)
            || !(1..=MAX_CONTEXT_TOKENS).contains(&self.max_estimated_tokens)
            || !(1..=MAX_CONTEXT_MS).contains(&self.max_latency_ms)
        {
            return Err(IndexError::InvalidQuery);
        }
        Ok(self
            .max_bytes
            .min(self.max_estimated_tokens.saturating_mul(4)))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ContextRequest {
    pub queries: Vec<QueryTerm>,
    pub filters: RetrievalFilters,
    pub preset: ContextPreset,
    /// Historical is based on captured_at, never a belief-state timestamp.
    pub as_of: Option<String>,
    pub max_sources: usize,
    pub max_excerpts: usize,
    pub max_per_source: usize,
    pub budget: ContextBudget,
}
impl Default for ContextRequest {
    fn default() -> Self {
        Self {
            queries: Vec::new(),
            filters: RetrievalFilters::default(),
            preset: ContextPreset::default(),
            as_of: None,
            max_sources: default_sources(),
            max_excerpts: default_excerpts(),
            max_per_source: default_per_source(),
            budget: ContextBudget::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ContextExcerpt {
    pub kind: String,
    pub message_id: String,
    /// None for a verified conversation title.
    pub message_index: Option<usize>,
    pub role: String,
    pub text: String,
    pub byte_start: usize,
    pub byte_end: usize,
    pub total_bytes: usize,
    pub score: f64,
    pub matched_queries: Vec<usize>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ContextGroup {
    pub source_id: String,
    pub archive_id: String,
    pub snapshot_id: String,
    pub conversation_id: String,
    pub title: String,
    pub source_url: String,
    pub captured_at: String,
    pub completeness: String,
    /// Represents the index generation used during this fetch, not a live provider status.
    pub indexed_is_current: bool,
    pub excerpts: Vec<ContextExcerpt>,
    /// If an excerpt hit a fetch budget, continue via the Phase 2B API using
    /// the matched message_id/window and this cursor.
    pub next_cursor: Option<FetchCursor>,
    pub continuation_message_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ContextPack {
    pub preset: ContextPreset,
    pub groups: Vec<ContextGroup>,
    pub candidate_count: usize,
    pub search_shortlist_truncated: bool,
    pub text_bytes: usize,
    pub estimated_tokens: usize,
    pub excerpt_count: usize,
    pub source_count: usize,
    pub truncated: bool,
    pub truncation_reasons: Vec<String>,
}

fn note_reason(reasons: &mut Vec<String>, reason: &str) {
    if !reasons.iter().any(|existing| existing == reason) {
        reasons.push(reason.to_string());
    }
}

fn apply_policy(request: &ContextRequest) -> Result<RetrievalFilters> {
    let mut filters = request.filters.clone();
    if request.preset != ContextPreset::Historical && request.as_of.is_some() {
        return Err(IndexError::InvalidQuery);
    }
    match request.preset {
        ContextPreset::Fast | ContextPreset::Balanced | ContextPreset::SourceFirst => {
            // Explicit history is not silently ignored by a current-only preset.
            if filters.include_history {
                return Err(IndexError::InvalidQuery);
            }
        }
        ContextPreset::Challenge => {
            filters.include_history = true;
        }
        ContextPreset::Historical => {
            let as_of = request.as_of.as_deref().ok_or(IndexError::InvalidQuery)?;
            let timestamp =
                DateTime::parse_from_rfc3339(as_of).map_err(|_| IndexError::InvalidQuery)?;
            if let Some(existing) = &filters.captured_to {
                let existing_date =
                    DateTime::parse_from_rfc3339(existing).map_err(|_| IndexError::InvalidQuery)?;
                if timestamp < existing_date {
                    filters.captured_to = Some(as_of.to_string());
                }
            } else {
                filters.captured_to = Some(as_of.to_string());
            }
            filters.include_history = true;
        }
    }
    Ok(filters)
}

/// Deterministic source-level round robin: one source cannot fill an entire
/// context pack before another relevant source gets a chance.
fn interleave(hits: Vec<RetrievalHit>) -> Vec<RetrievalHit> {
    let mut groups: Vec<(String, VecDeque<RetrievalHit>)> = Vec::new();
    for hit in hits {
        if let Some((_, queue)) = groups.iter_mut().find(|(id, _)| id == &hit.source_id) {
            queue.push_back(hit);
        } else {
            groups.push((hit.source_id.clone(), VecDeque::from([hit])));
        }
    }
    let mut ordered = Vec::new();
    loop {
        let mut any = false;
        for (_, queue) in &mut groups {
            if let Some(hit) = queue.pop_front() {
                ordered.push(hit);
                any = true;
            }
        }
        if !any {
            break;
        }
    }
    ordered
}

impl IndexStore {
    pub fn build_context(
        &self,
        archive: &ArchiveStore,
        request: &ContextRequest,
    ) -> Result<ContextPack> {
        self.build_context_with_check(archive, request, || Ok(()))
    }

    /// The caller must guard client permissions; the callback is checked at
    /// every source boundary and before returning any raw text to its caller.
    pub fn build_context_with_check(
        &self,
        archive: &ArchiveStore,
        request: &ContextRequest,
        mut check: impl FnMut() -> Result<()>,
    ) -> Result<ContextPack> {
        let started = Instant::now();
        let text_cap = request.budget.validate()?;
        if !(1..=MAX_SOURCES).contains(&request.max_sources)
            || !(1..=MAX_EXCERPTS).contains(&request.max_excerpts)
            || !(1..=MAX_PER_SOURCE).contains(&request.max_per_source)
        {
            return Err(IndexError::InvalidQuery);
        }
        let filters = apply_policy(request)?;
        check()?;
        let search = self.retrieve(&RetrievalRequest {
            queries: request.queries.clone(),
            filters,
            limit: SEARCH_POOL,
        })?;
        check()?;

        let mut groups: Vec<ContextGroup> = Vec::new();
        let mut source_counts: BTreeMap<String, usize> = BTreeMap::new();
        let mut seen_segments: BTreeSet<(String, String, String, usize)> = BTreeSet::new();
        let mut excerpt_count = 0usize;
        let mut used = 0usize;
        let mut reasons = Vec::new();

        for hit in interleave(search.hits) {
            check()?;
            let elapsed = started.elapsed();
            if elapsed >= Duration::from_millis(request.budget.max_latency_ms) {
                note_reason(&mut reasons, "latency_budget");
                break;
            }
            if excerpt_count >= request.max_excerpts {
                note_reason(&mut reasons, "excerpt_limit");
                break;
            }
            let n = source_counts.get(&hit.source_id).copied().unwrap_or(0);
            if n >= request.max_per_source {
                note_reason(&mut reasons, "per_source_limit");
                continue;
            }
            if n == 0 && source_counts.len() >= request.max_sources {
                note_reason(&mut reasons, "source_limit");
                continue;
            }
            let available = text_cap.saturating_sub(used);
            if available < 4 {
                note_reason(&mut reasons, "text_budget");
                break;
            }
            let remaining_ms = request
                .budget
                .max_latency_ms
                .saturating_sub(elapsed.as_millis() as u64)
                .max(1);
            let limit_bytes = available.min(16 * 1024);
            let fetch_request = FetchRequest {
                source_id: hit.source_id.clone(),
                archive_id: hit.archive_id.clone(),
                snapshot_id: hit.snapshot_id.clone(),
                granularity: if hit.kind == "title" {
                    FetchGranularity::Revision
                } else if request.preset.neighbors() == 0 {
                    FetchGranularity::Message
                } else {
                    FetchGranularity::Window
                },
                message_id: if hit.kind == "title" {
                    None
                } else {
                    Some(hit.message_id.clone())
                },
                before: if hit.kind == "title" {
                    0
                } else {
                    request.preset.neighbors()
                },
                after: if hit.kind == "title" {
                    0
                } else {
                    request.preset.neighbors()
                },
                budget: FetchBudget {
                    max_bytes: limit_bytes,
                    max_estimated_tokens: limit_bytes.div_ceil(4),
                    max_latency_ms: remaining_ms,
                },
                cursor: None,
            };
            // Phase 2B checks canonical identity, exact snapshot, raw digest
            // and match against index metadata before any text is emitted.
            let fetched = self.fetch_with_check(archive, &fetch_request, &mut check)?;
            let key = (fetched.source_id.clone(), fetched.snapshot_id.clone());
            let group_at = groups.iter().position(|g| {
                (g.source_id.as_str(), g.snapshot_id.as_str()) == (key.0.as_str(), key.1.as_str())
            });
            let mut pending = Vec::new();
            if hit.kind == "title" {
                let text = bounded_prefix(&fetched.title, available);
                let unique = seen_segments.insert((
                    hit.source_id.clone(),
                    hit.snapshot_id.clone(),
                    "title".to_string(),
                    0,
                ));
                if unique && !text.is_empty() {
                    pending.push(ContextExcerpt {
                        kind: "title".into(),
                        message_id: "@title".into(),
                        message_index: None,
                        role: "title".into(),
                        text: text.to_string(),
                        byte_start: 0,
                        byte_end: text.len(),
                        total_bytes: fetched.title.len(),
                        score: hit.score,
                        matched_queries: hit.matched_queries.clone(),
                    });
                    if text.len() != fetched.title.len() {
                        note_reason(&mut reasons, "text_budget");
                    }
                }
            } else {
                for segment in &fetched.segments {
                    if pending.len() + excerpt_count >= request.max_excerpts {
                        note_reason(&mut reasons, "excerpt_limit");
                        break;
                    }
                    let identity = (
                        hit.source_id.clone(),
                        hit.snapshot_id.clone(),
                        "message".to_string(),
                        segment.message_index,
                    );
                    let is_match = segment.message_id == hit.message_id;
                    if !seen_segments.insert(identity) {
                        // A neighbor fetched by an earlier hit can later become
                        // a direct match; enrich its provenance without
                        // returning the same original message twice.
                        if is_match {
                            if let Some(at) = group_at {
                                if let Some(existing) = groups[at].excerpts.iter_mut().find(|entry| {
                                    entry.message_index == Some(segment.message_index)
                                }) {
                                    existing.score = existing.score.max(hit.score);
                                    existing.matched_queries.extend(hit.matched_queries.iter().copied());
                                    existing.matched_queries.sort_unstable();
                                    existing.matched_queries.dedup();
                                }
                            }
                        }
                        continue;
                    }
                    if segment.text.is_empty() {
                        continue;
                    }
                    pending.push(ContextExcerpt {
                        kind: "message".into(),
                        message_id: segment.message_id.clone(),
                        message_index: Some(segment.message_index),
                        role: segment.role.clone(),
                        text: segment.text.clone(),
                        byte_start: segment.byte_start,
                        byte_end: segment.byte_end,
                        total_bytes: segment.message_total_bytes,
                        score: if is_match { hit.score } else { 0.0 },
                        matched_queries: if is_match { hit.matched_queries.clone() } else { Vec::new() },
                    });
                }
            }
            if pending.is_empty() {
                continue;
            }
            let pending_bytes: usize = pending.iter().map(|item| item.text.len()).sum();
            // Fetch bytes are bounded, and the title path is separately capped.
            if pending_bytes > available {
                return Err(IndexError::InvalidEvidence);
            }
            if n == 0 {
                source_counts.insert(hit.source_id.clone(), 1);
            } else {
                source_counts.insert(hit.source_id.clone(), n + 1);
            }
            if let Some(at) = group_at {
                groups[at].excerpts.extend(pending);
                if fetched.truncated {
                    groups[at].next_cursor = fetched.next_cursor;
                    groups[at].continuation_message_id = fetch_request.message_id.clone();
                }
            } else {
                groups.push(ContextGroup {
                    source_id: fetched.source_id,
                    archive_id: fetched.archive_id,
                    snapshot_id: fetched.snapshot_id,
                    conversation_id: fetched.conversation_id,
                    title: fetched.title,
                    source_url: fetched.source_url,
                    captured_at: fetched.captured_at,
                    completeness: fetched.completeness,
                    indexed_is_current: fetched.indexed_is_current,
                    excerpts: pending,
                    next_cursor: if fetched.truncated {
                        fetched.next_cursor
                    } else {
                        None
                    },
                    continuation_message_id: if fetched.truncated {
                        fetch_request.message_id.clone()
                    } else {
                        None
                    },
                });
            }
            used += pending_bytes;
            // Count only emitted segments after overlap deduplication.
            excerpt_count = groups.iter().map(|group| group.excerpts.len()).sum();
            if fetched.truncated && hit.kind != "title" {
                note_reason(&mut reasons, "partial_fetch");
                break;
            }
            if reasons.iter().any(|r| r == "text_budget") {
                break;
            }
        }
        check()?;
        let source_count = source_counts.len();
        Ok(ContextPack {
            preset: request.preset,
            groups,
            candidate_count: search.candidate_count,
            search_shortlist_truncated: search.truncated,
            text_bytes: used,
            estimated_tokens: used.div_ceil(4),
            excerpt_count,
            source_count,
            truncated: !reasons.is_empty() || search.truncated,
            truncation_reasons: reasons,
        })
    }
}

fn bounded_prefix(text: &str, bytes: usize) -> &str {
    let mut end = text.len().min(bytes);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

//! Phase 2A: bounded, provider-neutral retrieval over the disposable FTS projection.
//! No derived rowid is returned as a durable source reference.
use crate::{IndexError, IndexStore, Result};
use chrono::DateTime;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const MAX_QUERIES: usize = 8;
const MAX_QUERY_BYTES: usize = 1024;
const MAX_FILTER_VALUES: usize = 32;
const MAX_FILTER_BYTES: usize = 256;
const MAX_RESULTS: usize = 100;

fn default_weight() -> f64 {
    1.0
}
fn default_limit() -> usize {
    20
}

/// "Exact" means adjacent FTS5 tokens in the same order (not a byte-substring).
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MatchMode {
    #[default]
    Lexical,
    Exact,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WeightedQuery {
    pub text: String,
    #[serde(default = "default_weight")]
    pub weight: f64,
    #[serde(default)]
    pub mode: MatchMode,
}

/// Simple clients may send strings. Advanced clients can supply weight and mode.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum QueryTerm {
    Text(String),
    Weighted(WeightedQuery),
}
impl QueryTerm {
    fn parts(&self) -> (&str, f64, MatchMode) {
        match self {
            Self::Text(text) => (text, 1.0, MatchMode::Lexical),
            Self::Weighted(query) => (&query.text, query.weight, query.mode),
        }
    }
}

/// Filters operate only on fields actually projected in index schema v1.
/// Unknown fields (such as projects, filesystem roots, tags) fail closed rather
/// than silently broadening a search until their authorization-aware adapters exist.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct RetrievalFilters {
    pub include_history: bool,
    pub providers: Vec<String>,
    pub models: Vec<String>,
    pub roles: Vec<String>,
    pub kinds: Vec<String>,
    pub source_ids: Vec<String>,
    pub archive_ids: Vec<String>,
    pub conversation_ids: Vec<String>,
    pub message_ids: Vec<String>,
    /// Inclusive RFC3339 capture-time lower bound.
    pub captured_from: Option<String>,
    /// Inclusive RFC3339 capture-time upper bound.
    pub captured_to: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct RetrievalRequest {
    pub queries: Vec<QueryTerm>,
    pub filters: RetrievalFilters,
    pub limit: usize,
}
impl Default for RetrievalRequest {
    fn default() -> Self {
        Self {
            queries: Vec::new(),
            filters: RetrievalFilters::default(),
            limit: default_limit(),
        }
    }
}

/// Provenance is independent of ephemeral FTS rowids, and sufficient to route
/// a later Phase 2B fetch to the exact immutable archive revision and message.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct RetrievalHit {
    pub source_id: String,
    pub archive_id: String,
    pub snapshot_id: String,
    pub conversation_id: String,
    pub provider: String,
    pub source_url: String,
    pub captured_at: String,
    pub completeness: String,
    pub message_id: String,
    pub part: i64,
    pub kind: String,
    pub role: String,
    pub model: Option<String>,
    pub title: String,
    pub snippet: String,
    /// Higher is better; a deterministic weighted lexical/exact ranking.
    pub score: f64,
    /// Zero-based indexes of input queries matching this exact chunk.
    pub matched_queries: Vec<usize>,
    pub exact_queries: Vec<usize>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct RetrievalResponse {
    pub hits: Vec<RetrievalHit>,
    /// Unique retrieved candidates within the per-query bounded shortlist.
    /// This is NOT an estimate of all corpus matches.
    pub candidate_count: usize,
    pub truncated: bool,
}

fn validate_values(values: &[String]) -> Result<Option<String>> {
    if values.len() > MAX_FILTER_VALUES
        || values.iter().any(|value| {
            value.is_empty()
                || value.len() > MAX_FILTER_BYTES
                || value.chars().any(char::is_control)
        })
    {
        return Err(IndexError::InvalidQuery);
    }
    if values.is_empty() {
        Ok(None)
    } else {
        // JSON parameters, not SQL fragments; callers cannot inject predicates.
        serde_json::to_string(values)
            .map(Some)
            .map_err(|_| IndexError::InvalidQuery)
    }
}

fn expression(text: &str, mode: MatchMode) -> Result<String> {
    let text = text.trim();
    if text.is_empty()
        || text.len() > MAX_QUERY_BYTES
        || text.chars().any(char::is_control)
        || text.split_whitespace().count() > 24
    {
        return Err(IndexError::InvalidQuery);
    }
    // Always quote user terms: raw FTS syntax, boolean operators and prefix
    // operators must not escape the documented lexical/exact modes.
    let quote = |term: &str| format!("\"{}\"", term.replace('"', "\"\""));
    Ok(match mode {
        MatchMode::Lexical => text
            .split_whitespace()
            .map(quote)
            .collect::<Vec<_>>()
            .join(" AND "),
        MatchMode::Exact => quote(text),
    })
}

impl RetrievalFilters {
    fn validate(&self) -> Result<Vec<Option<String>>> {
        let encoded = [
            &self.providers,
            &self.models,
            &self.roles,
            &self.kinds,
            &self.source_ids,
            &self.archive_ids,
            &self.conversation_ids,
            &self.message_ids,
        ]
        .iter()
        .map(|values| validate_values(values))
        .collect::<Result<Vec<_>>>()?;
        let from = self
            .captured_from
            .as_deref()
            .map(|value| DateTime::parse_from_rfc3339(value).map_err(|_| IndexError::InvalidQuery))
            .transpose()?;
        let to = self
            .captured_to
            .as_deref()
            .map(|value| DateTime::parse_from_rfc3339(value).map_err(|_| IndexError::InvalidQuery))
            .transpose()?;
        if from.zip(to).is_some_and(|(from, to)| from > to) {
            return Err(IndexError::InvalidQuery);
        }
        Ok(encoded)
    }
}

impl IndexStore {
    /// Phase 2A retrieval primitive. Caller must enforce client authorization
    /// and check its unlock lease before disclosing the returned snippets.
    /// Later source adapters must apply root revocation before this point.
    pub fn retrieve(&self, request: &RetrievalRequest) -> Result<RetrievalResponse> {
        if request.queries.is_empty()
            || request.queries.len() > MAX_QUERIES
            || request.limit == 0
            || request.limit > MAX_RESULTS
        {
            return Err(IndexError::InvalidQuery);
        }
        let filters = request.filters.validate()?;
        let prepared = request
            .queries
            .iter()
            .map(|query| {
                let (text, weight, mode) = query.parts();
                if !weight.is_finite() || !(0.0 < weight && weight <= 10.0) {
                    return Err(IndexError::InvalidQuery);
                }
                Ok((expression(text, mode)?, weight, mode))
            })
            .collect::<Result<Vec<_>>>()?;

        // No dynamic SQL interpolation of user input. JSON1 membership binds
        // preserve exact structured matching and avoid oversized IN lists.
        let sql = "SELECT
              c.source_id,r.archive_id,c.snapshot_id,r.conversation_id,
              r.provider,r.source_url,r.captured_at,r.completeness,
              c.message_id,c.part,c.kind,c.role,c.model,r.title,
              snippet(chunks_fts,0,'[',']','…',18),bm25(chunks_fts)
            FROM chunks_fts
            JOIN chunks c ON c.id=chunks_fts.rowid
            JOIN revisions r ON r.source_id=c.source_id AND r.snapshot_id=c.snapshot_id
            WHERE chunks_fts MATCH ?1
              AND (?2=1 OR r.is_current=1)
              AND (?3 IS NULL OR r.provider IN (SELECT value FROM json_each(?3)))
              AND (?4 IS NULL OR c.model IN (SELECT value FROM json_each(?4)))
              AND (?5 IS NULL OR c.role IN (SELECT value FROM json_each(?5)))
              AND (?6 IS NULL OR c.kind IN (SELECT value FROM json_each(?6)))
              AND (?7 IS NULL OR c.source_id IN (SELECT value FROM json_each(?7)))
              AND (?8 IS NULL OR r.archive_id IN (SELECT value FROM json_each(?8)))
              AND (?9 IS NULL OR r.conversation_id IN (SELECT value FROM json_each(?9)))
              AND (?10 IS NULL OR c.message_id IN (SELECT value FROM json_each(?10)))
              AND (?11 IS NULL OR julianday(r.captured_at) >= julianday(?11))
              AND (?12 IS NULL OR julianday(r.captured_at) <= julianday(?12))
            ORDER BY bm25(chunks_fts),c.source_id,c.snapshot_id,c.message_id,c.part
            LIMIT ?13";

        let db = self.connection()?;
        let mut statement = db.prepare(sql)?;
        type Key = (String, String, String, String, i64);
        let mut candidates: BTreeMap<Key, RetrievalHit> = BTreeMap::new();
        // Bound both work and memory. Multiplying the requested limit provides
        // some headroom for merging/reranking multiple query shortlists.
        let per_query_limit = (request.limit * 8).min(800) as i64;

        for (index, (fts, weight, mode)) in prepared.iter().enumerate() {
            let rows = statement.query_map(
                params![
                    fts,
                    i64::from(request.filters.include_history),
                    filters[0].as_deref(),
                    filters[1].as_deref(),
                    filters[2].as_deref(),
                    filters[3].as_deref(),
                    filters[4].as_deref(),
                    filters[5].as_deref(),
                    filters[6].as_deref(),
                    filters[7].as_deref(),
                    request.filters.captured_from.as_deref(),
                    request.filters.captured_to.as_deref(),
                    per_query_limit,
                ],
                |row| {
                    let bm25: f64 = row.get(15)?;
                    Ok((
                        RetrievalHit {
                            source_id: row.get(0)?,
                            archive_id: row.get(1)?,
                            snapshot_id: row.get(2)?,
                            conversation_id: row.get(3)?,
                            provider: row.get(4)?,
                            source_url: row.get(5)?,
                            captured_at: row.get(6)?,
                            completeness: row.get(7)?,
                            message_id: row.get(8)?,
                            part: row.get(9)?,
                            kind: row.get(10)?,
                            role: row.get(11)?,
                            model: row.get(12)?,
                            title: row.get(13)?,
                            snippet: row.get(14)?,
                            score: 0.0,
                            matched_queries: Vec::new(),
                            exact_queries: Vec::new(),
                        },
                        bm25,
                    ))
                },
            )?;
            for row in rows {
                let (hit, bm25) = row?;
                let key = (
                    hit.source_id.clone(),
                    hit.snapshot_id.clone(),
                    hit.kind.clone(),
                    hit.message_id.clone(),
                    hit.part,
                );
                let candidate = candidates.entry(key).or_insert(hit);
                // FTS5's smaller/more-negative bm25 is better. Transform to
                // an ascending finite relevance measure; exact phrase adds an
                // explicit bounded bonus rather than relying on opaque SQL ties.
                let relevance = (-bm25).max(0.0).ln_1p();
                let exact_bonus = if *mode == MatchMode::Exact { 1.0 } else { 0.0 };
                candidate.score += weight * (relevance + exact_bonus);
                candidate.matched_queries.push(index);
                if *mode == MatchMode::Exact {
                    candidate.exact_queries.push(index);
                }
            }
        }
        let candidate_count = candidates.len();
        let mut hits: Vec<_> = candidates.into_values().collect();
        hits.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.source_id.cmp(&b.source_id))
                .then_with(|| a.snapshot_id.cmp(&b.snapshot_id))
                .then_with(|| a.kind.cmp(&b.kind))
                .then_with(|| a.message_id.cmp(&b.message_id))
                .then_with(|| a.part.cmp(&b.part))
        });
        let truncated = hits.len() > request.limit;
        hits.truncate(request.limit);
        Ok(RetrievalResponse {
            hits,
            candidate_count,
            truncated,
        })
    }
}

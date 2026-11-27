//! MP-R1-WP03: Curated Node Format (CNF v1) authoritative state store.
//!
//! Stores staged curation generations, immutable per-shard op logs with
//! span-level evidence (`src`, `msg`, `span`, `sha`, optional `quote`), and
//! the materialized accepted curated nodes, revisions, aliases and edges.
//!
//! MCP tool handlers and the deterministic archive-span ingest gate belong to
//! `MP-R1-WP04` / `MP-R1-WP05` and are deliberately absent here.
use crate::{
    source_record,
    topics::{fetch_topic, next_curated_clock, valid_cnf_slug, AssertionSource},
    valid_id, valid_text, verify_source_record, Result, StateError, StateStore,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub const MAX_SHARDS_PER_GENERATION: usize = 64;
pub const MAX_RECORDS_PER_SHARD: usize = 2_000;
pub const MAX_RECORDS_PER_APPEND: usize = 200;
pub const MAX_CNF_RECORD_BYTES: usize = 8 * 1024;
pub const MAX_EVIDENCE_SPANS_PER_RECORD: usize = 16;
pub const MAX_NODE_TITLE_CHARS: usize = 120;
pub const MAX_NODE_SUMMARY_CHARS: usize = 400;
pub const MAX_NODE_BODY_BYTES: usize = 4 * 1024;
pub const MAX_NODE_ATTRS: usize = 16;
pub const MAX_NODE_ALIASES: usize = 16;
pub const MAX_EVIDENCE_QUOTE_BYTES: usize = 8 * 1024;
const MAX_QUERY_LIMIT: usize = 500;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CuratedNodeKind {
    Topic,
    Entity,
    Concept,
    Decision,
    Fact,
    Question,
    Task,
    Event,
    Artifact,
}

impl CuratedNodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Topic => "topic",
            Self::Entity => "entity",
            Self::Concept => "concept",
            Self::Decision => "decision",
            Self::Fact => "fact",
            Self::Question => "question",
            Self::Task => "task",
            Self::Event => "event",
            Self::Artifact => "artifact",
        }
    }

    pub fn parse(raw: &str) -> Result<Self> {
        match raw {
            "topic" => Ok(Self::Topic),
            "entity" => Ok(Self::Entity),
            "concept" => Ok(Self::Concept),
            "decision" => Ok(Self::Decision),
            "fact" => Ok(Self::Fact),
            "question" => Ok(Self::Question),
            "task" => Ok(Self::Task),
            "event" => Ok(Self::Event),
            "artifact" => Ok(Self::Artifact),
            _ => Err(StateError::InvalidValue),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CuratedEdgeRel {
    PartOf,
    About,
    DependsOn,
    Supports,
    Contradicts,
    Supersedes,
    DecidedIn,
    Mentions,
    RelatedTo,
}

impl CuratedEdgeRel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PartOf => "part_of",
            Self::About => "about",
            Self::DependsOn => "depends_on",
            Self::Supports => "supports",
            Self::Contradicts => "contradicts",
            Self::Supersedes => "supersedes",
            Self::DecidedIn => "decided_in",
            Self::Mentions => "mentions",
            Self::RelatedTo => "related_to",
        }
    }

    pub fn parse(raw: &str) -> Result<Self> {
        match raw {
            "part_of" => Ok(Self::PartOf),
            "about" => Ok(Self::About),
            "depends_on" => Ok(Self::DependsOn),
            "supports" => Ok(Self::Supports),
            "contradicts" => Ok(Self::Contradicts),
            "supersedes" => Ok(Self::Supersedes),
            "decided_in" => Ok(Self::DecidedIn),
            "mentions" => Ok(Self::Mentions),
            "related_to" => Ok(Self::RelatedTo),
            _ => Err(StateError::InvalidValue),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CurationProfile {
    Exact,
    Detailed,
    Brief,
}

impl CurationProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Detailed => "detailed",
            Self::Brief => "brief",
        }
    }

    pub fn parse(raw: &str) -> Result<Self> {
        match raw {
            "exact" => Ok(Self::Exact),
            "detailed" => Ok(Self::Detailed),
            "brief" => Ok(Self::Brief),
            _ => Err(StateError::InvalidValue),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GenerationStatus {
    Staged,
    Published,
    Rejected,
}

impl GenerationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Staged => "staged",
            Self::Published => "published",
            Self::Rejected => "rejected",
        }
    }

    pub(crate) fn parse(raw: &str) -> Result<Self> {
        match raw {
            "staged" => Ok(Self::Staged),
            "published" => Ok(Self::Published),
            "rejected" => Ok(Self::Rejected),
            _ => Err(StateError::Corrupt),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CuratedNodeStatus {
    Active,
    Superseded,
    Retired,
}

impl CuratedNodeStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Superseded => "superseded",
            Self::Retired => "retired",
        }
    }

    pub(crate) fn parse(raw: &str) -> Result<Self> {
        match raw {
            "active" => Ok(Self::Active),
            "superseded" => Ok(Self::Superseded),
            "retired" => Ok(Self::Retired),
            _ => Err(StateError::Corrupt),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CuratedEdgeStatus {
    Active,
    Unlinked,
}

impl CuratedEdgeStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Unlinked => "unlinked",
        }
    }

    pub(crate) fn parse(raw: &str) -> Result<Self> {
        match raw {
            "active" => Ok(Self::Active),
            "unlinked" => Ok(Self::Unlinked),
            _ => Err(StateError::Corrupt),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CuratedKey {
    pub topic: String,
    pub kind: CuratedNodeKind,
    pub slug: String,
    pub canonical: String,
}

impl CuratedKey {
    /// Parses `<topic>/<kind>/<slug>` strictly per CNF v1.
    pub fn parse(raw: &str) -> Result<Self> {
        let mut parts = raw.split('/');
        let Some(topic) = parts.next() else {
            return Err(StateError::InvalidValue);
        };
        let Some(kind_str) = parts.next() else {
            return Err(StateError::InvalidValue);
        };
        let Some(slug) = parts.next() else {
            return Err(StateError::InvalidValue);
        };
        if parts.next().is_some() {
            return Err(StateError::InvalidValue);
        }
        valid_cnf_slug(topic)?;
        let kind = CuratedNodeKind::parse(kind_str)?;
        valid_cnf_slug(slug)?;
        Ok(Self {
            topic: topic.to_owned(),
            kind,
            slug: slug.to_owned(),
            canonical: raw.to_owned(),
        })
    }
}

/// Deterministic internal node identifier derived from canonical key `<topic>/<kind>/<slug>`.
pub fn curated_node_id_for_key(canonical_key: &str) -> Result<String> {
    let parsed = CuratedKey::parse(canonical_key)?;
    let mut hash = Sha256::new();
    hash.update(b"mind-place:curated-node:v1\0");
    hash.update((parsed.canonical.len() as u64).to_be_bytes());
    hash.update(parsed.canonical.as_bytes());
    Ok(format!("cn:v1:{:x}", hash.finalize()))
}

/// Deterministic edge identifier derived from `(from_node_id, rel, to_node_id)`.
pub fn curated_edge_id(
    from_node_id: &str,
    rel: CuratedEdgeRel,
    to_node_id: &str,
) -> Result<String> {
    valid_id(from_node_id)?;
    valid_id(to_node_id)?;
    if from_node_id == to_node_id {
        return Err(StateError::InvalidValue);
    }
    let mut hash = Sha256::new();
    hash.update(b"mind-place:curated-edge:v1\0");
    for part in [from_node_id, rel.as_str(), to_node_id] {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part.as_bytes());
    }
    Ok(format!("ce:v1:{:x}", hash.finalize()))
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum AttrValue {
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EvidenceSpan {
    pub src: String,
    pub msg: i64,
    pub span: [i64; 2],
    pub sha: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum CnfRecord {
    Upsert {
        key: String,
        kind: CuratedNodeKind,
        title: String,
        summary: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
        #[serde(default)]
        attrs: BTreeMap<String, AttrValue>,
        #[serde(default)]
        aliases: Vec<String>,
        evidence: Vec<EvidenceSpan>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        confidence: Option<f64>,
        #[serde(default)]
        base_rev: i64,
    },
    Patch {
        key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
        #[serde(default)]
        clear_body: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        attrs: Option<BTreeMap<String, AttrValue>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        aliases: Option<Vec<String>>,
        evidence: Vec<EvidenceSpan>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        confidence: Option<f64>,
        base_rev: i64,
    },
    Supersede {
        key: String,
        kind: CuratedNodeKind,
        supersedes: String,
        title: String,
        summary: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
        #[serde(default)]
        attrs: BTreeMap<String, AttrValue>,
        #[serde(default)]
        aliases: Vec<String>,
        evidence: Vec<EvidenceSpan>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        confidence: Option<f64>,
        #[serde(default)]
        base_rev: i64,
    },
    Retire {
        key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        evidence: Vec<EvidenceSpan>,
        base_rev: i64,
    },
    Link {
        from: String,
        rel: CuratedEdgeRel,
        to: String,
        evidence: Vec<EvidenceSpan>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        confidence: Option<f64>,
    },
    Unlink {
        from: String,
        rel: CuratedEdgeRel,
        to: String,
        evidence: Vec<EvidenceSpan>,
    },
}

impl CnfRecord {
    /// Parses and strictly validates one CNF v1 JSONL line (`<= 8 KiB`, UTF-8, no unknown fields).
    pub fn from_json_line(line: &str) -> Result<Self> {
        if line.is_empty()
            || line.len() > MAX_CNF_RECORD_BYTES
            || line.contains('\n')
            || line.contains('\r')
            || line.contains('\0')
        {
            return Err(StateError::InvalidValue);
        }
        let record: Self = serde_json::from_str(line).map_err(|_| StateError::InvalidValue)?;
        record.validate_shape()?;
        Ok(record)
    }

    pub fn op_name(&self) -> &'static str {
        match self {
            Self::Upsert { .. } => "upsert",
            Self::Patch { .. } => "patch",
            Self::Supersede { .. } => "supersede",
            Self::Retire { .. } => "retire",
            Self::Link { .. } => "link",
            Self::Unlink { .. } => "unlink",
        }
    }

    pub fn target_topic(&self) -> Result<String> {
        match self {
            Self::Upsert { key, .. }
            | Self::Patch { key, .. }
            | Self::Supersede { key, .. }
            | Self::Retire { key, .. } => Ok(CuratedKey::parse(key)?.topic),
            Self::Link { from, .. } | Self::Unlink { from, .. } => {
                Ok(CuratedKey::parse(from)?.topic)
            }
        }
    }

    pub fn evidence(&self) -> &[EvidenceSpan] {
        match self {
            Self::Upsert { evidence, .. }
            | Self::Patch { evidence, .. }
            | Self::Supersede { evidence, .. }
            | Self::Retire { evidence, .. }
            | Self::Link { evidence, .. }
            | Self::Unlink { evidence, .. } => evidence,
        }
    }

    pub(crate) fn validate_shape(&self) -> Result<()> {
        let encoded = serde_json::to_vec(self).map_err(|_| StateError::InvalidValue)?;
        if encoded.len() > MAX_CNF_RECORD_BYTES {
            return Err(StateError::CuratedLimit);
        }
        validate_evidence_list(self.evidence(), false)?;
        match self {
            Self::Upsert {
                key,
                kind,
                title,
                summary,
                body,
                attrs,
                aliases,
                confidence,
                base_rev,
                ..
            } => {
                let parsed = CuratedKey::parse(key)?;
                if parsed.kind != *kind || *base_rev < 0 {
                    return Err(StateError::InvalidValue);
                }
                validate_title(title)?;
                validate_summary(summary)?;
                if let Some(b) = body.as_deref() {
                    validate_body(b)?;
                }
                validate_attrs(attrs)?;
                validate_aliases(aliases)?;
                validate_confidence(*confidence)?;
            }
            Self::Patch {
                key,
                title,
                summary,
                body,
                clear_body,
                attrs,
                aliases,
                confidence,
                base_rev,
                ..
            } => {
                let _ = CuratedKey::parse(key)?;
                if *base_rev < 1 || (*clear_body && body.is_some()) {
                    return Err(StateError::InvalidValue);
                }
                if let Some(t) = title.as_deref() {
                    validate_title(t)?;
                }
                if let Some(s) = summary.as_deref() {
                    validate_summary(s)?;
                }
                if let Some(b) = body.as_deref() {
                    validate_body(b)?;
                }
                if let Some(map) = attrs.as_ref() {
                    validate_attrs(map)?;
                }
                if let Some(list) = aliases.as_ref() {
                    validate_aliases(list)?;
                }
                validate_confidence(*confidence)?;
            }
            Self::Supersede {
                key,
                kind,
                supersedes,
                title,
                summary,
                body,
                attrs,
                aliases,
                confidence,
                base_rev,
                ..
            } => {
                let parsed = CuratedKey::parse(key)?;
                let old = CuratedKey::parse(supersedes)?;
                if parsed.kind != *kind || parsed.canonical == old.canonical || *base_rev < 0 {
                    return Err(StateError::InvalidValue);
                }
                validate_title(title)?;
                validate_summary(summary)?;
                if let Some(b) = body.as_deref() {
                    validate_body(b)?;
                }
                validate_attrs(attrs)?;
                validate_aliases(aliases)?;
                validate_confidence(*confidence)?;
            }
            Self::Retire {
                key,
                reason,
                base_rev,
                ..
            } => {
                let _ = CuratedKey::parse(key)?;
                if *base_rev < 1 {
                    return Err(StateError::InvalidValue);
                }
                if let Some(r) = reason.as_deref() {
                    valid_text(r)?;
                    if r.chars().count() > MAX_NODE_SUMMARY_CHARS {
                        return Err(StateError::InvalidValue);
                    }
                }
            }
            Self::Link {
                from,
                to,
                confidence,
                ..
            } => {
                let f = CuratedKey::parse(from)?;
                let t = CuratedKey::parse(to)?;
                if f.canonical == t.canonical {
                    return Err(StateError::InvalidValue);
                }
                validate_confidence(*confidence)?;
            }
            Self::Unlink { from, to, .. } => {
                let f = CuratedKey::parse(from)?;
                let t = CuratedKey::parse(to)?;
                if f.canonical == t.canonical {
                    return Err(StateError::InvalidValue);
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct NewCurationGeneration<'a> {
    pub id: &'a str,
    pub topic_id: &'a str,
    pub run_id: &'a str,
    pub profile: CurationProfile,
    pub asserted_by: AssertionSource,
    pub model_id: Option<&'a str>,
    pub base_generation_id: Option<&'a str>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct CurationGeneration {
    pub id: String,
    pub topic_id: String,
    pub run_id: String,
    pub profile: CurationProfile,
    pub asserted_by: AssertionSource,
    pub model_id: Option<String>,
    pub status: GenerationStatus,
    pub base_generation_id: Option<String>,
    pub projection_sha256: Option<String>,
    pub created_at_ms: i64,
    pub published_at_ms: Option<i64>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct RecordedCuratedOp {
    pub generation_id: String,
    pub shard_id: String,
    pub seq: i64,
    pub op: String,
    pub target_key: String,
    pub record_sha256: String,
    pub recorded_at_ms: i64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct CuratedNode {
    pub id: String,
    pub topic_id: String,
    pub canonical_key: String,
    pub kind: CuratedNodeKind,
    pub slug: String,
    pub current_rev: i64,
    pub title: String,
    pub summary: String,
    pub body: Option<String>,
    pub attrs: BTreeMap<String, AttrValue>,
    pub aliases: Vec<String>,
    pub status: CuratedNodeStatus,
    pub supersedes_node_id: Option<String>,
    pub superseded_by_node_id: Option<String>,
    pub generation_id: String,
    pub asserted_by: AssertionSource,
    pub model_id: Option<String>,
    pub confidence: Option<f64>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct CuratedNodeRevision {
    pub node_id: String,
    pub rev: i64,
    pub base_rev: i64,
    pub generation_id: String,
    pub op: String,
    pub canonical_key: String,
    pub kind: CuratedNodeKind,
    pub title: String,
    pub summary: String,
    pub body: Option<String>,
    pub attrs: BTreeMap<String, AttrValue>,
    pub aliases: Vec<String>,
    pub status: CuratedNodeStatus,
    pub supersedes_node_id: Option<String>,
    pub superseded_by_node_id: Option<String>,
    pub asserted_by: AssertionSource,
    pub model_id: Option<String>,
    pub confidence: Option<f64>,
    pub recorded_at_ms: i64,
    pub evidence: Vec<EvidenceSpan>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct CuratedEdge {
    pub id: String,
    pub topic_id: String,
    pub from_node_id: String,
    pub from_key: String,
    pub rel: CuratedEdgeRel,
    pub to_node_id: String,
    pub to_key: String,
    pub status: CuratedEdgeStatus,
    pub generation_id: String,
    pub asserted_by: AssertionSource,
    pub model_id: Option<String>,
    pub confidence: Option<f64>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub evidence: Vec<EvidenceSpan>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct PublishedGenerationReceipt {
    pub generation_id: String,
    pub topic_id: String,
    pub ops_applied: usize,
    pub published_at_ms: i64,
    pub projection_sha256: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct CuratedRebuildReceipt {
    pub published_generations: usize,
    pub ops_replayed: usize,
    pub node_count: usize,
    pub revision_count: usize,
    pub edge_count: usize,
    pub projection_sha256: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct CuratedIntegrityReport {
    pub foreign_key_violations: usize,
    pub op_log_count: usize,
    pub node_count: usize,
    pub revision_count: usize,
    pub edge_count: usize,
    pub projection_sha256: String,
}

fn valid_hex_64(raw: &str) -> bool {
    raw.len() == 64
        && raw
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn valid_span_sha(raw: &str) -> bool {
    if let Some(rest) = raw.strip_prefix("b3:") {
        return valid_hex_64(rest);
    }
    if let Some(rest) = raw.strip_prefix("sha256:") {
        return valid_hex_64(rest);
    }
    false
}

fn validate_title(title: &str) -> Result<()> {
    valid_text(title)?;
    if title.trim().chars().count() > MAX_NODE_TITLE_CHARS || title.chars().any(char::is_control) {
        return Err(StateError::InvalidValue);
    }
    Ok(())
}

fn validate_summary(summary: &str) -> Result<()> {
    valid_text(summary)?;
    if summary.trim().chars().count() > MAX_NODE_SUMMARY_CHARS {
        return Err(StateError::InvalidValue);
    }
    Ok(())
}

fn validate_body(body: &str) -> Result<()> {
    valid_text(body)?;
    if body.len() > MAX_NODE_BODY_BYTES {
        return Err(StateError::CuratedLimit);
    }
    Ok(())
}

fn validate_confidence(confidence: Option<f64>) -> Result<()> {
    if let Some(c) = confidence {
        if !c.is_finite() || !(0.0..=1.0).contains(&c) {
            return Err(StateError::InvalidValue);
        }
    }
    Ok(())
}

fn normalize_confidence(confidence: Option<f64>) -> Option<f64> {
    confidence.map(|c| if c == 0.0 { 0.0 } else { c })
}

fn validate_attr_key(key: &str) -> Result<()> {
    let mut bytes = key.bytes();
    let Some(first) = bytes.next() else {
        return Err(StateError::InvalidValue);
    };
    if !first.is_ascii_lowercase() || key.len() > 48 {
        return Err(StateError::InvalidValue);
    }
    if !bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_') {
        return Err(StateError::InvalidValue);
    }
    Ok(())
}

fn validate_attrs(attrs: &BTreeMap<String, AttrValue>) -> Result<()> {
    if attrs.len() > MAX_NODE_ATTRS {
        return Err(StateError::CuratedLimit);
    }
    for (k, v) in attrs {
        validate_attr_key(k)?;
        match v {
            AttrValue::Bool(_) | AttrValue::Integer(_) => {}
            AttrValue::Float(f) => {
                if !f.is_finite() {
                    return Err(StateError::InvalidValue);
                }
            }
            AttrValue::String(s) => {
                if s.chars().count() > 512 || s.chars().any(char::is_control) {
                    return Err(StateError::InvalidValue);
                }
            }
        }
    }
    Ok(())
}

fn normalize_aliases(aliases: &[String]) -> Result<Vec<String>> {
    if aliases.len() > MAX_NODE_ALIASES {
        return Err(StateError::CuratedLimit);
    }
    let mut set = BTreeSet::new();
    for raw in aliases {
        let trimmed = raw.trim();
        if trimmed.is_empty()
            || trimmed.chars().count() > MAX_NODE_TITLE_CHARS
            || trimmed.chars().any(char::is_control)
        {
            return Err(StateError::InvalidValue);
        }
        set.insert(trimmed.to_owned());
    }
    Ok(set.into_iter().collect())
}

fn validate_aliases(aliases: &[String]) -> Result<()> {
    let _ = normalize_aliases(aliases)?;
    Ok(())
}

fn validate_evidence_list(evidence: &[EvidenceSpan], require_quote: bool) -> Result<()> {
    if evidence.is_empty() || evidence.len() > MAX_EVIDENCE_SPANS_PER_RECORD {
        return Err(StateError::InvalidValue);
    }
    let mut seen = BTreeSet::new();
    for item in evidence {
        valid_id(&item.src)?;
        if !item.src.starts_with("src:v1:") || !valid_hex_64(&item.src[7..]) {
            return Err(StateError::InvalidValue);
        }
        if item.msg < 0 || item.span[0] < 0 || item.span[1] <= item.span[0] {
            return Err(StateError::InvalidValue);
        }
        if !valid_span_sha(&item.sha) {
            return Err(StateError::InvalidValue);
        }
        if let Some(q) = item.quote.as_deref() {
            if q.is_empty() || q.len() > MAX_EVIDENCE_QUOTE_BYTES || q.contains('\0') {
                return Err(StateError::InvalidValue);
            }
        } else if require_quote {
            return Err(StateError::InvalidValue);
        }
        if !seen.insert((
            item.src.as_str(),
            item.msg,
            item.span[0],
            item.span[1],
            item.sha.as_str(),
        )) {
            return Err(StateError::InvalidValue);
        }
    }
    Ok(())
}

fn verify_registered_sources(db: &Connection, evidence: &[EvidenceSpan]) -> Result<()> {
    for item in evidence {
        let record = db
            .query_row(
                "SELECT id,provider,source_namespace,source_kind,key_type,source_key
                 FROM source_identities WHERE id=?1",
                [&item.src],
                source_record,
            )
            .optional()?
            .ok_or(StateError::InvalidValue)?;
        verify_source_record(&record)?;
        crate::source_redaction::assert_source_not_redacted(db, &item.src)?;
    }
    Ok(())
}

fn generation_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<(
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
    i64,
    Option<i64>,
)> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
    ))
}

fn build_generation(
    tuple: (
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        i64,
        Option<i64>,
    ),
) -> Result<CurationGeneration> {
    Ok(CurationGeneration {
        id: tuple.0,
        topic_id: tuple.1,
        run_id: tuple.2,
        profile: CurationProfile::parse(&tuple.3).map_err(|_| StateError::Corrupt)?,
        asserted_by: AssertionSource::parse(&tuple.4)?,
        model_id: tuple.5,
        status: GenerationStatus::parse(&tuple.6)?,
        base_generation_id: tuple.7,
        projection_sha256: tuple.8,
        created_at_ms: tuple.9,
        published_at_ms: tuple.10,
    })
}

fn fetch_generation(db: &Connection, generation_id: &str) -> Result<Option<CurationGeneration>> {
    let raw = db
        .query_row(
            "SELECT id,topic_id,run_id,profile,asserted_by,model_id,status,
                    base_generation_id,projection_sha256,created_at_ms,published_at_ms
             FROM curation_generations WHERE id=?1",
            [generation_id],
            generation_from_row,
        )
        .optional()?;
    raw.map(build_generation).transpose()
}

struct StoredOp {
    generation_id: String,
    shard_id: String,
    seq: i64,
    record: CnfRecord,
    record_sha256: String,
    recorded_at_ms: i64,
    asserted_by: AssertionSource,
    model_id: Option<String>,
    topic_id: String,
    profile: CurationProfile,
}

fn load_generation_ops(db: &Connection, generation: &CurationGeneration) -> Result<Vec<StoredOp>> {
    let mut stmt = db.prepare(
        "SELECT shard_id,seq,payload_json,record_sha256,recorded_at_ms
         FROM curated_op_log WHERE generation_id=?1",
    )?;
    let raw_ops = stmt
        .query_map([&generation.id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(stmt);

    let mut ops = Vec::with_capacity(raw_ops.len());
    for (shard_id, seq, payload_json, record_sha256, recorded_at_ms) in raw_ops {
        let actual_sha = format!("{:x}", Sha256::digest(payload_json.as_bytes()));
        if actual_sha != record_sha256 {
            return Err(StateError::Corrupt);
        }
        let record = CnfRecord::from_json_line(&payload_json).map_err(|_| StateError::Corrupt)?;
        let mut ev_stmt = db.prepare(
            "SELECT source_ref,msg_index,span_start,span_end,span_sha,quote
             FROM curated_op_evidence_spans
             WHERE generation_id=?1 AND shard_id=?2 AND seq=?3
             ORDER BY ordinal",
        )?;
        let stored_ev = ev_stmt
            .query_map(params![&generation.id, &shard_id, seq], |r| {
                Ok(EvidenceSpan {
                    src: r.get(0)?,
                    msg: r.get(1)?,
                    span: [r.get(2)?, r.get(3)?],
                    sha: r.get(4)?,
                    quote: r.get(5)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if stored_ev != record.evidence() {
            return Err(StateError::Corrupt);
        }
        ops.push(StoredOp {
            generation_id: generation.id.clone(),
            shard_id,
            seq,
            record,
            record_sha256,
            recorded_at_ms,
            asserted_by: generation.asserted_by,
            model_id: generation.model_id.clone(),
            topic_id: generation.topic_id.clone(),
            profile: generation.profile,
        });
    }
    ops.sort_by(|a, b| {
        (a.shard_id == "reconcile", &a.shard_id, a.seq).cmp(&(
            b.shard_id == "reconcile",
            &b.shard_id,
            b.seq,
        ))
    });
    Ok(ops)
}

#[derive(Clone, Debug)]
struct MaterializedNode {
    id: String,
    topic_id: String,
    canonical_key: String,
    kind: CuratedNodeKind,
    slug: String,
    current_rev: i64,
    title: String,
    summary: String,
    body: Option<String>,
    attrs: BTreeMap<String, AttrValue>,
    aliases: Vec<String>,
    status: CuratedNodeStatus,
    supersedes_node_id: Option<String>,
    superseded_by_node_id: Option<String>,
    generation_id: String,
    asserted_by: AssertionSource,
    model_id: Option<String>,
    confidence: Option<f64>,
    created_at_ms: i64,
    updated_at_ms: i64,
}

#[derive(Clone, Debug)]
struct MaterializedRevision {
    node_id: String,
    rev: i64,
    base_rev: i64,
    generation_id: String,
    op: String,
    canonical_key: String,
    kind: CuratedNodeKind,
    title: String,
    summary: String,
    body: Option<String>,
    attrs: BTreeMap<String, AttrValue>,
    aliases: Vec<String>,
    status: CuratedNodeStatus,
    supersedes_node_id: Option<String>,
    superseded_by_node_id: Option<String>,
    asserted_by: AssertionSource,
    model_id: Option<String>,
    confidence: Option<f64>,
    recorded_at_ms: i64,
    evidence: Vec<EvidenceSpan>,
}

#[derive(Clone, Debug)]
struct MaterializedEdge {
    id: String,
    topic_id: String,
    from_node_id: String,
    rel: CuratedEdgeRel,
    to_node_id: String,
    status: CuratedEdgeStatus,
    generation_id: String,
    asserted_by: AssertionSource,
    model_id: Option<String>,
    confidence: Option<f64>,
    created_at_ms: i64,
    updated_at_ms: i64,
    evidence: Vec<EvidenceSpan>,
}

#[derive(Default)]
struct CuratedProjectionState {
    nodes_by_key: BTreeMap<String, MaterializedNode>,
    key_by_id: HashMap<String, String>,
    aliases_by_topic: BTreeMap<(String, String), String>,
    revisions: Vec<MaterializedRevision>,
    edges: BTreeMap<String, MaterializedEdge>,
}

impl CuratedProjectionState {
    fn apply_op(&mut self, op: &StoredOp) -> Result<()> {
        let _ = &op.record_sha256;
        match &op.record {
            CnfRecord::Upsert {
                key,
                kind,
                title,
                summary,
                body,
                attrs,
                aliases,
                evidence,
                confidence,
                base_rev,
            } => {
                if op.profile == CurationProfile::Brief && body.is_some() {
                    return Err(StateError::InvalidValue);
                }
                let parsed = CuratedKey::parse(key)?;
                let node_id = curated_node_id_for_key(&parsed.canonical)?;
                let norm_aliases = normalize_aliases(aliases)?;
                let norm_conf = normalize_confidence(*confidence);
                let (new_rev, created_at_ms, supersedes_node_id) =
                    if let Some(existing) = self.nodes_by_key.get(&parsed.canonical) {
                        if existing.status != CuratedNodeStatus::Active {
                            return Err(StateError::InvalidValue);
                        }
                        if *base_rev != existing.current_rev {
                            return Err(StateError::RevisionConflict);
                        }
                        (
                            existing.current_rev + 1,
                            existing.created_at_ms,
                            existing.supersedes_node_id.clone(),
                        )
                    } else {
                        if *base_rev != 0 {
                            return Err(StateError::RevisionConflict);
                        }
                        (1, op.recorded_at_ms, None)
                    };
                self.replace_aliases(&op.topic_id, &node_id, &norm_aliases)?;
                let node = MaterializedNode {
                    id: node_id.clone(),
                    topic_id: op.topic_id.clone(),
                    canonical_key: parsed.canonical.clone(),
                    kind: *kind,
                    slug: parsed.slug,
                    current_rev: new_rev,
                    title: title.trim().to_owned(),
                    summary: summary.trim().to_owned(),
                    body: body.clone(),
                    attrs: attrs.clone(),
                    aliases: norm_aliases.clone(),
                    status: CuratedNodeStatus::Active,
                    supersedes_node_id: supersedes_node_id.clone(),
                    superseded_by_node_id: None,
                    generation_id: op.generation_id.clone(),
                    asserted_by: op.asserted_by,
                    model_id: op.model_id.clone(),
                    confidence: norm_conf,
                    created_at_ms,
                    updated_at_ms: op.recorded_at_ms,
                };
                self.key_by_id
                    .insert(node_id.clone(), parsed.canonical.clone());
                self.nodes_by_key
                    .insert(parsed.canonical.clone(), node.clone());
                self.revisions.push(MaterializedRevision {
                    node_id,
                    rev: new_rev,
                    base_rev: *base_rev,
                    generation_id: op.generation_id.clone(),
                    op: "upsert".into(),
                    canonical_key: parsed.canonical,
                    kind: *kind,
                    title: node.title,
                    summary: node.summary,
                    body: node.body,
                    attrs: node.attrs,
                    aliases: norm_aliases,
                    status: CuratedNodeStatus::Active,
                    supersedes_node_id,
                    superseded_by_node_id: None,
                    asserted_by: op.asserted_by,
                    model_id: op.model_id.clone(),
                    confidence: norm_conf,
                    recorded_at_ms: op.recorded_at_ms,
                    evidence: evidence.clone(),
                });
            }
            CnfRecord::Patch {
                key,
                title,
                summary,
                body,
                clear_body,
                attrs,
                aliases,
                evidence,
                confidence,
                base_rev,
            } => {
                if op.profile == CurationProfile::Brief && body.is_some() {
                    return Err(StateError::InvalidValue);
                }
                let parsed = CuratedKey::parse(key)?;
                let existing = self
                    .nodes_by_key
                    .get(&parsed.canonical)
                    .cloned()
                    .ok_or(StateError::InvalidValue)?;
                if existing.status != CuratedNodeStatus::Active {
                    return Err(StateError::InvalidValue);
                }
                if *base_rev != existing.current_rev {
                    return Err(StateError::RevisionConflict);
                }
                let next_title = title
                    .as_deref()
                    .map(|t| t.trim().to_owned())
                    .unwrap_or(existing.title);
                let next_summary = summary
                    .as_deref()
                    .map(|s| s.trim().to_owned())
                    .unwrap_or(existing.summary);
                let next_body = if *clear_body {
                    None
                } else if body.is_some() {
                    body.clone()
                } else {
                    existing.body
                };
                let mut next_attrs = existing.attrs;
                if let Some(patch_attrs) = attrs {
                    for (k, v) in patch_attrs {
                        next_attrs.insert(k.clone(), v.clone());
                    }
                    validate_attrs(&next_attrs)?;
                }
                let next_aliases = if let Some(list) = aliases {
                    let norm = normalize_aliases(list)?;
                    self.replace_aliases(&existing.topic_id, &existing.id, &norm)?;
                    norm
                } else {
                    existing.aliases
                };
                let new_rev = existing.current_rev + 1;
                let norm_conf = normalize_confidence(*confidence).or(existing.confidence);
                let node = MaterializedNode {
                    id: existing.id.clone(),
                    topic_id: existing.topic_id.clone(),
                    canonical_key: existing.canonical_key.clone(),
                    kind: existing.kind,
                    slug: existing.slug,
                    current_rev: new_rev,
                    title: next_title.clone(),
                    summary: next_summary.clone(),
                    body: next_body.clone(),
                    attrs: next_attrs.clone(),
                    aliases: next_aliases.clone(),
                    status: CuratedNodeStatus::Active,
                    supersedes_node_id: existing.supersedes_node_id.clone(),
                    superseded_by_node_id: None,
                    generation_id: op.generation_id.clone(),
                    asserted_by: op.asserted_by,
                    model_id: op.model_id.clone(),
                    confidence: norm_conf,
                    created_at_ms: existing.created_at_ms,
                    updated_at_ms: op.recorded_at_ms,
                };
                self.nodes_by_key.insert(parsed.canonical.clone(), node);
                self.revisions.push(MaterializedRevision {
                    node_id: existing.id,
                    rev: new_rev,
                    base_rev: *base_rev,
                    generation_id: op.generation_id.clone(),
                    op: "patch".into(),
                    canonical_key: parsed.canonical,
                    kind: existing.kind,
                    title: next_title,
                    summary: next_summary,
                    body: next_body,
                    attrs: next_attrs,
                    aliases: next_aliases,
                    status: CuratedNodeStatus::Active,
                    supersedes_node_id: existing.supersedes_node_id,
                    superseded_by_node_id: None,
                    asserted_by: op.asserted_by,
                    model_id: op.model_id.clone(),
                    confidence: norm_conf,
                    recorded_at_ms: op.recorded_at_ms,
                    evidence: evidence.clone(),
                });
            }
            CnfRecord::Supersede {
                key,
                kind,
                supersedes,
                title,
                summary,
                body,
                attrs,
                aliases,
                evidence,
                confidence,
                base_rev,
            } => {
                if op.profile == CurationProfile::Brief && body.is_some() {
                    return Err(StateError::InvalidValue);
                }
                let parsed = CuratedKey::parse(key)?;
                let old_key = CuratedKey::parse(supersedes)?;
                let old_node = self
                    .nodes_by_key
                    .get(&old_key.canonical)
                    .cloned()
                    .ok_or(StateError::InvalidValue)?;
                if old_node.status != CuratedNodeStatus::Active {
                    return Err(StateError::InvalidValue);
                }
                let new_id = curated_node_id_for_key(&parsed.canonical)?;
                if new_id == old_node.id {
                    return Err(StateError::InvalidValue);
                }
                let (new_rev, created_at_ms) =
                    if let Some(existing_b) = self.nodes_by_key.get(&parsed.canonical) {
                        if existing_b.status != CuratedNodeStatus::Active {
                            return Err(StateError::InvalidValue);
                        }
                        if *base_rev != existing_b.current_rev {
                            return Err(StateError::RevisionConflict);
                        }
                        (existing_b.current_rev + 1, existing_b.created_at_ms)
                    } else {
                        if *base_rev != 0 {
                            return Err(StateError::RevisionConflict);
                        }
                        (1, op.recorded_at_ms)
                    };
                let norm_aliases = normalize_aliases(aliases)?;
                let norm_conf = normalize_confidence(*confidence);
                self.replace_aliases(&op.topic_id, &new_id, &norm_aliases)?;
                let new_node = MaterializedNode {
                    id: new_id.clone(),
                    topic_id: op.topic_id.clone(),
                    canonical_key: parsed.canonical.clone(),
                    kind: *kind,
                    slug: parsed.slug,
                    current_rev: new_rev,
                    title: title.trim().to_owned(),
                    summary: summary.trim().to_owned(),
                    body: body.clone(),
                    attrs: attrs.clone(),
                    aliases: norm_aliases.clone(),
                    status: CuratedNodeStatus::Active,
                    supersedes_node_id: Some(old_node.id.clone()),
                    superseded_by_node_id: None,
                    generation_id: op.generation_id.clone(),
                    asserted_by: op.asserted_by,
                    model_id: op.model_id.clone(),
                    confidence: norm_conf,
                    created_at_ms,
                    updated_at_ms: op.recorded_at_ms,
                };
                self.key_by_id
                    .insert(new_id.clone(), parsed.canonical.clone());
                self.nodes_by_key
                    .insert(parsed.canonical.clone(), new_node.clone());
                self.revisions.push(MaterializedRevision {
                    node_id: new_id.clone(),
                    rev: new_rev,
                    base_rev: *base_rev,
                    generation_id: op.generation_id.clone(),
                    op: "supersede".into(),
                    canonical_key: parsed.canonical,
                    kind: *kind,
                    title: new_node.title,
                    summary: new_node.summary,
                    body: new_node.body,
                    attrs: new_node.attrs,
                    aliases: norm_aliases,
                    status: CuratedNodeStatus::Active,
                    supersedes_node_id: Some(old_node.id.clone()),
                    superseded_by_node_id: None,
                    asserted_by: op.asserted_by,
                    model_id: op.model_id.clone(),
                    confidence: norm_conf,
                    recorded_at_ms: op.recorded_at_ms,
                    evidence: evidence.clone(),
                });

                self.mark_node_superseded(&old_node, &new_id, op, "supersede", evidence);
                self.upsert_edge_internal(
                    op,
                    &new_id,
                    CuratedEdgeRel::Supersedes,
                    &old_node.id,
                    evidence,
                    norm_conf,
                )?;
            }
            CnfRecord::Retire {
                key,
                reason,
                evidence,
                base_rev,
            } => {
                let parsed = CuratedKey::parse(key)?;
                let existing = self
                    .nodes_by_key
                    .get(&parsed.canonical)
                    .cloned()
                    .ok_or(StateError::InvalidValue)?;
                if existing.status != CuratedNodeStatus::Active {
                    return Err(StateError::InvalidValue);
                }
                if *base_rev != existing.current_rev {
                    return Err(StateError::RevisionConflict);
                }
                let new_rev = existing.current_rev + 1;
                let summary = reason
                    .as_deref()
                    .map(|r| r.trim().to_owned())
                    .unwrap_or(existing.summary);
                let node = MaterializedNode {
                    id: existing.id.clone(),
                    topic_id: existing.topic_id.clone(),
                    canonical_key: existing.canonical_key.clone(),
                    kind: existing.kind,
                    slug: existing.slug,
                    current_rev: new_rev,
                    title: existing.title.clone(),
                    summary: summary.clone(),
                    body: existing.body.clone(),
                    attrs: existing.attrs.clone(),
                    aliases: existing.aliases.clone(),
                    status: CuratedNodeStatus::Retired,
                    supersedes_node_id: existing.supersedes_node_id.clone(),
                    superseded_by_node_id: existing.superseded_by_node_id.clone(),
                    generation_id: op.generation_id.clone(),
                    asserted_by: op.asserted_by,
                    model_id: op.model_id.clone(),
                    confidence: existing.confidence,
                    created_at_ms: existing.created_at_ms,
                    updated_at_ms: op.recorded_at_ms,
                };
                self.nodes_by_key.insert(parsed.canonical.clone(), node);
                self.revisions.push(MaterializedRevision {
                    node_id: existing.id,
                    rev: new_rev,
                    base_rev: *base_rev,
                    generation_id: op.generation_id.clone(),
                    op: "retire".into(),
                    canonical_key: parsed.canonical,
                    kind: existing.kind,
                    title: existing.title,
                    summary,
                    body: existing.body,
                    attrs: existing.attrs,
                    aliases: existing.aliases,
                    status: CuratedNodeStatus::Retired,
                    supersedes_node_id: existing.supersedes_node_id,
                    superseded_by_node_id: existing.superseded_by_node_id,
                    asserted_by: op.asserted_by,
                    model_id: op.model_id.clone(),
                    confidence: existing.confidence,
                    recorded_at_ms: op.recorded_at_ms,
                    evidence: evidence.clone(),
                });
            }
            CnfRecord::Link {
                from,
                rel,
                to,
                evidence,
                confidence,
            } => {
                let f = CuratedKey::parse(from)?;
                let t = CuratedKey::parse(to)?;
                let from_node = self
                    .nodes_by_key
                    .get(&f.canonical)
                    .cloned()
                    .ok_or(StateError::InvalidValue)?;
                let to_node = self
                    .nodes_by_key
                    .get(&t.canonical)
                    .cloned()
                    .ok_or(StateError::InvalidValue)?;
                if from_node.id == to_node.id {
                    return Err(StateError::InvalidValue);
                }
                if *rel == CuratedEdgeRel::Supersedes {
                    if from_node.status != CuratedNodeStatus::Active {
                        return Err(StateError::InvalidValue);
                    }
                    match to_node.status {
                        CuratedNodeStatus::Active => {
                            self.mark_node_superseded(
                                &to_node,
                                &from_node.id,
                                op,
                                "link",
                                evidence,
                            );
                            if let Some(mut updated_from) =
                                self.nodes_by_key.get(&from_node.canonical_key).cloned()
                            {
                                if updated_from.supersedes_node_id.is_none() {
                                    updated_from.supersedes_node_id = Some(to_node.id.clone());
                                    self.nodes_by_key
                                        .insert(updated_from.canonical_key.clone(), updated_from);
                                }
                            }
                        }
                        CuratedNodeStatus::Superseded
                            if to_node.superseded_by_node_id.as_deref() == Some(&from_node.id) => {}
                        _ => return Err(StateError::InvalidValue),
                    }
                }
                self.upsert_edge_internal(
                    op,
                    &from_node.id,
                    *rel,
                    &to_node.id,
                    evidence,
                    normalize_confidence(*confidence),
                )?;
            }
            CnfRecord::Unlink {
                from,
                rel,
                to,
                evidence,
            } => {
                let f = CuratedKey::parse(from)?;
                let t = CuratedKey::parse(to)?;
                let from_node = self
                    .nodes_by_key
                    .get(&f.canonical)
                    .ok_or(StateError::InvalidValue)?;
                let to_node = self
                    .nodes_by_key
                    .get(&t.canonical)
                    .ok_or(StateError::InvalidValue)?;
                let edge_id = curated_edge_id(&from_node.id, *rel, &to_node.id)?;
                let existing = self
                    .edges
                    .get_mut(&edge_id)
                    .ok_or(StateError::InvalidValue)?;
                if existing.status != CuratedEdgeStatus::Active {
                    return Err(StateError::InvalidValue);
                }
                existing.status = CuratedEdgeStatus::Unlinked;
                existing.generation_id = op.generation_id.clone();
                existing.asserted_by = op.asserted_by;
                existing.model_id = op.model_id.clone();
                existing.updated_at_ms = op.recorded_at_ms;
                existing.evidence = evidence.clone();
            }
        }
        Ok(())
    }

    fn replace_aliases(&mut self, topic_id: &str, node_id: &str, aliases: &[String]) -> Result<()> {
        for alias in aliases {
            if let Some(owner) = self
                .aliases_by_topic
                .get(&(topic_id.to_owned(), alias.clone()))
            {
                if owner != node_id {
                    return Err(StateError::InvalidValue);
                }
            }
        }
        self.aliases_by_topic
            .retain(|(t, _), owner| !(t == topic_id && owner == node_id));
        for alias in aliases {
            self.aliases_by_topic
                .insert((topic_id.to_owned(), alias.clone()), node_id.to_owned());
        }
        Ok(())
    }

    fn mark_node_superseded(
        &mut self,
        old_node: &MaterializedNode,
        by_node_id: &str,
        op: &StoredOp,
        rev_op: &str,
        evidence: &[EvidenceSpan],
    ) {
        let next_rev = old_node.current_rev + 1;
        let updated = MaterializedNode {
            current_rev: next_rev,
            status: CuratedNodeStatus::Superseded,
            superseded_by_node_id: Some(by_node_id.to_owned()),
            generation_id: op.generation_id.clone(),
            asserted_by: op.asserted_by,
            model_id: op.model_id.clone(),
            updated_at_ms: op.recorded_at_ms,
            ..old_node.clone()
        };
        self.nodes_by_key
            .insert(old_node.canonical_key.clone(), updated.clone());
        self.revisions.push(MaterializedRevision {
            node_id: old_node.id.clone(),
            rev: next_rev,
            base_rev: old_node.current_rev,
            generation_id: op.generation_id.clone(),
            op: rev_op.to_owned(),
            canonical_key: old_node.canonical_key.clone(),
            kind: old_node.kind,
            title: old_node.title.clone(),
            summary: old_node.summary.clone(),
            body: old_node.body.clone(),
            attrs: old_node.attrs.clone(),
            aliases: old_node.aliases.clone(),
            status: CuratedNodeStatus::Superseded,
            supersedes_node_id: old_node.supersedes_node_id.clone(),
            superseded_by_node_id: Some(by_node_id.to_owned()),
            asserted_by: op.asserted_by,
            model_id: op.model_id.clone(),
            confidence: old_node.confidence,
            recorded_at_ms: op.recorded_at_ms,
            evidence: evidence.to_vec(),
        });
    }

    fn upsert_edge_internal(
        &mut self,
        op: &StoredOp,
        from_node_id: &str,
        rel: CuratedEdgeRel,
        to_node_id: &str,
        evidence: &[EvidenceSpan],
        confidence: Option<f64>,
    ) -> Result<()> {
        let edge_id = curated_edge_id(from_node_id, rel, to_node_id)?;
        let created_at_ms = self
            .edges
            .get(&edge_id)
            .map_or(op.recorded_at_ms, |e| e.created_at_ms);
        self.edges.insert(
            edge_id.clone(),
            MaterializedEdge {
                id: edge_id,
                topic_id: op.topic_id.clone(),
                from_node_id: from_node_id.to_owned(),
                rel,
                to_node_id: to_node_id.to_owned(),
                status: CuratedEdgeStatus::Active,
                generation_id: op.generation_id.clone(),
                asserted_by: op.asserted_by,
                model_id: op.model_id.clone(),
                confidence,
                created_at_ms,
                updated_at_ms: op.recorded_at_ms,
                evidence: evidence.to_vec(),
            },
        );
        Ok(())
    }

    fn digest(&self) -> Result<String> {
        let mut hash = Sha256::new();
        hash.update(b"mind-place:curated-projection:v1\0");
        let mut nodes: Vec<&MaterializedNode> = self.nodes_by_key.values().collect();
        nodes.sort_by(|a, b| a.canonical_key.cmp(&b.canonical_key));
        for node in nodes {
            let attrs = serde_json::to_string(&node.attrs).map_err(|_| StateError::Corrupt)?;
            let aliases = serde_json::to_string(&node.aliases).map_err(|_| StateError::Corrupt)?;
            let conf = node
                .confidence
                .map(|v| format!("{v:.12}"))
                .unwrap_or_default();
            for field in [
                "N",
                &node.id,
                &node.topic_id,
                &node.canonical_key,
                node.kind.as_str(),
                &node.slug,
                &node.current_rev.to_string(),
                &node.title,
                &node.summary,
                node.body.as_deref().unwrap_or(""),
                &attrs,
                &aliases,
                node.status.as_str(),
                node.supersedes_node_id.as_deref().unwrap_or(""),
                node.superseded_by_node_id.as_deref().unwrap_or(""),
                &node.generation_id,
                node.asserted_by.as_str(),
                node.model_id.as_deref().unwrap_or(""),
                &conf,
                &node.created_at_ms.to_string(),
                &node.updated_at_ms.to_string(),
            ] {
                hash_field(&mut hash, field);
            }
        }
        let mut revs: Vec<&MaterializedRevision> = self.revisions.iter().collect();
        revs.sort_by(|a, b| a.node_id.cmp(&b.node_id).then_with(|| a.rev.cmp(&b.rev)));
        for rev in revs {
            let attrs = serde_json::to_string(&rev.attrs).map_err(|_| StateError::Corrupt)?;
            let aliases = serde_json::to_string(&rev.aliases).map_err(|_| StateError::Corrupt)?;
            let ev = serde_json::to_string(&rev.evidence).map_err(|_| StateError::Corrupt)?;
            for field in [
                "R",
                &rev.node_id,
                &rev.rev.to_string(),
                &rev.base_rev.to_string(),
                &rev.generation_id,
                &rev.op,
                &rev.canonical_key,
                rev.kind.as_str(),
                &rev.title,
                &rev.summary,
                rev.body.as_deref().unwrap_or(""),
                &attrs,
                &aliases,
                rev.status.as_str(),
                rev.supersedes_node_id.as_deref().unwrap_or(""),
                rev.superseded_by_node_id.as_deref().unwrap_or(""),
                &rev.recorded_at_ms.to_string(),
                &ev,
            ] {
                hash_field(&mut hash, field);
            }
        }
        let mut edges: Vec<&MaterializedEdge> = self.edges.values().collect();
        edges.sort_by(|a, b| a.id.cmp(&b.id));
        for edge in edges {
            let ev = serde_json::to_string(&edge.evidence).map_err(|_| StateError::Corrupt)?;
            let conf = edge
                .confidence
                .map(|v| format!("{v:.12}"))
                .unwrap_or_default();
            for field in [
                "E",
                &edge.id,
                &edge.topic_id,
                &edge.from_node_id,
                edge.rel.as_str(),
                &edge.to_node_id,
                edge.status.as_str(),
                &edge.generation_id,
                edge.asserted_by.as_str(),
                edge.model_id.as_deref().unwrap_or(""),
                &conf,
                &edge.created_at_ms.to_string(),
                &edge.updated_at_ms.to_string(),
                &ev,
            ] {
                hash_field(&mut hash, field);
            }
        }
        Ok(format!("{:x}", hash.finalize()))
    }

    /// Reconciles the materialized curated tables in one transaction while
    /// keeping `PRAGMA foreign_keys = ON` and existing `topic_memberships` FKs valid.
    fn write_to_db(&self, tx: &Transaction<'_>) -> Result<()> {
        tx.execute("DELETE FROM curated_edge_evidence_spans", [])?;
        tx.execute("DELETE FROM curated_edges", [])?;
        tx.execute("DELETE FROM curated_node_evidence_spans", [])?;
        tx.execute("DELETE FROM curated_node_revisions", [])?;
        tx.execute("DELETE FROM curated_node_aliases", [])?;
        tx.execute(
            "UPDATE curated_nodes SET status='active',supersedes_node_id=NULL,superseded_by_node_id=NULL",
            [],
        )?;

        let mut existing_ids = Vec::new();
        {
            let mut stmt = tx.prepare("SELECT id FROM curated_nodes")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            for row in rows {
                existing_ids.push(row?);
            }
        }
        let live_ids: BTreeSet<&str> = self.nodes_by_key.values().map(|n| n.id.as_str()).collect();

        for node in self.nodes_by_key.values() {
            let attrs_json = serde_json::to_string(&node.attrs).map_err(|_| StateError::Corrupt)?;
            tx.execute(
                "INSERT INTO curated_nodes (
                   id,topic_id,canonical_key,kind,slug,current_rev,title,summary,body,attrs_json,
                   status,supersedes_node_id,superseded_by_node_id,generation_id,asserted_by,
                   model_id,confidence,created_at_ms,updated_at_ms
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'active',NULL,NULL,?11,?12,?13,?14,?15,?16)
                 ON CONFLICT(id) DO UPDATE SET
                   topic_id=excluded.topic_id,
                   canonical_key=excluded.canonical_key,
                   kind=excluded.kind,
                   slug=excluded.slug,
                   current_rev=excluded.current_rev,
                   title=excluded.title,
                   summary=excluded.summary,
                   body=excluded.body,
                   attrs_json=excluded.attrs_json,
                   status='active',
                   supersedes_node_id=NULL,
                   superseded_by_node_id=NULL,
                   generation_id=excluded.generation_id,
                   asserted_by=excluded.asserted_by,
                   model_id=excluded.model_id,
                   confidence=excluded.confidence,
                   created_at_ms=excluded.created_at_ms,
                   updated_at_ms=excluded.updated_at_ms",
                params![
                    node.id,
                    node.topic_id,
                    node.canonical_key,
                    node.kind.as_str(),
                    node.slug,
                    node.current_rev,
                    node.title,
                    node.summary,
                    node.body,
                    attrs_json,
                    node.generation_id,
                    node.asserted_by.as_str(),
                    node.model_id,
                    node.confidence,
                    node.created_at_ms,
                    node.updated_at_ms,
                ],
            )?;
        }

        for id in existing_ids {
            if !live_ids.contains(id.as_str()) {
                tx.execute("DELETE FROM curated_nodes WHERE id=?1", [&id])
                    .map_err(|_| StateError::Corrupt)?;
            }
        }

        for node in self.nodes_by_key.values() {
            if node.status != CuratedNodeStatus::Active
                || node.supersedes_node_id.is_some()
                || node.superseded_by_node_id.is_some()
            {
                tx.execute(
                    "UPDATE curated_nodes
                     SET status=?2,supersedes_node_id=?3,superseded_by_node_id=?4
                     WHERE id=?1",
                    params![
                        node.id,
                        node.status.as_str(),
                        node.supersedes_node_id,
                        node.superseded_by_node_id,
                    ],
                )?;
            }
        }

        for ((topic_id, alias), node_id) in &self.aliases_by_topic {
            tx.execute(
                "INSERT INTO curated_node_aliases(topic_id,alias,node_id) VALUES(?1,?2,?3)",
                params![topic_id, alias, node_id],
            )?;
        }

        let mut revs: Vec<&MaterializedRevision> = self.revisions.iter().collect();
        revs.sort_by(|a, b| a.node_id.cmp(&b.node_id).then_with(|| a.rev.cmp(&b.rev)));
        for rev in revs {
            let attrs_json = serde_json::to_string(&rev.attrs).map_err(|_| StateError::Corrupt)?;
            let aliases_json =
                serde_json::to_string(&rev.aliases).map_err(|_| StateError::Corrupt)?;
            tx.execute(
                "INSERT INTO curated_node_revisions (
                   node_id,rev,base_rev,generation_id,op,canonical_key,kind,title,summary,body,
                   attrs_json,aliases_json,status,supersedes_node_id,superseded_by_node_id,
                   asserted_by,model_id,confidence,recorded_at_ms
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
                params![
                    rev.node_id,
                    rev.rev,
                    rev.base_rev,
                    rev.generation_id,
                    rev.op,
                    rev.canonical_key,
                    rev.kind.as_str(),
                    rev.title,
                    rev.summary,
                    rev.body,
                    attrs_json,
                    aliases_json,
                    rev.status.as_str(),
                    rev.supersedes_node_id,
                    rev.superseded_by_node_id,
                    rev.asserted_by.as_str(),
                    rev.model_id,
                    rev.confidence,
                    rev.recorded_at_ms,
                ],
            )?;
            for (ordinal, span) in rev.evidence.iter().enumerate() {
                tx.execute(
                    "INSERT INTO curated_node_evidence_spans (
                       node_id,rev,ordinal,source_ref,msg_index,span_start,span_end,span_sha,quote
                     ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                    params![
                        rev.node_id,
                        rev.rev,
                        ordinal as i64,
                        span.src,
                        span.msg,
                        span.span[0],
                        span.span[1],
                        span.sha,
                        span.quote,
                    ],
                )?;
            }
        }

        for edge in self.edges.values() {
            tx.execute(
                "INSERT INTO curated_edges (
                   id,topic_id,from_node_id,rel,to_node_id,status,generation_id,asserted_by,
                   model_id,confidence,created_at_ms,updated_at_ms
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                params![
                    edge.id,
                    edge.topic_id,
                    edge.from_node_id,
                    edge.rel.as_str(),
                    edge.to_node_id,
                    edge.status.as_str(),
                    edge.generation_id,
                    edge.asserted_by.as_str(),
                    edge.model_id,
                    edge.confidence,
                    edge.created_at_ms,
                    edge.updated_at_ms,
                ],
            )?;
            for (ordinal, span) in edge.evidence.iter().enumerate() {
                tx.execute(
                    "INSERT INTO curated_edge_evidence_spans (
                       edge_id,ordinal,source_ref,msg_index,span_start,span_end,span_sha,quote
                     ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                    params![
                        edge.id,
                        ordinal as i64,
                        span.src,
                        span.msg,
                        span.span[0],
                        span.span[1],
                        span.sha,
                        span.quote,
                    ],
                )?;
            }
        }
        Ok(())
    }
}

fn hash_field(hash: &mut Sha256, value: &str) {
    hash.update((value.len() as u64).to_be_bytes());
    hash.update(value.as_bytes());
}

fn load_published_projection_from_db(db: &Connection) -> Result<CuratedProjectionState> {
    let mut state = CuratedProjectionState::default();
    let mut node_stmt = db.prepare(
        "SELECT id,topic_id,canonical_key,kind,slug,current_rev,title,summary,body,attrs_json,
                status,supersedes_node_id,superseded_by_node_id,generation_id,asserted_by,
                model_id,confidence,created_at_ms,updated_at_ms
         FROM curated_nodes ORDER BY canonical_key",
    )?;
    let raw_nodes = node_stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, Option<String>>(8)?,
                r.get::<_, String>(9)?,
                r.get::<_, String>(10)?,
                r.get::<_, Option<String>>(11)?,
                r.get::<_, Option<String>>(12)?,
                r.get::<_, String>(13)?,
                r.get::<_, String>(14)?,
                r.get::<_, Option<String>>(15)?,
                r.get::<_, Option<f64>>(16)?,
                r.get::<_, i64>(17)?,
                r.get::<_, i64>(18)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(node_stmt);

    for tuple in raw_nodes {
        let attrs: BTreeMap<String, AttrValue> =
            serde_json::from_str(&tuple.9).map_err(|_| StateError::Corrupt)?;
        let aliases = load_node_aliases(db, &tuple.0)?;
        for alias in &aliases {
            state
                .aliases_by_topic
                .insert((tuple.1.clone(), alias.clone()), tuple.0.clone());
        }
        state.key_by_id.insert(tuple.0.clone(), tuple.2.clone());
        state.nodes_by_key.insert(
            tuple.2.clone(),
            MaterializedNode {
                id: tuple.0,
                topic_id: tuple.1,
                canonical_key: tuple.2,
                kind: CuratedNodeKind::parse(&tuple.3).map_err(|_| StateError::Corrupt)?,
                slug: tuple.4,
                current_rev: tuple.5,
                title: tuple.6,
                summary: tuple.7,
                body: tuple.8,
                attrs,
                aliases,
                status: CuratedNodeStatus::parse(&tuple.10)?,
                supersedes_node_id: tuple.11,
                superseded_by_node_id: tuple.12,
                generation_id: tuple.13,
                asserted_by: AssertionSource::parse(&tuple.14)?,
                model_id: tuple.15,
                confidence: tuple.16,
                created_at_ms: tuple.17,
                updated_at_ms: tuple.18,
            },
        );
    }

    let mut rev_stmt = db.prepare(
        "SELECT node_id,rev,base_rev,generation_id,op,canonical_key,kind,title,summary,body,
                attrs_json,aliases_json,status,supersedes_node_id,superseded_by_node_id,
                asserted_by,model_id,confidence,recorded_at_ms
         FROM curated_node_revisions ORDER BY node_id, rev",
    )?;
    let raw_revs = rev_stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, String>(8)?,
                r.get::<_, Option<String>>(9)?,
                r.get::<_, String>(10)?,
                r.get::<_, String>(11)?,
                r.get::<_, String>(12)?,
                r.get::<_, Option<String>>(13)?,
                r.get::<_, Option<String>>(14)?,
                r.get::<_, String>(15)?,
                r.get::<_, Option<String>>(16)?,
                r.get::<_, Option<f64>>(17)?,
                r.get::<_, i64>(18)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(rev_stmt);

    for tuple in raw_revs {
        let attrs: BTreeMap<String, AttrValue> =
            serde_json::from_str(&tuple.10).map_err(|_| StateError::Corrupt)?;
        let aliases: Vec<String> =
            serde_json::from_str(&tuple.11).map_err(|_| StateError::Corrupt)?;
        let evidence = load_node_rev_evidence(db, &tuple.0, tuple.1)?;
        if evidence.is_empty() {
            return Err(StateError::Corrupt);
        }
        state.revisions.push(MaterializedRevision {
            node_id: tuple.0,
            rev: tuple.1,
            base_rev: tuple.2,
            generation_id: tuple.3,
            op: tuple.4,
            canonical_key: tuple.5,
            kind: CuratedNodeKind::parse(&tuple.6).map_err(|_| StateError::Corrupt)?,
            title: tuple.7,
            summary: tuple.8,
            body: tuple.9,
            attrs,
            aliases,
            status: CuratedNodeStatus::parse(&tuple.12)?,
            supersedes_node_id: tuple.13,
            superseded_by_node_id: tuple.14,
            asserted_by: AssertionSource::parse(&tuple.15)?,
            model_id: tuple.16,
            confidence: tuple.17,
            recorded_at_ms: tuple.18,
            evidence,
        });
    }

    let mut edge_stmt = db.prepare(
        "SELECT id,topic_id,from_node_id,rel,to_node_id,status,generation_id,asserted_by,
                model_id,confidence,created_at_ms,updated_at_ms
         FROM curated_edges ORDER BY id",
    )?;
    let raw_edges = edge_stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, Option<String>>(8)?,
                r.get::<_, Option<f64>>(9)?,
                r.get::<_, i64>(10)?,
                r.get::<_, i64>(11)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(edge_stmt);

    for tuple in raw_edges {
        let evidence = load_edge_evidence(db, &tuple.0)?;
        if evidence.is_empty() {
            return Err(StateError::Corrupt);
        }
        state.edges.insert(
            tuple.0.clone(),
            MaterializedEdge {
                id: tuple.0,
                topic_id: tuple.1,
                from_node_id: tuple.2,
                rel: CuratedEdgeRel::parse(&tuple.3).map_err(|_| StateError::Corrupt)?,
                to_node_id: tuple.4,
                status: CuratedEdgeStatus::parse(&tuple.5)?,
                generation_id: tuple.6,
                asserted_by: AssertionSource::parse(&tuple.7)?,
                model_id: tuple.8,
                confidence: tuple.9,
                created_at_ms: tuple.10,
                updated_at_ms: tuple.11,
                evidence,
            },
        );
    }

    Ok(state)
}

pub(crate) fn load_node_aliases(db: &Connection, node_id: &str) -> Result<Vec<String>> {
    let mut stmt =
        db.prepare("SELECT alias FROM curated_node_aliases WHERE node_id=?1 ORDER BY alias")?;
    let out = stmt
        .query_map([node_id], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(out)
}

pub(crate) fn load_node_rev_evidence(
    db: &Connection,
    node_id: &str,
    rev: i64,
) -> Result<Vec<EvidenceSpan>> {
    let mut stmt = db.prepare(
        "SELECT source_ref,msg_index,span_start,span_end,span_sha,quote
         FROM curated_node_evidence_spans
         WHERE node_id=?1 AND rev=?2 ORDER BY ordinal",
    )?;
    let out = stmt
        .query_map(params![node_id, rev], |r| {
            Ok(EvidenceSpan {
                src: r.get(0)?,
                msg: r.get(1)?,
                span: [r.get(2)?, r.get(3)?],
                sha: r.get(4)?,
                quote: r.get(5)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(out)
}

pub(crate) fn load_node_all_evidence(
    db: &Connection,
    node_id: &str,
) -> Result<Vec<(i64, EvidenceSpan)>> {
    let mut stmt = db.prepare(
        "SELECT rev,source_ref,msg_index,span_start,span_end,span_sha,quote
         FROM curated_node_evidence_spans
         WHERE node_id=?1 ORDER BY rev, ordinal",
    )?;
    let out = stmt
        .query_map([node_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                EvidenceSpan {
                    src: r.get(1)?,
                    msg: r.get(2)?,
                    span: [r.get(3)?, r.get(4)?],
                    sha: r.get(5)?,
                    quote: r.get(6)?,
                },
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(out)
}

pub(crate) fn load_edge_evidence(db: &Connection, edge_id: &str) -> Result<Vec<EvidenceSpan>> {
    let mut stmt = db.prepare(
        "SELECT source_ref,msg_index,span_start,span_end,span_sha,quote
         FROM curated_edge_evidence_spans
         WHERE edge_id=?1 ORDER BY ordinal",
    )?;
    let out = stmt
        .query_map([edge_id], |r| {
            Ok(EvidenceSpan {
                src: r.get(0)?,
                msg: r.get(1)?,
                span: [r.get(2)?, r.get(3)?],
                sha: r.get(4)?,
                quote: r.get(5)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(out)
}

pub(crate) fn evidence_spans_have_redacted_source(
    db: &Connection,
    spans: &[EvidenceSpan],
) -> Result<bool> {
    for span in spans {
        if crate::source_redaction::source_is_redacted(db, &span.src)? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn curated_node_has_redacted_source(db: &Connection, node_id: &str) -> Result<bool> {
    for (_, span) in load_node_all_evidence(db, node_id)? {
        if crate::source_redaction::source_is_redacted(db, &span.src)? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn verify_curated_invariants_connection(db: &Connection) -> Result<()> {
    let bad_node_rev: bool = db.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM curated_nodes n
           WHERE NOT EXISTS(
             SELECT 1 FROM curated_node_revisions r
             WHERE r.node_id=n.id AND r.rev=n.current_rev
           )
         )",
        [],
        |r| r.get(0),
    )?;
    let bad_op_evidence: bool = db.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM curated_op_log o
           WHERE NOT EXISTS(
             SELECT 1 FROM curated_op_evidence_spans e
             WHERE e.generation_id=o.generation_id AND e.shard_id=o.shard_id AND e.seq=o.seq
           )
         )",
        [],
        |r| r.get(0),
    )?;
    let bad_rev_evidence: bool = db.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM curated_node_revisions r
           WHERE NOT EXISTS(
             SELECT 1 FROM curated_node_evidence_spans e
             WHERE e.node_id=r.node_id AND e.rev=r.rev
           )
         )",
        [],
        |r| r.get(0),
    )?;
    let bad_edge_evidence: bool = db.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM curated_edges c
           WHERE NOT EXISTS(
             SELECT 1 FROM curated_edge_evidence_spans e
             WHERE e.edge_id=c.id
           )
         )",
        [],
        |r| r.get(0),
    )?;
    if bad_node_rev || bad_op_evidence || bad_rev_evidence || bad_edge_evidence {
        return Err(StateError::Corrupt);
    }
    Ok(())
}

impl StateStore {
    /// Creates a new `staged` curation generation anchored to a topic.
    pub fn create_staged_generation(
        &self,
        input: &NewCurationGeneration<'_>,
    ) -> Result<CurationGeneration> {
        valid_id(input.id)?;
        valid_id(input.topic_id)?;
        valid_id(input.run_id)?;
        if input.asserted_by == AssertionSource::Model && input.model_id.is_none() {
            return Err(StateError::InvalidValue);
        }
        if let Some(m) = input.model_id {
            valid_text(m)?;
            if m.trim().len() > 128 || m.chars().any(char::is_control) {
                return Err(StateError::InvalidValue);
            }
        }
        if let Some(base) = input.base_generation_id {
            valid_id(base)?;
            if base == input.id {
                return Err(StateError::InvalidValue);
            }
        }
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let topic = fetch_topic(&tx, input.topic_id)?.ok_or(StateError::InvalidValue)?;
        if let Some(base) = input.base_generation_id {
            let base_gen = fetch_generation(&tx, base)?.ok_or(StateError::InvalidValue)?;
            if base_gen.topic_id != topic.id || base_gen.status != GenerationStatus::Published {
                return Err(StateError::InvalidValue);
            }
        }
        let clock = next_curated_clock(&tx, 0)?;
        let trimmed_model = input.model_id.map(str::trim);
        tx.execute(
            "INSERT INTO curation_generations (
               id,topic_id,run_id,profile,asserted_by,model_id,status,
               base_generation_id,projection_sha256,created_at_ms,published_at_ms
             ) VALUES (?1,?2,?3,?4,?5,?6,'staged',?7,NULL,?8,NULL)",
            params![
                input.id,
                topic.id,
                input.run_id,
                input.profile.as_str(),
                input.asserted_by.as_str(),
                trimmed_model,
                input.base_generation_id,
                clock,
            ],
        )
        .map_err(|err| match err {
            rusqlite::Error::SqliteFailure(code, _)
                if code.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                StateError::InvalidValue
            }
            other => StateError::Sql(other),
        })?;
        let created = fetch_generation(&tx, input.id)?.ok_or(StateError::Corrupt)?;
        tx.commit()?;
        Ok(created)
    }

    /// Appends validated CNF v1 records into one shard of a `staged` generation
    /// without mutating accepted curated nodes or touching raw evidence.
    pub fn append_staged_ops(
        &self,
        generation_id: &str,
        shard_id: &str,
        records: &[CnfRecord],
    ) -> Result<Vec<RecordedCuratedOp>> {
        valid_id(generation_id)?;
        valid_id(shard_id)?;
        if shard_id.len() > 64 || records.is_empty() {
            return Err(StateError::InvalidValue);
        }
        if records.len() > MAX_RECORDS_PER_APPEND {
            return Err(StateError::CuratedLimit);
        }
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let generation = fetch_generation(&tx, generation_id)?.ok_or(StateError::InvalidValue)?;
        if generation.status != GenerationStatus::Staged {
            return Err(StateError::InvalidValue);
        }
        let topic = fetch_topic(&tx, &generation.topic_id)?.ok_or(StateError::Corrupt)?;

        let shard_exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM curated_op_log WHERE generation_id=?1 AND shard_id=?2)",
            params![generation_id, shard_id],
            |r| r.get(0),
        )?;
        let shard_count: i64 = tx.query_row(
            "SELECT COUNT(DISTINCT shard_id) FROM curated_op_log WHERE generation_id=?1",
            [generation_id],
            |r| r.get(0),
        )?;
        if !shard_exists && shard_count as usize >= MAX_SHARDS_PER_GENERATION {
            return Err(StateError::CuratedLimit);
        }
        let mut next_seq: i64 = tx.query_row(
            "SELECT COALESCE(MAX(seq),0) FROM curated_op_log WHERE generation_id=?1 AND shard_id=?2",
            params![generation_id, shard_id],
            |r| r.get(0),
        )?;
        if (next_seq as usize) + records.len() > MAX_RECORDS_PER_SHARD {
            return Err(StateError::CuratedLimit);
        }

        let mut receipts = Vec::with_capacity(records.len());
        for record in records {
            record.validate_shape()?;
            if record.target_topic()? != topic.slug {
                return Err(StateError::InvalidValue);
            }
            if generation.profile == CurationProfile::Brief
                && matches!(
                    record,
                    CnfRecord::Upsert { body: Some(_), .. }
                        | CnfRecord::Patch { body: Some(_), .. }
                        | CnfRecord::Supersede { body: Some(_), .. }
                )
            {
                return Err(StateError::InvalidValue);
            }
            validate_evidence_list(
                record.evidence(),
                generation.profile == CurationProfile::Exact,
            )?;
            verify_registered_sources(&tx, record.evidence())?;

            let payload_json =
                serde_json::to_string(record).map_err(|_| StateError::InvalidValue)?;
            if payload_json.len() > MAX_CNF_RECORD_BYTES {
                return Err(StateError::CuratedLimit);
            }
            let record_sha256 = format!("{:x}", Sha256::digest(payload_json.as_bytes()));
            next_seq += 1;
            let clock = next_curated_clock(&tx, generation.created_at_ms)?;

            let (target_key, kind_str, rel_str, to_key, supersedes_key, base_rev): (
                String,
                Option<&'static str>,
                Option<&'static str>,
                Option<String>,
                Option<String>,
                i64,
            ) = match record {
                CnfRecord::Upsert {
                    key,
                    kind,
                    base_rev,
                    ..
                } => (
                    key.clone(),
                    Some(kind.as_str()),
                    None,
                    None,
                    None,
                    *base_rev,
                ),
                CnfRecord::Patch { key, base_rev, .. } => {
                    (key.clone(), None, None, None, None, *base_rev)
                }
                CnfRecord::Supersede {
                    key,
                    kind,
                    supersedes,
                    base_rev,
                    ..
                } => (
                    key.clone(),
                    Some(kind.as_str()),
                    None,
                    None,
                    Some(supersedes.clone()),
                    *base_rev,
                ),
                CnfRecord::Retire { key, base_rev, .. } => {
                    (key.clone(), None, None, None, None, *base_rev)
                }
                CnfRecord::Link { from, rel, to, .. } | CnfRecord::Unlink { from, rel, to, .. } => {
                    (
                        from.clone(),
                        None,
                        Some(rel.as_str()),
                        Some(to.clone()),
                        None,
                        0,
                    )
                }
            };

            tx.execute(
                "INSERT INTO curated_op_log (
                   generation_id,shard_id,seq,op,target_key,kind,rel,to_key,supersedes_key,
                   base_rev,payload_json,record_sha256,recorded_at_ms
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
                params![
                    generation_id,
                    shard_id,
                    next_seq,
                    record.op_name(),
                    target_key,
                    kind_str,
                    rel_str,
                    to_key,
                    supersedes_key,
                    base_rev,
                    payload_json,
                    record_sha256,
                    clock,
                ],
            )?;
            for (ordinal, span) in record.evidence().iter().enumerate() {
                tx.execute(
                    "INSERT INTO curated_op_evidence_spans (
                       generation_id,shard_id,seq,ordinal,source_ref,msg_index,
                       span_start,span_end,span_sha,quote
                     ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                    params![
                        generation_id,
                        shard_id,
                        next_seq,
                        ordinal as i64,
                        span.src,
                        span.msg,
                        span.span[0],
                        span.span[1],
                        span.sha,
                        span.quote,
                    ],
                )?;
            }
            receipts.push(RecordedCuratedOp {
                generation_id: generation_id.to_owned(),
                shard_id: shard_id.to_owned(),
                seq: next_seq,
                op: record.op_name().to_owned(),
                target_key,
                record_sha256,
                recorded_at_ms: clock,
            });
        }
        tx.commit()?;
        Ok(receipts)
    }

    /// Publishes a staged generation atomically into the accepted curated store.
    pub fn publish_generation(&self, generation_id: &str) -> Result<PublishedGenerationReceipt> {
        valid_id(generation_id)?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let generation = fetch_generation(&tx, generation_id)?.ok_or(StateError::InvalidValue)?;
        if generation.status != GenerationStatus::Staged {
            return Err(StateError::InvalidValue);
        }
        let ops = load_generation_ops(&tx, &generation)?;
        if ops.is_empty() {
            return Err(StateError::InvalidValue);
        }
        for op in &ops {
            for ev in op.record.evidence() {
                crate::source_redaction::assert_source_not_redacted(&tx, &ev.src)?;
            }
        }
        let mut state = load_published_projection_from_db(&tx)?;
        for op in &ops {
            state.apply_op(op)?;
        }
        let projection_sha256 = state.digest()?;
        let min_clock = ops
            .iter()
            .map(|o| o.recorded_at_ms)
            .max()
            .unwrap_or(generation.created_at_ms);
        let published_at_ms = next_curated_clock(&tx, min_clock)?;
        tx.execute(
            "UPDATE curation_generations
             SET status='published',published_at_ms=?2,projection_sha256=?3
             WHERE id=?1",
            params![generation_id, published_at_ms, projection_sha256],
        )?;
        state.write_to_db(&tx)?;
        let fk_bad: Option<i64> = tx
            .query_row("SELECT 1 FROM pragma_foreign_key_check LIMIT 1", [], |r| {
                r.get(0)
            })
            .optional()?;
        if fk_bad.is_some() {
            return Err(StateError::Corrupt);
        }
        verify_curated_invariants_connection(&tx)?;
        tx.commit()?;
        Ok(PublishedGenerationReceipt {
            generation_id: generation.id,
            topic_id: generation.topic_id,
            ops_applied: ops.len(),
            published_at_ms,
            projection_sha256,
        })
    }

    /// Marks a staged generation as rejected without touching accepted nodes or raw evidence.
    pub fn reject_generation(&self, generation_id: &str) -> Result<CurationGeneration> {
        valid_id(generation_id)?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let generation = fetch_generation(&tx, generation_id)?.ok_or(StateError::InvalidValue)?;
        if generation.status != GenerationStatus::Staged {
            return Err(StateError::InvalidValue);
        }
        tx.execute(
            "UPDATE curation_generations SET status='rejected' WHERE id=?1",
            [generation_id],
        )?;
        let updated = fetch_generation(&tx, generation_id)?.ok_or(StateError::Corrupt)?;
        tx.commit()?;
        Ok(updated)
    }

    /// Deterministically rebuilds the entire accepted curated store from
    /// published `curation_generations` and `curated_op_log`, leaving raw
    /// evidence, staged/rejected generations, and `topic_memberships` intact.
    pub fn rebuild_curated_store(&self) -> Result<CuratedRebuildReceipt> {
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut stmt = tx.prepare(
            "SELECT id,topic_id,run_id,profile,asserted_by,model_id,status,
                    base_generation_id,projection_sha256,created_at_ms,published_at_ms
             FROM curation_generations
             WHERE status='published'
             ORDER BY published_at_ms, id",
        )?;
        let gens = stmt
            .query_map([], generation_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .map(build_generation)
            .collect::<Result<Vec<_>>>()?;
        drop(stmt);

        let mut state = CuratedProjectionState::default();
        let mut ops_replayed = 0usize;
        for gen in &gens {
            let ops = load_generation_ops(&tx, gen)?;
            for op in &ops {
                state.apply_op(op)?;
                ops_replayed += 1;
            }
            if let Some(expected_sha) = gen.projection_sha256.as_deref() {
                if state.digest()? != expected_sha {
                    return Err(StateError::Corrupt);
                }
            }
        }
        let projection_sha256 = state.digest()?;
        state.write_to_db(&tx)?;
        let fk_bad: Option<i64> = tx
            .query_row("SELECT 1 FROM pragma_foreign_key_check LIMIT 1", [], |r| {
                r.get(0)
            })
            .optional()?;
        if fk_bad.is_some() {
            return Err(StateError::Corrupt);
        }
        verify_curated_invariants_connection(&tx)?;
        let receipt = CuratedRebuildReceipt {
            published_generations: gens.len(),
            ops_replayed,
            node_count: state.nodes_by_key.len(),
            revision_count: state.revisions.len(),
            edge_count: state.edges.len(),
            projection_sha256,
        };
        tx.commit()?;
        Ok(receipt)
    }

    pub fn get_generation(&self, generation_id: &str) -> Result<Option<CurationGeneration>> {
        valid_id(generation_id)?;
        let db = self.read_connection()?;
        fetch_generation(&db, generation_id)
    }

    pub fn list_topic_generations(
        &self,
        topic_id: &str,
        status: Option<GenerationStatus>,
        limit: usize,
    ) -> Result<Vec<CurationGeneration>> {
        valid_id(topic_id)?;
        if !(1..=MAX_QUERY_LIMIT).contains(&limit) {
            return Err(StateError::InvalidValue);
        }
        let db = self.read_connection()?;
        let mut stmt = db.prepare(
            "SELECT id,topic_id,run_id,profile,asserted_by,model_id,status,
                    base_generation_id,projection_sha256,created_at_ms,published_at_ms
             FROM curation_generations
             WHERE topic_id=?1 AND (?2 IS NULL OR status=?2)
             ORDER BY created_at_ms, id
             LIMIT ?3",
        )?;
        let rows = stmt
            .query_map(
                params![topic_id, status.map(GenerationStatus::as_str), limit as i64],
                generation_from_row,
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.into_iter().map(build_generation).collect()
    }

    pub fn get_curated_node(&self, key: &str) -> Result<Option<CuratedNode>> {
        let parsed = CuratedKey::parse(key)?;
        let db = self.read_connection()?;
        let state = load_published_projection_from_db(&db)?;
        let Some(node) = state.nodes_by_key.get(&parsed.canonical) else {
            return Ok(None);
        };
        if curated_node_has_redacted_source(&db, &node.id)? {
            return Ok(None);
        }
        Ok(Some(materialized_to_public_node(node)))
    }

    pub fn list_topic_curated_nodes(
        &self,
        topic_id: &str,
        include_non_active: bool,
        limit: usize,
    ) -> Result<Vec<CuratedNode>> {
        valid_id(topic_id)?;
        if !(1..=MAX_QUERY_LIMIT).contains(&limit) {
            return Err(StateError::InvalidValue);
        }
        let db = self.read_connection()?;
        let state = load_published_projection_from_db(&db)?;
        let mut out: Vec<CuratedNode> = Vec::new();
        for node in state.nodes_by_key.values() {
            if node.topic_id != topic_id
                || (!include_non_active && node.status != CuratedNodeStatus::Active)
            {
                continue;
            }
            if curated_node_has_redacted_source(&db, &node.id)? {
                continue;
            }
            out.push(materialized_to_public_node(node));
        }
        out.sort_by(|a, b| a.canonical_key.cmp(&b.canonical_key));
        out.truncate(limit);
        Ok(out)
    }

    pub fn curated_node_revisions(&self, key: &str) -> Result<Vec<CuratedNodeRevision>> {
        let parsed = CuratedKey::parse(key)?;
        let node_id = curated_node_id_for_key(&parsed.canonical)?;
        let db = self.read_connection()?;
        if curated_node_has_redacted_source(&db, &node_id)? {
            return Ok(Vec::new());
        }
        let state = load_published_projection_from_db(&db)?;
        let out: Vec<CuratedNodeRevision> = state
            .revisions
            .into_iter()
            .filter(|r| r.node_id == node_id)
            .map(|r| CuratedNodeRevision {
                node_id: r.node_id,
                rev: r.rev,
                base_rev: r.base_rev,
                generation_id: r.generation_id,
                op: r.op,
                canonical_key: r.canonical_key,
                kind: r.kind,
                title: r.title,
                summary: r.summary,
                body: r.body,
                attrs: r.attrs,
                aliases: r.aliases,
                status: r.status,
                supersedes_node_id: r.supersedes_node_id,
                superseded_by_node_id: r.superseded_by_node_id,
                asserted_by: r.asserted_by,
                model_id: r.model_id,
                confidence: r.confidence,
                recorded_at_ms: r.recorded_at_ms,
                evidence: r.evidence,
            })
            .collect();
        Ok(out)
    }

    pub fn list_topic_curated_edges(
        &self,
        topic_id: &str,
        include_unlinked: bool,
        limit: usize,
    ) -> Result<Vec<CuratedEdge>> {
        valid_id(topic_id)?;
        if !(1..=MAX_QUERY_LIMIT).contains(&limit) {
            return Err(StateError::InvalidValue);
        }
        let db = self.read_connection()?;
        let state = load_published_projection_from_db(&db)?;
        let mut out = Vec::new();
        for edge in state.edges.values() {
            if edge.topic_id != topic_id
                || (!include_unlinked && edge.status != CuratedEdgeStatus::Active)
            {
                continue;
            }
            if evidence_spans_have_redacted_source(&db, &edge.evidence)?
                || curated_node_has_redacted_source(&db, &edge.from_node_id)?
                || curated_node_has_redacted_source(&db, &edge.to_node_id)?
            {
                continue;
            }
            let from_key = state
                .key_by_id
                .get(&edge.from_node_id)
                .cloned()
                .ok_or(StateError::Corrupt)?;
            let to_key = state
                .key_by_id
                .get(&edge.to_node_id)
                .cloned()
                .ok_or(StateError::Corrupt)?;
            out.push(CuratedEdge {
                id: edge.id.clone(),
                topic_id: edge.topic_id.clone(),
                from_node_id: edge.from_node_id.clone(),
                from_key,
                rel: edge.rel,
                to_node_id: edge.to_node_id.clone(),
                to_key,
                status: edge.status,
                generation_id: edge.generation_id.clone(),
                asserted_by: edge.asserted_by,
                model_id: edge.model_id.clone(),
                confidence: edge.confidence,
                created_at_ms: edge.created_at_ms,
                updated_at_ms: edge.updated_at_ms,
                evidence: edge.evidence.clone(),
            });
        }
        out.sort_by(|a, b| {
            (a.from_key.as_str(), a.rel.as_str(), a.to_key.as_str()).cmp(&(
                b.from_key.as_str(),
                b.rel.as_str(),
                b.to_key.as_str(),
            ))
        });
        out.truncate(limit);
        Ok(out)
    }

    pub fn curated_projection_digest(&self) -> Result<String> {
        let db = self.read_connection()?;
        load_published_projection_from_db(&db)?.digest()
    }

    pub fn verify_curated_integrity(&self) -> Result<CuratedIntegrityReport> {
        let db = self.read_connection()?;
        let mut fk_stmt = db.prepare("SELECT rowid FROM pragma_foreign_key_check")?;
        let fk_violations = fk_stmt.query_map([], |_| Ok(()))?.count();
        drop(fk_stmt);
        verify_curated_invariants_connection(&db)?;
        let op_log_count: i64 =
            db.query_row("SELECT COUNT(*) FROM curated_op_log", [], |r| r.get(0))?;
        let projection = load_published_projection_from_db(&db)?;
        let projection_sha256 = projection.digest()?;
        Ok(CuratedIntegrityReport {
            foreign_key_violations: fk_violations,
            op_log_count: op_log_count as usize,
            node_count: projection.nodes_by_key.len(),
            revision_count: projection.revisions.len(),
            edge_count: projection.edges.len(),
            projection_sha256,
        })
    }
}

fn materialized_to_public_node(node: &MaterializedNode) -> CuratedNode {
    CuratedNode {
        id: node.id.clone(),
        topic_id: node.topic_id.clone(),
        canonical_key: node.canonical_key.clone(),
        kind: node.kind,
        slug: node.slug.clone(),
        current_rev: node.current_rev,
        title: node.title.clone(),
        summary: node.summary.clone(),
        body: node.body.clone(),
        attrs: node.attrs.clone(),
        aliases: node.aliases.clone(),
        status: node.status,
        supersedes_node_id: node.supersedes_node_id.clone(),
        superseded_by_node_id: node.superseded_by_node_id.clone(),
        generation_id: node.generation_id.clone(),
        asserted_by: node.asserted_by,
        model_id: node.model_id.clone(),
        confidence: node.confidence,
        created_at_ms: node.created_at_ms,
        updated_at_ms: node.updated_at_ms,
    }
}

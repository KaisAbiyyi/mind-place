//! MP-R1-WP03: Topic as the primary organizing scope.
//!
//! Extends the existing `topics`, `projects` and `project_topics` tables without
//! introducing a duplicate entity model:
//! - Topics support an acyclic parent hierarchy (`parent_topic_id`).
//! - A project is a topic carrying work state (`is_project = true` and `work_status`).
//! - Many-to-many memberships (`topic_memberships`) carry provenance and
//!   typed foreign keys back to authoritative records (`source_identities`,
//!   `documents`, `curated_nodes`, `memory_claims`).
use crate::{valid_id, valid_text, Result, StateError, StateStore};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_TOPIC_HIERARCHY_DEPTH: usize = 64;
const MAX_TOPICS_PAGE: usize = 200;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TopicWorkStatus {
    Active,
    Paused,
    Completed,
    Archived,
}

impl TopicWorkStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Archived => "archived",
        }
    }

    pub(crate) fn parse(raw: &str) -> Result<Self> {
        match raw {
            "active" => Ok(Self::Active),
            "paused" => Ok(Self::Paused),
            "completed" => Ok(Self::Completed),
            "archived" => Ok(Self::Archived),
            _ => Err(StateError::Corrupt),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssertionSource {
    User,
    Model,
    Import,
    Deterministic,
}

impl AssertionSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Model => "model",
            Self::Import => "import",
            Self::Deterministic => "deterministic",
        }
    }

    pub(crate) fn parse(raw: &str) -> Result<Self> {
        match raw {
            "user" => Ok(Self::User),
            "model" => Ok(Self::Model),
            "import" => Ok(Self::Import),
            "deterministic" => Ok(Self::Deterministic),
            _ => Err(StateError::Corrupt),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TopicMemberKind {
    Source,
    CuratedNode,
    MemoryClaim,
    Document,
}

impl TopicMemberKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::CuratedNode => "curated_node",
            Self::MemoryClaim => "memory_claim",
            Self::Document => "document",
        }
    }

    pub(crate) fn parse(raw: &str) -> Result<Self> {
        match raw {
            "source" => Ok(Self::Source),
            "curated_node" => Ok(Self::CuratedNode),
            "memory_claim" => Ok(Self::MemoryClaim),
            "document" => Ok(Self::Document),
            _ => Err(StateError::Corrupt),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TopicMemberRole {
    Primary,
    Member,
    Related,
}

impl TopicMemberRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Member => "member",
            Self::Related => "related",
        }
    }

    pub(crate) fn parse(raw: &str) -> Result<Self> {
        match raw {
            "primary" => Ok(Self::Primary),
            "member" => Ok(Self::Member),
            "related" => Ok(Self::Related),
            _ => Err(StateError::Corrupt),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MembershipStatus {
    Active,
    Superseded,
    Retracted,
}

impl MembershipStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Superseded => "superseded",
            Self::Retracted => "retracted",
        }
    }

    pub(crate) fn parse(raw: &str) -> Result<Self> {
        match raw {
            "active" => Ok(Self::Active),
            "superseded" => Ok(Self::Superseded),
            "retracted" => Ok(Self::Retracted),
            _ => Err(StateError::Corrupt),
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Topic {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub parent_topic_id: Option<String>,
    pub is_project: bool,
    pub work_status: Option<TopicWorkStatus>,
    pub description: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug)]
pub struct NewTopic<'a> {
    pub id: &'a str,
    pub slug: &'a str,
    pub name: &'a str,
    pub parent_topic_id: Option<&'a str>,
    pub is_project: bool,
    pub work_status: Option<TopicWorkStatus>,
    pub description: Option<&'a str>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct TopicMembership {
    pub id: String,
    pub topic_id: String,
    pub member_kind: TopicMemberKind,
    pub member_id: String,
    pub role: TopicMemberRole,
    pub asserted_by: AssertionSource,
    pub epistemic_origin: String,
    pub evidence_source_ref: Option<String>,
    pub evidence_locator: Option<String>,
    pub confidence: Option<f64>,
    pub status: MembershipStatus,
    pub superseded_by: Option<String>,
    pub valid_from_ms: i64,
    pub valid_until_ms: Option<i64>,
    pub recorded_at_ms: i64,
}

#[derive(Clone, Debug)]
pub struct NewTopicMembership<'a> {
    pub id: &'a str,
    pub topic_id: &'a str,
    pub member_kind: TopicMemberKind,
    pub member_id: &'a str,
    pub role: TopicMemberRole,
    pub asserted_by: AssertionSource,
    pub epistemic_origin: &'a str,
    pub evidence_source_ref: Option<&'a str>,
    pub evidence_locator: Option<&'a str>,
    pub confidence: Option<f64>,
}

/// Validates a CNF v1 slug segment (`[a-z0-9-]{1,64}`).
pub fn valid_cnf_slug(slug: &str) -> Result<()> {
    if slug.is_empty()
        || slug.len() > 64
        || !slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(StateError::InvalidValue);
    }
    Ok(())
}

fn now_ms() -> Result<i64> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| StateError::Corrupt)?
        .as_millis();
    i64::try_from(millis).map_err(|_| StateError::Corrupt)
}

fn now_secs() -> Result<i64> {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| StateError::Corrupt)?
        .as_secs();
    i64::try_from(secs).map_err(|_| StateError::Corrupt)
}

/// Acquires a strictly monotonic millisecond timestamp inside an `IMMEDIATE`
/// write transaction for curated state and topic membership events.
pub(crate) fn next_curated_clock(db: &Connection, minimum_ms: i64) -> Result<i64> {
    let floor = now_ms()?.max(minimum_ms);
    let clock: i64 = db.query_row(
        "UPDATE curated_state_clock
         SET last_ms=MAX(last_ms+1,?1) WHERE id=1 RETURNING last_ms",
        [floor],
        |row| row.get(0),
    )?;
    Ok(clock)
}

fn derive_slug_candidate(seed: &str, fallback_id: &str) -> String {
    let mut out = String::with_capacity(64);
    let mut last_dash = false;
    for ch in seed.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_lowercase() || lower.is_ascii_digit() {
            if out.len() < 64 {
                out.push(lower);
                last_dash = false;
            }
        } else if !last_dash && !out.is_empty() && out.len() < 64 {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        let digest = format!("{:x}", Sha256::digest(fallback_id.as_bytes()));
        return format!("topic-{}", &digest[..12]);
    }
    out
}

fn allocate_unique_slug(db: &Connection, preferred: &str, seed_id: &str) -> Result<String> {
    let base = derive_slug_candidate(preferred, seed_id);
    if !slug_exists(db, &base)? {
        return Ok(base);
    }
    let digest = format!("{:x}", Sha256::digest(seed_id.as_bytes()));
    let suffix = format!("-{}", &digest[..8]);
    let prefix_len = 64usize.saturating_sub(suffix.len());
    let mut candidate = String::new();
    for ch in base.chars() {
        if candidate.len() + ch.len_utf8() > prefix_len {
            break;
        }
        candidate.push(ch);
    }
    while candidate.ends_with('-') {
        candidate.pop();
    }
    if candidate.is_empty() {
        candidate.push_str("topic");
    }
    candidate.push_str(&suffix);
    if !slug_exists(db, &candidate)? {
        return Ok(candidate);
    }
    for n in 2..10_000 {
        let num_suffix = format!("-{n}");
        let max_base = 64usize.saturating_sub(num_suffix.len());
        let trimmed: String = base.chars().take(max_base).collect();
        let alt = format!("{}{num_suffix}", trimmed.trim_end_matches('-'));
        if !slug_exists(db, &alt)? {
            return Ok(alt);
        }
    }
    Err(StateError::Corrupt)
}

fn slug_exists(db: &Connection, slug: &str) -> Result<bool> {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM topics WHERE slug=?1)",
        [slug],
        |r| r.get(0),
    )
    .map_err(Into::into)
}

fn topic_from_columns(
    id: String,
    slug: Option<String>,
    name: String,
    parent_topic_id: Option<String>,
    is_project: i64,
    work_status: Option<String>,
    description: Option<String>,
    created_at: i64,
    updated_at: i64,
) -> Result<Topic> {
    let slug = slug.ok_or(StateError::Corrupt)?;
    valid_cnf_slug(&slug).map_err(|_| StateError::Corrupt)?;
    let parsed_work = work_status
        .as_deref()
        .map(TopicWorkStatus::parse)
        .transpose()?;
    if (is_project == 1) != parsed_work.is_some() {
        return Err(StateError::Corrupt);
    }
    Ok(Topic {
        id,
        slug,
        name,
        parent_topic_id,
        is_project: is_project == 1,
        work_status: parsed_work,
        description,
        created_at,
        updated_at,
    })
}

pub(crate) fn fetch_topic(db: &Connection, id: &str) -> Result<Option<Topic>> {
    let raw = db
        .query_row(
            "SELECT id,slug,name,parent_topic_id,is_project,work_status,description,created_at,updated_at
             FROM topics WHERE id=?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, i64>(8)?,
                ))
            },
        )
        .optional()?;
    raw.map(
        |(
            id,
            slug,
            name,
            parent_topic_id,
            is_project,
            work_status,
            description,
            created_at,
            updated_at,
        )| {
            topic_from_columns(
                id,
                slug,
                name,
                parent_topic_id,
                is_project,
                work_status,
                description,
                created_at,
                updated_at,
            )
        },
    )
    .transpose()
}

pub(crate) fn resolve_topic_by_id_or_slug(
    db: &Connection,
    id_or_slug: &str,
) -> Result<Option<Topic>> {
    let by_id = fetch_topic(db, id_or_slug)?;
    if by_id.is_some() {
        return Ok(by_id);
    }
    let id: Option<String> = db
        .query_row("SELECT id FROM topics WHERE slug=?1", [id_or_slug], |r| {
            r.get(0)
        })
        .optional()?;
    match id {
        Some(id) => fetch_topic(db, &id),
        None => Ok(None),
    }
}

fn assert_acyclic_parent(db: &Connection, topic_id: &str, parent_id: Option<&str>) -> Result<()> {
    let Some(mut cursor) = parent_id.map(str::to_owned) else {
        return Ok(());
    };
    let mut visited = HashSet::new();
    visited.insert(topic_id.to_owned());
    for _ in 0..MAX_TOPIC_HIERARCHY_DEPTH {
        if !visited.insert(cursor.clone()) {
            return Err(StateError::InvalidValue);
        }
        let next: Option<Option<String>> = db
            .query_row(
                "SELECT parent_topic_id FROM topics WHERE id=?1",
                [&cursor],
                |r| r.get(0),
            )
            .optional()?;
        let Some(parent) = next else {
            return Err(StateError::InvalidValue);
        };
        let Some(next_id) = parent else {
            return Ok(());
        };
        cursor = next_id;
    }
    Err(StateError::InvalidValue)
}

fn sync_project_compat(
    tx: &Transaction<'_>,
    topic_id: &str,
    name: &str,
    is_project: bool,
    work_status: Option<TopicWorkStatus>,
    updated_at: i64,
) -> Result<()> {
    let legacy_status = match work_status {
        Some(TopicWorkStatus::Archived) => "archived",
        _ => "active",
    };
    let existing_project: Option<String> = tx
        .query_row(
            "SELECT id FROM projects WHERE topic_id=?1",
            [topic_id],
            |r| r.get(0),
        )
        .optional()?;
    if is_project {
        if let Some(pid) = existing_project {
            tx.execute(
                "UPDATE projects SET name=?2,status=?3,updated_at=?4 WHERE id=?1",
                params![pid, name, legacy_status, updated_at],
            )?;
        } else {
            let collides: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1)",
                [topic_id],
                |r| r.get(0),
            )?;
            if !collides {
                tx.execute(
                    "INSERT INTO projects(id,name,status,topic_id,updated_at)
                     VALUES(?1,?2,?3,?1,?4)",
                    params![topic_id, name, legacy_status, updated_at],
                )?;
                tx.execute(
                    "INSERT OR IGNORE INTO project_topics(project_id,topic_id) VALUES(?1,?1)",
                    [topic_id],
                )?;
            }
        }
    } else if let Some(pid) = existing_project {
        tx.execute(
            "UPDATE projects SET status='archived',updated_at=?2 WHERE id=?1",
            params![pid, updated_at],
        )?;
    }
    Ok(())
}

/// Forward-migrates existing v1..v9 `topics` and `projects` rows into the
/// unified v10 topic/project representation without duplicating entities.
pub(crate) fn backfill_v10_topics_and_projects(tx: &Transaction<'_>) -> Result<()> {
    let mut topic_stmt = tx.prepare("SELECT id,name FROM topics WHERE slug IS NULL ORDER BY id")?;
    let unslugged = topic_stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(topic_stmt);
    for (id, name) in unslugged {
        let seed = if name.trim().is_empty() { &id } else { &name };
        let slug = allocate_unique_slug(tx, seed, &id)?;
        tx.execute("UPDATE topics SET slug=?2 WHERE id=?1", params![id, slug])?;
    }

    let mut proj_stmt = tx.prepare(
        "SELECT id,name,status,created_at,updated_at FROM projects WHERE topic_id IS NULL ORDER BY id",
    )?;
    let projects = proj_stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(proj_stmt);

    for (proj_id, proj_name, proj_status, created_at, updated_at) in projects {
        let work_status = if proj_status == "archived" {
            "archived"
        } else {
            "active"
        };
        let mapped_topic: Option<String> = tx
            .query_row(
                "SELECT pt.topic_id FROM project_topics pt
                 WHERE pt.project_id=?1
                   AND NOT EXISTS(SELECT 1 FROM projects p WHERE p.topic_id=pt.topic_id)
                 ORDER BY pt.topic_id LIMIT 1",
                [&proj_id],
                |r| r.get(0),
            )
            .optional()?;
        let topic_id = if let Some(tid) = mapped_topic {
            tx.execute(
                "UPDATE topics SET is_project=1,work_status=?2,updated_at=MAX(updated_at,?3) WHERE id=?1",
                params![tid, work_status, updated_at],
            )?;
            tid
        } else {
            let id_taken: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM topics WHERE id=?1)",
                [&proj_id],
                |r| r.get(0),
            )?;
            let tid = if !id_taken {
                proj_id.clone()
            } else {
                let digest = format!("{:x}", Sha256::digest(proj_id.as_bytes()));
                format!("topic:project:{}", &digest[..16])
            };
            let slug = allocate_unique_slug(tx, &proj_name, &tid)?;
            tx.execute(
                "INSERT INTO topics(id,slug,name,is_project,work_status,created_at,updated_at)
                 VALUES(?1,?2,?3,1,?4,?5,?6)",
                params![tid, slug, proj_name, work_status, created_at, updated_at],
            )?;
            tx.execute(
                "INSERT OR IGNORE INTO project_topics(project_id,topic_id) VALUES(?1,?2)",
                params![proj_id, tid],
            )?;
            tid
        };
        tx.execute(
            "UPDATE projects SET topic_id=?2 WHERE id=?1",
            params![proj_id, topic_id],
        )?;
    }
    Ok(())
}

/// Legacy compatibility helper used by `StateStore::create_project`.
pub(crate) fn create_legacy_project_tx(tx: &Transaction<'_>, id: &str, name: &str) -> Result<()> {
    let trimmed = name.trim();
    let slug = allocate_unique_slug(tx, id, id)?;
    tx.execute(
        "INSERT INTO topics(id,slug,name,is_project,work_status,created_at,updated_at)
         VALUES(?1,?2,?3,1,'active',unixepoch(),unixepoch())",
        params![id, slug, trimmed],
    )
    .map_err(map_constraint)?;
    tx.execute(
        "INSERT INTO projects(id,name,status,topic_id) VALUES(?1,?2,'active',?1)",
        params![id, trimmed],
    )
    .map_err(map_constraint)?;
    tx.execute(
        "INSERT INTO project_topics(project_id,topic_id) VALUES(?1,?1)",
        [id],
    )?;
    Ok(())
}

fn map_constraint(err: rusqlite::Error) -> StateError {
    match err {
        rusqlite::Error::SqliteFailure(code, _)
            if code.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            StateError::InvalidValue
        }
        other => StateError::Sql(other),
    }
}

fn parse_membership_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<(
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<f64>,
    String,
    Option<String>,
    i64,
    Option<i64>,
    i64,
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
        row.get(11)?,
        row.get(12)?,
        row.get(13)?,
        row.get(14)?,
    ))
}

fn build_membership(
    tuple: (
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<f64>,
        String,
        Option<String>,
        i64,
        Option<i64>,
        i64,
    ),
) -> Result<TopicMembership> {
    Ok(TopicMembership {
        id: tuple.0,
        topic_id: tuple.1,
        member_kind: TopicMemberKind::parse(&tuple.2)?,
        member_id: tuple.3,
        role: TopicMemberRole::parse(&tuple.4)?,
        asserted_by: AssertionSource::parse(&tuple.5)?,
        epistemic_origin: tuple.6,
        evidence_source_ref: tuple.7,
        evidence_locator: tuple.8,
        confidence: tuple.9,
        status: MembershipStatus::parse(&tuple.10)?,
        superseded_by: tuple.11,
        valid_from_ms: tuple.12,
        valid_until_ms: tuple.13,
        recorded_at_ms: tuple.14,
    })
}

fn validate_membership_input(input: &NewTopicMembership<'_>) -> Result<()> {
    valid_id(input.id)?;
    valid_id(input.topic_id)?;
    if input.member_id.trim().is_empty()
        || input.member_id.len() > 512
        || input.member_id.contains('\0')
    {
        return Err(StateError::InvalidValue);
    }
    if !matches!(
        input.epistemic_origin,
        "explicit" | "observed" | "inferred" | "summarized"
    ) {
        return Err(StateError::InvalidValue);
    }
    if input.asserted_by != AssertionSource::User && input.evidence_source_ref.is_none() {
        return Err(StateError::InvalidValue);
    }
    if let Some(src) = input.evidence_source_ref {
        valid_id(src)?;
        if !src.starts_with("src:v1:") {
            return Err(StateError::InvalidValue);
        }
    }
    if let Some(loc) = input.evidence_locator {
        valid_text(loc)?;
        if loc.trim().len() > 1024 {
            return Err(StateError::InvalidValue);
        }
    }
    if let Some(conf) = input.confidence {
        if !conf.is_finite() || !(0.0..=1.0).contains(&conf) {
            return Err(StateError::InvalidValue);
        }
    }
    Ok(())
}

fn membership_is_redacted(db: &Connection, m: &TopicMembership) -> Result<bool> {
    if m.member_kind == TopicMemberKind::Source
        && crate::source_redaction::source_is_redacted(db, &m.member_id)?
    {
        return Ok(true);
    }
    if let Some(src) = m.evidence_source_ref.as_deref() {
        if crate::source_redaction::source_is_redacted(db, src)? {
            return Ok(true);
        }
    }
    if m.member_kind == TopicMemberKind::MemoryClaim
        && crate::source_redaction::claim_has_redacted_source(db, &m.member_id)?
    {
        return Ok(true);
    }
    if m.member_kind == TopicMemberKind::CuratedNode
        && crate::curated::curated_node_has_redacted_source(db, &m.member_id)?
    {
        return Ok(true);
    }
    Ok(false)
}

fn insert_membership_tx(
    tx: &Transaction<'_>,
    input: &NewTopicMembership<'_>,
    clock: i64,
) -> Result<TopicMembership> {
    validate_membership_input(input)?;
    if input.member_kind == TopicMemberKind::Source {
        crate::source_redaction::assert_source_not_redacted(tx, input.member_id)?;
    }
    if let Some(src) = input.evidence_source_ref {
        crate::source_redaction::assert_source_not_redacted(tx, src)?;
    }
    if input.member_kind == TopicMemberKind::MemoryClaim
        && crate::source_redaction::claim_has_redacted_source(tx, input.member_id)?
    {
        return Err(StateError::MemoryDenied);
    }
    if input.member_kind == TopicMemberKind::CuratedNode
        && crate::curated::curated_node_has_redacted_source(tx, input.member_id)?
    {
        return Err(StateError::MemoryDenied);
    }
    let (source_ref, document_uri, curated_node_id, memory_claim_id) = match input.member_kind {
        TopicMemberKind::Source => (Some(input.member_id), None, None, None),
        TopicMemberKind::Document => (None, Some(input.member_id), None, None),
        TopicMemberKind::CuratedNode => (None, None, Some(input.member_id), None),
        TopicMemberKind::MemoryClaim => (None, None, None, Some(input.member_id)),
    };
    tx.execute(
        "INSERT INTO topic_memberships (
           id, topic_id, member_kind, member_id,
           source_ref, document_uri, curated_node_id, memory_claim_id,
           role, asserted_by, epistemic_origin,
           evidence_source_ref, evidence_locator, confidence,
           status, superseded_by, valid_from_ms, valid_until_ms, recorded_at_ms
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,'active',NULL,?15,NULL,?15)",
        params![
            input.id,
            input.topic_id,
            input.member_kind.as_str(),
            input.member_id,
            source_ref,
            document_uri,
            curated_node_id,
            memory_claim_id,
            input.role.as_str(),
            input.asserted_by.as_str(),
            input.epistemic_origin,
            input.evidence_source_ref,
            input.evidence_locator.map(str::trim),
            input.confidence,
            clock,
        ],
    )
    .map_err(map_constraint)?;
    Ok(TopicMembership {
        id: input.id.to_owned(),
        topic_id: input.topic_id.to_owned(),
        member_kind: input.member_kind,
        member_id: input.member_id.to_owned(),
        role: input.role,
        asserted_by: input.asserted_by,
        epistemic_origin: input.epistemic_origin.to_owned(),
        evidence_source_ref: input.evidence_source_ref.map(str::to_owned),
        evidence_locator: input.evidence_locator.map(|s| s.trim().to_owned()),
        confidence: input.confidence,
        status: MembershipStatus::Active,
        superseded_by: None,
        valid_from_ms: clock,
        valid_until_ms: None,
        recorded_at_ms: clock,
    })
}

impl StateStore {
    /// Creates a topic (optionally carrying project work state when `is_project = true`).
    pub fn create_topic(&self, input: &NewTopic<'_>) -> Result<Topic> {
        valid_id(input.id)?;
        valid_cnf_slug(input.slug)?;
        valid_text(input.name)?;
        if input.name.trim().chars().count() > 160 {
            return Err(StateError::InvalidValue);
        }
        if let Some(parent) = input.parent_topic_id {
            valid_id(parent)?;
            if parent == input.id {
                return Err(StateError::InvalidValue);
            }
        }
        if let Some(desc) = input.description {
            valid_text(desc)?;
            if desc.len() > 4096 {
                return Err(StateError::InvalidValue);
            }
        }
        let effective_status = match (input.is_project, input.work_status) {
            (false, None) => None,
            (false, Some(_)) => return Err(StateError::InvalidValue),
            (true, Some(ws)) => Some(ws),
            (true, None) => Some(TopicWorkStatus::Active),
        };
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(parent) = input.parent_topic_id {
            assert_acyclic_parent(&tx, input.id, Some(parent))?;
        }
        let now = now_secs()?;
        let trimmed_name = input.name.trim();
        let trimmed_desc = input.description.map(str::trim);
        tx.execute(
            "INSERT INTO topics(id,slug,name,parent_topic_id,is_project,work_status,description,created_at,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?8)",
            params![
                input.id,
                input.slug,
                trimmed_name,
                input.parent_topic_id,
                i64::from(input.is_project),
                effective_status.map(TopicWorkStatus::as_str),
                trimmed_desc,
                now,
            ],
        )
        .map_err(map_constraint)?;
        if input.is_project {
            sync_project_compat(&tx, input.id, trimmed_name, true, effective_status, now)?;
        }
        let created = fetch_topic(&tx, input.id)?.ok_or(StateError::Corrupt)?;
        tx.commit()?;
        Ok(created)
    }

    pub fn get_topic(&self, id: &str) -> Result<Option<Topic>> {
        valid_id(id)?;
        let db = self.read_connection()?;
        fetch_topic(&db, id)
    }

    pub fn get_topic_by_slug(&self, slug: &str) -> Result<Option<Topic>> {
        valid_cnf_slug(slug)?;
        let db = self.read_connection()?;
        let id: Option<String> = db
            .query_row("SELECT id FROM topics WHERE slug=?1", [slug], |r| r.get(0))
            .optional()?;
        match id {
            Some(id) => fetch_topic(&db, &id),
            None => Ok(None),
        }
    }

    /// Lists topics with optional parent filter and project-only filter.
    pub fn list_topics(
        &self,
        parent_topic_id: Option<&str>,
        projects_only: bool,
        limit: usize,
    ) -> Result<Vec<Topic>> {
        if !(1..=MAX_TOPICS_PAGE).contains(&limit) {
            return Err(StateError::InvalidValue);
        }
        if let Some(parent) = parent_topic_id {
            valid_id(parent)?;
        }
        let db = self.read_connection()?;
        let mut stmt = db.prepare(
            "SELECT id,slug,name,parent_topic_id,is_project,work_status,description,created_at,updated_at
             FROM topics
             WHERE (?1 IS NULL OR parent_topic_id=?1)
               AND (?2=0 OR is_project=1)
             ORDER BY slug, id
             LIMIT ?3",
        )?;
        let rows = stmt
            .query_map(
                params![parent_topic_id, i64::from(projects_only), limit as i64],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, i64>(4)?,
                        r.get::<_, Option<String>>(5)?,
                        r.get::<_, Option<String>>(6)?,
                        r.get::<_, i64>(7)?,
                        r.get::<_, i64>(8)?,
                    ))
                },
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(
                |(
                    id,
                    slug,
                    name,
                    parent_topic_id,
                    is_project,
                    work_status,
                    description,
                    created_at,
                    updated_at,
                )| {
                    topic_from_columns(
                        id,
                        slug,
                        name,
                        parent_topic_id,
                        is_project,
                        work_status,
                        description,
                        created_at,
                        updated_at,
                    )
                },
            )
            .collect()
    }

    /// Reparents a topic while enforcing acyclicity.
    pub fn reparent_topic(&self, topic_id: &str, parent_topic_id: Option<&str>) -> Result<Topic> {
        valid_id(topic_id)?;
        if let Some(parent) = parent_topic_id {
            valid_id(parent)?;
            if parent == topic_id {
                return Err(StateError::InvalidValue);
            }
        }
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if fetch_topic(&tx, topic_id)?.is_none() {
            return Err(StateError::InvalidValue);
        }
        assert_acyclic_parent(&tx, topic_id, parent_topic_id)?;
        let now = now_secs()?;
        tx.execute(
            "UPDATE topics SET parent_topic_id=?2,updated_at=?3 WHERE id=?1",
            params![topic_id, parent_topic_id, now],
        )?;
        let updated = fetch_topic(&tx, topic_id)?.ok_or(StateError::Corrupt)?;
        tx.commit()?;
        Ok(updated)
    }

    /// Returns the ancestor chain from immediate parent up to the root topic.
    pub fn topic_ancestors(&self, topic_id: &str) -> Result<Vec<Topic>> {
        valid_id(topic_id)?;
        let db = self.read_connection()?;
        let mut current = fetch_topic(&db, topic_id)?.ok_or(StateError::InvalidValue)?;
        let mut ancestors = Vec::new();
        let mut seen = HashSet::new();
        seen.insert(current.id.clone());
        while let Some(parent_id) = current.parent_topic_id.as_deref() {
            if ancestors.len() >= MAX_TOPIC_HIERARCHY_DEPTH || !seen.insert(parent_id.to_owned()) {
                return Err(StateError::Corrupt);
            }
            let parent = fetch_topic(&db, parent_id)?.ok_or(StateError::Corrupt)?;
            ancestors.push(parent.clone());
            current = parent;
        }
        Ok(ancestors)
    }

    /// Promotes a topic to a project or updates its work state (`project = topic with work state`).
    pub fn set_topic_work_state(
        &self,
        topic_id: &str,
        is_project: bool,
        work_status: Option<TopicWorkStatus>,
    ) -> Result<Topic> {
        valid_id(topic_id)?;
        let effective = match (is_project, work_status) {
            (false, None) => None,
            (false, Some(_)) => return Err(StateError::InvalidValue),
            (true, Some(ws)) => Some(ws),
            (true, None) => Some(TopicWorkStatus::Active),
        };
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = fetch_topic(&tx, topic_id)?.ok_or(StateError::InvalidValue)?;
        let now = now_secs()?;
        tx.execute(
            "UPDATE topics SET is_project=?2,work_status=?3,updated_at=?4 WHERE id=?1",
            params![
                topic_id,
                i64::from(is_project),
                effective.map(TopicWorkStatus::as_str),
                now,
            ],
        )?;
        sync_project_compat(&tx, topic_id, &current.name, is_project, effective, now)?;
        let updated = fetch_topic(&tx, topic_id)?.ok_or(StateError::Corrupt)?;
        tx.commit()?;
        Ok(updated)
    }

    /// Associates a source, curated node, memory claim, or document with a topic,
    /// enforcing typed foreign keys and provenance requirements.
    pub fn add_topic_membership(&self, input: &NewTopicMembership<'_>) -> Result<TopicMembership> {
        validate_membership_input(input)?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let clock = next_curated_clock(&tx, 0)?;
        let created = insert_membership_tx(&tx, input, clock)?;
        tx.commit()?;
        Ok(created)
    }

    /// Atomically supersedes an active topic membership while preserving provenance history.
    pub fn supersede_topic_membership(
        &self,
        membership_id: &str,
        replacement: &NewTopicMembership<'_>,
    ) -> Result<TopicMembership> {
        valid_id(membership_id)?;
        if membership_id == replacement.id {
            return Err(StateError::InvalidValue);
        }
        validate_membership_input(replacement)?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<(String, String, String, String, i64)> = tx
            .query_row(
                "SELECT topic_id,member_kind,member_id,status,valid_from_ms
                 FROM topic_memberships WHERE id=?1",
                [membership_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?;
        let Some((topic_id, member_kind, member_id, status, valid_from_ms)) = existing else {
            return Err(StateError::InvalidValue);
        };
        if status != "active"
            || topic_id != replacement.topic_id
            || member_kind != replacement.member_kind.as_str()
            || member_id != replacement.member_id
        {
            return Err(StateError::InvalidValue);
        }
        let clock = next_curated_clock(&tx, valid_from_ms)?;
        // `superseded_by` FK is DEFERRABLE INITIALLY DEFERRED so the active slot
        // is vacated before inserting the replacement within this transaction.
        tx.execute(
            "UPDATE topic_memberships
             SET status='superseded',superseded_by=?2,valid_until_ms=?3
             WHERE id=?1",
            params![membership_id, replacement.id, clock],
        )?;
        let created = insert_membership_tx(&tx, replacement, clock)?;
        tx.commit()?;
        Ok(created)
    }

    /// Retracts an active topic membership without deleting provenance history.
    pub fn retract_topic_membership(&self, membership_id: &str) -> Result<()> {
        valid_id(membership_id)?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<(String, i64)> = tx
            .query_row(
                "SELECT status,valid_from_ms FROM topic_memberships WHERE id=?1",
                [membership_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((status, valid_from_ms)) = existing else {
            return Err(StateError::InvalidValue);
        };
        if status != "active" {
            return Err(StateError::InvalidValue);
        }
        let clock = next_curated_clock(&tx, valid_from_ms)?;
        tx.execute(
            "UPDATE topic_memberships SET status='retracted',valid_until_ms=?2 WHERE id=?1",
            params![membership_id, clock],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Lists memberships attached to a topic.
    pub fn list_topic_memberships(
        &self,
        topic_id: &str,
        include_inactive: bool,
        limit: usize,
    ) -> Result<Vec<TopicMembership>> {
        valid_id(topic_id)?;
        if !(1..=MAX_TOPICS_PAGE).contains(&limit) {
            return Err(StateError::InvalidValue);
        }
        let db = self.read_connection()?;
        let mut stmt = db.prepare(
            "SELECT id,topic_id,member_kind,member_id,role,asserted_by,epistemic_origin,
                    evidence_source_ref,evidence_locator,confidence,status,superseded_by,
                    valid_from_ms,valid_until_ms,recorded_at_ms
             FROM topic_memberships
             WHERE topic_id=?1 AND (?2=1 OR status='active')
             ORDER BY recorded_at_ms, id",
        )?;
        let mapped = stmt.query_map(
            params![topic_id, i64::from(include_inactive)],
            parse_membership_row,
        )?;
        let mut out = Vec::with_capacity(limit.min(64));
        for row in mapped {
            let item = build_membership(row?)?;
            if !membership_is_redacted(&db, &item)? {
                out.push(item);
                if out.len() == limit {
                    break;
                }
            }
        }
        Ok(out)
    }

    /// Lists all topics associated with a given member (many-to-many inverse query).
    pub fn list_member_topics(
        &self,
        member_kind: TopicMemberKind,
        member_id: &str,
        include_inactive: bool,
        limit: usize,
    ) -> Result<Vec<TopicMembership>> {
        if member_id.trim().is_empty()
            || member_id.len() > 512
            || !(1..=MAX_TOPICS_PAGE).contains(&limit)
        {
            return Err(StateError::InvalidValue);
        }
        let db = self.read_connection()?;
        let mut stmt = db.prepare(
            "SELECT id,topic_id,member_kind,member_id,role,asserted_by,epistemic_origin,
                    evidence_source_ref,evidence_locator,confidence,status,superseded_by,
                    valid_from_ms,valid_until_ms,recorded_at_ms
             FROM topic_memberships
             WHERE member_kind=?1 AND member_id=?2 AND (?3=1 OR status='active')
             ORDER BY recorded_at_ms, id",
        )?;
        let mapped = stmt.query_map(
            params![member_kind.as_str(), member_id, i64::from(include_inactive),],
            parse_membership_row,
        )?;
        let mut out = Vec::with_capacity(limit.min(64));
        for row in mapped {
            let item = build_membership(row?)?;
            if !membership_is_redacted(&db, &item)? {
                out.push(item);
                if out.len() == limit {
                    break;
                }
            }
        }
        Ok(out)
    }
}

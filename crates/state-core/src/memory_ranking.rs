//! MP-5B: evidence-linked relations and conservative memory ranking.
//! No external write surface is exposed here. MP-5C owns authorization and forget.
use crate::{
    memory::{claim_as_of_connection, next_claim_clock},
    source_record, valid_id, valid_text, verify_source_record, ClaimState, Result, StateError,
    StateStore,
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_CANDIDATES: usize = 100;
const MAX_RELATIONS_PER_CLAIM: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelationKind {
    Supports,
    Contradicts,
    Supersedes,
    Refines,
}

impl RelationKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Supports => "supports",
            Self::Contradicts => "contradicts",
            Self::Supersedes => "supersedes",
            Self::Refines => "refines",
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ClaimRelation {
    pub from_claim_id: String,
    pub to_claim_id: String,
    pub relation: String,
    pub source_ref: String,
    pub reason: String,
    pub recorded_at_ms: i64,
}

/// Immutable evidence that an earlier relation was erroneous or obsolete.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ClaimRelationResolution {
    pub from_claim_id: String,
    pub to_claim_id: String,
    pub relation: String,
    pub source_ref: String,
    pub reason: String,
    pub recorded_at_ms: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RankingMode {
    Balanced,
    SourceFirst,
    Challenge,
}

#[derive(Clone, Debug)]
pub struct RankingPolicy {
    pub mode: RankingMode,
    /// None selects latest recorded revision; Some(t) is an inclusive event clock.
    pub as_of_ms: Option<i64>,
    /// None allows only global claims. A scoped request allows global + exact scope.
    /// This Rust-only policy is not an external authorization grant.
    pub scope_id: Option<String>,
    pub limit: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct RankedClaim {
    pub claim: ClaimState,
    pub score: i64,
    /// current_candidate is only an evidence-linked memory hypothesis, not raw proof.
    /// contested, superseded, non_current, qualified and unverified are not facts.
    pub disposition: String,
    pub related_claim_ids: Vec<String>,
}

fn now_ms() -> Result<i64> {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| StateError::Corrupt)?
        .as_millis();
    i64::try_from(ms).map_err(|_| StateError::Corrupt)
}

fn allowed_scope(claim: &ClaimState, scope: Option<&str>) -> bool {
    match scope {
        Some(scope) => claim.scope_id.as_deref().is_none_or(|s| s == scope),
        None => claim.scope_id.is_none(),
    }
}

fn is_live_at(db: &Connection, claim: &ClaimState, at_ms: i64) -> Result<bool> {
    if claim.status != "active" {
        return Ok(false);
    }
    // Validity bounds are authoritative epoch seconds in schema v1, while
    // claim/event clocks and RankingPolicy use inclusive milliseconds.
    let (valid_from, valid_until): (Option<i64>, Option<i64>) = db.query_row(
        "SELECT valid_from,valid_until FROM memory_claims WHERE id=?1",
        [&claim.id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let at_seconds = at_ms.div_euclid(1000);
    Ok(valid_from.is_none_or(|v| at_seconds >= v) && valid_until.is_none_or(|v| at_seconds <= v))
}

fn positive_evidence(claim: &ClaimState) -> usize {
    claim
        .evidence
        .iter()
        .filter(|e| {
            matches!(
                e.evidence_kind.as_str(),
                "support" | "correction" | "observation"
            )
        })
        .count()
}

fn load_relations(
    db: &Connection,
    ids: &BTreeSet<String>,
    as_of_ms: i64,
) -> Result<(Vec<ClaimRelation>, BTreeSet<String>)> {
    let mut found = BTreeMap::new();
    let mut correction_sources = BTreeSet::new();
    for id in ids {
        let mut stmt = db.prepare(
            "SELECT r.from_claim_id,r.to_claim_id,r.relation,r.source_ref,
                    r.reason,r.recorded_at_ms,x.source_ref,x.recorded_at_ms
             FROM memory_claim_relations r
             LEFT JOIN memory_claim_relation_resolutions x
               ON x.from_claim_id=r.from_claim_id
              AND x.to_claim_id=r.to_claim_id AND x.relation=r.relation
             WHERE (r.from_claim_id=?1 OR r.to_claim_id=?1) AND r.recorded_at_ms<=?2
             ORDER BY r.from_claim_id,r.to_claim_id,r.relation LIMIT 129",
        )?;
        let rows = stmt
            .query_map(params![id, as_of_ms], |row| {
                Ok((
                    ClaimRelation {
                        from_claim_id: row.get(0)?,
                        to_claim_id: row.get(1)?,
                        relation: row.get(2)?,
                        source_ref: row.get(3)?,
                        reason: row.get(4)?,
                        recorded_at_ms: row.get(5)?,
                    },
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if rows.len() > MAX_RELATIONS_PER_CLAIM {
            return Err(StateError::InvalidValue);
        }
        for (relation, correction_source, correction_at) in rows {
            if correction_at.is_some_and(|at| at <= as_of_ms) {
                correction_sources.insert(correction_source.ok_or(StateError::Corrupt)?);
                continue;
            }
            found.insert(
                (
                    relation.from_claim_id.clone(),
                    relation.to_claim_id.clone(),
                    relation.relation.clone(),
                ),
                relation,
            );
        }
    }
    Ok((found.into_values().collect(), correction_sources))
}

impl StateStore {
    /// Append a directional, immutable relation attested by one registered source.
    /// A -> B means A supports/contradicts/supersedes/refines B.
    pub fn add_claim_relation(
        &self,
        from_id: &str,
        to_id: &str,
        relation: RelationKind,
        source_ref: &str,
        reason: &str,
    ) -> Result<ClaimRelation> {
        valid_id(from_id)?;
        valid_id(to_id)?;
        valid_id(source_ref)?;
        valid_text(reason)?;
        if from_id == to_id || !source_ref.starts_with("src:v1:") || reason.len() > 4096 {
            return Err(StateError::InvalidValue);
        }
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for id in [from_id, to_id] {
            let forgotten: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM memory_forget_tombstones WHERE claim_id=?1)",
                [id], |r| r.get(0),
            )?;
            if forgotten { return Err(StateError::MemoryDenied); }
        }
        let source = tx
            .query_row(
                "SELECT id,provider,source_namespace,source_kind,key_type,source_key
                 FROM source_identities WHERE id=?1",
                [source_ref],
                source_record,
            )
            .optional()?
            .ok_or(StateError::InvalidValue)?;
        verify_source_record(&source)?;

        let mut bounds = Vec::new();
        for id in [from_id, to_id] {
            let claim: Option<(Option<String>, i64)> = tx
                .query_row(
                    "SELECT c.scope_id, COALESCE(e.recorded_at_ms,e.recorded_at*1000)
                     FROM memory_claims c JOIN memory_claim_events e ON e.claim_id=c.id
                     WHERE c.id=?1 ORDER BY e.revision DESC LIMIT 1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            bounds.push(claim.ok_or(StateError::InvalidValue)?);
        }
        if bounds[0].0 != bounds[1].0 {
            return Err(StateError::InvalidValue);
        }
        // Reader caps historical incident edges at 128 per claim. Enforce
        // the same bound atomically on writers; otherwise the 129th accepted
        // edge would make ranking that claim fail forever. Resolved edges
        // still count as immutable history and cannot be silently discarded.
        for id in [from_id, to_id] {
            let count: i64 = tx.query_row(
                "SELECT COUNT(*) FROM memory_claim_relations
                 WHERE from_claim_id=?1 OR to_claim_id=?1",
                [id],
                |r| r.get(0),
            )?;
            if count >= MAX_RELATIONS_PER_CLAIM as i64 {
                return Err(StateError::InvalidValue);
            }
        }
        if relation == RelationKind::Supersedes {
            let cycle: Option<i64> = tx
                .query_row(
                    "WITH RECURSIVE reachable(id) AS (
                        SELECT r.to_claim_id FROM memory_claim_relations r
                        LEFT JOIN memory_claim_relation_resolutions x
                          ON x.from_claim_id=r.from_claim_id
                         AND x.to_claim_id=r.to_claim_id AND x.relation=r.relation
                        WHERE r.from_claim_id=?1 AND r.relation='supersedes'
                          AND x.from_claim_id IS NULL
                        UNION
                        SELECT r.to_claim_id FROM memory_claim_relations r
                        JOIN reachable ON r.from_claim_id=reachable.id
                        LEFT JOIN memory_claim_relation_resolutions x
                          ON x.from_claim_id=r.from_claim_id
                         AND x.to_claim_id=r.to_claim_id AND x.relation=r.relation
                        WHERE r.relation='supersedes' AND x.from_claim_id IS NULL
                     )
                     SELECT 1 FROM reachable WHERE id=?2 LIMIT 1",
                    params![to_id, from_id],
                    |r| r.get(0),
                )
                .optional()?;
            if cycle.is_some() {
                return Err(StateError::InvalidValue);
            }
        }
        let clock = next_claim_clock(
            &tx,
            bounds[0]
                .1
                .max(bounds[1].1)
                .checked_add(1)
                .ok_or(StateError::Corrupt)?,
        )?;
        tx.execute(
            "INSERT INTO memory_claim_relations
             (from_claim_id,to_claim_id,relation,source_ref,reason,recorded_at_ms)
             VALUES(?1,?2,?3,?4,?5,?6)",
            params![from_id, to_id, relation.as_str(), source_ref, reason, clock],
        )?;
        tx.commit()?;
        Ok(ClaimRelation {
            from_claim_id: from_id.to_owned(),
            to_claim_id: to_id.to_owned(),
            relation: relation.as_str().to_owned(),
            source_ref: source_ref.to_owned(),
            reason: reason.to_owned(),
            recorded_at_ms: clock,
        })
    }

    /// Append a durable correction to an incorrect relation without deleting
    /// its historical source and without silently changing as-of results.
    /// The same relation may only be resolved once; fresh contradictory
    /// evidence requires a new claim/evidence link rather than rewriting history.
    pub fn resolve_claim_relation(
        &self,
        from_id: &str,
        to_id: &str,
        relation: RelationKind,
        source_ref: &str,
        reason: &str,
    ) -> Result<ClaimRelationResolution> {
        valid_id(from_id)?;
        valid_id(to_id)?;
        valid_id(source_ref)?;
        valid_text(reason)?;
        if from_id == to_id || !source_ref.starts_with("src:v1:") || reason.len() > 4096 {
            return Err(StateError::InvalidValue);
        }
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for id in [from_id, to_id] {
            let forgotten: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM memory_forget_tombstones WHERE claim_id=?1)",
                [id], |r| r.get(0),
            )?;
            if forgotten { return Err(StateError::MemoryDenied); }
        }
        let source = tx
            .query_row(
                "SELECT id,provider,source_namespace,source_kind,key_type,source_key
                 FROM source_identities WHERE id=?1",
                [source_ref],
                source_record,
            )
            .optional()?
            .ok_or(StateError::InvalidValue)?;
        verify_source_record(&source)?;
        let original: i64 = tx
            .query_row(
                "SELECT recorded_at_ms FROM memory_claim_relations
                 WHERE from_claim_id=?1 AND to_claim_id=?2 AND relation=?3",
                params![from_id, to_id, relation.as_str()],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(StateError::InvalidValue)?;
        let clock = next_claim_clock(&tx, original.checked_add(1).ok_or(StateError::Corrupt)?)?;
        tx.execute(
            "INSERT INTO memory_claim_relation_resolutions
             (from_claim_id,to_claim_id,relation,source_ref,reason,recorded_at_ms)
             VALUES(?1,?2,?3,?4,?5,?6)",
            params![from_id, to_id, relation.as_str(), source_ref, reason, clock],
        )?;
        tx.commit()?;
        Ok(ClaimRelationResolution {
            from_claim_id: from_id.into(),
            to_claim_id: to_id.into(),
            relation: relation.as_str().into(),
            source_ref: source_ref.into(),
            reason: reason.into(),
            recorded_at_ms: clock,
        })
    }

    /// Rank only bounded, explicitly selected IDs. Related claims outside that
    /// selection are checked too, so omitted opponents cannot silently vanish.
    /// Every read uses one authority SQLite snapshot; no derived FTS/index.
    pub fn rank_claims(
        &self,
        claim_ids: &[&str],
        policy: &RankingPolicy,
    ) -> Result<Vec<RankedClaim>> {
        if claim_ids.is_empty()
            || claim_ids.len() > MAX_CANDIDATES
            || policy.limit == 0
            || policy.limit > MAX_CANDIDATES
        {
            return Err(StateError::InvalidValue);
        }
        if let Some(scope) = policy.scope_id.as_deref() {
            valid_id(scope)?;
        }
        let mut ids = BTreeSet::new();
        for id in claim_ids {
            valid_id(id)?;
            if !ids.insert((*id).to_owned()) {
                return Err(StateError::InvalidValue);
            }
        }
        let cutoff = policy.as_of_ms.unwrap_or(i64::MAX);
        // Current-version projection selects latest event; validity semantics
        // must be evaluated against actual wall clock, not i64::MAX.
        let evaluation_ms = policy.as_of_ms.unwrap_or(now_ms()?);
        let db = self.read_connection()?;
        let tx = db.unchecked_transaction()?;
        let mut claims = BTreeMap::new();
        for id in &ids {
            if let Some(claim) = claim_as_of_connection(&tx, id, cutoff)? {
                if allowed_scope(&claim, policy.scope_id.as_deref()) {
                    claims.insert(id.clone(), claim);
                }
            }
        }
        let (relations, resolved_sources) = load_relations(&tx, &ids, cutoff)?;
        let mut neighbors = BTreeMap::new();
        for relation in &relations {
            for id in [&relation.from_claim_id, &relation.to_claim_id] {
                if !claims.contains_key(id) && !neighbors.contains_key(id) {
                    if let Some(claim) = claim_as_of_connection(&tx, id, cutoff)? {
                        neighbors.insert(id.clone(), claim);
                    }
                }
            }
        }
        // An already-open store must fail closed if authority source metadata
        // was modified after open. Foreign keys only prove a row exists, not
        // that its canonical id still matches its source descriptor.
        let mut source_refs = resolved_sources;
        for claim in claims.values().chain(neighbors.values()) {
            for evidence in &claim.evidence {
                source_refs.insert(evidence.source_ref.clone());
            }
        }
        for edge in &relations {
            source_refs.insert(edge.source_ref.clone());
        }
        for source_id in source_refs {
            let record = tx
                .query_row(
                    "SELECT id,provider,source_namespace,source_kind,key_type,source_key
                     FROM source_identities WHERE id=?1",
                    [&source_id],
                    source_record,
                )
                .optional()?
                .ok_or(StateError::Corrupt)?;
            verify_source_record(&record)?;
        }
        let newest = claims
            .values()
            .chain(neighbors.values())
            .map(|c| c.recorded_at_ms)
            .max()
            .unwrap_or(0);
        let mut ranked = Vec::new();
        for (id, claim) in claims {
            let support = positive_evidence(&claim);
            let current = is_live_at(&tx, &claim, evaluation_ms)?;
            // Confidence is an immutable state field; inferred confidence
            // is a bounded tie-breaker, never stronger than source authority.
            let confidence: Option<f64> = tx.query_row(
                "SELECT confidence FROM memory_claims WHERE id=?1",
                [&id],
                |r| r.get(0),
            )?;
            let confidence_weight = confidence
                .map(|value| (value.clamp(0.0, 1.0) * 100.0).round() as i64)
                .unwrap_or(0);
            let authority = match claim.asserted_by.as_str() {
                "user" => 300,
                "deterministic" => 170,
                "import" => 130,
                _ => 40,
            };
            let inference = match claim.epistemic_origin.as_str() {
                "explicit" => 260,
                "observed" => 210,
                "summarized" => 90,
                _ => 20,
            };
            let recency =
                120 - ((newest.saturating_sub(claim.recorded_at_ms) / 86_400_000).clamp(0, 120));
            let evidence_weight = (support.min(8) as i64)
                * if policy.mode == RankingMode::SourceFirst {
                    75
                } else {
                    35
                };
            let mut score = authority + inference + recency + evidence_weight + confidence_weight;
            if policy.scope_id.as_deref() == claim.scope_id.as_deref() {
                score += 120;
            }
            let mut contested = false;
            let mut superseded = false;
            let mut qualified = false;
            let mut related = BTreeSet::new();
            for edge in &relations {
                if edge.from_claim_id != id && edge.to_claim_id != id {
                    continue;
                }
                let other_id = if edge.from_claim_id == id {
                    &edge.to_claim_id
                } else {
                    &edge.from_claim_id
                };
                let other = neighbors.get(other_id).or_else(|| {
                    // For two ranked candidates, both are in the same scoped
                    // input but have not necessarily been processed yet.
                    // Their as-of snapshot is queried below when needed.
                    None
                });
                // Resolve both selected and omitted opponents from the same
                // read transaction. No claim text is serialized from outside
                // the explicitly selected candidate list.
                let other_snapshot = if other.is_none() {
                    claim_as_of_connection(&tx, other_id, cutoff)?
                } else {
                    None
                };
                let opponent = other.or(other_snapshot.as_ref());
                let Some(opponent) = opponent else {
                    continue;
                };
                if !allowed_scope(opponent, policy.scope_id.as_deref()) {
                    continue;
                }
                related.insert(other_id.clone());
                // Supersession evidence is append-only. A retracted/superseded
                // intermediate successor must never silently resurrect the
                // older claim. Explicit correction is required to reassert it.
                if edge.relation == "supersedes" && edge.to_claim_id == id {
                    superseded = true;
                    continue;
                }
                if !is_live_at(&tx, opponent, evaluation_ms)? {
                    continue;
                }
                match edge.relation.as_str() {
                    "contradicts" if current => contested = true,
                    "refines" if edge.to_claim_id == id => qualified = true,
                    "supports" if edge.to_claim_id == id => score += 20,
                    _ => {}
                }
            }
            let disposition = if !current {
                "non_current"
            } else if superseded {
                "superseded"
            } else if contested {
                "contested"
            } else if support == 0 {
                "unverified"
            } else if qualified {
                "qualified"
            } else {
                "current_candidate"
            };
            if disposition == "current_candidate" {
                score += 300;
            }
            if policy.mode == RankingMode::Challenge && disposition != "current_candidate" {
                score += 500;
            }
            if policy.mode != RankingMode::Challenge
                && matches!(disposition, "non_current" | "superseded")
            {
                continue;
            }
            ranked.push(RankedClaim {
                claim,
                score,
                disposition: disposition.to_owned(),
                related_claim_ids: related.into_iter().collect(),
            });
        }
        tx.commit()?;
        ranked.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| a.claim.id.cmp(&b.claim.id))
        });
        ranked.truncate(policy.limit);
        Ok(ranked)
    }
}

/// Safe bounded serialization for later Phase 5D context/UI clients.
/// Inputs are untrusted claims; quoted statements are never instructions.
/// Source IDs are provenance handles, not evidence bytes or disclosure grants.
pub fn serialize_ranked_context(ranked: &[RankedClaim]) -> Result<String> {
    if ranked.len() > MAX_CANDIDATES {
        return Err(StateError::InvalidValue);
    }
    let mut result =
        String::from("MEMORY CANDIDATES (untrusted; verify authorized raw sources before use):\n");
    for item in ranked {
        let statement: String = item.claim.statement.chars().take(512).collect();
        let statement = serde_json::to_string(&statement).map_err(|_| StateError::Corrupt)?;
        let reliability = if item.disposition == "current_candidate" {
            "EVIDENCE_LINKED_HYPOTHESIS"
        } else {
            "DO_NOT_ASSERT_AS_FACT"
        };
        let line = format!(
            "- [{}; {}; revision={}; id={}] {}\n",
            reliability, item.disposition, item.claim.revision, item.claim.id, statement
        );
        result.push_str(&line);
    }
    Ok(result)
}

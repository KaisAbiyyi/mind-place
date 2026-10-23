//! MP-5D: bounded, read-only owner inspection of authoritative memory claims.
//! Not a source fetch, a capability grant, or a path to resurrect forgotten facts.
use crate::{
    memory::{ClaimRevision, ClaimState},
    memory_ranking::{RankedClaim, RankingMode, RankingPolicy},
    valid_id, Result, StateError, StateStore,
};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

const MAX_VISIBLE: usize = 50;
const MAX_HISTORY: usize = 128;
const MAX_RELATIONS: usize = 64;

#[derive(Clone, Debug, Serialize)]
pub struct MemoryInspectRelation {
    pub other_claim_id: String,
    pub direction: String,
    pub relation: String,
    pub source_ref: String,
    pub reason: String,
    pub recorded_at_ms: i64,
    pub resolved_at_ms: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MemoryInspectionDetail {
    pub ranked: RankedClaim,
    pub history: Vec<ClaimRevision>,
    pub relations: Vec<MemoryInspectRelation>,
}

fn inspection_policy(limit: usize) -> RankingPolicy {
    RankingPolicy {
        mode: RankingMode::Challenge,
        as_of_ms: None,
        scope_id: None,
        limit,
    }
}

impl StateStore {
    /// Global owner view only: scoped claims are not mixed into a global listing.
    /// The Tauri caller MUST enforce an active unlock lease before/after the read.
    /// Deliberately excludes permanent forget tombstones, even when searching.
    pub fn memory_inspection_overview(&self, query: &str) -> Result<Vec<RankedClaim>> {
        let query = query.trim();
        if query.chars().count() > 128 {
            return Err(StateError::InvalidValue);
        }
        let db = self.read_connection()?;
        let mut statement = db.prepare(
            "SELECT c.id FROM memory_claims c
             WHERE c.scope_id IS NULL
               AND (?1='' OR instr(lower(c.statement),lower(?1))>0)
               AND NOT EXISTS (
                 SELECT 1 FROM memory_forget_tombstones t WHERE t.claim_id=c.id
               )
             ORDER BY c.id LIMIT ?2",
        )?;
        let ids = statement
            .query_map(params![query, MAX_VISIBLE as i64], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(statement);
        drop(db);
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        if refs.is_empty() {
            return Ok(Vec::new());
        }
        // Challenged/disputed/superseded claims MUST remain visible in this
        // diagnostic UI, never silently represented as current truth.
        self.rank_claims(&refs, &inspection_policy(MAX_VISIBLE))
    }

    pub fn memory_inspection_detail(&self, claim_id: &str) -> Result<Option<MemoryInspectionDetail>> {
        valid_id(claim_id)?;
        let Some(visible) = self.claim_as_of(claim_id, i64::MAX)? else {
            return Ok(None);
        };
        if visible.scope_id.is_some() {
            return Ok(None);
        }
        let Some(history) = self.claim_history(claim_id)? else {
            return Ok(None);
        };
        if history.len() > MAX_HISTORY {
            return Err(StateError::InvalidValue);
        }
        let Some(ranked) = self
            .rank_claims(&[claim_id], &inspection_policy(1))?
            .into_iter()
            .next()
        else {
            return Ok(None);
        };

        let db = self.read_connection()?;
        let mut statement = db.prepare(
            "SELECT r.from_claim_id,r.to_claim_id,r.relation,r.source_ref,
                    r.reason,r.recorded_at_ms,x.recorded_at_ms
             FROM memory_claim_relations r
             JOIN memory_claims f ON f.id=r.from_claim_id AND f.scope_id IS NULL
             JOIN memory_claims t ON t.id=r.to_claim_id AND t.scope_id IS NULL
             LEFT JOIN memory_claim_relation_resolutions x
               ON x.from_claim_id=r.from_claim_id
              AND x.to_claim_id=r.to_claim_id AND x.relation=r.relation
             WHERE (r.from_claim_id=?1 OR r.to_claim_id=?1)
               AND NOT EXISTS (SELECT 1 FROM memory_forget_tombstones z WHERE z.claim_id=f.id)
               AND NOT EXISTS (SELECT 1 FROM memory_forget_tombstones z WHERE z.claim_id=t.id)
             ORDER BY r.recorded_at_ms,r.from_claim_id,r.to_claim_id,r.relation
             LIMIT ?2",
        )?;
        let rows = statement.query_map(params![claim_id, (MAX_RELATIONS + 1) as i64], |row| {
            let from: String = row.get(0)?;
            let to: String = row.get(1)?;
            Ok(MemoryInspectRelation {
                direction: if from == claim_id { "outgoing" } else { "incoming" }.into(),
                other_claim_id: if from == claim_id { to } else { from },
                relation: row.get(2)?,
                source_ref: row.get(3)?,
                reason: row.get(4)?,
                recorded_at_ms: row.get(5)?,
                resolved_at_ms: row.get(6)?,
            })
        })?;
        let relations = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        if relations.len() > MAX_RELATIONS {
            return Err(StateError::InvalidValue);
        }
        drop(statement);
        drop(db);

        // An interleaved forget wins over earlier reads; never return stale
        // history to the UI if a memory was forgotten during inspection.
        let still_visible: Option<ClaimState> = self.claim_as_of(claim_id, i64::MAX)?;
        if still_visible.is_none_or(|claim| claim.scope_id.is_some()) {
            return Ok(None);
        }
        Ok(Some(MemoryInspectionDetail { ranked, history, relations }))
    }
}

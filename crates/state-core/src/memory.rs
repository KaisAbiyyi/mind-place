//! MP-5A: authoritative, append-oriented memory claim state and evidence.
//! This module is deliberately not exposed over Tauri/MCP. Phase 5C owns
//! capability-checked write entry points and destructive/forget behavior.
use crate::{
    source_record, valid_id, valid_text, verify_source_record, MemoryClaim, Result, StateError,
    StateStore,
};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClaimEvidence {
    pub source_ref: String,
    pub evidence_kind: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaimTransition {
    Dispute,
    MarkUncertain,
    Reinstate,
    Supersede,
    Retract,
    Archive,
}

impl ClaimTransition {
    fn status(self) -> &'static str {
        match self {
            Self::Dispute => "disputed",
            Self::MarkUncertain => "uncertain",
            Self::Reinstate => "active",
            Self::Supersede => "superseded",
            Self::Retract => "retracted",
            Self::Archive => "historical",
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ClaimRevision {
    pub revision: i64,
    pub recorded_at_ms: i64,
    pub event_type: String,
    pub status: String,
    pub reason: Option<String>,
    pub superseded_by: Option<String>,
    pub evidence: Vec<ClaimEvidence>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ClaimState {
    pub id: String,
    pub statement: String,
    pub asserted_by: String,
    pub epistemic_origin: String,
    pub scope_id: Option<String>,
    pub revision: i64,
    pub status: String,
    pub recorded_at_ms: i64,
    /// Evidence introduced at or before this revision; history remains available
    /// even after retraction. These are IDs, never retrieved source bytes.
    pub evidence: Vec<ClaimEvidence>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TransitionPayload {
    from_status: String,
    to_status: String,
    reason: String,
    superseded_by: Option<String>,
}

fn validate_claim(claim: &MemoryClaim) -> Result<()> {
    valid_id(&claim.id)?;
    valid_text(&claim.statement)?;
    if !matches!(
        claim.asserted_by.as_str(),
        "user" | "model" | "import" | "deterministic"
    ) || !matches!(
        claim.epistemic_origin.as_str(),
        "explicit" | "observed" | "inferred" | "summarized"
    ) || !matches!(claim.status.as_str(), "active" | "uncertain")
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

fn add_evidence(
    tx: &rusqlite::Transaction<'_>,
    claim_id: &str,
    revision: i64,
    evidence: &[ClaimEvidence],
) -> Result<()> {
    if evidence.is_empty() || evidence.len() > 32 {
        return Err(StateError::InvalidValue);
    }
    let mut seen = std::collections::HashSet::new();
    for entry in evidence {
        valid_id(&entry.source_ref)?;
        if !entry.source_ref.starts_with("src:v1:")
            || !matches!(
                entry.evidence_kind.as_str(),
                "support" | "correction" | "contradiction" | "retraction" | "observation"
            )
            || !seen.insert((&entry.source_ref, &entry.evidence_kind))
        {
            return Err(StateError::InvalidValue);
        }
        // All evidence must be registered in the authority DB. A disposable
        // index row, guessed source ID, or missing identity is never evidence.
        let record = tx
            .query_row(
                "SELECT id,provider,source_namespace,source_kind,key_type,source_key
             FROM source_identities WHERE id=?1",
                [&entry.source_ref],
                source_record,
            )
            .optional()?
            .ok_or(StateError::InvalidValue)?;
        verify_source_record(&record)?;
        tx.execute(
            "INSERT INTO memory_claim_event_evidence
             (claim_id,revision,source_ref,evidence_kind) VALUES(?1,?2,?3,?4)",
            params![claim_id, revision, entry.source_ref, entry.evidence_kind],
        )?;
        // Legacy index of all supporting/historical evidence. Consumers of
        // current truth must use claim_history/claim_as_of, not this union.
        tx.execute(
            "INSERT OR IGNORE INTO claim_evidence(claim_id,source_ref,evidence_kind)
             VALUES(?1,?2,?3)",
            params![claim_id, entry.source_ref, entry.evidence_kind],
        )?;
    }
    Ok(())
}

fn events(db: &rusqlite::Connection, claim_id: &str) -> Result<Vec<ClaimRevision>> {
    let mut stmt = db.prepare(
        "SELECT revision,COALESCE(recorded_at_ms, recorded_at*1000),
                event_type,payload_json
         FROM memory_claim_events WHERE claim_id=?1 ORDER BY revision",
    )?;
    let rows = stmt.query_map([claim_id], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    let mut history = Vec::new();
    for row in rows {
        let (revision, recorded_at_ms, event_type, json) = row?;
        let (status, reason, superseded_by) = if revision == 1 && event_type == "created" {
            let value: serde_json::Value =
                serde_json::from_str(&json).map_err(|_| StateError::Corrupt)?;
            if value.get("id").and_then(|v| v.as_str()) != Some(claim_id) {
                return Err(StateError::Corrupt);
            }
            (
                value
                    .get("status")
                    .and_then(|v| v.as_str())
                    .ok_or(StateError::Corrupt)?
                    .to_owned(),
                None,
                None,
            )
        } else if event_type == "transition" && revision > 1 {
            let payload: TransitionPayload =
                serde_json::from_str(&json).map_err(|_| StateError::Corrupt)?;
            if history.last().map(|h: &ClaimRevision| h.status.as_str())
                != Some(payload.from_status.as_str())
            {
                return Err(StateError::Corrupt);
            }
            (
                payload.to_status,
                Some(payload.reason),
                payload.superseded_by,
            )
        } else {
            return Err(StateError::Corrupt);
        };
        if revision != history.len() as i64 + 1
            || history
                .last()
                .is_some_and(|h: &ClaimRevision| recorded_at_ms < h.recorded_at_ms)
            || !matches!(
                status.as_str(),
                "active" | "uncertain" | "disputed" | "superseded" | "retracted" | "historical"
            )
        {
            return Err(StateError::Corrupt);
        }
        let mut evidence_stmt = db.prepare(
            "SELECT source_ref,evidence_kind FROM memory_claim_event_evidence
             WHERE claim_id=?1 AND revision=?2 ORDER BY source_ref,evidence_kind",
        )?;
        let evidence = evidence_stmt
            .query_map(params![claim_id, revision], |r| {
                Ok(ClaimEvidence {
                    source_ref: r.get(0)?,
                    evidence_kind: r.get(1)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        history.push(ClaimRevision {
            revision,
            recorded_at_ms,
            event_type,
            status,
            reason,
            superseded_by,
            evidence,
        });
    }
    if history.is_empty() {
        return Err(StateError::Corrupt);
    }
    Ok(history)
}

impl StateStore {
    /// Create an initial memory with source-backed evidence in one IMMEDIATE
    /// transaction. A bare claim creation remains supported for legacy tests,
    /// but is not evidence-qualified for Phase 5 truth decisions.
    pub fn create_evidenced_claim(
        &self,
        claim: &MemoryClaim,
        evidence: &[ClaimEvidence],
    ) -> Result<()> {
        validate_claim(claim)?;
        let clock = now_ms()?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO memory_claims(id,statement,asserted_by,epistemic_origin,status)
             VALUES(?1,?2,?3,?4,?5)",
            params![
                claim.id,
                claim.statement,
                claim.asserted_by,
                claim.epistemic_origin,
                claim.status
            ],
        )?;
        tx.execute(
            "INSERT INTO memory_claim_events
             (claim_id,revision,event_type,payload_json,recorded_at_ms)
             VALUES(?1,1,'created',?2,?3)",
            params![
                claim.id,
                serde_json::to_string(claim).map_err(|_| StateError::InvalidValue)?,
                clock
            ],
        )?;
        add_evidence(&tx, &claim.id, 1, evidence)?;
        tx.commit()?;
        Ok(())
    }

    /// No writer is exposed to clients by this phase. Caller is a trusted
    /// authority path; external capability checks will be added in MP-5C.
    /// Terminal superseded/retracted claims cannot be silently restored.
    pub fn transition_claim(
        &self,
        claim_id: &str,
        transition: ClaimTransition,
        reason: &str,
        evidence: &[ClaimEvidence],
        superseded_by: Option<&str>,
    ) -> Result<ClaimRevision> {
        valid_id(claim_id)?;
        valid_text(reason)?;
        if reason.len() > 4096 {
            return Err(StateError::InvalidValue);
        }
        if (transition == ClaimTransition::Supersede) != superseded_by.is_some() {
            return Err(StateError::InvalidValue);
        }
        if let Some(other) = superseded_by {
            valid_id(other)?;
            if other == claim_id {
                return Err(StateError::InvalidValue);
            }
        }
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let status: String = tx
            .query_row(
                "SELECT status FROM memory_claims WHERE id=?1",
                [claim_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(StateError::InvalidValue)?;
        if matches!(status.as_str(), "superseded" | "retracted") || status == transition.status() {
            return Err(StateError::InvalidValue);
        }
        if let Some(other) = superseded_by {
            if tx
                .query_row("SELECT id FROM memory_claims WHERE id=?1", [other], |r| {
                    r.get::<_, String>(0)
                })
                .optional()?
                .is_none()
            {
                return Err(StateError::InvalidValue);
            }
        }
        let history = events(&tx, claim_id)?;
        let last = history.last().ok_or(StateError::Corrupt)?;
        if last.status != status || last.revision >= 1_000_000 {
            return Err(StateError::Corrupt);
        }
        let revision = last.revision + 1;
        let clock = now_ms()?.max(
            last.recorded_at_ms
                .checked_add(1)
                .ok_or(StateError::Corrupt)?,
        );
        let payload = TransitionPayload {
            from_status: status,
            to_status: transition.status().to_owned(),
            reason: reason.to_owned(),
            superseded_by: superseded_by.map(str::to_owned),
        };
        tx.execute(
            "INSERT INTO memory_claim_events
             (claim_id,revision,event_type,payload_json,recorded_at_ms)
             VALUES(?1,?2,'transition',?3,?4)",
            params![
                claim_id,
                revision,
                serde_json::to_string(&payload).map_err(|_| StateError::InvalidValue)?,
                clock
            ],
        )?;
        add_evidence(&tx, claim_id, revision, evidence)?;
        tx.execute(
            "UPDATE memory_claims SET status=?2 WHERE id=?1",
            params![claim_id, transition.status()],
        )?;
        tx.commit()?;
        Ok(ClaimRevision {
            revision,
            recorded_at_ms: clock,
            event_type: "transition".into(),
            status: transition.status().into(),
            reason: Some(reason.to_owned()),
            superseded_by: superseded_by.map(str::to_owned),
            evidence: evidence.to_vec(),
        })
    }

    /// Returns immutable revision history with source IDs only (no disclosure).
    pub fn claim_history(&self, claim_id: &str) -> Result<Option<Vec<ClaimRevision>>> {
        valid_id(claim_id)?;
        let db = self.read_connection()?;
        let tx = db.unchecked_transaction()?;
        let current: Option<String> = tx
            .query_row(
                "SELECT status FROM memory_claims WHERE id=?1",
                [claim_id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(current) = current else {
            return Ok(None);
        };
        let history = events(&tx, claim_id)?;
        if history.last().map(|h| h.status.as_str()) != Some(current.as_str()) {
            return Err(StateError::Corrupt);
        }
        tx.commit()?;
        Ok(Some(history))
    }

    /// Reconstruct history at an inclusive Unix-millisecond boundary without
    /// querying the rebuildable index or resolving possibly revoked source data.
    pub fn claim_as_of(&self, claim_id: &str, as_of_ms: i64) -> Result<Option<ClaimState>> {
        valid_id(claim_id)?;
        let db = self.read_connection()?;
        let tx = db.unchecked_transaction()?;
        let base: Option<(String, String, String, Option<String>, String)> = tx
            .query_row(
                "SELECT statement,asserted_by,epistemic_origin,scope_id,status
             FROM memory_claims WHERE id=?1",
                [claim_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?;
        let Some((statement, asserted_by, epistemic_origin, scope_id, current_status)) = base
        else {
            return Ok(None);
        };
        let history = events(&tx, claim_id)?;
        if history.last().map(|h| h.status.as_str()) != Some(current_status.as_str()) {
            return Err(StateError::Corrupt);
        }
        let selected = history.iter().rev().find(|h| h.recorded_at_ms <= as_of_ms);
        let result = selected.map(|h| ClaimState {
            id: claim_id.to_owned(),
            statement,
            asserted_by,
            epistemic_origin,
            scope_id,
            revision: h.revision,
            status: h.status.clone(),
            recorded_at_ms: h.recorded_at_ms,
            evidence: history
                .iter()
                .take(h.revision as usize)
                .flat_map(|entry| entry.evidence.iter().cloned())
                .collect(),
        });
        tx.commit()?;
        Ok(result)
    }
}

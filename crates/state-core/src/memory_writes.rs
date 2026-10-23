//! MP-5C: transactional, capability-checked, idempotent curated memory mutations.
//! This trusted Rust API is intentionally NOT registered as an MCP/Tauri tool.
//! Callers must enforce owner confirmation for destructive actions, and may only
//! supply a SHA-256 derived from a verified bearer in the unlocked security path.
use crate::{
    memory::{add_evidence, events, next_claim_clock, validate_claim}, valid_id, valid_text,
    ClaimEvidence, ClientCapability, MemoryClaim, Result, StateError, StateStore,
};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum MemoryWrite<'a> {
    Create {
        claim: &'a MemoryClaim,
        scope_id: Option<&'a str>,
        evidence: &'a [ClaimEvidence],
    },
    // Immutable statements are updated by an atomic successor + supersession,
    // never by replacing the text/provenance of an old claim.
    Supersede {
        previous_id: &'a str,
        successor: &'a MemoryClaim,
        scope_id: Option<&'a str>,
        evidence: &'a [ClaimEvidence],
        reason: &'a str,
    },
    // Curated-memory non-disclosure, not physical destruction of source bytes.
    // Physical raw/source/backup erasure belongs to MP-9C.
    Forget {
        claim_id: &'a str,
        reason: &'a str,
    },
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct MemoryWriteReceipt {
    pub client_id: String,
    pub claim_id: String,
    pub operation: String,
    pub revision: i64,
    pub recorded_at_ms: i64,
}

fn valid_hash(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn check_text_reason(reason: &str) -> Result<()> {
    valid_text(reason)?;
    if reason.len() > 4096 {
        return Err(StateError::InvalidValue);
    }
    Ok(())
}

fn assert_not_forgotten(db: &rusqlite::Connection, id: &str) -> Result<()> {
    let erased: Option<i64> = db.query_row(
        "SELECT 1 FROM memory_forget_tombstones WHERE claim_id=?1",
        [id], |row| row.get(0),
    ).optional()?;
    if erased.is_some() {
        return Err(StateError::MemoryDenied);
    }
    Ok(())
}

fn assert_not_duplicate(db: &rusqlite::Connection, claim: &MemoryClaim, scope: Option<&str>) -> Result<()> {
    let exists: Option<String> = db.query_row(
        "SELECT id FROM memory_claims WHERE scope_id IS ?1
         AND lower(trim(statement))=lower(trim(?2))
         AND status IN ('active','uncertain')
         AND NOT EXISTS(SELECT 1 FROM memory_forget_tombstones t WHERE t.claim_id=memory_claims.id)
         LIMIT 1",
        params![scope, claim.statement], |row| row.get(0),
    ).optional()?;
    if exists.is_some() {
        return Err(StateError::InvalidValue);
    }
    Ok(())
}

fn insert_claim(
    tx: &rusqlite::Transaction<'_>,
    claim: &MemoryClaim,
    scope: Option<&str>,
    evidence: &[ClaimEvidence],
) -> Result<i64> {
    validate_claim(claim)?;
    if let Some(s) = scope { valid_id(s)?; }
    assert_not_duplicate(tx, claim, scope)?;
    let clock = next_claim_clock(tx, 0)?;
    tx.execute(
        "INSERT INTO memory_claims(id,statement,scope_id,asserted_by,epistemic_origin,status)
         VALUES(?1,?2,?3,?4,?5,?6)",
        params![claim.id, claim.statement, scope, claim.asserted_by, claim.epistemic_origin, claim.status],
    )?;
    tx.execute(
        "INSERT INTO memory_claim_events(claim_id,revision,event_type,payload_json,recorded_at_ms)
         VALUES(?1,1,'created',?2,?3)",
        params![claim.id, serde_json::to_string(claim).map_err(|_|StateError::InvalidValue)?, clock],
    )?;
    add_evidence(tx, &claim.id, 1, evidence)?;
    Ok(clock)
}

impl StateStore {
    /// One SQLite IMMEDIATE transaction covers authentication, idempotency,
    /// state, evidence, tombstone and receipt. Revocation cannot race after
    /// authentication but before the commit. Never accept an untrusted client_id.
    pub fn apply_memory_write(
        &self,
        token_sha256: &str,
        request_key: &str,
        operation: &MemoryWrite<'_>,
    ) -> Result<MemoryWriteReceipt> {
        if !valid_hash(token_sha256)
            || !(16..=128).contains(&request_key.len())
            || !request_key.bytes().all(|b|
                b.is_ascii_alphanumeric() || b == b':' || b == b'-' || b == b'_')
        {
            return Err(StateError::InvalidValue);
        }
        let kind = match operation {
            MemoryWrite::Create { .. } => "create",
            MemoryWrite::Supersede { .. } => "supersede",
            MemoryWrite::Forget { .. } => "forget",
        };
        let required = if kind == "forget" { ClientCapability::Admin } else { ClientCapability::Write };
        let canonical = serde_json::to_vec(operation).map_err(|_| StateError::InvalidValue)?;
        if canonical.len() > 64 * 1024 { return Err(StateError::InvalidValue); }
        let digest = format!("{:x}", Sha256::digest(&canonical));

        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let client_id: String = tx.query_row(
            "SELECT c.id FROM client_credentials k
             JOIN clients c ON c.id=k.client_id
             JOIN client_capabilities p ON p.client_id=c.id
             WHERE k.token_sha256=?1 AND c.state='enabled'
             AND p.capability=?2 AND p.revoked_at IS NULL",
            params![token_sha256, required.as_str()], |row| row.get(0),
        ).optional()?.ok_or(StateError::MemoryDenied)?;

        let receipt = tx.query_row(
            "SELECT operation_hash,operation_kind,claim_id,revision,recorded_at_ms
             FROM memory_write_receipts WHERE client_id=?1 AND request_key=?2",
            params![client_id, request_key], |row| Ok((
                row.get::<_, String>(0)?, row.get::<_, String>(1)?,
                row.get::<_, String>(2)?, row.get::<_, i64>(3)?, row.get::<_, i64>(4)?,
            )),
        ).optional()?;
        if let Some((hash, old_kind, id, revision, at)) = receipt {
            if hash != digest || old_kind != kind { return Err(StateError::InvalidValue); }
            tx.commit()?;
            return Ok(MemoryWriteReceipt { client_id, claim_id: id, operation: kind.into(), revision, recorded_at_ms: at });
        }

        let (id, revision, clock) = match operation {
            MemoryWrite::Create { claim, scope_id, evidence } => {
                let clock = insert_claim(&tx, claim, *scope_id, evidence)?;
                (claim.id.clone(), 1, clock)
            }
            MemoryWrite::Supersede { previous_id, successor, scope_id, evidence, reason } => {
                valid_id(previous_id)?;
                check_text_reason(reason)?;
                if *previous_id == successor.id { return Err(StateError::InvalidValue); }
                assert_not_forgotten(&tx, previous_id)?;
                let previous: Option<(String, Option<String>)> = tx.query_row(
                    "SELECT status,scope_id FROM memory_claims WHERE id=?1",
                    [previous_id], |r| Ok((r.get(0)?, r.get(1)?)),
                ).optional()?;
                let (status, previous_scope) = previous.ok_or(StateError::InvalidValue)?;
                if !matches!(status.as_str(), "active" | "uncertain" | "disputed" | "historical") 
                    || previous_scope.as_deref() != *scope_id { return Err(StateError::InvalidValue); }
                let history = events(&tx, previous_id)?;
                let last = history.last().ok_or(StateError::Corrupt)?;
                if last.status != status || last.revision >= 1_000_000 { return Err(StateError::Corrupt); }
                let created_at = insert_claim(&tx, successor, *scope_id, evidence)?;
                let revision = last.revision + 1;
                let clock = next_claim_clock(&tx, last.recorded_at_ms.checked_add(1).ok_or(StateError::Corrupt)?)?;
                let payload = serde_json::json!({
                    "from_status": status, "to_status": "superseded",
                    "reason": reason, "superseded_by": successor.id,
                });
                tx.execute(
                    "INSERT INTO memory_claim_events(claim_id,revision,event_type,payload_json,recorded_at_ms)
                     VALUES(?1,?2,'transition',?3,?4)",
                    params![previous_id, revision, payload.to_string(), clock],
                )?;
                // A new successor must have its own registered source-backed
                // evidence; the previous claim's proof is never rewritten.
                add_evidence(&tx, previous_id, revision, evidence)?;
                tx.execute(
                    "UPDATE memory_claims SET status='superseded' WHERE id=?1",
                    [previous_id],
                )?;
                (successor.id.clone(), 1, created_at)
            }
            MemoryWrite::Forget { claim_id, reason } => {
                valid_id(claim_id)?;
                check_text_reason(reason)?;
                assert_not_forgotten(&tx, claim_id)?;
                let history = events(&tx, claim_id)
                    .map_err(|e| if matches!(e, StateError::Corrupt) { StateError::InvalidValue } else { e })?;
                let last = history.last().ok_or(StateError::Corrupt)?;
                let clock = next_claim_clock(&tx, last.recorded_at_ms.checked_add(1).ok_or(StateError::Corrupt)?)?;
                // Deferred FK to the receipt is resolved on this transaction's
                // commit, so a failed receipt insert cannot leave a tombstone.
                tx.execute(
                    "INSERT INTO memory_forget_tombstones(claim_id,client_id,request_key,reason,recorded_at_ms)
                     VALUES(?1,?2,?3,?4,?5)",
                    params![claim_id, client_id, request_key, reason, clock],
                )?;
                ((*claim_id).to_owned(), last.revision, clock)
            }
        };
        tx.execute(
            "INSERT INTO memory_write_receipts
             (client_id,request_key,operation_hash,operation_kind,claim_id,revision,recorded_at_ms)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![client_id, request_key, digest, kind, id, revision, clock],
        )?;
        tx.commit()?;
        Ok(MemoryWriteReceipt { client_id, claim_id: id, operation: kind.into(), revision, recorded_at_ms: clock })
    }
}

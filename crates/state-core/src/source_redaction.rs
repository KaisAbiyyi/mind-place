//! MP-9C source-level logical redaction, compatible with existing encrypted
//! schema v9. This is NOT physical erasure or independently monotonic backup
//! authority. Until every archive/index/backup surface is reconciled, the
//! privacy lifecycle is not complete and must not be advertised as such.
//!
//! Tombstones use the already encrypted state.settings JSON store to avoid an
//! automatic v9 -> v10 upgrade. Neither provider clients nor MCP expose these
//! trusted owner-only commands. Redaction is append-only through this API;
//! a future schema migration must make this invariant database-enforced.
use crate::{
    memory::next_claim_clock, source_record, valid_text, verify_source_record, Result, StateError,
    StateStore,
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const PREFIX: &str = "privacy:source-redaction:v1:";
const FORMAT: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Tombstone {
    version: u32,
    source_id: String,
    actor_id: String,
    reason_sha256: String,
    recorded_at_ms: i64,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct SourceRedactionReceipt {
    pub source_id: String,
    pub recorded_at_ms: i64,
}

fn valid_sha256(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn valid_source_id(id: &str) -> bool {
    id.len() == 71 && id.starts_with("src:v1:") && valid_sha256(&id[7..])
}

fn key(id: &str) -> Result<String> {
    if !valid_source_id(id) {
        return Err(StateError::InvalidValue);
    }
    Ok(format!("{PREFIX}{id}"))
}

/// Read and validate the entire durable entry, not just its presence.
/// Corrupted redaction policy MUST fail closed.
fn tombstone(db: &Connection, id: &str) -> Result<Option<Tombstone>> {
    let stored: Option<String> = db
        .query_row(
            "SELECT value_json FROM settings WHERE key=?1",
            [key(id)?],
            |row| row.get(0),
        )
        .optional()?;
    let Some(value) = stored else { return Ok(None) };
    let parsed: Tombstone = serde_json::from_str(&value).map_err(|_| StateError::Corrupt)?;
    if parsed.version != FORMAT
        || parsed.source_id != id
        || parsed.actor_id.is_empty()
        || !valid_sha256(&parsed.reason_sha256)
        || parsed.recorded_at_ms <= 0
    {
        return Err(StateError::Corrupt);
    }
    Ok(Some(parsed))
}

pub(crate) fn source_is_redacted(db: &Connection, id: &str) -> Result<bool> {
    Ok(tombstone(db, id)?.is_some())
}

pub(crate) fn assert_source_not_redacted(db: &Connection, id: &str) -> Result<()> {
    if source_is_redacted(db, id)? {
        return Err(StateError::MemoryDenied);
    }
    Ok(())
}

/// Conservative disclosure policy: if ANY evidence ever tied a claim to a
/// redacted source, conceal its statement and entire revision history.
/// A claim's other (shared) source records are not themselves deleted.
pub(crate) fn claim_has_redacted_source(db: &Connection, claim_id: &str) -> Result<bool> {
    let mut statement = db.prepare(
        "SELECT source_ref FROM claim_evidence WHERE claim_id=?1
         UNION SELECT source_ref FROM memory_claim_event_evidence WHERE claim_id=?1
         UNION SELECT source_ref FROM memory_claim_relations
           WHERE from_claim_id=?1 OR to_claim_id=?1
         UNION SELECT source_ref FROM memory_claim_relation_resolutions
           WHERE from_claim_id=?1 OR to_claim_id=?1",
    )?;
    let ids = statement.query_map([claim_id], |row| row.get::<_, String>(0))?;
    for id in ids {
        if source_is_redacted(db, &id?)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Insert the logical tombstone in an existing IMMEDIATE transaction while its
/// caller owns the exclusive privacy fence. This is deliberately not an erase
/// completion receipt; post-restore monotonic policy is a separate authority.
pub(crate) fn commit_redaction(
    db: &Connection,
    source_id: &str,
    actor_id: &str,
    reason_sha256: &str,
) -> Result<i64> {
    if !valid_source_id(source_id) || !valid_sha256(reason_sha256) || actor_id.is_empty() {
        return Err(StateError::InvalidValue);
    }
    if tombstone(db, source_id)?.is_some() {
        return Err(StateError::MemoryDenied);
    }
    let at = next_claim_clock(db, 0)?;
    let entry = Tombstone {
        version: FORMAT,
        source_id: source_id.to_owned(),
        actor_id: actor_id.to_owned(),
        reason_sha256: reason_sha256.to_owned(),
        recorded_at_ms: at,
    };
    db.execute(
        "INSERT INTO settings(key,value_json) VALUES(?1,?2)",
        params![
            key(source_id)?,
            serde_json::to_string(&entry).map_err(|_| StateError::Corrupt)?
        ],
    )?;
    Ok(at)
}

impl StateStore {
    /// Trusted local owner decision only. The caller MUST enforce an active
    /// unlocked vault lease and present the admin credential's verified hash.
    /// This action deliberately does NOT erase raw archives or backups.
    pub fn redact_source(
        &self,
        admin_token_sha256: &str,
        source_id: &str,
        reason: &str,
    ) -> Result<SourceRedactionReceipt> {
        if !valid_sha256(admin_token_sha256) || !valid_source_id(source_id) {
            return Err(StateError::InvalidValue);
        }
        valid_text(reason)?;
        if reason.len() > 4096 {
            return Err(StateError::InvalidValue);
        }
        // Lock order: privacy exclusive -> state connection's shared writer
        // fence -> SQLite IMMEDIATE. Capture takes the same privacy lock
        // before its visibility check and holds it through publication.
        // Plaintext legacy state (without a vault root) is not a vault writer.
        let parent = self.path.parent().ok_or(StateError::UnsafePath)?;
        let _privacy = if self.cipher_key.is_some() && parent.join("vault-v1.json").exists() {
            Some(
                mind_place_security::vault_fence::VaultWriterFence::privacy(parent)
                    .map_err(|_| StateError::Corrupt)?,
            )
        } else {
            None
        };
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let actor: String = tx
            .query_row(
                "SELECT c.id FROM clients c
             JOIN client_credentials k ON k.client_id=c.id
             JOIN client_capabilities p ON p.client_id=c.id
             WHERE k.token_sha256=?1 AND c.state='enabled'
               AND p.capability='admin' AND p.revoked_at IS NULL",
                [admin_token_sha256],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(StateError::MemoryDenied)?;
        let source = tx
            .query_row(
                "SELECT id,provider,source_namespace,source_kind,key_type,source_key
             FROM source_identities WHERE id=?1",
                [source_id],
                source_record,
            )
            .optional()?
            .ok_or(StateError::InvalidValue)?;
        verify_source_record(&source)?;
        if let Some(existing) = tombstone(&tx, source_id)? {
            tx.commit()?;
            return Ok(SourceRedactionReceipt {
                source_id: source_id.to_owned(),
                recorded_at_ms: existing.recorded_at_ms,
            });
        }
        let at = commit_redaction(
            &tx,
            source_id,
            &actor,
            &format!("{:x}", Sha256::digest(reason.as_bytes())),
        )?;
        tx.commit()?;
        Ok(SourceRedactionReceipt {
            source_id: source_id.to_owned(),
            recorded_at_ms: at,
        })
    }

    /// This is a tombstone check, NOT a physical-deletion confirmation.
    pub fn source_is_redacted(&self, source_id: &str) -> Result<bool> {
        source_is_redacted(&self.read_connection()?, source_id)
    }
}

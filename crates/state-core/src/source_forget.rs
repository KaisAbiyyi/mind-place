//! Trusted owner-only MP-9C one-time physical-forget intent protocol.
//!
//! An authorized, explicit preview can commit *logical* source redaction and
//! an immutable pending-erasure receipt. Until MP-9E independently reconciles
//! monotonic authority and retained backups, no physical-erasure completion
//! can be asserted or issued by this module. No MCP/renderer exposure.
use crate::{
    source_record, source_redaction, verify_source_record, Result, StateError, StateStore,
};
use mind_place_security::vault_fence::VaultWriterFence;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

const INTENT_PREFIX: &str = "privacy:forget-preview:v1:";
const PROGRESS_PREFIX: &str = "privacy:forget-progress:v1:";
const EXPIRY_MS: i64 = 10 * 60 * 1000;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct SourceForgetPreview {
    pub source_id: String,
    /// Show only through a trusted, unlocked owner session; never log or export.
    pub one_time_token: String,
    pub preview_sha256: String,
    pub affected_claims: usize,
    pub shared_claims: usize,
    pub expires_at_ms: i64,
    /// False: archive/object enumeration and backup disposition are mandatory.
    pub physical_inventory_complete: bool,
    pub retained_backup_review_required: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceForgetProgress {
    pub source_id: String,
    pub owner_client_id: String,
    pub confirmed_at_ms: i64,
    pub stage: String,
    /// This value is intentionally never true in the current implementation.
    pub physical_erase_verified: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    version: u32,
    source_id: String,
    owner_client_id: String,
    token_sha256: String,
    impact_sha256: String,
    reason_sha256: String,
    expires_at_ms: i64,
    unlock_epoch: u64,
}

fn unlock_epoch(store: &StateStore) -> u64 {
    store.cipher_key.as_ref().map_or(0, |key| key.lease_epoch())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sha(input: &[u8]) -> String {
    format!("{:x}", Sha256::digest(input))
}

fn source_id_valid(id: &str) -> bool {
    id.len() == 71
        && id.starts_with("src:v1:")
        && id[7..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn now_ms() -> Result<i64> {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| StateError::Corrupt)?
        .as_millis();
    i64::try_from(ms).map_err(|_| StateError::Corrupt)
}

fn owner(db: &Connection, token_sha256: &str) -> Result<String> {
    if token_sha256.len() != 64
        || !token_sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(StateError::MemoryDenied);
    }
    db.query_row(
        "SELECT c.id FROM clients c
         JOIN client_credentials k ON k.client_id=c.id
         JOIN client_capabilities p ON p.client_id=c.id
         WHERE k.token_sha256=?1 AND c.state='enabled'
           AND p.capability='admin' AND p.revoked_at IS NULL",
        [token_sha256],
        |row| row.get(0),
    )
    .optional()?
    .ok_or(StateError::MemoryDenied)
}

fn source_exists(db: &Connection, id: &str) -> Result<()> {
    if !source_id_valid(id) {
        return Err(StateError::InvalidValue);
    }
    let src = db
        .query_row(
            "SELECT id,provider,source_namespace,source_kind,key_type,source_key
         FROM source_identities WHERE id=?1",
            [id],
            source_record,
        )
        .optional()?
        .ok_or(StateError::InvalidValue)?;
    verify_source_record(&src)?;
    Ok(())
}

/// Includes relation endpoints and historical event evidence, not just the
/// present claim state. Any evidence clock change invalidates the preview.
fn impact(db: &Connection, id: &str, actor_id: &str) -> Result<(usize, usize, String)> {
    let mut stmt = db.prepare(
        "SELECT claim_id FROM claim_evidence WHERE source_ref=?1
         UNION SELECT claim_id FROM memory_claim_event_evidence WHERE source_ref=?1
         UNION SELECT from_claim_id FROM memory_claim_relations WHERE source_ref=?1
         UNION SELECT to_claim_id FROM memory_claim_relations WHERE source_ref=?1
         UNION SELECT from_claim_id FROM memory_claim_relation_resolutions WHERE source_ref=?1
         UNION SELECT to_claim_id FROM memory_claim_relation_resolutions WHERE source_ref=?1
         ORDER BY claim_id",
    )?;
    let ids = stmt
        .query_map([id], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut shared = 0usize;
    for claim in &ids {
        let has_other: i64 = db.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM claim_evidence WHERE claim_id=?1 AND source_ref<>?2
                 UNION SELECT 1 FROM memory_claim_event_evidence
                   WHERE claim_id=?1 AND source_ref<>?2
                 UNION SELECT 1 FROM memory_claim_relations
                   WHERE (from_claim_id=?1 OR to_claim_id=?1) AND source_ref<>?2
                 UNION SELECT 1 FROM memory_claim_relation_resolutions
                   WHERE (from_claim_id=?1 OR to_claim_id=?1) AND source_ref<>?2
             )",
            params![claim, id],
            |row| row.get(0),
        )?;
        if has_other != 0 {
            shared += 1;
        }
    }
    let clock: i64 = db.query_row(
        "SELECT last_ms FROM memory_claim_clock WHERE id=1",
        [],
        |row| row.get(0),
    )?;
    let mut bytes = format!(
        "mp9c-preview/v1\0{id}\0{actor_id}\0{clock}\0{}\0{shared}",
        ids.len()
    )
    .into_bytes();
    for claim in &ids {
        bytes.extend_from_slice(claim.as_bytes());
        bytes.push(0);
    }
    Ok((ids.len(), shared, sha(&bytes)))
}

fn fence(store: &StateStore) -> Result<Option<VaultWriterFence>> {
    let root = store.path.parent().ok_or(StateError::UnsafePath)?;
    if store.cipher_key.is_some() && root.join("vault-v1.json").exists() {
        Ok(Some(
            VaultWriterFence::privacy(root).map_err(|_| StateError::Corrupt)?,
        ))
    } else {
        Ok(None)
    }
}

impl StateStore {
    /// Prepare a one-use, ten-minute, owner-bound disclosure of current impact.
    /// Caller must additionally verify local unlocked-session owner intent;
    /// this Rust primitive is deliberately not exposed to clients/MCP.
    pub fn preview_source_forget(
        &self,
        admin_token_sha256: &str,
        source_id: &str,
        reason: &str,
    ) -> Result<SourceForgetPreview> {
        if reason.trim().is_empty() || reason.len() > 4096 {
            return Err(StateError::InvalidValue);
        }
        let _privacy = fence(self)?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let actor = owner(&tx, admin_token_sha256)?;
        source_exists(&tx, source_id)?;
        if source_redaction::source_is_redacted(&tx, source_id)? {
            return Err(StateError::MemoryDenied);
        }
        let (claims, shared, impact_sha256) = impact(&tx, source_id, &actor)?;
        let mut token = [0u8; 32];
        getrandom::fill(&mut token).map_err(|_| StateError::Corrupt)?;
        let one_time_token = hex(&token);
        let expires_at_ms = now_ms()?
            .checked_add(EXPIRY_MS)
            .ok_or(StateError::Corrupt)?;
        let record = Intent {
            version: 1,
            source_id: source_id.to_owned(),
            owner_client_id: actor,
            token_sha256: sha(one_time_token.as_bytes()),
            impact_sha256: impact_sha256.clone(),
            reason_sha256: sha(reason.as_bytes()),
            expires_at_ms,
            unlock_epoch: unlock_epoch(self),
        };
        tx.execute(
            "INSERT INTO settings(key,value_json) VALUES(?1,?2)
             ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json",
            params![
                format!("{INTENT_PREFIX}{source_id}"),
                serde_json::to_string(&record).map_err(|_| StateError::Corrupt)?
            ],
        )?;
        tx.commit()?;
        Ok(SourceForgetPreview {
            source_id: source_id.to_owned(),
            one_time_token,
            preview_sha256: impact_sha256,
            affected_claims: claims,
            shared_claims: shared,
            expires_at_ms,
            physical_inventory_complete: false,
            retained_backup_review_required: true,
        })
    }

    /// Atomically consumes the owner token, asserts unchanged evidence and
    /// commits a logical tombstone plus durable *pending* erasure progress.
    /// Never call this silently; UI must show the preview and seek explicit
    /// irreversible owner confirmation. No physical objects/backups are removed.
    pub fn confirm_source_forget(
        &self,
        admin_token_sha256: &str,
        source_id: &str,
        one_time_token: &str,
        preview_sha256: &str,
    ) -> Result<SourceForgetProgress> {
        if !source_id_valid(source_id)
            || one_time_token.len() != 64
            || one_time_token.bytes().any(|b| !b.is_ascii_hexdigit())
        {
            return Err(StateError::InvalidValue);
        }
        let _privacy = fence(self)?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let actor = owner(&tx, admin_token_sha256)?;
        source_exists(&tx, source_id)?;
        if source_redaction::source_is_redacted(&tx, source_id)? {
            return Err(StateError::MemoryDenied);
        }
        let intent_key = format!("{INTENT_PREFIX}{source_id}");
        let stored: String = tx
            .query_row(
                "SELECT value_json FROM settings WHERE key=?1",
                [&intent_key],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(StateError::MemoryDenied)?;
        let intent: Intent = serde_json::from_str(&stored).map_err(|_| StateError::Corrupt)?;
        if intent.version != 1
            || intent.source_id != source_id
            || intent.owner_client_id != actor
            || intent.token_sha256 != sha(one_time_token.as_bytes())
            || intent.expires_at_ms < now_ms()?
            || intent.unlock_epoch != unlock_epoch(self)
        {
            return Err(StateError::MemoryDenied);
        }
        let (_, _, actual) = impact(&tx, source_id, &actor)?;
        if actual != intent.impact_sha256 || actual != preview_sha256 {
            return Err(StateError::MemoryDenied);
        }
        let at = source_redaction::commit_redaction(&tx, source_id, &actor, &intent.reason_sha256)?;
        let progress = SourceForgetProgress {
            source_id: source_id.to_owned(),
            owner_client_id: actor,
            confirmed_at_ms: at,
            stage: "pending_independent_backup_reconciliation".to_owned(),
            physical_erase_verified: false,
        };
        tx.execute(
            "INSERT INTO settings(key,value_json) VALUES(?1,?2)",
            params![
                format!("{PROGRESS_PREFIX}{source_id}"),
                serde_json::to_string(&progress).map_err(|_| StateError::Corrupt)?
            ],
        )?;
        tx.execute("DELETE FROM settings WHERE key=?1", [&intent_key])?;
        tx.commit()?;
        Ok(progress)
    }

    /// Durable, intentionally non-sensitive status. Missing or malformed state
    /// is not treated as erased; admin credential is required even for reads.
    pub fn source_forget_progress(
        &self,
        admin_token_sha256: &str,
        source_id: &str,
    ) -> Result<Option<SourceForgetProgress>> {
        if !source_id_valid(source_id) {
            return Err(StateError::InvalidValue);
        }
        let db = self.read_connection()?;
        let actor = owner(&db, admin_token_sha256)?;
        let value: Option<String> = db
            .query_row(
                "SELECT value_json FROM settings WHERE key=?1",
                [format!("{PROGRESS_PREFIX}{source_id}")],
                |row| row.get(0),
            )
            .optional()?;
        let Some(value) = value else { return Ok(None) };
        let progress: SourceForgetProgress =
            serde_json::from_str(&value).map_err(|_| StateError::Corrupt)?;
        if progress.source_id != source_id
            || progress.owner_client_id != actor
            || progress.physical_erase_verified
            || progress.confirmed_at_ms <= 0
            || progress.stage != "pending_independent_backup_reconciliation"
            || !source_redaction::source_is_redacted(&db, source_id)?
        {
            return Err(StateError::Corrupt);
        }
        Ok(Some(progress))
    }
}

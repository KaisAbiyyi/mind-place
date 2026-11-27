//! MP-7MCP-02 / MP-ADR-006: per-client read grants over archive sources.
//!
//! A client with the global `read` capability can disclose NOTHING until the
//! trusted local owner grants it individual sources (default deny; there is no
//! wildcard, prefix or "all" grant). A grant is stored as one namespaced row
//! of `client_capabilities` (`read_source:<canonical source id>`), so adding it
//! needs no schema change: encrypted state cannot be migrated in place and a
//! version bump would stop existing encrypted vaults from opening. Rows with
//! this prefix never match the closed `read`/`write`/`admin` capability
//! queries, so they confer no other authority.
//!
//! Every grant change bumps the client's policy revision and the global
//! security policy revision (revocations also advance the client revocation
//! generation), so an in-flight read can detect and fail closed on any change.
//! Audit rows reuse the closed `capability_granted` / `capability_revoked`
//! events with capability `read` (the audit CHECK cannot be widened without a
//! schema change); they carry no source ids.
use crate::clients::{audit, bump_policy, valid_hash};
use crate::source_redaction::source_is_redacted;
use crate::{valid_id, ClientCapability, Result, StateError, StateStore};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

pub const MAX_READ_GRANTS_PER_CLIENT: usize = 10_000;
const MAX_SOURCES_PER_CALL: usize = 1_000;
const PREFIX: &str = "read_source:";

/// The authoritative, point-in-time read authority of one credential.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientReadScope {
    pub client_id: String,
    /// Changes on ANY policy mutation of this client (grant, revoke, disable,
    /// rotate, capability change); a reader compares it to fail closed.
    pub policy_revision: i64,
    /// Sorted, de-duplicated canonical source ids this client may read.
    pub sources: Vec<String>,
}

fn valid_source(id: &str) -> bool {
    id.len() == 71
        && id.starts_with("src:v1:")
        && id[7..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn normalized(sources: &[String]) -> Result<Vec<String>> {
    if sources.is_empty() || sources.len() > MAX_SOURCES_PER_CALL {
        return Err(StateError::InvalidValue);
    }
    let mut list: Vec<String> = sources.to_vec();
    if !list.iter().all(|id| valid_source(id)) {
        return Err(StateError::InvalidValue);
    }
    list.sort();
    list.dedup();
    Ok(list)
}

impl StateStore {
    /// Trusted unlocked owner code only (like `set_client_capability`). Returns
    /// how many grants became newly active; re-granting is a no-op. A redacted
    /// source can never be granted. Revoked clients cannot be granted anything.
    pub fn grant_client_read_sources(&self, client_id: &str, sources: &[String]) -> Result<usize> {
        valid_id(client_id)?;
        let list = normalized(sources)?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let state: Option<String> = tx
            .query_row("SELECT state FROM clients WHERE id=?1", [client_id], |r| {
                r.get(0)
            })
            .optional()?;
        if !matches!(state.as_deref(), Some("enabled" | "disabled")) {
            return Err(StateError::InvalidValue);
        }
        let mut active: i64 = tx.query_row(
            "SELECT count(*) FROM client_capabilities
             WHERE client_id=?1 AND substr(capability,1,?2)=?3 AND revoked_at IS NULL",
            params![client_id, PREFIX.len() as i64, PREFIX],
            |r| r.get(0),
        )?;
        let mut changed = 0usize;
        for id in &list {
            if source_is_redacted(&tx, id)? {
                return Err(StateError::InvalidValue);
            }
            let capability = format!("{PREFIX}{id}");
            let was_active: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM client_capabilities
                 WHERE client_id=?1 AND capability=?2 AND revoked_at IS NULL)",
                params![client_id, capability],
                |r| r.get(0),
            )?;
            if was_active {
                continue;
            }
            active += 1;
            if active as usize > MAX_READ_GRANTS_PER_CLIENT {
                return Err(StateError::InvalidValue);
            }
            tx.execute(
                "INSERT INTO client_capabilities(client_id,capability,revoked_at)
                 VALUES (?1,?2,NULL) ON CONFLICT(client_id,capability)
                 DO UPDATE SET revoked_at=NULL,granted_at=unixepoch()",
                params![client_id, capability],
            )?;
            changed += 1;
        }
        if changed > 0 {
            tx.execute(
                "UPDATE clients SET policy_revision=policy_revision+1 WHERE id=?1",
                [client_id],
            )?;
            let policy = bump_policy(&tx, false)?;
            audit(
                &tx,
                Some(client_id),
                "capability_granted",
                Some(ClientCapability::Read),
                policy,
            )?;
        }
        tx.commit()?;
        Ok(changed)
    }

    /// Revocation takes effect for the very next authorization check and
    /// fences in-flight reads (client revocation generation advances).
    pub fn revoke_client_read_sources(&self, client_id: &str, sources: &[String]) -> Result<usize> {
        valid_id(client_id)?;
        let list = normalized(sources)?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM clients WHERE id=?1)",
            [client_id],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(StateError::InvalidValue);
        }
        let mut changed = 0usize;
        for id in &list {
            changed += tx.execute(
                "UPDATE client_capabilities SET revoked_at=unixepoch()
                 WHERE client_id=?1 AND capability=?2 AND revoked_at IS NULL",
                params![client_id, format!("{PREFIX}{id}")],
            )?;
        }
        if changed > 0 {
            tx.execute(
                "UPDATE clients SET policy_revision=policy_revision+1 WHERE id=?1",
                [client_id],
            )?;
            let policy = bump_policy(&tx, true)?;
            audit(
                &tx,
                Some(client_id),
                "capability_revoked",
                Some(ClientCapability::Read),
                policy,
            )?;
        }
        tx.commit()?;
        Ok(changed)
    }

    /// Revoke every source grant of a client in one transaction.
    pub fn revoke_all_client_read_sources(&self, client_id: &str) -> Result<usize> {
        valid_id(client_id)?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM clients WHERE id=?1)",
            [client_id],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(StateError::InvalidValue);
        }
        let changed = tx.execute(
            "UPDATE client_capabilities SET revoked_at=unixepoch()
             WHERE client_id=?1 AND substr(capability,1,?2)=?3 AND revoked_at IS NULL",
            params![client_id, PREFIX.len() as i64, PREFIX],
        )?;
        if changed > 0 {
            tx.execute(
                "UPDATE clients SET policy_revision=policy_revision+1 WHERE id=?1",
                [client_id],
            )?;
            let policy = bump_policy(&tx, true)?;
            audit(
                &tx,
                Some(client_id),
                "capability_revoked",
                Some(ClientCapability::Read),
                policy,
            )?;
        }
        tx.commit()?;
        Ok(changed)
    }

    /// Owner inventory of one client's active source grants (sorted, bounded).
    pub fn client_read_sources(&self, client_id: &str) -> Result<Vec<String>> {
        valid_id(client_id)?;
        let db = self.read_connection()?;
        read_sources(&db, client_id)
    }

    /// One consistent read transaction: the credential must be an enabled
    /// client holding the active global `read` capability; the result carries
    /// the client's policy revision and its active source grants. Never cached
    /// by callers beyond one request; a missing grant set is an empty scope.
    pub fn authorize_client_read_scope(
        &self,
        token_sha256: &str,
    ) -> Result<Option<ClientReadScope>> {
        valid_hash(token_sha256)?;
        let mut db = self.read_connection()?;
        let tx = db.transaction()?;
        let found: Option<(String, i64)> = tx
            .query_row(
                "SELECT c.id,c.policy_revision FROM client_credentials t
                 JOIN clients c ON c.id=t.client_id
                 JOIN client_capabilities p ON p.client_id=c.id
                 WHERE t.token_sha256=?1 AND c.state='enabled'
                   AND p.capability='read' AND p.revoked_at IS NULL",
                [token_sha256],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((client_id, policy_revision)) = found else {
            return Ok(None);
        };
        let sources = read_sources(&tx, &client_id)?;
        Ok(Some(ClientReadScope {
            client_id,
            policy_revision,
            sources,
        }))
    }

    /// Cheap fence used between read steps: `None` when the client no longer
    /// exists. Compare with `ClientReadScope::policy_revision`.
    pub fn client_policy_revision(&self, client_id: &str) -> Result<Option<i64>> {
        valid_id(client_id)?;
        self.read_connection()?
            .query_row(
                "SELECT policy_revision FROM clients WHERE id=?1",
                [client_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(StateError::from)
    }
}

/// MP-SEC-11 follow-up: whether `client_id` holds effective read authority
/// over `source_id` right now: an active global `read` capability AND an
/// active per-source grant (the same rule as `authorize_client_read_scope`).
/// Runs on the caller's connection so a writer can check it inside its own
/// IMMEDIATE transaction. A malformed id is simply "not granted". The caller
/// is responsible for the client's `enabled` state.
pub(crate) fn client_can_read_source(
    db: &Connection,
    client_id: &str,
    source_id: &str,
) -> Result<bool> {
    if !valid_source(source_id) {
        return Ok(false);
    }
    db.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM client_capabilities g
           JOIN client_capabilities r ON r.client_id=g.client_id
           WHERE g.client_id=?1 AND g.capability=?2 AND g.revoked_at IS NULL
             AND r.capability='read' AND r.revoked_at IS NULL
         )",
        params![client_id, format!("{PREFIX}{source_id}")],
        |r| r.get(0),
    )
    .map_err(StateError::from)
}

fn read_sources(db: &rusqlite::Connection, client_id: &str) -> Result<Vec<String>> {
    let mut statement = db.prepare(
        "SELECT substr(capability,?2+1) FROM client_capabilities
         WHERE client_id=?1 AND substr(capability,1,?2)=?3 AND revoked_at IS NULL
         ORDER BY capability LIMIT ?4",
    )?;
    let rows = statement.query_map(
        params![
            client_id,
            PREFIX.len() as i64,
            PREFIX,
            (MAX_READ_GRANTS_PER_CLIENT + 1) as i64
        ],
        |r| r.get::<_, String>(0),
    )?;
    let sources = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    // Fail closed on a malformed or oversized set instead of truncating it.
    if sources.len() > MAX_READ_GRANTS_PER_CLIENT || !sources.iter().all(|id| valid_source(id)) {
        return Err(StateError::Corrupt);
    }
    Ok(sources)
}

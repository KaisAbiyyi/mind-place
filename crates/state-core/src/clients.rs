//! Phase 3B authoritative client identity, capability and audit operations.
//! Never accept client claims about their identity: resolve it from the
//! cryptographically random credential's hash on *every* guarded read.
use crate::{valid_id, valid_text, Result, StateError, StateStore};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientCapability {
    Read,
    Write,
    Admin,
}
impl ClientCapability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Admin => "admin",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ClientRecord {
    pub id: String,
    pub display_name: String,
    pub state: String,
    pub policy_revision: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ClientAuditEvent {
    pub seq: i64,
    pub client_id: Option<String>,
    pub event: String,
    pub capability: Option<String>,
    pub policy_revision: i64,
    pub recorded_at: i64,
}

fn valid_hash(s: &str) -> Result<()> {
    if s.len() != 64
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(StateError::InvalidValue);
    }
    Ok(())
}

fn client_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ClientRecord> {
    Ok(ClientRecord {
        id: row.get(0)?,
        display_name: row.get(1)?,
        state: row.get(2)?,
        policy_revision: row.get(3)?,
    })
}
fn bump_policy(tx: &rusqlite::Transaction<'_>, revoke_generation: bool) -> Result<i64> {
    tx.execute(
        "UPDATE security_policy SET revision=revision+1,
         client_revocation_generation=client_revocation_generation+?1 WHERE id=1",
        [i64::from(revoke_generation)],
    )?;
    tx.query_row("SELECT revision FROM security_policy WHERE id=1", [], |r| {
        r.get(0)
    })
    .map_err(StateError::from)
}
fn audit(
    tx: &rusqlite::Transaction<'_>,
    client_id: Option<&str>,
    event: &str,
    capability: Option<ClientCapability>,
    policy_revision: i64,
) -> Result<()> {
    tx.execute(
        "INSERT INTO client_audit(client_id,event,capability,policy_revision)
         VALUES (?1,?2,?3,?4)",
        params![
            client_id,
            event,
            capability.map(ClientCapability::as_str),
            policy_revision
        ],
    )?;
    Ok(())
}

impl StateStore {
    /// Only trusted, unlocked local owner code may call this mutation.
    /// It accepts a hash, never the bearer token, and requires explicit grants.
    pub fn issue_client(
        &self,
        id: &str,
        display_name: &str,
        token_sha256: &str,
        capabilities: &[ClientCapability],
    ) -> Result<ClientRecord> {
        valid_id(id)?;
        valid_text(display_name)?;
        valid_hash(token_sha256)?;
        if !id.starts_with("client:v1:")
            || display_name.len() > 128
            || capabilities.is_empty()
            || capabilities.len() > 3
            || capabilities
                .iter()
                .enumerate()
                .any(|(i, cap)| capabilities[..i].contains(cap))
        {
            return Err(StateError::InvalidValue);
        }
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO clients(id,display_name,state,policy_revision)
             VALUES (?1,?2,'enabled',1)",
            params![id, display_name.trim()],
        )?;
        tx.execute(
            "INSERT INTO client_credentials(client_id,token_sha256) VALUES (?1,?2)",
            params![id, token_sha256],
        )?;
        for cap in capabilities {
            tx.execute(
                "INSERT INTO client_capabilities(client_id,capability) VALUES (?1,?2)",
                params![id, cap.as_str()],
            )?;
        }
        let policy = bump_policy(&tx, false)?;
        audit(&tx, Some(id), "issued", None, policy)?;
        tx.commit()?;
        Ok(ClientRecord {
            id: id.into(),
            display_name: display_name.trim().into(),
            state: "enabled".into(),
            policy_revision: 1,
        })
    }

    /// A lookup NEVER returns evidence if the client is disabled/revoked, its
    /// credential was rotated or the exact capability is not active.
    /// Called at each Phase 3A authorization callback, never cached.
    pub fn authorize_client_hash(
        &self,
        token_sha256: &str,
        capability: ClientCapability,
    ) -> Result<Option<String>> {
        valid_hash(token_sha256)?;
        self.connection()?
            .query_row(
                "SELECT c.id FROM client_credentials t
                 JOIN clients c ON c.id=t.client_id
                 JOIN client_capabilities p ON p.client_id=c.id
                 WHERE t.token_sha256=?1 AND c.state='enabled'
                   AND p.capability=?2 AND p.revoked_at IS NULL",
                params![token_sha256, capability.as_str()],
                |r| r.get(0),
            )
            .optional()
            .map_err(StateError::from)
    }

    pub fn client(&self, id: &str) -> Result<Option<ClientRecord>> {
        valid_id(id)?;
        self.connection()?
            .query_row(
                "SELECT id,display_name,state,policy_revision FROM clients WHERE id=?1",
                [id],
                client_row,
            )
            .optional()
            .map_err(StateError::from)
    }

    pub fn set_client_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        valid_id(id)?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let state = if enabled { "enabled" } else { "disabled" };
        if tx.execute(
            "UPDATE clients SET state=?2,policy_revision=policy_revision+1
             WHERE id=?1 AND state IN ('enabled','disabled') AND state!=?2",
            params![id, state],
        )? != 1
        {
            return Err(StateError::InvalidValue);
        }
        let policy = bump_policy(&tx, !enabled)?;
        audit(
            &tx,
            Some(id),
            if enabled { "enabled" } else { "disabled" },
            None,
            policy,
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Revocation is terminal. Never restore this client ID or its credential.
    pub fn revoke_client(&self, id: &str) -> Result<()> {
        valid_id(id)?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if tx.execute(
            "UPDATE clients SET state='revoked',revoked_at=unixepoch(),
             policy_revision=policy_revision+1 WHERE id=?1 AND state!='revoked'",
            [id],
        )? != 1
        {
            return Err(StateError::InvalidValue);
        }
        let policy = bump_policy(&tx, true)?;
        audit(&tx, Some(id), "revoked", None, policy)?;
        tx.commit()?;
        Ok(())
    }

    /// Credential rotation atomically invalidates the old token.
    pub fn rotate_client_hash(&self, id: &str, replacement_hash: &str) -> Result<()> {
        valid_id(id)?;
        valid_hash(replacement_hash)?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if tx.execute(
            "UPDATE client_credentials SET token_sha256=?2,revision=revision+1,
             rotated_at=unixepoch() WHERE client_id=?1 AND EXISTS
             (SELECT 1 FROM clients WHERE id=?1 AND state='enabled')",
            params![id, replacement_hash],
        )? != 1
        {
            return Err(StateError::InvalidValue);
        }
        tx.execute(
            "UPDATE clients SET policy_revision=policy_revision+1 WHERE id=?1",
            [id],
        )?;
        let policy = bump_policy(&tx, true)?;
        audit(&tx, Some(id), "rotated", None, policy)?;
        tx.commit()?;
        Ok(())
    }

    /// Capability changes require a trusted owner; read clients cannot call
    /// this from the shared-brain dispatcher.
    pub fn set_client_capability(
        &self,
        id: &str,
        cap: ClientCapability,
        granted: bool,
    ) -> Result<()> {
        valid_id(id)?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let state: Option<String> = tx
            .query_row("SELECT state FROM clients WHERE id=?1", [id], |r| r.get(0))
            .optional()?;
        if !matches!(state.as_deref(), Some("enabled" | "disabled")) {
            return Err(StateError::InvalidValue);
        }
        let changed = if granted {
            tx.execute(
                "INSERT INTO client_capabilities(client_id,capability,revoked_at)
                 VALUES (?1,?2,NULL) ON CONFLICT(client_id,capability)
                 DO UPDATE SET revoked_at=NULL WHERE revoked_at IS NOT NULL",
                params![id, cap.as_str()],
            )?
        } else {
            tx.execute(
                "UPDATE client_capabilities SET revoked_at=unixepoch()
                 WHERE client_id=?1 AND capability=?2 AND revoked_at IS NULL",
                params![id, cap.as_str()],
            )?
        };
        if changed != 1 {
            return Err(StateError::InvalidValue);
        }
        tx.execute(
            "UPDATE clients SET policy_revision=policy_revision+1 WHERE id=?1",
            [id],
        )?;
        let policy = bump_policy(&tx, !granted)?;
        audit(
            &tx,
            Some(id),
            if granted {
                "capability_granted"
            } else {
                "capability_revoked"
            },
            Some(cap),
            policy,
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Metadata-only request audit. This is not an access grant. Locked-process
    /// requests never reach state I/O and thus produce no local audit row.
    pub fn audit_client_read(&self, client_id: Option<&str>, event: &str) -> Result<()> {
        if !matches!(
            event,
            "read_attempt" | "read_completed" | "read_denied" | "read_failed"
        ) {
            return Err(StateError::InvalidValue);
        }
        if let Some(id) = client_id {
            valid_id(id)?;
        }
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let revision =
            tx.query_row("SELECT revision FROM security_policy WHERE id=1", [], |r| {
                r.get(0)
            })?;
        audit(
            &tx,
            client_id,
            event,
            Some(ClientCapability::Read),
            revision,
        )?;
        tx.commit()?;
        Ok(())
    }

    /// For trusted local owner inspection only; no credential material in rows.
    pub fn client_audit(&self, id: &str, max_events: usize) -> Result<Vec<ClientAuditEvent>> {
        valid_id(id)?;
        if !(1..=200).contains(&max_events) {
            return Err(StateError::InvalidValue);
        }
        let db = self.connection()?;
        let mut stmt = db.prepare(
            "SELECT seq,client_id,event,capability,policy_revision,recorded_at
             FROM client_audit WHERE client_id=?1 ORDER BY seq DESC LIMIT ?2",
        )?;
        let events = stmt.query_map(params![id, max_events as i64], |r| {
            Ok(ClientAuditEvent {
                seq: r.get(0)?,
                client_id: r.get(1)?,
                event: r.get(2)?,
                capability: r.get(3)?,
                policy_revision: r.get(4)?,
                recorded_at: r.get(5)?,
            })
        })?;
        events
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(StateError::from)
    }
}

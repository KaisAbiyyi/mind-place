//! MP-9C: private-scope authority. No MCP/Tauri exposure: the owner must
//! authenticate in the trusted unlocked Rust path and supply a verified token hash.
//! A scope label is not itself an access grant to an untrusted model client.
use crate::{valid_id, valid_text, Result, StateError, StateStore};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::Serialize;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct PrivateScope {
    pub id: String,
    pub name: String,
    pub parent_scope_id: Option<String>,
}

fn valid_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn scope_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PrivateScope> {
    Ok(PrivateScope {
        id: row.get(0)?,
        name: row.get(1)?,
        parent_scope_id: row.get(2)?,
    })
}

/// Recheck the precise unrevoked admin credential inside the same transaction
/// that creates/reads scope metadata. Never authorize from a client-supplied ID.
fn require_owner_admin(db: &rusqlite::Connection, hash: &str) -> Result<()> {
    if !valid_hash(hash) {
        return Err(StateError::InvalidValue);
    }
    let authorized: Option<String> = db
        .query_row(
            "SELECT c.id FROM client_credentials k
             JOIN clients c ON c.id=k.client_id
             JOIN client_capabilities p ON p.client_id=c.id
             WHERE k.token_sha256=?1 AND c.state='enabled'
               AND p.capability='admin' AND p.revoked_at IS NULL",
            [hash],
            |row| row.get(0),
        )
        .optional()?;
    authorized.ok_or(StateError::MemoryDenied)?;
    Ok(())
}

impl StateStore {
    /// Create a named, private memory scope. Never reparent or overwrite an
    /// existing scope; callers must confirm owner intent in the unlocked UI.
    /// This is a trusted Rust-only primitive, NOT a provider permission API.
    pub fn create_private_scope(
        &self,
        admin_token_sha256: &str,
        scope_id: &str,
        name: &str,
        parent_scope_id: Option<&str>,
    ) -> Result<PrivateScope> {
        valid_id(scope_id)?;
        valid_text(name)?;
        if !scope_id.starts_with("scope:") || name.trim().len() > 128 || name.trim().is_empty() {
            return Err(StateError::InvalidValue);
        }
        if let Some(parent) = parent_scope_id {
            valid_id(parent)?;
            if parent == scope_id {
                return Err(StateError::InvalidValue);
            }
        }
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_owner_admin(&tx, admin_token_sha256)?;
        if let Some(parent) = parent_scope_id {
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM scopes WHERE id=?1)",
                [parent],
                |row| row.get(0),
            )?;
            if !exists {
                return Err(StateError::InvalidValue);
            }
        }
        tx.execute(
            "INSERT INTO scopes(id,name,parent_scope_id) VALUES(?1,?2,?3)",
            params![scope_id, name.trim(), parent_scope_id],
        )?;
        tx.commit()?;
        Ok(PrivateScope {
            id: scope_id.to_owned(),
            name: name.trim().to_owned(),
            parent_scope_id: parent_scope_id.map(str::to_owned),
        })
    }

    /// Bounded, authenticated owner inventory. Returned scope IDs/labels do not
    /// grant any read/write authority to model clients.
    pub fn list_private_scopes(
        &self,
        admin_token_sha256: &str,
        after_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<PrivateScope>> {
        if !(1..=50).contains(&limit) {
            return Err(StateError::InvalidValue);
        }
        if let Some(after) = after_id {
            valid_id(after)?;
        }
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_owner_admin(&tx, admin_token_sha256)?;
        let result = {
            let mut stmt = tx.prepare(
                "SELECT id,name,parent_scope_id FROM scopes
                 WHERE id>?1 ORDER BY id LIMIT ?2",
            )?;
            stmt.query_map(params![after_id.unwrap_or(""), limit as i64], scope_row)?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        tx.commit()?;
        Ok(result)
    }
}

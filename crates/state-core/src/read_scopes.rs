//! MP-7MCP-02 per-client / per-collection read grants.
//!
//! A *collection* is an owner-defined, explicit closed set of canonical
//! `src:v1:` source IDs. A client reads exactly the union of the collections
//! granted to it. Nothing is inferred: a client without a grant, a grant that
//! names a deleted collection, or an empty collection authorizes NOTHING
//! (deny by default). Capability `Read` alone is not authority over any source.
//!
//! Storage deliberately reuses the existing authoritative `settings` table
//! under reserved keys, so no state schema bump is needed (an encrypted vault
//! state cannot migrate in place without separately consented staging).
//! Every mutation runs in one IMMEDIATE transaction and bumps
//! `security_policy.revision`; in-flight readers compare that revision and
//! abort when it moves. Grant/revoke is attributed in the append-only client
//! audit as `capability_granted` / `capability_revoked` (read), which is the
//! closest existing event; no source IDs, names or tokens enter the audit.
use crate::clients::{audit, bump_policy};
use crate::{valid_id, valid_text, ClientCapability, Result, StateError, StateStore};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_COLLECTIONS: usize = 64;
pub const MAX_SOURCES_PER_COLLECTION: usize = 2048;
pub const MAX_COLLECTIONS_PER_CLIENT: usize = 16;
pub const COLLECTION_ID_PREFIX: &str = "collection:v1:";

const COLLECTION_KEY: &str = "mcp.collection.v1:";
const GRANT_KEY: &str = "mcp.grants.v1:";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredCollection {
    name: String,
    sources: Vec<String>,
}
#[derive(Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct StoredGrants {
    collections: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CollectionRecord {
    pub id: String,
    pub name: String,
    pub source_count: usize,
}

/// Authoritative, per-request resolution of what one credential may read.
/// `policy_revision` fences in-flight work: any later change to any grant,
/// collection, client or credential moves it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadScope {
    pub client_id: String,
    pub policy_revision: i64,
    pub sources: BTreeSet<String>,
}

fn valid_collection_id(id: &str) -> Result<()> {
    let hex = id
        .strip_prefix(COLLECTION_ID_PREFIX)
        .ok_or(StateError::InvalidValue)?;
    if hex.len() != 32
        || !hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(StateError::InvalidValue);
    }
    Ok(())
}
fn valid_source_id(id: &str) -> bool {
    id.len() == 71
        && id.starts_with("src:v1:")
        && id[7..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn normalize_sources(sources: &[String]) -> Result<Vec<String>> {
    if sources.len() > MAX_SOURCES_PER_COLLECTION || !sources.iter().all(|s| valid_source_id(s)) {
        return Err(StateError::InvalidValue);
    }
    let set: BTreeSet<&String> = sources.iter().collect();
    Ok(set.into_iter().cloned().collect())
}
fn read_json<T: for<'de> Deserialize<'de>>(
    tx: &rusqlite::Transaction<'_>,
    key: &str,
) -> Result<Option<T>> {
    let raw: Option<String> = tx
        .query_row("SELECT value_json FROM settings WHERE key=?1", [key], |r| {
            r.get(0)
        })
        .optional()?;
    // A damaged record is authority corruption: fail closed, never widen.
    raw.map(|value| serde_json::from_str(&value).map_err(|_| StateError::Corrupt))
        .transpose()
}
fn write_json<T: Serialize>(tx: &rusqlite::Transaction<'_>, key: &str, value: &T) -> Result<()> {
    let json = serde_json::to_string(value).map_err(|_| StateError::InvalidValue)?;
    tx.execute(
        "INSERT INTO settings(key,value_json) VALUES(?1,?2)
         ON CONFLICT(key) DO UPDATE SET value_json=?2,updated_at=unixepoch()",
        params![key, json],
    )?;
    Ok(())
}
fn collection_count(tx: &rusqlite::Transaction<'_>) -> Result<usize> {
    let n: i64 = tx.query_row(
        "SELECT COUNT(*) FROM settings WHERE key GLOB 'mcp.collection.v1:*'",
        [],
        |r| r.get(0),
    )?;
    Ok(n as usize)
}
fn require_client(tx: &rusqlite::Transaction<'_>, client_id: &str) -> Result<()> {
    let state: Option<String> = tx
        .query_row("SELECT state FROM clients WHERE id=?1", [client_id], |r| {
            r.get(0)
        })
        .optional()?;
    if matches!(state.as_deref(), Some("enabled" | "disabled")) {
        Ok(())
    } else {
        Err(StateError::InvalidValue)
    }
}

impl StateStore {
    /// Trusted owner only. Creating a collection grants nothing to anyone.
    pub fn create_collection(&self, id: &str, name: &str, sources: &[String]) -> Result<()> {
        valid_collection_id(id)?;
        valid_text(name)?;
        if name.len() > 128 {
            return Err(StateError::InvalidValue);
        }
        let sources = normalize_sources(sources)?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let key = format!("{COLLECTION_KEY}{id}");
        if read_json::<StoredCollection>(&tx, &key)?.is_some()
            || collection_count(&tx)? >= MAX_COLLECTIONS
        {
            return Err(StateError::InvalidValue);
        }
        write_json(
            &tx,
            &key,
            &StoredCollection {
                name: name.trim().into(),
                sources,
            },
        )?;
        bump_policy(&tx, false)?;
        tx.commit()?;
        Ok(())
    }

    /// Replaces membership. Shrinking membership is a revocation event.
    pub fn set_collection_sources(&self, id: &str, sources: &[String]) -> Result<()> {
        valid_collection_id(id)?;
        let sources = normalize_sources(sources)?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let key = format!("{COLLECTION_KEY}{id}");
        let mut stored: StoredCollection = read_json(&tx, &key)?.ok_or(StateError::InvalidValue)?;
        let shrinks = stored.sources.iter().any(|s| !sources.contains(s));
        stored.sources = sources;
        write_json(&tx, &key, &stored)?;
        bump_policy(&tx, shrinks)?;
        tx.commit()?;
        Ok(())
    }

    /// Deletes the collection and removes it from every client's grants.
    pub fn delete_collection(&self, id: &str) -> Result<()> {
        valid_collection_id(id)?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let key = format!("{COLLECTION_KEY}{id}");
        if tx.execute("DELETE FROM settings WHERE key=?1", [&key])? != 1 {
            return Err(StateError::InvalidValue);
        }
        let grant_keys: Vec<String> = {
            let mut stmt =
                tx.prepare("SELECT key FROM settings WHERE key GLOB 'mcp.grants.v1:*'")?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            rows.collect::<std::result::Result<_, _>>()?
        };
        for grant_key in grant_keys {
            if let Some(mut grants) = read_json::<StoredGrants>(&tx, &grant_key)? {
                if grants.collections.iter().any(|c| c == id) {
                    grants.collections.retain(|c| c != id);
                    write_json(&tx, &grant_key, &grants)?;
                }
            }
        }
        bump_policy(&tx, true)?;
        tx.commit()?;
        Ok(())
    }

    pub fn grant_collection(&self, client_id: &str, collection_id: &str) -> Result<()> {
        valid_id(client_id)?;
        valid_collection_id(collection_id)?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_client(&tx, client_id)?;
        if read_json::<StoredCollection>(&tx, &format!("{COLLECTION_KEY}{collection_id}"))?
            .is_none()
        {
            return Err(StateError::InvalidValue);
        }
        let key = format!("{GRANT_KEY}{client_id}");
        let mut grants: StoredGrants = read_json(&tx, &key)?.unwrap_or_default();
        if grants.collections.iter().any(|c| c == collection_id)
            || grants.collections.len() >= MAX_COLLECTIONS_PER_CLIENT
        {
            return Err(StateError::InvalidValue);
        }
        grants.collections.push(collection_id.into());
        grants.collections.sort();
        write_json(&tx, &key, &grants)?;
        let policy = bump_policy(&tx, false)?;
        audit(
            &tx,
            Some(client_id),
            "capability_granted",
            Some(ClientCapability::Read),
            policy,
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn revoke_collection_grant(&self, client_id: &str, collection_id: &str) -> Result<()> {
        valid_id(client_id)?;
        valid_collection_id(collection_id)?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let key = format!("{GRANT_KEY}{client_id}");
        let mut grants: StoredGrants = read_json(&tx, &key)?.ok_or(StateError::InvalidValue)?;
        let before = grants.collections.len();
        grants.collections.retain(|c| c != collection_id);
        if grants.collections.len() == before {
            return Err(StateError::InvalidValue);
        }
        write_json(&tx, &key, &grants)?;
        let policy = bump_policy(&tx, true)?;
        audit(
            &tx,
            Some(client_id),
            "capability_revoked",
            Some(ClientCapability::Read),
            policy,
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Owner inspection; membership IDs are not returned, only counts.
    pub fn list_collections(&self) -> Result<Vec<CollectionRecord>> {
        let db = self.connection()?;
        let mut stmt = db.prepare(
            "SELECT key,value_json FROM settings WHERE key GLOB 'mcp.collection.v1:*' ORDER BY key",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (key, value) = row?;
            let stored: StoredCollection =
                serde_json::from_str(&value).map_err(|_| StateError::Corrupt)?;
            out.push(CollectionRecord {
                id: key[COLLECTION_KEY.len()..].to_owned(),
                name: stored.name,
                source_count: stored.sources.len(),
            });
        }
        Ok(out)
    }

    pub fn client_collections(&self, client_id: &str) -> Result<Vec<String>> {
        valid_id(client_id)?;
        let db = self.connection()?;
        let raw: Option<String> = db
            .query_row(
                "SELECT value_json FROM settings WHERE key=?1",
                [format!("{GRANT_KEY}{client_id}")],
                |r| r.get(0),
            )
            .optional()?;
        match raw {
            None => Ok(Vec::new()),
            Some(value) => Ok(serde_json::from_str::<StoredGrants>(&value)
                .map_err(|_| StateError::Corrupt)?
                .collections),
        }
    }

    /// Current global policy revision (moves on every authority change).
    pub fn policy_revision(&self) -> Result<i64> {
        self.connection()?
            .query_row("SELECT revision FROM security_policy WHERE id=1", [], |r| {
                r.get(0)
            })
            .map_err(StateError::from)
    }

    /// Atomic, uncached authorization + scope resolution for one credential.
    /// `None` when the credential is unknown, rotated, disabled, revoked or
    /// lacks an active Read capability. An authorized client with no grants
    /// resolves to an EMPTY source set (it can read nothing).
    pub fn client_read_scope(&self, token_sha256: &str) -> Result<Option<ReadScope>> {
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Deferred)?;
        let client: Option<String> = tx
            .query_row(
                "SELECT c.id FROM client_credentials t
                 JOIN clients c ON c.id=t.client_id
                 JOIN client_capabilities p ON p.client_id=c.id
                 WHERE t.token_sha256=?1 AND c.state='enabled'
                   AND p.capability='read' AND p.revoked_at IS NULL",
                [token_sha256],
                |r| r.get(0),
            )
            .optional()?;
        let Some(client_id) = client else {
            return Ok(None);
        };
        let policy_revision: i64 =
            tx.query_row("SELECT revision FROM security_policy WHERE id=1", [], |r| {
                r.get(0)
            })?;
        let grants: StoredGrants =
            read_json(&tx, &format!("{GRANT_KEY}{client_id}"))?.unwrap_or_default();
        let mut sources = BTreeSet::new();
        for collection in grants.collections.iter().take(MAX_COLLECTIONS_PER_CLIENT) {
            if let Some(stored) =
                read_json::<StoredCollection>(&tx, &format!("{COLLECTION_KEY}{collection}"))?
            {
                sources.extend(stored.sources);
            }
        }
        Ok(Some(ReadScope {
            client_id,
            policy_revision,
            sources,
        }))
    }
}

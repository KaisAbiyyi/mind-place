//! MP-SEC-11: the only memory-write entry point for MODEL-CLIENT credentials.
//!
//! Invariants enforced here, inside the same IMMEDIATE transaction as the write:
//! 1. `asserted_by` is always `model`: the proposal type has no such field, and
//!    JSON carrying one is rejected (`deny_unknown_fields`).
//! 2. The claim is always created `uncertain`; a model path never produces an
//!    accepted (`active`) claim and has no transition/supersede/forget operation.
//!    Promotion is the owner's trusted `transition_claim` path.
//! 3. Self-reported epistemic origin is limited to `inferred`/`summarized`; a
//!    model cannot claim `explicit`/`observed` to gain ranking authority.
//! 4. Scope is confined: cloud tier may write only into its own quarantine scope;
//!    local tier only unscoped or its own quarantine scope. A scope label is never
//!    a model permission (MP-9C).
//!
//! The tier is supplied by the trusted transport from the credential class, never
//! from request content. No production caller exists yet (MP-7MCP-03/05).
use crate::{
    memory_writes::{MemoryWrite, MemoryWriteReceipt},
    private_scopes::{require_owner_admin, PrivateScope},
    valid_id, ClaimEvidence, MemoryClaim, Result, StateError, StateStore,
};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

/// Credential class of an MCP client (MP-7MCP-01 section 2). Cloud: reached
/// through the hosted tunnel. Local: loopback/local-pipe credential.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialTier {
    Local,
    Cloud,
}

pub const QUARANTINE_SCOPE_PREFIX: &str = "scope:quarantine:";
const CLIENT_PREFIX: &str = "client:v1:";

/// The per-client quarantine scope id. One client owns exactly one.
pub fn quarantine_scope_id(client_id: &str) -> Result<String> {
    valid_id(client_id)?;
    let suffix = client_id
        .strip_prefix(CLIENT_PREFIX)
        .filter(|s| !s.is_empty() && s.len() <= 200)
        .ok_or(StateError::InvalidValue)?;
    Ok(format!("{QUARANTINE_SCOPE_PREFIX}{suffix}"))
}

fn default_origin() -> String {
    "inferred".into()
}

/// What a model client may propose. Fields such as `asserted_by`, `status`,
/// `confidence` or validity windows do not exist and are rejected on parse.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModelClaimProposal {
    pub id: String,
    pub statement: String,
    #[serde(default = "default_origin")]
    pub epistemic_origin: String,
    #[serde(default)]
    pub scope_id: Option<String>,
    pub evidence: Vec<ClaimEvidence>,
}

/// Re-checked inside the write transaction against the AUTHENTICATED client id.
pub(crate) fn enforce_model_write(
    db: &rusqlite::Connection,
    client_id: &str,
    tier: CredentialTier,
    operation: &MemoryWrite<'_>,
) -> Result<()> {
    // Model credentials may only create. Superseding or forgetting would let a
    // model change the standing of an owner-authored claim.
    let MemoryWrite::Create {
        claim, scope_id, ..
    } = operation
    else {
        return Err(StateError::MemoryDenied);
    };
    if claim.asserted_by != "model" || claim.status != "uncertain" {
        return Err(StateError::MemoryDenied);
    }
    let own = quarantine_scope_id(client_id).map_err(|_| StateError::MemoryDenied)?;
    match (tier, scope_id) {
        (CredentialTier::Cloud, Some(scope)) if *scope == own => {}
        (CredentialTier::Local, None) => return Ok(()),
        (CredentialTier::Local, Some(scope)) if *scope == own => {}
        // Cloud without scope, or any foreign/ordinary scope: opaque denial.
        _ => return Err(StateError::MemoryDenied),
    }
    let provisioned: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM scopes WHERE id=?1)",
        [&own],
        |r| r.get(0),
    )?;
    if !provisioned {
        return Err(StateError::MemoryDenied);
    }
    Ok(())
}

impl StateStore {
    /// Owner-only: create the client's quarantine scope (idempotent). Cloud
    /// credentials cannot write until the owner has provisioned it.
    pub fn provision_quarantine_scope(
        &self,
        admin_token_sha256: &str,
        client_id: &str,
    ) -> Result<PrivateScope> {
        let scope_id = quarantine_scope_id(client_id)?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_owner_admin(&tx, admin_token_sha256)?;
        let known: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM clients WHERE id=?1)",
            [client_id],
            |r| r.get(0),
        )?;
        if !known {
            return Err(StateError::InvalidValue);
        }
        let name = format!("Quarantine for {client_id}");
        let existing = tx
            .query_row(
                "SELECT id,name,parent_scope_id FROM scopes WHERE id=?1",
                [&scope_id],
                |r| {
                    Ok(PrivateScope {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        parent_scope_id: r.get(2)?,
                    })
                },
            )
            .optional()?;
        if let Some(scope) = existing {
            tx.commit()?;
            return Ok(scope);
        }
        tx.execute(
            "INSERT INTO scopes(id,name,parent_scope_id) VALUES(?1,?2,NULL)",
            params![scope_id, name],
        )?;
        tx.commit()?;
        Ok(PrivateScope {
            id: scope_id,
            name,
            parent_scope_id: None,
        })
    }

    /// Stage a model-authored memory proposal. The credential's Write capability
    /// is rechecked in the write transaction. The result is an `uncertain`,
    /// `asserted_by=model` candidate; nothing here can mark it accepted.
    pub fn apply_model_memory_write(
        &self,
        token_sha256: &str,
        request_key: &str,
        tier: CredentialTier,
        proposal: &ModelClaimProposal,
    ) -> Result<MemoryWriteReceipt> {
        if !matches!(
            proposal.epistemic_origin.as_str(),
            "inferred" | "summarized"
        ) {
            return Err(StateError::InvalidValue);
        }
        let claim = MemoryClaim {
            id: proposal.id.clone(),
            statement: proposal.statement.clone(),
            asserted_by: "model".into(),
            epistemic_origin: proposal.epistemic_origin.clone(),
            status: "uncertain".into(),
        };
        let operation = MemoryWrite::Create {
            claim: &claim,
            scope_id: proposal.scope_id.as_deref(),
            evidence: &proposal.evidence,
        };
        self.apply_memory_write_inner(token_sha256, request_key, &operation, Some(tier))
    }
}

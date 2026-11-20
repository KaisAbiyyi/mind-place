//! Phase 3B trusted-owner provisioning and credential-gated reads.
//! No network transport; never expose owner methods to external clients.
use crate::{IntegrationCore, IntegrationError, Result, ToolBatch, ToolBatchResponse};
use mind_place_security::SecurityGate;
use mind_place_state::{ClientCapability, ClientRecord, StateStore};
use serde_json::Value;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub const CREDENTIAL_PREFIX: &str = "mpc_v1_";

/// Secret is intentionally not Debug/Serialize/Clone.
pub struct IssuedClient {
    pub client: ClientRecord,
    credential: Zeroizing<String>,
}
impl IssuedClient {
    pub fn credential(&self) -> &str {
        self.credential.as_str()
    }
}

fn new_credential() -> Result<Zeroizing<String>> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| IntegrationError::CredentialGeneration)?;
    let credential = format!(
        "{CREDENTIAL_PREFIX}{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    );
    bytes.fill(0);
    Ok(Zeroizing::new(credential))
}
fn credential_hash(token: &str) -> Result<String> {
    if token.len() != CREDENTIAL_PREFIX.len() + 64
        || !token.starts_with(CREDENTIAL_PREFIX)
        || !token[CREDENTIAL_PREFIX.len()..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(IntegrationError::AccessDenied);
    }
    Ok(format!("{:x}", Sha256::digest(token.as_bytes())))
}

/// Local owner-only methods; future MCP/HTTP handlers must authenticate
/// separately and must NEVER dispatch these based on a read credential.
pub struct ClientAuthority<'a> {
    state: &'a StateStore,
    security: &'a SecurityGate,
}
impl<'a> ClientAuthority<'a> {
    pub fn new(state: &'a StateStore, security: &'a SecurityGate) -> Self {
        Self { state, security }
    }
    pub fn issue(&self, name: &str, capabilities: &[ClientCapability]) -> Result<IssuedClient> {
        let lease = self.security.require_unlocked()?;
        let credential = new_credential()?;
        let hash = credential_hash(&credential)?;
        let id = format!("client:v1:{}", &hash[..48]);
        let client = self.state.issue_client(&id, name, &hash, capabilities)?;
        self.security.validate_lease(lease)?;
        Ok(IssuedClient { client, credential })
    }
    pub fn rotate(&self, id: &str) -> Result<IssuedClient> {
        let lease = self.security.require_unlocked()?;
        let credential = new_credential()?;
        let hash = credential_hash(&credential)?;
        self.state.rotate_client_hash(id, &hash)?;
        let client = self
            .state
            .client(id)?
            .ok_or(IntegrationError::AccessDenied)?;
        self.security.validate_lease(lease)?;
        Ok(IssuedClient { client, credential })
    }
    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        let lease = self.security.require_unlocked()?;
        self.state.set_client_enabled(id, enabled)?;
        self.security.validate_lease(lease)?;
        Ok(())
    }
    pub fn revoke(&self, id: &str) -> Result<()> {
        let lease = self.security.require_unlocked()?;
        self.state.revoke_client(id)?;
        self.security.validate_lease(lease)?;
        Ok(())
    }
    pub fn set_capability(
        &self,
        id: &str,
        capability: ClientCapability,
        grant: bool,
    ) -> Result<()> {
        let lease = self.security.require_unlocked()?;
        self.state.set_client_capability(id, capability, grant)?;
        self.security.validate_lease(lease)?;
        Ok(())
    }
    /// MP-7MCP-02: grant a client read access to individual archive sources
    /// (default deny, no wildcard). Owner-only, requires a live unlock lease.
    pub fn grant_read_sources(&self, id: &str, sources: &[String]) -> Result<usize> {
        let lease = self.security.require_unlocked()?;
        let changed = self.state.grant_client_read_sources(id, sources)?;
        self.security.validate_lease(lease)?;
        Ok(changed)
    }
    pub fn revoke_read_sources(&self, id: &str, sources: &[String]) -> Result<usize> {
        let lease = self.security.require_unlocked()?;
        let changed = self.state.revoke_client_read_sources(id, sources)?;
        self.security.validate_lease(lease)?;
        Ok(changed)
    }
    pub fn revoke_all_read_sources(&self, id: &str) -> Result<usize> {
        let lease = self.security.require_unlocked()?;
        let changed = self.state.revoke_all_client_read_sources(id)?;
        self.security.validate_lease(lease)?;
        Ok(changed)
    }
    pub fn read_sources(&self, id: &str) -> Result<Vec<String>> {
        let lease = self.security.require_unlocked()?;
        let sources = self.state.client_read_sources(id)?;
        self.security.validate_lease(lease)?;
        Ok(sources)
    }
    pub fn authorize(&self, token: &str, capability: ClientCapability) -> Result<String> {
        let lease = self.security.require_unlocked()?;
        let hash = credential_hash(token)?;
        let client = self
            .state
            .authorize_client_hash(&hash, capability)?
            .ok_or(IntegrationError::AccessDenied)?;
        self.security.validate_lease(lease)?;
        Ok(client)
    }
}

/// Visit every source reference in a request or result tree. Fails closed on a
/// malformed reference. Shared by the redaction and the read-scope guards.
fn for_each_source_ref(value: &Value, mut visit: impl FnMut(&str) -> Result<()>) -> Result<()> {
    let mut pending = vec![value];
    let mut visited = 0usize;
    while let Some(node) = pending.pop() {
        visited += 1;
        if visited > 20_000 {
            return Err(IntegrationError::ResourceLimit);
        }
        match node {
            Value::Object(map) => {
                for (name, value) in map {
                    if matches!(name.as_str(), "source_id" | "source_ref") {
                        visit(value.as_str().ok_or(IntegrationError::AccessDenied)?)?;
                    } else if name == "source_ids" {
                        let ids = value.as_array().ok_or(IntegrationError::AccessDenied)?;
                        for source in ids {
                            visit(source.as_str().ok_or(IntegrationError::AccessDenied)?)?;
                        }
                    } else {
                        pending.push(value);
                    }
                }
            }
            Value::Array(values) => pending.extend(values),
            _ => {}
        }
    }
    Ok(())
}

/* Strict post-result privacy guard for the credentialed MCP dispatcher.
A stale FTS generation may still contain pre-redaction snippets; never
serialize those snippets to a model even if source indexing has not caught
up. Fail closed on malformed source references. */
fn reject_redacted_sources(state: &StateStore, value: &Value) -> Result<()> {
    for_each_source_ref(value, |source| {
        if state.source_is_redacted(source)? {
            return Err(IntegrationError::AccessDenied);
        }
        Ok(())
    })
}

/// MP-7MCP-02 defense in depth: no result may name a source outside the
/// client's read grants, even if an upstream filter were bypassed.
fn reject_out_of_scope_results(scope: &[String], value: &Value) -> Result<()> {
    for_each_source_ref(value, |source| {
        if scope.binary_search(&source.to_owned()).is_err() {
            return Err(IntegrationError::AccessDenied);
        }
        Ok(())
    })
}

impl IntegrationCore<'_> {
    /// Read requests use the SAME unlock lease as Phase 3A, and current
    /// authoritative client capability is checked at every callback.
    /// No credentials, queries or source contents enter the audit.
    pub fn execute_client_batch(
        &self,
        state: &StateStore,
        credential: &str,
        batch: &ToolBatch,
    ) -> Result<ToolBatchResponse> {
        self.execute_client_batch_observed(state, credential, batch, &mut || {})
    }

    /// Test seam (not for production callers): `observe` runs at the start of
    /// every live-authority recheck, so a test can mutate grants or the client
    /// mid-request and prove the batch fails closed.
    #[doc(hidden)]
    pub fn execute_client_batch_observed(
        &self,
        state: &StateStore,
        credential: &str,
        batch: &ToolBatch,
        observe: &mut dyn FnMut(),
    ) -> Result<ToolBatchResponse> {
        let observe = std::cell::RefCell::new(observe);
        // MP-9C: one shared privacy disclosure fence spans the argument
        // pre-check, every tool call, the post-result authority check and the
        // returned response. A redaction commit (exclusive) therefore either
        // completes before this batch's first visibility decision or waits
        // until its response exists. Acquired before any state connection.
        let _privacy = self.disclosure_fence()?;
        self.security.require_unlocked()?;
        let hash = credential_hash(credential)?;
        // One consistent snapshot of the credential, its policy revision and
        // its read grants. A credential with no grants reads nothing (default
        // deny) but is otherwise a normal, authenticated client.
        let scope = match state.authorize_client_read_scope(&hash)? {
            Some(scope) => scope,
            None => {
                state.audit_client_read(None, "read_denied")?;
                return Err(IntegrationError::AccessDenied);
            }
        };
        let client = scope.client_id.clone();
        let revision = scope.policy_revision;
        let mut granted = scope.sources;
        granted.sort();
        granted.dedup();
        state.audit_client_read(Some(&client), "read_attempt")?;
        // Check direct source filters before accessing any raw/index bytes.
        for call in &batch.calls {
            reject_redacted_sources(state, &call.arguments)?;
        }
        // Every authorization callback re-reads live authority: the credential
        // must still resolve to this client AND the client's policy revision
        // must be unchanged, so a grant, revoke, disable, rotation or capability
        // change during the request fails the whole batch closed.
        let authorize = || {
            (observe.borrow_mut())();
            if state
                .authorize_client_hash(&hash, ClientCapability::Read)?
                .as_deref()
                == Some(client.as_str())
                && state.client_policy_revision(&client)? == Some(revision)
            {
                Ok(())
            } else {
                Err(IntegrationError::AccessDenied)
            }
        };
        // Stale index hits for redacted sources are dropped (not rejected) so
        // the response is indistinguishable from "no match".
        // MP-9C: hidden sources are excluded from ranking by failing closed
        // on a stale projection; prune it and retry once so the client sees
        // only visible results and no stale-generation signal.
        let run = || {
            self.execute_batch_scoped(
                batch,
                authorize,
                &mut mind_place_index::AuthorityVisibility(state),
                Some(&granted),
            )
        };
        let result = match run() {
            Err(IntegrationError::Index(mind_place_index::IndexError::StaleGeneration)) => self
                .prune_stale_projection(state, authorize)
                .and_then(|()| run()),
            other => other,
        };
        // Defense in depth: recheck every returned source against fresh
        // authority and the read grants. Anything still naming a redacted or
        // ungranted source fails closed.
        let result = result.and_then(|response| {
            for item in &response.results {
                reject_redacted_sources(state, &item.result)?;
                reject_out_of_scope_results(&granted, &item.result)?;
            }
            authorize()?;
            Ok(response)
        });
        state.audit_client_read(
            Some(&client),
            if result.is_ok() {
                "read_completed"
            } else {
                "read_failed"
            },
        )?;
        result
    }
}

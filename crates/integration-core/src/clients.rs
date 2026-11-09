//! Phase 3B trusted-owner provisioning and credential-gated reads.
//! No network transport; never expose owner methods to external clients.
use crate::{IntegrationCore, IntegrationError, Result, ToolBatch, ToolBatchResponse};
use mind_place_index::SourceScope;
use mind_place_security::SecurityGate;
use mind_place_state::{ClientCapability, ClientRecord, StateStore};
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
    /// Owner-only collection management. A collection grants nothing by itself.
    pub fn create_collection(&self, name: &str, source_ids: &[String]) -> Result<String> {
        let lease = self.security.require_unlocked()?;
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|_| IntegrationError::CredentialGeneration)?;
        let id = format!(
            "{}{}",
            mind_place_state::COLLECTION_ID_PREFIX,
            bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
        );
        self.state.create_collection(&id, name, source_ids)?;
        self.security.validate_lease(lease)?;
        Ok(id)
    }
    pub fn set_collection_sources(&self, id: &str, source_ids: &[String]) -> Result<()> {
        let lease = self.security.require_unlocked()?;
        self.state.set_collection_sources(id, source_ids)?;
        self.security.validate_lease(lease)?;
        Ok(())
    }
    pub fn delete_collection(&self, id: &str) -> Result<()> {
        let lease = self.security.require_unlocked()?;
        self.state.delete_collection(id)?;
        self.security.validate_lease(lease)?;
        Ok(())
    }
    pub fn grant_collection(&self, client_id: &str, collection_id: &str) -> Result<()> {
        let lease = self.security.require_unlocked()?;
        self.state.grant_collection(client_id, collection_id)?;
        self.security.validate_lease(lease)?;
        Ok(())
    }
    pub fn revoke_collection_grant(&self, client_id: &str, collection_id: &str) -> Result<()> {
        let lease = self.security.require_unlocked()?;
        self.state
            .revoke_collection_grant(client_id, collection_id)?;
        self.security.validate_lease(lease)?;
        Ok(())
    }
    pub fn list_collections(&self) -> Result<Vec<mind_place_state::CollectionRecord>> {
        let lease = self.security.require_unlocked()?;
        let collections = self.state.list_collections()?;
        self.security.validate_lease(lease)?;
        Ok(collections)
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
        self.security.require_unlocked()?;
        let hash = credential_hash(credential)?;
        // Authorization and collection scope are resolved together from the
        // authoritative state for THIS request; nothing is cached across
        // requests, connections, cursors or unlock epochs.
        let granted = match state.client_read_scope(&hash)? {
            Some(scope) => scope,
            None => {
                state.audit_client_read(None, "read_denied")?;
                return Err(IntegrationError::AccessDenied);
            }
        };
        let client = granted.client_id.clone();
        let fence = granted.policy_revision;
        let scope = SourceScope::new(granted.sources);
        state.audit_client_read(Some(&client), "read_attempt")?;
        let result = self.execute_batch_scoped(
            batch,
            Some(&scope),
            read_fence(state, &hash, &client, fence),
        );
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

/// Per-check authorization for one resolved scope. The capability must still
/// be current AND no grant, collection, credential or client change may have
/// happened since the scope was resolved. A moved policy revision fails closed
/// (the client retries and re-resolves); in-flight work never outlives a
/// revocation, redaction-by-removal or rotation.
fn read_fence<'a>(
    state: &'a StateStore,
    hash: &'a str,
    client: &'a str,
    fence: i64,
) -> impl FnMut() -> Result<()> + 'a {
    move || {
        if state
            .authorize_client_hash(hash, ClientCapability::Read)?
            .as_deref()
            == Some(client)
            && state.policy_revision()? == fence
        {
            Ok(())
        } else {
            Err(IntegrationError::AccessDenied)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ToolCall, ToolName};
    use mind_place_archive::{ArchiveStore, Capture, Message};
    use mind_place_index::IndexStore;
    use serde_json::json;

    /// In-flight revocation: the fence moves between source reads of ONE batch
    /// and the whole batch (including already-fetched evidence) is discarded.
    #[test]
    fn grant_revocation_during_a_batch_discards_every_result() {
        let dir = tempfile::tempdir().unwrap();
        let archive = ArchiveStore::open(dir.path().join("archive-v1")).unwrap();
        let state = StateStore::open(dir.path().join("state.sqlite3")).unwrap();
        let index = IndexStore::open(dir.path().join("index.sqlite3")).unwrap();
        let conversation = "11111111-2222-4333-8444-555555555555";
        archive
            .save(Capture {
                schema_version: 1,
                provider: "chatgpt".into(),
                conversation_id: conversation.into(),
                source_url: format!("https://chatgpt.com/c/{conversation}"),
                title: "Fence".into(),
                conversation_created_at: None,
                conversation_updated_at: None,
                captured_at: "2026-11-09T10:00:00Z".into(),
                capture_method: "manual-dom".into(),
                completeness: "partial".into(),
                warnings: vec![],
                messages: vec![Message {
                    id: "m1".into(),
                    role: "user".into(),
                    text: "tessellated horizon".into(),
                    format: "plain".into(),
                    authored_at: None,
                    model: None,
                    attachments: vec![],
                }],
            })
            .unwrap();
        index.sync(&archive, &state).unwrap();
        let security = SecurityGate::open(dir.path().join("security.json")).unwrap();
        security.initialize(b"fence-test-secret-1").unwrap();
        let core = IntegrationCore::new(&archive, &index, &security);
        let batch = ToolBatch {
            calls: vec![
                ToolCall {
                    name: ToolName::MemorySearch,
                    arguments: json!({"queries": ["tessellated horizon"]}),
                };
                2
            ],
        };
        let sources: Vec<String> = core.execute_batch(&batch, || Ok(())).unwrap().results[0].result
            ["hits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| h["source_id"].as_str().unwrap().to_owned())
            .collect();
        let owner = ClientAuthority::new(&state, &security);
        let issued = owner.issue("Reader", &[ClientCapability::Read]).unwrap();
        let collection = owner.create_collection("Fence", &sources).unwrap();
        owner
            .grant_collection(&issued.client.id, &collection)
            .unwrap();
        let hash = credential_hash(issued.credential()).unwrap();
        let granted = state.client_read_scope(&hash).unwrap().unwrap();
        assert_eq!(granted.sources.len(), 1);
        let scope = SourceScope::new(granted.sources.clone());
        let mut checks = 0;
        let mut fence = read_fence(&state, &hash, &issued.client.id, granted.policy_revision);
        let result = core.execute_batch_scoped(&batch, Some(&scope), || {
            checks += 1;
            if checks == 4 {
                owner
                    .revoke_collection_grant(&issued.client.id, &collection)
                    .unwrap();
            }
            fence()
        });
        assert!(matches!(result, Err(IntegrationError::AccessDenied)));
        assert!(checks >= 4);
        // A fresh request re-resolves and now sees an empty scope.
        let fresh = state.client_read_scope(&hash).unwrap().unwrap();
        assert!(fresh.sources.is_empty());
        assert!(fresh.policy_revision > granted.policy_revision);
    }
}

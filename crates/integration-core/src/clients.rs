//! Phase 3B trusted-owner provisioning and credential-gated reads.
//! No network transport; never expose owner methods to external clients.
use crate::{IntegrationCore, IntegrationError, Result, ToolBatch, ToolBatchResponse};
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
        let client = match state.authorize_client_hash(&hash, ClientCapability::Read)? {
            Some(id) => id,
            None => {
                state.audit_client_read(None, "read_denied")?;
                return Err(IntegrationError::AccessDenied);
            }
        };
        state.audit_client_read(Some(&client), "read_attempt")?;
        let result = self.execute_batch(batch, || {
            if state
                .authorize_client_hash(&hash, ClientCapability::Read)?
                .as_deref()
                == Some(client.as_str())
            {
                Ok(())
            } else {
                Err(IntegrationError::AccessDenied)
            }
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

//! Transport-neutral, read-only shared-brain tool dispatcher.
//!
//! This crate does not start a listener or mint client credentials. Phase 3B
//! must provide an authorization callback bound to the calling client; Phase
//! 3C adapts this contract to local MCP. Never expose this service directly
//! from a network route without that authorization layer.
#![forbid(unsafe_code)]

use mind_place_archive::ArchiveStore;
use mind_place_index::{ContextRequest, FetchRequest, IndexError, IndexStore, RetrievalRequest};
use mind_place_security::{SecurityError, SecurityGate, UnlockLease};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

pub mod clients;
pub mod mcp;
pub mod transport;
pub use clients::{ClientAuthority, IssuedClient, CREDENTIAL_PREFIX};

pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_BATCH_CALLS: usize = 8;
pub const MAX_REQUEST_BYTES: usize = 64 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 512 * 1024;

pub type Result<T> = std::result::Result<T, IntegrationError>;

#[derive(Debug, Error)]
pub enum IntegrationError {
    #[error("Mind Place read access was denied or revoked")]
    AccessDenied,
    #[error("invalid shared-brain request")]
    InvalidRequest,
    #[error("invalid tool arguments")]
    InvalidArguments,
    #[error("shared-brain resource limit exceeded")]
    ResourceLimit,
    #[error("Mind Place is locked or its unlock epoch changed")]
    Locked,
    #[error("index operation failed: {0}")]
    Index(#[from] IndexError),
    #[error("response serialization failed")]
    Serialization,
    #[error("Mind Place client authority is unavailable")]
    Authority(#[from] mind_place_state::StateError),
    #[error("OS credential generation failed")]
    CredentialGeneration,
}

impl IntegrationError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::AccessDenied => "MP_INTEGRATION_DENIED",
            Self::InvalidRequest => "MP_INTEGRATION_INVALID_REQUEST",
            Self::InvalidArguments => "MP_INTEGRATION_INVALID_ARGUMENTS",
            Self::ResourceLimit => "MP_INTEGRATION_LIMIT",
            Self::Locked => "MP_INTEGRATION_LOCKED",
            Self::Index(IndexError::Revoked) => "MP_INTEGRATION_REVOKED",
            Self::Index(_) => "MP_INTEGRATION_INDEX",
            Self::Serialization => "MP_INTEGRATION_SERIALIZATION",
            Self::Authority(_) => "MP_INTEGRATION_AUTHORITY",
            Self::CredentialGeneration => "MP_INTEGRATION_CREDENTIAL_GENERATION",
        }
    }
}

impl From<SecurityError> for IntegrationError {
    fn from(_: SecurityError) -> Self {
        Self::Locked
    }
}

/// A protocol adapter maps its wire tool names to this closed enumeration.
/// No write, admin, graph, or arbitrary filesystem operations exist in 3A.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolName {
    MemorySearch,
    MemoryFetch,
    ContextBuild,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCall {
    pub name: ToolName,
    pub arguments: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolBatch {
    pub calls: Vec<ToolCall>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ToolResponse {
    pub name: ToolName,
    pub result: Value,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ToolBatchResponse {
    pub schema_version: u32,
    pub results: Vec<ToolResponse>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ToolDefinition {
    pub name: ToolName,
    pub description: &'static str,
    pub input_schema: Value,
}

/// Machine-readable discovery contract for MCP and future protocol adapters.
/// Schema advertises supported inputs; serde/backend validation is authoritative.
pub fn tool_catalog() -> Vec<ToolDefinition> {
    let query = json!({
        "oneOf": [
            {"type": "string", "minLength": 1, "maxLength": 1024},
            {
                "type": "object",
                "required": ["text"],
                "additionalProperties": false,
                "properties": {
                    "text": {"type": "string", "minLength": 1, "maxLength": 1024},
                    "weight": {"type": "number", "exclusiveMinimum": 0, "maximum": 10},
                    "mode": {"enum": ["lexical", "exact"]}
                }
            }
        ]
    });
    let filters = json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "include_history": {"type": "boolean"},
            "providers": {"type": "array", "items": {"type": "string"}, "maxItems": 32},
            "models": {"type": "array", "items": {"type": "string"}, "maxItems": 32},
            "roles": {"type": "array", "items": {"type": "string"}, "maxItems": 32},
            "kinds": {"type": "array", "items": {"type": "string"}, "maxItems": 32},
            "source_ids": {"type": "array", "items": {"type": "string"}, "maxItems": 32},
            "archive_ids": {"type": "array", "items": {"type": "string"}, "maxItems": 32},
            "conversation_ids": {"type": "array", "items": {"type": "string"}, "maxItems": 32},
            "message_ids": {"type": "array", "items": {"type": "string"}, "maxItems": 32},
            "captured_from": {"type": ["string", "null"], "format": "date-time"},
            "captured_to": {"type": ["string", "null"], "format": "date-time"}
        }
    });
    let budget = json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "max_bytes": {"type": "integer", "minimum": 4},
            "max_estimated_tokens": {"type": "integer", "minimum": 1},
            "max_latency_ms": {"type": "integer", "minimum": 1}
        }
    });
    vec![
        ToolDefinition {
            name: ToolName::MemorySearch,
            description: "Search indexed evidence with stable source/snapshot/message references and bounded snippets. Lexical ranking is not a confidence estimate.",
            input_schema: json!({
                "type": "object",
                "required": ["queries"],
                "additionalProperties": false,
                "properties": {
                    "queries": {"type": "array", "items": query, "minItems": 1, "maxItems": 8},
                    "filters": filters,
                    "limit": {"type": "integer", "minimum": 1, "maximum": 100}
                }
            }),
        },
        ToolDefinition {
            name: ToolName::MemoryFetch,
            description: "Fetch exact immutable source revision evidence by the stable identifiers from search; supports bounded windows and resumable cursors.",
            input_schema: json!({
                "type": "object",
                "required": ["source_id", "archive_id", "snapshot_id"],
                "additionalProperties": false,
                "properties": {
                    "source_id": {"type": "string"},
                    "archive_id": {"type": "string"},
                    "snapshot_id": {"type": "string"},
                    "granularity": {"enum": ["message", "window", "revision"]},
                    "message_id": {"type": ["string", "null"]},
                    "before": {"type": "integer", "minimum": 0, "maximum": 20},
                    "after": {"type": "integer", "minimum": 0, "maximum": 20},
                    "budget": budget,
                    "cursor": {
                        "oneOf": [
                            {"type": "null"},
                            {
                                "type": "object",
                                "required": ["message_index", "byte_offset"],
                                "additionalProperties": false,
                                "properties": {
                                    "message_index": {"type": "integer", "minimum": 0},
                                    "byte_offset": {"type": "integer", "minimum": 0}
                                }
                            }
                        ]
                    }
                }
            }),
        },
        ToolDefinition {
            name: ToolName::ContextBuild,
            description: "Build a source-diverse, provenance-backed and byte/token-bounded context pack from search results.",
            input_schema: json!({
                "type": "object",
                "required": ["queries"],
                "additionalProperties": false,
                "properties": {
                    "queries": {"type": "array", "items": query, "minItems": 1, "maxItems": 8},
                    "filters": filters,
                    "preset": {"enum": ["fast", "balanced", "source_first", "challenge", "historical"]},
                    "as_of": {"type": ["string", "null"], "format": "date-time"},
                    "max_sources": {"type": "integer", "minimum": 1, "maximum": 12},
                    "max_excerpts": {"type": "integer", "minimum": 1, "maximum": 40},
                    "max_per_source": {"type": "integer", "minimum": 1, "maximum": 6},
                    "budget": budget
                }
            }),
        },
    ]
}

/// Borrowed archive choice; the vault variant is never permitted to fall back
/// to a legacy on-disk ArchiveStore under any circumstances.
enum ArchiveSource<'a> {
    Legacy(&'a ArchiveStore),
    #[cfg(feature = "vault-mcp")]
    Vault(&'a mind_place_index::vault_archive::VaultCaptureStore),
}

/// This core intentionally borrows existing stores. It neither starts an HTTP
/// server nor owns an implicit always-authorized client identity.
pub struct IntegrationCore<'a> {
    archive: ArchiveSource<'a>,
    index: &'a IndexStore,
    security: &'a SecurityGate,
}

impl<'a> IntegrationCore<'a> {
    pub fn new(
        archive: &'a ArchiveStore,
        index: &'a IndexStore,
        security: &'a SecurityGate,
    ) -> Self {
        Self {
            archive: ArchiveSource::Legacy(archive),
            index,
            security,
        }
    }

    #[cfg(feature = "vault-mcp")]
    pub fn new_vault(
        archive: &'a mind_place_index::vault_archive::VaultCaptureStore,
        index: &'a IndexStore,
        security: &'a SecurityGate,
    ) -> Self {
        Self {
            archive: ArchiveSource::Vault(archive),
            index,
            security,
        }
    }

    /// Authorization MUST be supplied by the trusted Phase 3B caller and
    /// rechecked for every operation, between source reads and before the
    /// complete response is returned. Failure discards the entire batch.
    /// The callback must check the client's current read capability, not a
    /// cached allow decision made at connection establishment.
    pub fn execute_batch(
        &self,
        batch: &ToolBatch,
        mut authorize_read: impl FnMut() -> Result<()>,
    ) -> Result<ToolBatchResponse> {
        if batch.calls.is_empty() || batch.calls.len() > MAX_BATCH_CALLS {
            return Err(IntegrationError::InvalidRequest);
        }
        let encoded = serde_json::to_vec(batch).map_err(|_| IntegrationError::Serialization)?;
        if encoded.len() > MAX_REQUEST_BYTES {
            return Err(IntegrationError::ResourceLimit);
        }
        let lease = self.security.require_unlocked()?;
        self.check(lease, &mut authorize_read)?;
        let mut results = Vec::with_capacity(batch.calls.len());
        for call in &batch.calls {
            self.check(lease, &mut authorize_read)?;
            if !call.arguments.is_object() {
                return Err(IntegrationError::InvalidArguments);
            }
            let result = match call.name {
                ToolName::MemorySearch => {
                    let request: RetrievalRequest = serde_json::from_value(call.arguments.clone())
                        .map_err(|_| IntegrationError::InvalidArguments)?;
                    serde_json::to_value(self.index.retrieve(&request)?)
                        .map_err(|_| IntegrationError::Serialization)?
                }
                ToolName::MemoryFetch => {
                    let request: FetchRequest = serde_json::from_value(call.arguments.clone())
                        .map_err(|_| IntegrationError::InvalidArguments)?;
                    let mut check = || self.check_index(lease, &mut authorize_read);
                    let fetched = match self.archive {
                        ArchiveSource::Legacy(archive) =>
                            self.index.fetch_with_check(archive, &request, &mut check)?,
                        #[cfg(feature = "vault-mcp")]
                        ArchiveSource::Vault(archive) =>
                            self.index.fetch_vault_with_check(archive, &request, &mut check)?,
                    };
                    serde_json::to_value(fetched)
                        .map_err(|_| IntegrationError::Serialization)?
                }
                ToolName::ContextBuild => {
                    let request: ContextRequest = serde_json::from_value(call.arguments.clone())
                        .map_err(|_| IntegrationError::InvalidArguments)?;
                    let mut check = || self.check_index(lease, &mut authorize_read);
                    let context = match self.archive {
                        ArchiveSource::Legacy(archive) =>
                            self.index.build_context_with_check(archive, &request, &mut check)?,
                        #[cfg(feature = "vault-mcp")]
                        ArchiveSource::Vault(archive) =>
                            self.index.build_context_vault_with_check(archive, &request, &mut check)?,
                    };
                    serde_json::to_value(context)
                        .map_err(|_| IntegrationError::Serialization)?
                }
            };
            self.check(lease, &mut authorize_read)?;
            results.push(ToolResponse {
                name: call.name,
                result,
            });
            if serde_json::to_vec(&results)
                .map_err(|_| IntegrationError::Serialization)?
                .len()
                > MAX_RESPONSE_BYTES
            {
                return Err(IntegrationError::ResourceLimit);
            }
        }
        self.check(lease, &mut authorize_read)?;
        Ok(ToolBatchResponse {
            schema_version: CONTRACT_VERSION,
            results,
        })
    }

    fn check(
        &self,
        lease: UnlockLease,
        authorize_read: &mut impl FnMut() -> Result<()>,
    ) -> Result<()> {
        self.security.validate_lease(lease)?;
        authorize_read()?;
        self.security.validate_lease(lease)?;
        Ok(())
    }

    fn check_index(
        &self,
        lease: UnlockLease,
        authorize_read: &mut impl FnMut() -> Result<()>,
    ) -> mind_place_index::Result<()> {
        self.check(lease, authorize_read)
            .map_err(|_| IndexError::Revoked)
    }
}

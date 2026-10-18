//! MCP 2026-07-28 stateless and 2025-11-25 handshake-era wire projection.
//! Only Phase 3B's credential-authorized read dispatcher may serve evidence.
use crate::{tool_catalog, IntegrationCore, ToolBatch, ToolCall, ToolName};
use mind_place_state::{ClientCapability, StateStore};
use serde_json::{json, Value};

pub const MODERN: &str = "2026-07-28";
pub const LEGACY: &str = "2025-11-25";
pub const MCP_PORT: u16 = 43731;
pub const BODY_LIMIT: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct McpReply {
    pub status: u16,
    pub body: Option<Value>,
}
impl McpReply {
    fn ok(id: Value, result: Value) -> Self {
        Self { status: 200, body: Some(json!({"jsonrpc":"2.0","id":id,"result":result})) }
    }
    fn error(status: u16, id: Value, code: i32, message: &'static str) -> Self {
        Self {
            status,
            body: Some(json!({
                "jsonrpc": "2.0", "id": id,
                "error": {"code":code,"message":message}
            })),
        }
    }
    fn no_content() -> Self {
        Self { status: 202, body: None }
    }
}

fn name(name: ToolName) -> &'static str {
    match name {
        ToolName::MemorySearch => "memory_search",
        ToolName::MemoryFetch => "memory_fetch",
        ToolName::ContextBuild => "context_build",
    }
}

pub fn catalog() -> Vec<Value> {
    tool_catalog().into_iter().map(|tool| json!({
        "name": name(tool.name),
        "description": tool.description,
        "inputSchema": tool.input_schema,
        "annotations": {"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true}
    })).collect()
}

fn meta() -> Value {
    json!({"io.modelcontextprotocol/serverInfo":{"name":"mind-place","version":"0.2.5"}})
}

fn valid_id(id: &Value) -> bool {
    id.is_string() || id.is_i64() || id.is_u64()
}

/// After strict HTTP Host/Origin/content-type/size and bearer extraction.
/// Every method authenticates before protocol discovery or tool enumeration.
/// Header mirrors for modern MCP must agree with the JSON request verbatim.
pub fn dispatch(
    core: &IntegrationCore<'_>,
    state: &StateStore,
    bearer: &str,
    message: Value,
    protocol: &str,
    method_header: Option<&str>,
    name_header: Option<&str>,
) -> McpReply {
    let id = message.get("id").cloned().unwrap_or(Value::Null);
    if !message.is_object()
        || message.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || message.get("method").and_then(Value::as_str).is_none()
        || message.is_array()
        || (!id.is_null() && !valid_id(&id))
    {
        return McpReply::error(400, Value::Null, -32600, "Invalid Request");
    }
    let method = message["method"].as_str().unwrap_or_default();
    let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
    if !params.is_object() {
        return McpReply::error(400, id, -32602, "Invalid params");
    }
    if !matches!(protocol, MODERN | LEGACY) {
        return McpReply::error(400, id, -32022, "Unsupported protocol version");
    }
    let modern = protocol == MODERN;
    if modern {
        let request_meta = params.get("_meta");
        if request_meta
            .and_then(|m| m.get("io.modelcontextprotocol/protocolVersion"))
            .and_then(Value::as_str) != Some(MODERN)
            || method_header != Some(method)
            || (method == "tools/call" && name_header != params.get("name").and_then(Value::as_str))
            || (method != "tools/call" && name_header.is_some())
        {
            return McpReply::error(400, id, -32020, "HeaderMismatch");
        }
        if request_meta
            .and_then(|m| m.get("io.modelcontextprotocol/clientCapabilities"))
            .is_none_or(|v| !v.is_object())
        {
            return McpReply::error(400, id, -32602, "Invalid params");
        }
    } else if params.get("_meta").and_then(|m| m.get("io.modelcontextprotocol/protocolVersion")).is_some() {
        return McpReply::error(400, id, -32602, "Invalid params");
    }

    // Authentication is mandatory even for initialize, discovery, ping and
    // tools/list. No client ever identifies itself via untrusted request params.
    let authorized = crate::ClientAuthority::new(state, core.security)
        .authorize(bearer, ClientCapability::Read);
    if authorized.is_err() {
        return McpReply::error(401, id, -32001, "Unauthorized");
    }

    if id.is_null() {
        return if !modern && method == "notifications/initialized" {
            McpReply::no_content()
        } else {
            McpReply::error(400, Value::Null, -32600, "Invalid Request")
        };
    }
    let result = match method {
        "initialize" if !modern => json!({
            "protocolVersion": LEGACY,
            "capabilities":{"tools":{"listChanged":false}},
            "serverInfo":{"name":"mind-place","version":"0.2.5"}
        }),
        "server/discover" if modern => json!({
            "resultType":"complete",
            "supportedVersions":[MODERN],
            "capabilities":{"tools":{"listChanged":false}},
            "ttlMs":0,
            "cacheScope":"private",
            "_meta":meta()
        }),
        "ping" => if modern { json!({"resultType":"complete","_meta":meta()}) } else { json!({}) },
        "tools/list" => {
            if params.get("cursor").is_some() {
                return McpReply::error(400, id, -32602, "Invalid params");
            }
            if modern {
                json!({"resultType":"complete","tools":catalog(),"ttlMs":0,"cacheScope":"private","_meta":meta()})
            } else {
                json!({"tools":catalog()})
            }
        }
        "tools/call" => {
            let tool_name = params.get("name").and_then(Value::as_str);
            let tool = match tool_name {
                Some("memory_search") => ToolName::MemorySearch,
                Some("memory_fetch") => ToolName::MemoryFetch,
                Some("context_build") => ToolName::ContextBuild,
                _ => return McpReply::error(404, id, -32602, "Unknown tool"),
            };
            let arguments = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let response = match core.execute_client_batch(state, bearer, &ToolBatch {
                calls: vec![ToolCall { name: tool, arguments }]
            }) {
                Ok(reply) => reply,
                Err(crate::IntegrationError::AccessDenied | crate::IntegrationError::Locked) => {
                    return McpReply::error(403, id, -32001, "Access revoked");
                }
                Err(crate::IntegrationError::InvalidArguments | crate::IntegrationError::Index(mind_place_index::IndexError::InvalidQuery)) => {
                    return McpReply::error(400, id, -32602, "Invalid params");
                }
                Err(_) => return McpReply::error(500, id, -32603, "Internal error"),
            };
            let content = &response.results[0].result;
            let text = match serde_json::to_string(content) {
                Ok(s) => s,
                Err(_) => return McpReply::error(500, id, -32603, "Internal error"),
            };
            if modern {
                json!({"resultType":"complete","content":[{"type":"text","text":text}],
                    "structuredContent":content,"_meta":meta()})
            } else {
                json!({"content":[{"type":"text","text":text}],"structuredContent":content})
            }
        }
        _ => return McpReply::error(404, id, -32601, "Method not found"),
    };
    // A read permit can be withdrawn while preparing a discovery/list result,
    // so recheck the live authority before disclosure.
    if crate::ClientAuthority::new(state, core.security)
        .authorize(bearer, ClientCapability::Read)
        .is_err()
    {
        return McpReply::error(403, id, -32001, "Access revoked");
    }
    McpReply::ok(id, result)
}

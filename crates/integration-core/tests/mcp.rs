use mind_place_archive::{ArchiveStore, Capture, Message};
use mind_place_index::IndexStore;
use mind_place_integration::{
    mcp::{self, dispatch, LEGACY, MODERN},
    transport::LocalMcpServer,
    ClientAuthority, IntegrationCore,
};
use mind_place_security::SecurityGate;
use mind_place_state::{ClientCapability, StateStore};
use serde_json::{json, Value};
use std::io::{Read, Write};

const ID: &str = "11111111-2222-4333-8444-555555555555";
struct Fixture {
    temp: tempfile::TempDir,
    archive: ArchiveStore,
    index: IndexStore,
    state: StateStore,
    security: SecurityGate,
    token: String,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let archive = ArchiveStore::open(temp.path().join("archive-v1")).unwrap();
        let state = StateStore::open(temp.path().join("state.sqlite3")).unwrap();
        let index = IndexStore::open(temp.path().join("index.sqlite3")).unwrap();
        archive
            .save(Capture {
                schema_version: 1,
                provider: "chatgpt".into(),
                conversation_id: ID.into(),
                source_url: format!("https://chatgpt.com/c/{ID}"),
                title: "Transport".into(),
                conversation_created_at: None,
                conversation_updated_at: None,
                captured_at: "2026-10-18T11:00:00Z".into(),
                capture_method: "manual-dom".into(),
                completeness: "partial".into(),
                warnings: vec![],
                messages: vec![Message {
                    id: "m1".into(),
                    role: "user".into(),
                    text: "amber eclipse knowledge".into(),
                    format: "plain".into(),
                    authored_at: None,
                    model: None,
                    attachments: vec![],
                }],
            })
            .unwrap();
        index.sync(&archive, &state).unwrap();
        let security = SecurityGate::open(temp.path().join("security.json")).unwrap();
        security.initialize(b"test-password-456").unwrap();
        let token = ClientAuthority::new(&state, &security)
            .issue("Claude Code", &[ClientCapability::Read])
            .unwrap()
            .credential()
            .to_string();
        Self {
            temp,
            archive,
            index,
            state,
            security,
            token,
        }
    }
    fn core(&self) -> IntegrationCore<'_> {
        IntegrationCore::new(&self.archive, &self.index, &self.security)
    }
    fn call(&self, method: &str, params: Value, name: Option<&str>) -> mcp::McpReply {
        dispatch(
            &self.core(),
            &self.state,
            &self.token,
            json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}),
            MODERN,
            Some(method),
            name,
        )
    }
}
fn meta() -> Value {
    json!({"io.modelcontextprotocol/protocolVersion":MODERN,"io.modelcontextprotocol/clientCapabilities":{}})
}

#[test]
fn modern_wire_has_discovery_tools_and_verified_evidence() {
    let f = Fixture::new();
    let discover = f.call("server/discover", json!({"_meta":meta()}), None);
    assert_eq!(discover.status, 200);
    assert_eq!(
        discover.body.as_ref().unwrap()["result"]["supportedVersions"][0],
        MODERN
    );
    let list = f.call("tools/list", json!({"_meta":meta()}), None);
    assert_eq!(
        list.body.as_ref().unwrap()["result"]["tools"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let hit = f.call(
        "tools/call",
        json!({"_meta":meta(),"name":"memory_search","arguments":{"queries":["amber eclipse"]}}),
        Some("memory_search"),
    );
    assert_eq!(hit.status, 200);
    assert_eq!(
        hit.body.as_ref().unwrap()["result"]["structuredContent"]["hits"][0]["message_id"],
        "m1"
    );
    assert_eq!(
        hit.body.as_ref().unwrap()["result"]["resultType"],
        "complete"
    );
}
#[test]
fn legacy_handshake_and_modern_header_guards() {
    let f = Fixture::new();
    let hello = dispatch(
        &f.core(),
        &f.state,
        &f.token,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":LEGACY,"capabilities":{},"clientInfo":{"name":"codex","version":"1"}}}),
        LEGACY,
        None,
        None,
    );
    assert_eq!(
        hello.body.as_ref().unwrap()["result"]["protocolVersion"],
        LEGACY
    );
    let mismatched = dispatch(
        &f.core(),
        &f.state,
        &f.token,
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
  "params":{"_meta":meta(),"name":"memory_search","arguments":{"queries":["amber"]}}}),
        MODERN,
        Some("tools/call"),
        Some("memory_fetch"),
    );
    assert_eq!(mismatched.body.as_ref().unwrap()["error"]["code"], -32020);
    let forbidden = dispatch(
        &f.core(),
        &f.state,
        "no-token",
        json!({"jsonrpc":"2.0","id":1,"method":"server/discover","params":{"_meta":meta()}}),
        MODERN,
        Some("server/discover"),
        None,
    );
    assert_eq!(forbidden.status, 401);
    f.security.lock();
    assert_ne!(
        f.call("tools/list", json!({"_meta":meta()}), None).status,
        200
    );
}
fn tcp_request_result(payload: String) -> std::io::Result<String> {
    let mut connection = std::net::TcpStream::connect(("127.0.0.1", mcp::MCP_PORT))?;
    connection.set_read_timeout(Some(std::time::Duration::from_secs(8)))?;
    connection.write_all(payload.as_bytes())?;
    let mut bytes = Vec::new();
    connection.read_to_end(&mut bytes)?;
    String::from_utf8(bytes).map_err(std::io::Error::other)
}
fn tcp_request(payload: String) -> String {
    tcp_request_result(payload).unwrap()
}
fn raw(token: &str, host: &str, origin: &str) -> String {
    let data = json!({"jsonrpc":"2.0","id":1,"method":"server/discover","params":{"_meta":meta()}})
        .to_string();
    format!("POST /mcp HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nAuthorization: Bearer {token}\r\nMCP-Protocol-Version: 2026-07-28\r\nMcp-Method: server/discover\r\n{origin}Content-Length: {}\r\nConnection: close\r\n\r\n{data}",data.len())
}

fn simulated_forwarded_rpc(token: &str, method: &str, params: Value, extras: &str) -> String {
    let name_header = if method == "tools/call" {
        format!(
            "Mcp-Name: {}\r\n",
            params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
        )
    } else {
        String::new()
    };
    let body = json!({"jsonrpc":"2.0","id":11,"method":method,"params":params}).to_string();
    format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:43731\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nAuthorization: Bearer {token}\r\nMCP-Protocol-Version: 2026-07-28\r\nMcp-Method: {method}\r\n{name_header}{extras}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}
async fn simulated_forward(
    token: String,
    method: &'static str,
    params: Value,
    extras: &'static str,
) -> String {
    tokio::task::spawn_blocking(move || {
        tcp_request(simulated_forwarded_rpc(&token, method, params, extras))
    })
    .await
    .unwrap()
}
fn response_json(response: &str) -> Value {
    serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
}

#[tokio::test]
async fn real_http_deny_rebinding_origin_and_locked_epoch() {
    let f = Fixture::new();
    let server = LocalMcpServer::start(
        f.archive.clone(),
        f.security.clone(),
        f.state.path().to_owned(),
        f.temp.path().join("index.sqlite3"),
    )
    .await
    .unwrap();
    let token = f.token.clone();
    let response =
        tokio::task::spawn_blocking(move || tcp_request(raw(&token, "127.0.0.1:43731", "")))
            .await
            .unwrap();
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.contains("supportedVersions"));
    let token = f.token.clone();
    let denied = tokio::task::spawn_blocking(move || tcp_request(raw(&token, "evil.example", "")))
        .await
        .unwrap();
    assert!(denied.starts_with("HTTP/1.1 403"));
    let token = f.token.clone();
    let denied = tokio::task::spawn_blocking(move || {
        tcp_request(raw(
            &token,
            "127.0.0.1:43731",
            "Origin: http://localhost:3000\r\n",
        ))
    })
    .await
    .unwrap();
    assert!(denied.starts_with("HTTP/1.1 403"));

    // 3E: simulate remote tunnel requests at the sole local origin, without
    // pretending that an external OpenAI account/network has been exercised.
    let hosted = ClientAuthority::new(&f.state, &f.security)
        .issue("Simulated hosted reader", &[ClientCapability::Read])
        .unwrap();
    let hosted_id = hosted.client.id.clone();
    let hosted_token = hosted.credential().to_string();
    let discovery = simulated_forward(
        hosted_token.clone(),
        "server/discover",
        json!({"_meta":meta()}),
        "",
    )
    .await;
    assert!(discovery.starts_with("HTTP/1.1 200"), "{discovery}");
    let list = simulated_forward(
        hosted_token.clone(),
        "tools/list",
        json!({"_meta":meta()}),
        "",
    )
    .await;
    let tools = response_json(&list)["result"]["tools"]
        .as_array()
        .unwrap()
        .clone();
    let names: Vec<&str> = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["memory_search", "memory_fetch", "context_build"]);
    assert!(tools
        .iter()
        .all(|tool| tool["annotations"]["readOnlyHint"] == true));
    let search = simulated_forward(
        hosted_token.clone(),
        "tools/call",
        json!({"_meta":meta(),"name":"memory_search","arguments":{"queries":["amber eclipse"]}}),
        "",
    )
    .await;
    assert!(search.starts_with("HTTP/1.1 200"), "{search}");
    let search_json = response_json(&search);
    let hit = &search_json["result"]["structuredContent"]["hits"][0];
    assert_eq!(hit["message_id"], "m1");
    let fetch = simulated_forward(
        hosted_token.clone(),
        "tools/call",
        json!({"_meta":meta(),"name":"memory_fetch","arguments":{
            "source_id":hit["source_id"],
            "archive_id":hit["archive_id"],
            "snapshot_id":hit["snapshot_id"],
            "granularity":"message",
            "message_id":"m1"
        }}),
        "",
    )
    .await;
    assert!(fetch.starts_with("HTTP/1.1 200"), "{fetch}");
    assert!(fetch.contains("amber eclipse knowledge"));
    let context = simulated_forward(
        hosted_token.clone(),
        "tools/call",
        json!({"_meta":meta(),"name":"context_build","arguments":{"queries":["amber eclipse"]}}),
        "",
    )
    .await;
    assert!(context.starts_with("HTTP/1.1 200"), "{context}");
    assert!(context.contains("m1"));

    // Hosted clients never get owner/admin Tauri commands, even by guessing
    // the method or tool name; invalid metadata cannot widen the catalog.
    for name in [
        "revoke_local_mcp_client",
        "start_hosted_tunnel",
        "set_client_capability",
    ] {
        let denied = simulated_forward(
            hosted_token.clone(),
            "tools/call",
            json!({"_meta":meta(),"name":name,"arguments":{"client_id":hosted_id}}),
            "",
        )
        .await;
        assert!(denied.starts_with("HTTP/1.1 404"), "{denied}");
        assert!(!denied.contains("amber eclipse"));
    }
    let denied = simulated_forward(
        hosted_token.clone(),
        "resources/list",
        json!({"_meta":meta()}),
        "",
    )
    .await;
    assert!(denied.starts_with("HTTP/1.1 404"), "{denied}");

    // These are the dangerous forwarding and credential confusion cases:
    // a tunneled request must still face the origin's strict HTTP checks.
    for header in [
        "Origin: https://chatgpt.com\r\n",
        "Forwarded: for=198.51.100.7;host=evil.example\r\n",
        "X-Forwarded-Host: evil.example\r\n",
    ] {
        let denied = simulated_forward(
            hosted_token.clone(),
            "tools/list",
            json!({"_meta":meta()}),
            header,
        )
        .await;
        assert!(denied.starts_with("HTTP/1.1 403"), "{denied}");
    }
    let duplicated = format!("Authorization: Bearer {}\r\n", f.token);
    let denied = simulated_forwarded_rpc(
        &hosted_token,
        "tools/list",
        json!({"_meta":meta()}),
        &duplicated,
    );
    let denied = tokio::task::spawn_blocking(move || tcp_request(denied))
        .await
        .unwrap();
    assert!(denied.starts_with("HTTP/1.1 401"), "{denied}");
    let denied = simulated_forward(
        hosted_token.clone(),
        "tools/list",
        json!({"_meta":{"io.modelcontextprotocol/protocolVersion":"2025-11-25","io.modelcontextprotocol/clientCapabilities":{}}}),
        "",
    )
    .await;
    assert!(denied.starts_with("HTTP/1.1 400"), "{denied}");

    // A credential with admin but no read capability cannot enumerate tools
    // or elevate itself by supplying a claimed role in its JSON params.
    let admin = ClientAuthority::new(&f.state, &f.security)
        .issue("Isolated admin", &[ClientCapability::Admin])
        .unwrap();
    let denied = simulated_forward(
        admin.credential().to_string(),
        "tools/list",
        json!({"_meta":meta(),"capabilities":["read","admin"]}),
        "",
    )
    .await;
    assert!(denied.starts_with("HTTP/1.1 401"), "{denied}");
    assert!(!denied.contains("memory_search"));

    ClientAuthority::new(&f.state, &f.security)
        .set_capability(&hosted_id, ClientCapability::Read, false)
        .unwrap();
    let denied = simulated_forward(
        hosted_token.clone(),
        "tools/list",
        json!({"_meta":meta()}),
        "",
    )
    .await;
    assert!(denied.starts_with("HTTP/1.1 401"), "{denied}");
    ClientAuthority::new(&f.state, &f.security)
        .set_capability(&hosted_id, ClientCapability::Read, true)
        .unwrap();
    let rotated = ClientAuthority::new(&f.state, &f.security)
        .rotate(&hosted_id)
        .unwrap();
    let denied = simulated_forward(hosted_token, "tools/list", json!({"_meta":meta()}), "").await;
    assert!(denied.starts_with("HTTP/1.1 401"), "{denied}");
    ClientAuthority::new(&f.state, &f.security)
        .revoke(&hosted_id)
        .unwrap();
    let denied = simulated_forward(
        rotated.credential().to_string(),
        "tools/list",
        json!({"_meta":meta()}),
        "",
    )
    .await;
    assert!(denied.starts_with("HTTP/1.1 401"), "{denied}");
    assert!(!denied.contains("memory_search"));

    f.security.lock();
    let token = f.token.clone();
    let denied =
        tokio::task::spawn_blocking(move || tcp_request_result(raw(&token, "127.0.0.1:43731", "")))
            .await
            .unwrap();
    // A lock can close its listener before an HTTP response is written.
    // Windows reports TCP ConnectionReset for that refusal; neither form
    // may ever contain private MCP discovery or source material.
    match denied {
        Ok(response) => assert!(!response.contains("supportedVersions")),
        Err(error) => assert!(matches!(
            error.kind(),
            std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::ConnectionAborted
                | std::io::ErrorKind::BrokenPipe
                | std::io::ErrorKind::UnexpectedEof
        )),
    }
    server.stop();
}

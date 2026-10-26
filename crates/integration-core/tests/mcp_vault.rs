#![cfg(feature = "vault-mcp")]
//! Synthetic-only MP-9B transport core regression: keyed authority and epoch.
use mind_place_archive::Capture;
use mind_place_index::vault_archive::VaultCaptureStore;
use mind_place_integration::{mcp, ClientAuthority, IntegrationCore};
use mind_place_security::{vault::CryptoVault, SecurityGate};
use mind_place_state::ClientCapability;
use serde_json::json;

#[test]
fn vaulted_mcp_read_is_keyed_authorized_and_revoked_on_lock() {
    let temp = tempfile::tempdir().unwrap();
    let security = SecurityGate::open(temp.path().join("security.json")).unwrap();
    security.initialize(b"synthetic-vault-mcp-secret").unwrap();
    let lease = security.require_unlocked().unwrap();
    let (vault, _recovery, manifest) = CryptoVault::provision(&security, lease).unwrap();
    let root = temp.path().join("vault");
    manifest.persist_new(&root).unwrap();
    let archive = VaultCaptureStore::open(&root, vault, &security, lease).unwrap();
    let capture: Capture = serde_json::from_value(json!({
        "schema_version": 1,
        "provider": "chatgpt",
        "conversation_id": "123e4567-e89b-12d3-a456-426614174000",
        "source_url": "https://chatgpt.com/c/123e4567-e89b-12d3-a456-426614174000",
        "title": "Synthetic MCP",
        "captured_at": "2026-10-26T03:00:00Z",
        "capture_method": "manual-dom",
        "completeness": "partial",
        "warnings": [],
        "messages": [{
            "id": "synthetic-1",
            "role": "user",
            "text": "luminous vault example",
            "format": "plain",
            "attachments": []
        }]
    }))
    .unwrap();
    archive.save(capture).unwrap();
    let authority = archive.open_state_store().unwrap();
    let index = archive.open_index_store().unwrap();
    index
        .sync_vault_with_check(&archive, &authority, || Ok(()))
        .unwrap();
    let grant = ClientAuthority::new(&authority, &security)
        .issue("Synthetic MCP client", &[ClientCapability::Read])
        .unwrap();
    let credential = grant.credential();
    let core = IntegrationCore::new_vault(&archive, &index, &security);
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "memory_search",
            "arguments": {"queries": ["luminous"], "limit": 10},
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": mcp::MODERN,
                "io.modelcontextprotocol/clientCapabilities": {}
            }
        }
    });
    let reply = mcp::dispatch(
        &core,
        &authority,
        credential,
        request.clone(),
        mcp::MODERN,
        Some("tools/call"),
        Some("memory_search"),
    );
    assert_eq!(
        reply.status, 200,
        "vault search must authorize a live grant"
    );
    assert!(reply.body.unwrap().to_string().contains("Synthetic MCP"));
    assert!(!std::fs::read(root.join("state.sqlite3"))
        .unwrap()
        .starts_with(b"SQLite format 3\0"));
    let unauthorized = mcp::dispatch(
        &core,
        &authority,
        "mpc_v1_invalid",
        request.clone(),
        mcp::MODERN,
        Some("tools/call"),
        Some("memory_search"),
    );
    assert_eq!(unauthorized.status, 401);
    ClientAuthority::new(&authority, &security)
        .revoke(&grant.client.id)
        .unwrap();
    let revoked = mcp::dispatch(
        &core,
        &authority,
        credential,
        request.clone(),
        mcp::MODERN,
        Some("tools/call"),
        Some("memory_search"),
    );
    assert_eq!(revoked.status, 401);
    security.lock();
    let locked = mcp::dispatch(
        &core,
        &authority,
        credential,
        request,
        mcp::MODERN,
        Some("tools/call"),
        Some("memory_search"),
    );
    assert_ne!(locked.status, 200);
    assert!(archive.open_state_store().is_err());
}

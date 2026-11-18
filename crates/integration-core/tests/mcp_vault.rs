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

mod mp9c_privacy {
    use super::*;
    use mind_place_index::{QueryTerm, RetrievalRequest};
    use mind_place_integration::{IntegrationError, ToolBatch, ToolCall, ToolName};
    use sha2::Digest;
    use std::{sync::mpsc, thread, time::Duration};

    const SECRET_ID: &str = "123e4567-e89b-12d3-a456-426614174001";
    const KEPT_ID: &str = "123e4567-e89b-12d3-a456-426614174002";

    struct Rig {
        _temp: tempfile::TempDir,
        security: SecurityGate,
        archive: VaultCaptureStore,
        index: mind_place_index::IndexStore,
        reader: String,
        admin_hash: String,
        secret: String,
    }

    fn capture(id: &str, title: &str, text: &str) -> Capture {
        serde_json::from_value(json!({
            "schema_version": 1,
            "provider": "chatgpt",
            "conversation_id": id,
            "source_url": format!("https://chatgpt.com/c/{id}"),
            "title": title,
            "captured_at": "2026-10-26T03:00:00Z",
            "capture_method": "manual-dom",
            "completeness": "partial",
            "warnings": [],
            "messages": [{
                "id": "synthetic-1",
                "role": "user",
                "text": text,
                "format": "plain",
                "attachments": []
            }]
        }))
        .unwrap()
    }

    fn rig() -> Rig {
        let temp = tempfile::tempdir().unwrap();
        let security = SecurityGate::open(temp.path().join("security.json")).unwrap();
        security.initialize(b"synthetic-vault-mcp-secret").unwrap();
        let lease = security.require_unlocked().unwrap();
        let (vault, _recovery, manifest) = CryptoVault::provision(&security, lease).unwrap();
        let root = temp.path().join("vault");
        manifest.persist_new(&root).unwrap();
        let archive = VaultCaptureStore::open(&root, vault, &security, lease).unwrap();
        archive
            .save(capture(
                SECRET_ID,
                "Forgotten title",
                "violet spacecraft secret",
            ))
            .unwrap();
        archive
            .save(capture(KEPT_ID, "Kept title", "violet spacecraft kept"))
            .unwrap();
        let authority = archive.open_state_store().unwrap();
        let index = archive.open_index_store().unwrap();
        index
            .sync_vault_with_check(&archive, &authority, || Ok(()))
            .unwrap();
        let owner = ClientAuthority::new(&authority, &security);
        let reader = owner
            .issue("Model reader", &[ClientCapability::Read])
            .unwrap()
            .credential()
            .to_owned();
        let admin = owner.issue("Owner", &[ClientCapability::Admin]).unwrap();
        let admin_hash = format!("{:x}", sha2::Sha256::digest(admin.credential().as_bytes()));
        let secret = index
            .retrieve(&RetrievalRequest {
                queries: vec![QueryTerm::Text("secret".into())],
                limit: 5,
                ..Default::default()
            })
            .unwrap()
            .hits[0]
            .source_id
            .clone();
        Rig {
            _temp: temp,
            security,
            archive,
            index,
            reader,
            admin_hash,
            secret,
        }
    }

    fn search(query: &str) -> ToolBatch {
        ToolBatch {
            calls: vec![ToolCall {
                name: ToolName::MemorySearch,
                arguments: json!({"queries": [query]}),
            }],
        }
    }

    /// Stale index after a committed redaction: filtered, not an oracle.
    #[test]
    fn committed_redaction_is_filtered_and_indistinguishable_from_a_miss() {
        let r = rig();
        let state = r.archive.open_state_store().unwrap();
        state
            .redact_source(&r.admin_hash, &r.secret, "Owner redaction")
            .unwrap();
        let core = IntegrationCore::new_vault(&r.archive, &r.index, &r.security);
        // Index deliberately NOT resynchronized.
        let after = core
            .execute_client_batch(&state, &r.reader, &search("violet spacecraft"))
            .unwrap();
        let hits = after.results[0].result["hits"].as_array().unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0]["conversation_id"], KEPT_ID);
        assert_eq!(after.results[0].result["candidate_count"], 1);
        let encoded = serde_json::to_string(&after).unwrap();
        assert!(!encoded.contains(&r.secret) && !encoded.contains("Forgotten"));
        let probed = core
            .execute_client_batch(&state, &r.reader, &search("secret"))
            .unwrap();
        let missed = core
            .execute_client_batch(&state, &r.reader, &search("nonexistentterm"))
            .unwrap();
        assert_eq!(probed.results[0].result, missed.results[0].result);
        let named = ToolBatch {
            calls: vec![ToolCall {
                name: ToolName::MemorySearch,
                arguments: json!({"queries": ["violet"], "filters": {"source_ids": [r.secret]}}),
            }],
        };
        assert!(matches!(
            core.execute_client_batch(&state, &r.reader, &named),
            Err(IntegrationError::AccessDenied)
        ));
    }

    /// MP-9C review P1: a stale hidden row must not displace the visible hit at
    /// LIMIT nor shape ranking/counts. The credentialed read prunes the stale
    /// projection (fail closed first) and the client sees only visible rows.
    #[test]
    fn stale_hidden_row_cannot_displace_the_visible_hit_at_limit_one() {
        let r = rig();
        let state = r.archive.open_state_store().unwrap();
        state
            .redact_source(&r.admin_hash, &r.secret, "Owner redaction")
            .unwrap();
        let core = IntegrationCore::new_vault(&r.archive, &r.index, &r.security);
        // Whatever the raw ranking, the stale index still holds the secret.
        assert!(r
            .index
            .retrieve(&RetrievalRequest {
                queries: vec![QueryTerm::Text("violet".into())],
                limit: 5,
                ..Default::default()
            })
            .unwrap()
            .hits
            .iter()
            .any(|hit| hit.source_id == r.secret));
        let limited = ToolBatch {
            calls: vec![ToolCall {
                name: ToolName::MemorySearch,
                arguments: json!({"queries": ["violet"], "limit": 1}),
            }],
        };
        let response = core
            .execute_client_batch(&state, &r.reader, &limited)
            .unwrap();
        let result = &response.results[0].result;
        let hits = result["hits"].as_array().unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0]["conversation_id"], KEPT_ID);
        assert_eq!(result["candidate_count"], 1);
        assert_eq!(result["truncated"], false);
        assert!(!serde_json::to_string(&response)
            .unwrap()
            .contains(&r.secret));
        // The projection was pruned, so the derived rows no longer hold it.
        assert!(r
            .index
            .retrieve(&RetrievalRequest {
                queries: vec![QueryTerm::Text("violet".into())],
                limit: 5,
                ..Default::default()
            })
            .unwrap()
            .hits
            .iter()
            .all(|hit| hit.source_id != r.secret));
    }

    /// Review P1, ordering A: a batch that already decided the secret source
    /// is visible holds the disclosure fence, so a concurrent redaction cannot
    /// commit until the response exists. Ordering B: afterwards nothing leaks.
    #[test]
    fn in_flight_batch_blocks_redaction_until_its_response_is_complete() {
        let r = rig();
        let core = IntegrationCore::new_vault(&r.archive, &r.index, &r.security);
        let (decided_tx, decided_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let (redacted_tx, redacted_rx) = mpsc::channel();
        let (core_ref, archive) = (&core, &r.archive);
        thread::scope(|scope| {
            let reader = scope.spawn(move || {
                let state = archive.open_state_store().unwrap();
                let mut paused = false;
                core_ref.execute_batch_visible(
                    &search("violet spacecraft"),
                    || Ok(()),
                    &mut |source_id: &str| {
                        let visible = !state.source_is_redacted(source_id)?;
                        if !paused {
                            paused = true;
                            decided_tx.send(()).unwrap();
                            // Bounded: a regression fails the test instead of hanging it.
                            let _ = release_rx.recv_timeout(Duration::from_secs(20));
                        }
                        Ok(visible)
                    },
                )
            });
            decided_rx.recv_timeout(Duration::from_secs(30)).unwrap();
            let (admin, secret) = (r.admin_hash.clone(), r.secret.clone());
            let redactor = scope.spawn(move || {
                let result = archive.open_state_store().unwrap().redact_source(
                    &admin,
                    &secret,
                    "Concurrent owner redaction",
                );
                redacted_tx.send(()).unwrap();
                result
            });
            assert!(
                redacted_rx
                    .recv_timeout(Duration::from_millis(300))
                    .is_err(),
                "redaction committed while a vetted MCP batch was in flight"
            );
            release_tx.send(()).unwrap();
            let response = reader.join().unwrap().unwrap();
            // Linearized BEFORE the redaction: the pre-redaction view.
            assert_eq!(
                response.results[0].result["hits"].as_array().unwrap().len(),
                2
            );
            redacted_rx.recv_timeout(Duration::from_secs(30)).unwrap();
            redactor.join().unwrap().unwrap();
        });
        let state = r.archive.open_state_store().unwrap();
        let later = core
            .execute_client_batch(&state, &r.reader, &search("violet spacecraft"))
            .unwrap();
        assert_eq!(later.results[0].result["candidate_count"], 1);
        assert!(!serde_json::to_string(&later).unwrap().contains(&r.secret));
    }

    /// Lease revocation during an in-flight batch still fails closed.
    #[test]
    fn lock_during_an_in_flight_batch_discards_the_response() {
        let r = rig();
        let core = IntegrationCore::new_vault(&r.archive, &r.index, &r.security);
        let (decided_tx, decided_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let core_ref = &core;
        thread::scope(|scope| {
            let reader = scope.spawn(move || {
                let mut paused = false;
                core_ref.execute_batch_visible(
                    &search("violet spacecraft"),
                    || Ok(()),
                    &mut |_: &str| {
                        if !paused {
                            paused = true;
                            decided_tx.send(()).unwrap();
                            // Bounded: a regression fails the test instead of hanging it.
                            let _ = release_rx.recv_timeout(Duration::from_secs(20));
                        }
                        Ok(true)
                    },
                )
            });
            decided_rx.recv_timeout(Duration::from_secs(30)).unwrap();
            r.security.lock();
            release_tx.send(()).unwrap();
            assert!(reader.join().unwrap().is_err());
        });
    }
}

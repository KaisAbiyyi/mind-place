use mind_place_archive::{ArchiveStore, Capture, Message};
use mind_place_index::IndexStore;
use mind_place_integration::{
    tool_catalog, IntegrationCore, IntegrationError, ToolBatch, ToolCall, ToolName,
    MAX_BATCH_CALLS, MAX_REQUEST_BYTES,
};
use mind_place_security::SecurityGate;
use mind_place_state::StateStore;
use serde_json::{json, Value};

const CONVERSATION: &str = "11111111-2222-4333-8444-555555555555";

struct Fixture {
    _dir: tempfile::TempDir,
    archive: ArchiveStore,
    index: IndexStore,
    security: SecurityGate,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let archive = ArchiveStore::open(dir.path().join("archive-v1")).unwrap();
        let state = StateStore::open(dir.path().join("state.sqlite3")).unwrap();
        let index = IndexStore::open(dir.path().join("index.sqlite3")).unwrap();
        archive
            .save(Capture {
                schema_version: 1,
                provider: "chatgpt".into(),
                conversation_id: CONVERSATION.into(),
                source_url: format!("https://chatgpt.com/c/{CONVERSATION}"),
                title: "Protocol neutral bridge".into(),
                conversation_created_at: None,
                conversation_updated_at: None,
                captured_at: "2026-10-18T10:00:00Z".into(),
                capture_method: "manual-dom".into(),
                completeness: "partial".into(),
                warnings: vec![],
                messages: vec![Message {
                    id: "msg-1".into(),
                    role: "user".into(),
                    text: "copper lantern continuity proof".into(),
                    format: "plain".into(),
                    authored_at: None,
                    model: None,
                    attachments: vec![],
                }],
            })
            .unwrap();
        index.sync(&archive, &state).unwrap();
        let security = SecurityGate::open(dir.path().join("security.json")).unwrap();
        Self {
            _dir: dir,
            archive,
            index,
            security,
        }
    }

    fn core(&self) -> IntegrationCore<'_> {
        IntegrationCore::new(&self.archive, &self.index, &self.security)
    }

    fn unlock(&self) {
        self.security.initialize(b"test-password").unwrap();
    }
}

fn call(name: ToolName, arguments: Value) -> ToolCall {
    ToolCall { name, arguments }
}

fn search() -> ToolCall {
    call(
        ToolName::MemorySearch,
        json!({"queries": ["copper lantern"], "limit": 3}),
    )
}

#[test]
fn catalog_is_closed_read_only_and_machine_readable() {
    let tools = tool_catalog();
    assert_eq!(tools.len(), 3);
    assert_eq!(
        tools.iter().map(|tool| tool.name).collect::<Vec<_>>(),
        vec![
            ToolName::MemorySearch,
            ToolName::MemoryFetch,
            ToolName::ContextBuild
        ]
    );
    for tool in tools {
        assert_eq!(tool.input_schema["additionalProperties"], false);
        assert!(tool.input_schema["required"].is_array());
    }
    assert!(
        serde_json::from_str::<ToolCall>(r#"{"name":"memory_delete","arguments":{}}"#).is_err()
    );
    assert!(serde_json::from_str::<ToolCall>(
        r#"{"name":"memory_search","arguments":{},"write":true}"#
    )
    .is_err());
}

#[test]
fn locked_process_returns_no_evidence_and_never_invokes_client_guard() {
    let fixture = Fixture::new();
    let mut checked = 0;
    let batch = ToolBatch {
        calls: vec![search()],
    };
    let err = fixture
        .core()
        .execute_batch(&batch, || {
            checked += 1;
            Ok(())
        })
        .unwrap_err();
    assert_eq!(err.code(), "MP_INTEGRATION_LOCKED");
    assert_eq!(checked, 0);

    fixture.unlock();
    fixture.security.lock();
    assert_eq!(
        fixture
            .core()
            .execute_batch(&batch, || Ok(()))
            .unwrap_err()
            .code(),
        "MP_INTEGRATION_LOCKED"
    );
}

#[test]
fn search_then_pinned_fetch_and_context_preserve_raw_provenance() {
    let fixture = Fixture::new();
    fixture.unlock();
    let result = fixture
        .core()
        .execute_batch(
            &ToolBatch {
                calls: vec![search()],
            },
            || Ok(()),
        )
        .unwrap();
    assert_eq!(result.schema_version, 1);
    let hit = &result.results[0].result["hits"][0];
    assert!(hit["source_id"].as_str().unwrap().starts_with("src:v1:"));
    assert_eq!(hit["message_id"], "msg-1");
    let fetch = call(
        ToolName::MemoryFetch,
        json!({
            "source_id": hit["source_id"],
            "archive_id": hit["archive_id"],
            "snapshot_id": hit["snapshot_id"],
            "message_id": hit["message_id"]
        }),
    );
    let context = call(
        ToolName::ContextBuild,
        json!({"queries": ["copper lantern"], "max_sources": 2}),
    );
    let results = fixture
        .core()
        .execute_batch(
            &ToolBatch {
                calls: vec![fetch, context],
            },
            || Ok(()),
        )
        .unwrap();
    assert_eq!(results.results.len(), 2);
    let fetched = &results.results[0].result;
    assert_eq!(fetched["source_id"], hit["source_id"]);
    assert_eq!(fetched["snapshot_id"], hit["snapshot_id"]);
    assert_eq!(
        fetched["segments"][0]["text"],
        "copper lantern continuity proof"
    );
    let packed = &results.results[1].result;
    assert_eq!(packed["groups"][0]["source_id"], hit["source_id"]);
    assert_eq!(packed["groups"][0]["snapshot_id"], hit["snapshot_id"]);
    assert!(packed["text_bytes"].as_u64().unwrap() > 0);
}

#[test]
fn guard_must_be_rechecked_and_a_revocation_discards_the_whole_batch() {
    let fixture = Fixture::new();
    fixture.unlock();
    let mut checks = 0;
    let err = fixture
        .core()
        .execute_batch(
            &ToolBatch {
                calls: vec![search(), search()],
            },
            || {
                checks += 1;
                if checks >= 4 {
                    Err(IntegrationError::AccessDenied)
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
    assert!(checks >= 4);
    assert_eq!(err.code(), "MP_INTEGRATION_DENIED");
}

#[test]
fn lock_epoch_changed_during_search_prevents_response_disclosure() {
    let fixture = Fixture::new();
    fixture.unlock();
    let mut checks = 0;
    let err = fixture
        .core()
        .execute_batch(
            &ToolBatch {
                calls: vec![search()],
            },
            || {
                checks += 1;
                if checks == 2 {
                    fixture.security.lock();
                }
                Ok(())
            },
        )
        .unwrap_err();
    assert_eq!(err.code(), "MP_INTEGRATION_LOCKED");
}

#[test]
fn rejects_unknown_filters_unsafe_fetch_and_oversized_batches() {
    let fixture = Fixture::new();
    fixture.unlock();
    for bad in [
        call(
            ToolName::MemorySearch,
            json!({"queries": ["lantern"], "filters": {"approved_root": "C:/"}}),
        ),
        call(
            ToolName::MemoryFetch,
            json!({"source_id": "../../private", "archive_id": "bad", "snapshot_id": "bad"}),
        ),
        call(ToolName::ContextBuild, json!({"queries": []})),
    ] {
        let err = fixture
            .core()
            .execute_batch(&ToolBatch { calls: vec![bad] }, || Ok(()))
            .unwrap_err();
        assert!(matches!(
            err,
            IntegrationError::InvalidArguments | IntegrationError::Index(_)
        ));
    }
    let too_many = ToolBatch {
        calls: vec![search(); MAX_BATCH_CALLS + 1],
    };
    assert_eq!(
        fixture
            .core()
            .execute_batch(&too_many, || Ok(()))
            .unwrap_err()
            .code(),
        "MP_INTEGRATION_INVALID_REQUEST"
    );
    let huge = ToolBatch {
        calls: vec![call(
            ToolName::MemorySearch,
            json!({"queries": ["x".repeat(MAX_REQUEST_BYTES)]}),
        )],
    };
    assert_eq!(
        fixture
            .core()
            .execute_batch(&huge, || Ok(()))
            .unwrap_err()
            .code(),
        "MP_INTEGRATION_LIMIT"
    );
}

//! MP-7MCP-02 negative matrix: per-client / per-collection read grants are
//! enforced server-side across search, context and direct fetch. Synthetic
//! corpus only; this is NOT evidence that a real Claude client connected.
use mind_place_archive::{ArchiveStore, Capture, Message};
use mind_place_index::IndexStore;
use mind_place_integration::{
    mcp::{dispatch, McpReply, CLAUDE_LEGACY},
    ClientAuthority, IntegrationCore, ToolBatch, ToolCall, ToolName,
};
use mind_place_security::SecurityGate;
use mind_place_state::{ClientCapability, StateStore};
use serde_json::{json, Value};

const PUBLIC_ID: &str = "aaaaaaaa-2222-4333-8444-555555555555";
const PRIVATE_ID: &str = "bbbbbbbb-2222-4333-8444-555555555555";
const SECRET: &[u8] = b"scope-matrix-secret-1";
const PRIVATE_MARKER: &str = "obsidianledgerprivate";
const PUBLIC_MARKER: &str = "celestialatlaspublic";

struct Ids {
    source: String,
    archive: String,
    snapshot: String,
}
struct Fixture {
    _dir: tempfile::TempDir,
    archive: ArchiveStore,
    index: IndexStore,
    state: StateStore,
    security: SecurityGate,
    public: Ids,
    private: Ids,
    public_collection: String,
    private_collection: String,
    /// Reads only the public collection.
    public_reader: (String, String),
    /// Reads only the private collection.
    private_reader: (String, String),
    /// Valid Read credential with NO grant.
    ungranted: (String, String),
}

fn capture(conversation: &str, title: &str, text: &str) -> Capture {
    Capture {
        schema_version: 1,
        provider: "chatgpt".into(),
        conversation_id: conversation.into(),
        source_url: format!("https://chatgpt.com/c/{conversation}"),
        title: title.into(),
        conversation_created_at: None,
        conversation_updated_at: None,
        captured_at: "2026-11-09T10:00:00Z".into(),
        capture_method: "manual-dom".into(),
        completeness: "partial".into(),
        warnings: vec![],
        messages: vec![Message {
            id: "m1".into(),
            role: "user".into(),
            text: text.into(),
            format: "plain".into(),
            authored_at: None,
            model: None,
            attachments: vec![],
        }],
    }
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let archive = ArchiveStore::open(dir.path().join("archive-v1")).unwrap();
        let state = StateStore::open(dir.path().join("state.sqlite3")).unwrap();
        let index = IndexStore::open(dir.path().join("index.sqlite3")).unwrap();
        // Both conversations contain "meridian" so aggregate queries can span
        // collections; each also has a unique marker.
        archive
            .save(capture(
                PUBLIC_ID,
                "Public notes",
                &format!("meridian {PUBLIC_MARKER} open material"),
            ))
            .unwrap();
        archive
            .save(capture(
                PRIVATE_ID,
                "Private notes",
                &format!("meridian {PRIVATE_MARKER} confidential material"),
            ))
            .unwrap();
        index.sync(&archive, &state).unwrap();
        let security = SecurityGate::open(dir.path().join("security.json")).unwrap();
        security.initialize(SECRET).unwrap();
        let core = IntegrationCore::new(&archive, &index, &security);
        let ids = |marker: &str| -> Ids {
            let result = core
                .execute_batch(
                    &ToolBatch {
                        calls: vec![ToolCall {
                            name: ToolName::MemorySearch,
                            arguments: json!({"queries": [marker]}),
                        }],
                    },
                    || Ok(()),
                )
                .unwrap();
            let hit = &result.results[0].result["hits"][0];
            Ids {
                source: hit["source_id"].as_str().unwrap().into(),
                archive: hit["archive_id"].as_str().unwrap().into(),
                snapshot: hit["snapshot_id"].as_str().unwrap().into(),
            }
        };
        let public = ids(PUBLIC_MARKER);
        let private = ids(PRIVATE_MARKER);
        assert_ne!(public.source, private.source);
        let owner = ClientAuthority::new(&state, &security);
        let public_collection = owner
            .create_collection("Public", std::slice::from_ref(&public.source))
            .unwrap();
        let private_collection = owner
            .create_collection("Private", std::slice::from_ref(&private.source))
            .unwrap();
        let issue = |name: &str| {
            let issued = owner.issue(name, &[ClientCapability::Read]).unwrap();
            (issued.client.id.clone(), issued.credential().to_owned())
        };
        let public_reader = issue("Public reader");
        let private_reader = issue("Private reader");
        let ungranted = issue("Ungranted reader");
        owner
            .grant_collection(&public_reader.0, &public_collection)
            .unwrap();
        owner
            .grant_collection(&private_reader.0, &private_collection)
            .unwrap();
        Self {
            _dir: dir,
            archive,
            index,
            state,
            security,
            public,
            private,
            public_collection,
            private_collection,
            public_reader,
            private_reader,
            ungranted,
        }
    }
    fn owner(&self) -> ClientAuthority<'_> {
        ClientAuthority::new(&self.state, &self.security)
    }
    fn call(&self, token: &str, tool: &str, arguments: Value) -> McpReply {
        dispatch(
            &IntegrationCore::new(&self.archive, &self.index, &self.security),
            &self.state,
            token,
            json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{
                "name": tool, "arguments": arguments
            }}),
            CLAUDE_LEGACY,
            None,
            None,
        )
    }
    fn search(&self, token: &str, query: &str) -> McpReply {
        self.call(token, "memory_search", json!({"queries":[query]}))
    }
    fn context(&self, token: &str, query: &str) -> McpReply {
        self.call(token, "context_build", json!({"queries":[query]}))
    }
    fn fetch(&self, token: &str, ids: &Ids, cursor: Value, max_bytes: usize) -> McpReply {
        self.call(
            token,
            "memory_fetch",
            json!({
                "source_id": ids.source, "archive_id": ids.archive,
                "snapshot_id": ids.snapshot, "granularity": "revision",
                "budget": {"max_bytes": max_bytes, "max_estimated_tokens": max_bytes.max(4), "max_latency_ms": 2000},
                "cursor": cursor
            }),
        )
    }
}

fn hits(reply: &McpReply) -> Vec<Value> {
    assert_eq!(reply.status, 200, "{:?}", reply.body);
    reply.body.as_ref().unwrap()["result"]["structuredContent"]["hits"]
        .as_array()
        .unwrap()
        .clone()
}
fn text(reply: &McpReply) -> String {
    reply
        .body
        .as_ref()
        .map(Value::to_string)
        .unwrap_or_default()
}
/// Wire reply without the request correlation field, for exact comparison.
fn opaque(reply: &McpReply) -> (u16, Value) {
    let mut body = reply.body.clone().unwrap();
    body.as_object_mut().unwrap().remove("id");
    (reply.status, body)
}
fn fabricated() -> Ids {
    Ids {
        source: format!("src:v1:{}", "0".repeat(64)),
        archive: "1".repeat(64),
        snapshot: "2".repeat(64),
    }
}

#[test]
fn search_is_filtered_to_the_granted_collection_only() {
    let f = Fixture::new();
    let public = f.search(&f.public_reader.1, "meridian");
    let found = hits(&public);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0]["source_id"], f.public.source);
    assert!(!text(&public).contains(PRIVATE_MARKER));
    let private = f.search(&f.private_reader.1, "meridian");
    assert_eq!(hits(&private)[0]["source_id"], f.private.source);
    assert!(!text(&private).contains(PUBLIC_MARKER));
    // The other collection's unique marker yields nothing, not an error.
    assert!(hits(&f.search(&f.public_reader.1, PRIVATE_MARKER)).is_empty());
}

#[test]
fn explicit_filter_cannot_widen_beyond_the_grant_and_is_opaque() {
    let f = Fixture::new();
    let ask = |ids: &Ids| {
        f.call(
            &f.public_reader.1,
            "memory_search",
            json!({"queries":["meridian"],"filters":{"source_ids":[ids.source]}}),
        )
    };
    let denied = ask(&f.private);
    let unknown = ask(&fabricated());
    assert!(hits(&denied).is_empty());
    // Out-of-scope is byte-for-byte what a nonexistent source looks like.
    assert_eq!(opaque(&denied), opaque(&unknown));
}

#[test]
fn context_aggregation_never_leaks_another_collection() {
    let f = Fixture::new();
    let reply = f.context(&f.public_reader.1, "meridian");
    assert_eq!(reply.status, 200);
    let body = text(&reply);
    assert!(body.contains(PUBLIC_MARKER));
    assert!(!body.contains(PRIVATE_MARKER));
    assert!(!body.contains(&f.private.source));
    let marker_only = f.context(&f.public_reader.1, PRIVATE_MARKER);
    assert_eq!(marker_only.status, 200);
    assert!(!text(&marker_only).contains("confidential"));
    assert_eq!(
        marker_only.body.unwrap()["result"]["structuredContent"]["excerpt_count"],
        0
    );
}

#[test]
fn direct_fetch_by_guessed_exact_ids_is_denied_like_an_unknown_source() {
    let f = Fixture::new();
    // The attacker knows every exact identifier (e.g. from a leaked log).
    let denied = f.fetch(&f.public_reader.1, &f.private, Value::Null, 4096);
    let unknown = f.fetch(&f.public_reader.1, &fabricated(), Value::Null, 4096);
    assert_eq!(denied.status, 400);
    assert_eq!(opaque(&denied), opaque(&unknown));
    assert!(!text(&denied).contains("confidential"));
    // The rightful reader still succeeds with the same identifiers.
    let ok = f.fetch(&f.private_reader.1, &f.private, Value::Null, 4096);
    assert_eq!(ok.status, 200);
    assert!(text(&ok).contains("confidential"));
}

#[test]
fn credential_without_a_grant_reads_nothing_and_wrong_credentials_are_rejected() {
    let f = Fixture::new();
    // Valid Read capability is NOT authority over any collection.
    assert!(hits(&f.search(&f.ungranted.1, "meridian")).is_empty());
    let fetch = f.fetch(&f.ungranted.1, &f.public, Value::Null, 4096);
    assert_eq!(
        opaque(&fetch),
        opaque(&f.fetch(&f.ungranted.1, &fabricated(), Value::Null, 4096))
    );
    assert_eq!(fetch.status, 400);
    // Well-formed but unknown, and malformed, credentials.
    let unknown = format!("mpc_v1_{}", "0".repeat(64));
    assert_eq!(f.search(&unknown, "meridian").status, 401);
    assert_eq!(f.search("mpc_v1_short", "meridian").status, 401);
    // Another client's credential never carries this client's grant.
    assert!(hits(&f.search(&f.private_reader.1, PUBLIC_MARKER)).is_empty());
}

#[test]
fn cursors_do_not_carry_authority_across_clients_sources_or_revocation() {
    let f = Fixture::new();
    // Page one of the private revision with a tiny budget.
    let first = f.fetch(&f.private_reader.1, &f.private, Value::Null, 8);
    assert_eq!(first.status, 200);
    let result = first.body.unwrap()["result"]["structuredContent"].clone();
    assert_eq!(result["truncated"], true);
    let cursor = result["next_cursor"].clone();
    assert!(!cursor.is_null());
    // Cross-client reuse: the cursor is not a capability.
    let stolen = f.fetch(&f.public_reader.1, &f.private, cursor.clone(), 8);
    assert_eq!(
        opaque(&stolen),
        opaque(&f.fetch(&f.public_reader.1, &fabricated(), cursor.clone(), 8))
    );
    // Cross-source reuse by the owner of a different collection cannot reach
    // the private source through the public source's cursor either.
    let crossed = f.fetch(&f.public_reader.1, &f.private, cursor.clone(), 8);
    assert_eq!(crossed.status, 400);
    // Legitimate continuation works until the grant is revoked.
    assert_eq!(
        f.fetch(&f.private_reader.1, &f.private, cursor.clone(), 8)
            .status,
        200
    );
    f.owner()
        .revoke_collection_grant(&f.private_reader.0, &f.private_collection)
        .unwrap();
    let after = f.fetch(&f.private_reader.1, &f.private, cursor, 8);
    assert_eq!(after.status, 400);
    assert!(!text(&after).contains("confidential"));
}

#[test]
fn revocation_and_scope_changes_take_effect_on_the_next_request() {
    let f = Fixture::new();
    assert_eq!(hits(&f.search(&f.public_reader.1, "meridian")).len(), 1);
    // 1. per-collection grant revoke
    f.owner()
        .revoke_collection_grant(&f.public_reader.0, &f.public_collection)
        .unwrap();
    assert!(hits(&f.search(&f.public_reader.1, "meridian")).is_empty());
    // 2. re-grant restores exactly the same scope, nothing broader
    f.owner()
        .grant_collection(&f.public_reader.0, &f.public_collection)
        .unwrap();
    assert_eq!(hits(&f.search(&f.public_reader.1, "meridian")).len(), 1);
    // 3. shrinking membership
    f.owner()
        .set_collection_sources(&f.public_collection, &[])
        .unwrap();
    assert!(hits(&f.search(&f.public_reader.1, "meridian")).is_empty());
    f.owner()
        .set_collection_sources(&f.public_collection, std::slice::from_ref(&f.public.source))
        .unwrap();
    assert_eq!(hits(&f.search(&f.public_reader.1, "meridian")).len(), 1);
    // 4. deleting the collection removes it from every grant
    f.owner().delete_collection(&f.public_collection).unwrap();
    assert!(hits(&f.search(&f.public_reader.1, "meridian")).is_empty());
    assert!(f
        .state
        .client_collections(&f.public_reader.0)
        .unwrap()
        .is_empty());
    // 5. the Read capability itself
    f.owner()
        .set_capability(&f.private_reader.0, ClientCapability::Read, false)
        .unwrap();
    assert_eq!(f.search(&f.private_reader.1, "meridian").status, 401);
}

#[test]
fn rotation_disable_and_terminal_revoke_still_fail_closed_with_grants() {
    let f = Fixture::new();
    let rotated = f.owner().rotate(&f.public_reader.0).unwrap();
    assert_eq!(f.search(&f.public_reader.1, "meridian").status, 401);
    // The grant belongs to the client identity, so the new secret inherits it.
    assert_eq!(hits(&f.search(rotated.credential(), "meridian")).len(), 1);
    f.owner().set_enabled(&f.public_reader.0, false).unwrap();
    assert_eq!(f.search(rotated.credential(), "meridian").status, 401);
    f.owner().set_enabled(&f.public_reader.0, true).unwrap();
    f.owner().revoke(&f.public_reader.0).unwrap();
    assert_eq!(f.search(rotated.credential(), "meridian").status, 401);
    // A revoked client can no longer be granted anything.
    assert!(f
        .owner()
        .grant_collection(&f.public_reader.0, &f.private_collection)
        .is_err());
}

#[test]
fn lock_epoch_denies_then_unlock_re_resolves_the_same_scope() {
    let f = Fixture::new();
    assert_eq!(hits(&f.search(&f.public_reader.1, "meridian")).len(), 1);
    f.security.lock();
    let locked = f.search(&f.public_reader.1, "meridian");
    assert_ne!(locked.status, 200);
    assert!(!text(&locked).contains(PUBLIC_MARKER));
    f.security.unlock(SECRET).unwrap();
    // A fresh unlock epoch re-reads authority; scope is still only "public".
    let after = f.search(&f.public_reader.1, "meridian");
    let found = hits(&after);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0]["source_id"], f.public.source);
    assert!(!text(&after).contains(PRIVATE_MARKER));
}

#[test]
fn owner_grant_management_is_validated_bounded_and_not_reachable_by_readers() {
    let f = Fixture::new();
    let owner = f.owner();
    // Shape validation: only canonical source IDs may be members.
    for bad in ["", "src:v1:xyz", "../etc/passwd", &"a".repeat(71)] {
        assert!(owner.create_collection("Bad", &[bad.to_owned()]).is_err());
    }
    assert!(owner.create_collection("   ", &[]).is_err());
    // Grants require a real collection and a real, non-revoked client.
    assert!(owner
        .grant_collection(&f.public_reader.0, "collection:v1:deadbeef")
        .is_err());
    assert!(owner
        .grant_collection("client:v1:unknown", &f.public_collection)
        .is_err());
    // Duplicate grants are rejected rather than silently widening.
    assert!(owner
        .grant_collection(&f.public_reader.0, &f.public_collection)
        .is_err());
    // Read credentials expose no collection-management tool.
    let reply = f.call(&f.public_reader.1, "collection_grant", json!({}));
    assert_eq!(reply.status, 404);
    // Owner methods refuse while locked.
    f.security.lock();
    assert!(owner.create_collection("Locked", &[]).is_err());
    assert!(owner
        .grant_collection(&f.public_reader.0, &f.public_collection)
        .is_err());
}

#[test]
fn grant_changes_are_attributed_without_source_ids_or_secrets() {
    let f = Fixture::new();
    f.owner()
        .revoke_collection_grant(&f.public_reader.0, &f.public_collection)
        .unwrap();
    let audit = f.state.client_audit(&f.public_reader.0, 10).unwrap();
    assert_eq!(audit[0].event, "capability_revoked");
    assert_eq!(audit[1].event, "capability_granted");
    let serialized = serde_json::to_string(&audit).unwrap();
    assert!(!serialized.contains(&f.public.source));
    assert!(!serialized.contains("mpc_v1_"));
}

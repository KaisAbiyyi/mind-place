//! MP-7MCP-02 authoritative read-grant storage: deny by default, bounded,
//! persistent across reopen, fail-closed on damaged records.
use mind_place_state::{
    ClientCapability as Cap, StateError, StateStore, COLLECTION_ID_PREFIX, MAX_COLLECTIONS,
    MAX_COLLECTIONS_PER_CLIENT, MAX_SOURCES_PER_COLLECTION,
};
use rusqlite::Connection;

fn source(n: usize) -> String {
    format!("src:v1:{n:064x}")
}
fn collection(n: usize) -> String {
    format!("{COLLECTION_ID_PREFIX}{n:032x}")
}
const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn store() -> (tempfile::TempDir, StateStore, String) {
    let dir = tempfile::tempdir().unwrap();
    let state = StateStore::open(dir.path().join("state.sqlite3")).unwrap();
    let client = "client:v1:reader".to_string();
    state
        .issue_client(&client, "Reader", HASH, &[Cap::Read])
        .unwrap();
    (dir, state, client)
}

#[test]
fn authorized_client_without_grants_resolves_to_an_empty_scope() {
    let (_dir, state, client) = store();
    let scope = state.client_read_scope(HASH).unwrap().unwrap();
    assert_eq!(scope.client_id, client);
    assert!(scope.sources.is_empty());
    // Unknown credential hash: not authorized at all.
    assert!(state.client_read_scope(&"b".repeat(64)).unwrap().is_none());
}

#[test]
fn scope_is_the_union_of_granted_collections_and_survives_reopen() {
    let (dir, state, client) = store();
    state
        .create_collection(&collection(1), "One", &[source(1), source(2), source(1)])
        .unwrap();
    state
        .create_collection(&collection(2), "Two", &[source(3)])
        .unwrap();
    state
        .create_collection(&collection(3), "Never granted", &[source(9)])
        .unwrap();
    state.grant_collection(&client, &collection(1)).unwrap();
    state.grant_collection(&client, &collection(2)).unwrap();
    let reopened = StateStore::open(dir.path().join("state.sqlite3")).unwrap();
    let scope = reopened.client_read_scope(HASH).unwrap().unwrap();
    assert_eq!(
        scope.sources.into_iter().collect::<Vec<_>>(),
        vec![source(1), source(2), source(3)]
    );
    let listed = reopened.list_collections().unwrap();
    assert_eq!(listed.len(), 3);
    assert_eq!(listed[0].source_count, 2); // duplicates collapsed
}

#[test]
fn every_authority_change_moves_the_policy_revision_fence() {
    let (_dir, state, client) = store();
    let mut last = state.policy_revision().unwrap();
    let mut step = |label: &str, state: &StateStore| {
        let now = state.policy_revision().unwrap();
        assert!(now > last, "{label}");
        last = now;
    };
    state
        .create_collection(&collection(1), "One", &[source(1)])
        .unwrap();
    step("create", &state);
    state.grant_collection(&client, &collection(1)).unwrap();
    step("grant", &state);
    state
        .set_collection_sources(&collection(1), &[source(1), source(2)])
        .unwrap();
    step("grow", &state);
    state
        .set_collection_sources(&collection(1), &[source(2)])
        .unwrap();
    step("shrink", &state);
    state
        .revoke_collection_grant(&client, &collection(1))
        .unwrap();
    step("revoke", &state);
    state.delete_collection(&collection(1)).unwrap();
    step("delete", &state);
}

#[test]
fn identifiers_and_sizes_are_validated_and_bounded() {
    let (_dir, state, client) = store();
    assert!(state.create_collection("bad-id", "X", &[]).is_err());
    assert!(state
        .create_collection(&collection(1), "X", &["src:v1:nothex".into()])
        .is_err());
    let too_many: Vec<String> = (0..=MAX_SOURCES_PER_COLLECTION).map(source).collect();
    assert!(state
        .create_collection(&collection(1), "X", &too_many)
        .is_err());
    state
        .create_collection(&collection(1), "X", &[source(1)])
        .unwrap();
    assert!(matches!(
        state.create_collection(&collection(1), "Duplicate", &[]),
        Err(StateError::InvalidValue)
    ));
    for n in 2..=MAX_COLLECTIONS {
        state.create_collection(&collection(n), "C", &[]).unwrap();
    }
    assert!(state
        .create_collection(&collection(MAX_COLLECTIONS + 1), "Over", &[])
        .is_err());
    for n in 1..=MAX_COLLECTIONS_PER_CLIENT {
        state.grant_collection(&client, &collection(n)).unwrap();
    }
    assert!(state
        .grant_collection(&client, &collection(MAX_COLLECTIONS_PER_CLIENT + 1))
        .is_err());
}

#[test]
fn a_damaged_grant_record_fails_closed_instead_of_widening() {
    let (dir, state, client) = store();
    state
        .create_collection(&collection(1), "One", &[source(1)])
        .unwrap();
    state.grant_collection(&client, &collection(1)).unwrap();
    let db = Connection::open(dir.path().join("state.sqlite3")).unwrap();
    db.execute(
        "UPDATE settings SET value_json='{\"collections\":\"not-a-list\"}' WHERE key GLOB 'mcp.grants.v1:*'",
        [],
    )
    .unwrap();
    drop(db);
    assert!(matches!(
        state.client_read_scope(HASH),
        Err(StateError::Corrupt)
    ));
}

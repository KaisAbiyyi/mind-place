use mind_place_state::{
    ClientCapability as Cap, StateError, StateStore, APPLICATION_ID, SCHEMA_VERSION,
};
use rusqlite::Connection;
use sha2::{Digest, Sha256};

#[test]
fn v2_upgrade_creates_backed_up_credential_migration_without_trusting_old_clients() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state.sqlite3");
    let db = Connection::open(&path).unwrap();
    db.pragma_update(None, "application_id", APPLICATION_ID)
        .unwrap();
    for (number, sql) in [
        (1, include_str!("../src/schema_v1.sql")),
        (2, include_str!("../src/schema_v2.sql")),
    ] {
        db.execute_batch(sql).unwrap();
        let hash = format!("{:x}", Sha256::digest(sql.as_bytes()));
        db.execute(
            "INSERT INTO schema_migrations(version,checksum) VALUES(?1,?2)",
            rusqlite::params![number, hash],
        )
        .unwrap();
    }
    db.execute_batch(
        "INSERT INTO clients(id,display_name,state) VALUES('client:legacy','Legacy','enabled');
         INSERT INTO client_capabilities(client_id,capability) VALUES('client:legacy','read');",
    )
    .unwrap();
    db.pragma_update(None, "user_version", 2).unwrap();
    drop(db);
    let upgraded = StateStore::open(&path).unwrap();
    assert_eq!(SCHEMA_VERSION, 5);
    let no_token = format!("{:x}", Sha256::digest(b"mpc_v1_unsafe"));
    assert_eq!(
        upgraded
            .authorize_client_hash(&no_token, Cap::Read)
            .unwrap(),
        None
    );
    assert_eq!(
        std::fs::read_dir(temp.path().join("state-backups"))
            .unwrap()
            .count(),
        1
    );
    assert_eq!(
        upgraded.client("client:legacy").unwrap().unwrap().state,
        "enabled"
    );
    assert!(StateStore::open(&path).is_ok());
}

#[test]
fn client_audit_cannot_be_changed_and_bad_migration_checksum_fails_closed() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state.sqlite3");
    let s = StateStore::open(&path).unwrap();
    s.issue_client("client:v1:test", "Agent", &"a".repeat(64), &[Cap::Read])
        .unwrap();
    let raw = Connection::open(&path).unwrap();
    assert!(raw
        .execute("UPDATE client_audit SET event='read_completed'", [])
        .is_err());
    assert!(raw.execute("DELETE FROM client_audit", []).is_err());
    raw.execute(
        "UPDATE schema_migrations SET checksum='fake' WHERE version=3",
        [],
    )
    .unwrap();
    drop(raw);
    assert!(matches!(StateStore::open(&path), Err(StateError::Corrupt)));
}

#[test]
fn credentialed_inventory_is_paginated_and_excludes_unprovisioned_legacy_rows() {
    let temp = tempfile::tempdir().unwrap();
    let store = StateStore::open(temp.path().join("state.sqlite3")).unwrap();
    for (id, hash) in [
        ("client:v1:a", "a".repeat(64)),
        ("client:v1:b", "b".repeat(64)),
        ("client:v1:c", "c".repeat(64)),
    ] {
        store.issue_client(id, id, &hash, &[Cap::Read]).unwrap();
    }
    let raw = Connection::open(store.path()).unwrap();
    raw.execute(
        "INSERT INTO clients(id,display_name,state) VALUES ('client:v1:legacy','Legacy','enabled')",
        [],
    )
    .unwrap();
    let first = store.list_credentialed_clients(None, 2).unwrap();
    assert_eq!(first.len(), 2);
    assert_eq!(first[0].id, "client:v1:a");
    assert_eq!(first[1].id, "client:v1:b");
    let next = store
        .list_credentialed_clients(Some(&first[1].id), 2)
        .unwrap();
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].id, "client:v1:c");
    store.revoke_client(&next[0].id).unwrap();
    assert_eq!(
        store
            .list_credentialed_clients(Some(&first[1].id), 2)
            .unwrap()[0]
            .state,
        "revoked"
    );
    assert!(store.list_credentialed_clients(None, 0).is_err());
    assert!(store.list_credentialed_clients(None, 51).is_err());
    assert!(store.list_credentialed_clients(Some("!"), 10).is_err());
}

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
    assert_eq!(SCHEMA_VERSION, 3);
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

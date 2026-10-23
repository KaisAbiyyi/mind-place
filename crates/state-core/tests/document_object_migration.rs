use mind_place_state::{StateStore, APPLICATION_ID, SCHEMA_VERSION};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::fs;
fn v5(path: &std::path::Path) {
    let db = Connection::open(path).unwrap();
    db.pragma_update(None, "application_id", APPLICATION_ID)
        .unwrap();
    for (version, sql) in [
        (1, include_str!("../src/schema_v1.sql")),
        (2, include_str!("../src/schema_v2.sql")),
        (3, include_str!("../src/schema_v3.sql")),
        (4, include_str!("../src/schema_v4.sql")),
        (5, include_str!("../src/schema_v5.sql")),
    ] {
        db.execute_batch(sql).unwrap();
        db.execute(
            "INSERT INTO schema_migrations(version,checksum) VALUES(?1,?2)",
            rusqlite::params![version, format!("{:x}", Sha256::digest(sql))],
        )
        .unwrap();
    }
    db.pragma_update(None, "user_version", 5).unwrap();
    db.execute(
        "INSERT INTO projects(id,name) VALUES('project:existing','retained')",
        [],
    )
    .unwrap();
    db.execute("INSERT INTO source_roots(id,canonical_path,approval_state) VALUES('legacy','C:/legacy','pending')",[]).unwrap();
}
#[test]
fn schema_v5_upgrade_backs_up_authority_and_never_promotes_roots() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.sqlite3");
    v5(&path);
    assert_eq!(SCHEMA_VERSION, 8);
    let state = StateStore::open(&path).unwrap();
    assert_eq!(
        state.get_project("project:existing").unwrap().unwrap().name,
        "retained"
    );
    assert_eq!(
        state.root("legacy").unwrap().unwrap().approval_state,
        "pending"
    );
    let backups: Vec<_> = fs::read_dir(dir.path().join("state-backups"))
        .unwrap()
        .collect();
    assert_eq!(backups.len(), 1);
    let backup = Connection::open(backups[0].as_ref().unwrap().path()).unwrap();
    assert_eq!(
        backup
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        5
    );
    assert_eq!(
        backup
            .query_row("SELECT name FROM projects", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "retained"
    );
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM document_objects", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert!(state.list_document_roots().unwrap().is_empty());
    drop(state);
    StateStore::open(&path).unwrap();
    assert_eq!(
        fs::read_dir(dir.path().join("state-backups"))
            .unwrap()
            .count(),
        1
    );
}
#[test]
fn failed_v6_upgrade_rolls_back_and_preserves_snapshot_and_original_authority() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.sqlite3");
    v5(&path);
    let db = Connection::open(&path).unwrap();
    // Simulate an unexpected table from foreign/manual writes, not an upgrade to adopt.
    db.execute_batch("CREATE TABLE document_versions(sentinel TEXT); INSERT INTO document_versions VALUES('evidence');").unwrap();
    drop(db);
    assert!(StateStore::open(&path).is_err());
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        5
    );
    assert_eq!(
        db.query_row("SELECT sentinel FROM document_versions", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "evidence"
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='document_objects'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM schema_migrations WHERE version=6",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        fs::read_dir(dir.path().join("state-backups"))
            .unwrap()
            .count(),
        1
    );
}

use mind_place_state::{MemoryClaim, StateError, StateStore, APPLICATION_ID};
use rusqlite::{Connection, OpenFlags};
use std::fs;
fn fresh() -> (tempfile::TempDir, StateStore) {
    let d = tempfile::tempdir().unwrap();
    let s = StateStore::open(d.path().join("state.sqlite3")).unwrap();
    (d, s)
}
#[test]
fn bundled_sqlite_version_is_supported() {
    println!("Bundled Mind Place SQLite: {}", rusqlite::version());
    assert!(rusqlite::version_number() >= 3_053_002);
}
#[test]
fn newly_created_database_is_durable_and_reopenable() {
    let (_dir, db) = fresh();
    db.create_project("p:1", "Mind Place").unwrap();
    db.create_memory_claim(&MemoryClaim {
        id: "m:1".into(),
        statement: "Shared brain".into(),
        asserted_by: "user".into(),
        epistemic_origin: "explicit".into(),
        status: "active".into(),
    })
    .unwrap();
    db.add_pending_root("r:1", "C:/private").unwrap();
    assert_eq!(db.root("r:1").unwrap().unwrap().approval_state, "pending");
    db.revoke_root("r:1").unwrap();
    let s = StateStore::open(db.path()).unwrap();
    assert_eq!(s.get_project("p:1").unwrap().unwrap().name, "Mind Place");
    assert_eq!(s.root("r:1").unwrap().unwrap().approval_state, "revoked");
    let raw = Connection::open(db.path()).unwrap();
    let journal: String = raw
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(journal, "wal");
    let count: i64 = raw
        .query_row("SELECT count(*) FROM root_tombstones", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
    assert!(raw
        .execute("UPDATE memory_claim_events SET event_type='fake'", [])
        .is_err());
    assert!(raw.execute("DELETE FROM memory_claim_events", []).is_err());
    assert!(raw
        .execute("UPDATE root_tombstones SET reason='forged'", [])
        .is_err());
    assert!(raw.execute("DELETE FROM root_tombstones", []).is_err());
}
#[test]
fn never_overwrite_unknown_or_future_schema() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("state.sqlite3");
    let db = Connection::open(&p).unwrap();
    db.execute_batch("CREATE TABLE evidence(x TEXT);INSERT INTO evidence VALUES('persist');")
        .unwrap();
    drop(db);
    assert!(matches!(
        StateStore::open(&p),
        Err(StateError::ForeignDatabase)
    ));
    let db = Connection::open(&p).unwrap();
    assert_eq!(
        db.query_row("SELECT x FROM evidence", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "persist"
    );
    db.pragma_update(None, "application_id", APPLICATION_ID)
        .unwrap();
    db.pragma_update(None, "user_version", 8).unwrap();
    drop(db);
    assert!(matches!(
        StateStore::open(&p),
        Err(StateError::FutureSchema(8))
    ));
}
#[test]
fn upgrades_create_verified_sqlite_backup_before_write() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("state.sqlite3");
    let db = Connection::open(&p).unwrap();
    db.pragma_update(None, "application_id", APPLICATION_ID)
        .unwrap();
    db.pragma_update(None, "journal_mode", "WAL").unwrap();
    db.execute_batch(
        "CREATE TABLE old_evidence(x TEXT); INSERT INTO old_evidence VALUES('preserved');",
    )
    .unwrap();
    // Keep the original connection alive: backup must include committed WAL frames.
    StateStore::open(&p).unwrap();
    drop(db);
    let backups: Vec<_> = fs::read_dir(d.path().join("state-backups"))
        .unwrap()
        .collect();
    assert_eq!(backups.len(), 1);
    let backup = Connection::open_with_flags(
        backups[0].as_ref().unwrap().path(),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    assert_eq!(
        backup
            .query_row("SELECT x FROM old_evidence", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "preserved"
    );
    assert_eq!(
        backup
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}
#[test]
fn unknown_checksum_fails_closed_and_invalid_mutations_do_not_commit() {
    let (_dir, s) = fresh();
    let db = Connection::open(s.path()).unwrap();
    db.execute("UPDATE schema_migrations SET checksum='tampered'", [])
        .unwrap();
    drop(db);
    assert!(matches!(
        StateStore::open(s.path()),
        Err(StateError::Corrupt)
    ));
    let (_dir, s) = fresh();
    assert!(matches!(
        s.create_project("bad id", "x"),
        Err(StateError::InvalidValue)
    ));
    assert!(matches!(
        s.create_project("ok", "   "),
        Err(StateError::InvalidValue)
    ));
    assert!(matches!(
        s.create_memory_claim(&MemoryClaim {
            id: "m:1".into(),
            statement: "x".into(),
            asserted_by: "model".into(),
            epistemic_origin: "explicit".into(),
            status: "invented".into()
        }),
        Err(StateError::InvalidValue)
    ));
}
#[test]
fn failed_upgrade_preserves_old_data_and_backup() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("state.sqlite3");
    let db = Connection::open(&p).unwrap();
    db.pragma_update(None, "application_id", APPLICATION_ID)
        .unwrap();
    db.execute_batch(
        "CREATE TABLE projects(id INTEGER PRIMARY KEY);INSERT INTO projects VALUES(12);",
    )
    .unwrap();
    drop(db);
    assert!(StateStore::open(&p).is_err());
    let db = Connection::open(&p).unwrap();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("SELECT id FROM projects", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        12
    );
    assert_eq!(
        fs::read_dir(d.path().join("state-backups"))
            .unwrap()
            .count(),
        1
    );
}
#[cfg(unix)]
#[test]
fn rejects_symlinked_state() {
    let (_d, s) = fresh();
    let d = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(s.path(), d.path().join("state.sqlite3")).unwrap();
    assert!(matches!(
        StateStore::open(d.path().join("state.sqlite3")),
        Err(StateError::UnsafePath)
    ));
}

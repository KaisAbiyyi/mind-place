use mind_place_state::{
    canonical_source_id, MemoryClaim, SourceDescriptor, SourceKey, StateError, StateStore,
    APPLICATION_ID, SCHEMA_VERSION,
};
use rusqlite::{params, Connection, OpenFlags};
use sha2::{Digest, Sha256};
use std::fs;

fn descriptor<'a>(key: &'a str) -> SourceDescriptor<'a> {
    SourceDescriptor {
        provider: "chatgpt",
        source_namespace: "account-one",
        source_kind: "conversation",
        key: SourceKey::External(key),
    }
}

fn v1_database(path: &std::path::Path) {
    let db = Connection::open(path).unwrap();
    db.pragma_update(None, "application_id", APPLICATION_ID)
        .unwrap();
    db.execute_batch(include_str!("../src/schema_v1.sql"))
        .unwrap();
    let checksum = format!(
        "{:x}",
        Sha256::digest(include_str!("../src/schema_v1.sql").as_bytes())
    );
    db.execute(
        "INSERT INTO schema_migrations(version,checksum) VALUES(1,?1)",
        [&checksum],
    )
    .unwrap();
    db.pragma_update(None, "user_version", 1).unwrap();
}

#[test]
fn durable_source_ids_are_stable_without_the_derived_index() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.sqlite3");
    let state = StateStore::open(&path).unwrap();
    let key = descriptor("provider-message-key");
    let expected = canonical_source_id(&key).unwrap();
    assert_eq!(
        expected,
        "src:v1:d887eb7cbb0f22c685a2701430ed6abc3270e40fd781f42a677a339f0b020788"
    );
    assert_eq!(state.register_source(&key).unwrap(), expected);
    assert_eq!(state.register_source(&key).unwrap(), expected);
    assert_eq!(
        state.source(&expected).unwrap().unwrap().source_key,
        "provider-message-key"
    );
    let raw = Connection::open(&path).unwrap();
    let count: i64 = raw
        .query_row("SELECT COUNT(*) FROM source_identities", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
    drop(raw);
    // An index ID can change on rebuild without altering any canonical ID.
    let temporary_index = directory.path().join("index.sqlite3");
    fs::write(&temporary_index, b"disposable index fixture").unwrap();
    fs::remove_file(&temporary_index).unwrap();
    let reopened = StateStore::open(&path).unwrap();
    assert_eq!(reopened.register_source(&key).unwrap(), expected);
    assert_eq!(reopened.source(&expected).unwrap().unwrap().id, expected);
    assert_eq!(reopened.source("index:temporary-42").unwrap(), None);
}

#[test]
fn evidence_references_require_registered_authority_source() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.sqlite3");
    let state = StateStore::open(&path).unwrap();
    state
        .create_memory_claim(&MemoryClaim {
            id: "claim:1".into(),
            statement: "One brain".into(),
            asserted_by: "user".into(),
            epistemic_origin: "explicit".into(),
            status: "active".into(),
        })
        .unwrap();
    let source_id = state
        .register_source(&descriptor("conversation-1"))
        .unwrap();
    state
        .add_claim_source_evidence("claim:1", &source_id, "support")
        .unwrap();
    assert!(matches!(
        state.add_claim_source_evidence("claim:1", "index:row-42", "support"),
        Err(StateError::InvalidValue)
    ));
    let raw = Connection::open(&path).unwrap();
    raw.pragma_update(None, "foreign_keys", "ON").unwrap();
    assert!(raw
        .execute(
            "INSERT INTO claim_evidence(claim_id,source_ref,evidence_kind)
             VALUES(?1,?2,'support')",
            params!["claim:1", "index:row-42"],
        )
        .is_err());
    assert!(raw
        .execute("DELETE FROM source_identities WHERE id=?1", [&source_id])
        .is_err());
    let reopened = StateStore::open(&path).unwrap();
    assert_eq!(
        reopened.source(&source_id).unwrap().unwrap().provider,
        "chatgpt"
    );
}

#[test]
fn v1_upgrade_keeps_projects_and_creates_verified_v1_backup() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.sqlite3");
    v1_database(&path);
    let db = Connection::open(&path).unwrap();
    db.execute(
        "INSERT INTO projects(id,name) VALUES('project:old','Preserved')",
        [],
    )
    .unwrap();
    drop(db);
    let upgraded = StateStore::open(&path).unwrap();
    assert_eq!(SCHEMA_VERSION, 6);
    assert_eq!(
        upgraded.get_project("project:old").unwrap().unwrap().name,
        "Preserved"
    );
    let backup_paths: Vec<_> = fs::read_dir(directory.path().join("state-backups"))
        .unwrap()
        .map(|x| x.unwrap().path())
        .collect();
    assert_eq!(backup_paths.len(), 1);
    let snapshot =
        Connection::open_with_flags(&backup_paths[0], OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let version: i64 = snapshot
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 1);
    let original: String = snapshot
        .query_row(
            "SELECT name FROM projects WHERE id='project:old'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(original, "Preserved");
    drop(snapshot);
    StateStore::open(&path).unwrap();
    assert_eq!(
        fs::read_dir(directory.path().join("state-backups"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn v1_orphan_reference_fails_migration_without_losing_evidence() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.sqlite3");
    v1_database(&path);
    let db = Connection::open(&path).unwrap();
    db.execute_batch(
        "INSERT INTO memory_claims(id,statement,asserted_by,epistemic_origin,status)
         VALUES('claim:old','Statement','user','explicit','active');
         INSERT INTO claim_evidence(claim_id,source_ref,evidence_kind)
         VALUES('claim:old','index:transient-123','support');",
    )
    .unwrap();
    drop(db);
    assert!(StateStore::open(&path).is_err());
    let raw = Connection::open(&path).unwrap();
    let version: i64 = raw
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 1);
    let source_ref: String = raw
        .query_row("SELECT source_ref FROM claim_evidence", [], |r| r.get(0))
        .unwrap();
    assert_eq!(source_ref, "index:transient-123");
    assert_eq!(
        fs::read_dir(directory.path().join("state-backups"))
            .unwrap()
            .count(),
        1
    );
    let old: String = raw
        .query_row(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='source_identities'",
            [],
            |r| r.get(0),
        )
        .unwrap_or_default();
    assert!(old.is_empty());
}

#[test]
fn v1_bad_migration_checksum_refuses_upgrade_without_backup() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.sqlite3");
    v1_database(&path);
    let db = Connection::open(&path).unwrap();
    db.execute("UPDATE schema_migrations SET checksum='tampered'", [])
        .unwrap();
    drop(db);
    assert!(matches!(StateStore::open(&path), Err(StateError::Corrupt)));
    assert!(!directory.path().join("state-backups").exists());
}

#[test]
fn modified_authority_identity_fails_closed_on_read_and_restart() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.sqlite3");
    let state = StateStore::open(&path).unwrap();
    let id = state.register_source(&descriptor("one")).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute(
        "UPDATE source_identities SET source_key='two' WHERE id=?1",
        [&id],
    )
    .unwrap();
    drop(db);
    assert!(matches!(state.source(&id), Err(StateError::Corrupt)));
    assert!(matches!(StateStore::open(&path), Err(StateError::Corrupt)));
}

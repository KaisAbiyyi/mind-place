use mind_place_state::{StateError, StateStore, VirtualDocumentPath, SCHEMA_VERSION};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

fn store() -> (tempfile::TempDir, StateStore) {
    let home = tempfile::tempdir().unwrap();
    let db = StateStore::open(home.path().join("state.sqlite3")).unwrap();
    (home, db)
}

#[test]
fn namespace_round_trip_and_strict_rejection() {
    let root = format!("root:v1:{}", "a".repeat(64));
    let p = VirtualDocumentPath::from_segments(
        &root,
        &[
            "notes".into(),
            "résumé 2026.md".into(),
            "file_name-3.rs".into(),
        ],
    )
    .unwrap();
    let uri = p.to_uri();
    assert_eq!(
        uri,
        format!("mpdoc://root/{root}/notes/r%C3%A9sum%C3%A9%202026.md/file_name-3.rs")
    );
    assert_eq!(VirtualDocumentPath::parse(&uri).unwrap(), p);
    for invalid in [
        "".to_string(),
        "file:///C:/Users/me/notes".into(),
        format!("mpdoc://root/{root}/../secret"),
        format!("mpdoc://root/{root}/."),
        format!("mpdoc://root/{root}/some//thing"),
        format!("mpdoc://root/{root}/file/"),
        format!("mpdoc://root/{root}/%2E%2E"),
        format!("mpdoc://root/{root}/%2Fetc"),
        format!("mpdoc://root/{root}/%5Cwindows"),
        format!("mpdoc://root/{root}/C:"),
        format!("mpdoc://root/{root}/%252E%252E"),
        format!("mpdoc://root/{root}/a%2eb"),
        format!("mpdoc://root/{root}/a%2fb"),
        format!("mpdoc://root/{root}/%GG"),
        format!("mpdoc://root/{root}/%C0%AF"),
        format!("mpdoc://root/{root}/CON.txt"),
        format!("mpdoc://root/{root}/folder."),
        format!("mpdoc://root/{root}/file%25.txt"),
        format!("mpdoc://root/ROOT:v1:{}/foo", "a".repeat(64)),
        format!("mpdoc://root/{root}/{}", "x".repeat(4096)),
    ] {
        assert!(
            VirtualDocumentPath::parse(&invalid).is_err(),
            "accepted {invalid:?}"
        );
    }
    for bad in [
        "..", ".", "A/B", "A\\B", "X:Y", "NUL", "COM1.txt", "LPT9", "name%", "file.",
    ] {
        assert!(
            VirtualDocumentPath::from_segments(&root, &[bad.into()]).is_err(),
            "{bad}"
        );
    }
}

#[test]
fn root_staging_is_idempotent_pending_only_and_revocation_is_terminal() {
    let (home, state) = store();
    let folder = home.path().join("docs");
    fs::create_dir(&folder).unwrap();
    let first = state.register_document_root(&folder).unwrap();
    assert_eq!(first.approval_state, "pending");
    assert!(first.id.starts_with("root:v1:"));
    assert!(first.virtual_prefix.ends_with(&first.id));
    assert_eq!(state.register_document_root(&folder).unwrap(), first);
    assert_eq!(state.list_document_roots().unwrap(), vec![first.clone()]);
    let uri = VirtualDocumentPath::from_segments(&first.id, &["hello.md".into()])
        .unwrap()
        .to_uri();
    assert!(state.require_approved_document_uri(&uri).is_err());
    let db = Connection::open(state.path()).unwrap();
    // Simulate future MP-4B *explicit owner grant* only for the lookup contract.
    db.execute(
        "UPDATE source_roots SET approval_state='approved', approved_at=unixepoch() WHERE id=?1",
        [&first.id],
    )
    .unwrap();
    assert!(state.require_approved_document_uri(&uri).is_ok());
    state.revoke_root(&first.id).unwrap();
    assert!(state.require_approved_document_uri(&uri).is_err());
    assert!(state.register_document_root(&folder).is_err());
    assert_eq!(
        state.list_document_roots().unwrap()[0].approval_state,
        "revoked"
    );
    let count: i64 = db
        .query_row(
            "SELECT count(*) FROM root_tombstones WHERE root_id=?1",
            [&first.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
    assert!(db
        .execute(
            "UPDATE document_root_registry SET path_digest=?1 WHERE root_id=?2",
            rusqlite::params!["f".repeat(64), first.id]
        )
        .is_err());
    assert!(db
        .execute(
            "DELETE FROM document_root_registry WHERE root_id=?1",
            [&first.id]
        )
        .is_err());
    drop(db);
    assert_eq!(
        StateStore::open(state.path())
            .unwrap()
            .document_root(&first.id)
            .unwrap()
            .unwrap()
            .approval_state,
        "revoked"
    );
}

#[test]
fn rejects_non_directory_missing_relative_and_symlinked_paths() {
    let (home, state) = store();
    assert!(state.register_document_root(Path::new("relative")).is_err());
    assert!(state
        .register_document_root(&home.path().join("missing"))
        .is_err());
    let file = home.path().join("file.md");
    fs::write(&file, "no access").unwrap();
    assert!(matches!(
        state.register_document_root(&file),
        Err(StateError::InvalidValue)
    ));
    #[cfg(unix)]
    {
        let folder = home.path().join("docs");
        fs::create_dir(&folder).unwrap();
        let alias = home.path().join("alias");
        std::os::unix::fs::symlink(&folder, &alias).unwrap();
        assert!(matches!(
            state.register_document_root(&alias),
            Err(StateError::UnsafePath)
        ));
    }
}

#[test]
fn schema_v3_migrates_without_adopting_historical_approved_roots() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("state.sqlite3");
    let db = Connection::open(&path).unwrap();
    db.pragma_update(None, "application_id", mind_place_state::APPLICATION_ID)
        .unwrap();
    for (version, sql) in [
        (1, include_str!("../src/schema_v1.sql")),
        (2, include_str!("../src/schema_v2.sql")),
        (3, include_str!("../src/schema_v3.sql")),
    ] {
        db.execute_batch(sql).unwrap();
        let checksum = format!("{:x}", Sha256::digest(sql.as_bytes()));
        db.execute(
            "INSERT INTO schema_migrations(version,checksum) VALUES(?1,?2)",
            rusqlite::params![version, checksum],
        )
        .unwrap();
    }
    db.execute("INSERT INTO source_roots(id,canonical_path,approval_state,approved_at) VALUES('legacy','/legacy/secret','approved',unixepoch())", []).unwrap();
    db.pragma_update(None, "user_version", 3).unwrap();
    drop(db);
    assert_eq!(SCHEMA_VERSION, 4);
    let state = StateStore::open(&path).unwrap();
    assert_eq!(
        state.root("legacy").unwrap().unwrap().approval_state,
        "approved"
    );
    assert!(state.list_document_roots().unwrap().is_empty());
    let sql = Connection::open(state.path()).unwrap();
    let version: i64 = sql
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 4);
    assert_eq!(
        fs::read_dir(home.path().join("state-backups"))
            .unwrap()
            .count(),
        1
    );
}

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
fn fresh_read_checks_preserve_corruption_and_revocation_guards() {
    let (home, state) = store();
    let folder = home.path().join("approved");
    fs::create_dir(&folder).unwrap();
    let id = state.register_document_root(&folder).unwrap().id;
    let uri = VirtualDocumentPath::from_segments(&id, &["note.md".into()])
        .unwrap()
        .to_uri();
    assert!(state.require_approved_document_uri(&uri).is_err());
    state.approve_document_root(&id).unwrap();
    assert!(state.require_approved_document_uri(&uri).is_ok());
    // A different connection commits changes between polls; no prior snapshot
    // or grant may hide them. Closed handles also exercise WAL sidecar reopen.
    {
        let db = Connection::open(state.path()).unwrap();
        db.execute_batch("DROP TRIGGER document_root_authorizations_no_update;")
            .unwrap();
        db.execute(
            "UPDATE document_root_authorizations SET volume_id='invalid' WHERE root_id=?1",
            [&id],
        )
        .unwrap();
    }
    assert!(matches!(
        state.require_approved_document_uri(&uri),
        Err(StateError::Corrupt)
    ));
    {
        let db = Connection::open(state.path()).unwrap();
        db.execute(
            "UPDATE document_root_authorizations SET volume_id='1' WHERE root_id=?1",
            [&id],
        )
        .unwrap();
    }
    assert!(state.require_approved_document_uri(&uri).is_ok());
    state.revoke_document_root(&id).unwrap();
    assert!(state.require_approved_document_uri(&uri).is_err());
    {
        let db = Connection::open(state.path()).unwrap();
        db.execute_batch("DROP TRIGGER document_root_registry_no_update;")
            .unwrap();
        db.execute(
            "UPDATE document_root_registry SET path_digest=lower(hex(zeroblob(32))) WHERE root_id=?1",
            [&id],
        )
        .unwrap();
    }
    assert!(matches!(
        state.require_approved_document_uri(&uri),
        Err(StateError::Corrupt)
    ));
}

#[cfg(windows)]
#[test]
fn finished_revocation_cannot_overtake_an_active_publication_fence() {
    use std::{sync::mpsc, time::Duration};

    let (home, state) = store();
    let folder = home.path().join("approved");
    fs::create_dir(&folder).unwrap();
    let id = state.register_document_root(&folder).unwrap().id;
    state.approve_document_root(&id).unwrap();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let publisher_store = state.clone();
    let publisher_id = id.clone();
    let publisher = std::thread::spawn(move || {
        publisher_store.with_approved_document_roots(
            &[publisher_id],
            || -> Result<(), StateError> {
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                Ok(())
            },
        )
    });
    entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();

    let (started_tx, started_rx) = mpsc::channel();
    let (revoked_tx, revoked_rx) = mpsc::channel();
    let revoke_state = state.clone();
    let revoke_id = id.clone();
    let revoker = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        revoke_state.revoke_document_root(&revoke_id).unwrap();
        revoked_tx.send(()).unwrap();
    });
    started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(
        revoked_rx.recv_timeout(Duration::from_millis(150)).is_err(),
        "revocation must not complete while a publication fence holds authority"
    );
    release_tx.send(()).unwrap();
    publisher.join().unwrap().unwrap();
    revoked_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    revoker.join().unwrap();
    assert!(!state.document_root(&id).unwrap().unwrap().is_approved());
    assert!(state
        .with_approved_document_roots(&[id], || Ok::<_, StateError>(()))
        .is_err());
}

#[cfg(windows)]
#[test]
fn ntfs_root_replacement_cannot_overtake_a_publication_fence() {
    use std::{sync::mpsc, time::Duration};

    let (home, state) = store();
    let folder = home.path().join("approved");
    let moved = home.path().join("moved");
    fs::create_dir(&folder).unwrap();
    let id = state.register_document_root(&folder).unwrap().id;
    state.approve_document_root(&id).unwrap();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let worker_state = state.clone();
    let worker_id = id.clone();
    let publication = std::thread::spawn(move || {
        worker_state.with_approved_document_roots(&[worker_id], || -> Result<(), StateError> {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            Ok(())
        })
    });
    entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    // Windows must reject a rename while the short-lived no-delete-share
    // native root handle is held by a live publication.
    assert!(
        fs::rename(&folder, &moved).is_err(),
        "approved root was replaced during fenced publication"
    );
    release_tx.send(()).unwrap();
    publication.join().unwrap().unwrap();

    fs::rename(&folder, &moved).unwrap();
    fs::create_dir(&folder).unwrap();
    assert!(
        state
            .with_approved_document_roots(&[id], || Ok::<_, StateError>(()))
            .is_err(),
        "replaced NTFS directory must not inherit authorization"
    );
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
    assert_eq!(
        state
            .approve_document_root(&first.id)
            .unwrap()
            .approval_state,
        "approved"
    );
    assert!(state.approve_document_root(&first.id).is_err());
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
    assert_eq!(SCHEMA_VERSION, 6);
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
    assert_eq!(version, SCHEMA_VERSION);
    assert_eq!(
        fs::read_dir(home.path().join("state-backups"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn forged_sql_approval_without_capability_does_not_grant_access() {
    let (home, state) = store();
    let folder = home.path().join("documents");
    fs::create_dir(&folder).unwrap();
    let root = state.register_document_root(&folder).unwrap();
    let uri = VirtualDocumentPath::from_segments(&root.id, &["secret.txt".into()])
        .unwrap()
        .to_uri();
    let db = Connection::open(state.path()).unwrap();
    db.execute(
        "UPDATE source_roots SET approval_state='approved',approved_at=unixepoch() WHERE id=?1",
        [&root.id],
    )
    .unwrap();
    assert!(state.require_approved_document_uri(&uri).is_err());
    assert!(state.approve_document_root(&root.id).is_err());
}

#[test]
fn approval_is_identity_bound_and_revocation_is_terminal() {
    let (home, state) = store();
    let folder = home.path().join("approved");
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("note.md"), "authoritative").unwrap();
    let root = state.register_document_root(&folder).unwrap();
    let uri = VirtualDocumentPath::from_segments(&root.id, &["note.md".into()])
        .unwrap()
        .to_uri();
    assert!(state.read_document_bytes(&uri, 1024).is_err());
    state.approve_document_root(&root.id).unwrap();
    #[cfg(windows)]
    assert_eq!(
        state.read_document_bytes(&uri, 1024).unwrap(),
        b"authoritative"
    );
    #[cfg(not(windows))]
    assert!(state.read_document_bytes(&uri, 1024).is_err());
    assert!(state.read_document_bytes(&uri, 0).is_err());
    assert!(state.read_document_bytes(&uri, 33 * 1024 * 1024).is_err());
    state.revoke_document_root(&root.id).unwrap();
    assert!(state.require_approved_document_uri(&uri).is_err());
    assert!(state.read_document_bytes(&uri, 1024).is_err());
    assert!(state.approve_document_root(&root.id).is_err());
    assert!(state.revoke_document_root(&root.id).is_err());
    assert!(state.register_document_root(&folder).is_err());
}

#[cfg(windows)]
#[test]
fn windows_relative_handles_reject_escape_reparse_hardlinks_and_root_replacement() {
    let (home, state) = store();
    let root_path = home.path().join("granted");
    let outside = home.path().join("outside");
    fs::create_dir(&root_path).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("secret.txt"), "secret").unwrap();
    fs::write(root_path.join("allowed.txt"), "ok").unwrap();
    let root = state.register_document_root(&root_path).unwrap();
    state.approve_document_root(&root.id).unwrap();
    let uri = |segments: &[String]| {
        VirtualDocumentPath::from_segments(&root.id, segments)
            .unwrap()
            .to_uri()
    };
    assert_eq!(
        state
            .read_document_bytes(&uri(&["allowed.txt".into()]), 16)
            .unwrap(),
        b"ok"
    );
    assert!(state
        .read_document_bytes(&uri(&["allowed.txt".into()]), 1)
        .is_err());
    // An NTFS hardlink may point to a file originally outside the approved root.
    if fs::hard_link(outside.join("secret.txt"), root_path.join("linked.txt")).is_ok() {
        assert!(state
            .read_document_bytes(&uri(&["linked.txt".into()]), 128)
            .is_err());
    }
    // Symlink creation may require Windows developer mode; verify if available.
    if std::os::windows::fs::symlink_file(outside.join("secret.txt"), root_path.join("escape.txt"))
        .is_ok()
    {
        assert!(state
            .read_document_bytes(&uri(&["escape.txt".into()]), 128)
            .is_err());
    }
    let prior = home.path().join("prior");
    fs::rename(&root_path, &prior).unwrap();
    fs::create_dir(&root_path).unwrap();
    fs::write(root_path.join("allowed.txt"), "replaced").unwrap();
    assert!(state
        .read_document_bytes(&uri(&["allowed.txt".into()]), 128)
        .is_err());
    assert!(StateStore::open(state.path())
        .unwrap()
        .read_document_bytes(&uri(&["allowed.txt".into()]), 128)
        .is_err());
}

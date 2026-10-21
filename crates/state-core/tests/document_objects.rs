use mind_place_state::{
    DocumentMetadata, DocumentObjectStore, StateError, StateStore, VirtualDocumentPath,
};
use rusqlite::Connection;
use std::{
    fs,
    io::{self, Cursor, Read},
};

fn fixture() -> (tempfile::TempDir, StateStore, String) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("docs");
    fs::create_dir(&root).unwrap();
    let state = StateStore::open(dir.path().join("state.sqlite3")).unwrap();
    let root = state.register_document_root(&root).unwrap();
    state.approve_document_root(&root.id).unwrap();
    let uri = VirtualDocumentPath::from_segments(&root.id, &["notes.md".into()])
        .unwrap()
        .to_uri();
    (dir, state, uri)
}
fn metadata(size: u64, tick: u64) -> DocumentMetadata {
    DocumentMetadata {
        file_identity: "fixture:file:1".into(),
        byte_length: size,
        modified_ns: tick,
        changed_ns: tick,
    }
}
fn object_path(dir: &std::path::Path, hash: &str) -> std::path::PathBuf {
    dir.join("document-objects/v1/sha256")
        .join(&hash[..2])
        .join(hash)
}

#[test]
fn distinct_versions_survive_restart_and_identical_bytes_share_one_object() {
    let (dir, state, uri) = fixture();
    let store = DocumentObjectStore::open(state.clone()).unwrap();
    let first = store
        .preserve(&uri, &metadata(5, 1), Cursor::new(b"first"), || Ok(()))
        .unwrap();
    assert!(first.new_version);
    assert!(!first.reused_object);
    let same = store
        .preserve(&uri, &metadata(5, 2), Cursor::new(b"first"), || Ok(()))
        .unwrap();
    assert!(!same.new_version);
    assert!(same.reused_object);
    assert_eq!(same.version, first.version);
    let second = store
        .preserve(&uri, &metadata(6, 3), Cursor::new(b"second"), || Ok(()))
        .unwrap();
    assert_ne!(second.version.id, first.version.id);
    let other = uri.replace("notes.md", "copy.md");
    let copy = store
        .preserve(&other, &metadata(5, 4), Cursor::new(b"first"), || Ok(()))
        .unwrap();
    assert!(copy.reused_object);
    assert_ne!(copy.version.id, first.version.id);
    let store = DocumentObjectStore::open(StateStore::open(state.path()).unwrap()).unwrap();
    assert_eq!(
        store
            .read_version(&uri, &first.version.id, || Ok(()))
            .unwrap(),
        b"first"
    );
    assert_eq!(
        store
            .read_version(&uri, &second.version.id, || Ok(()))
            .unwrap(),
        b"second"
    );
    assert!(store
        .read_version(&other, &first.version.id, || Ok(()))
        .is_err());
    let reverted = store
        .preserve(&uri, &metadata(5, 5), Cursor::new(b"first"), || Ok(()))
        .unwrap();
    assert_eq!(reverted.version.id, first.version.id);
    assert_eq!(store.versions(&uri, None, 100, || Ok(())).unwrap().len(), 2);
    let db = Connection::open(state.path()).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM document_objects", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert!(db.execute("DELETE FROM document_versions", []).is_err());
    assert!(db
        .execute("UPDATE document_objects SET byte_length=0", [])
        .is_err());
    assert!(object_path(dir.path(), &first.version.content_hash).is_file());
}

#[test]
fn unchanged_probe_is_advisory_and_force_preserve_rehashes_equal_metadata() {
    let (dir, state, uri) = fixture();
    let store = DocumentObjectStore::open(state).unwrap();
    assert!(store
        .unchanged(&uri, &metadata(3, 1), || Ok(()))
        .unwrap()
        .is_none());
    let a = store
        .preserve(&uri, &metadata(3, 1), Cursor::new(b"one"), || Ok(()))
        .unwrap();
    assert_eq!(
        store
            .unchanged(&uri, &metadata(3, 1), || Ok(()))
            .unwrap()
            .unwrap(),
        a.version
    );
    assert!(store
        .unchanged(&uri, &metadata(3, 2), || Ok(()))
        .unwrap()
        .is_none());
    let b = store
        .preserve(&uri, &metadata(3, 1), Cursor::new(b"two"), || Ok(()))
        .unwrap();
    assert_ne!(a.version.id, b.version.id);
    fs::remove_file(object_path(dir.path(), &b.version.content_hash)).unwrap();
    assert!(store.unchanged(&uri, &metadata(3, 1), || Ok(())).is_err());
}

#[test]
fn revoked_or_pending_roots_and_failed_unlock_guards_never_disclose_versions() {
    let (_dir, state, uri) = fixture();
    let store = DocumentObjectStore::open(state.clone()).unwrap();
    assert!(store
        .preserve(&uri, &metadata(3, 1), Cursor::new(b"one"), || Err(
            StateError::DocumentDenied
        ))
        .is_err());
    let a = store
        .preserve(&uri, &metadata(3, 1), Cursor::new(b"one"), || Ok(()))
        .unwrap();
    let root_id = VirtualDocumentPath::parse(&uri)
        .unwrap()
        .root_id()
        .to_owned();
    state.revoke_root(&root_id).unwrap();
    for approved in [false, true] {
        // Even a tampered reapproval cannot erase a durable tombstone.
        if approved {
            Connection::open(state.path())
                .unwrap()
                .execute(
                    "UPDATE source_roots SET approval_state='approved' WHERE id=?1",
                    [&root_id],
                )
                .unwrap();
        }
        assert!(store.read_version(&uri, &a.version.id, || Ok(())).is_err());
        assert!(store.versions(&uri, None, 10, || Ok(())).is_err());
        assert!(store.unchanged(&uri, &metadata(3, 1), || Ok(())).is_err());
        assert!(store
            .preserve(&uri, &metadata(3, 1), Cursor::new(b"one"), || Ok(()))
            .is_err());
    }
}

struct RevokingReader {
    state: StateStore,
    root: String,
    read: bool,
}
impl Read for RevokingReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.read {
            return Ok(0);
        }
        self.read = true;
        self.state.revoke_root(&self.root).unwrap();
        buf[..3].copy_from_slice(b"one");
        Ok(3)
    }
}
#[test]
fn revocation_during_stream_prevents_authority_commit() {
    let (_dir, state, uri) = fixture();
    let store = DocumentObjectStore::open(state.clone()).unwrap();
    let reader = RevokingReader {
        state: state.clone(),
        root: VirtualDocumentPath::parse(&uri).unwrap().root_id().into(),
        read: false,
    };
    assert!(store
        .preserve(&uri, &metadata(3, 1), reader, || Ok(()))
        .is_err());
    assert_eq!(
        Connection::open(state.path())
            .unwrap()
            .query_row("SELECT count(*) FROM document_versions", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn corruption_is_rejected_without_overwriting_evidence() {
    let (dir, state, uri) = fixture();
    let store = DocumentObjectStore::open(state).unwrap();
    let a = store
        .preserve(&uri, &metadata(3, 1), Cursor::new(b"one"), || Ok(()))
        .unwrap();
    let file = object_path(dir.path(), &a.version.content_hash);
    fs::write(&file, b"bad").unwrap();
    assert!(matches!(
        store.read_version(&uri, &a.version.id, || Ok(())),
        Err(StateError::ObjectCorrupt)
    ));
    assert!(matches!(
        store.preserve(&uri, &metadata(3, 1), Cursor::new(b"one"), || Ok(())),
        Err(StateError::ObjectCorrupt)
    ));
    assert_eq!(fs::read(file).unwrap(), b"bad");
}

#[test]
fn bounded_streams_reject_wrong_lengths_errors_and_invalid_metadata() {
    let (_dir, state, uri) = fixture();
    let store = DocumentObjectStore::with_limit(state.clone(), 4).unwrap();
    assert!(store
        .preserve(&uri, &metadata(5, 1), Cursor::new(b"12345"), || Ok(()))
        .is_err());
    assert!(store
        .preserve(&uri, &metadata(3, 1), Cursor::new(b"12345"), || Ok(()))
        .is_err());
    assert!(store
        .preserve(&uri, &metadata(4, 1), Cursor::new(b"123"), || Ok(()))
        .is_err());
    assert!(store
        .preserve(&uri, &metadata(0, 1), Cursor::new(b""), || Ok(()))
        .is_ok());
    let mut invalid = metadata(1, 1);
    invalid.file_identity = "".into();
    assert!(store
        .preserve(&uri, &invalid, Cursor::new(b"x"), || Ok(()))
        .is_err());
    assert!(store.versions(&uri, None, 1001, || Ok(())).is_err());
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("fixture"))
        }
    }
    assert!(store
        .preserve(&uri, &metadata(1, 1), Broken, || Ok(()))
        .is_err());
    assert_eq!(store.versions(&uri, None, 100, || Ok(())).unwrap().len(), 1);
}

#[test]
fn published_orphan_is_reused_after_failed_authority_commit() {
    let (dir, state, uri) = fixture();
    let store = DocumentObjectStore::open(state.clone()).unwrap();
    let db = Connection::open(state.path()).unwrap();
    db.execute_batch("CREATE TRIGGER fail_document_commit BEFORE INSERT ON document_versions BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert!(store
        .preserve(&uri, &metadata(3, 1), Cursor::new(b"one"), || Ok(()))
        .is_err());
    assert!(store
        .versions(&uri, None, 100, || Ok(()))
        .unwrap()
        .is_empty());
    db.execute_batch("DROP TRIGGER fail_document_commit;")
        .unwrap();
    let restored = DocumentObjectStore::open(StateStore::open(state.path()).unwrap()).unwrap();
    let receipt = restored
        .preserve(&uri, &metadata(3, 1), Cursor::new(b"one"), || Ok(()))
        .unwrap();
    assert!(receipt.reused_object);
    assert_eq!(
        fs::read(object_path(dir.path(), &receipt.version.content_hash)).unwrap(),
        b"one"
    );
}

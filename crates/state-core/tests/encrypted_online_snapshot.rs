#![cfg(feature = "sqlcipher-preview")]
//! Synthetic cloud qualification of a WAL-aware, keyed SQLCipher online snapshot.
//! The companion higher-level vault generation/objects coordinator is separate.
use mind_place_security::SecurityGate;
use mind_place_state::{SourceDescriptor, SourceKey, StateStore};
use std::fs;

#[test]
fn online_snapshot_includes_committed_authority_and_stays_encrypted() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("state.sqlite3");
    let snapshot = temp.path().join("state-snapshot.sqlite3");
    let key = [0x9au8; 32];
    let gate = SecurityGate::open(temp.path().join("security.json")).unwrap();
    gate.initialize(b"synthetic online snapshot secret")
        .unwrap();
    let lease = gate.require_unlocked().unwrap();
    let db = StateStore::open_encrypted(&source, key, &gate, lease).unwrap();
    let descriptor = SourceDescriptor {
        provider: "chatgpt",
        source_namespace: "synthetic-account",
        source_kind: "conversation",
        key: SourceKey::External("snapshot-authority"),
    };
    let id = db.register_source(&descriptor).unwrap();
    db.create_encrypted_online_snapshot(&snapshot).unwrap();

    let image = fs::read(&snapshot).unwrap();
    assert!(!image.starts_with(b"SQLite format 3\0"));
    assert!(!image
        .windows(b"snapshot-authority".len())
        .any(|w| w == b"snapshot-authority"));
    assert!(
        StateStore::open(&snapshot).is_err(),
        "plaintext access must fail"
    );
    assert!(StateStore::open_encrypted(&snapshot, [0x9bu8; 32], &gate, lease).is_err());
    let restored = StateStore::open_encrypted(&snapshot, key, &gate, lease).unwrap();
    assert_eq!(
        restored.source(&id).unwrap().unwrap().source_key,
        "snapshot-authority"
    );
    assert!(
        db.create_encrypted_online_snapshot(&snapshot).is_err(),
        "must not replace backup"
    );
    assert_eq!(
        fs::read(&snapshot).unwrap(),
        image,
        "failed overwrite mutated owner snapshot"
    );

    gate.lock();
    let rejected_path = temp.path().join("locked-snapshot.sqlite3");
    assert!(db.create_encrypted_online_snapshot(&rejected_path).is_err());
    assert!(!rejected_path.exists(), "revoked lease created output");
}

#[test]
fn plaintext_state_cannot_enter_sqlcipher_online_snapshot_path() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("legacy.sqlite3");
    let destination = temp.path().join("not-created.sqlite3");
    let legacy = StateStore::open(&source).unwrap();
    assert!(legacy
        .create_encrypted_online_snapshot(&destination)
        .is_err());
    assert!(!destination.exists());
}

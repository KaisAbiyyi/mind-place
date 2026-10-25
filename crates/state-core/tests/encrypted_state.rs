#![cfg(feature = "sqlcipher-preview")]
//! MP-9B: prove product StateStore writes encrypted SQLCipher pages, not just an
//! application-layer cipher that leaves SQLite and its WAL in plaintext.
use mind_place_security::SecurityGate;
use mind_place_state::StateStore;
use std::fs;

#[test]
fn encrypted_state_roundtrip_and_wrong_key_fail_closed() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("state.sqlite3");
    let key = [0x45u8; 32];
    let gate = SecurityGate::open(temp.path().join("security.json")).unwrap();
    gate.initialize(b"synthetic full-length secret").unwrap();
    let lease = gate.require_unlocked().unwrap();
    let authority =
        StateStore::open_encrypted(&db, key, &gate, lease).expect("SQLCipher must open");
    drop(authority);
    let bytes = fs::read(&db).unwrap();
    assert!(
        !bytes.starts_with(b"SQLite format 3\0"),
        "unencrypted database header"
    );
    assert!(
        !bytes.windows(17).any(|w| w == b"source_identities"),
        "database schema leaked"
    );
    assert!(
        StateStore::open_encrypted(&db, [0x46u8; 32], &gate, lease).is_err(),
        "wrong key adopted vault"
    );
    assert!(
        StateStore::open(&db).is_err(),
        "plain open must not accept ciphertext"
    );
    StateStore::open_encrypted(&db, key, &gate, lease).expect("correct key and schema readback");
    let secured = StateStore::open_encrypted(&db, key, &gate, lease).unwrap();
    gate.lock();
    assert!(
        secured.source("source:v1:invalid").is_err(),
        "stale lease disclosed authority"
    );
}
#[test]
fn plaintext_state_cannot_be_silently_adopted() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("legacy.sqlite3");
    StateStore::open(&db).unwrap();
    let before = fs::read(&db).unwrap();
    assert!(before.starts_with(b"SQLite format 3\0"));
    let gate = SecurityGate::open(temp.path().join("security.json")).unwrap();
    gate.initialize(b"synthetic full-length secret").unwrap();
    let lease = gate.require_unlocked().unwrap();
    assert!(StateStore::open_encrypted(&db, [0x77u8; 32], &gate, lease).is_err());
    assert_eq!(
        fs::read(&db).unwrap(),
        before,
        "failed adoption mutated plaintext state"
    );
}

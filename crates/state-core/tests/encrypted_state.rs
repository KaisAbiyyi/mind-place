//! MP-9B: prove product StateStore writes encrypted SQLCipher pages, not just an
//! application-layer cipher that leaves SQLite and its WAL in plaintext.
use mind_place_state::StateStore;
use std::fs;

#[test]
fn encrypted_state_roundtrip_and_wrong_key_fail_closed() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("state.sqlite3");
    let key = [0x45u8; 32];
    let authority = StateStore::open_encrypted(&db, key).expect("SQLCipher must open");
    drop(authority);
    let bytes = fs::read(&db).unwrap();
    assert!(!bytes.starts_with(b"SQLite format 3\0"), "unencrypted database header");
    assert!(!bytes.windows(17).any(|w| w == b"source_identities"), "database schema leaked");
    assert!(StateStore::open_encrypted(&db, [0x46u8; 32]).is_err(), "wrong key adopted vault");
    assert!(StateStore::open(&db).is_err(), "plain open must not accept ciphertext");
    StateStore::open_encrypted(&db, key).expect("correct key and schema readback");
}
#[test]
fn plaintext_state_cannot_be_silently_adopted() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("legacy.sqlite3");
    StateStore::open(&db).unwrap();
    let before = fs::read(&db).unwrap();
    assert!(before.starts_with(b"SQLite format 3\0"));
    assert!(StateStore::open_encrypted(&db, [0x77u8; 32]).is_err());
    assert_eq!(fs::read(&db).unwrap(), before, "failed adoption mutated plaintext state");
}

#![cfg(feature = "sqlcipher-preview")]
//! Synthetic MP-9B SQLCipher index online snapshot qualification.
use mind_place_index::IndexStore;
use mind_place_security::{vault::CryptoVault, SecurityGate};
use std::fs;

#[test]
fn derived_index_backup_contains_committed_ciphertext_and_never_overwrites() {
    let home = tempfile::tempdir().unwrap();
    let gate = SecurityGate::open(home.path().join("security.json")).unwrap();
    gate.initialize(b"synthetic-index-qualification-secret")
        .unwrap();
    let lease = gate.require_unlocked().unwrap();
    let (vault, _, manifest) = CryptoVault::provision(&gate, lease).unwrap();
    let root = home.path().join("vault");
    manifest.persist_new(&root).unwrap();
    let key = vault
        .derived_key(&gate, lease, b"index/sqlcipher/v1")
        .unwrap();
    let mut raw = [0u8; 32];
    raw.copy_from_slice(key.as_ref());
    let index = IndexStore::open_encrypted(root.join("index.sqlite3"), raw, &gate, lease).unwrap();
    let prior = index.stats().unwrap();
    let dest = home.path().join("index-backup.sqlite3");
    index.create_encrypted_online_snapshot(&dest).unwrap();
    let bytes = fs::read(&dest).unwrap();
    assert!(!bytes.starts_with(b"SQLite format 3\0"));
    assert!(IndexStore::open(&dest).is_err());
    let restored = IndexStore::open_encrypted(&dest, raw, &gate, lease).unwrap();
    assert_eq!(restored.stats().unwrap().generation, prior.generation);
    assert!(index.create_encrypted_online_snapshot(&dest).is_err());
    assert_eq!(fs::read(&dest).unwrap(), bytes);
    gate.lock();
    assert!(index
        .create_encrypted_online_snapshot(home.path().join("revoked.sqlite3"))
        .is_err());
    assert!(!home.path().join("revoked.sqlite3").exists());
}

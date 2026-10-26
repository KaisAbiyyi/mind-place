#![cfg(feature = "sqlcipher-preview")]
//! Synthetic-only AEAD document source -> preserved object -> keyed FTS test.
use mind_place_index::vault_archive::VaultCaptureStore;
use mind_place_security::{vault::CryptoVault, SecurityGate};
use std::fs;

#[test]
fn approved_document_scan_uses_ciphertext_only_and_fences_after_lock() {
    let home = tempfile::tempdir().unwrap();
    let gate = SecurityGate::open(home.path().join("security.json")).unwrap();
    gate.initialize(b"synthetic-document-vault-secret").unwrap();
    let lease = gate.require_unlocked().unwrap();
    let (vault, _recovery, manifest) = CryptoVault::provision(&gate, lease).unwrap();
    let root = home.path().join("vault");
    manifest.persist_new(&root).unwrap();
    let docs = home.path().join("synthetic-docs");
    fs::create_dir(&docs).unwrap();
    fs::write(docs.join("sample.md"), b"## Synthetic\nencrypted_document_needle\n").unwrap();
    let archive = VaultCaptureStore::open(&root, vault, &gate, lease).unwrap();
    let authority = archive.open_state_store().unwrap();
    let approved = authority.register_document_root(&docs).unwrap();
    authority.approve_document_root(&approved.id).unwrap();
    let index = archive.open_document_index().unwrap();
    let report = index.refresh_root(&authority, &approved.id, || Ok(())).unwrap();
    assert_eq!(report.scanned_files, 1);
    assert_eq!(report.indexed_files, 1);
    let hits = index.search(&authority, "encrypted_document_needle", 5, || Ok(())).unwrap();
    assert_eq!(hits.len(), 1);
    let preview = index.preview(&authority, &hits[0].uri, &hits[0].version_id, || Ok(())).unwrap();
    assert!(preview.text.contains("encrypted_document_needle"));
    assert!(!root.join("document-objects/v1/sha256").exists());
    let shards = fs::read_dir(root.join("document-objects")).unwrap();
    let mut encrypted_objects = 0usize;
    for shard in shards {
        for object in fs::read_dir(shard.unwrap().path()).unwrap() {
            let raw = fs::read(object.unwrap().path()).unwrap();
            assert!(raw.starts_with(b"MPOS\x01"));
            assert!(!raw.windows(b"encrypted_document_needle".len()).any(|w| w == b"encrypted_document_needle"));
            encrypted_objects += 1;
        }
    }
    assert_eq!(encrypted_objects, 1);
    let state_bytes = fs::read(root.join("state.sqlite3")).unwrap();
    assert!(!state_bytes.starts_with(b"SQLite format 3\0"));
    gate.lock();
    assert!(index.search(&authority, "encrypted_document_needle", 5, || Ok(())).is_err());
    assert!(archive.open_document_index().is_err());
}

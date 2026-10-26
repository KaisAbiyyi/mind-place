#![cfg(feature = "sqlcipher-preview")]
//! Synthetic committed-WAL + AEAD object backup across encrypted state/archive.
use mind_place_archive::Capture;
use mind_place_index::{vault_archive::VaultCaptureStore, vault_online_backup::VaultOnlineBackup};
use mind_place_security::{vault::CryptoVault, vault_backup::PortableBackup, SecurityGate};
use mind_place_state::StateStore;

#[test]
fn online_snapshot_restores_cross_store_synthetic_capture_and_state() {
    let home = tempfile::tempdir().unwrap();
    let gate = SecurityGate::open(home.path().join("security.json")).unwrap();
    gate.initialize(b"synthetic-online-backup-secret").unwrap();
    let lease = gate.require_unlocked().unwrap();
    let (vault, recovery, manifest) = CryptoVault::provision(&gate, lease).unwrap();
    let root = home.path().join("vault");
    manifest.persist_new(&root).unwrap();
    let key = vault
        .derived_key(&gate, lease, b"state/sqlcipher/v1")
        .unwrap();
    let mut state_key = [0u8; 32];
    state_key.copy_from_slice(key.as_ref());
    let state =
        StateStore::open_encrypted(root.join("state.sqlite3"), state_key, &gate, lease).unwrap();
    let archive = VaultCaptureStore::open(&root, vault, &gate, lease).unwrap();
    let captured: Capture = serde_json::from_value(serde_json::json!({
        "schema_version":1,
        "provider":"chatgpt",
        "conversation_id":"123e4567-e89b-12d3-a456-426614174000",
        "source_url":"https://chatgpt.com/c/123e4567-e89b-12d3-a456-426614174000",
        "title":"Synthetic Backup",
        "captured_at":"2026-10-26T03:00:00Z",
        "capture_method":"manual-dom",
        "completeness":"partial",
        "messages":[{"id":"synthetic", "role":"user","text":"synthetic only","format":"plain",
            "attachments":[]}],
        "warnings":["synthetic observation"]
    }))
    .unwrap();
    let receipt = archive.save(captured).unwrap();
    let backup = home.path().join("portable-bundle");
    // Scope a second crypto handle to ensure the coordinator can recover
    // independently of the writer's ownership of its in-memory DEK.
    let verified = CryptoVault::restore_with_recovery(&gate, lease, &manifest, &recovery).unwrap();
    let saved = VaultOnlineBackup::create(&gate, lease, &verified, &root, &backup, 1).unwrap();
    assert!(saved.count >= 3);
    assert!(!backup.join("payload/state.sqlite3-wal").exists());
    assert!(!backup.join("payload/archive-index.sqlite3-wal").exists());
    assert!(
        PortableBackup::create(
            &gate,
            lease,
            &verified,
            &root,
            &home.path().join("offline-during-wal"),
            2
        )
        .is_err()
            || !root.join("state.sqlite3-wal").exists()
    );
    drop(state);
    let destination = home.path().join("restored-vault");
    PortableBackup::restore(&gate, lease, &backup, &destination, &recovery).unwrap();
    let rebuilt = CryptoVault::restore_with_recovery(&gate, lease, &manifest, &recovery).unwrap();
    let restored_archive = VaultCaptureStore::open(&destination, rebuilt, &gate, lease).unwrap();
    let loaded = restored_archive.read(&receipt.id, None).unwrap();
    assert_eq!(loaded.capture.messages[0].text, "synthetic only");
    assert_eq!(loaded.summary.snapshot_count, 1);
    let raw = std::fs::read(destination.join("state.sqlite3")).unwrap();
    assert!(!raw.starts_with(b"SQLite format 3\0"));
}

#[test]
fn online_backup_failure_injection_never_publishes_incomplete_restores() {
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    fn first_file(path: &Path) -> PathBuf {
        for entry in fs::read_dir(path).unwrap() {
            let item = entry.unwrap().path();
            if item.is_dir() {
                return first_file(&item);
            }
            if item.is_file() {
                return item;
            }
        }
        panic!("fixture must contain an encrypted object");
    }

    let home = tempfile::tempdir().unwrap();
    let gate = SecurityGate::open(home.path().join("security.json")).unwrap();
    gate.initialize(b"synthetic-online-failure-injection")
        .unwrap();
    let lease = gate.require_unlocked().unwrap();
    let (vault, recovery, manifest) = CryptoVault::provision(&gate, lease).unwrap();
    let root = home.path().join("vault");
    manifest.persist_new(&root).unwrap();
    let mut raw = [0u8; 32];
    raw.copy_from_slice(
        vault
            .derived_key(&gate, lease, b"state/sqlcipher/v1")
            .unwrap()
            .as_ref(),
    );
    let _state = StateStore::open_encrypted(root.join("state.sqlite3"), raw, &gate, lease).unwrap();
    let archive = VaultCaptureStore::open(&root, vault, &gate, lease).unwrap();
    let capture: Capture = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "provider": "chatgpt",
        "conversation_id": "123e4567-e89b-12d3-a456-426614174001",
        "source_url": "https://chatgpt.com/c/123e4567-e89b-12d3-a456-426614174001",
        "title": "Synthetic negative snapshot",
        "captured_at": "2026-10-26T07:00:00Z",
        "capture_method": "manual-dom",
        "completeness": "partial",
        "messages": [{"id": "one", "role": "user", "text": "synthetic", "format": "plain", "attachments": []}],
        "warnings": []
    })).unwrap();
    archive.save(capture).unwrap();
    let verified = CryptoVault::restore_with_recovery(&gate, lease, &manifest, &recovery).unwrap();

    // An unknown plaintext writer cannot be silently excluded from a "complete"
    // generation. No publicly named destination may be published on failure.
    fs::write(
        root.join("unclassified-new-writer.txt"),
        b"synthetic-not-vaulted",
    )
    .unwrap();
    let rejected = home.path().join("rejected");
    assert!(VaultOnlineBackup::create(&gate, lease, &verified, &root, &rejected, 2).is_err());
    assert!(!rejected.exists());
    fs::remove_file(root.join("unclassified-new-writer.txt")).unwrap();

    let backup = home.path().join("good-bundle");
    VaultOnlineBackup::create(&gate, lease, &verified, &root, &backup, 3).unwrap();
    let object = first_file(&backup.join("payload/archive"));
    let mut bytes = fs::read(&object).unwrap();
    bytes[bytes.len() - 1] ^= 0x80; // Synthetic ciphertext corruption after publication.
    fs::write(&object, bytes).unwrap();
    let destination = home.path().join("must-not-publish");
    assert!(PortableBackup::restore(&gate, lease, &backup, &destination, &recovery).is_err());
    assert!(!destination.exists());
    assert!(root.join("state.sqlite3").is_file());
}

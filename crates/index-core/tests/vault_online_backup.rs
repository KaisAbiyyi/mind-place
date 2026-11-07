#![cfg(feature = "sqlcipher-preview")]
//! Synthetic committed-WAL + AEAD object backup across encrypted state/archive.
use mind_place_archive::Capture;
use mind_place_index::{vault_archive::VaultCaptureStore, vault_online_backup::VaultOnlineBackup};
use mind_place_security::{
    vault::{CryptoVault, VaultManifest},
    vault_backup::PortableBackup,
    SecurityGate,
};
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
        "messages":[{"id":"synthetic", "role":"user","text":"synthetic archivebackupneedle","format":"plain",
            "attachments":[]}],
        "warnings":["synthetic observation"]
    }))
    .unwrap();
    let receipt = archive.save(captured).unwrap();
    let index = archive.open_index_store().unwrap();
    let indexed = index
        .sync_vault_with_check(&archive, &state, || Ok(()))
        .unwrap();
    assert!(indexed.chunks > 0);
    assert_eq!(
        index.search("archivebackupneedle", false, 5).unwrap().len(),
        1
    );
    let _document_index = archive.open_document_index().unwrap();
    let backup = home.path().join("portable-bundle");
    // Scope a second crypto handle to ensure the coordinator can recover
    // independently of the writer's ownership of its in-memory DEK.
    let verified = CryptoVault::restore_with_recovery(&gate, lease, &manifest, &recovery).unwrap();
    let saved = VaultOnlineBackup::create(&gate, lease, &verified, &root, &backup, 1).unwrap();
    assert_eq!(saved.generation, 1);
    assert_eq!(saved.vault_id_b64, manifest.vault_id_b64);
    assert!(saved.count >= 5);
    for name in [
        "state.sqlite3",
        "index.sqlite3",
        "document-index.sqlite3",
        "archive-index.sqlite3",
    ] {
        assert!(backup.join("payload").join(name).is_file());
        assert!(!backup.join("payload").join(format!("{name}-wal")).exists());
        assert!(!backup.join("payload").join(format!("{name}-shm")).exists());
        let raw = std::fs::read(backup.join("payload").join(name)).unwrap();
        assert!(!raw.starts_with(b"SQLite format 3\0"));
    }
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
    VaultOnlineBackup::restore(&gate, lease, &backup, &destination, &recovery).unwrap();
    // An existing published recovery never gets overwritten or adopted.
    assert!(VaultOnlineBackup::restore(&gate, lease, &backup, &destination, &recovery).is_err());
    let rebuilt = CryptoVault::restore_with_recovery(&gate, lease, &manifest, &recovery).unwrap();
    let restored_archive = VaultCaptureStore::open(&destination, rebuilt, &gate, lease).unwrap();
    let loaded = restored_archive.read(&receipt.id, None).unwrap();
    assert_eq!(
        loaded.capture.messages[0].text,
        "synthetic archivebackupneedle"
    );
    assert_eq!(loaded.summary.snapshot_count, 1);
    let restored_state = restored_archive.open_state_store().unwrap();
    let restored_index = restored_archive.open_index_store().unwrap();
    assert_eq!(
        restored_index
            .search("archivebackupneedle", false, 5)
            .unwrap()
            .len(),
        1,
        "restored FTS must retain the indexed archive generation"
    );
    let _restored_document_index = restored_archive.open_document_index().unwrap();
    let restored_manifest = VaultManifest::read(&destination).unwrap();
    assert_eq!(restored_manifest.vault_id_b64, manifest.vault_id_b64);
    drop(restored_state);
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
    let preserved_scratch: Vec<_> = fs::read_dir(home.path())
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(".mp9b-encrypted-online-"))
        })
        .collect();
    assert_eq!(preserved_scratch.len(), 1);
    assert!(preserved_scratch[0].join("vault/vault-v1.json").is_file());
    fs::remove_file(root.join("unclassified-new-writer.txt")).unwrap();

    let backup = home.path().join("good-bundle");
    VaultOnlineBackup::create(&gate, lease, &verified, &root, &backup, 3).unwrap();
    let scratch_count_after_success = fs::read_dir(home.path())
        .unwrap()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(".mp9b-encrypted-online-"))
        })
        .count();
    assert_eq!(
        scratch_count_after_success, 1,
        "successful backup removes only its own temporary scratch"
    );

    let staged_target = home.path().join("stale-staged-restore");
    let staged = staged_target.with_extension("mp9b-verified-restore-partial");
    fs::create_dir(&staged).unwrap();
    fs::write(staged.join("owner-kept"), b"explicit disposition").unwrap();
    assert!(VaultOnlineBackup::restore(&gate, lease, &backup, &staged_target, &recovery).is_err());
    assert!(!staged_target.exists());
    assert_eq!(
        fs::read(staged.join("owner-kept")).unwrap(),
        b"explicit disposition"
    );

    let object = first_file(&backup.join("payload/archive"));
    let mut bytes = fs::read(&object).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0x80; // Synthetic ciphertext corruption after publication.
    fs::write(&object, bytes).unwrap();
    let destination = home.path().join("must-not-publish");
    assert!(VaultOnlineBackup::restore(&gate, lease, &backup, &destination, &recovery).is_err());
    assert!(!destination.exists());
    assert!(!destination
        .with_extension("mp9b-verified-restore-partial")
        .exists());
    assert!(root.join("state.sqlite3").is_file());

    let digest_backup = home.path().join("payload-digest-bundle");
    VaultOnlineBackup::create(&gate, lease, &verified, &root, &digest_backup, 4).unwrap();
    let state_payload = digest_backup.join("payload/state.sqlite3");
    let mut state_bytes = fs::read(&state_payload).unwrap();
    state_bytes[32] ^= 0x40;
    fs::write(&state_payload, state_bytes).unwrap();
    let digest_target = home.path().join("digest-must-not-restore");
    assert!(
        VaultOnlineBackup::restore(&gate, lease, &digest_backup, &digest_target, &recovery)
            .is_err()
    );
    assert!(!digest_target.exists());
    assert!(!digest_target
        .with_extension("mp9b-verified-restore-partial")
        .exists());

    let manifest_backup = home.path().join("manifest-bundle");
    VaultOnlineBackup::create(&gate, lease, &verified, &root, &manifest_backup, 5).unwrap();
    let manifest_path = manifest_backup.join("manifest.enc");
    let mut manifest_bytes = fs::read(&manifest_path).unwrap();
    *manifest_bytes.last_mut().unwrap() ^= 0x01;
    fs::write(&manifest_path, manifest_bytes).unwrap();
    let manifest_target = home.path().join("manifest-must-not-restore");
    assert!(
        VaultOnlineBackup::restore(
            &gate,
            lease,
            &manifest_backup,
            &manifest_target,
            &recovery
        )
        .is_err()
    );
    assert!(!manifest_target.exists());
    assert!(!manifest_target
        .with_extension("mp9b-verified-restore-partial")
        .exists());

    gate.lock();
    let locked_backup = home.path().join("locked-online-backup");
    assert!(VaultOnlineBackup::create(&gate, lease, &verified, &root, &locked_backup, 6).is_err());
    assert!(!locked_backup.exists());
    let locked_restore = home.path().join("locked-online-restore");
    assert!(VaultOnlineBackup::restore(&gate, lease, &backup, &locked_restore, &recovery).is_err());
    assert!(!locked_restore.exists());
}

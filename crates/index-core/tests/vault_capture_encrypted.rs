#![cfg(feature = "sqlcipher-preview")]
//! Synthetic-only MP-9B encrypted capture fidelity and at-rest regression.
use mind_place_archive::Capture;
use mind_place_index::vault_archive::VaultCaptureStore;
use mind_place_security::{
    vault::{ContentKind, CryptoVault},
    vault_storage::ImmutableVaultStorage,
    SecurityGate, UnlockLease,
};
use mind_place_state::CipherKey;
use rusqlite::Connection;
use std::fs;

fn capture(at: &str) -> Capture {
    serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "provider": "chatgpt",
        "conversation_id": "123e4567-e89b-12d3-a456-426614174000",
        "source_url": "https://chatgpt.com/c/123e4567-e89b-12d3-a456-426614174000",
        "title": "Synthetic MP-9B Capture",
        "captured_at": at,
        "capture_method": "manual-dom",
        "completeness": "partial",
        "warnings": ["synthetic DOM fixture"],
        "messages": [{
            "id": "synthetic-1",
            "role": "user",
            "text": "This is entirely synthetic evidence.",
            "format": "plain",
            "attachments": []
        }]
    }))
    .unwrap()
}

#[test]
fn encrypted_archive_roundtrip_deduplicates_and_fails_closed_after_lock() {
    let home = tempfile::tempdir().unwrap();
    let gate = SecurityGate::open(home.path().join("security.json")).unwrap();
    gate.initialize(b"synthetic-encrypted-archive-secret")
        .unwrap();
    let lease = gate.require_unlocked().unwrap();
    let (vault, _, manifest) = CryptoVault::provision(&gate, lease).unwrap();
    let root = home.path().join("vault");
    manifest.persist_new(&root).unwrap();
    let store = VaultCaptureStore::open(&root, vault, &gate, lease).unwrap();

    let first = store.save(capture("2026-10-26T03:00:00Z")).unwrap();
    assert!(!first.unchanged);
    let second = store.save(capture("2026-10-26T03:00:01Z")).unwrap();
    assert!(second.unchanged);
    assert_eq!(first.id, second.id);
    assert_eq!(first.snapshot_id, second.snapshot_id);
    assert_eq!(store.snapshots(&first.id).unwrap().len(), 1);
    let loaded = store.read(&first.id, Some(&first.snapshot_id)).unwrap();
    assert_eq!(
        loaded.capture.messages[0].text,
        "This is entirely synthetic evidence."
    );
    assert_eq!(loaded.summary.snapshot_count, 1);
    let exported = store.export_markdown(&first.id, None).unwrap();
    assert!(
        exported.path.is_empty(),
        "vault must not silently publish plaintext exports"
    );
    assert!(exported
        .markdown
        .contains("This is entirely synthetic evidence."));
    assert!(!root.join("exports").exists());

    let authority = store.open_state_store().unwrap();
    let index = store.open_index_store().unwrap();
    let indexed = index
        .sync_vault_with_check(&store, &authority, || Ok(()))
        .unwrap();
    assert!(
        indexed.chunks > 0,
        "encrypted capture must become searchable"
    );
    let matches = index.search("entirely", false, 20).unwrap();
    assert!(!matches.is_empty());
    assert_eq!(matches[0].archive_id, first.id);
    assert!(!fs::read(root.join("index.sqlite3"))
        .unwrap()
        .starts_with(b"SQLite format 3\0"));

    let bytes = fs::read(root.join("archive-index.sqlite3")).unwrap();
    assert!(!bytes.starts_with(b"SQLite format 3\0"));
    let archives = root.join("archive");
    let shards = fs::read_dir(&archives).unwrap().count();
    assert!(shards > 0);
    gate.lock();
    assert!(store.read(&first.id, None).is_err());
    assert!(store.export_markdown(&first.id, None).is_err());
    assert!(index.search("entirely", false, 20).is_err());
    assert!(store.save(capture("2026-10-26T03:00:02Z")).is_err());
}

#[test]
fn unmarked_vault_cannot_create_encrypted_capture_index() {
    let home = tempfile::tempdir().unwrap();
    let gate = SecurityGate::open(home.path().join("security.json")).unwrap();
    gate.initialize(b"synthetic-encrypted-archive-secret")
        .unwrap();
    let lease = gate.require_unlocked().unwrap();
    let (vault, _, _) = CryptoVault::provision(&gate, lease).unwrap();
    let root = home.path().join("vault");
    fs::create_dir(&root).unwrap();
    assert!(VaultCaptureStore::open(&root, vault, &gate, lease).is_err());
    assert!(!root.join("archive-index.sqlite3").exists());
}

fn catalog_key(vault: &CryptoVault, gate: &SecurityGate, lease: UnlockLease) -> std::sync::Arc<CipherKey> {
    let key = vault
        .derived_key(gate, lease, b"archive-index/sqlcipher/v1")
        .unwrap();
    let mut raw = [0u8; 32];
    raw.copy_from_slice(key.as_ref());
    CipherKey::new(raw, gate, lease)
}

#[test]
fn interrupted_capture_publication_reuses_only_matching_authenticated_evidence() {
    for mismatched in [true, false] {
        let home = tempfile::tempdir().unwrap();
        let gate = SecurityGate::open(home.path().join("security.json")).unwrap();
        gate.initialize(b"synthetic-interrupted-capture").unwrap();
        let lease = gate.require_unlocked().unwrap();
        let (vault, recovery, manifest) = CryptoVault::provision(&gate, lease).unwrap();
        let root = home.path().join("vault");
        manifest.persist_new(&root).unwrap();
        let store = VaultCaptureStore::open(&root, vault, &gate, lease).unwrap();
        let first = store.save(capture("2026-10-26T03:00:00Z")).unwrap();
        drop(store);

        let vault = CryptoVault::restore_with_recovery(&gate, lease, &manifest, &recovery).unwrap();
        let key = catalog_key(&vault, &gate, lease);
        {
            let db = Connection::open(root.join("archive-index.sqlite3")).unwrap();
            key.unlock(&db).unwrap();
            // The exact durable state after object publication but before the
            // catalog transaction commits. No private corpus is used.
            db.execute("DELETE FROM vault_captures", []).unwrap();
        }
        let identity = format!("capture/{}/{}", first.id, first.snapshot_id);
        let storage = ImmutableVaultStorage::new(&root, &vault, &gate, lease).unwrap();
        let object = storage
            .location(&gate, lease, ContentKind::Archive, identity.as_bytes())
            .unwrap();
        if mismatched {
            fs::remove_file(&object).unwrap();
            let mut wrong = capture("2026-10-26T03:00:00Z");
            wrong.title = "Different authenticated evidence".into();
            storage
                .write_immutable(
                    &gate,
                    lease,
                    ContentKind::Archive,
                    identity.as_bytes(),
                    &serde_json::to_vec(&wrong).unwrap(),
                )
                .unwrap();
        }
        let ciphertext = fs::read(&object).unwrap();
        drop(storage);
        drop(key);
        drop(vault);
        gate.lock();
        gate.unlock(b"synthetic-interrupted-capture").unwrap();
        let lease = gate.require_unlocked().unwrap();
        let vault = CryptoVault::restore_with_recovery(&gate, lease, &manifest, &recovery).unwrap();
        let store = VaultCaptureStore::open(&root, vault, &gate, lease).unwrap();
        let result = store.save(capture("2026-10-26T03:00:01Z"));
        if mismatched {
            assert!(result.is_err());
            assert!(store.snapshots(&first.id).unwrap().is_empty());
        } else {
            let recovered = result.unwrap();
            assert_eq!(recovered.snapshot_id, first.snapshot_id);
            assert_eq!(store.snapshots(&first.id).unwrap().len(), 1);
            assert_eq!(
                store.read(&first.id, None).unwrap().capture.captured_at,
                "2026-10-26T03:00:00Z"
            );
            assert!(store.save(capture("2026-10-26T03:00:02Z")).unwrap().unchanged);
        }
        assert_eq!(fs::read(&object).unwrap(), ciphertext);
    }
}

#[test]
fn failed_catalog_initialization_rolls_back_its_partial_schema() {
    let home = tempfile::tempdir().unwrap();
    let gate = SecurityGate::open(home.path().join("security.json")).unwrap();
    gate.initialize(b"synthetic-catalog-rollback").unwrap();
    let lease = gate.require_unlocked().unwrap();
    let (vault, _, manifest) = CryptoVault::provision(&gate, lease).unwrap();
    let root = home.path().join("vault");
    manifest.persist_new(&root).unwrap();
    let key = catalog_key(&vault, &gate, lease);
    {
        let db = Connection::open(root.join("archive-index.sqlite3")).unwrap();
        key.unlock(&db).unwrap();
        db.execute_batch(
            "CREATE TABLE collision(value INTEGER);
             CREATE INDEX vault_capture_recent ON collision(value);",
        )
        .unwrap();
    }
    assert!(VaultCaptureStore::open(&root, vault, &gate, lease).is_err());
    let db = Connection::open(root.join("archive-index.sqlite3")).unwrap();
    key.unlock(&db).unwrap();
    let created: i64 = db
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='vault_captures'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(created, 0, "failed initialization must not publish a partial schema");
}

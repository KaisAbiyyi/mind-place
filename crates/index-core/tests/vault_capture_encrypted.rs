#![cfg(feature = "sqlcipher-preview")]
//! Synthetic-only MP-9B encrypted capture fidelity and at-rest regression.
use mind_place_archive::Capture;
use mind_place_index::vault_archive::VaultCaptureStore;
use mind_place_security::{vault::CryptoVault, SecurityGate};
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
    assert!(exported.path.is_empty(), "vault must not silently publish plaintext exports");
    assert!(exported.markdown.contains("This is entirely synthetic evidence."));
    assert!(!root.join("exports").exists());

    let bytes = fs::read(root.join("archive-index.sqlite3")).unwrap();
    assert!(!bytes.starts_with(b"SQLite format 3\0"));
    let archives = root.join("archive");
    let shards = fs::read_dir(&archives).unwrap().count();
    assert!(shards > 0);
    gate.lock();
    assert!(store.read(&first.id, None).is_err());
    assert!(store.export_markdown(&first.id, None).is_err());
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

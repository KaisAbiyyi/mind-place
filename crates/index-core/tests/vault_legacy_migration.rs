#![cfg(feature = "sqlcipher-preview")]
//! Offline synthetic-only owner-consented recovery qualification.
use mind_place_archive::{ArchiveStore, Capture};
use mind_place_index::{
    vault_archive::VaultCaptureStore, vault_legacy_migration::LegacyEncryptedConversion,
};
use mind_place_security::{
    vault::CryptoVault,
    vault_migration::{ExplicitMigrationConsent, LegacyMigrationPreflight},
    SecurityGate,
};
use mind_place_state::StateStore;
use sha2::{Digest, Sha256};
use std::fs;

#[test]
fn encrypted_migration_preserves_synthetic_captures_state_and_raw_document_bytes() {
    let home = tempfile::tempdir().unwrap();
    let source = home.path().join("legacy");
    fs::create_dir(&source).unwrap();
    let state = StateStore::open(source.join("state.sqlite3")).unwrap();
    drop(state);

    let archive = ArchiveStore::open(source.join("archive-v1")).unwrap();
    let conversation: Capture = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "provider": "chatgpt",
        "conversation_id": "123e4567-e89b-12d3-a456-426614174001",
        "source_url": "https://chatgpt.com/c/123e4567-e89b-12d3-a456-426614174001",
        "title": "synthetic migration",
        "captured_at": "2026-10-27T12:00:00Z",
        "capture_method": "manual-dom",
        "completeness": "partial",
        "warnings": ["only loaded messages"],
        "messages": [{
            "id": "synthetic",
            "role": "user",
            "text": "local synthetic fixture only",
            "format": "plain",
            "attachments": []
        }]
    }))
    .unwrap();
    let original_receipt = archive.save(conversation.clone()).unwrap();

    let document = b"synthetic permitted document evidence";
    let hash = format!("{:x}", Sha256::digest(document));
    let directory = source.join("document-objects/v1/sha256").join(&hash[..2]);
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join(&hash), document).unwrap();
    let legacy_sql = fs::read(source.join("state.sqlite3")).unwrap();

    let gate = SecurityGate::open(home.path().join("security-config.json")).unwrap();
    gate.initialize(b"owner-typed-synthetic-migration-passphrase")
        .unwrap();
    let lease = gate.require_unlocked().unwrap();
    let (_, recovery, manifest) = CryptoVault::provision(&gate, lease).unwrap();
    let consent =
        ExplicitMigrationConsent::from_owner_typed_confirmation("ENCRYPT MY LEGACY VAULT")
            .unwrap();
    let dest = home.path().join("encrypted-vault");
    let plan = LegacyMigrationPreflight::inspect(&source, &dest).unwrap();
    plan.begin_owner_consented(&consent, &gate, lease).unwrap();
    let converter = LegacyEncryptedConversion {
        gate: &gate,
        lease,
        consent: &consent,
        manifest: &manifest,
        recovery: &recovery,
    };
    converter.execute_or_resume(&plan).unwrap();

    assert_eq!(fs::read(source.join("state.sqlite3")).unwrap(), legacy_sql);
    assert!(!fs::read(dest.join("state.sqlite3"))
        .unwrap()
        .starts_with(b"SQLite format 3\0"));
    let recovered = CryptoVault::restore_with_recovery(&gate, lease, &manifest, &recovery)
        .unwrap();
    let reader = VaultCaptureStore::open(&dest, recovered, &gate, lease).unwrap();
    let read = reader.read(&original_receipt.id, Some(&original_receipt.snapshot_id))
        .unwrap();
    assert_eq!(read.capture.messages[0].text, "local synthetic fixture only");
    assert_eq!(
        fs::read(directory.join(&hash)).unwrap(),
        document
    );
    assert!(plan.rollback_to_legacy(&consent, &gate, lease).is_err());
}

#![cfg(all(feature = "sqlcipher-preview", windows))]
use mind_place_archive::{ArchiveStore, Capture};
use mind_place_index::{
    vault_archive::VaultCaptureStore, vault_owner_migration::OfflineLegacyRoot,
};
use mind_place_security::{
    vault::{CryptoVault, VaultManifest},
    vault_migration::ExplicitMigrationConsent,
    SecurityGate,
};
use mind_place_state::StateStore;
use std::{collections::BTreeMap, fs, path::Path};

fn inventory(root: &Path) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    for item in fs::read_dir(root).unwrap() {
        let path = item.unwrap().path();
        if path.file_name().unwrap() == "encrypted-vault-v1" {
            continue;
        }
        if path.is_dir() {
            for (name, bytes) in inventory(&path) {
                files.insert(
                    std::path::PathBuf::from(path.file_name().unwrap()).join(name),
                    bytes,
                );
            }
        } else {
            files.insert(path.file_name().unwrap().into(), fs::read(path).unwrap());
        }
    }
    files
}

fn fixture() -> (tempfile::TempDir, SecurityGate) {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("app");
    fs::create_dir(&root).unwrap();
    StateStore::open(root.join("state.sqlite3")).unwrap();
    let archive = ArchiveStore::open(root.join("archive-v1")).unwrap();
    let capture: Capture = serde_json::from_value(serde_json::json!({
        "schema_version":1,"provider":"chatgpt", "conversation_id":"123e4567-e89b-12d3-a456-426614174003",
        "source_url":"https://chatgpt.com/c/123e4567-e89b-12d3-a456-426614174003", "title":"Synthetic owner action",
        "captured_at":"2026-10-28T08:00:00Z", "capture_method":"manual-dom", "completeness":"partial", "warnings":[],
        "messages":[{"id":"one","role":"user","text":"synthetic offline owner confirmation", "format":"plain", "attachments":[]}]
    })).unwrap();
    archive.save(capture).unwrap();
    let gate = SecurityGate::open(root.join("security-v1.json")).unwrap();
    gate.initialize(b"synthetic-offline-owner-password")
        .unwrap();
    (home, gate)
}

#[test]
fn owner_runtime_requires_confirmation_retains_source_and_reopens_after_restart() {
    let (home, gate) = fixture();
    let root = home.path().join("app");
    let before = inventory(&root);
    let lease = gate.require_unlocked().unwrap();
    let consent =
        ExplicitMigrationConsent::from_owner_typed_confirmation("ENCRYPT MY LEGACY VAULT").unwrap();
    let offline = OfflineLegacyRoot::acquire(&root, &gate, lease).unwrap();
    let prepared = offline.prepare(&gate, lease).unwrap();
    let code = prepared.recovery_code();
    assert!(prepared
        .confirm(&offline, &gate, lease, "incorrect", &consent)
        .is_err());
    assert!(!root.join("encrypted-vault-v1").exists());
    prepared
        .confirm(&offline, &gate, lease, &code, &consent)
        .unwrap();
    drop(prepared);
    drop(offline);
    gate.lock();
    let fresh = SecurityGate::open(root.join("security-v1.json")).unwrap();
    assert!(fresh.status().locked);
    fresh.unlock(b"synthetic-offline-owner-password").unwrap();
    let lease = fresh.require_unlocked().unwrap();
    let offline = OfflineLegacyRoot::acquire(&root, &fresh, lease).unwrap();
    offline
        .resume(&fresh, lease, &code, &consent, false)
        .unwrap();
    assert!(offline
        .resume(&fresh, lease, &code, &consent, true)
        .is_err());
    let target = root.join("encrypted-vault-v1");
    let manifest = VaultManifest::read(&target).unwrap();
    let crypto = CryptoVault::restore_with_dpapi(&fresh, lease, &manifest).unwrap();
    let archive = VaultCaptureStore::open(&target, crypto, &fresh, lease).unwrap();
    assert_eq!(archive.list("").unwrap().len(), 1);
    drop(offline);
    assert_eq!(inventory(&root), before);
    fresh.lock();
    assert!(archive.list("").is_err());
}

#[test]
fn live_sqlite_or_capture_writer_blocks_offline_owner_action() {
    let (home, gate) = fixture();
    let lease = gate.require_unlocked().unwrap();
    let root = home.path().join("app");
    let db = rusqlite::Connection::open(root.join("state.sqlite3")).unwrap();
    assert!(OfflineLegacyRoot::acquire(&root, &gate, lease).is_err());
    drop(db);
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(root.join("archive-v1/.archive.lock"))
        .unwrap();
    mind_place_security::vault_migration::lock_legacy_archive(&lock).unwrap();
    assert!(OfflineLegacyRoot::acquire(&root, &gate, lease).is_err());
    drop(lock);
    let offline = OfflineLegacyRoot::acquire(&root, &gate, lease).unwrap();
    assert!(fs::OpenOptions::new()
        .write(true)
        .open(root.join("state.sqlite3"))
        .is_err());
    assert!(fs::rename(root.join("state.sqlite3"), root.join("moved.sqlite3")).is_err());
    drop(offline);
    assert!(fs::OpenOptions::new()
        .write(true)
        .open(root.join("state.sqlite3"))
        .is_ok());
    gate.lock();
    assert!(OfflineLegacyRoot::acquire(&root, &gate, lease).is_err());
}

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

#[test]
fn synthetic_owner_migration_child() {
    let Some(home) = std::env::var_os("MP9C_SYNTHETIC_OWNER_MIGRATION") else {
        return;
    };
    let home = std::path::PathBuf::from(home);
    let root = home.join("app");
    let gate = SecurityGate::open(root.join("security-v1.json")).unwrap();
    gate.unlock(b"synthetic-offline-owner-password").unwrap();
    let lease = gate.require_unlocked().unwrap();
    let consent =
        ExplicitMigrationConsent::from_owner_typed_confirmation("ENCRYPT MY LEGACY VAULT").unwrap();
    let offline = OfflineLegacyRoot::acquire(&root, &gate, lease).unwrap();
    let recovery_file = home.join("synthetic-recovery.txt");
    if !recovery_file.exists() {
        let prepared = offline.prepare(&gate, lease).unwrap();
        let code = prepared.recovery_code();
        // Synthetic fixture only. Never pass recovery material through argv,
        // environment variables, diagnostics or the repository.
        fs::write(&recovery_file, code.as_bytes()).unwrap();
        prepared
            .confirm(&offline, &gate, lease, &code, &consent)
            .unwrap();
    } else {
        let code = zeroize::Zeroizing::new(fs::read_to_string(recovery_file).unwrap());
        offline
            .resume(&gate, lease, &code, &consent, false)
            .unwrap();
        let target = root.join("encrypted-vault-v1");
        if std::env::var_os("MP9C_SYNTHETIC_OWNER_VERIFY_READ").is_some() {
            for sidecar in [
                "state.sqlite3-wal",
                "state.sqlite3-shm",
                "archive-index.sqlite3-wal",
                "archive-index.sqlite3-shm",
            ] {
                assert!(
                    !target.join(sidecar).exists(),
                    "{sidecar} remained after offline.resume"
                );
            }
        }
        let manifest = VaultManifest::read(&target).unwrap();
        let crypto = CryptoVault::restore_with_dpapi(&gate, lease, &manifest).unwrap();
        let archive = VaultCaptureStore::open(&target, crypto, &gate, lease).unwrap();
        let items = archive.list("").unwrap();
        assert_eq!(items.len(), 1);
        if std::env::var_os("MP9C_SYNTHETIC_OWNER_VERIFY_READ").is_some() {
            let capture = archive.read(&items[0].id, None).unwrap();
            assert_eq!(capture.capture.title, "Synthetic owner action");
        }
    }
    // Real process termination: no Rust destructor-based lock cleanup.
    std::process::exit(77);
}

#[test]
fn owner_confirm_then_fresh_process_resume_preserves_every_source_byte() {
    let (home, gate) = fixture();
    gate.lock();
    drop(gate);
    let before = inventory(&home.path().join("app"));
    for _ in 0..3 {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "synthetic_owner_migration_child", "--nocapture"])
            .env("MP9C_SYNTHETIC_OWNER_MIGRATION", home.path())
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(77),
            "synthetic child failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(inventory(&home.path().join("app")), before);
    }
}

#[test]
fn repeated_fresh_process_resumes_with_encrypted_reads_and_quiesce() {
    let (home, gate) = fixture();
    gate.lock();
    drop(gate);
    let before = inventory(&home.path().join("app"));
    for cycle in 0..4 {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "synthetic_owner_migration_child", "--nocapture"])
            .env("MP9C_SYNTHETIC_OWNER_MIGRATION", home.path())
            .env("MP9C_SYNTHETIC_OWNER_VERIFY_READ", "1")
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(77),
            "cycle {cycle} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(inventory(&home.path().join("app")), before);
    }
}

fn confirm_fixture() -> (
    tempfile::TempDir,
    std::path::PathBuf,
    std::path::PathBuf,
    SecurityGate,
    zeroize::Zeroizing<String>,
    BTreeMap<std::path::PathBuf, Vec<u8>>,
) {
    let (home, gate) = fixture();
    let root = home.path().join("app");
    let before = inventory(&root);
    let lease = gate.require_unlocked().unwrap();
    let consent =
        ExplicitMigrationConsent::from_owner_typed_confirmation("ENCRYPT MY LEGACY VAULT").unwrap();
    let offline = OfflineLegacyRoot::acquire(&root, &gate, lease).unwrap();
    let prepared = offline.prepare(&gate, lease).unwrap();
    let code = prepared.recovery_code();
    prepared
        .confirm(&offline, &gate, lease, &code, &consent)
        .unwrap();
    let target = root.join("encrypted-vault-v1");
    (home, root, target, gate, code, before)
}

#[test]
fn owner_resume_fails_closed_on_dirty_wal_forged_shm_and_live_writers() {
    use mind_place_security::vault_fence::VaultWriterFence;

    let consent =
        ExplicitMigrationConsent::from_owner_typed_confirmation("ENCRYPT MY LEGACY VAULT").unwrap();

    // 1. Live writer / privacy fences and open read-only SQLite WAL connection block resume,
    //    and once closed, clean empty sidecars resume and quiesce.
    {
        let (_home, root, target, gate, code, before) = confirm_fixture();
        let lease = gate.require_unlocked().unwrap();
        let offline = OfflineLegacyRoot::acquire(&root, &gate, lease).unwrap();
        let writer = VaultWriterFence::writer(&target).unwrap();
        assert!(offline
            .resume(&gate, lease, &code, &consent, false)
            .is_err());
        drop(writer);

        let privacy = VaultWriterFence::privacy_reader(&target).unwrap();
        assert!(offline
            .resume(&gate, lease, &code, &consent, false)
            .is_err());
        drop(privacy);

        let manifest = VaultManifest::read(&target).unwrap();
        let crypto = CryptoVault::restore_with_dpapi(&gate, lease, &manifest).unwrap();
        let archive = VaultCaptureStore::open(&target, crypto, &gate, lease).unwrap();
        assert_eq!(archive.list("").unwrap().len(), 1);
        drop(archive);

        let db = rusqlite::Connection::open_with_flags(
            target.join("state.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .unwrap();
        let derived = VaultManifest::read(&target)
            .and_then(|m| CryptoVault::restore_with_dpapi(&gate, lease, &m))
            .unwrap()
            .derived_key(&gate, lease, b"state/sqlcipher/v1")
            .unwrap();
        let mut raw = [0u8; 32];
        raw.copy_from_slice(derived.as_ref());
        mind_place_state::CipherKey::new(raw, &gate, lease)
            .unlock(&db)
            .unwrap();
        db.execute_batch("BEGIN").unwrap();
        let _count: i64 = db
            .query_row("SELECT COUNT(*) FROM sqlite_master", [], |r| r.get(0))
            .unwrap();
        assert!(offline
            .resume(&gate, lease, &code, &consent, false)
            .is_err());
        drop(db);

        offline
            .resume(&gate, lease, &code, &consent, false)
            .unwrap();
        assert!(!target.join("state.sqlite3-wal").exists());
        assert!(!target.join("state.sqlite3-shm").exists());
        drop(offline);
        assert_eq!(inventory(&root), before);
    }

    // 2. Non-empty WAL sidecar is rejected and preserved verbatim (never deleted).
    {
        let (_home, root, target, gate, code, before) = confirm_fixture();
        let lease = gate.require_unlocked().unwrap();
        let manifest = VaultManifest::read(&target).unwrap();
        let crypto = CryptoVault::restore_with_dpapi(&gate, lease, &manifest).unwrap();
        let archive = VaultCaptureStore::open(&target, crypto, &gate, lease).unwrap();
        assert_eq!(archive.list("").unwrap().len(), 1);
        drop(archive);

        let wal = target.join("state.sqlite3-wal");
        let dirty_wal = vec![0x37u8; 4152];
        fs::write(&wal, &dirty_wal).unwrap();
        let offline = OfflineLegacyRoot::acquire(&root, &gate, lease).unwrap();
        assert!(offline
            .resume(&gate, lease, &code, &consent, false)
            .is_err());
        assert_eq!(fs::read(&wal).unwrap(), dirty_wal);
        drop(offline);
        assert_eq!(inventory(&root), before);
    }

    // 3. Forged / corrupted SHM sidecar is rejected without mutating files.
    {
        let (_home, root, target, gate, code, before) = confirm_fixture();
        let lease = gate.require_unlocked().unwrap();
        let manifest = VaultManifest::read(&target).unwrap();
        let crypto = CryptoVault::restore_with_dpapi(&gate, lease, &manifest).unwrap();
        let archive = VaultCaptureStore::open(&target, crypto, &gate, lease).unwrap();
        assert_eq!(archive.list("").unwrap().len(), 1);
        drop(archive);

        let shm = target.join("state.sqlite3-shm");
        assert!(shm.exists(), "expected real Windows read-only -shm sidecar");
        let mut forged = fs::read(&shm).unwrap();
        forged[256] ^= 0x55;
        fs::write(&shm, &forged).unwrap();
        let offline = OfflineLegacyRoot::acquire(&root, &gate, lease).unwrap();
        assert!(offline
            .resume(&gate, lease, &code, &consent, false)
            .is_err());
        assert_eq!(fs::read(&shm).unwrap(), forged);
        drop(offline);
        assert_eq!(inventory(&root), before);
    }

    // 4. Unexpected journal sidecar and destination DB tamper fail closed.
    {
        let (_home, root, target, gate, code, before) = confirm_fixture();
        let lease = gate.require_unlocked().unwrap();
        let offline = OfflineLegacyRoot::acquire(&root, &gate, lease).unwrap();
        fs::write(target.join("state.sqlite3-journal"), b"").unwrap();
        assert!(offline
            .resume(&gate, lease, &code, &consent, false)
            .is_err());
        fs::remove_file(target.join("state.sqlite3-journal")).unwrap();

        let state_path = target.join("state.sqlite3");
        let mut tampered = fs::read(&state_path).unwrap();
        tampered[0] ^= 1;
        fs::write(&state_path, tampered).unwrap();
        assert!(offline
            .resume(&gate, lease, &code, &consent, false)
            .is_err());
        drop(offline);
        assert_eq!(inventory(&root), before);
    }
}

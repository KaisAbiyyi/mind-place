#![cfg(feature = "sqlcipher-preview")]
//! Real production writer/restore paths with isolated synthetic data only.
use mind_place_archive::Capture;
use mind_place_index::{vault_archive::VaultCaptureStore, vault_online_backup::VaultOnlineBackup};
use mind_place_security::{
    vault::{ContentKind, CryptoVault, RecoverySecret, VaultManifest},
    SecurityGate, UnlockLease,
};
use mind_place_state::CipherKey;
use std::{fs, path::PathBuf, sync::Arc};

struct Fixture {
    home: tempfile::TempDir,
    gate: SecurityGate,
    lease: UnlockLease,
    vault: CryptoVault,
    recovery: RecoverySecret,
    manifest: VaultManifest,
    root: PathBuf,
    archive: Arc<VaultCaptureStore>,
}

fn fixture() -> Fixture {
    let home = tempfile::tempdir().unwrap();
    let gate = SecurityGate::open(home.path().join("gate.json")).unwrap();
    gate.initialize(b"synthetic-coherent-backup-password")
        .unwrap();
    let lease = gate.require_unlocked().unwrap();
    let (vault, recovery, manifest) = CryptoVault::provision(&gate, lease).unwrap();
    let root = home.path().join("vault");
    manifest.persist_new(&root).unwrap();
    let archive = Arc::new(VaultCaptureStore::open(&root, vault, &gate, lease).unwrap());
    archive.open_state_store().unwrap();
    archive.open_index_store().unwrap();
    archive.open_document_index().unwrap();
    let vault = CryptoVault::restore_with_recovery(&gate, lease, &manifest, &recovery).unwrap();
    Fixture {
        home,
        gate,
        lease,
        vault,
        recovery,
        manifest,
        root,
        archive,
    }
}

fn capture() -> Capture {
    serde_json::from_value(serde_json::json!({
        "schema_version":1, "provider":"chatgpt",
        "conversation_id":"123e4567-e89b-12d3-a456-426614174005",
        "source_url":"https://chatgpt.com/c/123e4567-e89b-12d3-a456-426614174005",
        "title":"Synthetic coordinated backup", "captured_at":"2026-10-28T09:00:00Z",
        "capture_method":"manual-dom", "completeness":"partial", "warnings":[],
        "messages":[{"id":"one", "role":"user", "text":"synthetic backup only", "format":"plain", "attachments":[]}]
    })).unwrap()
}

#[test]
fn incomplete_capture_or_invalid_authority_is_never_restored_as_a_valid_vault() {
    for mode in ["missing-capture", "wrong-authority"] {
        let f = fixture();
        let receipt = f.archive.save(capture()).unwrap();
        if mode == "missing-capture" {
            let name = f
                .vault
                .storage_name(
                    &f.gate,
                    f.lease,
                    ContentKind::Archive,
                    format!("capture/{}/{}", receipt.id, receipt.snapshot_id).as_bytes(),
                )
                .unwrap();
            fs::remove_file(f.root.join("archive").join(&name[..2]).join(name)).unwrap();
        } else {
            let derived = f
                .vault
                .derived_key(&f.gate, f.lease, b"state/sqlcipher/v1")
                .unwrap();
            let mut raw = [0u8; 32];
            raw.copy_from_slice(derived.as_ref());
            let key = CipherKey::new(raw, &f.gate, f.lease);
            let db = rusqlite::Connection::open(f.root.join("state.sqlite3")).unwrap();
            key.unlock(&db).unwrap();
            db.pragma_update(None, "application_id", 123).unwrap();
        }
        let backup = f.home.path().join("backup");
        let created = VaultOnlineBackup::create(&f.gate, f.lease, &f.vault, &f.root, &backup, 1);
        if created.is_ok() {
            let restored = f.home.path().join("restored");
            assert!(
                VaultOnlineBackup::restore(&f.gate, f.lease, &backup, &restored, &f.recovery)
                    .is_err(),
                "published invalid {mode}"
            );
            assert!(!restored.exists());
        } else {
            assert!(!backup.exists());
        }
    }
}

#[cfg(windows)]
#[test]
fn online_snapshot_waits_for_real_document_object_and_state_index_commit() {
    use std::{sync::mpsc, thread, time::Duration};
    fn committed_object_exists(root: &std::path::Path) -> bool {
        let Ok(shards) = fs::read_dir(root.join("document-objects")) else {
            return false;
        };
        shards.flatten().any(|shard| {
            fs::read_dir(shard.path())
                .into_iter()
                .flatten()
                .flatten()
                .any(|item| {
                    item.file_name()
                        .to_str()
                        .is_some_and(|name| name.len() == 64)
                })
        })
    }
    let f = fixture();
    let receipt = f.archive.save(capture()).unwrap();
    let documents = f.home.path().join("approved-documents");
    fs::create_dir(&documents).unwrap();
    fs::write(
        documents.join("fixture.md"),
        "synthetic_coherent_document_needle",
    )
    .unwrap();
    let state = f.archive.open_state_store().unwrap();
    let root = state.register_document_root(&documents).unwrap();
    state.approve_document_root(&root.id).unwrap();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let writer = f.archive.clone();
    let vault_root = f.root.clone();
    let root_id = root.id.clone();
    let document_writer = thread::spawn(move || {
        let state = writer.open_state_store().unwrap();
        let index = writer.open_document_index().unwrap();
        let mut paused = false;
        index
            .refresh_root(&state, &root_id, || {
                if !paused && committed_object_exists(&vault_root) {
                    paused = true;
                    ready_tx.send(()).unwrap();
                    resume_rx.recv_timeout(Duration::from_secs(30)).unwrap();
                }
                Ok(())
            })
            .unwrap()
    });
    ready_rx.recv_timeout(Duration::from_secs(30)).unwrap();
    let backup = f.home.path().join("online-backup");
    let destination = backup.clone();
    let backup_writer = f.archive.clone();
    let (done_tx, done_rx) = mpsc::channel();
    let snapshot = thread::spawn(move || {
        let result = backup_writer.online_backup(&destination, 4);
        done_tx.send(result.is_ok()).unwrap();
        result
    });
    // The production object has been published, but its state/index unit is
    // still held at a real guard boundary. Backup cannot cut this generation.
    assert!(matches!(
        done_rx.recv_timeout(Duration::from_millis(150)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    resume_tx.send(()).unwrap();
    assert_eq!(document_writer.join().unwrap().indexed_files, 1);
    let completed = done_rx.recv_timeout(Duration::from_secs(30)).unwrap();
    assert_eq!(snapshot.join().unwrap().unwrap().generation, 4);
    assert!(completed);
    for db in [
        "state.sqlite3",
        "index.sqlite3",
        "document-index.sqlite3",
        "archive-index.sqlite3",
    ] {
        assert!(backup.join("payload").join(db).is_file());
        assert!(!backup.join("payload").join(format!("{db}-wal")).exists());
        assert!(!backup.join("payload").join(format!("{db}-shm")).exists());
    }
    let restored = f.home.path().join("restored");
    VaultOnlineBackup::restore(&f.gate, f.lease, &backup, &restored, &f.recovery).unwrap();
    let vault =
        CryptoVault::restore_with_recovery(&f.gate, f.lease, &f.manifest, &f.recovery).unwrap();
    let archive = VaultCaptureStore::open(&restored, vault, &f.gate, f.lease).unwrap();
    assert_eq!(
        archive
            .read(&receipt.id, Some(&receipt.snapshot_id))
            .unwrap()
            .capture
            .messages[0]
            .text,
        "synthetic backup only"
    );
    let state = archive.open_state_store().unwrap();
    let index = archive.open_document_index().unwrap();
    let hits = index
        .search(&state, "synthetic_coherent_document_needle", 5, || Ok(()))
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert!(index
        .preview(&state, &hits[0].uri, &hits[0].version_id, || Ok(()))
        .unwrap()
        .text
        .contains("synthetic_coherent_document_needle"));
}

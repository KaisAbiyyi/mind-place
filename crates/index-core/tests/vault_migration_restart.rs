#![cfg(feature = "sqlcipher-preview")]
//! Real SQLCipher + AEAD domains, actual child-process exit and fresh recovery.
//! Every key, credential, attachment and document here is synthetic test data.
use mind_place_archive::{ArchiveStore, Attachment, Capture};
use mind_place_index::{
    vault_archive::VaultCaptureStore,
    vault_legacy_migration::{LegacyConversionCheckpoint, LegacyEncryptedConversion},
};
use mind_place_security::{
    vault::{ContentKind, CryptoVault, RecoverySecret, VaultManifest},
    vault_migration::{ExplicitMigrationConsent, LegacyMigrationPreflight, MigrationCheckpoint},
    vault_storage::ImmutableVaultStorage,
    SecurityGate,
};
use mind_place_state::{CipherKey, SourceDescriptor, SourceKey, StateStore};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

const PASSWORD: &[u8] = b"synthetic-only-migration-restart-password";
const PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aN3sAAAAASUVORK5CYII=";

fn consent() -> ExplicitMigrationConsent {
    ExplicitMigrationConsent::from_owner_typed_confirmation("ENCRYPT MY LEGACY VAULT").unwrap()
}

fn inventory(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

struct Fixture {
    home: tempfile::TempDir,
    source_id: String,
    captures: Vec<(String, String, Capture)>,
    selected_current: (String, String),
    before: BTreeMap<PathBuf, Vec<u8>>,
}

fn fixture() -> Fixture {
    let home = tempfile::tempdir().unwrap();
    let source = home.path().join("legacy");
    fs::create_dir(&source).unwrap();
    let state = StateStore::open(source.join("state.sqlite3")).unwrap();
    let source_id = state
        .register_source(&SourceDescriptor {
            provider: "chatgpt",
            source_namespace: "synthetic-account",
            source_kind: "conversation",
            key: SourceKey::External("synthetic-stable-source-key"),
        })
        .unwrap();
    drop(state);
    let db = rusqlite::Connection::open(source.join("state.sqlite3")).unwrap();
    db.execute_batch("CREATE TABLE synthetic_authority(id INTEGER PRIMARY KEY, text TEXT, blob BLOB, number REAL);
        INSERT INTO synthetic_authority VALUES(7, 'original', X'0001FF', 3.25);
        INSERT INTO synthetic_authority VALUES(900, NULL, NULL, -0.125);").unwrap();
    drop(db);
    let archive = ArchiveStore::open(source.join("archive-v1")).unwrap();
    let mut capture: Capture = serde_json::from_value(serde_json::json!({
        "schema_version": 1, "provider": "chatgpt",
        "conversation_id": "123e4567-e89b-12d3-a456-426614174003",
        "source_url": "https://chatgpt.com/c/123e4567-e89b-12d3-a456-426614174003",
        "title": "Synthetic migration revisions", "captured_at": "2026-10-28T08:00:00Z",
        "capture_method": "manual-dom", "completeness": "partial", "warnings": ["synthetic partial observation"],
        "messages": [{"id":"one", "role":"user", "text":"synthetic first revision", "format":"plain", "attachments":[]}]
    })).unwrap();
    capture.messages[0].attachments.push(Attachment {
        id: "image-one".into(),
        name: "synthetic.png".into(),
        mime_type: "image/png".into(),
        source_url: Some("https://example.invalid/no-network".into()),
        data_url: Some(PNG.into()),
        asset_path: None,
        unavailable_reason: None,
    });
    let mut captures = Vec::new();
    for revision in 0..2 {
        capture.captured_at = format!("2026-10-28T08:0{revision}:00Z");
        capture.messages[0].text = format!("synthetic immutable revision {revision}");
        let receipt = archive.save(capture.clone()).unwrap();
        let loaded = archive
            .read(&receipt.id, Some(&receipt.snapshot_id))
            .unwrap();
        captures.push((receipt.id, receipt.snapshot_id, loaded.capture));
    }
    let selected_current = {
        let (id, snapshot, _) = captures.last().unwrap();
        (id.clone(), snapshot.clone())
    };
    // A later legacy import is retained as a raw revision but must not displace
    // the manual capture selected by the legacy meta.json pointer.
    let mut later_import = capture.clone();
    later_import.capture_method = "legacy-import".into();
    later_import.captured_at = "2026-10-28T09:00:00Z".into();
    later_import.messages[0].text = "synthetic later legacy import".into();
    let imported = archive.save(later_import).unwrap();
    let still_selected = archive.read(&imported.id, None).unwrap();
    assert_eq!(still_selected.snapshot_id, selected_current.1);
    let imported_revision = archive
        .read(&imported.id, Some(&imported.snapshot_id))
        .unwrap();
    captures.push((
        imported.id,
        imported.snapshot_id,
        imported_revision.capture,
    ));
    let generic_raw = serde_json::to_vec(&serde_json::json!({
        "title":"Synthetic raw agent import",
        "messages":[{"role":"user", "content":"preserve original raw record", "authored_at":"2026-10-28T08:00:00Z"},
                    {"role":"assistant", "text":"missing source time stays missing", "model":"synthetic-model"}]
    })).unwrap();
    let imported = archive.import_generic("json", &generic_raw).unwrap();
    assert!(imported.raw_preserved);
    let receipt = imported.saved.unwrap();
    let loaded = archive
        .read(&receipt.id, Some(&receipt.snapshot_id))
        .unwrap();
    assert!(loaded.capture.messages[1].authored_at.is_none());
    captures.push((receipt.id, receipt.snapshot_id, loaded.capture));
    drop(archive);
    // Several AEAD chunks, including bytes that are not a valid text document.
    let bytes: Vec<u8> = (0..(2 * 1024 * 1024 + 17))
        .map(|i| (i % 251) as u8)
        .collect();
    let hash = format!("{:x}", Sha256::digest(&bytes));
    let dir = source.join("document-objects/v1/sha256").join(&hash[..2]);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(&hash), bytes).unwrap();
    let gate = SecurityGate::open(home.path().join("gate.json")).unwrap();
    gate.initialize(PASSWORD).unwrap();
    let lease = gate.require_unlocked().unwrap();
    let (vault, recovery, manifest) = CryptoVault::provision(&gate, lease).unwrap();
    manifest
        .persist_new(&home.path().join("authorization"))
        .unwrap();
    // Synthetic-only recovery material travels by a temp fixture file, never
    // by command line/environment, logs or Git. TempDir owns cleanup.
    fs::write(
        home.path().join("synthetic-recovery.txt"),
        recovery.export_code().as_bytes(),
    )
    .unwrap();
    let plan =
        LegacyMigrationPreflight::inspect(&source, &home.path().join("destination")).unwrap();
    plan.begin_owner_consented(&consent(), &gate, lease, &vault)
        .unwrap();
    let before = inventory(&source);
    Fixture {
        home,
        source_id,
        captures,
        selected_current,
        before,
    }
}

fn phase_name(phase: LegacyConversionCheckpoint) -> &'static str {
    match phase {
        LegacyConversionCheckpoint::StateCopied => "state",
        LegacyConversionCheckpoint::ArchiveImported => "archive",
        LegacyConversionCheckpoint::RawArchivePreserved => "raw",
        LegacyConversionCheckpoint::DocumentObjectsPreserved => "documents",
        LegacyConversionCheckpoint::Journal(MigrationCheckpoint::ConversionComplete) => "converted",
        LegacyConversionCheckpoint::Journal(MigrationCheckpoint::Verified) => "verified",
        LegacyConversionCheckpoint::Journal(MigrationCheckpoint::Published) => "published",
    }
}

#[test]
fn synthetic_migration_child() {
    let Some(root) = std::env::var_os("MP9B_SYNTHETIC_MIGRATION_ROOT") else {
        return;
    };
    let home = PathBuf::from(root);
    let phase = std::env::var("MP9B_SYNTHETIC_MIGRATION_PHASE").unwrap();
    let gate = SecurityGate::open(home.join("gate.json")).unwrap();
    gate.unlock(PASSWORD).unwrap();
    let lease = gate.require_unlocked().unwrap();
    let manifest = VaultManifest::read(&home.join("authorization")).unwrap();
    let code =
        zeroize::Zeroizing::new(fs::read_to_string(home.join("synthetic-recovery.txt")).unwrap());
    let recovery = RecoverySecret::from_export_code(&code).unwrap();
    let consent = consent();
    let converter = LegacyEncryptedConversion {
        gate: &gate,
        lease,
        consent: &consent,
        manifest: &manifest,
        recovery: &recovery,
    };
    let plan = converter
        .resume_owner_consented(&home.join("legacy"), &home.join("destination"))
        .unwrap();
    converter
        .execute_or_resume_with_checkpoint(&plan, |checkpoint| {
            if phase_name(checkpoint) == phase {
                println!("SYNTHETIC_MIGRATION_CHECKPOINT {phase}");
                std::io::stdout().flush().unwrap();
                // Actual abrupt exit: OS closes handles, Rust destructors do not run.
                std::process::exit(77);
            }
            Ok(())
        })
        .unwrap();
    panic!("Requested synthetic crash boundary was not reached");
}

fn crash(fixture: &Fixture, phase: &str) {
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "synthetic_migration_child", "--nocapture"])
        .env("MP9B_SYNTHETIC_MIGRATION_ROOT", fixture.home.path())
        .env("MP9B_SYNTHETIC_MIGRATION_PHASE", phase)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(77),
        "child did not reach crash boundary: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains(&format!("SYNTHETIC_MIGRATION_CHECKPOINT {phase}")));
}

#[test]
fn real_domains_recover_after_fresh_process_exit_at_every_boundary() {
    for phase in [
        "state",
        "archive",
        "raw",
        "documents",
        "converted",
        "verified",
        "published",
    ] {
        let f = fixture();
        crash(&f, phase);
        let home = f.home.path();
        let source = home.join("legacy");
        let destination = home.join("destination");
        let gate = SecurityGate::open(home.join("gate.json")).unwrap();
        gate.unlock(PASSWORD).unwrap();
        let lease = gate.require_unlocked().unwrap();
        let manifest = VaultManifest::read(&home.join("authorization")).unwrap();
        let code = zeroize::Zeroizing::new(
            fs::read_to_string(home.join("synthetic-recovery.txt")).unwrap(),
        );
        let recovery = RecoverySecret::from_export_code(&code).unwrap();
        let consent = consent();
        let converter = LegacyEncryptedConversion {
            gate: &gate,
            lease,
            consent: &consent,
            manifest: &manifest,
            recovery: &recovery,
        };
        let plan = converter
            .resume_owner_consented(&source, &destination)
            .unwrap();
        converter.execute_or_resume(&plan).unwrap();
        converter
            .execute_or_resume(
                &converter
                    .resume_owner_consented(&source, &destination)
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(inventory(&source), f.before, "source changed at {phase}");
        assert_eq!(
            serde_json::to_vec(&VaultManifest::read(&destination).unwrap()).unwrap(),
            serde_json::to_vec(&manifest).unwrap()
        );
        let vault = CryptoVault::restore_with_recovery(&gate, lease, &manifest, &recovery).unwrap();
        let storage = ImmutableVaultStorage::new(&destination, &vault, &gate, lease).unwrap();
        for (relative, bytes) in &f.before {
            let (kind, identity) = if relative.starts_with("archive-v1") {
                (
                    ContentKind::Archive,
                    format!(
                        "legacy-file/v1/{}",
                        relative.to_str().unwrap().replace('\\', "/")
                    ),
                )
            } else if relative.starts_with("document-objects") {
                (
                    ContentKind::DocumentObject,
                    format!(
                        "document/{}",
                        relative.file_name().unwrap().to_str().unwrap()
                    ),
                )
            } else {
                continue;
            };
            assert_eq!(
                storage
                    .read(&gate, lease, kind, identity.as_bytes())
                    .unwrap()
                    .as_slice(),
                bytes
            );
        }
        let derived = vault
            .derived_key(&gate, lease, b"state/sqlcipher/v1")
            .unwrap();
        let mut key = [0u8; 32];
        key.copy_from_slice(derived.as_ref());
        let key = CipherKey::new(key, &gate, lease);
        let db = rusqlite::Connection::open_with_flags(
            destination.join("state.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        key.unlock(&db).unwrap();
        let rows: Vec<(i64, Option<String>, Option<Vec<u8>>, f64)> = db
            .prepare("SELECT id,text,blob,number FROM synthetic_authority ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            rows,
            vec![
                (7, Some("original".into()), Some(vec![0, 1, 255]), 3.25),
                (900, None, None, -0.125)
            ]
        );
        drop(db);
        drop(storage);
        let archive = VaultCaptureStore::open(&destination, vault, &gate, lease).unwrap();
        for (id, snapshot, expected) in &f.captures {
            let read = archive.read(id, Some(snapshot)).unwrap();
            assert_eq!(&read.capture, expected);
            assert_eq!(
                read.summary.snapshot_count,
                f.captures
                    .iter()
                    .filter(|(other, _, _)| other == id)
                    .count()
            );
        }
        let selected = archive.read(&f.selected_current.0, None).unwrap();
        assert_eq!(selected.snapshot_id, f.selected_current.1);
        assert_eq!(selected.capture.capture_method, "manual-dom");
        let listed = archive.list("").unwrap();
        let selected_summary = listed
            .iter()
            .find(|summary| summary.id == f.selected_current.0)
            .unwrap();
        assert_eq!(selected_summary.captured_at, selected.capture.captured_at);
        assert_eq!(
            archive
                .open_state_store()
                .unwrap()
                .source(&f.source_id)
                .unwrap()
                .unwrap()
                .source_key,
            "synthetic-stable-source-key"
        );
        assert!(converter.rollback_to_legacy(&plan).is_err());
    }
}

#[test]
fn real_conversion_rejects_partial_tampered_and_changed_source_without_overwrite() {
    for mode in [
        "state-value",
        "raw-object",
        "source-mutation",
        "orphan-staging",
    ] {
        let f = fixture();
        crash(
            &f,
            if mode == "state-value" {
                "state"
            } else {
                "raw"
            },
        );
        let home = f.home.path();
        let source = home.join("legacy");
        let destination = home.join("destination");
        let stage = destination.with_extension("mp9b-migration-staging");
        let gate = SecurityGate::open(home.join("gate.json")).unwrap();
        gate.unlock(PASSWORD).unwrap();
        let lease = gate.require_unlocked().unwrap();
        let manifest = VaultManifest::read(&home.join("authorization")).unwrap();
        let code = zeroize::Zeroizing::new(
            fs::read_to_string(home.join("synthetic-recovery.txt")).unwrap(),
        );
        let recovery = RecoverySecret::from_export_code(&code).unwrap();
        let consent = consent();
        let converter = LegacyEncryptedConversion {
            gate: &gate,
            lease,
            consent: &consent,
            manifest: &manifest,
            recovery: &recovery,
        };
        match mode {
            "state-value" => {
                let vault =
                    CryptoVault::restore_with_recovery(&gate, lease, &manifest, &recovery).unwrap();
                let derived = vault
                    .derived_key(&gate, lease, b"state/sqlcipher/v1")
                    .unwrap();
                let mut raw = [0u8; 32];
                raw.copy_from_slice(derived.as_ref());
                let key = CipherKey::new(raw, &gate, lease);
                let db = rusqlite::Connection::open(stage.join("state.sqlite3")).unwrap();
                key.unlock(&db).unwrap();
                db.execute(
                    "UPDATE synthetic_authority SET text='changed same count' WHERE id=7",
                    [],
                )
                .unwrap();
            }
            "raw-object" => {
                let relative = inventory(&stage.join("archive"))
                    .into_keys()
                    .next()
                    .unwrap();
                let path = stage.join("archive").join(relative);
                let mut bytes = fs::read(&path).unwrap();
                let last = bytes.len() - 1;
                bytes[last] ^= 1;
                fs::write(path, bytes).unwrap();
            }
            "source-mutation" => {
                let db = rusqlite::Connection::open(source.join("state.sqlite3")).unwrap();
                db.execute(
                    "UPDATE synthetic_authority SET text='new source' WHERE id=7",
                    [],
                )
                .unwrap();
            }
            "orphan-staging" => fs::write(
                stage.join("orphan-partial.tmp"),
                b"synthetic interrupted writer",
            )
            .unwrap(),
            _ => unreachable!(),
        }
        let preserved = inventory(&source);
        match converter.resume_owner_consented(&source, &destination) {
            Err(_) => assert_eq!(mode, "source-mutation"),
            Ok(plan) => {
                assert!(
                    converter.execute_or_resume(&plan).is_err(),
                    "accepted bad {mode}"
                );
                converter.rollback_to_legacy(&plan).unwrap();
                assert!(destination
                    .with_extension("mp9b-migration-rollback")
                    .is_dir());
                assert!(converter
                    .resume_owner_consented(&source, &destination)
                    .is_err());
            }
        }
        assert!(!destination.exists());
        assert_eq!(inventory(&source), preserved);
        if mode != "source-mutation" {
            assert_eq!(preserved, f.before);
        }
    }
}

#[test]
fn migrated_attachment_reopens_without_access_to_plaintext_legacy_source() {
    let f = fixture();
    let home = f.home.path();
    let source = home.join("legacy");
    let destination = home.join("destination");
    let gate = SecurityGate::open(home.join("gate.json")).unwrap();
    gate.unlock(PASSWORD).unwrap();
    let lease = gate.require_unlocked().unwrap();
    let manifest = VaultManifest::read(&home.join("authorization")).unwrap();
    let code =
        zeroize::Zeroizing::new(fs::read_to_string(home.join("synthetic-recovery.txt")).unwrap());
    let recovery = RecoverySecret::from_export_code(&code).unwrap();
    let consent = consent();
    let converter = LegacyEncryptedConversion {
        gate: &gate,
        lease,
        consent: &consent,
        manifest: &manifest,
        recovery: &recovery,
    };
    let plan = converter
        .resume_owner_consented(&source, &destination)
        .unwrap();
    converter.execute_or_resume(&plan).unwrap();
    fs::rename(&source, home.join("preserved-offline-legacy")).unwrap();
    let vault = CryptoVault::restore_with_recovery(&gate, lease, &manifest, &recovery).unwrap();
    let archive = VaultCaptureStore::open(&destination, vault, &gate, lease).unwrap();
    let (id, _, capture) = &f.captures[0];
    let name = capture.messages[0].attachments[0]
        .asset_path
        .as_ref()
        .unwrap();
    assert_eq!(archive.asset(id, name).unwrap().data_url, PNG);
    assert!(archive.asset(id, "../state.sqlite3").is_err());
    assert!(archive.asset(&"0".repeat(64), name).is_err());
    gate.lock();
    assert!(archive.asset(id, name).is_err());
}

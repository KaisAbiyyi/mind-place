use mind_place_state::{
    ClaimEvidence, ClientCapability as Cap, MemoryClaim, SourceDescriptor, SourceKey, StateError,
    StateStore,
};

fn fixture() -> (
    tempfile::TempDir,
    StateStore,
    String,
    String,
    String,
    String,
) {
    let home = tempfile::tempdir().unwrap();
    let state = StateStore::open(home.path().join("state.sqlite3")).unwrap();
    let owner = "a".repeat(64);
    let other = "b".repeat(64);
    state
        .issue_client("client:v1:owner", "Owner", &owner, &[Cap::Admin])
        .unwrap();
    state
        .issue_client("client:v1:other", "Other", &other, &[Cap::Read])
        .unwrap();
    let s = state
        .register_source(&SourceDescriptor {
            provider: "chatgpt",
            source_namespace: "synthetic",
            source_kind: "conversation",
            key: SourceKey::External("source-a"),
        })
        .unwrap();
    let second = state
        .register_source(&SourceDescriptor {
            provider: "chatgpt",
            source_namespace: "synthetic",
            source_kind: "conversation",
            key: SourceKey::External("source-b"),
        })
        .unwrap();
    (home, state, owner, other, s, second)
}

fn claim(db: &StateStore, id: &str, sources: &[String]) {
    db.create_evidenced_claim(
        &MemoryClaim {
            id: id.to_owned(),
            statement: format!("synthetic {id}"),
            asserted_by: "user".into(),
            epistemic_origin: "explicit".into(),
            status: "active".into(),
        },
        &sources
            .iter()
            .map(|s| ClaimEvidence {
                source_ref: s.clone(),
                evidence_kind: "support".into(),
            })
            .collect::<Vec<_>>(),
    )
    .unwrap();
}

#[test]
fn one_time_owner_confirmation_keeps_shared_source_and_pending_receipt_after_restart() {
    let (_home, db, owner, other, src, shared) = fixture();
    claim(&db, "claim:shared", &[src.clone(), shared.clone()]);
    claim(&db, "claim:only", &[src.clone()]);
    assert!(matches!(
        db.preview_source_forget(&other, &src, "forbidden"),
        Err(StateError::MemoryDenied)
    ));
    let preview = db
        .preview_source_forget(&owner, &src, "Owner deletion request")
        .unwrap();
    assert_eq!((preview.affected_claims, preview.shared_claims), (2, 1));
    assert!(!preview.physical_inventory_complete);
    assert!(preview.retained_backup_review_required);
    assert!(db
        .confirm_source_forget(
            &owner,
            &src,
            "f".repeat(64).as_str(),
            &preview.preview_sha256
        )
        .is_err());
    assert!(!db.source_is_redacted(&src).unwrap());
    let progress = db
        .confirm_source_forget(
            &owner,
            &src,
            &preview.one_time_token,
            &preview.preview_sha256,
        )
        .unwrap();
    assert_eq!(progress.stage, "pending_independent_backup_reconciliation");
    assert!(!progress.physical_erase_verified);
    assert!(db
        .confirm_source_forget(
            &owner,
            &src,
            &preview.one_time_token,
            &preview.preview_sha256,
        )
        .is_err());
    assert!(db.claim_history("claim:shared").unwrap().is_none());
    assert!(db.source(&shared).unwrap().is_some());
    let restarted = StateStore::open(db.path()).unwrap();
    assert_eq!(
        restarted.source_forget_progress(&owner, &src).unwrap(),
        Some(progress)
    );
    assert!(matches!(
        restarted.source_forget_progress(&other, &src),
        Err(StateError::MemoryDenied)
    ));
}

#[test]
fn concurrent_evidence_change_invalidates_prepared_owner_confirmation() {
    let (_home, db, owner, _, src, _) = fixture();
    claim(&db, "claim:initial", &[src.clone()]);
    let preview = db
        .preview_source_forget(&owner, &src, "Owner deletion request")
        .unwrap();
    claim(&db, "claim:subsequent", &[src.clone()]);
    assert!(matches!(
        db.confirm_source_forget(
            &owner,
            &src,
            &preview.one_time_token,
            &preview.preview_sha256
        ),
        Err(StateError::MemoryDenied)
    ));
    assert!(!db.source_is_redacted(&src).unwrap());
    assert!(db.source_forget_progress(&owner, &src).unwrap().is_none());
}

#[cfg(feature = "sqlcipher-preview")]
#[test]
fn encrypted_forget_preview_is_bound_to_unlock_epoch_and_restart() {
    use mind_place_security::{vault::CryptoVault, SecurityGate};

    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("vault");
    let gate = SecurityGate::open(home.path().join("security.json")).unwrap();
    let password = b"synthetic owner unlock secret";
    gate.initialize(password).unwrap();
    let lease = gate.require_unlocked().unwrap();
    let (_vault, _recovery, manifest) = CryptoVault::provision(&gate, lease).unwrap();
    manifest.persist_new(&root).unwrap();
    let path = root.join("state.sqlite3");
    let key = [0x51u8; 32];
    let db = StateStore::open_encrypted(&path, key, &gate, lease).unwrap();
    let admin = "a".repeat(64);
    let peer_admin = "c".repeat(64);
    db.issue_client("client:v1:owner", "Owner", &admin, &[Cap::Admin])
        .unwrap();
    db.issue_client("client:v1:peer", "Peer", &peer_admin, &[Cap::Admin])
        .unwrap();
    let source = db
        .register_source(&SourceDescriptor {
            provider: "chatgpt",
            source_namespace: "synthetic",
            source_kind: "conversation",
            key: SourceKey::External("encrypted-epoch-fixture"),
        })
        .unwrap();
    claim(&db, "claim:encrypted", &[source.clone()]);
    let original = db
        .preview_source_forget(&admin, &source, "synthetic consent")
        .unwrap();
    assert!(matches!(
        db.confirm_source_forget(
            &peer_admin,
            &source,
            &original.one_time_token,
            &original.preview_sha256
        ),
        Err(StateError::MemoryDenied)
    ));
    gate.lock();
    assert!(db
        .confirm_source_forget(
            &admin,
            &source,
            &original.one_time_token,
            &original.preview_sha256
        )
        .is_err());
    drop(db);
    gate.unlock(password).unwrap();
    let new_lease = gate.require_unlocked().unwrap();
    assert_ne!(lease.epoch(), new_lease.epoch());
    let db = StateStore::open_encrypted(&path, key, &gate, new_lease).unwrap();
    assert!(matches!(
        db.confirm_source_forget(
            &admin,
            &source,
            &original.one_time_token,
            &original.preview_sha256
        ),
        Err(StateError::MemoryDenied)
    ));
    let renewed = db
        .preview_source_forget(&admin, &source, "fresh synthetic consent")
        .unwrap();
    let pending = db
        .confirm_source_forget(
            &admin,
            &source,
            &renewed.one_time_token,
            &renewed.preview_sha256,
        )
        .unwrap();
    assert!(!pending.physical_erase_verified);
    drop(db);
    let reopened = StateStore::open_encrypted(&path, key, &gate, new_lease).unwrap();
    assert_eq!(
        reopened.source_forget_progress(&admin, &source).unwrap(),
        Some(pending)
    );
    assert!(reopened.source(&source).unwrap().is_none());
}

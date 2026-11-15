#![cfg(feature = "sqlcipher-preview")]
//! MP-9C: source redaction/forget is refused on plaintext state, so these
//! regressions run against SQLCipher-encrypted vault state (synthetic keys).
mod encrypted {
    use mind_place_security::{SecurityGate, UnlockLease};
    use mind_place_state::StateStore;
    use std::{
        collections::HashMap,
        path::{Path, PathBuf},
        sync::{Mutex, OnceLock},
    };

    pub const KEY: [u8; 32] = [0x51; 32];

    fn gates() -> &'static Mutex<HashMap<PathBuf, (SecurityGate, UnlockLease)>> {
        static GATES: OnceLock<Mutex<HashMap<PathBuf, (SecurityGate, UnlockLease)>>> =
            OnceLock::new();
        GATES.get_or_init(|| Mutex::new(HashMap::new()))
    }

    /// Open (or reopen) encrypted state at `path`; the first open provisions
    /// a synthetic gate beside it (the privacy fence itself is covered by the
    /// vault-level index-core regressions).
    pub fn open_state(path: impl AsRef<Path>) -> StateStore {
        let path = path.as_ref().to_path_buf();
        let (gate, lease) = {
            let mut gates = gates().lock().unwrap_or_else(|e| e.into_inner());
            gates
                .entry(path.clone())
                .or_insert_with(|| {
                    let root = path.parent().unwrap();
                    let gate = SecurityGate::open(root.join("security.json")).unwrap();
                    gate.initialize(b"synthetic-mp9c-test-secret").unwrap();
                    let lease = gate.require_unlocked().unwrap();
                    (gate, lease)
                })
                .clone()
        };
        StateStore::open_encrypted(&path, KEY, &gate, lease).unwrap()
    }

    /// The synthetic gate and lease bound to `path` (after `open_state`).
    pub fn gate_for(path: &Path) -> (SecurityGate, UnlockLease) {
        gates().lock().unwrap_or_else(|e| e.into_inner())[path].clone()
    }

    pub const SECRET: &[u8] = b"synthetic-mp9c-test-secret";

    /// Raw keyed connection for adversarial tampering.
    pub fn raw(path: &Path) -> rusqlite::Connection {
        let db = rusqlite::Connection::open(path).unwrap();
        let hex: String = KEY.iter().map(|b| format!("{b:02x}")).collect();
        db.execute_batch(&format!("PRAGMA key = \"x'{hex}'\";"))
            .unwrap();
        db
    }
}
use encrypted::open_state;

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
    let state = open_state(home.path().join("state.sqlite3"));
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
    let restarted = open_state(db.path());
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

fn tamper(db: &StateStore, sql: &str) {
    let raw = encrypted::raw(db.path());
    assert!(raw.execute(sql, []).unwrap() >= 1);
}

#[test]
fn expired_one_time_confirmation_is_rejected_without_redaction() {
    let (_home, db, owner, _, src, _) = fixture();
    let preview = db
        .preview_source_forget(&owner, &src, "Owner deletion request")
        .unwrap();
    tamper(
        &db,
        "UPDATE settings SET value_json=json_set(value_json,'$.expires_at_ms',1)
         WHERE key LIKE 'privacy:forget-preview:v1:%'",
    );
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

#[test]
fn a_different_admin_cannot_use_the_owners_one_time_token() {
    let (_home, db, owner, _, src, _) = fixture();
    let intruder = "c".repeat(64);
    db.issue_client("client:v1:intruder", "Intruder", &intruder, &[Cap::Admin])
        .unwrap();
    let preview = db
        .preview_source_forget(&owner, &src, "Owner deletion request")
        .unwrap();
    assert!(matches!(
        db.confirm_source_forget(
            &intruder,
            &src,
            &preview.one_time_token,
            &preview.preview_sha256
        ),
        Err(StateError::MemoryDenied)
    ));
    assert!(!db.source_is_redacted(&src).unwrap());
    // Wrong preview fingerprint with the right token is also refused.
    assert!(matches!(
        db.confirm_source_forget(&owner, &src, &preview.one_time_token, &"0".repeat(64)),
        Err(StateError::MemoryDenied)
    ));
    assert!(!db.source_is_redacted(&src).unwrap());
    // The owner's own confirmation still works exactly once.
    db.confirm_source_forget(
        &owner,
        &src,
        &preview.one_time_token,
        &preview.preview_sha256,
    )
    .unwrap();
    assert!(db.source_is_redacted(&src).unwrap());
}

#[test]
fn concurrent_confirmations_of_one_token_commit_exactly_once() {
    let (_home, db, owner, _, src, _) = fixture();
    let preview = db
        .preview_source_forget(&owner, &src, "Owner deletion request")
        .unwrap();
    let path = db.path().to_path_buf();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
    // Independent connections, opened up front; only confirmation races.
    let stores = (0..4).map(|_| open_state(&path)).collect::<Vec<_>>();
    let workers = stores
        .into_iter()
        .map(|store| {
            let (owner, src) = (owner.clone(), src.clone());
            let (token, digest) = (
                preview.one_time_token.clone(),
                preview.preview_sha256.clone(),
            );
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store
                    .confirm_source_forget(&owner, &src, &token, &digest)
                    .is_ok()
            })
        })
        .collect::<Vec<_>>();
    let committed = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .filter(|ok| *ok)
        .count();
    assert_eq!(committed, 1);
    assert!(db.source_is_redacted(&src).unwrap());
}

#[test]
fn redaction_policy_probe_reports_tombstones_and_fails_closed_when_malformed() {
    let (_home, db, owner, _, src, second) = fixture();
    assert!(!db.has_source_redactions().unwrap());
    db.redact_source(&owner, &src, "Owner redaction").unwrap();
    assert!(db.has_source_redactions().unwrap());
    // A forged/garbled tombstone must not be read as "no redaction".
    tamper(
        &db,
        &format!(
            "INSERT INTO settings(key,value_json)
             VALUES('privacy:source-redaction:v1:{second}','{{\"version\":9}}')"
        ),
    );
    assert!(matches!(
        db.has_source_redactions(),
        Err(StateError::Corrupt)
    ));
}

/// Trusted desktop owner path: each step consumes a fresh re-authentication
/// bound to the live unlock epoch; owner and admin intents never cross.
#[test]
fn owner_reauth_proof_is_epoch_bound_single_use_and_actor_isolated() {
    let (home, db, admin, _, src, _) = fixture();
    let path = home.path().join("state.sqlite3");
    let (gate, lease) = encrypted::gate_for(&path);
    assert!(gate
        .reauthenticate_owner(lease, b"wrong-owner-secret")
        .is_err());
    std::thread::sleep(std::time::Duration::from_millis(1100));

    let preview = db
        .preview_source_forget_as_owner(
            gate.reauthenticate_owner(lease, encrypted::SECRET).unwrap(),
            &src,
            "Owner request",
        )
        .unwrap();
    // An admin credential cannot redeem the owner's token, and vice versa.
    assert!(matches!(
        db.confirm_source_forget(
            &admin,
            &src,
            &preview.one_time_token,
            &preview.preview_sha256
        ),
        Err(StateError::MemoryDenied)
    ));
    assert!(!db.source_is_redacted(&src).unwrap());

    // Lock and unlock: a proof for the new epoch cannot confirm the old preview.
    gate.lock();
    assert!(gate.reauthenticate_owner(lease, encrypted::SECRET).is_err());
    gate.unlock(encrypted::SECRET).unwrap();
    let fresh = gate.require_unlocked().unwrap();
    let reopened = StateStore::open_encrypted(&path, encrypted::KEY, &gate, fresh).unwrap();
    assert!(matches!(
        reopened.confirm_source_forget_as_owner(
            gate.reauthenticate_owner(fresh, encrypted::SECRET).unwrap(),
            &src,
            &preview.one_time_token,
            &preview.preview_sha256,
        ),
        Err(StateError::MemoryDenied)
    ));
    assert!(!reopened.source_is_redacted(&src).unwrap());

    // Fresh preview + fresh confirmation in the same epoch commits once.
    let preview = reopened
        .preview_source_forget_as_owner(
            gate.reauthenticate_owner(fresh, encrypted::SECRET).unwrap(),
            &src,
            "Owner request",
        )
        .unwrap();
    let progress = reopened
        .confirm_source_forget_as_owner(
            gate.reauthenticate_owner(fresh, encrypted::SECRET).unwrap(),
            &src,
            &preview.one_time_token,
            &preview.preview_sha256,
        )
        .unwrap();
    assert_eq!(progress.owner_client_id, "owner:local");
    assert!(!progress.physical_erase_verified);
    assert!(reopened.source_is_redacted(&src).unwrap());
    assert_eq!(
        reopened.source_forget_progress_as_owner(&src).unwrap(),
        Some(progress)
    );
    assert!(reopened
        .confirm_source_forget_as_owner(
            gate.reauthenticate_owner(fresh, encrypted::SECRET).unwrap(),
            &src,
            &preview.one_time_token,
            &preview.preview_sha256,
        )
        .is_err());
}

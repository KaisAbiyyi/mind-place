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
    ClaimEvidence, ClientCapability as Cap, MemoryClaim, MemoryWrite, RankingMode, RankingPolicy,
    SourceDescriptor, SourceKey, StateError, StateStore,
};

fn source(db: &StateStore, key: &str) -> String {
    db.register_source(&SourceDescriptor {
        provider: "chatgpt",
        source_namespace: "synthetic",
        source_kind: "conversation",
        key: SourceKey::External(key),
    })
    .unwrap()
}

fn claim(id: &str, statement: &str) -> MemoryClaim {
    MemoryClaim {
        id: id.to_owned(),
        statement: statement.to_owned(),
        asserted_by: "user".to_owned(),
        epistemic_origin: "explicit".to_owned(),
        status: "active".to_owned(),
    }
}

#[test]
fn owner_source_redaction_is_durable_hides_memory_and_blocks_reimport() {
    let temp = tempfile::tempdir().unwrap();
    let db = open_state(temp.path().join("state.sqlite3"));
    let owner = "a".repeat(64);
    let reader = "b".repeat(64);
    db.issue_client("client:v1:owner", "Owner", &owner, &[Cap::Admin])
        .unwrap();
    db.issue_client("client:v1:reader", "Reader", &reader, &[Cap::Read])
        .unwrap();
    let secret = source(&db, "secret-conversation");
    let public = source(&db, "unrelated-conversation");
    db.create_evidenced_claim(
        &claim("claim:secret", "Synthetic private fact"),
        &[ClaimEvidence {
            source_ref: secret.clone(),
            evidence_kind: "support".into(),
        }],
    )
    .unwrap();
    db.create_evidenced_claim(
        &claim("claim:public", "Public synthetic fact"),
        &[ClaimEvidence {
            source_ref: public.clone(),
            evidence_kind: "support".into(),
        }],
    )
    .unwrap();
    assert_eq!(db.claim_history("claim:secret").unwrap().unwrap().len(), 1);

    assert!(matches!(
        db.redact_source(&reader, &secret, "Owner-only action"),
        Err(StateError::MemoryDenied)
    ));
    let receipt = db
        .redact_source(&owner, &secret, "Privacy request")
        .unwrap();
    assert_eq!(receipt.source_id, secret);
    assert_eq!(
        db.redact_source(&owner, &secret, "Repeated call").unwrap(),
        receipt
    );
    assert!(db.source_is_redacted(&secret).unwrap());
    assert!(db.source(&secret).unwrap().is_none());
    assert!(db.claim_history("claim:secret").unwrap().is_none());
    assert!(db.claim_as_of("claim:secret", i64::MAX).unwrap().is_none());
    assert!(db
        .memory_inspection_detail("claim:secret")
        .unwrap()
        .is_none());
    assert!(db
        .memory_inspection_overview("Synthetic private")
        .unwrap()
        .is_empty());
    assert!(db
        .rank_claims(
            &["claim:secret"],
            &RankingPolicy {
                mode: RankingMode::Balanced,
                as_of_ms: None,
                scope_id: None,
                limit: 10,
            }
        )
        .unwrap()
        .is_empty());
    assert!(matches!(
        db.register_source(&SourceDescriptor {
            provider: "chatgpt",
            source_namespace: "synthetic",
            source_kind: "conversation",
            key: SourceKey::External("secret-conversation"),
        }),
        Err(StateError::MemoryDenied)
    ));
    assert!(matches!(
        db.create_evidenced_claim(
            &claim("claim:blocked", "Should not insert"),
            &[ClaimEvidence {
                source_ref: secret.clone(),
                evidence_kind: "support".into()
            }]
        ),
        Err(StateError::MemoryDenied)
    ));
    assert!(db.claim_history("claim:public").unwrap().is_some());
    assert_eq!(db.source(&public).unwrap().unwrap().id, public);

    // Existing state v9 remains readable without an automatic schema change.
    let reopened = open_state(db.path());
    assert!(reopened.source_is_redacted(&secret).unwrap());
    assert!(reopened.claim_history("claim:secret").unwrap().is_none());
    assert!(reopened.claim_history("claim:public").unwrap().is_some());
    reopened.revoke_client("client:v1:owner").unwrap();
    assert!(matches!(
        reopened.redact_source(&owner, &public, "Revoked owner"),
        Err(StateError::MemoryDenied)
    ));
    assert!(!reopened.source_is_redacted(&public).unwrap());
}

#[test]
fn redacted_sources_reject_late_authenticated_memory_writes() {
    let temp = tempfile::tempdir().unwrap();
    let db = open_state(temp.path().join("state.sqlite3"));
    let owner = "a".repeat(64);
    let writer = "b".repeat(64);
    db.issue_client("client:v1:owner", "Owner", &owner, &[Cap::Admin])
        .unwrap();
    db.issue_client("client:v1:writer", "Writer", &writer, &[Cap::Write])
        .unwrap();
    let id = source(&db, "redacted-evidence");
    db.redact_source(&owner, &id, "No later ingestion").unwrap();
    let evidence = vec![ClaimEvidence {
        source_ref: id,
        evidence_kind: "support".into(),
    }];
    let record = claim("claim:blocked", "Not allowed");
    assert!(matches!(
        db.apply_memory_write(
            &writer,
            "source-redaction-001",
            &MemoryWrite::Create {
                claim: &record,
                scope_id: None,
                evidence: &evidence,
            }
        ),
        Err(StateError::MemoryDenied)
    ));
    assert!(db.claim_history("claim:blocked").unwrap().is_none());
}

#[test]
fn redacted_relation_source_does_not_break_global_inspection_or_leak_hidden_neighbor() {
    let temp = tempfile::tempdir().unwrap();
    let db = open_state(temp.path().join("state.sqlite3"));
    let owner = "a".repeat(64);
    db.issue_client("client:v1:owner", "Owner", &owner, &[Cap::Admin])
        .unwrap();
    let public = source(&db, "public-thread");
    let secret = source(&db, "secret-thread");
    let extra_secret = source(&db, "extra-secret-thread");

    db.create_evidenced_claim(
        &claim("claim:public", "Public claim"),
        &[ClaimEvidence {
            source_ref: public.clone(),
            evidence_kind: "support".into(),
        }],
    )
    .unwrap();
    db.create_evidenced_claim(
        &claim("claim:secret", "Secret claim"),
        &[ClaimEvidence {
            source_ref: secret.clone(),
            evidence_kind: "support".into(),
        }],
    )
    .unwrap();
    db.create_evidenced_claim(
        &claim("claim:third", "Third claim"),
        &[ClaimEvidence {
            source_ref: public.clone(),
            evidence_kind: "support".into(),
        }],
    )
    .unwrap();
    // Public relation from public -> secret (tests P2-1 hidden-neighbor filtering in detail),
    // plus a redacted-source relation between secret <-> third (tests P1-2 load_relations
    // not failing global inspection/ranking for unrelated surviving claims).
    db.add_claim_relation(
        "claim:public",
        "claim:secret",
        mind_place_state::RelationKind::Supports,
        &public,
        "Public link to secret claim",
    )
    .unwrap();
    db.add_claim_relation(
        "claim:secret",
        "claim:third",
        mind_place_state::RelationKind::Contradicts,
        &extra_secret,
        "Secret link",
    )
    .unwrap();

    db.redact_source(&owner, &secret, "Forget secret").unwrap();
    db.redact_source(&owner, &extra_secret, "Forget relation source")
        .unwrap();

    let overview = db.memory_inspection_overview("").unwrap();
    assert_eq!(overview.len(), 1);
    assert_eq!(overview[0].claim.id, "claim:public");

    let ranked = db
        .rank_claims(
            &["claim:secret", "claim:public"],
            &RankingPolicy {
                mode: RankingMode::Challenge,
                as_of_ms: None,
                scope_id: None,
                limit: 10,
            },
        )
        .unwrap();
    assert_eq!(ranked.len(), 1);
    assert_eq!(ranked[0].claim.id, "claim:public");
    assert!(ranked[0].related_claim_ids.is_empty());

    let detail = db
        .memory_inspection_detail("claim:public")
        .unwrap()
        .expect("public claim detail stays readable");
    assert!(detail.relations.is_empty());
    assert!(db
        .memory_inspection_detail("claim:secret")
        .unwrap()
        .is_none());
    assert!(db
        .memory_inspection_detail("claim:third")
        .unwrap()
        .is_none());
}

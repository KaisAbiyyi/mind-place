use mind_place_state::{
    ClaimEvidence, ClientCapability as Cap, MemoryClaim, MemoryWrite,
    RankingMode, RankingPolicy, SourceDescriptor, SourceKey, StateError, StateStore,
};

fn source(db: &StateStore, key: &str) -> String {
    db.register_source(&SourceDescriptor {
        provider: "chatgpt",
        source_namespace: "synthetic",
        source_kind: "conversation",
        key: SourceKey::External(key),
    }).unwrap()
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
    let db = StateStore::open(temp.path().join("state.sqlite3")).unwrap();
    let owner = "a".repeat(64);
    let reader = "b".repeat(64);
    db.issue_client("client:v1:owner", "Owner", &owner, &[Cap::Admin]).unwrap();
    db.issue_client("client:v1:reader", "Reader", &reader, &[Cap::Read]).unwrap();
    let secret = source(&db, "secret-conversation");
    let public = source(&db, "unrelated-conversation");
    db.create_evidenced_claim(&claim("claim:secret", "Synthetic private fact"), &[
        ClaimEvidence { source_ref: secret.clone(), evidence_kind: "support".into() },
    ]).unwrap();
    db.create_evidenced_claim(&claim("claim:public", "Public synthetic fact"), &[
        ClaimEvidence { source_ref: public.clone(), evidence_kind: "support".into() },
    ]).unwrap();
    assert_eq!(db.claim_history("claim:secret").unwrap().unwrap().len(), 1);

    assert!(matches!(
        db.redact_source(&reader, &secret, "Owner-only action"),
        Err(StateError::MemoryDenied)
    ));
    let receipt = db.redact_source(&owner, &secret, "Privacy request").unwrap();
    assert_eq!(receipt.source_id, secret);
    assert_eq!(db.redact_source(&owner, &secret, "Repeated call").unwrap(), receipt);
    assert!(db.source_is_redacted(&secret).unwrap());
    assert!(db.source(&secret).unwrap().is_none());
    assert!(db.claim_history("claim:secret").unwrap().is_none());
    assert!(db.claim_as_of("claim:secret", i64::MAX).unwrap().is_none());
    assert!(db.memory_inspection_detail("claim:secret").unwrap().is_none());
    assert!(db.memory_inspection_overview("Synthetic private").unwrap().is_empty());
    assert!(db.rank_claims(&["claim:secret"], &RankingPolicy {
        mode: RankingMode::Balanced, as_of_ms: None, scope_id: None, limit: 10,
    }).unwrap().is_empty());
    assert!(matches!(
        db.register_source(&SourceDescriptor {
            provider: "chatgpt", source_namespace: "synthetic",
            source_kind: "conversation", key: SourceKey::External("secret-conversation"),
        }),
        Err(StateError::MemoryDenied)
    ));
    assert!(matches!(
        db.create_evidenced_claim(&claim("claim:blocked", "Should not insert"), &[
            ClaimEvidence { source_ref: secret.clone(), evidence_kind: "support".into() }
        ]), Err(StateError::MemoryDenied)
    ));
    assert!(db.claim_history("claim:public").unwrap().is_some());
    assert_eq!(db.source(&public).unwrap().unwrap().id, public);

    // Existing state v9 remains readable without an automatic schema change.
    let reopened = StateStore::open(db.path()).unwrap();
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
    let db = StateStore::open(temp.path().join("state.sqlite3")).unwrap();
    let owner = "a".repeat(64);
    let writer = "b".repeat(64);
    db.issue_client("client:v1:owner", "Owner", &owner, &[Cap::Admin]).unwrap();
    db.issue_client("client:v1:writer", "Writer", &writer, &[Cap::Write]).unwrap();
    let id = source(&db, "redacted-evidence");
    db.redact_source(&owner, &id, "No later ingestion").unwrap();
    let evidence = vec![ClaimEvidence { source_ref: id, evidence_kind: "support".into() }];
    let record = claim("claim:blocked", "Not allowed");
    assert!(matches!(
        db.apply_memory_write(&writer, "source-redaction-001", &MemoryWrite::Create {
            claim: &record, scope_id: None, evidence: &evidence,
        }), Err(StateError::MemoryDenied)
    ));
    assert!(db.claim_history("claim:blocked").unwrap().is_none());
}

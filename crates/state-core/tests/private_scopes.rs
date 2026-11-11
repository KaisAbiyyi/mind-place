use mind_place_state::{
    ClaimEvidence, ClientCapability as Cap, MemoryClaim, MemoryWrite, RankingMode, RankingPolicy,
    SourceDescriptor, SourceKey, StateError, StateStore,
};

fn fixture() -> (tempfile::TempDir, StateStore, String) {
    let temp = tempfile::tempdir().unwrap();
    let db = StateStore::open(temp.path().join("state.sqlite3")).unwrap();
    db.issue_client("client:v1:owner", "Owner", &"a".repeat(64), &[Cap::Admin])
        .unwrap();
    db.issue_client(
        "client:v1:writer",
        "Provider",
        &"b".repeat(64),
        &[Cap::Write],
    )
    .unwrap();
    db.issue_client(
        "client:v1:owner_writer",
        "Trusted owner editor",
        &"c".repeat(64),
        &[Cap::Write, Cap::Admin],
    )
    .unwrap();
    db.issue_client("client:v1:reader", "Reader", &"d".repeat(64), &[Cap::Read])
        .unwrap();
    let source = db
        .register_source(&SourceDescriptor {
            provider: "chatgpt",
            source_namespace: "synthetic",
            source_kind: "conversation",
            key: SourceKey::External("privacy-fixture"),
        })
        .unwrap();
    (temp, db, source)
}

fn claim(id: &str, statement: &str) -> MemoryClaim {
    MemoryClaim {
        id: id.into(),
        statement: statement.into(),
        asserted_by: "user".into(),
        epistemic_origin: "explicit".into(),
        status: "active".into(),
    }
}

#[test]
fn private_scope_inventory_is_owner_only_bounded_and_durable() {
    let (_temp, db, _source) = fixture();
    assert!(matches!(
        db.create_private_scope(&"b".repeat(64), "scope:private", "Personal", None),
        Err(StateError::MemoryDenied)
    ));
    assert!(matches!(
        db.list_private_scopes(&"d".repeat(64), None, 50),
        Err(StateError::MemoryDenied)
    ));
    assert!(db.list_private_scopes(&"a".repeat(64), None, 0).is_err());
    assert!(db.list_private_scopes(&"a".repeat(64), None, 51).is_err());
    assert!(db
        .create_private_scope(&"a".repeat(64), "private", "Invalid ID", None)
        .is_err());
    assert!(db
        .create_private_scope(
            &"a".repeat(64),
            "scope:child",
            "Child",
            Some("scope:missing")
        )
        .is_err());

    let root = db
        .create_private_scope(&"a".repeat(64), "scope:private", " Personal ", None)
        .unwrap();
    assert_eq!(root.name, "Personal");
    let child = db
        .create_private_scope(
            &"a".repeat(64),
            "scope:private.child",
            "Child",
            Some("scope:private"),
        )
        .unwrap();
    assert_eq!(child.parent_scope_id.as_deref(), Some("scope:private"));
    assert!(db
        .create_private_scope(&"a".repeat(64), "scope:private", "Hijack", None)
        .is_err());
    let all = db.list_private_scopes(&"a".repeat(64), None, 50).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(
        db.list_private_scopes(&"a".repeat(64), Some("scope:private"), 1)
            .unwrap(),
        vec![child]
    );

    let reopened = StateStore::open(db.path()).unwrap();
    assert_eq!(
        reopened
            .list_private_scopes(&"a".repeat(64), None, 50)
            .unwrap(),
        all
    );
    db.revoke_client("client:v1:owner").unwrap();
    assert!(matches!(
        db.list_private_scopes(&"a".repeat(64), None, 50),
        Err(StateError::MemoryDenied)
    ));
    assert!(matches!(
        db.create_private_scope(&"a".repeat(64), "scope:after_revoke", "No", None),
        Err(StateError::MemoryDenied)
    ));
}

#[test]
fn scoped_memory_requires_same_credential_write_and_admin_and_stays_isolated() {
    let (_temp, db, source) = fixture();
    db.create_private_scope(&"a".repeat(64), "scope:private", "Personal", None)
        .unwrap();
    let secret = claim("claim:secret", "Sensitive test only");
    let proof = vec![ClaimEvidence {
        source_ref: source.clone(),
        evidence_kind: "support".into(),
    }];
    let scoped = MemoryWrite::Create {
        claim: &secret,
        scope_id: Some("scope:private"),
        evidence: &proof,
    };
    assert!(matches!(
        db.apply_memory_write(&"b".repeat(64), "scope-create-writer-01", &scoped),
        Err(StateError::MemoryDenied)
    ));
    assert!(matches!(
        db.apply_memory_write(&"a".repeat(64), "scope-create-admin-01", &scoped),
        Err(StateError::MemoryDenied)
    ));
    assert!(db.claim_history("claim:secret").unwrap().is_none());

    db.apply_memory_write(&"c".repeat(64), "scope-create-owner-01", &scoped)
        .unwrap();
    assert_eq!(
        db.apply_memory_write(&"c".repeat(64), "scope-create-owner-01", &scoped)
            .unwrap()
            .claim_id,
        "claim:secret"
    );
    assert!(db.memory_inspection_overview("").unwrap().is_empty());
    assert!(db
        .memory_inspection_detail("claim:secret")
        .unwrap()
        .is_none());
    let global = db
        .rank_claims(
            &["claim:secret"],
            &RankingPolicy {
                mode: RankingMode::Balanced,
                as_of_ms: None,
                scope_id: None,
                limit: 10,
            },
        )
        .unwrap();
    assert!(global.is_empty());

    // Scope-aware ranking is only a trusted-Rust policy, not a provider grant.
    let scoped_read = db
        .rank_claims(
            &["claim:secret"],
            &RankingPolicy {
                mode: RankingMode::Balanced,
                as_of_ms: None,
                scope_id: Some("scope:private".into()),
                limit: 10,
            },
        )
        .unwrap();
    assert_eq!(scoped_read.len(), 1);
    assert_eq!(scoped_read[0].claim.id, "claim:secret");

    let successor = claim("claim:corrected", "Corrected sensitive test");
    let update = MemoryWrite::Supersede {
        previous_id: "claim:secret",
        successor: &successor,
        scope_id: Some("scope:private"),
        evidence: &proof,
        reason: "Correction",
    };
    assert!(matches!(
        db.apply_memory_write(&"b".repeat(64), "scope-update-writer-01", &update),
        Err(StateError::MemoryDenied)
    ));
    db.apply_memory_write(&"c".repeat(64), "scope-update-owner-01", &update)
        .unwrap();
    assert!(db
        .memory_inspection_detail("claim:corrected")
        .unwrap()
        .is_none());
    db.revoke_client("client:v1:owner_writer").unwrap();
    assert!(matches!(
        db.apply_memory_write(&"c".repeat(64), "scope-update-owner-01", &update),
        Err(StateError::MemoryDenied)
    ));
}

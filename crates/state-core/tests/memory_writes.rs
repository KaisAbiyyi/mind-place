use mind_place_state::{
    ClaimEvidence, ClientCapability as Cap, MemoryClaim, MemoryWrite, RankingMode,
    RankingPolicy, SourceDescriptor, SourceKey, StateError, StateStore,
};
use rusqlite::Connection;

fn claim(id: &str, text: &str) -> MemoryClaim {
    MemoryClaim {
        id: id.into(),
        statement: text.into(),
        asserted_by: "user".into(),
        epistemic_origin: "explicit".into(),
        status: "active".into(),
    }
}
fn evidence(source: &str) -> Vec<ClaimEvidence> {
    vec![ClaimEvidence { source_ref: source.into(), evidence_kind: "support".into() }]
}
fn fixture() -> (tempfile::TempDir, StateStore, String) {
    let dir = tempfile::tempdir().unwrap();
    let db = StateStore::open(dir.path().join("state.sqlite3")).unwrap();
    let source = db.register_source(&SourceDescriptor {
        provider: "chatgpt",
        source_namespace: "private",
        source_kind: "conversation",
        key: SourceKey::External("source-1"),
    }).unwrap();
    db.issue_client("client:v1:writer", "Writer", &"a".repeat(64), &[Cap::Write])
        .unwrap();
    db.issue_client("client:v1:admin", "Owner", &"b".repeat(64), &[Cap::Admin])
        .unwrap();
    db.issue_client("client:v1:reader", "Reader", &"c".repeat(64), &[Cap::Read])
        .unwrap();
    (dir, db, source)
}

#[test]
fn creates_and_replays_exactly_once_across_reopen() {
    let (_dir, db, source) = fixture();
    let mem = claim("claim:alpha", "Use source-first evidence");
    let proof = evidence(&source);
    let op = MemoryWrite::Create { claim: &mem, scope_id: None, evidence: &proof };
    let key = "mp5c-create-alpha-001";
    let first = db.apply_memory_write(&"a".repeat(64), key, &op).unwrap();
    let second = db.apply_memory_write(&"a".repeat(64), key, &op).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.revision, 1);
    assert_eq!(db.claim_history("claim:alpha").unwrap().unwrap().len(), 1);
    let reloaded = StateStore::open(db.path()).unwrap();
    assert_eq!(reloaded.apply_memory_write(&"a".repeat(64), key, &op).unwrap(), first);
    assert_eq!(reloaded.claim_history("claim:alpha").unwrap().unwrap().len(), 1);
    assert_eq!(reloaded.source(&source).unwrap().unwrap().id, source);
}

#[test]
fn denies_missing_or_revoked_capabilities_and_reused_keys() {
    let (_dir, db, source) = fixture();
    let mem = claim("claim:alpha", "Private memory");
    let proof = evidence(&source);
    let op = MemoryWrite::Create { claim: &mem, scope_id: None, evidence: &proof };
    let key = "mp5c-create-alpha-002";
    assert!(matches!(db.apply_memory_write(&"c".repeat(64), key, &op), Err(StateError::MemoryDenied)));
    assert!(matches!(db.apply_memory_write(&"b".repeat(64), key, &op), Err(StateError::MemoryDenied)));
    assert!(db.claim_history("claim:alpha").unwrap().is_none());
    db.apply_memory_write(&"a".repeat(64), key, &op).unwrap();
    let modified = claim("claim:other", "Different content");
    let mismatch = MemoryWrite::Create { claim: &modified, scope_id: None, evidence: &proof };
    assert!(matches!(db.apply_memory_write(&"a".repeat(64), key, &mismatch), Err(StateError::InvalidValue)));
    assert!(db.claim_history("claim:other").unwrap().is_none());

    db.revoke_client("client:v1:writer").unwrap();
    assert!(matches!(db.apply_memory_write(&"a".repeat(64), key, &op), Err(StateError::MemoryDenied)));
    assert!(matches!(db.apply_memory_write(&"a".repeat(64), "mp5c-new-operation-01", &mismatch), Err(StateError::MemoryDenied)));
}

#[test]
fn reject_duplicate_truth_and_invalid_evidence_atomically() {
    let (_dir, db, source) = fixture();
    let a = claim("claim:one", "Prefer SQLite authority");
    let proof = evidence(&source);
    let good = MemoryWrite::Create { claim: &a, scope_id: None, evidence: &proof };
    db.apply_memory_write(&"a".repeat(64), "mp5c-create-one-0001", &good).unwrap();
    let b = claim("claim:two", " prefer sqlite authority ");
    let duplicate = MemoryWrite::Create { claim: &b, scope_id: None, evidence: &proof };
    assert!(matches!(db.apply_memory_write(&"a".repeat(64), "mp5c-create-two-0002", &duplicate), Err(StateError::InvalidValue)));
    assert!(db.claim_history("claim:two").unwrap().is_none());

    let no_proof = claim("claim:three", "New text");
    let invalid = MemoryWrite::Create { claim: &no_proof, scope_id: None, evidence: &[] };
    assert!(db.apply_memory_write(&"a".repeat(64), "mp5c-create-three-3", &invalid).is_err());
    assert!(db.claim_history("claim:three").unwrap().is_none());
}

#[test]
fn supersede_is_atomic_and_old_memory_cannot_become_current() {
    let (_dir, db, source) = fixture();
    let proof = evidence(&source);
    let old = claim("claim:old", "Model A is preferred");
    db.apply_memory_write(&"a".repeat(64), "mp5c-create-old-0001", &MemoryWrite::Create {
        claim: &old, scope_id: None, evidence: &proof,
    }).unwrap();
    let new = claim("claim:new", "Model B is preferred");
    let op = MemoryWrite::Supersede {
        previous_id: "claim:old", successor: &new,
        scope_id: None, evidence: &proof, reason: "User changed preference",
    };
    let receipt = db.apply_memory_write(&"a".repeat(64), "mp5c-supersede-0001", &op).unwrap();
    assert_eq!(db.apply_memory_write(&"a".repeat(64), "mp5c-supersede-0001", &op).unwrap(), receipt);
    assert_eq!(db.claim_as_of("claim:old", i64::MAX).unwrap().unwrap().status, "superseded");
    assert_eq!(db.claim_as_of("claim:new", i64::MAX).unwrap().unwrap().status, "active");
    assert_eq!(db.claim_history("claim:old").unwrap().unwrap().last().unwrap().superseded_by.as_deref(), Some("claim:new"));
    let ranked = db.rank_claims(&["claim:old", "claim:new"], &RankingPolicy {
        mode: RankingMode::Balanced, as_of_ms: None, scope_id: None, limit: 10,
    }).unwrap();
    assert_eq!(ranked.len(), 1);
    assert_eq!(ranked[0].claim.id, "claim:new");

    let bad = claim("claim:bad", "No valid source");
    assert!(db.apply_memory_write(&"a".repeat(64), "mp5c-supersede-0002", &MemoryWrite::Supersede {
        previous_id: "claim:new", successor: &bad,
        scope_id: None, evidence: &[], reason: "Bad",
    }).is_err());
    assert!(db.claim_history("claim:bad").unwrap().is_none());
    assert_eq!(db.claim_as_of("claim:new", i64::MAX).unwrap().unwrap().status, "active");
}

#[test]
fn forget_requires_admin_and_hides_all_as_of_context_without_deleting_raw() {
    let (_dir, db, source) = fixture();
    let proof = evidence(&source);
    let c = claim("claim:forgotten", "Old personal preference");
    db.apply_memory_write(&"a".repeat(64), "mp5c-create-forget-01", &MemoryWrite::Create {
        claim: &c, scope_id: None, evidence: &proof,
    }).unwrap();
    let snapshot = db.claim_as_of("claim:forgotten", i64::MAX).unwrap().unwrap();
    let op = MemoryWrite::Forget { claim_id: "claim:forgotten", reason: "Owner explicitly requests forget" };
    assert!(matches!(db.apply_memory_write(&"a".repeat(64), "mp5c-forget-owner-01", &op), Err(StateError::MemoryDenied)));
    let receipt = db.apply_memory_write(&"b".repeat(64), "mp5c-forget-owner-01", &op).unwrap();
    assert_eq!(db.apply_memory_write(&"b".repeat(64), "mp5c-forget-owner-01", &op).unwrap(), receipt);
    assert!(db.claim_as_of("claim:forgotten", snapshot.recorded_at_ms).unwrap().is_none());
    assert!(db.claim_as_of("claim:forgotten", i64::MAX).unwrap().is_none());
    assert!(db.claim_history("claim:forgotten").unwrap().is_none());
    assert!(db.rank_claims(&["claim:forgotten"], &RankingPolicy {
        mode: RankingMode::Challenge, as_of_ms: None, scope_id: None, limit: 10,
    }).unwrap().is_empty());
    let raw = Connection::open(db.path()).unwrap();
    let events: i64 = raw.query_row(
        "SELECT COUNT(*) FROM memory_claim_events WHERE claim_id='claim:forgotten'",
        [], |r| r.get(0),
    ).unwrap();
    assert_eq!(events, 1);
    assert!(db.source(&source).unwrap().is_some());
    assert!(db.apply_memory_write(&"b".repeat(64), "mp5c-forget-owner-02", &op).is_err());
}

use mind_place_state::{
    ClaimEvidence, ClaimTransition, ClientCapability, MemoryClaim, MemoryWrite, RelationKind,
    SourceDescriptor, SourceKey, StateStore,
};

fn fixture() -> (tempfile::TempDir, StateStore, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().join("state.sqlite3")).unwrap();
    let source = store
        .register_source(&SourceDescriptor {
            provider: "chatgpt",
            source_namespace: "synthetic",
            source_kind: "conversation",
            key: SourceKey::External("mp5d-scenario"),
        })
        .unwrap();
    (dir, store, source)
}
fn add(store: &StateStore, id: &str, statement: &str, source: &str) {
    store
        .create_evidenced_claim(
            &MemoryClaim {
                id: id.into(),
                statement: statement.into(),
                asserted_by: "user".into(),
                epistemic_origin: "explicit".into(),
                status: "active".into(),
            },
            &[ClaimEvidence {
                source_ref: source.into(),
                evidence_kind: "support".into(),
            }],
        )
        .unwrap();
}

#[test]
fn deterministic_history_and_conflict_are_visible_not_truthwashed() {
    let (_dir, db, source) = fixture();
    add(&db, "claim:old", "Older location", &source);
    add(&db, "claim:new", "Corrected location", &source);
    db.add_claim_relation(
        "claim:new",
        "claim:old",
        RelationKind::Contradicts,
        &source,
        "Source contradicts older location",
    )
    .unwrap();
    let rows = db.memory_inspection_overview("location").unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r.disposition == "contested"));

    let detail = db.memory_inspection_detail("claim:old").unwrap().unwrap();
    assert_eq!(detail.ranked.disposition, "contested");
    assert_eq!(detail.relations[0].direction, "incoming");
    assert_eq!(detail.relations[0].relation, "contradicts");
    assert_eq!(detail.relations[0].source_ref, source);
    assert_eq!(detail.history.len(), 1);

    db.transition_claim(
        "claim:old",
        ClaimTransition::Retract,
        "Original location disproved",
        &[ClaimEvidence {
            source_ref: source.clone(),
            evidence_kind: "retraction".into(),
        }],
        None,
    )
    .unwrap();
    let old = db.memory_inspection_detail("claim:old").unwrap().unwrap();
    assert_eq!(old.history.len(), 2);
    assert_eq!(old.history[0].status, "active");
    assert_eq!(old.history[1].status, "retracted");
    assert_eq!(old.ranked.disposition, "non_current");
}

#[test]
fn superseded_claim_does_not_become_current_and_corrections_are_explicit() {
    let (_dir, db, source) = fixture();
    add(&db, "claim:a", "Obsolete preference", &source);
    add(&db, "claim:b", "Updated preference", &source);
    db.add_claim_relation(
        "claim:b",
        "claim:a",
        RelationKind::Supersedes,
        &source,
        "Newer explicit correction",
    )
    .unwrap();
    let result = db.memory_inspection_detail("claim:a").unwrap().unwrap();
    assert_eq!(result.ranked.disposition, "superseded");
    assert_eq!(result.relations[0].relation, "supersedes");
    assert_eq!(result.relations[0].direction, "incoming");
}

#[test]
fn forgetting_blocks_history_listing_and_direct_lookup() {
    let (_dir, db, source) = fixture();
    db.issue_client(
        "client:v1:writer",
        "Synthetic writer",
        &"a".repeat(64),
        &[ClientCapability::Write],
    )
    .unwrap();
    db.issue_client(
        "client:v1:admin",
        "Synthetic owner",
        &"b".repeat(64),
        &[ClientCapability::Admin],
    )
    .unwrap();
    add(
        &db,
        "claim:private",
        "Private remembered assertion",
        &source,
    );
    db.apply_memory_write(
        &"b".repeat(64),
        "mp5d-forget-claim-0001",
        &MemoryWrite::Forget {
            claim_id: "claim:private",
            reason: "Owner requested forget",
        },
    )
    .unwrap();
    assert!(db.memory_inspection_overview("Private").unwrap().is_empty());
    assert!(db
        .memory_inspection_detail("claim:private")
        .unwrap()
        .is_none());
}

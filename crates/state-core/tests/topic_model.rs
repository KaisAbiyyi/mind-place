use mind_place_state::{
    AssertionSource, CnfRecord, CurationProfile, DocumentMetadata, DocumentObjectStore,
    EvidenceSpan, MembershipStatus, MemoryClaim, NewCurationGeneration, NewTopic,
    NewTopicMembership, SourceDescriptor, SourceKey, StateError, StateStore, TopicMemberKind,
    TopicMemberRole, TopicWorkStatus, VirtualDocumentPath,
};
use rusqlite::Connection;
use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;

fn setup() -> (tempfile::TempDir, StateStore, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().join("state.sqlite3")).unwrap();
    let s1 = store
        .register_source(&SourceDescriptor {
            provider: "claude",
            source_namespace: "account-1",
            source_kind: "conversation",
            key: SourceKey::External("conv-1"),
        })
        .unwrap();
    let s2 = store
        .register_source(&SourceDescriptor {
            provider: "gemini",
            source_namespace: "account-1",
            source_kind: "conversation",
            key: SourceKey::External("conv-2"),
        })
        .unwrap();
    (dir, store, s1, s2)
}

#[test]
fn topic_hierarchy_prevents_cycles_and_enforces_slug_invariants() {
    let (_dir, db, _, _) = setup();
    let root = db
        .create_topic(&NewTopic {
            id: "topic:sibambo",
            slug: "sibambo",
            name: "Sibambo",
            parent_topic_id: None,
            is_project: false,
            work_status: None,
            description: Some("Architecture and BIM domain"),
        })
        .unwrap();
    assert!(!root.is_project);
    assert_eq!(root.work_status, None);

    let child = db
        .create_topic(&NewTopic {
            id: "topic:sibambo-bom",
            slug: "sibambo-bom",
            name: "Sibambo BOM",
            parent_topic_id: Some("topic:sibambo"),
            is_project: false,
            work_status: None,
            description: None,
        })
        .unwrap();
    let grandchild = db
        .create_topic(&NewTopic {
            id: "topic:sibambo-bom-rust",
            slug: "sibambo-bom-rust",
            name: "Rust Port",
            parent_topic_id: Some(&child.id),
            is_project: false,
            work_status: None,
            description: None,
        })
        .unwrap();

    let ancestors = db.topic_ancestors(&grandchild.id).unwrap();
    assert_eq!(
        ancestors.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
        vec!["topic:sibambo-bom", "topic:sibambo"]
    );

    assert!(matches!(
        db.reparent_topic("topic:sibambo", Some("topic:sibambo-bom-rust")),
        Err(StateError::InvalidValue)
    ));
    assert!(matches!(
        db.reparent_topic("topic:sibambo", Some("topic:sibambo")),
        Err(StateError::InvalidValue)
    ));
    assert!(matches!(
        db.reparent_topic("topic:sibambo", Some("topic:missing")),
        Err(StateError::InvalidValue)
    ));

    for bad_slug in ["Upper", "bad_slug", "", &"a".repeat(65)] {
        assert!(matches!(
            db.create_topic(&NewTopic {
                id: "topic:bad",
                slug: bad_slug,
                name: "Bad",
                parent_topic_id: None,
                is_project: false,
                work_status: None,
                description: None,
            }),
            Err(StateError::InvalidValue)
        ));
    }

    let raw = Connection::open(db.path()).unwrap();
    assert!(raw
        .execute(
            "UPDATE topics SET parent_topic_id=id WHERE id='topic:sibambo'",
            []
        )
        .is_err());
}

#[test]
fn project_is_unified_topic_with_work_state_without_duplicates() {
    let (_dir, db, _, _) = setup();
    let proj_topic = db
        .create_topic(&NewTopic {
            id: "topic:mind-place",
            slug: "mind-place",
            name: "Mind Place",
            parent_topic_id: None,
            is_project: true,
            work_status: Some(TopicWorkStatus::Active),
            description: Some("Local continuity layer"),
        })
        .unwrap();
    assert!(proj_topic.is_project);
    assert_eq!(proj_topic.work_status, Some(TopicWorkStatus::Active));
    assert_eq!(
        db.get_project("topic:mind-place").unwrap().unwrap().name,
        "Mind Place"
    );
    assert_eq!(
        db.get_topic_by_slug("mind-place").unwrap(),
        Some(proj_topic.clone())
    );

    // Legacy `create_project` extends the same unified topic/project model.
    db.create_project("proj:legacy", "Legacy Project").unwrap();
    let legacy_as_topic = db.get_topic("proj:legacy").unwrap().unwrap();
    assert!(legacy_as_topic.is_project);
    assert_eq!(legacy_as_topic.work_status, Some(TopicWorkStatus::Active));
    assert_eq!(
        db.get_project("proj:legacy").unwrap().unwrap().name,
        "Legacy Project"
    );

    let ordinary = db
        .create_topic(&NewTopic {
            id: "topic:politik",
            slug: "politik",
            name: "Politik",
            parent_topic_id: None,
            is_project: false,
            work_status: None,
            description: None,
        })
        .unwrap();
    assert!(!ordinary.is_project);
    assert_eq!(db.get_project("topic:politik").unwrap(), None);

    let promoted = db
        .set_topic_work_state("topic:politik", true, Some(TopicWorkStatus::Paused))
        .unwrap();
    assert!(promoted.is_project);
    assert_eq!(promoted.work_status, Some(TopicWorkStatus::Paused));
    assert_eq!(db.list_topics(None, true, 50).unwrap().len(), 3);

    let archived = db
        .set_topic_work_state("proj:legacy", true, Some(TopicWorkStatus::Archived))
        .unwrap();
    assert_eq!(archived.work_status, Some(TopicWorkStatus::Archived));

    let raw = Connection::open(db.path()).unwrap();
    let compat_status: String = raw
        .query_row(
            "SELECT status FROM projects WHERE id='proj:legacy'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(compat_status, "archived");
    assert!(raw
        .execute(
            "UPDATE topics SET is_project=0,work_status='active' WHERE id='topic:politik'",
            []
        )
        .is_err());
    assert!(raw
        .execute(
            "UPDATE topics SET is_project=1,work_status=NULL WHERE id='topic:politik'",
            []
        )
        .is_err());
}

#[test]
fn many_to_many_topic_memberships_enforce_provenance_fks_and_supersession() {
    let (dir, db, s1, s2) = setup();
    let t1 = db
        .create_topic(&NewTopic {
            id: "topic:sibambo",
            slug: "sibambo",
            name: "Sibambo",
            parent_topic_id: None,
            is_project: true,
            work_status: Some(TopicWorkStatus::Active),
            description: None,
        })
        .unwrap();
    let t2 = db
        .create_topic(&NewTopic {
            id: "topic:rust",
            slug: "rust",
            name: "Rust",
            parent_topic_id: None,
            is_project: false,
            work_status: None,
            description: None,
        })
        .unwrap();

    db.create_memory_claim(&MemoryClaim {
        id: "claim:m1".into(),
        statement: "Use Rust for core".into(),
        asserted_by: "user".into(),
        epistemic_origin: "explicit".into(),
        status: "active".into(),
    })
    .unwrap();

    let docs_dir = dir.path().join("docs");
    fs::create_dir_all(&docs_dir).unwrap();
    let root = db.register_document_root(&docs_dir).unwrap();
    db.approve_document_root(&root.id).unwrap();
    let doc_uri = VirtualDocumentPath::from_segments(&root.id, &["spec.md".into()])
        .unwrap()
        .to_uri();
    let bytes = b"# Spec";
    let obj_store = DocumentObjectStore::open(db.clone()).unwrap();
    obj_store
        .preserve(
            &doc_uri,
            &DocumentMetadata {
                file_identity: "fixture:spec:1".into(),
                byte_length: bytes.len() as u64,
                modified_ns: 1,
                changed_ns: 1,
            },
            Cursor::new(bytes),
            || Ok(()),
        )
        .unwrap();

    let gen = db
        .create_staged_generation(&NewCurationGeneration {
            id: "gen:1",
            topic_id: &t1.id,
            run_id: "run:1",
            profile: CurationProfile::Brief,
            asserted_by: AssertionSource::Model,
            model_id: Some("claude-haiku-5.5"),
            base_generation_id: None,
        })
        .unwrap();
    db.append_staged_ops(
        &gen.id,
        "shard-0",
        &[CnfRecord::Upsert {
            key: "sibambo/concept/bom-core".into(),
            kind: mind_place_state::CuratedNodeKind::Concept,
            title: "BOM Core".into(),
            summary: "Core calculation engine".into(),
            body: None,
            attrs: BTreeMap::new(),
            aliases: vec![],
            evidence: vec![EvidenceSpan {
                src: s1.clone(),
                msg: 0,
                span: [0, 12],
                sha: format!("b3:{}", "a".repeat(64)),
                quote: None,
            }],
            confidence: Some(0.9),
            base_rev: 0,
        }],
    )
    .unwrap();
    db.publish_generation(&gen.id).unwrap();
    let node = db
        .get_curated_node("sibambo/concept/bom-core")
        .unwrap()
        .unwrap();

    // Many-to-many: `s1` belongs to both `t1` and `t2`.
    let m1 = db
        .add_topic_membership(&NewTopicMembership {
            id: "mem:s1-t1",
            topic_id: &t1.id,
            member_kind: TopicMemberKind::Source,
            member_id: &s1,
            role: TopicMemberRole::Primary,
            asserted_by: AssertionSource::Import,
            epistemic_origin: "observed",
            evidence_source_ref: Some(&s1),
            evidence_locator: Some("msg:0"),
            confidence: Some(1.0),
        })
        .unwrap();
    assert_eq!(m1.status, MembershipStatus::Active);

    db.add_topic_membership(&NewTopicMembership {
        id: "mem:s1-t2",
        topic_id: &t2.id,
        member_kind: TopicMemberKind::Source,
        member_id: &s1,
        role: TopicMemberRole::Related,
        asserted_by: AssertionSource::Model,
        epistemic_origin: "inferred",
        evidence_source_ref: Some(&s2),
        evidence_locator: Some("msg:4[10..40]"),
        confidence: Some(0.82),
    })
    .unwrap();

    db.add_topic_membership(&NewTopicMembership {
        id: "mem:node-t2",
        topic_id: &t2.id,
        member_kind: TopicMemberKind::CuratedNode,
        member_id: &node.id,
        role: TopicMemberRole::Member,
        asserted_by: AssertionSource::User,
        epistemic_origin: "explicit",
        evidence_source_ref: None,
        evidence_locator: None,
        confidence: None,
    })
    .unwrap();
    db.add_topic_membership(&NewTopicMembership {
        id: "mem:claim-t1",
        topic_id: &t1.id,
        member_kind: TopicMemberKind::MemoryClaim,
        member_id: "claim:m1",
        role: TopicMemberRole::Member,
        asserted_by: AssertionSource::User,
        epistemic_origin: "explicit",
        evidence_source_ref: None,
        evidence_locator: None,
        confidence: None,
    })
    .unwrap();
    db.add_topic_membership(&NewTopicMembership {
        id: "mem:doc-t1",
        topic_id: &t1.id,
        member_kind: TopicMemberKind::Document,
        member_id: &doc_uri,
        role: TopicMemberRole::Member,
        asserted_by: AssertionSource::Deterministic,
        epistemic_origin: "observed",
        evidence_source_ref: Some(&s1),
        evidence_locator: Some("path:spec.md"),
        confidence: Some(1.0),
    })
    .unwrap();

    assert_eq!(
        db.list_member_topics(TopicMemberKind::Source, &s1, false, 50)
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        db.list_topic_memberships(&t1.id, false, 50).unwrap().len(),
        3
    );

    // Model/import assertion without evidence fails closed.
    assert!(matches!(
        db.add_topic_membership(&NewTopicMembership {
            id: "mem:no-ev",
            topic_id: &t1.id,
            member_kind: TopicMemberKind::Source,
            member_id: &s2,
            role: TopicMemberRole::Member,
            asserted_by: AssertionSource::Model,
            epistemic_origin: "inferred",
            evidence_source_ref: None,
            evidence_locator: None,
            confidence: Some(0.5),
        }),
        Err(StateError::InvalidValue)
    ));

    // Unregistered FK targets fail closed.
    let missing_src = format!("src:v1:{}", "f".repeat(64));
    assert!(matches!(
        db.add_topic_membership(&NewTopicMembership {
            id: "mem:missing-src",
            topic_id: &t1.id,
            member_kind: TopicMemberKind::Source,
            member_id: &missing_src,
            role: TopicMemberRole::Member,
            asserted_by: AssertionSource::User,
            epistemic_origin: "explicit",
            evidence_source_ref: None,
            evidence_locator: None,
            confidence: None,
        }),
        Err(StateError::InvalidValue)
    ));

    // Duplicate active membership is rejected.
    assert!(matches!(
        db.add_topic_membership(&NewTopicMembership {
            id: "mem:dup",
            topic_id: &t1.id,
            member_kind: TopicMemberKind::Source,
            member_id: &s1,
            role: TopicMemberRole::Member,
            asserted_by: AssertionSource::User,
            epistemic_origin: "explicit",
            evidence_source_ref: None,
            evidence_locator: None,
            confidence: None,
        }),
        Err(StateError::InvalidValue)
    ));

    // Supersede membership atomically and verify provenance preservation.
    let replacement = db
        .supersede_topic_membership(
            "mem:s1-t1",
            &NewTopicMembership {
                id: "mem:s1-t1-v2",
                topic_id: &t1.id,
                member_kind: TopicMemberKind::Source,
                member_id: &s1,
                role: TopicMemberRole::Member,
                asserted_by: AssertionSource::User,
                epistemic_origin: "explicit",
                evidence_source_ref: Some(&s1),
                evidence_locator: Some("owner confirmed"),
                confidence: Some(1.0),
            },
        )
        .unwrap();
    assert_eq!(replacement.status, MembershipStatus::Active);

    let active_only = db.list_topic_memberships(&t1.id, false, 50).unwrap();
    assert_eq!(active_only.len(), 3);
    let with_history = db.list_topic_memberships(&t1.id, true, 50).unwrap();
    assert_eq!(with_history.len(), 4);
    let old = with_history.iter().find(|m| m.id == "mem:s1-t1").unwrap();
    assert_eq!(old.status, MembershipStatus::Superseded);
    assert_eq!(old.superseded_by.as_deref(), Some("mem:s1-t1-v2"));

    db.retract_topic_membership("mem:s1-t2").unwrap();
    assert_eq!(
        db.list_member_topics(TopicMemberKind::Source, &s1, false, 50)
            .unwrap()
            .len(),
        1
    );

    // Immutability triggers forbid deleting or mutating provenance.
    let raw = Connection::open(db.path()).unwrap();
    assert!(raw.execute("DELETE FROM topic_memberships", []).is_err());
    assert!(raw
        .execute(
            "UPDATE topic_memberships SET asserted_by='user' WHERE id='mem:s1-t2'",
            []
        )
        .is_err());
}

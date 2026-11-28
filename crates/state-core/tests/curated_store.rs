use mind_place_state::{
    AssertionSource, AttrValue, ClaimEvidence, CnfRecord, CuratedEdgeRel, CuratedEdgeStatus,
    CuratedNodeKind, CuratedNodeStatus, CurationProfile, EvidenceSpan, GenerationStatus,
    MemoryClaim, NewCurationGeneration, NewTopic, NewTopicMembership, SourceDescriptor, SourceKey,
    StateError, StateStore, TopicMemberKind, TopicMemberRole, APPLICATION_ID, SCHEMA_VERSION,
};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;

fn span(src: &str, msg: i64, start: i64, end: i64, quote: Option<&str>) -> EvidenceSpan {
    let tag = format!("{src}:{msg}:{start}:{end}");
    let digest = format!("{:x}", Sha256::digest(tag.as_bytes()));
    EvidenceSpan {
        src: src.to_owned(),
        msg,
        span: [start, end],
        sha: format!("b3:{digest}"),
        quote: quote.map(str::to_owned),
    }
}

fn fixture() -> (tempfile::TempDir, StateStore, String, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let db = StateStore::open(dir.path().join("state.sqlite3")).unwrap();
    let s1 = db
        .register_source(&SourceDescriptor {
            provider: "chatgpt",
            source_namespace: "acct-1",
            source_kind: "conversation",
            key: SourceKey::External("c-1"),
        })
        .unwrap();
    let s2 = db
        .register_source(&SourceDescriptor {
            provider: "claude",
            source_namespace: "acct-1",
            source_kind: "conversation",
            key: SourceKey::External("c-2"),
        })
        .unwrap();
    let topic = db
        .create_topic(&NewTopic {
            id: "topic:sibambo",
            slug: "sibambo",
            name: "Sibambo",
            parent_topic_id: None,
            is_project: true,
            work_status: None,
            description: None,
        })
        .unwrap();
    (dir, db, topic.id, s1, s2)
}

#[test]
fn cnf_v1_validation_enforces_closed_sets_keys_bounds_and_scope() {
    let (_dir, db, topic_id, s1, _) = fixture();

    // Parse exact CNF v1 JSONL shape.
    let valid_line = format!(
        r#"{{"op":"upsert","key":"sibambo/decision/bom-engine-rust-port","kind":"decision","title":"Port BOM engine","summary":"Owner chose Rust over Ruby plugin.","attrs":{{"status":"active","priority":1,"confirmed":true}},"aliases":["bom rewrite"],"evidence":[{{"src":"{s1}","msg":42,"span":[120,388],"sha":"b3:{}"}}],"confidence":0.8,"base_rev":0}}"#,
        "a".repeat(64)
    );
    let parsed = CnfRecord::from_json_line(&valid_line).unwrap();
    assert_eq!(parsed.op_name(), "upsert");

    // Unknown fields, unknown kind, unknown rel, bad keys, missing evidence, oversized bounds.
    let invalid_lines = [
        format!(
            r#"{{"op":"upsert","key":"sibambo/decision/x","kind":"decision","title":"T","summary":"S","extra":1,"evidence":[{{"src":"{s1}","msg":0,"span":[0,5],"sha":"b3:{}"}}]}}"#,
            "a".repeat(64)
        ),
        format!(
            r#"{{"op":"upsert","key":"sibambo/memo/x","kind":"memo","title":"T","summary":"S","evidence":[{{"src":"{s1}","msg":0,"span":[0,5],"sha":"b3:{}"}}]}}"#,
            "a".repeat(64)
        ),
        format!(
            r#"{{"op":"upsert","key":"Sibambo/decision/x","kind":"decision","title":"T","summary":"S","evidence":[{{"src":"{s1}","msg":0,"span":[0,5],"sha":"b3:{}"}}]}}"#,
            "a".repeat(64)
        ),
        format!(
            r#"{{"op":"link","from":"sibambo/decision/a","rel":"owns","to":"sibambo/decision/b","evidence":[{{"src":"{s1}","msg":0,"span":[0,5],"sha":"b3:{}"}}]}}"#,
            "a".repeat(64)
        ),
        format!(
            r#"{{"op":"link","from":"sibambo/decision/a","rel":"supports","to":"sibambo/decision/a","evidence":[{{"src":"{s1}","msg":0,"span":[0,5],"sha":"b3:{}"}}]}}"#,
            "a".repeat(64)
        ),
        r#"{"op":"upsert","key":"sibambo/fact/a","kind":"fact","title":"T","summary":"S","evidence":[]}"#.to_owned(),
        format!(
            r#"{{"op":"upsert","key":"sibambo/fact/a","kind":"fact","title":"{}","summary":"S","evidence":[{{"src":"{s1}","msg":0,"span":[0,5],"sha":"b3:{}"}}]}}"#,
            "x".repeat(121),
            "a".repeat(64)
        ),
        format!(
            r#"{{"op":"upsert","key":"sibambo/fact/a","kind":"fact","title":"T","summary":"S","attrs":{{"nested":{{"a":1}}}},"evidence":[{{"src":"{s1}","msg":0,"span":[0,5],"sha":"b3:{}"}}]}}"#,
            "a".repeat(64)
        ),
        format!(
            r#"{{"op":"upsert","key":"sibambo/fact/a","kind":"fact","title":"T","summary":"S","evidence":[{{"src":"{s1}","msg":0,"span":[5,5],"sha":"b3:{}"}}]}}"#,
            "a".repeat(64)
        ),
        format!(
            r#"{{"op":"upsert","key":"sibambo/fact/a","kind":"fact","title":"T","summary":"S","evidence":[{{"src":"{s1}","msg":0,"span":[0,5],"sha":"md5:1234"}}]}}"#
        ),
    ];
    for line in invalid_lines {
        assert!(
            CnfRecord::from_json_line(&line).is_err(),
            "expected invalid CNF line to fail: {line}"
        );
    }

    // All closed kinds and rels are accepted by parser.
    for kind in [
        CuratedNodeKind::Topic,
        CuratedNodeKind::Entity,
        CuratedNodeKind::Concept,
        CuratedNodeKind::Decision,
        CuratedNodeKind::Fact,
        CuratedNodeKind::Question,
        CuratedNodeKind::Task,
        CuratedNodeKind::Event,
        CuratedNodeKind::Artifact,
    ] {
        let rec = CnfRecord::Upsert {
            key: format!("sibambo/{}/item", kind.as_str()),
            kind,
            title: "Item".into(),
            summary: "Summary".into(),
            body: None,
            attrs: BTreeMap::new(),
            aliases: vec![],
            evidence: vec![span(&s1, 0, 0, 5, None)],
            confidence: None,
            base_rev: 0,
        };
        let encoded = serde_json::to_string(&rec).unwrap();
        assert!(CnfRecord::from_json_line(&encoded).is_ok());
    }
    for rel in [
        CuratedEdgeRel::PartOf,
        CuratedEdgeRel::About,
        CuratedEdgeRel::DependsOn,
        CuratedEdgeRel::Supports,
        CuratedEdgeRel::Contradicts,
        CuratedEdgeRel::Supersedes,
        CuratedEdgeRel::DecidedIn,
        CuratedEdgeRel::Mentions,
        CuratedEdgeRel::RelatedTo,
    ] {
        let rec = CnfRecord::Link {
            from: "sibambo/fact/a".into(),
            rel,
            to: "sibambo/fact/b".into(),
            evidence: vec![span(&s1, 0, 0, 5, None)],
            confidence: Some(0.75),
        };
        let encoded = serde_json::to_string(&rec).unwrap();
        assert!(CnfRecord::from_json_line(&encoded).is_ok());
    }

    // Brief forbids body; Exact requires quote; foreign topic rejected.
    let g_brief = db
        .create_staged_generation(&NewCurationGeneration {
            id: "gen:brief",
            topic_id: &topic_id,
            run_id: "run:brief",
            profile: CurationProfile::Brief,
            asserted_by: AssertionSource::Model,
            model_id: Some("claude-haiku-5.5"),
            base_generation_id: None,
        })
        .unwrap();
    assert!(db
        .append_staged_ops(
            &g_brief.id,
            "shard-00",
            &[CnfRecord::Upsert {
                key: "sibambo/fact/with-body".into(),
                kind: CuratedNodeKind::Fact,
                title: "Fact".into(),
                summary: "Short".into(),
                body: Some("Not allowed in brief".into()),
                attrs: BTreeMap::new(),
                aliases: vec![],
                evidence: vec![span(&s1, 0, 0, 4, None)],
                confidence: None,
                base_rev: 0,
            }]
        )
        .is_err());
    assert!(db
        .append_staged_ops(
            &g_brief.id,
            "shard-00",
            &[CnfRecord::Upsert {
                key: "other-topic/fact/cross-scope".into(),
                kind: CuratedNodeKind::Fact,
                title: "Cross".into(),
                summary: "Rejected".into(),
                body: None,
                attrs: BTreeMap::new(),
                aliases: vec![],
                evidence: vec![span(&s1, 0, 0, 4, None)],
                confidence: None,
                base_rev: 0,
            }]
        )
        .is_err());

    let g_exact = db
        .create_staged_generation(&NewCurationGeneration {
            id: "gen:exact",
            topic_id: &topic_id,
            run_id: "run:exact",
            profile: CurationProfile::Exact,
            asserted_by: AssertionSource::Model,
            model_id: Some("claude-haiku-5.5"),
            base_generation_id: None,
        })
        .unwrap();
    assert!(db
        .append_staged_ops(
            &g_exact.id,
            "shard-00",
            &[CnfRecord::Upsert {
                key: "sibambo/fact/no-quote".into(),
                kind: CuratedNodeKind::Fact,
                title: "Fact".into(),
                summary: "Needs quote".into(),
                body: None,
                attrs: BTreeMap::new(),
                aliases: vec![],
                evidence: vec![span(&s1, 0, 0, 4, None)],
                confidence: None,
                base_rev: 0,
            }]
        )
        .is_err());
    db.append_staged_ops(
        &g_exact.id,
        "shard-00",
        &[CnfRecord::Upsert {
            key: "sibambo/fact/with-quote".into(),
            kind: CuratedNodeKind::Fact,
            title: "Fact".into(),
            summary: "Quoted".into(),
            body: Some("Detailed body".into()),
            attrs: BTreeMap::new(),
            aliases: vec![],
            evidence: vec![span(&s1, 0, 0, 4, Some("exact span"))],
            confidence: None,
            base_rev: 0,
        }],
    )
    .unwrap();
}

#[test]
fn staged_generations_stay_isolated_and_op_log_is_immutable() {
    let (_dir, db, topic_id, s1, _) = fixture();
    let gen = db
        .create_staged_generation(&NewCurationGeneration {
            id: "gen:staged",
            topic_id: &topic_id,
            run_id: "run:1",
            profile: CurationProfile::Detailed,
            asserted_by: AssertionSource::Model,
            model_id: Some("gemini-flash"),
            base_generation_id: None,
        })
        .unwrap();
    db.append_staged_ops(
        &gen.id,
        "shard-00",
        &[CnfRecord::Upsert {
            key: "sibambo/fact/draft-only".into(),
            kind: CuratedNodeKind::Fact,
            title: "Draft Fact".into(),
            summary: "Must not leak before publish".into(),
            body: None,
            attrs: BTreeMap::new(),
            aliases: vec![],
            evidence: vec![span(&s1, 1, 0, 10, None)],
            confidence: Some(0.6),
            base_rev: 0,
        }],
    )
    .unwrap();

    assert!(db
        .get_curated_node("sibambo/fact/draft-only")
        .unwrap()
        .is_none());
    assert!(db
        .list_topic_curated_nodes(&topic_id, true, 50)
        .unwrap()
        .is_empty());
    let export = db.export_topic_markdown("sibambo").unwrap();
    assert_eq!(export.accepted_node_count, 0);
    assert!(!export.markdown.contains("Draft Fact"));

    let raw = Connection::open(db.path()).unwrap();
    assert!(raw.execute("DELETE FROM curated_op_log", []).is_err());
    assert!(raw
        .execute("UPDATE curated_op_log SET op='patch'", [])
        .is_err());
    assert!(raw
        .execute("DELETE FROM curated_op_evidence_spans", [])
        .is_err());
    assert!(raw.execute("DELETE FROM curation_generations", []).is_err());

    let rejected = db.reject_generation(&gen.id).unwrap();
    assert_eq!(rejected.status, GenerationStatus::Rejected);
    assert!(db.publish_generation(&gen.id).is_err());
}

#[test]
fn publish_handles_reconcile_base_rev_conflicts_supersedes_retire_and_unlink() {
    let (_dir, db, topic_id, s1, s2) = fixture();

    // Generation 1: multi-shard (`reconcile` runs after worker shards even if named `reconcile`).
    let g1 = db
        .create_staged_generation(&NewCurationGeneration {
            id: "gen:1",
            topic_id: &topic_id,
            run_id: "run:1",
            profile: CurationProfile::Detailed,
            asserted_by: AssertionSource::Model,
            model_id: Some("claude-haiku-5.5"),
            base_generation_id: None,
        })
        .unwrap();
    db.append_staged_ops(
        &g1.id,
        "reconcile",
        &[CnfRecord::Link {
            from: "sibambo/decision/patch-ruby-plugin".into(),
            rel: CuratedEdgeRel::About,
            to: "sibambo/concept/bom-engine".into(),
            evidence: vec![span(&s1, 3, 0, 20, None)],
            confidence: Some(0.9),
        }],
    )
    .unwrap();
    db.append_staged_ops(
        &g1.id,
        "shard-z",
        &[
            CnfRecord::Upsert {
                key: "sibambo/decision/patch-ruby-plugin".into(),
                kind: CuratedNodeKind::Decision,
                title: "Patch Ruby plugin".into(),
                summary: "Temporary workaround in Ruby plugin.".into(),
                body: None,
                attrs: BTreeMap::from([("status".into(), AttrValue::String("active".into()))]),
                aliases: vec!["ruby patch".into()],
                evidence: vec![span(&s1, 1, 0, 40, None)],
                confidence: Some(0.8),
                base_rev: 0,
            },
            CnfRecord::Upsert {
                key: "sibambo/concept/bom-engine".into(),
                kind: CuratedNodeKind::Concept,
                title: "BOM Engine".into(),
                summary: "Bill of materials calculator.".into(),
                body: Some("Computes quantities from CAD geometry.".into()),
                attrs: BTreeMap::new(),
                aliases: vec![],
                evidence: vec![span(&s1, 2, 0, 30, None)],
                confidence: Some(0.95),
                base_rev: 0,
            },
        ],
    )
    .unwrap();
    let pub1 = db.publish_generation(&g1.id).unwrap();
    assert_eq!(pub1.ops_applied, 3);

    // Stale `base_rev` turns into RevisionConflict without mutating accepted state.
    let g_conflict = db
        .create_staged_generation(&NewCurationGeneration {
            id: "gen:conflict",
            topic_id: &topic_id,
            run_id: "run:conflict",
            profile: CurationProfile::Brief,
            asserted_by: AssertionSource::Model,
            model_id: Some("claude-haiku-5.5"),
            base_generation_id: Some(&g1.id),
        })
        .unwrap();
    db.append_staged_ops(
        &g_conflict.id,
        "shard-0",
        &[CnfRecord::Patch {
            key: "sibambo/concept/bom-engine".into(),
            title: None,
            summary: Some("Stale patch".into()),
            body: None,
            clear_body: false,
            attrs: None,
            aliases: None,
            evidence: vec![span(&s2, 0, 0, 12, None)],
            confidence: None,
            base_rev: 99,
        }],
    )
    .unwrap();
    assert!(matches!(
        db.publish_generation(&g_conflict.id),
        Err(StateError::RevisionConflict)
    ));
    assert_eq!(
        db.get_curated_node("sibambo/concept/bom-engine")
            .unwrap()
            .unwrap()
            .current_rev,
        1
    );

    // Generation 2: patch + supersede A -> B + retire + unlink.
    let g2 = db
        .create_staged_generation(&NewCurationGeneration {
            id: "gen:2",
            topic_id: &topic_id,
            run_id: "run:2",
            profile: CurationProfile::Detailed,
            asserted_by: AssertionSource::Model,
            model_id: Some("claude-sonnet-5.5"),
            base_generation_id: Some(&g1.id),
        })
        .unwrap();
    db.append_staged_ops(
        &g2.id,
        "shard-0",
        &[
            CnfRecord::Patch {
                key: "sibambo/concept/bom-engine".into(),
                title: None,
                summary: Some("Bill of materials calculation core.".into()),
                body: None,
                clear_body: false,
                attrs: Some(BTreeMap::from([(
                    "layer".into(),
                    AttrValue::String("rust".into()),
                )])),
                aliases: Some(vec!["bom calculator".into()]),
                evidence: vec![span(&s2, 5, 10, 50, None)],
                confidence: Some(0.98),
                base_rev: 1,
            },
            CnfRecord::Supersede {
                key: "sibambo/decision/bom-engine-rust-port".into(),
                kind: CuratedNodeKind::Decision,
                supersedes: "sibambo/decision/patch-ruby-plugin".into(),
                title: "Port BOM engine to Rust".into(),
                summary: "Owner chose a Rust port over patching the Ruby plugin.".into(),
                body: Some("Full rationale in meeting transcript.".into()),
                attrs: BTreeMap::from([
                    ("status".into(), AttrValue::String("active".into())),
                    ("decided_at".into(), AttrValue::String("2026-10-02".into())),
                ]),
                aliases: vec!["bom rust port".into()],
                evidence: vec![span(&s2, 6, 120, 388, None)],
                confidence: Some(0.99),
                base_rev: 0,
            },
            CnfRecord::Unlink {
                from: "sibambo/decision/patch-ruby-plugin".into(),
                rel: CuratedEdgeRel::About,
                to: "sibambo/concept/bom-engine".into(),
                evidence: vec![span(&s2, 7, 0, 15, None)],
            },
        ],
    )
    .unwrap();
    db.publish_generation(&g2.id).unwrap();

    let old_a = db
        .get_curated_node("sibambo/decision/patch-ruby-plugin")
        .unwrap()
        .unwrap();
    let new_b = db
        .get_curated_node("sibambo/decision/bom-engine-rust-port")
        .unwrap()
        .unwrap();
    assert_eq!(old_a.status, CuratedNodeStatus::Superseded);
    assert_eq!(old_a.current_rev, 2);
    assert_eq!(
        old_a.superseded_by_node_id.as_deref(),
        Some(new_b.id.as_str())
    );
    assert_eq!(new_b.status, CuratedNodeStatus::Active);
    assert_eq!(new_b.supersedes_node_id.as_deref(), Some(old_a.id.as_str()));

    let a_revs = db
        .curated_node_revisions("sibambo/decision/patch-ruby-plugin")
        .unwrap();
    assert_eq!(a_revs.len(), 2);
    assert_eq!(a_revs[0].status, CuratedNodeStatus::Active);
    assert_eq!(a_revs[1].status, CuratedNodeStatus::Superseded);

    let active_edges = db.list_topic_curated_edges(&topic_id, false, 50).unwrap();
    assert_eq!(active_edges.len(), 1);
    assert_eq!(active_edges[0].rel, CuratedEdgeRel::Supersedes);
    assert_eq!(active_edges[0].from_key, new_b.canonical_key);
    assert_eq!(active_edges[0].to_key, old_a.canonical_key);

    let all_edges = db.list_topic_curated_edges(&topic_id, true, 50).unwrap();
    assert_eq!(all_edges.len(), 2);
    assert!(all_edges
        .iter()
        .any(|e| e.status == CuratedEdgeStatus::Unlinked));
}

#[test]
fn deterministic_rebuild_and_fk_evidence_integrity_preserve_raw_evidence() {
    let (dir, db, topic_id, s1, s2) = fixture();

    // Create raw archive bytes beside DB and memory claim to prove neither is touched.
    let raw_archive_dir = dir.path().join("archive-v1/conversations");
    fs::create_dir_all(&raw_archive_dir).unwrap();
    let raw_file = raw_archive_dir.join("raw.jsonl");
    let raw_bytes = br#"{"msg":"immutable raw evidence"}"#;
    fs::write(&raw_file, raw_bytes).unwrap();
    let raw_sha_before = format!("{:x}", Sha256::digest(raw_bytes));

    db.create_evidenced_claim(
        &MemoryClaim {
            id: "claim:raw-check".into(),
            statement: "Legacy memory claim".into(),
            asserted_by: "user".into(),
            epistemic_origin: "explicit".into(),
            status: "active".into(),
        },
        &[ClaimEvidence {
            source_ref: s1.clone(),
            evidence_kind: "support".into(),
        }],
    )
    .unwrap();

    let g1 = db
        .create_staged_generation(&NewCurationGeneration {
            id: "gen:r1",
            topic_id: &topic_id,
            run_id: "run:r1",
            profile: CurationProfile::Detailed,
            asserted_by: AssertionSource::Model,
            model_id: Some("claude-haiku-5.5"),
            base_generation_id: None,
        })
        .unwrap();
    db.append_staged_ops(
        &g1.id,
        "shard-00",
        &[
            CnfRecord::Upsert {
                key: "sibambo/fact/old-fact".into(),
                kind: CuratedNodeKind::Fact,
                title: "Old Fact".into(),
                summary: "Original summary".into(),
                body: None,
                attrs: BTreeMap::new(),
                aliases: vec!["legacy fact".into()],
                evidence: vec![span(&s1, 0, 0, 10, None)],
                confidence: Some(0.7),
                base_rev: 0,
            },
            CnfRecord::Upsert {
                key: "sibambo/task/verify-bom".into(),
                kind: CuratedNodeKind::Task,
                title: "Verify BOM".into(),
                summary: "Run parity check".into(),
                body: Some("Compare against spreadsheets.".into()),
                attrs: BTreeMap::from([("due".into(), AttrValue::String("2026-12-01".into()))]),
                aliases: vec![],
                evidence: vec![span(&s1, 1, 5, 25, None)],
                confidence: Some(0.9),
                base_rev: 0,
            },
        ],
    )
    .unwrap();
    db.publish_generation(&g1.id).unwrap();

    let g2 = db
        .create_staged_generation(&NewCurationGeneration {
            id: "gen:r2",
            topic_id: &topic_id,
            run_id: "run:r2",
            profile: CurationProfile::Detailed,
            asserted_by: AssertionSource::Model,
            model_id: Some("claude-sonnet-5.5"),
            base_generation_id: Some(&g1.id),
        })
        .unwrap();
    db.append_staged_ops(
        &g2.id,
        "shard-00",
        &[
            CnfRecord::Supersede {
                key: "sibambo/fact/new-fact".into(),
                kind: CuratedNodeKind::Fact,
                supersedes: "sibambo/fact/old-fact".into(),
                title: "New Fact".into(),
                summary: "Updated summary".into(),
                body: None,
                attrs: BTreeMap::new(),
                aliases: vec!["current fact".into()],
                evidence: vec![span(&s2, 2, 0, 30, None)],
                confidence: Some(0.95),
                base_rev: 0,
            },
            CnfRecord::Retire {
                key: "sibambo/task/verify-bom".into(),
                reason: Some("Completed".into()),
                evidence: vec![span(&s2, 3, 0, 18, None)],
                base_rev: 1,
            },
            CnfRecord::Link {
                from: "sibambo/fact/new-fact".into(),
                rel: CuratedEdgeRel::Supports,
                to: "sibambo/task/verify-bom".into(),
                evidence: vec![span(&s2, 4, 0, 15, None)],
                confidence: Some(0.85),
            },
        ],
    )
    .unwrap();
    db.publish_generation(&g2.id).unwrap();

    // Also add one staged (unpublished) generation and one topic membership pointing to `new-fact`.
    let g_staged = db
        .create_staged_generation(&NewCurationGeneration {
            id: "gen:staged-later",
            topic_id: &topic_id,
            run_id: "run:staged",
            profile: CurationProfile::Brief,
            asserted_by: AssertionSource::Model,
            model_id: Some("claude-haiku-5.5"),
            base_generation_id: Some(&g2.id),
        })
        .unwrap();
    db.append_staged_ops(
        &g_staged.id,
        "shard-00",
        &[CnfRecord::Upsert {
            key: "sibambo/question/unpublished".into(),
            kind: CuratedNodeKind::Question,
            title: "Unpublished Question".into(),
            summary: "Should never appear in rebuild".into(),
            body: None,
            attrs: BTreeMap::new(),
            aliases: vec![],
            evidence: vec![span(&s1, 9, 0, 8, None)],
            confidence: None,
            base_rev: 0,
        }],
    )
    .unwrap();

    let new_fact = db
        .get_curated_node("sibambo/fact/new-fact")
        .unwrap()
        .unwrap();
    db.add_topic_membership(&NewTopicMembership {
        id: "mem:curated-node-1",
        topic_id: &topic_id,
        member_kind: TopicMemberKind::CuratedNode,
        member_id: &new_fact.id,
        role: TopicMemberRole::Primary,
        asserted_by: AssertionSource::User,
        epistemic_origin: "explicit",
        evidence_source_ref: None,
        evidence_locator: None,
        confidence: None,
    })
    .unwrap();

    let digest_before = db.curated_projection_digest().unwrap();
    let md_before = db.export_topic_markdown(&topic_id).unwrap();
    assert_eq!(md_before.accepted_node_count, 3);
    assert_eq!(md_before.accepted_edge_count, 2);
    assert!(!md_before.markdown.contains("Unpublished Question"));
    assert!(md_before.markdown.contains("Superseded by"));
    assert!(md_before.markdown.contains("`retired`"));

    // Corrupt materialized projection via raw SQL, keeping authoritative op log intact.
    {
        let raw = Connection::open(db.path()).unwrap();
        raw.pragma_update(None, "foreign_keys", "ON").unwrap();
        raw.execute_batch(
            "DELETE FROM curated_edge_evidence_spans;
             DELETE FROM curated_edges;
             DELETE FROM curated_node_evidence_spans;
             DELETE FROM curated_node_revisions;
             DELETE FROM curated_node_aliases;
             UPDATE curated_nodes SET title='CORRUPTED', summary='CORRUPTED';",
        )
        .unwrap();
        // Verify opening fail-closes while evidence spans are missing.
        assert!(matches!(
            StateStore::open(db.path()),
            Err(StateError::Corrupt)
        ));
    }

    let rebuild1 = db.rebuild_curated_store().unwrap();
    let rebuild2 = db.rebuild_curated_store().unwrap();
    assert_eq!(rebuild1, rebuild2);
    assert_eq!(rebuild1.projection_sha256, digest_before);
    assert_eq!(rebuild1.published_generations, 2);
    assert_eq!(rebuild1.node_count, 3);
    assert_eq!(rebuild1.revision_count, 5);
    assert_eq!(rebuild1.edge_count, 2);

    let md_after = db.export_topic_markdown("sibambo").unwrap();
    assert_eq!(md_after, md_before);
    assert_eq!(
        db.list_topic_memberships(&topic_id, false, 50)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        db.claim_as_of("claim:raw-check", i64::MAX)
            .unwrap()
            .unwrap()
            .statement,
        "Legacy memory claim"
    );
    let raw_sha_after = format!("{:x}", Sha256::digest(fs::read(&raw_file).unwrap()));
    assert_eq!(raw_sha_after, raw_sha_before);
    let integrity = db.verify_curated_integrity().unwrap();
    assert_eq!(integrity.foreign_key_violations, 0);
    assert_eq!(integrity.projection_sha256, digest_before);

    // FK integrity rejects unregistered source references at both API and SQL levels.
    let unregistered = format!("src:v1:{}", "e".repeat(64));
    assert!(db
        .append_staged_ops(
            &g_staged.id,
            "shard-01",
            &[CnfRecord::Upsert {
                key: "sibambo/fact/forged-src".into(),
                kind: CuratedNodeKind::Fact,
                title: "Forged".into(),
                summary: "Forged source".into(),
                body: None,
                attrs: BTreeMap::new(),
                aliases: vec![],
                evidence: vec![span(&unregistered, 0, 0, 5, None)],
                confidence: None,
                base_rev: 0,
            }]
        )
        .is_err());

    let raw = Connection::open(db.path()).unwrap();
    raw.pragma_update(None, "foreign_keys", "ON").unwrap();
    assert!(raw
        .execute(
            "INSERT INTO curated_edge_evidence_spans
             (edge_id,ordinal,source_ref,msg_index,span_start,span_end,span_sha,quote)
             VALUES (?1,9,?2,0,0,5,?3,NULL)",
            rusqlite::params![
                db.list_topic_curated_edges(&topic_id, false, 10).unwrap()[0].id,
                unregistered,
                format!("b3:{}", "1".repeat(64)),
            ],
        )
        .is_err());
}

#[test]
fn v9_forward_migration_creates_backup_unifies_projects_and_rolls_back_on_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.sqlite3");

    let build_v9 = |target: &std::path::Path| {
        let conn = Connection::open(target).unwrap();
        conn.pragma_update(None, "application_id", APPLICATION_ID)
            .unwrap();
        for (version, sql) in [
            (1, include_str!("../src/schema_v1.sql")),
            (2, include_str!("../src/schema_v2.sql")),
            (3, include_str!("../src/schema_v3.sql")),
            (4, include_str!("../src/schema_v4.sql")),
            (5, include_str!("../src/schema_v5.sql")),
            (6, include_str!("../src/schema_v6.sql")),
            (7, include_str!("../src/schema_v7.sql")),
            (8, include_str!("../src/schema_v8.sql")),
            (9, include_str!("../src/schema_v9.sql")),
        ] {
            conn.execute_batch(sql).unwrap();
            let hash = format!("{:x}", Sha256::digest(sql.as_bytes()));
            conn.execute(
                "INSERT INTO schema_migrations(version,checksum) VALUES(?1,?2)",
                rusqlite::params![version, hash],
            )
            .unwrap();
        }
        conn.execute_batch(
            "INSERT INTO projects(id,name,status) VALUES('proj:sibambo','Sibambo Studio','active');
             INSERT INTO projects(id,name,status) VALUES('proj:old','Archived Work','archived');
             INSERT INTO topics(id,name) VALUES('topic:legacy','Legacy Research');
             INSERT INTO topics(id,name) VALUES('topic:mapped','Mapped Project Topic');
             INSERT INTO project_topics(project_id,topic_id) VALUES('proj:old','topic:mapped');",
        )
        .unwrap();
        conn.pragma_update(None, "user_version", 9).unwrap();
    };

    build_v9(&path);
    let upgraded = StateStore::open(&path).unwrap();
    assert_eq!(SCHEMA_VERSION, 10);
    assert_eq!(
        upgraded.get_project("proj:sibambo").unwrap().unwrap().name,
        "Sibambo Studio"
    );
    let sibambo_topic = upgraded.get_topic("proj:sibambo").unwrap().unwrap();
    assert!(sibambo_topic.is_project);
    let mapped_topic = upgraded.get_topic("topic:mapped").unwrap().unwrap();
    assert!(mapped_topic.is_project);
    assert_eq!(
        mapped_topic.work_status,
        Some(mind_place_state::TopicWorkStatus::Archived)
    );
    let legacy_topic = upgraded.get_topic("topic:legacy").unwrap().unwrap();
    assert_eq!(legacy_topic.slug, "legacy-research");
    assert!(!legacy_topic.is_project);

    let backups: Vec<_> = fs::read_dir(dir.path().join("state-backups"))
        .unwrap()
        .collect();
    assert_eq!(backups.len(), 1);
    let backup = Connection::open(backups[0].as_ref().unwrap().path()).unwrap();
    assert_eq!(
        backup
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        9
    );

    // Failed v10 migration rolls back atomically and preserves v9 state + backup.
    let fail_dir = tempfile::tempdir().unwrap();
    let fail_path = fail_dir.path().join("state.sqlite3");
    build_v9(&fail_path);
    let conn = Connection::open(&fail_path).unwrap();
    conn.execute_batch("CREATE TABLE curated_nodes(sentinel TEXT);")
        .unwrap();
    drop(conn);
    assert!(StateStore::open(&fail_path).is_err());
    let after = Connection::open(&fail_path).unwrap();
    assert_eq!(
        after
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        9
    );
    assert_eq!(
        fs::read_dir(fail_dir.path().join("state-backups"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn link_supersedes_batch_shard_limits_and_read_only_markdown_export() {
    let (_dir, db, topic_id, s1, s2) = fixture();

    // Batch limit (> 200 records) is rejected cleanly.
    let g_limits = db
        .create_staged_generation(&NewCurationGeneration {
            id: "gen:limits",
            topic_id: &topic_id,
            run_id: "run:limits",
            profile: CurationProfile::Brief,
            asserted_by: AssertionSource::Model,
            model_id: Some("claude-haiku-5.5"),
            base_generation_id: None,
        })
        .unwrap();
    let too_many: Vec<CnfRecord> = (0..201)
        .map(|i| CnfRecord::Upsert {
            key: format!("sibambo/fact/item-{i}"),
            kind: CuratedNodeKind::Fact,
            title: format!("Item {i}"),
            summary: "Summary".into(),
            body: None,
            attrs: BTreeMap::new(),
            aliases: vec![],
            evidence: vec![span(&s1, i, 0, 5, None)],
            confidence: None,
            base_rev: 0,
        })
        .collect();
    assert!(matches!(
        db.append_staged_ops(&g_limits.id, "shard-00", &too_many),
        Err(StateError::CuratedLimit)
    ));

    // > 64 shards in one generation is rejected cleanly.
    for shard_idx in 0..64 {
        db.append_staged_ops(
            &g_limits.id,
            &format!("s-{shard_idx:02}"),
            &[CnfRecord::Upsert {
                key: format!("sibambo/fact/s-{shard_idx:02}"),
                kind: CuratedNodeKind::Fact,
                title: format!("Shard {shard_idx}"),
                summary: "Valid shard item".into(),
                body: None,
                attrs: BTreeMap::new(),
                aliases: vec![],
                evidence: vec![span(&s1, shard_idx, 0, 5, None)],
                confidence: None,
                base_rev: 0,
            }],
        )
        .unwrap();
    }
    assert!(matches!(
        db.append_staged_ops(
            &g_limits.id,
            "s-64",
            &[CnfRecord::Upsert {
                key: "sibambo/fact/s-64".into(),
                kind: CuratedNodeKind::Fact,
                title: "Shard 64".into(),
                summary: "Over shard cap".into(),
                body: None,
                attrs: BTreeMap::new(),
                aliases: vec![],
                evidence: vec![span(&s1, 64, 0, 5, None)],
                confidence: None,
                base_rev: 0,
            }]
        ),
        Err(StateError::CuratedLimit)
    ));
    db.reject_generation(&g_limits.id).unwrap();

    // Link(Supersedes) + Markdown export escaping & determinism.
    let g_pub = db
        .create_staged_generation(&NewCurationGeneration {
            id: "gen:export",
            topic_id: &topic_id,
            run_id: "run:export",
            profile: CurationProfile::Detailed,
            asserted_by: AssertionSource::Model,
            model_id: Some("claude-sonnet-5.5"),
            base_generation_id: None,
        })
        .unwrap();
    db.append_staged_ops(
        &g_pub.id,
        "shard-00",
        &[
            CnfRecord::Upsert {
                key: "sibambo/decision/old-choice".into(),
                kind: CuratedNodeKind::Decision,
                title: "Old <script>Choice</script>".into(),
                summary: "Earlier choice <img src=x>".into(),
                body: Some("Detailed <note>".into()),
                attrs: BTreeMap::from([("status".into(), AttrValue::String("legacy".into()))]),
                aliases: vec!["old choice".into()],
                evidence: vec![span(&s1, 0, 0, 12, None)],
                confidence: Some(0.75),
                base_rev: 0,
            },
            CnfRecord::Upsert {
                key: "sibambo/decision/new-choice".into(),
                kind: CuratedNodeKind::Decision,
                title: "New Choice".into(),
                summary: "Current accepted choice.".into(),
                body: None,
                attrs: BTreeMap::from([("status".into(), AttrValue::String("active".into()))]),
                aliases: vec!["new choice".into()],
                evidence: vec![span(&s2, 1, 10, 40, None)],
                confidence: Some(0.95),
                base_rev: 0,
            },
            CnfRecord::Link {
                from: "sibambo/decision/new-choice".into(),
                rel: CuratedEdgeRel::Supersedes,
                to: "sibambo/decision/old-choice".into(),
                evidence: vec![span(&s2, 2, 0, 15, None)],
                confidence: Some(0.95),
            },
        ],
    )
    .unwrap();
    db.publish_generation(&g_pub.id).unwrap();

    let old_node = db
        .get_curated_node("sibambo/decision/old-choice")
        .unwrap()
        .unwrap();
    assert_eq!(old_node.status, CuratedNodeStatus::Superseded);

    let export_a = db.export_topic_markdown("sibambo").unwrap();
    let export_b = db.export_topic_markdown(&topic_id).unwrap();
    assert_eq!(export_a, export_b);
    assert!(!export_a.markdown.contains("<script>"));
    assert!(!export_a.markdown.contains("<img"));
    assert!(export_a
        .markdown
        .contains("&lt;script&gt;Choice&lt;/script&gt;"));
    assert!(export_a
        .markdown
        .contains("Superseded by:** `sibambo/decision/new-choice`"));
    assert!(export_a
        .markdown
        .contains("Supersedes:** `sibambo/decision/old-choice`"));
    assert!(export_a.markdown.contains(&s1));
    assert!(export_a.markdown.contains(&s2));
}

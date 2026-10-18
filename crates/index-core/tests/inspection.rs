use mind_place_archive::{ArchiveStore, Capture, Message};
use mind_place_index::{
    ContextPreset, ContextRequest, FetchRequest, IndexError, IndexStore, MatchMode, QueryTerm,
    RetrievalRequest, WeightedQuery,
};
use mind_place_state::StateStore;

const A: &str = "11111111-2222-4333-8444-555555555555";
const B: &str = "22222222-3333-4444-8555-666666666666";

fn capture(id: &str, text: &str, date: &str) -> Capture {
    Capture {
        schema_version: 1,
        provider: "chatgpt".into(),
        conversation_id: id.into(),
        source_url: format!("https://chatgpt.com/c/{id}"),
        title: "Synthetic ranking fixture".into(),
        conversation_created_at: None,
        conversation_updated_at: None,
        captured_at: date.into(),
        capture_method: "manual-dom".into(),
        completeness: "partial".into(),
        warnings: vec![],
        messages: vec![Message {
            id: "human".into(),
            role: "user".into(),
            text: text.into(),
            format: "plain".into(),
            authored_at: None,
            model: None,
            attachments: vec![],
        }],
    }
}

struct Rig {
    _tmp: tempfile::TempDir,
    archive: ArchiveStore,
    authority: StateStore,
    index: IndexStore,
}

impl Rig {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let archive = ArchiveStore::open(tmp.path().join("archive-v1")).unwrap();
        let authority = StateStore::open(tmp.path().join("state.sqlite3")).unwrap();
        let index = IndexStore::open(tmp.path().join("index.sqlite3")).unwrap();
        Self {
            _tmp: tmp,
            archive,
            authority,
            index,
        }
    }

    fn add(&self, id: &str, text: &str, date: &str) {
        self.archive.save(capture(id, text, date)).unwrap();
        self.index.sync(&self.archive, &self.authority).unwrap();
    }
}

fn terms() -> RetrievalRequest {
    RetrievalRequest {
        queries: vec![
            QueryTerm::Text("quartzmarker".into()),
            QueryTerm::Weighted(WeightedQuery {
                text: "quartzmarker crimson".into(),
                weight: 6.0,
                mode: MatchMode::Exact,
            }),
        ],
        ..Default::default()
    }
}

#[test]
fn weighted_exact_rank_and_components_are_reproducible_after_rebuild() {
    let rig = Rig::new();
    rig.add(A, "quartzmarker crimson decision", "2026-10-17T09:00:00Z");
    rig.add(B, "quartzmarker blue alternative", "2026-10-18T09:00:00Z");
    let initial = rig.index.inspect_retrieval(&terms()).unwrap();
    assert_eq!(initial.trace.operation, "search");
    assert_eq!(initial.trace.schema_version, 1);
    assert_eq!(initial.trace.candidate_count, 2);
    assert_eq!(initial.trace.result_count, 2);
    assert_eq!(initial.trace.source_count, 2);
    assert_eq!(initial.result.hits[0].conversation_id, A);
    assert_eq!(initial.result.hits[0].matched_queries, vec![0, 1]);
    assert_eq!(initial.result.hits[0].exact_queries, vec![1]);
    assert_eq!(initial.result.hits[0].score_contributions.len(), 2);
    assert_eq!(initial.result.hits[1].score_contributions.len(), 1);
    assert!(initial.result.hits[0].score > initial.result.hits[1].score);
    for hit in &initial.result.hits {
        assert!(hit.source_id.starts_with("src:v1:"));
        assert_eq!(hit.kind, "message");
        let sum = hit
            .score_contributions
            .iter()
            .map(|part| part.weighted_score)
            .sum::<f64>();
        assert!((sum - hit.score).abs() < 1e-9);
        assert!(hit
            .score_contributions
            .iter()
            .all(|part| part.bm25.is_finite()
                && part.lexical_relevance.is_finite()
                && part.weighted_score.is_finite()
                && part.exact_bonus >= 0.0));
    }
    assert_eq!(
        initial.result.hits[0].score_contributions[1].exact_bonus,
        1.0
    );

    rig.index.rebuild(&rig.archive, &rig.authority).unwrap();
    let rebuilt = rig.index.inspect_retrieval(&terms()).unwrap();
    // Time and index generation are deliberately not compared.
    assert_eq!(rebuilt.result, initial.result);
    assert!(rebuilt.trace.index_generation > initial.trace.index_generation);
}

#[test]
fn inspector_traces_do_not_include_user_corpus_or_identifiers() {
    let rig = Rig::new();
    let private = "quartzmarker sensitive-do-not-log-9382";
    rig.add(A, private, "2026-10-18T09:00:00Z");
    let search = rig.index.inspect_retrieval(&terms()).unwrap();
    let trace = serde_json::to_string(&search.trace).unwrap();
    assert!(!trace.contains(private));
    assert!(!trace.contains("quartzmarker"));
    assert!(!trace.contains(A));
    assert!(!trace.contains("chatgpt.com"));
    assert!(!trace.contains("src:v1:"));
    assert_eq!(search.trace.text_bytes, 0);
    let hit = search.result.hits.first().unwrap();
    let exact_fetch = FetchRequest {
        source_id: hit.source_id.clone(),
        archive_id: hit.archive_id.clone(),
        snapshot_id: hit.snapshot_id.clone(),
        message_id: Some(hit.message_id.clone()),
        ..Default::default()
    };
    let fetch = rig.index.inspect_fetch(&rig.archive, &exact_fetch).unwrap();
    assert_eq!(fetch.trace.operation, "fetch");
    assert_eq!(fetch.trace.result_count, 1);
    assert_eq!(fetch.trace.candidate_count, 0);
    assert_eq!(fetch.trace.text_bytes, private.len());
    assert!(fetch.result.segments[0].text.contains(private));
    assert!(!serde_json::to_string(&fetch.trace)
        .unwrap()
        .contains(private));

    let context_request = ContextRequest {
        queries: vec![QueryTerm::Text("quartzmarker".into())],
        preset: ContextPreset::Fast,
        ..Default::default()
    };
    let context = rig
        .index
        .inspect_context(&rig.archive, &context_request)
        .unwrap();
    assert_eq!(context.trace.operation, "context");
    assert_eq!(context.trace.text_bytes, context.result.text_bytes);
    assert_eq!(context.trace.source_count, 1);
    assert_eq!(context.trace.result_count, context.result.excerpt_count);
    assert!(!serde_json::to_string(&context.trace)
        .unwrap()
        .contains(private));
}

#[test]
fn inspector_respects_structured_filters_and_history_selection() {
    let rig = Rig::new();
    rig.add(A, "quartzmarker crimson old", "2026-09-01T09:00:00Z");
    rig.add(A, "quartzmarker crimson current", "2026-10-18T09:00:00Z");
    let current = rig.index.inspect_retrieval(&terms()).unwrap();
    assert_eq!(current.result.hits.len(), 1);
    let mut historical = terms();
    historical.filters.include_history = true;
    let all = rig.index.inspect_retrieval(&historical).unwrap();
    assert_eq!(all.result.hits.len(), 2);
    assert_ne!(
        all.result.hits[0].snapshot_id,
        all.result.hits[1].snapshot_id
    );
    historical.filters.providers = vec!["claude".into()];
    let filtered = rig.index.inspect_retrieval(&historical).unwrap();
    assert_eq!(filtered.trace.result_count, 0);
    assert_eq!(filtered.trace.source_count, 0);
    assert_eq!(filtered.result.hits.len(), 0);
}

#[test]
fn inspector_check_callback_revokes_before_any_payload() {
    let rig = Rig::new();
    rig.add(A, "quartzmarker crimson protected", "2026-10-18T09:00:00Z");
    let mut checks = 0;
    let denied = rig.index.inspect_retrieval_with_check(&terms(), || {
        checks += 1;
        if checks >= 2 {
            Err(IndexError::Revoked)
        } else {
            Ok(())
        }
    });
    assert!(matches!(denied, Err(IndexError::Revoked)));
    assert!(checks >= 2);
    let mut checks = 0;
    let context_request = ContextRequest {
        queries: vec![QueryTerm::Text("quartzmarker".into())],
        ..Default::default()
    };
    let denied = rig
        .index
        .inspect_context_with_check(&rig.archive, &context_request, || {
            checks += 1;
            if checks >= 3 {
                Err(IndexError::Revoked)
            } else {
                Ok(())
            }
        });
    assert!(matches!(denied, Err(IndexError::Revoked)));
    assert!(checks >= 3);
}

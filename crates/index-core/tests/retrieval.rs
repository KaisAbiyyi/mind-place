use mind_place_archive::{ArchiveStore, Capture, Message};
use mind_place_index::{
    IndexError, IndexStore, MatchMode, QueryTerm, RetrievalFilters, RetrievalRequest, WeightedQuery,
};
use mind_place_state::StateStore;

const A: &str = "11111111-2222-4333-8444-555555555555";
const B: &str = "22222222-3333-4444-8555-666666666666";

fn capture(
    id: &str,
    title: &str,
    user_text: &str,
    assistant_text: &str,
    captured_at: &str,
) -> Capture {
    Capture {
        schema_version: 1,
        provider: "chatgpt".into(),
        conversation_id: id.into(),
        source_url: format!("https://chatgpt.com/c/{id}"),
        title: title.into(),
        conversation_created_at: None,
        conversation_updated_at: None,
        captured_at: captured_at.into(),
        capture_method: "manual-dom".into(),
        completeness: "partial".into(),
        warnings: vec![],
        messages: vec![
            Message {
                id: "human-1".into(),
                role: "user".into(),
                text: user_text.into(),
                format: "plain".into(),
                authored_at: None,
                model: None,
                attachments: vec![],
            },
            Message {
                id: "ai-1".into(),
                role: "assistant".into(),
                text: assistant_text.into(),
                format: "plain".into(),
                authored_at: None,
                model: Some("test-gpt".into()),
                attachments: vec![],
            },
        ],
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    archive: ArchiveStore,
    state: StateStore,
    index: IndexStore,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let archive = ArchiveStore::open(dir.path().join("archive-v1")).unwrap();
        let state = StateStore::open(dir.path().join("state.sqlite3")).unwrap();
        let index = IndexStore::open(dir.path().join("index.sqlite3")).unwrap();
        Self {
            _dir: dir,
            archive,
            state,
            index,
        }
    }
    fn sync(&self) {
        self.index.sync(&self.archive, &self.state).unwrap();
    }
    fn search(&self, terms: Vec<QueryTerm>, filters: RetrievalFilters, limit: usize) -> mind_place_index::RetrievalResponse {
        self.index
            .retrieve(&RetrievalRequest { queries: terms, filters, limit })
            .unwrap()
    }
}
fn text(s: &str) -> QueryTerm {
    QueryTerm::Text(s.into())
}
fn exact(s: &str, weight: f64) -> QueryTerm {
    QueryTerm::Weighted(WeightedQuery {
        text: s.into(),
        weight,
        mode: MatchMode::Exact,
    })
}

#[test]
fn combines_weighted_queries_deduplicates_chunks_and_preserves_provenance() {
    let f = Fixture::new();
    let a = f.archive.save(capture(
        A, "Roadmap", "moonlit lantern planning and research",
        "alphahelp answer", "2026-10-17T10:00:00Z",
    )).unwrap();
    f.archive.save(capture(
        B, "Another project", "moonlit outreach and execution",
        "bravohelp answer", "2026-10-18T10:00:00Z",
    )).unwrap();
    f.sync();
    let request = RetrievalRequest {
        queries: vec![text("moonlit"), exact("moonlit lantern", 4.0)],
        filters: RetrievalFilters::default(),
        limit: 10,
    };
    let result = f.index.retrieve(&request).unwrap();
    assert_eq!(result.candidate_count, 2);
    assert!(!result.truncated);
    assert_eq!(result.hits[0].snapshot_id, a.snapshot_id);
    assert_eq!(result.hits[0].message_id, "human-1");
    assert_eq!(result.hits[0].matched_queries, vec![0, 1]);
    assert_eq!(result.hits[0].exact_queries, vec![1]);
    assert_eq!(result.hits[0].provider, "chatgpt");
    assert_eq!(result.hits[0].source_url, format!("https://chatgpt.com/c/{A}"));
    assert_eq!(result.hits[0].conversation_id, A);
    assert!(result.hits[0].source_id.starts_with("src:v1:"));
    assert_eq!(result.hits[0].part, 0);
    assert_eq!(f.index.retrieve(&request).unwrap(), result);
    let compact = f.search(vec![text("moonlit")], RetrievalFilters::default(), 1);
    assert_eq!(compact.hits.len(), 1);
    assert_eq!(compact.candidate_count, 2);
    assert!(compact.truncated);
}

#[test]
fn structured_filters_intersect_without_leaking_other_messages() {
    let f = Fixture::new();
    let saved = f.archive.save(capture(
        A, "Starbase alpha", "orbitapple visible user text",
        "orbitapple target assistant", "2026-10-17T10:00:00Z",
    )).unwrap();
    f.archive.save(capture(
        B, "Other starbase", "orbitapple other user",
        "orbitapple other assistant", "2026-10-18T10:00:00Z",
    )).unwrap();
    f.sync();
    let base = RetrievalFilters {
        providers: vec!["chatgpt".into()],
        models: vec!["test-gpt".into()],
        roles: vec!["assistant".into()],
        kinds: vec!["message".into()],
        archive_ids: vec![saved.id],
        conversation_ids: vec![A.into()],
        message_ids: vec!["ai-1".into()],
        captured_from: Some("2026-10-17T09:00:00+00:00".into()),
        captured_to: Some("2026-10-17T11:00:00Z".into()),
        ..Default::default()
    };
    let found = f.search(vec![text("orbitapple")], base.clone(), 10);
    assert_eq!(found.hits.len(), 1);
    assert_eq!(found.hits[0].message_id, "ai-1");
    assert_eq!(found.hits[0].model.as_deref(), Some("test-gpt"));
    assert_eq!(found.hits[0].role, "assistant");
    let source_id = found.hits[0].source_id.clone();
    let narrowed = RetrievalFilters { source_ids: vec![source_id], ..base.clone() };
    assert_eq!(f.search(vec![text("orbitapple")], narrowed, 10).hits.len(), 1);

    let absent = [
        RetrievalFilters { providers: vec!["claude".into()], ..base.clone() },
        RetrievalFilters { models: vec!["other-model".into()], ..base.clone() },
        RetrievalFilters { roles: vec!["user".into()], ..base.clone() },
        RetrievalFilters { captured_from: Some("2026-10-18T00:00:00Z".into()), ..base.clone() },
        RetrievalFilters { conversation_ids: vec![B.into()], ..base.clone() },
    ];
    for filters in absent {
        assert!(f.search(vec![text("orbitapple")], filters, 10).hits.is_empty());
    }
    let title = f.search(
        vec![text("Starbase")],
        RetrievalFilters {
            kinds: vec!["title".into()],
            roles: vec!["title".into()],
            ..Default::default()
        },
        10,
    );
    assert_eq!(title.hits.len(), 2);
    assert!(title.hits.iter().all(|h| h.kind == "title"));
}

#[test]
fn defaults_current_only_and_history_requires_explicit_opt_in() {
    let f = Fixture::new();
    let first = f.archive.save(capture(
        A, "Archives", "amberhistory initial evidence", "ok",
        "2026-10-17T10:00:00Z",
    )).unwrap();
    f.sync();
    let second = f.archive.save(capture(
        A, "Archives", "cobaltcurrent latest evidence", "ok",
        "2026-10-18T10:00:00Z",
    )).unwrap();
    f.sync();
    assert!(f.search(vec![text("amberhistory")], RetrievalFilters::default(), 10).hits.is_empty());
    let historical = f.search(
        vec![text("amberhistory")],
        RetrievalFilters { include_history: true, ..Default::default() },
        10,
    );
    assert_eq!(historical.hits.len(), 1);
    assert_eq!(historical.hits[0].snapshot_id, first.snapshot_id);
    let current = f.search(vec![text("cobaltcurrent")], RetrievalFilters::default(), 10);
    assert_eq!(current.hits.len(), 1);
    assert_eq!(current.hits[0].snapshot_id, second.snapshot_id);
    assert_eq!(historical.hits[0].source_id, current.hits[0].source_id);
}

#[test]
fn rejects_invalid_requests_and_unsupported_filters_without_sql_injection() {
    let f = Fixture::new();
    f.archive.save(capture(A, "SQL tests", "needle cautious", "ok", "2026-10-18T10:00:00Z")).unwrap();
    f.sync();
    let invalid = [
        RetrievalRequest::default(),
        RetrievalRequest { queries: vec![text("   ")], ..Default::default() },
        RetrievalRequest { queries: vec![text("a")], limit: 101, ..Default::default() },
        RetrievalRequest { queries: vec![text("a"); 9], ..Default::default() },
        RetrievalRequest { queries: vec![text("a")], filters: RetrievalFilters {
            models: vec!["".into()], ..Default::default()
        }, ..Default::default() },
        RetrievalRequest { queries: vec![text("a")], filters: RetrievalFilters {
            captured_from: Some("tomorrow".into()), ..Default::default()
        }, ..Default::default() },
        RetrievalRequest { queries: vec![text("a")], filters: RetrievalFilters {
            captured_from: Some("2026-10-19T00:00:00Z".into()),
            captured_to: Some("2026-10-18T00:00:00Z".into()),
            ..Default::default()
        }, ..Default::default() },
        RetrievalRequest { queries: vec![QueryTerm::Weighted(WeightedQuery {
            text: "needle".into(), weight: f64::NAN, mode: MatchMode::Lexical,
        })], ..Default::default() },
    ];
    for request in invalid {
        assert!(matches!(f.index.retrieve(&request), Err(IndexError::InvalidQuery)));
    }
    assert!(serde_json::from_str::<RetrievalRequest>(r#"{"queries":["needle"],"filters":{"projects":["secret"]}}"#).is_err());
    let malicious = f.search(vec![text(r#"needle" OR *"#)], RetrievalFilters::default(), 10);
    assert!(malicious.hits.is_empty());
    assert_eq!(f.search(vec![exact("needle cautious", 1.0)], RetrievalFilters::default(), 10).hits.len(), 1);
}

#[test]
fn accepts_compact_json_queries_without_exposing_schema_internals() {
    let request: RetrievalRequest = serde_json::from_str(
        r#"{"queries":["first issue",{"text":"second phrase","weight":2.0,"mode":"exact"}],"filters":{"include_history":true,"providers":["chatgpt"]}}"#,
    ).unwrap();
    assert_eq!(request.limit, 20);
    assert_eq!(request.queries.len(), 2);
    assert!(request.filters.include_history);
}

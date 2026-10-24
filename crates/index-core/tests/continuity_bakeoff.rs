//! MP-10B: synthetic, isolated bake-off using the actual IndexStore FTS5 retrieval engine.
//! This test-only projection is NOT the application's authorized source ingestion path.
//! The real graph API runs on a synthetic, test-only derived projection, not live authority.
use mind_place_index::{
    GraphDirection, GraphEdge, GraphEvidence, GraphKind, GraphNeighborhoodRequest, GraphNode,
    GraphNodeRef, GraphOrigin, GraphProjection, GraphRelation, IndexStore, MatchMode, QueryTerm,
    RetrievalFilters, RetrievalRequest, WeightedQuery,
};
use rusqlite::{params, Connection};
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    time::Instant,
};

const CORPUS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../benchmarks/continuity/v1/corpus.json"
));
const GOLD: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../benchmarks/continuity/v1/gold.json"
));
const MAX_HITS: usize = 12;

#[derive(Deserialize)]
struct Corpus {
    dataset: String,
    sources: Vec<Source>,
}
#[derive(Deserialize)]
struct Source {
    id: String,
    provider: String,
    kind: String,
    project: String,
    observed_at: String,
    revision: String,
    scope: String,
    text: String,
}
#[derive(Deserialize)]
struct Gold {
    dataset: String,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    id: String,
    question: String,
}

fn project_from_question(question: &str) -> Option<&'static str> {
    let lower = question.to_ascii_lowercase();
    if lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|w| w == "aster")
    {
        Some("aster")
    } else if lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|w| w == "beryl")
    {
        Some("beryl")
    } else {
        None
    }
}

// All query terms come solely from question text, not gold relevance, claim IDs or gold answers.
// No learned model, network calls, user files or oracle hits.
fn terms(question: &str) -> Vec<QueryTerm> {
    const STOP: &[&str] = &[
        "what",
        "which",
        "does",
        "is",
        "are",
        "the",
        "for",
        "can",
        "how",
        "who",
        "with",
        "after",
        "before",
        "when",
        "from",
        "into",
        "still",
        "was",
        "were",
        "has",
        "have",
        "its",
        "and",
        "or",
        "on",
        "of",
        "to",
        "do",
        "did",
        "a",
        "an",
        "at",
        "as",
        "in",
        "about",
        "aster",
        "beryl",
        "currently",
        "first",
        "formerly",
        "any",
    ];
    let mut seen = HashSet::new();
    question
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| s.len() >= 3 && !STOP.contains(s) && seen.insert((*s).to_owned()))
        .take(8)
        .map(|s| {
            QueryTerm::Weighted(WeightedQuery {
                text: s.to_owned(),
                weight: 1.0,
                mode: MatchMode::Lexical,
            })
        })
        .collect()
}

fn fixture_index(index: &IndexStore, corpus: &Corpus) {
    let mut db = Connection::open(index.path()).expect("isolated derived index");
    let tx = db.transaction().unwrap();
    // The actual runtime ingestion requires state authority. We intentionally seed ONLY
    // ephemeral synthetic derived tables with stable fixture IDs, never a live archive.
    for source in &corpus.sources {
        if source.scope != "public" {
            continue;
        }
        tx.execute(
            "INSERT INTO revisions(source_id,archive_id,snapshot_id,provider,conversation_id,title,source_url,captured_at,completeness,is_current,seen_generation) VALUES(?1,?1,?2,?3,?1,?4,?5,?6,'complete',1,1)",
            params![source.id, format!("synthetic-{}", source.revision), source.provider, source.id,
                format!("synthetic://{}", source.id), format!("{}T00:00:00Z", source.observed_at)],
        ).unwrap();
        tx.execute(
            "INSERT INTO chunks(source_id,snapshot_id,message_id,part,kind,role,model,body) VALUES(?1,?2,?3,0,?4,'assistant',NULL,?5)",
            params![source.id, format!("synthetic-{}", source.revision), format!("{}-msg",source.id), source.kind, source.text],
        ).unwrap();
    }
    tx.commit().unwrap();
}

fn fts(index: &IndexStore, corpus: &Corpus, question: &str, scoped: bool) -> Vec<String> {
    let terms = terms(question);
    if terms.is_empty() {
        return vec![];
    }
    let project = if scoped {
        project_from_question(question)
    } else {
        None
    };
    let allowed = project.map(|name| {
        corpus
            .sources
            .iter()
            .filter(|s| s.scope == "public" && s.project == name)
            .map(|s| s.id.clone())
            .collect::<Vec<_>>()
    });
    let filters = RetrievalFilters {
        source_ids: allowed.unwrap_or_default(),
        ..RetrievalFilters::default()
    };
    index
        .retrieve(&RetrievalRequest {
            queries: terms,
            filters,
            limit: MAX_HITS,
        })
        .expect("actual FTS5 engine search")
        .hits
        .into_iter()
        .map(|hit| hit.source_id)
        .collect()
}

// Fixture-only graph projection uses the REAL graph write/read API but no live privileges.
// Every project->source edge is deterministic synthetic metadata, not a learned relationship.
fn fixture_graph(index: &IndexStore, corpus: &Corpus) {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    for project in ["aster", "beryl"] {
        let project_ref = GraphNodeRef::new(GraphKind::Project, project);
        nodes.push(GraphNode {
            identity: project_ref.clone(),
            label: project.to_owned(),
            source_ref: format!("synthetic:project:{project}"),
        });
        for source in corpus.sources.iter().filter(|s| s.scope == "public" && s.project == project) {
            let kind = if source.kind == "document" {
                GraphKind::File
            } else {
                GraphKind::Conversation
            };
            let source_ref = GraphNodeRef::new(kind, &source.id);
            nodes.push(GraphNode {
                identity: source_ref.clone(),
                label: source.id.clone(),
                source_ref: source.id.clone(),
            });
            edges.push(GraphEdge {
                from: project_ref.clone(),
                to: source_ref,
                relation: GraphRelation::Contains,
                origin: GraphOrigin::Deterministic,
                evidence: GraphEvidence {
                    source_ref: source.id.clone(),
                    source_revision: source.revision.clone(),
                    locator: format!("synthetic:{}", source.id),
                },
            });
        }
    }
    index.replace_graph_projection("synthetic:mp10b", &GraphProjection { nodes, edges })
        .expect("persist isolated synthetic graph projection");
}

// Actual graph query with a benign test-only callback. Its callback is NOT a
// real unlock, root approval or scoped user authorization in production.
fn graph_project(index: &IndexStore, corpus: &Corpus, question: &str) -> Vec<String> {
    let Some(project) = project_from_question(question) else {
        return vec![];
    };
    let result = index.graph_neighbors_with_check(
        &GraphNeighborhoodRequest {
            scope: "synthetic:mp10b".to_owned(),
            seeds: vec![GraphNodeRef::new(GraphKind::Project, project)],
            direction: GraphDirection::Outbound,
            max_hops: 1,
            max_nodes: 100,
            max_edges: 100,
        },
        || Ok(()),
    ).expect("actual graph API retrieval over test projection");
    assert!(!result.truncated);
    let by_id = corpus.sources.iter().map(|s| (s.id.as_str(), s)).collect::<HashMap<_, _>>();
    let mut ids = result.nodes.into_iter()
        .filter(|node| node.depth == 1 && node.kind != GraphKind::Project)
        .map(|node| node.source_ref).collect::<Vec<_>>();
    assert!(ids.iter().all(|id| by_id.get(id.as_str()).is_some_and(|s| s.scope == "public")));
    // Graph API returns a deterministic node set; recency is a documented fixture
    // rerank, not a native graph relevance score or authority.
    ids.sort_by(|a, b| by_id[b.as_str()].observed_at.cmp(&by_id[a.as_str()].observed_at)
        .then_with(|| a.cmp(b)));
    ids.truncate(MAX_HITS);
    ids
}

fn fuse(lexical: &[String], graph: &[String]) -> Vec<String> {
    let mut score = BTreeMap::<String, f64>::new();
    for (rank, id) in lexical.iter().enumerate() {
        *score.entry(id.clone()).or_default() += 1.0 / (60 + rank + 1) as f64;
    }
    for (rank, id) in graph.iter().enumerate() {
        *score.entry(id.clone()).or_default() += 1.0 / (60 + rank + 1) as f64;
    }
    let mut ranked = score.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked.into_iter().take(MAX_HITS).map(|v| v.0).collect()
}

fn run_strategy(
    index: &IndexStore,
    corpus: &Corpus,
    gold: &Gold,
    strategy: &str,
) -> serde_json::Value {
    let by_id = corpus
        .sources
        .iter()
        .map(|s| (s.id.as_str(), s))
        .collect::<HashMap<_, _>>();
    let mut predictions = vec![];
    for question in &gold.cases {
        let start = Instant::now();
        let ids = match strategy {
            "lexical_fts5" => fts(index, corpus, &question.question, false),
            "structured_fts5" => fts(index, corpus, &question.question, true),
            "project_graph_api" => graph_project(index, corpus, &question.question),
            "hybrid_fts5_graph_api" => fuse(
                &fts(index, corpus, &question.question, true),
                &graph_project(index, corpus, &question.question),
            ),
            _ => panic!("unknown strategy"),
        };
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        // Score ALL candidate IDs for privacy in MP-10A, not just top-5.
        assert!(ids
            .iter()
            .all(|id| by_id.get(id.as_str()).is_some_and(|s| s.scope == "public")));
        assert_eq!(ids.iter().collect::<HashSet<_>>().len(), ids.len());
        let context_estimated_tokens: usize = ids
            .iter()
            .take(5)
            .map(|id| by_id[id.as_str()].text.len().div_ceil(4))
            .sum();
        predictions.push(json!({
            "id": question.id, "hits": ids, "claims": [],
            "abstained": false, // retrieval-only: no generated answer or oracle abstention
            "latency_ms": elapsed, "context_estimated_tokens": context_estimated_tokens
        }));
    }
    json!({"schema":"mind-place-continuity-predictions/v1","dataset":corpus.dataset,"cases":predictions})
}

#[test]
fn isolated_product_fts5_and_labeled_proxies_produce_bounded_valid_predictions() {
    let corpus: Corpus = serde_json::from_str(CORPUS).unwrap();
    let gold: Gold = serde_json::from_str(GOLD).unwrap();
    assert_eq!(corpus.dataset, gold.dataset);
    assert_eq!(corpus.sources.len(), 24);
    assert_eq!(gold.cases.len(), 18);
    let dir = tempfile::tempdir().unwrap();
    let index = IndexStore::open(dir.path().join("index.sqlite3")).unwrap();
    fixture_index(&index, &corpus);
    fixture_graph(&index, &corpus);
    for strategy in [
        "lexical_fts5",
        "structured_fts5",
        "project_graph_api",
        "hybrid_fts5_graph_api",
    ] {
        let report = run_strategy(&index, &corpus, &gold, strategy);
        assert_eq!(report["cases"].as_array().unwrap().len(), gold.cases.len());
        // This line is consumed by the explicit Node scorer runner. No oracle labels
        // or frozen fixture content are modified, and no benchmark pass score is assumed.
        println!("MP10B_PREDICTIONS|{}|{}", strategy, report);
    }
}

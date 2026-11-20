//! MP-9C Slice 1 review P1 regressions: hidden (redacted) sources must never
//! shape graph seeds/traversal, search shortlists, ranking, scores, counts or
//! aggregates through a projection that predates the redaction.
use mind_place_archive::{ArchiveStore, Capture, Message};
use mind_place_index::{
    GraphDirection, GraphEdge, GraphEvidence, GraphKind, GraphNeighborhood,
    GraphNeighborhoodRequest, GraphNode, GraphNodeRef, GraphOrigin, GraphProjection, GraphRelation,
    IndexError, IndexStore, QueryTerm, RetrievalRequest, SourceVisibility,
};
use mind_place_state::StateStore;

/// Enumerable tombstone set (the authority-backed shape).
struct Listed(Vec<String>);
impl SourceVisibility for Listed {
    fn is_visible(&mut self, id: &str) -> mind_place_index::Result<bool> {
        Ok(!self.0.iter().any(|hidden| hidden == id))
    }
    fn hidden_sources(&mut self) -> mind_place_index::Result<Option<Vec<String>>> {
        Ok(Some(self.0.clone()))
    }
}

const SCOPE: &str = "archive:visibility-fixture";
const VISIBLE: &str = "src:v1:visible";
const HIDDEN: &str = "src:v1:hidden";

fn node(kind: GraphKind, key: &str, source_ref: &str) -> GraphNode {
    GraphNode {
        identity: GraphNodeRef::new(kind, key),
        label: key.into(),
        source_ref: source_ref.into(),
    }
}

fn edge(from: &GraphNode, to: &GraphNode, relation: GraphRelation, source: &str) -> GraphEdge {
    GraphEdge {
        from: from.identity.clone(),
        to: to.identity.clone(),
        relation,
        origin: GraphOrigin::Deterministic,
        evidence: GraphEvidence {
            source_ref: source.into(),
            source_revision: "rev".into(),
            locator: format!("archive:{}->{}", from.identity.key, to.identity.key),
        },
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    index: IndexStore,
    /// Import with one visible and several hidden sources.
    mixed: GraphNode,
    /// Import whose only source is hidden.
    all_hidden: GraphNode,
    /// Import that no edge names at all (no canonical custody).
    orphan: GraphNode,
    visible_conversation: GraphNode,
    hidden_conversation: GraphNode,
}

fn add_source(
    nodes: &mut Vec<GraphNode>,
    edges: &mut Vec<GraphEdge>,
    name: &str,
    source: &str,
    import: &GraphNode,
) -> GraphNode {
    let conversation = node(GraphKind::Conversation, &format!("c-{name}"), source);
    let revision = node(GraphKind::Revision, &format!("r-{name}"), source);
    edges.push(edge(
        &conversation,
        &revision,
        GraphRelation::Version,
        source,
    ));
    edges.push(edge(&revision, import, GraphRelation::ImportedFrom, source));
    nodes.push(conversation.clone());
    nodes.push(revision);
    conversation
}

fn fixture(hidden_count: usize) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let index = IndexStore::open(dir.path().join("index.sqlite3")).unwrap();
    // Non-canonical archive IDs, exactly as the archive graph producer emits.
    let mixed = node(GraphKind::Import, "archive-mixed", "archive-mixed");
    let all_hidden = node(GraphKind::Import, "archive-hidden", "archive-hidden");
    let orphan = node(GraphKind::Import, "archive-orphan", "archive-orphan");
    let mut nodes = vec![mixed.clone(), all_hidden.clone(), orphan.clone()];
    let mut edges = Vec::new();

    let visible_conversation = add_source(&mut nodes, &mut edges, "visible", VISIBLE, &mixed);
    let hidden_conversation = add_source(&mut nodes, &mut edges, "hidden0", HIDDEN, &mixed);
    for number in 1..hidden_count {
        let source = format!("src:v1:hidden{number}");
        add_source(
            &mut nodes,
            &mut edges,
            &format!("hidden{number}"),
            &source,
            &mixed,
        );
    }
    add_source(&mut nodes, &mut edges, "only-hidden", HIDDEN, &all_hidden);
    index
        .replace_graph_projection(SCOPE, &GraphProjection { nodes, edges })
        .unwrap();
    Fixture {
        _dir: dir,
        index,
        mixed,
        all_hidden,
        orphan,
        visible_conversation,
        hidden_conversation,
    }
}

fn hidden_ids(count: usize) -> Vec<String> {
    let mut ids = vec![HIDDEN.to_owned()];
    ids.extend((1..count).map(|number| format!("src:v1:hidden{number}")));
    ids
}

fn request(
    seed: &GraphNode,
    hops: usize,
    max_nodes: usize,
    max_edges: usize,
) -> GraphNeighborhoodRequest {
    GraphNeighborhoodRequest {
        scope: SCOPE.into(),
        seeds: vec![seed.identity.clone()],
        direction: GraphDirection::Both,
        max_hops: hops,
        max_nodes,
        max_edges,
    }
}

/// Run one query through BOTH visibility shapes and require identical output.
fn visible_query(
    f: &Fixture,
    seed: &GraphNode,
    hops: usize,
    max_nodes: usize,
    max_edges: usize,
    hidden: usize,
) -> GraphNeighborhood {
    let query = request(seed, hops, max_nodes, max_edges);
    let ids = hidden_ids(hidden);
    let listed = f
        .index
        .graph_neighbors_visible_with_check(&query, &mut Listed(ids.clone()), || Ok(()))
        .unwrap();
    let predicate = f
        .index
        .graph_neighbors_visible_with_check(
            &query,
            &mut |id: &str| Ok(!ids.iter().any(|hidden| hidden == id)),
            || Ok(()),
        )
        .unwrap();
    assert_eq!(
        listed, predicate,
        "enumerated and predicate gates must agree"
    );
    listed
}

fn no_hidden_trace(graph: &GraphNeighborhood) {
    let encoded = format!("{graph:?}");
    assert!(
        !encoded.contains("hidden"),
        "hidden evidence leaked: {encoded}"
    );
}

#[test]
fn depth_zero_seed_of_an_all_hidden_or_edgeless_import_is_not_returned() {
    let f = fixture(1);
    // The unfiltered producer view still resolves it (the stale-projection hazard).
    let raw = f
        .index
        .graph_neighbors_with_check(&request(&f.all_hidden, 0, 10, 10), || Ok(()))
        .unwrap();
    assert_eq!(raw.nodes.len(), 1);

    for seed in [&f.all_hidden, &f.orphan, &f.hidden_conversation] {
        let graph = visible_query(&f, seed, 0, 10, 10, 1);
        assert!(graph.nodes.is_empty(), "{seed:?} -> {graph:?}");
        assert!(graph.edges.is_empty());
    }
    // A visible conversation and a mixed import both resolve at depth zero.
    assert_eq!(
        visible_query(&f, &f.visible_conversation, 0, 10, 10, 1)
            .nodes
            .len(),
        1
    );
    assert_eq!(visible_query(&f, &f.mixed, 0, 10, 10, 1).nodes.len(), 1);
}

#[test]
fn mixed_import_exposes_only_visible_sources_during_traversal() {
    let f = fixture(1);
    let graph = visible_query(&f, &f.mixed, 3, 100, 100, 1);
    no_hidden_trace(&graph);
    let labels: Vec<_> = graph.nodes.iter().map(|n| n.label.as_str()).collect();
    assert!(labels.contains(&"archive-mixed"));
    assert!(labels.contains(&"c-visible") && labels.contains(&"r-visible"));
    assert_eq!(graph.nodes.len(), 3, "{labels:?}");
    assert_eq!(graph.edges.len(), 2);

    // Seeding from the visible side never walks into hidden siblings via the import.
    let from_visible = visible_query(&f, &f.visible_conversation, 4, 100, 100, 1);
    no_hidden_trace(&from_visible);
    assert_eq!(from_visible.nodes.len(), 3);
}

#[test]
fn all_hidden_import_is_unreachable_from_any_direction() {
    let f = fixture(1);
    let graph = visible_query(&f, &f.mixed, 4, 100, 100, 1);
    assert!(graph.nodes.iter().all(|n| n.label != "archive-hidden"));
    let from_hidden = visible_query(&f, &f.hidden_conversation, 4, 100, 100, 1);
    assert!(from_hidden.nodes.is_empty() && from_hidden.edges.is_empty());
}

#[test]
fn hidden_rows_never_consume_budget_or_force_truncation() {
    // Seven hidden sources share the import with one visible source. Whatever
    // the edge ordering, a one-edge / two-node budget must return the visible
    // revision edge, not be exhausted (or flagged truncated) by hidden rows.
    let f = fixture(7);
    let graph = visible_query(&f, &f.mixed, 1, 2, 1, 7);
    no_hidden_trace(&graph);
    assert_eq!(graph.edges.len(), 1, "{graph:?}");
    assert_eq!(graph.nodes.len(), 2, "{graph:?}");
    assert!(!graph.truncated, "hidden rows must not signal truncation");
}

// ---------- retrieval / search / stats ----------

fn capture(id: &str, title: &str, text: &str) -> Capture {
    Capture {
        schema_version: 1,
        provider: "chatgpt".into(),
        conversation_id: id.into(),
        source_url: format!("https://chatgpt.com/c/{id}"),
        title: title.into(),
        conversation_created_at: None,
        conversation_updated_at: None,
        captured_at: "2026-10-17T10:00:00Z".into(),
        capture_method: "manual-dom".into(),
        completeness: "partial".into(),
        warnings: vec![],
        messages: vec![Message {
            id: "human-1".into(),
            role: "user".into(),
            text: text.into(),
            format: "plain".into(),
            authored_at: None,
            model: None,
            attachments: vec![],
        }],
    }
}

fn zebra(limit: usize) -> RetrievalRequest {
    RetrievalRequest {
        queries: vec![QueryTerm::Text("zebracorn".into())],
        limit,
        ..Default::default()
    }
}

/// Visible `A`, plus a hidden `B` that strongly outranks it for the query.
fn search_fixture() -> (tempfile::TempDir, IndexStore, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let archive = ArchiveStore::open(dir.path().join("archive-v1")).unwrap();
    let state = StateStore::open(dir.path().join("state.sqlite3")).unwrap();
    let index = IndexStore::open(dir.path().join("index.sqlite3")).unwrap();
    archive
        .save(capture(
            "11111111-2222-4333-8444-555555555555",
            "Visible title",
            "zebracorn kept note with a few more filler words around it",
        ))
        .unwrap();
    archive
        .save(capture(
            "22222222-3333-4444-8555-666666666666",
            "Hidden secret title",
            "zebracorn zebracorn zebracorn zebracorn zebracorn",
        ))
        .unwrap();
    index.sync(&archive, &state).unwrap();
    let all = index.retrieve(&zebra(20)).unwrap();
    assert_eq!(all.hits.len(), 2);
    let hidden = all
        .hits
        .iter()
        .find(|hit| hit.title.contains("Hidden"))
        .unwrap()
        .source_id
        .clone();
    let visible = all
        .hits
        .iter()
        .find(|hit| hit.title.contains("Visible"))
        .unwrap()
        .source_id
        .clone();
    (dir, index, visible, hidden)
}

#[test]
fn stale_hidden_row_cannot_displace_reorder_or_rescore_visible_hits() {
    let (_dir, index, visible, hidden) = search_fixture();
    let unfiltered = index.retrieve(&zebra(1)).unwrap();
    // The hazard: the hidden row owns the top LIMIT-1 slot of the shortlist.
    assert_eq!(unfiltered.hits[0].source_id, hidden);

    // Enumerated authority and bare predicate both fail closed: never an Ok
    // page that is empty/short/rescored because of a hidden row.
    let listed = index.retrieve_filtered(&zebra(1), &mut Listed(vec![hidden.clone()]));
    assert!(
        matches!(listed, Err(IndexError::StaleGeneration)),
        "{listed:?}"
    );
    let predicate = index.retrieve_filtered(&zebra(1), &mut |id: &str| Ok(id != hidden));
    assert!(matches!(predicate, Err(IndexError::StaleGeneration)));
    let search = index.search_visible("zebracorn", false, 1, &mut Listed(vec![hidden.clone()]));
    assert!(matches!(search, Err(IndexError::StaleGeneration)));

    // Once nothing hidden remains indexed the result is bit-identical to the
    // plain retrieval (same ranking AND scores), so nothing was approximated.
    let other = "src:v1:not-indexed".to_owned();
    let clean = index
        .retrieve_filtered(&zebra(20), &mut Listed(vec![other.clone()]))
        .unwrap();
    assert_eq!(clean, index.retrieve(&zebra(20)).unwrap());
    let plain = index
        .search_visible("zebracorn", false, 20, &mut Listed(vec![other]))
        .unwrap();
    assert_eq!(plain.len(), 2);
    assert!(plain.iter().any(|hit| hit.source_id == visible));
}

#[test]
fn aggregate_counts_exclude_hidden_sources_exactly() {
    let (_dir, index, _visible, hidden) = search_fixture();
    let raw = index.stats().unwrap();
    let counted = index.stats_visible(&mut Listed(vec![hidden])).unwrap();
    assert_eq!(raw.sources, 2);
    assert_eq!(counted.sources, 1);
    assert_eq!(counted.revisions, raw.revisions - 1);
    assert!(counted.chunks < raw.chunks && counted.chunks > 0);
    assert_eq!(counted.generation, raw.generation);
    // Nothing hidden indexed: identical to the raw aggregate.
    let untouched = index
        .stats_visible(&mut Listed(vec!["src:v1:absent".into()]))
        .unwrap();
    assert_eq!(untouched, raw);
}

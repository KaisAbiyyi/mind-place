use mind_place_archive::{ArchiveStore, Capture, Message};
use mind_place_index::{
    GraphEdge, GraphEvidence, GraphKind, GraphNode, GraphNodeRef, GraphProjection, GraphRelation,
    IndexError, IndexStore,
};
use mind_place_state::StateStore;
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::fs;

fn node(kind: GraphKind, key: &str) -> GraphNode {
    GraphNode {
        identity: GraphNodeRef::new(kind, key),
        label: key.to_string(),
        source_ref: "source:fixture".into(),
    }
}

fn evidence() -> GraphEvidence {
    GraphEvidence {
        source_ref: "source:fixture".into(),
        source_revision: "revision:v1".into(),
        locator: "line:10-20".into(),
    }
}

fn relation(from: &GraphNode, to: &GraphNode, kind: GraphRelation) -> GraphEdge {
    GraphEdge {
        from: from.identity.clone(),
        to: to.identity.clone(),
        relation: kind,
        origin: mind_place_index::GraphOrigin::Deterministic,
        evidence: evidence(),
    }
}

fn projection() -> GraphProjection {
    let project = node(GraphKind::Project, "project:alpha");
    let topic = node(GraphKind::Topic, "topic:rust");
    let file = node(GraphKind::File, "file:src/lib.rs");
    let memory = node(GraphKind::Memory, "memory:1");
    let conversation = node(GraphKind::Conversation, "conversation:1");
    let entity = node(GraphKind::Entity, "entity:Alice");
    let symbol = node(GraphKind::Symbol, "symbol:fn::main");
    let import = node(GraphKind::Import, "import:1");
    let revision = node(GraphKind::Revision, "revision:1");
    GraphProjection {
        nodes: vec![
            project.clone(),
            topic.clone(),
            file.clone(),
            memory.clone(),
            conversation.clone(),
            entity.clone(),
            symbol.clone(),
            import.clone(),
            revision.clone(),
        ],
        edges: vec![
            relation(&project, &topic, GraphRelation::Contains),
            relation(&topic, &file, GraphRelation::Contains),
            relation(&file, &memory, GraphRelation::References),
            relation(&entity, &conversation, GraphRelation::References),
            relation(&conversation, &revision, GraphRelation::Version),
            relation(&revision, &import, GraphRelation::ImportedFrom),
            relation(&file, &symbol, GraphRelation::DefinesSymbol),
        ],
    }
}

fn edge_ids(index: &IndexStore, scope: &str) -> Vec<String> {
    let db = Connection::open(index.path()).unwrap();
    let mut q = db
        .prepare("SELECT id FROM graph_edges WHERE scope=?1 ORDER BY id")
        .unwrap();
    q.query_map([scope], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

#[test]
fn supports_all_semantic_kinds_and_evidence_relations_deterministically() {
    let dir = tempfile::tempdir().unwrap();
    let index = IndexStore::open(dir.path().join("index.sqlite3")).unwrap();
    let input = projection();
    assert_eq!(
        index
            .replace_graph_projection("fixture", &input)
            .unwrap()
            .nodes,
        9
    );
    assert_eq!(index.graph_scope_stats("fixture").unwrap().edges, 7);
    let stable = edge_ids(&index, "fixture");
    let db = Connection::open(index.path()).unwrap();
    let (kind, origin, locator): (String, String, String) = db
        .query_row(
            "SELECT relation,origin,locator FROM graph_edges
             WHERE relation='defines_symbol' LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        (kind.as_str(), origin.as_str(), locator.as_str()),
        ("defines_symbol", "deterministic", "line:10-20")
    );
    drop(db);

    let mut shuffled = input.clone();
    shuffled.nodes.reverse();
    shuffled.edges.reverse();
    shuffled.edges.push(shuffled.edges[0].clone());
    index
        .replace_graph_projection("fixture", &shuffled)
        .unwrap();
    assert_eq!(edge_ids(&index, "fixture"), stable);
    let reopened = IndexStore::open(index.path()).unwrap();
    assert_eq!(reopened.graph_scope_stats("fixture").unwrap().edges, 7);
}

#[test]
fn scopes_are_independent_and_empty_replacement_prunes_orphans() {
    let dir = tempfile::tempdir().unwrap();
    let index = IndexStore::open(dir.path().join("index.sqlite3")).unwrap();
    let shared = node(GraphKind::Project, "project:shared");
    let solo = node(GraphKind::Topic, "topic:solo");
    index
        .replace_graph_projection(
            "one",
            &GraphProjection {
                nodes: vec![shared.clone(), solo.clone()],
                edges: vec![relation(&shared, &solo, GraphRelation::Contains)],
            },
        )
        .unwrap();
    index
        .replace_graph_projection(
            "two",
            &GraphProjection {
                nodes: vec![shared.clone(), solo.clone()],
                edges: vec![relation(&shared, &solo, GraphRelation::Contains)],
            },
        )
        .unwrap();
    // Identical structural evidence in different producer scopes is allowed.
    assert_eq!(index.graph_scope_stats("two").unwrap().edges, 1);
    let mut poisoned = shared.clone();
    poisoned.label = "another scope's forged label".into();
    assert!(matches!(
        index.replace_graph_projection(
            "one",
            &GraphProjection {
                nodes: vec![poisoned],
                edges: vec![],
            },
        ),
        Err(IndexError::InvalidQuery)
    ));
    index
        .replace_graph_projection("one", &GraphProjection::default())
        .unwrap();
    assert_eq!(index.graph_scope_stats("one").unwrap().nodes, 0);
    assert_eq!(index.graph_scope_stats("two").unwrap().nodes, 2);
    assert_eq!(index.graph_scope_stats("two").unwrap().edges, 1);
    index
        .replace_graph_projection("two", &GraphProjection::default())
        .unwrap();
    let db = Connection::open(index.path()).unwrap();
    let count: i64 = db
        .query_row("SELECT COUNT(*) FROM graph_nodes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn rejects_unsupported_or_unproven_edges_and_preserves_old_projection() {
    let dir = tempfile::tempdir().unwrap();
    let index = IndexStore::open(dir.path().join("index.sqlite3")).unwrap();
    let base = projection();
    index.replace_graph_projection("fixture", &base).unwrap();
    let before = edge_ids(&index, "fixture");

    let mut unknown = base.clone();
    unknown.edges[0].to = GraphNodeRef::new(GraphKind::File, "unknown");
    assert!(matches!(
        index.replace_graph_projection("fixture", &unknown),
        Err(IndexError::InvalidQuery)
    ));
    let mut fake = base.clone();
    fake.edges[0].evidence.locator.clear();
    assert!(matches!(
        index.replace_graph_projection("fixture", &fake),
        Err(IndexError::InvalidQuery)
    ));
    let mut wrong_direction = base.clone();
    wrong_direction.edges[0].relation = GraphRelation::DefinesSymbol;
    assert!(matches!(
        index.replace_graph_projection("fixture", &wrong_direction),
        Err(IndexError::InvalidQuery)
    ));
    assert!(matches!(
        index.replace_graph_projection("archive:chatgpt", &base),
        Err(IndexError::InvalidQuery)
    ));
    assert_eq!(edge_ids(&index, "fixture"), before);
    let mut callbacks = 0;
    let denied =
        index.replace_graph_projection_with_check("fixture", &GraphProjection::default(), || {
            callbacks += 1;
            if callbacks == 2 {
                Err(IndexError::Revoked)
            } else {
                Ok(())
            }
        });
    assert!(matches!(denied, Err(IndexError::Revoked)));
    assert_eq!(edge_ids(&index, "fixture"), before);
}

#[test]
fn legacy_index_migrates_without_discarding_fts_or_modifying_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.sqlite3");
    let state = StateStore::open(dir.path().join("state.sqlite3")).unwrap();
    let db = Connection::open(&path).unwrap();
    let old = include_str!("../src/schema_v1.sql");
    db.execute_batch(old).unwrap();
    db.pragma_update(None, "application_id", 0x4D504958i64)
        .unwrap();
    db.pragma_update(None, "user_version", 1i64).unwrap();
    db.execute(
        "INSERT INTO index_meta(key,value) VALUES('schema_hash',?1)",
        [format!("{:x}", Sha256::digest(old.as_bytes()))],
    )
    .unwrap();
    db.execute(
        "INSERT INTO revisions (source_id,archive_id,snapshot_id,provider,
          conversation_id,title,source_url,captured_at,completeness,is_current,seen_generation)
         VALUES('s','a','v','chatgpt','c','Legacy','url','today','partial',1,1)",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO chunks(source_id,snapshot_id,message_id,part,kind,role,body)
         VALUES('s','v','m',0,'message','user','legacygraphneedle')",
        [],
    )
    .unwrap();
    drop(db);
    let before_state_bytes = fs::read(state.path()).unwrap();
    let index = IndexStore::open(&path).unwrap();
    assert_eq!(
        index.search("legacygraphneedle", false, 10).unwrap().len(),
        1
    );
    assert_eq!(index.graph_scope_stats("archive:chatgpt").unwrap().nodes, 0);
    let db = Connection::open(&path).unwrap();
    let ver: i64 = db
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(ver, 4);
    assert_eq!(fs::read(state.path()).unwrap(), before_state_bytes);
    drop(db);
    assert!(IndexStore::open(&path).is_ok());
}

#[test]
fn stale_schema_hash_is_rejected_without_automatic_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.sqlite3");
    IndexStore::open(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute(
        "UPDATE index_meta SET value='bad' WHERE key='schema_hash'",
        [],
    )
    .unwrap();
    drop(db);
    assert!(matches!(
        IndexStore::open(&path),
        Err(IndexError::InvalidIndex)
    ));
}

fn capture(text: &str) -> Capture {
    Capture {
        schema_version: 1,
        provider: "chatgpt".into(),
        conversation_id: "11111111-2222-4333-8444-555555555555".into(),
        source_url: "https://chatgpt.com/c/11111111-2222-4333-8444-555555555555".into(),
        title: "Graph archive evidence".into(),
        conversation_created_at: None,
        conversation_updated_at: None,
        captured_at: "2026-10-23T11:00:00Z".into(),
        capture_method: "manual-dom".into(),
        completeness: "partial".into(),
        warnings: vec![],
        messages: vec![Message {
            id: "message-one".into(),
            role: "user".into(),
            text: text.into(),
            format: "plain".into(),
            authored_at: None,
            model: None,
            attachments: vec![],
        }],
    }
}

#[test]
fn archive_edges_are_source_proven_and_rebuildable_without_raw_changes() {
    let dir = tempfile::tempdir().unwrap();
    let archive = ArchiveStore::open(dir.path().join("archive-v1")).unwrap();
    let state = StateStore::open(dir.path().join("state.sqlite3")).unwrap();
    let index = IndexStore::open(dir.path().join("index.sqlite3")).unwrap();
    let receipt = archive.save(capture("graphbeacon first")).unwrap();
    let raw_path = archive
        .root()
        .join("conversations")
        .join(&receipt.id)
        .join("captures")
        .join(format!("{}.jsonl", receipt.snapshot_id));
    let raw_before = fs::read(&raw_path).unwrap();
    index.sync(&archive, &state).unwrap();
    assert_eq!(index.graph_scope_stats("archive:chatgpt").unwrap().nodes, 3);
    assert_eq!(index.graph_scope_stats("archive:chatgpt").unwrap().edges, 2);
    let stable = edge_ids(&index, "archive:chatgpt");
    index.rebuild(&archive, &state).unwrap();
    assert_eq!(edge_ids(&index, "archive:chatgpt"), stable);
    assert_eq!(fs::read(&raw_path).unwrap(), raw_before);
    let db = Connection::open(index.path()).unwrap();
    let (source, revision, locator): (String, String, String) = db
        .query_row(
            "SELECT source_ref,source_revision,locator FROM graph_edges
         WHERE relation='imported_from'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert!(!source.is_empty());
    assert_eq!(revision, receipt.snapshot_id);
    assert!(locator.contains(&receipt.id));
}

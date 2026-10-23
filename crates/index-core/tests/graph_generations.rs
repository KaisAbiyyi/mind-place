use mind_place_index::{
    GraphEdge, GraphEvidence, GraphInputHashes, GraphKind, GraphNode, GraphNodeRef,
    GraphProjection, GraphRelation, IndexError, IndexStore,
};
use rusqlite::Connection;
use sha2::{Digest, Sha256};

fn inputs() -> GraphInputHashes {
    GraphInputHashes {
        files: "a".repeat(64),
        content: "b".repeat(64),
        structure: "c".repeat(64),
    }
}
fn graph() -> GraphProjection {
    let a = GraphNode {
        identity: GraphNodeRef::new(GraphKind::Project, "project:a"),
        label: "Project A".into(),
        source_ref: "file:verified".into(),
    };
    let b = GraphNode {
        identity: GraphNodeRef::new(GraphKind::File, "file:one"),
        label: "File One".into(),
        source_ref: "file:verified".into(),
    };
    GraphProjection {
        nodes: vec![a.clone(), b.clone()],
        edges: vec![GraphEdge {
            from: a.identity,
            to: b.identity,
            relation: GraphRelation::Contains,
            evidence: GraphEvidence {
                source_ref: "file:verified".into(),
                source_revision: "rev:1".into(),
                locator: "line:1".into(),
            },
        }],
    }
}

#[test]
fn publishes_once_skips_unchanged_and_hash_invalidation_increments_generation() {
    let dir = tempfile::tempdir().unwrap();
    let index = IndexStore::open(dir.path().join("index.sqlite3")).unwrap();
    let first = index
        .activate_graph_generation("derived:files", &inputs(), &graph(), None)
        .unwrap();
    assert!(first.changed);
    assert_eq!(first.generation.generation, 1);
    assert_eq!(first.generation.stats.nodes, 2);
    assert_eq!(first.generation.stats.edges, 1);
    assert!(first.generation.active);

    let mut reordered = graph();
    reordered.nodes.reverse();
    reordered.edges.push(reordered.edges[0].clone());
    let repeat = index
        .activate_graph_generation("derived:files", &inputs(), &reordered, Some(1))
        .unwrap();
    assert!(!repeat.changed);
    assert_eq!(repeat.generation, first.generation);
    assert_eq!(index.graph_scope_stats("derived:files").unwrap().edges, 1);

    let mut changed_inputs = inputs();
    changed_inputs.structure = "d".repeat(64);
    let revised = index
        .activate_graph_generation("derived:files", &changed_inputs, &reordered, Some(1))
        .unwrap();
    assert!(revised.changed);
    assert_eq!(revised.generation.generation, 2);
    assert_eq!(revised.generation.input.structure, "d".repeat(64));

    let mut changed_projection = graph();
    changed_projection.nodes[0].label = "Project Renamed".into();
    let next = index
        .activate_graph_generation("derived:files", &changed_inputs, &changed_projection, Some(2))
        .unwrap();
    assert_ne!(next.generation.projection_hash, revised.generation.projection_hash);
    assert_eq!(next.generation.generation, 3);
    assert!(IndexStore::open(index.path()).unwrap().graph_generation("derived:files").unwrap().unwrap().active);
}

#[test]
fn stale_generation_does_not_overwrite_newer_graph_and_revocation_rolls_back() {
    let dir = tempfile::tempdir().unwrap();
    let index = IndexStore::open(dir.path().join("index.sqlite3")).unwrap();
    index.activate_graph_generation("scope", &inputs(), &graph(), None).unwrap();
    let previous = index.graph_generation("scope").unwrap().unwrap();
    let mut updated = graph();
    updated.edges.clear();
    assert!(matches!(
        index.activate_graph_generation("scope", &inputs(), &updated, None),
        Err(IndexError::StaleGeneration)
    ));
    assert_eq!(index.graph_generation("scope").unwrap().unwrap(), previous);

    let mut checks = 0;
    let revoked = index.activate_graph_generation_with_check(
        "scope",
        &inputs(),
        &updated,
        Some(1),
        || {
            checks += 1;
            if checks == 2 { Err(IndexError::Revoked) } else { Ok(()) }
        },
    );
    assert!(matches!(revoked, Err(IndexError::Revoked)));
    assert_eq!(index.graph_generation("scope").unwrap().unwrap(), previous);
    assert_eq!(index.graph_scope_stats("scope").unwrap().edges, 1);

    let mut checks = 0;
    let denied_invalidation = index.invalidate_graph_generation_with_check("scope", Some(1), || {
        checks += 1;
        if checks == 2 { Err(IndexError::Revoked) } else { Ok(()) }
    });
    assert!(matches!(denied_invalidation, Err(IndexError::Revoked)));
    assert_eq!(index.graph_scope_stats("scope").unwrap().edges, 1);
    assert_eq!(index.graph_generation("scope").unwrap().unwrap(), previous);
}

#[test]
fn invalidation_creates_tombstone_and_fences_stale_builder() {
    let dir = tempfile::tempdir().unwrap();
    let index = IndexStore::open(dir.path().join("index.sqlite3")).unwrap();
    let tombstone = index.invalidate_graph_generation("scope", None).unwrap();
    assert_eq!(tombstone.generation, 1);
    assert!(!tombstone.active);
    assert_eq!(index.graph_scope_stats("scope").unwrap().nodes, 0);
    assert!(matches!(
        index.activate_graph_generation("scope", &inputs(), &graph(), None),
        Err(IndexError::StaleGeneration)
    ));
    index.activate_graph_generation("scope", &inputs(), &graph(), Some(1)).unwrap();
    let tombstone = index.invalidate_graph_generation("scope", Some(2)).unwrap();
    assert_eq!(tombstone.generation, 3);
    assert!(!tombstone.active);
    assert_eq!(index.graph_scope_stats("scope").unwrap().nodes, 0);
    assert!(matches!(
        index.activate_graph_generation("scope", &inputs(), &graph(), Some(2)),
        Err(IndexError::StaleGeneration)
    ));
    let rebuilt = index.activate_graph_generation("scope", &inputs(), &graph(), Some(3)).unwrap();
    assert_eq!(rebuilt.generation.generation, 4);
    assert!(rebuilt.generation.active);
}

#[test]
fn legacy_unfenced_writer_invalidates_manifest_and_illegal_hashes_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let index = IndexStore::open(dir.path().join("index.sqlite3")).unwrap();
    index.activate_graph_generation("scope", &inputs(), &graph(), None).unwrap();
    index.replace_graph_projection("scope", &graph()).unwrap();
    let generation = index.graph_generation("scope").unwrap().unwrap();
    assert_eq!(generation.generation, 2);
    assert!(!generation.active);
    assert!(matches!(
        index.activate_graph_generation("scope", &inputs(), &graph(), Some(1)),
        Err(IndexError::StaleGeneration)
    ));
    assert!(matches!(
        index.activate_graph_generation("archive:chatgpt", &inputs(), &graph(), None),
        Err(IndexError::InvalidQuery)
    ));
    let mut malformed = inputs();
    malformed.files = "not-a-sha".into();
    assert!(matches!(
        index.activate_graph_generation("other", &malformed, &graph(), None),
        Err(IndexError::InvalidQuery)
    ));
    assert!(index.graph_generation("other").unwrap().is_none());
}

#[test]
fn existing_v2_index_migrates_losslessly_to_v3_with_fts_and_graph() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.sqlite3");
    let db = Connection::open(&path).unwrap();
    let v1 = include_str!("../src/schema_v1.sql");
    let v2 = include_str!("../src/schema_v2.sql");
    db.execute_batch(v1).unwrap();
    db.execute_batch(v2).unwrap();
    db.pragma_update(None, "application_id", 0x4D504958i64).unwrap();
    db.pragma_update(None, "user_version", 2i64).unwrap();
    let mut hash = Sha256::new();
    hash.update(v1.as_bytes());
    hash.update(v2.as_bytes());
    db.execute(
        "INSERT INTO index_meta(key,value) VALUES('schema_hash',?1)",
        [format!("{:x}", hash.finalize())],
    ).unwrap();
    db.execute_batch(
        "INSERT INTO revisions(source_id,archive_id,snapshot_id,provider,conversation_id,
           title,source_url,captured_at,completeness,is_current,seen_generation)
         VALUES('s','a','v','chatgpt','c','Legacy','u','today','partial',1,1);
         INSERT INTO chunks(source_id,snapshot_id,message_id,part,kind,role,body)
         VALUES('s','v','m',0,'message','user','preservedneedle');
         INSERT INTO graph_nodes(id,kind,canonical_key,label,source_ref)
         VALUES('g1','project','p','Project','source:p');
         INSERT INTO graph_scope_nodes(scope,node_id) VALUES('scope','g1');",
    ).unwrap();
    drop(db);

    let index = IndexStore::open(&path).unwrap();
    assert_eq!(index.search("preservedneedle", false, 10).unwrap().len(), 1);
    assert_eq!(index.graph_scope_stats("scope").unwrap().nodes, 1);
    assert!(index.graph_generation("scope").unwrap().is_none());
    let db = Connection::open(&path).unwrap();
    let version: i64 = db.query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap();
    assert_eq!(version, 3);
    drop(db);
    assert!(IndexStore::open(&path).is_ok());
}

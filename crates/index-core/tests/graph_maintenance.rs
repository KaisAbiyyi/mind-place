use mind_place_index::{
    GraphEdge, GraphEvidence, GraphInputHashes, GraphKind, GraphMaintenanceBatch, GraphNode,
    GraphNodeRef, GraphRelation, IndexError, IndexStore,
};
use mind_place_security::SecurityGate;
use mind_place_state::StateStore;
use std::{cell::RefCell, fs, rc::Rc, time::Duration};

fn setup() -> (tempfile::TempDir, StateStore, SecurityGate, IndexStore, String) {
    let home = tempfile::tempdir().unwrap();
    let state = StateStore::open(home.path().join("state.sqlite3")).unwrap();
    let security = SecurityGate::open(home.path().join("security.json")).unwrap();
    security.initialize(b"correct horse battery staple").unwrap();
    let root_dir = home.path().join("approved");
    fs::create_dir(&root_dir).unwrap();
    let root_id = state.register_document_root(&root_dir).unwrap().id;
    state.approve_document_root(&root_id).unwrap();
    let index = IndexStore::open(home.path().join("index.sqlite3")).unwrap();
    (home, state, security, index, root_id)
}

fn hashes() -> GraphInputHashes {
    GraphInputHashes {
        files: "a".repeat(64),
        content: "b".repeat(64),
        structure: "c".repeat(64),
    }
}

fn fixture(root: &str) -> (GraphMaintenanceBatch, GraphMaintenanceBatch) {
    let source = format!("mpdoc://root/{root}/source.md");
    let project = GraphNodeRef::new(GraphKind::Project, "project:a");
    let file = GraphNodeRef::new(GraphKind::File, "file:a");
    (
        GraphMaintenanceBatch {
            sequence: 0,
            nodes: vec![
                GraphNode { identity: project.clone(), label: "Project A".into(), source_ref: source.clone() },
                GraphNode { identity: file.clone(), label: "File A".into(), source_ref: source.clone() },
            ],
            edges: vec![],
        },
        GraphMaintenanceBatch {
            sequence: 1,
            nodes: vec![],
            edges: vec![GraphEdge {
                from: project,
                to: file,
                relation: GraphRelation::Contains,
                evidence: GraphEvidence {
                    source_ref: source,
                    source_revision: "b".repeat(64),
                    locator: "line:1".into(),
                },
            }],
        },
    )
}

#[test]
fn scoped_batches_are_sequenced_replay_safe_and_atomically_published() {
    let (_home, state, security, index, root) = setup();
    let revision = hashes().content;
    let mut job = index.begin_graph_maintenance(
        &state, &security, "derived:project-a", &root, &revision,
        hashes(), None, Duration::from_secs(60), || Ok(hashes().content)
    ).unwrap();
    assert_eq!(job.scope(), "derived:project-a");
    let (first, second) = fixture(&root);
    assert!(job.accept_batch(second.clone()).is_err()); // no skipping
    let r = job.accept_batch(first.clone()).unwrap();
    assert_eq!(r.next_sequence, 1);
    assert!(!r.replayed);
    assert!(job.accept_batch(first.clone()).unwrap().replayed);
    let mut collision = first.clone();
    collision.nodes[0].label = "changed".into();
    assert!(job.accept_batch(collision).is_err());
    assert_eq!(job.accept_batch(second).unwrap().next_sequence, 2);
    assert!(index.graph_generation("derived:project-a").unwrap().is_none());
    assert_eq!(index.graph_scope_stats("derived:project-a").unwrap().edges, 0);
    let commit = job.finish().unwrap();
    assert_eq!(commit.generation.generation, 1);
    assert_eq!(commit.generation.stats.nodes, 2);
    assert_eq!(commit.generation.stats.edges, 1);
    assert_eq!(job.finish().unwrap(), commit); // commit receipt is idempotent
    assert!(job.accept_batch(first).is_err());
    assert_eq!(index.graph_scope_stats("derived:project-a").unwrap().edges, 1);
}

#[test]
fn root_revocation_lock_and_stale_source_deny_unpublished_work() {
    let (_home, state, security, index, root) = setup();
    let source = Rc::new(RefCell::new(hashes().content));
    let producer_source = source.clone();
    let mut job = index.begin_graph_maintenance(
        &state, &security, "derived:project-a", &root, &hashes().content,
        hashes(), None, Duration::from_secs(60),
        move || Ok(producer_source.borrow().clone())
    ).unwrap();
    job.accept_batch(fixture(&root).0).unwrap();
    *source.borrow_mut() = "d".repeat(64);
    assert!(matches!(job.finish(), Err(IndexError::Revoked)));
    assert!(index.graph_generation("derived:project-a").unwrap().is_none());
    job.cancel();
    assert!(job.finish().is_err());

    let revision = hashes().content;
    let mut locked_job = index.begin_graph_maintenance(
        &state, &security, "derived:another", &root, &revision,
        hashes(), None, Duration::from_secs(60), || Ok(hashes().content)
    ).unwrap();
    security.lock();
    assert!(matches!(locked_job.accept_batch(fixture(&root).0), Err(IndexError::Revoked)));

    security.unlock(b"correct horse battery staple").unwrap();
    let revision = hashes().content;
    let mut revoked = index.begin_graph_maintenance(
        &state, &security, "derived:third", &root, &revision,
        hashes(), None, Duration::from_secs(60), || Ok(hashes().content)
    ).unwrap();
    revoked.accept_batch(fixture(&root).0).unwrap();
    state.revoke_document_root(&root).unwrap();
    assert!(revoked.finish().is_err());
    assert!(index.graph_generation("derived:third").unwrap().is_none());
}

#[test]
fn cross_root_evidence_and_incomplete_projection_fail_closed() {
    let (_home, state, security, index, root) = setup();
    let revision = hashes().content;
    let mut job = index.begin_graph_maintenance(
        &state, &security, "derived:scope", &root, &revision,
        hashes(), None, Duration::from_secs(60), || Ok(hashes().content)
    ).unwrap();
    let (mut first, _) = fixture(&root);
    first.nodes[0].source_ref = "mpdoc://root/root:v1:bogus/leak.md".into();
    assert!(job.accept_batch(first).is_err());
    let (_first, mut second) = fixture(&root);
    second.sequence = 0;
    job.accept_batch(second).unwrap();
    // The transaction is rejected without exposing a partial graph.
    assert!(job.finish().is_err());
    assert!(index.graph_generation("derived:scope").unwrap().is_none());
}

#[test]
fn stale_generation_fences_jobs_and_expiry_rejects_submissions() {
    let (_home, state, security, index, root) = setup();
    let revision = hashes().content;
    let mut job = index.begin_graph_maintenance(
        &state, &security, "derived:scope", &root, &revision,
        hashes(), None, Duration::from_secs(60), || Ok(hashes().content)
    ).unwrap();
    job.accept_batch(fixture(&root).0).unwrap();
    // A newer producer invalidates the scope before this worker can finish.
    index.invalidate_graph_generation("derived:scope", None).unwrap();
    assert!(matches!(job.finish(), Err(IndexError::StaleGeneration)));
    assert_eq!(index.graph_scope_stats("derived:scope").unwrap().nodes, 0);

    let revision = hashes().content;
    let mut expiring = index.begin_graph_maintenance(
        &state, &security, "derived:short-lived", &root, &revision,
        hashes(), None, Duration::from_millis(1), || Ok(hashes().content)
    ).unwrap();
    std::thread::sleep(Duration::from_millis(20));
    assert!(matches!(expiring.accept_batch(fixture(&root).0), Err(IndexError::Revoked)));
    assert!(index.graph_generation("derived:short-lived").unwrap().is_none());
}

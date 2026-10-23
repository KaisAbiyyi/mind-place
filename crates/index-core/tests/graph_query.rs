use mind_place_index::{
    GraphDirection, GraphEdge, GraphEvidence, GraphKind, GraphNeighborhoodRequest, GraphNode,
    GraphNodeRef, GraphProjection, GraphRelation, IndexError, IndexStore,
};

fn n(kind: GraphKind, key: &str) -> GraphNode {
    GraphNode {
        identity: GraphNodeRef::new(kind, key),
        label: key.into(),
        source_ref: format!("fixture:{key}"),
    }
}
fn edge(a: &GraphNode, b: &GraphNode) -> GraphEdge {
    GraphEdge {
        from: a.identity.clone(),
        to: b.identity.clone(),
        relation: GraphRelation::References,
        evidence: GraphEvidence {
            source_ref: "fixture:source".into(),
            source_revision: "revision:1".into(),
            locator: format!("edge:{}->{}", a.identity.key, b.identity.key),
        },
    }
}
fn store() -> (tempfile::TempDir, IndexStore) {
    let dir = tempfile::tempdir().unwrap();
    let index = IndexStore::open(dir.path().join("index.sqlite3")).unwrap();
    (dir, index)
}
fn request(seed: &GraphNode, scope: &str) -> GraphNeighborhoodRequest {
    GraphNeighborhoodRequest {
        scope: scope.into(),
        seeds: vec![seed.identity.clone()],
        direction: GraphDirection::Both,
        max_hops: 3,
        max_nodes: 100,
        max_edges: 100,
    }
}
fn chain(index: &IndexStore, scope: &str) -> (GraphNode, GraphNode, GraphNode) {
    let a = n(GraphKind::Project, "alpha");
    let b = n(GraphKind::Topic, "beta");
    let c = n(GraphKind::File, "gamma");
    index
        .replace_graph_projection(
            scope,
            &GraphProjection {
                nodes: vec![c.clone(), a.clone(), b.clone()],
                edges: vec![edge(&b, &c), edge(&a, &b)],
            },
        )
        .unwrap();
    (a, b, c)
}

#[test]
fn directed_neighbors_are_bounded_deterministic_and_source_proven() {
    let (_tmp, index) = store();
    let (a, b, c) = chain(&index, "private:one");
    let mut query = request(&a, "private:one");
    let both = index.graph_neighbors_with_check(&query, || Ok(())).unwrap();
    assert_eq!(
        both.nodes.iter().map(|n| n.depth).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    assert_eq!(both.nodes.len(), 3);
    assert_eq!(both.edges.len(), 2);
    assert!(both.edges.iter().all(|e| {
        e.source_ref == "fixture:source"
            && e.source_revision == "revision:1"
            && e.locator.starts_with("edge:")
    }));
    assert!(!both.truncated);

    query.direction = GraphDirection::Outbound;
    let out = index.graph_neighbors_with_check(&query, || Ok(())).unwrap();
    assert_eq!(out.nodes, both.nodes);
    query.seeds = vec![c.identity.clone()];
    let leaf = index.graph_neighbors_with_check(&query, || Ok(())).unwrap();
    assert_eq!(leaf.nodes.len(), 1);
    assert!(leaf.edges.is_empty());

    query.direction = GraphDirection::Inbound;
    let backward = index.graph_neighbors_with_check(&query, || Ok(())).unwrap();
    assert_eq!(backward.nodes.len(), 3);
    assert_eq!(backward.nodes[0].canonical_key, "gamma");

    // Stable order and stable deterministic evidence across replacement and reopen.
    let stable = both.clone();
    let mut projection = GraphProjection {
        nodes: vec![a.clone(), b.clone(), c.clone()],
        edges: vec![edge(&a, &b), edge(&b, &c)],
    };
    projection.nodes.reverse();
    projection.edges.reverse();
    index
        .replace_graph_projection("private:one", &projection)
        .unwrap();
    let reopened = IndexStore::open(index.path()).unwrap();
    assert_eq!(
        reopened
            .graph_neighbors_with_check(&request(&a, "private:one"), || Ok(()))
            .unwrap(),
        stable
    );
}

#[test]
fn scope_isolation_does_not_leak_shared_global_node_or_edges() {
    let (_tmp, index) = store();
    let (a, b, _) = chain(&index, "authorized");
    let hidden = n(GraphKind::Memory, "private");
    // Shared global identity is not an ACL: membership and edge scope enforce isolation.
    index
        .replace_graph_projection(
            "not-authorized",
            &GraphProjection {
                nodes: vec![a.clone(), hidden.clone()],
                edges: vec![edge(&a, &hidden)],
            },
        )
        .unwrap();
    let hit = index
        .graph_neighbors_with_check(&request(&a, "authorized"), || Ok(()))
        .unwrap();
    assert!(hit
        .nodes
        .iter()
        .all(|n| n.canonical_key != hidden.identity.key));
    assert!(hit.edges.iter().all(|e| !e.locator.contains("private")));
    let guessed = index
        .graph_neighbors_with_check(&request(&hidden, "authorized"), || Ok(()))
        .unwrap();
    assert!(guessed.nodes.is_empty());
    assert!(guessed.edges.is_empty());
    assert!(!guessed.truncated);
    assert_eq!(
        index
            .graph_communities_with_check("authorized", 100, 100, || Ok(()))
            .unwrap()
            .communities
            .len(),
        1
    );
    assert_eq!(b.identity.kind, GraphKind::Topic);
}

#[test]
fn permission_lease_denial_before_or_during_query_fails_closed() {
    let (_tmp, index) = store();
    let (a, _, _) = chain(&index, "scope");
    assert!(matches!(
        index.graph_neighbors_with_check(&request(&a, "scope"), || Err(IndexError::Revoked)),
        Err(IndexError::Revoked)
    ));
    let mut checks = 0;
    let denied = index.graph_neighbors_with_check(&request(&a, "scope"), || {
        checks += 1;
        if checks >= 3 {
            Err(IndexError::Revoked)
        } else {
            Ok(())
        }
    });
    assert!(checks >= 3);
    assert!(matches!(denied, Err(IndexError::Revoked)));
    let mut checks = 0;
    let communities = index.graph_communities_with_check("scope", 100, 100, || {
        checks += 1;
        if checks >= 2 {
            Err(IndexError::Revoked)
        } else {
            Ok(())
        }
    });
    assert!(matches!(communities, Err(IndexError::Revoked)));
    assert_eq!(index.graph_scope_stats("scope").unwrap().edges, 2);
}

#[test]
fn budgets_cannot_be_bypassed_and_truncation_is_truthful() {
    let (_tmp, index) = store();
    let (a, b, _) = chain(&index, "scope");
    let mut q = request(&a, "scope");
    q.max_nodes = 1;
    let limited = index.graph_neighbors_with_check(&q, || Ok(())).unwrap();
    assert!(limited.truncated);
    assert_eq!(limited.nodes.len(), 1);
    assert!(limited.edges.is_empty());
    q.max_nodes = 10;
    q.max_edges = 1;
    let limited_edges = index.graph_neighbors_with_check(&q, || Ok(())).unwrap();
    assert!(limited_edges.truncated);
    assert_eq!(limited_edges.edges.len(), 1);
    // Already visited edges do not mask unseen evidence under tight edge budgets.
    q.seeds = vec![b.identity.clone()];
    let middle = index.graph_neighbors_with_check(&q, || Ok(())).unwrap();
    assert!(middle.truncated);
    q.max_nodes = 501;
    assert!(matches!(
        index.graph_neighbors_with_check(&q, || Ok(())),
        Err(IndexError::InvalidQuery)
    ));
    q.max_nodes = 10;
    q.max_edges = 2_001;
    assert!(matches!(
        index.graph_neighbors_with_check(&q, || Ok(())),
        Err(IndexError::InvalidQuery)
    ));
    q.max_edges = 2;
    q.max_hops = 5;
    assert!(matches!(
        index.graph_neighbors_with_check(&q, || Ok(())),
        Err(IndexError::InvalidQuery)
    ));
    q.max_hops = 3;
    q.scope.clear();
    assert!(matches!(
        index.graph_neighbors_with_check(&q, || Ok(())),
        Err(IndexError::InvalidQuery)
    ));
}

#[test]
fn communities_are_complete_deterministic_and_do_not_fake_partial_clusters() {
    let (_tmp, index) = store();
    let (a, _, _) = chain(&index, "scope");
    let isolated = n(GraphKind::Entity, "isolated");
    let other = n(GraphKind::Memory, "lonely");
    let first = n(GraphKind::Topic, "topic");
    index
        .replace_graph_projection(
            "scope",
            &GraphProjection {
                nodes: vec![
                    a.clone(),
                    n(GraphKind::Topic, "beta"),
                    n(GraphKind::File, "gamma"),
                    isolated.clone(),
                    other.clone(),
                    first.clone(),
                ],
                edges: vec![
                    edge(&a, &n(GraphKind::Topic, "beta")),
                    edge(&n(GraphKind::Topic, "beta"), &n(GraphKind::File, "gamma")),
                ],
            },
        )
        .unwrap();
    let full = index
        .graph_communities_with_check("scope", 100, 100, || Ok(()))
        .unwrap();
    assert!(!full.truncated);
    assert_eq!(
        full.communities.iter().map(|c| c.node_count).sum::<usize>(),
        6
    );
    assert_eq!(full.communities.len(), 4);
    assert_eq!(full.communities[0].node_count, 3);
    assert_eq!(full.communities[0].edge_count, 2);
    assert_eq!(full.communities[0].kinds.get("project"), Some(&1));
    assert_eq!(full.communities[0].kinds.get("file"), Some(&1));
    let reopened = IndexStore::open(index.path()).unwrap();
    assert_eq!(
        reopened
            .graph_communities_with_check("scope", 100, 100, || Ok(()))
            .unwrap(),
        full
    );

    let node_overflow = index
        .graph_communities_with_check("scope", 2, 100, || Ok(()))
        .unwrap();
    assert!(node_overflow.truncated);
    assert!(node_overflow.communities.is_empty());
    let edge_overflow = index
        .graph_communities_with_check("scope", 100, 1, || Ok(()))
        .unwrap();
    assert!(edge_overflow.truncated);
    assert!(edge_overflow.communities.is_empty());
    assert_eq!(
        index
            .graph_communities_with_check("empty", 100, 100, || Ok(()))
            .unwrap()
            .communities
            .len(),
        0
    );
}

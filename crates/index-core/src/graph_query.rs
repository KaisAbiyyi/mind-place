//! MP-6B: bounded, scope-isolated queries over the disposable MP-6A graph.
//! This module never grants access: callers must verify their unlock/permission
//! lease in the supplied callback, including after the final SQLite read.
use crate::{
    GraphKind, GraphNodeRef, GraphOrigin, GraphRelation, IndexError, IndexStore, Result,
    SourceVisibility,
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const MAX_SEEDS: usize = 32;
const MAX_NODES: usize = 500;
const MAX_EDGES: usize = 2_000;
const MAX_HOPS: usize = 4;
const MAX_SCOPE_BYTES: usize = 4_096;
const MAX_COMMUNITIES: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphDirection {
    Outbound,
    Inbound,
    Both,
}
impl GraphDirection {
    fn as_str(self) -> &'static str {
        match self {
            Self::Outbound => "out",
            Self::Inbound => "in",
            Self::Both => "both",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphNeighborhoodRequest {
    /// An explicit, already authorized producer scope. No wildcard or cross-scope query.
    pub scope: String,
    pub seeds: Vec<GraphNodeRef>,
    pub direction: GraphDirection,
    /// 0 returns only the seeds; maximum 4.
    pub max_hops: usize,
    /// Maximum distinct nodes returned (1..=500).
    pub max_nodes: usize,
    /// Maximum evidence edges returned (1..=2000).
    pub max_edges: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphQueryNode {
    /// Stable derived ID; never an authorization token or source of truth.
    pub id: String,
    pub kind: GraphKind,
    pub canonical_key: String,
    pub label: String,
    pub source_ref: String,
    /// Smallest observed hop distance from any requested seed.
    pub depth: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphQueryEdge {
    pub id: String,
    pub from_node: String,
    pub to_node: String,
    pub relation: GraphRelation,
    /// Evidence origin and optional confidence come from the persisted projection.
    pub origin: GraphOrigin,
    pub source_ref: String,
    pub source_revision: String,
    pub locator: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphNeighborhood {
    /// Sorted by distance and ID, independent of insertion order.
    pub nodes: Vec<GraphQueryNode>,
    /// Sorted by stable edge ID; may contain edges between previously visited nodes.
    pub edges: Vec<GraphQueryEdge>,
    /// True if the traversal hit a bound. Not a full-corpus match estimate.
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphCommunity {
    /// Smallest stable node ID in this connected component.
    pub id: String,
    pub node_count: usize,
    pub edge_count: usize,
    pub kinds: BTreeMap<String, usize>,
    /// Explicit, bounded membership, sorted by stable node ID.
    pub member_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphCommunities {
    pub communities: Vec<GraphCommunity>,
    /// On node/edge budget overflow, no partial or misleading components are returned.
    pub truncated: bool,
}

fn validate_scope(scope: &str) -> Result<()> {
    if scope.trim().is_empty() || scope.len() > MAX_SCOPE_BYTES || scope.as_bytes().contains(&0) {
        return Err(IndexError::InvalidQuery);
    }
    Ok(())
}

fn parse_kind(kind: &str) -> Result<GraphKind> {
    Ok(match kind {
        "project" => GraphKind::Project,
        "topic" => GraphKind::Topic,
        "file" => GraphKind::File,
        "conversation" => GraphKind::Conversation,
        "memory" => GraphKind::Memory,
        "entity" => GraphKind::Entity,
        "revision" => GraphKind::Revision,
        "import" => GraphKind::Import,
        "symbol" => GraphKind::Symbol,
        _ => return Err(IndexError::InvalidIndex),
    })
}

fn parse_relation(relation: &str) -> Result<GraphRelation> {
    Ok(match relation {
        "contains" => GraphRelation::Contains,
        "references" => GraphRelation::References,
        "version" => GraphRelation::Version,
        "imported_from" => GraphRelation::ImportedFrom,
        "defines_symbol" => GraphRelation::DefinesSymbol,
        _ => return Err(IndexError::InvalidIndex),
    })
}

fn load_node(
    db: &Connection,
    scope: &str,
    id: &str,
    depth: usize,
) -> Result<Option<GraphQueryNode>> {
    let row: Option<(String, String, String, String)> = db
        .query_row(
            "SELECT n.kind,n.canonical_key,n.label,n.source_ref
             FROM graph_nodes n
             JOIN graph_scope_nodes m ON m.node_id=n.id
             WHERE m.scope=?1 AND n.id=?2",
            params![scope, id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    row.map(|(kind, canonical_key, label, source_ref)| {
        Ok(GraphQueryNode {
            id: id.to_owned(),
            kind: parse_kind(&kind)?,
            canonical_key,
            label,
            source_ref,
            depth,
        })
    })
    .transpose()
}

type EdgeRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    Option<i64>,
);

fn edge_from_row(row: EdgeRow) -> Result<GraphQueryEdge> {
    let origin = match (row.7.as_str(), row.8) {
        ("deterministic", None) => GraphOrigin::Deterministic,
        ("inferred", Some(v)) if (8_000..=10_000).contains(&v) => GraphOrigin::Inferred {
            confidence_bps: v as u16,
        },
        _ => return Err(IndexError::InvalidIndex),
    };
    Ok(GraphQueryEdge {
        id: row.0,
        from_node: row.1,
        to_node: row.2,
        relation: parse_relation(&row.3)?,
        origin,
        source_ref: row.4,
        source_revision: row.5,
        locator: row.6,
    })
}

fn graph_edges_for(
    db: &Connection,
    scope: &str,
    node_id: &str,
    direction: GraphDirection,
    limit: usize,
    seen_ids_json: &str,
    after_id: &str,
) -> Result<Vec<GraphQueryEdge>> {
    // Both endpoints must be members of the SAME requested scope. Merely
    // sharing a global node ID never authorizes reading another scope's edges.
    let mut stmt = db.prepare(
        "SELECT e.id,e.from_node,e.to_node,e.relation,
                e.source_ref,e.source_revision,e.locator,e.origin,e.confidence_bps
         FROM graph_edges e
         JOIN graph_scope_nodes f ON f.node_id=e.from_node AND f.scope=e.scope
         JOIN graph_scope_nodes t ON t.node_id=e.to_node AND t.scope=e.scope
         WHERE e.scope=?1 AND
             e.id NOT IN (SELECT value FROM json_each(?4)) AND
             e.id>?6 AND
             ((?3='both' AND (e.from_node=?2 OR e.to_node=?2))
              OR (?3='out' AND e.from_node=?2)
              OR (?3='in' AND e.to_node=?2))
         ORDER BY e.id LIMIT ?5",
    )?;
    let rows = stmt.query_map(
        params![
            scope,
            node_id,
            direction.as_str(),
            seen_ids_json,
            limit as i64,
            after_id
        ],
        |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
            ))
        },
    )?;
    rows.map(|row| edge_from_row(row?)).collect()
}

/// MP-9C: per-query visibility gate. Hidden sources are removed while the
/// graph is traversed (seed resolution and every expansion), never after, so
/// they cannot occupy node/edge budgets or be reached through a stale
/// projection. A non-canonical Import node carries no source of its own: it
/// is visible only while at least one `imported_from` edge names a visible
/// canonical source (all-hidden or edge-less imports are hidden).
struct Gate<'a, V: SourceVisibility + ?Sized> {
    visible: &'a mut V,
    hidden: Option<BTreeSet<String>>,
    sources: BTreeMap<String, bool>,
    imports: BTreeMap<String, bool>,
}

impl<'a, V: SourceVisibility + ?Sized> Gate<'a, V> {
    fn new(visible: &'a mut V) -> Result<Self> {
        let hidden = visible
            .hidden_sources()?
            .map(|ids| ids.into_iter().collect());
        Ok(Self {
            visible,
            hidden,
            sources: BTreeMap::new(),
            imports: BTreeMap::new(),
        })
    }

    fn source(&mut self, source_ref: &str) -> Result<bool> {
        if !source_ref.starts_with("src:v1:") {
            return Ok(true);
        }
        if let Some(hidden) = &self.hidden {
            return Ok(!hidden.contains(source_ref));
        }
        if let Some(shown) = self.sources.get(source_ref) {
            return Ok(*shown);
        }
        let shown = self.visible.is_visible(source_ref)?;
        self.sources.insert(source_ref.to_owned(), shown);
        Ok(shown)
    }

    fn node(&mut self, db: &Connection, scope: &str, node: &GraphQueryNode) -> Result<bool> {
        if node.source_ref.starts_with("src:v1:") {
            return self.source(&node.source_ref);
        }
        if node.kind != GraphKind::Import {
            return Ok(true);
        }
        if let Some(shown) = self.imports.get(&node.id) {
            return Ok(*shown);
        }
        let mut stmt = db.prepare(
            "SELECT DISTINCT source_ref FROM graph_edges
             WHERE scope=?1 AND to_node=?2 AND relation='imported_from' ORDER BY source_ref",
        )?;
        let refs = stmt
            .query_map(params![scope, node.id], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut shown = false;
        for source_ref in refs {
            if source_ref.starts_with("src:v1:") && self.source(&source_ref)? {
                shown = true;
                break;
            }
        }
        self.imports.insert(node.id.clone(), shown);
        Ok(shown)
    }

    fn edge(&mut self, edge: &GraphQueryEdge) -> Result<bool> {
        self.source(&edge.source_ref)
    }
}

impl IndexStore {
    /// Read-only graph traversal. The callback must validate current unlock and
    /// scope permission; it is invoked before opening, at every BFS expansion,
    /// and before disclosing results. Revoke causes an error, never partial hits.
    /// This variant applies NO source visibility; desktop/MCP disclosure must
    /// use `graph_neighbors_visible_with_check`.
    pub fn graph_neighbors_with_check(
        &self,
        request: &GraphNeighborhoodRequest,
        check: impl FnMut() -> Result<()>,
    ) -> Result<GraphNeighborhood> {
        self.neighbors_inner::<fn(&str) -> Result<bool>>(request, None, check)
    }

    /// MP-9C: traversal under current source authority. Seeds and every
    /// expansion are gated, so hidden sources never reach the result or its
    /// budgets, and `max_hops: 0` returns only visible seeds.
    pub fn graph_neighbors_visible_with_check(
        &self,
        request: &GraphNeighborhoodRequest,
        visible: &mut (impl SourceVisibility + ?Sized),
        check: impl FnMut() -> Result<()>,
    ) -> Result<GraphNeighborhood> {
        let gate = Gate::new(visible)?;
        self.neighbors_inner(request, Some(gate), check)
    }

    fn neighbors_inner<V: SourceVisibility + ?Sized>(
        &self,
        request: &GraphNeighborhoodRequest,
        mut gate: Option<Gate<'_, V>>,
        mut check: impl FnMut() -> Result<()>,
    ) -> Result<GraphNeighborhood> {
        validate_scope(&request.scope)?;
        if request.seeds.is_empty()
            || request.seeds.len() > MAX_SEEDS
            || request.max_hops > MAX_HOPS
            || !(1..=MAX_NODES).contains(&request.max_nodes)
            || !(1..=MAX_EDGES).contains(&request.max_edges)
            || request.seeds.len() > request.max_nodes
        {
            return Err(IndexError::InvalidQuery);
        }
        // Id validation rejects empty, oversized or NUL identity keys.
        let seeds: BTreeSet<String> = request
            .seeds
            .iter()
            .map(GraphNodeRef::id)
            .collect::<Result<_>>()?;
        check()?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Deferred)?;
        let mut found = BTreeMap::<String, GraphQueryNode>::new();
        let mut queue = VecDeque::<(String, usize)>::new();
        for id in seeds {
            if let Some(node) = load_node(&tx, &request.scope, &id, 0)? {
                if let Some(gate) = gate.as_mut() {
                    if !gate.node(&tx, &request.scope, &node)? {
                        continue;
                    }
                }
                found.insert(id.clone(), node);
                queue.push_back((id, 0));
            }
        }
        let mut edges = BTreeMap::<String, GraphQueryEdge>::new();
        let mut truncated = false;
        while let Some((id, depth)) = queue.pop_front() {
            check()?;
            if depth == request.max_hops {
                continue;
            }
            // One extra row signals possible truncation without scanning a
            // high-degree node or disclosing any out-of-scope information.
            // Hidden rows are skipped without consuming budget; paging by
            // edge ID continues past a page that was entirely hidden.
            let mut after = String::new();
            'pages: loop {
                let remaining = request.max_edges - edges.len();
                let seen_ids_json = serde_json::to_string(&edges.keys().collect::<Vec<_>>())
                    .map_err(|_| IndexError::InvalidQuery)?;
                let page_limit = remaining + 1;
                let adjacent = graph_edges_for(
                    &tx,
                    &request.scope,
                    &id,
                    request.direction,
                    page_limit,
                    &seen_ids_json,
                    &after,
                )?;
                let page_len = adjacent.len();
                for edge in adjacent {
                    after.clone_from(&edge.id);
                    if edges.contains_key(&edge.id) {
                        continue;
                    }
                    if let Some(gate) = gate.as_mut() {
                        if !gate.edge(&edge)? {
                            continue;
                        }
                    }
                    let target = if edge.from_node == id {
                        edge.to_node.clone()
                    } else {
                        edge.from_node.clone()
                    };
                    let node = if found.contains_key(&target) {
                        None
                    } else {
                        // Fail closed if the endpoint disappeared from this scope.
                        Some(
                            load_node(&tx, &request.scope, &target, depth + 1)?
                                .ok_or(IndexError::InvalidIndex)?,
                        )
                    };
                    if let (Some(gate), Some(node)) = (gate.as_mut(), node.as_ref()) {
                        if !gate.node(&tx, &request.scope, node)? {
                            continue;
                        }
                    }
                    if edges.len() >= request.max_edges {
                        truncated = true;
                        break 'pages;
                    }
                    if let Some(node) = node {
                        if found.len() >= request.max_nodes {
                            truncated = true;
                            break 'pages;
                        }
                        found.insert(target.clone(), node);
                        queue.push_back((target, depth + 1));
                    }
                    edges.insert(edge.id.clone(), edge);
                }
                if page_len < page_limit {
                    break;
                }
            }
            if truncated {
                break;
            }
        }
        check()?;
        tx.commit()?;
        let mut nodes: Vec<_> = found.into_values().collect();
        nodes.sort_by(|a, b| a.depth.cmp(&b.depth).then(a.id.cmp(&b.id)));
        Ok(GraphNeighborhood {
            nodes,
            edges: edges.into_values().collect(),
            truncated,
        })
    }

    /// Complete connected-component aggregation for a single authorized scope.
    /// If a budget is exceeded, returns no fake partial "communities" and
    /// explicitly sets truncated=true. Does not perform source or state writes.
    pub fn graph_communities_with_check(
        &self,
        scope: &str,
        max_nodes: usize,
        max_edges: usize,
        mut check: impl FnMut() -> Result<()>,
    ) -> Result<GraphCommunities> {
        validate_scope(scope)?;
        if !(1..=MAX_NODES).contains(&max_nodes) || !(1..=MAX_EDGES).contains(&max_edges) {
            return Err(IndexError::InvalidQuery);
        }
        check()?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Deferred)?;
        let all_nodes: Vec<GraphQueryNode> = {
            let mut stmt = tx.prepare(
                "SELECT n.id,n.kind,n.canonical_key,n.label,n.source_ref
                 FROM graph_scope_nodes m JOIN graph_nodes n ON n.id=m.node_id
                 WHERE m.scope=?1 ORDER BY n.id LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![scope, (max_nodes + 1) as i64], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })?;
            rows.map(|row| {
                let (id, kind, canonical_key, label, source_ref) = row?;
                Ok(GraphQueryNode {
                    id,
                    kind: parse_kind(&kind)?,
                    canonical_key,
                    label,
                    source_ref,
                    depth: 0,
                })
            })
            .collect::<Result<_>>()?
        };
        if all_nodes.len() > max_nodes {
            check()?;
            return Ok(GraphCommunities {
                communities: vec![],
                truncated: true,
            });
        }
        check()?;
        let all_edges: Vec<GraphQueryEdge> = {
            let mut stmt = tx.prepare(
                "SELECT e.id,e.from_node,e.to_node,e.relation,
                        e.source_ref,e.source_revision,e.locator,e.origin,e.confidence_bps
                 FROM graph_edges e
                 JOIN graph_scope_nodes a ON a.scope=e.scope AND a.node_id=e.from_node
                 JOIN graph_scope_nodes b ON b.scope=e.scope AND b.node_id=e.to_node
                 WHERE e.scope=?1 ORDER BY e.id LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![scope, (max_edges + 1) as i64], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                ))
            })?;
            rows.map(|row| edge_from_row(row?)).collect::<Result<_>>()?
        };
        if all_edges.len() > max_edges {
            check()?;
            return Ok(GraphCommunities {
                communities: vec![],
                truncated: true,
            });
        }
        let by_id: BTreeMap<_, _> = all_nodes.into_iter().map(|n| (n.id.clone(), n)).collect();
        let mut adjacency: BTreeMap<String, BTreeSet<String>> = by_id
            .keys()
            .map(|id| (id.clone(), BTreeSet::new()))
            .collect();
        for edge in &all_edges {
            adjacency
                .get_mut(&edge.from_node)
                .ok_or(IndexError::InvalidIndex)?
                .insert(edge.to_node.clone());
            adjacency
                .get_mut(&edge.to_node)
                .ok_or(IndexError::InvalidIndex)?
                .insert(edge.from_node.clone());
        }
        let mut unseen: BTreeSet<_> = by_id.keys().cloned().collect();
        let mut communities = Vec::new();
        while let Some(start) = unseen.iter().next().cloned() {
            check()?;
            unseen.remove(&start);
            let mut frontier = VecDeque::from([start.clone()]);
            let mut members = Vec::new();
            let mut kinds = BTreeMap::<String, usize>::new();
            while let Some(id) = frontier.pop_front() {
                *kinds
                    .entry(by_id[&id].kind.as_str().to_owned())
                    .or_default() += 1;
                members.push(id.clone());
                for next in &adjacency[&id] {
                    if unseen.remove(next) {
                        frontier.push_back(next.clone());
                    }
                }
            }
            members.sort();
            let member_set: BTreeSet<_> = members.iter().cloned().collect();
            let edge_count = all_edges
                .iter()
                .filter(|e| member_set.contains(&e.from_node) && member_set.contains(&e.to_node))
                .count();
            communities.push(GraphCommunity {
                id: members[0].clone(),
                node_count: members.len(),
                edge_count,
                kinds,
                member_ids: members,
            });
        }
        communities.sort_by(|a, b| b.node_count.cmp(&a.node_count).then(a.id.cmp(&b.id)));
        let truncated = communities.len() > MAX_COMMUNITIES;
        communities.truncate(MAX_COMMUNITIES);
        check()?;
        tx.commit()?;
        Ok(GraphCommunities {
            communities,
            truncated,
        })
    }
}

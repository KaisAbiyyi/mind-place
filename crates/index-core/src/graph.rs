//! MP-6A: bounded, rebuildable, evidence-linked graph projection.
//! Source records, memory decisions and permissions remain authoritative elsewhere.
use crate::{IndexError, IndexStore, Result};
use rusqlite::{params, Transaction, TransactionBehavior};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const MAX_GRAPH_ITEMS: usize = 100_000;
const MAX_GRAPH_FIELD_BYTES: usize = 4096;
const ARCHIVE_SCOPE: &str = "archive:chatgpt";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GraphKind {
    Project,
    Topic,
    File,
    Conversation,
    Memory,
    Entity,
    Revision,
    Import,
    Symbol,
}

impl GraphKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Topic => "topic",
            Self::File => "file",
            Self::Conversation => "conversation",
            Self::Memory => "memory",
            Self::Entity => "entity",
            Self::Revision => "revision",
            Self::Import => "import",
            Self::Symbol => "symbol",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphRelation {
    Contains,
    References,
    Version,
    ImportedFrom,
    DefinesSymbol,
}

impl GraphRelation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Contains => "contains",
            Self::References => "references",
            Self::Version => "version",
            Self::ImportedFrom => "imported_from",
            Self::DefinesSymbol => "defines_symbol",
        }
    }

    fn allowed(self, from: GraphKind, to: GraphKind) -> bool {
        match self {
            Self::Contains => matches!(
                from,
                GraphKind::Project
                    | GraphKind::Topic
                    | GraphKind::Conversation
                    | GraphKind::Entity
                    | GraphKind::Import
            ),
            Self::References => true,
            Self::Version => {
                matches!(
                    from,
                    GraphKind::Conversation | GraphKind::File | GraphKind::Memory
                ) && to == GraphKind::Revision
            }
            Self::ImportedFrom => to == GraphKind::Import && from != GraphKind::Import,
            Self::DefinesSymbol => from == GraphKind::File && to == GraphKind::Symbol,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct GraphNodeRef {
    pub kind: GraphKind,
    pub key: String,
}

impl GraphNodeRef {
    pub fn new(kind: GraphKind, key: impl Into<String>) -> Self {
        Self {
            kind,
            key: key.into(),
        }
    }

    pub fn id(&self) -> Result<String> {
        validate_field(&self.key)?;
        Ok(stable_id("gn", &[self.kind.as_str(), &self.key]))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphNode {
    pub identity: GraphNodeRef,
    pub label: String,
    /// Durable source identity or scoped source URI; an evidence pointer, not authority.
    pub source_ref: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphEvidence {
    pub source_ref: String,
    /// Empty string when the source has no version.
    pub source_revision: String,
    /// Stable document range, symbol location, message ID, or archive revision locator.
    pub locator: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphEdge {
    pub from: GraphNodeRef,
    pub to: GraphNodeRef,
    pub relation: GraphRelation,
    pub evidence: GraphEvidence,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GraphProjection {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GraphProjectionStats {
    pub nodes: i64,
    pub edges: i64,
}

fn validate_field(value: &str) -> Result<()> {
    if value.trim().is_empty()
        || value.len() > MAX_GRAPH_FIELD_BYTES
        || value.as_bytes().contains(&0)
    {
        return Err(IndexError::InvalidQuery);
    }
    Ok(())
}

fn stable_id(prefix: &str, parts: &[&str]) -> String {
    // Length-prefix every field to avoid ambiguous tuples and separator collisions.
    let mut hash = Sha256::new();
    hash.update(b"mind-place-graph-v1");
    for part in parts {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part.as_bytes());
    }
    format!("{prefix}-{:x}", hash.finalize())
}

fn checked_projection(
    scope: &str,
    projection: &GraphProjection,
) -> Result<(BTreeMap<String, &GraphNode>, BTreeMap<String, &GraphEdge>)> {
    if projection.nodes.len() > MAX_GRAPH_ITEMS || projection.edges.len() > MAX_GRAPH_ITEMS {
        return Err(IndexError::InvalidQuery);
    }
    let mut nodes = BTreeMap::new();
    for node in &projection.nodes {
        let id = node.identity.id()?;
        validate_field(&node.label)?;
        validate_field(&node.source_ref)?;
        if let Some(existing) = nodes.insert(id, node) {
            if existing != node {
                return Err(IndexError::InvalidQuery);
            }
        }
    }
    let mut edges = BTreeMap::new();
    for edge in &projection.edges {
        let from = edge.from.id()?;
        let to = edge.to.id()?;
        if from == to
            || !nodes.contains_key(&from)
            || !nodes.contains_key(&to)
            || !edge.relation.allowed(edge.from.kind, edge.to.kind)
        {
            return Err(IndexError::InvalidQuery);
        }
        validate_field(&edge.evidence.source_ref)?;
        if edge.evidence.source_revision.len() > MAX_GRAPH_FIELD_BYTES
            || edge.evidence.source_revision.as_bytes().contains(&0)
        {
            return Err(IndexError::InvalidQuery);
        }
        validate_field(&edge.evidence.locator)?;
        let id = stable_id(
            "ge",
            &[
                scope,
                &from,
                &to,
                edge.relation.as_str(),
                &edge.evidence.source_ref,
                &edge.evidence.source_revision,
                &edge.evidence.locator,
            ],
        );
        if let Some(existing) = edges.insert(id, edge) {
            if existing != edge {
                return Err(IndexError::InvalidQuery);
            }
        }
    }
    Ok((nodes, edges))
}

/// Atomically replace only one derived scope. Unrelated scoped projections remain.
/// Edges must reference nodes declared within this scope; no hidden cross-scope reads.
fn replace_tx(
    tx: &Transaction<'_>,
    scope: &str,
    projection: &GraphProjection,
) -> Result<GraphProjectionStats> {
    validate_field(scope)?;
    let (nodes, edges) = checked_projection(scope, projection)?;
    tx.execute("DELETE FROM graph_edges WHERE scope=?1", [scope])?;
    tx.execute("DELETE FROM graph_scope_nodes WHERE scope=?1", [scope])?;
    for (id, node) in &nodes {
        // Shared global identity must not allow one scope to overwrite another
        // scope's label or source pointer silently.
        let conflict: bool = tx.query_row(
            "SELECT EXISTS(
               SELECT 1 FROM graph_nodes n
               JOIN graph_scope_nodes m ON m.node_id=n.id
               WHERE n.id=?1 AND m.scope<>?2
               AND (n.label<>?3 OR n.source_ref<>?4)
             )",
            params![id, scope, node.label, node.source_ref],
            |r| r.get(0),
        )?;
        if conflict {
            return Err(IndexError::InvalidQuery);
        }
        tx.execute(
            "INSERT INTO graph_nodes(id,kind,canonical_key,label,source_ref)
             VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(id) DO UPDATE SET label=excluded.label,source_ref=excluded.source_ref",
            params![
                id,
                node.identity.kind.as_str(),
                node.identity.key,
                node.label,
                node.source_ref
            ],
        )?;
        tx.execute(
            "INSERT INTO graph_scope_nodes(scope,node_id) VALUES(?1,?2)",
            params![scope, id],
        )?;
    }
    for (id, edge) in &edges {
        tx.execute(
            "INSERT INTO graph_edges(id,scope,from_node,to_node,relation,origin,
                 source_ref,source_revision,locator)
             VALUES(?1,?2,?3,?4,?5,'deterministic',?6,?7,?8)",
            params![
                id,
                scope,
                edge.from.id()?,
                edge.to.id()?,
                edge.relation.as_str(),
                edge.evidence.source_ref,
                edge.evidence.source_revision,
                edge.evidence.locator
            ],
        )?;
    }
    // A node can be shared by multiple scopes. Only remove when all scopes release it.
    tx.execute(
        "DELETE FROM graph_nodes
         WHERE NOT EXISTS (SELECT 1 FROM graph_scope_nodes m WHERE m.node_id=graph_nodes.id)",
        [],
    )?;
    Ok(GraphProjectionStats {
        nodes: nodes.len() as i64,
        edges: edges.len() as i64,
    })
}

impl IndexStore {
    pub fn replace_graph_projection(
        &self,
        scope: &str,
        projection: &GraphProjection,
    ) -> Result<GraphProjectionStats> {
        self.replace_graph_projection_with_check(scope, projection, || Ok(()))
    }

    /// Check an unlock/permission lease before work and again before commit.
    pub fn replace_graph_projection_with_check(
        &self,
        scope: &str,
        projection: &GraphProjection,
        mut check: impl FnMut() -> Result<()>,
    ) -> Result<GraphProjectionStats> {
        // The archive scope is managed only by verified raw archive synchronization.
        if scope == ARCHIVE_SCOPE {
            return Err(IndexError::InvalidQuery);
        }
        check()?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let stats = replace_tx(&tx, scope, projection)?;
        check()?;
        tx.commit()?;
        Ok(stats)
    }

    pub fn graph_scope_stats(&self, scope: &str) -> Result<GraphProjectionStats> {
        validate_field(scope)?;
        let db = self.connection()?;
        let nodes = db.query_row(
            "SELECT COUNT(*) FROM graph_scope_nodes WHERE scope=?1",
            [scope],
            |r| r.get(0),
        )?;
        let edges = db.query_row(
            "SELECT COUNT(*) FROM graph_edges WHERE scope=?1",
            [scope],
            |r| r.get(0),
        )?;
        Ok(GraphProjectionStats { nodes, edges })
    }
}

/// Purely structural relationships proven by verified archive revision metadata.
/// Called inside the same transaction as FTS sync and its last security check.
pub(crate) fn refresh_archive_graph(tx: &Transaction<'_>) -> Result<()> {
    let mut stmt = tx.prepare(
        "SELECT source_id,archive_id,snapshot_id,title
         FROM revisions ORDER BY source_id,is_current DESC,snapshot_id",
    )?;
    let mut rows = stmt.query([])?;
    let mut nodes = BTreeMap::<GraphNodeRef, GraphNode>::new();
    let mut edges = Vec::new();
    while let Some(row) = rows.next()? {
        let source_id: String = row.get(0)?;
        let archive_id: String = row.get(1)?;
        let snapshot_id: String = row.get(2)?;
        let title: String = row.get(3)?;
        let conversation = GraphNodeRef::new(GraphKind::Conversation, &source_id);
        let revision = GraphNodeRef::new(GraphKind::Revision, format!("{source_id}/{snapshot_id}"));
        let import = GraphNodeRef::new(GraphKind::Import, &archive_id);
        nodes.entry(conversation.clone()).or_insert(GraphNode {
            identity: conversation.clone(),
            label: if title.trim().is_empty() {
                "Untitled conversation".into()
            } else {
                // Projection labels are bounded independently of the raw archive.
                let mut end = title.len().min(MAX_GRAPH_FIELD_BYTES);
                while !title.is_char_boundary(end) {
                    end -= 1;
                }
                title[..end].to_owned()
            },
            source_ref: source_id.clone(),
        });
        nodes.entry(revision.clone()).or_insert(GraphNode {
            identity: revision.clone(),
            label: snapshot_id.clone(),
            source_ref: source_id.clone(),
        });
        nodes.entry(import.clone()).or_insert(GraphNode {
            identity: import.clone(),
            label: archive_id.clone(),
            source_ref: archive_id.clone(),
        });
        let evidence = GraphEvidence {
            source_ref: source_id,
            source_revision: snapshot_id.clone(),
            locator: format!("archive:{archive_id}/{snapshot_id}"),
        };
        edges.push(GraphEdge {
            from: conversation,
            to: revision.clone(),
            relation: GraphRelation::Version,
            evidence: evidence.clone(),
        });
        edges.push(GraphEdge {
            from: revision,
            to: import,
            relation: GraphRelation::ImportedFrom,
            evidence,
        });
    }
    drop(rows);
    drop(stmt);
    replace_tx(
        tx,
        ARCHIVE_SCOPE,
        &GraphProjection {
            nodes: nodes.into_values().collect(),
            edges,
        },
    )?;
    Ok(())
}

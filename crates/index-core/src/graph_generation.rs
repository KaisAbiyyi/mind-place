//! MP-6C: scoped source fingerprints, generation fences and atomic graph activation.
//! Staging is bounded in memory; visible edges and manifests swap in one SQLite tx.
use crate::graph::{replace_tx, validate_field, GraphProjection, GraphProjectionStats, ARCHIVE_SCOPE};
use crate::{IndexError, IndexStore, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GraphInputHashes {
    /// Digest of canonical file identity/path inventory.
    pub files: String,
    /// Digest of verified source contents or revision IDs.
    pub content: String,
    /// Digest of normalized structural dependencies/edges.
    pub structure: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphGeneration {
    pub generation: i64,
    pub active: bool,
    pub input: GraphInputHashes,
    pub projection_hash: String,
    pub stats: GraphProjectionStats,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphActivation {
    pub generation: GraphGeneration,
    /// False when both source fingerprints and projected content are unchanged.
    pub changed: bool,
}

fn valid_digest(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn validate_input(input: &GraphInputHashes) -> Result<()> {
    if !valid_digest(&input.files)
        || !valid_digest(&input.content)
        || !valid_digest(&input.structure)
    {
        return Err(IndexError::InvalidQuery);
    }
    Ok(())
}

fn check_scope(scope: &str) -> Result<()> {
    validate_field(scope)?;
    if scope == ARCHIVE_SCOPE {
        return Err(IndexError::InvalidQuery);
    }
    Ok(())
}

fn read_generation(db: &Connection, scope: &str) -> Result<Option<GraphGeneration>> {
    db.query_row(
        "SELECT generation,active,files_hash,content_hash,structure_hash,projection_hash,nodes,edges
         FROM graph_generations WHERE scope=?1",
        [scope],
        |row| {
            Ok(GraphGeneration {
                generation: row.get(0)?,
                active: row.get::<_, i64>(1)? == 1,
                input: GraphInputHashes {
                    files: row.get(2)?,
                    content: row.get(3)?,
                    structure: row.get(4)?,
                },
                projection_hash: row.get(5)?,
                stats: GraphProjectionStats {
                    nodes: row.get(6)?,
                    edges: row.get(7)?,
                },
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

/// Deterministic digest over complete node and evidence tuples (not merely edge counts).
/// Order and identical duplicate declarations cannot change the digest.
fn projection_digest(projection: &GraphProjection) -> Result<String> {
    // Validate and bound before hashing to avoid unbounded allocations.
    if projection.nodes.len() > 100_000 || projection.edges.len() > 100_000 {
        return Err(IndexError::InvalidQuery);
    }
    let mut node_rows = Vec::with_capacity(projection.nodes.len());
    for node in &projection.nodes {
        validate_field(&node.identity.key)?;
        validate_field(&node.label)?;
        validate_field(&node.source_ref)?;
        node_rows.push(vec![
            node.identity.kind.as_str().to_owned(),
            node.identity.key.clone(),
            node.label.clone(),
            node.source_ref.clone(),
        ]);
    }
    node_rows.sort();
    node_rows.dedup();
    let mut edge_rows = Vec::with_capacity(projection.edges.len());
    for edge in &projection.edges {
        validate_field(&edge.from.key)?;
        validate_field(&edge.to.key)?;
        validate_field(&edge.evidence.source_ref)?;
        validate_field(&edge.evidence.locator)?;
        if edge.evidence.source_revision.len() > 4096
            || edge.evidence.source_revision.as_bytes().contains(&0)
        {
            return Err(IndexError::InvalidQuery);
        }
        edge_rows.push(vec![
            edge.from.kind.as_str().to_owned(),
            edge.from.key.clone(),
            edge.to.kind.as_str().to_owned(),
            edge.to.key.clone(),
            edge.relation.as_str().to_owned(),
            edge.evidence.source_ref.clone(),
            edge.evidence.source_revision.clone(),
            edge.evidence.locator.clone(),
        ]);
    }
    edge_rows.sort();
    edge_rows.dedup();
    let mut hash = Sha256::new();
    hash.update(b"mind-place-graph-projection-v1");
    for (tag, rows) in [(b'n', node_rows), (b'e', edge_rows)] {
        hash.update([tag]);
        hash.update((rows.len() as u64).to_be_bytes());
        for row in rows {
            hash.update((row.len() as u64).to_be_bytes());
            for field in row {
                hash.update((field.len() as u64).to_be_bytes());
                hash.update(field.as_bytes());
            }
        }
    }
    Ok(format!("{:x}", hash.finalize()))
}

impl IndexStore {
    /// Read the activation fence, including inactive tombstones. None means never built.
    pub fn graph_generation(&self, scope: &str) -> Result<Option<GraphGeneration>> {
        validate_field(scope)?;
        let db = self.connection()?;
        read_generation(&db, scope)
    }

    /// Stage verified source-derived graph in memory and atomically replace active rows.
    /// expected_generation is mandatory optimistic concurrency fencing: None only on first build.
    /// Source hash computation/permissions remain with the calling trusted producer.
    pub fn activate_graph_generation_with_check(
        &self,
        scope: &str,
        input: &GraphInputHashes,
        projection: &GraphProjection,
        expected_generation: Option<i64>,
        mut check: impl FnMut() -> Result<()>,
    ) -> Result<GraphActivation> {
        check_scope(scope)?;
        validate_input(input)?;
        if expected_generation.is_some_and(|value| value < 1) {
            return Err(IndexError::InvalidQuery);
        }
        check()?;
        let digest = projection_digest(projection)?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let previous = read_generation(&tx, scope)?;
        if previous.as_ref().map(|state| state.generation) != expected_generation {
            return Err(IndexError::StaleGeneration);
        }
        if let Some(ref prior) = previous {
            if prior.active && prior.input == *input && prior.projection_hash == digest {
                // Even no-op short circuits must recheck the permission lease.
                check()?;
                return Ok(GraphActivation {
                    generation: prior.clone(),
                    changed: false,
                });
            }
        }
        let stats = replace_tx(&tx, scope, projection)?;
        let next = previous
            .map_or(Some(1), |state| state.generation.checked_add(1))
            .ok_or(IndexError::InvalidIndex)?;
        tx.execute(
            "INSERT INTO graph_generations(scope,generation,active,files_hash,content_hash,structure_hash,projection_hash,nodes,edges)
             VALUES(?1,?2,1,?3,?4,?5,?6,?7,?8)
             ON CONFLICT(scope) DO UPDATE SET generation=excluded.generation,active=1,
               files_hash=excluded.files_hash,content_hash=excluded.content_hash,
               structure_hash=excluded.structure_hash,projection_hash=excluded.projection_hash,
               nodes=excluded.nodes,edges=excluded.edges",
            params![scope, next, input.files, input.content, input.structure, digest, stats.nodes, stats.edges],
        )?;
        check()?;
        tx.commit()?;
        Ok(GraphActivation {
            generation: GraphGeneration {
                generation: next,
                active: true,
                input: input.clone(),
                projection_hash: digest,
                stats,
            },
            changed: true,
        })
    }

    pub fn activate_graph_generation(
        &self,
        scope: &str,
        input: &GraphInputHashes,
        projection: &GraphProjection,
        expected_generation: Option<i64>,
    ) -> Result<GraphActivation> {
        self.activate_graph_generation_with_check(
            scope, input, projection, expected_generation, || Ok(())
        )
    }

    /// Atomically revoke visible rows and advance a tombstone to fence stale producers.
    /// A source change should invalidate before a potentially slow rebuild begins.
    pub fn invalidate_graph_generation_with_check(
        &self,
        scope: &str,
        expected_generation: Option<i64>,
        mut check: impl FnMut() -> Result<()>,
    ) -> Result<GraphGeneration> {
        check_scope(scope)?;
        check()?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let previous = read_generation(&tx, scope)?;
        if previous.as_ref().map(|state| state.generation) != expected_generation {
            return Err(IndexError::StaleGeneration);
        }
        let next = previous
            .map_or(Some(1), |state| state.generation.checked_add(1))
            .ok_or(IndexError::InvalidIndex)?;
        let stats = replace_tx(&tx, scope, &GraphProjection::default())?;
        tx.execute(
            "INSERT INTO graph_generations(scope,generation,active,files_hash,content_hash,structure_hash,projection_hash,nodes,edges)
             VALUES(?1,?2,0,'','','','',0,0)
             ON CONFLICT(scope) DO UPDATE SET generation=excluded.generation,active=0,
               files_hash='',content_hash='',structure_hash='',projection_hash='',nodes=0,edges=0",
            params![scope, next],
        )?;
        check()?;
        tx.commit()?;
        Ok(GraphGeneration {
            generation: next,
            active: false,
            input: GraphInputHashes::default(),
            projection_hash: String::new(),
            stats,
        })
    }

    pub fn invalidate_graph_generation(
        &self,
        scope: &str,
        expected_generation: Option<i64>,
    ) -> Result<GraphGeneration> {
        self.invalidate_graph_generation_with_check(scope, expected_generation, || Ok(()))
    }
}

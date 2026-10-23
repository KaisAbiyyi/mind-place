//! MP-6D: in-process, short-lived Graphify maintenance authority.
//!
//! No bearer secret or permanent HTTP/MCP write endpoint is created. A trusted
//! desktop producer owns this opaque job. Source evidence stays authoritative
//! in state/archive; only the disposable graph projection is mutated.
use crate::graph::{validate_field, GraphOrigin, GraphRelation, ARCHIVE_SCOPE};
use crate::{
    GraphActivation, GraphEdge, GraphInputHashes, GraphNode, GraphProjection, IndexError,
    IndexStore, Result,
};
use mind_place_security::{SecurityGate, UnlockLease};
use mind_place_state::{StateStore, VirtualDocumentPath};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

const MAX_LIFETIME: Duration = Duration::from_secs(15 * 60);
const MAX_BATCH: usize = 512;
const MAX_ITEMS: usize = 20_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphMaintenanceBatch {
    /// Starts at zero and increases by exactly one per distinct submission.
    pub sequence: u64,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GraphBatchReceipt {
    pub sequence: u64,
    pub next_sequence: u64,
    pub replayed: bool,
}

/// A job is intentionally neither Clone nor Serialize. Dropping it discards
/// uncommitted batches, including on process restart or worker cancellation.
pub struct GraphMaintenanceJob<'a> {
    index: &'a IndexStore,
    authority: &'a StateStore,
    security: &'a SecurityGate,
    lease: UnlockLease,
    scope: String,
    root_id: String,
    source_revision: String,
    input: GraphInputHashes,
    expected_generation: Option<i64>,
    expires_at: Instant,
    verify_revision: Box<dyn FnMut() -> Result<String> + 'a>,
    batches: BTreeMap<u64, GraphMaintenanceBatch>,
    next_sequence: u64,
    nodes: usize,
    edges: usize,
    cancelled: bool,
    committed: Option<GraphActivation>,
}

impl IndexStore {
    /// Issue only from a trusted, unlocked local producer. The revision callback
    /// must read authoritative/current source fingerprints, never a graph cache.
    /// A job binds exactly one approved root, derived scope, source revision,
    /// graph input hashes and optimistic generation. No MCP tool exposes this.
    #[allow(clippy::too_many_arguments)]
    pub fn begin_graph_maintenance<'a>(
        &'a self,
        authority: &'a StateStore,
        security: &'a SecurityGate,
        scope: &str,
        root_id: &str,
        source_revision: &str,
        input: GraphInputHashes,
        expected_generation: Option<i64>,
        lifetime: Duration,
        mut verify_revision: impl FnMut() -> Result<String> + 'a,
    ) -> Result<GraphMaintenanceJob<'a>> {
        validate_field(scope)?;
        if scope == ARCHIVE_SCOPE || !scope.starts_with("derived:") {
            return Err(IndexError::InvalidQuery);
        }
        validate_field(source_revision)?;
        if lifetime.is_zero()
            || lifetime > MAX_LIFETIME
            || !is_digest(source_revision)
            || input.content != source_revision
            || !is_digest(&input.files)
            || !is_digest(&input.structure)
            || expected_generation.is_some_and(|v| v < 1)
        {
            return Err(IndexError::InvalidQuery);
        }
        let lease = security
            .require_unlocked()
            .map_err(|_| IndexError::Revoked)?;
        // Root grants come exclusively from the state authority, never a
        // producer's asserted root ID.
        authority
            .with_approved_document_roots(&[root_id.to_owned()], || Ok::<_, IndexError>(()))?;
        let current = self.graph_generation(scope)?;
        if current.as_ref().map(|g| g.generation) != expected_generation {
            return Err(IndexError::StaleGeneration);
        }
        // Finish all potentially expensive issuance checks before the short
        // capability lifetime begins. On Windows, root identity pinning can
        // exceed a millisecond even when the grant remains valid.
        if verify_revision()? != source_revision {
            return Err(IndexError::Revoked);
        }
        security
            .validate_lease(lease)
            .map_err(|_| IndexError::Revoked)?;
        let job = GraphMaintenanceJob {
            index: self,
            authority,
            security,
            lease,
            scope: scope.into(),
            root_id: root_id.into(),
            source_revision: source_revision.into(),
            input,
            expected_generation,
            expires_at: Instant::now() + lifetime,
            verify_revision: Box::new(verify_revision),
            batches: BTreeMap::new(),
            next_sequence: 0,
            nodes: 0,
            edges: 0,
            cancelled: false,
            committed: None,
        };
        Ok(job)
    }
}

fn is_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl GraphMaintenanceJob<'_> {
    pub fn scope(&self) -> &str {
        &self.scope
    }

    pub fn next_sequence(&self) -> u64 {
        self.next_sequence
    }

    fn check_time_and_lease(&self) -> Result<()> {
        if self.cancelled || Instant::now() >= self.expires_at {
            return Err(IndexError::Revoked);
        }
        self.security
            .validate_lease(self.lease)
            .map_err(|_| IndexError::Revoked)
    }

    fn check_authority(&mut self) -> Result<()> {
        self.check_time_and_lease()?;
        self.authority
            .with_approved_document_roots(&[self.root_id.clone()], || Ok::<_, IndexError>(()))?;
        if (self.verify_revision)()? != self.source_revision {
            return Err(IndexError::Revoked);
        }
        self.check_time_and_lease()
    }

    fn check_evidence(&self, reference: &str) -> Result<()> {
        let path =
            VirtualDocumentPath::parse(reference).map_err(|_| IndexError::InvalidEvidence)?;
        if path.root_id() != self.root_id {
            return Err(IndexError::Revoked);
        }
        Ok(())
    }

    /// Sequence 0,1,... only. Exact identical retries are acknowledged without
    /// adding data. A conflicting retry, skipped sequence or oversized batch
    /// is rejected before mutating staged content.
    pub fn accept_batch(&mut self, batch: GraphMaintenanceBatch) -> Result<GraphBatchReceipt> {
        self.check_authority()?;
        if self.committed.is_some() {
            return Err(IndexError::StaleGeneration);
        }
        if batch.sequence < self.next_sequence {
            if self.batches.get(&batch.sequence) == Some(&batch) {
                return Ok(GraphBatchReceipt {
                    sequence: batch.sequence,
                    next_sequence: self.next_sequence,
                    replayed: true,
                });
            }
            return Err(IndexError::InvalidQuery);
        }
        if batch.sequence != self.next_sequence
            || batch.nodes.len() > MAX_BATCH
            || batch.edges.len() > MAX_BATCH
            || (batch.nodes.is_empty() && batch.edges.is_empty())
            || self
                .nodes
                .checked_add(batch.nodes.len())
                .is_none_or(|n| n > MAX_ITEMS)
            || self
                .edges
                .checked_add(batch.edges.len())
                .is_none_or(|n| n > MAX_ITEMS)
        {
            return Err(IndexError::InvalidQuery);
        }
        for node in &batch.nodes {
            validate_field(&node.identity.key)?;
            validate_field(&node.label)?;
            self.check_evidence(&node.source_ref)?;
        }
        for edge in &batch.edges {
            validate_field(&edge.from.key)?;
            validate_field(&edge.to.key)?;
            self.check_evidence(&edge.evidence.source_ref)?;
            validate_field(&edge.evidence.locator)?;
            if edge.evidence.source_revision != self.source_revision {
                return Err(IndexError::Revoked);
            }
            if let GraphOrigin::Inferred { confidence_bps } = edge.origin {
                if !(8_000..=10_000).contains(&confidence_bps)
                    || !matches!(
                        edge.relation,
                        GraphRelation::Contains | GraphRelation::References
                    )
                {
                    return Err(IndexError::InvalidEvidence);
                }
            }
        }
        self.nodes += batch.nodes.len();
        self.edges += batch.edges.len();
        let seq = self.next_sequence;
        self.batches.insert(seq, batch);
        self.next_sequence += 1;
        Ok(GraphBatchReceipt {
            sequence: seq,
            next_sequence: self.next_sequence,
            replayed: false,
        })
    }

    /// Publication is one generation-fenced SQLite transaction. The short
    /// state authority publication fence prevents a completed root revocation
    /// from overtaking this commit. All intermediate batches remain invisible.
    /// A repeated finish on the same live capability returns the same receipt.
    pub fn finish(&mut self) -> Result<GraphActivation> {
        self.check_authority()?;
        if let Some(result) = &self.committed {
            return Ok(result.clone());
        }
        if self.batches.is_empty() {
            return Err(IndexError::InvalidQuery);
        }
        let mut projection = GraphProjection::default();
        for batch in self.batches.values() {
            projection.nodes.extend(batch.nodes.iter().cloned());
            projection.edges.extend(batch.edges.iter().cloned());
        }
        // Check the source one more time before entering the cross-DB fence:
        // callback work is forbidden while holding the authority SQLite lock.
        self.check_authority()?;
        let roots = [self.root_id.clone()];
        let result = self.authority.with_approved_document_roots(&roots, || {
            self.index.activate_graph_generation_with_check(
                &self.scope,
                &self.input,
                &projection,
                self.expected_generation,
                || self.check_time_and_lease(),
            )
        })?;
        self.committed = Some(result.clone());
        self.batches.clear(); // release bulk graph data after publication
        Ok(result)
    }

    pub fn cancel(&mut self) {
        self.cancelled = true;
        self.batches.clear();
    }
}

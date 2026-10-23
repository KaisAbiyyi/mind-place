-- MP-6E: replace only the disposable derived graph edge table.
-- Existing deterministic edges are preserved; canonical state/archive never change.
CREATE TABLE graph_edges_v4 (
    id TEXT PRIMARY KEY,
    scope TEXT NOT NULL,
    from_node TEXT NOT NULL REFERENCES graph_nodes(id) ON DELETE CASCADE,
    to_node TEXT NOT NULL REFERENCES graph_nodes(id) ON DELETE CASCADE,
    relation TEXT NOT NULL CHECK(relation IN ('contains','references','version','imported_from','defines_symbol')),
    origin TEXT NOT NULL CHECK(origin IN ('deterministic','inferred')),
    confidence_bps INTEGER CHECK(confidence_bps BETWEEN 0 AND 10000),
    source_ref TEXT NOT NULL CHECK(length(source_ref) > 0),
    source_revision TEXT NOT NULL,
    locator TEXT NOT NULL CHECK(length(locator) > 0),
    CHECK ((origin='deterministic' AND confidence_bps IS NULL)
        OR (origin='inferred' AND confidence_bps IS NOT NULL))
) STRICT;
INSERT INTO graph_edges_v4(id,scope,from_node,to_node,relation,origin,confidence_bps,source_ref,source_revision,locator)
SELECT id,scope,from_node,to_node,relation,'deterministic',NULL,source_ref,source_revision,locator
FROM graph_edges;
DROP TABLE graph_edges;
ALTER TABLE graph_edges_v4 RENAME TO graph_edges;
CREATE INDEX graph_edges_scope ON graph_edges(scope, id);
CREATE INDEX graph_edges_outbound ON graph_edges(from_node, relation, id);
CREATE INDEX graph_edges_inbound ON graph_edges(to_node, relation, id);

-- MP-6A: derived-only graph projection, never canonical state.
-- Every node is owned by one or more replaceable scopes.
CREATE TABLE graph_nodes (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK(kind IN ('project','topic','file','conversation','memory','entity','revision','import','symbol')),
    canonical_key TEXT NOT NULL,
    label TEXT NOT NULL,
    source_ref TEXT NOT NULL,
    UNIQUE(kind, canonical_key)
) STRICT;

CREATE TABLE graph_scope_nodes (
    scope TEXT NOT NULL,
    node_id TEXT NOT NULL REFERENCES graph_nodes(id) ON DELETE CASCADE,
    PRIMARY KEY(scope, node_id)
) WITHOUT ROWID;

CREATE INDEX graph_scope_nodes_node ON graph_scope_nodes(node_id);

CREATE TABLE graph_edges (
    id TEXT PRIMARY KEY,
    scope TEXT NOT NULL,
    from_node TEXT NOT NULL REFERENCES graph_nodes(id) ON DELETE CASCADE,
    to_node TEXT NOT NULL REFERENCES graph_nodes(id) ON DELETE CASCADE,
    relation TEXT NOT NULL CHECK(relation IN ('contains','references','version','imported_from','defines_symbol')),
    origin TEXT NOT NULL DEFAULT 'deterministic' CHECK(origin = 'deterministic'),
    source_ref TEXT NOT NULL CHECK(length(source_ref) > 0),
    source_revision TEXT NOT NULL,
    locator TEXT NOT NULL CHECK(length(locator) > 0)
) STRICT;

CREATE INDEX graph_edges_scope ON graph_edges(scope, id);
CREATE INDEX graph_edges_outbound ON graph_edges(from_node, relation, id);
CREATE INDEX graph_edges_inbound ON graph_edges(to_node, relation, id);

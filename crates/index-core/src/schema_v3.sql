-- MP-6C: versioned, non-authoritative graph generation manifest.
-- A tombstone (active=0) fences in-flight builds after source revocation.
CREATE TABLE graph_generations (
    scope TEXT PRIMARY KEY,
    generation INTEGER NOT NULL CHECK(generation > 0),
    active INTEGER NOT NULL CHECK(active IN (0,1)),
    files_hash TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    structure_hash TEXT NOT NULL,
    projection_hash TEXT NOT NULL,
    nodes INTEGER NOT NULL CHECK(nodes >= 0),
    edges INTEGER NOT NULL CHECK(edges >= 0)
) STRICT;

-- MP-4A: explicit opt-in document namespace, separate from legacy source_roots.
-- Do not automatically adopt historical pending/approved roots as document grants.
CREATE TABLE document_root_registry (
  root_id TEXT PRIMARY KEY REFERENCES source_roots(id) ON DELETE RESTRICT,
  identity_version INTEGER NOT NULL CHECK(identity_version=1),
  path_digest TEXT NOT NULL UNIQUE
    CHECK(length(path_digest)=64 AND path_digest NOT GLOB '*[^0-9a-f]*'),
  created_at INTEGER NOT NULL DEFAULT (unixepoch()),
  CHECK(root_id='root:v1:' || path_digest)
) STRICT;

-- Bindings are immutable; revocation occurs on source_roots and emits a
-- durable append-only root_tombstones entry through StateStore::revoke_root.
CREATE TRIGGER document_root_registry_no_update
BEFORE UPDATE ON document_root_registry
BEGIN SELECT RAISE(ABORT,'immutable document root identity'); END;
CREATE TRIGGER document_root_registry_no_delete
BEFORE DELETE ON document_root_registry
BEGIN SELECT RAISE(ABORT,'immutable document root identity'); END;

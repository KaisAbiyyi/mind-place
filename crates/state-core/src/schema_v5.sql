-- MP-4B: explicit file-object binding. No migration grants existing roots.
CREATE TABLE document_root_authorizations (
  root_id TEXT PRIMARY KEY REFERENCES document_root_registry(root_id) ON DELETE RESTRICT,
  volume_id TEXT NOT NULL CHECK(length(volume_id) BETWEEN 1 AND 20),
  file_id TEXT NOT NULL CHECK(length(file_id) BETWEEN 1 AND 20),
  approved_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
CREATE TRIGGER document_root_authorizations_no_update
BEFORE UPDATE ON document_root_authorizations
BEGIN SELECT RAISE(ABORT,'immutable root grant'); END;
CREATE TRIGGER document_root_authorizations_no_delete
BEFORE DELETE ON document_root_authorizations
BEGIN SELECT RAISE(ABORT,'immutable root grant'); END;

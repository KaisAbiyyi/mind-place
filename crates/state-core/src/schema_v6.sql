-- MP-4D: raw evidence authority. No grants, retention deletes or index data.
CREATE TABLE document_objects (
  content_hash TEXT PRIMARY KEY CHECK(length(content_hash)=64 AND content_hash NOT GLOB '*[^0-9a-f]*'),
  format_version INTEGER NOT NULL CHECK(format_version=1),
  byte_length INTEGER NOT NULL CHECK(byte_length BETWEEN 0 AND 67108864),
  created_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
CREATE TABLE documents (
  uri TEXT PRIMARY KEY CHECK(length(uri) BETWEEN 1 AND 4096),
  root_id TEXT NOT NULL REFERENCES document_root_registry(root_id) ON DELETE RESTRICT,
  created_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
CREATE TABLE document_versions (
  id TEXT PRIMARY KEY,
  uri TEXT NOT NULL REFERENCES documents(uri) ON DELETE RESTRICT,
  content_hash TEXT NOT NULL REFERENCES document_objects(content_hash) ON DELETE RESTRICT,
  first_metadata_json TEXT NOT NULL CHECK(json_valid(first_metadata_json)),
  first_seen INTEGER NOT NULL DEFAULT (unixepoch()),
  UNIQUE(uri,content_hash),
  UNIQUE(uri,id)
) STRICT;
CREATE TABLE document_heads (
  uri TEXT PRIMARY KEY REFERENCES documents(uri) ON DELETE RESTRICT,
  version_id TEXT NOT NULL,
  metadata_json TEXT NOT NULL CHECK(json_valid(metadata_json)),
  last_seen INTEGER NOT NULL DEFAULT (unixepoch()),
  FOREIGN KEY(uri,version_id) REFERENCES document_versions(uri,id) ON DELETE RESTRICT
) STRICT;
CREATE TRIGGER document_objects_no_update BEFORE UPDATE ON document_objects
BEGIN SELECT RAISE(ABORT,'immutable document object'); END;
CREATE TRIGGER document_objects_no_delete BEFORE DELETE ON document_objects
BEGIN SELECT RAISE(ABORT,'immutable document object'); END;
CREATE TRIGGER documents_no_update BEFORE UPDATE ON documents
BEGIN SELECT RAISE(ABORT,'immutable document identity'); END;
CREATE TRIGGER documents_no_delete BEFORE DELETE ON documents
BEGIN SELECT RAISE(ABORT,'immutable document identity'); END;
CREATE TRIGGER document_versions_no_update BEFORE UPDATE ON document_versions
BEGIN SELECT RAISE(ABORT,'immutable document version'); END;
CREATE TRIGGER document_versions_no_delete BEFORE DELETE ON document_versions
BEGIN SELECT RAISE(ABORT,'immutable document version'); END;

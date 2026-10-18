-- Phase 3B: credentials never live in the disposable index or the wire response.
-- Legacy V1 clients are deliberately *not* credentialed by this migration.
CREATE TABLE client_credentials (
  client_id TEXT PRIMARY KEY REFERENCES clients(id) ON DELETE RESTRICT,
  token_sha256 TEXT NOT NULL UNIQUE
    CHECK(length(token_sha256)=64 AND token_sha256 NOT GLOB '*[^0-9a-f]*'),
  revision INTEGER NOT NULL DEFAULT 1 CHECK(revision>0),
  issued_at INTEGER NOT NULL DEFAULT (unixepoch()),
  rotated_at INTEGER
) STRICT;

-- Metadata-only, append-only local authorization and administration audit.
-- Never store token material, request bodies, searches, source URLs or snippets.
CREATE TABLE client_audit (
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  client_id TEXT REFERENCES clients(id) ON DELETE RESTRICT,
  event TEXT NOT NULL CHECK(event IN (
    'issued','enabled','disabled','revoked','rotated',
    'capability_granted','capability_revoked',
    'read_attempt','read_completed','read_denied','read_failed'
  )),
  capability TEXT CHECK(capability IN ('read','write','admin')),
  policy_revision INTEGER NOT NULL CHECK(policy_revision>=0),
  recorded_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
CREATE INDEX idx_client_audit_client_seq ON client_audit(client_id,seq);
CREATE TRIGGER client_audit_immutable_update BEFORE UPDATE ON client_audit
BEGIN SELECT RAISE(ABORT,'immutable client audit'); END;
CREATE TRIGGER client_audit_immutable_delete BEFORE DELETE ON client_audit
BEGIN SELECT RAISE(ABORT,'immutable client audit'); END;

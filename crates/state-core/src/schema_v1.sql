-- Mind Place state authority v1. The raw archive and derived search index are separate.
CREATE TABLE schema_migrations (
  version INTEGER PRIMARY KEY,
  checksum TEXT NOT NULL,
  applied_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
CREATE TABLE projects (
  id TEXT PRIMARY KEY, name TEXT NOT NULL CHECK(length(trim(name))>0),
  status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','archived')),
  created_at INTEGER NOT NULL DEFAULT (unixepoch()),
  updated_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
CREATE TABLE topics (
  id TEXT PRIMARY KEY, name TEXT NOT NULL,
  created_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
CREATE TABLE scopes (
  id TEXT PRIMARY KEY, name TEXT NOT NULL,
  parent_scope_id TEXT REFERENCES scopes(id) ON DELETE RESTRICT,
  created_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
CREATE TABLE tags (id TEXT PRIMARY KEY, name TEXT NOT NULL) STRICT;
CREATE TABLE project_topics (
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
  topic_id TEXT NOT NULL REFERENCES topics(id) ON DELETE RESTRICT,
  PRIMARY KEY(project_id,topic_id)
) STRICT;
-- Pending means NOT approved. Only Phase 4 can implement actual root grants.
CREATE TABLE source_roots (
  id TEXT PRIMARY KEY, canonical_path TEXT NOT NULL,
  approval_state TEXT NOT NULL DEFAULT 'pending'
    CHECK(approval_state IN ('pending','approved','revoked')),
  retention_policy TEXT NOT NULL DEFAULT 'retain_raw'
    CHECK(retention_policy IN ('retain_raw','forget_on_revoke')),
  approved_at INTEGER, revoked_at INTEGER,
  created_at INTEGER NOT NULL DEFAULT (unixepoch()),
  CHECK(approval_state!='approved' OR approved_at IS NOT NULL),
  CHECK(approval_state!='revoked' OR revoked_at IS NOT NULL)
) STRICT;
CREATE TABLE root_tombstones (
  id INTEGER PRIMARY KEY, root_id TEXT NOT NULL REFERENCES source_roots(id),
  revoked_at INTEGER NOT NULL DEFAULT (unixepoch()),
  reason TEXT NOT NULL DEFAULT 'user_revocation'
) STRICT;
CREATE INDEX idx_root_tombstones_root ON root_tombstones(root_id);
CREATE TABLE memory_claims (
  id TEXT PRIMARY KEY, statement TEXT NOT NULL,
  scope_id TEXT REFERENCES scopes(id) ON DELETE RESTRICT,
  asserted_by TEXT NOT NULL CHECK(asserted_by IN ('user','model','import','deterministic')),
  epistemic_origin TEXT NOT NULL CHECK(epistemic_origin IN ('explicit','observed','inferred','summarized')),
  status TEXT NOT NULL CHECK(status IN ('active','superseded','disputed','uncertain','retracted','historical')),
  confidence REAL CHECK(confidence BETWEEN 0.0 AND 1.0),
  valid_from INTEGER, valid_until INTEGER,
  created_at INTEGER NOT NULL DEFAULT (unixepoch()),
  CHECK(valid_until IS NULL OR valid_from IS NULL OR valid_until>=valid_from)
) STRICT;
CREATE INDEX idx_claim_scope_status ON memory_claims(scope_id,status);
CREATE TABLE memory_claim_events (
  claim_id TEXT NOT NULL REFERENCES memory_claims(id) ON DELETE RESTRICT,
  revision INTEGER NOT NULL CHECK(revision>0),
  event_type TEXT NOT NULL,
  payload_json TEXT NOT NULL CHECK(json_valid(payload_json)),
  recorded_at INTEGER NOT NULL DEFAULT (unixepoch()),
  PRIMARY KEY(claim_id,revision)
) STRICT;
-- No silent edits to provenance history. Phase 5 will add transition APIs.
CREATE TRIGGER claim_events_immutable_update BEFORE UPDATE ON memory_claim_events
BEGIN SELECT RAISE(ABORT,'immutable claim event'); END;
CREATE TRIGGER claim_events_immutable_delete BEFORE DELETE ON memory_claim_events
BEGIN SELECT RAISE(ABORT,'immutable claim event'); END;
CREATE TABLE claim_evidence (
  claim_id TEXT NOT NULL REFERENCES memory_claims(id) ON DELETE RESTRICT,
  source_ref TEXT NOT NULL, evidence_kind TEXT NOT NULL DEFAULT 'support',
  PRIMARY KEY(claim_id,source_ref,evidence_kind)
) STRICT;
CREATE TABLE clients (
  id TEXT PRIMARY KEY, display_name TEXT NOT NULL,
  state TEXT NOT NULL DEFAULT 'disabled' CHECK(state IN ('disabled','enabled','revoked')),
  policy_revision INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL DEFAULT (unixepoch()),
  revoked_at INTEGER
) STRICT;
CREATE TABLE client_capabilities (
  client_id TEXT NOT NULL REFERENCES clients(id) ON DELETE RESTRICT,
  capability TEXT NOT NULL,
  granted_at INTEGER NOT NULL DEFAULT (unixepoch()),
  revoked_at INTEGER,
  PRIMARY KEY(client_id,capability)
) STRICT;
CREATE TABLE security_policy (
  id INTEGER PRIMARY KEY CHECK(id=1),
  revision INTEGER NOT NULL DEFAULT 0,
  client_revocation_generation INTEGER NOT NULL DEFAULT 0
) STRICT;
INSERT INTO security_policy(id) VALUES (1);
CREATE TABLE maintenance_jobs (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL, scope_ref TEXT NOT NULL,
  state TEXT NOT NULL CHECK(state IN ('queued','running','completed','failed','cancelled')),
  input_revision TEXT,
  created_at INTEGER NOT NULL DEFAULT (unixepoch()),
  updated_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
CREATE TABLE settings (
  key TEXT PRIMARY KEY, value_json TEXT NOT NULL CHECK(json_valid(value_json)),
  updated_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

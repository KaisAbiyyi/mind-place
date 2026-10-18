-- Phase 1C: canonical identities belong to state, never to the rebuildable index.
-- A source key is either a provider's stable external identifier (scoped by an
-- explicit namespace) or the SHA-256 of an immutable raw object. A content hash
-- must not be used as the identity of a mutable conversation or file path.
CREATE TABLE source_identities (
  id TEXT PRIMARY KEY
    CHECK(length(id)=71 AND substr(id,1,7)='src:v1:'
      AND substr(id,8) NOT GLOB '*[^0-9a-f]*'),
  provider TEXT NOT NULL CHECK(length(provider) BETWEEN 1 AND 64),
  source_namespace TEXT NOT NULL CHECK(length(source_namespace) BETWEEN 1 AND 256),
  source_kind TEXT NOT NULL CHECK(length(source_kind) BETWEEN 1 AND 64),
  key_type TEXT NOT NULL CHECK(key_type IN ('external','raw_sha256')),
  source_key TEXT NOT NULL CHECK(length(source_key) BETWEEN 1 AND 2048),
  created_at INTEGER NOT NULL DEFAULT (unixepoch()),
  UNIQUE(provider,source_namespace,source_kind,key_type,source_key)
) STRICT;

-- V1 accepted unbound text refs (and had no public writer API). We must not
-- silently adopt those strings as canonical identities. Existing nonempty
-- unbound rows fail the FK constraint, rolling back this entire migration;
-- the verified pre-upgrade backup is left available for explicit recovery.
ALTER TABLE claim_evidence RENAME TO claim_evidence_v1;
CREATE TABLE claim_evidence (
  claim_id TEXT NOT NULL REFERENCES memory_claims(id) ON DELETE RESTRICT,
  source_ref TEXT NOT NULL REFERENCES source_identities(id) ON DELETE RESTRICT,
  evidence_kind TEXT NOT NULL DEFAULT 'support',
  PRIMARY KEY(claim_id,source_ref,evidence_kind)
) STRICT;
INSERT INTO claim_evidence (claim_id,source_ref,evidence_kind)
  SELECT claim_id,source_ref,evidence_kind FROM claim_evidence_v1;
DROP TABLE claim_evidence_v1;
CREATE INDEX idx_claim_evidence_source ON claim_evidence(source_ref);

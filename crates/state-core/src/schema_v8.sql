-- MP-5B: durable, source-linked directional claim relations.
-- A -> B means A supports/contradicts/supersedes/refines B. Evidence is
-- referenced by stable authority identity, never transient index rowids.
CREATE TABLE memory_claim_relations (
  from_claim_id TEXT NOT NULL REFERENCES memory_claims(id) ON DELETE RESTRICT,
  to_claim_id TEXT NOT NULL REFERENCES memory_claims(id) ON DELETE RESTRICT,
  relation TEXT NOT NULL CHECK(relation IN ('supports','contradicts','supersedes','refines')),
  source_ref TEXT NOT NULL REFERENCES source_identities(id) ON DELETE RESTRICT,
  reason TEXT NOT NULL CHECK(length(trim(reason)) BETWEEN 1 AND 4096),
  recorded_at_ms INTEGER NOT NULL CHECK(recorded_at_ms > 0),
  PRIMARY KEY(from_claim_id,to_claim_id,relation),
  CHECK(from_claim_id <> to_claim_id)
) STRICT;
CREATE INDEX idx_memory_relation_target ON memory_claim_relations(to_claim_id,recorded_at_ms);
CREATE INDEX idx_memory_relation_clock ON memory_claim_relations(recorded_at_ms);
CREATE TRIGGER memory_relation_immutable_update BEFORE UPDATE ON memory_claim_relations
BEGIN SELECT RAISE(ABORT,'immutable claim relation'); END;
CREATE TRIGGER memory_relation_immutable_delete BEFORE DELETE ON memory_claim_relations
BEGIN SELECT RAISE(ABORT,'immutable claim relation'); END;

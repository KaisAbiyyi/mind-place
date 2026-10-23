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

-- Correct erroneous relation evidence by appending a resolution record, never
-- rewriting or deleting the original provenance. A resolved edge is omitted
-- from current ranking but still exists in historical as-of snapshots.
CREATE TABLE memory_claim_relation_resolutions (
  from_claim_id TEXT NOT NULL,
  to_claim_id TEXT NOT NULL,
  relation TEXT NOT NULL,
  source_ref TEXT NOT NULL REFERENCES source_identities(id) ON DELETE RESTRICT,
  reason TEXT NOT NULL CHECK(length(trim(reason)) BETWEEN 1 AND 4096),
  recorded_at_ms INTEGER NOT NULL CHECK(recorded_at_ms > 0),
  PRIMARY KEY(from_claim_id,to_claim_id,relation),
  FOREIGN KEY(from_claim_id,to_claim_id,relation)
    REFERENCES memory_claim_relations(from_claim_id,to_claim_id,relation)
    ON DELETE RESTRICT
) STRICT;
CREATE INDEX idx_memory_relation_resolutions_clock
  ON memory_claim_relation_resolutions(recorded_at_ms);
CREATE TRIGGER memory_relation_resolutions_immutable_update
BEFORE UPDATE ON memory_claim_relation_resolutions
BEGIN SELECT RAISE(ABORT,'immutable claim relation resolution'); END;
CREATE TRIGGER memory_relation_resolutions_immutable_delete
BEFORE DELETE ON memory_claim_relation_resolutions
BEGIN SELECT RAISE(ABORT,'immutable claim relation resolution'); END;

-- Every source-backed claim event and relation write shares a single durable
-- monotonic millisecond clock. SQLite IMMEDIATE write transactions serialize
-- allocation, including across independent StateStore handles. Without this,
-- separate claims and edges written in the same wall millisecond can make a
-- previously queried inclusive "as-of" snapshot retroactively change.
CREATE TABLE memory_claim_clock (
  id INTEGER PRIMARY KEY CHECK(id=1),
  last_ms INTEGER NOT NULL CHECK(last_ms >= 0)
) STRICT;
INSERT INTO memory_claim_clock(id,last_ms)
SELECT 1, COALESCE(MAX(COALESCE(recorded_at_ms,recorded_at*1000)),0)
FROM memory_claim_events;

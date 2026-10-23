-- MP-5A: durable event clocks and revision-scoped authoritative evidence.
-- Existing v1-v6 events have no millisecond clock; readers use recorded_at*1000.
-- Do not rewrite pre-existing immutable events during migration.
ALTER TABLE memory_claim_events ADD COLUMN recorded_at_ms INTEGER;
CREATE UNIQUE INDEX idx_claim_event_clock
  ON memory_claim_events(claim_id, recorded_at_ms)
  WHERE recorded_at_ms IS NOT NULL;
CREATE TABLE memory_claim_event_evidence (
  claim_id TEXT NOT NULL,
  revision INTEGER NOT NULL CHECK(revision>0),
  source_ref TEXT NOT NULL REFERENCES source_identities(id) ON DELETE RESTRICT,
  evidence_kind TEXT NOT NULL CHECK(evidence_kind IN ('support','correction','contradiction','retraction','observation')),
  PRIMARY KEY(claim_id,revision,source_ref,evidence_kind),
  FOREIGN KEY(claim_id,revision) REFERENCES memory_claim_events(claim_id,revision) ON DELETE RESTRICT
) STRICT;
CREATE INDEX idx_claim_event_evidence_source ON memory_claim_event_evidence(source_ref);
CREATE TRIGGER claim_event_evidence_immutable_update
BEFORE UPDATE ON memory_claim_event_evidence
BEGIN SELECT RAISE(ABORT,'immutable claim event evidence'); END;
CREATE TRIGGER claim_event_evidence_immutable_delete
BEFORE DELETE ON memory_claim_event_evidence
BEGIN SELECT RAISE(ABORT,'immutable claim event evidence'); END;
-- The claim's statement, authorship, scope and origin are immutable. The status
-- column remains a transactional projection; the events are authoritative.
CREATE TRIGGER claim_identity_immutable_update
BEFORE UPDATE ON memory_claims
WHEN NEW.id != OLD.id OR NEW.statement != OLD.statement
  OR NEW.asserted_by != OLD.asserted_by
  OR NEW.epistemic_origin != OLD.epistemic_origin
  OR NEW.scope_id IS NOT OLD.scope_id
  OR NEW.created_at != OLD.created_at
  OR NEW.confidence IS NOT OLD.confidence
  OR NEW.valid_from IS NOT OLD.valid_from
  OR NEW.valid_until IS NOT OLD.valid_until
BEGIN SELECT RAISE(ABORT,'immutable claim identity'); END;

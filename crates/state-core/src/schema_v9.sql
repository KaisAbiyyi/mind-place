-- MP-5C: bounded, authenticated idempotent memory mutations.
-- Forget is a permanent non-disclosure tombstone of curated memory, NOT an
-- erasure of immutable raw archives. Physical erasure is reserved for MP-9C.
CREATE TABLE memory_write_receipts (
  client_id TEXT NOT NULL REFERENCES clients(id) ON DELETE RESTRICT,
  request_key TEXT NOT NULL CHECK(length(request_key) BETWEEN 16 AND 128),
  operation_hash TEXT NOT NULL CHECK(length(operation_hash)=64),
  operation_kind TEXT NOT NULL CHECK(operation_kind IN ('create','supersede','forget')),
  claim_id TEXT NOT NULL REFERENCES memory_claims(id) ON DELETE RESTRICT,
  revision INTEGER NOT NULL CHECK(revision>=1),
  recorded_at_ms INTEGER NOT NULL CHECK(recorded_at_ms>0),
  PRIMARY KEY(client_id,request_key)
) STRICT;
CREATE INDEX idx_memory_write_receipts_claim ON memory_write_receipts(claim_id);
CREATE TRIGGER memory_write_receipt_no_update BEFORE UPDATE ON memory_write_receipts
BEGIN SELECT RAISE(ABORT,'immutable memory write receipt'); END;
CREATE TRIGGER memory_write_receipt_no_delete BEFORE DELETE ON memory_write_receipts
BEGIN SELECT RAISE(ABORT,'immutable memory write receipt'); END;

CREATE TABLE memory_forget_tombstones (
  claim_id TEXT PRIMARY KEY REFERENCES memory_claims(id) ON DELETE RESTRICT,
  client_id TEXT NOT NULL REFERENCES clients(id) ON DELETE RESTRICT,
  request_key TEXT NOT NULL,
  reason TEXT NOT NULL CHECK(length(trim(reason)) BETWEEN 1 AND 4096),
  recorded_at_ms INTEGER NOT NULL CHECK(recorded_at_ms>0),
  FOREIGN KEY(client_id,request_key) REFERENCES memory_write_receipts(client_id,request_key)
    DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TRIGGER memory_forget_no_update BEFORE UPDATE ON memory_forget_tombstones
BEGIN SELECT RAISE(ABORT,'immutable forget tombstone'); END;
CREATE TRIGGER memory_forget_no_delete BEFORE DELETE ON memory_forget_tombstones
BEGIN SELECT RAISE(ABORT,'immutable forget tombstone'); END;

-- MP-R1-WP03: Topic model as primary scope, many-to-many membership with provenance,
-- and CNF v1 curated node/edge/revision/evidence/generation authority store.

-- 1. Extend existing topics and projects tables without creating duplicate models.
-- A project is a topic carrying work state (`is_project = 1`, `work_status` non-null).
ALTER TABLE topics ADD COLUMN slug TEXT;
ALTER TABLE topics ADD COLUMN parent_topic_id TEXT REFERENCES topics(id) ON DELETE RESTRICT;
ALTER TABLE topics ADD COLUMN is_project INTEGER NOT NULL DEFAULT 0 CHECK(is_project IN (0,1));
ALTER TABLE topics ADD COLUMN work_status TEXT CHECK(work_status IS NULL OR work_status IN ('active','paused','completed','archived'));
ALTER TABLE topics ADD COLUMN description TEXT;
ALTER TABLE topics ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
UPDATE topics SET updated_at = created_at WHERE updated_at = 0;

ALTER TABLE projects ADD COLUMN topic_id TEXT REFERENCES topics(id) ON DELETE RESTRICT;

CREATE UNIQUE INDEX idx_topics_slug ON topics(slug) WHERE slug IS NOT NULL;
CREATE INDEX idx_topics_parent ON topics(parent_topic_id);
CREATE INDEX idx_topics_project ON topics(is_project, work_status);
CREATE UNIQUE INDEX idx_projects_topic ON projects(topic_id) WHERE topic_id IS NOT NULL;

CREATE TRIGGER topics_validate_insert BEFORE INSERT ON topics
BEGIN
  SELECT RAISE(ABORT, 'invalid topic')
  WHERE length(trim(NEW.name)) = 0
    OR NEW.parent_topic_id = NEW.id
    OR (NEW.slug IS NOT NULL AND (length(NEW.slug) NOT BETWEEN 1 AND 64 OR NEW.slug GLOB '*[^a-z0-9-]*'))
    OR (NEW.is_project = 0 AND NEW.work_status IS NOT NULL)
    OR (NEW.is_project = 1 AND NEW.work_status IS NULL);
END;

CREATE TRIGGER topics_validate_update BEFORE UPDATE ON topics
BEGIN
  SELECT RAISE(ABORT, 'invalid topic')
  WHERE (NEW.name != OLD.name AND length(trim(NEW.name)) = 0)
    OR NEW.parent_topic_id = NEW.id
    OR (NEW.slug IS NOT NULL AND (length(NEW.slug) NOT BETWEEN 1 AND 64 OR NEW.slug GLOB '*[^a-z0-9-]*'))
    OR (NEW.is_project = 0 AND NEW.work_status IS NOT NULL)
    OR (NEW.is_project = 1 AND NEW.work_status IS NULL);
END;

-- Monotonic millisecond clock shared by curated state & topic membership mutations.
CREATE TABLE curated_state_clock (
  id INTEGER PRIMARY KEY CHECK(id=1),
  last_ms INTEGER NOT NULL CHECK(last_ms >= 0)
) STRICT;
INSERT INTO curated_state_clock(id, last_ms) VALUES (1, 0);

-- 2. Staged generations & immutable CNF v1 op log (authoritative history).
CREATE TABLE curation_generations (
  id TEXT PRIMARY KEY,
  topic_id TEXT NOT NULL REFERENCES topics(id) ON DELETE RESTRICT,
  run_id TEXT NOT NULL,
  profile TEXT NOT NULL CHECK(profile IN ('exact','detailed','brief')),
  asserted_by TEXT NOT NULL CHECK(asserted_by IN ('user','model','import','deterministic')),
  model_id TEXT CHECK(model_id IS NULL OR (length(trim(model_id)) BETWEEN 1 AND 128)),
  status TEXT NOT NULL CHECK(status IN ('staged','published','rejected')),
  base_generation_id TEXT REFERENCES curation_generations(id) ON DELETE RESTRICT,
  projection_sha256 TEXT CHECK(projection_sha256 IS NULL OR (length(projection_sha256)=64 AND projection_sha256 NOT GLOB '*[^0-9a-f]*')),
  created_at_ms INTEGER NOT NULL CHECK(created_at_ms > 0),
  published_at_ms INTEGER CHECK(published_at_ms IS NULL OR published_at_ms >= created_at_ms),
  CHECK(status != 'published' OR (published_at_ms IS NOT NULL AND projection_sha256 IS NOT NULL)),
  CHECK(asserted_by != 'model' OR model_id IS NOT NULL),
  CHECK(base_generation_id IS NULL OR base_generation_id <> id)
) STRICT;
CREATE INDEX idx_curation_generations_topic_status
  ON curation_generations(topic_id, status, published_at_ms, id);

CREATE TRIGGER curation_generations_no_delete BEFORE DELETE ON curation_generations
BEGIN SELECT RAISE(ABORT, 'immutable curation generation'); END;

CREATE TRIGGER curation_generations_immutable_update BEFORE UPDATE ON curation_generations
BEGIN
  SELECT RAISE(ABORT, 'invalid curation generation transition')
  WHERE NEW.id != OLD.id
    OR NEW.topic_id != OLD.topic_id
    OR NEW.run_id != OLD.run_id
    OR NEW.profile != OLD.profile
    OR NEW.asserted_by != OLD.asserted_by
    OR NEW.model_id IS NOT OLD.model_id
    OR NEW.base_generation_id IS NOT OLD.base_generation_id
    OR NEW.created_at_ms != OLD.created_at_ms
    OR OLD.status != 'staged'
    OR NEW.status NOT IN ('published','rejected');
END;

CREATE TABLE curated_op_log (
  generation_id TEXT NOT NULL REFERENCES curation_generations(id) ON DELETE RESTRICT,
  shard_id TEXT NOT NULL CHECK(length(trim(shard_id)) BETWEEN 1 AND 64),
  seq INTEGER NOT NULL CHECK(seq BETWEEN 1 AND 2000),
  op TEXT NOT NULL CHECK(op IN ('upsert','patch','link','unlink','supersede','retire')),
  target_key TEXT NOT NULL CHECK(length(target_key) BETWEEN 5 AND 140),
  kind TEXT CHECK(kind IS NULL OR kind IN ('topic','entity','concept','decision','fact','question','task','event','artifact')),
  rel TEXT CHECK(rel IS NULL OR rel IN ('part_of','about','depends_on','supports','contradicts','supersedes','decided_in','mentions','related_to')),
  to_key TEXT CHECK(to_key IS NULL OR (length(to_key) BETWEEN 5 AND 140)),
  supersedes_key TEXT CHECK(supersedes_key IS NULL OR (length(supersedes_key) BETWEEN 5 AND 140)),
  base_rev INTEGER NOT NULL DEFAULT 0 CHECK(base_rev >= 0),
  payload_json TEXT NOT NULL CHECK(json_valid(payload_json) AND length(payload_json) <= 8192),
  record_sha256 TEXT NOT NULL CHECK(length(record_sha256)=64 AND record_sha256 NOT GLOB '*[^0-9a-f]*'),
  recorded_at_ms INTEGER NOT NULL CHECK(recorded_at_ms > 0),
  PRIMARY KEY(generation_id, shard_id, seq),
  CHECK(
    (op IN ('upsert','patch','supersede','retire') AND rel IS NULL AND to_key IS NULL) OR
    (op IN ('link','unlink') AND rel IS NOT NULL AND to_key IS NOT NULL AND kind IS NULL AND target_key <> to_key)
  ),
  CHECK(op != 'upsert' OR kind IS NOT NULL),
  CHECK(op != 'supersede' OR (supersedes_key IS NOT NULL AND supersedes_key <> target_key))
) STRICT;

CREATE TRIGGER curated_op_log_immutable_update BEFORE UPDATE ON curated_op_log
BEGIN SELECT RAISE(ABORT, 'immutable curated op log'); END;
CREATE TRIGGER curated_op_log_immutable_delete BEFORE DELETE ON curated_op_log
BEGIN SELECT RAISE(ABORT, 'immutable curated op log'); END;

CREATE TABLE curated_op_evidence_spans (
  generation_id TEXT NOT NULL,
  shard_id TEXT NOT NULL,
  seq INTEGER NOT NULL,
  ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 0 AND 15),
  source_ref TEXT NOT NULL REFERENCES source_identities(id) ON DELETE RESTRICT,
  msg_index INTEGER NOT NULL CHECK(msg_index >= 0),
  span_start INTEGER NOT NULL CHECK(span_start >= 0),
  span_end INTEGER NOT NULL CHECK(span_end > span_start),
  span_sha TEXT NOT NULL CHECK(
    (length(span_sha)=67 AND substr(span_sha,1,3)='b3:' AND substr(span_sha,4) NOT GLOB '*[^0-9a-f]*') OR
    (length(span_sha)=71 AND substr(span_sha,1,7)='sha256:' AND substr(span_sha,8) NOT GLOB '*[^0-9a-f]*')
  ),
  quote TEXT CHECK(quote IS NULL OR (length(quote) BETWEEN 1 AND 8192)),
  PRIMARY KEY(generation_id, shard_id, seq, ordinal),
  FOREIGN KEY(generation_id, shard_id, seq)
    REFERENCES curated_op_log(generation_id, shard_id, seq) ON DELETE RESTRICT
) STRICT;
CREATE INDEX idx_curated_op_evidence_source ON curated_op_evidence_spans(source_ref);

CREATE TRIGGER curated_op_evidence_immutable_update BEFORE UPDATE ON curated_op_evidence_spans
BEGIN SELECT RAISE(ABORT, 'immutable curated op evidence'); END;
CREATE TRIGGER curated_op_evidence_immutable_delete BEFORE DELETE ON curated_op_evidence_spans
BEGIN SELECT RAISE(ABORT, 'immutable curated op evidence'); END;

-- 3. Materialized accepted curated nodes, aliases, revisions, edges & evidence.
-- Deterministically rebuildable from published `curation_generations` + `curated_op_log`.
CREATE TABLE curated_nodes (
  id TEXT PRIMARY KEY,
  topic_id TEXT NOT NULL REFERENCES topics(id) ON DELETE RESTRICT,
  canonical_key TEXT NOT NULL UNIQUE CHECK(length(canonical_key) BETWEEN 5 AND 140),
  kind TEXT NOT NULL CHECK(kind IN ('topic','entity','concept','decision','fact','question','task','event','artifact')),
  slug TEXT NOT NULL CHECK(length(slug) BETWEEN 1 AND 64 AND slug NOT GLOB '*[^a-z0-9-]*'),
  current_rev INTEGER NOT NULL CHECK(current_rev >= 1),
  title TEXT NOT NULL CHECK(length(trim(title)) >= 1 AND length(title) <= 480),
  summary TEXT NOT NULL CHECK(length(summary) <= 1600),
  body TEXT CHECK(body IS NULL OR length(body) <= 4096),
  attrs_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(attrs_json) AND json_type(attrs_json)='object'),
  status TEXT NOT NULL CHECK(status IN ('active','superseded','retired')),
  supersedes_node_id TEXT REFERENCES curated_nodes(id) ON DELETE RESTRICT,
  superseded_by_node_id TEXT REFERENCES curated_nodes(id) ON DELETE RESTRICT,
  generation_id TEXT NOT NULL REFERENCES curation_generations(id) ON DELETE RESTRICT,
  asserted_by TEXT NOT NULL CHECK(asserted_by IN ('user','model','import','deterministic')),
  model_id TEXT CHECK(model_id IS NULL OR (length(trim(model_id)) BETWEEN 1 AND 128)),
  confidence REAL CHECK(confidence IS NULL OR (confidence BETWEEN 0.0 AND 1.0)),
  created_at_ms INTEGER NOT NULL CHECK(created_at_ms > 0),
  updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms >= created_at_ms),
  CHECK(supersedes_node_id IS NULL OR supersedes_node_id <> id),
  CHECK(superseded_by_node_id IS NULL OR superseded_by_node_id <> id),
  CHECK(status != 'superseded' OR superseded_by_node_id IS NOT NULL)
) STRICT;
CREATE INDEX idx_curated_nodes_topic ON curated_nodes(topic_id, status, kind, slug);

CREATE TABLE curated_node_aliases (
  topic_id TEXT NOT NULL REFERENCES topics(id) ON DELETE RESTRICT,
  alias TEXT NOT NULL CHECK(length(trim(alias)) BETWEEN 1 AND 120),
  node_id TEXT NOT NULL REFERENCES curated_nodes(id) ON DELETE RESTRICT,
  PRIMARY KEY(topic_id, alias)
) STRICT;
CREATE INDEX idx_curated_node_aliases_node ON curated_node_aliases(node_id);

CREATE TABLE curated_node_revisions (
  node_id TEXT NOT NULL REFERENCES curated_nodes(id) ON DELETE RESTRICT,
  rev INTEGER NOT NULL CHECK(rev >= 1),
  base_rev INTEGER NOT NULL CHECK(base_rev >= 0 AND base_rev < rev),
  generation_id TEXT NOT NULL REFERENCES curation_generations(id) ON DELETE RESTRICT,
  op TEXT NOT NULL CHECK(op IN ('upsert','patch','supersede','retire','link')),
  canonical_key TEXT NOT NULL CHECK(length(canonical_key) BETWEEN 5 AND 140),
  kind TEXT NOT NULL CHECK(kind IN ('topic','entity','concept','decision','fact','question','task','event','artifact')),
  title TEXT NOT NULL CHECK(length(trim(title)) >= 1 AND length(title) <= 480),
  summary TEXT NOT NULL CHECK(length(summary) <= 1600),
  body TEXT CHECK(body IS NULL OR length(body) <= 4096),
  attrs_json TEXT NOT NULL CHECK(json_valid(attrs_json) AND json_type(attrs_json)='object'),
  aliases_json TEXT NOT NULL DEFAULT '[]' CHECK(json_valid(aliases_json) AND json_type(aliases_json)='array'),
  status TEXT NOT NULL CHECK(status IN ('active','superseded','retired')),
  supersedes_node_id TEXT REFERENCES curated_nodes(id) ON DELETE RESTRICT,
  superseded_by_node_id TEXT REFERENCES curated_nodes(id) ON DELETE RESTRICT,
  asserted_by TEXT NOT NULL CHECK(asserted_by IN ('user','model','import','deterministic')),
  model_id TEXT CHECK(model_id IS NULL OR (length(trim(model_id)) BETWEEN 1 AND 128)),
  confidence REAL CHECK(confidence IS NULL OR (confidence BETWEEN 0.0 AND 1.0)),
  recorded_at_ms INTEGER NOT NULL CHECK(recorded_at_ms > 0),
  PRIMARY KEY(node_id, rev),
  CHECK(supersedes_node_id IS NULL OR supersedes_node_id <> node_id),
  CHECK(superseded_by_node_id IS NULL OR superseded_by_node_id <> node_id)
) STRICT;
CREATE INDEX idx_curated_node_revisions_gen ON curated_node_revisions(generation_id);

CREATE TABLE curated_node_evidence_spans (
  node_id TEXT NOT NULL,
  rev INTEGER NOT NULL,
  ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 0 AND 15),
  source_ref TEXT NOT NULL REFERENCES source_identities(id) ON DELETE RESTRICT,
  msg_index INTEGER NOT NULL CHECK(msg_index >= 0),
  span_start INTEGER NOT NULL CHECK(span_start >= 0),
  span_end INTEGER NOT NULL CHECK(span_end > span_start),
  span_sha TEXT NOT NULL CHECK(
    (length(span_sha)=67 AND substr(span_sha,1,3)='b3:' AND substr(span_sha,4) NOT GLOB '*[^0-9a-f]*') OR
    (length(span_sha)=71 AND substr(span_sha,1,7)='sha256:' AND substr(span_sha,8) NOT GLOB '*[^0-9a-f]*')
  ),
  quote TEXT CHECK(quote IS NULL OR (length(quote) BETWEEN 1 AND 8192)),
  PRIMARY KEY(node_id, rev, ordinal),
  FOREIGN KEY(node_id, rev) REFERENCES curated_node_revisions(node_id, rev) ON DELETE RESTRICT
) STRICT;
CREATE INDEX idx_curated_node_evidence_source ON curated_node_evidence_spans(source_ref);

CREATE TABLE curated_edges (
  id TEXT PRIMARY KEY,
  topic_id TEXT NOT NULL REFERENCES topics(id) ON DELETE RESTRICT,
  from_node_id TEXT NOT NULL REFERENCES curated_nodes(id) ON DELETE RESTRICT,
  rel TEXT NOT NULL CHECK(rel IN ('part_of','about','depends_on','supports','contradicts','supersedes','decided_in','mentions','related_to')),
  to_node_id TEXT NOT NULL REFERENCES curated_nodes(id) ON DELETE RESTRICT,
  status TEXT NOT NULL CHECK(status IN ('active','unlinked')),
  generation_id TEXT NOT NULL REFERENCES curation_generations(id) ON DELETE RESTRICT,
  asserted_by TEXT NOT NULL CHECK(asserted_by IN ('user','model','import','deterministic')),
  model_id TEXT CHECK(model_id IS NULL OR (length(trim(model_id)) BETWEEN 1 AND 128)),
  confidence REAL CHECK(confidence IS NULL OR (confidence BETWEEN 0.0 AND 1.0)),
  created_at_ms INTEGER NOT NULL CHECK(created_at_ms > 0),
  updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms >= created_at_ms),
  UNIQUE(from_node_id, rel, to_node_id),
  CHECK(from_node_id <> to_node_id)
) STRICT;
CREATE INDEX idx_curated_edges_topic ON curated_edges(topic_id, status, rel);
CREATE INDEX idx_curated_edges_to ON curated_edges(to_node_id, rel, status);

CREATE TABLE curated_edge_evidence_spans (
  edge_id TEXT NOT NULL REFERENCES curated_edges(id) ON DELETE RESTRICT,
  ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 0 AND 15),
  source_ref TEXT NOT NULL REFERENCES source_identities(id) ON DELETE RESTRICT,
  msg_index INTEGER NOT NULL CHECK(msg_index >= 0),
  span_start INTEGER NOT NULL CHECK(span_start >= 0),
  span_end INTEGER NOT NULL CHECK(span_end > span_start),
  span_sha TEXT NOT NULL CHECK(
    (length(span_sha)=67 AND substr(span_sha,1,3)='b3:' AND substr(span_sha,4) NOT GLOB '*[^0-9a-f]*') OR
    (length(span_sha)=71 AND substr(span_sha,1,7)='sha256:' AND substr(span_sha,8) NOT GLOB '*[^0-9a-f]*')
  ),
  quote TEXT CHECK(quote IS NULL OR (length(quote) BETWEEN 1 AND 8192)),
  PRIMARY KEY(edge_id, ordinal)
) STRICT;
CREATE INDEX idx_curated_edge_evidence_source ON curated_edge_evidence_spans(source_ref);

-- 4. Many-to-many topic membership with provenance and typed FK enforcement.
CREATE TABLE topic_memberships (
  id TEXT PRIMARY KEY,
  topic_id TEXT NOT NULL REFERENCES topics(id) ON DELETE RESTRICT,
  member_kind TEXT NOT NULL CHECK(member_kind IN ('source','curated_node','memory_claim','document')),
  member_id TEXT NOT NULL CHECK(length(trim(member_id)) BETWEEN 1 AND 512),
  source_ref TEXT REFERENCES source_identities(id) ON DELETE RESTRICT,
  document_uri TEXT REFERENCES documents(uri) ON DELETE RESTRICT,
  curated_node_id TEXT REFERENCES curated_nodes(id) ON DELETE RESTRICT,
  memory_claim_id TEXT REFERENCES memory_claims(id) ON DELETE RESTRICT,
  role TEXT NOT NULL DEFAULT 'member' CHECK(role IN ('primary','member','related')),
  asserted_by TEXT NOT NULL CHECK(asserted_by IN ('user','model','import','deterministic')),
  epistemic_origin TEXT NOT NULL DEFAULT 'explicit' CHECK(epistemic_origin IN ('explicit','observed','inferred','summarized')),
  evidence_source_ref TEXT REFERENCES source_identities(id) ON DELETE RESTRICT,
  evidence_locator TEXT CHECK(evidence_locator IS NULL OR (length(trim(evidence_locator)) BETWEEN 1 AND 1024)),
  confidence REAL CHECK(confidence IS NULL OR (confidence BETWEEN 0.0 AND 1.0)),
  status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','superseded','retracted')),
  superseded_by TEXT REFERENCES topic_memberships(id) ON DELETE RESTRICT DEFERRABLE INITIALLY DEFERRED,
  valid_from_ms INTEGER NOT NULL CHECK(valid_from_ms > 0),
  valid_until_ms INTEGER CHECK(valid_until_ms IS NULL OR valid_until_ms >= valid_from_ms),
  recorded_at_ms INTEGER NOT NULL CHECK(recorded_at_ms > 0),
  CHECK(
    (member_kind='source' AND source_ref=member_id AND document_uri IS NULL AND curated_node_id IS NULL AND memory_claim_id IS NULL) OR
    (member_kind='document' AND document_uri=member_id AND source_ref IS NULL AND curated_node_id IS NULL AND memory_claim_id IS NULL) OR
    (member_kind='curated_node' AND curated_node_id=member_id AND source_ref IS NULL AND document_uri IS NULL AND memory_claim_id IS NULL) OR
    (member_kind='memory_claim' AND memory_claim_id=member_id AND source_ref IS NULL AND document_uri IS NULL AND curated_node_id IS NULL)
  ),
  CHECK(asserted_by='user' OR evidence_source_ref IS NOT NULL),
  CHECK(superseded_by IS NULL OR status='superseded'),
  CHECK(superseded_by IS NULL OR superseded_by <> id)
) STRICT;

CREATE UNIQUE INDEX idx_topic_memberships_active
  ON topic_memberships(topic_id, member_kind, member_id)
  WHERE status='active';
CREATE INDEX idx_topic_memberships_member
  ON topic_memberships(member_kind, member_id, status);
CREATE INDEX idx_topic_memberships_evidence
  ON topic_memberships(evidence_source_ref);

CREATE TRIGGER topic_memberships_no_delete BEFORE DELETE ON topic_memberships
BEGIN SELECT RAISE(ABORT, 'immutable topic membership'); END;

CREATE TRIGGER topic_memberships_immutable_update BEFORE UPDATE ON topic_memberships
BEGIN
  SELECT RAISE(ABORT, 'immutable topic membership provenance')
  WHERE NEW.id != OLD.id
    OR NEW.topic_id != OLD.topic_id
    OR NEW.member_kind != OLD.member_kind
    OR NEW.member_id != OLD.member_id
    OR NEW.source_ref IS NOT OLD.source_ref
    OR NEW.document_uri IS NOT OLD.document_uri
    OR NEW.curated_node_id IS NOT OLD.curated_node_id
    OR NEW.memory_claim_id IS NOT OLD.memory_claim_id
    OR NEW.role != OLD.role
    OR NEW.asserted_by != OLD.asserted_by
    OR NEW.epistemic_origin != OLD.epistemic_origin
    OR NEW.evidence_source_ref IS NOT OLD.evidence_source_ref
    OR NEW.evidence_locator IS NOT OLD.evidence_locator
    OR NEW.confidence IS NOT OLD.confidence
    OR NEW.valid_from_ms != OLD.valid_from_ms
    OR NEW.recorded_at_ms != OLD.recorded_at_ms
    OR OLD.status != 'active'
    OR NEW.status NOT IN ('superseded','retracted');
END;

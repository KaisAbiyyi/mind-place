# MP-R1-WP03 — Design Note: Topic Model and Curated Node Store (CNF v1)

Status: design note for claimed work package `MP-R1-WP03`. Not an acceptance claim; requires independent read-only review (Sonnet 5.5 High) and owner merge decision.

- **Notion card:** `MP-R1-WP03 · Topic model and curated node store (CNF v1)`
- **Resource locks:** `topic-model`, `memory-state-schema`
- **Gate:** `C, IR`
- **Base SHA:** `acdd19e8cb7169f18d15b7d355028b24d65d82a2` (`main`)
- **Absorbs:** `MP-TOP-01` and the authoritative storage slice of `MP-CUR-01`
- **Out of scope (explicit):** MCP tools (`curation_plan`, `topic_index`, `curation_submit`, `node_patch`) and deterministic archive-span ingest gate (`MP-R1-WP04` / `MP-R1-WP05`).

## 1. Unified Topic & Project Model (Extending Existing Tables)

Existing `schema_v1.sql` already defines `topics`, `projects`, `project_topics`, and `scopes`. To avoid duplicate models while making **topic** the primary organizing scope:

1. **Extend `topics` (additive `ALTER TABLE` in `schema_v10.sql`):**
   - `slug TEXT` (unique non-null lowercase ASCII `[a-z0-9-]{1,64}` via partial unique index `idx_topics_slug`)
   - `parent_topic_id TEXT REFERENCES topics(id) ON DELETE RESTRICT` (acyclic hierarchy; cycle check enforced in Rust + self-parent check at SQLite layer)
   - `is_project INTEGER NOT NULL DEFAULT 0 CHECK(is_project IN (0,1))`
   - `work_status TEXT CHECK(work_status IS NULL OR work_status IN ('active','paused','completed','archived'))` (project = topic with work state)
   - `description TEXT`
   - `updated_at INTEGER NOT NULL DEFAULT 0` (backfilled from `created_at`)
   - Consistency checks/triggers enforcing `parent_topic_id != id` and `(is_project = 0 AND work_status IS NULL) OR (is_project = 1 AND work_status IS NOT NULL)`.
2. **Extend `projects` without breaking existing callers:**
   - Add `topic_id TEXT REFERENCES topics(id) ON DELETE RESTRICT` (unique index on non-null `topic_id`).
   - Forward migration v10 links every existing `projects` row to its unified `topics` row (`is_project = 1`, `work_status` mapped from `projects.status`) and populates `project_topics`, reusing `projects.id` in `topics` when not already taken (or deterministic `topic:project:<id>` if collided) and deriving a unique valid slug.
   - Existing `StateStore::create_project` / `get_project` remain backward-compatible and update the unified `topics` + `projects` + `project_topics` records atomically inside one `Immediate` transaction.
3. **Many-to-many topic membership with provenance (`topic_memberships`):**
   - Supports `member_kind` in `('source', 'curated_node', 'memory_claim', 'document')`.
   - Enforces typed FK columns (`source_ref -> source_identities(id)`, `curated_node_id -> curated_nodes(id)`, `memory_claim_id -> memory_claims(id)`, `document_uri -> documents(uri)`) all `ON DELETE RESTRICT` so `PRAGMA foreign_key_check` enforces referential integrity.
   - Provenance fields: `role` (`primary` | `member` | `related`), `asserted_by` (`user` | `model` | `import` | `deterministic`), `epistemic_origin` (`explicit` | `observed` | `inferred` | `summarized`), `evidence_source_ref -> source_identities(id)`, `evidence_locator`, `confidence` (`0.0..=1.0`), `status` (`active` | `superseded` | `retracted`), `superseded_by -> topic_memberships(id)`, `valid_from`, `valid_until`.
   - Non-user assertions require `evidence_source_ref IS NOT NULL`.
   - Partial unique index `(topic_id, member_kind, member_id) WHERE status='active'`.
   - Immutability triggers prevent deleting or altering provenance identity columns after creation; only lifecycle supersession/retraction transitions are allowed.

## 2. Curated Node Store & Staged Generations (CNF v1)

Implemented in `crates/state-core/src/curated.rs` + `schema_v10.sql`:

1. **CNF v1 invariants and closed sets:**
   - Canonical key: `<topic>/<kind>/<slug>` where `<topic>` and `<slug>` match `[a-z0-9-]{1,64}` and `<topic>` resolves to a row in `topics(slug)`.
   - `kind` closed set: `topic`, `entity`, `concept`, `decision`, `fact`, `question`, `task`, `event`, `artifact`.
   - `rel` closed set: `part_of`, `about`, `depends_on`, `supports`, `contradicts`, `supersedes`, `decided_in`, `mentions`, `related_to`.
   - `op` closed set: `upsert`, `patch`, `link`, `unlink`, `supersede`, `retire`.
   - `profile` closed set: `exact`, `detailed`, `brief`.
   - Bounds (CNF v1 §5 & §8): record <= 8 KiB serialized JSON, `title` <= 120 chars, `summary` <= 400 chars, `body` <= 4 KiB (`detailed`/`exact`), `attrs` flat string/number/bool JSON map with <= 16 keys, 1..=16 `evidence` refs (`src` -> `source_identities(id)`, `msg >= 0`, `span_start >= 0`, `span_end > span_start`, `sha` domain-prefixed `b3:<64 hex>` or `sha256:<64 hex>`, optional `quote` required in `exact` profile), informational `confidence` in `0.0..=1.0`, `base_rev >= 0`.
2. **Staged generations & immutable op log:**
   - `curation_generations`: tracks `id`, `topic_id -> topics(id)`, `run_id`, `profile`, `asserted_by`, `model_id`, `status` (`staged` | `published` | `rejected` | `superseded`), `base_generation_id`, `projection_sha256`, timestamps.
   - `curated_op_log`: immutable append-only log (`BEFORE UPDATE`/`DELETE` abort triggers) keyed by `(generation_id, shard_id, seq)` with `op`, `target_key`, `payload_json`, `record_sha256`, `base_rev`, `supersedes_key`, `asserted_by`, `model_id`, `recorded_at_ms`.
   - `curated_op_evidence_spans`: immutable append-only spans `(generation_id, shard_id, seq, ordinal, source_ref -> source_identities(id), msg_index, span_start, span_end, span_sha, quote)` protected by triggers and FKs.
3. **Accepted curated nodes, revisions, edges & evidence:**
   - `curated_nodes`: materialized current state per accepted node (`id`, `topic_id`, `canonical_key`, `kind`, `slug`, `current_rev`, `title`, `summary`, `body`, `attrs_json`, `status`, `supersedes_node_id`, `superseded_by_node_id`, `generation_id`, `asserted_by`, `model_id`, `confidence`, timestamps).
   - `curated_node_aliases`: `(topic_id, alias, node_id)` unique per topic.
   - `curated_node_revisions`: full per-revision snapshots (`(node_id, rev)` PK, `base_rev`, `generation_id`, `op`, `supersedes_node_id`, `superseded_by_node_id`, `asserted_by`, `model_id`, `confidence`, `recorded_at_ms`).
   - `curated_node_evidence_spans`: `(node_id, rev, ordinal)` PK with FK to `curated_node_revisions(node_id, rev)` and `source_identities(id)`.
   - `curated_edges`: `(id)` PK, `topic_id`, `from_node_id`, `rel`, `to_node_id` (`from_node_id <> to_node_id`), `status` (`active` | `unlinked`), `generation_id`, `asserted_by`, `model_id`, `confidence`, timestamps.
   - `curated_edge_evidence_spans`: `(edge_id, ordinal)` PK with FK to `curated_edges(id)` and `source_identities(id)`.
   - `supersedes` A -> B semantics: newer node B links `supersedes` A, A stays visible with `status = 'superseded'` and `superseded_by_node_id = B`, and both carry evidence.

## 3. Migration, Rebuild, Integrity & Read-Only Markdown Export

- **Forward migration (`SCHEMA_VERSION` 9 -> 10):**
  - Plaintext `StateStore::open` creates a verified `VACUUM INTO` backup in `state-backups/` before running `schema_v10.sql` in one `Immediate` transaction.
  - `StateStore::open_encrypted` retains fail-closed refusal of silent migration on existing encrypted vaults; an explicit `#[cfg(feature = "sqlcipher-preview")] StateStore::migrate_encrypted_with_backup` provides caller-consented encrypted online backup outside the vault root before applying pending migrations.
- **Deterministic rebuild:**
  - `StateStore::rebuild_curated_store` replays all `published` `curation_generations` and their `curated_op_log` + `curated_op_evidence_spans` inside one `Immediate` transaction (keeping `PRAGMA foreign_keys = ON`) to reconstruct `curated_nodes`, `curated_node_aliases`, `curated_node_revisions`, `curated_node_evidence_spans`, `curated_edges`, and `curated_edge_evidence_spans` deterministically, returning a canonical `CuratedStoreDigest`.
  - Raw evidence (`source_identities`, `documents`, `document_objects`, `memory_claims`, `archive-v1`) is never modified.
- **Read-only Markdown export:**
  - `StateStore::export_topic_markdown(topic_id_or_slug)` renders deterministic Markdown strictly from accepted (`published`) nodes and edges (`active`, `superseded`, `retired`), omitting all staged/rejected generations, with sorted nodes, edges, and evidence citations `(src, msg, span, sha)`.

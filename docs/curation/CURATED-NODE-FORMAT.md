# Curated Node Format (CNF v1) — proposal

Status: **PROPOSED** (owner direction 2026-11-27). Implemented by `MP-R1-WP03` (store) and
`MP-R1-WP04` (ingest gate). Until those land, this file is the design target, not a contract.

## 1. The job

The owner types one prompt into an orchestrator model, for example:

> generate curated data untuk topik sibambo

The orchestrator (Claude Opus/Sonnet, Gemini Neon, or another planner-class model) splits the
work, starts several cheap workers (Claude Haiku, Gemini Flash, ...) that read the archive in
parallel, and the result becomes the nodes of the topic's **Constellation**. Every node can be
updated later by the same flow, without rewriting the rest of the topic.

Any agent that can read Mind Place over MCP and write local files can take part: Claude Code,
Antigravity, Gemini CLI, Codex. No provider-specific path exists.

## 2. Design goals, in order

1. **Evidence or nothing.** Every node and edge cites archive spans. An item that cannot cite its
   evidence is rejected at ingest. Curated data stays a derived assertion (`asserted_by=model`),
   never raw evidence (product invariant).
2. **Parallel without conflicts.** Workers never write the same file. Two workers that find the
   same thing converge on the same node instead of making duplicates.
3. **Cheap for workers.** A Haiku-class worker emits short records; it cites by reference
   (source id, message index, char span, hash) instead of copying text, except where the
   precision profile asks for verbatim quotes.
4. **Cheap to validate.** Deterministic parse, one record per line, fixed small vocabularies,
   bounded sizes. The ingest gate needs no model.
5. **Updatable per node.** A node has a stable key and a revision number. An update is a field
   patch with a base revision, not a full rewrite.
6. **Rebuildable views.** The graph/Constellation is projected from the accepted node set and can
   be rebuilt at any time; the op log and the accepted generations are the authority.

## 3. Why an op log in JSONL (and not Markdown pages)

| Option | Parallel writers | Validation | Update one node | Token cost | Verdict |
| --- | --- | --- | --- | --- | --- |
| One Markdown page per node (Obsidian style) | Collide on the same file | Fuzzy parse | Rewrite the page | High (prose + frontmatter) | Good export view, bad authoring format |
| One big JSON per topic | Every worker rewrites it | Easy | Whole-file merge | High | Rejected |
| **Append-only JSONL op log, one file per shard** | **No collisions** | **Deterministic, line by line** | **Field patch with `base_rev`** | **Low** | **Chosen** |

Markdown stays available as a generated, read-only export of the accepted node set.

## 4. Files a curation run produces

```text
<curation-inbox>/<run_id>/
  run.json                  # written by Mind Place when the plan is issued (read-only to agents)
  shards/<shard_id>.jsonl   # one file per worker; only that worker writes it
  reconcile.jsonl           # optional, written by the orchestrator after the workers finish
  DONE                      # empty marker; the orchestrator writes it last
```

`<curation-inbox>` is an owner-approved root (existing approved-root permission boundary).
Mind Place reads it only when the owner (or an owner-enabled watcher) starts ingest.

## 5. Record shape

Every line is one JSON object, at most 8 KiB, UTF-8, no comments.

```json
{"op":"upsert","key":"sibambo/decision/bom-engine-rust-port","kind":"decision",
 "title":"Port the BOM engine to Rust","summary":"Owner chose a Rust port over patching the Ruby plugin.",
 "attrs":{"status":"active","decided_at":"2026-10-02"},
 "aliases":["bom engine rewrite"],
 "evidence":[{"src":"src_9f3a…","msg":42,"span":[120,388],"sha":"b3:5e1c…"}],
 "confidence":0.8,"base_rev":0}
```

| Field | Rule |
| --- | --- |
| `op` | `upsert`, `patch`, `link`, `unlink`, `supersede`, `retire` |
| `key` | `<topic>/<kind>/<slug>`; slug is lowercase ASCII `[a-z0-9-]{1,64}`. Deterministic from the title so two workers converge. Mind Place maps it to an internal node id. |
| `kind` | `topic`, `entity`, `concept`, `decision`, `fact`, `question`, `task`, `event`, `artifact` (closed set) |
| `title` | ≤ 120 chars |
| `summary` | ≤ 400 chars in `brief`/`detailed`; `body` (≤ 4 KiB) only in `detailed` |
| `attrs` | Flat string/number/bool map, ≤ 16 keys, per-kind allowlist (for example `status`, `decided_at`, `due`, `owner_role`) |
| `evidence` | 1–16 refs. `src` = stable source id, `msg` = message index, `span` = char range inside that message, `sha` = hash of the span text. In `exact` profile each ref also carries `quote`, which must match the span byte for byte. |
| `confidence` | 0–1, the worker's own estimate; informational only, never used as authority |
| `base_rev` | Revision the worker read; `0` for a new node |

Edges:

```json
{"op":"link","from":"sibambo/decision/bom-engine-rust-port","rel":"supersedes",
 "to":"sibambo/decision/patch-ruby-plugin","evidence":[{"src":"src_9f3a…","msg":44,"span":[0,210],"sha":"b3:…"}]}
```

`rel` is one of `part_of`, `about`, `depends_on`, `supports`, `contradicts`, `supersedes`,
`decided_in`, `mentions`, `related_to`. `supersedes` is the A → B rule from the main plan: the
newer node carries B, and A stays visible as superseded.

## 6. Run flow

1. **Plan (read-only MCP).** The orchestrator calls `curation_plan(topic, profile)`. Mind Place
   answers with a `run_id`, a deterministic shard list (source windows that match the topic, each
   bounded in size) and a compact **topic index** of existing nodes (`key`, `kind`, `title`,
   `rev`, `aliases`). Workers use the index to patch existing nodes instead of duplicating them.
2. **Work.** Each worker takes one shard, reads it with the existing read tools (`memory_fetch`,
   `context_build`), and appends records to its own `shards/<shard_id>.jsonl`.
3. **Reconcile (optional).** The orchestrator merges near-duplicates and adds cross-shard edges in
   `reconcile.jsonl`, then writes `DONE`.
4. **Ingest (Mind Place, deterministic).** Schema check → evidence gate (span exists, hash
   matches, verbatim quote in `exact`) → key resolution and alias merge → revision check
   (`base_rev` behind the current revision turns the patch into a pending conflict) → staged
   generation.
5. **Publish.** The owner reviews by exception (rejected items, conflicts, risky changes, a random
   sample) and publishes the generation. The Constellation is re-projected from the new node set.

Release 2 adds the same payload over MCP (`curation_submit`), so a hosted Claude or Gemini chat can
curate one topic or the active conversation without the file inbox. The ingest gate is shared.

## 7. Updating one node

`patch` carries only changed fields plus `base_rev`. Example: the owner opens a node in the
Constellation, chooses "refresh", and Mind Place issues a one-shard plan scoped to that node's
evidence plus newer archive hits. A worker emits a `patch`, or a `supersede` when the decision
changed. Old revisions remain readable for audit.

## 8. Limits (initial, tune with measurements)

- 64 shards per run, 2 000 records per shard file, 8 KiB per record.
- 16 evidence refs per record; every ref must resolve to a span the run's plan covered.
- Unknown fields, kinds or relations are rejected, not ignored.

## 9. Security notes

- Inbox files are untrusted input. Text inside them is data, never instructions (MP-SEC-11).
- Ingest never follows paths found inside records and never reads outside the run folder.
- A worker cannot cite a source the plan did not hand out, so a poisoned record cannot pull in
  unrelated private material.
- Accepted nodes are `asserted_by=model` with the run id, model id and generation recorded.

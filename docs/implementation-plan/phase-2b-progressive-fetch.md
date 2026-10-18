# Phase 2B — Progressive evidence fetch and bounded output

Status: **DONE**. [PR #7](https://github.com/KaisAbiyyi/mind-place/pull/7)
squash-merged into `main` at
`5672b49ed508eb07e9331180e987a1313a8f9773` on 2026-10-18.
Implemented as a headless Rust primitive with a lease-guarded Tauri adapter.
Do not expose it over MCP/public endpoints without the Phase 3 capability checks.

## Architecture and proof of source

`IndexStore::fetch(&ArchiveStore, &FetchRequest)` resolves a
`source_id + archive_id + snapshot_id` triplet against **indexed revisions**
before reading evidence. It recomputes the canonical Phase 1C source ID and
archive identity, and validates the exact raw revision SHA-256 over the
normalized capture. The capture's provider, conversation ID, URL, title,
timestamp and completeness must match the index row; mismatches fail closed.
The index is a disposable routing catalog; raw capture is still authoritative.

A request cannot name an arbitrary file path, a transient SQLite row ID, or a
future unsupported provider. A pinned historical snapshot remains fetchable
even after a new current revision is selected. The index generation and
`indexed_is_current` are returned as explicit *index-time* freshness context;
clients must not treat that Boolean as a live remote-provider freshness proof.

## Request

```json
{
  "source_id": "src:v1:<64 lowercase hex>",
  "archive_id": "<64 lowercase hex>",
  "snapshot_id": "<64 lowercase hex>",
  "granularity": "window",
  "message_id": "<exact message ID>",
  "before": 2,
  "after": 2,
  "budget": {
    "max_bytes": 16384,
    "max_estimated_tokens": 4096,
    "max_latency_ms": 2000
  },
  "cursor": null
}
```

- `granularity: message` (default): one exact message, identified by its
  original message ID. `window`: target plus up to 20 neighboring messages
  on either side. `revision`: all messages in the specific immutable snapshot
  (omit `message_id`), paginated as necessary.
- `budget.max_bytes`: **4–262144** UTF-8 text bytes; strict text-content
  bound but deliberately *not* a whole-JSON-response size bound. Default 16384.
- `budget.max_estimated_tokens`: **1–65536**, default 4096. Estimated tokens
  are `ceil(total emitted UTF-8 bytes / 4)`, a budgeting heuristic rather
  than a model tokenizer count. The effective text-byte cap is the smaller of
  the byte budget and 4× this estimate budget.
- `budget.max_latency_ms`: **1–30000**, default 2000. Cooperative deadline
  checks happen before and during emission, but archive I/O/deserialization
  cannot be preempted. It is not an OS-level hard deadline.
- `cursor`: the exact returned `next_cursor`, on a **repeat request with
  identical source, revision, granularity and window**. It contains
  `message_index` and `byte_offset` on a UTF-8 boundary. No cursor is
  derived from an ephemeral row ID or textual match.

## Response

A response includes the same stable source/revision identity, provider,
conversation ID, URL, title, capture timestamp and completeness; index
generation/current flag; selected message range; and a list of ordered
`segments`. Each segment includes message ID/index, role, optional model and
authored date, text and exact UTF-8 start/end offsets, plus total message bytes.
No attachment bytes or inline embedded media are returned.

It also includes text-byte/estimated-token usage, `truncated`,
`truncation_reason` and `next_cursor`. A response is capped at 100 segments
even if individual messages are empty. Continuations preserve exact byte
offsets and Unicode boundaries so no original text is dropped or duplicated.

`truncation_reason` may be `byte_budget`, `token_estimate_budget`,
`utf8_boundary`, `segment_limit` or `latency_budget`. Unrecognized
identities/cursors, invalid bounds and tampered evidence return an error,
never substitute the current revision or a different message. The Tauri
`fetch_retrieval` command validates the unlock lease during the operation
and immediately before returning any result.

## Deferred

Phase 2C builds overlap deduplication, context assembly, grouping and
retrieval presets. Phase 2D adds dedicated scoring/performance telemetry.
Remote MCP authorization and approved-filesystem-root source filtering belong
to Phases 3 and 4, before any new external surface is published.

Windows product CI must pass formatting, repository verification, typecheck,
Rust/web integration tests and desktop build. Storage qualification is a
regression backstop. No local/manual session is required for this cloud unit.

## Completed cloud handoff (2026-10-18)

- **UNIT:** Phase 2B — progressive fetch and bounded output.
- **PR:** https://github.com/KaisAbiyyi/mind-place/pull/7 — squash-merged.
- **Qualified code SHA:** `5672b49ed508eb07e9331180e987a1313a8f9773`.
- **Windows product CI:** https://github.com/KaisAbiyyi/mind-place/actions/runs/39116420328 —
  PASS: Rust format, repo checks, TS typecheck, SQLite runtime qualification,
  full web/Rust regression suites including six new fetch tests, and desktop build.
- **Windows storage CI:** https://github.com/KaisAbiyyi/mind-place/actions/runs/39116420331 —
  PASS for archive/FTS/rebuild fixtures of 10K, 100K and 1M message chunks.
- **Storage changes:** none; raw evidence and authoritative identity remain unchanged.
- **Local required:** no — Phase 2B cloud-only (`C1/L0/U0`).
- **Known limits:** byte budget excludes JSON envelope; token counts are
  documented estimates; latency is cooperative, not a hard I/O timeout.
  Only the current local ChatGPT archive adapter is supported.
- **NEXT:** `implement phase 2C` — context builder, overlap dedupe,
  retrieval policies and diversity.

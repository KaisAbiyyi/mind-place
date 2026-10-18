# Phase 2A — Universal retrieval query and result contract

Status: **DONE**. [PR #6](https://github.com/KaisAbiyyi/mind-place/pull/6)
was squash-merged into `main` at
`d7790973cc8ae3ef32f11eaad938b652de96a7cf` on 2026-10-18.
This contract is an implementation record, not an authorization to expose
an external endpoint.

## Boundary

The protocol-independent Rust retrieval API lives in
`mind_place_index::IndexStore::retrieve(&RetrievalRequest)`. The Tauri
`search_retrieval` command routes through the existing unlock-lease gate and
revalidates the lease **after** the worker completes, before disclosure.
Existing Phase 1D `search_archive_index` remains supported for compatibility.

Phase 2A uses the existing disposable `index.sqlite3` schema v1 and its
external-content FTS5 index; it does not mutate raw captures, authoritative
`state.sqlite3`, or migration metadata. The only indexed source adapter so
far is the app-owned local ChatGPT archive. Source IDs and revision IDs remain
stable across index deletion and rebuild.

## Request

```json
{
  "queries": [
    "architecture decision",
    { "text": "reason for the migration", "weight": 2, "mode": "exact" }
  ],
  "filters": {
    "include_history": false,
    "providers": ["chatgpt"],
    "models": [],
    "roles": ["user", "assistant"],
    "kinds": ["message"],
    "source_ids": [],
    "archive_ids": [],
    "conversation_ids": [],
    "message_ids": [],
    "captured_from": "2026-09-01T00:00:00Z",
    "captured_to": "2026-10-18T23:59:59Z"
  },
  "limit": 20
}
```

- `queries`: 1–8 text or weighted queries; 1024 bytes and 24 whitespace-separated
  terms max each; weights finite and in `(0,10]`.
- `mode: lexical` (default) requires all quoted query terms in a matching
  chunk; `exact` requires a consecutive **FTS5 token phrase**. Exact means
  token adjacency, not raw byte-for-byte substring. FTS operator syntax is not
  accepted as user query operators.
- Structured filters intersect across fields and union within each field.
  Empty arrays mean no filter. Filters are exact, not fuzzy; a model filter
  intentionally excludes title and user chunks with no model metadata.
- Capture date range is inclusive and requires RFC3339. Comparisons use
  SQLite's normalized Julian-day interpretation; time-zone offsets work.
- Current archive revision only is the default. Explicit `include_history`
  admits older immutable revisions. No `as_of`/truth arbitration yet.
- `limit` defaults to 20, max 100. Individual filter lists cap at 32 values,
  max 256 bytes each. Unknown fields fail decoding rather than silently widen
  access.

## Response

```json
{
  "hits": [
    {
      "source_id": "src:v1:<sha256>",
      "archive_id": "<archive id>",
      "snapshot_id": "<immutable snapshot id>",
      "conversation_id": "<provider conversation id>",
      "provider": "chatgpt",
      "source_url": "<captured source URL>",
      "captured_at": "<RFC3339>",
      "completeness": "partial",
      "message_id": "<message or @title>",
      "part": 0,
      "kind": "message",
      "role": "user",
      "model": null,
      "title": "Conversation title",
      "snippet": "[matching] text…",
      "score": 1.0,
      "matched_queries": [0],
      "exact_queries": []
    }
  ],
  "candidate_count": 1,
  "truncated": false
}
```

All identifiers above represent canonical evidence provenance, never the
disposable SQLite row ID. Score is a higher-is-better deterministic combination
of weighted negative-BM25 relevance and an exact-phrase bonus. Scores are
**not** probabilities, cross-index calibrations or truth/confidence scores.
Results tie-break by immutable provenance fields.

The engine collects at most eight times the requested limit per query before
merging duplicate chunks and reranking. `candidate_count` is the count in this
**bounded shortlist**, never a total corpus-match count. `truncated` reports
whether that merged shortlist exceeded the requested limit; it is not a promise
that all matching chunks were visited. Snippets are derived data.

## Security and follow-on work

The current Tauri route remains fully locked by Phase 1A. This API must not be
exposed to remote clients without Phase 3 capability authorization. Phase 4
source adapters must apply *live approved-root authorization before candidates,
snippets or cached content leave the retrieval engine*; the current API is
limited to the app-owned ChatGPT archive and does not pretend to authorize
filesystem/project/tag filters that the existing index cannot enforce.

Phase 2B owns progressive exact fetch, message windows, byte/token/latency
budgets and truncation metadata for fetched content. Phase 2C owns context
construction, presets, diversity, contradictions and historical `as_of`.
Phase 2D owns score/provenance inspection, larger ranking regression corpora
and performance qualification.

## Completed cloud handoff (2026-10-18)

- **UNIT:** Phase 2A — multi-query search, structured filters and stable result contract.
- **PR:** https://github.com/KaisAbiyyi/mind-place/pull/6 — squash-merged.
- **Qualified code SHA:** `d7790973cc8ae3ef32f11eaad938b652de96a7cf`.
- **Windows product CI:** https://github.com/KaisAbiyyi/mind-place/actions/runs/39114819537 —
  success: format, repo verification, TypeScript typecheck, SQLite runtime,
  all Rust/web tests (including 5 new Phase 2A tests) and Tauri desktop build.
- **Storage qualification:** https://github.com/KaisAbiyyi/mind-place/actions/runs/39114819541 —
  success for 10K, 100K and 1M archive-message chunk fixtures.
- **Schema:** no migration; existing index schema v1, state/source IDs unchanged.
- **Local required:** no. Phase 2 is cloud-only until future connector/device phases.
- **Known limits:** archive-owned ChatGPT source adapter only; FTS5 phrase is
  token-exact, not arbitrary byte-substring; shortlist ranking is bounded
  and not exhaustive. Token/byte budgets and source windows are Phase 2B.
- **NEXT:** `implement phase 2B` — progressive exact fetch and budgets.

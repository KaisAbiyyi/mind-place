# Phase 2D — Local retrieval observability and qualification

Status: **IN PROGRESS** on `feat/phase-2d-retrieval-qualification`.

## Inspection surface

The Rust `IndexStore` exposes `inspect_retrieval`, `inspect_context`, and `inspect_fetch`. Matching Tauri commands require the existing unlock epoch both before execution and after the result is generated. These commands do not expose a remote endpoint or grant new permissions.

Every inspector returns the actual underlying Phase 2A/2B/2C response plus an **ephemeral, local-only metadata trace**. A trace has schema version, operation, monotonic elapsed microseconds, index generation, candidate/result/source counts, fetched text bytes, estimated tokens, and truncation status. It includes no query text, prompts, snippets, document content, source identifiers, source URLs, credentials, or raw PIN. No trace database, retained log or background process is created. The separate protected result object still contains evidence and must never be logged or disclosed by unauthorized clients.

Each Phase 2A search hit also carries `score_contributions`: input query index, weight, raw FTS5 BM25, lexical relevance transformation, exact phrase bonus, and final weighted contribution. The sum of contributions is the final hit score. This is an *audit trail of deterministic lexical ranking*, **not** a confidence value, a probability, a quality prediction, or a contradiction detector. Provenance is the stable source/revision/message tuple, never a temporary SQLite rowid.

## Deterministic qualification

`crates/index-core/tests/inspection.rs` checks ranking precedence for a weighted exact phrase, per-hit score arithmetic and finite values, tie/rebuild reproducibility, historical revisions and structured filters, trace privacy, exact fetch, context metadata parity, and unlock revocation.

The existing `storage_qualification.rs` generates the same deterministic Phase 1E synthetic archive corpus at **10K, 100K and 1M message chunks**. The Phase 2D gate adds warmed nine-sample medians of:

- the original Phase 1D lexical search as a same-run baseline;
- the Phase 2A weighted ranking and inspector;
- the Phase 2C context builder and inspector (which uses Phase 2B integrity-checked archive fetch).

The ranked-inspector median must be at most `max(20× baseline, 50,000 microseconds)`; the context-inspector median must be at most `max(100× baseline, 500,000 microseconds)`. These are **broad CI regression guardrails**, not user-device p95 or guaranteed service deadlines. The historical Phase 1E rare lookup pair was about 4 ms, but those original runner measurements must not be misrepresented as a same-run baseline. Relative comparisons are taken on the same current Windows runner. Receipts report all observed medians and gates, with no personally sourced content.

Phase 2D inspectors read the index-generation key directly, avoiding an
otherwise full-corpus count of every chunk merely to emit diagnostic metadata.
The first 1M Windows measurement exposed this unnecessary overhead; the
tightened gates verify the optimized implementation.

The test also verifies the ranked search response is unchanged following the Phase 1E physical index deletion and full rebuild. Previous Phase 1E proof of untouched raw hashes, curated state and authoritative IDs remains mandatory. The existing Windows matrix workflow uploads the updated JSON qualification receipts for all tiers.

## Boundaries and handoff

This unit changes no raw archive, index/state schema, persisted user data, OS services, external permissions or MCP capability. Observability is opt-in per local command call and not persisted automatically. Search and context are synthetic lexical workloads; actual personal or cross-provider relevance must be qualified during later MCP/source-adapter phases.

Required gates before squash merge: `cargo fmt --all -- --check`, repo verification, TypeScript typecheck, full Rust/web tests, Tauri desktop Windows build, and full 10K/100K/1M Windows storage/performance matrix. No local Windows device/manual gate is required for Phase 2D (C1/L0/U0).

**Next after green merge:** Phase 3A — shared-brain integration core and protocol-independent read adapter.

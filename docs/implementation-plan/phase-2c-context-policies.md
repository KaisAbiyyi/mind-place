# Phase 2C — context packs and retrieval policies

Status: **DONE**. PR #8 squash-merged at `326969be7de7fd0d5e93affb6cdda165ef1c8b8c` (2026-10-18).

## Core contract

`IndexStore::build_context` composes the Phase 2A lexical search and Phase 2B exact, hash-verified archive fetch. No migration, new canonical data, model-generated memory, filesystem permission, or external endpoint is added. The Tauri `build_retrieval_context` command is guarded by the existing unlock lease during work and before result disclosure.

A request accepts Phase 2A queries and filters, plus `preset`, `as_of`, `max_sources`, `max_excerpts`, `max_per_source`, and a text-byte / estimated-token / cooperative-latency budget.

Presets: `fast` fetches matching messages; `balanced` fetches one neighbor each side; `source_first` fetches two; `challenge` includes raw historical snapshots alongside current evidence; `historical` requires an RFC3339 `as_of` and filters snapshots by `captured_at` before that instant. Historical means observation-time archive evidence, **not a reconstructed belief or claim state**. Challenge does not claim contradiction detection.

## Evidence output

Candidates from bounded search are interleaved across canonical source IDs to prevent one large conversation from dominating. Groups preserve exact immutable source and snapshot IDs, capture metadata and current-in-index flag. Excerpts retain message ID, role, optional index, text-byte offsets, matching query IDs and score. Overlapping message windows deduplicate by source/snapshot/message. Title hits are sourced from the verified raw title. Up to 12 sources, 40 excerpts, and 6 matched hits per source are permitted.

Text budgets are 4–65536 bytes and 1–16384 estimated tokens, with a cooperative 1–30000 ms latency budget. Token estimate is UTF-8 bytes divided by four, rounded up, not an actual model tokenizer. Envelope/metadata bytes are not included. Partial source fetches expose Phase 2B cursor data for explicit continuation. Truncation and bounded-shortlist limitations are reported, rather than implying exhaustive search.

Only the app-owned ChatGPT archive adapter is supported. Future filesystem sources must enforce approved-root revocation before any evidence is returned. Phase 2D will add traces, inspector data and comprehensive retrieval performance gates. Local/manual qualification is not required for Phase 2C.

# MP-UI-07 — Global search, retrieval context builder and inspector

**Task:** MP-UI-07  
**Repository:** `KaisAbiyyi/mind-place`  
**Base:** `main` @ `24648c74f3f59e176974d3878049b882478db69b`  
**Branch:** `work/mp-ui-07-08-local`  
**Exclusive resource lock:** `ui-module/search-context-inspector`  
**Execution:** Local Windows Antigravity implementation

## 1. Objective and scope

Implement the global command palette and the full retrieval & context workspace. Extend the monochrome HTML search, result table, provenance detail, and drawer patterns to support:

1. Unified multi-query retrieval with term weighting and structured filters.
2. Progressive evidence fetch with visible limits and pagination (no hardcoded first-12 slice).
3. Context pack assembly, citation provenance and formatted export/copying.
4. Technical inspection drawer displaying SQLite FTS5 BM25 breakdown, candidate evaluation and microsecond latency.

## 2. Command palette enhancements

In `src/ui-shell.ts`:
- **Result grouping:** Group headings for `Destinations` and `Actions` separate navigation targets from actionable commands.
- **Accessible keyboard selection:** `ArrowDown` from the search input enters the results list; `ArrowDown`/`ArrowUp` cycle between result options; `ArrowUp` on the first option returns focus to the search field.
- **Focus restoration:** Captures triggering element upon `openPalette` and returns focus upon `closePalette`.
- **Visible result pagination:** Groups default to 15 items with an explicit "Show all results" expansion toggle rather than truncation without notice.

## 3. Retrieval and context workspace

In `src/search-workspace.ts` and `src/search-workspace.css`:
- **Cross-entity search:** Searches conversations (`search_archive_index`), approved document roots (`search_document_index`), and multi-term structured retrieval (`search_retrieval`).
- **Multi-query term builder:** Allows dynamically adding multiple query terms with individual weights (0.1 to 5.0).
- **Retrieval policies & presets:** Supports `minimal`, `balanced`, `full`, and `chronological` presets with customizable token budgets (500–32,000 tokens) and provider/role/kind filters.
- **Pagination & evidence cards:** Progressive pagination (`Page X of Y`, Next/Previous controls) with provenance tags (`archive_id`, `snapshot_id`, `message_id`, `score`, `role`, `kind`).
- **Context Pack Builder:** Invokes `build_retrieval_context` to assemble structured context packs with token counts and byte offsets, with a "Copy Context Pack" clipboard action.
- **Technical Inspector:** Invokes `inspect_retrieval` to display candidate counts, elapsed microseconds (`elapsed_us`), index generation, and per-query BM25 / lexical / exact bonus contributions.
- **Fail-closed security:** Validates `get_security_status`. When locked or revoked, fails closed with an alert panel and clears cached results.

## 4. Verification

- All 4 dedicated unit tests in `test/search-workspace.test.ts` pass.
- All 4 legacy library search tests in `test/library-workspace.test.ts` pass.
- Full web test suite (213/213 tests) passes.

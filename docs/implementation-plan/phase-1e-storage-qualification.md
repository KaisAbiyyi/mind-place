# Phase 1E — Windows storage and performance qualification

Status: **DONE** — [PR #5](https://github.com/KaisAbiyyi/mind-place/pull/5) squash-merged into `main` at `78853d35e0439c6fd201a667b6ba156bf22fb132` after successful Windows [product CI #39112178143](https://github.com/KaisAbiyyi/mind-place/actions/runs/39112178143) and [storage qualification #39112178136](https://github.com/KaisAbiyyi/mind-place/actions/runs/39112178136).

This is a **qualification harness**, not a new storage format or authorization surface.
The evidence source remains `archive-v1`; `state.sqlite3` keeps canonical source
identity and curated user state; `index.sqlite3` can be deleted and rebuilt.

## Deterministic synthetic corpus

`crates/index-core/tests/storage_qualification.rs` is an **ignored** Rust test,
run deliberately by the separate `storage-qualification.yml` workflow on Windows.
It generates no personal data, network traffic or files outside the temporary
test directory, apart from the optional machine-readable receipt.

| Tier | Messages/chunks | Conversations | FTS rows (including titles) |
| --- | ---: | ---: | ---: |
| Small | 10,000 | 1 | 10,001 |
| Medium | 100,000 | 10 | 100,010 |
| Large | 1,000,000 | 100 | 1,000,100 |

Each conversation contains 10,000 unique messages (the supported archive limit),
a current immutable JSONL revision, and a title chunk. Every message fits into
one FTS chunk (under 4096 UTF-8 bytes). The tier size counts **message** chunks;
the actual FTS index also includes one title chunk per conversation.

This is a stress corpus for the **actual** `ArchiveStore::save` →
`IndexStore::sync` → `IndexStore::search` → delete `index.sqlite3` →
`IndexStore::rebuild` path, not a microbenchmark of synthetic direct SQL inserts.
It uses the pinned `rusqlite` bundled SQLite and the real production schema.

## Required invariants

1. Initial and no-op incremental sync return the exact expected source,
   revision and FTS chunk counts. No-op sync adds zero revisions.
2. Rare FTS results preserve source identity, archive provenance, message ID
   and deterministic hit order through a full physical deletion and rebuild.
3. Existing `index.sqlite3` (and idle `-wal`/`-shm` sidecars) are removed.
   A fresh index is empty; the subsequent rebuild reprojects all raw evidence.
4. Original raw JSONL SHA-256 hashes are unchanged for **every** fixture
   conversation before and after deletion/rebuild.
5. Canonical source identities remain in `state.sqlite3`. An authority-owned
   project, curated memory claim and FK-backed claim→source evidence link all
   survive index deletion without being regenerated from derived row IDs.
6. Bundled SQLite passes both table/foreign-key integrity and external-content
   FTS5 consistency checks; subsequent state open validates persistent schema.
7. Windows workflow runs the existing state migration/recovery and index
   integration test suites in release profile, in addition to the scale suite.

## Performance observations

Each tier emits a JSON receipt `qualification/<tier>.json`, uploaded as a
GitHub Actions artifact. Timings are **elapsed milliseconds** for corpus
creation, initial sync, two rare searches, no-op sync, full deletion/rebuild,
and two post-rebuild searches. Disk footprint includes the main derived index
and live WAL sidecars. Receipts record OS and SQLite version.

There is intentionally **no fabricated absolute latency target**: GitHub-hosted
Windows runners vary, and the Phase 1D provisional search contract is not the
final Phase 2 retrieval kernel. Correctness and integrity are hard gates.
Measured bottlenecks and scaling ratios are documented below from the actual Windows CI
receipts; future performance budgets must use evidence from relevant workloads.

### Reproduction (Windows cloud CI)

```powershell
$env:MP_QUAL_CHUNKS = '10000' # or 100000 / 1000000
$env:MP_QUAL_RECEIPT = 'qualification/10000.json'
cargo test --release -p mind-place-index --test storage_qualification qualifies_archive_scale -- --ignored --exact --nocapture
```

The workflow runs all tiers automatically on PR changes to the archive,
authority or index crates, and supports manual dispatch. Ordinary `npm test`
continues to execute the fast integration regression tests but **does not run**
the million-chunk qualification.

## Qualified Windows results (2026-10-18)

[Windows qualification run 39112178136](https://github.com/KaisAbiyyi/mind-place/actions/runs/39112178136) passed **all three matrix jobs**. Each job uploaded a machine-readable JSON artifact named `phase-1e-storage-<tier>`, retained for 30 days. The test also ran the state schema/migration suite and Phase 1D index regressions in release profile.

| Metric | 10K | 100K | 1M |
| --- | ---: | ---: | ---: |
| Actual FTS rows (with title) | 10,001 | 100,010 | 1,000,100 |
| Corpus generation (ms) | 88 | 928 | 9,485 |
| Initial sync (ms) | 951 | 9,083 | 87,610 |
| No-op incremental sync (ms) | 55 | 325 | 3,163 |
| Two rare FTS lookups (ms) | 4 | 4 | 4 |
| Full delete-and-rebuild (ms) | 792 | 8,746 | 79,856 |
| Derived index size (bytes) | 7,081,984 | 70,270,976 | 691,134,464 |
| Raw JSONL hash invariant | PASS | PASS | PASS |
| Canonical IDs, project, claim + evidence | PASS | PASS | PASS |
| Foreign keys, SQLite and FTS integrity | PASS | PASS | PASS |

Values are single-run measurements on separate ephemeral Windows CI runners, **not** user-device SLAs or p95. Initial sync scales approximately with chunk count on this synthetic dataset; no-op scan still reads archived records and takes 3.16 seconds at 1M. Rare-term lookups are not a substitute for Phase 2 ranking and high-frequency query performance tests.

**Bundled SQLite measured:** 3.53.2 across all tiers. This meets the current source-code minimum but is below the strategic 3.54.1-or-newer upgrade target. Upgrading the pinned runtime remains a separate deliberate, regression-qualified dependency change; this phase did not silently change the storage ABI.

The regular [Windows product CI run 39112178143](https://github.com/KaisAbiyyi/mind-place/actions/runs/39112178143) also passed formatting, repository verification, TypeScript typecheck, bundled SQLite qualification, the full test suite and the native desktop build.

## Scope and qualification boundary

- No schema changes, no new private data access, no changes to captured raw
  evidence, no filesystem provider imports, no MCP/client tunnel.
- The benchmark does not measure representative personal data, WebView2 UI,
  browser capture, the user's own Windows disk, semantic retrieval or full
  corpus query quality. Those are later phase concerns.
- Large-tier runtime and disk usage are deliberately measured rather than
  assumed. If a tier times out, report BLOCKED/FAIL and split diagnosis from
  passing tier results; do **not** silently lower the corpus size.
- This unit requires no local Windows pull or manual acceptance unless hosted
  Windows CI reveals an irreducible local-only uncertainty.

## Completed cloud / Phase 1 handoff

- **UNIT:** 1E — storage qualification harness, closing Phase 1 (1A–1E).
- **PR:** [#5](https://github.com/KaisAbiyyi/mind-place/pull/5) — squash-merged.
- **Qualified Phase 1E code SHA:** `78853d35e0439c6fd201a667b6ba156bf22fb132`.
- **Validation:** [Windows CI #39112178143](https://github.com/KaisAbiyyi/mind-place/actions/runs/39112178143) **PASS**; [Windows 10K/100K/1M qualification #39112178136](https://github.com/KaisAbiyyi/mind-place/actions/runs/39112178136) **PASS (3/3)**, with all three JSON receipts uploaded.
- **Storage decisions:** immutable `archive-v1` remains canonical evidence, `state.sqlite3` retains identity/curated authority, `index.sqlite3` is disposable and rebuildable; no schema/protocol change in 1E.
- **Local required:** **No** (Phase 1 is C5/L0/U0). User should not pull locally just to begin Phase 2A.
- **Known limits:** Synthetic one-chunk-per-message corpus, rare query timing only, one runner observation per scale, bundled SQLite 3.53.2 (target upgrade later), no live provider or UI test.
- **NEXT:** `implement phase 2A` — universal search request model, structured filters, BM25/exact retrieval and stable result contract.

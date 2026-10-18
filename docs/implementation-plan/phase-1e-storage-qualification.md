# Phase 1E — Windows storage and performance qualification

Status: **IN PROGRESS** — cloud-only unit following Phase 1D at `76010d1f8599141b71a0478cd6aab053286ca1d7`.

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
Measured bottlenecks and scaling ratios will be documented from the real CI
receipts; any proposed future performance budget must use that evidence.

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

## Cloud handoff (fill from completed Actions runs)

- **Unit:** 1E — storage qualification harness
- **Branch:** `feat/phase-1e-storage-qualification`
- **PR / merged SHA:** pending
- **Windows CI run and artifact links:** pending
- **10K / 100K / 1M results:** pending
- **Local required:** no by default
- **Next:** Phase 2A after Phase 1E tests and code have green CI evidence.

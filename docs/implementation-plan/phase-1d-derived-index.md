# Phase 1D — derived archive index and FTS5

Status: **DONE** — [PR #4](https://github.com/KaisAbiyyi/mind-place/pull/4) squash-merged into `main` at `ccbb6e9ffadce1ebaefde3d328e3936725156823`. Windows [CI run 39108537645](https://github.com/KaisAbiyyi/mind-place/actions/runs/39108537645) passed formatting, repository verification, typecheck, bundled SQLite gate, web/Rust tests and Tauri desktop build.

The index is a performance projection, not a source of truth. Its SQLite file is
a sibling of the existing archive-v1 and state.sqlite3 files:

- archive-v1/conversations/ — immutable original capture JSONL and revision metadata;
- state.sqlite3 — authority-owned canonical source identity mappings;
- index.sqlite3 — entirely disposable source/revision/chunk catalog and FTS5.

## Projection contract

The mind-place-index Rust crate uses the same pinned, bundled SQLite build as
state-core. It refuses an existing foreign database, unknown schema or unsafe
symlink/reparse path. The index has its own application ID, schema version,
schema checksum and generation number. Normal index transactions use WAL with
NORMAL synchronous durability; authoritative state retains its independent,
more conservative settings.

Every indexed ChatGPT conversation first receives a canonical Phase 1C
source identity registered in state.sqlite3. The stable namespace
mind-place-chatgpt-archive-v1 identifies the local archive import origin; it
does not claim to know the user's actual provider account. The provider
conversation UUID is the logical external source key. Derived chunk rowids
never serve as authoritative evidence references.

Index revisions are keyed by canonical source ID plus the immutable archive
snapshot ID. The selected revision is marked current; older revisions remain
searchable when history is explicitly requested. Each revision indexes the
title and each nonblank message text, split on UTF-8 boundaries into at most
4096-byte derived chunks. Every hit includes canonical source ID, archive ID,
snapshot ID, message ID, chunk number and role. Attachments are not decoded or
OCR'd by this phase.

The chunks table stores search text once. An external-content FTS5 virtual
table with insert/delete/update triggers maintains a lexical BM25 and snippet
index. The phase-local search interface is intentionally narrow; Phase 2 owns
universal retrieval, structured filters, budgets and final ranking.

## API and lifecycle

After a successful Phase 1A unlock and authoritative database readiness,
these Tauri commands are available:

- refresh_archive_index — append new revisions and update current flags;
- rebuild_archive_index — transactionally reconstruct the index from evidence;
- get_archive_index_stats — source/revision/chunk/generation counts;
- search_archive_index — bounded lexical query with optional include_history.

All four commands use the unlock-epoch lease and block result disclosure after
a concurrent relock. Incremental updates and rebuilds check the lease during
ingestion and immediately before committing. A cancelled/erroring scan rolls
back derived updates. Re-registering existing canonical source mappings is
idempotent; the authority store never depends on the index file.

The rebuild is from the archive files, not from cached chunks. Reopening after
deletion of index.sqlite3 creates a fresh empty index, which the rebuild command
then populates. The old index must not be used as a backup of canonical data.

## Operational boundaries

- The archive is still readable on disk to local OS principals who can access
  it. Phase 1A PIN/unlock does not imply encryption at rest.
- No new file-system root is approved by indexing; this unit only indexes the
  app's own ChatGPT archive.
- The native capture host can save revisions independently of the Tauri
  process. An explicit incremental refresh is currently needed to project
  newly saved external revisions; live watcher/reconciliation belongs to
  later ingestion work.
- Existing indexed snapshots use a stable revision-ID fast path. Newly
  ingested revisions verify the archived snapshot hash. Expensive exhaustive
  revalidation and performance tuning belong to Phase 1E.
- An unrecognizable or corrupt derived SQLite file is refused rather than
  silently destroyed. It can be removed explicitly along with its idle WAL
  sidecars and rebuilt. The authoritative files must never be removed.
- There is no MCP, public tunnel, filesystem scanner, encryption migration,
  semantic/vector embedding, or new consumer UI in this unit.

## Regression proof

The integration fixtures cover current-versus-historical revisions, no-op
incremental updates, new revision ingestion, canonical source ID survival after
index deletion/rebuild, raw file hash preservation, tampered new snapshot
rejection, transaction rollback on relock, Unicode chunking, bounded FTS
queries and foreign SQLite refusal.

Windows CI runs repository verification, Rust formatting, all Rust/web tests
(including the new index package), SQLite runtime qualification and Tauri
desktop build. No local Windows/manual session is planned for this unit.

NEXT: Phase 1E benchmarks, deletion/rebuild at representative scale and
Windows SQLite-integrated qualification.

## Completed cloud handoff (2026-10-18)

- **UNIT:** Phase 1D — rebuildable derived index and FTS5.
- **PR:** https://github.com/KaisAbiyyi/mind-place/pull/4 — squash-merged.
- **Qualified code SHA:** `ccbb6e9ffadce1ebaefde3d328e3936725156823`.
- **Validation:** Windows CI run `39108537645` — successful; all Rust/web tests, format/typecheck, SQLite version and desktop build passed.
- **Storage contract:** `index.sqlite3` is disposable; `state.sqlite3` retains canonical identities; raw archive revisions remain immutable.
- **Local required:** No; Phase 1D is qualified by cloud CI. Do not pull a local checkout for this unit.
- **Known limits:** New externally saved captures require refresh or next unlock; selected index revision is the archive's latest selected revision, historical revisions remain queryable. Full at-scale measurement and stronger rebuild fault injection are Phase 1E work.
- **NEXT:** `implement phase 1E` — performance/scale and index deletion/rebuild qualification.

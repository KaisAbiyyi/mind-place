# Current implementation sequence

This file is the small execution-order pointer for cloud agents. Strategy lives in `../BIG-IMPLEMENTATION-PLAN.md`; detailed design notes live beside this file. If older phase labels conflict with this file, follow this file for the current execution order.

## Baseline

- **Phase 0 — current archive foundation: DONE / qualified compatibility floor.**
- GitHub/cloud is implementation source of truth.
- Local Windows is a qualification target only when a phase explicitly requires device/browser/account behavior.

## Phase 1 — durable brain foundation

Current execution order after the security-first reprioritization:

1. **Phase 1A — security root and fail-closed unlock architecture** — DONE. PR #1 squash-merged into `main` at `e19d5f36a80441cdd9938e395a07bfad43ef4f63` after Windows CI #25 passed.
   - versioned local security state;
   - memory-hard secret verifier;
   - locked-by-default process startup;
   - unlock epoch/lease invalidation contract;
   - privileged Tauri archive/setup commands gated before disclosure and revalidated before response;
   - first-run secret confirmation and fail-closed unlock UI;
   - security dependencies are committed in `Cargo.lock`; no temporary lockfile-refresh workflow remains in `main`;
   - no claim of encryption-at-rest yet.
2. **Phase 1B — authoritative `state.sqlite3`** — DONE. PR #2 squash-merged into `main` at `32278f6a2a71a5211c429d86b5e377284d95acf0` after Windows CI run `39099905432` passed.
   - migrations/backups;
   - authoritative mutable state;
   - root ACL/tombstones;
   - memory-claim/project/topic/scope foundations;
   - durable security/client state that belongs in the authority store.
3. **Phase 1C — stable identity and authority durability** — DONE. PR #3 squash-merged into `main` at `d007e80b079471bcb66661fb90feba907d837dc2` after Windows CI run `39105339145` passed.
   - versioned provider-neutral IDs from deterministic, length-framed source origin tuples;
   - authoritative `source_identities` mapping in `state.sqlite3` schema v2;
   - FK-backed memory claim evidence references independent of disposable indexes;
   - verified V1 snapshots, fail-closed orphan evidence, atomic rollback/recovery and restart/tamper tests;
   - explicit identity contract at `phase-1c-source-identity.md`.
4. **Phase 1D — rebuildable `index.sqlite3` + FTS5** — DONE. PR #4 squash-merged into `main` at `ccbb6e9ffadce1ebaefde3d328e3936725156823` after Windows CI run `39108537645` passed.
   - separate disposable index and source/revision/chunk catalog for ChatGPT archive;
   - external-content FTS5 with incremental, transactional rebuild and current/history lookup;
   - Phase 1C canonical source IDs reused in authoritative `state.sqlite3`;
   - bounded chunking and tamper/cancellation/rebuild regression tests;
   - unlock-lease guarded Tauri commands and post-unlock startup refresh;
   - documented contract and limits at `phase-1d-derived-index.md`.
5. **Phase 1E — storage/performance qualification** — DONE. PR #5 squash-merged into `main` at `78853d35e0439c6fd201a667b6ba156bf22fb132` after [Windows product CI run 39112178143](https://github.com/KaisAbiyyi/mind-place/actions/runs/39112178143) and [storage qualification run 39112178136](https://github.com/KaisAbiyyi/mind-place/actions/runs/39112178136) both passed.
   - actual archive→FTS5 benchmark at 10K/100K/1M messages with CI JSON receipts;
   - measured Windows initial index 0.951/9.083/87.610 seconds and full rebuild 0.792/8.746/79.856 seconds;
   - deletion/rebuild preserves raw evidence hashes, authoritative source IDs, curated claim/evidence and projects;
   - all tiers passed SQLite/FTS/FK integrity, schema/migration and index regression checks;
   - bundled SQLite observed 3.53.2; deliberate 3.54.1+ upgrade remains an independently qualified future change;
   - full evidence and limitations at `phase-1e-storage-qualification.md`.

**Phase 1 is complete** and remained cloud-only (`C5 / L0 / U0`). No device/manual gate is required.

## Phase 2 — universal retrieval kernel

1. **Phase 2A — search requests + structured filters** — **DONE**. [PR #6](https://github.com/KaisAbiyyi/mind-place/pull/6) squash-merged into `main` at `d7790973cc8ae3ef32f11eaad938b652de96a7cf`. Windows [product CI 39114819537](https://github.com/KaisAbiyyi/mind-place/actions/runs/39114819537) and [storage qualification 39114819541](https://github.com/KaisAbiyyi/mind-place/actions/runs/39114819541) all passed.
   - typed multi-query input with weighted FTS5 lexical and exact-phrase ranking;
   - bounded, intersecting metadata/provider/model/role/source/date filters and explicit historical revision lookup;
   - deterministic stable-provenance compact hit contract, with no disposable index row IDs;
   - unlock-lease guarded `search_retrieval` Tauri command and regression fixtures;
   - no state/index migration and no local/manual gate (`C1 / L0 / U0`);
   - details, limits and handoff: [phase-2a-retrieval.md](phase-2a-retrieval.md).
2. **Phase 2B — progressive fetch and budgets** — **DONE**. [PR #7](https://github.com/KaisAbiyyi/mind-place/pull/7) squash-merged into `main` at `5672b49ed508eb07e9331180e987a1313a8f9773`. Windows [product CI 39116420328](https://github.com/KaisAbiyyi/mind-place/actions/runs/39116420328) and [storage qualification 39116420331](https://github.com/KaisAbiyyi/mind-place/actions/runs/39116420331) all passed.
   - exact-source/snapshot message, window and revision fetch with canonical identity and integrity checks;
   - Unicode-safe resumable cursors, byte/token-estimate/latency bounds and explicit truncated/freshness receipts;
   - unlock-lease guarded `fetch_retrieval` Tauri command and negative/security regression fixtures;
   - no index/state schema migration, and no local/manual gate (`C1 / L0 / U0`);
   - details, limitations and cloud handoff: [phase-2b-progressive-fetch.md](phase-2b-progressive-fetch.md).
3. **Phase 2C — context builder and retrieval policies** — **DONE**. [PR #8](https://github.com/KaisAbiyyi/mind-place/pull/8) squash-merged into `main` at `326969be7de7fd0d5e93affb6cdda165ef1c8b8c`. Windows [product CI 39119472142](https://github.com/KaisAbiyyi/mind-place/actions/runs/39119472142) and [storage qualification 39119472075](https://github.com/KaisAbiyyi/mind-place/actions/runs/39119472075) passed.
   - compose Phase 2A lexical retrieval with Phase 2B snapshot-authenticated source fetch;
   - bounded source-diverse context packs, overlap dedupe and original evidence provenance;
   - `fast`, `balanced`, `source_first`, `challenge`, and capture-time `historical` presets;
   - unlock-lease guarded `build_retrieval_context`, ranking metadata correction and security regression tests;
   - no schema migration, local/manual gate not required (`C1 / L0 / U0`);
   - details/limits: [phase-2c-context-policies.md](phase-2c-context-policies.md).
4. **Phase 2D — observability and retrieval qualification** — **DONE**. [PR #9](https://github.com/KaisAbiyyi/mind-place/pull/9) squash-merged at `77a3f15607d143b94ae4b51c2dd95b98e1abf91b`. Windows [product CI 39123680880](https://github.com/KaisAbiyyi/mind-place/actions/runs/39123680880) and [storage/performance qualification 39123680905](https://github.com/KaisAbiyyi/mind-place/actions/runs/39123680905) all passed.
   - ephemeral metadata-only search/context/fetch traces and unlock-lease guarded local inspectors;
   - per-query BM25/exact score explanation with stable provenance, without confusing ranking score with confidence;
   - deterministic ranking, privacy, revocation and rebuild regression tests;
   - Phase 1E synthetic 10K/100K/1M Windows corpus extended with same-run median retrieval/context performance gates and machine-readable receipts;
   - optimized index generation read avoids corpus-wide count; 1M ranked-inspector median 2.156 ms and context-inspector median 14.982 ms on the qualified Windows runner;
   - no schema migration, raw data rewrite, MCP exposure, or local/manual qualification (`C1 / L0 / U0`);
   - details and caveats: [phase-2d-retrieval-qualification.md](phase-2d-retrieval-qualification.md).

**Phase 2 is complete** (`C4 / L0 / U0`), with all four units 2A–2D cloud qualified and merged. Phase 3A has now merged to `main`; the next cloud instruction is `implement phase 3B` — authenticated client identities, scoped capabilities, and immediate revocation. No local Windows pull is required merely to start 3B.

## Phase 3 — shared-brain MCP

1. **Phase 3A — protocol-independent integration core** — **DONE**. [PR #10](https://github.com/KaisAbiyyi/mind-place/pull/10) squash-merged to `main` at `a42c90fc4f874e870f3f0a669340af27bd672bcc` after [Windows CI run 39125313066](https://github.com/KaisAbiyyi/mind-place/actions/runs/39125313066) passed (format, repository, typecheck, SQLite, full tests, desktop build).
   - read-only `memory_search`, `memory_fetch` and `context_build` contracts, protocol-neutral discovery and bounded all-or-nothing batches;
   - mandatory injected live read authorization plus unlock-epoch checks (3B supplies real client identities);
   - no HTTP/MCP listener yet; no local/manual gate required;
   - details: [phase-3a-shared-brain-core.md](phase-3a-shared-brain-core.md).
2. **Next: Cloud 3B — client authorization** (scoped capabilities, revocation, durable state, audit) before local MCP transport in 3C.

## Phase 1A explicit boundary

Phase 1A establishes the **local process security root**. It intentionally does not yet complete:

- encryption of existing raw archives at rest;
- Windows auto-start / Windows Hello / OS credential wrapping;
- persistent security state that belongs in `state.sqlite3` (Phase 1B);
- browser capture-host cross-process lock enforcement;
- MCP/tunnel connector startup and credential lifecycle, which is qualified with the connector phase.

Do not weaken Phase 1A to fake those later guarantees. UI/documentation must state the boundary accurately.

## Handoff rule

A cloud unit is complete only when its PR/branch is CI-green and its final SHA is reported. If a cloud-discovered bug is fixed, rerun the relevant checks before merge. The next normal instruction after completed Phase 3A is:

`implement phase 3B`

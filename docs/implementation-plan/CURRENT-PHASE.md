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
5. **Phase 1E — storage/performance qualification** — NEXT (cloud-only).
   - 10K/100K/1M fixtures;
   - delete/rebuild invariants;
   - Windows cloud CI and performance evidence.

Phase 1 remains cloud-only by default (`L0 / U0`). Do not pull local merely to continue feature implementation.

## Phase 1A explicit boundary

Phase 1A establishes the **local process security root**. It intentionally does not yet complete:

- encryption of existing raw archives at rest;
- Windows auto-start / Windows Hello / OS credential wrapping;
- persistent security state that belongs in `state.sqlite3` (Phase 1B);
- browser capture-host cross-process lock enforcement;
- MCP/tunnel connector startup and credential lifecycle, which is qualified with the connector phase.

Do not weaken Phase 1A to fake those later guarantees. UI/documentation must state the boundary accurately.

## Handoff rule

A cloud unit is complete only when its PR/branch is CI-green and its final SHA is reported. If a cloud-discovered bug is fixed, rerun the relevant checks before merge. The next normal instruction after completed Phase 1D is:

`implement phase 1E`

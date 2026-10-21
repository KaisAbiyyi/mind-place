# MP-4D cloud implementation checkpoint

## Identity and integration

- Project/repository: Mind Place / `KaisAbiyyi/mind-place`.
- Task/claim: `mind-place/MP-4D`, Codex GPT-6 managed cloud implementer, session `20261021-cloud-mp4d-01`, generation 1. Owner assigned this unit; status remains **Claimed** until the acceptance evidence is complete.
- Base: `3b56726a9eadb9e65b46788b900b2edd9be2719f` (MP-4A #23).
- Branch: `feature/mp-4d-document-objects`; [claim #25](https://github.com/KaisAbiyyi/mind-place/issues/25); [PR #27](https://github.com/KaisAbiyyi/mind-place/pull/27).
- Last code checkpoint: `1c298a6d76c2353b1b0498d4473171f9402cce78`, pushed. This document's commit and final-head CI are recorded in the PR/Notion handoff after publication.
- Logical resources: document object format v1 and additive authority schema v6. Concurrent MP-4B #26 owns schema v5 and root/NTFS authorization; this branch stacks its published checkpoints. **MP-4B must merge first.** Reconcile the final upstream SHA before integration; never merge conflicting schema-v5 implementations.

## Implemented contract

`mind_place_state::DocumentObjectStore` is a trusted internal library, with no desktop/MCP entry point. It accepts an already authorized byte reader; it never opens source paths. MP-4B supplies native authorization and MP-4C will supply scanner/stability checks.

- SHA-256 raw objects live beside `state.sqlite3` under `document-objects/v1/sha256/<first-two-hex>/<hash>`. Source bytes stream through a 64 KiB buffer to a temporary file, bounded by a configurable cap up to 64 MiB and the declared source length.
- Flush the raw object, atomically publish without overwriting, and synchronize directory entries on Unix before committing references. Existing objects are fully hashed before reuse. A failed authority transaction can leave a safe orphan; retry verifies and reuses it. No automatic evidence deletion, garbage collection or corrupt-object overwrite.
- Authority v6 adds `document_objects`, immutable logical `documents`, immutable `document_versions`, and mutable `document_heads`. Versions bind canonical MP-4A URI and raw content hash using a domain-separated, length-framed ID. Identical bytes share one object across roots/paths; repeated versions and A→B→A reuse the same version. The current observation changes without losing old raw versions or their first metadata.
- `preserve` always reads/hashes input. `unchanged` is an explicit **advisory** metadata probe (file identity, size, modification/change timestamps), with no content read. Matching metadata cannot prove byte equality; force preservation/read verifies hashes. A missing or wrong-size stored object is an error.
- `versions` uses stable-ID pagination with at most 1,000 rows; `read_version` only fetches a version bound to the requested URI, verifies length/hash, and enforces the store byte cap. There is no public fetch-by-hash bypass across roots.
- Every operation checks a caller-provided unlock/source guard and the current enrolled approved root, actual grant, and revocation tombstones. Preserve rechecks inside an immediate authority transaction. Guards run after potentially blocking writer admission and immediately before commit; final disclosure is rechecked too. **Guards must validate only: never mutate StateStore or acquire a writer.**
- Source-size mismatch, I/O failure, guard rejection and overflow/cap failures cannot register partial evidence. Revocation preserves raw objects and provenance but denies subsequent store access, even after forged SQL reapproval.

Errors have stable codes: `MP_DOCUMENT_DENIED`, `MP_DOCUMENT_OBJECT_CORRUPT`, `MP_DOCUMENT_LIMIT`, `MP_DOCUMENT_CHANGED`, plus existing state errors.

## Verification and independent review

- Independent reviewer examined MP-4D storage/schema changes against the MP-4B checkpoint. It identified an unlock/source guard window around SQLite writer waits and a forged-grant fixture missing `approved_at`; both were corrected and re-reviewed. No remaining concrete P1/P2 findings in MP-4D.
- Both transaction-security regression tests were observed failing with one committed version before the fix, then passed with zero authority/head changes. Focused independent verification: **12 object tests + 2 migration tests PASS**.
- Coverage includes bounded/error/empty streams, unchanged hints versus force hashing, history/restart/dedup/pagination, wrong-URI denial, pending/forged/unknown/revoked roots, mid-read revocation, stale unlock while waiting for a writer, precommit guard failure, raw corruption, symlinked objects/directories (Unix), injected SQLite failure and orphan recovery, v5 backup/reopen and transactional v6 migration rollback.
- Repo contract, TypeScript and Rust format checks PASS at the code checkpoint. Full `npm test` and exact-final-head cloud Windows product/storage qualification are pending at this document checkpoint; use PR/Notion receipts for the final result.
- `npm run build` reached web build successfully, then Linux Tauri failed because GLib/GTK development packages are absent. Cloud Windows CI is the qualified product build check; Linux desktop build success is not claimed.
- Cargo resolution regenerates pre-existing missing `httpdate`/`tokio-macros` and index `chrono` edges from the baseline lockfile. MP-4D promotes already-present `tempfile` from dev-only to production; it introduces no new locked package. Unrelated resolution drift is excluded from this unit; no frozen `--locked` qualification is claimed.

## Limits and handoff

LOCAL_REQUIRED: **NONE for this cloud unit**. HUMAN_REQUIRED: **NO**. Phase MP-4L1/MP-4L2/MP-4U remain separate later acceptance gates.

No local machine or private archive was accessed. Scanner/watcher, extractors, retention policy/GC, unified retrieval/UI, source identity routing and encryption are outside this unit. The metadata probe does not detect equal-size/equal-timestamp tampering; callers must force verification when necessary. Windows power-loss directory-entry durability and hostile native filesystem races remain native qualification work; cloud simulation does not certify them. Raw storage is not encrypted at rest.

Next: finish exact-head CI, integrate after MP-4B, then compose this store with MP-4C's authorized scanner and MP-4E's derived extractors. Recovery: fetch the pushed branch/PR checkpoint; revalidate claim generation and upstream v5; rerun focused storage tests and the full baseline before publishing further changes. An interrupted cloud session is not transfer authorization.

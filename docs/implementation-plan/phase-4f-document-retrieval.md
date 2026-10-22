# MP-4F — Cloud document retrieval integration

Task claim: [#31](https://github.com/KaisAbiyyi/mind-place/issues/31). Base `9f9038e05f00fb5554a6735b253ca2cb60365ca0`. Owner ChatGPT Cloud session `2026-10-22-mp4f-01`, single writer lock `document-integration/retrieval`.

## Security-first contract

- Explicit desktop-owner root scan and document search only after unlock. First indexing is owner initiated and also opts that root into a bounded 5-minute background reconciliation schedule. No automatic filesystem inspection on startup, no MCP exposure and no implicit project-to-folder authority.
- Polling is capped at 16 active roots and is session-scoped (Lock, revoke and owner Pause all cancel it). The scheduler rechecks lease, root approval and NTFS identity in the scanner; failed scans do not publish partial index generations. Search checks native bytes before disclosure even between polling cycles. A scheduler cap is reported without invalidating a successful manual scan.
- MP-4C enumerates by NTFS relative handles and MP-4D preserves immutable versions. MP-4E extracts bounded document chunks with cooperative cancellation and no network or OCR. Failure cannot publish a partial indexed generation.
- Document-derived SQLite FTS5 projection is disposable/rebuildable and isolated from the v1 conversation index schema. Document projection schema v2 stores extractor revision and bounded parser warning JSON for each chunk, while source bytes remain immutable MP-4D objects. Authorized v1 projections are atomically reconstructed as empty v2 projections; they must be reindexed. Foreign SQLite identities are rejected without adopting or rewriting them. Every candidate's saved provenance and text must match a fresh authorized extraction, with live root, version and file bytes checked before disclosure. Root revocation or lock fences disclosure.
- Search never exposes native filesystem paths or raw object storage, and desktop renders snippets/warnings with inert text nodes. The 5-minute poll is conservative periodic reconciliation, **not** a lossless OS watcher: new files appear after the next successful scan. Failures or long-running scans may delay discovery; search refuses stale file bytes in the meantime. MP-4L1 remains the real NTFS race/event qualification gate.

## Qualification / acceptance

- Cloud Windows CI: format, repository rules, Rust fixtures, TypeScript tests, desktop build.
- Cloud storage qualification 10K, 100K and 1M remains regression gate. MP-4L1 real Windows filesystem races, MP-4L2 representative corpus and MP-4U owner UX remain mandatory.
- No local checkout, archive or personal files accessed. Update this receipt with exact tested head/CI/limitations after review.

## Cloud schedule and provenance tests

- Registry cancellation and old-worker replacement are unit tested; no worker starts before the owner's first Index command.
- Derived-schema migration, foreign-DB identity, forged parser revision/warning suppression, live file edits, revocation and inert UI rendering have isolated regressions.
- Final artifact remains a draft until exact-head Windows product CI and all storage regressions pass and the review gate is clear. Local Windows/owner qualification remains MP-4L1/MP-4L2/MP-4U.

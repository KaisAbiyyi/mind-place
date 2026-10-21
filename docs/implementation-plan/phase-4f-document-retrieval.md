# MP-4F — Cloud document retrieval integration (in progress)

Task claim: [#31](https://github.com/KaisAbiyyi/mind-place/issues/31). Base `9f9038e05f00fb5554a6735b253ca2cb60365ca0`. Owner ChatGPT Cloud session `2026-10-22-mp4f-01`, single writer lock `document-integration/retrieval`.

## Security-first contract

- Explicit desktop-owner root scan and document search only after unlock. No automatic filesystem inspection on startup, no MCP exposure and no implicit project-to-folder authority.
- MP-4C enumerates by NTFS relative handles and MP-4D preserves immutable versions. MP-4E extracts bounded document chunks with cooperative cancellation and no network or OCR. Failure cannot publish a partial indexed generation.
- Document-derived SQLite FTS5 projection is disposable/rebuildable and isolated from the v1 conversation index schema. Every returned candidate is checked against live approved root, current version and source bytes. Root revocation or lock fences disclosure.
- Search never exposes filesystem paths or raw object storage, and desktop renders snippets with inert text nodes. File changes require an explicit refresh; results are best-effort not a completeness guarantee without a native watcher/MP-4L1.

## Qualification / acceptance

- Cloud Windows CI: format, repository rules, Rust fixtures, TypeScript tests, desktop build.
- Cloud storage qualification 10K, 100K and 1M remains regression gate. MP-4L1 real Windows filesystem races, MP-4L2 representative corpus and MP-4U owner UX remain mandatory.
- No local checkout, archive or personal files accessed. Update this receipt with exact tested head/CI/limitations after review.

# MP-4B — NTFS approved-root authorization and handle containment

Cloud claim `2026-10-21-mp4b-01`, base `3b56726a9eadb9e65b46788b900b2edd9be2719f`, lock `folder-authority`, prerequisite MP-4A merged #23. Claim recorded in Notion and [draft PR #26](https://github.com/KaisAbiyyi/mind-place/pull/26); acceptance depends on exact final-head CI and review.

## Security contract

- Explicit local owner approval after staging, only with active unlock lease. Pending is never access. Terminal revocation denies all future reads. Public/MCP clients do not gain approval or direct path APIs.
- Bind each grant to the on-disk root's volume/file identity. After process restart or root replacement, mismatch denies rather than silently approving a different directory.
- Open every child from a verified directory handle, without following reparse points; reject traversal and disallowed filesystem objects. Avoid path-string `File::open` for descendants.
- Reads are bounded, and authorization is verified before and after reading. Revoked data is never returned after the final check. Scanner/parser integration is deferred to MP-4C/4E.
- Windows NTFS / junction / rename / root-replacement native acceptance remains MP-4L1. Cloud CI exercises Windows synthetic fixtures but is not local-owner verification.

## Evidence

## Implementation map

- `crates/state-core/src/schema_v5.sql`, `lib.rs`: backed-up, checksum-validated v5 migration; explicit immutable root object grants, never auto-adopting v4/legacy roots.
- `crates/state-core/src/document_roots.rs`: pending-to-approved owner-only transition, terminal tombstone revocation, active binding checks, 32 MiB bounded document reads with post-read authorization check.
- `crates/state-core/src/ntfs.rs`: stable Windows volume+file identifier; open descendants relative to already-held directory handles using NT Native File API and no-reparse flags. Reject reparse points and multiply-hardlinked files; fail closed on unsupported platforms.
- `src-tauri/src/main.rs`, `src/document-roots.ts`: lease-gated Tauri owner commands, explicit approval and double-confirm revocation. No new MCP endpoints or scanning.
- `crates/state-core/tests/document_roots.rs`, `test/document-roots.test.ts`: unauthorized/staged reads, fake approval, revoked grant, size bounds, Windows relative-handle symlink/hardlink/root-replacement, consent UX and cleanup.

## Cloud qualification / limitations

- Cloud Windows jobs check formatting, SQLite migration and rollback compatibility, Rust/TS tests, desktop bundle and storage tiers (10K/100K/1M). Follow the exact final PR head; intermediate failed format/compile/fixture runs are not acceptance.
- After formatting/visibility repairs, Windows test log on preliminary head `7ac1f4c` showed **PASS** for root replacement and reparse/hardlink tests. The fake-grant negative test fixture violated the existing `approved_at` constraint; corrected in `07c41cd`. Full final-head CI remains required before merge.
- No user filesystem was opened during development; GitHub-hosted Windows runners use synthetic fixtures. Real NTFS devices, junction race stress and user-owned folder consent remain **MP-4L1 and MP-4U**, never inferred from cloud CI.
- `read_document_bytes` is an internal bounded primitive for later MP-4C/4F; no scanner, watcher, automatic ingest, MCP document reads or indexing is introduced in 4B.
- Native `NtCreateFile`/file identity safety needs critical review and later real Windows qualification before real private-root ingestion is enabled.

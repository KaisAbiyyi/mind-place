# MP-4B — NTFS approved-root authorization and handle containment

Cloud claim `2026-10-21-mp4b-01`, base `3b56726a9eadb9e65b46788b900b2edd9be2719f`, lock `folder-authority`, prerequisite MP-4A merged #23. This is a checkpoint, **not** acceptance.

## Security contract

- Explicit local owner approval after staging, only with active unlock lease. Pending is never access. Terminal revocation denies all future reads. Public/MCP clients do not gain approval or direct path APIs.
- Bind each grant to the on-disk root's volume/file identity. After process restart or root replacement, mismatch denies rather than silently approving a different directory.
- Open every child from a verified directory handle, without following reparse points; reject traversal and disallowed filesystem objects. Avoid path-string `File::open` for descendants.
- Reads are bounded, and authorization is verified before and after reading. Revoked data is never returned after the final check. Scanner/parser integration is deferred to MP-4C/4E.
- Windows NTFS / junction / rename / root-replacement native acceptance remains MP-4L1. Cloud CI exercises Windows synthetic fixtures but is not local-owner verification.

## Evidence

Pending implementation, review, and final-head CI.

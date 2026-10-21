# MP-4C — cloud scanner and watch reconciliation

Claim: ChatGPT Cloud, session 2026-10-22-mp4c-01, GitHub issue #29. Resource: folder-scanner. Base: 32eb086e6ae841d37cc30cca7659bb5d9eb39d78.

## Contract

- Scanner runs only after an explicit owner-unlock caller invokes rescan and supplies a live unlock guard. There is no startup auto-adoption, background directory scanning, or MCP document access in MP-4C.
- Every rescan is bounded (10,000 entries, depth 24, 32 MiB per file by default) and explicitly allows common document/source extensions while skipping cache/build directories and reparse points. Policy may only tighten these global ceilings.
- Folder enumeration itself now uses NTFS handle-bound `NtQueryDirectoryFile` over 4B-verified root/directory handles; reparse points are rejected, and no path-string `read_dir` occurs. Files are also opened only by 4B NTFS handle-relative verified opens. The same approved root binding and unlock guard are rechecked during the 4D immutable content store commit.
- The watcher contract is an invalidation API, not a claim of lossless filesystem notification. Change, deletion, rename, overflow or disconnect marks the root dirty and demands full rescan. On restart, all roots begin dirty; an incomplete/failed rescan never publishes a current inventory.
- The in-memory inventory is never authorization or derived index state. Removed files are removed from the current inventory but immutable version history remains preserved. Current URI discovery rechecks current root approval and unlock.
- MP-4F must integrate native event input / scheduling, persist or rebuild its derived search membership, and filter **every** search/fetch hit against live root authorization and current inventory. No stale hit can become a filesystem authority.
- MP-4L1 must qualify actual Windows NTFS enumeration races, junctions/reparse, watcher event loss, root-replacement and revoke-during-indexing on representative owner paths. CI synthetic fixtures never substitute that gate.

## Evidence and acceptance

- Rust scanner tests cover bounded policy, lost/duplicate watcher hints, clean rescan, change, rename, exclusion, revocation, and unlock abort.
- Validate exact final head with cloud Windows CI (format, repo verification, Rust/TS tests, build) and storage qualification (10K/100K/1M).
- This cloud unit introduces no state migration or destructive deletion. Existing MP-4D immutable object store and 4B root authority are reused.

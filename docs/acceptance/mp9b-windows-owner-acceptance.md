# MP-9B — owner Windows acceptance (not yet executed)

**Status: NOT RUN.** GitHub-hosted Windows CI and synthetic SQLCipher benchmarks are not substitutes for the owner's installed application and native browser-host behavior. All tests below use a new, dedicated **synthetic profile**, never the owner's existing archive, DPAPI credentials, browser history, or recovery secrets.

## Preflight: isolate and attest

1. Obtain a signed/traceable Windows artifact built from the final, reviewed MP-9B commit. Record repository, full commit SHA, build workflow URL, SHA-256 of installer, desktop executable and capture-host executable, Windows version, and WebView2 runtime version. **Do not test a partial PR build against real user data.**
2. Choose a newly created empty path (for example `D:\MP9B-Synthetic-Acceptance\archive-v1`). Point `MIND_PLACE_DATA_DIR` to this absolute synthetic archive root for **both** desktop and native host processes. Verify neither executable resolves the owner’s normal `%APPDATA%\Mind Place`. If isolated host/desktop root parity cannot be proved, **STOP**.
3. Never delete, reset, or migrate any existing directories; preserve all failed staging directories and encrypted captures for forensic examination.

## Mandatory production checks (release-blocking)

| ID | Procedure | Required acceptance |
| --- | --- | --- |
| WIN-01 | Install into empty, isolated synthetic profile; launch first-run. | No plaintext `state.sqlite3`, raw archive, unkeyed index, or document-object materialization before recovery confirmation. UI must not merely initialize an unlock PIN. |
| WIN-02 | Provision recovery code, abort before confirmation, re-launch. | No activated vault; no exposed code in logs, URLs, localStorage, temp files, process args, or telemetry. |
| WIN-03 | Confirm recovery code, restart app, unlock via DPAPI; test wrong code in isolated restore profile. | Valid code restores encrypted state, invalid key rejects without overwrite or fallback. Key bytes disappear from UI after confirmation. |
| WIN-04 | Register Brave/Chrome native host and capture synthetic ChatGPT/Claude/Gemini conversations using the extension. | One authenticated owner-session writer, never direct legacy plaintext `ArchiveStore::save` in vault; identity and epoch checks, no plaintext artifacts in the isolated profile. |
| WIN-05 | Search/retrieve/read archive, snapshot, attachment, memory, document preview, index and FTS; lock and retry each command. | Correct fidelity while unlocked; lock/revoke stops every path and no stale lease/replay succeeds. |
| WIN-06 | Start concurrent indexed writes + native capture; trigger owner-approved online full-vault backup. | Manifest represents one consistent generation over state, all indexes, archive and document objects; no mixed-generation references, WAL omission, silent file omission, partial or plaintext backup. |
| WIN-07 | Restore backup into a distinct new isolated profile; unplug/interrupt during staging, resume/rollback. | Exact object AEAD, schema, counts, identities, recovery binding verified. Existing source unchanged; partial destination never becomes active. |
| WIN-08 | Migrating an explicit synthetic legacy fixture, reject unknown file, live WAL, symlink/junction and insufficient disk; restart after each phase. | Migration only with specific owner consent; full source integrity, deterministic rollback, no deletion of legacy source without a separate recorded approval. |
| WIN-09 | Force-lock, crash/restart the native host, browser profile switch, auth expiry and stale/replayed IPC requests. | No capture of privileged content without current owner session; no cross-profile or stale-key reuse. |
| WIN-10 | Independent reviewer signs off on final SHA and P0/P1 disposition. | Reviewer is not implementor; verified source commit exactly matches installed artifact; no unresolved P0/P1. |

## Performance qualification

- Use at least three independently repeatable synthetic corpus sizes, including a large fixture with many snapshots, messages, and document objects. Compare plaintext legacy baseline (synthetic only) to the encrypted candidate **on the same Windows owner machine**.
- For each profile/size, run at least five cold and five warm iterations under controlled idle and loaded conditions. Record CPU, memory private bytes/peak working set, startup, 95th percentile capture/read/search/write latency, archive+WAL+SHM/index/object bytes, backup and restore wall times; record process counts and power state.
- Report medians, high-percentile distributions, environment details, test ordering, absolute and relative overhead. Do not infer SLA from a single GitHub-hosted benchmark or conflate the native host's memory with Tauri memory.
- Preserve structured, redacted receipts with binary SHA, fixture seed/size, run timestamps, failures, and environment. Never attach plaintext fixtures with real user content.

## Current cloud handoff

As of 2026-10-26 this PR includes a cooperative cross-process writer fence for encrypted immutable vault writes and recognized SQLCipher state/index/document FTS connections, a quiesced portable backup, a read-only migration preflight, and isolated recovery-confirmed first-run StateStore preview. **Production Tauri/native host, global online backup coordinator, complete consented migration and rollback, recovery UI, and independent final review are still missing.** This document is a test protocol, not acceptance evidence. PR #127 remains DRAFT / Changes Required.

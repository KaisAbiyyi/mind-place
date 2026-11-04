# MP-9B security and rollout gates — cloud reclaim, 2026-10-25

**Status:** CHANGES REQUIRED / PR #127 DRAFT. This is an implementor's security self-review and acceptance handoff, **not an independent security review**. No real owner archive, key, plaintext backup, or Windows installation was touched.

## Verified baseline

- Pre-reclaim exact HEAD `d4489378e6cb91a62558cbb1efa3c3e2449f6d05`: SQLCipher Ubuntu+Windows (#40201083218), Windows Product (#40201083286) and Phase 1E Storage (#40201083207) completed **SUCCESS**.
- Reclaim branch adds fail-closed backup inventory checking, support for encrypted backup objects, and read-only legacy migration preflight. **Requalify on final HEAD**; earlier success does not establish acceptance of new commits.
- Synthetic 160-source native StateStore run on GitHub-hosted Windows: plaintext writes **4156 ms**, encrypted writes **4193 ms**; plaintext reads **1416 ms**, encrypted reads **1484 ms**; reported state+WAL+SHM bytes **389120** for both. Ubuntu: **1603/1667 ms** writes, **1432/1459 ms** reads. These are single, short, sequential synthetic tests; do not infer a user-device SLA, concurrency, multi-GiB cost, or storage parity.

## Blocking security findings (severity = blocker priority, not an assertion of exploitation)

### P0 — First-run is still plaintext, not encrypted-by-default

`src-tauri/src/main.rs` initializes `ArchiveStore::open(root)` in `.setup()` and calls `StateStore::open(state_path)` in both `initialize_security` and `unlock_security`. `src/security.ts` correctly warns that the legacy security gate is not data-at-rest encryption. New installs are **not** protected by SQLCipher yet. Never market or release this PR as mandatory encryption.

**Required:** isolate legacy from new-install detection; stage an encrypted vault before any raw materialization, confirm secure offline recovery export before activation, use keyed state/index/FTS and encrypted immutable archive/object I/O, then audit every Tauri/MCP/tunnel/document-refresh call path for plaintext fallback.

### P0 — Native capture host cannot currently write a live encrypted vault

`crates/capture-host/src/lib.rs` calls plaintext `ArchiveStore::open(root)?.save(capture)`. The vault marker guard prevents a plaintext write inside a recognized vault (positive containment), but does not provide an encrypted writer. A separate native host process must not invent an unlocked epoch or load raw keys from command-line arguments, environment variables, or unprotected files.

**Required:** authenticated owner-session IPC/attestation to a Tauri-owned encrypted capture writer; epoch/lock/replay checks, origin validation, backpressure, restart and profile isolation tests. Do not bypass the guard.

### P0 — Portable backup is still offline-only

`crates/security-core/src/vault_backup.rs` now refuses unknown vault files and active SQLite WAL/SHM/journal, inventories state/index/document-index + encrypted archive/document/backup objects, verifies source inventory and SHA-256 before atomic publish. These checks do not create cross-process writer exclusion. A write after the final hash, between multiple database snapshots, or during object-generation advancement can still produce a mixed-generation backup if callers fail to stop writers.

**Required:** global writer fence shared by Tauri, native host, MCP and background indexers; SQLCipher online snapshot of each database at a single generation boundary; manifest of immutable evidence references; restore all and independently verify SQLCipher identity, object AEAD, index reproducibility, and replay/rollback.

### P0 — Legacy migration and rollback are not executable

`crates/security-core/src/vault_migration.rs` is a read-only preflight for known legacy state/archive/document files with bounded scan and source change checks. It intentionally performs no source modifications and cannot be counted as a complete migration.

**Required:** owner consent specifically naming source and destination, encrypted temporary destination, crash-resumable per-phase journal, source and destination identity readback, retention of original plaintext until explicit approval, interruption/restart tests, recovery behavior when disk is full, symlink and NTFS junction testing.

### P1 — Recovery UI and lifecycle are absent

The vault's independent recovery secret now has a versioned typo-detecting export/import format. No end-to-end first-run display/confirmation, secure copy/export opt-in, lost-key guidance, DPAPI rebind/rotation, or restoration wizard is yet integrated with Tauri.

**Required:** secrets never enter logs, telemetry, URLs, command-line flags, plain local storage, or long-lived UI state; prove explicit confirmation and code-clearing after use; verify account/device change and wrong-key recovery from an isolated backup.

### P1 — Independent security sign-off and Windows owner acceptance missing

No independent reviewer has yet approved the final diff or threat model. Hosted Windows CI is not a physical owner-installed WebView2/native-host qualification. Keep all missing evidence marked **NOT_RUN** rather than PASS.

**Required:** independent review by someone other than the implementor, no unresolved P0/P1 findings, exact SHA/migration ledger, Windows build identity and host pairing on the owner's isolated synthetic test profile, malicious input/restart/lock/replay tests, performance repetitions on controlled idle vs loaded states, no data deletion without separate owner consent.

## Merge and board policy

PR #127 must remain **DRAFT / DO NOT MERGE** until every production code path and acceptance gate above is satisfied. MP-9B stays **In Progress / Changes Required**; immediate successors MP-9C, MP-9D and MP-9E stay **Blocked**. Shared logical lock: `crypto-migration`. Treat any subsequent code commit as invalidating previous exact-head CI proof until new checks pass.

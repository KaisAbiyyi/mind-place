# Phase 1B: authoritative state authority — completed

Implementer contract:
- This unit adds only authoritative mutable state. Raw `archive-v1` JSONL remains immutable and external to SQLite. `index.sqlite3` is Phase 1D.
- `state.sqlite3` lives beside `archive-v1`, not inside a capture conversation.
- Phase 1A's `security-v1.json` verifier remains the active authentication source. This phase does not migrate plaintext passwords, store secrets, or claim encryption-at-rest.
- SQLite is compiled/bundled via pinned `rusqlite` and Cargo.lock. Report the actually bundled SQLite version in CI; upgrading to SQLite 3.54.1+ remains a deliberate qualification checkpoint.
- Gate initialization behind successful local unlock; no MCP, capture-host, or folder API is added by this unit.
- V1 is an append-oriented, typed schema foundation. Project/source IDs, memory transition policy, approved-root filesystem logic, client grant endpoints, and full credential persistence remain later scoped units.
- The default for new source roots and clients is `pending` / `disabled`, never implicit approval.

Run `cargo test -p mind-place-state`, full workspace tests, rustfmt, TypeScript checks, and Windows desktop build.

Security and recovery invariants:
1. Foreign DB, future schema, invalid checksum and malformed DB fail closed.
2. New schema is one SQLite transaction. Migration from recognized v0 uses a verified `VACUUM INTO` snapshot backup first (including committed WAL content).
3. Old canonical rows survive failed migrations; never attempt silent recovery/overwrite.
4. Mutable source root revocation is audited in append-only tombstones.
5. Archived JSONL is never read or rewritten by the authority layer.

## Completed cloud handoff (2026-10-18)

- **UNIT:** Phase 1B / authoritative SQLite state.
- **Branch:** `feat/phase-1b-state-authority`.
- **PR:** https://github.com/KaisAbiyyi/mind-place/pull/2 — squash-merged.
- **Qualified code SHA:** `32278f6a2a71a5211c429d86b5e377284d95acf0`.
- **Windows CI:** https://github.com/KaisAbiyyi/mind-place/actions/runs/39099905432 — success (Rust format, repository contract, TypeScript checks, bundled SQLite runtime, full web/Rust tests, desktop build).
- **Bundled SQLite:** `3.53.2` on Windows CI, from pinned `rusqlite = 0.40.2` / `libsqlite3-sys = 0.38.2`; qualify a future `3.54.1+` upgrade separately.
- **Changes:** Transactionally initialized state authority, verified migration snapshots, project/topic/scope/tag foundation, memory claims and append-only claim/revocation events, pending root/client policy foundations, and post-unlock desktop integration.
- **Recovery proof:** Unknown or future database schemas fail closed; invalid migration checksum fails closed; failed migration retains old evidence and backups; committed WAL content is included in migration backup; Windows backup fsync uses a write-capable handle.
- **Local required:** No; Windows CI provides the Phase 1B compatibility gate. Local checkout is reserved for later explicit qualification.
- **Known limits:** No source identity mapping until Phase 1C, no disposable FTS index until Phase 1D, no capture-host/MCP authorization propagation or encrypted raw archives yet.
- **NEXT:** `implement phase 1C` — provider-neutral canonical IDs, deterministic/persisted identity mapping, rollback/recovery hardening and referential stability.

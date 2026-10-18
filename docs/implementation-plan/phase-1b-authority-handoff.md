# Phase 1B: authoritative state authority

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

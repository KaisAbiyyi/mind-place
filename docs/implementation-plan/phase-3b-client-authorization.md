# Phase 3B — durable client authorization and audit

Status: **DONE**. [PR #11](https://github.com/KaisAbiyyi/mind-place/pull/11) squash-merged at `b88a26f28a6a601f9b307049946d7b9547e1907c`. [Windows CI 39129016103](https://github.com/KaisAbiyyi/mind-place/actions/runs/39129016103) and [10K/100K/1M storage qualification 39129016073](https://github.com/KaisAbiyyi/mind-place/actions/runs/39129016073) passed.

## Boundaries

Authoritative state.sqlite3 schema v3 adds client_credentials (SHA-256 digest of 256-bit OS-random bearer secret) and append-only client_audit with immutable SQL triggers. Existing enabled v1/v2 clients are not provisioned with credentials during migration. Backup, migration checksum, FKs, and DB identity remain verified.

ClientAuthority is for the trusted unlocked local owner code only. It issues, rotates, disables, revokes, and updates capabilities with an unlock lease; no external protocol routes call this owner API. IssuedClient never derives Debug, Clone or Serialize and zeroizes its one-time credential. Never persist credential plaintext or put bearer secrets in URLs or logs.

read, write, admin are explicit independent grants with no inheritance. Credential hash + enabled state + live capability are read from SQLite on every authorization callback, including mid-batch, not cached. Rotation and removal invalidate old access. Revoked clients cannot be re-enabled. Neither writes nor admin tools are available over the 3A dispatcher.

IntegrationCore::execute_client_batch is the credential-gated 3B entry point and reuses 3A all-or-nothing batches. Metadata-only read attempt and completion/failure decisions are audited without queries, evidence, tokens, source URLs or PINs. Local locked requests do not open the authority DB.

## Qualification

Windows CI baseline: cargo fmt, verify:repo, typecheck, npm test and desktop build. New tests exercise v2→v3 migration and backup, legacy default-deny, immutable audit, locked processing, token storage, read/write/admin isolation, immediate revocation, disable, rotation, and batch invalidation.

Cloud-only unit. No direct local or hosted MCP transport until 3C/3D; no manual Windows qualification required now. Next: Phase 3C local MCP transport.

## Final cloud handoff

- Phase 3B code SHA: `b88a26f28a6a601f9b307049946d7b9547e1907c` (`main`).
- CI: format, repository verification, TypeScript typecheck, SQLite, full tests and Windows desktop build — **PASS**.
- Storage benchmark: 10K, 100K and 1M message chunks — **PASS**.
- No Windows local/manual gate for 3B; 3L1 and 3L2 remain later phase gates.
- Next cloud phase is **3C — local MCP transport**. Use only the credential-gated dispatcher, never unconditional authorizer closures in any exposed route.

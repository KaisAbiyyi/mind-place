# Phase 3B — durable client authorization and audit

Status: cloud implementation; requires Windows CI before merge.

## Boundaries

Authoritative state.sqlite3 schema v3 adds client_credentials (SHA-256 digest of 256-bit OS-random bearer secret) and append-only client_audit with immutable SQL triggers. Existing enabled v1/v2 clients are not provisioned with credentials during migration. Backup, migration checksum, FKs, and DB identity remain verified.

ClientAuthority is for the trusted unlocked local owner code only. It issues, rotates, disables, revokes, and updates capabilities with an unlock lease; no external protocol routes call this owner API. IssuedClient never derives Debug, Clone or Serialize and zeroizes its one-time credential. Never persist credential plaintext or put bearer secrets in URLs or logs.

read, write, admin are explicit independent grants with no inheritance. Credential hash + enabled state + live capability are read from SQLite on every authorization callback, including mid-batch, not cached. Rotation and removal invalidate old access. Revoked clients cannot be re-enabled. Neither writes nor admin tools are available over the 3A dispatcher.

IntegrationCore::execute_client_batch is the credential-gated 3B entry point and reuses 3A all-or-nothing batches. Metadata-only read attempt and completion/failure decisions are audited without queries, evidence, tokens, source URLs or PINs. Local locked requests do not open the authority DB.

## Qualification

Windows CI baseline: cargo fmt, verify:repo, typecheck, npm test and desktop build. New tests exercise v2→v3 migration and backup, legacy default-deny, immutable audit, locked processing, token storage, read/write/admin isolation, immediate revocation, disable, rotation, and batch invalidation.

Cloud-only unit. No direct local or hosted MCP transport until 3C/3D; no manual Windows qualification required now. Next: Phase 3C local MCP transport.

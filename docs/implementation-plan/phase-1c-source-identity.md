# Phase 1C — source identity and authority durability

Status: **DONE** — PR #3 squash-merged at `d007e80b079471bcb66661fb90feba907d837dc2` after Windows CI run [39105339145](https://github.com/KaisAbiyyi/mind-place/actions/runs/39105339145) passed.

## Identity contract

Only authoritative state may register durable source referents. Source IDs are
versioned, provider-neutral `src:v1:<lowercase-sha256>` identifiers. They are
derived from a canonical, length-framed UTF-8 tuple with the fixed domain prefix
`mind-place:source-identity:v1\\0`:

1. provider token (lowercase ASCII, 1–64 bytes);
2. stable account/import namespace (1–256 UTF-8 bytes, explicit; **not** a cache
   generation, randomized search/index rowid, session PID or absolute local path);
3. source kind token (e.g. `conversation`, `message`, `document_version`);
4. identity key type `external` or `raw_sha256`;
5. original external source key, or lowercase SHA-256 digest of immutable raw
   evidence, respectively.

Each part is encoded as an unsigned **8-byte big-endian byte length** followed
by the exact bytes. The provider/source kind tokens are lowercase, but external
keys and namespaces are case sensitive and never silently normalized. The
`raw_sha256` key identifies immutable objects/versions only; it is **not** a
substitute for a durable conversation/document identity when content changes.

The `source_identities` table in `state.sqlite3` persistently binds all
canonical IDs to their complete origin tuples. Re-registering the same origin
is idempotent; different namespaces/providers/kinds/keys cannot intentionally
alias. Querying a mapping verifies the hash, and database open validates the
mapping table before an unlocked service can rely on it.

The future derived `index.sqlite3` may copy these IDs for search acceleration
but must never create a canonical identity by copying its own rowid. Derived
indexes can be deleted/rebuilt without changing canonical source IDs.

## Evidence references and migration

`claim_evidence.source_ref` now has a foreign key to
`source_identities.id`; legacy unbound references are **not** silently
rebound. The migration to schema v2 copies V1 evidence in the same transaction;
an orphan unbound text reference aborts the migration, leaves V1 untouched and
leaves a verified pre-upgrade snapshot for explicit inspection/recovery. A
caller must register the real raw/provider source and reconcile the legacy
evidence before retrying; no fabricated placeholder/derived ID is accepted.

Version 0→2 and version 1→2 migrations take one verified
`VACUUM INTO` backup before mutation and apply the pending schema steps in a
single transaction. V1 checksum is validated **before** upgrade; V2 checksums,
foreign keys, DB health and canonical mapping hashes are validated on open.
Unknown, foreign, corrupt or future schemas fail closed. Backups are
non-destructive; automatic replacement/restore is deliberately unsupported.

API surface provided by `mind-place-state`:
- `canonical_source_id(&SourceDescriptor)` for deterministic precomputation;
- `StateStore::register_source` for durable, idempotent binding;
- `StateStore::source` for verified origin resolution;
- `StateStore::add_claim_source_evidence` for authority-registered evidence.

Source registration is **not** filesystem approval or an MCP permission grant.
Only call these authority operations after the Phase 1A unlock boundary.
Capture normalization and indexing are Phase 1D; routing/import adapters must
supply their own stable origin namespace rather than assuming every provider
exposes an account ID.

## Verification

The integration suite proves:

- canonical equality across restart/rebuild and divergence by namespace,
  provider, kind and key type;
- idempotent registry, no dependency on `index.sqlite3`;
- rejection of temporary index references even through direct SQL with FK ON;
- V1→V2 upgrade, verified V1 snapshot and once-only backup;
- V1 orphan-reference rollback without authoritative evidence loss;
- checksum-preflight rejection and fail-closed altered identity mappings.

Cloud Windows CI runs the Rust tests as part of the existing workspace test
suite. Phase 1 needs no local/manual acceptance gate by default.

## Completed cloud handoff (2026-10-18)

- **UNIT:** Phase 1C — stable identity and authority durability.
- **PR:** https://github.com/KaisAbiyyi/mind-place/pull/3 — squash-merged.
- **Qualified code SHA:** `d007e80b079471bcb66661fb90feba907d837dc2`.
- **Windows CI:** https://github.com/KaisAbiyyi/mind-place/actions/runs/39105339145 — success; Rust formatting, repository contract, TypeScript typecheck, bundled SQLite gate, full web/Rust tests and Tauri desktop build.
- **Schema/protocol:** authoritative schema v2; deterministic versioned source IDs; provider/account/import namespaces, origin keys and typed source references; no derived index added.
- **Recovery:** V0/V1 transactional migration, snapshot verification, orphan-reference rejection/rollback, checksum checks, foreign key integrity and identity-tamper fail-closed tests.
- **Local required:** No; Phase 1C is qualified on Windows CI. Do not modify local checkout for implementation.
- **Known limits:** Existing V1 unbound evidence cannot be auto-adopted; explicit origin-based reconciliation is required. Phase 1D must implement archive adapter, `index.sqlite3` rebuild and FTS5 while reusing the existing canonical ID contract. Raw-object digest authenticity must be validated by the future ingestion adapter before registration. At-scale identity open-validation cost belongs in the Phase 1E benchmark.
- **NEXT:** `implement phase 1D`.

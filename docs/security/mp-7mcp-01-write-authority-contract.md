# MP-7MCP-01 — Scoped MCP write-authority contract

**Status: PROPOSED; NOT APPROVED; production MCP stays READ-ONLY.**  
Task: MP-7MCP-01 | Lock: mcp-write-authority | Baseline main: 24648c74f3f59e176974d3878049b882478db69b | 2026-11-06.

References: [canonical ADR register](https://app.notion.com/p/40ac4a6aaf7381aa93cdc99957ae9fee), [vault security](https://app.notion.com/p/40ac4a6aaf7381969fddc8d0284131d2), [Phase 3B](../implementation-plan/phase-3b-client-authorization.md), [ADR-006 write amendment proposal](mp-adr-006-mcp-write-amendment-proposed.md).

## 1. Existing accepted boundary

MP-ADR-006 (accepted 2026-10-31) authorizes **read-only** MCP for explicit per-client approved collections. This proposal amends only future MCP write scope; it does not change manual unlock, per-user server, desktop-close, Win+L, logout/restart/crash behavior or existing read grants. Production write tools, listener routes, credentials and capabilities MUST remain disabled until exact operation scope is approved by owner, independently reviewed, implemented, regression tested and qualified on real clients. Neither this PR nor its merge is owner approval.

Phase 3B's historical independent read/write/admin capability bits are insufficient to expose MCP writes. A legacy client possessing a write bit has zero new MCP write authority until a fresh exact owner grant. Browser capture, local owner administration, and maintenance paths are separate principals and cannot lend their privilege to external clients.

## 2. Proposed separately-granted operation matrix

| Operation | Proposed eligible target | Prohibited / additional gate | Owner approval |
| --- | --- | --- | --- |
| create_import | New raw-preserving external agent-session import (original bytes/provenance preserved) or new curated memory, limited to approved import/curated collection | No replace of captured revision, invented original metadata, or arbitrary filesystem import | PROPOSED opt-in |
| append | New immutable import/evidence revision or bounded derived note/graph staging batch | No in-place raw edits; derived batch activation requires verification | PROPOSED opt-in |
| update | New superseding revision of editable curated memory, route assertion or authorized derived assertion, with exact expected_revision | No mutation of immutable raw source or silently overriding manual claims | PROPOSED opt-in |
| move | Logical collection/project/topic reassignment only, exact expected_revision | Require move grants for both current and destination collection; never move physical file/root | PROPOSED opt-in |
| delete | Recoverable tombstone for a specific curated/derived object | Separate destructive grant and interactive owner-confirmed scope; hard delete of archive, audit or raw bytes NEVER MCP | PROPOSED separately; DEFAULT DENY |

Every operation is **independent**. “Full authorized write” means every operation explicitly granted for that client and collection, not blanket write access. Optional Gemini does not inherit Claude/ChatGPT grants. New collections never inherit access. Wildcard/all-vault collection grants, unrestricted SQL, filesystem read/write, shell, root changes, key export, policy or grant administration, bulk export, backup/restore, auto-unlock and physical deletion are always denied. Hard delete requires a separate future owner decision.

## 3. Principal, grant and enforcement

The principal is a validated server-side credential/client identity with issuer and audience binding; a client-supplied provider/model string is not an identity. Each grant record binds: immutable client identity, vault identity, one canonical collection, object kind, **one operation**, enabled/revoked state, owner approval receipt, versioned policy revision, issue/expiry time, and optional request/byte/object limits. No wildcard/prefix/path membership, role inheritance or implicit read from write. Owner grants/change/revoke only through trusted interactive local UI, never MCP. A delete grant needs distinct informed confirmation.

The authorization predicate MUST check, in conjunction: authenticated currently enabled principal, exact issuer/audience, unlocked vault, expected live unlock epoch, matching vault, external access not paused, valid unexpired exact operation+object-kind+collection grant, current policy revision, resource limits, correct current authoritative object membership, required root/source authority, and source revision preconditions.

Validate at request admission, before every mutation, **inside final serialized writer transaction before commit**, and before all response disclosure (including snippets, metadata, receipts, cache, streaming, errors and pagination). A move verifies *both* authoritative source and destination collection under the same principal/operation before commit. After a write, independently apply the existing read scope; never reveal content solely because a write was permitted. A write-only principal receives only sanitized non-content result metadata.

Lock, revoke, token rotation, grant edit, external-access pause and unlock-epoch change fence queued/in-flight work; repeat authorization at commit and disclosure, abort or drain safely on stale policy. Closing desktop or Windows Win+L is not equivalent to Lock under accepted ADR-006. Logout, restart and crash must leave restarted server locked. If authority DB cannot be read safely, fail closed. Denials must not disclose object/collection existence, keys or native paths.

## 4. Idempotency, concurrency and recovery

Each mutation requires unpredictable operation_id, canonical body SHA-256, client/vault/operation/scope binding, and expected_revision for update/move/delete or append-at-head. Durable receipt identity: (vault_id, client_id, operation_id). Store digest, operation, scoped IDs, status, resulting revision, commit marker and redacted timestamp. Same identity/key/digest retry returns the previous receipt *only after fresh authorization and disclosure check*. Same key with changed digest/operation/scope is conflict. Another principal cannot borrow a receipt.

A single vault writer owns state changes. Stage new immutable bytes in non-visible authenticated storage, verify integrity, flush durably, then atomically publish with authoritative state/version check and audit receipt. Apply compare-and-swap expected_revision. Interrupted/pre-commit work stays invisible or quarantined; derived graph jobs remain staging until provenance/epoch/source-scope verification and one transaction activates the generation. If committed but acknowledgement lost, exact authorized retry reconciles the durable receipt. Never erase published raw revisions as “rollback”; use logical supersession/tombstone only where approved. Audit decisions with opaque metadata but no content, raw paths, passwords, keys, tokens or full payload.

## 5. Threat model and required negative tests

| Threat | Protection | Required proof |
| --- | --- | --- |
| Prompt injection or malicious model | Per-client independent operation and collection grant; no admin path | Unauthorized operation and wrong collection denied |
| Stolen/replayed token | Identity/audience validation, expiring grant, revocation, bound operation_id | Cross-client/cross-vault replay and post-revoke retry denied |
| Confused deputy and move | Server-resolved membership + dual collection grant | Wrong destination and stale membership denied |
| Lock/revoke/race | Epoch/policy fence inside writer and before disclosure | Lock during staged write: no unauthorized commit/output |
| Concurrent writers and retry | Vault single writer, expected_revision CAS, durable receipt | Identical retry one revision; divergent retry conflict |
| Crash/partial failure | Authenticated immutable staging, journal/quarantine, atomic activation | Fail at stage, commit, ack; source hashes and recovery checked |
| Write-to-read privilege escalation | Independent read predicate for response/cache/search | Write-only gets no source text |
| Destructive/inferred output | Tombstone-only, separate grant, staged derived provenance | Hard delete and source mutation never exposed |

## 6. Verification stages (truthful boundaries)

**Stage A — this PR:** security proposal + executable isolated reference-policy model + negative unit tests for default deny, wrong operation, collection, vault, epoch, revision, replay, revoke, pause, commit fence and independent read. These tests only verify a *reference model*, not actual production MCP authorization.

**Stage B — AFTER owner scope approval:** persist authoritative policy schema, wire server-side evaluation and operation-specific handlers in separate implementation tasks; run true integration negative tests against state/storage/transport, crash and concurrency fixtures, exact-source full checks (verify:repo, typecheck, npm test, Rust fmt and build) and hosted CI.

**Stage C — AFTER security review and Stage B:** separate Claude MP-7MCP-03, ChatGPT MP-7MCP-05, optional Gemini MP-7MCP-07 require their respective full-read predecessor, real-client write proof and Windows/native Lock/revoke qualifications. No provider write path may be advertised before these gates.

## 7. Owner decision record — PENDING

Proposed version MP-7MCP-01 v1. Allowed clients/providers: NOT APPROVED. Allowed collection/object kinds: NOT APPROVED. Allowed operations: NOT APPROVED. Destructive tombstone: NOT APPROVED. Owner approval source/time: NONE. Independent security review: NOT RUN. Real endpoint/native tests: NOT RUN. Current ADR-006 read-only remains ACCEPTED and unchanged.
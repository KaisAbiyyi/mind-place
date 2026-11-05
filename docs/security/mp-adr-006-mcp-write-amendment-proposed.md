# MP-ADR-006 — MCP write-scope amendment proposal v1

**Status: PROPOSED / NOT ACCEPTED.** Date: 2026-11-06. Owner decision: PENDING.  
**Current MP-ADR-006 is still ACCEPTED as READ-ONLY.** This proposal is a separate candidate and is **not** itself an updated grant, a change to the accepted decision register, or authorization to publish writable MCP tools.

## Context and problem

The accepted [MP-ADR-006 decision](https://app.notion.com/p/40ac4a6aaf7381aa93cdc99957ae9fee) permits Windows per-user local/server MCP clients to read only individually approved collections while the vault is unlocked. The owner separately requested that Claude and ChatGPT, and optionally Gemini, be able to write authorized curated/derived information and imports. The existing [Phase 3B](../implementation-plan/phase-3b-client-authorization.md) separates read/write/admin bits, but it does not define fine-grained MCP write operation grants, a durable idempotency protocol or authorization to enable any write tool.

## Proposed decision text (ONLY effective after exact owner approval)

Keep all MP-ADR-006 Windows lifecycle/unlock behavior. Extend the per-client policy with **explicit opt-in per-vault, per-collection, per-object-kind, per-operation write grants**, never inherited from read grants or historical write bits. Supported individually authorized operations: (1) create_import of original-preserving external agent sessions/curated memories, (2) append immutable revision or derived staging batch, (3) superseding update of curated/derived records with revision preconditions, (4) logical collection move with both source and destination authority, (5) separately gated reversible tombstone for eligible curated/derived records. **Hard delete, archive byte rewrites, filesystem/root grants, SQL/shell/admin, key export and backups/restore remain unavailable over MCP.**

Every policy edit, new write grant or destructive grant requires explicit trusted interactive owner approval. Runtime validates client credential+audience, vault, current exact collection/object/operation, policy revision, source revision and live unlock epoch at admission, mutation/commit and all disclosure. Lock/revoke/permission change fences pending work. Durable bound idempotency receipts, append-only provenance/audit and crash-recoverable atomic staging are mandatory. Write never confers read. Future provider-specific implementation/real Windows acceptance is outside this decision.

Read the [normative proposed contract](mp-7mcp-01-write-authority-contract.md) for the exact operation matrix, threat model, receipts, recovery and negative-test plan; it is versioned together with this amendment.

## Considered alternatives

- **A — remain read-only:** minimum exposure; AI clients cannot save curated knowledge directly. Safe default until approval.
- **B — blanket write access:** fewer permission prompts but unacceptable cross-collection overwrite, replay and exfiltration risk; **REJECTED as proposal**.
- **C — separate operation-scoped grants (recommended):** more policy/UX/transaction work, best match for explicit owner authority and immutable archive. **PROPOSED** pending owner choice.
- **D — GUI-mediated manual export/import only:** avoids MCP mutations but creates friction for iterative agent curation; remains an available fallback.

## Decision and rollout gates

1. Independent reviewer validates exact operation matrix, destructive-policy treatment, source immutability, lease fences, principal binding and absence of read escalation.
2. Owner explicitly accepts or revises exact operations, object kinds, and collection approval UX, with an attributable dated approval. Only then record an ACCEPTED amendment in the Notion ADR register; preserve original 2026-10-31 decision history.
3. Implement authenticated authoritative policy/store and staged write engine in separately scoped source work; no raw keys or owner corpus.
4. Negative production transport/storage tests and exact-source CI, including lock/revoke/race/crash/replay, then real Claude/ChatGPT (optional Gemini) client and Windows/native qualification.
5. Keep all client MCP write catalogs **disabled** until matching provider gate passes. Do not silently enable for read-only users.

## Current outcome

No owner write-scope approval; no production write authorization; no production negative-test or native acceptance claim. Existing MP-ADR-006 read-only policy remains binding.

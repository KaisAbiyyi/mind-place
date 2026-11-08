# MP-ADR-006 — MCP write-scope amendment proposal v2

**Status: PROPOSED / NOT ACCEPTED.** Date: 2026-11-08 (v1: 2026-11-06). Owner decision: direction recorded, **final acceptance PENDING**.
**Current MP-ADR-006 is still ACCEPTED as READ-ONLY.** This proposal is a separate candidate and is **not** itself an updated grant, a change to the accepted decision register, or authorization to publish writable MCP tools. It is meant to be recorded as its own ADR entry, preserving the accepted 2026-10-31 decision history.

## Context and problem

The accepted [MP-ADR-006 decision](https://app.notion.com/p/40ac4a6aaf7381aa93cdc99957ae9fee) permits per-user local/server MCP clients to read only individually approved collections while the vault is unlocked. The owner wants cloud assistants (ChatGPT, Claude, Codex, Cowork and Claude Code running in the cloud) to read and write **curated** knowledge, and local agents with local access (Codex, Cowork, Claude Code CLI/desktop) to additionally read raw data, process curated data, build the graph and run user-scoped bulk curation. [Phase 3B](../implementation-plan/phase-3b-client-authorization.md) separates read/write/admin bits but defines no fine-grained write operations, idempotency protocol or tier model.

## Proposed decision text (ONLY effective after exact owner approval)

Keep all MP-ADR-006 Windows lifecycle and unlock behavior. Add:

1. **Principal tiers** decided by credential class and transport: *cloud* (any client reaching Mind Place through the hosted tunnel or registered as cloud) reads and writes the curated layer only, with per-request limits, and never reads or processes raw data or runs bulk; *local* (loopback-only credentials, never valid through the tunnel) may additionally read raw, process curated, build the graph and run bulk jobs.
2. **Per-client, per-vault, per-collection, per-object-kind, per-operation policy** with modes `allow` / `ask_first` / `deny` (default deny), never inherited from read grants or legacy capability bits. Operations: `create_import`, `append`, `update`, logical `move`, and tombstone-only `delete`. `ask_first` is a server-enforced owner approval in the trusted local UI.
3. **Raw evidence is immutable over MCP** (only `create_import`/`append` of new revisions); tombstoned objects are immutable over MCP; restore, rollback and physical purge are owner-only local actions. **Hard delete, archive byte rewrites, filesystem/root grants, SQL/shell/admin, key export and backup/restore remain unavailable over MCP.**
4. **Bulk** is a local-tier, user-scoped, out-of-band job: staged evidence-backed proposals invisible until a separate apply step (default `ask_first`, all-or-nothing), versioned, with owner-only logical rollback. Sub-agents and model choice carry no authority of their own.
5. Runtime validates client credential and audience, tier/transport, vault, current exact grant, server-derived policy revision, source revision and live unlock epoch at admission, mutation/commit and all disclosure; fails closed if the authority store is unreadable; writes content-free audit for every decision. Durable bound idempotency receipts and crash-recoverable atomic staging are mandatory. Write never confers read.

The [normative proposed contract (v2)](mp-7mcp-01-write-authority-contract.md) contains the exact matrix, limits, threat model and negative-test plan, and is versioned together with this amendment.

## Considered alternatives

- **A — remain read-only:** minimum exposure; assistants cannot save curated knowledge. Safe default until approval.
- **B — blanket write access:** unacceptable cross-collection overwrite, replay and exfiltration risk; **REJECTED**.
- **C — operation-scoped grants with tiers (recommended, matches the owner's direction):** more policy/UX/transaction work, best match for explicit owner authority and an immutable archive. **PROPOSED**.
- **D — GUI-mediated manual export/import only:** avoids MCP mutations but adds friction; remains a fallback.

## Decision and rollout gates

1. Independent reviewer validates the v2 matrix, tiering, tombstone/restore semantics, bulk model, source immutability, fences, principal binding and absence of read escalation.
2. Owner explicitly accepts or revises the final text with an attributable, dated approval; only then record an ACCEPTED amendment in the Notion ADR register. A GitHub APPROVE from the PR author's own account is not possible; record the owner's decision separately.
3. Implement the authenticated authoritative policy store, tier/transport binding and staged write engine in separately scoped source work; no raw keys or owner corpus.
4. Production negative transport/storage tests and exact-source CI, including lock/revoke/race/crash/replay, then real-client and Windows/native qualification per provider and per principal class.
5. Keep all MCP write catalogs **disabled** until the matching gate passes; advertise write tools only to credentials holding at least one write grant.

## Current outcome

No final owner approval; no production write authorization; no production negative-test or native acceptance claim. Existing MP-ADR-006 read-only policy remains binding.

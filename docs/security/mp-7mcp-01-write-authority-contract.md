# MP-7MCP-01 — Scoped MCP write-authority contract (v2)

**Status: PROPOSED; NOT APPROVED; production MCP stays READ-ONLY.**
Task: MP-7MCP-01 | Lock: mcp-write-authority | Contract v2, 2026-11-08 (v1: 2026-11-06, baseline main `24648c74f3f59e176974d3878049b882478db69b`).

v2 incorporates the owner's direction of 2026-11-08 (recorded on the PR and in the task record) and the independent-review findings F1–F8. The owner direction is **not** an ADR acceptance: the amendment is accepted only when the owner confirms this final text.

References: [canonical ADR register](https://app.notion.com/p/40ac4a6aaf7381aa93cdc99957ae9fee), [vault security](https://app.notion.com/p/40ac4a6aaf7381969fddc8d0284131d2), [Phase 3B](../implementation-plan/phase-3b-client-authorization.md), [ADR-006 write amendment proposal](mp-adr-006-mcp-write-amendment-proposed.md).

## 1. Existing accepted boundary

MP-ADR-006 (accepted 2026-10-31) authorizes **read-only** MCP for explicit per-client approved collections. This proposal amends only future MCP write scope and the read layer visible to cloud clients (section 2); it does not change manual unlock, per-user server, desktop-close, Win+L, logout/restart/crash behavior.

Production write tools, listener routes, credentials and capabilities MUST remain disabled until the owner approves this exact text, it is independently reviewed, implemented, regression tested and qualified on real clients. Neither this PR nor its merge is owner approval. Phase 3B's legacy `read`/`write`/`admin` capability bits (`Cap::Write`, `Cap::Admin`) confer **zero** MCP write authority, before or after this change; a regression test MUST prove that a credential with legacy bits and no grant is denied.

## 2. Principal tiers (owner decision)

The tier is a property of the **credential class and the transport**, never of the product name or a client claim.

| Tier | Typical clients | May | May not |
| --- | --- | --- | --- |
| **cloud** | ChatGPT, Claude (web), Codex cloud, Cowork cloud, Claude Code cloud, anything reaching Mind Place through the hosted tunnel | read curated; write curated within per-request limits | read raw; process raw; bulk/mass writes; graph build |
| **local** | Codex local, Cowork local, Claude Code CLI/desktop with local access | read raw; process curated; build graph; run bulk jobs (section 6) | act outside collections it was granted |

Rules: the server issues a credential with exactly one class; the class must equal the client's registered tier; a **local-class credential is valid only on the local transport (loopback / local pipe) and is rejected through the hosted tunnel**; anything arriving through the tunnel is cloud by construction. Sub-agents and model choice (for example a cheaper worker model) carry **no authority of their own**: they act under the credential of the principal that spawned them, and audit records name that principal only.

Consequence for reads: cloud clients no longer receive raw conversation hits (message-level archive evidence) from `memory_search` / `memory_fetch` / `context_build`; they see only the curated layer. This narrows MP-7MCP-02 and MP-7MCP-04 and is a change to the read behavior accepted in MP-ADR-006.

Every object kind belongs to exactly one layer (`raw`, `curated`, `derived`); every collection has one layer; unknown kinds or collections are denied.

## 3. Operations and per-operation policy

Operations, each granted **independently**: `create_import`, `append`, `update`, logical `move`, and `delete`, where **delete is tombstone only**.

| Layer | Allowed operations (static eligibility, enforced below grants) |
| --- | --- |
| raw | `create_import` (new raw-preserving revision/import) and `append` only. Never `update`, `move`, `delete` |
| curated, derived | all five |

Each (client, vault, collection, object kind, operation) has one policy mode: **`allow`**, **`ask_first`** or **`deny`**; absent or invalid means `deny`. `ask_first` is a **server-enforced owner approval** in the trusted local Mind Place UI (a provider-side prompt is client UX, never a security boundary); a pending approval that times out is a denial. Tombstone (`delete`) requires an owner approval regardless of mode unless the owner later decides otherwise in a separate decision.

Hard delete, physical purge, archive-byte rewrites, SQL, shell, filesystem or root grants, key export, backup/restore, grant or policy administration, unlock and restore of a tombstone are **never** available over MCP. Physical purge exists only as the owner's last-resort local destructive-forget flow (MP-9C), outside MCP.

Grants are issued, changed and revoked only in the trusted local UI. A grant record has: id, immutable client id, vault id, one canonical collection, one object kind, one operation, mode, enabled/revoked, per-client **write policy revision**, optional expiry, optional limits. A **grant validator** rejects at issuance: wildcard/prefix/empty/ill-formed values, unknown operation/kind/mode, an operation ineligible for the kind's layer, and for the cloud tier any bulk operation, raw or derived kind, a missing expiry, or an expiry beyond the cloud maximum.

## 4. Tombstones, approvals and restore

- A tombstoned object is **immutable over MCP**: `update`, `append`, `move` and a second `delete` are denied. Content and history are retained; tombstone never erases bytes.
- **Restore** (clearing a tombstone) is an owner-only local-UI action producing a new revision; it is not an MCP operation.
- An owner approval is bound to one client, vault, collection, operation, `operation_id`, server-computed body digest, policy revision and unlock epoch, has an expiry, and is **single-use** (consumed at commit). A second tombstone, even with an identical body, needs a fresh approval.

## 5. Enforcement, identity and trusted inputs

The principal is a validated server-side credential with issuer and audience binding. **The server derives** the clock, the unlock epoch/lease, the per-client policy revisions and the **canonical request digest**; request-supplied `now`, `epoch`, `policyRevision` and `bodyDigest` are ignored. The digest covers operation, vault, collection, kind, object id, destination, expected revision and payload.

The authorization predicate is the conjunction of: authenticated, enabled principal; credential class equal to tier and, for local, loopback transport; exact issuer/audience; unlocked vault, live epoch, external access not paused, **authority store readable (otherwise fail closed)**; layer and eligibility rules; exact grant (non-expired, current policy revision, mode not `deny`); limits; owner approval where required; object membership, not tombstoned, `expected_revision`. It is evaluated at admission, before every mutation, **inside the final serialized writer transaction**, and before any disclosure (receipts, metadata, errors, pagination). A move verifies source and destination grants and layers. Write never confers read: reads use independent read grants with their own **read policy revision**, so editing write grants never disturbs read-only providers.

`create_import` ids are **server-assigned**; a caller-supplied id is denied opaquely so that existence of other collections' objects cannot be probed. Denials never disclose object or collection existence, keys or native paths.

Limits (owner-configurable per grant; defaults): **cloud** — grant lifetime ≤ 30 days, ≤ 1 MiB and ≤ 20 objects per request, ≤ 60 writes per hour per grant. **local** single writes — as granted by the owner. Limit failures return a distinct `LIMIT` result.

Audit: every decision, allow and deny, writes an append-only, content-free record (time, principal, operation, collection, decision, code). Audit never stores payloads, paths, passwords, keys or tokens.

## 6. Local-tier bulk jobs ("dreaming")

Bulk mirrors an out-of-band curation pass: asynchronous, user-scoped, evidence-backed, versioned and reversible. It is **not** whole-vault processing; the owner scopes it in the prompt to a project/topic/collection set, and the job declares that scope up front (for example: "gather and build comprehensive curated data for project mind-place; use sub-agents as needed").

1. **Start** — local tier on loopback only; requires a `bulk_stage` grant on the target collection; the declared scope collections each need a read grant; target layer must be curated or derived. A **private** source may only feed a target whose readers it also covers; ordinary raw → curated promotion is the purpose of the job and is gated by tier and the apply step.
2. **Stage** — proposals are written to a staged generation, **invisible to every reader**. Each item carries operation (`create_import`, `append` or `update` only — never `move` or tombstone in bulk), a payload, source collections inside the scope, and non-empty **evidence pointers** into the scope. Item `update`/`append` require `expected_revision`. Defaults, owner-configurable: lease 15 min, ≤ 512 items per batch, ≤ 20,000 items per job (existing graph-maintenance constants).
3. **Apply** — a separate step requiring a `bulk_apply` grant. Default mode `ask_first`: one single-use owner approval bound to the batch digest. Apply is **all-or-nothing**: every item is re-authorized (epoch, grants, policy revision, tombstone, revision) before and again inside the commit; any failure applies nothing. Lock, revoke, epoch or policy change between stage and apply denies the batch.
4. **Rollback** — owner-only and logical: objects created by the batch are tombstoned; objects changed by the batch receive a **new revision restoring prior content**; history is retained; rollback refuses (conflict, nothing changed) if any affected object was edited afterwards.
5. Concurrency: expected-revision/hash compare-and-swap in the single writer; a stale proposal conflicts instead of overwriting.

## 7. Idempotency, recovery and ordering

Every mutation requires an unpredictable `operation_id`; the durable receipt identity is `(vault_id, client_id, operation_id)` with the server-computed digest, operation, scoped ids, resulting revision and commit marker. Same identity and digest returns the previous receipt **only after fresh authorization and a disclosure check**; same identity with a different digest/operation/scope is a conflict; another principal cannot borrow a receipt. A single vault writer stages immutable bytes in non-visible storage, verifies, flushes, then atomically publishes with the state check and audit record. Interrupted pre-commit work stays invisible; committed-but-unacknowledged work is reconciled by an authorized retry. Published revisions are never erased as "rollback".

## 8. Threat model and required negative tests

| Threat | Protection | Required proof |
| --- | --- | --- |
| Prompt injection / malicious model (cloud) | tier limits, per-operation grants, no raw access, no bulk | wrong operation, collection, kind, tier denied |
| Prompt injection through raw content (local) | staged, evidence-backed proposals; apply approval; batch rollback; sub-agents have no authority | staged invisible; apply needs approval; rollback restores |
| Stolen/replayed token | audience/issuer, expiring grants, revocation, bound operation_id, tier-transport binding | cross-client/vault replay, post-revoke and local-credential-over-tunnel denied |
| Confused deputy / move | server-resolved membership + dual-collection grant | wrong destination, stale membership denied |
| Lock/revoke/race | epoch and policy fence inside the writer and before disclosure | commit and disclosure fences |
| Tombstone abuse | tombstone immutable over MCP; owner-only restore; single-use approval | resurrection and approval reuse denied |
| Raw tampering | static layer×operation eligibility below grants | raw update/move/delete denied despite a grant |
| Existence probing | server-assigned ids, opaque denial | existing and absent ids indistinguishable |
| Policy/read coupling | separate read and write policy revisions | write edit leaves read grants valid |
| Client-supplied trust inputs | server clock, epoch, digest | forged `now`/digest ignored |
| Privilege escalation via legacy bits | closed operation set, grant required | legacy `read`/`write`/`admin` bits confer nothing |
| Resource exhaustion | per-grant limits, bulk caps, lease | LIMIT results |
| Crash/partial failure | staging, journal, atomic activation, all-or-nothing apply | fail at stage, commit, ack |
| Missing authority store | fail closed | unreadable store denies |

## 9. Verification stages (truthful boundaries)

**Stage A — this PR:** security contract + executable isolated reference model + negative tests. These tests verify the **reference model only**, not production MCP authorization.
**Stage B — after owner approval of this text:** persistent policy schema, server-side evaluation, tier/transport binding in the Rust transport, operation handlers, and true integration negative tests against state/storage/transport, crash and concurrency fixtures; exact-source full checks and hosted CI.
**Stage C — after independent review and Stage B:** provider write tasks (Claude MP-7MCP-03, ChatGPT MP-7MCP-05, optional Gemini MP-7MCP-07) and any principal classes not yet covered by a task (Codex, Cowork, Claude Code), each after its read gate, with real-client write proof and Windows/native Lock/revoke qualification. No provider write path may be advertised before these gates.

## 10. Owner decision record

| Item | State |
| --- | --- |
| Direction: operation-scoped grants, per-operation allow/ask_first/deny, default deny | owner direction recorded 2026-11-08 — **final acceptance PENDING** |
| Tiers cloud/local; cloud curated-only; local raw + bulk | owner direction recorded — **final acceptance PENDING** |
| Full operation set from the first release; tombstone only | owner direction recorded — **final acceptance PENDING** |
| Cloud limits (30 days, 1 MiB, 20 objects, 60 writes/hour); local limits owner-configurable | owner direction recorded — **final acceptance PENDING** |
| Bulk model (section 6), apply default `ask_first` | owner direction recorded — **final acceptance PENDING** |
| Independent review of this v2 text | NOT RUN |
| Real endpoint/native tests | NOT RUN |
| Current ADR-006 read-only | ACCEPTED and unchanged |

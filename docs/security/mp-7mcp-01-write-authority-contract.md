# MP-7MCP-01 — Scoped MCP write-authority contract (v3)

**Status: PROPOSED; NOT APPROVED; production MCP stays READ-ONLY.**
Task: MP-7MCP-01 | Lock: mcp-write-authority | Contract v3, 2026-11-08 (v2 earlier the same day; v1: 2026-11-06, baseline main `24648c74f3f59e176974d3878049b882478db69b`).

v2 incorporated the owner's direction of 2026-11-08 and review findings F1–F8; v3 resolves the independent review's C1 (owner-configurable hard caps), C2 (worker lease vs proposal lifetime) and C3 (review by exception). The owner direction is **not** an ADR acceptance: the amendment is accepted only when the owner confirms this final text.

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

### Limits: safe defaults, owner hard caps, engine ceilings

Every limit has three levels, and **the effective value is `min(override ?? default, owner cap, engine ceiling)`, recomputed from the current policy on every use**:

1. **Safe defaults** (smallest).
2. **Owner-configurable hard caps** (`ownerPolicy.caps`), settable only in the trusted local UI, never over MCP. A cap must be an integer ≥ the default and ≤ the ceiling; invalid, negative, zero, fractional, non-numeric, unknown-key or oversized values are rejected, and a multi-key change is atomic. Until the owner sets a cap, the factory cap applies. A grant or job may **override a limit downward or up to the current owner cap, never above it**: the issuance validator rejects an over-cap override, and enforcement independently clamps one, so a lowered cap takes effect immediately on existing grants and a record that bypassed the validator still cannot exceed it. A malformed override fails closed; a malformed stored cap falls back to the safe default.
3. **Engine safety ceilings**, compile-time constants that no policy, grant or request can exceed; they protect against resource exhaustion.

| Limit | Default | Factory owner cap | Engine ceiling |
| --- | --- | --- | --- |
| payload bytes per request (`maxBytes`) | 32 KiB | 48 KiB | 48 KiB |
| objects per request (`maxObjects`) | 20 | 50 | 100 |
| writes per hour per grant (`maxWritesPerHour`) | 60 | 600 | 3,600 |
| cloud grant lifetime (`maxGrantMs`) | 30 days | 60 days | 90 days |
| worker lease (`leaseMs`) | 15 min | 30 min | 60 min |
| items per staged batch (`maxBatch`) | 512 | 1,024 | 2,048 |
| items per job (`maxItems`) | 20,000 | 50,000 | 100,000 |
| concurrent dreaming workers (`maxConcurrentWorkers`) | **3** | **5** | 8 |
| staged-proposal lifetime (`proposalTtlMs`) | 7 days | 14 days | 30 days |
| owner approval lifetime (`approvalTtlMs`) | 10 min | 60 min | 24 h |

Worker concurrency is enforced per client (effective limit) **and** vault-wide (owner cap). A lowered cap blocks new workers at once and lets running ones finish. The numbers are Stage A proposals for the owner to set; only the relations (default ≤ cap ≤ ceiling) are normative, and Stage B fixes the ceilings from measured resource budgets.

**Correction to the earlier proposal.** The production MCP **request** envelope is 64 KiB (`BODY_LIMIT` / `MAX_REQUEST_BYTES`); the 1 MiB figure in the earlier text is the transport wire cap, not the request cap. This contract therefore does **not** promise 1 MiB requests: the payload ceiling is set under the current request envelope. Raising the envelope is a separate Stage B transport and security decision that the owner must take explicitly; no limit in this contract may silently enlarge it. Limit failures return a distinct `LIMIT` result.

Audit: every decision, allow and deny, writes an append-only, content-free record (time, principal, operation, collection, decision, code). Audit never stores payloads, paths, passwords, keys or tokens.

## 6. Local-tier bulk jobs ("dreaming")

Bulk mirrors an out-of-band curation pass: asynchronous, user-scoped, evidence-backed, versioned and reversible. It is **not** whole-vault processing; the owner scopes it in the prompt to a project/topic/collection set, and the job declares that scope up front. The AI side performs clustering, deduplication and consolidation; the **engine** performs deterministic validation and risk classification, so the owner reviews by exception rather than item by item.

### 6.1 Three independent lifetimes

| Lifetime | Meaning | Rules |
| --- | --- | --- |
| **Worker execution lease** | how long a worker may stage | renewable by the worker only while **unexpired** and still fully authorized (grant, scope read grants, private-audience rule); an expired lease cannot be revived (start a new job); counts toward worker concurrency while active; governs **staging only** |
| **Staged-proposal TTL** | how long a staged proposal may still be reviewed and applied | set at staging, owner-configurable within caps; **never extended by lease renewal**; tightening a TTL or cap reaches already staged proposals, loosening never resurrects one; after expiry a proposal is `expired`, can be neither approved nor applied, and stays expired |
| **Owner apply approval** | the authorization to apply one exact batch | short-lived (default 10 min), **single-use**, bound to the batch; expires independently of the proposal and of the worker |

Worker lease expiry **never destroys** a valid staged proposal, and apply does not require a live worker lease.

### 6.2 Pipeline

1. **Start** — local tier on loopback only; `bulk_stage` grant on the target collection; target layer curated or derived; each declared scope collection needs a current read grant; a **private** source may only feed a target whose readers it also covers; concurrency limits (section 5) apply.
2. **Stage** — items are written to a staged generation **invisible to every reader**. Each item has an operation (`create_import`, `append` or `update` only — never `move` or tombstone in bulk), a payload, source collections inside the scope, a cluster, an optional claim `subject`, and **evidence pointers** `{collection, ref, content digest}` into the scope. The engine rejects exact duplicate payloads within a batch and updates that change nothing, requires every evidence pointer to resolve to the **current** source digest and not be redacted, and keeps only deterministic fields: any model-supplied confidence, score or label is dropped and never enters a digest.
3. **Classify (deterministic, versioned)** — each item is `low` (new object, no flags), `medium` (append or overwrite of a non-owner object), or `high` (an **exception**) when any flag holds: `owner_claim` (changes an owner-authored object), `conflict` (contradicts another item of the batch on the same subject), `private_source`, `sensitive_target`. Model confidence is never an input. The rules carry a `riskPolicyVersion` bound into every digest and approval.
4. **Review surface** — see 6.3.
5. **Apply** — a separate step requiring a `bulk_apply` grant (default `ask_first`). It needs the owner approval unless the batch qualifies for auto-apply (6.4). **Strictest policy wins:** a single `ask_first` on `bulk_apply` or on any contained operation, a missing grant, any non-low item, a disabled policy or an unlisted client routes the batch to the owner. Apply is **all-or-nothing**.
6. **Rollback** — owner-only and logical: objects created by the batch are tombstoned; objects changed by it receive a **new revision restoring prior content**; history is retained; rollback refuses (nothing changed) if an affected object was edited afterwards.

### 6.3 Review by exception: the approval interface contract

The review surface is a pure function of the immutable batch and current state. It exposes:

- a concise **thematic summary** (clusters flagged `themeIsUntrustedText`, since themes are model-authored and never influence policy), total and per-operation counts, consolidated-item count, evidence count, scope and target;
- the **risk breakdown** (low / medium / high) and the **exception list**: only high-risk items, each with its flags, operation, object, cluster and evidence count; item content is not part of the summary;
- **expandable** per-item evidence and the **complete diff** (before and after) on demand, never required;
- `requiredDecisions` (indices of exceptions) and `autoApplyEligible`;
- a `summaryDigest` over the whole surface, including the batch digest and `riskPolicyVersion`.

**One approval for the whole eligible batch**, created only in the trusted local UI, requires the `summaryDigest` the owner is looking at **now** (a stale view is rejected) and an explicit **`confirm` or `withhold` decision for every exception** (a missing decision, an extra decision or an unknown value is invalid). Sensitive, contradictory, owner-claim and private-scope changes are therefore never approved implicitly: they are separately confirmed or withheld. A withheld item is never applied and does not block the rest. The approval is bound to the batch digest, summary digest, decisions digest, risk-policy version, client, collection, policy revision and unlock epoch, and is single-use.

**Immutability.** The staged batch must hash, at apply, to the digest recorded at staging, and the approval must match the digest and summary recomputed **at apply**. Any mutation of items, clusters or themes, any change in classification (for example a target becoming owner-authored), a different risk-policy version, a tampered decision, or an approval for another batch invalidates the approval and applies nothing, including a change made inside the commit. Raw archives remain immutable.

### 6.4 Auto-apply

Auto-apply is **off by default**. It exists only under an explicit owner policy (`autoApply`: enabled, listed local clients, `maxRisk` = `low`, item limit within the batch cap), written only in the trusted local UI, and only when deterministic checks pass: every item `low`, no exception flag, within the item limit, `bulk_apply` and every contained operation `allow`, proposal unexpired, and all revalidation below passing. Model confidence is insufficient and ignored. The policy is re-read inside the commit, so disabling it between staging and commit stops the apply. Auto-applied batches are audited and remain fully rollback-able.

### 6.5 Revalidation before and inside the final apply transaction

Epoch and principal; policy revisions; `bulk_apply` and per-operation grants; the declared scope (every scope collection exists, has a current read grant and satisfies the private-audience rule); **source evidence authority** — every evidence pointer of every item that will be applied must not be redacted or forgotten and must still resolve to the same content digest (`DENIED` for redaction, `STALE_EVIDENCE` for stale or removed); tombstone state and `expected_revision` of every target; proposal expiry; approval validity and route. Any failure applies nothing.

**MP-9C coordination (no change to its files).** Redaction and forget authority belongs to the MP-9C privacy contract. This contract only consumes it through a read-only interface, "is evidence pointer (collection, ref), or the whole collection, redacted?", evaluated inside the writer. It guarantees that a staged, unapplied proposal citing forgotten evidence is never applied. Propagating a forget to **already-applied** derived objects is MP-9C's concern and is not claimed here.

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
| Source revoked or narrowed after staging | scope (read grant, layer, private audience) re-validated inside the apply commit | revoked/narrowed/removed source blocks the staged batch |
| Bulk weakening per-operation policy | strictest mode wins; approval re-evaluated at commit | `bulk_apply=allow` with an `ask_first` operation still needs approval |
| Override above the owner cap / cap lowered later | issuance validator, enforcement clamp, fail-closed on malformed values; caps bounded by default and ceiling | over-cap, negative, oversized, tampered overrides and cap decreases |
| Worker or approval outliving its purpose | independent worker lease, proposal TTL and short single-use approval | apply after worker expiry works; expired proposals and expired approvals never apply |
| Forgotten or stale evidence behind a staged proposal | evidence authority and digest revalidated before and inside the commit | redaction, whole-collection forget, stale digest and removed source block apply |
| Owner rubber-stamping hundreds of items | review by exception with mandatory per-exception decisions | exceptions need confirm/withhold; surface carries no item content |
| Approval for something other than what was shown | approval bound to batch, summary and decisions digests and the risk-policy version | any mutation or stale view invalidates; a failed apply consumes nothing |
| Unauthorized auto-apply | off by default, owner policy, deterministic low-risk checks, strictest policy wins | model confidence ignored; disabled, unlisted, medium/high, ask_first and a late policy change all route to the owner |
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
| Cloud limits (30 days, 20 objects, 60 writes/hour) and owner-configurable local limits; the earlier '1 MiB per request' is **withdrawn** because the production request envelope is 64 KiB | owner direction recorded — **final acceptance PENDING**; the owner must separately decide whether a larger request envelope is wanted in Stage B |
| Bulk model (section 6), apply default `ask_first` | owner direction recorded — **final acceptance PENDING** |
| Hard caps, worker concurrency (default 3, owner cap 5), lease/proposal/approval lifetimes, review by exception, auto-apply policy | owner direction recorded — **final acceptance PENDING** |
| Independent review of this v3 text | NOT RUN |
| Real endpoint/native tests | NOT RUN |
| Current ADR-006 read-only | ACCEPTED and unchanged |

# Mind Place — main plan (owner-approved 2026-11-10)

**Authority.** On 2026-11-10 the owner approved this plan as the main plan and authorized
aligning the repository documents and the Notion execution database to it. For every task
listed in §4, this file is the canonical statement of scope, prerequisites, resource lock,
gate and target release. Where an older document disagrees with this file about *remaining*
work, this file wins.

**What this file is not.** It is not a status board. Live task status, claims and Focus Lane
live only in the Notion database *Mind Place · Execution Tasks*. It does not reopen accepted
work, and it grants no merge, release, repository-settings or data-deletion authority. All
gates, evidence rules and the owner-controlled merge policy in `AGENTS.md` still apply.

**History and rationale.** Completed phases: [BIG plan](BIG-IMPLEMENTATION-PLAN.md) and
[CURRENT-PHASE](implementation-plan/CURRENT-PHASE.md). Reasoning behind this plan (Indonesian):
[plan review](plan-review/2026-11-10-plan-review-and-differentiation.md),
[owner decisions and roadmap](plan-review/2026-11-10-owner-decisions-and-long-term-roadmap.md),
[UI/UX brainstorm](plan-review/2026-11-10-ui-ux-brainstorm.md).

## 1. Product direction (owner decisions, 2026-11-10)

1. **Mind Place is the brain and the data; work runs in agents.** The app does not run models,
   compare models or orchestrate agents. Claude Code, Codex and similar clients do the work
   and reach Mind Place through MCP. The app is where the owner *sees, approves and locks*.
2. **Topic first.** The primary organizing object is the **topic** (e.g. `sibambo`, `politik`,
   `film`). A project is a topic that also carries work state. Curation, retrieval scope,
   Resume Packs and navigation are addressed by topic. Topic membership is a derived assertion
   with provenance and may be many-to-many.
3. **Cloud tier sees the curated layer only.** Hosted clients never receive raw conversation
   hits. Curated data may be as detailed as raw; its fidelity is set by the curation request.
   When a decision changed over time (A → B), the curated layer carries B and links A as
   superseded.
4. **Tier is decided by transport and credential class, not by product name.** Local tier
   (Claude Code, Codex, Cowork local): read raw, write curated, bulk and curation jobs. Cloud
   tier (hosted clients through the remote gateway): read curated; write later.
5. **Curation is done by agent swarms outside the app.** An orchestrator model delegates to
   cheaper worker models. Mind Place is the deterministic referee: it shards the work, issues
   narrow credentials, and rejects any curated item that cannot cite its evidence.
6. **Strict unlock is the default.** The owner password must cryptographically wrap the vault
   key; a Windows-only (DPAPI) unwrap is not sufficient.
7. **One universal, zero-cost remote path.** A tunnel-agnostic remote gateway with Tailscale
   Funnel as the primary adapter; the OpenAI Secure MCP Tunnel stays as a second adapter. A
   public URL makes OAuth 2.1 mandatory.
8. **Persona continuity.** Switching provider should feel like talking to the same person with
   a different brain: style, rules and corrections travel with the owner.
9. **The Workspace is a locked mirror of GitHub.** GitHub remains the source of truth for
   tasks and documents. The Workspace never inherits GitHub visibility: it is always behind
   authentication, even for a public repository. A server-readable Hosted mode is allowed per
   topic under that rule; the chat archive and vault never leave the device in readable form.
10. **v1.0 is a selection of what is proper, not everything.** Mind Place is a long-term,
    continuously developed project; later work is scheduled on the release train in §3.
11. **Provider parity (2026-11-11).** Claude, ChatGPT and Gemini get the same capabilities:
    all can read and all can write, except that ChatGPT hosted write is deferred to a December 2026
    owner decision (the capability stays in the contract; no ChatGPT credential gets write
    until then). Parity is a property of Mind Place's contract (one tool
    catalog, one conformance suite, no engine branch on provider name), not a promise about
    each vendor's client. Authorization is always enforced server-side. See §6.

Rejected as app features: built-in multi-model chat, a model comparison lab, an agent
orchestration engine, knowledge packs imported by other people, social recovery, a usage
dashboard. Where useful, these are agent-side skills over existing MCP tools.

## 2. Process rules that change

- **Risk-tiered review.** Work under the locks `mcp-write-authority`, `privacy-deletion`,
  `crypto-migration`, `backup`, `folder-authority`, `remote-gateway`, `mcp-parity-contract`, or any change to
  `crates/security-core`, requires an independent read-only review by a different session or
  agent before it can be accepted. UI, documentation and non-trust-boundary CI may use
  implementor self-review.
- **Task pages show current state only** (about 30 lines: status, exact SHA, latest evidence,
  blockers, next action). Older checkpoints move to PR comments or the Notion archive page.
- **One status source.** Notion holds live status. This file holds plan and dependencies.
  `CURRENT-PHASE.md` and `PARALLEL-EXECUTION-BOARD.md` are pointers plus history.
- **Qualification is tied to a SHA, not to PR state.** `MP-CI-QUAL-01` provides on-demand
  exact-SHA qualification and a nightly full matrix on `main`. Performance gates use p95/p99.
- **Local qualification is batched.** One script, one runbook and one machine-readable receipt
  per candidate SHA cover every pending Local gate.

## 3. Release train

| Release | Theme | Exit condition |
| --- | --- | --- |
| **v0.9 Continuity Preview** | Safe for the owner's daily use | `main` protected; tunnel helper cannot outlive the app; logical redaction and backup merged; Claude local read qualified; UI native gate run |
| **v1.0 Trust & Continuity** | Encrypted, recoverable, curated with evidence, readable and writable from Claude, ChatGPT and Gemini with the same capabilities | Every v1.0 task in §4 accepted; Parity Matrix (§6) recorded for all three providers; MP-10E candidate; MP-10L1–3 and MP-10U1–2 passed |
| **v1.1 Hardening & Reach** | Stronger and easier to install | Parser jail and fuzzing; context without MCP; persona continuity; topic-first navigation |
| **v1.2 Memory UX** | Memory as a product surface | AI gateway; decision and time-travel UI; memory inbox |
| **v2.0 Workspace** | Locked GitHub mirror, local | WS-00..03 |
| **v2.x Cloud** | Sync, web, sharing | WS-04..06; other platforms |
| **v3** | Reach | New modalities; team vaults; adapter SDK |

## 4. Task registry (remaining and new work)

Gate codes: `C` cloud, `L` local Windows, `O` owner. `IR` = independent review required (§2).
Existing IDs keep their Notion pages and history; changed scope is marked **(re-scoped)**.

### 4.1 v0.9 Continuity Preview

| ID | Deliverable | Prerequisites | Lock | Gate |
| --- | --- | --- | --- | --- |
| `MP-GOV-04` | Minimal `main` ruleset: PR required, force-push and deletion blocked, required CI check, empty bypass list, Auto-merge off; exact readback | MP-GOV-01, MP-GOV-02 | `owner/repository-settings/main-protection` | O |
| `MP-GOV-03` | Read-only AI review and prompt-injection boundary (PR #140) | — | `ai-review/read-only-capability` | C |
| `MP-SEC-10` | Hosted helper lifetime containment (issue #110): Windows Job Object with kill-on-close, parent heartbeat, orphan sweep and credential revocation at startup | — | `hosted-helper-lifetime` | C + L |
| `MP-DOC-01` | Close superseded PRs and finished issues with pointers; remove stale claims; keep pointer documents in sync with this file | — | `docs/board-hygiene` | C |
| `MP-9C` **(re-scoped)** | Slice 1 only: private scopes, logical redaction/tombstone enforced on every read path (archive, index and rebuild, MCP, export), owner preview and one-time confirmation (PR #151) | MP-9B, MP-5C | `privacy-deletion` | C, IR |
| `MP-9E` | Atomic backup, restore, corruption/integrity (PR #152) | MP-9B | `backup` | C, IR |
| `MP-7MCP-02` | Claude local-tier read (Claude Code / Claude Desktop) with a real client handshake (PR #155) | as recorded | `claude-mcp-client` | C + L |
| `MP-UI-L1` | Native Windows WebView2, DPI and large-corpus qualification | MP-UI-09 | `windows-ui-rollout-qualification` | L |
| `MP-UI-U1` | Owner acceptance of the UI rollout | MP-UI-L1 | `owner-ui-rollout-acceptance` | O |

### 4.2 v1.0 Trust & Continuity

| ID | Deliverable | Prerequisites | Lock | Gate |
| --- | --- | --- | --- | --- |
| `MP-9C2` | Physical erasure by crypto-erase: per-object keys wrapped by the vault key, destroy key + ciphertext, crash journal, shared-object reference custody, erasure receipt | MP-9C | `privacy-deletion` | C + L, IR |
| `MP-9C3` | Restore ↔ tombstone contract: append-only hash-chained forget ledger kept outside rollback, copied into every newer backup and the recovery kit; restore re-applies the ledger before activation | MP-9C, MP-9E | `backup` | C, IR |
| `MP-9F` **(re-scoped prerequisites)** | Portable export/import, schema migration and rebuild | MP-9C, MP-9E | `portable-migration` | C |
| `MP-10D` | Fault injection, crash, restore and migration proof | MP-9F | `release-recovery` | C |
| `MP-SEC-11` | Retrieval-output injection boundary and anti-poisoning: MCP output marks content as untrusted data with provenance; model-authored writes are always `asserted_by=model` and never auto-promoted; cloud writes go to quarantine collections; red-team fixtures join the MP-10A gold set | MP-7MCP-01 | `mcp-output-boundary` | C, IR |
| `MP-SEC-12` | Supply-chain gate: `cargo-deny`, `npm audit` threshold, Dependabot, pinned Rust toolchain, CycloneDX SBOM per release, CodeQL, OSSF Scorecard | — | `ci-supply-chain` | C |
| `MP-SEC-13` | Password-bound vault key (strict unlock, default): Argon2id-derived key wraps the vault key, layered with DPAPI; re-wrap migration without re-encrypting data; auto-lock on idle, sleep and screen lock | MP-9B | `crypto-migration` | C + L, IR |
| `MP-SEC-15` | One consolidated threat model: assets, adversaries, claims, non-claims, mapped to tests | — | `docs/threat-model` | C |
| `MP-REL-01` | Code-signing certificate procurement (long lead); releases publish SHA-256, build attestation and SBOM | — | `release-signing` | C + O |
| `MP-CI-QUAL-01` | On-demand exact-SHA qualification independent of PR state; nightly full matrix on `main`; p95/p99 performance gates | — | `ci-document-scale-workflow` | C |
| `MP-JOB-01` | Durable background job kernel: job table and state machine, checkpoints, idempotence, resume after restart, low-priority workers that yield to interactive queries, lock-aware pause, atomic publication per batch | MP-9B | `job-kernel` | C |
| `MP-TOP-01` | Topic as the primary scope: verify and extend the existing topic/project model (hierarchy, many-to-many membership with provenance, project = topic with work state); topic as curation scope and MCP filter | — | `topic-model` | C |
| `MP-11D` | Bulk history onboarding as a background job: official provider exports and agent session folders; each committed batch is searchable immediately | MP-JOB-01 | `capture-bulk-import` | C + L |
| `MP-CUR-01` | Curation job contract: recipe (topic scope, precision profile `exact`/`detailed`/`brief`, supersession policy), deterministic sharding, staged generations, delegated per-shard worker credentials | MP-7MCP-01, MP-JOB-01, MP-TOP-01 | `curation-jobs` | C, IR |
| `MP-CUR-02` | Deterministic faithfulness gate: evidence spans required and hash-checked; in `exact`, verbatim quotes matched against raw spans; uncited items rejected at staging; A → B requires an explicit supersession link | MP-CUR-01 | `curation-jobs` | C |
| `MP-CUR-03` | Coverage and staleness map per topic; coverage signal in cloud-tier MCP responses; incremental curation that skips unchanged shards | MP-CUR-01 | `curation-coverage` | C |
| `MP-CUR-04` | Agent kit: orchestrator skill and cheap-model worker definitions for Claude Code and Codex; sampled audit and per-generation quality score | MP-CUR-02, MP-7MCP-03 | `curation-agent-kit` | C + L |
| `MP-7MCP-01` | Scoped MCP write-authority contract (PR #153), plus shared JSON test vectors that both the JS reference model and the Rust engine must pass | as recorded | `mcp-write-authority` | C, IR |
| `MP-7MCP-03` **(re-scoped)** | Local-tier write for all local clients (Claude Code, Codex, Gemini CLI, Cowork local): Rust write engine, bulk and curation operations, per-client conformance checklist | MP-7MCP-01, MP-7MCP-02, MP-SEC-11, MP-PAR-01 | `mcp-write-local` | C + L, IR |
| `MP-7MCP-04` | ChatGPT read qualification under the cloud-tier (curated-only) policy | as recorded | `chatgpt-mcp-client` | C |
| `MP-PAR-01` | Provider-parity contract: one tool catalog with read-only/destructive annotations, one error model and limit set; Streamable HTTP that works without SSE; tool-schema subset rules; Parity Matrix schema; shared executable conformance suite extending the MP-7MCP-01 test vectors, run against the Rust engine and the JS reference model | MP-7MCP-01 | `mcp-parity-contract` | C, IR |
| `MP-7MCP-05` **(re-scoped, moved to v1.0)** | Cloud-tier write engine for hosted clients, provider-neutral (v1.0 qualification covers Claude, and Gemini through MP-7MCP-07; ChatGPT write is deferred to a December 2026 owner decision): staged proposals into per-client quarantine collections through the remote gateway, same operation set as the local tier | MP-7MCP-01, MP-PAR-01, MP-SEC-11, MP-NET-01 | `mcp-write-cloud` | C, IR |
| `MP-7MCP-06` **(promoted from optional)** | Gemini read adaptation: Gemini CLI against the local-tier engine (stdio or local HTTP, per-server `trust` off); setup guides for Gemini CLI, the Gemini app and Gemini Business; tool-schema compatibility findings feed MP-PAR-01 | MP-7E, MP-7IX, MP-7L2, MP-7U2 (accepted) | `gemini-mcp-client` | C + L |
| `MP-7MCP-07` **(promoted from optional)** | Gemini write adaptation: write tools through Gemini CLI (local tier) and through the Gemini app or Gemini Business (cloud tier) where the owner's account is eligible | MP-7MCP-03, MP-7MCP-05, MP-7MCP-06 | `gemini-mcp-write` | C + L |
| `MP-PAR-02` | Parity qualification: real Claude, ChatGPT and Gemini clients × local and hosted surface × read and write, run with the shared suite on one exact SHA; records the Parity Matrix (§6). A client's hosted write is enabled only after its hosted read cell is PASS. The ChatGPT hosted-write cell is `DEFERRED` until the owner's December 2026 decision | MP-7MCP-02, MP-7MCP-04, MP-7MCP-06, MP-7MCP-03, MP-7MCP-05, MP-7MCP-07, MP-NET-03 | `parity-qualification` | L + O |
| `MP-NET-01` | Remote gateway ADR and implementation: separate listener serving the cloud tier only, OAuth 2.1 with PKCE (S256) and discovery metadata (RFC 8414, RFC 9728), clients pre-registered by the owner by default and Dynamic Client Registration only behind owner approval, owner approval of each client in the local UI, rate limits and per-client egress circuit breaker; active only while unlocked with explicit consent | MP-7MCP-01, MP-SEC-10 | `remote-gateway` | C, IR |
| `MP-NET-02` | Tunnel adapters: Tailscale Funnel (installed CLI, not bundled), OpenAI Secure MCP Tunnel (existing), none | MP-NET-01 | `remote-gateway` | C + L |
| `MP-NET-03` | Real qualification: Claude custom connector and ChatGPT through the universal URL; revoke and lock tests | MP-NET-02, MP-CUR-03 | `hosted-account` | L + O |
| `MP-11A` | Resume Pack: evidence-backed, topic-scoped handoff (goals, active decisions, open questions, last state, artifacts) sized to the client budget; MCP resource and copy action | MP-7MCP-02, MP-TOP-01 | `resume-pack` | C |
| `MP-11B1` | Verifiable memory levels 1–2: hash-chained append-only log (captures, curation generations, disclosures, writes, forgets) storing salted commitments only; Merkle checkpoints signed by a device key; `memory_verify` | MP-9E | `verifiable-log` | C, IR |
| `MP-11C` | Secret Shield v1: local detection (provider key patterns, entropy near keywords, optional PII classes, owner list), keyed fingerprints, masking with stable tokens before any snippet is emitted; cloud tier always masked | MP-9C, MP-JOB-01 | `disclosure-filter` | C, IR |
| `MP-UX-01` | Plain-language mode as default; engineering terms only in technical mode | MP-UI-L1 | `ui-shell` | C |
| `MP-UX-02` | Exposure bar: who can read what right now; one accent colour reserved for "data is leaving this machine" | MP-UI-L1 | `ui-shell` | C |
| `MP-UX-03` | Home as *Continue* (recent topics with Resume Pack) and *Needs attention* (one inbox for every owner decision) | MP-UI-L1, MP-TOP-01 | `ui-overview` | C |
| `MP-UX-04` | Activity centre for all background jobs: progress, pause/resume, never a blocking modal | MP-JOB-01 | `ui-health` | C |
| `MP-UX-05` | Recovery-code ceremony with verification; passphrase strength for strict unlock; plain-language locked screen | MP-SEC-13 | `ui-security` | C + O |
| `MP-UX-06` | Curation approval by exception (thematic summary, rejected items, risky exceptions, random sample) and per-topic coverage bar | MP-CUR-02, MP-CUR-03 | `ui-memory` | C |
| `MP-UX-07` | Evidence split view, verification badges, decision thread (B above struck-through A), disclosure timeline per AI | MP-11B1 | `ui-memory` | C |
| `MP-10E` **(re-scoped prerequisites)** | Signed immutable candidate, qualified integration and release | MP-8F, MP-9F, MP-9C2, MP-9C3, MP-10B, MP-10C, MP-10D, MP-UI-U1, MP-SEC-11, MP-SEC-12, MP-SEC-13, MP-REL-01, MP-NET-03, MP-CUR-04, MP-11A, MP-11B1, MP-11C, MP-11D, MP-PAR-02 | `release` | C |
| `MP-9L1`, `MP-9L2`, `MP-9U1`, `MP-9U2` | Unchanged; MP-9L1 also qualifies MP-SEC-13, MP-9L2 also qualifies MP-9C2 | MP-9F (+ MP-SEC-13 / MP-9C2) | as recorded | L / O |
| `MP-10L1`–`MP-10L3`, `MP-10U1`, `MP-10U2` | Unchanged | MP-10E | as recorded | L / O |

### 4.3 v1.1 Hardening & Reach

| ID | Deliverable | Prerequisites | Gate |
| --- | --- | --- | --- |
| `MP-SEC-14` | Parser jail and coverage-guided fuzzing; measure OS subprocess jail against a WASM sandbox with fuel and memory limits | MP-9D | C + L |
| `MP-SEC-16` | Tripwire: decoy memories; a request for or write of a decoy pauses the client and notifies the owner | MP-NET-01, MP-11C | C |
| `MP-10S1` **(re-scoped)** | Measure lexical misses with a bilingual and paraphrase gold set; integrate an offline hybrid layer only if the measurement justifies it; zero-embedding mode stays fully functional | MP-10B | C |
| `MP-11B2` | Verifiable memory level 3: external anchoring of checkpoints (hash only leaves the device); restore compares the forget-ledger head with the last anchored checkpoint | MP-11B1 | C |
| `MP-11F` | Local agent transport: stdio shim and named pipe with peer check; `mp` CLI | MP-7MCP-03 | C + L |
| `MP-11L` | Auto-continuity hooks for agent sessions: inject Resume Pack at start, archive transcript before compaction and at end | MP-11A, MP-11F | C + L |
| `MP-11M` | One-step install packages for Claude Code, Claude Desktop and Codex | MP-11F | C |
| `MP-11G` | Written p95 performance budgets enforced in CI; reproducible public continuity benchmark | MP-10B, MP-CI-QUAL-01 | C |
| `MP-11H` | Open archive format specification and standalone verifier | MP-9F | C |
| `MP-11O` | Cache-stable context packs (byte-stable serialization) | MP-11A | C |
| `MP-11P` | Personal memory regression: gold questions derived from curated decisions, run after each reindex or curation generation | MP-CUR-03 | C |
| `MP-CTX-01` | Browser extension side panel: relevant memory while typing on any AI site, exact preview before insertion, never automatic, every insertion logged; extension permissions stay minimal | MP-11A, MP-11C | C + L |
| `MP-CTX-02` | System-wide hotkey overlay: search, copy or paste a context pack into any app | MP-11A, MP-11C | C + L |
| `MP-PER-01` | Persona continuity: versioned persona profile, per-topic rules, corrections ledger, per-tool rendering and sync status, consistency probes run from an agent | MP-7MCP-03 | C + L |
| `MP-UX-10` | Topic-first navigation (changes the MP-UI-01 route registry) | MP-TOP-01, MP-UI-U1 | C + L + O |
| `MP-UX-11` | Indonesian/English UI | MP-UX-01 | C |
| `MP-PAR-03` | Capture parity: evaluate automatic saving and manual history for Claude and Gemini. Today only ChatGPT has all three capture modes (extension capability matrix) | ADR amending the ChatGPT-only automatic-saving rule and optional host permissions; MP-PAR-02 | C + L |

### 4.4 v1.2 and later

| ID | Deliverable | Prerequisites | Release |
| --- | --- | --- | --- |
| `MP-GW-01` | Local AI gateway: passive API-compatible proxy capturing requests and responses, outbound Secret Shield, encrypted API-key store; **must retain usage metadata** (model, tokens, time); separate process from the vault core; requires an ADR | MP-11C | v1.2 |
| `MP-11B3` | Verifiable memory level 4: client-signed writes and exportable proof bundles | MP-11B1, MP-7MCP-03 | v1.2 |
| `MP-11I` | Time-travel (`as_of`) and decision ledger UI | MP-UX-07 | v1.2 |
| `MP-11Q` | Memory inbox: conflicts, open loops, topic digest | MP-CUR-03 | v1.2 |
| `MP-11N` | Git linkage: conversation ↔ commit, PR, issue | MP-11D | v1.2 |
| `MP-CTX-03` | Editor extension: discussions behind a symbol or file | MP-11N | v1.2 |
| `MP-WS-00` | Workspace ADR: locked GitHub mirror, invariant amendments (§5), where non-GitHub metadata lives | — | v2.0 |
| `MP-WS-01` | Read mirror of GitHub issues, PRs and repository documents; *Last Done / Now / Up Next* computed automatically from the DAG | MP-WS-00, MP-TOP-01 | v2.0 |
| `MP-WS-02` | Atomic claims with lease and fencing token; engine-enforced resource locks | MP-WS-01 | v2.0 |
| `MP-WS-03` | Status-dependent action that hands a task to an agent (generated command plus brief); allowlisted runner templates; task text passed as data, never as shell arguments | MP-WS-02, MP-11A | v2.0 |
| `MP-EVT-01` | Event bus and automations: local rules, webhooks, scheduled agent jobs | MP-JOB-01 | v2.0 |
| `MP-WS-04` | End-to-end encrypted sync relay and web client | MP-WS-01 | v2.x |
| `MP-WS-05` | Explicit, scoped, expiring, revocable share links; per-topic Hosted mode; no anonymous access | MP-WS-04 | v2.x |
| `MP-WS-06` | Hand-off to cloud agents | MP-WS-03, MP-WS-05 | v2.x |
| `MP-11J` | macOS/Linux: key-store abstraction replacing direct DPAPI dependence | MP-SEC-13 | v2.x |
| `MP-11K` | Rebuild claims and graph with a newer model while keeping old generations for audit | MP-CUR-04 | v2.x |
| `MP-MOD-01` | New modalities: local transcription, screenshot OCR (low priority) | MP-JOB-01 | v3 |

`MP-10S2` remains optional and unscheduled. `MP-7MCP-06` and `MP-7MCP-07` (Gemini) are scheduled in §4.2 since 2026-11-11.

## 5. Invariants that need an explicit ADR before the work starts

| Current rule | Amendment | Owning task |
| --- | --- | --- |
| No public endpoint and no OAuth (Phase 3D) | OAuth-protected remote gateway, cloud tier only, only while unlocked with consent | MP-NET-01 |
| The app has no outbound HTTP client | The gateway makes provider calls from a separate process | MP-GW-01 |
| Avoid a distributed service stack for a single-user local workload | Optional sync relay; the vault stays local | MP-WS-00 |
| Mutable data lives only in `state.sqlite3` | Workspace mirror objects as an operation log | MP-WS-00 |
| No external application source trees in this repository | Location of web client and relay | MP-WS-00 |
| Eight primary routes (MP-UI-01) | Topic-first navigation | MP-UX-10 |
| Automatic saving inspects only the loaded ChatGPT DOM (`AGENTS.md`) | Opt-in automatic saving and manual history for Claude and Gemini behind optional host permissions | MP-PAR-03 |

Until its ADR is accepted, the current rule stays in force.

## 6. Provider parity (owner decision 2026-11-11)

Owner direction: Claude, ChatGPT and Gemini have the same capabilities; all can read and all can write.
Revised the same day: ChatGPT hosted write is deferred. The owner decides in December 2026 whether
it is needed (the extension already captures conversations and local agents do the curation). The
capability stays in the contract, but no ChatGPT credential receives write grants until then.

**What parity means.**

- One tool catalog, one set of read-only/destructive annotations, one error model, one set of
  limits and one conformance suite. Nothing in the engine branches on provider name; the tier
  comes from transport and credential class (§1, item 4).
- Two surfaces per provider, identical across providers. *Local*: an agent with local access
  (Claude Code, Codex, Gemini CLI, Cowork local) uses the local tier. *Hosted*: a web or app
  client through the remote gateway uses the cloud tier (curated read, staged write).
- Identical defaults. The owner still grants or denies per credential and per operation
  (allow / ask first / deny); the defaults are the same for all three providers.
- Authorization is enforced by Mind Place, never by the vendor client. A vendor's own
  confirmation behaviour (ChatGPT prompts, the Gemini CLI per-server `trust` flag that skips
  per-tool approval) is not a control. Writes are staged proposals under the owner's policy.

**What parity does not mean.** Whether a vendor client can connect, or call write tools, depends
on that vendor's plan, region and settings. The Parity Matrix (qualified by MP-PAR-02) records
each cell as `PASS` (real client, exact SHA), `PROVIDER_BLOCKED` (evidence of the vendor
restriction, plus the fallback surface), `DEFERRED` (the owner chose not to enable it yet, with the
review date) or `NOT_RUN`. A blocked cell is a finding to report, not
a failure to hide.

**What the research found (web search 2026-11-11; mostly third-party, verify against the vendors' own documentation).**

| Provider | Local surface | Hosted surface | Constraints to check on the owner's account |
| --- | --- | --- | --- |
| Claude | Claude Code, Claude Desktop (MP-7MCP-02) | Custom connector; the connection is made from Anthropic's cloud, so the URL must be public HTTPS; Streamable HTTP; OAuth optional | Free plan limited to one custom connector |
| ChatGPT | Codex | Custom connector through developer mode, or the existing OpenAI tunnel (the path Chat On Steroids Core also uses) | OpenAI's help page describes full MCP including write as beta for Business/Enterprise/Edu; the Chat On Steroids README says Pro is read/fetch only. The plan on the owner's account is unknown, so it is tested (see below). ChatGPT may ask for confirmation or block risky writes |
| Gemini | Gemini CLI: stdio and remote HTTP, per-server `trust` | Gemini app custom apps reported since 2026-06-29; Gemini Business with admin-configured servers; Gemini API remote MCP for managed agents (July 2026) | The owner's Gemini app already shows **Connect to an MCP server** (HTTPS URL plus optional OAuth client ID and secret; owner screenshot 2026-11-11), so this cell is expected to be reachable. Still unknown: whether the OAuth flow, tool calls and write confirmation complete on the owner's account. Third-party sources list account, age, language and Keep Activity conditions that Google's developer pages do not confirm. Gemini Business does not allow SSE |

**Gemini app: what Google's developer documentation says (fetched 2026-11-11 from `developers.google.com/connected-apps/mcp`; it describes the Connected Apps platform and Developer Mode, so confirm each item in Developer Mode on the owner's account).**

- Endpoint: a public HTTPS MCP URL. Streamable HTTP only; legacy HTTP+SSE is not supported. Replies may be `application/json` or a stream inside the POST response, and the optional GET stream may return `405`. Protocol versions up to 2025-11-25.
- OAuth: authorization code grant with PKCE (S256). Discovery tries `/.well-known/oauth-authorization-server`, then `/.well-known/openid-configuration`, at the domain root, and uses the first URL that answers at all, including with an error, so the preferred one must return `200`. Required fields: `authorization_endpoint`, `token_endpoint`, `response_types_supported` containing `code`.
- Registration: Dynamic Client Registration (RFC 7591) is the default; a server without it can be connected by pasting a pre-registered client ID and secret, which is the dialog the owner sees.
- Redirect URIs to allow: `https://oauth-redirect.googleusercontent.com/r/*` and the `-sandbox` and `-test` hosts, each with `/r/*` and `/a/*` (six in total).
- Refresh: optional but recommended; Gemini expects the same refresh token to be returned and its expiry extended, not rotated.
- Annotations: a tool without `readOnlyHint` is treated as destructive. Google names `destructiveHint: true` and `openWorldHint: true` as the flags that trigger confirmation prompts.
- Shape: servers with dozens of tools are flagged in review; `verb_noun` snake_case names; string IDs; avoid `oneOf` and `anyOf`; return JSON as text and filter server-side instead of dumping rows. A schema change after registration may disable the app. No numeric limits are published for tool count, response size or timeout.
- Testing: Gemini Apps Developer Mode (`gemini.google.com/devmode`) shows the tool schemas, tool results and JSON-RPC errors for a custom MCP server. It is the test harness for MP-7MCP-06 and MP-PAR-02 on the hosted surface.

**ChatGPT and Chat On Steroids Core (observed, third-party).** Chat On Steroids is a community project (MIT, not affiliated with OpenAI): a desktop app that runs a loopback MCP server exposing `read`, `find`, `apply_patch`, `exec_command` and related tools, which ChatGPT reaches through developer-mode custom MCP apps and the OpenAI Secure MCP Tunnel, the same path Mind Place already uses (Phase 3D). It does not bypass OpenAI's plan rules: its README repeats that full MCP write support is a Business/Enterprise/Edu beta and that Pro is read/fetch only. Consequences for this plan:

- If Core's write tools work on the owner's ChatGPT, that account already allows custom-MCP writes, and Mind Place's write path needs no workaround. The cheapest test is to ask Core to write a harmless file and check that it appears. Use it in the December 2026 evaluation of ChatGPT hosted write.
- Mind Place does not build on browser-side injection into the ChatGPT page. It is brittle, the project's own README warns it may fall under OpenAI's terms on automated access, and it conflicts with the minimal-permission extension rule. If the plan blocks writes, the ChatGPT hosted-write cell is recorded `PROVIDER_BLOCKED`, with Codex as the fallback surface.
- Do not let a hosted model drive a local Mind Place command line to reach local-tier data. A tool such as Core's `exec_command` runs as the Windows user with no OS sandbox, so a hosted model would be using a local-tier credential and the tier boundary would be gone.
- Owner note: with Core installed, ChatGPT, and anything injected into its context, can run commands as the owner. Keep the Mind Place data directory out of any approved folder and keep the vault locked when not in use. This is the same-user threat that the password-bound vault key (MP-SEC-13) addresses.

**Design constraints that follow.**

1. The remote gateway speaks Streamable HTTP and must work without SSE.
2. Tool input schemas stay flat and simple (no `$ref`, `anyOf` or `oneOf`; bounded sizes), because
   Gemini function declarations accept only a subset of JSON Schema (general knowledge, verify).
3. Every tool carries explicit read-only/destructive annotations, identical across providers.
4. Cloud-tier writes are staged proposals into per-client quarantine collections, not direct
   mutations. This also keeps them low-risk for vendors that block high-risk writes.
5. Tool names and descriptions are short and provider-neutral; there are no provider-specific tools.
6. Setup guides for each provider are generated from one source.
7. Tool schemas are versioned and frozen once a client has registered; changes are additive only.
8. Annotations are truthful: `readOnlyHint` on read tools, `destructiveHint` on write tools. A write tool is never marked read-only to avoid a confirmation prompt.
9. The catalog stays small, with `verb_noun` snake_case names, string IDs, no `oneOf` or `anyOf`, and JSON text responses within the existing budgets.
10. The gateway publishes OAuth discovery at the domain root with `200` responses, accepts clients the owner pre-registers (client ID and secret shown once in the local UI, pasted into the vendor's dialog), enables Dynamic Client Registration only behind an owner toggle with each new client pending until approved, and allowlists redirect URIs per client class. Refresh tokens are revocable instantly and bound to the client; where a vendor expects non-rotating refresh tokens, access tokens stay short.

**Tasks.** `MP-PAR-01` contract and suite; `MP-7MCP-02/04/06` read adaptation per provider;
`MP-7MCP-03/05` write engines per tier; `MP-7MCP-07` Gemini write adaptation; `MP-PAR-02`
qualification and Parity Matrix; `MP-PAR-03` capture parity (v1.1, needs an ADR).

**Cost.** Moving cloud-tier write from v1.2 to v1.0 widens v1.0. It is sequenced last, after the
hosted read cell is qualified, and stays default-deny per client.

## 7. Still open

- Where Workspace metadata that GitHub lacks (prerequisites, locks, evidence) is stored.
  Recommendation: files in the repository, so GitHub stays the only source.
- Each new task receives its GitHub issue when it is claimed, not before.
- **ChatGPT hosted write (decide in December 2026).** Until then no ChatGPT credential receives write grants and the Parity Matrix cell is `DEFERRED`. Inputs for the decision: how the Claude hosted-write experience turns out, whether extension capture plus local-agent curation already covers the need (see `MP-PAR-03`, since only ChatGPT has automatic capture today), whether the owner's plan allows custom-MCP writes (the Chat On Steroids Core write test in §6), and the cost of qualifying one more client.
- **Gemini hosted write** stays scheduled with `MP-7MCP-07` unless the owner defers it the same way.

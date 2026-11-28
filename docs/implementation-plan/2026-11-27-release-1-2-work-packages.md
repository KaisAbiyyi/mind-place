# Release 1 and Release 2 — priority shift and work packages (owner decision 2026-11-27)

**Authority.** The owner moved the execution priority of Mind Place on 2026-11-27. This file sets
the order of work. For scope, gates and invariants of the tasks it absorbs, the main plan
(`docs/MAIN-PLAN.md`, owner-approved 2026-11-10, still on branch
`claude/project-plan-review-y4rrmf`) and `AGENTS.md` still apply. Live status stays in the Notion
database *Mind Place · Execution Tasks*. Nothing in this file accepts a task, passes a gate or
grants merge authority.

## 0. Naming used in this plan (owner wording)

- A **provider name alone** ("Claude", "Gemini") means the **remote** surface: the hosted app
  (claude.ai custom connector, Gemini app) reaching Mind Place through the remote gateway, cloud tier.
- A **local agent** is named by its client: **Claude Code**, **Antigravity**. These use the local tier
  (loopback/stdio MCP on the owner's machine).

## 1. The two targets

**Release 1 (R1) — "my archive becomes a curated brain, readable from anywhere"**

1. Import from ChatGPT, Gemini and Claude official exports.
2. Local read: Claude Code and Antigravity.
3. Remote read: Claude and Gemini (hosted apps through the remote gateway).
4. Curated data generation per topic by an agent swarm with local access: an orchestrator
   (Opus/Sonnet in Claude Code, Gemini Neon in Antigravity) starts cheap workers (Haiku) that search
   the archive for the requested topic and **write curated data over local MCP**. The curated data is
   the material for the Constellation (graph nodes), and every node can be updated. Format:
   [CURATED-NODE-FORMAT.md](../curation/CURATED-NODE-FORMAT.md).

**Release 2 (R2) — "write back from the chat, and run my projects here instead of Notion"**

1. Remote curated write from Claude and Gemini: one topic, or based on the active chat.
2. Task management better than Notion: GitHub-mirrored work items with automatic
   Last Done / Now / Up Next computed from the dependency graph, atomic claims, enforced locks and
   a one-click hand-off to an agent.

**Unchanged rule that shapes R1:** the cloud tier reads the **curated layer only** (MAIN-PLAN §1.3).
Remote Claude and Gemini therefore read curated nodes, topics and their evidence excerpts; raw
archive search stays on the local tier (Claude Code, Antigravity). Changing that needs a new owner
decision and an ADR.

## 2. What moves

- **Pulled forward into R1:** topic model (MP-TOP-01); curation contract and gate (MP-CUR-01..04);
  local curated write (curated slice of MP-7MCP-03); remote gateway (MP-NET-01/02, needs MP-SEC-10);
  remote curated read for Claude and Gemini (read part of MP-NET-03 and the hosted read cells of
  MP-PAR-02); Constellation (MP-UI2-07, MP-UI2-BG-01); Claude export import (new); local read for
  Antigravity (MP-7MCP-06).
- **Pulled forward into R2:** remote curated write (Claude + Gemini slice of MP-7MCP-05/07);
  Workspace (MP-WS-00..03, previously v2.0).
- **Owner decision recorded:** Gemini hosted curated write is enabled in R2 (replaces the December
  2026 decision point for Gemini in MP-7MCP-07). ChatGPT hosted write stays deferred.
- **Not in R1/R2 scope:** Gemini CLI and Claude Desktop qualification, ChatGPT MCP read
  (MP-7MCP-04). They stay eligible and come after.
- **Moved after R2, unchanged in scope:** MP-9C2, MP-9C3, MP-9F, MP-10D, MP-SEC-13, MP-SEC-15,
  MP-11B1, MP-11C, MP-REL-01, MP-CI-QUAL-01, MP-JOB-01, MP-11D, MP-7MCP-04 and the rest of the
  v1.0 list. They keep their Release label and are ordered after the work packages.
- **Not relaxed:** locked-by-default startup, approved-root filesystem boundary, immutable raw
  evidence, sanitized transcripts, curated-only cloud tier, OAuth on any public URL, independent
  review on the security-critical locks in MAIN-PLAN §2, owner-controlled merge.

## 3. Model roles (all projects, owner decision 2026-11-27)

| Role | Allowed models | Why |
| --- | --- | --- |
| **Planner / architect** (writes plans, work packages, ADRs, DAG changes) | Opus-class only: Claude Opus 5.5 ($4 input / $20 output per MTok) or a higher tier (Claude Fable); Gemini 4 Neon; ChatGPT Astra | Planning errors are the most expensive; only top-tier models plan |
| **Independent reviewer** | Sonnet-class: Claude Sonnet 5.5 at effort High ($2 / $10 per MTok), or another model of the same price and capability | Read-only review needs strong judgement, not the top tier |
| **Implementor** | Any capable model the owner assigns | Unchanged |
| **Curation worker** | Cheap models: Claude Haiku 5.5 ($0.10 / $0.50 per MTok), Gemini Flash-class | Bulk reading under a strict deterministic gate |

Prices are Anthropic first-party API rates as cached on 2026-11-03; they change, so the rule is
"Opus price tier" and "Sonnet price tier", not a fixed number. A plan written by a lower tier is a
draft until a planner-class model adopts it.

## 4. Work packages

Each package is sized for **one Claude Code or Antigravity implementation session** ("one go"):
one branch, one PR, tests included. Packages marked **IR** need an independent read-only review
(MAIN-PLAN §2) before acceptance. `C` cloud, `L` local Windows, `O` owner.

### Release 1

| ID | Package | Absorbs | Prerequisites | Lock | Gate |
| --- | --- | --- | --- | --- | --- |
| `MP-R1-WP01` | **Import trio.** Add the Claude.ai official export importer (`conversations.json`, projects, attachment metadata) beside the existing ChatGPT and Gemini Takeout importers, through the same raw-first publication (MP-IMPORT-RAW-01) and in-app flow (MP-IMPORT-APP-01); sanitized fixtures for all three; one local runbook for real exports on a disposable archive | new Claude importer, MP-IMPORT-L1 | — | `capture-import` | C + L |
| `MP-R1-WP02` | **Local read: Claude Code and Antigravity.** Merge MP-7MCP-06; one setup source that generates read-only configs for both clients (no wildcard, no auto-approve, no per-tool `eager`); run the local-read parity cells for both on one SHA with Antigravity auto-update paused or recorded | MP-7MCP-06 close-out, local read cells of MP-PAR-02 | MP-7MCP-06 merged | `mcp-parity-contract` (read) | C + L |
| `MP-R1-WP03` | **Topic model and curated node store.** Topic as primary scope (hierarchy, many-to-many membership with provenance); state tables for curated nodes, edges, revisions, evidence spans and staged generations per CNF v1; migration and rebuild tests; Markdown export view | MP-TOP-01, store part of MP-CUR-01 | — | `topic-model`, `memory-state-schema` | C, IR |
| `MP-R1-WP04` | **Curation plan and ingest gate.** Read-only MCP tools `curation_plan` (run id, deterministic shards, compact topic index) and `topic_index`; transport-independent ingest core: schema, evidence span/hash gate, verbatim match in `exact`, key and alias merge, `base_rev` conflicts, staged generation, owner publish; per-topic coverage count | MP-CUR-01 contract, MP-CUR-02, coverage part of MP-CUR-03 | MP-R1-WP03 | `curation-jobs` | C, IR |
| `MP-R1-WP05` | **Local curated write over MCP.** `curation_submit` (batch of CNF v1 records for one shard of a planned run) and `node_patch` tools on the local tier only, routed through the WP04 gate; per-run, per-shard write scope bound to the plan; quarantine and owner publish unchanged; per-client conformance for Claude Code and Antigravity | curated-write slice of MP-7MCP-03 | MP-R1-WP04 (MP-7MCP-01, MP-SEC-11 Done) | `mcp-write-local` | C + L, IR |
| `MP-R1-WP06` | **Curation agent kit.** Orchestrator skill and worker prompts for Claude Code (Opus/Sonnet → Haiku subagents) and Antigravity (Neon → Flash); the prompt "generate curated data untuk topik X" runs end to end over local MCP; node refresh flow; sampled audit and per-generation quality score; synthetic dry run in CI, real run on one owner topic | MP-CUR-04 | MP-R1-WP02, MP-R1-WP05 | `curation-agent-kit` | C + L |
| `MP-R1-WP07` | **Constellation.** Graph projection from accepted curated nodes (reuse MP-6 generations); bounded graph query (G1); Constellation page in the UI2 shell with node aside (evidence, revisions, superseded links) and "refresh this node"; review-by-exception screen for staged generations; EN/ID copy and design gap ledger | MP-UI2-07, MP-UI2-BG-01, review screen part of MP-UX-06 | MP-R1-WP03 | `ui-memory`, `graph-query` | C |
| `MP-R1-WP08` | **Remote gateway.** ADR amending the Phase 3D no-public-endpoint rule; separate listener for the cloud tier only; OAuth 2.1 + PKCE (S256), discovery at the domain root, owner-pre-registered clients, rate limits, active only while unlocked with consent; Tailscale Funnel adapter; Streamable HTTP without SSE | MP-NET-01, MP-NET-02 | MP-SEC-10 | `remote-gateway` | C + L, IR |
| `MP-R1-WP09` | **Remote read: Claude and Gemini.** Cloud-tier curated read tools (topics, nodes, edges, evidence excerpts with Secret-Shield-style masking of obvious secrets); setup guides for the claude.ai custom connector and the Gemini app; real qualification of read, revoke and lock on both; Parity Matrix hosted-read cells | read part of MP-NET-03, hosted read cells of MP-PAR-02, hosted Gemini part of MP-7MCP-06 | MP-R1-WP04, MP-R1-WP08 | `hosted-account` | C + L + O |
| `MP-R1-WP10` | **R1 candidate** (the single Ready PR of the WP04..WP09 stack and its one 100K run, see §7 DRAFT). Freeze one SHA, package, run one batched local runbook (import trio, Claude Code and Antigravity read and curated write, remote Claude and Gemini read, curation on one topic, Constellation on WebView2/DPI), owner acceptance | — | WP01–WP09 | `release` | C + L + O |

### Release 2

| ID | Package | Absorbs | Prerequisites | Lock | Gate |
| --- | --- | --- | --- | --- | --- |
| `MP-R2-WP01` | **Remote curated write: Claude and Gemini.** `curate_topic` (one topic) and `curate_active_chat` (the client sends the active-chat excerpt; Mind Place stores it as a new raw capture revision first, and curated records must cite it); same CNF v1 payload and WP04 gate; per-client quarantine; owner publish; ChatGPT grants stay off | Claude + Gemini slice of MP-7MCP-05 and MP-7MCP-07 | MP-R1-WP05, MP-R1-WP09 | `mcp-write-cloud` | C, IR |
| `MP-R2-WP02` | **Remote write qualification.** claude.ai connector and Gemini app: curate one topic, curate the active chat, revoke and lock tests; Parity Matrix hosted-write cells | write part of MP-NET-03, hosted write cells of MP-PAR-02 | MP-R2-WP01 | `hosted-account` | L + O |
| `MP-R2-WP03` | **Workspace board.** Workspace ADR; read mirror of GitHub issues, PRs and plan files; Last Done / Now / Up Next computed from the dependency graph (no manual Focus Lane); task ↔ topic ↔ curated node links; always behind authentication | MP-WS-00, MP-WS-01 | MP-R1-WP03 | `workspace` | C |
| `MP-R2-WP04` | **Claims, locks and hand-off.** Atomic claims with lease and fencing token; engine-enforced resource locks; status-dependent "hand to agent" action that generates a brief for Claude Code or Antigravity (task text passed as data, allowlisted runner templates) | MP-WS-02, MP-WS-03 | MP-R2-WP03 | `workspace` | C + L, IR |
| `MP-R2-WP05` | **R2 candidate.** Freeze, package, batched local runbook, owner acceptance | — | WP01–WP04 | `release` | C + L + O |

## 5. Prompt shape for one work package

```text
Ambil <WP-ID> di Mind Place.
Baca AGENTS.md, docs/implementation-plan/2026-11-27-release-1-2-work-packages.md (baris <WP-ID>),
dokumen yang diserap paket ini, dan docs/curation/CURATED-NODE-FORMAT.md bila relevan.
Claim di Notion (Status=Claimed, Claim Owner, lock), buat branch dan draft PR lebih dulu.
Kerjakan seluruh scope paket dalam satu PR, termasuk tes dan migrasi.
Jalankan baseline verifikasi AGENTS.md. Laporkan PASS/FAIL/NOT_RUN apa adanya.
Paket bertanda IR: minta review independen read-only (Sonnet-class) sebelum acceptance.
Jangan merge; owner yang memutuskan.
```

## 6. Board projection at adoption (2026-11-27)

- **Now:** MP-R1-WP01, MP-R1-WP03 (Ready); MP-SEC-10 (Ready, prerequisite of MP-R1-WP08);
  in-flight MP-7MCP-06 (#216) and MP-UI2-06 (#215); the other eligible tasks already in Now,
  ordered after the work packages.
- **Up Next:** MP-R1-WP02 (after MP-7MCP-06), MP-R1-WP04 and MP-R1-WP07 (after MP-R1-WP03),
  MP-R1-WP08 (after MP-SEC-10), MP-R2-WP03 (after MP-R1-WP03).
- Everything else waits on a predecessor outside Now and has no lane.

## 7. One 100K qualification for MP-R1-WP04 through the end (owner request 2026-11-28) — DRAFT

**Status: DRAFT.** Written by Claude Sonnet 5.5 at the owner's request. Under the 2026-11-27 model-role
rule it is not part of the plan until a planner-class (Opus-tier) session adopts it.

**Why.** The 100K document-scale job takes about 4.7 hours (historical 16 959 s). The effective-input
fingerprint (`scripts/mp10c_ci_gate.py`, `excluded()`) covers every tracked path except `docs/`,
`ci.yml`, other workflows and templates. So every code-changing PR that is Ready for Review starts its
own full 100K run, and a proof is reusable only inside the same PR. A Draft PR skips the 100K job
(`Draft development / qualification PENDING`) while still running the cheap checks.

**Rule (no CI change needed).** WP04..WP10 land on `main` through **one** PR that becomes Ready once.

1. After MP-R1-WP03 is merged, create one integration branch from `main`, for example
   `integration/r1-curation`. It is a task branch, not `main`.
2. Each package WP04..WP09 works on its own branch and opens a **Draft** PR whose base is the
   integration branch. The gate records the base ref, so a non-`main` base is accepted; drafts run
   fast feedback, Rust compile, SQLCipher, CodeQL and supply-chain checks, and skip the 100K.
3. A package is accepted at the cloud-gate level when its exact-head cheap checks are green and, for
   packages marked IR (WP04, WP05, WP08, WP09), the independent read-only review passes. It then enters
   the integration branch by a normal `git merge --no-ff` (the same integration method used for
   #129 into #127), only after the owner says so for that PR and SHA. Drafts are never marked Ready.
   Packages that share a lock or files are merged one at a time.
4. **MP-R1-WP10** is the only PR from the integration branch to `main`. It is marked Ready exactly
   once, after the whole stack is frozen, cheap-green and reviewed, and after the batched local
   runbook is prepared. That one transition runs native Windows Product and one 100K run on the final
   head. The owner reviews and merges once.
5. **Freeze before Ready.** After the PR is Ready, any change to a fingerprinted path (code, scripts,
   tests, lockfiles, the scale workflow) invalidates the proof and costs another run. Fixes found
   late go in before the transition, or the owner accepts the extra run.
6. Do not dispatch `manual` or `final` qualification before native CI has passed on the exact head
   (see `mp-ci-100k-01-qualification-gate.md`, operator sequence).

**Consequences.** Nothing from WP04..WP09 reaches `main` until WP10, so partial value is not usable
from `main` in between and a late defect holds the whole stack. WP01 and WP03 touch Rust production
code, so under the normal gate each triggers its own 100K when Ready; the owner may fold them into the
same integration branch if literally one run is wanted (decision open).

**Alternative, not the default.** Move the 100K from per-PR to nightly plus on-demand final
qualification. That is `MP-CI-QUAL-01` (Ready, currently after the work packages in order). It changes
required-check semantics, so it needs independent review and an owner decision.

**Prompt addition for WP04..WP09 agents.**

```text
Basis branch: integration/r1-curation (bukan main). Buka PR Draft dengan base itu dan biarkan Draft.
Jangan tandai Ready dan jangan dispatch qualification 100K. Merge ke integration branch hanya
setelah owner setuju untuk PR dan SHA tersebut.
```

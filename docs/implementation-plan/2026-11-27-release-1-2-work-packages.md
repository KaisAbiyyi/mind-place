# Release 1 and Release 2 — priority shift and work packages (owner decision 2026-11-27)

**Authority.** The owner moved the execution priority of Mind Place on 2026-11-27. This file sets
the order of work. For scope, gates and invariants of the tasks it absorbs, the main plan
(`docs/MAIN-PLAN.md`, owner-approved 2026-11-10, still on branch
`claude/project-plan-review-y4rrmf`) and `AGENTS.md` still apply. Live status stays in the Notion
database *Mind Place · Execution Tasks*. Nothing in this file accepts a task, passes a gate or
grants merge authority.

## 1. The two targets

**Release 1 (R1) — "my archive becomes a curated brain any local agent can read"**

1. Import from ChatGPT, Gemini and Claude official exports.
2. Read over MCP from Claude Code, Claude Desktop, Antigravity and Gemini CLI (local tier).
3. Generate curated data per topic with an agent swarm that has local file access: an
   orchestrator (Opus/Sonnet, or Gemini Neon) starts cheap workers (Haiku) that search the archive
   for the requested topic. The curated data is the material for the Constellation (graph nodes),
   and every node can be updated. Format: [CURATED-NODE-FORMAT.md](../curation/CURATED-NODE-FORMAT.md).

**Release 2 (R2) — "write back from the chat, and run my projects here instead of Notion"**

1. Claude and Gemini write curated data from the chat: one topic, or based on the active chat.
2. Task management better than Notion: GitHub-mirrored work items with automatic
   Last Done / Now / Up Next computed from the dependency graph, atomic claims, enforced locks and
   a one-click hand-off to an agent.

## 2. What moves

- **Pulled forward into R1:** topic model (MP-TOP-01), curation contract and gate (MP-CUR-01..04),
  Constellation (MP-UI2-07, MP-UI2-BG-01), Claude export import (new), local read on four clients
  (MP-7MCP-06 + local cells of MP-PAR-02).
- **Pulled forward into R2:** remote gateway (MP-NET-01/02, needs MP-SEC-10), hosted curated write
  (slice of MP-7MCP-05), local curated write over MCP (slice of MP-7MCP-03), Workspace
  (MP-WS-00..03, previously v2.0).
- **Owner decision recorded:** Gemini hosted curated write is enabled in R2 (this replaces the
  December 2026 decision point for Gemini in MP-7MCP-07). ChatGPT hosted write stays deferred.
- **Moved after R2, unchanged in scope:** MP-9C2, MP-9C3, MP-9F, MP-10D, MP-SEC-13, MP-SEC-15,
  MP-11B1, MP-11C, MP-REL-01, MP-CI-QUAL-01, MP-JOB-01, MP-11D, MP-7MCP-04 (ChatGPT MCP read), and
  the rest of the v1.0 list. They keep their Release label and stay eligible; they are ordered
  after the R1/R2 work packages.
- **Not relaxed:** locked-by-default startup, approved-root filesystem boundary, immutable raw
  evidence, sanitized transcripts, independent review on the security-critical locks listed in
  MAIN-PLAN §2, owner-controlled merge.

## 3. Model roles (all projects, owner decision 2026-11-27)

| Role | Allowed models | Why |
| --- | --- | --- |
| **Planner / architect** (writes plans, work packages, ADRs, DAG changes) | Opus-class only: Claude Opus 5.5 ($4 input / $20 output per MTok) or a higher tier (Claude Fable); Gemini 4 Neon; ChatGPT Astra | Planning errors are the most expensive; only top-tier models plan |
| **Independent reviewer** | Sonnet-class: Claude Sonnet 5.5 at effort High ($2 / $10 per MTok), or another model of the same price and capability | Read-only review needs strong judgement, not the top tier |
| **Implementor** | Any capable model the owner assigns (for example Opus or Sonnet in Claude Code, Antigravity) | Unchanged |
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
| `MP-R1-WP01` | **Import trio.** Add the Claude.ai official export importer (`conversations.json`, projects, attachments metadata) beside the existing ChatGPT and Gemini Takeout importers, through the same raw-first publication (MP-IMPORT-RAW-01) and in-app flow (MP-IMPORT-APP-01); sanitized fixtures for all three; one local runbook for real exports on a disposable archive | new Claude importer, MP-IMPORT-L1 | — | `capture-import` | C + L |
| `MP-R1-WP02` | **Local read on four clients.** Finish and merge MP-7MCP-06; one setup source that generates configs for Claude Code, Claude Desktop, Antigravity and Gemini CLI (read-only grants, no wildcard, no auto-approve); run the local-read cells of the parity suite on one SHA; Gemini CLI stays `PROVIDER_BLOCKED` with evidence if it still stalls | MP-7MCP-06 close-out, local read cells of MP-PAR-02 | MP-7MCP-06 merged | `mcp-parity-contract` (read) | C + L |
| `MP-R1-WP03` | **Topic model and curated node store.** Topic as primary scope (hierarchy, many-to-many membership with provenance); state tables for curated nodes, edges, revisions, evidence spans and staged generations per CNF v1; migration and rebuild tests; Markdown export view | MP-TOP-01, store part of MP-CUR-01 | — | `topic-model`, `memory-state-schema` | C, IR |
| `MP-R1-WP04` | **Curation run and ingest gate.** Read-only MCP tools `curation_plan` (deterministic shards + topic index) and `topic_index`; curation inbox as an approved root; deterministic ingest: schema, evidence/hash/verbatim gate, key and alias merge, `base_rev` conflicts, staged generation, owner publish; per-topic coverage count | MP-CUR-01, MP-CUR-02, coverage part of MP-CUR-03 | MP-R1-WP03 | `curation-jobs` | C, IR |
| `MP-R1-WP05` | **Agent kit.** Orchestrator skill and worker prompts for Claude Code (Opus/Sonnet → Haiku subagents), Antigravity (Neon → Flash), Gemini CLI and Codex; the prompt "generate curated data untuk topik X" runs end to end; node refresh flow; sampled audit and a per-generation quality score; synthetic dry run in CI, real run on one owner topic | MP-CUR-04 | MP-R1-WP02, MP-R1-WP04 | `curation-agent-kit` | C + L |
| `MP-R1-WP06` | **Constellation.** Graph projection from accepted curated nodes (reuse MP-6 generations); bounded graph query (G1); Constellation page in the UI2 shell with node aside (evidence, revisions, superseded links) and "refresh this node"; review-by-exception screen for staged generations; EN/ID copy and design gap ledger | MP-UI2-07, MP-UI2-BG-01, review screen part of MP-UX-06 | MP-R1-WP03 | `ui-memory`, `graph-query` | C |
| `MP-R1-WP07` | **R1 candidate.** Freeze one SHA, package, run one batched local runbook (import trio, four read clients, curation on one topic, Constellation on WebView2/DPI), owner acceptance | — | WP01–WP06 | `release` | C + L + O |

### Release 2

| ID | Package | Absorbs | Prerequisites | Lock | Gate |
| --- | --- | --- | --- | --- | --- |
| `MP-R2-WP01` | **Curated write over local MCP.** `curation_submit` and `node_patch` tools with the CNF v1 payload, routed through the same ingest gate; local-tier credentials only; quarantine and owner publish unchanged | curated-write slice of MP-7MCP-03 | MP-R1-WP04 | `mcp-write-local` | C + L, IR |
| `MP-R2-WP02` | **Remote gateway.** ADR plus listener for the cloud tier only, OAuth 2.1 + PKCE, discovery metadata, owner-pre-registered clients, rate limits, active only while unlocked with consent; Tailscale Funnel adapter | MP-NET-01, MP-NET-02 | MP-SEC-10 | `remote-gateway` | C + L, IR |
| `MP-R2-WP03` | **Hosted curated write for Claude and Gemini.** `curate_topic` (one topic) and `curate_active_chat` (the client sends the active-chat excerpt; Mind Place stores it as a new raw capture revision first, then curated records must cite it); per-client quarantine; owner publish; ChatGPT grants stay off | Claude + Gemini slice of MP-7MCP-05 and MP-7MCP-07 | MP-R2-WP01, MP-R2-WP02 | `mcp-write-cloud` | C, IR |
| `MP-R2-WP04` | **Hosted qualification.** claude.ai custom connector and the Gemini app through the universal URL: read curated, write curated, revoke and lock tests; Parity Matrix cells recorded | MP-NET-03, hosted Claude/Gemini cells of MP-PAR-02 | MP-R2-WP03 | `hosted-account` | L + O |
| `MP-R2-WP05` | **Workspace board.** Workspace ADR; read mirror of GitHub issues, PRs and plan files; Last Done / Now / Up Next computed from the dependency graph (no manual Focus Lane); task ↔ topic ↔ curated node links | MP-WS-00, MP-WS-01 | MP-R1-WP03 | `workspace` | C |
| `MP-R2-WP06` | **Claims, locks and hand-off.** Atomic claims with lease and fencing token; engine-enforced resource locks; status-dependent "hand to agent" action that generates a brief (task text passed as data, allowlisted runner templates) | MP-WS-02, MP-WS-03 | MP-R2-WP05 | `workspace` | C + L, IR |
| `MP-R2-WP07` | **R2 candidate.** Freeze, package, batched local runbook, owner acceptance | — | WP01–WP06 | `release` | C + L + O |

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

- **Now:** MP-R1-WP01, MP-R1-WP03 (Ready), in-flight MP-7MCP-06 (#216) and MP-UI2-06 (#215),
  plus the other eligible tasks already in Now, ordered after the work packages.
- **Up Next:** MP-R1-WP02 (after MP-7MCP-06), MP-R1-WP04 and MP-R1-WP06 (after MP-R1-WP03),
  MP-R2-WP05 (after MP-R1-WP03), MP-R2-WP02 (after MP-SEC-10).
- Everything else waits on a predecessor outside Now and has no lane.

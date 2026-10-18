# Mind Place — BIG implementation plan

This is the high-level roadmap for Mind Place. It is intentionally less rigid than the detailed implementation contracts under `docs/implementation-plan/`. A capable implementation agent should be given a phase, inspect the current repository, make the best engineering decisions that satisfy the goals below, verify the result, and leave a short implementation receipt.

Where an older implementation plan conflicts with this document, this document is the current product direction.

## 1. The product we are actually building

Mind Place is not a ChatGPT memory plugin. It is a **local continuity and knowledge layer for AI in general**.

ChatGPT, Claude, Gemini, Codex, Claude Code, local models, and future AI clients are replaceable frontends. Mind Place is the durable brain they can all research.

The product should make this normal:

```text
ChatGPT today
   |
Claude tomorrow
   |
Gemini next week
   |
Codex on one project
   |
local model offline
   |
   v
same Mind Place
same evidence
same projects
same historical context
same personal continuity
```

The core principle remains:

> Raw source is sacred. Derived intelligence is disposable. Models are replaceable. Personal continuity is not.

Mind Place may contain excellent memories, summaries, graphs and rankings, but none of those may become an unquestionable source of truth that makes the connected model less intelligent.

### Older ideas that remain part of the vision

Earlier Mind Place discussions produced several ideas that are still useful and should not disappear just because the current shipping product began as a ChatGPT archive:

- **A conversation is not the identity of a project.** Projects/workspaces need durable identity across many chats and many model providers.
- **Compact & Resume should become provider-independent.** A long working session should be resumable by another model through a small evidence-backed handoff/context pack rather than by copying a giant transcript.
- **Project state is broader than memory.** Goals, plans, decisions, milestones, artifacts, unresolved questions and important handoffs may become first-class indexed objects later.
- **Capture and execution are separate concerns.** Mind Place should not couple its durable brain to consumer-browser automation or one provider's UI controls.
- **The visual metaphor is an investigation space.** The original "Mind Place" name was inspired by the mental case-board idea: a user should eventually be able to explore evidence, relationships and timelines as a knowledge workspace rather than stare at database rows.
- **The model race is expected.** A better model appearing tomorrow should increase the value of the same archive, because derived memories/graphs can be re-evaluated without rebuilding personal history from scratch.

## 2. Memory is evidence, not truth

The most important refinement to the original memory design is that a memory is a **claim with provenance and state**, not a fact injected blindly into every prompt.

A useful long-term memory record should be able to represent:

```text
claim
  id
  statement
  scope
  type
  asserted_by                user | model | import | deterministic process
  epistemic_origin           explicit | observed | inferred | summarized
  status                     active | superseded | disputed | uncertain | retracted | historical
  confidence                 optional, mostly meaningful for inference
  valid_from / valid_until
  observed_at
  last_verified_at
  source_refs[]
  supersedes[]
  contradicts[]
  supports[]
  metadata
```

### Retrieval rule

Mind Place should never silently say "memory says X, therefore X is true".

When a claim matters to an answer:

1. prefer the user's current instruction over stored memory;
2. distinguish explicit user statements from model inference;
3. prefer current scoped claims when scope is clear;
4. surface material conflicts instead of hiding them;
5. allow the model to inspect original evidence;
6. keep old/wrong claims for history, but mark and down-rank them;
7. support `as_of` retrieval so an AI can ask what was believed at a past point in time;
8. never delete raw evidence merely because a curated claim was corrected.

Example:

```text
2026-08: "Use Model A for implementation"
  status: superseded

2026-10: "Use Model B for implementation"
  status: active
  supersedes: previous claim
```

If later evidence makes both uncertain, both remain inspectable. A connected frontier model can re-evaluate the evidence instead of inheriting an old summarizer's mistake.

### Retrieval modes

Useful presets should eventually include:

- `memory_fast` — compact active claims first, source fetch only when needed;
- `balanced` — claims plus supporting evidence and relevant history;
- `source_first` — raw evidence first, memories used mainly as navigation hints;
- `challenge` — deliberately include contradictions, disputed claims and alternative evidence;
- `historical` — resolve information as of a chosen date/revision.

The default should be `balanced`.

## 3. Storage architecture decision

SQLite is still the best primary database technology for Mind Place, but Mind Place should **not put every byte into one SQLite file**.

The workload is unusual: very high read frequency, relatively low write concurrency, many structured filters, full-text search, graph adjacency, immutable source blobs, and a local single-user deployment. SQLite fits the structured/query side extremely well. RocksDB and LMDB can beat it at narrower key/value workloads, but would force Mind Place to build relational querying, FTS, graph/filter joins and migration tooling around them. Tantivy is a strong search engine, but using it from day one would add another durable index format before FTS5 has demonstrated a real bottleneck.

Recommended layout:

```text
Mind Place data/
  archive-v1/
    conversations/          existing immutable raw captures
    objects/                content-addressed binaries/document versions
    extracted/              optional compressed extracted-text objects

  state.sqlite3             small authoritative mutable state
  index.sqlite3             disposable/rebuildable search + derived intelligence
```

### `state.sqlite3` — authoritative mutable state

Contains things that cannot simply be regenerated:

- approved folder identities and revocation/tombstone state;
- user settings;
- client identities and permissions;
- curated memory claims and their history;
- manual graph overrides;
- user-created tags/projects/scopes;
- retention policies;
- maintenance-job state that must survive restart.

Use conservative durability settings here. Back it up before migrations.

### `index.sqlite3` — rebuildable performance layer

Contains:

- source/chunk catalog;
- FTS5;
- extracted metadata;
- derived graph nodes/edges/communities;
- ranking statistics;
- optional embedding vectors;
- context-pack cache;
- parser/Graphify manifests;
- retrieval telemetry that is safe to discard.

If this database corrupts, Mind Place should be able to delete and rebuild it from raw evidence plus `state.sqlite3`.

### Stable identity across rebuilds

Authoritative state may reference conversations, messages, files, document versions, graph subjects or other source objects. Those references must survive deletion of `index.sqlite3`.

Therefore every ID that can appear inside `state.sqlite3` must satisfy one of these rules:

1. it is deterministically reconstructible from immutable/source identity, or
2. its identity mapping is itself persisted in `state.sqlite3`.

The disposable index may cache or reproject those IDs, but it may not invent a random canonical ID that an authoritative memory/source reference or manual graph override depends on. Rebuilding the derived index must reproduce the same referents.

### Raw/object storage

Keep large immutable source material outside SQLite:

- current JSONL capture revisions remain valid L0 evidence;
- document/file versions use SHA-256 content-addressed objects;
- identical bytes across paths and revisions are stored once;
- large extracted text may be Zstandard-compressed as derived objects;
- SQLite stores identity, provenance, ranges, hashes and pointers.

This avoids database bloat and keeps backup/export formats inspectable.

## 4. SQLite profile for Mind Place

Target a bundled modern SQLite build rather than whichever version happens to be installed on the machine. As of 2026-10-18, SQLite 3.54.1 is the current patch release; the earlier 3.53 line fixed the WAL-reset corruption bug, and 3.54 adds further query-planner/Windows performance work. The implementation should therefore target **3.54.1 or newer after qualification**, and upgrade deliberately rather than inherit a random system library.

Use WAL because MCP reads and background indexing should coexist. SQLite WAL allows readers and a writer to operate concurrently, while still having a single writer, which matches Mind Place's workload.

Suggested policy:

```text
state.sqlite3
  journal_mode = WAL
  synchronous = FULL or equivalent conservative profile
  foreign_keys = ON
  busy_timeout configured

index.sqlite3
  journal_mode = WAL
  synchronous = NORMAL
  foreign_keys = ON
  batched indexing transactions
  checkpoint/optimize during idle windows
```

The exact cache/page/mmap values should be benchmarked on representative corpora rather than hard-coded from generic SQLite tuning folklore.

### FTS design

Start with SQLite FTS5.

For the first production implementation, keep chunk text once in a derived `chunks` table and use an **external-content FTS5 table** maintained in the same transaction. This gives BM25, phrase/NEAR queries and fast snippet generation without FTS storing a second private copy of the full chunk content.

Use normal B-tree indexes for IDs, dates, providers, scopes, paths and exact metadata filters before FTS ranking.

Do not create one huge trigram index over all body text by default. If code/path substring lookup needs it, use a small trigram FTS index for filenames, paths, symbols and short identifiers. SQLite's trigram tokenizer is useful for substring matching, but indexing every character trigram of years of prose would spend unnecessary disk space.

### Vector search

Vectors stay optional. `sqlite-vec` is attractive because it keeps vector retrieval inside SQLite, but it remains pre-1.0 and should therefore be treated as a replaceable derived extension. If semantic retrieval becomes necessary, benchmark it behind an interface and never make vector data canonical.

### When SQLite stops being enough

Do not migrate because another engine is theoretically faster.

Escalation order:

1. optimize schema/query/index strategy;
2. split hot/cold derived indexes;
3. benchmark a Rust Tantivy search index for the lexical layer while keeping SQLite as metadata authority;
4. adopt Tantivy only if measured corpora show FTS5 is the actual retrieval bottleneck.

RocksDB/LMDB are not current targets because Mind Place needs structured queries and full-text research more than raw KV throughput.

## 5. Authorization must happen before information leaves the engine

An approved folder is a live permission, not just an indexing hint.

Revoking a root must prevent its content from appearing in:

- search snippets;
- graph results;
- context packs;
- exact fetch;
- cached retrieval responses;
- maintenance jobs.

Filtering after `memory_search` has already produced a snippet is too late. Search candidates must carry source/root identity, and the current authorization snapshot is applied **before ranking output or snippet emission**.

Previously archived copies can remain locally if the user chose that retention policy, but they become inaccessible to ordinary model clients once their source permission is revoked. The UI can offer a separate destructive action to remove retained local copies.

Filesystem ingestion should use the strongest CoS-derived path model, including canonical approved roots and handle-level revalidation around file opens so a symlink/junction swap cannot race between validation and ingestion.

PDF/Office/archive parsers must have expanded-size, nesting, page/object, CPU/time and memory limits; small compressed input must not be allowed to expand without bounds.

## 6. MCP philosophy

MCP is an adapter, not Mind Place itself.

Core retrieval must remain callable without MCP so another future protocol can replace it.

Keep the model-facing surface small and configurable:

```text
memory_search
memory_fetch
memory_get / memory_write (when allowed)
graph_neighbors
context_build
```

Most complexity belongs in parameters:

- queries / query weights;
- source types;
- providers/models/roles;
- projects/tags/scopes;
- folder roots/path patterns/file types;
- date ranges / `as_of`;
- memory status/origin/confidence;
- lexical/exact/semantic modes;
- graph expansion depth/relation/origin;
- ranking weights;
- granularity;
- grouping/diversity;
- version policy;
- max results/per-source limit;
- token/byte/latency budgets;
- provenance detail;
- freshness requirements.

The model can research iteratively instead of receiving an enormous prebuilt memory prompt.

Hosted clients use a narrow HTTPS tunnel. Local agents can use direct local MCP. Read and write/maintenance capabilities remain separate. Graphification/write access should use **short-lived job credentials** bound to one scope/generation, with batch limits and revocation on finish/cancel, rather than a permanent maintenance URL.

## 7. BIG roadmap

Mind Place development is **cloud-first**. ChatGPT/Codex or another capable coding agent should do implementation in a cloud development environment backed by GitHub. The local Windows machine is primarily a qualification target for behavior that genuinely depends on the user's device, browser profile, provider account, WebView2/Tauri runtime, filesystem semantics, keychain, tunnel reachability, or representative private corpus.

The user should not need to babysit cloud implementation. The intended rhythm is:

```text
give cloud agent one bounded unit
        |
        v
agent implements + tests + pushes GitHub
        |
        v
CI green + cloud handoff
        |
        +---- more cloud units in this phase ----+
        |                                         |
        v                                         |
phase cloud-complete                              |
        |                                         |
        v                                         |
handoff to user                                   |
        |                                         |
        v                                         |
pull exact SHA locally                            |
        |                                         |
        v                                         |
local qualification agent                        |
        |
        +--- FAIL ---> evidence handoff ---> cloud fix unit ---> new SHA ---> retest
        |
        v
PASS
        |
        +--- user acceptance required? ---> user tests only the irreducibly human flow
        |
        v
next phase
```

### 7.1 One cloud development unit

One cloud unit is deliberately smaller than one product phase. It should normally own **one coherent subsystem or one vertical integration boundary**, including its tests, migrations/contracts, and documentation changes. A unit is sized so a strong agent can understand the relevant code without needing to reconstruct the whole project or rely on context compaction to finish safely.

Default rule:

- one cloud unit = one explicit instruction such as `implement phase 4C`;
- keep `main` or the phase integration branch CI-green at the handoff boundary;
- prefer a short-lived branch/PR and merge on green rather than accumulating a giant unreviewable phase branch;
- each unit may contain several commits if useful, but should expose one reviewable outcome;
- no local-only workaround becomes the canonical implementation;
- if a cloud unit discovers that its scope was underestimated, finish the coherent subset, push it, and hand off the remainder as the next named unit rather than rushing an unsafe mega-patch.

### 7.2 Cloud handoff contract

Every cloud unit should finish with a compact handoff containing:

```text
UNIT
branch / PR / merge status
final commit SHA

CHANGES
what was implemented
schema/protocol/migration decisions

VALIDATION
exact cloud/CI checks and results
benchmark results where relevant

LOCAL REQUIRED
no | phase gate only | immediate
why local qualification is needed

KNOWN LIMITS
anything intentionally deferred

NEXT
the exact next cloud unit or local gate
```

The final cloud unit of a phase additionally produces a **phase handoff** summarizing every merged unit and the exact SHA local qualification must pull.

### 7.3 Local qualification contract

Local qualification starts only after the user has the cloud handoff. The local agent should:

1. pull/checkout the exact approved GitHub SHA;
2. verify the tree is clean and identify the machine/runtime used;
3. run only the device-dependent qualification assigned to that gate;
4. collect logs, screenshots/receipts and benchmark evidence when relevant;
5. return `PASS`, `FAIL`, `PARTIAL` or `BLOCKED` with exact evidence;
6. **not silently turn the local checkout into the main implementation branch**.

If local qualification exposes a product bug, the default route is:

```text
local failure handoff
        -> user
        -> cloud fix unit
        -> GitHub SHA
        -> local retest
```

Small diagnostic edits are fine when needed to prove the cause, but the product fix should return to the cloud source of truth unless the user explicitly authorizes local implementation.

### 7.4 Manual user testing contract

The user's own testing time is reserved for things an automated local agent cannot honestly establish, such as:

- signing into the user's real hosted AI account or approving connector permissions;
- deciding whether retrieval quality feels correct on real personal/project questions;
- checking whether a UX interaction is understandable rather than merely functional;
- approving destructive privacy/forget/restore behavior;
- verifying a provider-specific live flow that cannot be safely automated with fixtures.

Manual acceptance should come **after** automated local qualification is green. Each manual gate should be a short checklist prepared by the agent, not an open-ended debugging session.

### 7.5 Planned workload notation

Each phase below lists:

- **C** — planned cloud development units;
- **L** — planned local qualification sessions;
- **U** — planned manual user acceptance sessions.

Fix/retry units created after a failed gate are intentionally not counted in the base plan.

Base planning matrix:

| Phase | Cloud development | Local qualification | User acceptance | Why local/user exists |
| --- | ---: | ---: | ---: | --- |
| 0 — Current foundation | 0 | 0 | 0 | already qualified |
| 1 — Security + durable brain storage | 5 | 0 | 0 | backend + Windows CI are sufficient |
| 2 — Retrieval kernel | 4 | 0 | 0 | deterministic/headless |
| 3 — MCP + tunnel | 5 | 2 | 1 | real Windows client + real hosted connector/account |
| 4 — Documents/folders | 6 | 2 | 1 | NTFS/watcher/parser corpus + folder UX |
| 5 — Memory truth | 4 | 0 | 0 | synthetic conflict/history corpus can prove semantics; UX waits for Phase 8 |
| 6 — Graph/Graphify | 5 | 0 | 0 | cloud repos/fixtures can prove graph engine; real UX waits for Phase 8/10 |
| 7 — Multi-provider | 5 | 2 | 2 | real Claude/Gemini account/browser behavior |
| 8 — Knowledge OS UI | 6 | 2 | 2 | packaged WebView + subjective daily/Hive UX |
| 9 — Privacy/portability | 6 | 2 | 2 | Windows keychain + destructive/recovery UX |
| 10 — v1.0 qualification | 5 | 3 | 2 | target hardware/install/update/full real E2E |
| **Mandatory total** | **51** | **13** | **10** | |

These are a **safe ceiling for planned handoff boundaries**, not a target for bureaucracy. Adjacent low-risk cloud units may be combined after the earlier unit is green and the agent still has healthy context. Do not combine schema, security, filesystem authorization, provider live behavior, or destructive-data work merely to reduce the prompt count.

### Phase 0 — Current archive foundation — DONE

Planned remaining workload: **C0 / L0 / U0**.

Already proven:

- Rust/Tauri local archive reader;
- dedicated ChatGPT capture extension;
- manual bounded history capture;
- opt-in loaded-DOM automatic capture;
- immutable raw revisions;
- offline reader/search/export;
- Windows release packaging;
- strict capture/privacy boundaries.

This becomes the compatibility baseline. New phases should extend it rather than rewrite it unnecessarily.

### Phase 1 — Security root and durable brain storage

Planned workload: **C5 / L0 / U0**. Security was deliberately promoted ahead of storage; `CURRENT-PHASE.md` records the live execution state.

**Cloud 1A — security root and fail-closed unlock — DONE**

- versioned Argon2id security verifier and explicit first-run/unlock flow;
- locked-by-default startup, unlock epoch/lease invalidation and privileged desktop command gating;
- no claim of encryption-at-rest at this stage.

**Cloud 1B — authoritative state database — DONE**

- introduce `state.sqlite3` with transactional migrations and verified backups;
- memory-claim history schema, root ACL/tombstones, client/security policy and project/tag/scope primitives;
- pin/bundle the SQLite runtime used by Rust and qualify it in Windows CI;
- fail closed on foreign, corrupt, future-schema or invalid migration state without rewriting raw archives.

**Cloud 1C — stable identity and authority durability — DONE**

- stable provider-neutral source IDs and deterministic/persisted identity mapping;
- migration rollback/recovery and state recovery tests;
- prove authoritative references cannot depend on disposable random index IDs.

**Cloud 1D — derived index + FTS — DONE**

- introduce disposable `index.sqlite3`;
- index existing immutable ChatGPT archive into derived source/chunk rows;
- FTS5 external-content index;
- rebuild command and incremental update path.

**Cloud 1E — storage qualification harness — DONE**

- benchmark representative 10K/100K/1M chunk corpora;
- prove deleting `index.sqlite3` loses no canonical information;
- Windows GitHub Actions coverage for migration/rebuild and bundled SQLite/runtime integration.

Completed in [PR #5](https://github.com/KaisAbiyyi/mind-place/pull/5); [Windows storage CI](https://github.com/KaisAbiyyi/mind-place/actions/runs/39112178136) passed the three tiers, with detailed receipts and current SQLite-version limits in [`phase-1e-storage-qualification.md`](implementation-plan/phase-1e-storage-qualification.md).

No local or manual gate is required unless cloud Windows CI exposes a platform-only uncertainty that cannot be reproduced there.

Exit: the current chat archive is searchable through the new engine with stable IDs and predictable latency.

### Phase 2 — Universal retrieval kernel

Planned workload: **C4 / L0 / U0**.

**Cloud 2A — search request model + structured filters**

- multi-query search;
- structured filtering;
- BM25/exact ranking;
- stable compact search-result contract.

**Cloud 2B — progressive fetch + budgets**

- message/window/revision fetch;
- token/byte/latency budgets;
- result truncation/freshness metadata.

**Cloud 2C — context builder + retrieval policies**

- context builder;
- overlap dedupe;
- grouping/diversity;
- retrieval presets (`fast`, `balanced`, `source_first`, `challenge`, `historical`);

**Cloud 2D — observability + retrieval qualification**

- local retrieval traces and score/provenance inspector data.
- deterministic regression corpus and ranking tests;
- performance regression gates against the Phase 1 benchmark harness.

No phase-local session is mandatory: this phase is deliberately headless and can be qualified completely in cloud CI/synthetic corpora. Real-device retrieval quality is exercised when MCP and personal sources become available in later phases.

Exit: a model can research years of chat history without loading whole conversations.

### Phase 3 — Shared-brain MCP and hosted tunnel

Planned workload: **C5 / L2 / U1**.

**Cloud 3A — protocol-independent integration core**

- protocol-independent core adapter;
- read MCP tools;
- batching;

**Cloud 3B — client authorization**

- enforced client identities/capabilities and immediate revocation;
- minimum read/write/admin separation before any external client is connected;
- local API/MCP authorization audit;

**Cloud 3C — local MCP transport**

- Streamable HTTP/local transport compatibility;
- lifecycle/drain/error semantics;
- request/body/result bounds;
- direct local setup for Codex/Claude Code/other agents.

**Cloud 3D — hosted tunnel + setup flow**

- loopback HTTP MCP;
- secure HTTPS tunnel lifecycle;
- separate capability-scoped endpoints;
- ChatGPT setup UI + request proof;
- reusable connector setup patterns for other hosted products that support remote MCP.

**Cloud 3E — security and E2E cloud qualification**

- tunnel/public-surface security audit before publication;
- stale/revoked credential tests;
- read-client escalation tests;
- automated MCP conformance and simulated tunnel tests;
- phase handoff with exact local connector procedures.

**Local 3L1 — direct local MCP qualification**

- packaged Tauri/core process on Windows;
- real Codex/Claude Code or equivalent local MCP client;
- search/fetch/context flows over an actual existing archive;
- disconnect/restart/revocation behavior.

**Local 3L2 — real hosted-tunnel qualification**

- actual outbound HTTPS tunnel from the user's Windows/network environment;
- public endpoint cannot reach admin/write surfaces;
- request proof from a real hosted-client-compatible route;
- restart/token rotation/revocation behavior.

**User 3U — hosted AI acceptance**

- connect the user's real ChatGPT account/connector;
- ask a small set of known project-history questions;
- verify source/provenance navigation is believable;
- revoke the client and confirm the next request loses access.

Exit: a new AI client with no conversation history can recover project context through Mind Place alone.

Security exit condition: a revoked client loses access on its next request, the public tunnel cannot reach admin/write surfaces it was not granted, and no hosted read client can escalate itself to maintenance authority.

### Phase 4 — Personal documents and approved folders

Planned workload: **C6 / L2 / U1**.

**Cloud 4A — approved-root state + virtual namespace**

- approved-folder UI and canonical security boundary;
- virtual paths;

**Cloud 4B — filesystem authorization hardening**

- handle-level TOCTOU protection;
- traversal/symlink/junction/reparse and root-replacement tests;
- Windows/path edge-case fixtures.

**Cloud 4C — scanner/watcher/reconciliation**

- scanner/watcher/reconciliation;
- include/exclude/retention policies;
- immediate authorization filtering on search and fetch.

**Cloud 4D — document versions + object store**

- content-addressed versions/dedup;
- metadata fast-path for unchanged files;

**Cloud 4E — extraction + bounded parsers**

- text/source/Markdown/PDF/OOXML extraction with resource limits;
- structure-aware chunks;

**Cloud 4F — search/UI integration**

- incremental FTS updates;
- one-off file import.
- revoked-root filtering across search/graph/context/cache paths;
- phase-scale filesystem/index benchmark fixtures.

**Local 4L1 — real Windows filesystem security qualification**

- approved roots on NTFS;
- actual junction/symlink/reparse/rename/root replacement cases;
- watcher event loss/reconciliation;
- revoke a root while indexing/searching and verify zero result leakage.

**Local 4L2 — real document corpus qualification**

- index a representative user-approved folder with source code, Markdown, PDF and Office files;
- record initial scan, no-op rescan and changed-file latency;
- validate parser limits on harmless stress fixtures;
- confirm preserved document versions/dedup on real storage.

**User 4U — folder UX acceptance**

- add a real folder from the UI;
- inspect what Mind Place says it can read and exclude;
- search/fetch a few known documents;
- revoke it and verify the behavior is understandable.

Exit: chat and personal documents are one searchable evidence universe.

### Phase 5 — Memory truth engine

Planned workload: **C4 / L0 / U0**.

**Cloud 5A — claim transitions + historical state**

- append-oriented memory claims;
- supersession/dispute/retraction/uncertainty/history;
- evidence references;
- `as_of` state reconstruction;

**Cloud 5B — conflict resolution + ranking**

- claim relations (`supports`, `contradicts`, `supersedes`, `refines`);
- ranking that distinguishes authority, scope, recency and inference confidence.
- `balanced`, `source_first`, `challenge`, historical/current behavior;
- conflict-aware context serialization;
- regression cases where an old/wrong memory must not override current/raw evidence.

**Cloud 5C — memory write/update/delete/forget workflows**

- search-before-write;
- user/model memory create/update flows;
- capability checks and idempotent write behavior;
- safe deletion/forget cascade.

**Cloud 5D — provenance/history/conflict UI**

- memory diff/history;
- conflict viewer;
- pin/manual correction;
- "why is this remembered?" provenance inspection;
- synthetic temporal/conflict fixtures that explicitly prove obsolete or disputed claims cannot silently override current/raw evidence.

No local/manual gate is required here. The semantics should be proven deterministically in cloud tests; subjective memory UX and real-project acceptance are deliberately deferred to Phase 8 and the final Phase 10 continuity test.

Exit: wrong memories can exist without silently poisoning the AI, and better models can re-evaluate old evidence.

### Phase 6 — Knowledge graph and Graphify

Planned workload: **C5 / L0 / U0**.

**Cloud 6A — graph schema + deterministic edges**

- graph adjacency tables in `index.sqlite3`;
- projects/topics/files/conversations/memories/entities;
- deterministic contains/references/version/import/symbol relationships;

**Cloud 6B — graph retrieval + scale strategy**

- graph-neighborhood retrieval;
- community aggregation for large graphs.

**Cloud 6C — generation manifests + incremental rebuild**

- graph generation manifests;
- incremental invalidation by file/content/structure hash;
- staging + atomic activation;

**Cloud 6D — scoped Graphify maintenance protocol**

- short-lived scoped maintenance jobs;
- idempotent/sequence-checked batches;
- generation/root/revision authorization;

**Cloud 6E — prompt generator + Graphify qualification**

- ready-to-paste `Codebase`, `Documents`, `Mixed` graphification prompts;
- confidence/provenance on inferred edges.
- synthetic Graphify/evidence coverage evaluation and large-graph lazy/community tests.

No local/manual gate is required by default. Cloud repositories/fixtures can exercise the graph engine and maintenance protocol; real project usefulness and graph UX are intentionally evaluated once the Knowledge OS UI exists in Phase 8 and again in Phase 10.

Exit: the graph improves navigation and recall but remains a derived map, never canonical truth.

### Phase 7 — Multi-provider capture

Planned workload: **C5 / L2 / U2**.

**Cloud 7A — provider-neutral contract + ChatGPT regression refactor**

- formal provider adapter contract;
- move ChatGPT capture behind the same normalized interface;
- preserve all current ChatGPT capture/privacy semantics under the adapter framework;
- fixture/regression coverage proving no archive behavior regression.

**Cloud 7B — Claude adapter**

- Claude conversation adapter/import path;
- provider/model/branch/regeneration metadata normalization.

**Cloud 7C — Gemini adapter**

- Gemini adapter/import path;

**Cloud 7D — generic provider import framework**

- portable JSON/HTML/Markdown provider import framework;

**Cloud 7E — provider capability matrix + regression suite**

- provider capability matrix;
- regression fixtures so a provider UI change cannot corrupt existing archives.

**Local 7L1 — live Claude qualification**

- real signed-in Claude browser flow/import surface available to the user;
- manual/automatic behavior only where the provider safely supports it;
- duplicate/revision/private conversation semantics;
- normalized archive roundtrip.

**User 7U1 — Claude acceptance**

- save/import one known Claude conversation;
- confirm transcript, model/provider metadata and revision behavior.

**Local 7L2 — live Gemini qualification**

- equivalent real Gemini flow;
- provider-specific URL/DOM/privacy and regeneration behavior.

**User 7U2 — Gemini acceptance**

- save/import one known Gemini conversation and verify it is usable through the same retrieval/MCP path as ChatGPT/Claude.

Exit: "one brain" is no longer synonymous with ChatGPT capture.

### Phase 8 — Knowledge OS UI

Planned workload: **C6 / L2 / U2**.

**Cloud 8A — information architecture + large-data UI foundation**

- navigation/data contracts;
- virtualization/lazy loading primitives;
- responsive/error/loading states.

**Cloud 8B — conversations/files/search library**

- unified Conversations / Files / Memories / Projects / People / Topics library;
- advanced search playground;
- context preview;
- source/version inspector;

**Cloud 8C — memories/projects/entities workspace**

- memory/project/person/topic detail flows;
- timelines and source relationships;

**Cloud 8D — retrieval/health workspace**

- capture/index/connector health.

- retrieval inspector;
- context cost/provenance/debug flow.

**Cloud 8E — Hive / graph playground**

- semantic zoom;
- lazy graph expansion;
- timeline;
- direct relationship editing;
- memory conflict UI;

**Cloud 8F — UI integration/performance/accessibility**

- end-to-end desktop navigation tests;
- large-library rendering budgets;
- keyboard/accessibility pass;
- error/recovery states;
- final phase integration polish without redesigning proven backend contracts.

**Local 8L1 — packaged desktop functional qualification**

- packaged Tauri app against a real archive/index;
- all primary library/search/memory/health flows;
- Windows/WebView2 behavior and startup/restart.

**Local 8L2 — real-scale UI performance qualification**

- representative large library and graph;
- scrolling/virtualization/semantic-zoom responsiveness;
- memory growth and repeated navigation/retrieval inspector use.

**User 8U1 — daily-library UX acceptance**

- perform normal daily navigation/search/context-preview tasks without developer instructions;
- report confusing labels/flows only after functionality is green.

**User 8U2 — Hive UX acceptance**

- explore a real project graph, timeline and source drill-down;
- judge whether the visual hierarchy helps thinking rather than creating decorative noise.

Exit: ordinary maintenance and debugging no longer require raw database tools.

### Phase 9 — Privacy, portability and resilience

Planned workload: **C6 / L2 / U2**.

**Cloud 9A — encryption design + measured prototype**

- encrypted-at-rest strategy and keychain UX after benchmarking;
- benchmark state/index/object encryption choices before committing migrations;

**Cloud 9B — key management + encrypted storage integration**

- OS keychain/key lifecycle;
- locked/unlocked/recovery behavior;

**Cloud 9C — privacy policy + destructive semantics**

- private scopes / source redaction;
- advanced permission UX and policy profiles on top of the minimum controls already required by Phases 3-4;
- secure destructive delete/forget semantics for retained canonical data;

**Cloud 9D — isolation + mature-surface security hardening**

- bounded parser/maintenance-job isolation;
- repeat/fuzz the tunnel and local API security audit against the mature feature set.

**Cloud 9E — backup/restore/integrity**

- atomic backup/restore;
- integrity verification;

**Cloud 9F — portability + migration compatibility**

- portable archive export/import;
- rebuild derived indexes after restore;
- migration/version compatibility tests.

**Local 9L1 — Windows keychain/encryption qualification**

- real Windows credential/keychain behavior;
- app restart, locked/unlocked, backup and recovery paths;
- verify performance impact on representative data.

**Local 9L2 — destructive backup/restore qualification**

- operate only on controlled copies of a real archive;
- backup, restore, migrate, rebuild index and verify hashes/source counts;
- exercise delete/forget cascades with recovery evidence.

**User 9U1 — unlock/key-recovery acceptance**

- evaluate first unlock, restart/lock behavior and recovery messaging;
- confirm the key-management UX is understandable without implementation knowledge.

**User 9U2 — destructive/recovery acceptance**

- evaluate backup/restore/forget prompts;
- explicitly approve whether destructive and recovery semantics are understandable enough for v1.0.

Exit: Mind Place can safely become long-lived personal infrastructure rather than a single-machine experiment.

### Phase 10 — Intelligence bake-off, scale qualification and v1.0

Planned mandatory workload: **C5 / L3 / U2**.

Optional semantic/search branch if justified: **+C2 / +L1 / U0 by default**.

**Cloud 10A — benchmark corpus + gold questions**

- build a representative personal benchmark suite;
- define retrieval/support-quality metrics and reproducible gold cases.

**Cloud 10B — retrieval bake-off + tuning**

- measure lexical, structured, graph and hybrid recall/latency;
- measure context-token cost and answer support quality;
- identify actual misses instead of assuming embeddings solve them.

**Cloud 10C — scale/performance qualification**

- 1M+ chunk / 100K-file scale tests;
- startup/reconciliation benchmarks;

**Cloud 10D — failure/recovery qualification**

- corruption/rebuild tests;
- crash/interrupted-write recovery;
- backup/migration compatibility matrix.

**Cloud 10E — release engineering + immutable v1.0 RC**

- cross-provider continuity acceptance scenarios;
- installer/update/migration qualification;
- performance budgets and regression gates.
- close remaining release-blocking gaps;
- final documentation/support matrix;
- produce one immutable RC SHA and exact local/manual acceptance handoff.

**Local 10L1 — representative-hardware performance qualification**

- cold/warm startup;
- 1M-chunk/100K-file or largest practical qualified fixture;
- retrieval latency, background indexing, UI responsiveness and resource usage.

**Local 10L2 — installer/update/migration qualification**

- clean install;
- upgrade from the last public pre-v1 release;
- state/index migration and rollback/recovery evidence;
- packaged extension/native-host registration.

**Local 10L3 — full one-brain E2E qualification**

- at least two hosted/local AI clients;
- chat + document + memory + graph retrieval;
- client revoke;
- app restart;
- provider continuity scenario on the same project.

**User 10U1 — real continuity acceptance**

- begin with a fresh AI conversation/model;
- ask it to recover one real ongoing project using Mind Place only;
- judge whether important state is recovered without misleading memory dominating the answer.

**User 10U2 — release acceptance**

- use the RC as a normal user across the critical daily flows;
- explicitly approve the release or return a short blocking list.

**Optional Cloud 10S1 — semantic retrieval experiment/integration**

- benchmark `sqlite-vec` or another local derived vector implementation;
- simple hybrid fusion with lexical/graph scores;
- integrate the measured winner behind the replaceable retrieval interface only if it wins the benchmark;
- preserve a zero-embedding fully functional mode.

**Optional Cloud 10S2 — Tantivy bake-off only if FTS5 itself is the bottleneck**

- Tantivy bake-off if FTS5 itself is the measured bottleneck;

**Optional Local 10SL — semantic/search winner qualification**

- reproduce the claimed gain on representative local hardware/corpus before the optional layer becomes a default recommendation.

Exit: **Mind Place v1.0 = model-independent one-brain continuity system**, with evidence-aware memory instead of a giant unquestioned prompt.

### Total expected sessions from the current state

Base plan:

| Work type | Planned sessions |
| --- | ---: |
| Cloud development units | **51** |
| Local automated/device qualification | **13** |
| Manual user acceptance | **10** |

Optional semantic/search branch, only if Phase 10 measurements justify it:

- **+2 cloud units**;
- **+1 local qualification**;
- normally **no additional manual user gate** unless it materially changes retrieval behavior visible in the release candidate.

The 51 cloud units are safe handoff boundaries, not a requirement to waste 50 separate calendar days. Once earlier architecture is stable, adjacent low-risk units can realistically collapse the actual cloud prompt count toward roughly **35-40** without increasing risk. A capable cloud agent may continue into the next unit in the same run **only when the current unit is merged/green, enough context remains, and the user's instruction explicitly allows continuing**. For unattended overnight work, one named cloud unit per instruction is the safest default because the user wakes up to a bounded diff and a deterministic handoff.

Manual user work is deliberately much smaller than cloud work. The target is that the user mainly approves real-account/provider behavior and subjective product quality, while agents handle implementation, CI, local diagnostics and reproducible qualification evidence.

## 8. Potential feature universe

These are possible features, not all v1.0 requirements.

### Continuity

- one shared personal/project brain across ChatGPT, Claude, Gemini, Codex, Claude Code and local models;
- instant "continue this project" context reconstruction on a fresh model;
- model-to-model handoff packs;
- project/workspace identity independent of individual chat IDs;
- provider migration without memory migration.

### Memory quality

- conflict-aware claims;
- time-travel / `as_of` memory;
- stale-memory detection;
- confidence and provenance badges;
- user correction/supersession history;
- "why did Mind Place retrieve this?" explanation;
- challenge mode that intentionally retrieves counter-evidence;
- memory-health dashboard for unsupported/old/contradicted claims.

### Sources

- ChatGPT/Claude/Gemini capture;
- Codex/Claude Code session import where formats are available;
- approved local folders;
- one-off files;
- exported chats from other providers;
- optional future connectors for notes, Drive, GitHub, email or other user-authorized sources.

### Retrieval

- rich parameterized search;
- bulk research calls;
- exact source windows;
- context packs;
- retrieval presets;
- project-aware and temporal search;
- semantic/graph expansion;
- query/result caching;
- prompt-cache-friendly stable project prefixes;
- fresh-index fallback verification when a watcher is behind.

### Graph / knowledge navigation

- Hive semantic map;
- project/topic/person/entity/decision nodes;
- code/document Graphify;
- community clustering;
- semantic zoom;
- source-linked graph edges;
- graph diff across document/project versions;
- user-authoritative manual edges over inferred edges.

### UI and control

- context preview before sending;
- per-client permission dashboard;
- root/folder permission manager;
- retrieval inspector;
- graph generation status;
- capture/index/tunnel health;
- global pause;
- source retention/delete controls;
- prompt generator for folder Graphify and connector setup.

### Privacy and portability

- encrypted vault;
- multiple vaults/profiles;
- private project scopes;
- portable offline archive;
- backup/restore integrity checks;
- optional end-to-end encrypted multi-device sync after v1.0;
- local-only mode with no public tunnel.

### Advanced future intelligence

- optional local model for maintenance/curation;
- automatic but reviewable project/topic suggestions;
- retrieval feedback learning from accepted/rejected context;
- per-model retrieval profiles based on context-window/cost behavior;
- automatic context warm-up when a new model connects;
- memory/graph rebuild using a newer model while preserving all old generations for audit;
- corpus quality benchmarks that detect whether a new retrieval strategy actually makes answers better.

## 9. What Mind Place should explicitly avoid

- treating summaries as canonical history;
- deleting wrong memories just to make the database look clean;
- injecting every remembered fact into every model call;
- mandatory embeddings or paid inference for ordinary retrieval;
- making one AI provider's schema the internal schema;
- a graph database solely because the product has a graph;
- a separate distributed database/service stack for a local single-user workload;
- exposing the whole filesystem because one project folder was approved;
- giving long-lived write authority to a hosted model by default;
- continuously running an autonomous model over all private files;
- optimizing benchmark numbers at the cost of provenance and reversibility.

The desired end state is a system that can remember almost everything while still allowing a smarter future model to disagree with, reinterpret, or completely rebuild the derived understanding of that history.

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

The user-facing unit below is an **implementation invocation**: one substantial instruction to the coding agent such as `implement fase 3A`. These counts are planning guidance, not an artificial requirement to split work when one capable agent can safely finish more.

### Phase 0 — Current archive foundation — DONE

Agent invocations remaining: **0**.

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

### Phase 1 — Durable brain storage core

Recommended agent invocations: **2** (`1A`, `1B`).

**1A — authority + schema foundation**

- introduce `state.sqlite3` with migrations/backups;
- stable provider-neutral source IDs;
- memory claim schema, root ACL/tombstone schema, project/tag/scope primitives;
- define `index.sqlite3` as disposable;
- pin/bundle the SQLite runtime used by Rust.

**1B — derived catalog + benchmark harness**

- index existing immutable ChatGPT archive into derived source/chunk rows;
- FTS5 external-content index;
- rebuild command;
- benchmark representative 10K/100K/1M chunk corpora;
- prove deleting `index.sqlite3` loses no canonical information.

Exit: the current chat archive is searchable through the new engine with stable IDs and predictable latency.

### Phase 2 — Universal retrieval kernel

Recommended agent invocations: **2** (`2A`, `2B`).

**2A — progressive research primitives**

- multi-query search;
- structured filtering;
- BM25/exact ranking;
- message/window/revision fetch;
- token/byte/latency budgets;
- result truncation/freshness metadata.

**2B — context and observability**

- context builder;
- overlap dedupe;
- grouping/diversity;
- retrieval presets (`fast`, `balanced`, `source_first`, `challenge`, `historical`);
- local retrieval traces and score/provenance inspector data.

Exit: a model can research years of chat history without loading whole conversations.

### Phase 3 — Shared-brain MCP and hosted tunnel

Recommended agent invocations: **2** (`3A`, `3B`).

**3A — local MCP**

- protocol-independent core adapter;
- read MCP tools;
- batching;
- enforced client identities/capabilities and immediate revocation;
- minimum read/write/admin separation before any external client is connected;
- local API/MCP authorization audit;
- direct local setup for Codex/Claude Code/other agents.

**3B — hosted AI connectivity**

- loopback HTTP MCP;
- secure HTTPS tunnel lifecycle;
- separate capability-scoped endpoints;
- tunnel/public-surface security audit before publication;
- ChatGPT setup UI + request proof;
- reusable connector setup patterns for other hosted products that support remote MCP.

Exit: a new AI client with no conversation history can recover project context through Mind Place alone.

Security exit condition: a revoked client loses access on its next request, the public tunnel cannot reach admin/write surfaces it was not granted, and no hosted read client can escalate itself to maintenance authority.

### Phase 4 — Personal documents and approved folders

Recommended agent invocations: **2** (`4A`, `4B`).

**4A — filesystem source engine**

- approved-folder UI and canonical security boundary;
- virtual paths;
- handle-level TOCTOU protection;
- scanner/watcher/reconciliation;
- include/exclude/retention policies;
- immediate authorization filtering on search and fetch.

**4B — document history + extraction**

- content-addressed versions/dedup;
- metadata fast-path for unchanged files;
- text/source/Markdown/PDF/OOXML extraction with resource limits;
- structure-aware chunks;
- incremental FTS updates;
- one-off file import.

Exit: chat and personal documents are one searchable evidence universe.

### Phase 5 — Memory truth engine

Recommended agent invocations: **2** (`5A`, `5B`).

**5A — claims and conflict semantics**

- append-oriented memory claims;
- supersession/dispute/retraction/uncertainty/history;
- evidence references;
- claim relations (`supports`, `contradicts`, `supersedes`, `refines`);
- `as_of` state reconstruction;
- ranking that distinguishes authority, scope, recency and inference confidence.

**5B — memory workflow and UI**

- search-before-write;
- user/model memory create/update flows;
- memory diff/history;
- conflict viewer;
- pin/manual correction;
- "why is this remembered?" provenance inspection;
- safe deletion/forget cascade.

Exit: wrong memories can exist without silently poisoning the AI, and better models can re-evaluate old evidence.

### Phase 6 — Knowledge graph and Graphify

Recommended agent invocations: **2** (`6A`, `6B`).

**6A — deterministic graph**

- graph adjacency tables in `index.sqlite3`;
- projects/topics/files/conversations/memories/entities;
- deterministic contains/references/version/import/symbol relationships;
- graph-neighborhood retrieval;
- community aggregation for large graphs.

**6B — agent-assisted Graphify**

- graph generation manifests;
- incremental invalidation by file/content/structure hash;
- staging + atomic activation;
- short-lived scoped maintenance jobs;
- ready-to-paste `Codebase`, `Documents`, `Mixed` graphification prompts;
- confidence/provenance on inferred edges.

Exit: the graph improves navigation and recall but remains a derived map, never canonical truth.

### Phase 7 — Multi-provider capture

Recommended agent invocations: **2** (`7A`, `7B`).

**7A — provider-neutral capture framework + Claude**

- formal provider adapter contract;
- move ChatGPT capture behind the same normalized interface;
- Claude conversation adapter/import path;
- provider/model/branch/regeneration metadata normalization.

**7B — Gemini + generic imports**

- Gemini adapter/import path;
- portable JSON/HTML/Markdown provider import framework;
- provider capability matrix;
- regression fixtures so a provider UI change cannot corrupt existing archives.

Exit: "one brain" is no longer synonymous with ChatGPT capture.

### Phase 8 — Knowledge OS UI

Recommended agent invocations: **2** (`8A`, `8B`).

**8A — daily library**

- unified Conversations / Files / Memories / Projects / People / Topics library;
- advanced search playground;
- context preview;
- source/version inspector;
- capture/index/connector health.

**8B — Hive / graph playground**

- semantic zoom;
- lazy graph expansion;
- timeline;
- direct relationship editing;
- memory conflict UI;
- retrieval debugger showing queries, graph hops, selected evidence and token cost.

Exit: ordinary maintenance and debugging no longer require raw database tools.

### Phase 9 — Privacy, portability and resilience

Recommended agent invocations: **2** (`9A`, `9B`).

**9A — privacy/security**

- encrypted-at-rest strategy and keychain UX after benchmarking;
- private scopes / source redaction;
- advanced permission UX and policy profiles on top of the minimum controls already required by Phases 3-4;
- secure destructive delete/forget semantics for retained canonical data;
- bounded parser/maintenance-job isolation;
- repeat/fuzz the tunnel and local API security audit against the mature feature set.

**9B — backup and portability**

- atomic backup/restore;
- portable archive export/import;
- integrity verification;
- rebuild derived indexes after restore;
- migration/version compatibility tests.

Exit: Mind Place can safely become long-lived personal infrastructure rather than a single-machine experiment.

### Phase 10 — Intelligence bake-off, scale qualification and v1.0

Recommended agent invocations: **2 mandatory**, **+1 optional**.

**10A — retrieval bake-off**

- build a representative personal benchmark suite;
- measure lexical, structured, graph and hybrid recall/latency;
- measure context-token cost and answer support quality;
- identify actual misses instead of assuming embeddings solve them.

**10B — v1.0 hardening**

- 1M+ chunk / 100K-file scale tests;
- startup/reconciliation benchmarks;
- corruption/rebuild tests;
- cross-provider continuity acceptance scenarios;
- installer/update/migration qualification;
- performance budgets and regression gates.

**10C — OPTIONAL semantic layer, only if 10A proves it useful**

- benchmark `sqlite-vec` or another local derived vector implementation;
- simple hybrid fusion with lexical/graph scores;
- Tantivy bake-off if FTS5 itself is the measured bottleneck;
- preserve a zero-embedding fully functional mode.

Exit: **Mind Place v1.0 = model-independent one-brain continuity system**, with evidence-aware memory instead of a giant unquestioned prompt.

### Total expected implementation invocations from the current state

Mandatory guidance: **20 substantial agent invocations** across Phases 1-10.

Optional semantic/vector work: **+1** only if benchmarks justify it.

This is intentionally much coarser than the earlier Luna-oriented plans. If a strong coding agent can safely complete `4A` and `4B` together with good tests and review, there is no product reason to force two chats.

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

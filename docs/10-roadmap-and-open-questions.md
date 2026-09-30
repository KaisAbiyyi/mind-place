# Roadmap and Open Questions

## 1. Implementation philosophy

The project can become enormous if it begins with every advanced idea at once.

The recommended sequence is to first establish the irreversible foundations:

- canonical schema;
- raw capture;
- durable local storage;
- reliable search;
- retrieval API.

Graph intelligence, semantic vectors, autonomous maintenance, and polished visualization should come after the archive and fetch path are trustworthy.

## 2. Proposed phases

### Phase 0 - Canonical storage foundation

Goal: establish a stable local representation.

Build:

- repository structure;
- TypeScript core;
- SQLite database;
- migrations;
- conversation schema;
- message schema;
- attachment metadata;
- content hashing;
- local object directory;
- basic CRUD.

Exit criteria:

- normalized conversation can be inserted and read back;
- duplicate insert is idempotent;
- data is provider-neutral.

### Phase 1 - Browser capture

Goal: automatically preserve ordinary conversations.

Start with one provider, likely ChatGPT, then generalize.

Build:

- extension shell;
- localhost connection;
- provider adapter interface;
- user-message capture;
- assistant-completion capture;
- temporary-chat detection;
- pause controls;
- retry and idempotency.

Exit criteria:

- a full real conversation appears locally without manual action;
- page refresh does not duplicate records;
- temporary chat is not captured by default.

### Phase 2 - Search and progressive fetch

Goal: make raw history genuinely useful.

Build:

- FTS5;
- search endpoint;
- conversation and message filters;
- snippets;
- exact message-window fetch;
- section and full-conversation fetch;
- token estimation.

Exit criteria:

- model or user can locate obscure historical details without loading full chats.

### Phase 3 - MCP read integration

Goal: let any compatible AI model research Mind Place.

Build:

- MCP server;
- memory search;
- progressive source fetch;
- structured compact results;
- batching.

Exit criteria:

- a fresh model can answer a project-history question using only MCP retrieval.

### Phase 4 - File ingestion

Goal: make conversations and documents part of one context universe.

Build:

- drag and drop ingestion;
- content-addressed object store;
- file metadata;
- text extraction adapters;
- FTS indexing;
- file excerpts;
- conversation-attachment linking.

Exit criteria:

- a model can retrieve both a chat excerpt and relevant file evidence in one research flow.

### Phase 5 - Curated memory

Goal: support compact durable memories while retaining raw evidence.

Build:

- memory CRUD;
- search-before-write workflow;
- source and provenance links;
- manual memory creation;
- edit and delete;
- optional supersession.

Exit criteria:

- a compatible model can autonomously create or update a preference memory;
- user can inspect the source.

### Phase 6 - Context builder and research retrieval

Goal: make retrieval efficient at scale.

Build:

- multiple-query search;
- token budgets;
- grouping;
- source deduplication;
- context packs;
- optional graph-assisted expansion.

Exit criteria:

- complex cross-conversation questions can be researched iteratively without huge context dumps.

### Phase 7 - Semantic graph

Goal: create the hive-mind relationship layer.

Build:

- graph node and edge tables;
- manual relationship editing;
- project and topic entities;
- provenance for inferred edges;
- graph neighborhood API.

Exit criteria:

- user can navigate from high-level project to memory to source conversation.

### Phase 8 - Interactive GUI

Goal: turn the engine into a useful daily product.

Build:

- library;
- search;
- memory manager;
- graph visualization;
- semantic zoom;
- context preview;
- source inspector;
- capture health;
- graph editing.

Exit criteria:

- normal maintenance no longer requires database or developer tooling.

### Phase 9 - Optional maintenance workers

Goal: exploit frontier models without making them a core dependency.

Build:

- worker API;
- structured derived operations;
- topic clustering;
- relationship suggestions;
- project timelines;
- rebuild tooling.

Exit criteria:

- derived graph and index can be regenerated with a different model without modifying raw source.

### Phase 10 - Semantic or vector retrieval only if needed

Goal: improve recall for conceptual queries that lexical and graph search miss.

Build only after measuring gaps.

Potential technology:

- sqlite-vec or equivalent;
- embedding cache;
- hybrid ranker.

Do not let vectors become the only retrieval path.

## 3. Suggested technical direction

Current preferred direction:

~~~text
Language:       TypeScript
Core runtime:   Node.js or Bun
Database:       SQLite
ORM or query:   Drizzle or direct typed SQL
Search:         SQLite FTS5
Objects:        content-addressed local directory
Compression:    Zstandard later
Desktop:        Tauri later
UI:             React/Vite candidate
Extension:      Chromium Manifest V3 first
Model protocol: MCP
Local protocol: HTTP or WebSocket as needed
Vectors:        optional later
~~~

These are design preferences, not locked decisions.

## 4. Open question: global pool versus namespaces

Should memory be one global pool, or should every project or conversation have a namespace?

Likely answer is a hybrid:

- one physical store;
- flexible scopes, tags, and projects;
- cross-scope search allowed;
- model can constrain retrieval.

Avoid hard isolation that prevents useful cross-project recall unless privacy requires it.

## 5. Open question: how much schema is enough?

Too little structure makes retrieval weak.

Too much structure creates brittle ontology maintenance.

Initial recommendation:

- strongly structured identity and provenance;
- lightly structured memory semantics;
- extensible metadata JSON for provider-specific fields.

## 6. Open question: capture implementation durability

Provider web UIs change frequently.

Need to investigate:

- stable page state;
- network or API observation;
- DOM semantics;
- browser permission constraints;
- extension-store policy;
- how to test adapters against UI changes.

Provider adapters should be independently testable.

## 7. Open question: source deletions

Need an explicit product rule for:

- provider conversation deleted;
- original file removed;
- user asks to forget;
- derived memory references deleted source.

The archive should not silently retain data against clear user intent, but local archival independence must also be understandable.

## 8. Open question: attachment policy

Text is cheap; binary data may not be.

Possible settings:

- save all attachments;
- save only files below a threshold;
- save metadata or reference only;
- save selected MIME types;
- user-defined per-provider policy.

Content hashing should prevent duplicates.

## 9. Open question: graph inference authority

If an AI worker says A is related to B, how should that differ from a manually created edge?

Recommendation:

- origin metadata;
- user or manual edges are authoritative;
- inferred edges are removable and rebuildable;
- confidence is a hint, not truth.

## 10. Open question: context-pack caching

Many prompts within one project may need the same stable context.

Potential optimization:

- compute stable project prefix;
- cache or reuse it with provider prompt caching;
- fetch only new deltas.

This could materially reduce model cost.

## 11. Open question: curated memory write bridge

When a hosted chat interface cannot directly write through custom MCP:

Options include:

- extension UI action;
- extension-mediated local tool bridge;
- manual promotion;
- use a different agent or client for curation;
- wait for provider support.

This should not block raw capture.

## 12. Open question: local encryption

If Mind Place becomes the user's long-term AI memory, it will contain unusually sensitive data.

Need to evaluate:

- SQLCipher or filesystem-level encryption;
- object-store encryption;
- passphrase and keychain UX;
- search performance;
- backup and restore implications.

## 13. Explicit non-goals for the first usable version

Do not block V1 on:

- autonomous AI curation;
- perfect embeddings;
- graph auto-layout at million-node scale;
- multi-device sync;
- cloud hosting;
- mobile apps;
- collaboration;
- complex ontology;
- fully automatic project classification;
- every AI provider.

## 14. First meaningful milestone

The smallest version that proves the thesis is:

> A browser extension captures every non-temporary ChatGPT conversation into a provider-neutral local SQLite database, and an MCP server lets another AI client search snippets and progressively fetch exact historical context.

That alone proves:

- continuity is local;
- model and provider can be swapped;
- full history can remain cheap;
- retrieval can be selective.

Everything else compounds the value.

## 15. Long-term product vision

A mature Mind Place should make switching models feel trivial.

The experience should be:

~~~text
new frontier model appears
       |
connect Mind Place
       |
model researches relevant personal and project context
       |
continue working
~~~

No manual migration of memories.

No re-explaining every project.

No dependence on one vendor's retention system.

The model race can continue indefinitely while the user's personal continuity remains stable.

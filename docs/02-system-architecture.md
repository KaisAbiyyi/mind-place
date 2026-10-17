# System Architecture

## 1. Architecture goal

Mind Place must support two goals that seem contradictory:

1. Preserve extremely high-fidelity history.
2. Expose only a small, relevant amount of that history to an AI model at any given time.

This is achieved by separating storage into layers.

## 2. Layered model

~~~text
+---------------------------------------------------------+
| External clients                                        |
| ChatGPT | Claude | Gemini | Codex | Claude Code | etc. |
+-----------------------------+---------------------------+
                              |
                       MCP / Local API
                              |
+-----------------------------v---------------------------+
| L4 - Context and derived views                          |
| Context packs, timelines, summaries, project views     |
+-----------------------------+---------------------------+
                              |
+-----------------------------v---------------------------+
| L3 - Semantic knowledge graph                          |
| Topics, projects, entities, relationships              |
+-----------------------------+---------------------------+
                              |
+-----------------------------v---------------------------+
| L2 - Curated memory                                    |
| Model/user-selected durable memories                   |
+-----------------------------+---------------------------+
                              |
+-----------------------------v---------------------------+
| L1 - Search/index layer                                |
| FTS, chunks, metadata, optional vectors                |
+-----------------------------+---------------------------+
                              |
+-----------------------------v---------------------------+
| L0 - Raw source archive                                |
| Conversations, messages, files, provider metadata      |
+-----------------------------+---------------------------+
                              |
                        Local storage
~~~

## 3. L0: raw source archive

L0 preserves the closest practical representation of original source information.

Examples:

- normalized complete conversation messages;
- message IDs;
- timestamps;
- provider and model information;
- branch relationships;
- regenerated replies;
- source links;
- attachment hashes;
- imported files;
- raw exports when appropriate.

Properties:

- durable;
- provenance-rich;
- append-oriented;
- never replaced by summaries;
- independently exportable;
- readable without any AI component.

L0 is the reconstruction boundary. Everything else should ideally be derivable from L0 plus explicit user- or model-authored curated memories.

## 4. L1: search and index layer

L1 exists because reading raw archives directly is inefficient.

Possible components:

- SQLite FTS5 lexical index;
- message and chunk index;
- normalized titles;
- source metadata;
- timestamp indexes;
- provider indexes;
- project/topic tags;
- optional embedding vectors later.

L1 may be rebuilt without information loss.

The first implementation should prefer FTS5 and structured filtering because personal corpora often contain highly distinctive terms such as project names, acronyms, branch names, issue IDs, people, and model names.

Semantic vectors should be added only after measured retrieval failures justify them.

## 5. L2: curated memory

Curated memory contains compact statements deliberately promoted into durable personal context.

Examples:

- "User prefers implementation work to follow a completed plan."
- "A project milestone was frozen after a specific release."
- "Use feature-based architecture and SOLID principles in this codebase."

A curated memory is not a replacement for its evidence.

Each memory should be capable of referencing:

- one or more source messages;
- a conversation;
- a file section;
- another memory;
- manual user input.

The active AI model may decide what should become a curated memory. The application should not require its own embedded model for that judgment.

## 6. L3: knowledge graph

The graph connects concepts rather than exposing every raw message as a top-level visual node.

Example:

~~~text
User
 |
 +-- Project A
 |     |
 |     +-- Product analytics
 |     +-- Daily challenge
 |     +-- Model workflow
 |
 +-- Project B
       |
       +-- File format
       +-- Benchmark
       +-- Editor
~~~

Graph nodes may represent:

- projects;
- people;
- topics;
- decisions;
- concepts;
- files;
- selected memories;
- conversations.

Raw messages remain accessible through provenance links without overwhelming the visible graph.

## 7. L4: derived views and context packs

L4 contains disposable representations optimized for consumption.

Examples:

- current project context;
- model workflow context;
- all decisions related to a storage format;
- project timeline;
- conversation digest;
- context pack capped at a token budget.

These should be treated as cache-like objects, not canonical truth.

## 8. Capture path

~~~text
Provider UI
   |
browser extension
   |
provider adapter
   |
normalize
   |
deduplicate/idempotency check
   |
L0 raw archive
   |
L1 indexing
~~~

The extension should commit completed turns rather than every streamed token.

## 9. Curated-memory path

~~~text
User conversation
       |
active model decides a memory matters
       |
memory search for existing related state
       |
create / update / supersede memory
       |
L2 curated memory
       |
source references point to L0
~~~

This keeps intelligence in the model currently being used.

## 10. Retrieval path

~~~text
User asks something
      |
model forms one or more retrieval queries
      |
search metadata and snippets
      |
model inspects results
      |
fetch exact source windows / traverse graph
      |
repeat if evidence is incomplete
      |
construct compact context
      |
answer
~~~

The preferred mental model is research over personal history, not a one-shot nearest-neighbor lookup.

## 11. Optional maintenance path

~~~text
L0 / L2
 |
optional worker
(Codex / Claude Code / Antigravity / other)
 |
derive topics, relationships, summaries, indexes
 |
L1 / L3 / L4
~~~

Workers must not mutate L0 historical content.

## 12. Core process boundaries

A practical future layout could be:

~~~text
mind-place-core
  database
  object store
  indexing
  graph
  context builder

mind-place-mcp
  MCP tool server

mind-place-api
  localhost HTTP / WebSocket API

mind-place-extension
  provider capture adapters

mind-place-ui
  local web or Tauri desktop application

mind-place-workers
  optional enrichment jobs
~~~

They may initially live in one monorepo or process, but the responsibilities should remain conceptually separate.

## 13. Trust boundaries

### Trusted canonical components

- local database;
- local object store;
- deterministic import and capture normalization.

### Semi-trusted derived components

- chunkers;
- summarizers;
- graph inference;
- embeddings;
- model-generated tags.

Their output must be labeled as derived.

### External replaceable components

- hosted AI models;
- provider web UIs;
- changing DOM structures;
- external APIs.

The architecture should assume these will change.

## 14. Failure strategy

Important failures should degrade gracefully.

If an embedding model disappears, lexical search still works.

If a maintenance worker has not run, raw search still works.

If a provider DOM changes, existing archive still works and only the capture adapter needs repair.

If graph data is corrupted, rebuild graph from sources and curated data.

If a new frontier model appears, connect it to the same interface.

This resilience is a major advantage of the layered design.

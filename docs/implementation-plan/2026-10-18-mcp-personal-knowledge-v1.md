# MCP personal knowledge and allowed-folder indexing — implementation contract v1

> Strategy note: this document is a detailed design reference. The current project-level roadmap is [`../BIG-IMPLEMENTATION-PLAN.md`](../BIG-IMPLEMENTATION-PLAN.md). Where the two conflict, the BIG plan wins; implementation no longer needs to preserve this document's original fine-grained task splitting.

This plan turns Mind Place from a ChatGPT archive reader into a local personal knowledge substrate that any compatible AI can research. It extends the current Rust archive without weakening its existing guarantees: captured conversation revisions remain immutable raw evidence, derived indexes remain rebuildable, automatic ChatGPT saving keeps its current non-scrolling boundary, and hosted models receive only the evidence the user has explicitly made available.

The central design decision is to keep intelligence outside the core. Mind Place stores, indexes, filters, ranks, packages, and proves provenance deterministically. ChatGPT, Codex, Claude Code, Antigravity, or another agent can perform higher-level semantic work when the user asks for it. The baseline must not require a bundled LLM, paid embedding API, graph database, vector database, or cloud storage.

## 1. Product outcome

Mind Place becomes one local evidence universe containing:

- immutable ChatGPT capture revisions;
- personal files from directories the user explicitly approves;
- version history for indexed documents where preservation is enabled;
- curated memories added later;
- derived search chunks, summaries, and knowledge-graph data;
- provenance linking every derived item back to exact chat messages or exact document versions.

AI clients connect through a small MCP surface with rich retrieval parameters. Local clients can connect directly. Hosted ChatGPT connects through an HTTPS tunnel that publishes only the MCP reader endpoint, never the desktop administration API or raw filesystem.

The expected interaction is:

```text
user question in ChatGPT / Codex / another agent
        |
        v
Mind Place MCP search (cheap metadata + snippets)
        |
        v
agent selects useful source ids
        |
        v
precise fetch / graph expansion / bounded context pack
        |
        v
agent answers with provenance
```

For folder graphification:

```text
user approves folder in Mind Place
        |
        v
Mind Place incrementally indexes deterministic structure
        |
        v
UI generates a ready-to-paste graphification prompt
        |
        v
ChatGPT / Codex / another agent researches that folder through MCP
        |
        v
agent stages derived semantic nodes + edges
        |
        v
Mind Place validates coverage/provenance and atomically activates the generation
```

## 2. Preserve the current archive; add a derived catalog beside it

Do not rewrite the current `archive-v1/conversations/.../captures/*.jsonl` model as part of this work. It is already the proven L0 boundary and changing it would combine a storage migration with retrieval, MCP, and file ingestion.

Add small authoritative state and a separate rebuildable index under the Mind Place data root, for example:

```text
archive-v1/
  conversations/               current immutable chat evidence
  objects/                     content-addressed preserved document versions
  exports/
  legacy/
state.sqlite3                  approved roots, memories, permissions, manual overrides
index.sqlite3                  rebuildable search/index metadata + derived graph
```

`state.sqlite3` contains user-authored or security-relevant state that cannot simply be regenerated. `index.sqlite3` contains search rows, chunks, automatic graph generations, extracted metadata, ranking caches and other derived views. Rebuilding the index must never delete or rewrite authoritative state.

The first database should use SQLite WAL mode, normal indexed tables, and FTS5. No ORM is required in the Rust core unless it materially improves migrations; direct typed SQL is acceptable and keeps dependencies small.

## 3. Unified source identity

Every searchable item gets a stable Mind Place identity independent of a provider URL or native filesystem path.

Suggested prefixes:

```text
conv_    conversation
snap_    conversation capture revision
msg_     message within a revision
root_    approved folder source
file_    logical file identity within a root
ver_     immutable document/file version
chunk_   derived searchable chunk
mem_     curated memory
node_    graph node
edge_    graph edge
gen_     graph/derivation generation
```

Stable IDs exposed to models must never contain a username, drive letter, home path, or tunnel secret.

Any stable ID referenced by authoritative `state.sqlite3` records must remain valid after `index.sqlite3` is deleted and rebuilt. Such IDs must either be deterministically derived from immutable/source identity or have their identity mapping persisted in authoritative state. The derived index may cache these IDs but must not become their only source of identity.

The local catalog may map a logical path to a native path, but MCP should expose a virtual source path such as `/research/papers/model.pdf` or `/sibambo/src/engine.rs` rather than `C:\Users\...` by default.

## 4. Approved directory model

Mind Place should adapt the strongest parts of Chat On Steroids' approved-root model and narrow them to read/index use.

Important inherited rules:

1. Folder approval is the permission boundary. A project/tag association grants no additional filesystem permission.
2. The folder selected by the user is canonicalized before it is stored.
3. Entire filesystem roots/drives are rejected by default.
4. Overlapping approved roots are rejected to keep identity and revocation unambiguous.
5. Every access resolves through one path authority. No caller falls back to a raw path after a failed resolution.
6. Traversal segments, unsafe Windows device/ADS names, unexpected UNC paths, and paths outside the root are refused.
7. Symlink, junction, and reparse-point escapes must be detected by canonical destination, not only lexical prefix checks.
8. The approved root itself is revalidated; replacing it with a link must not silently move the permission boundary.
9. Permission is checked again when a delayed/indexing job actually opens a file.
10. File ingestion should hold/revalidate safe directory or file handles around actual reads so a symlink/junction/reparse swap cannot win a time-of-check/time-of-use race after path validation.

Mind Place can be stricter than CoS because it does not need shell execution or model-controlled writes for the document reader. The default document source is read-only.

### 4.1 Root configuration

Each `root_...` record should support:

```text
id
alias                         user-facing virtual root name
canonical_path                local-only native path
enabled
include_globs[]
exclude_globs[]
follow_links = false
include_hidden = false
max_file_bytes
copy_policy                   auto | always | never
history_policy                current | versions
extract_policy                text | supported-documents | metadata-only
watch_mode                    automatic | manual
created_at
last_scan_at
catalog_revision
```

Defaults should exclude noisy/generated trees such as `.git`, `node_modules`, `target`, `dist`, `build`, caches, package stores, editor metadata, and virtual environments while still allowing an advanced user to override exclusions.

### 4.2 Revocation

Removing or disabling a root must immediately block new reads, fetches, watches, indexing, search snippets, graph results, context packs, cached results and maintenance access from that native directory. Authorization is applied before result ranking/snippet emission, not only before an exact follow-up fetch.

The UI should make retained local evidence explicit with two separate actions:

- **Remove folder access** — revoke live filesystem access while preserving already archived versions and derived evidence.
- **Remove access and local copies** — revoke access and delete stored versions, extracted text, index rows, and derived graph records after confirmation.

This distinction prevents both surprise data loss and surprise retention.

## 5. Document history without wasteful duplication

Treat a logical file and its versions separately.

```text
file
  stable root-relative identity
  current version pointer
  virtual path

file_version
  content hash
  observed size / mtime / file identity
  extracted text hash
  preserved object reference if copied
  parser/version metadata
  first_seen / last_seen
```

Use content-addressed storage so identical bytes across folders or versions occupy one object.

The cost-efficient default is `copy_policy=auto`:

- preserve ordinary text, source code, Markdown, office/PDF documents, and other supported document files below a configurable threshold;
- deduplicate by SHA-256;
- keep very large media/reference-heavy files as metadata/reference unless the user opts into copying them;
- extract searchable text once per content hash, not once per path;
- retain old copied versions when `history_policy=versions` so documents can be researched historically like chat revisions.

For very large corpora, `copy_policy=never` still permits indexing live files while making it clear that old bytes cannot be reconstructed after the source changes.

## 6. Incremental scanner and watcher

Do not recursively reread every file on every launch.

Use a two-stage scanner:

1. **Metadata pass** — enumerate path, type, size, modification time, and a platform file identity where available.
2. **Content pass** — hash/extract only new or changed candidates.

Fast path for unchanged files:

```text
(root id, relative path, file identity, size, mtime)
       -> unchanged metadata signature
       -> skip hash, parser, chunker, and FTS work
```

If the signature changes, compute the real content hash before creating a new version. The hash remains the authority; timestamps are only an admission optimization.

Use bounded producer/consumer queues rather than launching one task per file. A practical initial range is 4-8 concurrent content readers with adaptive reduction for slow/network-like storage. Measure before increasing it.

### 6.1 Watch strategy

CoS intentionally watches only a bounded set of expanded UI directories. Mind Place needs durable indexing of complete approved roots, so use a different strategy:

- one platform-recursive watcher when the platform supports it reliably;
- otherwise a bounded watch set for recently active directories plus periodic reconciliation;
- debounce bursty changes before indexing;
- watcher events are hints, never truth;
- every event is re-resolved through the approved-root authority;
- periodic reconciliation repairs missed watcher events and renames.

The reconciliation scan should reuse the metadata fast path and therefore be cheap when nothing changed.

Keep a bounded live-search fallback for stale-index diagnosis and first-run gaps. The index is the fast path for repeated retrieval; a live scanner can verify a narrow approved subtree when the caller explicitly requests freshness or when the catalog reports that watcher/reconciliation work is incomplete. It must use the same permission authority and explicit time, file-count, file-size, and result ceilings as indexed retrieval.

## 7. Extraction and chunking

Start with deterministic local extractors.

Priority order:

1. UTF-8/UTF-16 plain text, Markdown, source code, JSON/YAML/TOML/CSV and similar text.
2. PDF text extraction with page/object/time/memory ceilings.
3. Office Open XML documents (`docx`, `pptx`, `xlsx`) using local parsers with ZIP entry-count, nesting, expanded-size, CPU/time and memory ceilings.
4. Additional formats only when real user data justifies them.

Do not require OCR, cloud document services, or an LLM for baseline ingestion.

Chunking should be structure-aware where cheap:

- Markdown headings;
- source-code symbols when a deterministic parser is available;
- document pages/sections;
- paragraph/line windows as fallback.

Every chunk carries exact provenance:

```text
version id
virtual path
byte/line/page range where available
heading/symbol path where available
extractor id + version
content hash
```

Chunk IDs should be stable for an unchanged file version so reindexing does not invalidate graph provenance.

## 8. Universal retrieval catalog

Build one FTS5 index across chat and document text, with source-specific metadata outside FTS.

The initial ranking path is deliberately inexpensive:

```text
structured filters
       |
       v
FTS5 / BM25 lexical candidates
       |
       v
deterministic score adjustments
       |
       +-- exact phrase/entity
       +-- recency when requested
       +-- manual pin/authority
       +-- graph distance when requested
       |
       v
bounded snippets
```

Do not add embeddings until measured failures show lexical + metadata + graph recall is insufficient. When vectors are eventually added, they are another signal and cache, not the only search path.

### 8.1 Rich fetch configuration

Expose many retrieval knobs through a few primitives. The server remains deterministic; the calling model chooses the research strategy.

Conceptual `memory_search` request:

```json
{
  "queries": [
    { "q": "BOMC writeable hierarchy", "weight": 1.0, "exact": false },
    { "q": "arbitrary hierarchy round trip", "weight": 0.7, "exact": false }
  ],
  "scope": {
    "source_types": ["conversation", "document", "memory"],
    "root_ids": ["root_sibambo"],
    "conversation_ids": [],
    "projects": ["sibambo"],
    "providers": ["chatgpt"],
    "models": [],
    "roles": ["user", "assistant"],
    "mime_types": [],
    "extensions": ["md", "rs", "ts"],
    "path_globs": ["**/*"],
    "tags": []
  },
  "time": {
    "field": "source_time",
    "from": null,
    "to": null,
    "as_of": null
  },
  "search": {
    "mode": "lexical",
    "phrase_boost": 1.5,
    "prefix": true,
    "case_sensitive": false
  },
  "rank": {
    "bm25": 1.0,
    "recency": 0.0,
    "graph": 0.3,
    "authority": 0.2,
    "manual_pin": 0.5,
    "semantic": 0.0
  },
  "graph": {
    "expand": true,
    "depth": 1,
    "relations": [],
    "origins": ["manual", "deterministic", "model"],
    "min_confidence": 0.0,
    "max_nodes": 40
  },
  "versions": {
    "documents": "current",
    "conversations": "selected-or-latest"
  },
  "result": {
    "granularity": "snippet",
    "group_by": "source",
    "diversity": "balanced",
    "max_results": 30,
    "max_per_source": 5,
    "include_provenance": true,
    "include_scores": false
  },
  "budget": {
    "max_tokens": 6000,
    "max_bytes": 48000,
    "max_latency_ms": 250
  }
}
```

The production schema should provide presets so ordinary agents do not need to fill every field:

- `fast` — FTS + metadata only, small snippets;
- `balanced` — FTS + structured ranking + shallow graph;
- `deep` — broader result budget and graph traversal;
- `exact` — lexical/phrase focused, no semantic expansion;
- `historical` — include versions and chronological ordering.

Every field remains overrideable. This is the intended meaning of "few tools, many parameters."

## 9. Progressive MCP surface

Keep the read surface small:

### `memory_search`

Multi-query search and browse across conversations, files, curated memory, and optional graph expansion. Empty query plus structured scope can act as a bounded browse operation, so a separate file-tree tool is not mandatory.

### `memory_fetch`

Fetch exact evidence after search. Supported targets should include:

- message;
- message window;
- conversation section;
- conversation revision;
- document chunk;
- line/page/section range;
- complete small document;
- exact document version;
- memory plus provenance.

Full conversations and full large files must be deliberate requests with explicit budgets.

### `graph_neighbors`

Traverse semantic relationships by node/source id with depth, relation, origin, confidence, and node-count limits.

### `context_build`

Package already selected evidence into a deduplicated, token-bounded context pack. This tool performs no hidden semantic judgment; it trims overlap, orders content, includes provenance, and reports cost.

The MCP response should always indicate truncation, stop reason, token estimate, catalog revision, and stable follow-up IDs.

## 10. Hosted ChatGPT tunnel

Run the MCP HTTP endpoint on `127.0.0.1` with an ephemeral local port. Never bind the reader to `0.0.0.0` by default.

Use capability-scoped endpoints with independent high-entropy path tokens, borrowing the proven CoS pattern:

```text
http://127.0.0.1:<port>/mcp/read/<secret>
http://127.0.0.1:<port>/mcp/maintain/<different-secret>
```

The **read** surface is what hosted ChatGPT normally receives. Maintenance/write capability is created only for an explicit graphification/curation job: use a short-lived credential bound to its `generation_id`, authorized root/source revision, allowed operations and batch ceilings, then revoke it on finish/cancel. Write batches must be idempotent or sequence-checked so a retry cannot duplicate a generation mutation.

For each request:

- re-read current permissions/root state;
- enforce a body-size ceiling;
- return JSON protocol errors rather than leaking local paths;
- keep tunnel secrets out of logs;
- support clean draining on shutdown so a completed operation is not retried ambiguously;
- record local latency/result-count/token metrics without logging retrieved private content by default.

The tunnel manager is an adapter around this loopback endpoint. Support, in order:

1. the same secure tunnel path used successfully by CoS where reusable without coupling the products;
2. Cloudflare quick tunnel as a zero-cost development/fallback option;
3. user-supplied HTTPS endpoint later.

Tunnel implementation must not become the business logic. Local Codex/Claude Code/Antigravity should be able to use a direct MCP transport without a public tunnel.

## 11. Knowledge graph: deterministic first, model semantic second

Do not pay an AI model to rediscover relationships that a parser can prove.

Deterministic graph examples:

- folder contains file;
- Markdown document contains heading;
- source file defines/imports symbol/module when a parser supports it;
- conversation contains message;
- message references attachment;
- document version supersedes prior version;
- link points to another local document;
- exact stable identifiers/tags connect known project records.

Use an external AI agent only for semantic work such as:

- "this file implements the archive permission boundary";
- topic/project/entity clustering;
- decision/rationale extraction;
- conceptual relationships across documents and conversations;
- naming higher-level modules where deterministic structure is insufficient.

This sharply reduces token cost and improves reproducibility.

## 12. Graph generation transaction

Never let a half-finished agent job become the active graph.

Add derived generation records:

```text
graph_generation
  id
  root_id / source scope
  source_catalog_revision
  generator client/model
  prompt/profile version
  status = staging | complete | rejected
  coverage metadata
  created_at

graph_manifest
  generation_id
  file/version id
  observed metadata signature
  content hash
  deterministic/AST hash where applicable
  semantic hash
  parser/index schema versions

graph_node / graph_edge
  generation_id
  stable canonical key
  origin = deterministic | model | manual | imported
  exact evidence refs[]
  confidence where applicable
```

The maintenance MCP surface accepts bounded batches into `staging`. At finish, Mind Place validates:

- every source reference still exists and belongs to the authorized scope;
- every model edge has evidence or is explicitly labeled inference;
- node/edge counts remain within configured limits;
- the source catalog revision did not change beyond the job's declared tolerance.

Then one SQLite transaction activates the generation. Interrupted jobs remain inactive and can be discarded safely.

Manual graph edits live outside disposable generations and take precedence over inferred edges.

For large graphs, add a derived community layer and load community summaries before individual nodes. The CoS Graphify work already demonstrates that useful code graphs can exceed 5,000 nodes, so Mind Place should lazy-expand communities, files, and entities rather than hydrate an entire raw graph into the UI or one MCP response. Community membership is derived and rebuildable, while source locations, confidence, and evidence remain attached to the underlying nodes and edges.

## 13. Ready-to-paste Graphify prompt in the UI

The approved-folder page should provide **Generate AI indexing prompt** with profile options:

- `Codebase`;
- `Documents`;
- `Mixed`.

The generated prompt contains only the virtual root alias/id, active include/exclude policy, desired graph profile, and MCP workflow. It should not expose the native directory path or connector secret.

Template shape:

```text
Build or refresh the Mind Place knowledge graph for root <root_id> (<alias>).

Use Mind Place MCP as the source of truth. Work in bounded batches: browse/search metadata first,
then fetch only the source windows needed to understand structure and semantics. Do not request the
entire folder at once. Preserve exact source ids and ranges as evidence for every semantic relation.

Profile: <Codebase|Documents|Mixed>
Source catalog revision: <revision>
Scope: <include/exclude summary>

First map deterministic/high-confidence structure already exposed by Mind Place, then add only the
semantic entities and relationships that materially improve future navigation and retrieval. Reuse
existing canonical nodes where possible. Do not edit source files. Write only derived graph batches
through the maintenance MCP surface. Keep inferred claims marked as inferred with confidence and
evidence. At the end, validate coverage, unresolved areas, duplicates, and stale references, then
finalize the staged generation atomically.

Optimize for future retrieval: a model should be able to start from a project/topic and reach the
smallest useful files, sections, decisions, conversations, and related entities without loading the
whole corpus.
```

The final UI prompt can include current MCP tool names and exact schema automatically so it stays compatible as the protocol evolves.

## 14. Performance strategy relative to CoS

CoS's live file search is intentionally index-free and bounded, with ripgrep when available and a streaming fallback. That is appropriate for an interactive coding workbench. Mind Place has a different workload: the same personal corpus is searched repeatedly for months or years. Therefore Mind Place should spend work once during ingestion and make retrieval much cheaper afterward.

Reuse from CoS:

- canonical approved-root enforcement;
- bounded scanning and explicit truncation reasons;
- streaming reads instead of unbounded whole-file buffers;
- sensible generated-directory exclusions;
- revalidation at actual use;
- small/bounded MCP responses;
- current permission checks per request.

Upgrade for Mind Place:

- persistent FTS5 rather than rescanning content for every query;
- metadata signatures to skip unchanged files;
- extraction cache keyed by content hash plus parser/index schema version;
- stable chunks and source IDs;
- incremental reconciliation after watcher hints;
- content-addressed deduplication across paths and versions;
- batch MCP queries in one round trip;
- context-pack caching keyed by source-version set + retrieval parameters;
- optional graph expansion after lexical filtering instead of before it.

For graph refreshes, use a generation manifest similar to the proven Graphify pattern: compare per-file metadata, content, structural, and semantic hashes and rerun only the stages invalidated by a change. A large graph refresh should become proportional to changed files instead of total corpus size.

Do not cache authorization. A cached search result must still pass current source/root permission before fetch.

## 15. Initial qualification targets

Treat these as engineering targets to benchmark, not marketing guarantees:

| Path | Initial target |
| --- | ---: |
| Indexed lexical search over ~1M chunks | p95 < 30 ms local query time |
| Exact indexed source fetch | p95 < 20 ms excluding large-file decode |
| No-op reconciliation of a previously indexed large tree | metadata-bound; no content reads for unchanged files |
| One changed text file to searchable | < 500 ms after debounced watcher event on local SSD |
| Default `memory_search` response | <= 2,000 estimated tokens unless caller raises budget |
| MCP default response bytes | <= 32 KiB |
| Core AI/embedding spend | $0 required |
| Interrupted graphification | zero partial active graph state |

Add synthetic fixtures at 10K, 100K, and 1M chunks and at least one 100K-file metadata tree before claiming scale.

## 16. Implementation phases

### Phase A — catalog sidecar and chat indexing

Build:

- SQLite migration framework;
- unified source ids;
- source/chunk/FTS schema;
- adapter that indexes current immutable chat revisions without modifying them;
- catalog revision counter;
- rebuild command.

Exit:

- obscure text can be found through FTS and fetched back from exact immutable chat evidence;
- deleting the derived catalog and rebuilding it yields equivalent searchable chat sources.

### Phase B — approved roots and document versions

Build:

- root registry and folder picker UI;
- canonical sandbox adapted from CoS principles;
- virtual paths;
- scanner, exclusions, metadata signatures, hashing and object deduplication;
- version records and revocation semantics.

Exit:

- the indexer cannot read outside an approved root through traversal/link/reparse tricks;
- unchanged files skip content work;
- modifying a preserved document creates a new version without losing the prior version.

### Phase C — local extractors and FTS

Build:

- text/source/Markdown extraction;
- PDF and Office extraction in separate adapters;
- structure-aware chunking;
- extraction cache by content hash;
- incremental FTS updates.

Exit:

- one search can return evidence from a chat and a personal document;
- every document result maps to exact version + range provenance.

### Phase D — rich retrieval engine

Build:

- parameter schema and presets;
- bulk query execution;
- ranking/filtering/grouping/diversity;
- token/byte/latency budgets;
- progressive fetch;
- deterministic context builder;
- retrieval traces.

Exit:

- a fresh agent can answer a cross-chat/cross-document project question without a full-corpus dump.

### Phase E — MCP reader + hosted tunnel

Build:

- Streamable HTTP MCP adapter;
- direct local adapter where useful;
- `memory_search`, `memory_fetch`, `graph_neighbors`, `context_build`;
- per-surface tokenized URLs;
- live permission checks;
- tunnel lifecycle/setup UI;
- ChatGPT connection test and end-to-end request proof.

Exit:

- hosted ChatGPT can research Mind Place through the tunnel while a revoked root becomes inaccessible on the next call;
- the public tunnel cannot reach desktop/admin APIs.

### Phase F — deterministic graph and Graphify workflow

Build:

- graph schema;
- deterministic structural edges;
- generation staging/activation;
- maintenance MCP capability;
- ready-to-paste prompt generator;
- Codebase/Documents/Mixed prompt profiles;
- coverage and provenance validator.

Exit:

- Codex or ChatGPT can graphify an approved folder in bounded batches and the resulting graph accelerates later navigation without becoming canonical truth.

### Phase G — optional semantic retrieval only after measurement

Measure lexical/graph misses first. If semantic recall is materially weak:

- add local or user-selected embeddings;
- store vectors as derived cache;
- combine with BM25 using a simple fusion strategy;
- preserve lexical-only operation when embeddings are disabled or unavailable.

Do not introduce a dedicated vector service unless SQLite-scale evidence proves it necessary.

## 17. Non-goals for this implementation sequence

- no background autonomous LLM continuously reading the user's disk;
- no cloud upload requirement;
- no Neo4j or separate graph service;
- no mandatory vector database;
- no required embedding bill;
- no model-generated rewrite of raw chat or document evidence;
- no shell/command capability through the Mind Place reader connector;
- no exposure of native folder paths to hosted models by default;
- no full-corpus prompt dump;
- no graph generation that can mutate source files;
- no replacement of the current proven archive before the new retrieval path is qualified.

## 18. Why this sequence is the cost/performance optimum

The expensive operations happen once when information changes, not every time an AI asks a question. SQLite/FTS gives micro-to-millisecond local candidate retrieval without an inference call. Content hashes deduplicate history. Deterministic parsers construct obvious structure without tokens. Graphification spends frontier-model tokens only when the user explicitly asks for semantic enrichment. Progressive MCP fetch keeps hosted context small. The tunnel is transport only, so local clients avoid its network cost entirely.

The result is one durable local memory substrate that can outlive any specific model, while allowing each new model to research exactly as deeply as the task requires.

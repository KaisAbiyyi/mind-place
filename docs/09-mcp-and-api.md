# MCP and Local API Design

## 1. Goals

Mind Place needs two integration surfaces:

1. MCP for AI models and agents.
2. Local API for the browser extension and GUI.

They should expose the same conceptual capabilities even if protocol details differ.

## 2. Design principle: few primitives, powerful configuration

Avoid creating a unique MCP tool for every retrieval combination.

Prefer a compact set of tools whose arguments express:

- source types;
- time filters;
- graph expansion;
- result granularity;
- provider filters;
- project and topic filters;
- token budgets;
- grouping;
- multiple queries.

This gives intelligent models freedom to research without making the tool surface impossible to learn.

## 3. Candidate MCP primitives

A future initial tool set could be:

~~~text
memory_search
memory_fetch

memory_get
memory_create
memory_update
memory_delete

graph_get
graph_search
graph_neighbors

source_get
context_build
~~~

The exact number should stay small.

Some tools can be merged after experimentation.

## 4. memory_search

Purpose:

- search raw history;
- search curated memories;
- search indexed files;
- optionally expand graph relationships.

Conceptual input:

~~~json
{
  "queries": ["..."],
  "sources": ["raw", "curated", "files"],
  "mode": "lexical",
  "providers": ["chatgpt"],
  "time": {
    "from": null,
    "to": null
  },
  "graph": {
    "expand": false,
    "depth": 0
  },
  "granularity": "snippet",
  "group_by": "conversation",
  "max_results": 20,
  "token_budget": 6000
}
~~~

Conceptual output:

- stable source IDs;
- compact snippets;
- timestamps;
- source type;
- provider;
- matching metadata;
- enough information for the model to choose a second fetch.

## 5. memory_fetch

Purpose:

Retrieve precise source content after search.

Possible modes:

- by message;
- around message;
- conversation section;
- entire conversation;
- file range;
- memory with provenance.

The tool should make narrow windows cheap and full-history fetches explicit.

## 6. Curated memory CRUD

### memory_create

The model explicitly creates a durable memory.

### memory_update

Update or supersede existing memory.

### memory_delete

Delete curated memory when explicitly appropriate.

### memory_get

Inspect current memory plus provenance and history.

The model should usually search before creating.

## 7. context_build

An optional higher-level deterministic helper can assemble bounded context after the model identifies relevant sources.

Input might specify:

- selected IDs;
- token budget;
- ordering policy;
- include provenance;
- deduplicate overlap.

The engine should not independently decide what the user means; it packages selected evidence efficiently.

## 8. Local HTTP API

The extension needs low-latency local endpoints.

Possible V1 surface:

~~~text
POST   /v1/conversations/upsert
POST   /v1/messages/upsert
POST   /v1/attachments
GET    /v1/health

GET    /v1/search
POST   /v1/context

POST   /v1/memories
PATCH  /v1/memories/:id
DELETE /v1/memories/:id
~~~

Actual routes should be chosen during implementation.

## 9. Authentication

Even on localhost, do not assume every local process should have full access to personal history.

Potential controls:

- random local API token;
- extension-specific token;
- MCP client token;
- OS keychain storage;
- origin checks for browser requests;
- bind only to localhost by default.

## 10. Tool permissions

It may be useful to distinguish:

- read clients;
- memory-write clients;
- admin clients;
- capture clients.

Example:

~~~text
browser extension: append raw data
chat model: read + curated memory CRUD
maintenance worker: read + derived-write
GUI: full local admin
~~~

This reduces blast radius.

## 11. Hosted chat limitations

Some hosted chat products may not expose writable custom MCP tools on all plans or surfaces.

The architecture should therefore not depend on model-triggered writes for raw archival.

The browser extension solves raw capture independently.

Curated memory writes may use:

- direct MCP where supported;
- a future extension-mediated bridge;
- manual GUI action;
- another connected agent.

The limitation is a client integration issue, not a core architecture blocker.

## 12. Bulk operations

To support research-style retrieval efficiently, MCP tools should allow batching where reasonable.

Example:

~~~json
{
  "queries": [
    {"q": "current project architecture"},
    {"q": "storage optimization"},
    {"q": "app separation"}
  ]
}
~~~

One round trip can return grouped results.

## 13. Stable identifiers

Tool results should use Mind Place IDs rather than provider URLs as primary identity.

Examples:

~~~text
conv_...
msg_...
mem_...
file_...
node_...
edge_...
~~~

Provider IDs remain metadata.

Stable local IDs prevent a provider migration from breaking internal references.

## 14. Tool response design

The default response should be compact.

Do not return huge full conversations, complete attachment text, or entire graph clusters unless requested.

Every result should support progressive follow-up.

## 15. Observability and debugging

The local service should log tool operations with privacy-conscious local logs:

- tool;
- client;
- arguments with sensitive content redacted where practical;
- result count;
- token estimate;
- latency;
- errors.

The GUI may expose these traces in a developer or retrieval inspector.

## 16. Future compatibility

MCP is currently a strong interface for model interoperability, but core business logic should not be coupled tightly to the MCP protocol.

Architecture:

~~~text
core service API
      |
   adapters
   /     \
 MCP    HTTP
~~~

If a future standard replaces MCP, only the adapter layer changes.

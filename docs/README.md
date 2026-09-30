# Mind Place Documentation

Mind Place is a local-first, provider-neutral personal context and memory system for AI conversations.

The project exists because model capability changes much faster than a person's accumulated context. A user may prefer ChatGPT today, Claude tomorrow, Gemini next month, and another frontier model later. The model should be replaceable. The user's continuity should not be.

Mind Place therefore treats personal context as durable infrastructure and treats AI models as interchangeable clients.

This documentation captures the current product and architecture brainstorming. It is intentionally broader than an implementation plan: it records ideas, constraints, non-goals, unresolved questions, and design principles that should guide implementation.

## Core thesis

The permanent asset is not the model.

The permanent asset is the user's accumulated context:

- complete conversations;
- selected long-term memories;
- project state;
- preferences;
- decisions;
- files and documents;
- relationships between ideas;
- provenance showing where a memory came from;
- history showing how context changed over time.

Mind Place should preserve that context locally and expose it to any capable model through a small, powerful retrieval and memory interface.

## High-level architecture

~~~text
ChatGPT / Claude / Gemini / future models
                |
        Browser extension / MCP
                |
                v
+---------------------------------------+
|              MIND PLACE               |
|                                       |
| L0  Raw archive                       |
| L1  Search/index layer                |
| L2  Curated memories                  |
| L3  Knowledge graph                   |
| L4  Derived views/context packs       |
+-------------------+-------------------+
                    |
                    v
          Local durable storage
~~~

The critical distinction is between raw evidence and derived understanding.

Raw source data should be durable. Everything derived from that source should be disposable and rebuildable.

## Documentation map

1. [Product Vision and Principles](./01-product-vision.md)
2. [System Architecture](./02-system-architecture.md)
3. [Data Model and Storage](./03-data-model-and-storage.md)
4. [Conversation Capture and Ingestion](./04-capture-ingestion.md)
5. [Curated Memory Model](./05-curated-memory.md)
6. [Retrieval and Context Engine](./06-retrieval-context-engine.md)
7. [Knowledge Graph and Interactive UI](./07-knowledge-graph-and-ui.md)
8. [Maintenance and Derivation Workers](./08-maintenance-and-derivation.md)
9. [MCP and Local API Design](./09-mcp-and-api.md)
10. [Roadmap and Open Questions](./10-roadmap-and-open-questions.md)

## Short product definition

> Mind Place is a local, durable context layer for AI: it automatically archives full conversations, stores model- or user-curated memories, indexes files and history, connects related knowledge into an inspectable graph, and lets any current or future model research that context efficiently without owning it.

## Non-negotiable ideas

- Save complete conversations. Storage is cheap; losing history is expensive.
- Optimize retrieval, not preservation. The bottleneck is context injection, not disk space.
- Never make summaries the only copy. Raw source remains available.
- Temporary/private chats are not captured by default.
- Do not store provider HTML or DOM as the canonical record.
- Use a provider-independent normalized representation.
- The model decides what deserves curated memory. Core storage does not need an embedded LLM.
- Curated memory must retain source provenance.
- Derived data must be rebuildable from raw data.
- Retrieval should behave more like research than a single vector search.
- The graph should represent semantic knowledge, not every raw message as a visible node.
- The GUI should be a playground, not an admin panel.
- Models are replaceable. Personal continuity is not.

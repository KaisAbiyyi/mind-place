# Mind Place documentation

Mind Place separates the **current product contract** from the **longer-term architecture roadmap**.

## Current product

- [Current product architecture and guarantees](./current-product.md)
- [Conversation capture and ingestion](./04-capture-ingestion.md) — broader capture design with the implemented behavior called out at the top
- [Changelog](../CHANGELOG.md)
- [Contributing](../CONTRIBUTING.md)
- [Security policy](../SECURITY.md)

The shipping product is a local Rust/Tauri archive reader plus a dedicated ChatGPT capture extension. Manual capture is always available. Automatic saving is an explicit opt-in and observes only already-loaded messages; it does not scroll or load history.

## Roadmap and design notes

The documents below describe the larger provider-neutral vision. They are design material rather than claims about currently shipped behavior.

1. [Product vision and principles](./01-product-vision.md)
2. [System architecture](./02-system-architecture.md)
3. [Data model and storage](./03-data-model-and-storage.md)
4. [Conversation capture and ingestion](./04-capture-ingestion.md)
5. [Curated memory model](./05-curated-memory.md)
6. [Retrieval and context engine](./06-retrieval-context-engine.md)
7. [Knowledge graph and interactive UI](./07-knowledge-graph-and-ui.md)
8. [Maintenance and derivation workers](./08-maintenance-and-derivation.md)
9. [MCP and local API design](./09-mcp-and-api.md)
10. [Roadmap and open questions](./10-roadmap-and-open-questions.md)

## Active implementation plans

- [BIG implementation plan](./BIG-IMPLEMENTATION-PLAN.md) — current high-level roadmap for Mind Place as a model-independent continuity layer, including the storage stack, evidence-aware memory model, phase sizing, multi-provider direction, and potential feature universe.
- [MCP personal knowledge and allowed-folder indexing](./implementation-plan/2026-10-18-mcp-personal-knowledge-v1.md) — executable architecture for unified chat/document retrieval, approved folders, a ChatGPT MCP tunnel, rich fetch configuration, and agent-assisted Graphify generations.

The core long-term principle is stable: models are replaceable, while a user's accumulated context should remain durable, inspectable, portable, and under the user's control.

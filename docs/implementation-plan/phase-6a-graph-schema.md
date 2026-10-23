# MP-6A — Derived knowledge graph schema and deterministic edges

Task: [GitHub issue #52](https://github.com/KaisAbiyyi/mind-place/issues/52).
Owner: GPT-6 Cloud, session `mp6a-20261023-1838-wib-gpt6-cloud`.
Resource lock: `graph-index-schema`. Cloud-only implementation; no local workspace touched.

## Storage boundary

- `index.sqlite3` version 2 is a disposable **projection**, not the authoritative source. Original archive captures are immutable; `state.sqlite3` still owns permissions, IDs and memory status.
- `schema_v1.sql` remains the exact historical v1 schema. `schema_v2.sql` adds only `graph_nodes`, `graph_scope_nodes`, and `graph_edges` with deterministic indexes.
- `IndexStore::open` verifies application ID, quick integrity check, version and the original v1 schema hash *before* an existing v1 database is upgraded, then runs all v2 additions and the version/hash update atomically. It refuses unknown/corrupted schemas instead of deleting the file.
- A deleted derived index is repopulated from raw/state by normal archive sync and future graph producer replays; no graph projection contains irreplaceable annotations.

## Producer-scoped replacement

- `replace_graph_projection[_with_check](scope, projection)` atomically replaces one producer scope. Call only after unlock and scoped source authorization; the caller must supply a **live, revocation-sensitive check callback** when external source grants are involved. The default convenience method does not assert authorization on its own.
- `GraphKind`: project, topic, file, conversation, memory, entity, revision, import, symbol. Canonical identifiers include the kind and a stable source key, never a display label.
- `GraphRelation`: contains, references, version, imported_from, defines_symbol. Invalid orientations, missing endpoints, empty provenance and oversized input are rejected.
- Every deterministic edge stores `source_ref`, optional `source_revision`, and a non-empty `locator`. IDs are SHA-256 of **length-prefixed** canonical fields, including the producer scope for edges, so repeated/shuffled inputs are idempotent, and identical edges from different scopes coexist.
- A scope can release its nodes and edges without removing nodes still referenced by another scope. Conflicting labels/source pointers cannot silently overwrite a node owned by another scope.
- The reserved `archive:chatgpt` scope can be written only by verified archive synchronization, inside the same FTS transaction and final cancellation gate. It projects conversation-version and revision-import evidence, no speculative semantic claims.

## Boundaries left to future tasks

- **6B**: bounded neighborhood traversal, communities and retrieval ranking. This phase does not introduce graph-backed search or infer semantic relations from arbitrary message text.
- **6C**: generation manifests, content-hash invalidation, staged/atomic graph activation and incremental rebuild strategy.
- **6D**: Graphify maintenance grants, sequence/idempotence, source revision and root fences.
- **6E**: graphification prompts, inferred-edge confidence and corpus evaluation.

## Validation requirement

Mandatory cloud checks: `cargo fmt --all -- --check`, `npm run verify:repo`, `npm run typecheck`, `npm test`, Windows product CI and storage qualification 10K/100K/1M, exact SHA self-review against base. Record PASS/FAIL/NOT_RUN as observed and re-run after every fix; passing synthetic cloud CI does not substitute for native/owner-specific tests in later phases.

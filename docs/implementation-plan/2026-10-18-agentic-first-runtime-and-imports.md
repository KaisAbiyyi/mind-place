# Agentic-first runtime, conversation routing, and import direction

This document amends the current Mind Place BIG implementation plan with four product decisions that are now first-class requirements:

1. every writable AI session should be able to classify its intent into Mind Place projects/topics as the conversation begins and evolves;
2. Mind Place must import and normalize agent-session histories from Codex, Claude Code/Cowork, and future agent runtimes without depending on one vendor's storage format;
3. the repository and runtime are designed **agentic-workflow first**: security, data integrity, performance, deterministic contracts, and AI legibility take priority over human-oriented code aesthetics;
4. the installed app may start with Windows, but it starts **locked**. No MCP/tunnel/client connection becomes usable until the user unlocks Mind Place with a PIN/password or a future stronger local-unlock method.

Where this document conflicts with older phase descriptions, this direction wins until the BIG plan is folded forward.

## 1. Conversation intent routing

A conversation is evidence and activity inside a larger project/topic graph; it is not itself the durable project identity.

When an AI client has Mind Place write capability, the preferred start-of-conversation flow is:

```text
first meaningful user intent
        |
        v
client asks Mind Place for likely project/topic candidates
        |
        v
Mind Place returns compact candidates + evidence
        |
        v
active AI reasons about the intent
        |
        v
AI writes a reversible routing assertion
        |
        v
conversation is associated with project/topic/workspace
```

Mind Place should not require its own always-running LLM classifier. The frontier model already talking to the user is normally the best semantic classifier available; Mind Place supplies deterministic candidate retrieval, durable assignments, provenance, and correction history.

### 1.1 Routing record

A routing decision is derived state, not raw truth. Conceptually:

```text
conversation_route
  conversation_id
  primary_project_id?       optional
  topic_ids[]
  workspace/folder_id?      optional logical workspace, not filesystem permission
  asserted_by               user | model | deterministic | import
  confidence?               meaningful for inferred routing
  evidence_refs[]
  valid_from
  valid_until?
  status                    active | superseded | disputed
  created_at
```

Rules:

- manual/user assignment outranks inferred model assignment;
- an assignment never grants filesystem access;
- one conversation may belong to several topics while normally having one primary project;
- unknown intent is allowed — do not force a project just to avoid `unassigned`;
- assignments are append/supersede oriented so history remains inspectable;
- a material topic shift may trigger a new routing assertion instead of rewriting history;
- imported old conversations can be backfilled later using the same route model;
- routing is a retrieval hint and organizational layer, never a reason to suppress conflicting evidence.

### 1.2 MCP/session shape

Keep the MCP surface compact. The exact tool names are implementation details, but the capability should support a session bootstrap operation that can:

- register or resolve a provider-neutral conversation/session identity;
- accept the current user intent and provider metadata;
- return likely projects/topics and compact prior context;
- optionally write a route when the client has the correct capability;
- re-evaluate routing on explicit topic change or when the client judges that the intent materially drifted.

A read-only client can receive candidates/context but cannot create routing assertions.

## 2. Agent-session imports are first-class sources

Browser capture is only one ingestion path. Mind Place must also ingest work performed in coding/agent products where conversations are stored locally or exported in a different representation.

Initial targets:

- Codex sessions/work histories where an accessible local/export format exists;
- Claude Code sessions;
- Claude Cowork sessions/exports where accessible;
- generic JSON/JSONL/Markdown/HTML transcript exports;
- future agent runtimes through the same adapter contract.

Do not bake assumptions about today's vendor file layout into the canonical archive schema. These formats can change.

### 2.1 Raw-preserving conversion

Import is conversion, not destructive migration.

For every imported session:

```text
source artifact / source records
        |
        +--> preserved raw import bytes or stable raw records
        |
        v
versioned importer adapter
        |
        v
provider-neutral normalized session/events
        |
        v
search / memory / project routing / graph
```

The raw source remains available for re-import with a better parser later.

Importer metadata should include:

- importer id + version;
- source family and detected source format/version;
- original source identity/fingerprint where available;
- import time;
- raw content/object hashes;
- parse warnings and unsupported records;
- mapping from normalized events back to source records/ranges.

Unknown source fields should be retained as bounded opaque metadata when useful rather than silently discarded. Mind Place must never fabricate tool calls, timestamps, model identities, hidden reasoning, or message boundaries that the source format did not actually provide.

### 2.2 Normalized agent event model

The provider-neutral representation should be able to preserve more than user/assistant text:

- user messages;
- assistant messages;
- system/developer messages when the source legitimately exposes them;
- tool invocations/results;
- file/artifact references;
- model/provider/runtime metadata;
- branches/regenerations/retries where representable;
- timestamps and session boundaries;
- source repository/workspace identity when explicitly available and authorized.

This normalized model must remain extensible. Provider-specific fields stay metadata unless they become genuinely cross-provider concepts.

### 2.3 Import UX

Mind Place should expose both:

- an **Import chats / agent sessions** action; and
- onboarding/tutorial guidance explaining how to import Codex, Claude Code/Cowork, and other supported histories.

The importer should:

- detect supported formats rather than make the user understand them;
- preview source family, session count, approximate data volume, and warnings before commit;
- import incrementally/resumably;
- deduplicate using source identity + stable fingerprints/content hashes;
- report partial/unsupported records explicitly;
- preserve the source even when normalized conversion is incomplete;
- optionally remember an approved import directory for future incremental imports, but only with explicit permission.

A remembered agent-session folder is still an approved root and follows the same revocation/security model as every other folder.

## 3. Agentic-first repository doctrine

Mind Place is expected to be implemented and maintained primarily by strong coding agents operating in cloud environments. Human readability remains useful, but it is not the primary optimization target.

Priority order:

```text
1. security and permission correctness
2. data integrity / provenance / reversibility
3. performance, latency, memory and storage efficiency
4. deterministic machine-readable contracts
5. testability, observability and reproducibility
6. AI/agent legibility and navigation cost
7. human-oriented elegance/aesthetics
```

This does **not** mean intentionally obscure source. It means the repository should minimize ambiguity for agents rather than optimize for tutorial-like code.

### 3.1 AI-legible source layout

Prefer:

- small modules with one responsibility and explicit invariants;
- stable schemas and explicit state machines;
- protocol definitions close to their implementations;
- generated or machine-readable schema/contract artifacts where helpful;
- stable error codes instead of prose-only failure behavior;
- deterministic commands for build/test/bench/migrate/rebuild;
- narrow dependency direction and few hidden side effects;
- fixtures that encode real contracts and historical regressions;
- root/nested `AGENTS.md` guidance when a subsystem becomes complex enough to justify it;
- a generated repository/architecture manifest later if it measurably reduces agent discovery cost.

Avoid:

- giant mixed-responsibility files;
- clever metaprogramming that makes behavior difficult to recover from static inspection;
- duplicated sources of truth;
- hidden manual setup steps that CI cannot express;
- human-facing prose being the only specification for a security or storage invariant.

### 3.2 Low-level optimization policy

Mind Place may use low-level or binary representations aggressively when measurements justify them.

Potential examples include:

- compact binary derived indexes;
- memory mapping for trusted bounded derived data;
- zero-copy/borrowed parsing on hot paths;
- packed/varint representations;
- Zstandard compression;
- SIMD/parallel search or hashing;
- specialized Rust data structures instead of generic abstractions.

But the rule is **benchmark-gated optimization**, not complexity theater.

The canonical/evidence layer must stay recoverable and versioned. A fast binary format should normally be a derived/rebuildable representation unless there is a compelling, tested migration and recovery story. Unsafe code, mmap, custom allocators, lock-free structures, or custom binary protocols require explicit threat-model/bounds tests because performance never outranks security or provenance.

## 4. Locked startup and connection gate

Mind Place should be able to start automatically with Windows so capture/retrieval infrastructure is ready early, but the process starts in a minimal **LOCKED** state.

Expected behavior:

```text
Windows login / user launches Mind Place
        |
        v
minimal Mind Place process/tray starts
        |
        v
LOCKED
        |
        +--> no MCP client access
        +--> no public tunnel
        +--> no folder scanning
        +--> no archive/search disclosure
        +--> no privileged maintenance jobs
        |
        v
user enters PIN/password (or future Windows Hello equivalent)
        |
        v
UNLOCKED epoch begins
        |
        +--> authorized services start
        +--> client/tunnel credentials become usable
        +--> approved watchers/indexers may run
```

### 4.1 PIN/password security rules

A short PIN is a convenience unlock factor and must **not** be treated as sufficient cryptographic entropy by itself.

Design requirements:

- never store plaintext PIN/password;
- never pass the secret on command lines, URLs, logs, telemetry, crash metadata, or environment variables;
- use a memory-hard password KDF such as Argon2id with measured parameters for password/PIN verification or key wrapping;
- store device key material through the OS credential/key protection facility where appropriate;
- rate-limit/back off failed unlock attempts;
- use constant-time comparisons where relevant;
- best-effort zeroize sensitive key material when locking/shutting down;
- invalidate client/tunnel/write credentials on lock through an unlock/session epoch;
- auto-lock on Windows session lock/sleep should be supported and qualified;
- the local API while locked exposes at most minimal non-sensitive health/unlock state;
- the browser extension must show `Mind Place locked` rather than silently keeping a plaintext private capture queue.

Until full encryption-at-rest covers the relevant archive/object state, the UI must not claim that the PIN makes raw files unreadable to another local process. Service locking and at-rest encryption are related but distinct security guarantees.

### 4.2 Encryption moves earlier conceptually

Because Mind Place will hold unusually sensitive long-lived personal context, encryption architecture should be decided before large new personal-folder/provider ingestion becomes normal. Phase 9 remains the deep hardening/backup/recovery phase, but earlier phases must establish a compatible master-key/unlock design so the project does not need a dangerous late rewrite.

## 5. Roadmap delta

The current cloud-first workflow remains unchanged: GitHub/cloud is the implementation source of truth; local is qualification only unless explicitly authorized otherwise.

These ideas increase the safe cloud-unit ceiling because they cross security, storage, import, and provider boundaries. Revised planning guidance:

| Phase | Previous C/L/U | Revised C/L/U | Added scope |
| --- | ---: | ---: | --- |
| 1 — Durable brain storage | 4/0/0 | **5/0/0** | unlock/key architecture compatibility + agentic state contracts |
| 2 — Retrieval kernel | 4/0/0 | **4/0/0** | no structural change |
| 3 — MCP + tunnel | 5/2/1 | **6/2/1** | locked startup, unlock epoch, connection gate |
| 4 — Documents/folders | 6/2/1 | **6/2/1** | approved roots already cover remembered import folders |
| 5 — Memory truth | 4/0/0 | **5/0/0** | conversation intent/project/topic routing semantics |
| 6 — Graph/Graphify | 5/0/0 | **5/0/0** | routes feed graph/project context |
| 7 — Multi-provider/import | 5/2/2 | **8/3/2** | Codex, Claude Code/Cowork, raw-preserving converters/import center |
| 8 — Knowledge OS UI | 6/2/2 | **7/2/2** | route correction + import management UI |
| 9 — Privacy/portability | 6/2/2 | **6/2/2** | hardens earlier unlock/key foundation rather than inventing it late |
| 10 — v1.0 qualification | 5/3/2 | **5/3/2** | final E2E now includes lock/routing/import continuity |
| **Total** | **50/13/10** | **57/14/10** | safe ceiling, before retry/fix units |

Optional semantic/search work remains benchmark-gated and is not included in the 57 cloud units.

### New cloud unit boundaries

Add the following named units when the BIG plan is next folded forward:

- **1E — security root/unlock architecture**: key hierarchy abstraction, unlock state/epoch contracts, secret-handling tests, future encryption compatibility;
- **3F — locked startup and connector gate**: Windows-start behavior, unlock prompt/state, no MCP/tunnel before unlock, credential invalidation on relock;
- **5E — conversation intent router**: session bootstrap, project/topic candidates, reversible model-authored routing assertions, drift/reassignment semantics;
- **7D — agent-session interchange/raw-preserving conversion model**;
- **7E — Codex importer**;
- **7F — Claude Code/Cowork importer**;
- **7G — import center/onboarding + generic format detection**;
- **7H — cross-import regression/capability matrix**;
- **8G — routing/import management UI**.

Phase 7 receives one additional local qualification session:

- **7L3 — real agent-session import qualification**: import representative real Codex and Claude Code/Cowork histories from user-authorized locations, verify raw preservation, normalized event fidelity, deduplication, incremental re-import, project routing, and MCP retrieval.

Existing Phase 3 local/user gates should explicitly test locked startup/unlock/relock. Existing Phase 10 E2E should begin from a locked process, unlock, connect a fresh AI client, route a new conversation, retrieve an imported coding-agent session, and continue the same project across providers.

## 6. Immediate repository consequences

From this point forward, implementation agents should assume:

- cloud-first/GitHub source of truth;
- security review is part of architecture, not a final polish pass;
- new persistent formats need explicit versioning and recovery/rebuild behavior;
- provider/import adapters preserve raw source before normalization;
- AI-authored routing/memory/graph claims are labeled derived assertions rather than truth;
- hot-path performance should be measured and may justify low-level Rust/binary optimization;
- repository structure should minimize agent discovery cost and ambiguity;
- local-only assumptions must be surfaced in the handoff instead of silently solved on a developer machine.

The intended end state is a personal AI substrate that can absorb work from consumer chats and coding agents, automatically keep that activity organized around durable projects, and remain fast and secure enough that the memory layer expands a frontier model's intelligence instead of constraining it.
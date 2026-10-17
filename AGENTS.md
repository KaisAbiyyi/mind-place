# Working on Mind Place

Mind Place is developed **cloud-first and agentic-workflow first**. GitHub is the implementation source of truth. Local Windows checkouts are primarily qualification targets unless the user explicitly authorizes local implementation.

Read `README.md`, `docs/current-product.md`, and `docs/BIG-IMPLEMENTATION-PLAN.md` before changing product behavior. For current agentic runtime/import direction also read `docs/implementation-plan/2026-10-18-agentic-first-runtime-and-imports.md`.

## Engineering priority

Optimize in this order:

1. security and permission correctness;
2. data integrity, provenance, reversibility, and migration safety;
3. performance, latency, memory, storage, and startup cost;
4. deterministic machine-readable contracts;
5. testability, observability, reproducibility, and benchmarkability;
6. AI/agent legibility and low repository-discovery cost;
7. human-oriented code aesthetics.

Human readability is useful but is not the primary design target. Do not intentionally obfuscate source. Prefer explicit invariants, small modules, stable schemas/state machines, stable error codes, deterministic commands, fixtures, and narrow dependency direction over tutorial-style prose or clever abstractions.

Low-level Rust, compact binary derived formats, mmap, zero-copy parsing, SIMD/parallel work, packed representations, or specialized data structures are welcome **only when measured**. Performance never outranks security, provenance, bounds checking, or recoverability. Derived optimized representations should normally remain rebuildable.

## Product invariants

- The product is the root Rust workspace (`crates/`, `src-tauri/`), TypeScript reader (`src/`), and dedicated capture extension (`extension/`). Do not introduce external application source trees or generated build output into this repository.
- Preserve immutable raw capture/import revisions. Markdown, normalized events, summaries, indexes, graph data, and other derived views must never replace raw evidence.
- AI-authored memory, graph, project/topic routing, and classification records are derived assertions with provenance, not unquestionable truth.
- Provider/import adapters must preserve the original source representation or a stable raw record mapping before normalization. Never fabricate missing messages, tool calls, timestamps, model identities, hidden reasoning, or branches.
- Folder/project association never grants filesystem authority. All filesystem reads go through the approved-root permission boundary.
- Mind Place starts locked when startup/unlock support exists. MCP, tunnel, folder scanning, archive disclosure, and privileged maintenance must remain unavailable until an authorized unlock epoch exists.
- Never log, commit, expose in URLs, or pass via command line raw PINs/passwords, tunnel secrets, vault keys, or private corpus contents.
- Automatic saving is opt-in and may inspect only the loaded ChatGPT DOM. It must never scroll, change scroll offsets, or load older history.
- Manual history scrolling is allowed only during an explicit Save job. Preserve bounded execution, progress, Stop & save, Cancel, identity/privacy checks, and partial-completeness reporting.
- Keep extension permissions minimal. Required checks reject unconditional host access and content scripts.
- Preserve unrelated working-tree changes and local user archives.

## Cloud/local workflow

A cloud unit should implement one bounded subsystem or integration boundary, run appropriate tests/benchmarks, push GitHub, and leave a handoff with the final SHA and whether local qualification is required.

When a phase requires device/browser/account qualification:

1. hand off the cloud-complete SHA to the user;
2. pull that exact SHA locally;
3. run only the assigned qualification;
4. return evidence as `PASS`, `FAIL`, `PARTIAL`, or `BLOCKED`;
5. if a product bug is found, send the evidence back to a cloud fix unit instead of silently turning the local checkout into the canonical implementation branch.

## Required verification

Run the checks appropriate to the change. Before publishing a milestone, run the full baseline:

```text
npm run verify:repo
npm run typecheck
npm test
cargo fmt --all -- --check
npm run build
```

Add focused security, migration, fuzz/bounds, benchmark, provider-fixture, and rebuild tests when the changed subsystem requires them.

Windows x64 is the qualified packaging target. Use `npm run dist:win` when release packaging changes.
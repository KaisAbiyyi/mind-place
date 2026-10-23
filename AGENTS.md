# Working on Mind Place

Mind Place is developed **cloud-first and agentic-workflow first**. GitHub is the implementation source of truth. Local Windows checkouts are primarily qualification targets unless the user explicitly authorizes local implementation.

Read `README.md`, `docs/current-product.md`, `docs/BIG-IMPLEMENTATION-PLAN.md`, and `docs/implementation-plan/CURRENT-PHASE.md` before changing product behavior. For current agentic runtime/import direction also read `docs/implementation-plan/2026-10-18-agentic-first-runtime-and-imports.md`. `CURRENT-PHASE.md` wins when older roadmap labels disagree about the current execution order.

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

## Shared multi-agent orchestration (2026-10-19)

**Required reading:** [docs/AGENT-ORCHESTRATION.md](docs/AGENT-ORCHESTRATION.md). This is an additive execution protocol; existing product, phase, local D:, security and acceptance rules continue to apply.

- All roadmap tasks remain **unassigned** until an agent explicitly claims a bounded unit. ChatGPT Work, Codex Cloud/local and optional future executors may each architect, implement or review based on tool availability and budget.
- Before editing, check active PRs/claims, base SHA, dependencies and overlapping **logical** resources. Exclusive single-writer ownership per unit; no conflicting parallel implementation. Issue labels are **not** atomic locks. Without a working claim controller, a coordinator must serialize conflicting work.
- Use task-specific branches and early draft PRs with safe, sanitized, **pushed checkpoint commits**; record last durable SHA and unfinished work. An interrupted Codex VM or browser session is not proof that its changes reached GitHub. Recover/report first; take over only after fencing/releasing the previous claim.
- Keep GitHub as source of truth and respect all named cloud/local/human gates. Preserve unrelated work, private fixtures and security constraints. No local/native test is silently replaced by hosted CI.
- Prefer the owner's verified GitHub noreply identity for new commits where supported and authorized; disclose attribution exceptions. PR creation/push identity and commit author may differ. Keep actual agent provenance in PR/task records; avoid unsolicited AI co-author trailers.

## Stable neutral task IDs and parallel lanes

Before claiming work read [docs/PARALLEL-EXECUTION-BOARD.md](docs/PARALLEL-EXECUTION-BOARD.md). A request such as `ambil MP-4E` selects a bounded unit subject to its dependencies and exclusive locks; board entries never preassign an agent.

## Universal task lifecycle, self-review, and parallel execution board (2026-10-23)

**Owner-corrected universal policy (supersedes any older mandatory independent-review or single-card board instructions).** Applies to every implementation task in this repository and its Notion board. An `implement` / `kerjakan` / `execute` request covers claim, implementation, verification, **direct self-review by the implementor**, repairs, eligible PR merge, and same-session GitHub/Notion synchronization. Do not delegate a mandatory review to Copilot or another agent. Optional external feedback must never become a required acceptance dependency.

- **Ready**: every prerequisite is satisfied, the required environment is available, no conflicting resource/file/schema lock is held, and the task has no current claimant. A merely completed predecessor does **not** override another task's active exclusive lock.
- **Claimed / In Progress**: the same implementation phase; use `Claimed` for new claims, with a real owner/session, task ID, environment, branch/PR, base SHA, lock scope and explicit claim before modifying code. Placement in Now does not itself claim or start a task.
- **Review**: after code is pushed for CI/tests/self-review, while checks run, material issues are repaired, PR awaits merge or required local/owner gate is pending. The **implementor reviews their own exact HEAD vs base directly**, critically checking changed contracts, dependencies, security/privacy, authorization, migration/state, races/resource limits, backwards compatibility, scope, tests and CI. Recheck on revised SHA. There is **no independent-review gate**, no Copilot dependency, and no required different agent. Identify this accurately as `Self-review — <implementor/session>`; never label it `independent`.
- **Evidence**: report exact head/base SHA, changed paths, credible P0/P1/P2 findings and corrections, test/CI links and verdict, PASS/FAIL/PARTIAL/BLOCKED/NOT_RUN/unknown, outstanding risks and environment limitations. Do not invent tests, manipulate fixtures, hide findings, downgrade open defects or treat hosted CI as real Windows/SketchUp/owner qualification. No self-review verdict overrides a failing required gate.
- **Done**: only when real task-specific acceptance, self-review, CI and eligible merge (where required) are verifiably complete. Record merge SHA, release claim, remove from 02 · Now, retain among at most **five most recent** genuine 01 · Last Done cards, and recalculate next waves. A cloud merge does not certify physical-native or owner gates.
- **Reclaim**: exceptional fenced transfer only when the prior implementor is genuinely blocked/interrupted and cannot continue. Record prior owner, blocker, checkpoint SHA/PR, released locks, new owner. Ordinary CI wait/review and unclaimed tasks are not Reclaim.
- **Board 00 · Current State**: `01 · Last Done` (max five), `02 · Now`, `03 · Up Next`. **Now may contain more than one card**, and must include **every task already runnable in parallel now** (prerequisites satisfied, pairwise nonconflicting locks/authority), even if the individual task is still `Ready` and unclaimed. **Up Next may also have multiple cards**, but only for the next future dependency wave, *not* for tasks already able to run concurrently with Now. If only one task is runnable now, one Now card is correct. Do not manufacture concurrency: serialize overlapping schema/file/authority contracts even when nominal lock labels differ. A Review task in Now does not block unrelated Ready tasks from also appearing in Now. Up Next may show a truly blocked successor with its factual Blocked status/reason; do not call it Ready solely because a predecessor has finished.
- **Merge/sync**: when required checks and self-review pass, merge eligible PRs without waiting for a separate reviewer, verify merge SHA, and update GitHub issue/PR and Notion Status, Focus Lane, Claim Owner, self-review identity/verdict, evidence, blocker, next action **within the same session**, then read back. Do not force merge through failed CI, proven security defects, write collisions, required physical/owner gates, or unresolved dependencies. Environment permission limits still apply.

Canonical policy: [Projects — Notion](https://app.notion.com/p/400c4a6aaf738028be89c5dd6e509f77).

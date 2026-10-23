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

## Universal task status and integrated independent review (2026-10-23)

This owner-approved rule applies to every implementation task in this repository and its project Notion board. It supersedes older status transition guidance; it does not bypass dependency locks, security controls, or Cloud/Local/Owner acceptance. A request to `implement`, `kerjakan`, or `execute` authorizes the complete scoped cycle, including independent review, repairs, eligible PR merge, and Notion/GitHub sync in the same work session.

- **Ready**: required dependencies and resource availability verified; no current implementor. Placement in **02 · Now** is a focus lane, not a claim.
- **Claimed** (legacy **In Progress** has the same meaning): before edits, explicitly claim with the identifiable actor/session, environment, task ID, branch/PR, base SHA and exclusive logical/resource lock in GitHub and Notion. Prefer `Claimed` for new claims. Announce the claim and retain accurate ownership.
- **Review / In review**: transition as soon as implementation is pushed for GitHub CI/tests or independent code review, including while checks are running, fixes are being made, native/owner gates are pending, or the PR remains draft. Assign a reviewer *separate from the implementor*. Prefer Copilot when available, otherwise promptly use an independent ChatGPT worker, Codex, or another agent with a clean read-only brief pinned to exact head and base. An agent cannot self-certify independence, and a shared GitHub account does not prove separate agent identity.
- Reviewer checks the actual diff, contracts, tests, security, authorization, privacy, persistence, races, resource limits, backward compatibility, and evidence provenance as applicable. Record P0/P1/P2 with file/line and consequences; distinguish PASS, FAIL, PARTIAL, BLOCKED, NOT_RUN, HOLD and unknown. Never invent runs, downgrade findings, alter fixtures or acceptance criteria, or treat CI as native/user acceptance. Fix supported findings, rerun affected checks and commission a new independent exact-SHA review after material changes. A Copilot quota or unavailable reviewer is a scheduling issue: use an available independent agent; if none exists, keep **Review**.
- **Done**: required CI, independent review, task-specific acceptance gates, and the eligible merge are proven. Record implementor, reviewer identity/session, reviewed head/base, findings/verdict, CI run links, native/owner receipt limits, merge SHA, remaining risks and next action in GitHub PR/task and Notion. Remove the task from **02 · Now**, show it in **01 · Last Done** (maximum five recent done tasks), and promote genuinely eligible next task(s) to **Now / Up Next** using prerequisite and parallel-lock checks. Read Notion back before closing.
- **Reclaim** is *only* for a prior implementor who is concretely blocked/interrupted and requires a different implementor: record old owner, last durable SHA/PR, blocker, fence/release/transfer proof and new owner. CI pending, ordinary review and never-claimed tasks are **not** Reclaim.
- **Merge rule**: after independent review and applicable checks, merge the approved PR and verify exact merged SHA without requiring another generic request. Never force merge through actual security findings, failed CI, unresolved conflicts, overlapping writer claims or a gate explicitly required before merge. Keep **Review** or the appropriate explicit gate status when blocked; a cloud merge does not certify local Windows, SketchUp, installer or owner acceptance.
- Synchronize the corresponding Notion task's Status, Focus Lane, Claim Owner, Review Owner/Verdict (if present), PR, head/merge SHA, test evidence, blockers and Next Action whenever claim, review, merge or acceptance changes. The [Projects global policy](https://app.notion.com/p/400c4a6aaf738028be89c5dd6e509f77) governs all current and new projects.

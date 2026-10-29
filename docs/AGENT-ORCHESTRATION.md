# Shared multi-agent development orchestration

**Policy version:** 2026-10-19. **Scope:** `KaisAbiyyi/mind-place` within **Mind Place**. This is **execution governance**, not a re-planning or completion receipt. The existing BIG plan, phase ordering, quality limits, privacy requirements and current-phase pointer remain authoritative for *what* to build. This policy controls *how independent workers coordinate*. If a product/security rule is stricter, retain the stricter rule.

## Actors and authority

- **Human owner:** absolute product/release authority; approves real-account consent, destructive changes, irreversible publication and subjective acceptance, and initiates named local/native qualification gates. Do not burden the owner with routine agent-to-agent dispatch.
- **ChatGPT Work/GitHub-connected agent, Codex Cloud/local, and optional future Antigravity:** capability-based workers. Any can plan, research, implement, test, review or coordinate if the actual environment has the necessary tools. **Never hard-code a task to one vendor/model.** The implementor MUST directly self-review the exact base-to-HEAD diff and critical security, data and integration boundaries; a separate reviewer/agent is NOT mandatory and MUST NOT be a merge gate. Record this truthfully as self-review, not independent approval; honor all actual CI/native/owner gates.
- **GitHub:** durable source of truth for committed work, PRs, CI, SHA-specific evidence and handoffs. Conversation context and temporary cloud VMs are not durable coordination stores.
- **Shared usage:** budget ChatGPT Work and Codex as one constrained pool where they share allowance; do not assume switching between them replenishes usage. Prefer bounded tasks, no idle agent waiting for CI, and measured concurrency over maximum simultaneous sessions. Do not assume rate/usage APIs exist.

## Neutral roadmaps and work units

Roadmaps and phase documents list task IDs, scope, prerequisites, expected outputs, tests, local (L) and human (U) gates; they **do not preassign agents**. Phase completion is a dependency/acceptance result, never merely a finished conversation. Before starting a task, inspect current `main` SHA, active PRs/claims, instructions, neighboring work, unmerged changes and its dependency DAG.

Every executable unit must have: globally unique `project/repo/task-id`, one bounded deliverable, testable acceptance criteria, dependency IDs, affected repos/files/**logical resources** (e.g. schema/protocol/security authority), explicit cloud/native/human gates, isolation branch, resource/usage-risk budget, recovery checkpoint, and integration plan. Split work if it cannot safely reach a reviewable checkpoint within one agent run. Independent siblings may run in parallel only when their dependency contracts and *logical* write sets are disjoint. The same agent may change role between units.

## Claim, exclusive ownership and safe parallelism

1. **Claim before modifying:** announce `TASK_ID, AGENT, INSTANCE/SESSION, REPO, BRANCH, RESOURCE_LOCKS, BASE_SHA, DEPENDENCIES, CLAIM_VERSION, CHECKPOINT_SHA, LAST_ACTIVITY, STATUS` in the shared task ledger and link the corresponding draft PR/issue. A claimed unit is single-writer. Other agents may inspect/review but must not modify that unit, overlapping files or logical contracts.
2. **Atomicity matters:** issue labels/comments and an unversioned markdown edit **are not locks**. A fully automated controller must use an atomic compare-and-swap/transaction with monotonically increasing claim generation (fencing) and cross-repo resource locks. Until it exists, serialize conflicting units via one coordinator and explicit owner-confirmed assignment; do not claim multi-agent exclusivity just because a task says `TAKEN`.
3. **Lease expiry is not transfer:** classify owner state as `ACTIVE / SUSPECTED_STALLED / INTERRUPTED / BLOCKED / RELEASED`. Browser errors, long tests, Codex quota exhaustion or lack of heartbeat do not prove the first worker stopped. Inspect VM/session availability, outstanding PRs and last remote checkpoint; wait or fence the former owner before transferring a write lease. Old workers must revalidate claim generation before another push.
4. **Recovery choices:** `REPORT_ONLY` may inspect the isolated branch/PR without taking write ownership. `CONTINUE` requires an atomic fenced transfer, a new owner/session, reconciliation of last pushed SHA with active environment changes, and a new task log. Never run two independent writers on one unit.

## Durable checkpoints and recovery

- Create a dedicated `work/<task-id>/<slug>` or comparable short-lived branch; open a **draft PR early** where supported. Use small coherent commits and push safe checkpoints **before costly tests, context/usage exhaustion, provider handoff, and whenever a verifiable substep is done**. Record `DONE / IN_PROGRESS / NEXT / TESTS / RISKS`, branch HEAD and the exact unpublished work boundary. Do not wait for full task completion to push.
- Treat **uncommitted/unpushed Codex Cloud VM changes as at-risk**, even if that task's VM may be resumable. A fresh task/agent cannot be assumed to see those bytes. If push is unavailable, record `CHECKPOINT_NOT_REMOTE`; prioritize resuming the original workspace or making a vetted patch from it rather than falsely claiming recovery.
- Never checkpoint secrets, real tokens, private user data, generated binaries, confidential fixtures or other excluded artifacts. Check `git status`, staged diff, ignored paths and secret screening. For public repositories, a checkpoint branch is **publicly visible** too. Unsafe WIP stays isolated until sanitized.
- An interrupted task remains `INTERRUPTED` with precise last durable SHA and missing steps. After transfer, start from the latest verified remote snapshot, not an assumed conversational state. Re-run appropriate regression tests. Only CI-green reviewed work may merge; `MERGED` does not imply `L/U ACCEPTED`.

## GitHub attribution and traceability

- **Owner-preferred identity:** when the repository owner has explicitly authorized it and the environment supports it, set `git config user.name "Kais Abiyyi"` and `git config user.email "93252311+KaisAbiyyi@users.noreply.github.com"` for **new commits in this repository only**. This noreply form is grounded in this owner's existing repository commit metadata; verify it still belongs to the connected owner before use. Never guess addresses or use another person's credentials. If identity cannot be verified or set, disclose `ATTRIBUTION_EXCEPTION` rather than forging authorship. Respect actual contributor/authorship transparency through task/PR records.
- Commit **author**, GitHub **pusher**, **PR creator**, and **squash-merge author** are different. The remote app/bot may remain visible as PR creator/pusher; an agent cannot guarantee its absence from GitHub's contributor UI. Check resulting GitHub author attribution on committed and final squashed SHAs. Do not rewrite verified published history to cosmetically hide collaborators.
- Keep provenance of actual agent work in task/PR metadata (`agent, role, environment, model if known, task ID, execution/checkpoint/test SHA`). Do not silently append `Co-authored-by: Codex` or another AI credit unless expressly requested; do not falsify historical contribution. Prefer owner-linked author attribution for approved new commits where technically supported, with transparent agent execution records.

## Acceptance, reporting and interruption protocol

Use `UNCLAIMED → CLAIMED → RUNNING → CHECKPOINTED → REVIEW → MERGED → (LOCAL_PENDING / USER_PENDING) → ACCEPTED`; additionally `BLOCKED / INTERRUPTED / FAILED / ABANDONED` with explicit evidence. Preserve the project's existing `PASS / FAIL / PARTIAL / BLOCKED / NOT RUN / HOLD` gate vocabulary. Follow dependency edges: do not begin a blocked dependent unit merely to keep workers busy; select another safe ready unit.

Each handoff must give: `PROJECT, REPO(S), TASK_ID, CLAIM_STATUS/OWNER/GENERATION, ROLE, BASE_SHA, BRANCH, CHECKPOINT_SHA, PR, IMPLEMENTED, TESTS/CI, LIMITATIONS, LOCAL_REQUIRED (L-ID or NONE), HUMAN_REQUIRED (U-ID or NO), NEXT, RECOVERY_INSTRUCTIONS`. On failure report what is **not** saved remotely. Agent reviewers cannot mark device/browser/SketchUp/account/UI gates passed from cloud-only simulation. The human receives consolidated decisions and qualified test instructions, not continuous micro-approval requests.

## Repository-specific qualification boundary

Phase 3E PR #14 is ongoing; this governance-only change neither claims nor completes it. Keep 3L1/3L2/3U live validation after cloud security qualification.

Current phase/dependency evidence: [implementation-plan/CURRENT-PHASE.md](implementation-plan/CURRENT-PHASE.md). This policy does **not** claim any existing phase as complete or start/stop another agent. No task registry, lock controller, checkpoint automation or agent dispatch API is installed by this docs change; these are enforceable operating requirements for agents and a specification for a future controller.


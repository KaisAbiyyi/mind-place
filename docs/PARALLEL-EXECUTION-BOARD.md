# Mind Place — dependency and parallel execution board

**Last coordinated baseline:** 2026-10-21. This supplements [the BIG plan](BIG-IMPLEMENTATION-PLAN.md) and [current phase](implementation-plan/CURRENT-PHASE.md); it does **not** replace acceptance gates or rewrite historical status. Read [agent orchestration policy](AGENT-ORCHESTRATION.md).

## P0 · Repository governance — MP-GOV-01..04 (31 October 2026)

The current repository policy is **owner-controlled manual merge**. Agents may implement and test on isolated branches or contributor-owned forks; AI review is read-only and advisory; only the repository owner approves and manually merges. GitHub Auto-merge, merge queues, custom Merge Brokers, autonomous merge, and bot/AI/App bypass actors are not authorized.

- **MP-GOV-01 / #136:** prepare the exact main-branch admission policy, canonical governance updates, owner runbook, and negative-test matrix. [Notion task](https://app.notion.com/p/409c4a6aaf738181bd96e5e7ea0583fa).
- **MP-GOV-02 / #137:** audit fork-PR workflow trust boundaries. This must be accepted before MP-GOV-04 applies the branch policy.
- **MP-GOV-03 / #138:** harden read-only AI review and prompt-injection boundaries. It is required before connecting an AI reviewer to GitHub, but is not a technical prerequisite for the owner applying branch protection.
- **MP-GOV-04 / #139:** owner-only apply/readback gate. It remains an Owner Gate until MP-GOV-01 and MP-GOV-02 are accepted and the owner applies and verifies the settings.

Read-only baseline at main SHA efe91155d37d8b1ca0608fe3bd862be311fed004: main.protected=false, classic required-check enforcement off, repository ruleset inventory empty, and repository Auto-merge disabled. The full classic-protection endpoint was not readable by the connected integration (HTTP 403); this is not evidence that additional settings exist. **Do not claim main is protected until MP-GOV-04 exact readback succeeds.**

The exact candidate settings, check-source evidence, owner-identity caveat, administrator/root-of-trust limitation, and disposable-repository negative-test matrix are in [the MP-GOV-01 owner runbook](governance/OWNER-MANUAL-MERGE-RUNBOOK.md). This is repository governance, not a product-phase completion or authorization to merge any current PR.

## P0 · License migration — MP-LIC-00 (22 October 2026)

**TOP PRIORITY / NOW — CLAIMED by ChatGPT Cloud on 2026-10-22, `feat/mp-lic-00-mit-relicense`; acceptance pending rights/compatibility + PR CI.** [Issue #33](https://github.com/KaisAbiyyi/mind-place/issues/33) and the [Notion task](https://app.notion.com/p/401c4a6aaf7381778e6fe0d3ed83da62) track the **AGPLv3 → MIT** migration for future Mind Place releases. Root `LICENSE` on `main` at claim baseline declares GNU AGPLv3; the dedicated branch proposes MIT pending approval. Before changing it, audit contributor copyrights, upstream/vendored content, assets and dependency notices; secure permission or rewrite code that cannot be relicensed. Update MIT LICENSE, SPDX/package metadata, README, legal/installer notices, CI checks and release notes together. Existing AGPL-licensed releases keep their historical license grants. Do **not** mark done from a mere file replacement.

This governance work uses the independent `license-governance/release-notices` lock; it can execute in parallel with the already-claimed `MP-4F` (`document-integration/retrieval`) without taking over or delaying its unrelated technical implementation. The Notion three-column Current State is the **live** focus/claim authority; this section records priority/dependencies, not a competing snapshot. The plan does not authorize unreviewed relicensing.

## Live focus-lane enforcement (24 October 2026)

The Notion Current State board is the live source of task state, not the historical static phase registry below. Reconcile every AND prerequisite against Done/merged evidence at each checker pass:

- **Now:** All actionable tasks with all AND predecessors Done, including **unclaimed Ready** tasks, active implementation, and **Review**. Do not hide Ready work from Now because its gate is LOCAL or OWNER; leave local acceptance NOT_RUN until actually tested. Parallel Now tasks are allowed when resource locks do not conflict.
- **Up Next:** Only direct successors that are truly **Blocked on tasks currently in Now**. Do not park already Ready tasks or unrelated backlog in Up Next; include multiple successors when applicable.
- **Last Done:** At most the five most recent accepted tasks. Remove older accepted tasks from this focus lane, without changing their Done status. Implementors self-review exact SHA/diff and CI; absent local/owner evidence is never a PASS.

**Corrected provider gates (date-scoped snapshot):** MP-7E is Done. MP-7L1 (Claude local Windows, [#96](https://github.com/KaisAbiyyi/mind-place/issues/96)) and MP-7L2 (Gemini local Windows, [#97](https://github.com/KaisAbiyyi/mind-place/issues/97)) are independent Ready candidates in Now. Their direct owner-acceptance successors MP-7U1 ([#98](https://github.com/KaisAbiyyi/mind-place/issues/98)) and MP-7U2 ([#99](https://github.com/KaisAbiyyi/mind-place/issues/99)) are Blocked / Up Next. Recheck live Notion for subsequent changes.

## Owner decision · functional first, UI overhaul deferred (2026-10-24)

The current Phase 8 navigation shell is provisional. **Core functional development must continue despite known visual debt.** Owner-observed archive sidebar compression at actual Windows scaling 150%/175% is recorded at [#115](https://github.com/KaisAbiyyi/mind-place/issues/115) and explicitly **waived as a blocker** for MP-8L1 functional progression. [Draft CSS mitigation #116](https://github.com/KaisAbiyyi/mind-place/pull/116) is optional/held for UX strategy; its separate Windows DPI acceptance is not required before other eligible Cloud/backend/Phase 9/performance tasks proceed. Document the limitation truthfully, do not label the high-DPI view a visual PASS.

**MP-8L1 remains a real LOCAL functional gate**, covering proper installed/packaged Windows SHA, startup/lock/unlock, permissions, authorized archive access and search/provenance, restart/persistence and install/migration safety. It is not automatically Done due to the owner's UX waiver; missing functional proof remains PARTIAL/NOT_RUN. Downstream acceptance tasks must distinguish operational failures from cosmetic debt. Owner UX acceptance and whole-shell redesign can be recorded separately without holding independent development lanes.

**Future UI research [MP-UXR-01 / #117](https://github.com/KaisAbiyyi/mind-place/issues/117)** belongs in Backlog, unclaimed and without a functionality prerequisite: investigate a fixed, non-expandable icon-only primary activity rail and per-workspace expandable contextual secondary sidebar (Conversations -> archived conversations/search; other workspaces get their own contextual panes). Evaluate alternatives, high DPI and accessibility, migration risk, virtualized lists and memory/startup budgets before choosing and implementing anything. It is intentionally outside Now/Up Next until actionable by owner priority and prerequisites.

## How to request work

Say `ambil MP-4E` (or `ambil 4E` in Mind Place context). The agent first **checks latest GitHub task claims/PRs, the current roadmap and dependencies**, then declares `CLAIM MP-4E` with unique session ID, resource locks, branch and base SHA before editing. If claimed by another agent, do **REPORT_ONLY** or request fenced transfer; do not silently duplicate the work. Descriptive unit IDs are stable, vendor-neutral, and independent of ChatGPT/Codex/Gemini identity. GitHub comments alone are not atomic claims: serialize conflicting units without a real coordinator.

Historical Cloud 1A–1E, 2A–2D, 3A–3F are merged (3E #14: `5546504`; 3F #17: `f9d1313`). MP-3F's former Codex-local claim is complete; do not reimplement it. `3L1/3L2/3U` remain mandatory and are not proved by CI. Rows below are *eligible work descriptions*, not assertions that their dependencies have passed. `LOCAL` and `HUMAN` cannot be taken as generic cloud implementations.

## Task dependency registry (all remaining named units)

| Task ID | Deliverable | Required predecessors | Exclusive logical resource | Gate / parallel note |
|---|---|---|---|---|
| `MP-LIC-00` | **P0: rights-cleared AGPLv3 → MIT licensing and release notices** | Independent; legal rights audit must PASS before license switch | `license-governance/release-notices` | CLOUD; NOW/Claimed ChatGPT, dedicated licensing branch; parallel with MP-4F; MIT release blocked until cleared; [#33](https://github.com/KaisAbiyyi/mind-place/issues/33) |
| `MP-3E` | Cloud MCP adversarial security and simulated tunnel review | `MP-3D` | `mcp-security` | MERGED #14; local/user gates pending |
| `MP-3F` | Bundle verified official Windows helper and remove manual copy | `MP-3E` | `windows-packaging/hosted-helper-resolution/hosted-setup-copy` | DONE; #17 squash-merged `f9d1313`, Windows CI PASS |
| `MP-3L1` | Real Windows local MCP client qualification | `MP-3F` | `native-mcp` | LOCAL; no cloud simulation substitute |
| `MP-3L2` | Official hosted tunnel remote proof | `MP-3L1` | `hosted-account` | LOCAL; requires scoped real access |
| `MP-3U` | Actual ChatGPT source continuity and revocation acceptance | `MP-3L2` | `human-acceptance` | HUMAN |
| `MP-4A` | Approved-root registry, virtual namespace and contract | `MP-3U` | `folder-identity/state-schema` | C |
| `MP-4B` | NTFS root authorization and path containment | `MP-4A` | `folder-authority` | C |
| `MP-4C` | Scanner, watchers, rescan and revocation reconciliation | `MP-4B`, `MP-4D` | `folder-scanner` | C |
| `MP-4D` | Versioned document object store and dedup | `MP-4A` | `object-store/state-schema` | C |
| `MP-4E` | Bounded PDF/OOXML/Markdown/source extractors | `MP-4A` | `document-extractors` | C; parallel with 4B/4D after interface freeze |
| `MP-4F` | Unified document retrieval/UI and indexed qualification | `MP-4C`, `MP-4D`, `MP-4E` | `document-integration/retrieval` | C |
| `MP-4L1` | Windows NTFS junction/reparse/races | `MP-4F` | `windows-fs` | LOCAL |
| `MP-4L2` | Representative documents/parser/resource limits | `MP-4F`, `MP-4L1` | `windows-corpus` | LOCAL |
| `MP-4U` | Folder add/search/revoke workflow acceptance | `MP-4L2` | `human-acceptance` | HUMAN |
| `MP-5A` | Evidence-based claim transitions and as-of history | `MP-4U` | `memory-state-schema` | C |
| `MP-5B` | Conflict relations and evidence-aware ranking | `MP-5A` | `memory-retrieval` | C |
| `MP-5C` | Memory create/update/forget with idempotence/authorization | `MP-5A` | `memory-writes` | C; may parallel with 5B after 5A |
| `MP-5D` | History/conflict/provenance UI and deterministic scenarios | `MP-5B`, `MP-5C` | `memory-ui` | C |
| `MP-6A` | Rebuildable graph schema and deterministic edges | `MP-4U` | `graph-index-schema` | C; may parallel with 5A if migrations do not overlap |
| `MP-6B` | Graph neighbors, community aggregation, bounded retrieval | `MP-6A` | `graph-query` | C |
| `MP-6C` | Graph generations/hash invalidation and atomic activation | `MP-6A` | `graph-build` | C; parallel with 6B after 6A |
| `MP-6D` | Scoped graph maintenance capabilities and transactions | `MP-6C` | `graph-authority` | C |
| `MP-6E` | Graphify prompt templates and graph qualification | `MP-6B`, `MP-6D` | `graphify-integration` | C |
| `MP-7A` | Provider-neutral capture contracts and ChatGPT regression | `MP-4U` | `capture-contract` | C; can run beside graph/memory lanes |
| `MP-7B` | Claude capture/import adapter and branches | `MP-7A` | `capture-claude` | C |
| `MP-7C` | Gemini capture/import adapter and revisions | `MP-7A` | `capture-gemini` | C; parallel with 7B |
| `MP-7D` | Generic JSON/HTML/Markdown import adapter | `MP-7A` | `capture-generic` | C; parallel with 7B/7C |
| `MP-7E` | Provider capability matrix, privacy and regression | `MP-7B`, `MP-7C`, `MP-7D` | `capture-integration` | C |
| `MP-7L1` | Real Claude session/device qualification | `MP-7E` | `claude-account` | LOCAL |
| `MP-7U1` | Claude transcript/provider acceptance | `MP-7L1` | `human-acceptance` | HUMAN |
| `MP-7L2` | Real Gemini session/device qualification | `MP-7E` | `gemini-account` | LOCAL |
| `MP-7U2` | Gemini transcript/provider acceptance | `MP-7L2` | `human-acceptance` | HUMAN |
| `MP-8A` | Virtualized Knowledge OS navigation/data architecture | `MP-5D`, `MP-6E`, `MP-7E` | `ui-shell` | C |
| `MP-8B` | Conversation/files/search library and context preview | `MP-8A` | `ui-library` | C |
| `MP-8C` | Memories/projects/people/topics workspace | `MP-8A` | `ui-memory` | C; parallel with 8B |
| `MP-8D` | Retrieval/connector health and inspector UI | `MP-8A` | `ui-health` | C; parallel with 8B/8C |
| `MP-8E` | Hive graph explorer, zoom and timeline | `MP-8A`, `MP-6E` | `ui-hive` | C; parallel with 8B/8C/8D |
| `MP-8F` | Desktop integration, accessiblity and performance | `MP-8B`, `MP-8C`, `MP-8D`, `MP-8E` | `ui-integration` | C |
| `MP-8L1` | Real packaged WebView2 navigation and startup | `MP-8F` | `windows-ui` | LOCAL |
| `MP-8L2` | Large corpus UI/graph responsiveness and memory | `MP-8L1` | `windows-ui-benchmark` | LOCAL |
| `MP-8U1` | Knowledge library daily UX acceptance | `MP-8L1` | `human-acceptance` | HUMAN |
| `MP-8U2` | Hive usability and source drilldown acceptance | `MP-8L2` | `human-acceptance` | HUMAN |
| `MP-9A` | Measure encrypted storage options and backup impacts | `MP-8F` | `security-architecture` | C; research may start early without migration |
| `MP-9B` | Windows key lifecycle, encrypted state/object integration | `MP-9A` | `crypto-migration` | C |
| `MP-9C` | Private scopes, redaction, destructive forget | `MP-9B`, `MP-5C` | `privacy-deletion` | C |
| `MP-9D` | Parsers and connector attack/fuzz hardening | `MP-9B`, `MP-7E` | `privacy-boundaries` | C; parallel with 9C/9E if contracts frozen |
| `MP-9E` | Atomic backup, restore, corruption/integrity | `MP-9B` | `backup` | C |
| `MP-9F` | Portable export/import, schema migration/rebuild | `MP-9C`, `MP-9E` | `portable-migration` | C |
| `MP-9L1` | Native Windows credential store and unlock measurement | `MP-9F` | `windows-keychain` | LOCAL |
| `MP-9L2` | Destructive recovery/backup on isolated copy | `MP-9F` | `windows-recovery` | LOCAL |
| `MP-9U1` | Owner key recovery/lock experience | `MP-9L1` | `human-acceptance` | HUMAN |
| `MP-9U2` | Owner forget/restore approvals | `MP-9L2` | `human-acceptance` | HUMAN |
| `MP-10A` | Gold continuity questions and reproducible benchmark set | `MP-7E` | `benchmark-gold` | C; can prepare while other lanes run |
| `MP-10B` | Retrieval/ranking bakeoff and cost tuning | `MP-10A`, `MP-6E`, `MP-5D` | `benchmark-retrieval` | C |
| `MP-10C` | 1M-chunk/100K-file scale, resource and startup gates | `MP-10A`, `MP-4F` | `benchmark-scale` | C; parallel with 10B |
| `MP-10D` | Fault injection, crash, restore and migration proof | `MP-9F` | `release-recovery` | C; parallel with 10B/C |
| `MP-10E` | Signed immutable candidate, qualified integration and release | `MP-8F`, `MP-9F`, `MP-10B`, `MP-10C`, `MP-10D` | `release` | C |
| `MP-10L1` | Representative hardware performance qualification | `MP-10E` | `windows-performance` | LOCAL |
| `MP-10L2` | Installer/migration/update/rollback qualification | `MP-10E` | `windows-release` | LOCAL |
| `MP-10L3` | Two-client real one-brain end-to-end | `MP-10E` | `windows-e2e` | LOCAL |
| `MP-10U1` | Owner real-project continuity decision | `MP-10L3` | `human-acceptance` | HUMAN |
| `MP-10U2` | Owner final release decision | `MP-10L1`, `MP-10L2`, `MP-10U1` | `human-acceptance` | HUMAN |
| `MP-10S1` | OPTIONAL vector/hybrid benchmark only if metrics justify | `MP-10B` | `optional-semantic` | OPT-IN |
| `MP-10S2` | OPTIONAL Tantivy benchmark only if FTS5 bottleneck | `MP-10C` | `optional-tantivy` | OPT-IN |

## Safe parallel examples (only after predecessor gates PASS)

- After `MP-4A`: separate `MP-4B` (NTFS authorization) and `MP-4E` (bounded extractors). `MP-4D` may join if no state-schema migration lock overlaps; use explicit interface freeze. `MP-4C` waits for both 4B and 4D.
- After `MP-5A`: `MP-5B` and `MP-5C` are separate ranking/read vs write lanes; `MP-6A` graph index schema and `MP-7A` provider-neutral adapter may run in parallel if they do not modify the same schema/authorization definitions.
- After `MP-7A`: `MP-7B`, `MP-7C`, `MP-7D` are independent provider adapters. Do not merge 7E until all are qualified.
- After `MP-8A`: 8B/8C/8D/8E can run in separate UI feature directories only with frozen navigation contracts; 8F is the integration gate.
- After `MP-9B`: 9C (destructive/privacy), 9D (attack validation), and 9E (backup) may run independently with explicit locked security interfaces; 9F waits for 9C/9E.
- Phase 10B, 10C, 10D are separate benchmark/performance/fault lanes after their declared dependencies; 10E waits for all.

## Task output contract

Every unit must conclude with `TASK_ID, CLAIM, OWNER, BASE_SHA, BRANCH, REMOTE_CHECKPOINT_SHA, PR, CHECKS, CHANGED_LOGICAL_RESOURCES, NEXT, LOCAL_REQUIRED, HUMAN_REQUIRED`. Only owner-approved, CI-green pull requests may be manually merged by the repository owner; agent merge, Auto-merge, merge queues, and Brokers are not authorized. If the agent/browser/quota stops before push, report **UNPUSHED_WORK_AT_RISK** and favor resuming its original workspace. Avoid changing a live private archive during cloud tests. No phase is accepted until all planned C/L/U gates for that phase succeed.

Historical tasks are retained in the BIG plan rather than being duplicated as available claims here.

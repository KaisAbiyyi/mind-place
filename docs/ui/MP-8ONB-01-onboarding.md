# MP-8ONB-01 — Setup & Onboarding wizard

**Task:** MP-8ONB-01 · **Repository:** `KaisAbiyyi/mind-place`
**Base:** `main @ 24648c74f3f59e176974d3878049b882478db69b` (MP-UI-01 merged, PR #144)
**Branch:** `work/mp-8onb-01/onboarding` · **Resource lock:** `ui-module/onboarding`
**Execution:** Antigravity local worktree session `antigravity-mp8onb01-20261106-g1`, generation 1.

## What it is

The `setup-onboarding` route now mounts a four-step wizard that follows the approved HTML `page-setup` flow (welcome → vault → optional providers → first result) while keeping the pieces the prototype does not cover: browser-capture setup, permission explanations, skip/resume and failure recovery.

Code: [`src/onboarding-workspace.ts`](../../src/onboarding-workspace.ts), [`src/onboarding-workspace.css`](../../src/onboarding-workspace.css). Reader wiring is limited to route dispatch, lifecycle and two hooks in [`src/reader.ts`](../../src/reader.ts) and first-run detection in [`src/app.ts`](../../src/app.ts). No tokens, shell, route registry, native command or schema changed.

## Steps and the real state behind each

| Step | Requirement | Backend source | What is never claimed |
|---|---|---|---|
| Welcome & architecture | Optional | none (static explanation) | no counts, no activity |
| Secure vault | Required | `get_security_status` (`configured`, `locked`, `vault_mode`) | `legacy` mode is shown as *unlock secret only — archive files are not encrypted at rest*; encryption is only stated when `vault_mode === "encrypted"` |
| Capture & AI providers | Optional | `get_setup`, `register_extension`, `get_local_mcp_status` | registration is *configuration saved, connection untested*; a running MCP listener is *no client verified*; Claude/Gemini are capture adapters, not provider connections |
| First useful result | Required | `list_conversations`, `search_archive_index`, `read_conversation` | success requires a real read whose conversation ID (and snapshot ID when selected) matches and that contains messages |

The lock screen remains the only place that collects or shows the unlock secret and the offline recovery code. The wizard never asks for, receives, stores or logs either.

## Entry points

- Sidebar **Setup & Onboarding** opens the wizard (previously the capture dialog).
- The session that created the unlock secret (`get_security_status.configured === false` at the gate) opens it once after the archive view is ready, unless a finished record exists.
- The wizard's **Open full capture settings** opens the existing detailed dialog (import, legacy import, folder roots, MCP credentials); closing it returns to the wizard.
- **Open AI Providers & MCP** / **Open Files & Secure Vault** use the existing routes; closing their dialog returns to the saved wizard step.

## Skip, resume, finish

`localStorage["mind-place.onboarding.v1"]` stores only `{version, step, providersSkipped, finishedAt}` and is validated on read (unknown fields are dropped). Storage failure degrades to in-memory progress. Finishing does not claim a verified result; the finish control says so when no real read happened. A finished wizard is not forced open again but remains reviewable.

## Failure and recovery

- Locked or expired session → locked state panel with unlock instruction; Check again.
- Backend failure → generic message (no paths or raw errors), Check again, nothing changed.
- Browser registration failure → previous state kept, button re-enabled, pointer to full capture settings.
- Empty archive → honest empty state with capture/import path; Finish is allowed but flagged unverified.
- Mismatched source, empty transcript, search failure → never counted as success.
- Route change, Lock or reader destroy unmounts the wizard, clears query/excerpt and fences late responses (generation counters).

## Accessibility and layout

Stepper is a list of real buttons with `aria-current="step"`; each step heading takes focus on navigation and focus is restored to the same control after a status refresh; notices use `role=status`/`role=alert`. Layout uses MP-UI-01 tokens (so both themes and density apply) with 1120 px and 780 px breakpoints; the stepper becomes a horizontal strip on narrow windows.

## Verification

- `test/onboarding-workspace.test.ts` (16 tests) and `test/onboarding-integration.test.ts` (7 tests) cover the table above, stale-response fencing, destroy/lock, first-run vs returning, dialog/route round-trips and inert rendering of hostile text.
- Existing `reader.test.ts` and `manual-lock.test.ts` were updated only to reach the capture dialog through the wizard.
- Cloud/local synthetic checks only. **Not run:** HTML pixel comparison in both themes and narrow windows by the owner (UI-U1), installed Windows/WebView2, real browser registration, real provider connection, native Rust tests, hosted exact-head CI.

## Known limits

- `list_conversations` is not paginated; the wizard reads it only on the final step and uses only the count and newest entry.
- The prototype's `Vault Identifier`, `AES-256` badges and `128 transcripts / 33 clusters` numbers are demo data and are deliberately not reproduced.
- The wizard does not issue MCP credentials or start the tunnel; those flows (which handle secrets) stay in AI Providers & MCP.

# MP-UI2 — Human-first UI redesign: migration plan (DRAFT for owner approval)

Status: **draft, not claimed, no task created, no code changed.** Written 2026-11-11 after the owner review of the UI-09 preview (MP-UI-U1 = Changes Required, 4/10).

Inputs: owner feedback (`MP-UI-U1-owner-feedback.md`), the clickable wireframe (published as an artifact, v1 with the owner's answers), the UI-09 feature coverage matrix, and a read-only look at the frozen source `77abfa45`.

Binding companion: [UI-PRINCIPLES.md](UI-PRINCIPLES.md) (the "UI soul", pattern catalog and gap protocol for capabilities the guide does not show). In-repo ground truth: `evidence/mp-ui2/wireframe-v1.html` with `manifest.json`.

Scope: redesign and re-implement the presentation layer **on the current stack (Tauri + TypeScript, WebView2)**. A native-UI port (GPUI) is explicitly **out of scope** and deferred; see "Deferred decision" at the end.

## 1. Decisions already made by the owner

1. Setup merges into Home and **hides itself when finished**.
2. The graph page gets a new, evocative name (working name **Constellation**; alternatives in the wireframe: Atlas, Mindscape, Synapse, Hive).
3. Search must reach **memory and chats** (and files). Memory results come first.
4. **EN + ID from the start**, architecture for more languages.
5. Saving options (automatic save, browser registration) live in **Connections**.
6. Memory offers **both list and cards**.
7. Navigation order is **memory first, then chats** (chats are the messy raw place).
8. Sources gets a **native folder picker and file picker**.
9. A **breadcrumb** appears in the top bar because of nested links.
10. Sidebar toggle lives in the **top bar**; the sidebar header slot is the logo only.
11. Chat detail looks like ChatGPT/Claude; metadata sits behind an **Info** button (aside/dialog), grouped.
12. Technical text is hidden by default (Details / technical mode). One type scale and one spacing scale across all pages.
13. The graph is derived **only from curated memory**, renders immediately, has **no seed input**, and shows nothing (an empty state) when no memory exists. Clicking a node shows the curated memory and the raw chats behind it.

## 2. What the plan must not break (carry-over constraints)

- Backend crates and the 49 Tauri commands stay as they are unless a gap below says otherwise. This is a presentation migration.
- Security fences stay: locked by default, unlock epochs, lock destroys the reader and fences late work, recovery ceremony only on the locked gate, no secrets in URLs/logs, CSP, DOMPurify pipeline for transcripts. Redesigned code reuses these paths; it does not reimplement them.
- Every capability in `MP-UI-09-feature-coverage-matrix.md` keeps a reachable home or is listed in a **deviation ledger** with owner sign-off. Nothing disappears silently (for example "Import a transcript" has no home in the wireframe yet).
- Raw evidence is never replaced; AI-written memory is labelled as AI-made; unsupported writes stay disabled or absent, never faked.
- Governance: work arrives by pull request from isolated branches/forks, owner approval and manual merge, exact-head CI, no auto-merge. Board rules (Now / Up Next / Last Done, AND prerequisites, locks) apply.

## 3. Findings from the source that shape the plan

| Finding | Consequence |
| --- | --- |
| The only graph command is `read_archive_hive`, an **archive-only** graph (conversation and revision nodes, seed IDs). | The memory-driven Constellation needs a **new read-only backend contract** (gap G1). |
| Curated memory is created by "the active AI model or the user" through write primitives; the UI is read-only and MCP write authority is not enabled. | Until some write path exists, a real profile may have **zero memories**, so Home, Memory and Constellation would show empty states. The redesign needs a **memory-creation track** to feel alive (R1). |
| Memory proof/evidence IDs are opaque; the coverage matrix says there is no exact resolver to the source archive. | "Chats behind this memory" cannot be done honestly yet (gap G2). |
| No dialog/picker dependency or Tauri capability exists. | A native folder/file picker is new privileged surface (gap G3, needs security review; it must feed the existing Stage → Approve → Index flow, never grant authority by itself). |
| UI is ~13k lines of TS/CSS in 40 files (`reader.ts` 1.8k, `search-workspace.ts` ~1k, `settings-workspace.ts` ~0.7k, `onboarding-workspace.ts` ~0.6k). Strings are inline. | i18n needs an extraction pass; do it before page rewrites to avoid doing every page twice. |
| Tests run on `node --test` + `tsx` + `jsdom`, visual smoke on Playwright, native smoke on WebView2 over CDP (stale selectors, see MP-UI-L1 PARTIAL). | New tests follow the same tooling; the native harness must be updated for the new UI before L1. |

## 4. Backend and product gaps (must be resolved or explicitly scoped out)

| ID | Gap | Needed by | Notes |
| --- | --- | --- | --- |
| G1 | Read-only, bounded **memory graph** query: nodes = curated memories (kind, title, size = number of evidence chats), edges = relations, plus a cheap "memories exist" signal. | Constellation, Home | Could be derived from `list_memory_inspection`/`get_memory_inspection` if relations are exposed; the audit (UI2-01) decides. Keep read-only and bounded like the Hive contract (120 nodes / 240 edges today). |
| G2 | **Memory to source chats** resolution (opaque evidence ID to archive conversation/snapshot). | Memory detail "Chats behind this", Constellation node panel | If it cannot be built now, the UI shows that section only where a mapping exists and hides it otherwise. It must never guess. |
| G3 | **Native folder and file picker** command + minimal capability. | Sources | Returns paths only; selection still goes through Stage, Approve, Index; revoke semantics unchanged. Needs a security review task. |
| G4 | **Language preference** storage. | i18n | Small; frontend storage is probably enough, audit confirms. |
| G5 | **Memory creation path** (product): who creates memories and when. | Whole redesign feels empty without it | Not a UI task. Options: enable MCP write for a trusted local client, or an in-app "curate from chats" flow. Owner decision. |

## 5. Strategy

Build the new UI as a **parallel v2 shell** (`src/ui2/`, own entry, enabled by a dev/owner setting) while the current UI stays the default until cut-over. Reasons: the information architecture changes as a whole (8 pages become 7, Setup merges into Home, Hive becomes Constellation), so page-by-page edits of the old shell would fight each other; `main` stays shippable; the owner can switch between old and new in the same build for comparison. At cut-over (UI2-12) the old modules and the flag are deleted in one reviewed change.

Rejected alternatives: big-bang rewrite on a long branch (unreviewable, merge conflicts with security work) and in-place edits of the old shell (cannot express the new IA without temporary breakage).

Keep view logic separate from Tauri IPC adapters (the existing `ui-contracts.ts` direction). That is also what keeps a future native-UI port possible without redoing contracts.

## 6. Work breakdown and dependencies

Sizes are relative (S/M/L), not time estimates. "Locks" are exclusive logical resources; shared files are serialized.

| ID | Task | Env | Depends on | Locks | Size | Done means |
| --- | --- | --- | --- | --- | --- | --- |
| UI2-00 | **Design lock**: owner finalises the wireframe; feature placement matrix (every existing capability mapped to its new home or listed as deferred); copy glossary EN/ID; performance budgets set by the owner; acceptance criteria per page | Owner + planning | none | owner-ui2-design | S | Signed-off wireframe committed in the repo with its hash, `UI-PRINCIPLES.md` approved, placement matrix, glossary, budgets |
| UI2-01 | **Backend audit** (read-only): for each new screen, required data vs existing commands; confirm or refine G1 to G5; write backend task cards | Cloud | UI2-00 | none (read-only) | S | Gap report; backend tasks created or scoped out |
| UI2-02 | **i18n foundation**: `t()` with EN/ID catalogs, plurals and Intl dates/numbers, locale persistence, a test that fails on missing keys and on raw strings in v2 modules, pseudo-locale layout test | Cloud | UI2-00 | `src/ui2/i18n` | M | EN/ID switch works end to end on a demo page; tests green |
| UI2-03 | **Design system v2**: tokens (one type scale, one spacing scale), light/dark, density; primitives (button, chip, tag, switch, segmented, list row, card, empty state, banner, dialog, aside, breadcrumb, toast); gallery page for visual regression; **UI rules lint** (tokens only, no raw strings, type and spacing scales only, no technical identifiers outside Details) wired into `verify:repo` | Cloud | UI2-00 | `src/ui2/ui-kit`, `scripts/verify-ui-rules.mjs` | M | Gallery renders in light/dark, EN/ID, three widths; axe checks pass; the rules lint fails on a seeded violation |
| UI2-04 | **Shell v2**: top bar (toggle, breadcrumb, search trigger, language, lock), logo-only sidebar slot, 7 destinations, nested router and breadcrumb model, responsive behaviour, focus management; reuses security gate and lock fences | Cloud | UI2-02, UI2-03 | `src/ui2/shell`, route table | L | Navigation, breadcrumbs, lock/unlock, collapse, small windows tested; v2 flag works |
| UI2-05 | **Memory** (list and cards, detail page, tabs Topics/Projects/People, empty states) | Cloud | UI2-04 | `src/ui2/memory` | M | Real data via existing inspection commands; read-only; coverage matrix rows updated |
| UI2-06 | **Chats and chat reader**: chat-style thread, Info aside (grouped), copy/export, partial-capture notice, provider filter, long-transcript behaviour kept; reuses the sanitizer unchanged | Cloud | UI2-04 | `src/ui2/chats` (touches reader logic, high risk) | L | Existing reader tests still pass; new visual and keyboard tests |
| UI2-07 | **Constellation**: memory-only graph, immediate render, empty state, node aside, accessible node list, bounded | Cloud | UI2-04, G1 (and G2 for the chat list) | `src/ui2/constellation` | M | Works on real data or documented empty state; no seed input |
| UI2-08 | **Sources**: add folder / add files via picker, review step, status, remove access | Cloud | UI2-04, G3 | `src/ui2/sources` | M | Stage, Approve, Index flow unchanged; picker cannot grant access on its own |
| UI2-09 | **Connections**: provider capture setup, automatic save, browser registration, AI-access flow, client list grouped (active / revoked), advanced hosted connection collapsed | Cloud | UI2-04 | `src/ui2/connections` | M | Consent and revoke semantics identical to today; no secrets persisted |
| UI2-10 | **Settings**: appearance, language, privacy/lock, encryption/backup, maintenance, recovery (still launched on the locked gate) | Cloud | UI2-04 | `src/ui2/settings` | M | Recovery cancel path and lock fences tested |
| UI2-11 | **Home + onboarding fold-in + Search v2**: memory-first Home, setup checklist that hides when done (keeps "verified first result" logic), search scopes and grouped results, "copy for an AI" context bar, advanced policies behind a disclosure | Cloud | UI2-05, UI2-06 | `src/ui2/home`, `src/ui2/search` | L | First-run and returning-user flows tested; search tests ported |
| UI2-12 | **Integration and cut-over**: coverage matrix and deviation ledger complete, visual gallery (light/dark, EN/ID, widths), a11y, i18n completeness, old UI and flag removed | Cloud | UI2-05..11 | whole `src/` UI (serialized) | M | Full baseline green; ledger accepted by the owner |
| UI2-L0 | **Native harness update** for the new UI (selectors, fixture root creation); not a product change | Cloud | UI2-12 | `scripts/*native*` | M | Harness runs on a fresh main SHA in CI-safe mode |
| UI2-L1 | **Native Windows/WebView2 qualification** on a fresh main SHA: install, DPI 100 to 200 percent, keyboard/focus, all pages, vault/permissions, large synthetic corpus, resource measurements | Local Windows | UI2-L0 | windows-ui-rollout-qualification | M | Per-gate PASS/FAIL/NOT_RUN with SHA and receipts |
| UI2-U1 | **Owner acceptance**, page by page against the signed-off wireframe | Owner | UI2-L1 | owner-ui-rollout-acceptance | S | Explicit ACCEPT or CHANGES REQUIRED |

Backend tasks from the audit (G1 to G4) are separate cards with their own review and tests; G5 is a product decision.

Dependency picture: `00 -> 01`; `00 -> 02, 03 -> 04 -> (05, 06, 07, 08, 09, 10) -> 11 -> 12 -> L0 -> L1 -> U1`. After the shell lands, pages 05 to 10 are parallelizable (separate modules); 07 waits on G1, 08 waits on G3. The shell, the route table and the i18n/ui-kit folders are single-writer.

## 7. Verification baseline for every UI2 task

- Repository baseline: `npm run verify:repo`, `npm run typecheck`, `npm test`, `cargo fmt --all -- --check`, `npm run build` (plus focused tests for the changed module).
- Visual: Playwright gallery in light and dark, EN and ID, three widths, with screenshots attached to the PR.
- Accessibility: automated checks in the gallery and keyboard-only pass notes (focus order, dialog/aside focus return, Escape).
- i18n: no missing keys, no raw user-facing strings in v2 modules, pseudo-locale overflow check.
- Security regressions: existing lock-fence, sanitizer, recovery-cancel and permission tests stay green; any change touching them needs a focused review.
- Coverage: the placement matrix row for every touched capability is updated in the PR.
- Hosted CI never replaces native qualification; UI2-L1 is the native gate.

## 8. Risks and open questions

| ID | Risk or question | Mitigation or owner action |
| --- | --- | --- |
| R1 | Without a memory-creation path (G5) the memory-first UI is empty on real profiles. | Decide the creation path in UI2-00; ship honest empty states; do not fake data. |
| R2 | Memory to chat resolution (G2) may not be buildable soon. | Show "Chats behind this" only where a mapping exists; otherwise hide, never guess. |
| R3 | Native picker is new privileged surface. | Security review task; paths only; same Stage, Approve, Index flow. |
| R4 | Strings extracted late cause rewrite churn. | i18n foundation (UI2-02) before any page work. |
| R5 | `reader.ts` is large and security-critical. | Presentation changes only; sanitizer and asset validation untouched; existing reader tests are the safety net. |
| R6 | Features lost in the merge (8 pages to 7). | Placement matrix in UI2-00 and ledger in UI2-12. |
| R7 | Memory text language (AI-written) and where "Import a transcript" lives. | Owner decisions in UI2-00. |
| R8 | Two UIs coexist temporarily. | Flag-gated v2, single cut-over change, no long-lived duplicate. |

## 9. Deferred decision: native UI (GPUI)

Not part of this plan. The owner has an existing GPUI experiment (`gemini-rust-gpui-test`) with benchmark numbers vs WebView2. The decision should be data-driven: UI2-00 sets performance budgets and UI2-L1 measures cold start, idle memory, long-transcript render and graph interaction on the WebView2 build. If a budget is missed, the GPUI question reopens with that evidence. Known unknowns for that future decision are API stability (pre-1.0), screen-reader support, the native test harness, and long-transcript text selection.

## 10. Immediate next steps

1. Owner finishes wireframe corrections and answers the open questions (R7, G5, Constellation name).
2. Record UI2-00 as the design-lock gate; then create UI2-01..U1 cards on the execution board following the Now / Up Next rules (only direct successors of Now are shown).
3. Cloud agents claim UI2-01 first (read-only audit), then UI2-02 and UI2-03 in parallel.

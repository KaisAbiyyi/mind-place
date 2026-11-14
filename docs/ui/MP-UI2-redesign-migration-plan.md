# MP-UI2 — Human-first UI redesign: migration plan

Status: **PROPOSED FOR OWNER GATE SIGN-OFF (MP-UI2-00 / PR #172; initial human-first direction merged 2026-11-11 in PR #165).** Written after the owner review of the UI-09 preview (`MP-UI-U1 = Changes Required`, 4/10). The 2026-11-11 owner decisions remain approved; the reconciled `MP-UI2-00` gate items in PR #172 become the binding execution contract upon final owner approval and manual merge of PR #172 (`MP-UI2-01..03` remain `Blocked` until then).

Inputs: owner feedback (`MP-UI-U1-owner-feedback.md`), the clickable wireframe (published as an artifact, v1 with the owner's answers), the UI-09 feature coverage matrix, and a read-only look at the frozen source `77abfa45`.

Binding companion: [UI-PRINCIPLES.md](UI-PRINCIPLES.md) (the "UI soul", pattern catalog and gap protocol for capabilities the guide does not show). In-repo ground truth: `evidence/mp-ui2/wireframe-v1.html` (revision 2, 2026-11-11, see `manifest.json`). Feature mapping: [MP-UI2-placement-matrix.md](MP-UI2-placement-matrix.md).

Scope: redesign and re-implement the presentation layer **on the current stack (Tauri + TypeScript, WebView2)**. A native-UI port (GPUI) is explicitly **out of scope** and deferred; see "Deferred decision" at the end.

## 1. Decisions already made by the owner

1. Setup merges into Home and **hides itself when finished**.
2. The graph page is called **Constellation** by default, and **the user can choose another name in Settings** (Atlas, Mindscape, Synapse, Hive offered in the prototype).
3. Search must reach **memory and chats** (and files). Memory results come first.
4. **EN + ID from the start**, architecture for more languages.
5. Saving options (automatic save, browser registration) live in **Connections**.
6. Memory offers **both list and cards**.
7. Navigation order is **memory first, then the archive**. "Chats" is renamed **Archive** (ID: Arsip): the raw, messy place that holds browser-saved chats **and imports**. Import lives in the Archive (ChatGPT export and Gemini/Google Takeout folders/zips supported by current `MP-IMPORT-APP-01` commands; arbitrary transcript files remain deferred per Section 6b and `MP-UI2-placement-matrix.md` §3).
8. Sources gets a **native folder picker and file picker**.
9. A **breadcrumb** appears in the top bar because of nested links.
10. Sidebar toggle lives in the **top bar**; the sidebar header slot is the logo only.
11. Chat detail looks like ChatGPT/Claude; metadata sits behind an **Info** button (aside/dialog), grouped.
12. Technical text is hidden by default (Details / technical mode). One type scale and one spacing scale across all pages.
13. The graph is derived **only from curated memory**, renders immediately, has **no seed input**, and shows nothing (an empty state) when no memory exists. Clicking a node shows the curated memory and the raw chats behind it.
14. The breadcrumb **starts at Home**.
15. Curated memory text follows the app language and can be translated (with a "translated from" note and View original).
16. **Memory may be empty for now.** Memory is created by importing exports first and then asking Codex/Claude Code to curate; empty Constellation and Memory states are acceptable until then.
17. Performance budget given by the owner: Task Manager memory of the whole app (main plus WebView2 processes) above **1 GB is a warning**, above **2 GB is a failure**, even with a large curated memory. Other budgets are proposed in section 6a.

### 1a. Owner sign-off status for MP-UI2-00 (PR #172)

`MP-UI2-00` distinguishes what the owner already decided on 2026-11-11 from the additional execution-contract artifacts proposed in PR #172 for final owner gate sign-off:

| Category | Status | Scope & authority rule |
| --- | --- | --- |
| Items `1..16` above, Item `17` hard memory limits (`> 1 GB` warn, `> 2 GB` fail), and `wireframe-v1.html` revision 2 visual direction | **Approved by owner (2026-11-11)** | Merged initially in PR #165 (`MP-UI-09` owner review notes). Remains authoritative and unchanged. |
| Feature placement matrix & Initial Design Gap Register (`DEV-01..DEV-04`), EN/ID glossary, `manifest.json` wireframe lock metadata, proposed Section 6a targets + `MP-UI2-L1` measurement protocol, and Section 6b per-page acceptance criteria | **Proposed in PR #172 — pending final owner sign-off and manual merge** | Advisory gate candidate only until the owner explicitly approves and merges PR #172. Downstream tasks `MP-UI2-01..03` stay `Blocked` until merge. |

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
| No dialog/picker dependency or Tauri capability exists. | Two separate native picker gaps exist, both security-reviewed privileged surfaces: **`G3`** (Sources folder/file picker for `MP-UI2-08`, returning paths into `Stage → Approve → Index`, never granting authority by itself) and **`G6`** (Archive Import picker for `MP-UI2-06`, which must call backend `grant_selection` inside Rust to mint an opaque 128-bit `selection` token with 10m TTL/cap 4/cleared on Lock without returning raw host paths into an un-minted command contract; existing `inspect_import_source` / `start_import` fail closed with `E_IMPORT_SELECTION` until `G6` lands). |
| UI is ~13k lines of TS/CSS in 40 files (`reader.ts` 1.8k, `search-workspace.ts` ~1k, `settings-workspace.ts` ~0.7k, `onboarding-workspace.ts` ~0.6k). Strings are inline. | i18n needs an extraction pass; do it before page rewrites to avoid doing every page twice. |
| Tests run on `node --test` + `tsx` + `jsdom`, visual smoke on Playwright, native smoke on WebView2 over CDP (stale selectors, see MP-UI-L1 PARTIAL). | New tests follow the same tooling; the native harness must be updated for the new UI before L1. |

## 4. Backend and product gaps (must be resolved or explicitly scoped out)

| ID | Gap | Needed by | Notes |
| --- | --- | --- | --- |
| G1 | Read-only, bounded **memory graph** query: nodes = curated memories (kind, title, size = number of evidence chats), edges = relations, plus a cheap "memories exist" signal. | Constellation, Home | Could be derived from `list_memory_inspection`/`get_memory_inspection` if relations are exposed; the audit (UI2-01) decides. Keep read-only and bounded like the Hive contract (120 nodes / 240 edges today). |
| G2 | **Memory to source chats** resolution (opaque evidence ID to archive conversation/snapshot). | Memory detail "Chats behind this", Constellation node panel | If it cannot be built now, the UI shows that section only where a mapping exists and hides it otherwise. It must never guess. |
| G3 | **Native Sources folder/file picker** command + minimal capability. | Sources (`MP-UI2-08`) | Returns paths into the existing `Stage → Approve → Index` flow; revoke semantics unchanged; needs a security review task. Separate from Archive Import's `G6` selection-grant picker. |
| G4 | **Language preference** storage. | i18n | Small; frontend storage is probably enough, audit confirms. |
| G5 | **Memory creation path** (product). **Owner decision 2026-11-11:** import ChatGPT/Gemini exports first, then Codex/Claude Code create curated memory; empty states are fine meanwhile. | Whole redesign feels empty without it | Chain: (1) backend import core and owner commands for `chatgpt` and `gemini` folders/zips on `legacy_archive` plaintext vaults landed on `main` (PRs #166, #167, #168, #169, #170 at `1212990`), still requiring `G6` before interactive UI use and not supporting arbitrary transcript files or encrypted vaults; (2) scoped MCP write authority (MP-7MCP-01, PR #153, Review/HOLD on the board) remains separate. Neither is a UI task; both gate real memory. |
| G6 | **Native Archive Import selection-grant picker** (`grant_selection` command wrapper + minimal dialog capability). | Archive (`Import`, `MP-UI2-06`) | Security-reviewed Tauri command inside `src-tauri/src/import_commands.rs` that opens the native folder/zip picker, calls `grant_selection(&path)`, and returns only an opaque 128-bit `selection` token (10m TTL, max 4 active grants, cleared on Lock). Existing `inspect_import_source` and `start_import` fail closed with `E_IMPORT_SELECTION` until `G6` lands. Supports `chatgpt` and `gemini` on plaintext `legacy_archive` vaults only; does **not** add arbitrary transcript-file import. |

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
| UI2-06 | **Archive and chat reader** (formerly Chats), with the **Import** entry: chat-style thread, Info aside (grouped), copy/export, partial-capture notice, provider filter, long-transcript behaviour kept; reuses the sanitizer unchanged; adds the security-reviewed `G6` native dialog `grant_selection` picker command in `src-tauri/src/import_commands.rs` (`chatgpt` / `gemini` folders/zips on `legacy_archive` plaintext vaults only; arbitrary transcript-file import unsupported) | Cloud | UI2-04 | `src/ui2/archive`, `src-tauri/src/import_commands.rs` (+ dialog capability; security review required) | L | Existing reader tests + Rust import command fail-closed/grant tests pass; new visual, keyboard, and encrypted/unsupported-path state tests |
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

### 6a. Performance limits and proposed targets (reconciled for MP-UI2-00; measured in UI2-L1)

Measured as Task Manager **Memory** across the entire application process tree (`mind-place.exe` plus all child `msedgewebview2.exe` processes; raw receipts also record `Private Bytes` for reproducibility). Reference from the owner's earlier experiment: WebView2 idle about 392 MB across 6 processes, about 525 MB peak in a 1,000-node graph test.

| Metric | Threshold / target | Authority status | Measurable acceptance criteria |
| --- | --- | --- | --- |
| Whole-app memory warning | **> 1,024 MB (`1 GB`) -> WARN** (investigate) | **Owner-approved limit (2026-11-11)** ("more than 1 GB is already crazy") | Measured peak Working Set sum across `mind-place.exe` + child `msedgewebview2.exe` during any `MP-UI2-L1` scenario; exceeding `1,024 MB` records a warning requiring triage before release sign-off. |
| Whole-app memory hard failure | **> 2,048 MB (`2 GB`) -> HARD FAIL** | **Owner-approved limit (2026-11-11)** | Exceeding `2,048 MB` peak Working Set sum in any `MP-UI2-L1` scenario (including large synthetic corpus, many curated memories, and a `10,000`-message open transcript) fails the gate automatically. |
| Idle memory (unlocked, Home) | `<= 500 MB` target | **Proposed target (PR #172 — pending owner sign-off)** | Median across `N = 5` idle steady-state samples taken 5 s after Home unlock with 0 background import/index jobs running. |
| Cold start to lock screen | `<= 2,000 ms` (`p95`) | **Proposed target (PR #172 — pending owner sign-off)** | `p95` elapsed wall-clock ms from process launch to lock-screen interactive first paint across `N = 5` packaged cold starts. |
| Long transcript render & scroll | `<= 1,000 ms` (`p95`) first screen; `p95 <= 16.7 ms` scroll frame time, 0 long tasks `> 50 ms` | **Proposed target (PR #172 — pending owner sign-off)** | `10,000`-message fixture chat: time from click/open to first rendered thread viewport, plus 5 s scripted smooth-scroll frame timing (Performance observer / RAF frame deltas). |
| Constellation pan/zoom | `>= 60 FPS` (`p95 <= 16.7 ms`, `p99 <= 25.0 ms`) at `120` nodes / `240` edges | **Proposed target (PR #172 — pending owner sign-off)** | 5 s scripted pan+zoom trajectory on the `120`-node / `240`-edge curated-memory fixture with DevTools closed. |

**Reproducible `MP-UI2-L1` measurement profile:** run on the packaged Windows x64 release build (`npm run build` / `npm run dist:win`), DevTools closed, recording hardware/OS spec, Windows DPI scaling (`100%`, `150%`, and `175%`), `N = 5` measured trials per timing metric after environment stabilization. Failing a **proposed** target triggers optimization triage and owner review of the measured receipt; reopening the native-UI (**GPUI**) port requires an explicit **owner decision** based on measured `MP-UI2-L1` receipts against the owner-approved limits (`> 2,048 MB` hard failure or unresolved `> 1,024 MB` warning / owner-confirmed interactive latency blocker).

### 6b. Per-page acceptance criteria (proposed in MP-UI2-00; binding upon owner merge)

- **Shell & Top Bar (`UI2-04`)**: Logo-only sidebar header; top-bar sidebar toggle, Home-rooted breadcrumb (`Home > ...`), global Search trigger (`Ctrl+K` / `Meta+K`), EN/ID language switch, and Lock button; 7 destinations in exact order (`Home`, `Memory`, `Archive`, `Constellation`, `Sources`, `Connections`, `Settings`); responsive at 1120px / 780px; dev/owner v2 flag switches cleanly without breaking lock fences.
- **Home (`UI2-11`)**: Memory-first layout with real counts (never demo data), recent memories and recent archive chats, and the setup checklist that folds in and hides itself once complete (with "verified first result" rule preserved and re-openable from Settings).
- **Memory (`UI2-05`)**: Both list and card views; Topics, Projects, and People tabs; full Memory detail page with breadcrumb, collapsed History disclosure, Related items, "Chats behind this" (shown only when an exact mapping exists per G2, never guessed), AI-language translation note ("Translated from ...") + "View original", and Technical details collapsed inside the Info aside; honest empty state when zero memories exist.
- **Archive & Chat Reader (`UI2-06`)**: Provider filter chips, paged/virtualized conversation list, prominent **Import** entry, chat-app-style thread (user bubble right, assistant left, DOMPurify + asset validation intact), and grouped **Info** aside holding saved copies (revision selector), copy/export actions, partial-capture warning banner ("Why?" opens Info aside), and collapsed Technical details. The **Import** dialog implements the `MP-IMPORT-APP-01` + `G6` selection-grant contract in `src-tauri/src/import_commands.rs`: supports `"chatgpt"` and `"gemini"` export folders/zips on plaintext `legacy_archive` vaults only; shows a plain encrypted-vault notice when `legacy_archive(&state)` would reject the vault; treats arbitrary transcript-file import as unsupported (disabled `"Transcript file (coming later)"` or omitted per §3 deferral); mints an opaque 128-bit `selection` token via the `G6` native dialog `grant_selection` command (never trusting raw un-minted JS paths; fail-closed `E_IMPORT_SELECTION` remains in force until `G6` lands); runs `inspect_import_source` -> Review (`source_token`, member selection) -> `start_import` with re-auth/unlock confirmation -> `import-progress` phases (`staging`, `verified`, `publishing`, `finishing`) and `cancel_import`; and handles `E_AUTHORIZATION`, `E_IMPORT_SELECTION`, `E_IMPORT_BUSY`, `E_SOURCE_MISMATCH`, and `E_IMPORT_PROVIDER` with clear recovery copy and grant cleanup on Lock.
- **Constellation (`UI2-07`)**: Immediate render from curated memory only (via G1 read-only contract) with no seed input; honest empty state when no curated memory exists; single "Find a topic" field; clicking a node opens the node aside with memory summary and "Chats behind this" (when mapped); accessible keyboard node list; target `>= 60 FPS` (`p95 <= 16.7 ms`) pan/zoom at `120` nodes / `240` edges; page name defaults to Constellation and reflects the user's choice in Settings.
- **Sources (`UI2-08`)**: Approved folders and files list with plain status tags, inline Pause/Resume action, inline two-step "Remove access" confirmation stating "Your files are never deleted.", and single-column stepped Add dialog (Choose via `G3` Sources native picker or path fallback -> Review -> Progress) that feeds the existing `Stage -> Approve -> Index` boundary without granting filesystem authority on picker selection alone (`G3` remains separate from Archive Import's `G6` selection-grant picker).
- **Connections (`UI2-09`)**: Browser capture setup (ChatGPT, Claude, Gemini capability honesty; ChatGPT opt-in automatic save; Chrome/Edge/Brave registration), "Let an AI app read your memory" (local MCP start/stop, one-time access key issue/copy, active/revoked client groups with inline two-step revoke), and collapsed "Advanced: connect hosted ChatGPT" disclosure preserving explicit remote-access consent.
- **Settings (`UI2-10`)**: Grouped Configure cards (max ~7 rows/card) for Appearance (theme, density, technical mode, language EN/ID, map page name), Privacy & Lock, Encryption & Backup (factual vault state, online backup when encrypted/unlocked), Maintenance ("Search is up to date" status tag, single-confirmation rebuild search, technical diagnostics), and Recovery (launches on locked security gate with safe cancel path).
- **Global Search (`UI2-11`)**: Command-palette dialog (`Ctrl+K`) with scope chips (`Everything`, `Memory`, `Archive`, `Files`), grouped results with **Memory first**, "Go to" navigation/action group, "Copy for an AI" context bar, and collapsed "Advanced" disclosure for multi-query/policy presets/token budgets and technical trace.

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

1. PR #172 presents the reconciled `MP-UI2-00` design-gate candidate for final owner review, clearly separating the 2026-11-11 owner-approved direction from the proposed execution-contract additions (`MP-UI2-placement-matrix.md`, `DEV-01..DEV-04`, EN/ID glossary, `manifest.json` lock metadata, proposed Section 6a targets + `MP-UI2-L1` measurement profile, and Section 6b acceptance criteria) and aligning `UI2-06` with `src-tauri/src/import_commands.rs` (`G6` selection-grant contract, plaintext `legacy_archive` limitation, and unsupported transcript-file deferral).
2. `MP-UI2-01`, `MP-UI2-02`, and `MP-UI2-03` remain `Blocked` in `03 · Up Next` until the owner explicitly approves and manually merges PR #172.
3. Once PR #172 is reviewed and manually merged by the owner, promote `MP-UI2-01`, `MP-UI2-02`, and `MP-UI2-03` from `03 · Up Next` to `02 · Now` (`Ready`) so cloud agents can claim them under their disjoint locks.

# MP-UI-09 Integrated Knowledge OS — Full UI Remaster & HTML Fidelity Recovery

**Status summary:**
- **Prior candidate (`c1ce4a586bfdc0520a53248fe9276ad82331808e` / product `1fde14f9a8c1956360cdb5c73bb6ebd641609ae0`):** **CHANGES REQUIRED** (owner rejected visual quality and canonical HTML fidelity: remaining legacy warm brown/gold/beige colors, double header bars, unstyled raw grids, and mismatched component density/hierarchy).
- **Remastered candidate (frozen product source `77abfa45f37d14bf558e235e47b48cf67468fa95`, tree `d2f5664ce280445810e8a9121d0694e6f0b90a2f`):** **READY FOR NATIVE QUALIFICATION & OWNER VISUAL REVIEW**.
- **Windows native qualification (`MP-UI-L1`) and Owner visual/product acceptance (`MP-UI-U1`):** **NOT_RUN** on the remastered candidate (`77abfa45f37d14bf558e235e47b48cf67468fa95`). Formal `MP-UI-09` task status remains **Blocked** until all AND predecessor feature tasks and native/owner gates are accepted.

One [Draft integration PR #159](https://github.com/KaisAbiyyi/mind-place/pull/159) targets `main` from `codex/mp-ui-09-full-integration-20261106`. `main` (`24648c74f3f59e176974d3878049b882478db69b`) and the five source feature PRs (`#156`, `#147`, `#148`, `#157`, `#158`) remain untouched and in the ancestry of this branch.

---

## 1. Verified Sources, Takeover & Ancestry Preservation

Single-writer takeover `Antigravity · mp-ui09-remaster-20261107-g2` was registered on Notion `MP-UI-09` (`40cc4a6aaf73818eacfbe50ea1801e19`) and PR #159 after verifying that prior claim `mp-ui09-20261106-g1` was `RELEASED` and no active writer held `ui-rollout-integration/shared-route-resolution` or shared design token/shell locks.

| Source PR | Exact verified HEAD | Scope preserved |
| --- | --- | --- |
| [#156](https://github.com/KaisAbiyyi/mind-place/pull/156) | `bab547fe6e399decc27b55ece7d95b21383ffc0a` | Overview, Conversations Archive, Curated Memory, AI Providers & MCP (`MP-UI-02`, `MP-UI-03`, `MP-UI-06`) |
| [#147](https://github.com/KaisAbiyyi/mind-place/pull/147) | `3d2e954244fa847ef50c0f8c19aff8c28696052c` | Hive Mind Graph explorer (`MP-UI-04`) |
| [#148](https://github.com/KaisAbiyyi/mind-place/pull/148) | `3816b615c8a97c79130688e3a824056fdf35cd48` | Files & Secure Vault (`MP-UI-05`) |
| [#157](https://github.com/KaisAbiyyi/mind-place/pull/157) | `ec907e490fc124b39e80a1a164fef9ce9135e3a8` | Global Search & Retrieval + Settings & Operational Diagnostics (`MP-UI-07`, `MP-UI-08`) |
| [#158](https://github.com/KaisAbiyyi/mind-place/pull/158) | `e544aea094cd7cf68b02868f1961befa631f3bf2` | First-run Setup & Onboarding wizard (`MP-8ONB-01`) |

---

## 2. Canonical HTML Ground Truth & Token/Component Inventory Mapping

Canonical ground truth: `docs/ui/evidence/mp-ui-09/reference.html` (`182,264` bytes, SHA-256 `b28acf5d50c62bfbaecc6285c0621e9903c04fe73b5a10de6f22948cdcf47579`, matching Notion attachment `mind_place_monochrome_knowledge_os_prototype.html` on page `40cc4a6aaf73814584b3fa7c2e10ae0d`).

### 2.1 Semantic Token System (`src/ui-tokens.css`)
All legacy warm brown/gold/beige/sepia hex literals (`#f4efe6`, `#ece4d7`, `#8b3d1f`, `#6b2b12`, `#2a1e17`, etc.) were purged from `src/style.css`, `src/security.css`, `src/memory-inspector.css`, and `src/memory-entity-lenses.css`. Zero hex literals exist in any `src/*.css` file outside `src/ui-tokens.css`.

| Token Category | Light Mode (`:root`, `[data-theme="light"]`) | Dark Mode (`[data-theme="dark"]`) |
| --- | --- | --- |
| Surfaces (`--bg-app..backdrop`) | `#F4F4F5` (app), `#FFFFFF` (surface), `#FAFAFA` (elevated), `#F4F4F5` (subtle), `#E4E4E7` (muted), `#EFEFF1` (hover), `#E4E4E7` (active), `#09090B` (inverse), `#18181B` (inverse-subtle), `rgba(255,255,255,0.86)` (glass), `rgba(9,9,11,0.42)` (backdrop) | `#09090B` (app), `#111113` (surface), `#16161A` (elevated), `#18181C` (subtle), `#222228` (muted), `#1C1C21` (hover), `#26262C` (active), `#FAFAFA` (inverse), `#E4E4E7` (inverse-subtle), `rgba(17,17,19,0.86)` (glass), `rgba(0,0,0,0.68)` (backdrop) |
| Text (`--text-primary..inverse`) | `#09090B` (primary), `#52525B` (secondary), `#71717A` (muted/tertiary), `#A1A1AA` (faint), `#FAFAFA` (inverse) | `#FAFAFA` (primary), `#A1A1AA` (secondary), `#71717A` (muted/tertiary), `#52525B` (faint), `#09090B` (inverse) |
| Borders (`--border-subtle..focus`) | `#E4E4E7` (subtle/default), `#D4D4D8` (strong), `#09090B` (focus) | `#232329` (subtle/default), `#32323A` (strong), `#FAFAFA` (focus) |
| Shadows (`--shadow-xs..lg`) | `0 1px 2px rgba(9,9,11,0.04)` .. `0 24px 64px rgba(9,9,11,0.16)` | `0 1px 2px rgba(0,0,0,0.35)` .. `0 24px 70px rgba(0,0,0,0.72)` |
| Graph Canvas (`--canvas-*`) | `#FAFAFA` (bg), `#E4E4E7` (grid), `#09090B` (node), `#71717A` (node-sub), `rgba(9,9,11,0.16)` / `0.55` (edges) | `#0C0C0F` (bg), `#1D1D22` (grid), `#FAFAFA` (node), `#A1A1AA` (node-sub), `rgba(250,250,250,0.14)` / `0.62` (edges) |
| Radii & Typography | `6px` (`xs`), `8px` (`sm`), `12px` (`md`), `16px` (`lg`), `999px` (`pill`); `--font-sans` (`Inter`/system sans), `--font-mono` (`JetBrains Mono`/system mono) | Identical |

### 2.2 HTML Element/Component → Production Component → Data Contract → Known Deviation

| Canonical HTML Element / Surface | Production Module & Selector | Backend / State Contract | Intentional Factual Deviation from Prototype Demo |
| --- | --- | --- | --- |
| `#appShell`, `.sidebar` (`256px` / `64px`), `.brand-row`, `.vault-pill`, `.nav-scroll`, `.sidebar-footer` | `src/ui-shell.ts`, `src/ui-shell.css`, `src/knowledge-os.ts`, `src/knowledge-os.css` (`.app-shell`, `.app-sidebar`, `.brand-row`, `.vault-pill`, `#knowledge-os-nav`, `.sidebar-footer`) | `getPreferences()`, `data-route-badge` counts updated from live `list_conversations`, `list_memory_inspection`, `read_archive_hive`, `list_document_roots` | Secondary workspace lenses (`Search all sources`, `Projects`/`People`/`Topics`, `Diagnostics`) remain keyboard-accessible under their parent routes to preserve `MP-UI-01`..`08` contract tests. |
| `.topbar` (`58px`), `.breadcrumbs`, `.command-trigger`, `.topbar-actions` | `src/ui-shell.ts`, `src/ui-shell.css` (`.app-topbar`, `.app-breadcrumb`, `.command-palette-trigger`, `.topbar-actions`, integrated `.reader-toolbar`) | Single 58px topbar; contextual `Copy answer` / `Export Markdown` appear inline when a transcript is open | Replaces the prior candidate's stacked double-header (`app-topbar` + legacy `reader-toolbar`). |
| `page-home` (Overview: `.page-head`, `.status-strip`, `.grid.stats`, `.home-grid`) | `src/reader.ts` (`renderOverviewWorkspace`), `src/conversation-archive.css` (`.ui-overview-page`, `.status-strip`, `.ui-overview-grid`, `.ui-overview-lower`) | `list_conversations` + `fetchHealthSummary` (`get_security_status`, `get_archive_index_stats`, `get_local_mcp_status`, `get_hosted_tunnel_status`) | Displays actual saved conversation/message/completeness counts and epoch-fenced local index/MCP state; never fabricates prototype demo totals (`1,284`) or latency telemetry. |
| `page-conversations` (`.toolbar`, `.table-wrap`, `.tag.outline.mono`, `.status-pill`) | `src/conversation-archive.ts`, `src/conversation-archive.css` (`.archive-workspace`, `.archive-page-head`, `.archive-toolbar`, `.ui-table-scroll`) | `list_conversations` (50-row bounded pagination, provider & completeness filters) | Only actual captured fields (`title`, `provider`, `message_count`, `snapshot_count`, `completeness`, `captured_at`) are shown. |
| `page-conversation-detail` (`.detail-grid`: transcript + `340px` right inspector) | `src/reader.ts`, `src/conversation-archive.css`, `src/style.css` (`.conversation-detail-page`, `.conversation-detail-grid`, `.conversation-detail-aside`) | `read_conversation`, `list_snapshots`, DOMPurify-sanitized Markdown, KaTeX math, validated inline data assets | Preserves immutable snapshot selector, partial-capture warnings, and explicit notice that related memories are not supplied by the archive response. |
| `page-memory`, `page-memory-detail`, `#quickDrawer` | `src/memory-inspector.ts`, `src/memory-inspector.css`, `src/memory-entity-lenses.ts`, `src/memory-entity-lenses.css` | `list_memory_inspection`, `get_memory_inspection`, `investigate_entity_memory` | Collections group visible global claims by disposition (`Active`, `Contested`, `Historical`); write/forget actions remain truthfully disabled or draft-only where native write authority is absent; opaque proof refs offer copy rather than fake source navigation. |
| `page-hive` (`.graph-shell`: radial canvas + `330px` node inspector) | `src/hive-workspace.ts`, `src/hive-workspace.css` (`.hive-workspace`, `.hive-map`, `ellipse.hive-orbit`, `text.hive-node-label`, `.hive-details`) | `read_archive_hive` (bounded 120 nodes / 240 edges, deterministic island layout) | Uses `<ellipse class="hive-orbit">` for orbital rings so `svg circle` remains strictly 1:1 with graph nodes; opens conversation only on exact archive/import identity match. |
| `page-files` (Storage protection banner, 3-step cards, folder list, authorize dialog, provenance inspector) | `src/reader.ts`, `src/document-roots.ts`, `src/files-vault-workspace.css`, `src/workspace-integration.css` | `get_security_status`, `list_document_roots`, `stage_document_root`, `approve_document_root`, `index_document_root`, `search_document_index`, `preview_document_source`, `revoke_document_root` | Reports actual `vault_mode` (`encrypted` vs `legacy`), manual absolute path entry (no native OS folder picker), and session-observed watcher state. |
| `page-providers` (3-column provider cards, MCP transport & browser setup) | `src/reader.ts`, `src/providers-workspace.css`, `src/workspace-integration.css` (`.providers-workspace`, `.providers-grid`, `.provider-card`, `.setup-dialog`) | `get_setup`, `register_extension`, `get_local_mcp_status`, `list_local_mcp_clients`, `get_hosted_tunnel_status` | Explicitly distinguishes browser capture adapter support from MCP client handshakes and write authority. |
| `page-setup` (4-step horizontal stepper + guided step panel) | `src/onboarding-workspace.ts`, `src/onboarding-workspace.css` (`.onboarding-workspace`, `.onboarding-steps`, `.onboarding-step-button`, `.onboarding-panel`) | `get_security_status`, `get_setup`, `get_local_mcp_status`, `list_conversations`, `search_archive_index`, `read_conversation` | Never collects secrets outside the lock screen; requires a live `read_conversation` before marking the first result verified. |
| `page-settings` (`220px` left category nav + right cards) | `src/settings-workspace.ts`, `src/settings-workspace.css`, `src/health-workspace.ts`, `src/health-workspace.css` | `get_security_status`, `get_archive_index_stats`, `rebuild_archive_index`, `create_vault_online_backup`, `inspect_retrieval` | Reuses the locked security gate for recovery; online backup enabled only when `vault_mode === "encrypted"` and unlocked. |
| `#vaultLockOverlay` & `#cmdModal` | `src/security.ts`, `src/security.css`, `src/ui-shell.ts`, `src/ui-shell.css` | `get_security_status`, `unlock_security`, `initialize_security`, `recover_security` | Full-viewport monochrome security gate and command palette dialog with focus trapping and restoration. |

---

## 3. Visual Mismatch Ledger (Prior Candidate vs. Canonical HTML Reference)

| Surface | Canonical Reference (`reference.html`) | Observed Mismatch in Prior Candidate (`c1ce4a5` / `1fde14f`) | Severity | Fix Implemented in `77abfa4` | Evidence (`artifacts/mp8f-visual/ui09/`) | Remaining Difference & Factual Reason |
| --- | --- | --- | --- | --- | --- | --- |
| Global Tokens & Legacy Styles (`src/style.css`, `src/security.css`, `src/memory-*.css`) | Pure monochrome zinc palette (`#09090B`..`#FAFAFA`), `Inter` + `JetBrains Mono` | 48+ hardcoded warm brown/gold/beige/sepia hex colors (`#8b3d1f`, `#f4efe6`, etc.) and `Georgia` serif rules leaked into buttons, transcript, lock screen, and memory inspector | High (Blocker) | Replaced all legacy hex values and serif rules with canonical light/dark semantic tokens in `src/ui-tokens.css` and component stylesheets; removed native blue search cancel icon | `light-1440-expanded-*-integrated.png`, `dark-1440-expanded-*-integrated.png` | None (zero non-zinc hex colors in `src/`) |
| Application Shell & Topbar (`src/ui-shell.ts`, `src/ui-shell.css`) | `256px`/`64px` sidebar with brand mark, vault pill, grouped nav with count badges, footer, and single `58px` topbar | Stacked double header bar (`app-topbar` + full-width `.reader-toolbar`), missing brand SVG/vault pill/footer structure, static placeholder nav badges | High (Blocker) | Rebuilt `.app-sidebar` and `.app-topbar`, integrated `.reader-toolbar` into `.topbar-actions`, and wired live route count badges | `light-1440-expanded-home-integrated.png`, `light-1440-collapsed-home-integrated.png` | Contextual secondary nav links remain accessible under parent routes for `MP-UI-01..08` contract compatibility |
| Overview (`page-home`) | Overline, header + CTAs, `.status-strip`, 4 metric cards, 3-column lower grid (`1.35fr 1fr 0.95fr`) | Missing overline and `.status-strip`, flat action buttons without directional arrows, unaligned lower cards | High | Added `.overline`, `.status-strip`, 4 `.ui-stat-card` cards, and 3-column `.home-grid` with directional quick-access rows and monospace runtime health box | `light-1440-expanded-home-integrated.png`, `dark-1440-expanded-home-integrated.png` | Shows real archive counts and observed local health instead of prototype demo numbers |
| Conversations Archive & Detail (`page-conversations`, `page-conversation-detail`) | Header with right-aligned primary setup CTA, filter bar, table with `.tag.outline.mono` provider pills, and 2-column detail grid (`1fr 340px`) | Secondary setup button, unstyled provider text, full-width single-column transcript without right provenance rail | High | Styled `.archive-page-head`, primary setup button, `.tag.outline.mono` pills, and 2-column `.conversation-detail-grid` with sticky `340px` provenance/metadata card | `light-1440-expanded-conversations-integrated.png`, `light-1440-transcript-integrated.png` | Displays real capture/revision metadata and honest related-memory boundary |
| Curated Memory (`page-memory`, `page-memory-detail`) | Segmented view controls, 3-column collection cards with count pills, 2-column list + `380px` detail inspector, monochrome Quick Peek drawer | Warm brown/terracotta borders and backgrounds (`#d9cfc0`, `#8b3d1f`), unstyled collection buttons and lens forms | High (Blocker) | Purged all 21 warm hex colors in `memory-inspector.css` and `memory-entity-lenses.css`; styled segmented controls, collection cards, 2-column inspector, and drawer | `light-1440-expanded-memory-integrated.png`, `light-1440-memory-detail-integrated.png`, `light-1440-memory-quick-integrated.png` | Groups by native claim disposition; write/forget controls reflect actual backend capabilities |
| Hive Mind Graph (`page-hive`) | Dot-matrix radial canvas with concentric orbital rings, node labels, and side-by-side `330px` right inspector | No orbital rings, no SVG node text labels, `.hive-node-list` pushed `.hive-details` below the SVG instead of side-by-side, tight node radius overlapped labels | High | Added `ellipse.hive-orbit` rings, `text.hive-node-label`, `grid-auto-flow: dense` side-by-side layout (`minmax(0,1fr) 330px`), and wider connected-node radius (`76px`) | `light-1440-expanded-hive-integrated.png`, `dark-1440-expanded-hive-integrated.png` | Bounded to native 120-node/240-edge read-only graph projection |
| Files & Secure Vault (`page-files`) | Storage protection banner with policy pill, 3-column onboarding step cards (`01 Stage`, `02 Approve`, `03 Index`), folder cards, modal dialog | Closed `<dialog class="document-authorize-dialog">` rendered visibly inline due to `display: grid` override; `.document-onboarding-steps` rendered as plain numbered text (`1. 01Stage...`) | High (Blocker) | Added global `dialog:not([open]) { display: none !important; }`; styled `.document-onboarding-steps` as 3-column monochrome step cards | `light-1440-expanded-files-integrated.png`, `light-1440-folder-inspector-integrated.png`, `light-1440-folder-approval-integrated.png` | Uses manual absolute path input because native folder picker is not exposed |
| Setup & Onboarding (`page-setup`) & Settings (`page-settings`) | 4-step horizontal stepper cards (`page-setup`) and `220px` left category tabs + right cards (`page-settings`) | CSS selectors in `onboarding-workspace.css` and `settings-workspace.css` did not match the DOM classes emitted by `HEAD` `.ts` modules, leaving stepper and settings nav unstyled | High (Blocker) | Aligned all exact selectors (`.onboarding-steps ol`, `.onboarding-step-button[aria-current="step"]`, `.settings-sections-nav`, `.settings-section-tab[aria-selected="true"]`, `.settings-status-grid`) | `light-1440-expanded-setup-integrated.png`, `light-1440-expanded-settings-integrated.png` | Every status and action is backed by live native commands |
| Responsive Mobile (`390px`) | Clean compact header + horizontal icon bar without inactive subnav clutter or horizontal overflow | `@media (max-width: 780px)` used `display: flex !important` on `.knowledge-os-nav-secondary-group`, exposing hidden subnav buttons | Medium | Scoped mobile rule to `.knowledge-os-nav-secondary-group:not([hidden])` | `light-390-expanded-conversations-integrated.png`, `light-390-expanded-home-integrated.png` | Zero viewport overflow across all 272 states |

---

## 4. Verification Baseline & Independent Review

### 4.1 Executed Verification Checks (`77abfa45f37d14bf558e235e47b48cf67468fa95`)

| Command / Check | Result | Notes |
| --- | --- | --- |
| `npm run verify:repo` | **PASS** | Repository contract OK — Mind Place `0.2.5`, extension `1.3.1` |
| `npm run typecheck` | **PASS** | `tsc --noEmit` (`0` errors) |
| `npm run test:web` | **PASS** | **261 tests: 261 PASS, 0 FAIL, 0 SKIP** |
| `cargo fmt --all -- --check` | **PASS** | All Rust crates formatted cleanly |
| `cargo check --locked -p mind-place-security -p mind-place-state -p mind-place-index -p mind-place-integration` | **PASS** | Core security, state, index, and integration crates compile cleanly |
| `cargo test --locked -p mind-place-security --lib` | **PASS** | **37 passed, 0 failed** |
| `npm run build:web` | **PASS** | `tsc --noEmit && vite build` succeeded (`dist/assets/index-*.css` `130.88 kB`, `index-*.js` `547.70 kB`) |
| `npm test` / `npm run test:rust` / `npm run build` | **PARTIAL / ENVIRONMENT-BOUNDED** | Full workspace `cargo test --locked` and `tauri build` require `perl` on `PATH` to compile vendored OpenSSL (`openssl-sys` for `reqwest` in `mind-place-integration` / `src-tauri`). Documented in the Windows qualification runbook so the local qualifier installs Perl alongside Rust 1.90.0 and VS C++ Build Tools |
| `node scripts/mp8f-visual-smoke.mjs` | **PASS** | 16 synthetic viewport/keyboard screenshots (`1180x820`, `800x520`, `390x700`, `360x520`), zero horizontal overflow |
| `node scripts/mp-ui09-visual.mjs` | **PASS** | **272 candidate states, 172 A/B pairs**, `findings: []`, `errors: []` across `1440`/`1120`/`780`/`390` × `light`/`dark` × `expanded`/`collapsed` + detail views, drawers, modals, onboarding steps, settings tabs, empty/error/loading/locked states |
| `git diff --check` | **PASS** | Zero whitespace or conflict marker issues |

### 4.2 Independent Read-Only Subagent Review
- **Reviewer**: Independent `research` subagent (`44426f1a-9881-4d67-99f5-07804dd7ad6f`)
- **Inspected Source**: `77abfa45f37d14bf558e235e47b48cf67468fa95` (tree `d2f5664ce280445810e8a9121d0694e6f0b90a2f`)
- **Findings**: **0 P0/P1 findings, 0 actionable P2 findings**. Verified canonical HTML token hierarchy, complete absence of legacy warm colors/gradients/grayscale filters, 8 primary destinations + detail views + modals/drawers, unlock epoch and lock boundaries, inert DOM rendering, truthful runtime states, and preservation of all 5 source PR histories (`#156`, `#147`, `#148`, `#157`, `#158`).
- **Advisory Notice**: Per `AGENTS.md`, AI technical review is read-only and advisory; it is **not** owner approval or security approval.

---

## 5. Remaining Gates & Handoff

- **Cloud implementation & synthetic visual verification (`MP-UI-09` pre-acceptance candidate)**: **COMPLETE** at frozen product source `77abfa45f37d14bf558e235e47b48cf67468fa95` (tree `d2f5664ce280445810e8a9121d0694e6f0b90a2f`).
- **Windows native qualification (`MP-UI-L1`)**: **NOT_RUN** on `77abfa45f37d14bf558e235e47b48cf67468fa95`. Follow [MP-UI-09 Windows preview and owner acceptance handoff](MP-UI-09-owner-acceptance-handoff.md).
- **Owner visual & product acceptance (`MP-UI-U1`)**: **NOT_RUN**. Only the repository owner may record final visual acceptance and perform the manual merge to `main`.

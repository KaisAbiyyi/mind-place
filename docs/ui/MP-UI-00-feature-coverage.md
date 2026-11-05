# MP-UI-00 — Existing-feature coverage and HTML contract mapping

**Task:** MP-UI-00  
**Repository:** `KaisAbiyyi/mind-place`  
**Audited baseline:** `main` @ `efe91155d37d8b1ca0608fe3bd862be311fed004`  
**Claim session:** `mpui00-20261103-0012-wib-chatgpt-cloud`  
**Executor:** ChatGPT Cloud  
**Environment:** Strict cloud-only  
**Branch:** `codex/mp-ui-00-feature-coverage-20261103`  
**GitHub tracking issue:** [#143](https://github.com/KaisAbiyyi/mind-place/issues/143)  
**Notion task:** [MP-UI-00](https://app.notion.com/p/40cc4a6aaf73815891abd5e9dd093b35)  
**Ground truth:** [Mind Place · UI/UX Ground Truth — Monochrome Knowledge OS](https://app.notion.com/p/40cc4a6aaf73814584b3fa7c2e10ae0d)  
**Exclusive resource lock:** `ui-coverage-inventory/docs`

> Documentation/inventory only. No application, shared shell, route registry, runtime, backend, or feature-module code is changed by this task. No local checkout, local files, terminal, owner device, or personal archive was accessed.

## 1. Executive result

The current application is not an eight-route Knowledge OS yet. It is a single Tauri reader shell with ten internal workspace-navigation entries. Those entries are navigation concepts, not URL routes, authorization grants, or proof of feature parity.

The current source already exposes these ten workspace IDs as `available`:

`conversations`, `library`, `memories`, `documents`, `connections`, `projects`, `people`, `topics`, `graph`, and `health`.

Important consequences:

- Do not remove or strand any of the ten existing entries while moving to the eight HTML destinations.
- `projects`, `people`, and `topics` are read-only name-based lenses over global memory hypotheses, not canonical entity registries.
- `graph` is a bounded, read-only archive graph surface, not the full spatial Hive shown in the prototype.
- `health` is a metadata-only local diagnostics surface, not a general provider-health dashboard.
- `documents` and `connections` currently open tabs inside the Settings dialog rather than independent pages.
- `library` is a separate bounded search workspace; there is no global command palette yet.
- The current first-run security gate is not the four-step onboarding wizard in the HTML.
- Current settings are a modal with four tabs; the HTML's dedicated Settings page, theme/density controls, and technical provenance mode are not present in this baseline.

The eight main HTML destinations plus the global palette can absorb the existing functionality without deleting it. The route/module ownership proposed below preserves legacy entry points and assigns each destination to its existing feature lane.

## 2. Canonical HTML evidence and visual contract

The owner-selected source is:

- File: `mind_place_monochrome_knowledge_os_prototype.html`
- Size: **182,264 bytes**
- Lines: **4,684**
- SHA-256: `b28acf5d50c62bfbaecc6285c0621e9903c04fe73b5a10de6f22948cdcf47579`
- Canonical Notion page: [UI/UX Ground Truth](https://app.notion.com/p/40cc4a6aaf73814584b3fa7c2e10ae0d)

This replaces `mind_place_full_app_prototype.html` and the earlier warm-gray/stone direction. The old prototype must not be used as the active design source.

The ground-truth page records the following exact direction:

| Contract | Owner-selected HTML direction |
|---|---|
| Light theme anchors | `--bg-app: #F4F4F5`, `--bg-surface: #FFFFFF`, `--text-primary: #09090B` |
| Dark theme anchors | `--bg-app: #09090B`, `--bg-surface: #121215`, `--text-primary: #FAFAFA` |
| UI typography | Inter / system sans |
| Technical metadata | JetBrains Mono / system monospace |
| Radius scale | 5 / 7 / 10 / 14 px |
| Responsive breakpoints | 1120 px and 780 px |
| Navigation grouping | Knowledge OS / Authority & Infrastructure / System |
| Shell behaviors | Collapsible sidebar, breadcrumbs, global command palette, theme state, technical provenance mode |
| Main destinations | Overview, Conversations, Curated Memory, Hive Mind Graph, Files & Secure Vault, AI Providers & MCP, Setup & Onboarding, Settings |

The complete token set, exact component geometry, and detailed CSS must be extracted from the canonical HTML by the shared UI owner, MP-UI-01. This inventory deliberately does not invent additional token values.

**Attachment-access limitation:** the Notion page exposes the canonical file identity, SHA-256, design contract, page IDs and behavior notes, but the Notion connector returned an embed reference rather than a downloadable file URL. Files Library search surfaced the explicitly superseded `mind_place_full_app_prototype.html`, not the canonical file; that old file was not substituted. MP-UI-01 remains the owner for exact raw-HTML token/component extraction. The source page's full design contract is used for this mapping.

## 3. Source inventory at the audited baseline

All paths below were resolved against the exact audited `main` SHA above.

| Layer | Source inventory | Responsibility / evidence |
|---|---|---|
| Application bootstrap | `src/main.ts`, `src/app.ts` | Tauri invoke bridge, style imports, mount lifecycle, first-run/unlock gate, lock teardown and post-unlock archive refresh |
| Security gate | `src/security.ts`, `src/security.css` | First-run secret creation, unlock, retry/rate-limit messaging and fail-closed startup |
| Main reader shell | `src/reader.ts` | Existing archive shell, conversation list/reader, settings dialog, import/export/copy actions, keyboard behavior and workspace mounting |
| Workspace registry | `src/knowledge-os.ts`, `src/knowledge-os.css` | `WorkspaceId`, ten navigation entries, phase visibility, active state and keyboard navigation |
| Shared data contracts | `src/contracts.ts` | Capture, conversation, snapshot, setup and import result shapes |
| Library search | `src/library-workspace.ts`, `src/library-workspace.css` | Read-only archive/document search, source/version preview and bounded results |
| Curated memory | `src/memory-inspector.ts`, `src/memory-inspector.css` | Global memory hypothesis search, claim detail, revision history, conflict/relation view, evidence references and correction draft |
| Entity lenses | `src/memory-entity-lenses.ts`, `src/memory-entity-lenses.css` | Projects/People/Topics name-based investigation over the same global memory claims |
| Hive graph | `src/hive-workspace.ts`, `src/hive-workspace.css` | Read-only archive graph query, seed selection, bounded rendering, zoom, revision filter and source evidence |
| Approved documents | `src/document-roots.ts`, `src/workspace-integration.css` | Stage/approve/index/pause/revoke roots, document search and verified source preview |
| Connections and MCP | `src/mcp-setup.ts` | Local MCP client lifecycle, credential issuance/revocation and hosted tunnel controls |
| Health diagnostics | `src/health-workspace.ts`, `src/health-workspace.css` | Read-only security/index/MCP/tunnel status and bounded retrieval trace metadata |
| Main visual styles | `src/style.css`, `src/library-workspace.css`, `src/memory-inspector.css`, `src/memory-entity-lenses.css`, `src/hive-workspace.css`, `src/health-workspace.css`, `src/security.css`, `src/workspace-integration.css` | Existing module styling to be reconciled by the shared visual system; not a second design authority |
| Native command boundary | `src-tauri/src/main.rs`, `src-tauri/src/mcp_commands.rs`, `src-tauri/src/hosted_tunnel.rs`, `src-tauri/src/document_refresh.rs` | Registered Tauri commands, owner authority, local MCP and hosted tunnel lifecycle, document refresh |
| Browser capture | `extension/manifest.json`, `extension/popup.mjs`, `extension/worker.mjs`, `extension/automatic.mjs`, `extension/auto-observer.js`, `extension/providers/index.mjs`, `extension/providers/capability-matrix.mjs`, `extension/providers/chatgpt.mjs`, `extension/providers/claude.mjs`, `extension/providers/gemini.mjs` | Supported capture adapters, manual history loading, optional automatic capture, permissions and native host handoff |
| Archive backend | `crates/archive-core/src/store.rs`, `crates/archive-core/src/generic.rs`, `crates/archive-core/src/legacy.rs` | Immutable capture revisions, archive list/read, attachment assets, Markdown export and imports |
| Search/context backend | `crates/index-core/src/retrieval.rs`, `crates/index-core/src/fetch.rs`, `crates/index-core/src/context.rs`, `crates/index-core/src/inspection.rs` | Lexical/exact retrieval, bounded evidence fetch, context packs and inspection traces |
| Graph backend | `crates/index-core/src/graph_query.rs`, `crates/index-core/src/graph.rs`, `crates/index-core/src/graphify.rs`, `crates/index-core/src/graph_generation.rs`, `crates/index-core/src/graph_maintenance.rs` | Scoped graph query and graph projection/generation/maintenance contracts; app-facing exposure is narrower than the core types |
| Memory backend | `crates/state-core/src/memory.rs`, `crates/state-core/src/memory_inspection.rs`, `crates/state-core/src/memory_writes.rs` | Claim/history/evidence read model and a transactional write API; the write API is intentionally not registered as a Tauri or MCP tool |
| MCP backend | `crates/integration-core/src/lib.rs`, `crates/integration-core/src/mcp.rs`, `crates/integration-core/src/transport.rs`, `crates/integration-core/src/clients.rs` | Read-only tool catalog, credential authorization and loopback Streamable HTTP transport |
| Document backend | `crates/document-core/src/lib.rs`, `crates/document-core/src/ooxml.rs`, `crates/document-core/src/pdf.rs`, `crates/state-core/src/document_roots.rs`, `crates/state-core/src/document_scanner.rs` | Approved-root authority, bounded extraction, document index and source revision validation |
| Build / CI evidence | `package.json`, `.github/workflows/ci.yml` | Typecheck, repository guard, web/Rust tests and Windows product build on pull requests |

### Documentation drift found

- `docs/implementation-plan/phase-8a-knowledge-os-ui.md` describes Projects/People/Topics, Hive and Health as planned/non-clickable. At the audited main SHA, `src/knowledge-os.ts` marks all ten workspace entries `available`. The source is newer evidence for actual navigation reachability; the old phase document must not be used as the current route inventory.
- `docs/current-product.md` describes the earlier four-component product and ChatGPT capture path. The audited extension source has statically bundled ChatGPT, Claude and Gemini adapters. Claude and Gemini support explicit manual capture only; they do not thereby become connected MCP providers.
- The above documentation drift is recorded as evidence, not repaired here; this task owns the coverage inventory and does not rewrite unrelated architecture documents.

## 4. Current workspace and action inventory

### Current workspace IDs

| `WorkspaceId` | Current registered label | Actual action / destination | HTML mapping |
|---|---|---|---|
| `conversations` | Conversations | Return to archive list/reader | `page-home`, `page-conversations`, `page-conversation-detail` |
| `library` | Library search | Mount bounded cross-source search | Global palette/results and search/context workspace |
| `memories` | Memories | Mount memory inspector | `page-memory`, `page-collection-detail`, `page-memory-detail` |
| `documents` | Documents | Open Settings → Documents & Permissions tab | `page-files`, authorize-folder modal, folder drawer |
| `connections` | Connections | Open Settings → Connections & MCP tab | `page-providers` |
| `projects` | Projects | Mount Projects entity lens | Curated Memory contextual lens |
| `people` | People | Mount People entity lens | Curated Memory contextual lens |
| `topics` | Topics | Mount Topics entity lens | Curated Memory contextual lens |
| `graph` | Hive graph | Mount read-only archive graph | `page-hive`, canvas and branch inspector |
| `health` | Health | Mount metadata-only health inspector | Overview operations panel and Settings diagnostics |

Every current `WORKSPACES` entry has `phase: "available"`. This controls whether a navigation button is shown; it is not a backend feature flag, a permission grant, or a statement that every planned action is implemented.

The application has no URL-based page router in this baseline. Workspace IDs and action dispatch in `src/reader.ts` are the current navigation mechanism. The Settings dialog is modal state, not a route.

### Existing user-facing actions

| Surface | Reachable actions / interactions |
|---|---|
| Archive shell | Refresh archive; search saved conversations; select conversation; keyboard ArrowUp/ArrowDown/Home/End; slash shortcut focuses archive search; virtualized rows for large lists |
| Conversation reader | Select saved snapshot/revision; read transcript; view partial-capture warnings and capture metadata; render captured attachments through native asset lookup; copy assistant answer or individual message; export Markdown; copy exported Markdown |
| Settings → Archive & Capture | Register Chrome, Edge or Brave native host; import legacy archive; import selected JSON/Markdown/HTML transcript |
| Settings → Documents & Permissions | Stage folder; approve folder; explicitly index folder; pause automatic refresh; resume by indexing again; permanently revoke with two-step 15-second confirmation; search indexed documents; inspect verified source excerpt |
| Settings → Connections & MCP | Start/stop local MCP; issue one-time read-only client credential; copy credential; list clients in pages of up to 50; revoke client; start/stop hosted tunnel with explicit remote-access acknowledgement |
| Library search | Search all/conversations/files; select a result; preview exact conversation snapshot/message or verified document excerpt |
| Memory inspector | Search claims; inspect claim disposition; inspect revision history, conflict/relations and evidence references; view-only pin; copy a correction request draft |
| Projects/People/Topics | Select lens; search by supplied name; inspect matching global memory hypotheses, timelines and recorded source relationships |
| Hive graph | Choose archive seed type and exact internal key; explore bounded graph; select node/edge evidence; adjust zoom; filter evidence by source revision |
| Health inspector | Refresh security/index/local-MCP/hosted-tunnel status; inspect a retrieval query and view metadata-only trace |
| Security gate | First-run secret initialization; unlock; rate-limit/error state; explicit Lock; fail-closed lock retry if native lock is not confirmed |

The Settings dialog currently has four tabs: Archive & Capture, Documents & Permissions, Connections & MCP, and Privacy & Security. Escape closes the dialog; its tabs support arrow/Home/End keyboard navigation. The shell does not yet expose the HTML global palette or a dedicated Overview, Providers, Setup, or Settings page.

## 5. Native command inventory and exposure

The following command groups are registered in `src-tauri/src/main.rs`. Registration means the native bridge exposes the command to the Tauri frontend; it does not mean every command has a current UI control.

| Capability group | Registered native commands | Current UI exposure |
|---|---|---|
| Security | `get_security_status`, `initialize_security`, `unlock_security`, `lock_security` | Security gate and Lock action |
| Archive / reader | `list_conversations`, `read_conversation`, `list_snapshots`, `get_asset`, `export_markdown` | Archive list, transcript, snapshot selection, attachment rendering and export |
| Capture / import / setup | `get_setup`, `register_extension`, `import_legacy`, `import_generic_text` | Settings → Archive & Capture |
| Archive index | `refresh_archive_index`, `rebuild_archive_index`, `get_archive_index_stats` | Refresh and Health; `rebuild_archive_index` has no current reader UI action |
| Memory inspection | `list_memory_inspection`, `get_memory_inspection` | Memory inspector and entity lenses; read-only |
| Documents | `list_document_roots`, `stage_document_root`, `approve_document_root`, `revoke_document_root`, `refresh_document_root_index`, `pause_document_root_refresh`, `search_document_index`, `preview_document_source` | Settings → Documents & Permissions and Library search |
| Hive graph | `read_archive_hive` | Read-only archive graph |
| Retrieval / context | `search_archive_index`, `search_retrieval`, `fetch_retrieval`, `build_retrieval_context`, `inspect_retrieval`, `inspect_retrieval_context`, `inspect_retrieval_fetch` | Library uses `search_archive_index`; Health uses `inspect_retrieval`; the complete context builder/inspector workflow is not yet a user-facing workspace |
| Local MCP | `get_local_mcp_status`, `start_local_mcp`, `stop_local_mcp`, `issue_local_mcp_reader`, `revoke_local_mcp_client`, `list_local_mcp_clients` | Settings → Connections & MCP |
| Hosted tunnel | `get_hosted_tunnel_status`, `start_hosted_tunnel`, `stop_hosted_tunnel` | Settings → Connections & MCP and Health |

The current external MCP tool catalog contains exactly three read-only tools:

- `memory_search`
- `memory_fetch`
- `context_build`

The MCP transport authenticates clients before discovery or tool enumeration and advertises read-only annotations. The local listener is loopback-only and explicitly started after owner unlock. The hosted tunnel is outbound-only, requires explicit remote-access acknowledgement and a runtime API key, and creates a separate read-only client identity. A running helper is not proof of a verified remote connection.

**Memory mutation boundary:** `crates/state-core/src/memory_writes.rs` contains transactional create/supersede/forget operations, but its own module contract explicitly says it is not registered as a Tauri or MCP tool. The current memory UI therefore cannot claim create/update/forget capability. The correction textarea copies a request draft; the pin is view-only. Do not turn either into a production write action without the separately qualified authority contract.

**Graph boundary:** the graph query core understands more node kinds than the current UI. The app-facing `read_archive_hive` command and the frontend validator currently accept only `conversation`, `revision`, and `import` seeds. Graph generation/maintenance is not exposed as a current UI action or Tauri command in this baseline.

## 6. Existing feature → HTML placement → owner → backend readiness → acceptance matrix

| Existing capability | Current source / behavior | HTML placement and migration rule | Owner lane | Backend readiness and actual gate | Required acceptance evidence |
|---|---|---|---|---|---|
| Overview and archive summary | No dedicated Overview page; the current default surface is the archive reader. Do not infer dashboard totals from demo data. | `page-home`; show only verified archive/index status and useful actions. | MP-UI-02; diagnostics data from MP-UI-08 | Summary UI is new; source counts/telemetry must come from real commands. | Empty/new archive, stale index, locked and partial-status states; no sample counts or fake activity. |
| Conversation list and search | `list_conversations`; archive search; virtualized row rendering after threshold. The list loads summaries as a set; native cursor pagination is not implemented. | `page-conversations`; preserve list, filters/search, row selection and empty/loading/error states. | MP-UI-02 | Existing archive backend. Search-all-messages belongs to the indexed Library/retrieval contract, not an assumption about the list API. | Large-list DOM bounds, search, focus/keyboard, empty/error, stale result fencing and exact base/CI evidence. |
| Transcript reader and source identity | `read_conversation`; snapshot selector; capture metadata, source URL, timestamps, completeness warnings and message count. | `page-conversation-detail`; reuse the HTML detail hierarchy and provenance/metadata patterns. | MP-UI-02 | Implemented read path; captures remain immutable revisions. The reader is currently linear with snapshot selection, not a separate branch navigator. Claude's adapter preserves observed branch/regeneration IDs inside message identity; ChatGPT manual capture explicitly warns that other branches may be missing. | Exact selected snapshot, partial warning, source identity, timestamps, attachment rendering, visible observed branch/regeneration evidence where present, and navigation back to the correct list state. |
| Revisions / capture history | `list_snapshots`; immutable archive revisions; selecting a snapshot reads that exact revision. | Conversation detail revision/history selector; preserve older manual and partial automatic revisions. | MP-UI-02 | Existing backend; no revision mutation in UI. | Older revision remains readable; no latest-view overwrite; exact snapshot ID and provenance verified. |
| Copy / Markdown export | `copy-answer`, `copy-message`, `export_markdown`, `copy-markdown`. | Conversation detail actions and overflow/action menu pattern. | MP-UI-02 | Implemented. | Copy only selected assistant/message content; export exact selected snapshot; error and receipt state. |
| Browser capture and native host registration | Extension supports ChatGPT, Claude and Gemini manual loaded-DOM capture; ChatGPT supports explicit manual history loading. Native host registration supports Chrome, Edge and Brave. | Provider setup and onboarding; keep capture controls discoverable from Conversations. | MP-UI-06 + MP-8ONB-01 | Existing capture path. Claude/Gemini are capture adapters, not direct MCP provider connections. | Per-provider capability matrix, registration state, capture partiality/warnings, timeout/cancel and denied/unsupported page states. |
| Automatic capture | Explicit opt-in; optional host permission is only `https://chatgpt.com/*`; observer saves already-loaded ChatGPT messages and never scrolls. | Provider/capture detail and onboarding optional step. | MP-UI-06 + MP-8ONB-01 | Implemented for ChatGPT loaded DOM only. Not available for Claude/Gemini. | Permission request only on opt-in, disable removes observer/permission, no scroll/history, uncertain native acknowledgement is visible. |
| Legacy and generic import | `import_legacy`; `import_generic_text`; generic import accepts JSON, Markdown and HTML, up to 8 MiB, with explicit role labels and no URL fetching. | Archive actions in Conversations and provider/onboarding setup; use the HTML file-picker/modal pattern. | MP-UI-06; archive entry point coordinated with MP-UI-02 | Implemented for supported formats. | Unsupported/empty/oversize/malformed input, raw-source retention, normalized receipt and duplicate import behavior. |
| Global Library search | `library-workspace.ts` searches archive index and authorized document index in parallel; All/Conversations/Files filters; up to 40 matches per index; exact snapshot/message preview or verified document excerpt. | Global palette/results and search workspace; retain `library` as a legacy alias/deep link. | MP-UI-07 | Partial UI exists; bounded search and previews are implemented. No pagination/full-corpus enumeration claim. | Query/filter, 40-result visible limit, partial-source failure, exact snapshot provenance, source revision mismatch and locked/revoked behavior. |
| Retrieval and context builder | Retrieval, fetch, context-build and inspection commands exist; MCP exposes `memory_search`, `memory_fetch`, `context_build`. | Global palette → grouped results → context workspace/drawer → technical inspector. | MP-UI-07 | Backend contracts exist; full user-facing multi-query/context-builder workspace is not present. | Multi-query filters/presets/budgets, progressive fetch/cursor, source diversity, exact revision, truncation reasons and metadata-only traces. |
| Curated memory claims | `list_memory_inspection` and `get_memory_inspection`; global claims are labeled hypotheses, not raw evidence. Scoped/forgotten claims are not listed. | `page-memory`, `page-collection-detail`, `page-memory-detail`, quick drawer. | MP-UI-03 | Read-only inspection is implemented. | Hypothesis labels, evidence references, history, no raw-source access from a reference, locked/empty/error states. |
| Memory history and conflicts | Revision timeline, status transitions, reasons, supersession, evidence and relation/conflict viewer. | Memory detail history/provenance and related-record drawer. | MP-UI-03 | Read-only inspection implemented; writes not exposed. The current UI/command reads the present claim state (`claim_as_of(..., i64::MAX)`); no as-of timestamp selector or argument is exposed. | Exact revision ordering, relation provenance, unresolved/resolved display, explicit absence of as-of controls until supported, and no mutation claim. |
| Memory create/update/forget | Write API exists in `memory_writes.rs` but is intentionally not Tauri/MCP registered. Current correction action only copies a request; pin is view-only. | Keep lifecycle controls in memory detail with disabled/pending factual states until authorized backend path is accepted. | MP-UI-03 | **Not available through current app UI/native bridge/MCP.** Preserve MP-5A/5B/5C/5D contracts; production writes require the accepted memory authority path. The backend Forget operation is a curated-memory non-disclosure tombstone, not physical deletion of source bytes; raw/source/backup erasure belongs to MP-9C. | No optimistic success; no fake pin persistence; create/supersede/forget only after authorized command and adversarial tests; distinguish hiding a claim from erasing original/source/backup bytes. |
| Projects / People / Topics | Read-only name-based lenses over global memory hypotheses using the same memory list/detail commands; explicitly not canonical entity registry or validated identity. | Contextual tabs/filters/collections inside Curated Memory; preserve existing workspace aliases. | MP-UI-03 | Partial read-only lens only. | No synthetic entity registry or identity inference; exact query, claim provenance and history remain visible. |
| Private scopes / redaction / destructive privacy actions | Current memory inspector is global-only and hides scoped/forgotten claims; no user-facing private-scope creation/management or source-redaction action is exposed in the audited UI. | Place controls in the relevant Memory, Files or Provider detail/permission pattern; never add a global privacy toggle that bypasses resource ownership. | MP-UI-03 / MP-UI-05 / MP-UI-06 / MP-UI-08 | Partial backend primitives exist; user-facing scope/redaction actions are not exposed. Production privacy/erasure behavior must follow MP-9C / MP-9E / MP-9F as applicable. | Verify scope isolation, redaction semantics, confirmation, lock/revoke behavior and source/backup erasure separately; do not label a memory tombstone as physical deletion. |
| Hive graph | `read_archive_hive`; frontend accepts conversation/revision/import seeds; zoom and source-revision filter; node/edge source/revision/locator detail. | `page-hive` canvas and branch inspector; extend with evidence-backed clusters/filaments and isolated nodes only when actual graph data supports them. | MP-UI-04 | Bounded read-only archive graph is implemented. Broader graph query/generation/maintenance is not fully surfaced by the app-facing command. MP-6 graph contracts and MP-8HIVE-01 product direction remain required. | Real graph IDs/counts/confidence/provenance, isolated/low-confidence nodes, generation invalidation/rebuild status, bounded rendering, keyboard node-list alternative. |
| Approved document roots | Stage records a pending path without reading; explicit approval; separate Index action; five-minute refresh after indexing; pause/resume; permanent revoke. | `page-files`, authorize-folder modal, folder/document drawer. | MP-UI-05 | Existing MP-4 root/document contracts. | Staging/approval never imply scan; only approved roots indexed; explicit rescan; pause/revoke fences; lock/revoke invalidates search. |
| Document search and preview | `search_document_index` bounded results; preview rechecks live permission and exact source revision; displayed preview capped at 4,096 characters. | Files table/filter and document drawer/source inspector. | MP-UI-05; Library search integration coordinated with MP-UI-07 | Implemented bounded extraction/search/preview. | Supported/unsupported formats, extractor warnings, exact version, 4,096-character cap, permission revoked/source changed/locked states. |
| Files encryption / vault state | Current security gate protects privileged services but explicitly does not claim existing raw archive files are encrypted at rest. | `page-files` vault panel; show only verified protection state. | MP-UI-05 | Production encryption controls additionally require MP-9B acceptance. | Actual crypto/integrity state, lock/revoke fence and recovery behavior; no simulated Secure All or crypto-profile success. |
| Provider and client permissions | Current UI exposes local MCP read-only client credentials and an optional hosted read-only tunnel; it does not implement the prototype's general per-provider read/write scope matrix. | `page-providers`; provider detail, scope summary, lifecycle and audit/status patterns. | MP-UI-06 | Current read-only MCP path exists. Full read/write capability follows MP-7MCP-01..07 as applicable. | Separate capture availability from provider connection/handshake; show actual capability, consent, stop/revoke and denial. No switch may grant unsupported authority. |
| Hosted MCP lifecycle | Local MCP is loopback-only and owner-started; credential issue/list/revoke is read-only. Hosted tunnel requires local MCP, tunnel ID, runtime key and explicit remote acknowledgement. Status reports helper process, not verified connection. | Provider detail and connection-status/audit panel; Stop/revoke remains prominent. | MP-UI-06 | Current read-only path; full provider/write authority remains gated. | Helper-running vs connector-verified distinction; no secret leakage; lock/stop/revoke invalidates remote access. |
| Onboarding | Current first run is `initialize_security` / unlock gate; no guided welcome → vault → optional provider → first result wizard. | `page-setup` stepper with skip/resume, permission explanation and first useful result. | MP-8ONB-01 | Gate exists; guided flow is new. Actual vault encryption/provider connection/first result must use real backend state. | First-run, returning user, skipped optional provider, capture setup, denied permission, locked state, failed first search/read and recovery. |
| Settings / privacy | Current Settings modal has Archive & Capture, Documents & Permissions, Connections & MCP, Privacy & Security tabs. Privacy content is explanatory boundaries; not a full recovery console. | `page-settings`; theme/density/provenance, storage, permission and recovery panels. | MP-UI-08 | Lock/unlock and existing root/client authority commands exist. Theme/density state is MP-UI-01. | Accurate locked/permission/revoked state; action confirmation; no hidden destructive control; normal/technical mode. |
| Backup / restore / key recovery / portability | No corresponding current UI action is registered in the reader. Markdown export and import are not a backup/restore system. | Settings → storage/recovery; Files/Vault state and explicit confirmations. | MP-UI-08; root/vault view in MP-UI-05 | Production actions depend on MP-9C/MP-9E/MP-9F acceptance as applicable. | Crash-safe backup/restore, integrity, key recovery, portable migration, rollback and clear irreversible-action behavior; no simulated success. |
| Health and diagnostics | Health checks security epoch, index generation/source/revision/chunk counts, local MCP process and hosted tunnel status; retrieval inspection reports metadata only, not source text or credentials. | Overview operations panel plus Settings diagnostics drawer/technical mode. Preserve `health` alias. | MP-UI-08 + summary in MP-UI-02 | Read-only diagnostics implemented; coverage is limited to current exposed checks. | Real state only; partial failures, stale generation, locked/epoch-changed, helper-running vs verified; no sample latency or fake connector status. |
| Index rebuild / maintenance | `rebuild_archive_index` is registered but has no current reader action; document indexing has explicit index/pause/resume controls. | Settings diagnostics / maintenance panel with permission-aware action and progress. | MP-UI-08; document-specific controls remain MP-UI-05 | Backend command exists for archive rebuild; production UX/confirmation/progress is absent. | Exact generation transition, failure recovery, lock fencing, no implicit rebuild during search. |

## 7. HTML page map and route/module ownership

The HTML defines eight main destinations. The global command palette is an overlay/action surface, not a ninth navigation destination.

| HTML page / surface | Current app source to absorb | Required preservation / extension | Module owner |
|---|---|---|---|
| `page-home` — Overview | Current default archive shell plus Health metadata | Real activity/status and quick actions only; no prototype counts or fabricated telemetry | MP-UI-02, with health summaries supplied by MP-UI-08 |
| `page-conversations` | `reader.ts`, archive store and snapshot contracts | Existing list/search, virtualization, partial capture, immutable revisions and selection state | MP-UI-02 |
| `page-conversation-detail` | `reader.ts`, transcript sanitizer, attachment asset lookup | Full transcript, exact snapshot/revision, source identity, timestamps, warnings, attachments, copy/export and return navigation | MP-UI-02 |
| `page-memory` | `memory-inspector.ts`, `memory-entity-lenses.ts` | Claim list and contextual Projects/People/Topics lenses; do not imply canonical entities | MP-UI-03 |
| `page-collection-detail` | Current entity lens and memory result/detail model | Collection/taxonomy views only where supported by existing memory contracts; keep legacy lens aliases | MP-UI-03 |
| `page-memory-detail` + quick drawer | Memory history/conflict/evidence detail | Source provenance, revisions, relations and lifecycle state; write controls only after accepted authority capability | MP-UI-03 |
| `page-hive` | `hive-workspace.ts` and graph query core | Canvas, branch inspector, source trace, evidence-backed clusters, isolated nodes and keyboard alternative; no sample graph data | MP-UI-04 |
| `page-files` | `document-roots.ts` and document backend | Approved roots, authorize modal, folder drawer, search, revision/source preview, pause/rescan/revoke and factual vault state | MP-UI-05 |
| `page-providers` | `mcp-setup.ts`, extension popup/worker/adapters, native MCP/tunnel commands | Distinguish browser capture, provider connection, MCP handshake, transport and actual read/write capability | MP-UI-06 |
| `page-setup` | `security.ts`, setup data and capture/provider flows | Welcome → vault → optional providers → first useful result; preserve optionality, permission explanation, skip/resume and failure recovery | MP-8ONB-01 |
| `page-settings` | Current Settings modal and Health inspector | Theme/density/provenance, storage, permissions, recovery and diagnostics; no simulated encryption/recovery actions | MP-UI-08 |
| Global command palette | Current archive slash shortcut and Library search | Ctrl/Meta+K trigger, grouped search/actions, keyboard selection, focus restore and no-results state | MP-UI-01 owns shell trigger/shortcut registration; MP-UI-07 owns palette behavior/results |

### Legacy alias preservation

These mappings are required to avoid silently removing current entry points:

| Existing workspace alias | New destination / behavior |
|---|---|
| `conversations` | Conversations list; retain selected-conversation deep state |
| `library` | Global search/results and context workspace |
| `memories` | Curated Memory |
| `projects`, `people`, `topics` | Curated Memory with the corresponding contextual lens selected |
| `graph` | Hive Mind Graph |
| `documents` | Files & Secure Vault |
| `connections` | AI Providers & MCP |
| `health` | Overview operations summary and/or Settings diagnostics deep link; preserve direct navigation/deep-link behavior |
| Settings dialog state | Settings page |
| First-run security gate | Setup & Onboarding entry; security gate remains authoritative |

No legacy alias should be deleted until the new destination is mounted, permission-checked, and verified. A navigation label being changed is not evidence that the old workflow has been preserved.

## 8. Shared UI contract proposals for MP-UI-01

MP-UI-01 is independently claimed under the separate lock `ui-design-tokens/shared-primitives/app-shell/route-registry`. This task does not edit those shared files.

1. **Single route-registry writer.** MP-UI-01 owns global route registration, page shell, tokens, primitives, breadcrumb, theme/density/provenance state and shell shortcut registration. Feature lanes register only their module route and scoped styles.
2. **Stable HTML route IDs.** Preserve the exact HTML page IDs as stable route identifiers: `home`, `conversations`, `conversation-detail`, `memory`, `collection-detail`, `memory-detail`, `hive`, `files`, `providers`, `setup`, `settings`. The command palette is an overlay.
3. **Legacy workspace aliases.** Route registration must accept the ten current `WorkspaceId` values as aliases and resolve them to the new destinations listed above. Keep lens selection and selected conversation/snapshot state where applicable.
4. **Module contract.** A module registration should declare route ID, owning task, legacy aliases, mount/unmount lifecycle, unlock requirement, data loading state, empty/error/partial/locked/permission-denied states and return/focus behavior. Route selection must never grant permission.
5. **Lock epoch.** Route unmount and lock must destroy module instances, clear sensitive view state and invalidate late async responses. Backend lock/revoke remains authoritative; hiding a page is not a security boundary.
6. **Global palette split.** MP-UI-01 owns the global trigger, OS-appropriate Ctrl/Meta+K shortcut and focus/overlay shell. MP-UI-07 owns search grouping, action catalog, result behavior, context assembly and inspector. Archive and memory lanes own their detail rendering.
7. **Health alias.** Do not add a ninth main navigation destination solely for Health. Surface its verified summary in Overview and detailed diagnostics in Settings; keep the old `health` alias as a deep link.
8. **Search alias.** Do not discard `library`. Route it into the global search/results experience with a compatible source filter and preserved query.
9. **Capability vocabulary.** UI states should distinguish `implemented`, `read-only`, `planned`, `simulated prototype`, `blocked by backend`, `permission denied`, `locked`, `disconnected` and `unavailable`. A switch or card must not grant backend authority.
10. **Normal and technical modes.** Normal mode shows understandable state and next action. Technical provenance mode reveals source IDs, revisions, generation, transport and diagnostic details without exposing credentials or private raw data by default.
11. **Responsive and accessible behavior.** Implement the source breakpoints (1120/780 px), collapsed/expanded sidebar, keyboard focus, focus restore, reduced motion and graph list alternative. Do not copy the prototype's tiny demo type as an accessibility target.
12. **No speculative route promises.** A page may display a disabled/pending action with its real backend prerequisite; it must not fabricate records, progress, counts, provider connection or encryption state.

## 9. Backend gates and task ownership

| Lane | Required shared contract | Existing backend/task contracts to preserve | Additional production gates |
|---|---|---|---|
| MP-UI-02 — Overview/archive/reader | MP-UI-01 | Existing archive/capture/revision contracts | Native/owner rollout remains separate |
| MP-UI-03 — Curated Memory/taxonomy | MP-UI-01 | MP-5A / MP-5B / MP-5C / MP-5D | Authorized create/supersede/forget only when the write-authority path is accepted |
| MP-UI-04 — Hive | MP-UI-01 | MP-6 graph contracts; owner direction MP-8HIVE-01 | Graph generation/invalidation/maintenance must be exposed and qualified before production controls appear |
| MP-UI-05 — Files/Vault | MP-UI-01 | MP-4 root/document contracts | Production encryption/vault actions additionally require MP-9B acceptance |
| MP-UI-06 — Providers/capture/MCP | MP-UI-01 | Existing capture/host/MCP contracts | Full read/write follows MP-7MCP-01..07 as applicable |
| MP-UI-07 — Search/context/inspector | MP-UI-01 palette/results/navigation contract | MP-2 retrieval contracts | Pagination, budgets, source provenance and revocation behavior must be preserved |
| MP-UI-08 — Settings/recovery/diagnostics | MP-UI-01 | Existing security, root and MCP authority | Production privacy/recovery/portability actions depend on MP-9C / MP-9E / MP-9F as applicable |
| MP-8ONB-01 — Onboarding | MP-UI-01 | Existing security gate; coordinate MP-UI-05 and MP-UI-06 | Provider connection and first useful result must be real backend outcomes |
| MP-UI-09 — Integration | Accepted feature-lane contracts | Complete coverage matrix from this task | AND-join; no uncovered capability or unverified feature may pass integration |

MP-UI-00 and MP-UI-01 can run concurrently because their resource locks are disjoint. The feature lanes remain Up Next until the MP-UI-01 shared contract is accepted. MP-UI-09 is not ready for integration merely because this inventory is complete.

## 10. Feature flags and visibility rules

A repository code search for `FEATURE_FLAGS` and `feature_flag` returned no application-level feature-flag registry at the audited baseline.

The visibility/capability mechanisms actually found are:

- `WorkspaceDefinition.phase` in `src/knowledge-os.ts` (`available` / `planned`); all ten current entries are `available`.
- `PROVIDER_CAPABILITIES` in `extension/providers/capability-matrix.mjs`; this is explicitly a read-only projection of shipped adapters and is not an authorization list.
- Browser manifest permissions: `activeTab`, `scripting`, `nativeMessaging`, `storage`; optional host permission only for ChatGPT automatic capture.
- Backend authorization and unlock/permission leases, which remain the real security boundary.

Do not introduce a UI-only boolean as a substitute for these backend checks.

## 11. Verification and handoff checklist

- [x] Exact current `main` SHA recorded.
- [x] Existing MP-UI-00 branch/issue absence checked before claim.
- [x] Dedicated branch and tracking issue created.
- [x] Notion task claimed, then advanced to In Progress with PR/head/CI evidence.
- [x] Current workspace IDs and reachable action surfaces inventoried.
- [x] Registered Tauri commands and external MCP tools inventoried.
- [x] Backend-only, read-only, simulated and missing capabilities distinguished.
- [x] Every HTML destination assigned to a feature owner; legacy aliases mapped.
- [x] Shared route/shell ownership and MP-UI-01 coordination contract documented.
- [x] Documentation drift and canonical HTML attachment access limitation recorded.
- [x] Draft PR opened and exact PR/head recorded in Notion.
- [ ] GitHub PR CI completed on the exact head.
- [x] Self-review of the exact diff and base/head completed; a Markdown escaping issue was corrected before review handoff.
- [ ] MP-UI-01 and feature-lane owners receive the mapping/route proposals.
- [ ] Independent review and owner acceptance remain pending.

### CI / qualification boundary

The repository PR workflow runs the Windows product CI job, including repository guard, TypeScript, web tests, Rust tests, bundled SQLite/MCP/tunnel checks and desktop build. That is cloud GitHub Actions evidence only. This documentation-only task does not claim installed Windows/WebView2, real browser registration, owner-device, visual acceptance, or native qualification.

### Review verdict

**NOT_RUN — implementation inventory is complete as a documentation draft; PR CI, independent review and owner acceptance are pending.** No runtime or product capability is claimed as newly implemented by MP-UI-00.

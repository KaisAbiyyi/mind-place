# MP-UI2-01 — Backend Capability Audit for Mind Place UI2

- **Task ID**: `MP-UI2-01` (read-only audit; zero Rust/backend edits)
- **Baseline SHA**: `fa383eef267a54826e9ad5a2498ddee3a0dbfc06` (`origin/main`)
- **Wireframe Authority**: `docs/ui/evidence/mp-ui2/wireframe-v1.html` (`84,173` bytes, SHA-256 `2b4bbce50a079516e7db2326345cfd0bd227ff7b6a5dd347616fc9c790bc3a8c`)
- **Design & Migration Authorities**: `docs/ui/UI-PRINCIPLES.md`, `docs/ui/MP-UI2-redesign-migration-plan.md`, `docs/ui/MP-UI2-placement-matrix.md`

---

## 1. Executive Summary

A full inspection of `src-tauri/src/main.rs`, `src-tauri/src/mcp_commands.rs`, `src-tauri/src/hosted_tunnel.rs`, `src-tauri/src/vault_commands.rs`, `src-tauri/src/import_commands.rs`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, and `crates/state-core/src/memory*.rs` establishes the exact desktop Tauri command surface available to UI2:

1. **`src-tauri/src/main.rs` registers 52 `#[tauri::command]` handlers** in `tauri::generate_handler![...]` (lines 1467–1520), updating the informal `49` count cited in pre-audit planning notes.
2. **Core Security, Archive, Documents, MCP, and Tunnel flows are backed**, subject to encrypted-vault vs legacy-archive guards (`legacy_archive(&state)?`), owner unlock leases (`begin_unlocked` / `finish_unlocked`), and explicit bounds (`MAX_VISIBLE = 50`, `MAX_HISTORY = 128`, `MAX_RELATIONS = 64`, `max_nodes = 120`, `max_edges = 240`).
3. **Six gaps (`G1`–`G6`) require explicit handling** during UI2 implementation:
   - **`G1` (`MP-UI2-BG-01`)**: No curated-memory taxonomy (projects / topics / people / preferences) or memory constellation graph endpoint exists.
   - **`G2` (`MP-UI2-BG-02`)**: Memory evidence records store opaque `{ source_ref, evidence_kind }` strings; no Tauri command resolves a `source_ref` directly into an archive conversation/excerpt for one-click "Behind this memory" navigation.
   - **`G3` (`MP-UI2-BG-03`)**: No native file/folder dialog crate (`tauri-plugin-dialog` / `rfd`) or picker Tauri command exists to populate `stage_document_root(path)`.
   - **`G4` (Scoped Out)**: App language (`en`/`id`) and UI display preferences (Constellation display name `MAPNAMES`, theme, compact density, reduced motion) remain frontend `localStorage` preferences.
   - **`G5` (Scoped Out)**: Memory write/edit/forget endpoints are deliberately absent from Tauri/MCP (`crates/state-core/src/memory.rs:1-3`); UI2 remains read-only with copyable manual correction text unless Phase 5C explicitly authorizes write endpoints.
   - **`G6` (Backend Subtask of `MP-UI2-06`)**: `import_commands.rs` already implements `grant_selection(path)` (currently `#[cfg_attr(not(test), allow(dead_code))]`), `inspect_import_source`, `start_import`, and `cancel_import`, **but** no Tauri command invokes `grant_selection` in production, and `inspect_import_source`/`start_import` fail closed with `E_IMPORT_SELECTION` and require `legacy_archive(&state)?` (disabled on encrypted vaults).
4. **Additional Wireframe Surface Note (`Automatic save`)**: In `wireframe-v1.html`, the Connections view shows an `Automatic save` switch. In the current codebase, `auto-save` (`automaticSavingEnabled` / `automaticSavingStatus` in `chrome.storage.local`) lives exclusively inside the browser extension (`extension/popup.mjs`) — there is no desktop Tauri command reading or toggling the extension's loaded-DOM auto-save switch. UI2 Connections must present extension setup (`get_setup`, `register_extension`) honestly and never claim desktop control over extension-local `chrome.storage.local` state unless synced IPC is built separately.

---

## 2. Complete Verified Tauri Command Inventory (52 Commands)

All commands are registered in `src-tauri/src/main.rs` lines 1467–1520. Every privileged command checks `authority_ready` + active unlock lease (`begin_unlocked` / `finish_unlocked`) and fails closed when locked.

### 2.1 Security & Vault Lifecycle (10 commands)

| Command | Source | Parameters | Return | Lock / Mode Boundary |
|---|---|---|---|---|
| `get_security_status` | `main.rs:648` | — | `DesktopSecurityStatus` (`SecurityStatus` + `vault_mode`: `"legacy" \| "encrypted"`) | Always readable (content-free status only) |
| `initialize_security` | `main.rs:660` | `secret: String` | `Result<SecurityStatus, String>` | Legacy mode only (`store.is_some()`); rejects encrypted mode with `MP_VAULT_PROVISION_REQUIRED` |
| `unlock_security` | `main.rs:787` | `secret: String` | `Result<SecurityStatus, String>` | Serialized on `owner_unlock_gate`; checks shutdown/supersession |
| `lock_security` | `main.rs:970` | — | `SecurityStatus` | Tears down owner session, stops MCP/tunnel/import/refresh handles |
| `vault_commands::begin_vault_setup` | `vault_commands.rs:16` | `secret: String` | `Result<String, String>` (recovery code display-only) | Encrypted installs without existing vault; first-run ceremony |
| `vault_commands::resume_vault_setup` | `vault_commands.rs:74` | — | `Result<String, String>` | Requires unlocked session + active pending vault ceremony |
| `vault_commands::confirm_vault_setup` | `vault_commands.rs:90` | `code: String` | `Result<(), String>` | Verifies recovery code, provisions vault, then locks (`security.lock()`) |
| `vault_commands::cancel_vault_setup` | `vault_commands.rs:241` | — | `Result<(), String>` | Clears pending ceremony and locks |
| `vault_commands::unlock_vault_with_recovery` | `vault_commands.rs:121` | `secret: String, code: String` | `Result<SecurityStatus, String>` | Encrypted installs; restores vault via recovery secret |
| `vault_commands::create_vault_online_backup` | `vault_commands.rs:203` | `destination: String, generation: u64` | `Result<BackupMetadata, String>` | Encrypted owner only (`generation > 0`, absolute path required) |

### 2.2 Archive, Reader & Import (10 commands)

| Command | Source | Parameters | Return | Lock / Mode Boundary |
|---|---|---|---|---|
| `list_conversations` | `main.rs:1160` | `query: Option<String>` | `Result<Vec<ConversationSummary>, String>` | Supports encrypted vault (`VaultCaptureStore`) and legacy archive |
| `read_conversation` | `main.rs:1173` | `id: String, snapshot_id: Option<String>` | `Result<Conversation, String>` | Supports vault & legacy |
| `list_snapshots` | `main.rs:1192` | `id: String` | `Result<Vec<SnapshotSummary>, String>` | Supports vault & legacy |
| `export_markdown` | `main.rs:1205` | `id: String, snapshot_id: Option<String>` | `Result<ExportResult, String>` | Returns plaintext only in owner IPC response; never writes unencrypted disk export on vault |
| `get_asset` | `main.rs:1229` | `id: String, asset_path: String` | `Result<AssetResult, String>` | Supports vault & legacy |
| `import_legacy` | `main.rs:1246` | — | `Result<ImportReceipt, String>` | Legacy archive only (`legacy_archive(&state)?`) |
| `import_generic_text` | `main.rs:1255` | `format: String, raw: String` | `Result<GenericImportReceipt, String>` | Legacy archive only; accepts text payload, never a filesystem path |
| `import_commands::inspect_import_source` | `import_commands.rs:521` | `provider: String, selection: String` | `Result<SourceInspection, String>` | Legacy only; requires server-side `grant_selection` (`SELECTION_TTL = 600s`, `MAX_SELECTIONS = 4`) |
| `import_commands::start_import` | `import_commands.rs:541` | `provider: String, selection: String, source_token: String, credential: String, members: Option<Vec<String>>` | `Result<ImportJobReceipt, String>` | Legacy only; requires `grant_selection` + re-authentication `credential`; emits `import-progress` |
| `import_commands::cancel_import` | `import_commands.rs:585` | — | `bool` | Requests cancellation of active import slot |

### 2.3 Curated Memory Inspection (2 commands)

| Command | Source | Parameters | Return | Lock / Mode Boundary |
|---|---|---|---|---|
| `list_memory_inspection` | `main.rs:311` | `query: Option<String>` | `Result<Vec<RankedClaim>, String>` | Read-only; `MAX_VISIBLE = 50`; excludes `scope_id.is_some()` & `memory_forget_tombstones` (`memory_inspection.rs:11,46-75`) |
| `get_memory_inspection` | `main.rs:322` | `claim_id: String` | `Result<Option<MemoryInspectionDetail>, String>` | Read-only; `MAX_HISTORY = 128`, `MAX_RELATIONS = 64` (`memory_inspection.rs:12-13,77-154`) |

### 2.4 Archive Index, Retrieval & Hive Graph (11 commands)

| Command | Source | Parameters | Return | Lock / Mode Boundary |
|---|---|---|---|---|
| `refresh_archive_index` | `main.rs:333` | — | `Result<IndexStats, String>` | Supports vault & legacy under `archive_index_write_gate` |
| `rebuild_archive_index` | `main.rs:363` | — | `Result<IndexStats, String>` | Supports vault & legacy under `archive_index_write_gate` |
| `get_archive_index_stats` | `main.rs:470` | — | `Result<IndexStats, String>` | Vault/legacy SQLite index stats |
| `read_archive_hive` | `main.rs:396` | `kind: String, key: String, max_hops: Option<usize>` | `Result<serde_json::Value, String>` | Archive-only (`kind` in `"conversation" \| "revision" \| "import"`, `key` non-empty `<= 4096` bytes, `max_nodes = 120`, `max_edges = 240`) |
| `search_archive_index` | `main.rs:480` | `query: String, include_history: Option<bool>, limit: Option<usize>` | `Result<Vec<SearchHit>, String>` | Default `limit = 20` |
| `search_retrieval` | `main.rs:499` | `request: RetrievalRequest` | `Result<RetrievalResponse, String>` | Vault/legacy index retrieval |
| `fetch_retrieval` | `main.rs:512` | `request: FetchRequest` | `Result<FetchResponse, String>` | Vault/legacy index fetch |
| `build_retrieval_context` | `main.rs:540` | `request: ContextRequest` | `Result<ContextPack, String>` | Vault/legacy bounded context build |
| `inspect_retrieval` | `main.rs:568` | `request: RetrievalRequest` | `Result<SearchInspection, String>` | Retrieval ranking/trace inspection |
| `inspect_retrieval_context` | `main.rs:585` | `request: ContextRequest` | `Result<ContextInspection, String>` | Context pack inspection |
| `inspect_retrieval_fetch` | `main.rs:613` | `request: FetchRequest` | `Result<FetchInspection, String>` | Fetch provenance inspection |

### 2.5 Approved Document Roots / Sources (8 commands)

| Command | Source | Parameters | Return | Lock / Mode Boundary |
|---|---|---|---|---|
| `list_document_roots` | `main.rs:976` | — | `Result<Vec<DocumentRoot>, String>` | Owner-only (`never exposed via MCP`) |
| `stage_document_root` | `main.rs:993` | `path: String` | `Result<DocumentRoot, String>` | Stages pending directory (`1..=4096` chars, no `\0`); no scan/read |
| `approve_document_root` | `main.rs:1016` | `root_id: String` | `Result<DocumentRoot, String>` | Explicit owner grant under `document_authority_gate` |
| `revoke_document_root` | `main.rs:1042` | `root_id: String` | `Result<(), String>` | Terminal revocation tombstone + stops periodic refresh |
| `refresh_document_root_index` | `main.rs:1074` | `root_id: String` | `Result<DocumentRefreshResponse, String>` | Owner-triggered scan + activates periodic refresh |
| `pause_document_root_refresh` | `main.rs:1108` | `root_id: String` | `Result<(), String>` | Pauses periodic refresh without revoking approval |
| `search_document_index` | `main.rs:1117` | `query: String, limit: Option<usize>` | `Result<Vec<DocumentHit>, String>` | Verifies live root grant, content hash & lock lease before disclosing snippet |
| `preview_document_source` | `main.rs:1139` | `uri: String, version_id: String` | `Result<DocumentPreview, String>` | Bounded extracted preview by `(uri, version_id)`, never native path |

### 2.6 Local MCP (6 commands)

| Command | Source | Parameters | Return | Lock / Mode Boundary |
|---|---|---|---|---|
| `mcp_commands::get_local_mcp_status` | `mcp_commands.rs:24` | — | `McpStatus` (`running`, `endpoint`) | Loopback `http://127.0.0.1:43731/mcp`; `running` always `false` if locked |
| `mcp_commands::start_local_mcp` | `mcp_commands.rs:36` | — | `Result<McpStatus, String>` | Vault & legacy supported; read-only local MCP transport |
| `mcp_commands::stop_local_mcp` | `mcp_commands.rs:73` | — | `Result<McpStatus, String>` | Stops tunnel if active, then shuts down local MCP listener |
| `mcp_commands::issue_local_mcp_reader` | `mcp_commands.rs:85` | `display_name: String` | `Result<McpReaderCredential, String>` | Issues single read-only credential (`client_id`, `display_name`, `credential`, `endpoint`) |
| `mcp_commands::list_local_mcp_clients` | `mcp_commands.rs:106` | `after: Option<String>` | `Result<Vec<ClientRecord>, String>` | Paginated list up to 50 clients |
| `mcp_commands::revoke_local_mcp_client` | `mcp_commands.rs:121` | `client_id: String` | `Result<(), String>` | Stops active tunnel if `client_id` owns it, otherwise revokes client |

### 2.7 Hosted Tunnel (3 commands)

| Command | Source | Parameters | Return | Lock / Mode Boundary |
|---|---|---|---|---|
| `hosted_tunnel::get_hosted_tunnel_status` | `hosted_tunnel.rs:324` | — | `HostedTunnelStatus` (`running`, `tunnel_id`, `connection_verified`) | Returns stopped if locked; cleans up dead helper read credential |
| `hosted_tunnel::start_hosted_tunnel` | `hosted_tunnel.rs:348` | `tunnel_id: String, api_key: String, acknowledged_remote_access: bool` | `Result<HostedTunnelStatus, String>` | Requires local MCP running, valid `tunnel_id` + `api_key`, consent flag, and pinned binary SHA-256 verification |
| `hosted_tunnel::stop_hosted_tunnel` | `hosted_tunnel.rs:411` | — | `Result<HostedTunnelStatus, String>` | Stops helper and revokes its ephemeral read client |

### 2.8 Setup & Browser Registration (2 commands)

| Command | Source | Parameters | Return | Lock / Mode Boundary |
|---|---|---|---|---|
| `get_setup` | `main.rs:1298` | — | `Result<SetupInfo, String>` | `archive_path, extension_path, host_path, extension_id, native_host_name, registered_browsers, platform, legacy_available` |
| `register_extension` | `main.rs:1325` | `browser: String` | `Result<SetupInfo, String>` | Validates `"chrome" \| "edge" \| "brave"`; Windows registry registration serialized under `owner_unlock_gate` |

---

## 3. Wireframe-v1 Screen-to-Backend Capability Matrix

Every surface from `docs/ui/evidence/mp-ui2/wireframe-v1.html` and `docs/ui/UI-PRINCIPLES.md` §6 is mapped below. Where the backend does not supply a wireframe field or action, the UI2 page must render **honest availability state** (`Unknown` / empty / unavailable), never fabricated counts or silent fallbacks.

| UI2 Destination / Surface | Wireframe Data & Actions | Existing Tauri Commands / Sources | Gap / Constraint |
|---|---|---|---|
| **Shell / Global Search / Lock** | Unlock modal, Lock button, Recovery ceremony, Global Search bar (`Ctrl+K`) | `get_security_status`, `unlock_security`, `lock_security`, `initialize_security`, `vault_commands::*`, `list_memory_inspection`, `search_archive_index`, `search_document_index` | Covered. Must keep technical diagnostics behind `Details` (`UI-PRINCIPLES.md` §5.12). |
| **1. `Home`** (`vHome`) | Hero prompt, recent conversations (`list_conversations`), summary stat cards (Saved chats, Remembered items, Approved folders), quick actions | `list_conversations`, `list_memory_inspection`, `list_document_roots`, `get_archive_index_stats` | `list_memory_inspection` caps at `MAX_VISIBLE = 50` global claims (not total memory count across all scopes — label count honestly as `50+` or visible count, or use `IndexStats` for archive counts). |
| **2. `Memory`** (`vMemory`) | Category tabs (All / Projects / Topics / People / Preferences / Contested), search, card/list view, Memory detail drawer + "Behind this memory" source chats, language badge + "View original", optional correction request | `list_memory_inspection(query?)`, `get_memory_inspection(claim_id)` | **`G1`** (no project/topic/person/preference taxonomy field on `RankedClaim`; only `disposition` and free-text `query`); **`G2`** (`ClaimEvidence.source_ref` is an opaque ID without resolved chat title/excerpt); **`G5`** (no edit/forget command — copy-only manual correction request remains). |
| **3. `Archive`** (`vArchive`) | Filter chips (All / ChatGPT / Claude / Gemini / Imports / Saved notes), search, conversation rows, Message reader + Revision history + Export Markdown, **Import Dialog** | `list_conversations`, `read_conversation`, `list_snapshots`, `export_markdown`, `get_asset`, `search_archive_index`, `import_generic_text`, `import_legacy`, `import_commands::{inspect_import_source, start_import, cancel_import}` | **`G6`**: `import_generic_text` works for text paste in legacy mode; `inspect_import_source` / `start_import` require a server-side `grant_selection` call (currently `#[cfg(test)]`/unwired to IPC) AND reject encrypted vaults (`legacy_archive(&state)?`). |
| **4. `Constellation`** (`vMap`) | Interactive spatial node graph of memories/concepts, type filter, inspector aside (`Details` + connected items), configurable name (`Constellation` default) | Partial: `read_archive_hive(kind, key, max_hops?)` (archive/revision/import graph only, max 120 nodes / 240 edges); `list_memory_inspection`; `get_memory_inspection` | **`G1` (`MP-UI2-BG-01`)**: `read_archive_hive` requires an archive seed `(kind, key)` and only returns archive/revision/import nodes — it does **not** return a curated-memory graph or global overview without a seed key. Wait for `MP-UI2-BG-01` or render explicit honest fallback until `read_memory_constellation` lands. |
| **5. `Sources`** (`vSources`) | List of staging/approved/revoked folders, Add folder (native browse picker + review step), Approve / Refresh scan / Pause / Remove (`Revoke`), Document preview drawer | `list_document_roots`, `stage_document_root`, `approve_document_root`, `revoke_document_root`, `refresh_document_root_index`, `pause_document_root_refresh`, `search_document_index`, `preview_document_source` | **`G3` (`MP-UI2-BG-03`)**: `stage_document_root(path)` takes a string path, but there is no native OS directory picker command (`pick_document_source`) or `tauri-plugin-dialog` in `Cargo.toml`. |
| **6. `Connections`** (`vConnect`) | Browser capture card (`Chrome / Edge / Brave`, registration, `Automatic save` switch), Local MCP card (Start/Stop, `127.0.0.1:43731/mcp`, issue/list/revoke read-only credentials), Optional Hosted Tunnel card | `get_setup`, `register_extension`, `mcp_commands::*`, `hosted_tunnel::*` | **Honesty constraint**: `Automatic save` in `wireframe-v1.html` corresponds to `extension/popup.mjs` (`chrome.storage.local`), NOT a desktop Tauri setting. Do not show a fake desktop auto-save toggle claiming live browser sync; keep extension popup state explicitly described unless synced storage is added later. Also `audit` feed and `connection_verified` handshake proof remain unavailable per `src/providers-workspace.ts:48`. |
| **7. `Settings`** (`vSettings`) | Language selector (`EN`/`ID`), Constellation page name (`MAPNAMES`), Theme (Light/Dark/System), Compact density, Backup & Recovery / Security lock status | Frontend preferences (`G4`: `localStorage`), `get_security_status`, `lock_security`, `vault_commands::create_vault_online_backup`, `refresh_archive_index`, `rebuild_archive_index`, `get_archive_index_stats` | **`G4`** intentionally frontend-only; no backend settings table required. Note `UI-PRINCIPLES.md` rule: max 7 rows per Settings card; technical diagnostics/paths hidden inside `Details` disclosures. |

---

## 4. Gap Analysis & Backend Follow-Up Decisions

| Gap ID | Summary | Codebase Evidence | Decision / Action |
|---|---|---|---|
| **`G1`** | **Memory Constellation graph & taxonomy** | `crates/state-core/src/memory.rs:53-65` (`ClaimState` has no category/topic/person/preference field); `crates/state-core/src/memory_inspection.rs:11` (`MAX_VISIBLE = 50`); `src-tauri/src/main.rs:396-467` (`read_archive_hive` is archive/revision/import only, requires `(kind, key)` seed). | **Create `MP-UI2-BG-01`**: Add bounded read-only `read_memory_constellation` Tauri command returning classified nodes (`limit <= 120`) + relation edges (`<= 240`) over unlocked visible global claims, preserving tombstone & scope filters. Unblocks full `MP-UI2-07`. |
| **`G2`** | **Memory Evidence -> Archive Chat resolution** | `crates/state-core/src/memory.rs:12-16` (`ClaimEvidence { source_ref, evidence_kind }` opaque strings); `crates/state-core/src/memory_inspection.rs:77-154` does not hydrate conversation metadata or excerpts. | **Create `MP-UI2-BG-02`**: Add bounded read-only `resolve_memory_evidence_chats` command resolving visible claim `source_ref` entries to `(conversation_id, title, updated_at_ms, provider, excerpt)` after unlock lease + tombstone checks. Enhances `MP-UI2-05` & `MP-UI2-07`. |
| **`G3`** | **Native Sources folder/file picker** | `src-tauri/Cargo.toml` has no `tauri-plugin-dialog` / `rfd`; `src-tauri/src/main.rs:993-1012` (`stage_document_root` accepts a manual `path: String`). Web `<input type="file">` cannot disclose local directory paths on desktop WebView2. | **Create `MP-UI2-BG-03`**: Security-reviewed native directory picker command (`pick_document_source`) gated on unlocked owner session, non-UNC validation, minimal permissions, and content-free errors. Unblocks friction-free `MP-UI2-08`. |
| **`G4`** | **App language & display preferences persistence** | `src/ui-shell.ts` stores UI prefs client-side; no backend preference store needed. | **Scoped Out of Backend**: Handled by `MP-UI2-02` (`UI2_LOCALE_STORAGE_KEY`) and `MP-UI2-10` in `localStorage`. Zero backend work. |
| **`G5`** | **Memory creation / edit / forget from UI** | `crates/state-core/src/memory.rs:1-3` explicitly restricts claim writes/forget until Phase 5C capability-checked write endpoints exist; `src/memory-inspector.ts:409-422` supports manual correction copy only. | **Scoped Out of UI2**: Keep Memory read-only with manual correction copy & honest unavailability note; do not introduce unapproved write endpoints. |
| **`G6`** | **File/Zip Import path selection + Encrypted Vault import** | `src-tauri/src/import_commands.rs:521-580` requires prior `grant_selection(path)` (currently `#[cfg_attr(not(test), allow(dead_code))]` with no IPC caller) and `legacy_archive(&state)?` rejects encrypted vaults. | **Subtask Inside `MP-UI2-06` (`MP-UI2-BG-06`)**: Add security-reviewed picker -> `grant_selection` IPC command and clarify/enable vault vs legacy import boundary under `MP-UI2-06`. |

---

## 5. Planned Notion Backend Task Cards (`MP-UI2-BG-01..03`)

All cards live in the shared Notion Execution Board with strict security invariants (unlocked owner lease required inside blocking worker, no MCP tool exposure, bounded output caps, content-free error codes, unchanged CSP):

1. **`MP-UI2-BG-01`** — `UI2-BG-01 · Bounded read-only memory Constellation graph & taxonomy query (G1)`
   - **Prerequisites**: `MP-UI2-01`
   - **Unlocks**: `MP-UI2-07`
   - **Resource Locks**: `crates/state-core/src/memory_inspection.rs; src-tauri/src/main.rs (read_memory_constellation)`
   - **Environment**: `Cloud` (Phase `08`)
2. **`MP-UI2-BG-02`** — `UI2-BG-02 · Exact memory-evidence-to-archive-chat resolver (G2)`
   - **Prerequisites**: `MP-UI2-01`
   - **Unlocks**: `MP-UI2-05`, `MP-UI2-07` (optional enrichment behind "Behind this memory")
   - **Resource Locks**: `crates/state-core/src/memory_inspection.rs; src-tauri/src/main.rs (resolve_memory_evidence_chats)`
   - **Environment**: `Cloud` (Phase `08`)
3. **`MP-UI2-BG-03`** — `UI2-BG-03 · Security-reviewed native Sources folder/file path picker (G3)`
   - **Prerequisites**: `MP-UI2-01`
   - **Unlocks**: `MP-UI2-08`
   - **Resource Locks**: `src-tauri/Cargo.toml; src-tauri/tauri.conf.json; src-tauri/src/main.rs (pick_document_source)`
   - **Environment**: `Cloud` + `Local Windows` interactive dialog smoke verification (Phase `08`)

---

## 6. Security & Non-Modification Confirmation

- Zero files under `crates/`, `src-tauri/`, `src/*.ts`, `src/*.css`, `index.html`, or `extension/` were modified by `MP-UI2-01`.
- `docs/ui/evidence/mp-ui2/wireframe-v1.html` remains unmodified at `84,173` bytes.

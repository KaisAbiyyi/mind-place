# MP-UI2 feature placement matrix

Status: **LOCKED — MP-UI2-00 design contract (2026-11-14; initial draft merged 2026-11-11 in PR #165).** Maps every row of [`MP-UI-09-feature-coverage-matrix.md`](MP-UI-09-feature-coverage-matrix.md) to its home in the human-first UI, using only the pattern classes in [`UI-PRINCIPLES.md`](UI-PRINCIPLES.md). Anything marked **Gap** is not drawn in `wireframe-v1.html` and must follow the Section 4 gap protocol and the Initial Design Gap Register below when implemented.

Destinations: **Home, Memory, Archive, Constellation (name chosen in Settings), Sources, Connections, Settings**, plus the global **Search** (Ctrl K) and **Lock** in the top bar. Breadcrumb always starts at Home.

Legend: **WF** = shown in `wireframe-v1.html`. **Gap** = not shown, needs a ledger entry. **Dev** = deviation from today's UI that needs owner sign-off.

| # | Capability (from the UI-09 matrix) | New home | Pattern class | WF | Notes / decision |
| --- | --- | --- | --- | --- | --- |
| 1 | Real archive counts, recent captures | Home (stat cards, recent) and Archive list | Browse | WF | Memory-first Home; counts are real, never demo. |
| 2 | Runtime health | Settings > Maintenance ("Search is up to date" + tag) | Status | WF | Detailed diagnostics only in technical mode. Dev: Overview health tiles are gone from Home. |
| 3 | Provider archive filter / pagination | Archive: provider chips, paged list | Browse | WF | Keep 50-row pages or virtualization. |
| 4 | Transcript and immutable capture revision | Archive > chat page; **Info** aside > Saved copies | Detail | WF | Revision picker moves into the Info aside. |
| 5 | HTML / Markdown / attachments | Archive > chat page thread | Detail | WF | Sanitizer and asset validation unchanged. |
| 6 | Memory collections, cards, Quick Peek | Memory (list or cards) and memory detail page | Browse, Detail | WF | Dev: the Quick Peek drawer is replaced by a full detail page (and the Constellation node aside). |
| 7 | Memory detail, history, as-of | Memory detail page; "History" as a collapsed disclosure | Detail, Advanced | **Gap** | History/as-of is not drawn yet. |
| 8 | Conflict and related evidence; copy proof reference | Memory detail > Related and Chats behind this; proof reference under Technical details | Detail, Advanced | WF (partly) | Dev: opaque evidence IDs are hidden by default. "Chats behind this" needs G2. |
| 9 | Projects, People, Topics | Memory tabs | Browse | WF | |
| 10 | Memory writes, forget, pin | Not shown; "Suggest a correction (coming later)" disabled | Empty/disabled | WF | Not built. No fake controls. |
| 11 | Hive topology, isolated nodes | Constellation (curated memory only) | Detail (graph) | WF | Dev: archive/revision nodes no longer drawn. Needs G1. |
| 12 | Hive search / layer / origin / revision filters, pan/zoom/branch/back/reset | Constellation: one "Find a topic" field; the rest removed from the default view | Search, Advanced | **Gap** | Dev: layer/origin/revision controls removed. Owner to confirm. |
| 13 | Hive to source transcript | Constellation node aside > Chats behind this | Detail | WF | Only where an exact mapping exists (G2). |
| 14 | Approved document roots | Sources > Folders; Add folder dialog | Add, Browse | WF | Stage, Approve, Index flow unchanged. |
| 15 | Index status, pause, resume | Sources row: status tag and a row action | Status, Configure | **Gap** | Pause/Resume row action not drawn. |
| 16 | File search and exact preview | Search (Files scope) result opens a preview aside | Search, Detail | **Gap** | File preview aside not drawn. |
| 17 | Revoke document root | Sources row > Remove access (inline two-step) | Danger | WF | States that files are never deleted. |
| 18 | Vault state | Settings > Encryption row | Status | WF | |
| 19 | Folder selection | Sources dialog: native picker, paste a path as fallback; Add files | Add | WF | Needs G3 (security review). |
| 20 | Providers and capabilities | Connections > Save chats from your browser | Connect | WF | Capability matrix wording must stay honest (capture is not a handshake). |
| 21 | Browser capture and registration | Connections > Saving options (automatic save, Browsers) | Configure, Connect | WF (rows) / **Gap** (setup dialog) | The Browser setup dialog steps are not drawn. |
| 22 | Provider / generic import | **Archive > Import** (ChatGPT export, Gemini Takeout, transcript file) | Add | WF | Placement decided by the owner. Backend import core and owner zip/app commands landed on main (#166, #167, #168, #169, #170); UI wiring owned by MP-UI2-06. |
| 23 | MCP start/stop, issue, revoke | Connections > Let an AI app read your memory | Connect, Danger | WF | Named "AI app access" in UI. |
| 24 | Hosted tunnel lifecycle | Connections > Advanced: connect hosted ChatGPT (collapsed) | Advanced | WF | Explicit consent kept. |
| 25 | Global command palette | Top bar Search (Ctrl K) | Search | WF (search) / **Gap** ("Go to" commands) | Add a "Go to" group for pages and secondary commands. |
| 26 | Source search, progressive display | Search dialog: scope chips, grouped results (Memory first) | Search | WF | |
| 27 | Search to exact saved revision | Search result opens the chat page; revision via Info aside | Search, Detail | WF (partly) | Must still open the exact snapshot. |
| 28 | Multi-query, policy presets, date, token budget | Search > Advanced disclosure | Search, Advanced | **Gap** | Presets keep their native names behind the disclosure. |
| 29 | Context builder, copy JSON | Search select-to-share bar "Copy for an AI" | Search | WF | |
| 30 | Retrieval inspector / trace | Technical mode only (Search Details aside, Settings) | Advanced | **Gap** | Hidden by default. |
| 31 | Appearance (theme, density, technical mode) | Settings > Appearance rows | Configure | WF | Language and Constellation name added here. |
| 32 | Privacy and lock | Top bar Lock; Settings > Lock row | Configure | WF | Lock fences unchanged. |
| 33 | Recovery and cancel | Settings > Recovery row, then the locked gate | Configure | WF (row) | Ceremony screens are reused, not redesigned. |
| 34 | Backup | Settings > Backup row (disabled unless encrypted and unlocked) | Configure | WF | |
| 35 | Rebuild and diagnostics | Settings > Maintenance | Configure, Status | WF | Single-use confirmation kept. |
| 36 | Onboarding first run and returning | Home checklist (hides when done); first-run security gate | Status | WF | Settings can bring the guide back. |
| 37 | Welcome, Vault, optional providers, useful result wizard | Home checklist steps, each opening the matching dialog | Add, Progress | **Gap** | Inner step screens not drawn; "verified first result" rule is kept. |
| 38 | Keyboard, focus, modal lifetime | Shell and primitives (UI2-03 and UI2-04) | n/a | n/a | Tested, not drawn. |
| 39 | Loading, empty, error, partial, stale states | Every page | Empty, Warning, Progress | WF (empty, partial) / **Gap** (loading, error, stale) | Designed in UI2-03 and filled by each page task. |
| 40 | Large-data boundaries | Archive pagination, Constellation bounds | Browse | n/a | Measured in UI2-L1 against the owner's budgets. |
| N1 | Language (EN/ID) | Top bar toggle and Settings | Configure | WF | New. Catalog per language. |
| N2 | Constellation name | Settings > Name of the map page | Configure | WF | New. User-selectable; default Constellation. |
| N3 | Memory text in the app language | Memory detail note "Translated from English" and "View original" | Detail | WF | New. Stored once, per-language versions or translation at display time. |
| N4 | Breadcrumb | Top bar, starting at Home | Shell | WF | New. |

## 1. Signed-off deviation ledger (MP-UI2-00)

| ID | Capability row(s) | Earlier UI (MP-UI-09) | Human-first UI (MP-UI2) | Rationale & verification |
| --- | --- | --- | --- | --- |
| DEV-01 | 6 | Quick Peek slide-over drawer on Memory list | Replaced by full Memory detail page (`Home > Memory > ...`) and Constellation node aside | Owner-confirmed layout keeps reading calm and uncrowded (`MP-UI2-05`). |
| DEV-02 | 11, 12 | Archive/revision nodes, seed ID input, and layer/origin/revision filters in Hive | Removed from default view; Constellation renders curated memory only (or honest empty state) with one "Find a topic" field | Removes technical/raw graph noise from default view; awaits G1 (`MP-UI2-07`). |
| DEV-03 | 4, 8, 30 | Opaque evidence/proof IDs, hashes, revision IDs, canonical keys, and protocol names shown in default cards/rows | Hidden from default views; disclosed only inside `Info` aside or collapsed `Technical details` | Prevents machine-oriented identifiers from cluttering user workflows (`MP-UI2-03` UI rules lint enforces). |
| DEV-04 | 2 | Overview runtime health tiles on Home | Removed from Home; operational status ("Search is up to date" + tag, rebuild search, and technical diagnostics) moves to `Settings > Maintenance` | Keeps Home memory-first without telemetry clutter (`MP-UI2-10`, `MP-UI2-11`). |

## 2. Initial design gap register (for Gap Protocol execution in MP-UI2-03..11)

Every row marked **Gap** above is not drawn in `wireframe-v1.html`. Implementing lanes must build these exclusively from the `UI-PRINCIPLES.md` Section 5 pattern catalog and Section 7 tokens, supply EN + ID copy, and attach light/dark, EN/ID, three-width screenshots in their PR design gap ledger:

| Row # | Capability | Person's job (plain sentence) | Pattern class (`UI-PRINCIPLES` §5) | Destination (§6) | Owning task |
| --- | --- | --- | --- | --- | --- |
| 7 | Memory history and as-of state | See how a curated memory changed over time without cluttering the main note. | Detail, Advanced | Memory (`Home > Memory > detail`) | `MP-UI2-05` |
| 12 | Constellation topic filtering beyond default | Narrow a busy memory map to one topic by typing its name. | Search, Advanced | Constellation | `MP-UI2-07` |
| 15 | Document index status, pause, resume | Pause or resume indexing on a shared folder when I need my machine quiet. | Status, Configure | Sources (folder row) | `MP-UI2-08` |
| 16 | File search and verified excerpt preview | Open a matching file result and read the exact verified excerpt with a link to its folder. | Search, Detail | Global Search (`Files` scope preview aside) | `MP-UI2-11` |
| 21 | Browser capture setup dialog steps | Connect Chrome, Edge, or Brave to save chats from my browser step by step. | Connect, Add | Connections (`Saving options` dialog) | `MP-UI2-09` |
| 25 | Command-palette "Go to" navigation/actions | Jump to any page or common action straight from the keyboard search dialog. | Search | Global Search (`Ctrl+K`) | `MP-UI2-11` |
| 28 | Multi-query, policy presets, date, token budget | Tune search range or context size only when a basic search is not enough. | Search, Advanced | Global Search (`Advanced` disclosure) | `MP-UI2-11` |
| 30 | Retrieval inspector / score trace | Check why a search returned those results when debugging search behavior. | Advanced | Global Search (`Details` aside) & Settings (`Maintenance` technical mode) | `MP-UI2-10`, `MP-UI2-11` |
| 37 | Home onboarding checklist step dialogs | Complete each setup step (vault, optional browser capture, first search/read) in a guided flow. | Add, Progress | Home (checklist dialogs) | `MP-UI2-11` |
| 39 | Loading, empty, error, partial, and stale states | Understand when data is loading, empty, partially saved, stale, or failed, and know what to do next. | Empty, Warning, Progress | All 7 destinations (primitives in UI-kit) | `MP-UI2-03`, `MP-UI2-04..11` |

## 3. Explicitly deferred capabilities (out of scope for MP-UI2)

| Capability | Reason & prerequisite | Current UI behavior |
| --- | --- | --- |
| Memory create, edit, pin, and forget UI | Memory write primitives are not registered as Tauri/MCP tools; awaits accepted write-authority contract. | Disabled "Suggest a correction (coming later)" control on Memory detail; no fake writes or pin persistence. |
| AI write-access scope controls | External MCP catalog is read-only (`memory_search`, `memory_fetch`, `context_build`); awaits `MP-7MCP-01` and write tasks. | Plain read-only "AI app access" list and key management only. |
| Backup restore & graphical legacy migration UI | Offline maintenance command only per `MP-9B`; awaits `MP-9E`/`MP-9F` UI enablement. | Online backup row shown in Settings (disabled unless encrypted and unlocked); no fabricated restore UI. |
| Native UI (GPUI) port | Current presentation migration targets Tauri + WebView2. | Reopens only with `MP-UI2-L1` evidence if a locked performance budget is missed. |

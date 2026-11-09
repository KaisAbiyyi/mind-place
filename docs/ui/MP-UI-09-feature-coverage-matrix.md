# MP-UI-09 integrated feature coverage

This resolves the placement inventory in [UI-00](MP-UI-00-feature-coverage.md) against the five integrated source histories. “Cloud” means synthetic contracts/browser evidence, not installed Windows or owner acceptance. All source tasks remain unaccepted Review tasks; UI-09 formal AND gate remains Blocked.

| Capability / workflow | Reachable placement and implementation | Evidence / limit |
| --- | --- | --- |
| Real archive counts, recent captures | Overview metrics and recent conversation table | `reader.test.ts`, UI09 overview-health regression; counts from archive summaries |
| Runtime health | Overview Runtime health; Settings diagnostics | Observed index and listener status; failure explicitly unavailable; no demo telemetry |
| Provider archive filter / pagination | Conversations table, provider selector, 50-row pages | `ui-archive.test.ts`; browser fixture has 65 conversations and long titles |
| Transcript / immutable capture revision | Table or sidebar → full transcript → capture selector → archive table | `reader.test.ts`; paginated transcript, full copy/export, original capture metadata |
| HTML / Markdown / attachments | Sanitized transcript, safe asset validation | Injection/remote-image/late-asset regressions in reader tests |
| Memory collections / cards / Quick Peek | Curated Memory view collections and quick drawer | `memory-inspector.test.ts`; grouping is a view of native metadata, not fabricated classification |
| Detail / history / as-of | Memory detail and recorded revisions | Actual native memory inspection; historical hypotheses remain labeled |
| Conflict / related evidence | Detail relation viewer, related-memory actions, copy proof reference | Opaque evidence identifiers do not have an exact archive resolver; copying a reference is available, source drilldown is unavailable where no identity mapping exists |
| Projects / People / Topics | Contextual memory subnavigation and command palette | Entity lens tests; existing read-only investigation paths retained |
| Memory writes / forget / pin | Detail disabled writes, correction draft, view-only pin | Native write authority not invented; a draft/pin does not change authoritative memory |
| Hive topology / isolated nodes | Hive Mind Graph query and SVG; accessible node list | UI04 tests; deterministic topology layout, no invented edges; bounded 120 nodes / 240 edges |
| Hive search / layer / origin / revision filter | Graph controls, pan/zoom/branch/back/reset | UI04 focused tests; exact provenance available from returned edges |
| Hive → source transcript | Inspector “matching saved conversation” action | Opens only an exact loaded archive identity match; otherwise explanatory unavailable state |
| Approved document roots | Files & Secure Vault, authorize-folder dialog | UI05 and document-root tests; explicit Stage → Approve → Index; no automatic authority |
| Index / pause / resume / status | Root cards and permission/index inspector | Actual backend receipts; watcher status only session-observed, not durable cross-session claims |
| File search / exact preview | Files index search, version-checked source preview; global-search file preview | Existing `preview_document_source` with URI/version validation; revoked roots remain backend-fenced |
| Revoke | Deliberate root revoke confirmation | Document-root tests clear search/inspector; source files are not deleted |
| Vault state | Files protection panel, onboarding Vault, Settings Storage | Actual `vault_mode`; legacy and unknown never imply encryption; external originals/backups separate |
| Folder selection | Approved-root modal absolute-path entry | Native folder-picker API absent; clearly explained manual entry |
| Providers / capabilities | AI Providers & MCP page | Bundled provider capture matrix; capture support ≠ handshake or write authority |
| Browser capture / registration | Providers → Browser setup; onboarding → full capture settings | Actual Chrome/Edge/Brave registration; configuration saved ≠ tested browser connection |
| Provider / generic import | Providers “Import a transcript” → existing file workflow | Raw source retained, limits and partial receipts preserved; no fabricated provider messages |
| MCP start / stop / issue / revoke | Provider page and detailed Connections & MCP panel | Existing native authorization/consent; listener running ≠ verified client; credentials cleared on close/lock |
| Hosted tunnel lifecycle | Connections controls and Settings diagnostics | Explicit owner consent; helper process ≠ verified remote connectivity; no write capability granted |
| Global command palette | Search or jump / Ctrl or Meta K; routes and secondary commands | Shell tests; keyboard selection, grouped results, pagination and focus restoration |
| Source search / progressive display | Palette “Search all sources” / Overview secondary action | Archive/files bounded results; partial failures labeled, client-side pagination beyond 12 |
| Search → exact saved revision | Search result → verified provenance → Open full saved revision | New UI09 regression; actual archive ID + snapshot, full transcript remains reachable |
| Multi-query retrieval / policy / budgets | Search Multi-Query & Policies | Native presets fast/balanced/source_first/challenge/historical; RFC3339 date + `max_estimated_tokens`; date-less Historical rejected locally |
| Context builder / copy | Search → Build Context Pack → copy JSON | Reader clipboard wired; native estimates, citations and truncation preserved; no file export API fabricated |
| Retrieval inspector | Search trace drawer; Settings metadata tracer | Actual native trace/score contribution fields; no query/token logs |
| Appearance | Shell controls and Settings General | Single shell preference source; light/dark, density and technical mode consistent |
| Privacy / lock | Settings Privacy / toolbar Lock | App destroys reader, confirms native lock, fences late work, fails closed if lock unconfirmed |
| Recovery / cancel | Settings Storage → confirmation → locked security gate | Existing recovery ceremony reused; Settings does not collect credentials or re-provision active vault; cancel has no mutation |
| Backup | Settings Storage online backup when encrypted/unlocked | Native metadata `{version,vault_id_b64,generation,count}`; disabled in legacy/unknown mode; no restore/migration invented |
| Rebuild / diagnostics | Settings Diagnostics; preserved health inspector | Native archive rebuild with single-use confirmation; failed status unavailable, unknown never zero/stopped |
| Onboarding first run / returning | Security initialization → wizard; returning users Overview | `onboarding-integration.test.ts`, manual-lock tests |
| Welcome / Vault / optional providers / useful result | Setup & Onboarding four-step wizard | Non-sensitive validated progress; resume/skip; verified exact snapshot/message before “verified” |
| Keyboard / focus / modal lifetime | Shell, setup dialog, Settings tabs, search drawers | UI09 and existing workspace tests; background provider preserved during capture dialog, route transitions dispose modules |
| Loading / empty / error / partial / stale | Every module's factual states | Browser matrix plus focused tests; no service data substituted by demo metrics |
| Large-data boundaries | Archive pagination/virtualized sidebar, bounded Hive/search | Existing synthetic virtualization/long-transcript tests, 65-row browser fixture; NOT 100K/1M native acceptance |

## Remaining backend and native gates

Full MCP write-authority, accepted MP-9C redaction/erasure, general legacy encrypted migration, portable restore and graph maintenance APIs are separate tasks. None is enabled by this candidate. Exact original-memory-source resolution is unavailable for opaque proof IDs. The retained monochrome layouts show these boundaries; they are intentional capability deviations from the demo HTML.

Windows WebView2, real browser/client handshakes, root permission/revocation on actual NTFS, DPI/zoom, restart/installation and owner data are UI-L1/native gates. Final owner visual/product decision is UI-U1, NOT_RUN.

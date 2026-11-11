# MP-IMPORT-FIX-01 — import core corrections from the independent review of PR #166

Status: **READY FOR INDEPENDENT RE-REVIEW**. This is neither acceptance nor a merge request. Only the repository owner merges.

- Repository: `KaisAbiyyi/mind-place`. Source review: [PR #166](https://github.com/KaisAbiyyi/mind-place/pull/166) (Draft, head `bb5c0cf`, base `06b6199`).
- Branch: `agent/lucid-euler-l3lopd`, built as `origin/feat/mp-import-core-01` (`bb5c0cf`) plus a clean merge of `origin/main` (`06b6199`) plus **new commits only** (no rebase, no force-push, so the SHAs cited by earlier handoffs stay traceable).
- Executor: Claude Code, cloud Linux x86_64, synthetic fixtures only. No owner archive or corpus content was read.
- Claim: single-writer on logical resources `archive-core importers and historical write boundary` and the `ci.yml` archive test step. Notion card claimed 2026-11-11. No claim-controller generation exists; exclusivity is by owner assignment only.

## What this branch is, and is not

It provides **parsing and capture-only primitives**: a streaming ChatGPT `conversations.json` parser, a Gemini Takeout card parser, validation boundaries, and an owner-authorized write primitive for plaintext archives.

It does **not** include: raw-evidence persistence, independent source-manifest measurement, zip handling, Tauri commands, or UI. Those belong to MP-IMPORT-RAW-01 and later cards. Lossless raw persistence is **not claimed**.

## Scope item results

| # | Finding | Result |
| --- | --- | --- |
| 1 | P1-1 historical-write authority | Inventory and per-entry-point tests below. The blanket claim is withdrawn. |
| 2 | P2-4 completeness and provenance | ChatGPT is `complete` only when every content part is represented; Gemini is never `complete`; no card time is copied into `authored_at`. |
| 3 | P2-5 bounded input | Per-record limit enforced while reading; bounded ID state; bounded collection. |
| 4 | Notices | Dependency-specific coverage check derived from the manifests and `Cargo.lock`. |
| 5 | CI | Hosted `rust-fast` job runs `cargo test --locked -p mind-place-archive --tests`. |
| 6 | Honest scope | Banners added to both earlier handoff docs; PR text proposed below. |

## 1. Historical-write authority ledger (P1-1)

Public entry points that can persist a capture, found by reading every caller (`ArchiveStore::save`, `save_validated`, `save_authorized_historical_import`, `import_generic`, `import_legacy`, the capture host, the desktop IPC writer, `VaultCaptureStore`). Guarded by `every_public_persisting_entry_point_is_classified` in `crates/archive-core/tests/historical_write_authority.rs`: a new public `save*`/`import*` function, or a third call site of the shared sink, fails that test until it is classified here and there.

| Entry point | Governance | Evidence |
| --- | --- | --- |
| `ArchiveStore::save_authorized_historical_import` | **Requires `HistoricalImportAuthorization`.** The only path that persists `takeout-import` / `export-import`. | `authorized_entry_persists_only_historical_methods_for_the_authorized_provider`; capability scope, revocation, expiry and foreign-gate cases in `import_second_correction.rs` |
| `ArchiveStore::save_historical_import` | Rejecting shim; always errors. | `legacy_unauthenticated_historical_entry_rejects_every_provider` |
| `ArchiveStore::save` | **Documented exception, bounded.** Refuses `takeout-import` and `export-import`; refuses any unknown method for any provider; accepts live DOM captures plus `legacy-import` / `generic-import` (generic only with matching preserved raw). | `ordinary_save_refuses_export_and_takeout_historical_methods`, `ordinary_save_exception_is_limited_to_live_capture_and_legacy_or_generic_methods` |
| `ArchiveStore::import_generic` | **Documented exception.** Owner-initiated, bytes in, raw preserved first, can mint only provider `generic` / method `generic-import`. | `generic_import_can_only_mint_generic_provider_generic_method_captures` |
| `ArchiveStore::import_legacy` | **Documented exception.** Owner-initiated from the trusted legacy profile directory; writes `legacy-import` only after copying the raw session. | `legacy_import_requires_an_existing_trusted_directory_and_never_writes_historical_methods`; `tests/legacy.rs` |
| `VaultCaptureStore::save` (index-core) | Live-capture-only (`validate_live_capture`); refuses every historical and generic method. | `ordinary_vault_save_rejects_every_historical_and_generic_method` (feature `sqlcipher-preview`) |
| `VaultCaptureStore::import_legacy_consented` (index-core) | **Documented exception.** Requires `ExplicitMigrationConsent` (a different authority) and re-validates previously stored evidence, including already-authorized import captures, so an offline vault migration does not lose them. It cannot mint a new import: the consent object comes only from the owner migration flow. | `vault_migration_*` tests; code review |
| Native capture host / desktop IPC writer | Browser requests must be `manual-dom` / `automatic-dom`; the IPC writer is the vault store, whose `save` is live-only. | `crates/capture-host`; `native_capture_ipc` tests |

### Why the exceptions exist, and what they are not

`import_legacy` and `import_generic` predate PR #166 and are the owner's existing migration/import features. They are reachable only through Tauri commands behind the unlock gate. They never produce `export-import` / `takeout-import` captures and do not use the new authorization, which exists for the larger, riskier provider-export imports.

**Residual risk, not fixed here.** `ArchiveStore::save` itself still accepts a `legacy-import` capture from any in-process caller, with no preserved-raw requirement (generic does require it). The production callers are the two importers above; the browser-facing paths cannot reach it. Making `save` live-only and routing both importers through a private sink would remove the exception, but it changes pre-existing behavior outside this card and should be decided with MP-IMPORT-RAW-01, which reworks the historical write boundary.

## 2. Completeness and provenance (P2-4)

- **ChatGPT.** `complete` only when the active-branch chain is valid **and** every message on it has `content_type` of `text` or `multimodal_text`, string parts or image-asset parts with a readable asset pointer, a known author role, and not both `parts` and `text`. Anything else (unknown content type, opaque object part, numeric part, asset without a pointer, unknown role) yields `partial` with up to four distinct `Completeness not proved:` warnings plus a count of omitted ones. Alternate branches still produce the existing raw-evidence warning, and the active branch is still reported against its own content. A message's own `create_time` remains its `authored_at`; nothing is copied.
- **Gemini.** Equal user/assistant counts no longer prove anything. Every Takeout card is `partial` with the warning `Takeout activity cards cannot prove the full conversation; completeness is partial`. The card's activity time is **not** copied into any message's `authored_at` nor used as `conversation_created_at`. It is kept as `provider_event_time` in the raw-evidence envelope metadata.
- **Known limitation, unchanged.** Gemini `captured_at` still carries the activity time when one is present (and processing time with a warning otherwise); it is excluded from revision identity and is used for ordering undated Gemini captures. Revisit with MP-IMPORT-RAW-01.

Adversarial tests: `any_unrepresented_content_part_downgrades_the_whole_conversation_to_partial` (8 cases), `completeness_warnings_stay_bounded_under_hostile_part_variety`, `message_times_are_the_messages_own_times_only`, `card_event_time_is_never_copied_into_message_or_conversation_times`, `card_without_event_time_keeps_null_event_metadata_and_warns`, `balanced_turn_counts_never_prove_completeness`.

## 3. Bounded input (P2-5)

`stream_bounded_top_level_array` (new, `importers/mod.rs`) reads the export array in 64 KiB chunks and tracks string/escape/nesting state without recursion, so a record is rejected **the moment it would exceed the limit**, before it is buffered or handed to serde. The previous path called `serde_json` `RawValue`, which buffers a whole record before any size check could run. It also rejects whitespace floods, a missing or doubled comma, a trailing comma, trailing data and a truncated array. The old `stream_top_level_array` remains exported for compatibility and is now documented as unsuitable for untrusted input.

| Limit | Value | Behavior at the limit |
| --- | ---: | --- |
| ChatGPT record (`MAX_CHATGPT_RECORD_BYTES`) | 64 MiB | stream aborts while reading |
| Whitespace run between records (`MAX_CHATGPT_SEPARATOR_BYTES`) | 1 MiB | stream aborts |
| Distinct conversation IDs tracked (`MAX_TRACKED_CONVERSATION_IDS`) | 250,000 | stream aborts; uniqueness could no longer be guaranteed |
| Conversation ID length (`MAX_CONVERSATION_ID_BYTES`) | 256 B | record preserved raw-only, ID never tracked |
| In-memory collection, count (`MAX_ITEMS_IN_MEMORY`) | 50,000 | `parse_conversations_json` errors; use `stream_conversations_json` |
| In-memory collection, bytes (`MAX_COLLECTED_RAW_BYTES`) | 256 MiB | same |
| Gemini Takeout HTML | 64 MiB | read is capped at the limit plus one byte, then rejected |

Memory is bounded by the largest record plus the ID table, not by the export size. Tests: oversized record rejected while streaming without draining the source (an endless reader is read for at most the limit plus one chunk), whitespace flood, read-chunking independence, structure errors, ID tracker bound, overlong/empty IDs.

## 4. Notices

`THIRD-PARTY-NOTICES.txt` is generated by `scripts/generate-notices.mjs` and was already emitting `html5gum 0.8.0` with its MIT license text; what was missing was any check that it does. `scripts/notice-coverage.mjs` now derives every exact-pinned shipped crate from the Cargo manifests (9 today: `aes-gcm`, `crc32fast`, `flate2`, `hmac`, `html5gum`, `quick-xml`, `rusqlite`, `tauri`, `tauri-build`), takes the locked version from `Cargo.lock`, and requires for each one a notice section for that version, a declared license, and real bundled license text (not the generator's "no license file" fallback). A manifest pin that drifts from the lock also fails. `npm run verify:distribution-license` calls it. A newly pinned crate is covered automatically.

## 5. CI

One step added to the existing `rust-fast` job of `.github/workflows/ci.yml`: `cargo test --locked -p mind-place-archive --tests` (all archive test binaries, including `import_second_correction`, `validation_boundaries` and the new ledger test). No trigger, permission, secret or action change; `verify-workflow-security` and its tests pass. Review it as a workflow change.

Not covered by hosted CI: the vault feature tests (hosted CI only `cargo check`s `sqlcipher-preview`) and `verify:distribution-license` (needs a built release). **"Verify Windows product" skipped is NOT_RUN, not PASS.**

## 6. Proposed PR #166 description changes

PR text was not edited from this branch because #166's head is `feat/mp-import-core-01`. Replace its "What this PR does not include" section with:

> This PR provides parsing and capture-only primitives. It does not include raw-evidence persistence, independent source-manifest measurement, zip handling, Tauri commands or UI. Not every historical write requires `HistoricalImportAuthorization`: takeout/export captures persist only through `save_authorized_historical_import`; the legacy-import and generic-import migration paths are documented exceptions (see `docs/implementation-summary/2026-11-11-mp-import-fix-01.md`). Gemini Takeout projections are always `partial`; ChatGPT projections are `complete` only when every content part is represented.

## Verification

Environment: Linux x86_64, Cargo `1.97.0` locally (hosted CI pins Rust 1.90.0 and has not yet run this branch), Node 22. Synthetic fixtures only.

| Command | Result |
| --- | --- |
| `cargo test --locked -p mind-place-archive --tests` | PASS: 116 passed, 0 failed, 0 ignored (48 unit, 19 archive, 5 generic, 8 historical-write authority, 18 second-correction, 7 legacy, 2 registration, 9 validation-boundary) |
| `cargo test --locked -p mind-place-index --features sqlcipher-preview --test vault_capture_encrypted` | PASS: 5 passed (local only; not run by hosted CI) |
| `cargo fmt --all -- --check` | PASS |
| `npm run verify:repo` | PASS |
| `node --import tsx --test test/notice-coverage.test.mjs test/workflow-security.test.mjs` | PASS: 15 passed, 0 failed |
| `node scripts/verify-workflow-security.mjs` | PASS |
| `npm run verify:distribution-license` | PARTIAL: PASS against a throwaway directory holding the freshly generated notices (9 crates covered) and FAIL when the `html5gum` section was altered; **NOT_RUN against a real `release/` package** |
| `npm run test:rust` (archive, capture-host, security, state, index, integration, documents) | PASS, exit 0 (totals in `evidence/mp-import-fix-01-20261111.json`) |
| `npm run test:web` | PASS: 271 tests, 269 passed, 0 failed, 2 skipped (Windows-only) |

Behaviors removed: `complete` from equal Gemini turn counts, the card time copied into `authored_at`, and the unbounded `RawValue` record read.

## Not run / limits

- Hosted CI on this branch: **NOT_RUN** at handoff time.
- Windows SQLCipher, installed-app, hostile-account and real-provider qualification: **NOT_RUN**.
- Real ChatGPT/Gemini exports: **NOT_RUN** (private; see MP-IMPORT-SANITIZE-01).
- The ChatGPT zip layout is unverified; no zip handling exists.
- The residual `save` exception above is documented, not removed.

## Handoff

`TASK_ID` MP-IMPORT-FIX-01 · `CLAIM` claimed by the Claude Code cloud session; no fencing generation · `BASE_SHA` PR head `bb5c0cf` merged with `main` `06b6199` (merge `66fb06e`) · `BRANCH` `agent/lucid-euler-l3lopd` · `REMOTE_CHECKPOINT_SHA` see the final chat handoff and `git rev-parse HEAD` · `PR` none opened by this task; the parsers live on #166 · `LOCAL_REQUIRED` NONE for this card · `HUMAN_REQUIRED` re-review, then the owner's manual merge decision.

Recovery: the FIX-01 commits sit directly on the PR #166 history. If the owner wants them on `feat/mp-import-core-01`, a fast-forward of that branch to the last FIX-01 commit is sufficient; no history is rewritten.

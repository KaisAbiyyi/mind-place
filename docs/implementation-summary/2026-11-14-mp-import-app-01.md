# MP-IMPORT-APP-01 — import in the app: zip source, Tauri commands, owner re-authentication and progress

Status: **READY FOR SECURITY REVIEW** (draft PR). Neither acceptance nor a merge request. Only the repository owner merges. Synthetic fixtures only; no real export, archive or corpus content was read.

- Repository `KaisAbiyyi/mind-place`, branch `work/mp-import-app-01/zip-source-and-commands`, base `main` `5290753` (after PR #166, #167 and #168).
- Design note: [`docs/implementation-plan/2026-11-14-mp-import-app-01-design.md`](../implementation-plan/2026-11-14-mp-import-app-01-design.md).
- Claim: Claude Code local session on the owner's Windows checkout (owner authorized local implementation in chat). Locks: `src-tauri` import commands, zip adapter, import entry, plus the archive-core source manifest and security-core import authorization for the zip source kind. No claim controller exists; exclusivity is by owner assignment.

## Owner decisions (2026-11-14, in chat)

1. Build against synthetic fixtures and the documented layout. Real-export layout verification stays **NOT_RUN** and moves to MP-IMPORT-L1.
2. Extend the RAW-01 manifest and the import authorization so a **single zip file** is a source, streamed without extraction.
3. Entry point **option A**: backend commands only. The Import button arrives with MP-UI2-06. No UI, copy or i18n change in this card.

## What was built

| Piece | File | Role |
| --- | --- | --- |
| Zip reader | `crates/archive-core/src/historical/zip.rs` | Strict, read-only. Stored and deflate members, zip64, single disk. Typed errors for traversal names, duplicates, encryption, unknown methods, overlapping entries, ratio bombs, lying sizes or CRC, multi-disk, data before/after the archive. Built on `flate2` (already pinned through document-core); no new crate |
| Zip source in the manifest | `crates/archive-core/src/historical/manifest.rs` | `SourceManifest::measure` accepts a folder or a zip. A zip is measured by hashing the archive and every decompressed member; `open_member` streams one member with size and CRC enforced; `verify_layout_unchanged` re-reads the directory and archive length/mtime. Separate manifest-hash domain that binds the archive length and SHA-256 |
| Job | `crates/archive-core/src/historical/job.rs` | Staging opens members through `open_member` (the folder path is unchanged); zip corruption surfaces as its coded error; new default `ImportProgress::items_total` hook |
| Member selection | `manifest.rs` | `candidate_members(provider)` from the documented layout; `member_extension_allowed` for an owner-chosen list |
| Authorization | `crates/security-core/src/vault_import_auth.rs` | `validate_import_source`: a safe directory or a regular file; `authorize_import_session` uses it. Everything else (canonical path binding, manifest hash, epoch, TTL, revoke, lock fence) is unchanged |
| Tauri commands | `src-tauri/src/import_commands.rs`, `main.rs` | `inspect_import_source`, `start_import`, `cancel_import`; Lock and window close cancel a running import |
| CI | none changed | the new archive and security tests run in hosted `ci.yml` (`rust-fast`: `cargo test -p mind-place-archive --tests`, `-p mind-place-security --lib`). `import_commands::tests` are **not** in hosted CI: adding a step to `windows-product.yml` breaks the trusted native-step contract (`.github/windows-policy/native-steps.json`, `scripts/mp10c_ci_gate.py`), which is governance policy outside this card. The step was reverted; owner decision pending |

### Commands

| Command | Gate | Does |
| --- | --- | --- |
| `inspect_import_source { provider, path }` | unlocked owner session, legacy plaintext archive | measures the source (read-only); returns kind, file count, bytes, candidate members and an opaque `sourceToken` |
| `start_import { provider, path, sourceToken, credential, members? }` | unlocked session + owner credential (gate backoff) | verifies the credential, re-measures, refuses if the token differs (`E_SOURCE_CHANGED`), chooses members, authorizes a capability bound to the measured hash (TTL ≤ 1 h), runs `import_authorized_source` with `import-progress` events and cancel, revokes the capability, returns the receipt |
| `cancel_import` | none (sets a flag) | stops the running import at its next safe point |

Progress events are `{ phase, done, total }` (`staging`, `verified`, `publishing`, `finishing`), throttled to one per 64 records; they carry counts only. Errors are coded strings; an OS error text is never forwarded (it may name a path). The credential is wrapped in `Zeroizing` on arrival and never logged or echoed.

## Acceptance mapping

| Card item | Evidence |
| --- | --- |
| Synthetic exports through the commands fill the archive; re-import idempotent | `inspect_then_start_imports_a_zip_and_reports_count_only_progress` (zip → 3 conversations, re-run `already_complete`), `a_chatgpt_export_zip_imports_without_extraction_and_matches_a_folder_import` (raw blobs byte-identical to the decompressed member slices, same raw blobs as a folder import, nothing extracted), `reimporting_the_same_zip_is_idempotent` (disk byte-identical), `a_gemini_takeout_zip_imports_from_its_documented_path`, `a_zip64_export_is_read_and_imported` |
| Cancel and lock mid-import behave safely | `cancel_while_staging_leaves_nothing_and_cancel_while_publishing_resumes` (staging: nothing visible, no job; publishing: partial, job kept, re-run `resumed` and complete), `locking_mid_import_stops_publication` (`E_AUTHORIZATION`, no further items), `locking_mid_job_stops_publication_from_a_zip`, `a_member_outside_the_manifest_or_a_crash_mid_job_behaves_like_a_folder_source` |
| Zip adapter: no extraction, traversal, bombs, oversize, unsupported zip64 | `unsafe_member_names_are_refused` (8 names + non-UTF-8), `duplicate_member_names_are_refused`, `encrypted_and_unknown_methods_are_refused`, `a_high_ratio_member_is_refused_as_a_bomb`, `overlapping_members_are_refused_as_a_bomb`, `bytes_hidden_between_the_directory_and_the_end_record_are_refused`, `lying_sizes_and_checksums_are_refused` (5 variants), `malformed_truncated_split_and_padded_archives_are_refused` (incl. inconsistent zip64), `archive_limits_are_enforced` |
| Substitution after authorization | `a_zip_replaced_after_authorization_is_refused_before_anything_is_written`, `a_zip_changed_while_it_is_staged_is_detected_and_nothing_is_visible`, `a_source_changed_since_inspection_is_refused` |
| Owner re-authentication | `a_wrong_credential_writes_nothing_and_backs_off`; security-core `a_zip_file_source_is_authorized_and_bound_to_its_canonical_path`, `an_import_source_may_be_a_folder_or_a_regular_file_but_nothing_else` |
| Memory bounded on a large synthetic export | probe: 1 GB decompressed export, peak 71.1 MB (Verification) |
| Security review passes; lock-fence tests stay green | security review **NOT_RUN** (required); existing lock-fence and historical-write ledger tests pass (below) |

Mutation checks: with each guard disabled the named test fails, then the guard was restored: overlap rule, central-directory-end rule, CRC check, per-member ratio rule, archive-wide ratio rule, drive-prefix rule, duplicate rule, staging lease check.

## Advisory review (AI, read-only, not the card's security review)

A read-only advisory reviewer (Claude Sonnet subagent) read `5290753..8943d30` and reported no P0 or P1. Every finding was checked against the code before acting:

| Finding | Action |
| --- | --- |
| P2 many small members, each exempt from the per-member ratio, add up to ~900:1; measurement not cancellable | **Fixed**: archive-wide limit (`many_small_members_cannot_add_up_to_a_bomb`). Cancellation of measurement: residual 9 |
| P3 Lock does not stop staging (only publication is fenced) | **Fixed**: lease checked at every safe point (`locking_while_staging_stops_before_anything_is_published`) |
| P3 inspect accepts UNC paths (could leak network credentials), no credential, not serialized | **Fixed** for `\\host\share`, `\\?\UNC\..` and `\\.\..` paths (`inputs_are_validated_before_any_read`). Rest: residual 2 |
| P3 member read error not sticky | **Fixed** (`a_member_read_error_is_sticky`); was not reachable through current callers |
| P3 capability not revoked if the job panics | **Fixed**: revoke on drop |
| P3 bidi control characters in member names (display spoofing) | **Fixed** (`unsafe_member_names_are_refused`) |
| P3 the IPC-deserialized credential copy is not zeroized | Not changed: same pattern as `unlock_security`; the `String` is wrapped in `Zeroizing` on arrival |

## Residual risks and limits (read before acceptance)

1. **Real export layouts are unverified** (owner decision 1). Candidate selection may miss localized or renamed members; `members` in `start_import` is the escape hatch. MP-IMPORT-L1 must record the real layout.
2. **`inspect_import_source` reads a path supplied by the webview** with only an unlocked session (no credential). It returns no content (counts, candidate names, a hash token) and is not reachable from MCP or the extension, but it can confirm that a file exists, its size, and (through the token) a guessed exact content. Network shares and device paths are refused. It is not serialized through the one-import slot and cannot be cancelled. `start_import` requires the credential. Recommendation for MP-UI2-06: choose the source with a native dialog in the backend so the renderer never supplies a path.
3. The old `save_authorized_historical_import` (RAW-01 residual 1) is still public and unchanged; the new commands use only `import_authorized_source`.
4. A zip is read three times (inspect, job-start measurement, staging), like a folder. Large-export throughput is only the probe below, not the owner's machine.
5. Windows-native behaviour (antivirus holding the zip, very long paths, reparse points on the zip path) is **NOT_RUN** until MP-IMPORT-L1.
6. A non-zip64 archive whose bytes just before the end record happen to look like a zip64 locator is refused (fail closed). Archives with data descriptors are accepted (sizes come from the central directory).
7. Images and other assets in an export are measured but not ingested (RAW-01 behaviour).
8. Encrypted vault profiles cannot import (RAW-01 refuses an encrypted vault); the commands return `MP_VAULT_NOT_READY`.
9. **Measurement cannot be cancelled.** Inspect, the command's re-measure and the job-start measure each decompress and hash the whole source with no cancel or lease check; Lock and Cancel take effect at the next staging checkpoint. The archive-wide ratio limit bounds a hostile zip to `200 x archive length + 64 MiB` of work per pass (and `max_total_bytes`).
10. **Long jobs outlive the capability.** A capability lives at most one hour. In the probe, publication ran at roughly 12 items per second on this machine, so an export far beyond ~40 000 conversations ends with `E_IMPORT_AUTH_EXPIRED` and needs one more `start_import` to resume (tested).

## Verification

Environment: Windows 11 x64, Rust 1.91.1 (workspace MSRV 1.90), Node 24.21.0; local run by the implementing session. Self-reported; **not** exact-head hosted CI and **not** an owner qualification.

| Command | Result |
| --- | --- |
| `cargo test -p mind-place-archive --test zip_source_import` and `--lib` | PASS: 19 passed, 1 ignored (the memory probe); lib 63 passed (incl. the sticky-error unit test) |
| `cargo test -p mind-place --bin mind-place import_commands` | PASS: 11 passed |
| `npm run test:rust` (archive, capture-host, security, state, index, integration, documents) | PASS: 432 passed, 0 failed, 3 ignored, exit 0 (run with `CARGO_BUILD_JOBS=2`; a first attempt stopped while *compiling* `integration-core` tests because `rustc` ran out of memory next to the running probe, not a test failure) |
| `npm run test:web` | PASS: 333 tests, 333 passed, 0 failed |
| `cargo fmt --all -- --check`, `npm run verify:repo`, `npm run typecheck`, `node scripts/verify-workflow-security.mjs` | PASS |
| `npm run build` | PASS (release build, `mind-place.exe` produced; only pre-existing warnings) |
| `cargo clippy -p mind-place-archive -p mind-place-security --tests` | no warning in the changed files |
| Mutation checks (8 guards) | each named test failed with its guard disabled; restored |
| Large synthetic export memory probe (release, 20 000 conversations, 997 MB decompressed JSON in a 384 MB zip) | PASS: peak working set **71.1 MB** for the whole process (sampled every 300 ms; owner thresholds 1 GB warning, 2 GB failure). Measure 5.0 s, stage and verify 174 s, publish 1713 s (about 12 items per second; other builds and test suites ran on the machine during part of it), 20 000 imported in one run. An earlier run with a 300 s test capability stopped with `E_AUTHORIZATION` at about 6 minutes, peak 70.9 MB: that is why residual 10 exists. Throughput is indicative only |
| True symlink tests (`a_zip_source_reached_through_a_symlink_is_refused`, security-core symlink cases) | **unix only**; run on Linux hosted CI, not on this Windows runner |
| Hosted CI on the exact head | see the PR checks; not claimed here. A head that added an `import_commands` step to `windows-product.yml` (`8943d30`) failed `Fast developer feedback` on the native-step contract test; the step was reverted. `import_commands::tests` are local only (**PARTIAL** for card acceptance 4) |
| Security review of the new commands and the zip reader | **NOT_RUN** (required by the card) |
| Real ChatGPT / Gemini export, Windows-native behaviour | **NOT_RUN** (MP-IMPORT-L1) |

## Handoff

`TASK_ID` MP-IMPORT-APP-01 · `CLAIM` Claude Code local session; no fencing generation · `BASE_SHA` `5290753` · `BRANCH` `work/mp-import-app-01/zip-source-and-commands` · `PR` see the draft PR · `LOCAL_REQUIRED` MP-IMPORT-L1 (real exports on a disposable archive, Windows behaviour) · `HUMAN_REQUIRED` security review of the new commands and the zip reader, then the owner's manual merge decision. Unlocks: MP-IMPORT-L1; the Import entry (MP-UI2-06).

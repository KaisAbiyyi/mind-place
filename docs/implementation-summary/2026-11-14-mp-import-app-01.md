# MP-IMPORT-APP-01 — import in the app: zip source, Tauri commands, owner re-authentication and progress

Status: **REVIEW ROUND 1 ADDRESSED, READY FOR SECURITY RE-REVIEW** (draft PR; round 1 was the owner's independent review of `fc1bbc8`, see below). Neither acceptance nor a merge request. Only the repository owner merges. Synthetic fixtures only; no real export, archive or corpus content was read.

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
| Zip reader | `crates/archive-core/src/historical/zip.rs` | Strict, read-only. Stored and deflate members, zip64, single disk. Typed errors for traversal names, duplicates, encryption, unknown methods, overlapping entries, ratio bombs, lying sizes or CRC, multi-disk, and any byte that is not a measured member, a descriptor that agrees with the directory, or a parsed record (prefix, gaps, trailing data). Built on `flate2` (already pinned through document-core); no new crate |
| Zip source in the manifest | `crates/archive-core/src/historical/manifest.rs` | `SourceManifest::measure` accepts a folder or a zip. A zip is measured by hashing the archive and every decompressed member; `open_member` streams one member with size and CRC enforced; `verify_layout_unchanged` re-reads the directory and archive length/mtime. Separate manifest-hash domain that binds the archive length and SHA-256 |
| Job | `crates/archive-core/src/historical/job.rs` | Staging opens members through `open_member` (the folder path is unchanged); zip corruption surfaces as its coded error; new default `ImportProgress::items_total` hook |
| Member selection | `manifest.rs` | `candidate_members(provider)` from the documented layout; `member_extension_allowed` for an owner-chosen list |
| Authorization | `crates/security-core/src/vault_import_auth.rs` | `validate_import_source`: a safe directory or a regular file; `authorize_import_session` uses it. Everything else (canonical path binding, manifest hash, epoch, TTL, revoke, lock fence) is unchanged |
| Tauri commands | `src-tauri/src/import_commands.rs`, `main.rs` | `inspect_import_source`, `start_import`, `cancel_import`; Lock and window close cancel a running import |
| CI | none changed | the new archive and security tests run in hosted `ci.yml` (`rust-fast`: `cargo test -p mind-place-archive --tests`, `-p mind-place-security --lib`). `import_commands::tests` are **not** in hosted CI: adding a step to `windows-product.yml` breaks the trusted native-step contract (`.github/windows-policy/native-steps.json`, `scripts/mp10c_ci_gate.py`), which is governance policy outside this card. The step was reverted; owner decision pending |

### Commands

| Command | Gate | Does |
| --- | --- | --- |
| `inspect_import_source { provider, selection }` | unlocked owner session, legacy plaintext archive, a live selection grant | takes the import slot and measures the source (read-only; Cancel and Lock stop it); returns kind, file count, bytes, candidate members and an opaque `sourceToken` |
| `start_import { provider, selection, sourceToken, credential, members? }` | unlocked session + live selection grant + owner credential (gate backoff) | verifies the credential, re-measures, refuses if the token differs (`E_SOURCE_CHANGED`), chooses members, authorizes a capability bound to the measured hash (TTL ≤ 1 h), runs `import_authorized_source` with `import-progress` events and cancel, revokes the capability, returns the receipt |
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

## Independent review round 1 (owner, PR #169 comment on `fc1bbc8`) and response

Disposition was CHANGES REQUESTED / merge hold; no P0 or P1. All three P2 findings were reproduced against the code before acting.

| Finding | Action | Evidence |
| --- | --- | --- |
| P2 prefix/gap rejection not enforced; the existing prefix test did not rebase offsets, so it passed for the wrong reason | **Fixed.** The reader now requires the members to tile the file (first local header at 0, each next at the previous end plus its descriptor, last ending at the central directory). Data-descriptor policy defined: bit 3 declared, exactly one descriptor, CRC and sizes equal to the directory | `a_prefix_with_rebased_offsets_is_refused` (classic and zip64; builds a consistent archive, with a control that the helper changes nothing when the prefix is empty), `unaccounted_bytes_between_members_or_before_the_directory_are_refused`, `a_data_descriptor_that_agrees_with_the_directory_is_accepted` (signed/unsigned, 32/64 bit), `a_data_descriptor_that_disagrees_or_is_misplaced_is_refused`. Mutation: with the first-header guard disabled the prefix and gap tests fail; with the end guard disabled the gap and descriptor tests fail |
| P2 Cancel and Lock cannot interrupt source measurement; inspect has no slot or cancel route | **Fixed.** `SourceManifest::measure_interruptible` polls between entries and about once per MiB (archive hash, member inflate, folder walk); `ImportProgress::measuring` covers the job's own start-of-run measurement; `inspect_import_source` now holds the import slot, so Cancel and Lock reach it and it never overlaps an import | `a_long_measurement_polls_the_interrupt_and_stops_on_its_error` (folder and zip, 5 MiB, stops at the second poll), `the_jobs_own_measurement_at_start_can_be_stopped_and_writes_nothing`, `cancel_and_lock_stop_the_measurement_of_a_large_source` (cancel before, cancel mid-read, Lock), `inspection_and_import_never_run_at_the_same_time` (`E_IMPORT_BUSY`, `retire` reaches the holder). A stale lease now stops at the first poll with `E_AUTHORIZATION` (the old assertion expected the gate's `MP_` code from a later step) |
| P2 / security acceptance: arbitrary local path inspection exposed to the webview | **Fenced now, picker deferred.** Commands take a backend-minted selection grant (`selection`), not a path. No command mints one yet, so until MP-UI2-06 adds the native picker the two commands refuse every request (`E_IMPORT_SELECTION`). Tracked as a security dependency of MP-UI2-06 (design note, residual 5) | `a_source_must_be_named_by_a_backend_grant_and_grants_expire` (typed paths and unknown tokens refused, UNC/relative refused at mint, cap, expiry, Lock forgets grants) |

Owner call worth confirming: the command signature changed from `path` to `selection`. If a path-based dev entry point is wanted before MP-UI2-06, that is a deliberate, separate decision.

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
2. **Source selection by native picker is not built.** The commands accept only a backend-minted selection grant, and nothing mints one until MP-UI2-06 adds the native dialog; that card must not add a renderer-path-to-grant command. Until then the commands are unusable from the webview (fail closed). Network shares and device paths are still refused at mint and at use. Inspection returns no content (counts, candidate names, a hash token).
3. The old `save_authorized_historical_import` (RAW-01 residual 1) is still public and unchanged; the new commands use only `import_authorized_source`.
4. A zip is read three times (inspect, job-start measurement, staging), like a folder. Large-export throughput is only the probe below, not the owner's machine.
5. Windows-native behaviour (antivirus holding the zip, very long paths, reparse points on the zip path) is **NOT_RUN** until MP-IMPORT-L1.
6. A non-zip64 archive whose bytes just before the end record happen to look like a zip64 locator is refused (fail closed). Archives with data descriptors are accepted only when the descriptor agrees with the central directory and is the only thing between the member and the next record. Archives with other padding or extra records are refused (`E_ZIP_MALFORMED`).
7. Images and other assets in an export are measured but not ingested (RAW-01 behaviour).
8. Encrypted vault profiles cannot import (RAW-01 refuses an encrypted vault); the commands return `MP_VAULT_NOT_READY`.
9. **Measurement is cancellable at about 1 MiB granularity** (was: not cancellable). Parsing the central directory is not polled; it is bounded by `max_entries` and the directory size. A single `read` of the decoder is not interrupted, but it is bounded by the buffer. The archive-wide ratio limit still bounds a hostile zip to `200 x archive length + 64 MiB` of work per pass.
10. **Long jobs outlive the capability.** A capability lives at most one hour. In the probe, publication ran at roughly 12 items per second on this machine, so an export far beyond ~40 000 conversations ends with `E_IMPORT_AUTH_EXPIRED` and needs one more `start_import` to resume (tested).

## Verification

Environment: Windows 11 x64, Rust 1.91.1 (workspace MSRV 1.90), Node 24.21.0; local run by the implementing session. Self-reported; **not** exact-head hosted CI and **not** an owner qualification.

| Command | Result |
| --- | --- |
| `cargo test -p mind-place-archive` (round 1 fixes, re-run) | PASS: `zip_source_import` 25 passed, 1 ignored (the memory probe); lib 63 passed; every other archive suite passed |
| `cargo test -p mind-place --bin mind-place import_commands::tests` (round 1 fixes, re-run) | PASS: 14 passed (11 before; three new tests) |
| `npm run test:rust` (archive, capture-host, security, state, index, integration, documents) | PASS: 432 passed, 0 failed, 3 ignored before round 1; **re-run after the round 1 fixes: 440 passed, 0 failed, 3 ignored, exit 0** (run with `CARGO_BUILD_JOBS=2`; a first attempt stopped while *compiling* `integration-core` tests because `rustc` ran out of memory next to the running probe, not a test failure) |
| `npm run test:web` | PASS before round 1 (333/333); not re-run, no web or TypeScript file changed in round 1 |
| `cargo fmt --all -- --check`, `npm run verify:repo`, `npm run typecheck`, `node scripts/verify-workflow-security.mjs` | PASS |
| `npm run build` | PASS (release build, `mind-place.exe` produced; only pre-existing warnings) |
| `cargo clippy -p mind-place-archive -p mind-place-security --tests` | no warning in the changed files |
| Mutation checks | 8 guards before round 1 and 2 more for it (first-local-header-at-zero, last-member-ends-at-directory): each named test failed with its guard disabled; restored |
| Large synthetic export memory probe (release, 20 000 conversations, 997 MB decompressed JSON in a 384 MB zip) | PASS: peak working set **71.1 MB** for the whole process (sampled every 300 ms; owner thresholds 1 GB warning, 2 GB failure). Measure 5.0 s, stage and verify 174 s, publish 1713 s (about 12 items per second; other builds and test suites ran on the machine during part of it), 20 000 imported in one run. An earlier run with a 300 s test capability stopped with `E_AUTHORIZATION` at about 6 minutes, peak 70.9 MB: that is why residual 10 exists. Throughput is indicative only |
| True symlink tests (`a_zip_source_reached_through_a_symlink_is_refused`, security-core symlink cases) | **unix only**; run on Linux hosted CI, not on this Windows runner |
| Hosted CI on the exact head | see the PR checks; not claimed here. A head that added an `import_commands` step to `windows-product.yml` (`8943d30`) failed `Fast developer feedback` on the native-step contract test; the step was reverted. `import_commands::tests` are local only (**PARTIAL** for card acceptance 4) |
| Security review of the new commands and the zip reader | **NOT_RUN** (required by the card) |
| Real ChatGPT / Gemini export, Windows-native behaviour | **NOT_RUN** (MP-IMPORT-L1) |

## Handoff

`TASK_ID` MP-IMPORT-APP-01 · `CLAIM` Claude Code local session; no fencing generation · `BASE_SHA` `5290753` · `BRANCH` `work/mp-import-app-01/zip-source-and-commands` · `PR` see the draft PR · `LOCAL_REQUIRED` MP-IMPORT-L1 (real exports on a disposable archive, Windows behaviour) · `HUMAN_REQUIRED` security review of the new commands and the zip reader, then the owner's manual merge decision. Unlocks: MP-IMPORT-L1; the Import entry (MP-UI2-06).

# MP-IMPORT-RAW-01 — atomic raw-evidence and projection publication with an independent source manifest

Status: **READY FOR INDEPENDENT SECURITY RE-REVIEW** (draft PR). Neither acceptance nor a merge request. Only the repository owner merges. Synthetic fixtures only; no real export, archive or corpus content was read.

- Repository: `KaisAbiyyi/mind-place`. Branch `work/mp-import-raw-01/atomic-raw-publication`, base `main` `8350f8e` (after PR #166 and PR #167 merged). Draft [PR #168](https://github.com/KaisAbiyyi/mind-place/pull/168).
- Design note (with the review decisions in section 9, which supersede earlier text): [`docs/implementation-plan/2026-11-13-mp-import-raw-01-design.md`](../implementation-plan/2026-11-13-mp-import-raw-01-design.md). The design was reviewed by an **independent AI reviewer (advisory, read-only)**; that is not the owner's design approval and not the card's "independent security re-review", which is still required.
- Claim: single writer on `archive-core historical write path` and `security-core import authorization` (no `security-core` source change was needed). No claim controller exists; exclusivity is by owner assignment.

## What was built

`ArchiveStore::import_authorized_source(&SourceImportRequest)` in `crates/archive-core/src/historical/`:

| Piece | File | Role |
| --- | --- | --- |
| Independent manifest | `manifest.rs` | `SourceManifest::measure` walks the source folder and hashes every regular file (path, size, SHA-256); the hash is domain-separated and length-framed. Fields are private, so a caller cannot supply a manifest or a hash. Typed `E_SOURCE_*` errors, hard limits, no-follow opens, size/mtime checks while hashing |
| Journaled job | `job.rs` | measure and pin, stage in a private spool while hashing the parsed bytes, record a root hash, publish item by item, per-item done markers, receipt |
| Parser spans | `importers/*` | additive: record offset, length and SHA-256 from the extraction layer; a job-wide, case-insensitive duplicate-ID tracker; an unparseable ChatGPT record is kept raw-only with a generic reason instead of aborting the job; a Gemini card cap |
| Store refactor | `store.rs` | shared `snapshot_identity` and `archive_id_for`; shared `require_same_owner`; a provider export never displaces a live (manual or automatic) latest capture |

### How the owner flow uses it (for MP-IMPORT-APP-01)

1. `SourceManifest::measure(root, limits)` (show the owner the file count and bytes).
2. `gate.authorize_import_session(lease, credential, provider, root, *manifest.sha256(), ttl)` pins that hash.
3. `store.import_authorized_source(&SourceImportRequest::new(&gate, lease, &capability, &members))`. The import **takes no inventory and no hash from the caller**: the root, provider and hash come from the capability, and the import re-measures the folder itself and refuses (`E_SOURCE_CHANGED`) when it differs from what was authorized.
4. A failed publication is repeated by calling the same function (new authorization is fine: the job key is derived from provider, manifest, members, importer version and limits, not from the gate's job id). `incomplete_jobs()` lists unfinished jobs; `discard_staged_job(key)` removes a spool and never touches published data.

Choosing the members (`conversations*.json`, the Takeout HTML) from the real export layout is MP-IMPORT-APP-01's job; the layout is still unverified (real exports: `NOT_RUN`).

### Storage layout and order

`imports/raw/<aa>/<sha256>.raw` immutable content-addressed bytes; `imports/evidence/<aa>/<id>.json` immutable provenance (provider, domain, source identifier, payload hash and length, member path and hash, offset, length, manifest hash, opaque metadata; no job, time or disposition); `imports/dispositions/<aa>/<id>.json` replaceable link (`projected` / `raw-only` / `rejected`, reason, archive and snapshot id); `imports/jobs/<job_key>/` state, manifest, spool, done markers, receipt. Per item, inside the archive lock and the gate action: raw blob, evidence, projection (`meta.json` last), link, then the done marker. A projection is never visible without a durable raw blob and an evidence record.

## Acceptance and scope mapping

| Card item | Evidence |
| --- | --- |
| P1-3 raw persistence of `RawEvidenceEnvelope` and `PreservedRawOnly` (full ChatGPT DAG with alternate branches, raw-only Gemini cards, rejected projections) | `publishes_raw_evidence_and_projections_and_the_receipt_equals_the_disk`, `raw_blobs_are_byte_identical_to_the_independently_read_source_slices` (alternate branch and unparseable record are in the raw bytes), `gemini_cards_are_preserved_with_their_exact_bytes_and_non_card_bytes_are_counted` |
| Atomic, crash-safe, idempotent, deduplicated, recoverable, per-job receipt | `a_crash_at_any_publication_step_is_repaired_by_running_the_import_again` (seven crash points; the repaired disk equals a clean run byte for byte), `a_crash_while_staging_...`, `reimport_of_the_same_source_is_idempotent_and_writes_nothing`, `a_new_export_with_the_same_content_reuses_raw_blobs_and_adds_provenance`, `identical_records_in_one_job_store_the_bytes_once_...` |
| Bounded memory, streaming, oversized/abusive input | `oversized_and_abusive_sources_are_bounded_and_leave_nothing`, `the_manifest_measurement_is_bounded`, and the existing bounded-stream tests |
| P1-2 independent manifest, substitution and TOCTOU defense, binding to verified bytes | `a_source_changed_after_authorization_is_refused_before_anything_is_written` (5 mutations), `a_member_changed_while_it_is_parsed_...`, `a_file_added_while_staging_...`, `changing_the_source_after_verification_cannot_alter_what_is_stored`, `a_source_that_is_itself_truncated_...`, `the_gate_manifest_hash_must_be_the_measured_one`, `the_manifest_hash_is_deterministic_...` |
| Owner authorization, lock/revoke serialization, vault fence kept | `locking_the_gate_mid_job_...`, `revoking_the_authorization_mid_job_...`, `a_foreign_gate_a_wrong_provider_and_a_vault_marker_write_nothing`; the historical-write ledger test now classifies the new entry point and the third `save_under_lock` call site |
| No raw replacement | `existing_raw_evidence_is_never_replaced` |
| Receipts match the disk | first test above counts raw, evidence, link files and projections against the receipt |

## Residual risks and limits (read before acceptance)

1. **The old `save_authorized_historical_import` is unchanged.** It is public, takes a caller-built inventory (the P1-2 self-comparison) and projects without raw evidence. The review rated this P1. Removing or demoting it means rewriting its callers and tests, so it was left as a documented capture-only primitive. **MP-IMPORT-APP-01 must use `import_authorized_source`**; the owner should decide whether to retire the old entry point.
2. **Path-based source walk.** Defense in depth only (lstat, no-follow open, handle checks, Unix identity). Not defended: swapped intermediate directories, a FIFO swapped in before the handle check on Unix, hard links, NTFS alternate streams. These need write access to the source folder. Content binding bounds what is read only **after** a hash is pinned; the first measurement pins whatever it read.
3. **Windows crash durability is not claimed.** Data files are fsynced; directory fsync exists only on Unix. Windows behaviour (true reparse points, NTFS locking, sharing violations from antivirus) is `NOT_RUN` until the Windows qualification.
4. **Gate lock held across the projection write** (the same I/O the existing authorized entry already does inside the gate). Hashing, spool reads and raw temp-file writes happen before it.
5. **After a resume**, the one item that was in flight may be counted `deduplicated` instead of `imported` in the receipt (`resumed = true`). The link records are unaffected (they hold only `projected` / `raw-only` / `rejected`).
6. **Receipts never over-claim lossless:** each member lists `unpreserved_bytes` (array framing, whitespace, text between Takeout cards). Gemini invalid UTF-8, tracker overflow and over-limit records still abort the job. Assets (images) are not ingested; a parser item that carries assets fails the job, and a capture with attachment storage paths is rejected (raw kept).
7. Order of two exports of the same conversation is the owner's choice: a newer import becomes the latest snapshot, except that a live capture is never displaced.
8. Capability TTL is at most 3600 s; very large exports are expected to resume under a new authorization. No performance numbers were measured (staging fsyncs every item); a 10k-conversation run is not measured.
9. Spool temp files from a hard crash (`.tmp*` in `imports/raw/..`) are not swept yet.

## Verification

Environment: Windows 11 x64, Rust 1.91.1, Node 24.21.0, local run by the implementing session on the branch head before the docs-only commit. Synthetic data only. Self-reported; **not** exact-head hosted CI and **not** an owner Windows requalification.

| Command | Result |
| --- | --- |
| `cargo test --locked -p mind-place-archive --tests` | PASS: 159 passed, 0 failed after review round 2 (57 unit, 19 archive, 5 generic, 8 historical-write authority, 3 sanitizer compat, 18 second-correction, 6 legacy, **32 raw-evidence publication**, 2 registration, 9 validation boundaries) |
| `npm run test:rust` (archive, capture-host, security, state, index, integration, documents) | PASS, exit 0 |
| `npm run test:web` | PASS: 333 tests, 329 passed, 0 failed, 4 skipped |
| `cargo fmt --all -- --check`, `npm run verify:repo`, `npm run typecheck` | PASS |
| `npm run build` | PASS (release build, `mind-place.exe` produced). A first attempt failed only because the shared Cargo target directory was outside the checkout; re-run through a directory link |
| Mutation check | With the member hash comparison disabled, `a_member_changed_while_it_is_parsed_...` fails; restored |
| True symlink member refusal (`a_symlink_in_the_source_is_refused`) | **BLOCKED locally** (this runner cannot create symlinks; the test prints `BLOCKED` and returns). Runs for real on Linux hosted CI. Junction/reparse behaviour on Windows: `NOT_RUN` |
| Hosted CI on the exact head | see PR #168 checks; not claimed here |
| Windows native qualification (NTFS locking, antivirus sharing violations, reparse points, directory durability) | **NOT_RUN** |
| Real ChatGPT or Gemini export | **NOT_RUN** (private; the layout and member selection are MP-IMPORT-APP-01) |
| Independent security re-review of the implementation | **NOT_RUN** (required by the card; the design review was an AI reviewer and advisory) |
| Throughput with a large export (10k+ conversations, staging fsyncs) | **NOT_RUN** |

## Handoff

`TASK_ID` MP-IMPORT-RAW-01 · `CLAIM` claimed by a Claude Code local session; no fencing generation · `BASE_SHA` `8350f8e` · `BRANCH` `work/mp-import-raw-01/atomic-raw-publication` · `PR` #168 (draft) · `LOCAL_REQUIRED` Windows qualification of the new tests and of crash/locking behaviour (NOT_RUN) · `HUMAN_REQUIRED` independent security re-review, then the owner's manual merge decision.

## Independent implementation review, round 1 (exact head `393a214`) — fixes

Verdict of the review: CHANGES REQUIRED (two P1, one P2). All three are fixed, each with a regression test that was checked against the defect (the guard was removed and the test failed).

| Finding | Correction | Regression test |
| --- | --- | --- |
| **P1** `discard_staged_job` could erase a finished job's receipt and state | It now needs the owner's unlocked gate for this archive (`gate`, `lease`), refuses a finished job (state `complete` **or** a receipt on disk, even when the state file is unreadable) with `E_JOB_COMPLETE`, never touches published raw/evidence/projections, and keeps superseded receipts. Unfinished spools, missing keys and unreadable state without a receipt can still be discarded | `a_finished_job_is_an_audit_record_and_cannot_be_discarded`, `discarding_needs_the_owner_gate_and_only_removes_unfinished_spools` (finished, locked gate, foreign gate, invalid key, missing key, corrupt state) |
| **P1** a finished-job replay returned success from a stale receipt | Completion now writes a per-item `ledger.jsonl`. A replay verifies, before returning, that every listed raw blob (existence and size), evidence record, disposition link and projection (`meta.json` and snapshot) still exists, else `E_PUBLISHED_MISMATCH`. A healthy replay reports `published_check = "presence-verified"` (a fresh run reports `published-now`). `verify_published_job(key, deep)` is a read-only check; `deep` also re-hashes raw blobs and parses evidence. `SourceImportRequest.repair = true` (off by default) re-stages and re-publishes a mismatched job, keeping the old receipt and ledger as `*.superseded-N` | `a_finished_job_is_checked_against_the_disk_before_success_is_reported` (missing raw, evidence, link, projection, ledger: fails without repair, repairs to a disk equal to the original with repair, old receipt kept), `a_same_size_corruption_of_a_raw_blob_needs_the_deep_check` |
| **P2** job key omitted `max_staged_raw_bytes` | Added to the key, so a different cap is a different job and a verified spool is never reused under a new cap | `resuming_with_a_different_raw_byte_cap_never_reuses_the_old_spool` |

Documented limits that follow from the review:

- The cheap replay check proves presence and size, **not content**: a same-size corruption of a raw blob is found only by `verify_published_job(.., deep = true)` (tested).
- The whole-folder manifest claim is bounded: after staging only paths and sizes are re-checked, so a same-size change to an **unselected** file between job start and publication is not detected at publication time (member bytes are hash-bound while parsed, and the job-start measurement covers every file).
- A job that was finished by an earlier build has no ledger; its replay fails with `E_PUBLISHED_MISMATCH` until a repair or deep verification is run (no such job exists outside tests).
- The old public `save_authorized_historical_import` remains an open owner decision (unchanged). The archive import surface is **not** raw-evidence-enforced while that API exists.

## Independent re-review, round 2 (exact head `b1c2795`) — fixes

The review confirmed the three round-1 findings as fixed and found two new integrity gaps. Both are fixed; each regression test was checked against its defect (guard removed, test failed).

| Finding | Correction | Regression test |
| --- | --- | --- |
| **P1** a missing, malformed or unrelated `receipt.json` bypassed the repair decision or was reported as a verified job | The finished-job check now starts from the receipt: it must exist and parse, describe this job (job key, manifest, importer version, provider, staged root, record count, members) and **equal what the ledger adds up to** (the same `tally` function builds both: outcome and blob/evidence counters, raw bytes, exceptions and omitted count). Any failure is a typed `E_PUBLISHED_MISMATCH`, which `repair = true` can recover (the old receipt, if any, is kept as `receipt.superseded-N.json`) | `a_missing_malformed_or_foreign_receipt_is_a_typed_mismatch_and_can_be_repaired` (missing, truncated, malformed, a valid receipt of another job, valid receipt with altered counts, valid receipt naming another manifest; each fails typed, then repairs to a disk equal to the original with a correct receipt) |
| **P2** deep verification did not bind provenance and disposition | The ledger now records the SHA-256 of the evidence and link records as written. Deep mode requires both files to equal those hashes **and** checks them semantically: the evidence id is re-derived from its fields, and manifest, provider, payload hash and size, member (hash, size) and span must agree with the job; the link must agree with the ledger on evidence id, importer version, disposition, reason, archive id and snapshot id | `deep_verification_binds_evidence_and_links_to_the_ledger` (valid-JSON-but-wrong link: other snapshot, `raw-only`, invented reason, other evidence id, other importer; evidence: edited metadata with the payload hash kept, other source identifier, other member path, moved span, other manifest, other provider; each fails deep, passes shallow as documented, and is restored) |

What the checks prove and do not prove: shallow = receipt/ledger consistency plus presence and size of raw, evidence, link and projection files. Deep = also byte equality of the raw blob, evidence and link with what the job wrote. Neither re-derives a projection from the raw bytes, and neither can prove the original source bytes were authentic (that is the manifest and member-hash binding at import time).

## Windows qualification findings F1 and F2 — fixes (head `0891160`)

Source: the Windows native qualification of `1703f36` (report and addendum on PR #168). Both defects were reproduced there and are now fixed; each fix has tests that fail without it, and the Windows scenarios that exposed them were re-run on the exact fixed head.

| Finding | Correction | Tests |
| --- | --- | --- |
| **F1** a file held open by another program (virus scanner) made an atomic replace fail with a raw `Access is denied (os error 5)` and no retry | `atomic_write`, the raw-blob install, the ledger install and the job-directory cleanup retry **only on Windows** and **only** on `ERROR_ACCESS_DENIED`, `ERROR_SHARING_VIOLATION` and `ERROR_LOCK_VIOLATION`, with a bounded back-off (10 ms doubling to 1 s, 9 retries, about 3 s in total). The temporary file survives a failed attempt, so nothing is rewritten. If the holder outlasts the retries the import fails with the stable, content-free code **`E_STORAGE_BUSY`** (no OS message) and the job stays recoverable | unit tests for the classification, the retry and its bound; Windows integration tests: a file held for 1.2 s is retried and the import completes after waiting; a file held for 8 s fails with `E_STORAGE_BUSY` (no `os error` text), nothing is published, and the rerun finishes the same job. Disabling the retry makes both fail |
| **F2** a hard kill left an orphaned `.tmpXXXXXX` file, so the disk was not identical to a clean run | Leftover temporary files (name exactly `.tmp` plus six letters or digits, regular files only, never links or directories, not recursive) are removed under the archive lock: at the start of every import (job directory, `items`, the raw/evidence/disposition shards), before each in-flight item (its conversation directory **and** its `captures` and `assets` subdirectories), and by the new maintenance call `sweep_orphaned_temp_files()` (all conversations, shards and jobs; refuses with `E_JOB_BUSY` while an import runs). Holding the archive lock guarantees no live writer owns such a file in those directories | unit tests of the exact-name rule and of the non-recursive sweep; integration tests that plant orphans in every location, keep look-alike names and a directory with the exact name, show a restart removes the in-flight item's leftovers while an already finished conversation's leftover needs the maintenance call, and show published data is untouched. The first Windows re-run found a gap (a leftover in `captures/`, trial K5); the sweep was extended and the test now plants one there (mutation checked) |

**Windows re-qualification on the exact head `0891160`** (synthetic data, real Defender on, harness outside the repository): 24 real process kills during a 300-conversation job (random times across staging and publication, every third trial with a second kill): **24/24** restarts finished, deep-verified, and equal to the clean-run digest; **8** of the kills left a temp file and **0** remained after the restart (before the fix: 2 of 16). Targeted scanners holding share-read handles on the replaced files during a 300-conversation import: **8/8** imports succeeded (before the fix: 3 of 4). Still not claimed: power-loss durability, third-party antivirus, true symlink behaviour beyond the earlier addendum, real exports.

Local verification of this head on Windows: `cargo test --locked -p mind-place-archive --tests` **168 passed** (62 unit, 36 in the raw-evidence file incl. 2 Windows-only), 0 failed; the other Rust crates (`npm run test:rust` set, built with one job because the machine was short of paging file) **411 passed, 0 failed, 2 ignored** in total; `cargo fmt --all -- --check` PASS. Exact-head hosted CI: see the PR checks; the two Windows-only retry tests do not run on the Linux job.

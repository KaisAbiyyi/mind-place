# MP-4L1 — local Windows NTFS qualification

## Latest result — full local native qualification PASS (2026-10-22)

- **Product under test:** merged first-party product commit `6b93b05d55a795fb4fcac11cdfeb8fa6dff5b2f9`, including fixes from security PR #38. Windows product CI #294 and storage 10K/100K/1M CI #137 previously passed.
- **Full original Codex harness:** **10 PASS / 0 FAIL / 0 BLOCKED**; `native_exit:0`. The original harness source is unmodified (SHA-256 `9640e79ece8113454576fe7574eaebe0f6df88e552686b5c26a686f278a4120a`). The executable was recompiled from the exact merged product code (SHA-256 `c5a848762bc39ac64b4e8fd2510469903d097727a936fd09b8a588b167eb022a`).
- **Symlink blocker resolved without changing product code or permanent Windows settings:** A normal-user control run at that exact SHA returned **9 PASS / 0 FAIL / 1 BLOCKED** because native `std::os::windows::fs::symlink_file` creation returned Windows privilege error 1314. A one-time administrator-approved process reran the same harness on native D: NTFS; both real file and directory symlinks were created and denied by the Mind Place boundary: `symlinks_created:2`, `outside_hits:0`. No privilege was added to the normal-user process and Developer Mode remained unchanged.
- **Revocation fixes independently requalified:** search returns zero disclosed revoked hits; index generation remains 1→1 after revoke; junction race has zero outside-data reads. All ten synthetic native cases passed.
- **Durable evidence:** [merged-code native receipt](evidence/mp-4l1-20261022/native-receipt-merged-elevated.json) and [native receipt metadata](evidence/mp-4l1-20261022/run-metadata-merged-elevated.json). Original on-disk complete transcript and fixtures remain under `D:\\projects\\mind-place-mp4l1-fix-evidence\\run-04-elevated`. Raw native receipt SHA-256 `76c501ee12ee2b787229664926d730808b2c473c8c7bc383b2e3bc2312cbb81e`; the JSON here preserves its semantic result (potential trailing newline differences do not change the native receipt).
- **Scope:** native Windows NTFS core qualification. MP-4L2 representative private corpus, MP-4U installed-app owner acceptance and real wall-clock scheduling remain **separate**, not silently marked PASS.
- **Worktree/ownership:** The Codex-owned local checkout and unrelated WIP were not edited; this follow-up was performed by ChatGPT through a separate D: detached main checkout and a bounded elevated native fixture runner, then added to this evidence PR after user requested resolution. The original historical failing baseline below is retained verbatim as a security regression record.

---

## Historical result — pre-fix baseline (superseded)

Claim: Codex (GPT-6), desktop local Windows, session `20261022-mp4l1-01`, generation 1; lock `windows-fs`; [issue #35](https://github.com/KaisAbiyyi/mind-place/issues/35). Live Notion task is **Claimed**. Owner explicitly requested this unit. No other unit is taken over.

Product baseline: `634a78c9d6900a387f7458aa5e2b1cc5dec1b2dd` (MP-4F #32 merged, final-head Windows product and storage 10K/100K/1M CI PASS). `git pull --ff-only` completed before qualification. Original checkout's unrelated `Cargo.lock` is preserved. Isolated qualification checkout has unchanged product source.

## Original pre-fix outcome: FAIL (archived, not current)

- DONE: prerequisite/claim checks, pull, isolated branch, native harness and PowerShell evidence runner.
- DONE: final native D: NTFS fixture run-05 at harness `eed5dd97f257269720015551d681fca92281d057`: **7 PASS, 2 FAIL, 1 BLOCKED**. Revoke executes on a separate thread; the security callback synchronizes only and continues returning a valid unlock lease. [Native receipt](evidence/mp-4l1-20261022/native-receipt.json), [runtime/source/binary hashes](evidence/mp-4l1-20261022/run-metadata.json), [regression checks](evidence/mp-4l1-20261022/regression-checks.json).
- FAIL: initial exact-baseline `cargo test --locked` refuses stale committed `Cargo.lock`. A qualification-only Cargo resolution refresh is isolated and must not be committed or treated as exact dependency qualification. Product repair belongs to cloud. Diff retained outside repository.
- NOT RUN: installed app, representative private corpus MP-4L2, owner UI acceptance MP-4U, five-minute scheduler wall-clock timing, extended scanner enumeration/root-swap stress. This bounded failing run does not qualify the entire phase.

| Native case | Result | Evidence |
|---|---|---|
| Actual root/descendant junctions | PASS | Two junctions created; outside reads denied, outside search hits zero |
| File and directory symlinks | BLOCKED | File symlink creation denied by Windows privilege error 1314; neither symlink case accepted |
| Hardlink rejection and failed-refresh atomicity | PASS | Outside hardlink denied; generation unchanged |
| Root replacement and authority restart | PASS | Replaced identity denied after reopen; no stale search hit |
| Missed-event reconciliation and immutable history | PASS | Actual rename/edit/add/delete without hints; explicit full rescan repairs membership; five invalidation hints tested; old raw version preserved |
| Concurrent junction swap/read | PASS | Safe baseline read; 5 real swaps, 128 denied concurrent reads including 4 proven live-junction overlaps, zero outside reads; no successful normal read occurred during this bounded race interval |
| Revoke during scanner work | PASS | Scan aborts, no current inventory published |
| Revoke immediately before index commit | **FAIL** | Revoke completes on second thread at checkpoint 34/35, refresh succeeds, generation advances 1 to 2 |
| Revoke immediately before search return | **FAIL** | Revoke completes on second thread at checkpoint 13/13, response still contains 1 hit; subsequent request returns zero |
| Invalid core callback guard | PASS | Injected callback cancellation denies scan/index/search; failed early index refresh preserves generation. Actual SecurityGate/Tauri Lock path not exercised |

The two failures are handed to **[cloud fix issue #37](https://github.com/KaisAbiyyi/mind-place/issues/37)**. Search currently performs only the injected unlock check after accumulating hits; the desktop injects an unlock-lease check, not root approval. Similarly, refresh checks root identity before its final precommit unlock checkpoint. Root revocation and Lock are different authority changes. No product repair was made in this local qualification unit.

Run-01 established both failures with direct checkpoint injection. Run-02 confirmed both with second-thread revocation but exposed a **harness writer sharing violation** in the separate junction stress case, not an outside-data leak. That failed attempt remains in the external ledger. Run-03 added bounded sharing retries and a safe pre-race read, but its junction PASS did not yet prove live-junction overlap. Runs 04–05 add that explicit overlap requirement, bounded PowerShell child/worker lifetimes, fresh evidence directories, and per-run isolated Cargo targets. All runs continue to reproduce the two product failures. No permission boundary or product threshold was relaxed.

Independent read-only review by a `gpt-6-luna` reviewer confirmed both product findings. Its five material harness findings were addressed in one repair pass: live-junction overlap, exact-root/reparse evidence-path rejection, no historical evidence overwrite, bounded junction subprocess/worker, and isolated/recorded Cargo output. Additional source guards reject tracked/untracked protected build inputs and configuration. Negative runner checks rejected the repository itself, a junction alias, an existing evidence directory, and an untracked protected input; the historical receipt hash stayed unchanged. These fixes were reverified by final native run-05. The reviewer did not certify a second full review of the repaired harness; the author verified that bounded repair.

Regression evidence: repository contract, TypeScript typecheck, 111 web tests, 134 Rust tests and formatting **PASS**; one pre-existing scale benchmark remains ignored. These are supplemental checks with the recorded qualification-only lock refresh. `npm run build`/packaging were **NOT RUN**: this PR changes only explicit qualification artifacts and the native gate already failed. No milestone, exact-lock, installed-runtime or release acceptance is claimed.

## Reproduction

Run `scripts/qualify-ntfs.ps1 -EvidenceDirectory <absolute external NTFS folder>` on Windows. It checks product sources against the exact product SHA and rejects lock drift by default. An explicit `-AllowQualificationLockRefresh` permits supplemental investigation with a recorded HOLD; it can never produce an overall PASS while dependency drift remains.

For this baseline only, first preserve the committed lock and record the `--locked` failure. A diagnostic `cargo test --offline -p mind-place-state --test document_roots --no-run` minimally refreshes the isolated checkout's dependency resolution; preserve its diff before invoking the diagnostic override. Do not use an unconstrained full lock regeneration: it may upgrade existing packages. The [actual minimal lock delta](evidence/mp-4l1-20261022/qualification-lock-diff.json) is retained as evidence, not a proposed product fix.

The harness invokes the actual state, scanner, immutable object store and document FTS APIs. Reparse entries are created on disk; race mutations occur on a separate thread. Symlink privilege failures become explicit BLOCKED cases, not silent passes. Missed events are modeled by actual file changes without notifications, followed by the product's full reconciliation API; there is no claim of lossless OS watcher delivery.

Synthetic fixtures remain outside the repository for inspection. No recursive cleanup, private archive, secret, real account, or installed runtime is used. `Cargo.lock` refresh and generated evidence/binaries stay uncommitted. Any discovered product bug is returned as a reproducible cloud fix handoff rather than silently repaired in this local gate.

## Handoff

PROJECT/REPO: Mind Place / `KaisAbiyyi/mind-place`. TASK: MP-4L1. CLAIM: Claimed, Codex (GPT-6), desktop local Windows; session `20261022-mp4l1-01`, generation 1. ROLE: native qualification executor. BASE: `634a78c9d6900a387f7458aa5e2b1cc5dec1b2dd`. BRANCH: `feature/mp-4l1-ntfs-qualification`. TESTED REMOTE CHECKPOINT: `eed5dd97f257269720015551d681fca92281d057`; receipt-only commits may follow. PR: **[draft #36](https://github.com/KaisAbiyyi/mind-place/pull/36)**. Changed logical resource: `windows-fs` qualification artifacts only. Product source, schema, permissions and packaging unchanged. Hosted product/storage CI is tracked separately on PR #36; it was running on receipt checkpoint `a0aa9b2` at the final remote inspection. No hosted green result is claimed or substituted for this failing native gate; draft remains unmerged.

NEXT: cloud unit claims/fixes #37, independently reviews the critical authority boundary, commits the canonical lock and passes final-head checks; then pull the new exact merged SHA for MP-4L1 rerun. Symlink privilege must be available for that subcase to pass; this task did not change Windows security settings. LOCAL_REQUIRED: MP-4L1 rerun; MP-4L2 remains gated. HUMAN_REQUIRED: MP-4U later, not exercised here.

RECOVERY: resume from draft #36's latest pushed checkpoint. Retained synthetic evidence is under `D:/projects/mind-place-qualification/mp-4l1-20261022/` (runs 01–05, lock baseline/delta and Rust regression transcript). These local fixtures are not a private corpus. Keep ownership generation 1 until explicitly released/fenced; no second writer may modify this unit. Original `D:/projects/mind-place/Cargo.lock` SHA-256 stayed `A8CAA0FEFBCAB0A17A392E85CC2D6376F79D9C4456CA766E1AC514026B68A2EC`. The qualification-only refreshed lock is preserved as external evidence; restore the isolated checkout to its committed lock after investigation. No changes to the user's original checkout were committed.

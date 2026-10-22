# MP-4L1 — local Windows NTFS qualification

Claim: Codex (GPT-6), desktop local Windows, session `20261022-mp4l1-01`, generation 1; lock `windows-fs`; [issue #35](https://github.com/KaisAbiyyi/mind-place/issues/35). Live Notion task is **Claimed**. Owner explicitly requested this unit. No other unit is taken over.

Product baseline: `634a78c9d6900a387f7458aa5e2b1cc5dec1b2dd` (MP-4F #32 merged, final-head Windows product and storage 10K/100K/1M CI PASS). `git pull --ff-only` completed before qualification. Original checkout's unrelated `Cargo.lock` is preserved. Isolated qualification checkout has unchanged product source.

## Scope and current ledger

- DONE: prerequisite/claim checks, pull, isolated branch, native harness and PowerShell evidence runner.
- IN PROGRESS: native D: NTFS fixtures, junction/reparse/symlink/hardlink/race, rename/root replacement/restart, missed-event reconciliation, concurrent revocation and lock boundaries.
- FAIL: initial exact-baseline `cargo test --locked` refuses stale committed `Cargo.lock`. A qualification-only Cargo resolution refresh is isolated and must not be committed or treated as exact dependency qualification. Product repair belongs to cloud. Diff retained outside repository.
- NOT RUN: installed app, representative private corpus MP-4L2, owner UI acceptance MP-4U, five-minute scheduler wall-clock timing.

## Reproduction

Run `scripts/qualify-ntfs.ps1 -EvidenceDirectory <absolute external NTFS folder>` on Windows. It checks product sources against the exact product SHA and rejects lock drift by default. An explicit `-AllowQualificationLockRefresh` permits supplemental investigation with a recorded HOLD; it can never produce an overall PASS while dependency drift remains.

The harness invokes the actual state, scanner, immutable object store and document FTS APIs. Reparse entries are created on disk; race mutations occur on a separate thread. Symlink privilege failures become explicit BLOCKED cases, not silent passes. Missed events are modeled by actual file changes without notifications, followed by the product's full reconciliation API; there is no claim of lossless OS watcher delivery.

Synthetic fixtures remain outside the repository for inspection. No recursive cleanup, private archive, secret, real account, or installed runtime is used. `Cargo.lock` refresh and generated evidence/binaries stay uncommitted. Any discovered product bug is returned as a reproducible cloud fix handoff rather than silently repaired in this local gate.

## Handoff

Branch: `feature/mp-4l1-ntfs-qualification`. Checkpoint/PR, native receipts and exact result will be filled after execution. MP-4L2 remains gated until MP-4L1 PASS; MP-4U remains owner-only. Review independent qualification artifacts before integration; no product merge or acceptance is implied by this checkpoint.

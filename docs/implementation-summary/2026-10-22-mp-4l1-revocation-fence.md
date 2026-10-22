# MP-4L1-FIX — live root authority fence

**Issue:** [#37](https://github.com/KaisAbiyyi/mind-place/issues/37).
**Owner:** ChatGPT (GPT-6); claim session `20261022-mp4l1-fix-01`.
**Baseline:** `634a78c9d6900a387f7458aa5e2b1cc5dec1b2dd` (merged MP-4F).
**Scope:** product fix only, independent of Codex's `windows-fs` native acceptance #35/#36.

## Defects

Native MP-4L1 showed two completed root revocations overtaking the last
unlock-only checkpoint: one revoked-root search hit was returned, and one
new document projection generation was committed despite prior revocation.
The old unlock lease did not represent root authority. Source validation
earlier in the pipeline did not make the final boundary atomic.

## Repair

- `StateStore::with_approved_document_roots` opens an IMMEDIATE transaction
  against the **authority** SQLite DB, checks terminal tombstones, current
  approval, registered root/digest, authorized volume/file IDs and **native
  root identity**, pins the approved Windows directory with a no-delete-share
  handle across the short final action, then releases the transaction. A
  concurrent root rename/replacement or authority revoke cannot finish
  during the fenced action. No scanning, parsing or caller callback waits
  occur within the writer fence.
- `DocumentIndex::refresh_root` completes its costly source/inventory
  checks and caller cancellation checkpoints before the fence, and commits
  the disposable FTS generation only inside it. An invalid root rolls back
  the projection transaction without publishing a new generation.
- `DocumentIndex::search` checks the final caller lease, then validates
  **all** roots represented by the candidate result under the authority
  fence before handing hits back. Mixed-root results fail closed if a
  required root was revoked; they never partially disclose a revoked root.
- The committed workspace lock is refreshed minimally for an existing
  `chrono` workspace dependency and transitive `httpdate` and
  `tokio-macros` entries. Existing package versions are retained.

## Verification and boundaries

- Three Windows `mind-place-index` regressions inject a completed revoke
  from another thread at the final search/commit lease checkpoint and test
  a mixed-root search. A fourth `mind-place-state` test holds the live
  publication fence, proves a concurrent revoke **cannot** complete during
  the action, releases it, and proves terminal revocation wins thereafter.
  A fifth test proves an NTFS rename fails while the directory is pinned,
  and a root replacement after release does not inherit approval.
- Windows index `documents` tests: 9/9 PASS. Root authority tests: 9/9 PASS.
- `cargo test --locked` default Rust workspace: PASS.
- `cargo test --locked -p mind-place --bin mind-place`: 13 PASS,
  1 pre-existing ignored after staging build-only assets.
- `npm run verify:repo`, `npm run typecheck`, `npm run test:web`:
  PASS, 111 web tests.
- `cargo fmt --all -- --check`, `git diff --check`: PASS.

**Not yet accepted:** The native ten-case MP-4L1 harness must be rerun
against the final merged product SHA and its unchanged evidence harness,
including a real NTFS fixture on D:. Symlink setup may remain BLOCKED by
Windows privilege error 1314. MP-4L2 representative corpus and MP-4U
installed-owner acceptance are separate gates. This document is not a
claim that MP-4L1 has passed.

**Review limitation:** SQLite's 5-second writer busy timeout means unusually
slow filesystem operations or FTS publication may delay owner revocation or
yield an explicit busy error requiring a retry. End-to-end owner retry UX and
response-dispatch-time invalidation remain outside these unit tests.

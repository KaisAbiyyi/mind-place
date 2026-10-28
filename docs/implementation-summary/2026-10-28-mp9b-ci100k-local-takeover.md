# MP-9B + MP-CI-100K-01 authorized local takeover

Date: 2026-10-28 (Asia/Jakarta). Status: **IN PROGRESS / PARTIAL / DRAFT / DO NOT MERGE**.

Single execution owner: Codex local Windows, instance `codex-local-20261028-01a18146`. Owner-directed transfer supersedes the former execution owner in issue [126](https://github.com/KaisAbiyyi/mind-place/issues/126#issuecomment-6462618046) and [128](https://github.com/KaisAbiyyi/mind-place/issues/128#issuecomment-6462618767). Locks remain `crypto-migration` and `ci-document-scale-workflow`; no other agents were dispatched.

## Baseline and preservation

- PR #127 begins at `255db9633eebcf534d2fd01e7ce46bc65c8f2d73`; PR #129 at `d59f7fbfbeed3aee938b57539d234fd9423360c8`.
- Both latest remote heads, open PRs, authorization and strict reviews were read before editing. GitHub remains the durable source. Both PRs remain Draft; no merge/release is authorized.
- Managed task worktrees isolate both branches. Original `D:/projects/mind-place` remains on its original branch with its preexisting `Cargo.lock` change; its hash and status are recorded in the local evidence directory.
- Only synthetic fixtures are used. No real archive, vault, recovery key, account credential or production secret is read or migrated.

## Implemented and verified

### Windows migration flush

The exact reported unit regression was reproduced with Rust **1.90.0**, MSVC and vendored SQLCipher/OpenSSL on local Windows. Temporary test-only instrumentation identified `migration.state.sync_all` returning OS error **5**. A separate minimal native reproduction demonstrated that a read-only handle fails while a read-write handle succeeds. This matches the [Windows FlushFileBuffers write-access contract](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers).

The converter now checks SQLCipher connection close, reopens the staged ciphertext with read/write access **without truncation**, reads its header, flushes it and closes that handle before hard-link publication and partial-file removal. The regression also exercises interrupted partial cleanup, completed-link cleanup, changed values with unchanged row count and changed row identities.

| Check | Result |
| --- | --- |
| Exact Windows regression before repair | FAIL, reproduced OS error 5 at `sync_all` |
| Exact Windows regression after repair | PASS, 1 test |
| Real SQLCipher/AEAD migration integration | PASS, 1 test (original coverage; additional crash/domain coverage pending) |
| Original CI-gate baseline | PASS, 20 synthetic Python tests |
| Dispatch acceptance regressions | Confirmed RED: dispatch proof rejected; PR zero accepted |
| Hosted exact final-head Windows/Ubuntu CI | NOT RUN on the unfinished takeover candidate |
| Fresh final 100K/1M qualification | NOT RUN; expensive execution remains gated |

Local evidence and build tooling live under `D:/projects/mind-place-takeover-evidence-20261028/`. The first SQLCipher compile failed before test execution because native Perl was missing; an isolated, SHA256-verified portable Strawberry Perl and pinned Rust toolchain resolved the build prerequisite. No dependency or encryption requirement was weakened.

### Authenticated restart and real migration qualification

- Journal v2 is AEAD authenticated in a separate `migration-journal/v2` domain and bound to the canonical source/destination path pair, source inventory and verified encrypted-output inventory. Prior unauthenticated pre-release journals are refused; no automatic adoption/upgrade of arbitrary journals is attempted.
- `resume_owner_consented` reconstructs a fresh plan only after journal authentication. An existing destination must match the authorized published output hash and phase; unrelated destinations, wrong vaults, relocated/tampered journals and simultaneous staging/published ambiguity are rejected. Ordinary `inspect` still rejects an existing destination.
- A cross-process operation lock serializes begin/resume/rollback. Windows publication uses write-through rename; Linux uses atomic no-replace `renameat2` plus parent sync. Journal replacement is distinct from never-overwriting vault-directory publication.
- Real converter progress boundaries support interruption without exposing data/key material. Actual child processes exit abruptly after state, captures, raw archive, document objects, conversion, verification and publication-before-journal. Fresh processes recover all seven boundaries and repeat execution idempotently before runtime activation.
- Qualification preserves stable registered source identity, noncontiguous SQL row IDs, NULL/BLOB/REAL values, two capture revisions and metadata, inline-image attachment bytes, all raw archive records, the full manifest, and a document spanning multiple AEAD chunks. Source bytes are compared before/after.
- Additional real-converter tests reject changed SQL values with equal row count, ciphertext corruption, source mutation and orphan partial/plaintext staging. Unknown staging bytes are never certified; explicit rollback quarantines staging without deleting source. A new RED test exposed mutation after the verification callback; a final output-hash check closes that publication gap.

Local verification: **8/8 migration security tests PASS**, **3/3 real-converter test entries PASS** (one is the child probe; the parent verifies seven actual crash exits), full `mind-place-state` + `mind-place-index` SQLCipher suite PASS, repository contract/typecheck PASS, **186/186 web tests PASS**. The ignored 100K test was not run by these suites. Initial repair checkpoint `7a38b731cc7307cbc239e6b3ecf41a0a696c23ad` independently passed [hosted Windows/Ubuntu SQLCipher](https://github.com/KaisAbiyyi/mind-place/actions/runs/40648850426) and [storage qualification](https://github.com/KaisAbiyyi/mind-place/actions/runs/40648850458); its Windows Product CI is still in progress at this checkpoint. These prior-head receipts do not qualify subsequent source changes.

CI-only checkpoint on PR #129: `0958ee0acd44d3cff206fbb3a0580c5dcf7c4f6b` adds PR-bound dispatch, independent tree/fingerprint verification, original scale-attempt preservation and distinct Draft/final checks. **28 Python tests + actionlint PASS** locally. Exact-head [Windows Product CI](https://github.com/KaisAbiyyi/mind-place/actions/runs/40651547354) and [development/100-file workflow](https://github.com/KaisAbiyyi/mind-place/actions/runs/40651547326) SUCCESS. Full 100K was SKIPPED/PENDING; that green development result is not full qualification. Historical run #40499159529's live artifact list contains only the 100-file receipt, so no raw full-tier proof can be recovered.

### Complete candidate domains and owner entry point

- Added an immutable encrypted raw-file map before normalization, retaining original generic-import records and missing source timestamps. Migrated attachment reads now use the encrypted map, AEAD, byte/MIME/digest validation and the live epoch. A test removes access to the entire plaintext source and still reads the migrated image; traversal, wrong conversation and locked reads fail.
- Added the explicit Windows `mp-vault-migrate begin|resume|rollback` source maintenance runtime. It requires an existing unlock epoch, typed consent and recovery confirmation, acquires real Windows SQLite sharing barriers plus the legacy capture OS lock, and publishes to the application-discovered child vault. All working artifacts remain outside the source inventory. Interrupted wrapped authorization can reopen; no secret appears in arguments/environment/logs and no service starts. It is not an added installer shortcut/graphical screen. [Owner contract and full writer inventory](../implementation-plan/2026-10-28-mp9b-owner-runtime-and-writers.md).
- The document scan now holds a shared writer fence for its whole object/state/FTS generation. A real paused production writer exposed the prior cut (restored search had zero hits); the fixed test proves backup waits and restored search/preview work. Snapshot/restore validates schema, SQLite/FK/FTS, all catalog revisions, raw-map objects and document state references before publication. Missing capture/invalid authority are rejected. Publication uses canonical overlap checks and durable atomic no-replace rename on Windows/Linux.
- Windows descriptors are pinned against overwrite/replacement and checked for current owner plus exact protected DACL on the same handle. Tests cover broad ACL rejection, real separate-process anonymous-token denial, forged/replayed/oversized IPC and old sessions after unlock rotation. The anonymous token is not a different installed Windows account. Actual hostile-account and owner-installed gates remain NOT RUN.

Fresh complete keyed suites: **231 passing test entries / zero failures / two explicitly ignored scale tests**, including all state/index/security/capture/MCP suites. Real migration restart has four entries (one helper; the parent verifies seven actual child exits), backup consistency + online backup four, offline owner runtime two, capture-host library five and native Tauri IPC two (one helper). All use synthetic data. Repository/typecheck and **186 web tests PASS**; CI Python28/actionlint PASS. SQLCipher diagnostics in intentional wrong-key/corruption tests do not indicate a failing test. The required default-feature baseline/build and final combined-head hosted receipts are tracked below/in PR conversation as they complete.

Earlier durable `6a638980848a8e51b8a869723b68ea8ae303ade5` independently completed [Windows Product CI](https://github.com/KaisAbiyyi/mind-place/actions/runs/40652214435), [Windows/Ubuntu SQLCipher](https://github.com/KaisAbiyyi/mind-place/actions/runs/40652214489), [storage](https://github.com/KaisAbiyyi/mind-place/actions/runs/40652214472) and visual CI. Its old Draft full-scale guard was intentionally red; the integrated #129 workflow replaces that reporting. Those receipts remain historical and do not certify the later candidate.

## Remaining execution ledger

1. Finish the required default baseline/desktop build and exact combined-head hosted Windows/Ubuntu checks; freeze that source before one full 100K qualification. No expensive run while a relevant cheap gate fails.
2. Push the coherent integrated #127 candidate and retarget its base to #129 after the merge is durable. Keep both Draft. Exact head/receipt URLs belong in the PR and issue records; no stale checkpoint is promoted to final PASS.
3. Preserve real different-account Windows and installed-owner recovery/migration qualification as NOT RUN. Independent reviewer approval is PENDING; implementor self-review cannot substitute. No owner data was read or migrated.
4. Recomputed the entire live Notion DAG: Now MP-9B + MP-CI-100K-01; Up Next MP-9C/9D/9E with all other AND prerequisites Done; Last Done five accepted tasks. Optional research/optimization is not authorized by prerequisite completion alone. No focus cap applies to Now/Up Next and no task was falsely marked Done.

## Integration ruling

The four shared CI implementation files are owned by PR #129. Integrate its validated commits into PR #127 without rewriting published history, then use a stacked PR base so the security diff does not independently own the CI implementation. A single frozen **combined** #127 candidate is the full-scale target. An infrastructure review may reference that integration receipt, but a different fingerprint or another PR must never be relabeled a fresh same-source 100K PASS. No historical missing raw artifact is fabricated or reconstructed from log text.

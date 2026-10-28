# MP-CI-100K-01 — Source-bound MP-10C qualification gate

Issue: [#128](https://github.com/KaisAbiyyi/mind-place/issues/128) · [Notion task](https://app.notion.com/p/406c4a6aaf7381c89564ef41a5c71336). This is CI-only work. It does not modify MP-9B or reopen the accepted MP-10C workload.

## CI policy and qualification scheduling

On every PR the cheap Python gate (including real shallow-clone tests) and Windows **100-file** smoke run. A Draft without matching evidence reports **qualification PENDING / NOT RUN** in a successful informational development job; this is not a 100K PASS. The separate **100K qualification accepted** job runs on `ready_for_review`, subsequent ready-PR pushes, manual/final dispatch and release tags. It fails without authenticated fresh or matching prior full evidence. Configure that named job as a required merge check; a green development workflow alone does not grant acceptance. Both takeover PRs remain Draft until independent approval.

Full indexing waits for the smoke plus successful **same-source** Windows Product CI and, when present, Windows/Ubuntu SQLCipher CI. The full job rechecks proof after acquiring its concurrency slot. Manual/final dispatch requires a positive `pr_number` and selection of that open, same-repository PR's current head branch; default-branch, stale-head, fork, closed-PR and PR-zero dispatches fail before indexing. All jobs checkout the gate's frozen commit. Dispatch tests the exact PR head; ordinary PR runs test the synthetic merge. Reuse remains same-PR and requires identical effective inputs, so an unrelated push can reuse either event's verified evidence without treating branch and merge SHAs as interchangeable.

The source/dependency fingerprint includes the tracked object IDs, file modes and paths of production Rust code, tests, lockfile/toolchain, security, authority, and the scale workflow itself; non-input documentation/templates and unrelated workflows are deliberately excluded. This avoids GitHub's cumulative PR paths filter. No reuse is possible until matching *successful* full scale provenance exists.

### Split evidence and safe retry

1. Full Windows job runs the unchanged **100,000 files / 1,000,000 FTS chunks** test; validates the exact-tier raw receipt and **immediately uploads** `mp10c-raw-100000-attempt-N` (90-day retention).
2. The lightweight `Source-bound 100K provenance (no indexing)` job downloads that original scale-attempt artifact and checks the frozen checkout fingerprint, PR identity, branch SHA and event provenance. A PR merge's second parent is read with `git cat-file -p HEAD`, including in shallow checkouts. Its `mp10c-qualified-100000-attempt-M` artifact contains the exact raw receipt plus version-2 provenance recording both the original scale attempt N and attestation attempt M.
3. If provenance alone fails, **rerun failed provenance/acceptance jobs only**, never the successful indexing job. The raw artifact remains inspectable and attempt-specific names prevent accidental overwrite. Future reuse verifies the original full job through `/attempts/N/jobs` and attestation through `/attempts/M/jobs`, both required step sets, run/SHA/PR/event binding, both unexpired artifacts and their independent GitHub SHA256 ZIP digests, exact raw 100K assertions and byte equality. It independently fetches the qualified Git commit/tree from GitHub, validates merge parents or dispatch PR history, and recomputes the effective-input fingerprint from that tree instead of trusting a fingerprint merely printed inside a proof.
4. If raw evidence, job status, attestation or provenance is missing, stale, expired, tampered, or mismatched, **fail closed**. A green lightweight gate, skipped heavyweight job, or prior failed workflow is never new 100K PASS. Old incompatible artifact schemas are not accepted.

### Recovery of 2026-10-27 original failed run

[Run #40499159529](https://github.com/KaisAbiyyi/mind-place/actions/runs/40499159529), authored source `3c5d7c3c0d2c8645ed83e4c4d29f2f68dc56dfe2`, synthetic merge `2e40048f79df4cdb558866ff52b7d6afd1f798bf`: the GitHub Windows job log proves the 100K test and exact raw-tier validation finished successfully; the overall workflow failed on the old shallow checkout attestation. Only a **100-file smoke artifact** was uploaded, so there is **no authoritative raw 100K artifact to recover or reuse** under the revised contract. Printed log JSON may be audited as historical performance evidence but cannot substitute for an authenticated raw file, successful attestation and correct source fingerprint. Never invent a receipt or mark the run PASS.

### Current performance evidence

Historical run log: 100K fixture generated in about **68.2 seconds**; initial indexing 10 × 10K roots took **15,569.2 seconds**, and first-root reconciliation took **1,067.6 seconds**. Total exact test **16,959.1 seconds**. Indexing clearly dominates. Per-stage scanner/extraction/FTS write time is **not individually instrumented** yet; do not attribute the entire indexing time to FTS without profiling. Keep 10K-per-root cap, authorization/revocation, raw byte integrity and full 1M FTS chunks unchanged.

### Operator instructions

- On Draft: run cheap regressions, Windows 100-file smoke and normal security/product CI. Development can be green while 100K remains explicitly PENDING; final acceptance is a distinct check.
- Keep both takeover PRs Draft. Once the combined candidate is frozen and cheap Windows/Ubuntu checks pass, dispatch this workflow on its exact head branch with `qualification=final` and `pr_number=127`. The resulting receipt is reusable by PR #127 when merge checkout inputs are identical. No need to promote Draft merely to run the final test.
- If attestation alone fails after a successful raw upload, inspect the raw artifact, then rerun the provenance job alone. Validate the final **whole workflow** outcome and qualified artifact before claiming acceptance.
- A dispatch receipt records its actual event and head checkout, not a synthetic merge. Requalify if merge integration changes effective source/dependency/test inputs. Version-1 artifacts cannot satisfy the version-2 contract.
- Cheap regression command: `python3 -m unittest discover -s scripts/tests -p test_mp10c_ci_gate.py -v`. Real depth-one clone coverage is mandatory.

### Existing acceptance held constant

100K synthetic physical documents, 1M FTS chunks, 10K per approved root, raw source preservation, SQLite/FTS integrity, Windows product/SQLCipher/security/revocation behavior and all unrelated acceptance gates remain mandatory. MP-10C prior Done/PASS evidence is historical and not a PASS on arbitrary new source. MP-9B remains independently owned by the `crypto-migration` lock. No automatic merge before independent review.

## Integration ownership for PR #127 and #129

The workflow, gate script, gate regression suite and this document are owned by **#129** (`ci-document-scale-workflow`). Integrate #129's validated commits into #127 by a normal merge and set #127's review base to #129's branch while stacked. This removes duplicate CI ownership from the security diff, preserves published history, and makes the final combined #127 head the expensive qualification target. Independent review may use that run to validate the infrastructure's integrated behavior; it must not claim that standalone #129, with different product inputs, earned a separate fresh 100K PASS. After independent approval, the integration order is #129 then #127; this handoff authorizes no merge. Moving the base later requires recomputing the fingerprint and preserving the same-PR proof checks.

The full Windows test has its own PR-bound concurrency group; lightweight gate and smoke jobs are not serialized behind 100K. Gate failure, expired or missing artifacts, failed/cancelled/skipped full steps, a changed source tree, wrong run attempt or event mismatch all deny acceptance. Logs from the original failed run remain historical evidence only; there is still no preserved authenticated 100K artifact to recover from that run.

## Local-first takeover verification (2026-10-28)

Single Codex local Windows execution owner, `codex-local-20261028-01a18146`, authorized in issues #126/#128. Before changes, 20 tests passed; new regression tests reproduced rejected manual proof and accepted PR-zero identity. The repaired suite has **28 passing tests**, including real shallow clones, a real attestation CLI process, exact event/head/branch binding, original-scale-attempt retry, independent GitHub-tree fingerprint verification, digest/tamper/failure denial and prerequisite CI identity. Actionlint 1.7.12 validates the workflow. Synthetic receipts used by unit tests are local fixtures, never uploaded qualification evidence. Hosted exact-head CI and a fresh frozen integrated 100K run remain pending; no new scale acceptance is claimed.

Live GitHub artifact inventory for historical run #40499159529 was rechecked on this date: only artifact `12731159218` (`mp10c-document-100`) exists, with ZIP digest `sha256:44728084c0de2f3157fbecaeb9323cdc626b423de3cbad6d5c2f9c215e49b8f2`. There is no retained raw 100K artifact. New workflow/script source also changes the effective fingerprint, so historical log output cannot qualify the new candidate.

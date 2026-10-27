# MP-CI-100K-01 — Source-bound MP-10C qualification gate

Issue: [#128](https://github.com/KaisAbiyyi/mind-place/issues/128) · [Notion task](https://app.notion.com/p/406c4a6aaf7381c89564ef41a5c71336). This is CI-only work. It does not modify MP-9B or reopen the accepted MP-10C workload.

## CI policy and qualification scheduling

On every PR the cheap Python gate (including real shallow-clone tests) and Windows **100-file** smoke run. A Draft PR missing qualified evidence for its effective inputs reports **NOT PASS / final qualification pending**, but it **does not** queue multi-hour indexing on every corrective commit. After source and cheap CI freeze, move the PR to **Ready for review** to permit exactly the required 100K Windows qualification. An explicit `workflow_dispatch` or release-tag trigger also requests full qualification, but its direct branch checkout/event is **not** automatically equivalent to a PR synthetic merge proof.

The source/dependency fingerprint includes the tracked object IDs, file modes and paths of production Rust code, tests, lockfile/toolchain, security, authority, and the scale workflow itself; non-input documentation/templates and unrelated workflows are deliberately excluded. This avoids GitHub's cumulative PR paths filter. No reuse is possible until matching *successful* full scale provenance exists.

### Split evidence and safe retry

1. Full Windows job runs the unchanged **100,000 files / 1,000,000 FTS chunks** test; validates the exact-tier raw receipt and **immediately uploads** `mp10c-raw-100000` (30-day retention).
2. The lightweight `Source-bound 100K provenance (no indexing)` job downloads the exact validated raw receipt artifact and checks the same checkout fingerprint, PR identity, branch SHA and synthetic merge parent identities. The merge parent is read with `git cat-file -p HEAD`, not a shallow-boundary `git rev-list --parents` traversal. Its successful artifact `mp10c-qualified-100000` contains the raw receipt and provenance.
3. If only provenance fails, **rerun that job only** using GitHub Actions' "Re-run job", not the entire workflow. The raw artifact remains inspectable. On a completed successful run, future gate reuse checks both successful job steps, matching source/attempt/PR/fingerprint, both unexpired artifacts and their GitHub SHA256 ZIP digests, full 100K raw fixture/FTS assertions and byte-for-byte equality of both copies of the receipt.
4. If raw evidence, job status, attestation or provenance is missing, stale, expired, tampered, or mismatched, **fail closed**. A green lightweight gate, skipped heavyweight job, or prior failed workflow is never new 100K PASS. Old incompatible artifact schemas are not accepted.

### Recovery of 2026-10-27 original failed run

[Run #40499159529](https://github.com/KaisAbiyyi/mind-place/actions/runs/40499159529), authored source `3c5d7c3c0d2c8645ed83e4c4d29f2f68dc56dfe2`, synthetic merge `2e40048f79df4cdb558866ff52b7d6afd1f798bf`: the GitHub Windows job log proves the 100K test and exact raw-tier validation finished successfully; the overall workflow failed on the old shallow checkout attestation. Only a **100-file smoke artifact** was uploaded, so there is **no authoritative raw 100K artifact to recover or reuse** under the revised contract. Printed log JSON may be audited as historical performance evidence but cannot substitute for an authenticated raw file, successful attestation and correct source fingerprint. Never invent a receipt or mark the run PASS.

### Current performance evidence

Historical run log: 100K fixture generated in about **68.2 seconds**; initial indexing 10 × 10K roots took **15,569.2 seconds**, and first-root reconciliation took **1,067.6 seconds**. Total exact test **16,959.1 seconds**. Indexing clearly dominates. Per-stage scanner/extraction/FTS write time is **not individually instrumented** yet; do not attribute the entire indexing time to FTS without profiling. Keep 10K-per-root cap, authorization/revocation, raw byte integrity and full 1M FTS chunks unchanged.

### Operator instructions

- On Draft: run cheap gate, Python real-shallow regressions, Windows 100-file smoke and normal security/product CI; full-scale pending is explicitly non-PASS.
- When code and integrated inputs are frozen, mark the PR Ready for Review to allow the required full 100K PR merge-ref qualification. Do this once, not after each source correction. Do not merge until independent review.
- If attestation alone fails after a successful raw upload, inspect the raw artifact, then rerun the provenance job alone. Validate the final **whole workflow** outcome and qualified artifact before claiming acceptance.
- Explicit branch/release `workflow_dispatch` is a fresh test for that branch, **not** proof that a separate PR's merge SHA passed. Requalify when effective source/dependency/test inputs change.
- Cheap regression command: `python3 -m unittest discover -s scripts/tests -p test_mp10c_ci_gate.py -v`. Real depth-one clone coverage is mandatory.

### Existing acceptance held constant

100K synthetic physical documents, 1M FTS chunks, 10K per approved root, raw source preservation, SQLite/FTS integrity, Windows product/SQLCipher/security/revocation behavior and all unrelated acceptance gates remain mandatory. MP-10C prior Done/PASS evidence is historical and not a PASS on arbitrary new source. MP-9B remains independently owned by the `crypto-migration` lock. No automatic merge before independent review.

The full Windows test has its own concurrency group; lightweight gate and smoke jobs are not serialized behind 100K. When a full job acquires a previously occupied slot, it independently rechecks the proof so a now-qualified identical-input PR push does not start another full test. GitHub's PR run `head_sha` records the branch commit separately from the synthetic merge checkout; the attestation inspects the immutable merge commit object to verify its second parent even in a shallow clone.

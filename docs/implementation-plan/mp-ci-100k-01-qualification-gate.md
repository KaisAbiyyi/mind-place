# MP-CI-100K-01 — Source-bound MP-10C qualification gate

Issue: [#128](https://github.com/KaisAbiyyi/mind-place/issues/128) · [Notion task](https://app.notion.com/p/406c4a6aaf7381c89564ef41a5c71336). This is CI-only work. It does not modify MP-9B or reopen the accepted MP-10C workload.

## CI policy

The [MP-10C workflow](../../.github/workflows/mp-10c-scale.yml) runs on every PR, explicit `workflow_dispatch`, and release tags `v*`. Its Windows **100-file** smoke still executes normally. The separate Windows **100,000-file / 1,000,000-chunk** tier runs only if a source-bound, successful 100K receipt for the **same PR** and **identical effective input fingerprint** cannot be verified, or when a manual/final/release trigger explicitly requests it.

This fixes the cumulative-PR-diff behavior of GitHub `pull_request.paths`: an unrelated later push changes commit SHA but does not change the effective input fingerprint. It does **not** skip 100K due merely to the last commit being unrelated if earlier changes have not been qualified.

**Fingerprint input contract (v1):** Git tracked file mode, object ID and path for every tracked source, test, build, dependency, Rust lockfile/toolchain, security/authority implementation, and the MP-10C workflow itself. Exclusions are intentionally *small*: `docs/**`, root README/license/changelog, GitHub issue/discussion/PR templates and workflows other than MP-10C (including Windows Product `ci.yml`). If a currently excluded file becomes an actual scale-test dependency, **update the exclusion contract and requalify**. Unexpected/new files outside the exclusion set force fresh qualification.

**Receipt contract:** Full Windows job must finish its exact `cargo test --release -p mind-place-index --test document_scale_qualification qualifies_document_scale -- --ignored --exact --nocapture` (100,000, no downscale), pass the existing strict receipt check, and then create a provenance JSON bound to PR branch source SHA AND tested PR merge-checkout SHA (recorded separately), workflow run + attempt, source fingerprint, PR number, repo, raw receipt SHA256 and tier 100000. The successful job uploads *both* raw and provenance files in `mp10c-qualified-100000` (30-day retention).

Before reuse, the gate checks GitHub Actions **successful run**, matching full Windows job with three successful steps (test, raw validation, provenance mint), same PR, nonexpired artifact, GitHub artifact archive SHA256, exact ZIP entries, matching run/attempt/repository/checkout SHA, valid 100K/1M raw receipt and identical effective-input fingerprint. Failed, skipped, cancelled, partial, stale, mismatched, inaccessible, or corrupt proof forces a fresh 100K qualification. Previous old-format MP-10C artifacts are intentionally **not reusable**.

**Provenance disclosure:** The gate job summary shows input SHA256, run-versus-skip decision, prior qualified checkout SHA, GitHub Actions run URL, artifact ID and ZIP digest. When 100K is skipped, it says **NOT RUN / not a new 100K PASS**; the new PR head is qualified only for the identical effective inputs by historical evidence. A final release requires an explicit fresh full run; never equate a green gate with an executed 100K test.

## Operator instructions

- **Ordinary PR change:** Run normally; smoke executes and gate attempts reuse. If relevant bytes/dependency/test/workflow change, or proof has expired, full 100K runs again.
- **Independent final qualification:** Actions → MP-10C cloud document scale qualification → Run workflow → select `final` on the desired exact branch. It always runs 100K. A `v*` release tag likewise forces full qualification.
- **Inspect proof:** Open gate job summary, then the linked run and `mp10c-qualified-100000` artifact. Validate source SHA, fingerprint, file/chunk counts, integrity and check statuses; retained receipts expire after 30 days.
- **Regressions:** `python3 -m unittest discover -s scripts/tests -p test_mp10c_ci_gate.py -v`. CI runs these without network and without private data at each PR invocation.

### Existing acceptance held constant

100K synthetic physical documents, 1M FTS chunks, 10K per approved root, raw source preservation, SQLite/FTS integrity, Windows product/SQLCipher/security/revocation behavior and all unrelated acceptance gates remain mandatory. MP-10C prior Done/PASS evidence is historical and not a PASS on arbitrary new source. MP-9B remains independently owned by the `crypto-migration` lock. No automatic merge before independent review.

The full Windows test has its own concurrency group; lightweight gate and smoke jobs are not serialized behind 100K. When a full job acquires a previously occupied slot, it independently rechecks the proof so a now-qualified identical-input PR push does not start another full test. GitHub's pull-request run `head_sha` identifies the branch source commit, while the checked-out `refs/pull/N/merge` is a synthetic commit; the attestation records both and verifies the merge's second parent is the PR head before minting proof.

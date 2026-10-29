# Mind Place — Independent Windows Security Authority (staging payload)
**NONACTIVATED. NOT INDEPENDENT UNTIL COPIED INTO A SEPARATELY CONTROLLED REPOSITORY. DO NOT MERGE THIS STAGING DIRECTORY INTO THE PRODUCT.**

This directory is a deployable *repository payload*, stored temporarily under `authority-staging/` in `mind-place` solely for review. All paths inside this directory become repository-root paths when the owner creates a separate private policy repository. The target workflow and source are exact frozen copies. The policy does not execute any target checkout: it uses immutable Git objects and GitHub-owned metadata to verify native jobs/steps and artifact digests. PR-produced check names, PR artifacts, and advisory results are never trusted authority.

## Frozen source / proof
- Target: `KaisAbiyyi/mind-place`, PR #132, HEAD `3ab2516ae468e2dfe03bac3b26395db44df2db03`
- Synthetic merge commit `e0610f8522881c93b2dd644d0490ef298534febb`; effective Git tree `4aada364dfa9575d0b74736ab5773e9690c1ecf3`; base `7733a91fa7bbfc23504bf4b2f8af09baaa3f8a3f`
- Effective native-input fingerprint: `08c80f2ee6e6bbdfe7bcf47c2d8e5eb35e37a3c839bf215ebd07d9c455a0c11f` (`windows-product/v1/inputs-v2`, all regular/symlink/gitlink paths except the two exact audited handoff/fast-orchestration paths)
- Hosted functional evidence: run 40812460976 attempt 1, hosted Windows Product and SQLCipher Windows/Ubuntu successful. **Not an App check, and not live acceptance.**

## Files
- `.github/workflows/verify-windows.yml`: independent controller workflow, copied from frozen target verifier template; currently nested and therefore not scheduled here.
- `.github/workflows/windows-product.yml`, `.github/windows-policy/native-steps.json`: pinned native execution contract.
- `scripts/windows_product_gate.py`, `scripts/windows_product_inputs.py`: pinned verifier/fingerprint implementation. Tested by copied suites.
- `.github/windows-policy/config.json`: **disabled**, app_id 0, no approved fingerprints or credentials.
- `policy/proposed-fingerprint.json`: SHA/tree/fingerprint *proposal*, not signed approval.
- `tests`: source regressions and additional authority staging boundaries under `scripts/tests`.

## Run focused checks
```bash
python3 -m pip install PyYAML==6.0.3
python3 -m unittest discover -s scripts/tests -p 'test_windows_product*.py' -v
python3 -m unittest discover -s scripts/tests -p 'test_authority_staging_boundaries.py' -v
actionlint .github/workflows/verify-windows.yml .github/workflows/windows-product.yml
```
On GitHub, the root `.github/workflows/mp-2a-security-staging.yml` is a **credential-free, staging-branch-only** focused CI runner, not the independent authority.

## Status
Cloud preparation only. No private repository, installed App, protected environment, branch protection, publication of trusted checks, main merge or local-machine work. See SECURITY.md, ACTIVATION.md, and REVIEW.md. Verify that the staged tree matches these files after push. The staging branch must not be used as an independent policy trust anchor.

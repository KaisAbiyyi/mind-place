# Checkpoint 2A — corrected candidate for INDEPENDENT REVIEW

**STAGING-ONLY / DO NOT MERGE INTO PRODUCT. P0 OPEN / ACTIVATION BLOCKED.** This is not a production App acceptance or a deployment authorization.

## Frozen evidence preserved

- Product PR: https://github.com/KaisAbiyyi/mind-place/pull/132; HEAD `3ab2516ae468e2dfe03bac3b26395db44df2db03`, stacked base `codex/mp-ci-100k-01-20261027`.
- Effective merge tree `4aada364dfa9575d0b74736ab5773e9690c1ecf3`, fingerprint `08c80f2ee6e6bbdfe7bcf47c2d8e5eb35e37a3c839bf215ebd07d9c455a0c11f`.
- Native previously: https://github.com/KaisAbiyyi/mind-place/actions/runs/40812460976 / attempt 1; not rerun and not independent authority.
- Source/verifier/inputs/native-steps blob IDs pinned by `policy/contract-inventory.json`, preserved byte-for-byte in `policy/contracts`.
- Live `scripts/windows_product_gate.py` and `scripts/windows_product_inputs.py` are unchanged original bytes. The controller TEMPLATE only adds `--policy-root policy/contracts`.

## P1 workflow isolation candidate

Four product/controller workflow copies formerly under staging `.github/workflows/` have been removed. Exact frozen product snapshot files are now in inert `policy/contracts/.github/workflows/`; the unpromoted controller template is `.github/windows-policy/verifier.yml`. The deploy validator enforces no active root workflow, six approved Git blob IDs, disallows missing/extra/ambiguous/traversing/symlink contracts and checks the controller root mapping. Independent deployment into a private policy repository remains unperformed.

## P0 lifecycle: documented BLOCKER, not resolved

Previously successful checks continue to exist even after lease expiration, fingerprint revocation, App credential revocation or controller outage. Source verification is fail-closed for a *new* evaluation, but no immediate historic check revocation is guaranteed without an independently enforced merge freeze or a separately proven fresh merge-admission design. A merge queue with new queue SHA checks is a candidate requiring independent implementation and live tests, NOT an activated feature. Do not close P0 on green synthetic tests.

## Independent reviewer checklist

1. Re-fetch source blobs and confirm `policy/contract-inventory.json` vs frozen PR HEAD and effective merge; verify target bytes and native execution step manifest/IPC.
2. Check path isolation and exact controller `--policy-root policy/contracts`; reject direct-copy activation of any target workflow. Check source verifier has not been modified.
3. Review adversarial `test_deployment_layout.py`, existing 58+ Windows Product tests, added stale-success models and 2A staging boundaries; inspect both negative proof tests for zero IPC matches.
4. Inspect updated Ubuntu-only GitHub staging CI, actionlint, JSON, source blob inventory and independent reviewer report; do not repeat native, SQLCipher or scale qualification.
5. Independently validate platform behavior: old SUCCESS on same SHA, new SHA, new merge source, revoked fingerprint, policy revision, wrong App, outage/restart/API failure. Record actual enforcement gaps.
6. Prepare owner/admin 2B gate only after independent reviewed GitHub merge-freeze or equivalent fail-closed mechanism; no App install, branch protection, policy repo activation or merge within this change.

**Verdict requested:** P1 fixed in reviewed package, or explain specific failed invariant. **P0 OPEN / NO-GO for production until real GitHub enforcement is independently demonstrated.** All staging work remains nonactivated.

## Final P0 admission candidate — independent security review required

- **Recommended:** Option B, permanent default-closed branch lock on all real destination bases with manual, separately audited per-merge exceptions. No unattended green-check merge.
- **Rejected:** Option A merge queue for this checkpoint (already-successful queue entries retain outage race); Option C (lease/check status does not expire completed SUCCESS).
- **Code:** new read-only `scripts/merge_freeze.py`, disabled `policy/merge-freeze.example.json`, synthetic negative tests `scripts/tests/test_merge_freeze.py`. The original frozen verifier, native source, workflow template and contract blob inventory remain byte-identical.
- **Reviewer test:** independently inspect actual branch protection `lock_branch`, `enforce_admins`, strict required context sourced from exact App, inherited/active rulesets and actor bypass. Verify that ordinary-writer REST and UI merge attempts fail with otherwise-green PRs on both stacked base and main. Read back actual protection after testing. Mock verification is explicitly **NOT_YET_LIVE_VALIDATED**.
- **Residual:** existing SUCCESS is not revoked, outage in an open/manual exception window and privileged administrator changes can still admit stale success; no immediate revocation guarantee. GitHub admin/API permission was unavailable to the connected reviewer; live verification is outstanding.
- **Verdict:** implementation candidate READY FOR INDEPENDENT REVIEW, **P0 OPEN / Checkpoint 2B NO-GO / production NO-GO** until actual GitHub branch enforcement and bypass tests pass. Do not merge staging payload into product.

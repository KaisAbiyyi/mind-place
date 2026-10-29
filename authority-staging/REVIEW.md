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

## Prior P0 candidate (superseded by final corrections below)

- **Superseded recommendation:** manual per-merge exceptions are now prohibited; see the final correction decision below.
- **Rejected:** Option A merge queue for this checkpoint (already-successful queue entries retain outage race); Option C (lease/check status does not expire completed SUCCESS).
- **Code:** new read-only `scripts/merge_freeze.py`, disabled `policy/merge-freeze.example.json`, synthetic negative tests `scripts/tests/test_merge_freeze.py`. The original frozen verifier, native source, workflow template and contract blob inventory remain byte-identical.
- **Reviewer test:** independently inspect actual branch protection `lock_branch`, `enforce_admins`, strict required context sourced from exact App, inherited/active rulesets and actor bypass. Verify that ordinary-writer REST and UI merge attempts fail with otherwise-green PRs on both stacked base and main. Read back actual protection after testing. Mock verification is explicitly **NOT_YET_LIVE_VALIDATED**.
- **Residual:** existing SUCCESS is not revoked, outage in an open/manual exception window and privileged administrator changes can still admit stale success; no immediate revocation guarantee. GitHub admin/API permission was unavailable to the connected reviewer; live verification is outstanding.
- **Verdict:** implementation candidate READY FOR INDEPENDENT REVIEW, **P0 OPEN / Checkpoint 2B NO-GO / production NO-GO** until actual GitHub branch enforcement and bypass tests pass. Do not merge staging payload into product.

**Additional reviewer regression:** ensure synthetic negative tests reject enabled/missing fork sync, force-push and deletion allowances under an otherwise locked branch. Verify the same settings live alongside administrator bypass rules.

## Final independent review corrections — new review requested

**READY FOR INDEPENDENT REVIEW — CLOUD PREPARATION COMPLETE** is conditional on exact candidate Ubuntu CI below. **STAGING ONLY — DO NOT MERGE INTO PRODUCT.**

- Baseline reviewed HEAD: `8aa1924adc76b308b9bfcce3645193bd76d7aff0`; reviewer [two outstanding findings](https://github.com/KaisAbiyyi/mind-place/pull/132#issuecomment-6493930958).
- **P2 pagination: FIXED candidate.** Strict bounded complete inventory preserves `includes_parents=true`; every active detail requires independently visible empty bypass actors. The actual repository and inherited Organization source are checked, with GitHub effective-branch rules independently paginated and crosschecked. Unknown enforcement/source/target, incomplete details, duplicates, truncation, sensitive-field omission, HTTP/API/rate-limit errors reject. Frozen transport cannot validate page types/Link metadata, so the inspector uses a GET-only transport and strict local paginator with the same 20x100 bound. No frozen contract bytes changed.
- **P1 manual admission: MITIGATED.** The former unlock procedure is removed and temporary unlocking explicitly prohibited. Successful inspector output cannot authorize merges. Option B is viable **only as permanent-lock/no-admission containment**; useful merge admission requires a stronger independently reviewed alternative. No replacement controller is built.
- **Platform versus operation:** GitHub documents branch locks/admin enforcement/App identity checks, pending live validation. Owner/admin exclusivity, fresh identity/competitor reads and approvals are operational prerequisites, not atomic enforcement. The merge API's source-HEAD guard does not bind expected base SHA, approved tree and a one-use actor/window. Read sequences plus unlock cannot guarantee singleton admission.
- **P0 stale-success enforcement: OPEN. Checkpoint 2B production activation: NO-GO.** No real enforcement probe or administrative change is authorized here. A completed SUCCESS is not retracted by controller outage or internal lease expiration.
- **Owner handoff:** `ACTIVATION.md` supplies complete paginated PR/auto-merge/queue queries, exact source/current-base/tree/App/policy/approval identity checks, immediate pre-probe recheck, post-operation settings/ref readback, exclusive owner/admin constraints, direct/alternative path review, incident handling and the full live negative matrix. No exceptional unlock is permitted even after these reads.
- **Independent review requested:** re-evaluate exact new staging HEAD, changed-file list and Ubuntu run; focus on cross-page and inherited applicability/visibility failures, fail-closed responses, prohibition of all manual admission, unchanged frozen contract/source-equivalent/native requirements and truthful live gates. Do not treat author self-review or synthetic tests as independent security approval.

Exact final HEAD is the commit containing this section (recorded by the durable GitHub handoff, avoiding a self-referential commit hash). Final CI, test counts and changed paths are recorded with that exact SHA in the review handoff.

Cloud validation before push: **35 focused tests PASS**; staging security regressions **120 run / 119 PASS / 1 PowerShell-unavailable SKIP** in this shell. The existing workflow runs the same suite from `authority-staging` on Ubuntu with PowerShell available. Actionlint v1.7.9, all 10 staging JSON/YAML files and frozen six-blob contract/layout validation PASS. An incidental run from the product root exposed the existing cwd-dependent `product_workflow()` expectation; the documented staging-directory command passes, and no unrelated routing code was changed. Final Ubuntu results supersede the local skip.

Fresh read-only candidate review identified two response-completeness gaps: inconsistent non-next pagination links and a valid JSON prefix received before declared HTTP body completion. Both reproductions were observed failing before the fix, then passing after it. All pagination relations are now validated; declared Content-Length must match the bounded body. The HTTP regression uses a real `http.client.HTTPResponse`, including both paginated endpoints. Final external independent security review is still requested; this internal candidate review is not production acceptance.

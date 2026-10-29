# Checkpoint 2A independent-review handoff
**Decision: PREPARATION CANDIDATE / PRODUCTION ACTIVATION BLOCKED.** All files are nonactivated and staged in the target repo. A separately administered policy repository and App are NOT provisioned.

## Frozen target evidence
- PR #132 draft: https://github.com/KaisAbiyyi/mind-place/pull/132
- HEAD: `3ab2516ae468e2dfe03bac3b26395db44df2db03`
- Native source synthetic merge: `e0610f8522881c93b2dd644d0490ef298534febb`; effective tree: `4aada364dfa9575d0b74736ab5773e9690c1ecf3`
- Native input fingerprint: `08c80f2ee6e6bbdfe7bcf47c2d8e5eb35e37a3c839bf215ebd07d9c455a0c11f` computed from immutable GitHub recursive tree entry mode/blob SHA/path with pinned `windows_product_inputs.py` algorithm; independently recalculate before approval.
- Prior native run: https://github.com/KaisAbiyyi/mind-place/actions/runs/40812460976 attempt 1 Windows Product and SQLCipher hosted success. This run was not App acceptance and was not rerun here.

## Static sources
- Native workflow Git blob: `b4d0d2dd4c67eab154773aaaab15616bbced3c64`
- Verifier Git blob: `4688735e7b1ee616d7e65c66ac66b19289054d5f`
- Inputs Git blob: `4117fd03521803057e91a6bf17f7cb422d7a9f6f`
- Native required steps: `1086337f15989e828eae0fc69e43447ab10a491e`
- Controller workflow template: `5e6dd975c8958e1fc11c47851a7a3591cec91c44`.
All are *Git blob IDs* of frozen source objects, not SHA-256; do not interchange these with proof archive SHA-256. Store signed/independently reviewed inventory at deployment.

## Reviewer checklist
1. Compare staged copies to original blob IDs and exact source at PR HEAD + merge; verify all required native jobs/steps and IPC zero-test guard.
2. Inspect `scripts/windows_product_gate.py` for fail-closed semantics, source/race checks, native contract parity, artifact digest and App identity.
3. Run `scripts/tests/test_windows_product*.py` and `test_authority_staging_boundaries.py`; confirm negative scenarios around stale SHA/merge, wrong App, wrong attempt, corrupted ZIP, skipped job, zero tests, approved fingerprint, protection unavailable.
4. Inspect policy workflow with actionlint, protected environment/checkout behavior, pinned action versions, and App permissions.
5. Review controller outage / stale SUCCESS persistence and independent policy-revocation handling before considering production activation.
6. Confirm actual independent policy repo ownership/access and App ID/branch protection against BOTH stacked base and main at release phase.

## Unperformed administrative steps
Separate private repo, protected env, approved fingerprint, owner-held App key, App installation, required checks, main changes, production activation, native reruns, owner-local tests. The dedicated policy repository cannot be created through the available GitHub connector in this run, so only the staging payload has been prepared.

## Known risk
**P0 for production:** Existing completed green GitHub check runs do not expire when the controller stops. The gate's worker lease-expiry checks do not change GitHub branch-protection interpretation. Requires explicit lifecycle mitigation before enforcing as a production security gate. Future independent reviewer may identify further issues.

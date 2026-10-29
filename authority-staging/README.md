# Mind Place — Independent Windows Authority (Checkpoint 2A, staging only)

**STAGING-ONLY / DO NOT MERGE INTO PRODUCT. No controller, App or policy repository has been activated.**
The directory is a *future private policy repository root payload* currently held within the product repository for independent review; it is not itself an independent trust anchor.

## Safe copy layout

- `policy/contracts/.github/workflows/ci.yml`, `windows-product.yml`, and `mp9b-sqlcipher-qualification.yml` are **inert immutable target workflow snapshots**, never live workflows.
- `policy/contracts/scripts/windows_product_gate.py` and `windows_product_inputs.py` are immutable **original target contract data**.
- `policy/contracts/.github/windows-policy/native-steps.json` is the pinned native-step contract.
- `scripts/windows_product_gate.py` and `scripts/windows_product_inputs.py` remain the exact original reviewed verifier/input implementation bytes, unmodified.
- `scripts/staging_layout.py` is a staging-only fail-closed deployment validator; run before copying/activating anything.
- `.github/windows-policy/verifier.yml` is the **inactive independent controller template**, not an executable Actions workflow.
- `.github/windows-policy/config.json` is deliberately disabled (app_id 0; no approvals).
- `policy/contract-inventory.json` pins the immutable Git blob IDs and path inventory.
- `.github/workflows/` MUST NOT exist in the staged independent policy repository root. Only Checkpoint 2B may promote a separately reviewed controller template into this location.

The controller template uses `--policy-root policy/contracts`, so `validate_contract` compares target-source paths (e.g. `.github/workflows/windows-product.yml`) against `policy/contracts/<identical target path>` without changing target source or frozen verifier bytes. Do not point it back to `.`.

## Frozen candidate

Product Draft PR #132: HEAD `3ab2516ae468e2dfe03bac3b26395db44df2db03`; effective tree `4aada364dfa9575d0b74736ab5773e9690c1ecf3`; input fingerprint `08c80f2ee6e6bbdfe7bcf47c2d8e5eb35e37a3c839bf215ebd07d9c455a0c11f`. These are **review proposals**, NOT approvals. Prior hosted Windows/SQLCipher native run 40812460976 attempt 1 must not be rerun as part of this correction.

## Verification (staging only)

```bash
python3 -m pip install PyYAML==6.0.3
python3 scripts/staging_layout.py
python3 -m unittest discover -s scripts/tests -p 'test_*.py' -v
python3 -m json.tool policy/contract-inventory.json >/dev/null
python3 -m json.tool policy/contracts/.github/windows-policy/native-steps.json >/dev/null
actionlint .github/windows-policy/verifier.yml policy/contracts/.github/workflows/*.yml
```

The single Ubuntu staging workflow in the **product** root, `.github/workflows/mp-2a-security-staging.yml`, runs focused policy tests. It is not copied to the private policy repository and never has authority secrets.

**P0 OPEN / ACTIVATION BLOCKED**: a previously successful check is not automatically revoked on lease expiry, controller outage or fingerprint revocation. See [SECURITY.md](SECURITY.md) and [ACTIVATION.md](ACTIVATION.md). Test success alone does not close this.

## Selected P0 containment candidate

Option B: **default-closed independent branch lock with manual per-merge exception only**. The read-only inspector is `scripts/merge_freeze.py` and its deliberately disabled config example is `policy/merge-freeze.example.json`. The staging inspector **never** publishes a check, changes GitHub settings or unfreezes a branch, and cannot certify actual enforcement. See SECURITY.md/ACTIVATION.md for the required owner-admin live negative test. P0 remains OPEN; Checkpoint 2B NO-GO until independent live validation.

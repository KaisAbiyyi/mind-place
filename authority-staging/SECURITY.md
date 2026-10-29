# Security specification / threat model
**Authority and source must be administered separately.** A branch, Draft PR, or workflow inside `mind-place` is not an independent trust boundary.

```
PR author / untrusted code --> immutable target commit, merge tree, GitHub workflow execution
                                     |
                                     v (read-only public Git blobs/API; NEVER imports target Python)
Private independent policy repo --> pinned policy commit + reviewed fingerprint list
    protected default branch      --> protected environment (App key, ID)
    dedicated GitHub App          --> GitHub-owned actual run, attempt, step, ZIP digest checks
                                     |
                                     v
                           Checks API: Windows Product / trusted acceptance
                                     |
                                     v
                 Branch protection: exact context + independent App ID
```

## GitHub App permission minimum
Repository installation selected ONLY for `KaisAbiyyi/mind-place`. Contents **Read**, Actions **Read**, Pull requests **Read**, Administration **Read** (inspect branch protection), Checks **Read & Write**. Metadata Read is implicit. The independently controlled workflow mints a short-lived installation token via `actions/create-github-app-token`. No code-write, secrets-read, workflows-write, org permissions, or target repository secret containing private key. App name suggestion: `Mind Place Windows Authority`. Confirm exact permissions via GitHub App UI at creation. Check-run publisher must equal the actual numeric GitHub App ID in the target's required check configuration, not just the check name. An App token used to write acceptance must be short lived. Store the App key only as a protected environment secret in the private policy repo. Rotate by creating a new key, updating protected secret, validating the App identity and deleting old key; revoke installation if compromise is suspected. Never log or paste key.

## Exact native contract
Four independently pinned files are compared as bytes to both source and PR HEAD: target Windows Product workflow, target verifier, input-closure implementation, required-step manifest. The current frozen source requires *both* real encrypted IPC tests:
- `native_capture_ipc::qualification::separate_process_capture_probe`
- `native_capture_ipc::qualification::owner_local_capture_roundtrip_requires_unlocked_epoch`

Cargo's zero-test filter is insufficient. The native job and every applicable required step must actually be successful, on a Windows runner and with exact GitHub-owned run attempt. The artifact's GitHub digest is checked against full ZIP bytes *before* unzipping the bounded single JSON member. A PR-owned artifact or check cannot mint acceptance.

## Fail-closed behaviors
Missing source identity, synthetic merge mismatch, wrong workflow bytes, stale run, skipped native job or step, wrong repo/PR/attempt, unapproved fingerprint, missing/wrong protection or API error -> no fresh acceptance. Source-equivalent reuse depends on the *same effective immutable input closure* and still binds the current PR HEAD/base/current merge. `enabled=false` prevents issuance. Tests are synthetic; they do not substitute for real administrator activation tests.

## P0/P1 residual risk requiring independent security review BEFORE activation
GitHub required checks are persistent completed check runs. A previously successful check is not automatically expired when its embedded lease expires or when the controller becomes unavailable. The verifier rejects expired leases for *future worker reuse*, and an online controller can overwrite previous success during revocation, but **GitHub branch protection does not honor the lease timestamp on an existing SUCCESS**. Consequently “fail closed on controller outage/revocation” must NOT be claimed as proven. Owner must establish a reviewed lifecycle mitigation and test stale-success revocation/availability behavior before enabling live enforcement, or accept this as a documented security blocker. A best-effort five-minute sweep is insufficient for strict immediate revocation.

The target's current PR #132 targets `codex/mp-ci-100k-01-20261027`, not `main`. Main-only protection cannot enforce its current stacked PR. Preserve existing checks and apply any future identity-bound requirement only on an intended protected target branch after independent App configuration is ready.

Never activate a required check before the App is confirmed able to publish one, or merges could deadlock.

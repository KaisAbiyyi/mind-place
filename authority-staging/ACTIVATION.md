# OWNER/ADMIN ACTIVATION RUNBOOK — CHECKPOINT 2A STAGING ONLY

**P0 OPEN / ACTIVATION BLOCKED. DO NOT MERGE the staging payload into the product.** All steps marked OWNER require separate explicit authorization and are **not executed** here. Checkpoint 2B must not begin until independent review of these corrections and an enforceable stale-success mitigation.

## 1. Immutable staged deployment payload

1. **REVIEWER — Confirm** branch `codex/mp-p0-authority-2a` at reviewed SHA, all Git blobs and the current PR #132 HEAD `3ab2516ae468e2dfe03bac3b26395db44df2db03`. If the product HEAD/base/merge tree changes, independently recompute approval; do not reuse proposal.
2. **REVIEWER — Validate** `python3 scripts/staging_layout.py`, focused negative tests, and `policy/contract-inventory.json`. Target product workflows exist as `policy/contracts/.github/workflows/{ci,windows-product,mp9b-sqlcipher-qualification}.yml`, never as executable repo-root workflows.
3. **OWNER — Copy only approved contents of `authority-staging/`** into the root of a new separately owned **private policy repo** after independent review. Include `scripts/`, `policy/contracts/`, `policy/contract-inventory.json`, `.github/windows-policy/`, and `docs` when supplied; preserve file bytes. Do **not** copy product-root `.github/workflows/mp-2a-security-staging.yml`. Verify `.github/workflows` is absent from the new policy repo root. Independently run the validator and a full file inventory after copying. **Do not enable workflows during this staging checkpoint.**
4. **REVIEWER — Pin policy commit externally**, including the entire deployment tree, active `scripts/windows_product_gate.py` and `scripts/windows_product_inputs.py`, the manifest and `.github/windows-policy/verifier.yml`. Those active scripts remain byte-identical to the frozen original source at this checkpoint. The target contract *mapping* is exactly `target .github/workflows/windows-product.yml` -> `policy/contracts/.github/workflows/windows-product.yml`; `target scripts/windows_product_gate.py` -> `policy/contracts/scripts/windows_product_gate.py`; `target scripts/windows_product_inputs.py` -> `policy/contracts/scripts/windows_product_inputs.py`; `target .github/windows-policy/native-steps.json` -> `policy/contracts/.github/windows-policy/native-steps.json`. The verifier keeps the original **target** names and is supplied `--policy-root policy/contracts`.

## 2. Independent authority (future, owner only)

5. **OWNER — Protect private policy repo** with mandatory separate reviewers/CODEOWNERS for policy, contract, controller and config changes, protected default branch and environment, no untrusted writers or bypass actors, and required checks. Unreviewed PR Actions must not access App secrets.
6. **OWNER — Create dedicated GitHub App** installed on `KaisAbiyyi/mind-place` only. Require Contents Read, Actions Read, Pull Requests Read, Administration Read, Checks Read/Write; no code write or target-repo App private key. Record its **real numeric App ID**. Keep production private key in the independently protected policy environment `windows-policy-authority`; never print or check in keys. Rotation/revocation must follow incident procedure below.
7. **REVIEWER — Independently approve native fingerprint** after verifying exact PR HEAD + effective merge tree, workflow/verifier/input/manifest Git blobs, actual native jobs/steps and both IPC cases. The staged proposal fingerprint `08c80f2ee6e6bbdfe7bcf47c2d8e5eb35e37a3c839bf215ebd07d9c455a0c11f` is **NOT an approval**. Only after approval in a reviewed policy commit may the owner change `.github/windows-policy/config.json` to enabled=true, app_id=REAL_APP_ID, approved_fingerprints=[reviewed value]; pin its policy SHA in `vars.WINDOWS_POLICY_COMMIT`.
8. **OWNER — Establish/test independently enforced merge-freeze** on all protected target bases before accepting any persistent SUCCESS. A ruleset/branch restriction must demonstrably block even otherwise-green merges for non-admins, with no ordinary-writer bypass. Do not rely on disabled config, a stale check's output, or a periodic monitor. Record exact rollback/recovery steps and separately verify branch/ruleset settings and an otherwise-green PR **cannot merge** during a simulated outage. This is **P0 OPEN until demonstrated**.
9. **OWNER — Configure strict required checks** bound to the exact App ID on the actual PR base, including stacked base `codex/mp-ci-100k-01-20261027` for #132 and later `main` when retargeted; include administrators and **preserve every existing required context**. Confirm the policy App has already proven its ability to publish the named check in a controlled isolated test before requiring it. Do not delete required contexts to resolve deadlocks. Recheck GitHub branch/ruleset semantics; branch protection API response differs from rulesets.
10. **OWNER — Promote ONLY controller template** `.github/windows-policy/verifier.yml` to repo-root `.github/workflows/verify-windows.yml` in a separately reviewed policy commit during the expressly authorized **Checkpoint 2B**. The controller command must include `--policy-root policy/contracts`, from the protected pinned policy checkout. This operation intentionally makes 2A's staging-only layout validator reject the active repository; switch to a separately reviewed 2B activation validator thereafter. Never promote product workflow snapshots.
11. **OWNER — Controlled first official proof**: verify the protected App identity and repo restrictions, real strict branch protection, source/merge identities and native fingerprint. Dispatch a fresh independent authority evaluation; require actual GitHub App check on current source, verified run attempt/artifact SHA-256, Windows/SQLCipher applicability and both native IPC names. Prior run 40812460976 is hosted native evidence only, not App-acceptance proof. A new expensive native build is requested only when source/closure policy demands it, not to repair scheduling.
12. **OWNER — Verified reuse demonstration**: after approved excluded handoff-only changes, prove the same immutable fingerprint, same valid native proof, new current HEAD/base/merge validation and new independent App acceptance, without executing another Windows build. Verify wrong App, wrong source, missing IPC, changed source and previous check all fail closed.

## 3. Outage, stale-success, merge-freeze and rollback (future, owner only)

13. **OWNER — Before activation, reproduce P0 adversarial live tests**: a completed SUCCESS with expired internal lease; revoked fingerprint; controller outage and restart; stolen/revoked App credentials; wrong App identity; stale/superseded merge; policy change; unavailable GitHub API. Verify that merging is actually prohibited by GitHub while incident freeze is active. A controller error, failed API PATCH or absent App key **does not** remove existing SUCCESS. If this cannot be shown, leave **P0 OPEN / NO-GO**.
14. **OWNER — On real incident**: execute verified independent **merge-freeze** on each affected protected base; halt new policy publications; enumerate all current PR HEADs and exact App check IDs; with authorized App PATCH affected completed checks to failure and **read them back**; rotate/revoke App key/installation if compromised; revoke fingerprints in separately reviewed policy. If PATCH/API/permissions fail, treat all existing SUCCESS as still valid and keep external merge freeze/incident stop active. Do **not** remove the required App context or relax protections to unblock builds. If GitHub or admin enforcement is unavailable, immediate fail-closed behavior cannot be guaranteed and merges/releases must stop operationally.
15. **OWNER — Recovery**: independently review new policy commit, verify protected config, revalidate all affected source/merge/check identities and actual live freeze settings, publish fresh App checks as needed, and retain permanent locks. Any future admission requires a stronger separately reviewed design; do not authorize unfreezing under this runbook. Preserve incident logs privately; never assume a restarted controller automatically invalidated historical successes. Retarget stacked PR protection and its App-origin requirement when transitioning to main.

**No privileged action was performed in Checkpoint 2A.** Independent reviewer must issue a documented GO/NO-GO before Checkpoint 2B. Current result is **P1 staging fix candidate, P0 OPEN, activation NO-GO**.

## Permanent-lock containment handoff — no exceptions (NOT EXECUTED)

**Temporary unlocking is PROHIBITED. No merge authorization exists under this candidate, including after independent approval of these corrections.** Option B is viable only for permanent-lock containment. A stronger admission design requires a new independent review, live enforcement evidence and separate owner authorization. Operational serialization alone never establishes an atomic single-PR guarantee.

The following is an exact **future owner/admin test handoff**, not authorization to perform administrative actions now. Use a controlled disposable equivalent first; tests against actual `main` and stacked base `codex/mp-ci-100k-01-20261027` require separate owner approval. Never merge #132/#127/#129/#131 as probes.

1. **Exclusive owner/admin + separate reviewer:** name one settings operator, one independent approver, precise repository/destinations, pinned policy/staging SHA, actor/App IDs, time window, test resources and incident contact. Inventory all admins, custom bypass roles, teams, Apps, deploy keys, integrations, merge bots and automation that can update refs/settings. Independently constrain competing actors/credentials and review direct push, force push, fork sync, REST/UI/GraphQL merges and queue paths. A promise to “halt other merges” is operational only. Concurrent or unknown administrators => stop; no exception.
2. **Read current identities:** independently record repository ID/full name, exact destination ref SHAs, selected PR number/source repository/source SHA, current destination name and SHA, approved effective merge tree, policy revision, fingerprint and source-bound native proof/IPC, current App check ID/App ID/conclusion and current independent approval. Do not use a PR's potentially stale `baseRefOid` as the destination's current SHA; read the branch ref. Any source/base/tree/policy/check/approval change invalidates the record. No approval or revoked/old App check => stop. These are evidence prerequisites, not permission to merge.
3. **Enumerate competitors completely** using the two read-only queries below for **each destination**, saving every page and rejecting partial data, GraphQL errors, null repository/ref, repeated cursor/entry or unavailable fields. Review every open PR as potentially green, not just those awaiting checks. Explicitly enumerate `autoMergeRequest`, `mergeQueueEntry` and queue entries, including “Merge when ready.” An empty or independently constrained state is required before any future proposed admission; cancellation/dequeue or credential restrictions require separate owner authorization and fresh readback. Empty snapshots alone do not exclude new requests during an open window.
4. **Independently inspect all protections:** fully paginate repository rulesets with `includes_parents=true`, GET every active detail including inherited rules, inspect sensitive bypass actors with an authorized owner/admin identity, and GET fully paginated `/rules/branches/{encoded-branch}` for effective applicability. Unknown/inaccessible/inconsistent state => NOT VERIFIED. Inspect strict required contexts bound to the real numeric App ID while preserving other checks. Verify lock, administrator enforcement and explicitly disabled fork sync, force pushes and deletions on every destination. The disabled sample is not evidence of protection.
5. **Only after separate administrative authorization:** establish permanent locks and keep them locked throughout negative probes. Run the read-only inspector from the independently pinned policy checkout using a separately reviewed config with real App ID and complete destination set. Recheck source SHA, **current branch ref SHA**, effective tree, approval, competitors, check state and settings immediately before the owner authorizes each **denial probe**. No unlock or real merge is authorized by this procedure.
6. **Challenge enforcement:** execute the matrix below with disposable test resources and independently captured actor/API/UI responses. After **every** attempted operation, read back the exact destination ref SHA and every effective protection/ruleset; require unchanged branch SHA and lock still enforced. Evidence must identify repository, destination, PR/source/base/tree, actor/App and timestamps. If a disposable equivalent is used, real-destination equivalence and remaining real-base tests must be independently reviewed, not assumed.
7. **Incident on any unexpected update/unlock, unavailable readback or relock failure:** halt all merge/release and policy publication activity immediately; notify the named owner/admin and independent reviewer through an authorized incident channel. Treat historical SUCCESS as valid and the branch as potentially open. Owner/admin must restore protection using an available authorized channel, then read back lock/admin/bypass state and destination SHA on every base. If GitHub/API/admin access is unavailable, report containment **NOT VERIFIED**, retain the incident stop and do not retry a merge. Controller outage after unlock has no automatic fail-closed guarantee. Recovery requires new independent approval; no automatic or manual unfreeze under this candidate.
8. **Reviewer decision:** attach actual denial/readback evidence and declare only the narrowly demonstrated containment guarantee. Keep P0 OPEN until independent real enforcement acceptance; Checkpoint 2B and production remain NO-GO under this cloud-only delivery. No live test alone retroactively makes the former manual-unlock design atomic.

### Read-only competing-request inventory

Run for `main`, then the current stacked base (and any other actual destination). `--paginate` follows the explicit connection's `pageInfo`; the owner must check that the final page has `hasNextPage=false`, all cursors progress, and no response contains `errors`. API failure/rate limit/authorization failure is NOT VERIFIED. The `ref` query independently reads the current base identity. Run again immediately before and after each authorized denial probe.

```bash
REVIEW_BASE=main
gh api graphql --paginate -f branch="$REVIEW_BASE" -f ref="refs/heads/$REVIEW_BASE" -f query='
query($branch:String!, $ref:String!, $endCursor:String) {
  repository(owner:"KaisAbiyyi", name:"mind-place") {
    id nameWithOwner autoMergeAllowed ref(qualifiedName:$ref) { target { oid } }
    pullRequests(states:OPEN, baseRefName:$branch, first:100, after:$endCursor) {
      nodes {
        number headRefOid baseRefName headRepository { nameWithOwner }
        autoMergeRequest { enabledAt enabledBy { login } }
        mergeQueueEntry { id }
      }
      pageInfo { hasNextPage endCursor }
    }
  }
}'
gh api graphql --paginate -f branch="$REVIEW_BASE" -f query='
query($branch:String!, $endCursor:String) {
  repository(owner:"KaisAbiyyi", name:"mind-place") {
    id nameWithOwner
    mergeQueue(branch:$branch) {
      entries(first:100, after:$endCursor) {
        nodes { id pullRequest { number headRefOid baseRefName } }
        pageInfo { hasNextPage endCursor }
      }
    }
  }
}'
```

A null queue must be reconciled with independently inspected branch settings/plan availability; it is not permission to skip “Merge when ready” requests or the per-PR queue fields. Repository auto-merge disabled does not prove queues disabled. Queries enumerate current state; they neither reserve the branch nor bind an admission transaction.

### Negative enforcement matrix

All actual merge/push/settings behavior below is **LIVE VERIFICATION REQUIRED**, not simulated platform proof. Local fixtures can only reject unsafe/malformed snapshots; existing verifier tests cover new-evaluation source/base/tree/App/proof rejection without proving GitHub retracts completed checks.

| Scenario | Required denial / owner evidence |
| --- | --- |
| Another otherwise-green PR pending | Enumerate all open PRs; it cannot merge under permanent lock; readback unchanged |
| Unrelated auto-merge request, including later-green PR | Enumerate request, independently constrain it; it cannot update locked destination |
| Nonempty merge queue / Merge when ready | Enumerate PR requests and every queue entry; no locked-destination update |
| Concurrent administrator or settings change | Fence/serialize administrators operationally; any change invalidates snapshot. No claim that GitHub prevents an admin editing settings |
| Source SHA changes during approval | Reject superseded source/approval; rerun exact identity evaluation, remain locked |
| Base SHA changes during approval | Reject superseded base/tree; inspect actual ref, remain locked |
| Previously successful but revoked App check / expired lease | Acknowledge SUCCESS may persist; otherwise-green stale check cannot defeat lock |
| Stale approved merge tree | Reject tree mismatch; source-head match alone cannot authorize admission |
| Controller outage before admission | No admission permitted; lock must independently deny otherwise-green PR |
| Controller outage after unauthorized unlock | Incident: automatic closure is not guaranteed; do not deliberately unlock production to test this |
| API failure while relocking / uncertain lock readback | Incident; no verified closure until settings and branch SHA independently read back; isolate failure injection from production |
| Direct push / force push / fork sync / deletion | Test each applicable path/identity under lock; no update or deletion; readback settings/SHA |
| Wrong App ID or spoofed context | Exact App-origin check requirement rejects unrelated success; verify identity live |
| Branch protection/ruleset bypass actor | No ordinary-writer bypass; settings-changing admin power remains a residual risk; unknown bypass => stop |
| Exception without independent approval | Always prohibited; even approved exception is prohibited under this candidate |

### Read-only observations from this correction session

The staging HEAD and frozen PR #132 HEAD matched the request. A complete one-page GraphQL read returned eight open PRs with `autoMergeRequest=null` and `mergeQueueEntry=null`; a separate queue query for each destination returned null. Both actual destination branches still reported `protected=false`, and complete repository ruleset enumeration was empty; containment is not currently established. These are transient observations, not future acceptance evidence. No queue cancellation, App installation, branch protection activation, live merge attempt or privileged enforcement change was performed.

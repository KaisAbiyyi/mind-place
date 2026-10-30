# Mind Place P0 Checkpoint 2B — Actual Enforcement Architecture

**Status: DESIGN ONLY — NOT IMPLEMENTATION AUTHORIZATION**
**Decision: NO-GO for production activation or useful merge admission.**
**Baseline:** `codex/mp-p0-authority-2a` at `c861bc14c2179407ad71959fec836bf3bf77da0c`
**Tracking:** [Issue #134](https://github.com/KaisAbiyyi/mind-place/issues/134)
**Design branch:** `codex/mp-p0-authority-2b-design`

---

## 1. Executive verdict

### 1.1 Decision

GitHub-native required checks, App-bound check identities and merge queues do **not** provide an atomic, live-revocable merge authorization primitive.

A completed SUCCESS can remain a successful historical check after:

- the controller's internal lease expires;
- the approved policy fingerprint is revoked;
- the policy revision changes;
- the GitHub App is suspended or uninstalled;
- the controller or authority service becomes unavailable.

GitHub's required-check freshness rule limits how old a successful required check may be: it must have completed successfully within the previous **seven days**. That is not a custom lease, is not immediate revocation, and is far longer than the existing 600-second controller lease. A success inside that window can continue satisfying the required check unless it is superseded, becomes stale under GitHub's rules, or is actively changed.

Therefore:

1. **A — PR-head checks:** necessary evidence, insufficient as merge authorization.
2. **B — mandatory merge queue:** materially improves source/base/queue-tree freshness, insufficient for live policy revocation after a successful merge-group check.
3. **C — App-mediated merge authorization:** the strongest candidate for a useful merge path because the broker can independently revalidate live authority and be the only permitted branch updater. It still depends on external coordination, GitHub settings and a trusted privileged App. GitHub does not atomically commit the external authorization decision with the merge.
4. **Safest currently supportable posture:** permanent-lock / no-admission containment. No stale check can authorize a merge if GitHub actually denies every merge/update path and the lock remains enforced. This posture sacrifices availability and still trusts the account owner not to remove the lock.

**Checkpoint 2B design verdict: GO for independent review of this document only. NO-GO for implementation, App installation, settings changes, production activation or merging.**

### 1.2 What is and is not guaranteed

| Property | GitHub-native guarantee? | Finding |
|---|---|---|
| Required check came from a selected GitHub App | Yes, when the expected source is configured and enforced | Binds producer identity, not current authority |
| Required check applies to latest commit SHA | Yes, subject to required-check configuration | Does not bind a mutable policy revision to the same SHA |
| Required check must be recent | Yes: completed successfully within the past seven days | Not a 600-second lease or immediate revocation |
| Merge-group checks bind the current queue group | Yes, for the group SHA GitHub creates | Improves code/base freshness, not authority liveness |
| App suspension revokes old SUCCESS | No documented automatic behavior | Treat old SUCCESS as still valid until GitHub readback proves otherwise |
| Controller outage invalidates old SUCCESS | No | No automatic lease-to-check coupling |
| External policy revocation and GitHub merge are one atomic transaction | No | No GitHub API accepts an external authorization epoch/one-use permit as a merge precondition |
| Merge API checks expected PR head SHA | Yes | No documented expected-base-SHA parameter |
| GitHub prevents the resource owner from changing enforcement | No | Owner/enterprise-owner remains a root of trust |
| Immediate cancellation of an already accepted asynchronous merge | Not established | No cancellation operation is documented in the asynchronous merge API |

The architecture must not claim stronger guarantees than these.

---

## 2. Current architecture and repository-verified state

### 2.1 Owner-designated 2A starting point and acceptance-record discrepancy

The supplied starting commit exists and is the current tip of `codex/mp-p0-authority-2a`. The execution brief supplied for this checkpoint describes the bounded 2A correction as accepted after independent code review. However, the latest visible comment in [Issue #133](https://github.com/KaisAbiyyi/mind-place/issues/133) still says not to treat the implementation/CI result as Checkpoint 2A acceptance; the inspected issue thread contains no later acceptance update. This is an unresolved record discrepancy.

This design uses `c861bc14c2179407ad71959fec836bf3bf77da0c` as the owner-designated starting point, but does not independently assert formal 2A acceptance or production security acceptance. P0 remains OPEN in either case.

The supplied starting commit is:

- Branch: `codex/mp-p0-authority-2a`
- HEAD: `c861bc14c2179407ad71959fec836bf3bf77da0c`
- Parent: `a9276f64c58ce03ab90fe29d5c84ee2fdc80bd46`
- Exact-HEAD Ubuntu run: [41013445234](https://github.com/KaisAbiyyi/mind-place/actions/runs/41013445234)
- Checkpoint 2A handoff: [Issue #133](https://github.com/KaisAbiyyi/mind-place/issues/133)

The 2A correction is a read-only staging inspector. `authority-staging/scripts/merge_freeze.py`:

- performs GET-only inspection;
- validates repository identity, ruleset inventory, active detail and effective branch rules;
- rejects incomplete, inconsistent or hidden bypass evidence;
- reads branch-lock and required-check configuration;
- reports a configuration snapshot only;
- has `no_merge_authorization=true`;
- prohibits temporary unlock and automatic unfreeze.

The example freeze config is disabled and uses `app_id: 0`. The inspector does not merge, publish an authorization, change settings, or invalidate existing check runs. Passing its tests is not live enforcement.

### 2.2 Actual target configuration at the inspected HEAD

The repository metadata and public branch summaries returned:

| Item | Observed value |
|---|---|
| Repository | `KaisAbiyyi/mind-place` |
| Visibility | Public |
| Owner type | Personal user (`User`), not an Organization |
| Default branch | `main` |
| `main` HEAD | `efe91155d37d8b1ca0608fe3bd862be311fed004` |
| `main` protection | `protected=false`, protection disabled, required checks off |
| Stacked base `codex/mp-ci-100k-01-20261027` HEAD | `7733a91fa7bbfc23504bf4b2f8af09baaa3f8a3f` |
| Stacked base protection | `protected=false`, protection disabled, required checks off |
| 2A staging branch | `protected=false` |
| Repository ruleset inventory with `includes_parents=true` | Empty |
| Repository `allow_auto_merge` metadata | `false` |

Evidence endpoints:

- [Repository metadata](https://api.github.com/repos/KaisAbiyyi/mind-place)
- [main branch summary](https://api.github.com/repos/KaisAbiyyi/mind-place/branches/main)
- [stacked-base branch summary](https://api.github.com/repos/KaisAbiyyi/mind-place/branches/codex%2Fmp-ci-100k-01-20261027)
- [2A branch summary](https://api.github.com/repos/KaisAbiyyi/mind-place/branches/codex%2Fmp-p0-authority-2a)
- [ruleset inventory including parents](https://api.github.com/repos/KaisAbiyyi/mind-place/rulesets?includes_parents=true)

The explicit branch-protection endpoint returned HTTP 403 to the connected inspection integration. The public branch summaries nevertheless report `protected=false` and `protection.enabled=false`. This document does not claim to have inspected every privileged settings field or every account-level setting.

Because the repository is currently user-owned, Organization-level inherited rulesets are not currently in scope. If the repository is transferred to an Organization or enterprise, inherited Organization/enterprise rulesets become a new mandatory inspection gate. The empty repository ruleset inventory is not a substitute for future owner/admin inspection of every applicable scope.

**Current conclusion:** no production merge enforcement has been established. Existing destination branches are not protected in the observed GitHub branch summaries. Production enforcement remains NO-GO.

### 2.3 Existing workflows and authority template

At the supplied HEAD:

- `.github/workflows/ci.yml` triggers on `push` to `main`, `pull_request`, and manual dispatch. It has no `merge_group` trigger.
- `.github/workflows/windows-product.yml` triggers on `pull_request`, `push` to `main`, and manual dispatch. It has no `merge_group` trigger.
- `.github/workflows/mp-2a-security-staging.yml` is limited to the 2A staging branch and tests the staging package.
- `authority-staging/.github/windows-policy/verifier.yml` is an unpromoted template scheduled every five minutes plus manual dispatch. It does not handle `merge_group`.
- The template's proposed target App permissions are Contents read, Actions read, Pull requests read, Administration read and Checks write. It does not have Contents write or a merge permission.
- `authority-staging/policy/merge-freeze.example.json` is disabled.
- PR #132 remains Draft, with head `3ab2516ae468e2dfe03bac3b26395db44df2db03` and stacked base `codex/mp-ci-100k-01-20261027`.

The current template is a verifier, not a merge broker. No independent App installation, live authority acceptance, branch restriction, required App check, merge queue or production freeze is established by this checkpoint.

---

## 3. Threat model and trust boundaries

### 3.1 Assets

1. Protected destination branch refs and their effective trees.
2. Source PR identity, repository ID and exact head SHA.
3. Base branch name and exact base SHA.
4. Approved merge/test tree SHA.
5. Immutable workflow, verifier, native-step manifest and input-contract blobs.
6. Policy revision and fingerprint.
7. Native qualification proof, workflow run/attempt and artifact digest.
8. Approval identity, approval generation/epoch, expiry and revocation state.
9. Required check run ID, conclusion and producing App ID.
10. Rulesets, branch protection, merge queue configuration and bypass actors.
11. Broker credentials, checker credentials, signing keys and durable merge journal.

### 3.2 Adversaries and failures

- A PR author updates the source after verification.
- A PR author or maintainer retargets a PR to another base.
- The base branch changes while a PR is being verified.
- A previously approved fingerprint is revoked while its old check remains successful.
- A policy revision changes while the controller is unavailable.
- A controller or authority service stops, restarts, loses its database connection or receives delayed/out-of-order webhooks.
- The checker App is suspended, uninstalled, loses permissions or has its credentials compromised.
- The merge broker is compromised or its installation token is stolen.
- A human, bot, GitHub Action, API client, GraphQL client or UI actor attempts to merge or update a protected branch outside the broker.
- A repository admin, Organization owner or enterprise owner changes enforcement or bypass configuration.
- A repository is transferred, renamed, forked or moved under a different inherited ruleset scope.
- GitHub API responses are delayed, rate-limited, incomplete or unavailable.
- A merge request is accepted by GitHub but its response is lost.

### 3.3 Trust boundaries

| Boundary | Trusted for | Not trusted for |
|---|---|---|
| PR source repository | Supplying source objects | Approval, policy, check identity or controller code |
| GitHub Checks API | Recording a check run and its producer | Current external policy validity |
| GitHub branch protection/rulesets | Enforcing configured branch requirements | Preventing the resource owner from editing configuration |
| GitHub merge queue | Building and validating current merge groups | Live policy revocation after a required check succeeds |
| Checker App | Producing a check for an exact SHA and evidence tuple | Merging or changing repository settings |
| Merge Broker App | Executing a serialized, live-authorized merge | Acting as its own independent approver |
| External authority service | Current policy epoch, approval and revocation state | Atomic control of GitHub's merge transaction |
| Repository/account owner | Ultimate repository administration | A security boundary that GitHub can enforce against itself |

A compromised checker App must not be able to merge. A compromised merge broker remains a critical risk because GitHub does not natively require the broker to consult an external authority service before each merge.

---

## 4. GitHub platform behavior

This section separates **documented behavior** from **repository observations** and **design inference**.

### 4.1 Completed SUCCESS and freshness

**Documented:**

- A required status check must have completed successfully in the chosen repository within the previous seven days.
- A required check must pass on the latest commit SHA; a check from an earlier commit does not satisfy the latest-SHA requirement.
- A required check can be configured to require the branch to be up to date with its base (strict mode).
- A completed check run records its status and conclusion. The Checks API exposes an update operation; its conclusion vocabulary includes `failure`, `success`, `cancelled`, `timed_out`, etc. Only GitHub can set the `stale` conclusion.
- The API supports updating a check run, but this is an explicit API action, not a lease-aware automatic revocation system.

**Security consequence:**

The internal 600-second lease is invisible to branch protection. If a successful check is still within GitHub's seven-day freshness window and is attached to the required SHA/context/source, it can continue to satisfy the required check after the controller's own lease expires or its policy changes.

The check may remain visible in history after it stops satisfying the seven-day rule. Thus:

- "SUCCESS remains in history indefinitely" and
- "SUCCESS satisfies a required check indefinitely"

are different claims. The first is not a merge authorization rule; the second is false because of the seven-day requirement.

**Revocation consequence:**

An authorized App with Checks write access can attempt to update a check run to a failing conclusion and read the result back. This can be a useful incident action while the App is reachable and authorized. It is not a reliable primary control because:

- it requires a successful API write;
- the App can be suspended or uninstalled;
- credentials, permissions, rate limits or GitHub API availability may fail;
- the controller may not know every relevant check run ID;
- a merge can race with the attempted update;
- GitHub does not bind this update to the external policy's revocation transaction.

Do not close P0 based on a periodic sweep or a best-effort PATCH.

References:
- [Troubleshooting required status checks](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/troubleshooting-required-status-checks)
- [REST API endpoints for check runs](https://docs.github.com/en/rest/checks/runs)
- [About protected branches](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches)

### 4.2 Required status-check App identity

**Documented:**

GitHub can configure a required status check to accept updates only from a selected GitHub App. If another person or integration sets a status, the check does not satisfy that expected-source requirement. Rulesets support an expected App source for required status checks. The Checks API identifies the App associated with a check run.

**What this proves:**

- The check came from the configured App identity, assuming the expected source is configured correctly.
- A different App or ordinary status writer cannot satisfy that App-bound requirement.

**What this does not prove:**

- The App is still installed, active or uncompromised.
- The App's policy revision remains current.
- The approval or fingerprint remains valid.
- The controller is currently healthy.
- The check's internal lease has not expired.
- The App has not been suspended after it created the check.
- The merge is still authorized at the moment GitHub updates the target branch.

App-ID binding is an identity control, not a liveness or revocation protocol.

**Permission nuance:** The Checks API documents `Checks:write` for creating/updating check runs. The rulesets documentation describes `Statuses:write` for an expected source that publishes commit statuses. These permission sets are not interchangeable assumptions. The 2A template uses the Check Runs API path and `Checks:write`; the exact check type, App-source selector and required-check representation must be proven together on a disposable repository. Do not broaden App permissions or silently switch from check runs to commit statuses to make a setting appear green.

Reference: [Available rules for rulesets — required status checks](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/available-rules-for-rulesets#require-status-checks-to-pass-before-merging).

### 4.3 App suspension, uninstallation and credential revocation

**Documented:**

Suspending a GitHub App installation blocks that App from accessing resources owned by the installation account. Uninstalling it removes its access to that account's resources. The documentation describes access control; it does not document a retroactive rewrite of previously completed check-run conclusions.

**Design rule:**

Treat existing completed SUCCESS runs as still successful until GitHub readback proves otherwise. App suspension or uninstall must not be treated as check invalidation.

The same conservative rule applies to private-key rotation, installation-token expiry and controller shutdown: none is a documented retroactive invalidation mechanism for a completed check run.

References:
- [Reviewing and modifying installed GitHub Apps](https://docs.github.com/en/apps/using-github-apps/reviewing-and-modifying-installed-github-apps)
- [Suspending a GitHub App installation](https://docs.github.com/en/apps/maintaining-github-apps/suspending-a-github-app-installation)

### 4.4 Merge queue and `merge_group`

**Documented:**

- A merge queue builds temporary merge-group branches.
- The merge group includes the latest target base and changes from PRs ahead of the queued PR.
- GitHub sends a `merge_group` webhook with action `checks_requested`.
- CI must subscribe to `merge_group` separately from `pull_request`; otherwise required merge-group checks are not reported.
- When base/queue composition changes, GitHub recreates merge groups and re-runs the required checks against the new group.
- The queue waits for required checks and has a configurable timeout. A timeout or failing check can remove a PR from the queue.
- When required merge-group checks succeed, GitHub merges the group.

**Security benefit:**

A successful check on an old PR-head SHA is not a substitute for the required check on the current merge-group SHA. The queue materially improves the binding between the code tested and the base/preceding queue entries that GitHub will merge.

**Security limitation:**

The merge-group check is still a completed check result. The queue does not ask the external authority service for a second atomic approval at the exact instant the ref update occurs. If the checker publishes SUCCESS and the approval is revoked or the App is suspended before GitHub completes the queue merge, the already successful merge-group check may still satisfy the queue. A queue timeout protects the no-response case; it does not expire a successful response or make it live-revocable.

Removing a queued PR or changing its check can be an incident response while the relevant API and permissions are available. It is not a guarantee under controller/API outage.

**Repository eligibility:**

GitHub documents merge queues for public repositories owned by an Organization, or private repositories owned by Organizations using GitHub Enterprise Cloud. This repository is currently public and user-owned. It is therefore not currently eligible for a merge queue without an ownership/plan change.

**Repository implementation gap:**

Neither `.github/workflows/ci.yml` nor `.github/workflows/windows-product.yml` currently includes `merge_group`. The authority template does not either. Merge queue activation would require explicit workflow changes and live qualification; no such change is authorized here.

References:
- [Managing a merge queue](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue)
- [Webhook events — merge_group](https://docs.github.com/en/webhooks/webhook-events-and-payloads#merge_group)

### 4.5 Merge API compare-and-swap semantics

**Documented:**

- The direct merge API accepts `sha`, which is the expected PR **head** SHA.
- If the PR head no longer matches, GitHub rejects the merge.
- The documented API does not accept an expected base SHA, expected effective merge-tree SHA, policy fingerprint, approval epoch, one-use authorization nonce or external lease.
- The synchronous direct merge endpoint does not support stacked pull requests or merge queues.
- The asynchronous merge endpoint is required for stacked pull requests and includes all open downstack PRs. It accepts an expected head SHA, but not an expected base SHA. It returns a UUID and can remain pending while GitHub processes the merge.
- The asynchronous merge endpoint can enqueue a PR; an `enqueued` result is not a completed merge. The asynchronous request result is retained for 24 hours after its most recent update.

**Security consequence:**

A final read of the base SHA followed by a merge API call is not an atomic base-SHA compare-and-swap. A base update or PR retarget can race between those operations unless all competing update paths are prevented by a separate, continuously enforced single-writer design.

For stacked PRs, the asynchronous merge path adds a pending operation. The documented API does not provide a cancellation primitive for a pending direct asynchronous merge. An accepted-but-unresolved merge must be treated as in flight, not as safely revoked.

**Recommended constraint for any first broker prototype:**

Do not authorize stacked PR merges. Require the downstack PRs to be merged and the candidate to be retargeted/rebased and freshly qualified as an ordinary, unstacked PR before broker admission. If stacked merges are required, the asynchronous merge operation and its non-cancellation/indeterminate-result behavior must be accepted as an explicit residual risk. Do not silently treat it as an atomic synchronous operation.

References:
- [REST API endpoints for pull requests — merge](https://docs.github.com/en/rest/pulls/pulls#merge-a-pull-request)
- [REST API endpoints for pull requests — asynchronous merge](https://docs.github.com/en/rest/pulls/pulls#merge-a-pull-request-asynchronously)

### 4.6 Branch protection, rulesets and bypass

**Documented:**

- Classic branch protection does not apply to administrators and custom roles with bypass permission by default; the owner can enable "Do not allow bypassing the above settings."
- Rulesets can define bypass actors. Eligible actors include repository administrators, Organization owners, enterprise owners, roles, teams, GitHub Apps and Dependabot.
- A ruleset can restrict updates so only bypass actors can push.
- Organization-level rulesets aggregate with repository rulesets. Repository rulesets can make the result more restrictive; they cannot make an Organization-level rule less restrictive.
- Organization-level rulesets are managed by Organization owners; repository administrators cannot edit the inherited Organization ruleset itself.
- The repository/account owner can still change account/repository configuration or transfer/delete the repository.

**Security consequence:**

A bypass list is part of the security policy, not harmless administration metadata. An App or role on a bypass list can create an alternate update path. A rule that requires a specific App check is insufficient if another actor can bypass that rule.

For an Organization-owned deployment, the desired posture is no human, role, team or unrelated App bypass actor on the enforcement ruleset; branch protection should include administrators; organization-level rulesets should be used where the plan supports them to keep repository admins from weakening inherited policy.

No GitHub configuration can prevent the ultimate account/Organization/enterprise owner from changing the controls they administer. That owner is a root of trust. External governance and audit alerts can make changes accountable, but they do not create an unchangeable GitHub lock.

References:
- [About protected branches](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches)
- [Available rules for rulesets](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/available-rules-for-rulesets)
- [Creating rulesets for repositories in your organization](https://docs.github.com/en/organizations/managing-organization-settings/creating-rulesets-for-repositories-in-your-organization)

---

## 5. Architecture alternatives

**Option-label note:** A/B/C in this document follow the explicit Checkpoint 2B execution brief: A = PR-head checks, B = mandatory merge queue, C = App-mediated merge authorization. Older staging documents use legacy option labels for different proposals; no legacy label mapping is assumed.

### A. PR-head check enforcement

**Shape**

1. A trusted checker App evaluates the exact PR head SHA and source-bound evidence.
2. It publishes a required App-bound check.
3. Branch protection requires that context from the exact App and requires the PR branch to be up to date.
4. GitHub permits merge when all required rules pass.

**Guarantees**

- A check from another App does not satisfy an App-bound expected source.
- A check for an older head SHA does not satisfy the latest-SHA requirement.
- Strict required checks require the PR branch to be up to date with the base.
- Checks older than the seven-day freshness window no longer satisfy required status checks.

**Failure modes**

- Same-head SUCCESS remains eligible during the seven-day window after policy revocation.
- Internal lease expiry is not interpreted by GitHub.
- App suspension or controller outage does not automatically rewrite the old conclusion.
- A best-effort check-run PATCH can fail or race with a merge.
- A base update after a check can create a new merge state; strict mode helps only with branch freshness, not external policy revocation.
- Admin/bypass configuration can remove the intended gate.

**TOCTOU**

The check is completed before the merge. GitHub does not query the authority service again when consuming the check. The policy may change after SUCCESS and before merge.

**Operational complexity:** Low to medium.

**Permissions/configuration:** Checker App needs the relevant read permissions and Checks write. Branch protection/ruleset must require the exact App identity. The App must not have merge or settings write permissions.

**Recovery:** Re-evaluate current policy and source; publish a new check only after current authority is established. Attempt to fail old check runs when possible, but do not depend on it.

<badge color="danger">Insufficient as sole enforcement</badge>

**Can stale SUCCESS still authorize a merge? Yes.** This is the original problem.

### B. Mandatory merge queue with merge-group validation

**Shape**

1. Require the merge queue on every destination branch.
2. Require all normal PR checks and the authority check.
3. Subscribe to `merge_group.checks_requested`.
4. Evaluate the exact merge-group SHA, current base and queued composition.
5. Publish required checks for that merge-group SHA.
6. GitHub merges the group after all required checks pass.

**Guarantees**

- A PR-head success alone is insufficient where the queue requires merge-group checks.
- The group SHA binds the latest base and earlier queue entries.
- Changes to base or queue composition result in new merge groups and checks.
- Missing checks time out; failing checks can remove the PR from the queue.
- The queue serializes integration into the target branch.

**Failure modes**

- If the controller is down before it publishes SUCCESS, the queue waits and eventually times out.
- If the controller publishes SUCCESS and then becomes unavailable, GitHub may still merge the already-green group.
- Revoking an approval or fingerprint after the group check succeeds does not automatically revoke that check.
- A queued PR may merge after checker App suspension if its required checks are already successful.
- Bypass actors can bypass the queue/rules unless the effective rules are configured to prevent it.
- Current repository ownership does not qualify for merge queues.

**TOCTOU**

The queue substantially reduces code/base TOCTOU: it validates the actual merge group and rebuilds when the base or queue changes. It does not eliminate the authority TOCTOU between check completion and merge. The queue does not consume a live external revocation token.

**Operational complexity:** High. Every relevant workflow and check provider must handle `merge_group`; queue timeout, build concurrency, queue membership and bypass configuration require live tests.

**Permissions/configuration:** Admin configures required merge queue. Checker App needs Merge queues read access for the webhook and Checks write to report checks. The repository must meet GitHub's merge-queue eligibility. Direct merge, auto-merge and bypass paths must be separately controlled.

**Recovery:** If no successful merge-group check exists, let the queue time out/fail. If an entry is already green and authority is revoked, remove it or invalidate its check only through an available authorized path, then read back queue and branch state. During API outage, do not claim the entry was revoked.

<badge color="warning">Useful freshness control, not revocation control</badge>

**Can stale SUCCESS still authorize a merge? Yes.** A SUCCESS on the current merge-group SHA can remain sufficient after a later authority revocation.

### C. App-mediated merge authorization

**Shape**

Separate the checker from the merge actor:

- **Checker App:** validates source/evidence and publishes an App-bound check. It has no Contents write or merge capability.
- **Merge Broker App:** performs the only permitted merge path. It cannot publish the checker App's required check and cannot change repository settings.
- **External authority service:** stores current policy epoch, approved fingerprint, approval identity, revocation state and a durable per-base merge journal.
- **Protected policy repository:** owns the controller and policy configuration separately from the target repository.

The broker must not treat check SUCCESS as authorization. It must independently query the current authority service and validate the complete admission tuple immediately before requesting a merge.

**Proposed admission tuple**

```text
repository_id
pull_request_number
source_repository_id
source_head_sha
base_ref
base_sha
effective_merge_tree_sha
policy_repository_id
policy_commit_sha
policy_fingerprint
native_proof_run_id_and_attempt
native_proof_artifact_digest
checker_app_id
required_check_run_id
approval_id
approval_generation
authorization_epoch
one_use_nonce
broker_app_id
```

Every field must be read from authoritative GitHub/API or policy-service state, not copied from untrusted PR text or a check summary.

**Proposed control flow**

1. Acquire a durable exclusive lock for the destination base.
2. Read current PR state, source repository/head, base ref/SHA, effective merge tree and exact branch/ruleset configuration.
3. Verify the required checker App run and exact check-run identity. SUCCESS is necessary evidence, not sufficient authorization.
4. Query the live authority service for the current policy commit, fingerprint, approval generation and revocation state.
5. Reject if any identity, SHA, tree, approval, policy epoch or App identity differs.
6. Build an immutable broker-owned promotion PR for the approved source tree. The first prototype must use an unstacked PR; stacked PRs require the asynchronous merge API and are excluded from the initial admission path.
7. Re-read source, base, PR and policy state. Reject if anything changed.
8. Obtain a one-use authorization while holding the same per-base transaction lock used by revocation.
9. Use the direct merge API with `sha=<expected PR head SHA>`, `bypass_rules=false`. The broker must not unlock the branch or disable any protection.
10. Read back the merge result, resulting base SHA and PR state. Record the result in an append-only journal before releasing the lock.

**Required GitHub-side single-writer posture**

The broker can only provide useful security if ordinary actors cannot update the target branch outside it.

Preferred candidate after an Organization migration:

- Require pull requests.
- Require strict required checks, including the exact Checker App identity.
- Enable administrator enforcement / "Do not allow bypassing the above settings".
- Restrict who can push to the Merge Broker App only.
- Block force pushes and deletions; disable fork syncing for protected destinations.
- No human, team, role, unrelated App or Dependabot bypass actor.
- No auto-merge or merge-queue path for the broker-controlled direct-merge mode.
- Limit repository write access; the bot-owned promotion PR must not be editable/retargetable by ordinary contributors.
- Keep repository settings administration separate from the broker App.

GitHub documents the "restrict who can push" setting for public repositories owned by a GitHub Free Organization and for Organization-owned repositories on Team/Enterprise plans. The current repository is personal-account-owned, so this setting is not currently available in the documented eligible configurations. A transfer to an Organization is therefore an owner decision/dependency for this preferred design.

A ruleset `restrict updates` rule with the broker App as its only pull-request-only bypass actor is a possible alternative, but it makes the broker a bypass actor for that ruleset. It must be isolated from the separate required-check ruleset and proven on a disposable repository before consideration. Do not put the broker in a bypass list for the same ruleset that carries required checks.

**Guarantees if correctly implemented and live-proven**

- A stale SUCCESS alone cannot cause a merge because the broker independently checks live authority.
- Controller/authority outage before a new authorization causes the broker to refuse new merge requests.
- A source-head change is guarded by the merge API's expected-head SHA.
- Base updates by competing writers can be prevented if GitHub continuously enforces the broker as the only permitted updater and the broker serializes all merges to that base.
- Approval revocation and merge requests can be serialized by one durable authority transaction.
- Recovery can require a new policy epoch and new authorization instead of reusing old SUCCESS.

**Failure modes and hard limitations**

1. GitHub does not atomically commit the external authority transaction with the branch update.
2. The merge API's expected SHA is the PR head SHA, not the base SHA or effective merge tree.
3. The PR base can be edited through PR APIs by sufficiently privileged actors. A bot-owned promotion PR and tightly restricted repository write access reduce this race, but repository admins/owners remain trusted.
4. A merge request already sent to GitHub may complete even if the controller or authority service becomes unavailable immediately afterwards.
5. If the merge request response is lost, the broker cannot assume failure. It must mark the transaction INDETERMINATE and stop all further merges until it reads back the PR and base state.
6. The asynchronous merge endpoint required for stacked PRs returns a pending request UUID. No documented cancellation endpoint establishes that a pending direct merge can be revoked. The initial broker must therefore reject stacked PRs rather than claim atomic revocation.
7. A compromised Merge Broker App or stolen Contents-write installation token can attempt a merge using any still-valid required check. GitHub does not force that App to consult the external authority service.
8. The repository owner/Organization owner can change the settings or remove the App. GitHub cannot enforce rules against the ultimate account owner.

**TOCTOU**

The design reduces the race using a single writer, per-base serialization, bot-owned immutable promotion PRs, a live authority transaction and the expected-head SHA. It does not eliminate every race at the GitHub/platform boundary. The absence of an expected-base-SHA compare-and-swap and the existence of in-flight merge requests are explicit residuals.

**Operational complexity:** Very high. Requires a separate authority service, durable transaction journal, two App identities, restricted branch-update configuration, serialized merges, exact PR/tree reconstruction, webhook reconciliation and live adversarial qualification.

**Permissions/configuration**

- Checker App: Metadata read, Contents read, Pull requests read, Actions read as needed, Checks write. No Contents write, no PR write, no Administration write.
- Merge Broker App: Metadata read, Contents write (required by the merge API), Pull requests read/write for bot-owned promotion PR lifecycle, Checks read. No Checks write, no Administration write, no policy-signing key.
- Authority service: independent policy signing/verification and strongly consistent storage; no GitHub branch-settings permission.
- Settings operator: separate owner/admin identity used only for configuration and readback, never embedded in the checker or broker workflow.
- Policy repository: separate ownership, protected environment, pinned commit and independent reviewer.

**Recovery**

- Never release a lock automatically after an ambiguous merge request.
- Reconcile the merge request UUID (if asynchronous), PR merged state, merge commit and base ref SHA.
- If GitHub/API state cannot be reconciled, remain INDETERMINATE and halt all further merges.
- A restart must not replay an old permit or restore a previous epoch.
- Revalidate all source/base/tree/policy/approval/App identities and issue a new one-use authorization only after independent approval.
- A best-effort PATCH of old checks is secondary incident hygiene, not the admission control.

<badge color="warning">Strongest useful-admission candidate; not a complete GitHub-native guarantee</badge>

**Can stale SUCCESS still independently authorize a merge? No, if the broker is the only enforced update actor and it never treats SUCCESS as sufficient.**
**Can GitHub alone guarantee immediate revocation across an already in-flight merge? No.** The broker design has a transaction boundary and residual root-of-trust assumptions. P0 must remain open until the owner accepts the stated revocation semantics and independent live tests prove the single-writer path.

---

## 6. Recommended architecture and rationale

### 6.1 Immediate recommendation: permanent-lock / no-admission

Until the owner explicitly accepts the residuals of a useful merge broker, retain the 2A rule:

- Keep all destination branches permanently locked.
- No temporary unlock, exception, direct merge, auto-merge or merge-queue admission.
- Keep required checks intact; never remove a required check to recover availability.
- Treat every historical SUCCESS as still successful until GitHub readback proves otherwise.
- If a lock cannot be independently read back, report containment as NOT VERIFIED and stop merges/releases operationally.
- Do not claim that a controller restart, App key rotation or check PATCH alone restores enforcement.

This is the only currently supportable posture that makes stale SUCCESS irrelevant by denying all merge admission, provided the lock is actually enforced and not changed by the account owner.

### 6.2 Future useful-merge candidate: C with an external serialized authority

If the owner requires normal merges, C is the preferred architecture to prototype after separate approval:

1. Transfer the repository to an Organization if the owner wants the documented App-only branch-push restriction and/or merge queue eligibility.
2. Establish a separate policy repository and independent authority service.
3. Split Checker App and Merge Broker App.
4. Make the broker the only ordinary permitted updater through PR-only branch restrictions; no temporary unlock.
5. Keep branch protection and required checks enforced for administrators.
6. Use a broker-owned promotion PR and prohibit stacked admission in the first version.
7. Serialize all merges and revocations with one durable per-base lock and an append-only journal.
8. Define revocation as effective only after any already in-flight merge transaction is resolved. If the owner requires revocation to take effect at the instant it is requested, this architecture cannot guarantee that property with GitHub's merge API.
9. If a merge request is ambiguous, halt all further merges until reconciled.
10. Keep P0 open until independent live qualification demonstrates every required denial path.

### 6.3 Why B is not the final authority

A merge queue is a useful integration mechanism, but it should not be presented as a live revocation mechanism. If the organization later adopts a queue for throughput and code freshness, retain an independent authority gate and explicitly accept that a successful merge-group check can still be consumed after a subsequent revocation unless another enforced gate prevents it.

Do not enable both a queue and a broker without specifying exactly which one owns final merge admission. A queue automatically merges after checks pass; a direct broker owns the merge call. Mixing both creates ambiguous authority and bypass paths.

### 6.4 Mandatory invariant disposition

The following table is the explicit security disposition for the ten required invariants. "Conditional" means a proposed control exists but has not been implemented or live-proven. "Not guaranteeable" means the requested absolute property is not provided by GitHub's documented primitives under the stated trust model.

| # | Required invariant | Permanent-lock / no-admission | App-mediated broker candidate | Platform limitation / qualification |
|---:|---|---|---|---|
| 1 | Fail closed when controller or authority is unavailable | Yes for new admission while the lock remains enforced | Conditional for new authorization; an already-sent/in-flight GitHub merge may complete | Immediate cancellation of an accepted merge is not guaranteed |
| 2 | Stale SUCCESS cannot independently authorize a merge | Yes, because no merge admission exists | Yes, only if the broker is the sole enforced updater and rechecks live authority | A/B alone do not satisfy this invariant |
| 3 | Revoked approvals/fingerprints cannot authorize future merges | Yes for admission, because all merges are denied | Conditional after serialized in-flight work is resolved and the epoch is incremented | Immediate revocation of an already accepted merge is not guaranteeable |
| 4 | Exact source SHA, base SHA, policy fingerprint and authorization identity remain bound | No admission is issued | Conditional via signed tuple, bot-owned PR, live reads and durable journal | Merge API has expected head SHA, not expected base SHA/tree SHA |
| 5 | PR updates during verification invalidate obsolete authorization | No admission | Conditional via expected head SHA and re-read; bot-owned PR limits ordinary retargeting | No atomic base/PR-metadata compare-and-swap exists |
| 6 | No administrator or unintended actor silently bypasses enforcement | Conditional on the lock being enforced and not changed by the owner | Conditional for ordinary actors with no bypass and enforced App-only updates | Ultimate account/Organization/enterprise owner can change the settings; absolute protection against that root is impossible |
| 7 | Inherited Organization rulesets are accounted for | Current repository is user-owned; future transfer requires a full parent-scope audit | Same; every repo/Organization/enterprise ruleset and effective branch rule must be re-read | Current repo has no Organization owner; future inheritance is a migration gate |
| 8 | No temporary unlock or protection-removal window | Yes by policy; no admission | Yes by design; protection remains continuously enabled | Requires live proof that broker admission does not depend on unlocking |
| 9 | No race-prone transition between verification and merge authorization | Yes for merge admission, because no merge occurs | Conditional mitigation through single writer, lock and head SHA | GitHub has no atomic external-authority + expected-base merge transaction; in-flight ambiguity remains |
| 10 | Recovery never restores expired or revoked authorization | Yes; no authorization is restored | Conditional through epoch bump, journal reconciliation and fresh approval only | Must be implemented and tested; GitHub does not perform this external recovery logic |

**Result:** no useful-merge architecture can currently be certified as satisfying all ten invariants absolutely. Permanent-lock/no-admission is the safest current posture. The broker candidate can satisfy "stale SUCCESS is not independently sufficient" but retains explicit in-flight, base-SHA and root-owner limitations. P0 therefore remains OPEN until those residuals are either accepted by the owner or eliminated by a stronger independently proven mechanism.

---

## 7. Authorization and revocation state machine

### 7.1 Broker transaction states

| State | Meaning | May request merge? |
|---|---|---|
| `UNVERIFIED` | No complete current tuple | No |
| `OBSERVED` | Source, base, tree, policy and checks read | No |
| `AUTHORITY_PENDING` | Live authority query/approval validation in progress | No |
| `AUTHORIZED_IN_FLIGHT` | One-use grant acquired under per-base lock | Only the single recorded transaction |
| `MERGE_REQUEST_SENT` | GitHub merge request submitted | No second request |
| `MERGED` | GitHub merge and base readback confirmed | Transaction complete |
| `DENIED` | A required identity/approval/policy check failed | No |
| `INDETERMINATE` | Request may have succeeded but cannot be reconciled | No; halt base |
| `REVOKE_PENDING` | Revocation requested; new grants denied; an existing transaction is being resolved | No new request |
| `REVOKED` | In-flight transaction resolved; epoch incremented; old grants invalid | No |
| `AUTHORITY_UNAVAILABLE` | Authority service unavailable before authorization | No |

No state may transition from `DENIED`, `REVOKED` or `INDETERMINATE` to `AUTHORIZED_IN_FLIGHT` by replaying a previous check result.

### 7.2 Revocation linearization

The authority service must use the same durable lock/fencing generation for merge admission and revocation.

- A revocation request immediately blocks **new** grants.
- If a merge transaction has not sent a GitHub request, it is aborted and the epoch is incremented.
- If a synchronous merge request is already in flight, revocation remains `REVOKE_PENDING` until the outcome is read back.
- If the outcome cannot be determined, the system remains `INDETERMINATE`; it must not claim the approval is fully revoked and must stop further merges.
- Only after all in-flight transactions are resolved may the service commit `REVOKED` and increment the authority epoch.
- A check run that still shows SUCCESS after this transition is historical evidence only; the broker must reject it as an authorization source.

This defines revocation as effective after the serialized in-flight transaction boundary. If the owner requires a revocation request to cancel a merge that GitHub has already accepted, GitHub provides no documented atomic cancellation primitive. That requirement remains unsupported.

### 7.3 Permanent-lock mode

In the immediate recommended posture, the merge state machine has no admission transition:

```text
ANY REQUEST -> DENIED
ANY CONTROLLER STATE -> NO MERGE AUTHORIZATION
```

The lock itself must be independently enforced and read back. If its state is unknown, containment is NOT VERIFIED.

---

## 8. Sequence diagrams

### 8.1 Normal broker operation (proposed, not implemented)

```mermaid
sequenceDiagram
    participant PR as Source PR
    participant CH as Checker App
    participant GH as GitHub
    participant AS as Authority Service
    participant MB as Merge Broker

    PR->>CH: Source/evidence event
    CH->>GH: Publish App-bound check for exact SHA
    GH-->>CH: Check run ID and App identity
    PR->>MB: Owner-approved merge request
    MB->>AS: Acquire per-base lock and current epoch
    AS-->>MB: Lock/fencing generation
    MB->>GH: Read PR, source SHA, base SHA, tree and rules
    GH-->>MB: Current GitHub state
    MB->>AS: Validate current fingerprint and approval
    AS-->>MB: One-use authorization under lock
    MB->>GH: Create/verify bot-owned unstacked promotion PR
    GH-->>MB: Exact promotion head and base
    MB->>GH: Re-read state; merge with expected head SHA
    GH-->>MB: Merge result
    MB->>GH: Read back PR and destination ref
    GH-->>MB: Confirmed merge commit and new base SHA
    MB->>AS: Commit result; release lock
    AS-->>MB: Transaction complete
```

The check run is an input to the broker's decision, not the decision itself.

### 8.2 Revocation

```mermaid
sequenceDiagram
    participant O as Owner/Approver
    participant AS as Authority Service
    participant MB as Merge Broker
    participant GH as GitHub

    O->>AS: Revoke approval/fingerprint
    AS->>AS: Mark REVOKE_PENDING; deny new grants
    AS-->>MB: Revocation/fencing update
    alt No merge request in flight
        MB->>MB: Abort admission
        AS->>AS: Increment epoch; mark REVOKED
    else Merge request already sent
        MB->>GH: Read merge request / PR / base outcome
        GH-->>MB: Merged, failed or unresolved
        alt Outcome confirmed
            MB->>AS: Record outcome
            AS->>AS: Increment epoch; mark REVOKED
        else API unavailable or ambiguous
            MB->>AS: Mark INDETERMINATE
            AS->>AS: Keep base halted; do not claim revocation complete
        end
    end
    MB->>GH: Best-effort PATCH old check to failure
    Note over GH,MB: Historical SUCCESS is never trusted as current authorization
```

### 8.3 Controller or authority outage

```mermaid
sequenceDiagram
    participant MB as Merge Broker
    participant AS as Authority Service
    participant GH as GitHub

    MB->>AS: Request current authorization
    AS--xMB: Timeout / unavailable
    MB->>MB: DENIED; no new merge request
    Note over MB,GH: Existing check SUCCESS may remain visible
    alt Merge request was already sent
        MB->>GH: Attempt outcome readback
        GH--xMB: API unavailable
        MB->>MB: INDETERMINATE; halt base
        Note over MB,GH: In-flight request may still complete
    end
```

### 8.4 Recovery

```mermaid
sequenceDiagram
    participant MB as Merge Broker
    participant AS as Authority Service
    participant GH as GitHub
    participant R as Independent Reviewer

    MB->>AS: Enter RECOVERY; keep admission disabled
    MB->>GH: Read PR, merge result, base SHA, rules and checks
    GH-->>MB: Current GitHub state
    MB->>AS: Reconcile durable journal and authority epoch
    alt Complete, consistent evidence
        R->>AS: Independently approve current policy/fingerprint
        AS->>AS: Increment generation; issue new authorization state
        MB->>AS: Request a new one-use grant for fresh tuple
    else Missing, stale or ambiguous evidence
        AS->>AS: Remain HALTED / INDETERMINATE
        Note over MB,AS: No automatic unlock or old-permit replay
    end
```

---

## 9. Failure-injection and adversarial test matrix

Synthetic tests are necessary for state-machine and parser correctness, but they do not prove GitHub enforcement. Every GitHub-side denial must be demonstrated on a disposable Organization-owned test repository before production authorization. No test may temporarily unlock a production branch.

| # | Injection | Expected result | Evidence required |
|---:|---|---|---|
| 1 | Same PR head, successful check younger than 7 days, internal lease expired | A/B may still see SUCCESS; broker denies if live authority expired | Real check run + broker denial |
| 2 | Same PR head, successful check older than 7 days | Required check no longer satisfies GitHub freshness | GitHub merge-box/API observation |
| 3 | Fingerprint revoked after SUCCESS | Existing check may remain green; no broker grant | Revocation epoch + merge denial |
| 4 | Policy revision changes after SUCCESS | Old tuple rejected | Exact policy SHA/epoch mismatch |
| 5 | Approval revoked before authorization | No merge request | Authority journal and no merge event |
| 6 | Revocation concurrent with merge authorization | Serialized; either merge transaction completes before revocation is effective or no merge is sent | Lock/fencing and ordered audit records |
| 7 | Revocation after GitHub accepts merge request | Mark REVOKE_PENDING; reconcile; no false claim of immediate cancellation | Merge request UUID/result and base readback |
| 8 | Controller unavailable before authorization | No merge request | Broker denial and unchanged base |
| 9 | Controller unavailable after merge request sent | INDETERMINATE until GitHub readback | Durable journal; no retry |
| 10 | Authority database unavailable | No new grant | Fail-closed broker log |
| 11 | Checker App suspended/uninstalled after SUCCESS | Historical check treated as still successful; broker denies if authority cannot be verified | App state, existing check run, broker denial |
| 12 | Wrong App publishes same context | App-bound required check rejects it | App IDs in check runs and merge-box result |
| 13 | Checker App credential revoked | No new check; old check not trusted as live authorization | API failure and broker denial |
| 14 | Broker App token stolen | Branch rules and required checker App still apply; incident halt and key revocation | Disposable repo red-team test |
| 15 | PR head changes after verification | Expected-head SHA mismatch; reject and re-evaluate | GitHub merge API conflict |
| 16 | PR base retargets after verification | Reject promotion PR / require fresh admission | PR base before/after and journal |
| 17 | Base branch changes between read and merge | Only broker may update; serialization prevents competing broker update; otherwise halt | Ref history and broker lock |
| 18 | Two brokers race on same base | Exactly one transaction holds the lock | Concurrent test and append-only journal |
| 19 | Merge API returns timeout after request | INDETERMINATE; no retry or new merge until reconciliation | API trace and PR/base readback |
| 20 | Stacked PR uses asynchronous merge | Reject in initial broker version | Explicit no-stacked admission record |
| 21 | Queue base/preceding PR changes | New merge_group SHA and new checks | Webhook payloads and group SHAs |
| 22 | Queue SUCCESS followed by revocation | Demonstrate queue may still consume old SUCCESS; do not claim revocation guarantee | Live disposable queue observation |
| 23 | Queue controller outage before check | Queue timeout/failure, no merge | Queue timeline |
| 24 | Queue controller outage after SUCCESS | Residual race recorded; no guarantee claimed | Queue timeline and merge-group check |
| 25 | Repository admin attempts bypass | Denied under effective settings | Live merge/push probes |
| 26 | App/team/role/Dependabot bypass actor added | Configuration audit rejects; production remains disabled | Complete bypass inventory |
| 27 | Organization ruleset inherited | Effective rules and bypass actors inspected at Organization scope | Parent inventory + effective branch rules |
| 28 | Organization ruleset changed during operation | Broker detects policy/settings fingerprint drift and halts | Before/after settings snapshots |
| 29 | Force push, direct push, deletion or fork sync | Denied for protected destinations | Live API and Git push probes |
| 30 | REST, GraphQL, UI, CLI, auto-merge and queue alternate paths | All unauthorized paths denied | Per-path denial receipts |
| 31 | Required checker context removed or renamed | Activation gate rejects configuration | Settings readback |
| 32 | Existing check PATCH fails | Treat SUCCESS as still valid; merge remains blocked by independent control | PATCH error and merge denial |
| 33 | Webhook duplicated, delayed, replayed or reordered | Idempotent event processing; state re-read from GitHub | Event IDs and journal |
| 34 | API 403/404/409/422/429/5xx or malformed response | Fail closed; no guessed state | Recorded response and no merge |
| 35 | Broker restarts with stale lock/lease | No automatic lease expiry/unlock; reconcile journal | Recovery test |
| 36 | Account owner changes ruleset or transfers repository | Treat as root-of-trust configuration change; halt pending independent review | Audit log and settings diff |

### Required live test design

- Use a disposable public Organization-owned repository with the intended plan and App restrictions.
- Use an otherwise-green PR and an App-bound check.
- Exercise each allowed and denied path using separate identities.
- Include a controller outage after a successful check, not only before it.
- Include a revoked fingerprint with the same source SHA.
- Include an App suspension with an existing SUCCESS.
- Test a real merge-group queue if B is retained as defense in depth.
- Test an in-flight direct and asynchronous merge separately.
- Never remove required checks or unlock a production branch as a test.
- Capture exact repository ID, refs, check run IDs, App IDs, ruleset IDs, bypass actors, timestamps, API responses and post-test readback.
- Independent reviewer must reproduce at least the critical denial paths.

---

## 10. Required GitHub permissions and configuration

### 10.1 Checker App

| Permission | Proposed level | Reason |
|---|---|---|
| Metadata | Read | Repository identity |
| Contents | Read | Exact source/blob/tree inspection |
| Pull requests | Read | PR/base/head state |
| Actions | Read | Run/job/attempt evidence |
| Checks | Write | Publish/update the check run |

No Contents write, Pull requests write or Administration write. The Checker App must not be able to merge or change its own required-check configuration.

The current 2A template additionally requests Administration read for inspection. Prefer a separate owner/admin inspection identity for privileged settings and bypass visibility rather than expanding the Checker App's trust.

### 10.2 Merge Broker App (future, not created)

| Permission | Proposed level | Reason |
|---|---|---|
| Metadata | Read | Repository identity |
| Contents | Write | GitHub merge API permission |
| Pull requests | Read/write | Bot-owned promotion PR lifecycle |
| Checks | Read | Inspect required check identity/result |
| Administration | None | Cannot weaken branch protection/rulesets |
| Actions | None unless separately justified | No need to execute workflows |

The broker must not have Checks write, policy signing keys or permission to edit enforcement. Its installation must be limited to the target repository.

A GitHub App with Contents write is powerful. The broker is therefore a critical trusted component and requires independent code review, pinned deployment, short-lived installation tokens, protected secrets, incident procedures and live adversarial tests.

### 10.3 Branch protection / rulesets

For an Organization-owned future target:

- Require PRs for protected destination branches.
- Require strict status checks.
- Bind the authority check to the exact Checker App ID.
- Require current review/approval policy and dismiss stale approvals where appropriate.
- Enforce rules for administrators.
- Restrict who can push to the Merge Broker App only, if eligible and live-proven.
- Block force pushes and deletions.
- Disable fork syncing for protected destinations.
- Keep bypass actors empty for enforcement rules.
- Keep any broker-only update restriction isolated from the required-check ruleset.
- Preserve all existing required checks.
- Protect every actual destination, including stacked bases during migration.
- Disable auto-merge and do not require a merge queue in the direct-broker mode.
- Audit repository, Organization and enterprise rulesets plus every effective branch rule.

The current personal-account repository does not have the documented eligibility for the App-only "Restrict who can push" branch protection setting. A repository transfer to an Organization is an owner decision, not an implementation detail.

### 10.4 Owner/admin trust

Even if branch protection applies to administrators and Organization rulesets prevent repository admins from editing inherited rules, Organization owners can change Organization rulesets. The account/enterprise owner remains a root of trust.

Required governance decisions:

- Who are the trusted Organization owners?
- Who may edit repository/Organization/enterprise rules?
- How are changes independently approved?
- Where are settings-change audit events retained?
- Who receives alerts for App permission, installation, bypass or ruleset changes?
- What is the incident escalation path if the owner/admin is unavailable?

No answer may claim GitHub prevents its ultimate owner from changing the controls.

---

## 11. Migration and implementation sequence

This is a future sequence only. It is not authorized by this design PR.

### Phase 0 — Owner decisions

1. Decide whether the repository may be transferred to an Organization.
2. Decide whether the product requires a useful merge path or accepts permanent-lock/no-admission.
3. Decide whether revocation is effective only after an in-flight transaction resolves, or whether the owner requires immediate cancellation. The latter is not guaranteed by GitHub.
4. Decide whether stacked PRs may be prohibited at admission. The first broker design should prohibit them.
5. Decide the trusted Organization owner/admin set and external policy-service ownership.
6. Decide whether the GitHub plan supports Organization-level rulesets and the required branch restrictions.

### Phase 1 — Independent design acceptance

1. Independently review this document.
2. Confirm every documented GitHub behavior against current official docs.
3. Verify the proposed App-only branch update path on a disposable Organization-owned repository.
4. Verify the checker App cannot merge.
5. Verify the broker cannot publish the Checker App's required check.
6. Verify admin/bypass and direct update paths.
7. Do not change Mind Place production settings.

### Phase 2 — Disposable implementation prototype

1. Create the separate policy repository and protected environment.
2. Create Checker App and Merge Broker App with the minimum permissions.
3. Implement the durable authority epoch, per-base lock and append-only transaction journal.
4. Implement source/base/tree/fingerprint/approval binding.
5. Implement immutable bot-owned promotion PRs.
6. Implement synchronous direct merge only for unstacked PRs.
7. Treat asynchronous stacked merges as unsupported until a separate decision.
8. Implement idempotent webhook handling, API readback and INDETERMINATE recovery.
9. Keep the actual Mind Place repository unchanged.

### Phase 3 — Adversarial qualification

1. Run the full matrix in Section 9 on the disposable repository.
2. Include same-SHA revoked approvals, old SUCCESS, App suspension and API outage.
3. Test concurrent merge/revoke and lost merge responses.
4. Test every direct, UI, REST, GraphQL, auto-merge and queue path.
5. Obtain independent security review of implementation and live evidence.
6. Reject any unproven branch restriction or bypass assumption.

### Phase 4 — Separate production authorization

Only after a new explicit owner authorization:

1. Establish permanent protection on every real destination.
2. Independently read back effective rules and bypass actors.
3. Demonstrate an otherwise-green unauthorized merge is denied.
4. Deploy the independently reviewed Checker App.
5. Deploy the broker only after its sole-writer control is proven.
6. Keep temporary unlock prohibited.
7. Revalidate all actual source, base, merge-tree and native proof identities.
8. Start with a controlled canary destination, not PR #132 or the production main branch.
9. Keep P0 open until the owner and independent reviewer accept the residuals.

No step in this sequence is executed by Checkpoint 2B design work.

---

## 12. Explicit unresolved risks and owner decisions

### 12.1 P0 risks

1. **No GitHub-native live revocation:** check SUCCESS is not coupled to an external policy epoch.
2. **In-flight merge:** an accepted merge operation may complete after the controller/authority service becomes unavailable. GitHub does not document a cancellation primitive for a pending asynchronous merge.
3. **Base compare-and-swap absent:** merge APIs guard PR head SHA, not expected base SHA/effective merge tree.
4. **Ultimate owner bypass:** GitHub cannot prevent the resource owner/Organization owner/enterprise owner from changing enforcement.
5. **Broker compromise:** the Merge Broker App has Contents write and is therefore a high-value principal.
6. **Current repository configuration:** main and the current stacked base are reported unprotected; no active rulesets are visible in the current repository inventory.
7. **Current account eligibility:** the repository is personal-account-owned, so the preferred App-only branch-push restriction and merge queue require an ownership/plan decision.
8. **Stacked PRs:** the synchronous direct merge endpoint does not support stacked PRs; the asynchronous endpoint is required and introduces a pending operation with unresolved cancellation behavior.

### 12.2 P1 risks

- Webhook delay, duplication or ordering may cause stale internal state unless every admission performs authoritative reads.
- Check-run PATCH and readback may fail during incident response.
- Organization ruleset inheritance and sensitive bypass visibility require appropriate owner/admin access.
- Policy repository or broker deployment compromise can invalidate the independence assumption.
- Restricting all human write access to protected branches changes current maintainer workflows.
- App-only update restrictions and required-check enforcement need live proof on the selected GitHub plan.

### 12.3 Required owner decisions

| Decision | Why it matters | Safe default pending decision |
|---|---|---|
| Transfer repo to an Organization? | Required for the preferred App-only push restriction and merge queue eligibility | Do not activate C or B |
| Allow any merge admission? | Permanent lock prevents useful merges | Keep permanent-lock/no-admission |
| Accept revocation linearization after in-flight merge resolution? | Immediate cancellation is not guaranteed | Do not authorize broker merges |
| Prohibit stacked PR admission? | Async merge introduces pending/in-flight semantics | Reject stacked PRs |
| Who is the root settings authority? | Owners can change enforcement | Require explicit trusted-owner list and independent audit |
| Accept a privileged Merge Broker App? | Broker compromise can cause merges | Keep Checker App read-only for merge purposes |
| Use merge queue for integration? | Queue improves code freshness but not revocation | Do not treat queue as authorization |
| Formal 2A acceptance record reconciled? | Issue #133's latest visible comment conflicts with the execution brief | Keep c861 as the owner-designated baseline; do not infer production acceptance |

---

## 13. Independent-review acceptance criteria

The independent reviewer must:

1. Verify the exact baseline commit and all repository observations.
2. Independently read every relevant official GitHub reference, especially seven-day status freshness, App-bound required checks, check-run update semantics, merge-group behavior, bypass actors, App suspension and merge API preconditions.
3. Confirm the distinction between a historical SUCCESS and a SUCCESS that still satisfies required checks.
4. Confirm that App ID binding does not prove current policy validity.
5. Challenge the claim that a merge queue handles base/head freshness but not post-success revocation.
6. Verify the current repository's personal-account ownership and merge-queue eligibility.
7. Verify current branch protection/ruleset evidence and clearly retain any inaccessible privileged settings as NOT VERIFIED.
8. Review the App-mediated design for single-writer enforcement, base-SHA TOCTOU, PR retargeting, stacked-PR asynchronous behavior, ambiguous merge results and compromised-broker risk.
9. Reject any claim that GitHub provides an atomic external-approval-plus-merge transaction.
10. Confirm that no temporary unlock is proposed or authorized.
11. Confirm that recovery never replays an expired or revoked authorization.
12. State which mandatory invariants are fully guaranteed, conditionally mitigated or impossible under the selected trust model.
13. Provide a separate GO/NO-GO for architecture acceptance and for any future implementation.
14. Preserve production activation as NO-GO until owner authorization and live enforcement evidence exist.

### Required review verdict format

- Architecture document: ACCEPT / CHANGES REQUIRED
- PR-head check model: ACCEPT / REJECT
- Merge queue model: ACCEPT / REJECT
- App-mediated broker model: ACCEPT / REJECT / CONDITIONAL
- Permanent-lock containment: ACCEPT / REJECT
- Unmet invariants:
- Unsupported GitHub guarantees:
- Owner decisions required:
- Live evidence required before implementation:
- Production activation: NO-GO unless separately authorized

---

## 14. Evidence classification

| Classification | Meaning | Examples in this document |
|---|---|---|
| **Documented platform behavior** | Explicitly stated by current GitHub documentation | Seven-day required-check freshness; expected App source; merge-group construction; merge API expected head SHA; App suspension blocks access |
| **Repository-verified behavior** | Read from the repository/API at the supplied exact HEAD | Personal-account owner; branch summaries report unprotected; empty ruleset inventory; no merge_group triggers; disabled example config |
| **Proposed behavior** | Design requiring implementation and independent review | Checker/Broker split; per-base lock; authority epoch; bot-owned promotion PR; one-use grant |
| **Unverified assumption** | Requires live test or privileged readback | App-only branch restriction works with the selected installation; every alternate merge path is denied; ruleset visibility is complete; in-flight merge behavior under outage |
| **Explicit limitation** | No supported GitHub primitive establishes the property | Atomic external revocation plus merge; expected-base-SHA compare-and-swap; prevention of ultimate owner configuration changes |

---

## 15. Final decision

**Checkpoint 2B is an architecture-design checkpoint, not production enforcement.**

The 2A read-only inspector is useful evidence collection, but it is not a merge gate. Required App checks are useful identity-bound evidence, but they are not live authority. A merge queue improves current merge-group validation, but it does not revoke an already successful check. App-mediated merging is the strongest useful-admission candidate, but GitHub does not provide the atomic base/authority transaction needed to claim every requested invariant without residuals.

Until the owner accepts a bounded broker model and its explicit in-flight/root-admin limitations, the security-correct operating mode is **permanent-lock / no-admission**.

**P0 stale-success enforcement remains OPEN. Production activation remains NO-GO. No implementation or privileged GitHub action is authorized by this document.**

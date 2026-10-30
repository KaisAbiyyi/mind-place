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

A completed check-run SUCCESS is not a live authorization. It can continue satisfying a required-check rule while the check remains eligible under GitHub's **seven-day freshness rule**, even after:

- the controller's internal 600-second authority lease expires;
- the approved policy fingerprint is revoked;
- the policy revision changes;
- the GitHub App is suspended or uninstalled;
- the controller or authority service becomes unavailable.

This is a merge-eligibility window, not a promise that the check record is retained indefinitely. Since **October 1, 2026**, GitHub check suites, check runs and commit statuses are subject to the configured Actions check/workflow/status retention period. The documented default is 90 days; public repositories can configure 1–90 days. The retention value configured for this repository has not been verified. Retention controls data visibility/deletion, not authority validity.

GitHub's required-check freshness rule requires a successful check within the previous **seven days**. The existing 600-second lease is internal to the controller and is not consumed by GitHub. A check younger than seven days can therefore remain eligible after that shorter lease or external approval expires.

Therefore:

1. **A — PR-head checks:** necessary evidence, insufficient as merge authorization.
2. **B — mandatory merge queue:** materially improves source/base/queue-tree freshness, insufficient for live policy revocation after a successful merge-group check.
3. **C — App-mediated merge authorization:** the strongest useful-admission candidate only if the Merge Broker is trusted and its sole-writer ruleset is live-proven. A compromised broker or stolen broker token can invoke the merge API against an otherwise-green required check without consulting the external authority service. GitHub does not independently enforce that service.
4. **Current containment:** permanent-lock / no-admission is a recommended operating policy, **not an established GitHub control**. Main and the stacked base are currently reported unprotected and no active rulesets were returned. Containment is **NOT VERIFIED**.
5. **Checkpoint 2B design verdict:** GO for independent review of this corrective document only. NO-GO for implementation, App installation, settings changes, production activation or merging.

### 1.2 What is and is not guaranteed

| Property | GitHub-native guarantee? | Finding |
|---|---|---|
| Required check came from a selected GitHub App | Yes, when the expected source is configured and enforced | Binds producer identity, not current authority |
| Required check applies to latest commit SHA | Yes, subject to required-check configuration | Does not bind a mutable policy revision to the same SHA |
| Required check must be recent | Yes: completed successfully within the past seven days | Merge eligibility, not retention or a 600-second lease |
| Check-data retention | Yes: check suites, check runs and statuses follow configured Actions retention from October 1, 2026 | Default 90 days; public repositories 1–90 days; this repository's configured value is unverified |
| Internal authority lease | No GitHub-native enforcement | The 600-second lease is controller state; GitHub does not consume it |
| Current permanent lock | No | Main and stacked base report unprotected; ruleset inventory is empty; containment is NOT VERIFIED |
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

The current branch-list API read also reports `protected=false` for both `main` and `codex/mp-ci-100k-01-20261027`. The repository ruleset inventory with `includes_parents=true` returned an empty array. The explicit classic branch-protection endpoint for `main` returned HTTP 403 to the connected inspection integration, so privileged protection settings were not independently readable through that endpoint. The public branch-list evidence still reports the two destination branches as unprotected.

**Containment status: NOT VERIFIED / NOT ESTABLISHED.** No live permanent lock has been established on `main` or the stacked base. The recommendation for permanent-lock/no-admission elsewhere in this document is an operating-policy recommendation, not a statement that GitHub currently enforces a lock. The current GitHub evidence shows no active repository rulesets and reports both relevant branches unprotected.

Containment may only be reported as established after an authorized settings operator independently reads back the effective classic branch protection and every applicable repository/parent ruleset, including all bypass actors, and successful negative merge/update probes demonstrate that otherwise-green unauthorized updates are denied across supported paths. A branch-list summary or a synthetic test alone is insufficient. Until then, current containment remains **NOT VERIFIED** and production enforcement remains NO-GO.

Because the repository is currently user-owned, Organization-level inherited rulesets are not currently in scope. If the repository is transferred to an Organization or enterprise, inherited Organization/enterprise rulesets become a new mandatory inspection gate. The empty repository ruleset inventory is not a substitute for future owner/admin inspection of every applicable scope.

### 2.3 Existing workflows and authority template

At the supplied HEAD:

- `.github/workflows/ci.yml` triggers on `push` to `main`, `pull_request`, and manual dispatch. It has no `merge_group` trigger.
- `.github/workflows/windows-product.yml` triggers on `pull_request`, `push` to `main`, and manual dispatch. It has no `merge_group` trigger.
- The Windows Product workflow's `Windows Product / candidate result` job is deliberately successful for Draft PRs while its native verification job is skipped and its summary says qualification is PENDING. This is a development-status UX signal, not a native-acceptance or authority check.
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

GitHub also treats `success`, `skipped` and `neutral` as successful required-check states. A required authority context must therefore never be a job that can be skipped or return neutral while validation is pending, draft-only or unavailable. The current Draft-success candidate jobs described in Section 2.3 are not authority signals and must not be selected as the sole required authority check. Use a distinct, uniquely named Checker App check whose only SUCCESS path is complete validation; unknown/pending/unavailable state must remain non-successful. Avoid duplicate check/status contexts with the same name.

**Keep four different clocks and states separate:**

1. **Configured check-data retention:** From October 1, 2026, GitHub retains check suites, check runs and commit statuses according to the repository/Organization/enterprise Actions retention setting. GitHub documents a 90-day default and a 1–90-day range for public repositories. A custom value is subject to higher-level caps and applies to new records; changing it does not retroactively rewrite existing records. The current Mind Place setting has not been verified. Check data from third-party Apps is included.
2. **Seven-day required-check freshness:** a required status check must have completed successfully in the chosen repository within the previous seven days. This determines whether the check can satisfy the required-check rule; it does not determine how long the record remains stored.
3. **Internal 600-second authority lease:** this is controller/authority-service state. GitHub branch protection and rulesets do not read or enforce it.
4. **Historical visibility versus merge eligibility:** a check record may remain visible after it is no longer eligible to satisfy the seven-day requirement, until configured retention deletes it. After retention, the check data is permanently deleted. Neither historical visibility nor deletion is a revocation protocol. Do not claim that SUCCESS remains visible indefinitely or that retention itself revokes a merge authorization.

References:
- [GitHub changelog: Actions retention now covers checks, runs and statuses (October 1, 2026)](https://github.blog/changelog/2026-10-01-actions-retention-now-covers-checks-runs-and-statuses/)
- [GitHub check-data retention documentation](https://docs.github.com/en/rest/guides/using-the-rest-api-to-interact-with-checks)
- [Required status-check freshness](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/troubleshooting-required-status-checks)

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

**Documented platform behavior:**

- **Classic branch protection has an administrator exception.** By default, branch-protection restrictions do not apply to repository administrators or custom roles with the bypass-branch-protections permission. The "Do not allow bypassing the above settings" option can apply branch-protection restrictions to those actors. However, GitHub's specific documentation for **Restrict who can push to matching branches** states that people and Apps with repository admin permissions are always able to push to a protected branch. Do not claim that classic branch protection's push restriction makes the Merge Broker the sole writer, and do not assume the general bypass checkbox removes this explicit push exception.
- **Repository-level branch rulesets are available on public personal repositories.** GitHub documents rulesets for public repositories on GitHub Free (and higher eligible plans). The feature is not restricted to Organization-owned repositories.
- **Restrict updates** means only users with bypass permission can push to the matching branch/tag.
- **GitHub App bypass actors are supported.** The repository-ruleset REST schema supports actor type `Integration` with an actor ID. It supports bypass mode `pull_request` for branch rulesets; that mode means the actor can bypass the rules only through a pull request. The schema says `OrganizationAdmin`, `EnterpriseOwner` and `EnterpriseRole` are not applicable to personal repositories; it does not exclude the `Integration` actor.
- **Ruleset bypass is per ruleset.** Applicable rulesets are aggregated and all applicable rules are enforced. A bypass actor configured on one ruleset must not be assumed to bypass a different ruleset.
- **Ruleset administration is privileged.** People with repository admin access or a custom role with `edit repository rules` can create, edit and delete repository rulesets. A repository owner/admin may therefore change, disable or remove the control even if that person is not a bypass actor.
- Organization-level and enterprise-level rulesets are additional policy scopes only when the repository belongs to those scopes. A future transfer to an Organization introduces inherited rulesets that must be inspected; it is not a prerequisite for a repository-level ruleset on the current public personal repository.

**Security consequence and preferred repository-level candidate:**

The classic "Restrict who can push" setting is not a single-writer control because administrators can still push. The stronger candidate for this repository is a **repository-level branch ruleset using Restrict updates**, with the Merge Broker App as the only bypass actor in pull-request-only mode. This is documented as feature-eligible for a public personal-account repository; actual merge-path behavior remains unverified until live disposable-repository testing.

Use separate active rulesets for distinct authority domains:

1. **Update-restriction ruleset:** target every protected destination branch; include the Restrict updates rule; configure exactly one bypass actor: the Merge Broker GitHub App (`actor_type=Integration`, the App's integration ID, `bypass_mode=pull_request`). Do not include a user, repository role, team, other App, Dependabot, deploy key or other bypass actor. Do not put required-check or deletion rules into this ruleset, because the Broker's bypass applies to every rule contained in this same ruleset.
2. **Required-check ruleset:** target the same destination branches; require pull requests and the exact Checker App required status/check context; use strict/latest-base policy. Configure an empty bypass-actor list. The Broker App is not a bypass actor for this ruleset, so its update-rule bypass must not exempt the required checks.
3. **Branch-integrity ruleset:** where needed, keep deletion and non-fast-forward/force-push restrictions in a separate active ruleset with no bypass actors. Do not place these rules in the Broker-bypass ruleset.

The update ruleset should not allow fork fetch-and-merge updates. Target all actual destination refs, including `main`, the stacked base and any future protected base. No temporary unlock or protection-removal window is part of this design.

**Actor authority under the proposed configuration (not yet live-proven):**

| Actor | Update/merge destination | Bypass authority | Modify enforcement |
|---|---|---|---|
| PR author or ordinary contributor | May create/update a source branch and open a PR; cannot update/merge the protected destination | None | No, unless separately granted repository admin or edit-repository-rules permission |
| Human reviewer / ordinary write collaborator | May review and approve; cannot merge or update the protected destination | None | No, unless separately granted privileged repository role |
| Checker App | Publishes the required check; no Contents write and no target update | None | No |
| Merge Broker App | Sole intended actor to merge a PR into a protected destination; direct push must remain denied | Only the update-restriction ruleset, only through a PR | No Administration write; no ruleset-edit permission |
| Other Apps, Actions tokens, bots and Dependabot | Cannot update/merge protected destination | None unless explicitly added (which activation must reject) | No, absent privileged repository role |
| Repository administrator / custom edit-rules role | Must be denied direct update/merge while the active ruleset remains unchanged; this denial requires live proof | None by default in the proposed list | Can create/edit/delete repository rulesets and can therefore weaken or remove enforcement |
| Personal repository owner | Same ruleset behavior as other non-bypass actors while it remains active; owner remains the ultimate configuration authority | None unless explicitly configured | Can change repository settings, rulesets and App installation; root of trust |

**Critical live-proof requirement:** the GitHub documentation defines the ruleset and bypass-mode primitives, but it does not replace an end-to-end test proving that a merge request authenticated as the Merge Broker App is recognized as the sole pull-request bypass actor, that a direct push using the same App token is rejected, and that human/admin/API/GraphQL/UI/CLI/auto-merge paths are denied while the ruleset remains active. The exact App identity, branch patterns, bypass list and effective rules must be read back with sufficient permission. GitHub's ruleset API omits `bypass_actors` from a detail response when the caller lacks write access to that ruleset; a missing field must not be interpreted as an empty list.

**Personal-account feasibility verdict:** a repository-level `Restrict updates` ruleset with an `Integration` actor and `pull_request` bypass is a documented candidate on the existing public personal-account repository. **Organization transfer is not mandatory for this ruleset-based single-writer experiment.** Transfer is a separate owner decision if Organization-level rulesets, Organization governance or merge-queue eligibility are wanted. The classic branch-protection push restriction remains Organization/plan-gated and is not the preferred sole-writer mechanism.

No GitHub configuration can prevent the ultimate personal-account owner from changing repository settings, removing the ruleset or revoking the App. That owner remains a root of trust.

References:
- [About protected branches — administrator exception and push restriction](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches)
- [About rulesets — availability and administration](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/about-rulesets)
- [Available rules for rulesets — Restrict updates](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/available-rules-for-rulesets)
- [REST API endpoints for repository rules — Integration and pull_request bypass mode](https://docs.github.com/en/enterprise-cloud@latest/rest/repos/rules)

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
- **Merge Broker App:** performs the intended merge path and has Contents write authority. It cannot publish the Checker App's required check or change repository settings.
- **External authority service:** stores current policy epoch, approved fingerprint, approval identity, revocation state and a durable per-base merge journal.
- **Protected policy repository:** owns the controller and policy configuration separately from the target repository.

**The Merge Broker is part of the Trusted Computing Base (TCB).** This is not merely an orchestration detail. A compromised Broker process or stolen Broker installation token can invoke GitHub's merge API using its Contents-write authority. If the target PR already has a required Checker App check that GitHub still considers successful, the attacker can attempt to merge it without consulting the external authority service. The required-check ruleset only verifies that the required check is passing; it does not ask the external authority service whether the approval/fingerprint is still current. The update-restriction ruleset recognizes the Broker's App identity, not whether the caller is the genuine Broker process.

GitHub does not independently enforce the external authority check against a compromised Broker. Splitting Checker and Broker Apps prevents the Checker from merging, but it does not make the Broker compromise-resistant. A stolen Broker token is equivalent to compromise of the Broker's GitHub authority for the duration of that credential's validity.

**Distinguish two failure classes:**

- **Controller/authority outage with an intact Broker:** the honest Broker must fail closed and refuse a new merge if it cannot obtain current authority. An already-sent GitHub merge can still complete, as documented elsewhere.
- **Broker compromise or stolen Broker token:** the attacker can skip the Broker's external-authority logic and call GitHub directly. An existing green required check may be enough for GitHub to accept the merge. Controller availability, lease expiry and policy revocation do not protect against this attacker.

Accordingly, invariant #2 (stale SUCCESS cannot independently authorize a merge) is **conditional on Broker integrity and exclusive custody of its installation credentials**. If the threat model includes a compromised Broker or stolen Broker token, this useful-merge candidate does not satisfy invariant #2: stale SUCCESS can be exploited through the compromised TCB. This residual is acceptable only if the owner explicitly accepts the Broker as a trusted principal and accepts the residual risk. If Broker compromise must be resisted, do not authorize this useful-merge architecture; retain no-admission or require a different independently enforced mechanism that has not been established in this design.

The honest Broker must not treat check SUCCESS as authorization. It must independently query the current authority service and validate the complete admission tuple immediately before requesting a merge.

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
9. Use synchronous `PUT /pulls/{number}/merge` with `sha=<expected PR head SHA>`. This endpoint does not expose `bypass_rules`; do not send it. It relies on normal GitHub rules enforcement and the configured actor-specific ruleset bypass behavior: the Broker's PR-only bypass applies to Ruleset A, while Rulesets B and C remain independently enforced. This behavior must be live-proven before the initial unstacked prototype is accepted. The broker must not unlock the branch or disable any protection.
10. Read back the merge result, resulting base SHA and PR state. Record the result in an append-only journal before releasing the lock.

**Required GitHub-side single-writer posture**

The preferred candidate is the repository-level ruleset design in Section 4.6; it does **not** require an Organization migration for this public personal-account repository.

- Use an active update-only branch ruleset with Restrict updates and exactly one Integration bypass actor: the Merge Broker App in pull-request-only mode.
- Use a separate active required-check ruleset with no bypass actors; require PRs and the exact Checker App check.
- Keep deletion/force-push protection in a separate no-bypass integrity ruleset.
- Do not use classic branch protection's "Restrict who can push" as the single-writer primitive: GitHub explicitly says repository admins and Apps with admin permissions can always push under that setting.
- No human, team, role, unrelated App, Actions token or Dependabot bypass actor may be configured.
- No auto-merge or merge-queue path may become an alternate merge authority in the direct-broker mode.
- Limit repository write access; keep the bot-owned promotion PR immutable/retarget-resistant to ordinary contributors.
- Keep repository settings administration separate from the Broker App.

The documented plan/API eligibility supports this repository-level ruleset candidate on a public personal repository. That is not proof that the App merge identity is enforced as intended. Live disposable-repository proof remains mandatory before calling it single-writer.

**Conditional guarantees if correctly implemented and live-proven**

- While the Broker is intact and is the only enforced updater, stale SUCCESS is not sufficient by itself: the honest Broker must revalidate live authority. This guarantee is conditional on Broker integrity and token custody; it does not survive Broker compromise.
- Controller/authority outage before a new authorization causes an intact Broker to refuse new merge requests. This is outage protection, not compromise protection.
- A source-head change is guarded by the merge API's expected-head SHA.
- Base updates by competing writers can be prevented only if the repository-level update ruleset continuously recognizes the Broker as the sole PR bypass actor and live tests prove all other update paths are denied; the Broker must serialize its own merges.
- Approval revocation and merge requests can be serialized by one durable authority transaction only while the Broker and authority service remain trustworthy.
- Recovery can require a new policy epoch and new authorization instead of reusing old SUCCESS.

**Failure modes and hard limitations**

1. GitHub does not atomically commit the external authority transaction with the branch update.
2. The merge API's expected SHA is the PR head SHA, not the base SHA or effective merge tree.
3. The PR base can be edited through PR APIs by sufficiently privileged actors. A bot-owned promotion PR and tightly restricted repository write access reduce this race, but repository admins/owners remain trusted.
4. A merge request already sent to GitHub may complete even if the controller or authority service becomes unavailable immediately afterwards.
5. If the merge request response is lost, the broker cannot assume failure. It must mark the transaction INDETERMINATE and stop all further merges until it reads back the PR and base state.
6. The asynchronous merge endpoint required for stacked PRs returns a pending request UUID. No documented cancellation endpoint establishes that a pending direct merge can be revoked. The initial broker must therefore reject stacked PRs rather than claim atomic revocation.
7. A compromised Merge Broker App or stolen Contents-write installation token can invoke the merge API without consulting the external authority service. If an existing required Checker App check is still green and eligible, GitHub may accept the merge. The Broker is therefore TCB; this architecture does not resist Broker compromise and invariant #2 is conditional on Broker integrity.
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

**Can stale SUCCESS independently authorize a merge?** Not while the Broker is intact, the repository-level update ruleset is active and live-proven, the Broker is the sole PR bypass actor, and the Broker revalidates live authority. **If the Broker is compromised or its installation token is stolen, stale SUCCESS can be exploited** because GitHub does not enforce the external authority query. Invariant #2 is therefore conditional on Broker integrity.
**Can GitHub alone guarantee immediate revocation across an already in-flight merge? No.** The broker design has a transaction boundary and residual root-of-trust assumptions. P0 must remain open until the owner explicitly accepts the Broker TCB risk and stated revocation semantics and independent live tests prove the single-writer path.

---

## 6. Recommended architecture and rationale

### 6.1 Immediate recommendation: permanent-lock / no-admission (policy recommendation only)

Until the owner explicitly accepts the residuals of a useful merge broker, the recommended operating policy is no merge admission:

- Do not merge protected destinations, even if existing checks are green.
- Do not use direct merge, auto-merge, merge-queue admission or human exceptions.
- Do not temporarily unlock branches, remove required checks or remove/update enforcement rules to recover availability.
- Treat a historical SUCCESS as evidence only; it is not current authority.
- If a lock cannot be independently read back, report containment as NOT VERIFIED and halt merges/releases operationally.
- Do not claim that a controller restart, App key rotation or check-run PATCH alone restores enforcement.

**This policy is not currently enforced by GitHub.** At the latest repository read, main and the stacked base were reported unprotected and the ruleset inventory was empty. There is no live permanent lock established. The current state is therefore **NOT VERIFIED**, not "contained".

To claim technical containment in a future separately authorized activation, an owner/settings operator must establish an active deny-all update/lock control on every destination, independently read back its effective settings and bypass list, and pass negative merge/update probes with an otherwise-green PR through UI, REST, GraphQL, Git push, CLI, auto-merge and any enabled queue path. No such configuration or probe is performed by this documentation-only checkpoint.

### 6.2 Future useful-merge candidate: C with an external serialized authority

If the owner requires normal merges, C is the preferred architecture to prototype after separate approval:

1. Prefer a repository-level Rulesets Restrict updates candidate for the existing public personal-account repository, with the Broker Integration as the sole `pull_request` bypass actor. Organization transfer is optional for Organization-level governance, inherited rulesets or merge-queue eligibility; it is not required for this candidate. Classic branch protection's push restriction is not the preferred single-writer mechanism because of its administrator exception.
2. Establish a separate policy repository and independent authority service.
3. Split Checker App and Merge Broker App.
4. Make the broker the only ordinary permitted updater through PR-only branch restrictions; no temporary unlock.
5. Do not configure repository administrators as bypass actors; while the intended ruleset remains active, they should be denied destination updates, subject to live proof. Administrators can modify or delete repository rulesets, and the account owner remains the root of trust. Keep the separate required-check and integrity rulesets enforced.
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

**Current-state warning:** the permanent-lock/no-admission policy is not currently enforced in GitHub. Main and the stacked base are reported unprotected, no active repository rulesets were returned, and no negative merge probe has established containment. The current containment state is **NOT VERIFIED**.

| # | Required invariant | Permanent-lock / no-admission | App-mediated broker candidate | Platform limitation / qualification |
|---:|---|---|---|---|
| 1 | Fail closed when controller or authority is unavailable | Policy says deny, but GitHub currently has no verified lock enforcing it | Conditional for new authorization while the Broker is intact; an already-sent/in-flight GitHub merge may complete | Immediate cancellation of an accepted merge is not guaranteed |
| 2 | Stale SUCCESS cannot independently authorize a merge | Only if a deny-all lock is separately established and live-proven; not true of the current repository state | **Conditional on Broker integrity, exclusive token custody and the live-proven sole-writer ruleset. A compromised Broker/stolen token can exploit an existing green check without querying authority.** | GitHub does not enforce the external authority query against the Broker; A/B alone do not satisfy this invariant |
| 3 | Revoked approvals/fingerprints cannot authorize future merges | Only if all admission paths are technically denied; current containment is not verified | Conditional on an intact Broker, serialized in-flight work being resolved and the epoch being incremented | Immediate revocation of an already accepted merge is not guaranteeable |
| 4 | Exact source SHA, base SHA, policy fingerprint and authorization identity remain bound | No admission is issued only after a real deny-all control is established | Conditional via signed tuple, bot-owned PR, live reads and durable journal | Merge API has expected head SHA, not expected base SHA/tree SHA |
| 5 | PR updates during verification invalidate obsolete authorization | No admission is issued only after a real deny-all control is established | Conditional via expected head SHA and re-read; bot-owned PR limits ordinary retargeting | No atomic base/PR-metadata compare-and-swap exists |
| 6 | No administrator or unintended actor silently bypasses enforcement | Not established; account owner can change/remove any repository-level lock | Conditional on the exact bypass list, effective rules, admin denial probes and separate configuration governance | Repository admins/custom edit-rules roles can modify/delete repo rulesets; owner remains root of trust |
| 7 | Inherited Organization rulesets are accounted for | Current repository is personal-account-owned; no Organization parent was returned | Same; if transferred later, every repo/Organization/enterprise ruleset and effective branch rule must be re-read | Organization transfer is not required for repository-level rulesets; it is a migration gate only for Organization-level policy/queue needs |
| 8 | No temporary unlock or protection-removal window | Policy forbids it, but no live lock is currently established | Intended by design; the update ruleset remains continuously active and no temporary unlock is permitted | Requires live proof that PR merge works without disabling/removing protection |
| 9 | No race-prone transition between verification and merge authorization | No merge is admitted only after a real deny-all control is established | Conditional mitigation through single writer, lock and expected-head SHA | GitHub has no atomic external-authority + expected-base merge transaction; in-flight ambiguity remains |
| 10 | Recovery never restores expired or revoked authorization | No authorization is restored by policy; technical containment remains unverified | Conditional through epoch bump, journal reconciliation and fresh approval only | Must be implemented and tested; GitHub does not perform this external recovery logic |

**Result:** no useful-merge architecture can currently be certified as satisfying all ten invariants against a compromised Broker. The repository-level ruleset candidate may enforce a single App updater against ordinary/human actors, but only after live proof. Permanent-lock/no-admission remains the safest recommended policy, not an established current GitHub state. P0 remains OPEN.

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

Synthetic tests are necessary for state-machine and parser correctness, but they do not prove GitHub enforcement. Before production authorization, demonstrate the repository-level Restrict updates and Integration/pull_request bypass allow/deny paths on a disposable public personal-account repository. Use a separate disposable Organization-owned repository only for Organization inheritance and merge-queue qualification. No test may temporarily unlock a production branch.

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
| 14 | Broker App token stolen or Broker process compromised while an otherwise-green required Checker App check exists and authority is revoked/unavailable | **Unauthorized merge may succeed.** The attacker can call the merge API as the Broker App and bypass the update-only ruleset through the PR path; GitHub does not independently require the external authority query. This is an expected TCB-compromise residual, not a denial guarantee. | Disposable personal-repository test using a deliberately controlled test Broker token; record whether GitHub accepts the merge despite the revoked authority state |
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
| 37 | Repository-level Restrict updates with sole Integration bypass actor in pull_request mode on a public personal-account repository | Broker-authenticated PR merge is accepted; direct push by the same App token and human/admin merge/update attempts are denied, if the documented candidate works as intended | Disposable public personal-account repository, exact ruleset readback and negative/positive API/UI probes |
| 38 | Broker is bypass actor on update-only ruleset but absent from required-check and branch-integrity rulesets | Broker can bypass only the update rule; required Checker App check and no-bypass integrity rules remain enforced | Three independent ruleset detail readbacks plus attempts to merge with missing/failing required check and to delete/force-push |
| 39 | Repository administrator attempts direct push while update ruleset is active | Denied while ruleset remains active; administrator must not be treated as an automatic bypass actor; administrator can still edit/delete the ruleset | Disposable repo using owner/admin identity; direct push/PR merge denial plus ruleset-edit capability evidence |
| 40 | Classic branch protection Restrict who can push used as sole-writer control | Admin exception remains; do not accept as sole-writer proof | Disposable repo demonstrates documented administrator push exception |
| 41 | Human UI, REST, GraphQL, CLI, Actions token, auto-merge or queue attempts target update while only Broker App is configured for pull_request bypass | All non-Broker update paths denied; any enabled alternate merge path is a blocker | Separate actor credentials and per-path negative receipts |
| 42 | Broker outage with intact code/credentials versus Broker compromise/token theft | Outage: fail closed before sending a merge. Compromise: attacker may merge against an existing eligible green check without authority. These are distinct outcomes. | Paired disposable-repository tests, with an explicit test-only Broker token for the compromise simulation |

### Required live test design

- Use a disposable **public personal-account repository** to test the repository-level Rulesets Restrict updates candidate with an Integration actor; Organization transfer is not a prerequisite for this test.
- Use a separate disposable Organization-owned repository only for Organization-level inheritance or merge-queue tests.
- Use an otherwise-green PR and an App-bound Checker check.
- Exercise source-branch writes, PR creation, human merge, admin merge, Broker PR merge and Broker direct push using separate identities.
- Confirm the Broker App can merge through a PR in pull_request bypass mode but cannot direct-push to the protected ref.
- Confirm the Broker's bypass on the update-only ruleset does not bypass the separate required-check or branch-integrity rulesets.
- Include a controller/authority outage after a successful check, not only before it.
- Include a revoked fingerprint with the same source SHA and an App suspension with an existing SUCCESS.
- Simulate Broker compromise with a deliberately controlled test installation token. The expected result is that an unauthorized merge **may succeed** if an eligible green required check exists; record this as the TCB residual, not a test failure.
- Test repository-admin direct update denial and separately verify that the admin can alter/delete repository rulesets. Test the classic branch-protection administrator exception.
- Test a real merge-group queue if B is retained as defense in depth.
- Test an in-flight direct and asynchronous merge separately.
- Never remove required checks or unlock a production branch as a test.
- Capture exact repository ID, refs, check run IDs, App IDs, ruleset IDs, bypass actors, timestamps, API responses and post-test readback.
- Independent reviewer must reproduce the critical allow/deny paths and the Broker-compromise residual.

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

For the current public personal-account repository, the preferred candidate is three repository-level branch rulesets, all active and targeting every protected destination ref:

**Ruleset A — Broker update restriction**

- Target `main`, the current stacked base and every future destination branch.
- Include the Restrict updates rule only.
- Configure exactly one bypass actor: the Merge Broker GitHub App as Integration, using its App/integration ID, with bypass mode pull_request.
- Do not include required checks, deletion or force-push rules in this ruleset; the Broker can bypass rules in the ruleset where it is listed.
- Disable update/fetch-and-merge from upstream forks.
- This is the only ruleset in which the Broker appears as a bypass actor.

**Ruleset B — Required PR and checks**

- Target the same destination refs.
- Require pull requests and strict/latest-base status checks.
- Require the unique Checker App context and expected App identity.
- Set the bypass-actor list to empty. The Broker must not be able to bypass this ruleset.
- Preserve all existing required checks; never replace them with a similarly named weaker context.

**Ruleset C — Branch integrity**

- Target the same destination refs.
- Keep deletion and force-push/non-fast-forward restrictions here.
- Set the bypass-actor list to empty.
- Do not give the Broker deletion or force-push bypass authority.

**Actor and settings authority**

- PR authors and ordinary collaborators may create/update source branches and open PRs. They cannot update or merge protected destinations while Ruleset A remains active and they are not bypass actors.
- Human reviewers may approve or request changes. Review permission does not grant destination-update or merge authority.
- The Checker App can publish the required check but has no Contents write and is not a bypass actor.
- The Merge Broker App is the only intended destination updater/merger, and only through a PR. Its bypass is limited to Ruleset A; Rulesets B and C remain enforced.
- Other Apps, Actions tokens and bots have no destination-update permission unless explicitly added as a bypass actor. Activation must reject any additional actor.
- Repository administrators and custom roles with edit repository rules are not to be configured as bypass actors. While the rulesets remain active, they must be denied destination updates in live probes; however, those privileged roles can edit/delete repository rulesets and can therefore weaken enforcement.
- The personal-account owner can alter repository settings, rulesets and App installation. This is the ultimate configuration root of trust and cannot be constrained by the repository's own ruleset.

Do not use classic branch protection's Restrict who can push as the single-writer primitive: GitHub explicitly documents that repository admins and Apps with admin permissions can always push under that setting. "Do not allow bypassing the above settings" does not erase the documented administrator exception for that push restriction.

Repository-level branch rulesets and the Integration/pull_request bypass actor are documented as available for public personal-account repositories; **no Organization transfer is required for this ruleset-based experiment**. Organization transfer is an optional future decision for Organization-level rulesets, governance or merge-queue eligibility. The documented semantics are not a live guarantee: the Broker PR merge, same-App direct-push denial, human/admin denial and separate-ruleset enforcement must all be proven on a disposable personal-account repository before single-writer enforcement is claimed.

The Broker App must have no Administration permission. Repository settings administration remains a separate owner/admin responsibility. No production settings change is authorized by this document.

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

1. Decide whether the owner explicitly accepts the Merge Broker as a Trusted Computing Base, including the possibility that a compromised Broker or stolen token can merge against an existing eligible green check. If not accepted, do not authorize a useful merge path.
2. Decide whether the product requires a useful merge path or accepts permanent-lock/no-admission.
3. Decide whether revocation is effective only after an in-flight transaction resolves, or whether the owner requires immediate cancellation. The latter is not guaranteed by GitHub.
4. Decide whether stacked PRs may be prohibited at admission. The first broker design should prohibit them.
5. Decide the trusted personal-account owner/admin set and external policy-service ownership.
6. Decide whether an Organization transfer is wanted for Organization-level rulesets/governance or merge queues. It is **not required** for the documented repository-level Rulesets Restrict updates candidate on this public personal repository.
7. Verify the current Actions check-data retention setting; its actual repository value has not been inspected.

### Phase 1 — Independent design acceptance

1. Independently review this document.
2. Confirm every documented GitHub behavior against current official docs.
3. Verify the repository-level Integration/pull_request update-restriction ruleset on a disposable public personal-account repository.
4. Verify that the Broker App can merge a PR but cannot direct-push with the same App token.
5. Verify the Checker App cannot merge and the Broker cannot publish the Checker App's required check.
6. Verify that the Broker bypass in the update-only ruleset does not bypass the separate required-check or integrity rulesets.
7. Verify human and repository-admin denial paths, and separately verify which admins can edit/delete the rulesets.
8. Verify the classic branch-protection administrator push exception.
9. Do not change Mind Place production settings.

### Phase 2 — Disposable implementation prototype

1. Create the separate policy repository and protected environment.
2. Create Checker App and Merge Broker App with the minimum permissions.
3. Implement the durable authority epoch, per-base lock and append-only transaction journal.
4. Implement source/base/tree/fingerprint/approval binding.
5. Implement immutable bot-owned promotion PRs.
6. Implement the three separate repository rulesets from Section 10.3 on the disposable public personal-account repository.
7. Implement synchronous direct merge only for unstacked PRs.
8. Treat asynchronous stacked merges as unsupported until a separate decision.
9. Implement idempotent webhook handling, API readback and INDETERMINATE recovery.
10. Keep the actual Mind Place repository unchanged.

### Phase 3 — Adversarial qualification

1. Run the full matrix in Section 9 on the disposable repository.
2. Include same-SHA revoked approvals, old SUCCESS, App suspension and API outage.
3. Test concurrent merge/revoke and lost merge responses.
4. Test human/admin/other-App denials, classic branch-protection admin exception and ruleset bypass scope.
5. Simulate Broker compromise/token theft and record the expected unauthorized-merge residual.
6. Test every direct, UI, REST, GraphQL, auto-merge and queue path.
7. Obtain independent security review of implementation and live evidence.
8. Reject any unproven branch restriction or bypass assumption.

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

1. **No GitHub-native live revocation:** check SUCCESS is not coupled to an external policy epoch; seven-day freshness is not a live authority lease.
2. **In-flight merge:** an accepted merge operation may complete after the controller/authority service becomes unavailable. GitHub does not document a cancellation primitive for a pending asynchronous merge.
3. **Base compare-and-swap absent:** merge APIs guard PR head SHA, not expected base SHA/effective merge tree.
4. **Broker is TCB:** a compromised Merge Broker or stolen Contents-write installation token can invoke the merge API against an otherwise-green required check without consulting the external authority service. GitHub does not independently enforce the external authority check. Invariant #2 is conditional on Broker integrity; if Broker compromise is in scope, the useful-merge candidate is NO-GO for that threat model.
5. **Single-writer is not yet proven:** the repository-level Restrict updates ruleset with a sole Integration/pull_request bypass actor is documented as eligible on this public personal repository, but the actual Broker merge identity and all denial paths require live disposable-repository proof.
6. **Classic branch protection is insufficient for sole writer:** its Restrict who can push setting explicitly allows repository admins/Apps with admin permissions to push.
7. **Ultimate owner/configuration authority:** repository admins or custom edit-repository-rules roles can change/delete repository rulesets; the personal-account owner can alter settings and App installation.
8. **Current containment is NOT VERIFIED:** main and the stacked base report protected=false, the repository ruleset inventory is empty, and no live permanent lock or negative merge probe has been established.
9. **Personal-account feature boundary:** repository-level rulesets do not require Organization ownership; merge queue and Organization-level rulesets remain separate features with their own eligibility.
10. **Stacked PRs:** the synchronous direct merge endpoint does not support stacked PRs; the asynchronous endpoint is required and introduces a pending operation with unresolved cancellation behavior.
11. **Check retention setting:** check records are now governed by configured Actions retention; the current repository's actual setting has not been verified.

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
| Transfer repo to an Organization? | Not required for repository-level Restrict updates rulesets; may be wanted for Organization-level rulesets, governance or merge queues | Do not transfer solely for single-writer enforcement |
| Accept Merge Broker as TCB? | A compromised Broker/stolen token can merge using an existing eligible green check without external authority validation | If not accepted, do not authorize useful merges |
| Allow any merge admission? | Current repository has no verified lock; permanent-lock/no-admission is policy only until separately enforced | Do not merge; containment remains NOT VERIFIED |
| Accept revocation linearization after in-flight merge resolution? | Immediate cancellation is not guaranteed | Do not authorize broker merges |
| Prohibit stacked PR admission? | Async merge introduces pending/in-flight semantics | Reject stacked PRs |
| Who is the root settings authority? | Admins can modify/delete repo rulesets and the owner can change settings/App installation | Require explicit trusted-owner/admin list and independent audit |
| Use merge queue for integration? | Queue improves code freshness but not revocation and has separate eligibility | Do not treat queue as authorization |
| Verify check-data retention setting? | Retention is configured and the current repository value is unknown | Do not claim indefinite historical visibility |
| Formal 2A acceptance record reconciled? | Issue #133's latest visible handoff still says not to treat the implementation/CI result as 2A acceptance, despite the execution brief's accepted characterization | Preserve the discrepancy; do not claim GitHub's tracking record is reconciled |

---

## 13. Independent-review acceptance criteria

The independent reviewer must:

1. Verify the exact baseline commit and all repository observations.
2. Independently read every relevant official GitHub reference, especially seven-day status freshness, configured check-data retention, App-bound required checks, check-run update semantics, merge-group behavior, bypass actors, App suspension and merge API preconditions.
3. Confirm the distinction between check-data retention, seven-day required-check freshness, the internal 600-second lease and historical visibility versus merge eligibility.
4. Confirm that App ID binding does not prove current policy validity.
5. Challenge the claim that a merge queue handles base/head freshness but not post-success revocation.
6. Verify that classic branch protection's Restrict who can push setting retains the documented administrator push exception.
7. Verify repository-level Rulesets Restrict updates availability for a public personal-account repository and the REST schema support for an Integration actor with pull_request bypass mode; do not require an Organization transfer for this candidate.
8. Verify the proposed separation between the Broker-only update-restriction ruleset and the no-bypass required-check/integrity rulesets.
9. Verify current branch protection/ruleset evidence and clearly retain any inaccessible privileged settings as NOT VERIFIED; confirm that current containment is NOT VERIFIED, not established.
10. Review the actor matrix: who can open PRs, approve, update, merge, bypass each ruleset, and edit/delete enforcement.
11. Reproduce the positive Broker PR-merge path and negative direct-push/human/admin/other-App paths on a disposable public personal-account repository.
12. Simulate Broker compromise/stolen token. The expected residual is that an attacker may merge against an eligible green required check without querying external authority; invariant #2 is conditional on Broker integrity.
13. Review the App-mediated design for base-SHA TOCTOU, PR retargeting, stacked-PR asynchronous behavior, ambiguous merge results and compromised-Broker risk.
14. Reject any claim that GitHub provides an atomic external-approval-plus-merge transaction.
15. Confirm that no temporary unlock is proposed or authorized.
16. Confirm that recovery never replays an expired or revoked authorization.
17. State which mandatory invariants are fully guaranteed, conditionally mitigated or impossible under the selected trust model, explicitly marking invariant #2 conditional on Broker integrity.
18. Provide a separate GO/NO-GO for architecture acceptance and for any future implementation.
19. Preserve production activation as NO-GO until owner authorization and live enforcement evidence exist.

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
| **Documented platform behavior** | Explicitly stated by current GitHub documentation | Seven-day required-check freshness; check-data retention from October 1, 2026; classic branch-protection administrator push exception; public personal-repository ruleset availability; Restrict updates and Integration/pull_request bypass semantics; merge-group construction; merge API expected head SHA |
| **Repository-verified behavior** | Read from the repository/API during this corrective revision | Personal-account owner; main and stacked base report protected=false; empty repository ruleset inventory; no live lock established; no merge_group triggers; disabled example config |
| **Proposed behavior** | Design requiring implementation and independent review | Checker/Broker split; separate update-only, required-check and branch-integrity rulesets; per-base lock; authority epoch; bot-owned promotion PR; one-use grant |
| **Unverified assumption** | Requires live test or privileged readback | Broker App is recognized as sole PR bypass actor; same-App direct push is rejected; humans/admins/other Apps are denied; bypass list is fully visible; all alternate merge paths are denied; in-flight merge behavior under outage |
| **Explicit limitation** | No supported GitHub primitive establishes the property | Atomic external revocation plus merge; expected-base-SHA compare-and-swap; GitHub independently enforcing external authority against a compromised Broker; prevention of ultimate owner configuration changes |

---

## 15. Final decision

**Checkpoint 2B is an architecture-design checkpoint, not production enforcement.**

The 2A read-only inspector is useful evidence collection, but it is not a merge gate. Required App checks are useful identity-bound evidence, but they are not live authority. A merge queue improves current merge-group validation, but it does not revoke an already successful check. App-mediated merging is the strongest useful-admission candidate, but GitHub does not provide the atomic base/authority transaction needed to claim every requested invariant without residuals.

Until the owner accepts the Broker TCB risk, in-flight limitations and root-owner assumptions, the recommended operating policy is **permanent-lock / no-admission**. This is not currently a live GitHub lock: main and the stacked base report unprotected, the ruleset inventory is empty, and containment remains **NOT VERIFIED** until effective settings readback and negative merge probes succeed.

The preferred future single-writer candidate is a repository-level Restrict updates branch ruleset on the existing public personal-account repository, with the Merge Broker Integration as the sole pull_request bypass actor and separate no-bypass required-check/integrity rulesets. Organization transfer is not required for that repository-level experiment. The candidate is not accepted as single-writer until live disposable-repository proof succeeds.

The Broker is a TCB. If Broker compromise is in scope, invariant #2 is not guaranteed and the useful-merge candidate is NO-GO for that threat model: GitHub may accept a merge using an existing eligible green required check without the external authority query.

**P0 stale-success enforcement remains OPEN. Checkpoint 2B is not accepted. Production activation remains NO-GO. No implementation or privileged GitHub action is authorized by this document.**

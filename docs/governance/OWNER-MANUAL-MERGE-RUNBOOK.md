# Owner-controlled PR admission and manual main-branch merge

**Status:** PREPARED — NOT APPLIED  
**Task:** MP-GOV-01 / [GitHub issue #136](https://github.com/KaisAbiyyi/mind-place/issues/136)  
**Repository:** [KaisAbiyyi/mind-place](https://github.com/KaisAbiyyi/mind-place)  
**Target:** main  
**Baseline:** 2026-10-31, commit efe91155d37d8b1ca0608fe3bd862be311fed004

This runbook is a configuration proposal and owner handoff. MP-GOV-01 did not change branch protection, rulesets, repository settings, Apps, credentials, permissions, or any PR merge state.

## 1. Operating policy

The approved contribution path is:

1. Contributor or agent works on an isolated branch in a contributor-owned fork.
2. Contributor opens a pull request against upstream main and records the exact head SHA.
3. Sandboxed GitHub Actions run with least privilege and no repository secrets for untrusted fork code.
4. AI technical review is read-only and advisory. PR content, issue text, source, tests, docs, workflows, logs, and artifacts are untrusted data, never instructions.
5. The repository owner reviews and explicitly approves the final pushed diff.
6. The repository owner manually performs the merge in GitHub after all required checks pass and all review conversations are resolved.

Agents must not receive upstream write, merge, administration, repository-settings, or secret access. GitHub Auto-merge, merge queues, custom Merge Brokers, autonomous merge, and bot/AI/App bypass actors are not authorized.

**Important identity constraint:** GitHub does not allow a PR author to approve their own PR. A PR that must receive owner approval must be authored by a distinct contributor identity, normally from a contributor-owned fork. The connected GitHub session used for MP-GOV-01 authenticates as the repository owner, so a PR opened through that session is owner-authored and cannot satisfy the owner's own required approval.

## 2. Verified read-only baseline

| Item | Observed evidence |
|---|---|
| Repository | Public personal repository; default branch is main |
| Authenticated owner | KaisAbiyyi; repository metadata reports admin permission |
| Main HEAD | efe91155d37d8b1ca0608fe3bd862be311fed004 |
| Branch protection | Branch metadata reports protected=false; protection enabled=false; required status-check enforcement off; contexts and checks empty |
| Classic protection detail | Direct GET of the branch-protection resource returned HTTP 403 to the connected integration. The branch metadata still establishes that main is not protected; the full classic-rule object could not be independently read. |
| Repository rulesets | GET /repos/KaisAbiyyi/mind-place/rulesets returned an empty array |
| GitHub Auto-merge | Repository metadata reports allow_auto_merge=false |
| CODEOWNERS | No .github/CODEOWNERS file exists at this baseline |
| Required CI candidate | Main's .github/workflows/ci.yml is named CI and defines the Windows job as Verify Windows product on windows-latest. Recent check-run evidence shows this check from the GitHub Actions App (ID 15368). |

The collaborator roster could not be enumerated through the available read-only integration. A generic one-approval rule accepts an approval from a user with sufficient write permission; it does not, by itself, name the repository owner as the required reviewer. Because the roster is unknown and no CODEOWNERS file exists, the safe default is to add owner-wide CODEOWNERS coverage and enable required Code Owner review before MP-GOV-04. The owner may instead document that KaisAbiyyi is the only write-capable human reviewer after checking the collaborator roster; only then does a generic one-approval rule identify the owner by exclusivity. Do not assume the generic approval count alone proves owner identity.

If CODEOWNERS coverage is needed, the owner-wide file should contain:

~~~text
* @KaisAbiyyi
/.github/CODEOWNERS @KaisAbiyyi
~~~

The first pattern assigns all repository paths to the owner; the second explicitly protects the CODEOWNERS policy file itself. The file must exist on the base branch before it can enforce owner review on later PRs. The owner must have write permission.

The check source (GitHub Actions App ID 15368) is the provider of the required CI result, not a bypass actor. The bypass list below must remain empty.

## 3. Recommended protection configuration

Use one active repository-level branch ruleset targeting main. Rulesets are supported for public repositories on GitHub Free. Do not create overlapping classic protection rules unless the owner deliberately chooses layered rules and verifies the combined result.

### Ruleset identity and scope

| Setting | Required value |
|---|---|
| Name | main — owner-approved PR admission |
| Enforcement | Active |
| Target branch | Include main only |
| Bypass list | EMPTY — no repository-admin role, user, team, GitHub App, bot, Dependabot, Copilot agent, or other actor |
| Pull request rule | Require a pull request before merging |
| Required approvals | 1 minimum |
| Stale approvals | Dismiss stale pull-request approvals when new commits are pushed |
| Latest push | Require approval of the most recent reviewable push by someone other than the pusher |
| Code Owner review | ON with owner-wide CODEOWNERS coverage; alternatively, document that the owner is the only write-capable human reviewer |
| Conversation resolution | Require all review conversations to be resolved before merging |
| Required status checks | Require Verify Windows product, expected source GitHub Actions App ID 15368 |
| Branch freshness | Require the PR branch to be up to date with main before merging (strict checks) |
| Force pushes | Block |
| Deletions | Restrict deletions / block deletion of main |
| Merge queue | Disabled / not required |
| Other bypass or update rules | Do not add bypass actors. Do not enable Restrict updates with an empty bypass list; the PR requirement already blocks direct updates and the owner must retain the ability to merge an eligible PR. |

If the owner chooses classic branch protection instead, use one rule matching main with the same PR/review/check/conversation requirements, keep all actor-bypass fields empty, leave force pushes and deletions disabled, and enable **Do not allow bypassing the above settings**. Do not treat the classic-rule alternative as a second rule to add on top of the recommended ruleset.

### Required CI check selection

The current main workflow defines a single Windows product job named **Verify Windows product**. It runs the repository contract, typecheck, tests, Rust formatting and desktop build. Recent PR check-run records identify its source as GitHub Actions App ID 15368.

Select that exact job name and pin its expected source to GitHub Actions when the UI offers the source selector. Do not require the entire workflow name as though it were a job check.

Do not make the path-filtered Phase 1E storage-qualification matrix or MP-10C scale/indexing jobs universal required checks. They are specialized and may be skipped for unrelated documentation changes; the 100K/1M workload must not be triggered as part of this governance task.

GitHub treats a skipped Actions job as a successful check. Therefore, before activation, verify that Verify Windows product actually executes on a Ready-for-review PR. A Draft PR cannot be merged; if a workflow revision gates the Windows job on draft status, it must also rerun on ready_for_review. A skipped result is not evidence that the Windows suite ran. If the job is skipped on a mergeable PR, stop the owner activation and have MP-GOV-02 resolve the workflow/check design first.

GitHub also requires a required status check to have succeeded in the repository within the previous seven days and to report on the latest commit SHA. Re-run checks after the final push and do not rely on an earlier commit's result.

## 4. Owner application steps

These are owner-only actions for MP-GOV-04. They were not performed by MP-GOV-01.

1. Wait until MP-GOV-01 and MP-GOV-02 are accepted. MP-GOV-03 must be accepted before connecting an AI reviewer to GitHub, but it is not a prerequisite for applying the manual-merge branch rules.
2. Open the repository on GitHub and navigate to **Settings → Rules → Rulesets**.
3. Create one branch ruleset named **main — owner-approved PR admission**.
4. Set enforcement to **Active** and target only the main branch.
5. Leave the bypass list completely empty. Do not add repository admins, users, teams, Apps, bots, Dependabot, or Copilot agents.
6. Enable the pull-request, review, stale-dismissal, latest-push approval, conversation-resolution, required-status-check, strict-up-to-date, force-push-block, and deletion-block settings listed above.
7. Select **Verify Windows product** from GitHub Actions (App ID 15368) only after confirming that it is the exact, current, always-executed check on a Ready-for-review PR. Resolve any MP-GOV-02 workflow findings before selecting a replacement check.
8. Enforce owner-specific approval: add .github/CODEOWNERS with the patterns above and enable Require review from Code Owners. The CODEOWNERS file itself must be owned by the repository owner. The only alternative is to verify and record that no other human has write access, so the one required approval can only come from the owner.
9. Save the ruleset. Keep repository Auto-merge disabled. Do not enable a merge queue or install an App.
10. Read the saved ruleset back from the Settings UI and the repository ruleset API. Verify its target, Active enforcement, every rule, exact required-check name/source, and an empty bypass list. Also read back the repository Auto-merge setting as false.
11. Record the effective settings, readback timestamp, and evidence in MP-GOV-04. If any required field cannot be read back, status remains NOT VERIFIED.

The current connected integration cannot read the classic branch-protection detail endpoint (HTTP 403). The owner must use the Settings UI or an appropriately authorized read-only administrative view for the final effective-settings readback. Do not infer successful activation from a saved page, an issue comment, or this runbook.

## 5. Manual merge procedure

For each contribution:

- Confirm the PR targets main and comes from a distinct contributor identity.
- Inspect the exact current head SHA and full diff; treat all contributor-controlled content as untrusted data.
- Confirm the required CI check completed successfully on that exact head and was not merely skipped.
- Confirm at least one valid owner approval after the latest reviewable push; stale approvals must be dismissed.
- Confirm all review conversations are resolved.
- The owner explicitly approves and then uses GitHub's manual merge control.
- Do not click Enable auto-merge, add the PR to a merge queue, invoke a Broker, or let an agent merge.
- Record the merge commit SHA and the final source SHA in the task handoff.

An owner-authored PR cannot satisfy the owner-approval rule because GitHub disallows self-approval. Re-submit such work from a distinct contributor-owned fork/account rather than bypassing the rule.

## 6. Negative-test matrix

Run mutation probes only in a disposable test repository or disposable branch configured with equivalent rules. Never test a rejected push, force-push, deletion, or bypass by risking production main.

| Probe | Expected result | Evidence / disposition |
|---|---|---|
| Contributor attempts a direct push to main | Rejected; changes must arrive through a PR | Disposable-repository test only |
| Repository admin attempts a direct push without a configured bypass | Rejected by the active ruleset | Disposable-repository test only; admin can still edit/delete the ruleset |
| Force-push to main | Rejected | Disposable-repository test only |
| Delete main | Rejected | Disposable-repository test only |
| PR has no approving review | Merge blocked | Disposable PR |
| PR author attempts to approve their own PR | GitHub rejects self-approval; owner-authored PR cannot satisfy owner review | Verify with distinct contributor identity |
| New code commit is pushed after approval | Stale approval is dismissed; another approval is required | Disposable PR |
| Latest reviewable push has no approval from someone other than its pusher | Merge blocked | Disposable PR |
| A review conversation remains unresolved | Merge blocked | Disposable PR |
| Required check is pending or failing | Merge blocked | Disposable PR |
| Required check is reported by an unexpected source | Merge blocked when expected GitHub Actions source is pinned | Disposable PR |
| Required Actions job is skipped | GitHub may report the skipped job as success; this is not test execution | Verify Ready-for-review event actually runs the check; otherwise fail MP-GOV-04 activation |
| Bot, AI agent, or GitHub App attempts to bypass | No bypass path exists because the bypass list is empty | Inspect saved bypass list; probe only in disposable repo |
| Auto-merge or merge queue is offered/enabled | Not part of the operating policy | Confirm repository Auto-merge false and no merge-queue rule |
| Administrator edits or deletes the ruleset | Technically possible for a repository admin | Document as root-of-trust limitation; protect the owner account and monitor settings changes |

No negative mutation probe against production main was run as part of MP-GOV-01.

## 7. Administrator and root-of-trust limitation

GitHub allows repository administrators to create, edit, and delete repository rulesets. An empty bypass list prevents an actor from using a configured ruleset bypass, but it does not make the policy immutable against an administrator who can change or remove the ruleset.

The repository owner account is therefore part of the root of trust. Protect it with strong multi-factor authentication or passkeys, secure recovery methods, and careful review of authorized applications and account sessions. This task does not change account-security settings or create credentials.

A compromised owner/admin account can alter the policy and then modify main. GitHub repository-level branch protection cannot independently defend against that root-account compromise. Do not claim that it can.

## 8. References

- [GitHub: About rulesets](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/about-rulesets)
- [GitHub: Creating rulesets for a repository](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/creating-rulesets-for-a-repository)
- [GitHub: Available rules for rulesets](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/available-rules-for-rulesets)
- [GitHub: About protected branches](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches)
- [GitHub: About code owners](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/about-code-owners)
- [GitHub: Troubleshooting required status checks](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/troubleshooting-required-status-checks)

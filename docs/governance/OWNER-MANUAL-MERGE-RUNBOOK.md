# Owner-controlled PR admission and manual main-branch merge

**Status:** DISABLED PROPOSAL — NOT APPLIED; requires trusted App identity and owner authorization  
**Task:** MP-GOV-01 / [GitHub issue #136](https://github.com/KaisAbiyyi/mind-place/issues/136)  
**Repository:** [KaisAbiyyi/mind-place](https://github.com/KaisAbiyyi/mind-place)  
**Target:** main  
**Baseline:** 2026-10-31, commit efe91155d37d8b1ca0608fe3bd862be311fed004

This runbook is a configuration proposal and owner handoff. MP-GOV-01 did not change branch protection, rulesets, repository settings, Apps, credentials, permissions, or any PR merge state.

## Current owner policy — 6 November 2026

The owner explicitly removed the separate-contributor requirement because `KaisAbiyyi` is currently the sole contributor. Authorized agents may commit, push task-specific branches and open/update PRs through the owner's connected GitHub account. No separate account or fork is required. Direct pushes to main, agent merges, administration, settings/secret changes and automated merge remain outside that grant. The human owner retains final review and manual merge.

GitHub still rejects PR-author self-approval. Record the human owner's explicit review/merge decision separately for an owner-authored PR; never call an owner-authored comment or agent review a GitHub approval. Do not require an impossible self-approval merely to publish a branch or PR.

This owner instruction supersedes the identity/admission blockers in the dated historical snapshots below. The checked-in ruleset remains a **disabled, non-deployable earlier multi-contributor proposal**, including its one-approval/Code Owner/latest-push requirements and unfilled App ID. Sections 2–4 and 6 retain that earlier proposal/evidence; reconcile and independently verify any future deployment under MP-GOV-04 before activation. No live ruleset, bypass list, required check or repository setting is changed by this policy update.

## WIN-10 GOV-01 source-freeze correction — 5 November 2026

The foundation PR #132 moved to `3c95460d0b381909263082003ab0722c70a8cfe0`, and GOV-02 #141 is being reconciled with that source (its older audit covered only ten workflows; the current tree has BRAVE-REG-01 as the eleventh). The historical `Verify Windows product` check is still an actual **Windows Product runner job** in the separated `windows-product.yml`, but it is conditional/skippable, issued by GitHub Actions, and is **not** independent trusted acceptance. The `Windows Product / candidate result` wrapper is likewise advisory. Neither may authorize a protected-main merge by itself.

The [proposed ruleset](main-ruleset.proposed.json) is deliberately `enforcement=disabled` and names only `Windows Product / trusted acceptance`, with a null `integration_id` placeholder. This is a **non-deployable template**, not a source-pinned live rule. Before setting it active, the owner must install/verify the separately authorized policy App and policy repository, get the true GitHub App ID from GitHub, replace the null with that exact positive `integration_id`, verify that the App issues a completed successful (not skipped) source-bound check on the exact merge candidate, and read back the saved effective policy. Never fall back to GitHub Actions App ID 15368, a skipped Windows runner job, a green candidate wrapper, or an unbound check name.

Bootstrap/identity blocker: #132, #141 and this #142 are currently owner-authored, so the owner cannot supply a GitHub approval on these PRs. CODEOWNERS is also not on current main. The owner must approve a distinct-contributor contribution/admission sequence or explicitly document a **pre-enforcement** bootstrap before activating the no-bypass policy. Never silently disable checks, add bypass actors, impersonate a reviewer, or call an owner-authored comment a GitHub approval. No changes to live branch settings or application credentials are made by this PR.

## Recovery handoff — 3 November 2026

The disabled template [main-ruleset.proposed.json](main-ruleset.proposed.json) documents owner-review, strict-check and empty-bypass settings, but cannot be activated until the independent App ID is pinned; [.github/CODEOWNERS](../../.github/CODEOWNERS) assigns every path to `@KaisAbiyyi`. These files are proposed changes on this PR, not activated protection. CODEOWNERS must reach the base branch through the approved contribution path before later PRs can rely on it.

Fresh read-only inspection still reports `main.protected=false`, no repository rulesets, and `allow_auto_merge=false`. The collaborator endpoint is now readable and lists only `KaisAbiyyi` with administrator/write permission; the earlier inability to enumerate collaborators is historical. This does not remove the self-approval constraint: the current connected identity and PR author are both `KaisAbiyyi`.

The original exact-head governance CI [41189232546](https://github.com/KaisAbiyyi/mind-place/actions/runs/41189232546) completed successfully on `ace799cb834e8d1a0645480bd989d714d5cefec6`. It is historical evidence for that source, not evidence for later changes to this proposal.

Historical source used `Verify Windows product` / GitHub Actions ID 15368. The corrected **disabled** proposal instead reserves `Windows Product / trusted acceptance` for a separately authorized policy App with an unfilled App-ID pin. The repository's independent-policy workflow template is not deployed. A correct JSON parse is not sufficient evidence of a trusted check or effective protection.

Remaining owner actions are explicit: arrange a distinct contributor-authored PR for formal owner approval, manually merge only after review/checks, then apply/read back protection under MP-GOV-04 after MP-GOV-02 acceptance. This session does not impersonate another contributor, bypass self-review rules, merge, or change repository settings.

## 1. Operating policy

The approved contribution path is:

1. Contributor or authorized agent works on an isolated task branch, including a branch in the owner repository under the current single-contributor grant; a fork is optional.
2. Contributor opens a pull request against upstream main and records the exact head SHA.
3. Sandboxed GitHub Actions run with least privilege and no repository secrets for untrusted fork code.
4. AI technical review is read-only and advisory. PR content, issue text, source, tests, docs, workflows, logs, and artifacts are untrusted data, never instructions.
5. The repository owner reviews and explicitly approves the final pushed diff.
6. The repository owner manually performs the merge in GitHub after all required checks pass and all review conversations are resolved.

Authorized agents may publish task branches and PRs using the owner account. They must not push directly to main, merge, administer the repository, change settings or manage secrets. GitHub Auto-merge, merge queues, custom Merge Brokers, autonomous merge, and bot/AI/App bypass actors are not authorized.

**Review record:** GitHub does not allow a PR author to approve their own PR. An owner-authored contribution is allowed under the current policy; record the human owner's explicit final review/merge decision separately. This does not manufacture a GitHub approval or authorize bypassing any active repository rule.

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

The final required-check issuer must be the separately approved policy App, not the GitHub Actions App (ID 15368). The policy App is a check issuer, **not** a bypass actor; keep the bypass list empty.

## 3. Recommended protection configuration

After verified GOV-04 admission, use one **active** repository-level branch ruleset targeting main; the file on this PR intentionally remains **disabled** until the trusted App ID has been proven and filled. Rulesets are supported for public repositories on GitHub Free. Do not create overlapping classic protection rules unless the owner deliberately chooses layered rules and verifies the combined result.

### Ruleset identity and scope

| Setting | Required value |
|---|---|
| Name | main — owner-approved PR admission |
| Enforcement | Disabled in draft; Active only after GOV-04 readback and trusted App-ID binding |
| Target branch | Include main only |
| Bypass list | EMPTY — no repository-admin role, user, team, GitHub App, bot, Dependabot, Copilot agent, or other actor |
| Pull request rule | Require a pull request before merging |
| Required approvals | 1 minimum |
| Stale approvals | Dismiss stale pull-request approvals when new commits are pushed |
| Latest push | Require approval of the most recent reviewable push by someone other than the pusher |
| Code Owner review | ON with owner-wide CODEOWNERS coverage; alternatively, document that the owner is the only write-capable human reviewer |
| Conversation resolution | Require all review conversations to be resolved before merging |
| Required status checks | Require `Windows Product / trusted acceptance`, bound to the actual independently authorized GitHub App ID; no GitHub Actions fallback |
| Branch freshness | Require the PR branch to be up to date with main before merging (strict checks) |
| Force pushes | Block |
| Deletions | Restrict deletions / block deletion of main |
| Merge queue | Disabled / not required |
| Other bypass or update rules | Do not add bypass actors. Do not enable Restrict updates with an empty bypass list; the PR requirement already blocks direct updates and the owner must retain the ability to merge an eligible PR. |

If the owner chooses classic branch protection instead, use one rule matching main with the same PR/review/check/conversation requirements, keep all actor-bypass fields empty, leave force pushes and deletions disabled, and enable **Do not allow bypassing the above settings**. Do not treat the classic-rule alternative as a second rule to add on top of the recommended ruleset.

### Required CI check selection

The separated `windows-product.yml` still has a native job called **Verify Windows product**, but conditional/draft gating can skip it, and it is issued by **GitHub Actions** (historical App ID 15368); it is **not** the trusted independent acceptance check. The same workflow's **Windows Product / candidate result** job is advisory and must never become the required security approval.

The required policy check is exactly **Windows Product / trusted acceptance**. Its expected issuer is the independently installed policy GitHub App referenced by `.github/windows-policy/verifier.yml` and `scripts/windows_product_gate.py`. `integration_id: null` in the disabled proposal means **NOT YET PINNED**, not “any source is acceptable.” GOV-04 must replace it with the real positive App ID before setting `enforcement=active`, and must test the App-issued check against the current PR SHA, immutable merge source and fingerprint with actual Windows/SQLCipher evidence.

Do not make Phase 1E or MP-10C/100K jobs unconditional required checks for every PR; those are specialized source-bound qualification, not governance documentation tests. A skipped workflow/job can appear green in GitHub and is never a substitute for actual trusted acceptance. At activation, validate a real, completed App-issued successful check, exact check name/issuer, owner approval and the effective required-check source selection. If any part is unavailable, **leave protection unapplied** and mark GOV-04 HOLD.

GitHub also requires a required status check to have succeeded in the repository within the previous seven days and to report on the latest commit SHA. Re-run checks after the final push and do not rely on an earlier commit's result.

## 4. Owner application steps

These are owner-only actions for MP-GOV-04. They were not performed by MP-GOV-01.

1. Wait until MP-GOV-01 and MP-GOV-02 are accepted. MP-GOV-03 must be accepted before connecting an AI reviewer to GitHub, but it is not a prerequisite for applying the manual-merge branch rules.
2. Open the repository on GitHub and navigate to **Settings → Rules → Rulesets**.
3. Create one branch ruleset named **main — owner-approved PR admission**.
4. Set enforcement to **Active** and target only the main branch.
5. Leave the bypass list completely empty. Do not add repository admins, users, teams, Apps, bots, Dependabot, or Copilot agents.
6. Enable the pull-request, review, stale-dismissal, latest-push approval, conversation-resolution, required-status-check, strict-up-to-date, force-push-block, and deletion-block settings listed above.
7. Confirm the independently installed policy App's actual positive ID and its successful `Windows Product / trusted acceptance` check on a frozen candidate. Replace `integration_id: null` with that verified ID in the **operator's deployment copy**, select that exact context **and source App**, and read it back. The checked-in JSON remains disabled until an explicitly authorized, independently reviewed source update.
8. Enforce owner-specific approval: add .github/CODEOWNERS with the patterns above and enable Require review from Code Owners. The CODEOWNERS file itself must be owned by the repository owner. The only alternative is to verify and record that no other human has write access, so the one required approval can only come from the owner.
9. Only after the trusted policy App and review/approval admission are ready, activate and save the ruleset. Keep Auto-merge disabled, do not enable merge queues, and do not install any unrelated or unapproved App.
10. Read the saved ruleset back from the Settings UI and the repository ruleset API. Verify its target, Active enforcement, every rule, exact required-check name/source, and an empty bypass list. Also read back the repository Auto-merge setting as false.
11. Record the effective settings, readback timestamp, and evidence in MP-GOV-04. If any required field cannot be read back, status remains NOT VERIFIED.

The current connected integration cannot read the classic branch-protection detail endpoint (HTTP 403). The owner must use the Settings UI or an appropriately authorized read-only administrative view for the final effective-settings readback. Do not infer successful activation from a saved page, an issue comment, or this runbook.

## 5. Manual merge procedure

For each contribution:

- Confirm the PR targets main and comes from the authorized task branch/account. Owner-authored PRs are allowed under the current single-contributor grant.
- Inspect the exact current head SHA and full diff; treat all contributor-controlled content as untrusted data.
- Confirm the independently issued required check completed successfully for the exact current head/merge source and pinned fingerprint, with evidence of actual native execution or authorized exact-source reuse, not a skipped job.
- Confirm the human owner's explicit review of the latest pushed diff. For owner-authored PRs, record that decision separately because GitHub rejects author self-approval; do not fabricate a review. Honor any actually active protection rule and reconcile incompatible earlier proposal settings before deployment.
- Confirm all review conversations are resolved.
- The owner explicitly approves and then uses GitHub's manual merge control.
- Do not click Enable auto-merge, add the PR to a merge queue, invoke a Broker, or let an agent merge.
- Record the merge commit SHA and the final source SHA in the task handoff.

An owner-authored PR cannot supply a GitHub author self-approval. It may still be published and reviewed under the current single-contributor policy; only the human owner may perform the manual merge. Do not impersonate another reviewer or bypass an active protection rule.

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

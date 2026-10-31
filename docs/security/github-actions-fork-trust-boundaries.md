# MP-GOV-02 — GitHub Actions fork trust-boundary audit

**Audit baseline:** `main` at `efe91155d37d8b1ca0608fe3bd862be311fed004`  
**Date:** 2026-10-31  
**Scope:** workflow triggers, `GITHUB_TOKEN` permissions, secrets/environments/OIDC, checkout refs and credential persistence, fork execution, artifacts/caches, third-party action pinning, and privileged event paths.

## Executive result

The seven-workflow baseline does not contain a `pull_request_target`, `workflow_run`, `issue_comment`, or `workflow_call` trigger. Each baseline workflow explicitly grants only `contents: read`; no baseline workflow references repository secrets, requests `id-token: write`, declares a protected environment, or grants a write-scoped token permission.

The normal `pull_request` workflows do execute pull-request-controlled code. That code must therefore be treated as hostile. For public-repository fork pull requests, GitHub documents a read-only `GITHUB_TOKEN` and no access to repository secrets. The explicit `contents: read` policy also prevents these workflows from requesting write access. No privileged event is used to check out or execute fork code.

Two confirmed hardening issues were found:

1. Six of the seven baseline workflows used the default checkout credential persistence. This leaves the checkout token in local Git configuration for subsequent commands. The four files outside the active MP-CI workflow lock are now set to `persist-credentials: false`; the remaining three are deliberately untouched until that lock is released.
2. External actions were referenced by mutable version tags rather than immutable commit SHAs. All actions in the four unlocked workflows are now pinned to upstream commit SHAs, with their version labels retained as comments. Three workflow files remain pending because active PR #129 owns their shared workflow edits.

**Disposition: partial hardening, not final acceptance.** The active-lock exceptions are explicit and regression-tested. MP-GOV-02 must be revisited after the overlapping MP-CI work is integrated.

## Main-branch workflow inventory

The complete recursive Git tree at the baseline contained seven files under `.github/workflows/`, no additional YAML action definitions, and no local composite-action directory. No reusable workflow call was found in the baseline.

| Workflow | Trigger | Token scope | Secrets / environment / OIDC | Checkout and output boundary | Disposition |
|---|---|---|---|---|---|
| `ci.yml` | `push: main`, `pull_request` | `contents: read` | No secret reference, environment, or OIDC permission | Checkout default persists credentials; npm cache; no artifact consumer | **Pending** — active `ci-document-scale-workflow` lock |
| `mp-10c-scale.yml` | Path-filtered `pull_request`, `workflow_dispatch` | `contents: read` | No secret reference, environment, or OIDC permission | Checkout default persists credentials; uploads a qualification receipt; no download consumer on baseline | **Pending** — active `ci-document-scale-workflow` lock |
| `mp10b-bakeoff.yml` | Path-filtered `pull_request`, `workflow_dispatch` | `contents: read` | No secret reference, environment, or OIDC permission | Checkout already disabled persistence; uploads synthetic report | **Hardened in this branch** |
| `mp8f-visual.yml` | Path-filtered `pull_request`, `workflow_dispatch` | `contents: read` | No secret reference, environment, or OIDC permission | Checkout now disables persistence; npm cache; uploads synthetic screenshots | **Hardened in this branch** |
| `mp9a-security-probe.yml` | Path-filtered `pull_request`, `workflow_dispatch` | `contents: read` | No secret reference, environment, or OIDC permission | Checkout now disables persistence; uploads aggregate synthetic receipt | **Hardened in this branch** |
| `release.yml` | `push` for `v*` tags, `workflow_dispatch` | `contents: read` | No secret reference, environment, or OIDC permission | Checkout now disables persistence; npm cache; uploads workflow artifact only; does not publish a GitHub Release | **Hardened in this branch** |
| `storage-qualification.yml` | Path-filtered `pull_request`, `workflow_dispatch` | `contents: read` | No secret reference, environment, or OIDC permission | Checkout default persists credentials; uploads synthetic qualification receipt | **Pending** — active `ci-document-scale-workflow` lock |

All `pull_request` triggers use the ordinary `pull_request` event. No baseline workflow elevates fork code through a privileged event. The workflow files are not proof that repository-level Actions settings or branch protection are configured; those settings were not changed or inspected as part of this bounded audit.

## Threat analysis

### Fork pull-request execution

A fork PR can modify source, tests, build scripts, package metadata, and workflow YAML. The resulting code runs on GitHub-hosted runners, so the workflow must assume arbitrary code execution within the job. The baseline permission boundary is read-only and no workflow references secrets. The remediation does not change event types or move PR-controlled code into a privileged context.

Do not introduce `pull_request_target` or a privileged `workflow_run` consumer that checks out or executes the PR head. Any future privileged workflow must be separated from PR-controlled code and reviewed as a new trust boundary.

### Checkout credentials

The checkout action persists its token by default unless disabled. No baseline workflow needs to push from its checkout. The unlocked workflows now explicitly set `persist-credentials: false`. This is defense in depth: the token remains read-only, but should not be left in Git configuration for untrusted build/test steps to inspect.

### Third-party actions

The unlocked workflows now use full 40-character commit SHAs. The pins were resolved from the corresponding upstream action repositories at audit time:

| Action | Upstream version | Pinned commit |
|---|---|---|
| `actions/checkout` | v4 | `11d5960a326750d5838078e36cf38b85af677262` |
| `actions/checkout` | v5 | `fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09` |
| `actions/setup-node` | v4 | `49933ea5288caeca8642d1e84afbd3f7d6820020` |
| `actions/setup-node` | v5 | `a0853c24544627f65ddf259abe73b1d18a591444` |
| `actions/setup-python` | v5 | `a26af69be951a213d495a4c3e4e4022e16d87065` |
| `actions/upload-artifact` | v4 | `ea165f8d65b6e75b540449e92b4886f43607fa02` |
| `actions/upload-artifact` | v6 | `b7c566a772e6b6bfb58ed0dc250532a479d7789f` |
| `dtolnay/rust-toolchain` | stable branch as of audit | `b725eda525362e7cf34af9a7243cac1e3f01a7ec` |

Policy: external actions must use a full commit SHA, with a human-readable version comment. Updates must verify that the SHA belongs to the intended upstream repository and corresponds to the reviewed release. A tag or branch name alone is not an immutable pin.

### Cache boundary

The baseline uses `actions/setup-node` npm caching in CI, visual regression, and release workflows. These caches contain dependency-package cache data, not application secrets or private build credentials. GitHub documents that fork PR runs can restore caches from the base/default branch, but pull-request cache writes are scoped to the PR merge ref and cannot overwrite the default-branch cache. Never place credentials, private corpus data, or secret-bearing files in these cache paths.

There is no explicit `actions/cache` step in the seven-file baseline. The active MP-CI-100K-01 branch adds `Swatinem/rust-cache`; that cache implementation and its immutable pin must be rechecked at the final integrated head. Its current upstream v2 tag resolves to commit `d1527c8f67ef9af357290475d4af4b0fab8f12e0`, but this audit does not modify that locked branch.

### Artifact boundary

The seven-file baseline uploads build or synthetic qualification outputs. It has no `download-artifact` consumer and no `workflow_run` trigger that could promote a fork-controlled artifact into a privileged workflow. Treat all PR artifacts as untrusted data; do not execute scripts or binaries from them. Do not place secrets or private user content in artifacts.

The unmerged MP-CI-100K-01 branch adds a same-run artifact download/provenance path. That path is outside this branch's write scope and must be revalidated at the integrated PR head, including that the downloaded receipt is parsed as data and is bound to the exact source/run/artifact digest.

## Unmerged workflow work and active ownership

These files are not part of the seven-file `main` baseline, but are present in open Draft PRs. They are listed so this audit does not imply that unmerged workflows are accepted.

- **PR #129 — MP-CI-100K-01:** owns the `ci-document-scale-workflow` lock and changes `ci.yml`, `mp-10c-scale.yml`, and `storage-qualification.yml`. Its branch adds `Swatinem/rust-cache@v2` and `actions/download-artifact@v4`; checkout credentials remain at the default in the inspected branch snapshot. Do not edit these files concurrently. Re-audit their exact integrated head after the lock is released.
- **PR #127 — MP-9B:** stacks on #129, changes `ci.yml`, and adds `mp9b-sqlcipher-qualification.yml`. The new workflow explicitly has `contents: read`, but its checkout is tag-pinned and uses default credential persistence. It needs the same immutable SHA and `persist-credentials: false` treatment after the stacked workflow lock is released.
- **PR #131 — MP-CI-WIN-01:** adds `windows-product.yml`. Its inspected snapshot already uses SHA-pinned checkout/setup-node/upload-artifact actions and `persist-credentials: false`, with read-only `contents`, `actions`, `checks`, and `pull-requests` permissions. It uses a repository variable for the App ID and does not reference a secret in the target workflow.
- **PR #131 policy template:** `.github/windows-policy/verifier.yml` is outside `.github/workflows/` and is explicitly a deployment template for a separate owner-controlled policy repository. It contains a secret reference and requests `checks: write` for a scoped GitHub App token. It is inert in the target repository tree; do not copy or activate it as a target-repository workflow. Its separate deployment and authority review remain mandatory.
- **PR #132:** is the Draft integration candidate for #127/#129/#131 and contains their overlapping workflow changes. It is not a new independent baseline.

The three pending baseline files and these in-flight files must be reconciled after the existing owner/agent locks are released. This task does not take over MP-CI-100K-01, MP-9B, or MP-CI-WIN-01.

## Regression coverage

- `workflow-security.yml` runs actionlint v1.7.12 and the repository policy script whenever workflow YAML or this policy's test/documentation changes.
- `test/workflow-security.test.mjs` runs through the normal `npm test` suite.
- The policy script verifies that the workflow inventory is explicit; all workflows retain read-only `contents`, no write permissions, no secret references, and no privileged/reusable event triggers; and every unlocked workflow pins external actions to full SHAs and disables checkout credential persistence.
- The three exact active-lock exceptions are enumerated. A new workflow or changed inventory fails the regression until it is reviewed and classified.

## Out of scope and remaining gates

- No repository settings, branch protection, rulesets, secrets, GitHub App, tokens, release, or workflow dispatch were changed or invoked.
- No 100K/1M scale qualification, separate MP-CI native-evidence/Windows-product qualification, or owner-machine/local qualification was invoked. The ordinary PR CI workflow's standard hosted Windows test/build job was allowed to run.
- The branch-protection observation `main protected=false` belongs to MP-GOV-01/04; this task does not change it.
- Repository secret inventory was not read. The finding is that the audited workflow source contains no secret references, not that the repository contains no configured secrets.
- Final acceptance requires the active MP-CI workflow lock to be released, all in-flight workflow changes to be integrated/reconciled, the remaining checkouts and action references to be hardened, the temporary exceptions removed, and the resulting exact head to pass actionlint and regression CI.

## References

- [GitHub Actions security hardening](https://docs.github.com/en/actions/how-tos/secure-your-work)
- [GitHub secure-use reference](https://docs.github.com/en/actions/reference/security/secure-use)
- [GitHub dependency caching and cache access restrictions](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching)
- [GitHub guidance for securely using pull_request_target](https://docs.github.com/en/actions/reference/security/securely-using-pull_request_target)
- [GitHub compromised-runner security model](https://docs.github.com/en/actions/concepts/security/compromised-runners)

# MP-SEC-12 — supply-chain security gate

Claim: `mp-sec-12-20261115-codex-01`, generation 1, Codex / ChatGPT Work.
Exclusive logical resource: `ci-supply-chain`. Base main:
`fa2e6b7e8b8ce919067e6e0def28032261646050`. Task: [#177](https://github.com/KaisAbiyyi/mind-place/issues/177).
[Notion](https://app.notion.com/p/414c4a6aaf7381bebeaac0711910f3c4).

This is an additive gate proposal, not owner acceptance, a release, or an assertion
that existing dependencies are vulnerability-free. Keep Draft until independent
security review. No merge or self-approval is authorized.

## Scope and coordination

Live Notion and open-PR ownership were checked before claiming. No overlapping
`ci-supply-chain` claimant was present. Existing `ci.yml`, `release.yml`, Windows,
SQLCipher, storage, and scale workflows are byte-for-byte unchanged. The only
existing policy edit registers four new workflows, recognizes the two pinned
CodeQL subactions, and explicitly rejects `security-events: write` as well as
other write permissions. UI2, MP-9C and dependency manifests/locks are untouched.
Notion Focus Lane and Order remain unchanged at the owner's explicit request.

The task's `docs/MAIN-PLAN.md` §4.2 pointer is absent from current main. Its scope
was read from plan PR #164 at `ac7b7f20fa5a6774101593a8a0602298a85fa288`, in
addition to the live task and direct owner instructions. This does not adopt or
merge that separate plan PR.

## Policy

| Check | Coverage | Blocking policy |
| --- | --- | --- |
| cargo-deny 0.18.6 | Whole workspace, all features and targets, including build/dev graph | Every vulnerability/unsound/notice advisory as implemented by this pinned cargo-deny; unmaintained crates and yanked versions fail; no ignore IDs or graph pruning |
| Rust licenses | Workspace and third-party crates; private packages included | Deny by default; listed license families below only; unknown/ambiguous licensing fails; no crate exceptions or invented clarifications |
| Rust sources | Resolved dependency graph | crates.io registry only; unknown registries and every Git source fail |
| npm audit | Lockfile closure including production, dev, optional and peer packages | High and critical fail even if already present on base; low/moderate remain in evidence; registry/network/parser errors are BLOCKED, never PASS |
| CodeQL | JS/TS, Rust and Python, security-extended, build-mode none | All SARIF findings block pending independent disposition, including suppressed results; incomplete scans fail; no automatic baseline waiver |
| OpenSSF Scorecard | Public remote default-branch posture | Collection only, not an aggregate-score acceptance gate; retain all scores, errors and `-1` unavailable results |

The npm high threshold prioritizes actionable severe runtime and build-time
exposure without treating every low/moderate advisory as a release stop. Including
development dependencies is deliberate: build/test tooling executes on runners.
Low/moderate findings still require triage. This is a severity policy, not an
advisory allowlist, and can be tightened through a reviewed policy change.

The Rust license set is a distribution policy for an MIT application, not a
relicense grant: MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause and ISC require
preserving their respective notices; Unicode-3.0, Zlib, BSL-1.0 and OpenSSL retain
their license-specific notice/attribution requirements; CC0-1.0 permits public
domain dedication. OpenSSL is included for multi-license cryptographic sources
such as ring, not as an advisory exemption. Alternative license expressions may
select an allowed branch, but AND expressions must satisfy every required term.
Copyleft or an unrecognized license is held for explicit review, not silently
accepted to make CI green. Existing Windows license/notices packaging remains
authoritative and independently verified; this gate does not replace it.

## Base findings versus regressions

On each PR, audit the exact head and base with the same **head policy**. npm saves
both raw reports, counts, package/range/severity/advisory identities, and separate
existing, introduced/changed, and resolved findings. Rust fetches the advisory DB
once, records its Git commit, and disables refetch for both audits. Both Rust
diagnostic streams are retained with exit codes and common/new advisory IDs.
An ID-only comparison cannot classify affected-version, license or source changes:
review both logs and dependency diffs before deciding whether those are regressions.
Existing failures remain failures. Tool/database failures remain visible as failed
steps, not a fabricated clean inventory. CodeQL needs independent base triage for
any finding; a finding is not automatically called a regression.

After merge, push/scheduled/manual audits compare main with itself. They detect
new upstream advisories even when no dependency file changes. There are no path
filters on the main audit gate. No `npm audit fix`, lock regeneration or automatic
dependency upgrade is performed by this change.

## Reproducibility and updater

`rust-toolchain.toml` pins Rust **1.90.0**, minimal profile plus rustfmt, matching
existing production/release CI and the workspace's 1.90 MSRV. No compiler upgrade
is part of this PR. OS tools, Node 24 patch releases and registry/advisory data are
not claimed hermetic. Both dependency lockfiles remain unchanged and are checked
after Rust audits/SBOM generation.

Dependabot opens separate weekly Cargo, npm and GitHub Actions proposals. It does
not approve, auto-merge, change repository settings or replace exact-head CI.
Actions updates must retain full reviewed commit SHAs. Dependabot's managed
service and repository enablement require owner-side availability after merge;
no first PR or enabled service is claimed before observing it.

Security-tool downloads are pinned by version, official release URL and SHA-256
in `scripts/supply-chain/tools.json`. Checksums were read from the intended upstream
GitHub release asset metadata on 2026-11-15, not downloaded dynamically by the
workflow as trust anchors. Archive extraction selects exactly one bounded regular
binary; no archive path is extracted. A checksum mismatch, duplicate, traversal,
symlink or hardlink fails. No mutable Docker image is added through Scorecard's
action wrapper: the checksum-pinned upstream CLI is used directly.

## Release SBOM

`release-sbom.yml` runs for every `v*` tag, explicit dispatch and relevant PR
changes. It validates tag/version equality and produces an independent workflow
artifact bound to the exact source SHA. It never creates/publishes a GitHub Release,
downloads another workflow's artifacts, or executes an untrusted artifact.

- cargo-cyclonedx 0.5.10 emits a validated CycloneDX 1.5 inventory per workspace
  member for `x86_64-pc-windows-msvc`, using release manifest defaults. The
  desktop and capture-host dependency closures are represented separately.
- Built-in `npm sbom` emits the locked npm closure including development tooling;
  this is a conservative source/build inventory, not a claim all npm packages ship.
- A separate CycloneDX document records the bundled helper's exact version,
  executable hash and upstream SPDX-resource hash from the existing helper pin.
  Its transitive Go dependencies stay in the pinned upstream SPDX file already
  included by the existing Windows packaging. cloudflared remains excluded.
- `manifest.json` records source SHA, target, features, lock hashes, tool pins,
  document component counts and SHA-256 hashes. Missing/empty BOMs or modified
  lockfiles fail. Artifacts are retained for 90 days; permanent release attachment,
  binary-to-SBOM attestation, signing and installer inventory remain MP-REL-01.
- OS/WebView2 dependencies are outside this source SBOM. Do not label it a fully
  attested installer SBOM or a native packaging qualification.

## Trust boundary and service limitations

All workflows retain `contents: read`, no secrets, no persisted checkout token,
no OIDC, and no privileged/reusable event. PR jobs check out the explicit PR head;
findings/artifacts are untrusted review data. Existing native gates remain
separate, and a successful scan is not owner approval or branch-rule enforcement.

CodeQL action pin `33d7b85da1a04171e211befe2c739303b9a4c066` was resolved from
the upstream v4 annotated tag. Analysis is supported for the public repository.
Private clones fail explicitly pending license/Code Security entitlement review.
SARIF and databases are **not** uploaded to code scanning: that would require a
separately reviewed `security-events: write` boundary and possibly disabling
default setup. No repository setting is modified here. Artifacts remain available
for independent security review.

Scorecard CLI runs with the built-in read-only token. GitHub API limits, unavailable
branch-protection data and checks that need broader visibility may produce unknown
scores; the original reasons are retained, never converted into PASS. Scorecard
observes the remote default branch (including its reported commit), not the PR
snapshot. No results are sent to the Scorecard publication service.

Required external endpoints include crates.io, RustSec/GitHub, npm registry,
upstream tool releases and CodeQL bundle/package services. An outage, missing
permission, unsupported language/tool or download rejection is BLOCKED/failed,
not a skip-based security certificate. Configure required checks only through a
separate owner-controlled settings decision; this PR does not claim enforcement.

## Verification and independent handoff

Local focused checks: `node --test test/supply-chain.test.mjs test/workflow-security.test.mjs`,
`python -m unittest discover -s scripts/supply-chain/tests -v`,
`node scripts/verify-workflow-security.mjs`, and `cargo fmt --all -- --check`.
Hosted policy also validates all workflows with actionlint v1.7.12. Full existing
CI remains required at the final head. Local restricted networking does not stand
in for real registry/tool audits; obtain the hosted logs/artifacts before verdict.

The PR evidence block must name its final SHA, every hosted run and outcome,
pre-existing vulnerabilities, newly introduced findings, unavailable checks and
any full-baseline limitations. Independent reviewer: recheck tool provenance,
license policy, all negative tests, raw base/head diagnostics, SARIF and Scorecard
unknowns, SBOM completeness/claims, original-workflow equality, exact head and
no dependency changes. Owner approval and manual merge remain separate.

## Primary references

- [cargo-deny checks](https://embarkstudios.github.io/cargo-deny/checks/index.html)
- [Pinned cargo-deny config and semantics](https://github.com/EmbarkStudios/cargo-deny/tree/0.18.6)
- [npm audit](https://docs.npmjs.com/cli/v11/commands/npm-audit)
- [npm SBOM](https://docs.npmjs.com/cli/v11/commands/npm-sbom)
- [cargo-cyclonedx](https://github.com/CycloneDX/cyclonedx-rust-cargo/tree/cargo-cyclonedx-0.5.10)
- [CodeQL pinned action](https://github.com/github/codeql-action/tree/33d7b85da1a04171e211befe2c739303b9a4c066)
- [OpenSSF Scorecard](https://github.com/ossf/scorecard/tree/v5.6.0)

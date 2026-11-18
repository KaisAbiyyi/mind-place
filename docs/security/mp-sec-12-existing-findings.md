# MP-SEC-12 — existing findings, separate from implementation regressions

Continuation: per-finding triage and owner-approved remediation decisions
(`#179`, `#180`, `#181`, and `chrono` → `time`) are recorded in
[independent review disposition](mp-sec-12-review-disposition.md). Historical
scan observations below from base `fa2e6b7e8b8ce919067e6e0def28032261646050` are retained for provenance.

Dependency inventory at base `fa2e6b7e8b8ce919067e6e0def28032261646050`:
464 Cargo lock entries (456 crates.io packages, eight workspace members), 138 npm
lock entries (all resolved through registry.npmjs.org). No alternate Cargo registry
or Git dependency source was present. Lockfiles and dependency manifests are
unchanged by MP-SEC-12. This is a resolved-source inventory, not a license or
vulnerability-free certificate.

Initial evidence source: checkpoint `73e97bdf0dfed83846744e50555602fe4ef27301`,
[supply-chain run 43679747890](https://github.com/KaisAbiyyi/mind-place/actions/runs/43679747890).
RustSec DB `4ef2df8e806a65e2f1ecc14f56f51249062b23d6` was used for **both**
base and head. The final-head receipt and independent review live in Draft PR #178;
this record does not pretend an earlier run certifies a later source.

## Rust advisory findings — base and head identical, still blocking

| Advisory | Locked package | Tool classification | Finding |
| --- | --- | --- | --- |
| RUSTSEC-2024-0429 | glib 0.18.5 | Unsound | Iterator/DoubleEndedIterator implementations for VariantStrIter |
| RUSTSEC-2024-0370 | proc-macro-error 1.0.4 | Unmaintained | Upstream no longer maintained |
| RUSTSEC-2026-0486 | rustix 1.1.5 | Unsound | Extended-attribute calls can mark unwritten bytes initialized with a zero-length buffer |
| RUSTSEC-2026-0502 | sqlcipher-src 419.0.6+sqlcipher-4.19.0-sqlite-3.53.4-libtomcrypt-1.18.2 | Vulnerability, CVE-2026-119375 | Crafted local database/WAL pair can expose uninitialized SQLite page-cache memory |
| RUSTSEC-2026-0543 | chrono 0.4.45 | Unmaintained | Observed in RustSec DB `233155b4025b854c68a4b199c0e32d7fd53291a2` on both base (`43763a4`) and head (`396d7c2`); all versions affected, recommended alternatives `jiff` or `time`; resolved for Windows via `mind-place-archive` and `mind-place-index` |

No new advisory IDs were introduced at the audited checkpoint. Platform,
reachability and exploitability still require independent triage; an all-target,
all-feature scan is deliberately broader than the qualified Windows product.
In particular, the SQLCipher dependency is part of a separately qualified storage
stack: remediation needs a separately scoped upgrade and native compatibility
qualification. Do not use this finding as permission to change MP-9C or storage
dependencies here. No advisory is ignored, downgraded or fixed by this PR.

## Rust license findings — base and head identical, still blocking

These are proposed-policy incompatibilities requiring a license/distribution
decision, **not** assertions that the upstream license is invalid or necessarily
incompatible with every MIT distribution.

| Locked package(s) | Unaccepted expression/term |
| --- | --- |
| cssparser 0.37.0, cssparser-macros 0.7.1, dtoa-short 0.3.5, option-ext 0.2.0, selectors 0.38.0 | MPL-2.0 |
| sqlcipher-src 419.0.6+sqlcipher-4.19.0-sqlite-3.53.4-libtomcrypt-1.18.2 | `MIT AND BSD-3-Clause AND blessing AND WTFPL`; blessing and WTFPL need review |
| target-lexicon 0.12.16 | Apache-2.0 WITH LLVM-exception |

Seven package/version license rejections were observed. No per-crate allowlist,
license clarification hash, private-workspace exemption or changed source policy
was added to hide them. Review the existing generated third-party notices and
source/distribution obligations before proposing any policy amendment.

## npm advisory — visible below the blocking threshold

Both base and head reported one low-severity KaTeX finding:
[GHSA-238p-pmpm-9mq7](https://github.com/advisories/GHSA-238p-pmpm-9mq7),
existing prototype pollution can bypass trust restrictions. The audit's affected
range is 0.11.0–0.18.1; its proposed remediation is KaTeX 0.19.1, a major upgrade.
High=0, critical=0; introduced/changed=0. The npm gate passes its documented high
threshold; this is **not** a clean vulnerability inventory. No upgrade performed.

### Update 2026-11-18 — new high advisory, no patched version exists

The npm gate now **fails** at its documented `high` threshold on
[GHSA-fc5q-39xc-vjpf](https://github.com/advisories/GHSA-fc5q-39xc-vjpf) (picomatch ReDoS through
single-branch repeated extglobs, published 2026-10-19). It reaches `fdir`, `tinyglobby` and `vite` through the
lock (`picomatch 4.0.7`), giving 4 high findings; the KaTeX low finding is unchanged. This is not introduced by
this PR: main carries the same lock. The advisory range is `<= 4.0.7` with **no first patched version**, 4.0.7
is the latest release, and the current `vite` 8.x, `tinyglobby` and `fdir` all still depend on `picomatch ^4`.
`npm audit`'s only suggestion is a `vite` major downgrade, which does not remove the vulnerable package either.
There is therefore no dependency change that resolves it today. The owner explicitly authorized (2026-11-18) a
**time-boxed acceptance**, not a suppression: `scripts/supply-chain/npm-accepted-advisories.json` names exactly this
advisory URL on `picomatch`, valid only while npm still reports no fixed range (`*`), up to `high`, scope
`build-time-dependency`, **expiring 2026-12-18**. The receipt keeps the raw `FAIL` and the four accepted findings;
transitive packages (`fdir`, `tinyglobby`, `vite`) are covered only while every link of their chain is covered. Any
other high finding, a different advisory, a published fix, an expired or malformed entry makes the job fail again.
This is not an independent review and not an assertion that the package is safe; it only lets the unrelated
supply-chain gate be read for its other findings. Removal on upstream patch or at expiry is required.

## CodeQL — untriaged findings in unchanged source

The first JS/TS `security-extended` scan reported eight findings at checkpoint
`925b962a72d0dd5b748398860cb06d2a09faf00f`:
[run 43679567912](https://github.com/KaisAbiyyi/mind-place/actions/runs/43679567912),
artifact `14147682497`. SARIF was inspected as data. Every cited file is
byte-identical to the frozen base. These are existing-code findings requiring
triage, not eight confirmed exploitable product vulnerabilities. No CodeQL query,
test directory, result or suppression was excluded to obtain green CI.

| Rule | Existing location |
| --- | --- |
| js/incomplete-url-substring-sanitization | test/import-sanitizer.test.mjs:839 |
| js/file-system-race | scripts/benchmark-continuity.mjs:185 |
| js/file-system-race | scripts/generate-notices.mjs:25 |
| js/file-system-race | scripts/import-sanitizer/sanitize.mjs:102 |
| js/file-system-race | scripts/tunnel-helper.mjs:14 |
| js/file-system-race | scripts/tunnel-helper.mjs:27 |
| js/file-system-race | test/tunnel-package.test.mjs:21 |
| js/file-system-race | test/import-sanitizer.test.mjs:1412 |

Python analysis completed with no findings at that checkpoint. Rust analysis was
interrupted by a subsequent checkpoint; its final result must be read from the
final-head run in the PR. Neither cancellation nor an unfinished scan is PASS.
Findings anywhere, including test code, continue to block the configured CodeQL
gate until independently disposed in a separately authorized change.

## Scorecard — existing default-branch posture, not a PR verdict

[Collection run 43679747900](https://github.com/KaisAbiyyi/mind-place/actions/runs/43679747900)
completed using read-only access. Significant raw results included Branch-Protection
0, Code-Review 0, Dependency-Update-Tool 0, Fuzzing 0, SAST 0 and Signed-Releases 0;
Token-Permissions and Dangerous-Workflow scored 10. Packaging was **-1**, with
reason “packaging workflow not detected”; it is unknown, not a packaging failure
or PASS. The report also lists five existing vulnerabilities from its own source.
Those counts are not substituted for cargo-deny/npm audit results.

The default branch does not contain this unmerged PR's workflows/Dependabot config.
Scorecard's review/maintainer metrics do not override the owner's single-contributor
manual-merge policy. No repository protection, release signing, extra contributor,
external badge or broader token access was activated in response to these scores.

## Implementation regressions and next steps

The initial implementation had a wrong CycloneDX output-name assumption and a
shellcheck SC2016 warning. Both were corrected in `73e97bd`; actionlint/policy and
the SBOM workflow then passed, producing eight Rust BOMs, the 138-component npm
BOM, the helper-component BOM and source/hash manifest. These were implementation
defects, distinct from the pre-existing dependency/source findings above.

Independent security review remains pending. Final exact-head tests and failures
must be linked in PR #178 and Notion before any acceptance decision. Existing
findings stay blocking; no main merge, self-approval, release publication, unrelated
upgrade or UI2/MP-9C change is authorized by this work.

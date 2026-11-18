# MP-SEC-12 — independent review disposition and remediation decisions

Review: [comment 6889781163](https://github.com/KaisAbiyyi/mind-place/pull/178#issuecomment-6889781163).
Reviewed source: `4e5d85c397d50ce6b80033806047d8f305c14f7e`; main:
`fa2e6b7e8b8ce919067e6e0def28032261646050`. Continuation is authorized by the
owner, under the same `mp-sec-12-20261115-codex-01` / `ci-supply-chain` claim.
This document is implementer triage, **not independent approval, accepted risk,
release authorization or a waiver of any red check**. Final exact-head evidence
is recorded in PR #178, without a self-referential source-SHA claim in this file.

## Review findings and implemented fixes

The independent reviewer verified that both original Rust diagnostic streams were
identical (40,641 bytes), all eight original JS results named unchanged source,
and all ten SBOM document hashes/counts matched. That supports the separation of
baseline findings from this PR's code changes; it does not clear the baseline.

| Review item | Decision and action |
| --- | --- |
| Four RustSec advisory findings | Per-advisory platform, function and upgrade-cohort triage below; retain every failure; follow-ups #179/#180 |
| Seven license-policy rejections | Distribution/notice decision with exact package terms; keep policy unchanged; follow-up #181 |
| Eight JS results | Individual data-flow/threat/ownership triage below; no query/path exclusions or accepted baseline; follow-up #182 |
| ID-only Rust comparison can miss changed affected versions | Fixed in the owned audit wrapper: identity includes code, affected package/version, rejected terms and inclusion-graph hash; schema 2 reports existing, introduced/changed and resolved findings |
| Rust tool failure must not resemble a clean audit | Fixed: nonzero exit without policy diagnostics is BLOCKED; contradictory successful exit plus error diagnostics fails closed; raw diagnostic hashes retained |
| Need reproducible source-level JS triage | Implemented committed-blob SHA-256 proofs for every SARIF location at base and head, with rule/message/line; path/URI validation prevents reading arbitrary local files; equality never waives a result |
| Need target evidence without hiding other platforms | Implemented Windows all-feature Cargo-resolution presence and an example path for every affected package; explicitly not runtime reachability or native proof; all-target gate unchanged |
| Findings/provenance must survive a red CodeQL gate | Moved artifact retention after the unchanged fail-on-any-finding gate with `if: always()`; retains SARIF and `triage.json` even when JS fails |
| Required-check enforcement and release linkage absent | Explicit owner decisions and acceptance contracts below; no settings or production release workflow changes |

Only supply-chain scripts/tests, their new CodeQL workflow and supply-chain docs
change in this continuation. No dependency manifest/lock, `deny.toml`, sanitizer,
helper/packaging runtime, storage/MP-9C, UI2 or original production workflow is
edited. Active claims were re-read before editing. The sanitizer has an existing
claim; MP-9C and MP-9E retain privacy-deletion/backup ownership. The follow-up
issues below are proposed/unclaimed and do not expand Now or authorize work.

## RustSec decisions

Primary advisory evidence is frozen at
[`RustSec/advisory-db` 4ef2df8](https://github.com/RustSec/advisory-db/tree/4ef2df8e806a65e2f1ecc14f56f51249062b23d6).
Reproduce target evidence with `cargo metadata --locked --all-features
--format-version 1 --filter-platform x86_64-pc-windows-msvc` and traverse the
workspace's resolved dependency edges, not merely the packages list. The audit
wrapper now emits that traversal. Target absence is **not** an ignore rule.
First-party searches found no direct VariantStrIter or rustix xattr call sites;
this search cannot establish transitive non-reachability or immunity.

| ID / affected package | Evidence and exposure decision | Remediation decision |
| --- | --- | --- |
| [RUSTSEC-2026-0502](https://github.com/RustSec/advisory-db/blob/4ef2df8e806a65e2f1ecc14f56f51249062b23d6/crates/sqlcipher-src/RUSTSEC-2026-0502.md), sqlcipher-src 419.0.6 | Resolves for Windows through mind-place-state → rusqlite 0.41.0 → libsqlite3-sys 0.39.0. Desktop manifest explicitly enables SQLCipher features. Upstream describes crafted local DB/WAL memory disclosure, CVE-2026-119375. No end-to-end exploit reproduction or safe-path exemption claimed. **P1 release hold.** | [#179](https://github.com/KaisAbiyyi/mind-place/issues/179): coordinated storage cohort upgrade. RustSec lists >=420.0.0, while current libsqlite3-sys bundled-SQLCipher path pins =419.0.6; do not force one lock entry or upgrade SQLCipher here. Requires owner scope plus MP-9C/MP-9E/storage/Cargo.lock coordination, encrypted Windows/Ubuntu migration/read/write/FTS/backup/restore/recovery qualification and independent review. |
| [RUSTSEC-2026-0486](https://github.com/RustSec/advisory-db/blob/4ef2df8e806a65e2f1ecc14f56f51249062b23d6/crates/rustix/RUSTSEC-2026-0486.md), rustix 1.1.5 | Not resolved by the inspected Windows target. Affected Unix xattr functions can initialize unwritten bytes for zero-length buffers. The all-target graph remains affected. | [#180](https://github.com/KaisAbiyyi/mind-place/issues/180), bounded Unix patch unit: evaluate >=1.1.6 with Rust 1.90 and existing consumers; serialize shared Cargo.lock with storage claims. No upgrade performed or advisory suppressed. |
| [RUSTSEC-2024-0429](https://github.com/RustSec/advisory-db/blob/4ef2df8e806a65e2f1ecc14f56f51249062b23d6/crates/glib/RUSTSEC-2024-0429.md), glib 0.18.5 | Not resolved by inspected Windows target; GTK desktop dependency cohort on other targets. Upstream identifies VariantStrIter iterator UB; patched >=0.20.0. | [#180](https://github.com/KaisAbiyyi/mind-place/issues/180), GTK/Tauri cohort unit: review compatible downstream versions and native Linux behavior, plus Windows regressions. A blind glib major override is not justified. |
| [RUSTSEC-2024-0370](https://github.com/RustSec/advisory-db/blob/4ef2df8e806a65e2f1ecc14f56f51249062b23d6/crates/proc-macro-error/RUSTSEC-2024-0370.md), proc-macro-error 1.0.4 | Not resolved by inspected Windows target; existing GTK macro/build graph. Upstream lists no patched version. Unmaintained is a maintenance/supply-chain finding, not a reproduced runtime exploit. | [#180](https://github.com/KaisAbiyyi/mind-place/issues/180), migrate the dependent macro cohort to a maintained alternative after API/MSRV tests; no package-name substitution or `unmaintained = none`. |

## Seven license-policy rejections

[Follow-up #181](https://github.com/KaisAbiyyi/mind-place/issues/181) is an owner
distribution-policy/notice review, unclaimed and not accepted. An SPDX term is
evidence about terms, not proof that distribution obligations are satisfied or
that every such dependency is incompatible with an MIT application.

| Exact package | Resolved for inspected Windows target | Decision / required evidence |
| --- | --- | --- |
| cssparser 0.37.0 | Yes, Tauri utilities → dom_query | MPL-2.0: inspect source-availability and notice obligations for actual distribution, then explicit owner decision; no allowance added |
| cssparser-macros 0.7.1 | Yes, through cssparser | Same family review; retain separate exact package/version evidence |
| dtoa-short 0.3.5 | Yes, through cssparser | Same family review |
| option-ext 0.2.0 | Yes, Tauri → dirs → dirs-sys | Same family review; this is not merely an unused lock entry |
| selectors 0.38.0 | Yes, through dom_query | Published source has MPL header and metadata; inspected crate has no root LICENSE text. Verify complete source and packaged notices rather than declaring notice completeness from metadata |
| sqlcipher-src 419.0.6+sqlcipher-4.19.0-sqlite-3.53.4-libtomcrypt-1.18.2 | Yes, bundled storage build | Composite `MIT AND BSD-3-Clause AND blessing AND WTFPL`: wrapper's root LICENSE text is MIT and cannot substitute for all bundled C/component terms; inspect complete bundled provenance and installed notices with storage upgrade coordination |
| target-lexicon 0.12.16 | No, absent from inspected Windows graph | `Apache-2.0 WITH LLVM-exception`: separately review exact exception text/notice requirements for supported all-target builds; source was not in local Windows cache, so local full-text review is NOT_RUN |

Reproducibility anchors from inspected registry source (SHA-256): cssparser and
cssparser-macros LICENSE `fab3dd6bdab226f1c08630b1dd917e11fcb4ec5e1e020e2c16f83a0a13863e85`;
dtoa-short LICENSE `1f256ecad192880510e84ad60474eab7589218784b9a50bc7ceee34c2b91f1d5`;
option-ext LICENSE.txt `66a3107d5ad6a058aab753eaac2047ccb2ed0e39465dd0fe5844da3e300d5172`;
SQLCipher wrapper LICENSE `413f82e0f4f8191a8624a7b7b2c9809466f7f3d930ad52a74ad29f2dd907881d`.
These hashes identify inspected evidence; they are **not** cargo-deny clarification
hashes, legal acceptance or replacements for published crate checksums/notices.
The policy's seven rejections remain blocking; no allowlist is changed.

## Eight CodeQL JS decisions

All locations below are those in the independently inspected original SARIF.
The new artifact binds current source bytes to base/head Git blobs and retains
every finding. Unchanged source does not establish exploitability, safety or an
automatic accepted disposition. [#182](https://github.com/KaisAbiyyi/mind-place/issues/182)
contains the unclaimed, separately scoped follow-up work packages.

| Rule and original location | Evidence-backed triage | Safe remediation / ownership decision |
| --- | --- | --- |
| URL-substring, test/import-sanitizer.test.mjs:839 | The expression is Array.includes on `residualMetadata.linkHosts`, not String.includes on a URL. `rules.mjs` residualReport builds this field from a Set using `[...set].sort()`; host insertion uses exact KEEP_HOSTS membership. It is a fixture assertion, not a URL authorization branch. **Candidate scanner false positive; independent disposition required.** | No test rewrite merely to hide the result. Sanitizer owner may make the data type explicit and add adversarial host-label assertions; independent reviewer must accept the disposition. Query remains enabled and result still fails. |
| Filesystem race, scripts/benchmark-continuity.mjs:185 | Owner-selected `--predictions FILE` CLI path: stat size check precedes a separate path read. File replacement/growth can bypass the intended 2 MiB read budget. No remote entry point or exploit is claimed. | Separate benchmark-file scope: open once, check descriptor regular-file metadata, bounded read of limit+1, handle growth/replacement/special-file cases with deterministic injection tests. Do not fix with a second pathname stat. |
| Filesystem race, scripts/generate-notices.mjs:25 | Build-time dependency/license paths from Cargo metadata and installed npm packages; stat limit then path read can be swapped or enlarged by a concurrent cache/dependency-tree writer. Potential packaging provenance/resource issue, not a demonstrated remote product exploit. | Separate packaging/notices scope: descriptor-bound bounded license reads and directory/symlink handling; prove generated notice equivalence and race rejection. This PR's ci-supply-chain claim does not own release notices. |
| Filesystem race, scripts/import-sanitizer/sanitize.mjs:102 | Input lstat/type/link check precedes open; directory entries also defer pathname opens. Owner-supplied input is the entry point; concurrent replacement can invalidate link/type checks. A no-follow flag alone does not protect intermediate Windows ancestors. | Coordinate existing MP-IMPORT-SANITIZE claim. Freeze source descriptors and source/ancestor identity; test swaps at deterministic boundaries, no disclosure/publication after failure, and actual Windows junction/symlink behavior. No sanitizer source change here. |
| Filesystem race, scripts/tunnel-helper.mjs:14 | lstat/type/size check and hash read use separate pathname operations. Hash validation still anchors expected bytes, but size/type and subsequent copy are not descriptor-bound. Concurrent replacement is the threat, not a shown network checksum bypass. | Separate helper/packaging scope: one handle for regular-file/size/hash, bounded reads, and verify final exclusively staged copy; include malicious cache replacement tests and native Windows reparse considerations. Keep all existing hashes and receipts. |
| Filesystem race, scripts/tunnel-helper.mjs:27 | Receipt is lstat'ed twice then read by path; bundle verification is followed later by copy/staging. Parent-directory and post-verification mutation must be considered. | Same helper unit; bind inventory, resource bytes and receipt consistently through staging, verify destination and preserve corrupt-cache rejection. No assumption that two path stats close a race; no helper runtime change here. |
| Filesystem race, test/tunnel-package.test.mjs:21 | After a synthetic download's nonexistence assertion, the test writes a stale-cache fixture inside mkdtemp and exercises repair. This is test setup, not shipped acquisition logic. **Candidate test-only finding**, subject to independent review and hostile concurrent-writer model. | Helper test owner may separate fixture setup from verification or use exclusive fixture creation with a collision test; do not remove the corrupt-cache assertion or suppress the whole test tree. |
| Filesystem race, test/import-sanitizer.test.mjs:1412 | Synthetic failure-path test asserts preserved report/output, then removes its own fixture report. It is cleanup in a temporary fixture, not production delete authority. **Candidate test-only finding**, not an approved waiver. | Existing sanitizer owner to review private-fixture custody and robust cleanup; retain rollback/preservation assertions and collision tests. No fenced test edit here. |

The current npm low KaTeX finding is also retained; this continuation does not
change the justified high/critical threshold or perform an unrelated upgrade.

## Integration status after main `43763a4` (2026-11-18)

This PR was reconciled with main `43763a4aeda4729481004153f372ca7363cf6cdf` (clean merge, no textual conflict, no
policy/lock/dependency change in this PR). Remediations already accepted on main are **not** redone here:

| Item | State on main `43763a4` | What remains |
| --- | --- | --- |
| RUSTSEC-2026-0486, rustix | `Cargo.lock` locks `rustix 1.1.6` (PR #189). The decision row above that says "not resolved" is historical and superseded. | Nothing in this PR. The gate still compares and reports exactly. |
| CodeQL (JS/TS, Rust, Python) | Source remediations merged through #183, #186 and #187. On `396d7c2`, CodeQL `javascript-typescript`, `rust`, and `python` each passed with `findings: 0` in `triage.json` (run `44263381909`). | Re-verify exact-head CodeQL on every subsequent head; if any finding appears, it stays red. |
| RUSTSEC-2026-0502, sqlcipher-src `419.0.6` | Still locked. | [#179](https://github.com/KaisAbiyyi/mind-place/issues/179): storage compatibility cohort and an owner-assigned `Cargo.lock` window with #151/#152. Not changed here. |
| RUSTSEC-2024-0429 glib `0.18.5`, RUSTSEC-2024-0370 proc-macro-error `1.0.4` | Still locked (GTK/Tauri macro cohort). | [#180](https://github.com/KaisAbiyyi/mind-place/issues/180) unit 2, serialized with other `Cargo.lock` changes. Not changed here. |
| RUSTSEC-2026-0543, chrono `0.4.45` (`unmaintained`) | Still locked (observed in RustSec DB `233155b4025b854c68a4b199c0e32d7fd53291a2` on both `base` and `head` at `396d7c2`). Resolves for Windows via `mind-place-archive` and `mind-place-index` (`Cargo.toml` workspace dependency; `targetPresence.resolvedForTarget: true`). Upstream advisory (`2026-09-14`) marks all `chrono` versions unmaintained with no safe upgrade and recommends `jiff` or `time`. | Separate migration unit (replacing `chrono` usage in `crates/archive-core` and `crates/index-core` with `jiff` or `time` under a serialized `Cargo.toml`/`Cargo.lock` window with #151/#152/#179/#180) or explicit owner advisory policy decision. No `deny.toml` ignore or `unmaintained` relaxation added here. |
| Seven license rejections | `deny.toml` unchanged; no allowlist edit. Packaged `selectors` MPL-2.0 and SQLite `blessing`/notice texts are now merged on main via [#193](https://github.com/KaisAbiyyi/mind-place/pull/193) (`43763a4`). | [#181](https://github.com/KaisAbiyyi/mind-place/issues/181) owner distribution-policy decision (whether to explicitly allow `MPL-2.0`, SQLite `blessing`/`WTFPL`, and `Apache-2.0 WITH LLVM-exception` in `deny.toml`, or replace/isolate the crates). |

The Rust advisory and license jobs remain expected to fail until the items above are resolved by their owners. A red
job is never reclassified as PASS and a skipped job is never counted as PASS.

## Required-check enforcement — owner decision, MP-GOV-04

On 2026-11-15, read-only API inspection returned no repository rulesets; the
classic branch-protection endpoint returned **403, resource not accessible by
integration**. Therefore exact effective required-check enforcement is NOT
VERIFIED, not inferred from CI success or from the empty ruleset list alone.
No settings mutation or token escalation was attempted.

Owner task [MP-GOV-04 / #139](https://github.com/KaisAbiyyi/mind-place/issues/139)
must decide and apply the final check set/source identity after remediation and
independent review. Actual observed check-run names from GitHub Actions include:
`Supply-chain policy and negative tests`, `npm high and critical vulnerabilities
(all dependencies)`, `Rust advisories licenses and sources`, `CodeQL
javascript-typescript`, `CodeQL rust`, and `CodeQL python`, alongside the existing
production/native checks. Bind to the intended GitHub Actions app/source, verify
fresh exact-source status, and preserve no-bypass/manual-owner-merge policy.

Decisions to record explicitly: exact protected branch/tag/ref scope; which
checks are mandatory; whether CodeQL artifact-only operation is the intended
mode (current mode needs no security-events write); treatment of unsupported
visibility/service failures as blocked; and negative enforcement tests for
failed/missing/stale checks and a new push. Do not require the path-filtered SBOM
PR job indiscriminately without first defining an always-produced status or a
release-only requirement. Scorecard collection is not an acceptance threshold.
Do not relabel a skipped Draft native wrapper as an executed native check.

## Release SBOM linkage — owner decision, MP-REL-01

Existing release.yml can build/upload independently of red supply-chain/CodeQL
checks. Current release-sbom.yml produces a separate 90-day source inventory,
not an attached installer inventory or an operational release-admission barrier.
This boundary remains intentional until the owner authorizes MP-REL-01's release
workflow/signing/artifact-linkage scope. No production release YAML changes here.

Required design decision and qualification contract for that separate unit:

1. Freeze one tag/commit/target, validate package version and lock hashes, run
   required source/native gates, and make package promotion depend on all of them.
   A red/unavailable scan or an expired/missing qualification must block promotion.
2. Generate source SBOMs at that same frozen source. Prefer an explicit same-run
   dependency chain with read-only build jobs; no privileged workflow_run consumer
   of arbitrary PR artifacts and no mutable branch resolution during promotion.
3. Bind the installer/portable SHA-256, SBOM manifest/document hashes, source SHA,
   tag, target, feature set, run/attempt and qualified evidence in one receipt.
   Confirm the bundled helper bytes/resources actually match their pin. Source
   SBOMs alone do not inventory OS/WebView2 or attest final packaged bytes.
4. Stage the agreed SBOM/manifest/receipt/checksums beside the exact installer and
   portable artifacts. Decide durable retention/owner publication; do not rely on
   an independently expiring 90-day artifact as permanent release provenance.
5. Independently test wrong SHA/tag/target/lock hash, missing or tampered BOM,
   mismatched installer/helper digest, failed gate, stale run and expired artifact
   rejection. Owner acceptance/publication remains separate; no release published
   by this continuation. Signing/attestation credentials need separate approval.

## Re-review contract

Review the new exact head, raw schema-2 Rust comparison and Windows presence
evidence, SARIF plus triage.json and the per-finding decisions above. Verify that
the diagnostic identity regression tests catch changed package versions/terms/
paths and that source-equality tests never turn a finding green. All gate failures
remain failures. Proposed follow-ups #179–182 are unclaimed, unaccepted and do
not modify Now priorities. Required-check and release-linkage work remains with
the owner-governed tasks. No merge, self-approval or release publication.

# MP-SEC-12-R4 — Independent CodeQL JavaScript Triage and Bounded Remediation

- **Tracking issue:** [#182](https://github.com/KaisAbiyyi/mind-place/issues/182)
- **Claimed task:** `MP-SEC-12-R4` (`mp-sec-12-r4-20261115-antigravity-50b7b167`)
- **Upstream scan source:** PR [#178](https://github.com/KaisAbiyyi/mind-place/pull/178) (`work/mp-sec-12/supply-chain-gate`, left untouched), HEAD `cfb2c1367b5396c638d8e97be77ae11dac1476c3`, base `fa2e6b7e8b8ce919067e6e0def28032261646050`
- **Independent SEC-12 re-review reference:** [PR #178 comment #6892142719](https://github.com/KaisAbiyyi/mind-place/pull/178#issuecomment-6892142719)
- **Downloaded SARIF / provenance source:** GitHub Actions run `43682564065`, artifact `14148826909` (`codeql-javascript-typescript-cfb2c1367b5396c638d8e97be77ae11dac1476c3`, `javascript.sarif` and `triage.json`); Rust (`codeql-rust-...`) and Python (`codeql-python-...`) scans reported zero findings.

## Scope, Resource Locks, and Policy Invariants

- **Available / Claimed in this unit:**
  - `scripts/benchmark-continuity.mjs` (`MP-10A` is `Done`, `benchmark-gold` lock released, zero open competing PRs)
  - `test/continuity-benchmark.test.mjs`
  - `docs/security/mp-sec-12-r4-codeql-js-triage.md` (this triage record)
- **Deferred without modification until explicit authorization / lock release:**
  - **Import sanitizer (`MP-IMPORT-SANITIZE-01` Review lock):** `scripts/import-sanitizer/sanitize.mjs:102`, `test/import-sanitizer.test.mjs:839`, `test/import-sanitizer.test.mjs:1412`
  - **Helper & packaging (`MP-REL-01` / release-notices & helper verification scope):** `scripts/tunnel-helper.mjs:14`, `scripts/tunnel-helper.mjs:27`, `test/tunnel-package.test.mjs:21`, `scripts/generate-notices.mjs:25`
  - **Supply-chain gate (`ci-supply-chain` lock / PR #178):** `.github/workflows/codeql.yml`, `scripts/supply-chain/**`, and `docs/security/mp-sec-12-*.md` on PR #178
- **No-suppression invariant:** Zero CodeQL suppressions, zero path/test-directory exclusions, zero query weakening, zero SARIF waivers, and zero edits to PR #178.

## Independent Triage of All Eight CodeQL JavaScript Findings

### Summary by Category

| Category | Count | Locations |
| --- | ---: | --- |
| Confirmed vulnerabilities / TOCTOU bugs | 5 | `scripts/benchmark-continuity.mjs:185` (Fixed here), `scripts/import-sanitizer/sanitize.mjs:102`, `scripts/generate-notices.mjs:25`, `scripts/tunnel-helper.mjs:14`, `scripts/tunnel-helper.mjs:27` |
| False-positive candidate | 1 | `test/import-sanitizer.test.mjs:839` |
| Test-only findings | 2 | `test/tunnel-package.test.mjs:21`, `test/import-sanitizer.test.mjs:1412` |

### Per-Finding Disposition

| # | Location | Rule ID | Classification | Action / Lock Status | Technical Basis & Next Requirement |
| ---: | --- | --- | --- | --- | --- |
| 1 | `scripts/benchmark-continuity.mjs:185` | `js/file-system-race` | **Confirmed local CLI TOCTOU / budget-bypass bug** | **Fixed in this PR** (available `MP-10A` Done scope) | `statSync(path).size <= 2 * 1024 * 1024` ran before separate pathname `readFileSync(path, 'utf8')` without `isFile()` or chunk-limit enforcement. Remediated via single `openSync` (`O_RDONLY` + `O_NOFOLLOW`/`O_NONBLOCK` when defined), `fstatSync(fd, { bigint: true }).isFile()`, initial size cap, post-open leaf identity / non-symlink check when `O_NOFOLLOW` is absent (Windows), and bounded `64 KiB` `readSync` chunks enforcing the 2 MiB ceiling across concurrent file growth. |
| 2 | `scripts/import-sanitizer/sanitize.mjs:102` | `js/file-system-race` | **Confirmed local TOCTOU vulnerability** | **Deferred** (`MP-IMPORT-SANITIZE-01` lock held) | `fs.promises.lstat(input)` at line 93 precedes pathname-based `fs.promises.open(input, 'r')` at line 102 and `fs.createReadStream` at lines 83 and 118. Requires coordinated `MP-IMPORT-SANITIZE-01` authorization and Windows/Unix parent/ancestor identity fencing (`O_NOFOLLOW` alone does not prevent Windows parent-directory junction races). |
| 3 | `scripts/generate-notices.mjs:25` | `js/file-system-race` | **Confirmed build/packaging TOCTOU / unbounded-read bug** | **Deferred** (packaging / release-notices scope) | `fs.statSync(file).size > 1024 * 1024` precedes separate pathname `fs.readFileSync(file, 'utf8')` during third-party notice collection invoked by `scripts/tauri.mjs:57` and desktop build/CI workflows. Needs descriptor-bound regular-file bounded read with growth/swap tests under packaging owner authorization. |
| 4 | `scripts/tunnel-helper.mjs:14` | `js/file-system-race` | **Confirmed helper/packaging TOCTOU bug** | **Deferred** (helper / packaging scope) | `verifyPinnedFile` runs `fs.lstatSync(file)` at line 12 before pathname `fs.readFileSync(file)` at line 14. Although SHA-256 verifies the read bytes, read/hash/stage operations are not bound to an opened regular-file handle. Needs descriptor-bound verify/copy and post-staging verification while preserving helper pins/receipts. |
| 5 | `scripts/tunnel-helper.mjs:27` | `js/file-system-race` | **Confirmed helper/packaging TOCTOU bug** | **Deferred** (helper / packaging scope) | `verifyHelperBundle` calls `fs.lstatSync(receiptPath)` twice at line 27 before pathname `fs.readFileSync(receiptPath, 'utf8')` at line 29. Needs descriptor-bound receipt verification under helper/packaging authorization. |
| 6 | `test/import-sanitizer.test.mjs:839` | `js/incomplete-url-substring-sanitization` | **False-positive candidate** | **Deferred** (`MP-IMPORT-SANITIZE-01` test scope + independent disposition) | Expression is `gemini.report.residualMetadata.linkHosts.includes('gemini.google.com')`. In `scripts/import-sanitizer/rules.mjs:279,470`, `residualMetadata.linkHosts` is constructed via `KEEP_HOSTS.has(host)` as an `Array<string>` of normalized exact hostnames, not a URL string substring check and not a security authorization decision. Requires independent disposition review (or clearer test helper name if authorized) without query exclusion. |
| 7 | `test/tunnel-package.test.mjs:21` | `js/file-system-race` | **Test-only finding** | **Deferred** (helper test scope) | Line 19 checks `assert.equal(fs.existsSync(destination), false)` before line 21 `fs.writeFileSync(destination, 'stale')` in a temporary directory created via `fs.mkdtempSync`. Not a shipped runtime path; can be simplified to assert clean setup without a pre-write existence race check when authorized. |
| 8 | `test/import-sanitizer.test.mjs:1412` | `js/file-system-race` | **Test-only finding** | **Deferred** (`MP-IMPORT-SANITIZE-01` test scope) | Line 1390 checks `assert.equal(fs.existsSync(staged), false, ...)` before line 1412 `fs.writeFileSync(staged, 'OWNER FILE')` inside a synthetic temporary fixture directory (`reportFixture`). Not a shipped runtime path; defer cleanup to coordinated `MP-IMPORT-SANITIZE-01` test maintenance. |

## Platform & Qualification Notes

- **Bounded Windows vs POSIX open semantics (`scripts/benchmark-continuity.mjs`):**
  - On POSIX platforms defining `fs.constants.O_NOFOLLOW` and `fs.constants.O_NONBLOCK`, `openSync` refuses final-component symlinks atomically (`ELOOP`) and avoids FIFO open blocking; `fstatSync(fd, { bigint: true }).isFile()` then rejects directories/special files, and `readSync(fd, ...)` reads strictly from the opened descriptor up to `2 * 1024 * 1024` bytes.
  - On Windows (`fs.constants.O_NOFOLLOW` is `undefined`), `node:fs` does not expose Win32 `FILE_FLAG_OPEN_REPARSE_POINT`, so `readSmallJson` opens once, checks `fstatSync(fd, { bigint: true })` (`isFile()` and size), verifies post-open leaf `!isSymbolicLink()` and `(dev, ino)` match via `lstatSync` before reading any chunk, and enforces the 2 MiB ceiling across chunks read from `fd`.
- **Hosted CodeQL visibility on `main`:** `.github/workflows/codeql.yml` currently resides on draft PR #178 (`work/mp-sec-12/supply-chain-gate`), not `origin/main` (`fa2e6b7e8b8ce919067e6e0def28032261646050`), and the `codeql` CLI is not installed on the local qualification host. Live CodeQL verification of the remediated finding (`scripts/benchmark-continuity.mjs`) and the seven deferred findings requires running the PR #178 CodeQL workflow against the merged/rebased commit once authorized.

# MP-IMPORT-SANITIZE-01 — structure-preserving sanitizer for real ChatGPT/Gemini exports

Status: **READY FOR REVIEW**. Neither acceptance nor a merge request. Only the repository owner merges.

- Repository: `KaisAbiyyi/mind-place`. Base: `main` `06b6199`, built on the same branch as MP-IMPORT-FIX-01 (`agent/lucid-euler-l3lopd`) as **separate commits** that touch no FIX-01 file, so they can be cherry-picked on their own.
- Executor: Claude Code, cloud Linux x86_64. **Synthetic exports only.** No real export was requested, read or pasted; nothing in this task touched private content.
- Claim: single-writer on the logical resource `new import sanitizer tool`. Notion card claimed 2026-11-11.
- Cross-resource note: `crates/archive-core/tests/import_sanitizer_compat.rs` is a **new** file in the archive-core tests directory (a FIX-01 resource). It only reads committed fixtures and calls public parser APIs; no archive-core source or existing test changed.

## What was built

`scripts/import-sanitizer/` — a dependency-free Node tool (`node scripts/import-sanitizer/cli.mjs`, or `npm run sanitize:import`). Node rather than a new Rust crate because the repository already ships Node tooling, it needs a zip reader/writer, and adding Rust dependencies would add third-party notice obligations to a tool that is not part of the product. Usage, the keep/replace table and limits are in [`scripts/import-sanitizer/README.md`](../../scripts/import-sanitizer/README.md).

| Module | Role |
| --- | --- |
| `zip.mjs` | Dependency-free zip reader (CRC/size verified while streaming, ZIP64) and deterministic writer (fixed timestamps, input order, ZIP64 counts/offsets) |
| `json-stream.mjs` | Streaming JSON tokenizer and rewriter: no document is built; layout, key order and number formats are kept byte-for-byte; nesting depth and token size are budgeted |
| `html-stream.mjs` | Streaming HTML scanner and rewriter that keeps tags, attribute names, quoting style, case and comment boundaries; independent of read chunking |
| `rules.mjs` | Default-deny rules: enum allow-list, same-shape keyed pseudonyms, length-class filler, URL/timestamp/name handling, media stand-ins |
| `budgets.mjs` | Named resource budgets for every input-dependent structure, the `BudgetError` (exit 5) and the worst-case memory model |
| `leak-check.mjs` | 64-bit-hash registry of every input string and 8+ character token; no content stored or printed |
| `analysis.mjs` | Structure inventory, input-vs-output structure equality, independent leak scan |
| `sanitize.mjs`, `cli.mjs` | Orchestration (zip / directory / file), spooling, verification pass, atomic publish, report, exit codes |

## Acceptance criteria

| # | Criterion | Evidence |
| --- | --- | --- |
| 1 | Tests with synthetic ChatGPT and Gemini exports show structure preserved and a property-style leak check passing | `test/import-sanitizer.test.mjs`: ChatGPT multi-member zip (node/role/content-type/part counts, mapping keys equal ids, reciprocal parent/child links, branches, orphan, duplicate message ids, literal root, unknown fields, number formats as text); Gemini folder and zip (card, content-cell, `Prompted`, `<br>`, comment, `<p>`, `<img>`, uppercase tags, single-quoted and unquoted attributes, duplicate card); property test over 12 random exports with an **independent oracle** (does not use the tool's own registry); and `crates/archive-core/tests/import_sanitizer_compat.rs`, where the **real Rust parsers** give the same shape (variants, roles, completeness, warnings, attachments) for a synthetic original and its sanitized twin |
| 2 | Inventory works on an export with several `conversations-*.json` members and on a single-file export | `ChatGPT export with several conversations-*.json members…` (two members, 3 records each, 2 media stand-ins) and `single-file ChatGPT export…` (4 records, pretty-printed layout kept); both assert the report contains no planted string |
| 3 | Usage documented; running on a real export recorded as NOT_RUN | README, CLI epilogue ("record it as NOT_RUN until done") and this document: **real export run: NOT_RUN** |
| 4 | Repository baseline passes | see Verification |

Other requirements from the card: reproducible output (byte-identical zips across runs **with an owner-supplied key**; the default is a random key per run, see SAN-F03); no network (`the sanitizer tree is offline…` forbids network, process, eval and `console.*` APIs); no content logging (CLI test asserts stdout, stderr and the report contain none of the planted strings); a leak check that **fails the run** (fault-injection test: a rule that passes a title through makes the run exit 3 and leave no output, `.partial` file or spool); structure check that fails the run (fault-injection test, exit 4); stripped media (tiny valid stand-ins of the same type, media never read).

## Review findings that shaped the tool

1. **Leak registry scope.** My first design tracked only strings that rules chose to replace, so a rule that forgot a string would never be caught. It now tracks *every* input string and exempts only values an allow-list rule explicitly registers as kept. The fault-injection test failed before this change and passes after.
2. **Allow-list outputs must be registered.** The leak check flagged the URL scheme `file-service`, the data-URI stand-ins and coarsened timestamps because the rules emitted them without registering them. Every allow-list emission now goes through `kept()`.
3. **Reference integrity.** Mapping keys and the `id`/`parent`/`children` values referring to them were pseudonymized with different labels and no longer matched; fixed and covered by the reciprocal-link test.
4. **Fidelity.** End-tag case was normalized while start-tag case was kept; both are now preserved. Placeholder length classes round **down**, so the sanitized copy is never larger than the original (measured 151 MB in, 109 MB out).
5. **Transparency.** Enum-like values and known hosts are kept by design and exempt from the leak check, so the report now lists every kept enum value by key for the owner to review.

## Re-review round 1 — SAN-F01 … SAN-F05

Independent security review ([comment](https://github.com/KaisAbiyyi/mind-place/pull/166#issuecomment-6802603764)) found three P1 and two P2 issues in `bda9ad8`. All five are fixed with regression tests; the review's recommendation of a **separate sanitizer PR** is followed (PR #166 received only the FIX-01 commits).

| Finding | Correction | Regression test |
| --- | --- | --- |
| **SAN-F01 (P1)** `--force` could delete the original: `--input /exports/original.zip --output /exports --force` removed the output directory, which contained the input. | Overlap is checked on **canonical** paths (symlinks resolved in every existing component, case-folded on Windows/macOS) and refuses the input, anything inside it **and any ancestor of it**, before anything is created or deleted. Nothing is removed up front: the old output is moved aside, the verified replacement swapped in, and the old one removed last (restored if the swap fails). A directory is replaced only if this tool created it (marker file); symlinks and unrelated directories are refused. `--report` is checked the same way before the run. | `SAN-F01: an output that contains the input is refused…` (file under output dir, trailing slash, `..`, grandparent, symlinked output and symlinked input), `…--force never destroys the existing output unless the replacement is complete and verified` (leak failure keeps the old output; no `.partial` or aside copy), `…only replaces sanitizer output, never an unrelated directory or a symlink`, `…the report path cannot clobber the input or the output` |
| **SAN-F02 (P1)** Broad allow-lists: any regex-shaped `author.name`, enum-key value, class or attribute survived and was exempt from the leak check. | Default-deny vocabularies: enum values are kept only from a built-in provider vocabulary (roles, content types, statuses, known tool names, model-slug families, MIME types, …); `author.name` only for known tool names; HTML class tokens only from the Takeout/Material Design Lite vocabulary (others get a same-shape pseudonym); `lang`/`dir`/`charset`/`type`/`role`/`target`/`rel` only with known values. Replaced enum-shaped values are counted, never listed. Explicit, reviewed opt-in via `--keep-enum key=value` and `--keep-class`. | `SAN-F02: private author names, unknown enum values, custom classes and attributes are replaced, not kept` (private author name, `x-acme-secret-type`, custom `recipient`/`channel`/`language`/`mime_type`/model slug, custom classes, `lang="xx-Private"`, custom `role`, `title`, `data-*`, `id`), plus the opt-in path |
| **SAN-F03 (P1)** Public default HMAC key made low-entropy identifiers enumerable offline. | The default key is a fresh random 256-bit secret per run, never written, logged or reported. Reproducibility is an explicit owner opt-in (`--salt-file` / environment). The report records only `pseudonymKey: random-per-run` or `owner-supplied`. | `SAN-F03: the default pseudonym key is a fresh random secret…` (40 sequential identifiers; two runs differ; an attacker using the former public key finds zero matches; no original identifier, key or mapping in the output or report) |
| **SAN-F04 (P2)** A leak-tracking overflow was reported as `partial` coverage but still published. | Fail closed: `E_LEAK_COVERAGE`, exit 3, `checks.leak = INCOMPLETE`, no output. | `SAN-F04: if leak tracking overflows the run fails closed and publishes nothing` (via an internal `leakLimit` option) |
| **SAN-F05 (P2)** Structure fidelity was conflated with a privacy qualification; kept metadata was not reported as residual. | The report has `qualification` (`contentFreeAssertion: NOT_ASSERTED`, with an explicit meaning of PASS) and `residualMetadata` (member names, enum values, HTML class tokens and attribute values, link hosts, timestamp precision, structural labels). The CLI prints a **RESIDUAL METADATA** block and its help and the README say PASS is not an anonymity guarantee. | `SAN-F05: the report separates structure fidelity from privacy and lists the residual metadata` |

Also fixed while doing this: a class attribute made only of known tokens survived as a whole string that was not registered as kept (found by the leak check itself); the parser-compatibility fixtures were regenerated with a fixed test key and the Rust cross-check still passes.

Still **NOT_RUN** and still required by the review: running the CLI **on Windows** (directory, symlink/reparse-point and `--force` failure paths, zip fixtures; path handling is covered here only on Linux, with Windows-reserved-character and backslash-separator unit tests), the owner-local trial on a **real** export, and exact-head hosted CI. A reviewer should also decide whether the built-in vocabularies are the right size: a smaller vocabulary is safer and keeps less parser-relevant structure (for example a real class that the Gemini parser treats specially is replaced unless you pass `--keep-class`).

## Re-review round 2 — SAN-F06 (P2): unbounded cardinality memory growth

Finding ([comment](https://github.com/KaisAbiyyi/mind-place/pull/167#issuecomment-6810518849), reviewed head `b6ec07a`): the README said streaming memory is bounded by the largest single string, but `DistinctCounter`/`HashSet64`, `JsonStructure.ids`, `HtmlStructure.unknownClassTokens` and `Rules.shortIds`/`shortIdTaken` grow with the number of distinct identifiers or tokens, and only `LeakRegistry.protectedSet` had a cap. A crafted high-cardinality export could exhaust memory. All other findings of the previous round were confirmed addressed; the hosted CI run at that head was SUCCESS.

I audited every structure whose size depends on the input, not only the ones named. Besides the reviewer's list, these were also unbounded and are now budgeted: the leak registry's *kept* table, the JSON frame stack (nesting depth), JSON scalar tokens (only strings were capped), complete tokens that fit inside one read chunk (the cap was only enforced while a token was still incomplete; found by the new test), member and zip-entry counts (the whole zip central directory is read into memory), and the summaries kept across members.

| Structure | Budget (default) | On overflow |
| --- | --- | --- |
| leak registry protected table | `maxProtectedPieces` 8,000,000 | stops at the next member boundary: `E_LEAK_COVERAGE`, exit 3, `leak: INCOMPLETE` |
| leak registry kept table | `maxKeptPieces` 1,000,000 | `E_BUDGET`, exit 5 |
| `JsonStructure.ids` (input and output analysis) | `maxDistinctIdentifiers` 4,000,000 | `E_BUDGET`, exit 5 |
| `HtmlStructure.unknownClassTokens` | `maxUnknownClassTokens` 1,000,000 | `E_BUDGET`, exit 5 |
| `Rules.shortIds` / `shortIdTaken` | `maxShortIdentifiers` 100,000 and `maxShortIdentifierBytes` 8 MiB | `E_BUDGET`, exit 5 |
| members and zip entries | `maxMembers` 100,000 | `E_BUDGET`, exit 5 |
| summaries across members | `maxSummaryEntries` 500,000 | `E_BUDGET`, exit 5 |
| JSON nesting | `maxJsonDepth` 256 | `E_BUDGET`, exit 5 (previously a per-member stand-in) |
| one token / text node / tag | `maxTokenChars` 32 Mi chars | `E_BUDGET`, exit 5 (previously a per-member stand-in) |

A budget stop never publishes, never touches a previous output, never quotes input, and removes every spool, partial and aside file. Depth and token overflows are deliberately whole-run failures rather than quiet stand-ins.

**Upper bound, demonstrated.** `totalUpperBoundBytes()` models the worst case if every budget were reached simultaneously: under 1 GiB (asserted in a test). Measured locally: ordinary 151 MB export 140 MiB peak RSS; 128 MB export with 1,000,000 distinct identifiers passes at 208 MiB; a 576 MB crafted export with 4,500,000 distinct identifiers is stopped at the 4,000,000 budget at 327 MiB with the previous output intact. These are local one-off observations, not CI measurements. The README's memory claim is replaced by the budget table.

Regression tests (all deterministic, using small explicit budgets): `every input-dependent structure has a named budget and the worst case is bounded and documented`, `the hash tables never grow past their budget, however many distinct values arrive`, `many distinct identifiers stop the run safely` (JSON identifiers, short identifiers, short-identifier bytes, kept keys), `many distinct HTML class names stop the run safely`, `too many members, too many summary entries, and leak-table overflow stop the run safely` (directory, zip and the leak table), `nesting depth and token size are bounded` (a 2 MB `[[[[…` bomb is refused in well under 5 s; JSON string, JSON scalar, HTML text, and run-level), and `a high-cardinality export within the budgets completes and reports its limits`. Every budget-stop test asserts the exact budget, exit code 5, the previous output unchanged and an unchanged directory listing.

Still **NOT_RUN** (unchanged from the review): execution of the CLI on **Windows** (directory, symlink/reparse-point and `--force` failure paths, zip fixtures), the owner-local trial on a **real** export, and exact-head hosted CI for the new commit.

## Re-review round 3 — SAN-F07 (P2): remaining unbounded ingest buffers

Finding ([comment](https://github.com/KaisAbiyyi/mind-place/pull/167#issuecomment-6810832761), reviewed head `5e78d81`): SAN-F01 to F05 remain addressed and SAN-F06 is "substantially repaired", but three input paths still accumulated or enumerated without a bound, and the 1 GiB figure was presented as a guarantee. Hosted CI at `5e78d81` was SUCCESS (it does not cover these fixtures). Scope of this round: SAN-F07 only; nothing else was reworked.

| Finding | Cause | Correction |
| --- | --- | --- |
| **F07-A** text member without newlines | `neutralizeTextStream()` and `scanForLeaks()` kept `carry = (carry + chunk).split('\n').pop()` with no cap, so one long line grew the buffer without limit (and `scanForLeaks` also split UTF-8 sequences across chunks) | New `text-stream.mjs` `boundedLines()`: decodes with a `StringDecoder`, yields complete lines and checks `carry + piece` against `maxTokenChars` **before** concatenating. Both callers use it. Overflow is `E_BUDGET(maxTokenChars)`, exit 5, nothing published |
| **F07-B** unfinished HTML declarations | `<!` / `<?` with no `>` returned for more data without a size check (only comments, tags and raw text were checked) | The same pre-buffer check was added to that branch, and a completed declaration is checked at scan time too |
| **F07-C** directory enumeration | `readdir(dir)` built and sorted the whole name array before `maxMembers` was applied | `opendir()` iterated incrementally; every name seen (files, directories, links) counts against `maxMembers` and the walk stops with `E_BUDGET` after `maxMembers` names, before anything is sorted |
| **Docs** the 1 GiB figure | `totalUpperBoundBytes()` and the README read as an absolute memory guarantee although some paths sat outside the model | Renamed to `structureEstimateBytes()` / `totalStructureEstimateBytes()` and the report field to `enumeratedStructuresEstimateBytes`; README and the report note now say it is an **estimate of the enumerated structures, not a bound on process peak memory** (Node heap, buffers, zlib and spool are outside it). A test fails if the README claims otherwise |

Regression tests (`test/import-sanitizer.test.mjs`, all deterministic, every one asserting exit 5, the exact budget, the previous output unchanged and an unchanged directory listing):

- `SAN-F07-A: a text member without newlines is bounded before the buffer grows` — chunk-independent lines and UTF-8, exact-limit boundary, an endless single line refused after about one budget of input (source consumption asserted), a long line after ordinary lines, and the leak scan.
- `SAN-F07-A: .txt and .md members without newlines stop the run safely` — file, directory and zip inputs; ordinary multi-line text is still neutralized.
- `SAN-F07-B: unfinished or oversized HTML declarations are bounded before the buffer grows` — `<!`, `<?`, comment and tag, unterminated across many chunks (consumption asserted) and completed-but-oversized, via both the rewriter and the scanner, and at run level.
- `SAN-F07-C: directories are enumerated incrementally with a cap` — a 2,000-entry directory with `maxMembers: 10`: `readdir` is replaced by a thrower, at most 11 entries are pulled from the directory, subdirectory names count too, and a small directory is still sorted and processed.

**The tests were checked against the defect.** Each guard was removed in turn and the matching test failed: text-line cap removed (both A tests fail), unfinished `<!`/`<?` cap removed (the B test fails; the test source now throws `SOURCE_DRAINED` after a few MB so an unguarded scanner fails fast instead of hanging CI), `readdir` materialization restored (the C test fails). The guards were then restored.

Still **NOT_RUN**: execution of the CLI on **Windows**, the owner-local **real-export** trial, and exact-head hosted CI for the new commit. A step-by-step owner runbook for both is now in the README ("Owner qualification runbook"). It records results as PASS/FAIL/PARTIAL/BLOCKED and asks that no real data be pasted anywhere.

**Found while verifying this round (not from the review).** Re-running the suite repeatedly exposed two intermittent failures that the random per-run key (SAN-F03) had made visible:

1. *A defect in the tool.* Member names were mapped by shape without a uniqueness guarantee, so two different short names (for example `a.json` and `b.json`) could map to the same output name and the second member was dropped as `failed:E_NAME_COLLISION` (exit 2). With the former fixed key this never showed. Identifiers and class tokens already had injective tables; names did not. Fix: on a collision only the final name segment is re-keyed deterministically (directory prefixes stay consistent between entries), with a numeric-suffix fallback so uniqueness never depends on luck; kept structural names such as `conversations.json` are never renamed, so a genuine duplicate is still reported. Test: `member names never collide…` (52 single-letter names, directory and zip, every run).
2. *A flaky test of mine.* The Gemini test rejected output matching `/app/c_`; because identifiers keep their shape, a pseudonym starts with `c` about one time in 26. The check now asserts the planted identifiers are absent instead. The suite passed 8 consecutive runs after both fixes.

## Re-review round 4 — SAN-F08 (P2): `--report` could change the verified output after publication

Finding ([comment](https://github.com/KaisAbiyyi/mind-place/pull/167#issuecomment-6811476735), reviewed head `aa8b1c4`): SAN-F07 is **resolved** and hosted CI at that head was SUCCESS. New: `assertReportPath()` only rejected input/report overlap and an exact output/report match, and the CLI wrote the report **after** `sanitize()` had verified and published the output. So `--output out/ --report out/user.json --force` replaced a verified member with the report (the printed PASS then described bytes that were no longer there); without `--force` the exclusive write failed after the output was already published; a new report name inside the output added an unverified file.

| Item | Correction |
| --- | --- |
| Preflight | `assertReportPath()` now requires the report to be disjoint from the input **and from the output** on canonical paths (equal, inside or containing, including symlink/junction aliases), and not the tool's own `<output>.partial` / `<output>.replaced-*` names. Codes: `E_REPORT_OVERLAPS_OUTPUT`, `E_REPORT_OVERLAPS_INPUT`. It runs inside `sanitize()` (so API callers are covered too), before the source is opened, and for a dry run against the input |
| Ordering | `sanitize()` owns the report. The report is **staged** (exclusive create next to its final path) after verification and **before** the output is published; then the output is published; then the report is committed (rename with `--force`, no-clobber `link` without) |
| Failure | If the report cannot be staged, nothing has been published (`E_REPORT_WRITE`). If the final commit fails, the output publication is **rolled back**: a new output is removed, a replaced output is restored from its aside copy, a previous report is untouched. Never a published output with a failed report. `publishAtomically()` now returns `commit()` / `undo()`; the previous output is deleted only after the report is committed |
| Existing report | Needs `--force` (`E_REPORT_EXISTS`); a directory or symlink at the report path is never replaced (`E_REPORT_NOT_REPLACEABLE`); a run that fails verification leaves an existing report untouched |

Regression tests (CLI level for the isolation matrix, API level for failure injection):

- `SAN-F08: a report inside, at, above or aliasing the output is refused before anything is read or written` — 11 report paths × with and without `--force`: an existing member (`out/user.json`, the reviewer's scenario), a new file in the output, a nested path, an ancestor of the output, the output itself, `.partial`, `.replaced-*`, a symlink alias into the output, a symlink alias to the output, and report = input file / input directory. Each asserts exit 1, the exact error code, the previous verified output byte-identical (sha256 snapshot), the input byte-identical, and an unchanged directory listing next to and above the output; plus the same codes for programmatic callers and a dry run.
- `SAN-F08: a report beside (not inside) the output is allowed, staged first, and never changes the verified output` — the report's per-member sizes equal the published files, no report or staging file inside the output, an existing report needs `--force`, a refused report stops the run before any output exists.
- `SAN-F08: if the report cannot be written the output is not published and a previous output survives` — the final commit is made to fail (`ENOSPC`): no previous output (nothing published, nothing left over), a previous tool-made directory output plus a previous report (both restored byte for byte), and a single-file output.
- `SAN-F08: a report that cannot be staged, or a run that fails verification, publishes nothing and leaves the old report` — missing parent directory (CLI exit 1), a leak failure with an existing report, a directory at the report path.

**Checked against the defect:** removing the report-vs-output preflight makes the isolation test fail, and removing the rollback makes the failure-injection test fail; both were restored.

Still **NOT_RUN**: Windows execution of the CLI (the report/output overlap checks rely on `realpath`; junction behaviour is in the owner runbook), the owner-local real-export trial, and the hosted CI run on the new head.

## Re-review round 5 — SAN-F09 (P2): report staging error paths could leave an artifact

Finding ([comment](https://github.com/KaisAbiyyi/mind-place/pull/167#issuecomment-6814412069), reviewed head `cfc30ad`): SAN-F08's overlap and rollback corrections were confirmed (hosted CI at that head SUCCESS). Two error paths in `stageReport()` still broke the no-artifact-on-failure contract:

| Path | Defect | Correction |
| --- | --- | --- |
| **A** partial staged write | `writeFile(staged, json, { flag: 'wx' })` can create the file and write some bytes before failing (`ENOSPC`, `EIO`). The catch threw `E_REPORT_WRITE` without removing it, and the caller's `stagedReport` was still `null`, so `discard()` could never clean it: a `report.json.partial-<pid>` with residual metadata was left behind | The catch removes the staged file before rethrowing. It does **not** remove it when the error is `EEXIST`: the exclusive create did not happen, so that name belongs to something that is not ours and is never touched. The original error code is kept in the message |
| **B** link succeeded, staged cleanup failed | Without `--force` the commit is `link(staged, report)` then `rm(staged)`. If the `rm` failed, the caller rolled the output back, but the final report created by the `link` stayed, so a failed run left a committed report with no published output | The commit is transactional: if `rm(staged)` fails after the link, the newly linked report is removed (a successful `link` proves the path was free, so it is ours) and the error is rethrown; `discard()` retries the staged name. The `--force` path is a single atomic `rename` and was not affected |

Regression tests (both at API level with in-process fault injection, and through the **real CLI** in a child process via a `--import` preload that injects the fault):

- `SAN-F09-A`: a write that creates the staged file, writes 10 bytes, then throws `ENOSPC`. With no previous output: `E_REPORT_WRITE`, exit 1, no staged file, no report, no output, directory listings unchanged, the message carries only the code and no input text or path. With a previous output and report plus `--force`: both byte-identical afterwards. A file that already has the staging name is refused and left untouched (`OWNER FILE`).
- `SAN-F09-B`: `link` succeeds, then the first plain `rm` of the staged name fails with `EIO`. `E_REPORT_WRITE`, exit 1, **no orphaned report**, no staged leftover (the retry in `discard()` removes it), output rolled back, listings unchanged; the same run without the fault commits one report and no staged name.
- **Checked by mutation:** keeping the partial staged file, leaving the linked report behind, and also removing a foreign file on `EEXIST` each made the matching test fail. All restored.

The output/report overlap protections, `--force` rollback and budgets are unchanged and their tests still pass.

Still **NOT_RUN**: Windows execution of the CLI, the owner-local real-export trial, and hosted CI for the new head until the run completes.

## Windows qualification review — WIN-SAN-F01 (P2), WIN-SAN-F02 (P2), WIN-SAN-F03 (P3)

Source: the independent Windows qualification review of `e56bff7` on PR #167 (owner-run synthetic Windows 11 report: suite 48 pass / 7 fail / 0 skipped of 55; 75 manual scenarios mostly PASS; one long-output-path FAIL; locked input + `--force` PARTIAL; true symlinks BLOCKED). SAN-F09 was closed at source level by that review; this round addresses what it asked for.

**WIN-SAN-F01 — a read failure silently downgraded a good `--force` output.** `sanitizeMember()` caught *every* error and replaced the member with an empty stand-in, so a locked file (`EBUSY`), `EACCES`, `EIO`, a corrupt zip entry or a failed spool write ended as exit 2 and replaced the previous complete output.
- A member is now a recoverable failure **only for a parse error** (`JsonError`/`HtmlError`: the bytes were read and are not valid JSON/HTML). Everything else throws `E_MEMBER_IO` (exit 1, message = content-free error code only): nothing is published, the previous output and report are untouched, no staging leftovers. This includes a corrupt zip entry, so the earlier test that expected a stand-in for tampered zip data now expects the run to fail closed.
- Exit 2 is documented (README and `--help`) as a **partial successful publication**. A partial output that would **replace an existing output** is refused (`E_PARTIAL_WOULD_REPLACE`, exit 1) unless the owner passes the new explicit `--allow-partial`; a first-time output with parse failures is published as before.
- Tests: injected read failures (`EBUSY`/`EACCES`/`EIO`, at the first bytes and at the middle of a **non-first** member, so earlier members are already staged) with `--force` assert code, exit code, no path/content in the message, **hash snapshots of the output equal, previous report byte-equal, directory listing unchanged**; a single-file source; the same through the real CLI with a `file://` preload (asserting the injected code appears); parse failure + `--force` refused, first-time output exit 2, `--allow-partial` replaces. Each guard was removed in turn and the matching test failed.

**WIN-SAN-F02 — test portability, not product defects.**
- `SAN-F09-A/B` CLI halves passed a `C:\…` path to `node --import`; they now pass `pathToFileURL(preload).href` and also assert the injected fault ran (`ENOSPC` / `EBUSY` in stderr), not just a non-zero exit.
- `member names never collide` wrote `a.json…z.json` and `A.json…Z.json` into one directory (26 files on NTFS). It now uses 52 case-insensitively distinct names for directory and zip, asserts the fixture itself has 52, and keeps a separate zip-only case-variant run (all 52 output names distinct).
- Symlink tests now go through `linkTest()`: each runs once per link kind. `[symlink]` is **SKIPPED with a `BLOCKED:` reason** when the runner cannot create symbolic links (Windows without Developer Mode/elevation) and still runs in Linux CI and on privileged machines; on Windows `[junction]` variants (no privilege) run the same assertions. Nothing was relaxed.
- Local result on an unprivileged Windows 11 account (Node v24.21.0, no Developer Mode, not elevated), five consecutive runs: **62 tests, 58 pass, 0 fail, 4 skipped** (the four `[symlink]` variants, reported as BLOCKED). The `[junction]` variants pass. Self-reported, not exact-head hosted CI and not an owner requalification.

**WIN-SAN-F03 — long output path.** `fs.mkdtemp` for the staging folder next to the output fails with `ENOENT` once the output path exceeds the Windows path limit (about 260 characters; input paths and zip member names of that length work). The raw error is now `E_SPOOL_INIT` (exit 1, message has only an error code, no path), a missing parent folder maps to the same code, the dry-run work folder is removed if the second staging folder fails, and the source is closed. Long output paths are **documented as not supported**; the tool does not create parent folders. A Windows-only test accepts either success or `E_SPOOL_INIT`, never a raw error; cross-platform tests inject the OS error.

**Documented, not changed (review "other findings").** `--force` over an existing *file* replaces it (no marker is possible); a file input reached through a *parent* junction is accepted (only the final component is checked; output/report overlap rules still use real paths). Both are in README "Known limits and design notes".

**Still NOT_RUN / not claimed.** True-symlink variants on Windows (BLOCKED here, covered by CI and junction variants), Explorer GUI, CLI exits 3/4/5 on Windows, hosted Windows Product job (skipped), and the owner real-export trial.

## Verification

Environment: Linux x86_64, Node 22.23, Cargo 1.97. Synthetic data only.

| Command | Result |
| --- | --- |
| `node --import tsx --test test/import-sanitizer.test.mjs` | PASS: 55 passed, 0 failed (29 initially; 37, 44, 49 and 53 after rounds 1 to 4; stable over repeated runs) |
| `cargo test --locked -p mind-place-archive --tests` | PASS: 124 passed, 0 failed, 0 ignored (includes the 3 parser cross-check tests and the 5 FIX-F01 tests) |
| `npm run test:web` | PASS: 326 tests, 324 passed, 0 failed, 2 skipped (Windows-only) |
| `npm run test:rust` | PASS: 355 passed, 0 failed, 1 ignored (existing qualification-ignored test) |
| `cargo fmt --all -- --check`, `npm run verify:repo`, `npm run typecheck`, `node scripts/verify-workflow-security.mjs`, `git diff --check` | PASS |
| Run on a **real** ChatGPT/Gemini export | **NOT_RUN** (owner-local step) |
| The tool on Windows | **NOT_RUN** (verified on Linux only; member names are hardened for Windows-reserved characters and backslash separators by a unit test) |
| Hosted CI on this branch | **NOT_RUN** |

Performance (local observation, not CI): a synthetic 151 MB `conversations.json` sanitized and fully verified in about 13 s, 140 MB resident.

## Limits and NOT_RUN

- **Real exports: NOT_RUN.** The real ChatGPT layout (zip member names, whether `conversations.json` is split) and the real Takeout layout, including the localized `Prompted` label, were never inspected. Expect the first owner run to surface unknown field names, a localized label (use `--owner-hints`, then `--keep-label`) or members reported as `failed:` and replaced by stand-ins. Those are the findings this tool exists to produce.
- The leak check cannot see strings shorter than 8 characters by design, and it exempts allow-listed values; the report's kept-values list is the compensating review step. It is a safety net, not a proof of anonymity: an owner should still read the inventory.
- Distinct identifiers are counted by 64-bit hash; a collision would at worst fail a run (fail-safe direction).
- Non-ASCII object keys that look like data are pseudonymized to ASCII; the output-side structure analysis cannot recognize that and would fail the run with a structure difference rather than pass silently.
- The sanitized copy has placeholder text, so deduplication, search and title behaviour of Mind Place on it are not representative. Only structure is.
- Hosted CI runs this suite in the existing `fast` job through `npm run test:web` and the Rust cross-check through the new `rust-fast` archive step; **hosted CI on this branch is NOT_RUN at handoff time**.
- ChatGPT zip handling for MP-IMPORT-APP-01 is not provided here; this tool only reads zips to sanitize them.

## Handoff

`TASK_ID` MP-IMPORT-SANITIZE-01 · `CLAIM` claimed by the Claude Code cloud session; no fencing generation · `BASE_SHA` `main` `06b6199` (branch also carries PR #166 and the FIX-01 commits) · `BRANCH` `agent/lucid-euler-l3lopd` · `REMOTE_CHECKPOINT_SHA` see the final chat handoff · `PR` none opened · `LOCAL_REQUIRED` owner-local run on a real export (**NOT_RUN**) · `HUMAN_REQUIRED` that run, review of the printed inventory before sharing, and the owner's manual merge decision.

Unlocks (after the owner-local run): MP-IMPORT-APP-01 (verified export layout) and MP-IMPORT-L1.

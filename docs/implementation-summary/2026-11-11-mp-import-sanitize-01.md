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

## Verification

Environment: Linux x86_64, Node 22.23, Cargo 1.97. Synthetic data only.

| Command | Result |
| --- | --- |
| `node --import tsx --test test/import-sanitizer.test.mjs` | PASS: 44 passed, 0 failed (29 before re-review round 1; 37 before round 2) |
| `cargo test --locked -p mind-place-archive --tests` | PASS: 124 passed, 0 failed, 0 ignored (includes the 3 parser cross-check tests and the 5 FIX-F01 tests) |
| `npm run test:web` | PASS: 315 tests, 313 passed, 0 failed, 2 skipped (Windows-only) |
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

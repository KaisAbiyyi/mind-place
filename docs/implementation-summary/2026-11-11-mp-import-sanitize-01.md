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
| `json-stream.mjs` | Streaming JSON tokenizer and rewriter: no document is built; layout, key order and number formats are kept byte-for-byte |
| `html-stream.mjs` | Streaming HTML scanner and rewriter that keeps tags, attribute names, quoting style, case and comment boundaries; independent of read chunking |
| `rules.mjs` | Default-deny rules: enum allow-list, same-shape keyed pseudonyms, length-class filler, URL/timestamp/name handling, media stand-ins |
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

Other requirements from the card: deterministic output (byte-identical zips across runs, `output is deterministic…`); no network (`the sanitizer tree is offline…` forbids network, process, eval and `console.*` APIs); no content logging (CLI test asserts stdout, stderr and the report contain none of the planted strings); a leak check that **fails the run** (fault-injection test: a rule that passes a title through makes the run exit 3 and leave no output, `.partial` file or spool); structure check that fails the run (fault-injection test, exit 4); stripped media (tiny valid stand-ins of the same type, media never read).

## Review findings that shaped the tool

1. **Leak registry scope.** My first design tracked only strings that rules chose to replace, so a rule that forgot a string would never be caught. It now tracks *every* input string and exempts only values an allow-list rule explicitly registers as kept. The fault-injection test failed before this change and passes after.
2. **Allow-list outputs must be registered.** The leak check flagged the URL scheme `file-service`, the data-URI stand-ins and coarsened timestamps because the rules emitted them without registering them. Every allow-list emission now goes through `kept()`.
3. **Reference integrity.** Mapping keys and the `id`/`parent`/`children` values referring to them were pseudonymized with different labels and no longer matched; fixed and covered by the reciprocal-link test.
4. **Fidelity.** End-tag case was normalized while start-tag case was kept; both are now preserved. Placeholder length classes round **down**, so the sanitized copy is never larger than the original (measured 151 MB in, 109 MB out).
5. **Transparency.** Enum-like values and known hosts are kept by design and exempt from the leak check, so the report now lists every kept enum value by key for the owner to review.

## Verification

Environment: Linux x86_64, Node 22.23, Cargo 1.97. Synthetic data only.

| Command | Result |
| --- | --- |
| `node --import tsx --test test/import-sanitizer.test.mjs` | PASS: 29 passed, 0 failed |
| `cargo test --locked -p mind-place-archive --tests` | PASS: 119 passed, 0 failed, 0 ignored (includes the 3 new parser cross-check tests) |
| `npm run test:web` | PASS: 300 tests, 298 passed, 0 failed, 2 skipped (Windows-only) |
| `npm run test:rust` | PASS: 350 passed, 0 failed, 1 ignored (existing qualification-ignored test) |
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

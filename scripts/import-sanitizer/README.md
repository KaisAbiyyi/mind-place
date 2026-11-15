# Import sanitizer

Turns a **real** ChatGPT export or Gemini Takeout into a **structurally identical but content-free**
copy, so real-world layouts and edge cases can be used as fixtures and for parser qualification
**without any private text leaving your machine**. It runs offline, has no dependencies, and prints
only counts and sanitized names.

> Task `MP-IMPORT-SANITIZE-01`. Built and tested with synthetic exports only. **Running it on a real
> export is an owner-local step and stays `NOT_RUN` until you do it.** Agents never read real exports.

## Use

```bash
# ChatGPT export zip -> sanitized zip (same member layout)
node scripts/import-sanitizer/cli.mjs --input chatgpt-export.zip --output sanitized-chatgpt.zip --report chatgpt-report.json

# A loose conversations*.json, or a directory of an extracted export
node scripts/import-sanitizer/cli.mjs --input conversations.json --output sanitized-conversations.json
node scripts/import-sanitizer/cli.mjs --input extracted-export/ --output sanitized-export/

# Gemini Takeout (HTML file, extracted folder, or zip)
node scripts/import-sanitizer/cli.mjs --input Takeout/ --output sanitized-takeout/ --report gemini-report.json

# Check everything and print the inventory, keep nothing
node scripts/import-sanitizer/cli.mjs --input chatgpt-export.zip --dry-run
```

`npm run sanitize:import -- <options>` is the same command. Share **only** the sanitized output and the
report, after reading the inventory printed on screen.

| Option | Meaning |
| --- | --- |
| `--input` / `--output` | zip → zip, directory → directory, file → file. The output must not exist (`--force` replaces it) and must not be inside the input. |
| `--dry-run` | Run all checks, print the inventory, keep no output. |
| `--report <file>` | Write the shareable JSON inventory. It describes the **sanitized output only**. The report is a separate artifact: its path must be outside the input **and outside the output** (not equal to it, not inside it, not a directory that contains it, also through symlinks/junctions, and not a `.partial` / `.replaced-*` name the tool uses), otherwise it could replace or add to the files that were verified. It is staged **before** the output is published and committed right after; if it cannot be written the run is rolled back (nothing is published, a previous output and report are restored). An existing report needs `--force`. |
| `--timestamps day\|keep` | `day` (default) keeps the date and drops the time of day, preserving number formats. `keep` leaves timestamps as they are. |
| `--keep-label <text>` | Also keep this structural HTML label, repeatable. Use it for a localized "Prompted". |
| `--keep-enum <key=value>` | Also keep this enum value verbatim (repeatable), e.g. `status=queued`. Review the report first: kept values are exempt from the leak check. |
| `--keep-class <name>` | Also keep this HTML class token (repeatable). Unknown class names are private by default. |
| `--owner-hints` | Prints frequent leading words of HTML text nodes **to the terminal only**, to help you choose `--keep-label`. Never written to the report. |
| `--salt-file <path>` / `MIND_PLACE_SANITIZER_SALT` | Pseudonym key, **opt-in** for reproducible output. By default every run uses a fresh random 256-bit key that is never written, logged or reported, so identifiers (even short, sequential ones) cannot be confirmed offline by enumerating candidates. There is deliberately no command-line salt option. |
| `--allow-partial` | Only with `--force`: allow replacing an existing output with a **partial** one (see exit `2`). Without it a run whose members include parse failures publishes nothing and keeps the existing output (`E_PARTIAL_WOULD_REPLACE`, exit 1). A first-time output is not affected. |
| `--force` | Replace an existing output (and an existing report file), but **only after the new one is fully written and verified** (the old one is moved aside and removed last, and restored if the swap fails). Refuses a directory that this tool did not create, a symbolic link, and any output that is the input, inside it, or a directory that contains it (checked on canonical paths, after resolving links). |

Exit codes: `0` PASS · `1` usage or I/O error · `2` PARTIAL · `3` leak check FAILED **or INCOMPLETE** (tracking overflowed, so part of
the input was never checked) · `4` structure check FAILED · `5` resource budget exceeded. On `1`, `3`, `4` and `5` **nothing is published** and an
existing output and report are untouched.

**Exit `2` is a partial successful publication**, not a failure: the output *was* published, but some members were read fine and are not
valid JSON/HTML (**parse failures**), so they became empty stand-ins and are listed as `failed:` in the inventory. It is never used for a member
that could not be *read*: an OS read error (`EBUSY` for a locked file, `EACCES`, `EIO`, ...), a truncated or corrupt zip entry (CRC, size or
inflate error) or a failure to stage a member means the source was not read completely, so the run **fails closed** (`E_MEMBER_IO`, exit `1`,
nothing published, existing output kept). A partial output also never replaces an existing output unless you pass `--allow-partial`.

## What PASS does and does not mean

**Structure fidelity** and **privacy** are different questions. `PASS` means the structure is unchanged and no
input string of 8+ characters outside the allow-lists survived. It is **not** an anonymity guarantee. Whatever the
allow-lists keep stays in the output (member names, enum values from the known provider vocabulary, known HTML class
names and attribute values, known link hosts, the date part of timestamps, field names, record counts and size
buckets). The report records `contentFreeAssertion: NOT_ASSERTED`, the CLI prints a **RESIDUAL METADATA** block and the
report's `residualMetadata` section lists every kept value. Read it before sharing anything, and use `--timestamps`,
`--keep-enum` and `--keep-class` deliberately.

## What is kept, replaced and neutralized

The rules are **default-deny**: a string survives only when an explicit allow-list rule keeps it.

| Input | Output |
| --- | --- |
| Keys, nesting, array lengths, key order, whitespace, number formats, `true`/`false`/`null`, escaping of kept strings | Kept byte-for-byte |
| Free text (messages, titles, names, e-mail addresses, prompts, comments, script/style bodies) | Replaced by filler words of the **same length class**, rounded **down** (never longer), capped at 16,384 characters. Line count and markdown markers (`#`, `-`, `1.`, `>`, code fences) are kept. |
| Identifiers (`id`, `parent`, `children`, `mapping` keys, `current_node`, UUIDs, `file-…` ids, asset pointers) | Keyed pseudonym of the **same shape** (UUID version/variant, `file-`/`user-` prefixes, hex/alphanumeric classes). Equal inputs give equal outputs and distinct inputs give distinct outputs, so references, duplicates, orphans and branches are preserved. |
| Enum-like values (`role`, `content_type`, `status`, `recipient`, `channel`, `model_slug`, known tool names, `mime_type`, …) | Kept **only if the value is in the built-in provider vocabulary**. A private author name, a custom plugin id or any other enum-shaped value is replaced and only counted. `client-created-root` is kept. |
| URLs | Scheme and known hosts (`gemini.google.com`, `chatgpt.com`, …) kept; other hosts become `*.invalid`; ids in paths pseudonymized; query values and fragments dropped; `data:` URLs become a tiny valid image |
| Timestamps (time-like keys, Takeout `D Mon YYYY, HH:MM:SS ZONE` text) | `--timestamps day`: same representation, time of day zeroed |
| Media and binary members (png, jpg, webp, pdf, audio, …) | Replaced by a tiny valid stand-in of the same type; media bytes are never read |
| Member and file names | Structural names kept (`conversations-000.json`, `user.json`, `chat.html`, `Takeout`, …); everything else shape-mapped with the extension kept |
| HTML | Tags, attribute names, quoting style, case, comment boundaries and `<br>`/`<p>` structure kept. Class tokens are kept only from the known Takeout/Material Design Lite vocabulary (others get a same-shape pseudonym); `lang`, `dir`, `charset`, `type`, `role`, `target`, `rel`, `colspan`, `rowspan` only with known values; text nodes, every other attribute value and comment/script bodies are replaced. The `Prompted` label is kept. |
| Unknown fields | Kept as structure and listed for your review |

Why identifiers are protected: names, e-mail addresses and text are replaced by content-independent filler and are never
hashed. Identifiers are pseudonymized with a keyed hash whose key is random per run by default, so neither random UUIDs
nor low-entropy identifiers can be confirmed by enumeration. If you supply a key to reproduce output, keep it private:
anyone holding it can test candidate identifiers.

## Checks that run on every execution

1. **Structure check.** The output is re-read from disk and compared with the input: every JSON path, value
   type and count, the number of distinct identifiers, and for HTML every tag, attribute name, class token,
   text-node and timestamp-node count. Any difference fails the run.
2. **Leak check.** Every input string and distinctive token of **8+ characters** is tracked as a 64-bit hash
   (nothing is stored or printed). The re-read output must not contain any of them, except values an
   allow-list rule deliberately preserved. A single survivor fails the run. If the tracking table's 40M-entry cap is
   reached, part of the input was never checked, so the run **fails** (`E_LEAK_COVERAGE`, exit 3) instead of passing.
3. Atomic publish: output is written to `<output>.partial` and swapped in only after both checks pass.

Limits worth knowing: strings shorter than 8 characters are below the leak threshold by design (they are
still replaced, just not tracked); the checks cover what the tool can parse, and a member it cannot parse
becomes an empty stand-in and is reported, never copied; free text is replaced, so Mind Place
**deduplication and search behaviour on the sanitized copy is not representative**, only structure is.

## Inventory

The printed table and the JSON report list, for the **sanitized output**: member names, kind, output size, a
power-of-two bucket for the source size, record counts, per-path value-type counts, distinct identifier
counts, HTML tag/attribute/class/host counts, unknown field names, the **residual metadata** (every enum value, member
name, class token, attribute value and host kept verbatim) and the two check results. It works for a
single file, a directory, a zip with one `conversations.json`, and a zip with several
`conversations-*.json` members. It never contains input text. Read it before sharing anyway. The leak check deliberately
exempts allow-listed values, so the residual-metadata list is where you spot anything you would rather not disclose.

## Resource budgets (memory is bounded by budgets, not by the largest string)

Streaming keeps the *content* flowing, but several structures grow with the **number of distinct** identifiers,
tokens, members or the nesting depth of the input. Each one has a named budget. Exceeding any budget stops the run
with `E_BUDGET` (exit `5`; the leak table overflow keeps its own `E_LEAK_COVERAGE`, exit `3`): **nothing is
published, an existing output is left untouched, and every spool and partial file is removed.** Budgets are
recorded in the report (`resourceBudgets`).

| Budget | Default | Bounds |
| --- | ---: | --- |
| `maxProtectedPieces` | 8,000,000 | leak registry: distinct input strings and tokens of 8+ characters |
| `maxKeptPieces` | 1,000,000 | leak registry: distinct allow-listed values (kept keys, enum values, …) |
| `maxDistinctIdentifiers` | 4,000,000 | per JSON member, for the input-side and the output-side structure analysis |
| `maxUnknownClassTokens` | 1,000,000 | per HTML member: distinct unknown class names |
| `maxShortIdentifiers` / `maxShortIdentifierBytes` | 100,000 / 8 MiB | injectivity table for short identifiers |
| `maxMembers` | 100,000 | names enumerated in a directory tree (files, directories, links; counted while listing, before sorting) or zip entries (also caps the zip central directory read into memory) |
| `maxSummaryEntries` | 500,000 | path/tag entries held across all members' summaries |
| `maxJsonDepth` | 256 | JSON nesting (a file of `[[[[…` can no longer grow a stack) |
| `maxTokenChars` | 32 Mi chars | one JSON string or scalar, one HTML text node, tag, comment or declaration, one plain-text line |

Estimate: if **every** budget were reached at the same time, the enumerated structures would hold roughly 0.7 to 0.9 GiB
(`totalStructureEstimateBytes()` in `budgets.mjs`; hash tables are typed arrays at 8 bytes per slot and briefly hold old
and new tables while growing). **This is an estimate of those structures, not a guaranteed upper bound on the process's
peak memory**: Node's heap and garbage collector, read and parse buffers, zlib state and the spool are outside it, so
peak RSS can be higher. Every input path that accumulates text is covered by `maxTokenChars` (JSON strings and scalars,
HTML text, tags, comments and `<!`/`<?` declarations, and plain-text lines in `.txt`/`.md` members, checked before the
buffer grows), and directory listings are enumerated incrementally and refused beyond `maxMembers` names before anything is
sorted. Observed peaks, measured locally (Linux, Node 22), were:

| Input | Result | Peak RSS |
| --- | --- | ---: |
| 151 MB synthetic `conversations.json` (ordinary text-heavy export) | PASS in about 13 s | 140 MiB |
| 128 MB export with 1,000,000 distinct identifiers | PASS in about 66 s | 208 MiB |
| 576 MB crafted export with 4,500,000 distinct identifiers | stopped at the 4,000,000 budget, exit 5, previous output intact, nothing left behind | 327 MiB |

These are one-off local observations, not CI measurements. There is no command-line option to raise a budget; the
`budgets` option of the programmatic `sanitize()` exists for tests and for owners who knowingly need more.

## Owner qualification runbook (Windows, then a real export)

These two steps are **owner-local** and remain `NOT_RUN` until you do them. Record each as `PASS`, `FAIL`,
`PARTIAL` or `BLOCKED` with the exact commit SHA. Do not paste, upload or attach any real export or sanitized
output to GitHub or to an agent as part of recording a result; record counts, exit codes and your own review notes only.

### 1. Windows qualification (synthetic data only)

Record the run **exactly** as the runner reports it: `tests`, `pass`, `fail`, `skipped`. On a Windows account without Developer Mode or elevation
the four true-symlink variants (`[symlink]`) are reported as **SKIPPED with a `BLOCKED:` reason**, never as a pass, and the same defenses are
exercised with directory **junctions** (`[junction]` variants, no privilege needed). The symlink variants run in Linux CI and on a Windows
machine that can create symbolic links; do not turn the test count green by weakening them.

Use a checkout of the exact commit under review, on Windows x64, in a scratch directory outside the repository.

```powershell
npm ci
node --import tsx --test test/import-sanitizer.test.mjs            # whole suite; expect 0 failures
node test/fixtures/import-sanitizer/generate-parser-compat.mjs      # then: git status must show no change
```

The suite creates symbolic links. On Windows that needs Developer Mode or an elevated shell; if the symlink-based tests
cannot run, record `BLOCKED` for them rather than `PASS`. Then exercise the failure paths by hand, with a synthetic export
you create (for example by running the test fixture builders), and confirm every one of these:

| Check | Expected |
| --- | --- |
| `--output` is the folder that **contains** the input file, with and without `--force` | refused, exit 1 (`E_OUTPUT_OVERLAPS_INPUT`); the input file is untouched |
| `--output` is a directory **junction** (`mklink /J`) or symlink that points at the input's folder | refused, exit 1; input untouched |
| `--input` is a junction or symlink | refused, exit 1 (`E_INPUT_SYMLINK`) |
| `--force` over an existing output after a deliberately failing run | previous output intact, no `.partial`, `.replaced-*` or `.mp-sanitize-spool-*` left |
| `--force` onto a folder that this tool did not create | refused (`E_OUTPUT_NOT_REPLACEABLE`), folder untouched |
| ZIP in, ZIP out, then open the result with Explorer and `tar -tf` | valid archive, same member layout |
| Member names that Windows rejects (`:`, `?`, `*`, trailing dot) inside a ZIP | sanitized names are valid on NTFS |
| Paths longer than 260 characters | input paths and zip member names of that length work (observed). An **output** path that long fails in `mkdtemp` for the staging folder: stable `E_SPOOL_INIT`, exit 1, nothing created or left. Long output paths are **not supported yet**; use a shorter output folder. Record what you see; do not assume |
| `--force` with a source member that cannot be read (for example a file another program holds open) | `E_MEMBER_IO`, exit 1, previous output and report intact, nothing left behind |
| Run twice in a row | no leftovers; second run needs `--force` |

### 2. First trial on a real export

1. Work on your own machine only. First run `--dry-run` and read the whole terminal output: structure check, leak check,
   members with `failed:` status, unknown field names and the **RESIDUAL METADATA** block.
2. If `Prompted` is localized, rerun with `--owner-hints` (terminal only) and then `--keep-label <word>`. If a class name the
   Gemini parser needs was replaced, add it with `--keep-class` only after reviewing it.
3. Run for real into a new output path. Treat exit `2` (some members replaced by empty stand-ins), `3`, `4` and `5` as
   findings to report as counts, not as a reason to loosen a budget or an allow-list.
4. Open the sanitized output yourself and the JSON report. Decide whether the residual metadata (member names, enum values,
   class names, hosts, dates, field names, counts) is acceptable to disclose. `PASS` is not an anonymity guarantee.
5. Only then decide whether to share the sanitized output, and with whom. The tool never decides that for you.
6. Record: commit SHA, OS, input kind and rough size, exit code, counts from the report, which options you used, and your
   residual-metadata decision. A localized label, unknown fields or `failed:` members are exactly what MP-IMPORT-APP-01 needs.

## Known limits and design notes

- **Output parent folder:** it must already exist; the tool never creates parents. If the staging folder next to the output cannot be created
  (missing or read-only parent, a Windows path longer than about 260 characters) the run stops with `E_SPOOL_INIT` (exit 1) and a message that
  contains only an error code, never a path.
- **`--force` onto an existing *file*** (file-to-file and zip-to-zip output) replaces that file once the new output is verified. Unlike a
  directory, a file carries no "made by this tool" marker, so any existing file at that path is replaced: point `--output` at a path you are
  willing to lose, or omit `--force`.
- **Links:** the input path itself may not be a symbolic link or junction (`E_INPUT_SYMLINK`), links *inside* an input tree are skipped and
  counted (`skippedSymbolicLinks`), and an output or report that resolves through a link into the input or the output is refused. An input *file*
  whose **parent folder** is reached through a junction is accepted (only the final path component is checked); the output and report overlap
  rules still use canonical (real) paths.
- **Case-insensitive file systems:** output member names are distinct byte for byte. A zip that holds names differing only by case (which cannot
  coexist in one NTFS/APFS folder) keeps them distinct in a zip output, but extracting that zip on such a file system may merge some of them.

## Development

```bash
node --import tsx --test test/import-sanitizer.test.mjs
cargo test --locked -p mind-place-archive --test import_sanitizer_compat
node test/fixtures/import-sanitizer/generate-parser-compat.mjs   # regenerate the committed fixtures
```

The Rust test proves a sanitized synthetic export parses to the same shape (variants, roles, completeness,
warnings, attachment counts) as its original in the real ChatGPT and Gemini parsers.

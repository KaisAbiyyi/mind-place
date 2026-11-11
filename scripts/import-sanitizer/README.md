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
| `--report <file>` | Write the shareable JSON inventory. It describes the **sanitized output only**. |
| `--timestamps day\|keep` | `day` (default) keeps the date and drops the time of day, preserving number formats. `keep` leaves timestamps as they are. |
| `--keep-label <text>` | Also keep this structural HTML label, repeatable. Use it for a localized "Prompted". |
| `--keep-enum <key=value>` | Also keep this enum value verbatim (repeatable), e.g. `status=queued`. Review the report first: kept values are exempt from the leak check. |
| `--keep-class <name>` | Also keep this HTML class token (repeatable). Unknown class names are private by default. |
| `--owner-hints` | Prints frequent leading words of HTML text nodes **to the terminal only**, to help you choose `--keep-label`. Never written to the report. |
| `--salt-file <path>` / `MIND_PLACE_SANITIZER_SALT` | Pseudonym key, **opt-in** for reproducible output. By default every run uses a fresh random 256-bit key that is never written, logged or reported, so identifiers (even short, sequential ones) cannot be confirmed offline by enumerating candidates. There is deliberately no command-line salt option. |
| `--force` | Replace an existing output, but **only after the new one is fully written and verified** (the old one is moved aside and removed last, and restored if the swap fails). Refuses a directory that this tool did not create, a symbolic link, and any output that is the input, inside it, or a directory that contains it (checked on canonical paths, after resolving links). |

Exit codes: `0` PASS · `1` usage or I/O error · `2` PARTIAL (some members could not be parsed and were
replaced by empty stand-ins; see STATUS) · `3` leak check FAILED **or INCOMPLETE** (tracking overflowed, so part of
the input was never checked) · `4` structure check FAILED. On `3` and `4` no output is left behind.

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

## Performance

Streaming, memory bounded by the largest single string. Measured on a synthetic 151 MB `conversations.json`
(Linux, Node 22): about 13 s including the full verification pass, 140 MB resident, output 109 MB (measured before the
re-review changes; the new checks add negligible work). Expect
roughly 1.5 minutes for a 1 GB input. Hosted CI does not time this; the figure is a local observation.

## Development

```bash
node --import tsx --test test/import-sanitizer.test.mjs
cargo test --locked -p mind-place-archive --test import_sanitizer_compat
node test/fixtures/import-sanitizer/generate-parser-compat.mjs   # regenerate the committed fixtures
```

The Rust test proves a sanitized synthetic export parses to the same shape (variants, roles, completeness,
warnings, attachment counts) as its original in the real ChatGPT and Gemini parsers.

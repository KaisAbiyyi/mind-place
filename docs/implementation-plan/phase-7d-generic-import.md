# MP-7D — Generic transcript import adapter (JSON / Markdown / HTML)

Task: **MP-7D**; cloud claim `mp7d-20261023-2048-wib-chatgpt`; GitHub issue #66; resource lock `capture-generic`. Prerequisite MP-7A merged. This is an explicit owner-side *file import*, not another browser provider/capture observer.

## Supported V1 interchange

Only the following unambiguous textual interchange subsets are admitted. The importer does **not** scrape arbitrary web pages or infer roles from free-form documents.

- JSON UTF-8: a root array of message objects, or `{"title":"...","messages":[...]}`. Every message requires `role` equal to `user`, `assistant`, `system`, or `tool`, and **exactly one** string `content` or `text`. Optional `id`, `authored_at` (RFC3339), `model`; `attachments: []` accepted, nonempty attachments rejected pending a separate secure attachment importer. Unknown object fields, opaque rich content and ambiguous shapes are **rejected**, not silently dropped.
- Markdown UTF-8: optional single `# Title`, then one or more exact `## User`, `## Assistant`, `## System`, `## Tool` headers (also `###`). Message bodies remain Markdown. Unlabelled prose is not silently converted into a fabricated turn. Treat role-like headings inside message content as delimiters; escape or restructure before importing if they are not boundaries.
- HTML UTF-8: one or more `<article data-role="user">...</article>` or `<section data-role="assistant">...</section>` (other listed roles supported). Content is converted to plain text by removing tags, retaining paragraph breaks and decoding common entities. Scripts, styles, hidden template and nested transcript role sections are rejected. Arbitrary provider HTML exports need separate, qualified adapters.

## Source integrity, identity and privacy

The user selects a local text file from the unlocked Tauri reader Settings view. The file picker is explicit; no watched directories, broad filesystem permission, network fetch, extension host permissions or automated private-session capture are added.

The Rust importer accepts a **bounded byte buffer, not a path**. Source must be 1 byte–8 MiB. A SHA-256 digest of the exact source bytes is used as the idempotent raw artifact identity. Raw bytes are saved immutably at `archive/imports/generic/<sha256>.raw` **before** any transcript parse. On malformed/unsupported text the UI reports `raw_preserved: true`, `saved: null` and a bounded error, without creating a fake conversation. Existing raw content is verified before reuse. Only UTF-8 text can pass through the reader UI; no lossy decoding replacement is permitted. Files are not deleted or modified.

Normalized immutable Capture V1 uses provider `generic`, method `generic-import`, and a scoped synthetic internal locator `mindplace-import://generic/<sha256>` (not a user-authored URL). It never claims original provider, full history, tool execution, model or authored time unless explicitly present in records. Imported time is the *capture observation* only. Provenance warnings name adapter `generic-<format>/1` and document the ordinal record map to preserved raw bytes. All importer records receive stable ordinal IDs when source IDs are absent. `complete` is never asserted. The browser native-host route continues to accept DOM captures only, and cannot inject generic-import records.

Archive validation checks generic identities and methods separately from ChatGPT. The raw evidence is the recovery source for future higher-fidelity adapters; normalization is a reversible, derived read view. Generic provider source links display as unavailable rather than opening a forged site.

## Tests and scope boundary

Rust regression tests cover all three admitted formats, complete raw-byte readback, SHA identity, duplicate reimport/snapshot idempotence, explicit metadata provenance, missing timestamp policy, rejected unsupported JSON/HTML, byte limits, and provider/DOM spoofing. The repository baseline, Rustfmt, TypeScript typecheck, web tests, desktop build and exact-head GitHub CI are review gates: they are **not** assumed PASS until observed. Windows/native user qualification and real provider-specific exports are out of scope for this cloud task.

The phase-7 provider integration matrix remains **MP-7E** and must join this unit with MP-7B/MP-7C only after all are accepted.

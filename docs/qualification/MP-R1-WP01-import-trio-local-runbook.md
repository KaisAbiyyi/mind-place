# MP-R1-WP01 · Import trio — local runbook (absorbs MP-IMPORT-L1)

Owner-run, on Windows x64, on **real** exports, into a **disposable** archive. Agents never run this and never see the exports. Until the owner reports a result, the local gate of MP-R1-WP01 is `NOT_RUN`.

## What this proves and what it does not

- Proves, for your own exports: the documented layouts are found, every record is preserved as raw evidence whose bytes hash to their name, the projection counts add up, a second run is idempotent, and nothing touches your real archive.
- Exercises the same archive-core job (`import_authorized_source`) that the app's `start_import` command calls, under a throwaway security gate. It does **not** exercise the in-app picker or Import dialog: the native selection-grant picker (`G6`) has not landed yet, so the app's import commands still refuse every request with `E_IMPORT_SELECTION`. That part stays `NOT_RUN` until the picker exists.
- Does not claim lossless import. Raw-only and partial outcomes are expected and honest (see "Reading the result").

## Before you start

1. Pull the exact PR head you are qualifying and note its SHA: `git rev-parse HEAD`.
2. Download fresh exports (do not edit or unzip-and-rezip them):
   - ChatGPT: Settings → Data controls → Export data (a `.zip` by e-mail).
   - Gemini: Google Takeout → "My Activity", Gemini Apps only (a `.zip`).
   - Claude.ai: Settings → Privacy → Export data (a `.zip` by e-mail).
3. Put the exports somewhere outside the repository, for example `D:\mp-import-l1\exports\`.
4. Pick a **new** directory for each run, for example `D:\mp-import-l1\run-claude-1`. The harness refuses a directory that is not empty, that is inside the export, or that is inside the real Mind Place data directory.
5. Optional: pause antivirus scanning of `D:\mp-import-l1\` only if a run fails with a sharing violation, and record that you did.

## Run (one provider at a time)

```powershell
cargo build --release -p mind-place-archive --example import_trio_local
```

```powershell
cargo run --release -p mind-place-archive --example import_trio_local -- claude D:\mp-import-l1\exports\claude-export.zip D:\mp-import-l1\run-claude-1
```

```powershell
cargo run --release -p mind-place-archive --example import_trio_local -- chatgpt D:\mp-import-l1\exports\chatgpt-export.zip D:\mp-import-l1\run-chatgpt-1
```

```powershell
cargo run --release -p mind-place-archive --example import_trio_local -- gemini D:\mp-import-l1\exports\takeout.zip D:\mp-import-l1\run-gemini-1
```

Repeat one provider with the **extracted folder** instead of the `.zip` (new run directory) to check that a folder source behaves the same.

The harness prints counts, member names of the export (for example `conversations.json`), content-free reason and warning categories, and `RESULT=PASS` or `FAIL`. It never prints titles, text, identifiers or other file names.

## Reading the result

| Line | Expect |
| --- | --- |
| `candidate_members` | ChatGPT: `conversations.json` (or `conversations-NNN.json` shards). Gemini: the `My Activity/Gemini Apps/MyActivity.html` path (localized folder names are fine). Claude: `conversations.json` and `projects.json`; **never** `users.json`. `RESULT=BLOCKED` means no member was found: record the folder names (only) and report. |
| `run=1` | `scanned = imported + deduplicated + raw_only + rejected`. `rejected` should be 0 or very small. |
| `run=2` | `already_complete=true`, same counts: re-import is idempotent. |
| `member_receipt` | `unpreserved_bytes` is small (array framing, whitespace, Takeout page chrome). A large value means records were not found where expected: report it. |
| reasons | Claude: projects are always raw-only; conversations whose parent links fork are raw-only by design. ChatGPT: branching DAGs without `current_node` are raw-only by design. |
| `completeness` | Claude exports without parent links, or with thinking/tool blocks, are `partial` by design (nothing is guessed). |
| `raw_blobs_hash_mismatch` | must be `0`. |

## Report back (copy into the PR or the Notion card)

```text
MP-R1-WP01 local qualification
SHA: <exact head>
Machine: Windows <version>, x64
Antivirus paused: yes/no
ChatGPT zip:   RESULT=<PASS|FAIL|BLOCKED>  scanned/imported/raw_only/rejected = ...
ChatGPT folder (optional): ...
Gemini zip:    RESULT=...  ...
Claude zip:    RESULT=...  ...
Claude folder: RESULT=...  ...
Unexpected reasons or warning categories: <paste the category lines only>
Layout notes (folder/member names only, no content): ...
Overall: PASS | FAIL | PARTIAL | BLOCKED
```

Paste **only** the harness output lines. Do not attach the exports, the run directories, or screenshots of conversations. If you want to share structure for a parser bug, run the sanitizer first and review its report before sharing:

```powershell
node scripts/import-sanitizer/cli.mjs --input D:\mp-import-l1\exports\claude-export.zip --output D:\mp-import-l1\sanitized-claude.zip --report D:\mp-import-l1\sanitized-claude-report.json
```

## Clean up

Delete each `D:\mp-import-l1\run-*` directory when done. They hold full copies of your conversations (raw evidence and projections) in plaintext.

## If something fails

Send the failing lines (and, if needed, a sanitized export) back to a cloud fix unit. Do not patch the local checkout to make the run pass.

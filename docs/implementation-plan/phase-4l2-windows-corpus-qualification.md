# MP-4L2 — Native Windows document corpus qualification

Task: [GitHub #39](https://github.com/KaisAbiyyi/mind-place/issues/39). Review:
[PR #40](https://github.com/KaisAbiyyi/mind-place/pull/40).

## Scope and provenance

- Qualified **product baseline**: `5301ad773d32a5ac88ba5ec9dcc1232c8b8e8f10`, fetched and fast-forward checked out from `origin/main`.
- Native Windows x64, Rust 1.91.1 (MSVC), D: NTFS. Qualification-only Rust example and PowerShell runner; no product behavior, schema or release packaging was modified.
- `scripts/qualify-corpus.ps1 -EvidenceDirectory D:\MindPlaceQualification\<unique-run>` requires a fresh, non-reparse NTFS evidence path outside the repository. It rejects product source/lockfile drift from the pinned baseline. All generated data is retained for inspection; cleanup is manual.
- The test explicitly approves its own **disposable synthetic** folder. It neither opens nor enrolls the owner's private document directory or Mind Place archive. It does not qualify an installed desktop application's user interface.

## Windows NTFS fixture run — 22 October 2026

Run ID: `mp4l2-20261022-run03`; native receipt `PASS` (2 cases, 0 failures; Cargo exit 0).

| Test / metric | Observed |
| --- | ---: |
| Approved fixture files | 31 |
| Formats | Markdown, plain text, Rust, PDF, DOCX, PPTX, XLSX |
| Initial scanned / newly preserved | 31 / 31 |
| Initial indexed files / chunks | 31 / 31 |
| Initial scan / index | 2,719 ms / 9,146 ms |
| No-op rescan / reindex | 2,832 ms / 8,468 ms |
| Changed-file rescan / reindex | 2,807 ms / 7,742 ms |
| No-op retained heads | 31, no new versions |
| Changed-file versions | 1 new, 30 unchanged |
| Content-addressed objects (before / after edit) | 30 / 31 |
| Immutable history on edited Markdown file | 2 readable versions |
| Format-specific search and stale-content checks | PASS |
| Excluded `.git` canary | Not indexed, not returned |
| Parser stress checks | 7/7 PASS |

Parser checks assert bounded input bytes, output bytes, chunk count, ZIP declared expansion, OOXML depth, PDF page count, and ZIP path traversal rejection. They exercise real Rust parsers but do not constitute a hostile-input process sandbox or complete PDF/OCR fidelity qualification. Timing numbers describe this modest synthetic corpus on this device; they are evidence, not a production throughput target.

The local evidence directory contains the original native `native-receipt.json`, `run-metadata.json`, source/output hashes, and separate build/runtime transcripts. Public task evidence deliberately excludes fixture contents and machine-local absolute source paths.

The independent repeat `mp4l2-20261022-run04` also returned **PASS** (2/2 cases, Cargo exit 0) on harness HEAD `0a7eb989eba5ec24b3c74dd882fedd250b86bc16`, against the same pinned product baseline. Run04 recorded initial scan/index **2,658 / 9,423 ms**, no-op scan/index **3,010 / 8,987 ms**, and edited-file scan/index **3,164 / 10,000 ms**. The run04 native receipt SHA-256 is `1DE15066BF572E9B58FFBC31A06C46A536C96ECA4AD81D4591D2562403C15764`. A subsequent hardening change removes the runner's caller-supplied baseline override; a negative invocation with `-ProductSha` fails before any evidence directory is created.

## Remaining acceptance boundary

The generated corpus validates native Windows behavior and the integration between scanning, immutable storage, parsing and search. The phase plan additionally asks for **a representative user-approved real document folder** to establish realistic document distributions, initial/no-op/changed-file latency and PDF/Office extraction coverage. That dataset has not been selected or scanned in this run. Consequently the synthetic-native case is **PASS**, while the full MP-4L2 real-corpus acceptance remains **PARTIAL / LOCAL gate** until evidence from an explicitly approved folder is available. MP-4U owner UX acceptance is separate and follows MP-4L2.

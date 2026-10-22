# MP-4L2 — owner-approved real document sample, Windows NTFS

## Permission and isolation

On 2026-10-22, the owner approved local **`D:\projects`** for MP-4L2 qualification (correcting a typo `D:\projectss`). The real-file sample runner accepts an explicit manifest of 5–50 ordinary files under that approved root. It rejects generated/sensitive subtrees, reparse-point traversal, non-D sources or evidence, unsupported extensions and oversized inputs. Source files are **copied into disposable D: staging**; only a generated staging marker is edited for the changed-version test. Nothing modifies the owner's source repositories, archive or installed desktop application. The manifest and content are local-only; public reports contain aggregate metrics only.

Reproduce with:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/qualify-real-corpus.ps1 `
  -ApprovedRoot 'D:\projects' `
  -ManifestFile 'D:\MindPlaceQualification\mp4l2-real-manifest-20261022.json' `
  -EvidenceDirectory 'D:\MindPlaceQualification\mp4l2-real-<fresh-run-id>'
```

The manifest is intentionally **not** committed or uploaded. This real-sample tool is complementary to `scripts/qualify-corpus.ps1` (31 generated fixtures, seven parser limit checks, three native PASS receipts in PR #40). Do not confuse the synthetic runner with coverage of actual Office/PDF files.

## Real sample — initial receipts

- Product main baseline: `08b310b2b14639594edcf5210d9a28b8b677846f` (PR #40 merged).
- Native Windows x64, NTFS D:, Rust 1.91.1 MSVC. Nine owner-approved real source files: 3 Markdown, 1 Rust source, 2 PDF, 2 DOCX, 2 PPTX; plus one generated marker = 10 scanned files. Genuine XLSX was not found in the selected sample.
- First measured run: `D:\MindPlaceQualification\mp4l2-real-20261022-run02`. Native receipt: **PARTIAL**.
- Initial scan 2,768 ms, 10 new immutable versions; initial derived index 19,855 ms, **4 indexed / 6 skipped**.
- No-op scan 3,288 ms, 10 unchanged, 0 new versions; reindex 20,651 ms.
- After staging-only marker edit, scan 3,674 ms, 1 new and 9 unchanged; reindex 19,272 ms, still **4 indexed / 6 skipped**.
- Version history has 2 readable versions, old FTS hit suppressed, new FTS hit returned: **PASS**.
- Direct extraction: DOCX 2/2, PPTX 2/2, Markdown 3/3 and Rust 1/1 nonempty; PDF 1/2 nonempty with 1 direct parser error. Therefore **real PDF fidelity is incomplete even without live lease checks**.
- Read-lease diagnostic repeat `run03`: six binary files encountered extraction limits with ~2,000 ms per file during a repeated live authorization check; 2 DOCX, 2 PPTX, 2 PDF. All 4 text/source inputs passed the same checked path. This is evidence of a runtime authorization-check overhead versus parser wall-time budget, **not** permission to bypass or relax security. See `real-receipt.json` in local evidence.

## Outcome and follow-up

**PARTIAL / LOCAL GATE (MP-4L2 stays open; MP-4U still gated).** All 10 files are safely scanned/preserved and version transitions behave correctly, but the real Office/PDF indexing requirement fails (6 of 10 skipped). A separately reviewed change must reconcile parser budget with live NTFS authorization checks without weakening revocation or disclosure gating. The PDF direct-extraction failure also needs format-specific coverage/triage. Repeat final-head native qualification on real documents and, if required, expand the owner-approved sample to real XLSX before promoting the gate. Never publish original filenames, excerpts, document contents or local manifest.

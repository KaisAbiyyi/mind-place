# MP-4L2 — Windows parser and live-authorization recovery

Date: 2026-10-23 (Asia/Jakarta). Task: MP-4L2. Executor: Codex Local, session mp4l2-20261023, claim generation 3 after generation 2 was explicitly released. Owner authorized bounded local implementation in this session. Exclusive resources: windows-corpus and parser/NTFS authorization recovery. PR #42 supersedes the reproducer-only PR #41.

## Implemented

- Each live document permission check now uses one fresh read-only SQLite connection and one joined statement. Path safety, NOFOLLOW, database identity/schema, digest validation, immutable grant IDs, terminal tombstones and fresh committed revocation checks remain enforced. No permission booleans or long-lived snapshots are cached.
- XML parsing checks the elapsed deadline every event and performs full live authorization at bounded 256-event/4096-byte work quanta, plus existing entry/output/final checks. A single XML event remains bounded by input/expanded limits and is read atomically.
- PDF decode/explicit scan loops use a bounded 32 KiB live authorization cadence (per-iteration/8 KiB cheap deadlines, checks before decode and after each expanded read), preserves CR/LF bytes belonging to the compressed checksum, recognizes ET outside literal strings, rejects embedded BT bytes as text operators, polls by consumed bytes and isolates current stream filters through bounded dictionary lexing. Unsupported filters, OCR, CMaps and full PDF semantics remain outside the conservative extractor contract.
- Extractor revision advances to mp-4e:2 so derived indexes rebuild. Raw revisions are untouched.
- The opt-in qualification runner records the tested clean product SHA and explicit debug/release profile. Private manifests and real document content stay local. Only the disposable staging marker is changed.

## Verification and evidence

Baseline reproducer: main 08b310b2b14639594edcf5210d9a28b8b677846f, harness 872b211. Unchanged debug and optimized release both indexed only 4/10; the runner correctly returned PARTIAL/exit 3. Intermediate failed attempts remain local and are not reclassified as passes.

Independent code review: gpt-6-luna reviewer approved b8b3807 after focused document/state tests, including fresh post-revocation reads, corrupt grant/digest rejection, stored OOXML mid-parser cancellation, PDF checksum CR/LF, ET inside literals, prior-stream and prior-nonstream filter cases. General PDF capability caveats remain explicit. Review is code evidence, separate from native performance.

Native optimized Windows qualification **PASS** on clean product SHA `60233a054974a3b0ede157fb8962c34302142b2e`, Rust 1.91.1 x86_64-pc-windows-msvc, Windows 11 build 26300 / D: NTFS. Local receipt: `D:\MindPlaceQualification\mp4l2-real-20261023-release09\real-receipt.json`; source manifest and contents are not uploaded.

| Gate | Result |
|---|---|
| Nine real samples + one disposable marker | 10/10 indexed, 0 skips |
| Initial scan | 10 new versions; 3.562 s full indexing |
| No-op scan | 0 new / 10 unchanged; 10 indexed |
| Changed marker scan | 1 new / 9 unchanged; 10 indexed, 0 skips |
| PDF / DOCX / PPTX live extraction | 2/2 each; combined per-format 850 / 1003 / 516 ms, each file below unchanged 2000 ms bound |
| Immutable history / stale suppression / fresh search | PASS |
| Original source SHA256 before/after | 9/9 unchanged |
| Runtime receipt / process | PASS / exit 0 |

Earlier debug runs under compilation load and the pre-32-KiB release run remained PARTIAL and are preserved. Acceptance qualifies the optimized product runtime, not universal debug-build throughput under concurrent compilers.

Full local baseline **PASS** on the final product code: `npm run verify:repo`, `npm run typecheck`, `npm test`, `cargo fmt --all -- --check`, and `npm run build` (Windows optimized desktop payload 31.68 MiB). All 14 focused extractor tests pass, including revocation immediately after the first expanded PDF decode read. No release packaging behavior changed; installer acceptance is not claimed.

Required integration: final-head Windows product/storage CI must pass before reviewed merge. GitHub PR #42 records those live results; this native receipt alone does not imply merge or owner acceptance.

MP-4U is the next separate owner workflow gate. Installed UI acceptance and unsampled formats/files are not inferred from parser qualification. A long literal, dictionary token or uncompressed copy remains a single operation under existing input/expanded bounds; 32 KiB is not a universal per-operation cancellation ceiling.

Genuine XLSX was unavailable in the approved sample; synthetic XLSX remains covered by the existing extractor gate.

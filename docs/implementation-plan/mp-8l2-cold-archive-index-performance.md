# MP-8L2-PERF-FIX-01 — cold Windows archive listing and index write fencing

**Scope:** [issue #121](https://github.com/KaisAbiyyi/mind-place/issues/121), [PR #122](https://github.com/KaisAbiyyi/mind-place/pull/122). Works alongside the separate Windows qualification [PR #120](https://github.com/KaisAbiyyi/mind-place/pull/120); no owner corpus is read or modified.

## Reproduction and diagnosis (25 October 2026)

- The same qualified native WebView2 executable, SHA-256 `e437c9d3f741c7cbfbe700800ab435777fd502028cca6bedd52a03361caf811d`, showed **102,906.8 ms** and **90,216.0 ms** first unlock-to-list with **3,000 native-host-created synthetic conversations**, 131 extra immutable revision snapshots, 11/11 functional checks PASS per run, no page errors. The 800-source run was 859.3 ms.
- An initial mitigation—waiting until `ReaderHandle.ready` before automatic derived-index refresh—preserved all 181 web tests but did **not** fix this workload: a new exact-head Windows build at `e93462cfa5f314ce223b12bcee63085a5405ab72` took **91,642.8 ms** on 3,000 synthetic conversations, 11/11 checks PASS. That negative result was not presented as an improvement.
- A standalone release-mode native archive profiler reproduced the underlying cold file I/O without WebView2: on one newly copied 3,000-source synthetic directory **107,880 ms** for the first `ArchiveStore::list("")`, then **1,811 / 1,735 ms** for repeated hot reads. The original source fixture already warm in cache took about 1 second.
- Bounded metadata parallelism (up to 8 workers, no async raw archive mutation, no cache of unverified file content) on a **different fresh synthetic copy** took **10,226 ms** cold and **491 / 484 ms** warm; ~10.5× cold-read speedup and ~3.7× warm-read speedup. The copies were different and overall host load was not fixed, so these are comparative observations, not a controlled hardware-independent promise.

## Product changes and safety

1. `ArchiveStore::list` retains the same bounded file reading, per-directory symlink/reparse checks, date-backfill behavior, UTF-8/search error handling, stable final date/title/id sorting, and fail-closed errors. For archives with 160+ entries, it partitions entries among up to eight scoped worker threads. Small archives remain sequential. Raw captures and metadata are never rewritten.
2. The initial reader list settles before a background index refresh starts. A stale reader or unlock generation cannot schedule work after Lock.
3. Native `refresh_archive_index` and `rebuild_archive_index` share one in-process exclusive writer gate. Queued writers revalidate their unlock lease inside the gate before opening a derived-index write transaction. Index writes still check their lease throughout scans and before commit. This prevents competing transactions launched through the desktop API from receiving unnecessary SQLite BUSY failures, without changing index contents or authorizing a locked user.
4. `crates/archive-core/examples/mp8l2_archive_profile.rs` accepts only a D:-resident synthetic MP-8L2 fixture path; it prints timing and item counts, not content.

## Test and acceptance evidence

- JS: `npm run verify:repo`, `npm run typecheck`, `npm run build:web`, and **181/181** `npm run test:web` PASS at source commit `e93462c` for the frontend change.
- Rust: the 160-item regression fixture verifies stable order, title search, and byte-identical immutable snapshots across the parallel list path. New Tauri tests verify current writer authorization and fail-closed queued work after Lock.
- To profile the real native archive path without source data disclosure:

  ```powershell
  cargo run --release -p mind-place-archive --example mp8l2_archive_profile -- 'D:\projects\mp8l2-native-runs\<isolated-synthetic-id>\archive-v1'
  ```

**Final native acceptance:** verify the final source SHA and executable SHA, rerun WebView2 on fresh synthetic 800/3,000-source roots with startup, virtualization, graph, scroll/zoom, index and working-set receipts; repeat the 3,000-source path under recorded device-load conditions. Hosted CI does not substitute for local Windows qualification. Do not promote MP-8L2 or unlock MP-8U2 until the required acceptance is actually supported.

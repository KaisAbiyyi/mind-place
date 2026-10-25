# MP-8L2 — Windows native UI performance qualification

**Task / issue:** MP-8L2 · [#119](https://github.com/KaisAbiyyi/mind-place/issues/119), [draft PR #120](https://github.com/KaisAbiyyi/mind-place/pull/120).  
**Claim:** ChatGPT GPT-6 via Chat On Steroids Core, `mp8l2-20261025-1055-wib`, generation 1, resource lock `windows-ui-benchmark`.  
**Predecessor:** MP-8L1 local functional qualification Done/PASS, owner-authenticated final-main checks.  
**Overall verdict: PARTIAL / REVIEW.** All 11 executable checks passed at 800 and 3,000 conversations; numeric latency/RAM acceptance, controlled repeat runs and representative owner workload remain unqualified. **MP-8U2 is not unlocked yet.**

## Exact Windows executable / workload

- Packaged Tauri/WebView2 app built from `main` commit `bfff35463880533e25866efae7e4f14e004cafd6`. Packaged `mind-place.exe` SHA-256: `e437c9d3f741c7cbfbe700800ab435777fd502028cca6bedd52a03361caf811d`; identical executable in both runs. Native capture-host SHA also recorded in the local JSON receipts.
- Windows 11 build 26300, x64, 24 logical processors, 16 GiB RAM. Each tier used a new `D:`-resident archive/security/index/WebView2 profile. The owner's actual archive was never used or changed.
- The 3,000-tier receipt's `runner_source_sha` identifies the then-current Git HEAD (it does **not** prove the runner had no uncommitted changes); SHA-256 of the exact benchmark runner source for that tier was `71cda575f04e5886bad4ecee80b832dca15200f7d89ef0e60fb8019ef8e9f295`. The new runner additionally records its own source-byte hash for future runs.
- Synthetic data was generated using the **real packaged native-messaging host**: 800 or 3,000 conversations, each with two short messages; one seed contains 131 immutable revisions. Derived search/index/graph created by the **real native Tauri backend**. The second tier reused a copy of native-host-created synthetic archive bytes in a fresh runtime.
- The graph's real scoped traversal returns **120 nodes / 119 edges**, explicitly truncated at the 120-node safety cap. It is not evidence for an unrestricted 1M-node graph or 240-edge worst case.
- CDP connects to the real WebView2 process; no browser-only Chromium simulation or fake graph API response. The instrumentation itself changes timing. The five RAM snapshots total Windows working-set bytes for the app and its WebView2 child-process tree, including process overhead/shared mappings.

## Native results (2026-10-25, WIB)

| Observation | 800 conversations | 3,000 conversations |
| --- | ---: | ---: |
| Derived indexed sources / revisions / chunks | 800 / 930 / 2,790 | 3,000 / 3,130 / 9,390 |
| Virtualized list DOM entries | 9 | 9 |
| Locked native startup, one sample | 631.5 ms | 520.6 ms |
| Unlock until virtualized list ready, one sample | 859.3 ms | **102,906.8 ms** |
| Native index synchronization/readiness after navigation, one sample | 9,940.2 ms | 30,376.5 ms |
| Virtualized scroll, 20 alternations, median / p95 | 21.0 / 27.3 ms | 20.9 / 21.1 ms |
| Workspace navigation, 12 switches, median / p95 | 26.6 / 37.7 ms | 23.3 / 29.8 ms |
| Native graph neighborhood, one sample | 75.4 ms | 100.8 ms |
| Native graph rerender, one sample | 86.0 ms | 148.9 ms |
| Hive zoom, 8 operations, median / p95 | 17.6 / 18.6 ms | 14.7 / 18.4 ms |
| Hive revision filter, 6 operations, median / p95 | 11.3 / 15.5 ms | 9.8 / 15.9 ms |
| Retrieval inspector, 10 requests, median / p95 | 77.9 / 185.4 ms | 188.2 / 199.1 ms |
| Process-tree working set after unlock | 401.5 MiB | 410.3 MiB |
| Working set after scroll → navigation → Hive → inspector | 459.4 → 479.0 → 490.9 → 492.6 MiB | 423.3 → 439.9 → 454.8 → 455.3 MiB |
| Within-run RAM difference (first vs last) | +91.1 MiB | +45.0 MiB |
| JavaScript page errors | 0 | 0 |
| Executable qualification checks | 11 PASS | 11 PASS |

**Key observation:** Once the large library is responsive, DOM virtualization remained bounded and scrolling/zoom remained usable in both tiers. However, the 3,000-source first unlock-to-list interval was approximately **103 seconds**, greatly exceeding the 800-source run. Index completion required another measured ~30 seconds in that run. This is a candidate first-load/index contention problem and must not be dismissed because interaction tests later passed. A single run at each tier and uncontrolled host load cannot establish a stable latency regression or root cause.

Independent cloud-first performance investigation: [MP-8L2-PERF-FIX-01, issue #121](https://github.com/KaisAbiyyi/mind-place/issues/121).

One initial 3,000-tier experiment failed with `MP_INDEX_SQL:index SQLite: database is locked` after the runner invoked `refresh_archive_index` concurrently with the application's own post-unlock background refresh. The revised runner **waits for the existing native indexing job** using read-only `get_archive_index_stats`, validating all sources and revisions before querying graph. The fresh-runtime rerun passed. The initial contention failure is retained in its local failure receipt; the runner does not silently classify it as PASS.

## Reproducibility and remaining acceptance

Runner: `scripts/mp8l2-native-performance.mjs` in the dedicated worktree. From PowerShell, with the exact qualified packaged binary available:

```powershell
$env:MP8L2_PACKAGE_ROOT = 'D:\projects\mind-place-mp8l1-final-qualification-20261025'
node scripts/mp8l2-native-performance.mjs --conversations 800
node scripts/mp8l2-native-performance.mjs --conversations 3000
```

To replay an existing **synthetic-only** tier in a fresh isolated security/profile root, use `--source-fixture 'D:\projects\mp8l2-native-runs\<run-id>\archive-v1'`. The runner validates the source is under the MP-8L2 synthetic qualification root, checks source count and the 131-revision synthetic graph identity, and writes only to a new folder.

Local machine-readable receipts (intentionally excluded from GitHub and containing no owner data):

- `D:\projects\mind-place-mp8l2-20261025\outputs\mp-8l2\2026-10-25T04-07-41-641Z\receipt.json` (800).
- `D:\projects\mind-place-mp8l2-20261025\outputs\mp-8l2\2026-10-25T04-13-14-822Z\receipt.json` (3,000).

The exact qualification result is **PARTIAL**, pending (1) repeated warm/cold trials under recorded idle and busy host-load conditions, (2) investigation of the slow first unlock and index concurrency, (3) explicit latency/RAM acceptance budgets before declaring performance PASS, and (4) a representative graph/owner workload where authorized. Process-tree working-set growth across one sequence is not by itself proof of a memory leak. Do not treat cloud CI or two synthetic native test passes as MP-8U2 human Hive acceptance.

Security and product code were not modified in this PR. The test records original executable SHA, source commit, sanitized timings, bounds and known limitations; private owner PIN, corpus, UI screenshots and credentials are never included.

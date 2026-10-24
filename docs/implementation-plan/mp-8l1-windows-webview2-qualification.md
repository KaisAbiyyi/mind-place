# MP-8L1 — native Windows WebView2 qualification

**Task:** MP-8L1 · packaged desktop functional qualification (LOCAL)

**Issue:** https://github.com/KaisAbiyyi/mind-place/issues/101

**Claim:** ChatGPT GPT-6 via Chat On Steroids Core, `mp8l1-20261024-2219-wib`, generation 1, logical lock `windows-ui`. Exclusive ownership is manually coordinated; this repository has no atomic claim controller.

**Predecessor:** MP-8F PR #95, merged at `f6b098e4462a11c42414d6c65d55ea9ca3f1edf6`.

**Qualification source:** `cd0b10fd53c21b6f967d31d57a0c0b175f0c50c8` (`origin/main`, fetched 2026-10-24). Any tested binary must be tied to its actual build/commit SHA; an older installed build is insufficient.

## Acceptance method

Use a Windows x64 **packaged Tauri build** and a real Microsoft WebView2 runtime. A browser-only Vite preview or synthetic Chromium screenshots cannot satisfy this gate. For every observation, record the installed package/executable hash, runtime version, Windows scaling/resolution, test time, actual result, and sanitized evidence. Use `PASS`, `FAIL`, `PARTIAL`, `BLOCKED`, or `NOT_RUN`. Do not equate a successful build or CI run with interactive native acceptance.

For smoke tests with test-only data, run with a dedicated absolute `MIND_PLACE_DATA_DIR` and record that it is synthetic. Tests that require a real authorized archive, owner unlock, or meaningful populated search/memory/graph records must be identified separately. Do not modify or expose private archive contents or credentials in the issue, PR, screenshots, or log artifacts.

## Required checks

| Gate | Required observation | Result |
| --- | --- | --- |
| Package provenance | Exact build commit, SHA-256 of packaged EXE/installer, Windows x64 and WebView2 runtime | NOT_RUN |
| Cold start | Process and visible WebView2 window start without error; record startup duration | NOT_RUN |
| Lock/privacy | Locked launch suppresses private data and privileged navigation until owner authorizes it | NOT_RUN |
| Workspace navigation | Main library, search, memories, projects/entities, retrieval/health, Hive route correctly | NOT_RUN |
| Functional content | Real authorized archive/index gives coherent source, search, context, memory and health detail | NOT_RUN |
| Keyboard accessibility | Keyboard navigation, visible focus, Escape/dialog close and focus restoration | NOT_RUN |
| Layout | Desktop display scaling/resolution and narrow-window text clipping/overflow checks | NOT_RUN |
| Restart/persistence | Close and relaunch; no unintended reset, corruption or lost state | NOT_RUN |
| Responsiveness baseline | Record process and WebView2 child memory and representative navigation latency | NOT_RUN |

## Status and interpretation

**In progress.** The Windows host has a running Mind Place process from a separate Phase 7 worktree and a previously packaged installer dated 2026-10-23. Neither establishes MP-8L1 acceptance for MP-8F or later. A build from the exact qualification source and native observations are required. MP-8L2 and MP-8U1 remain blocked until MP-8L1 is actually `PASS`.

Local acceptance is independent of cloud CI. Native UI defects should be triaged into bounded cloud repair units before claiming the local gate `PASS`.

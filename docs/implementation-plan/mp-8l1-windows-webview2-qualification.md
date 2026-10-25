# MP-8L1 — native Windows WebView2 qualification

**Task:** MP-8L1 · packaged desktop functional qualification (LOCAL)

**Issue:** https://github.com/KaisAbiyyi/mind-place/issues/101

**Claim:** ChatGPT GPT-6 via Chat On Steroids Core, `mp8l1-20261024-2219-wib`, generation 1, logical lock `windows-ui`. Exclusive ownership is manually coordinated; this repository has no atomic claim controller.

**Predecessor:** MP-8F PR #95, merged at `f6b098e4462a11c42414d6c65d55ea9ca3f1edf6`.

**Qualification source:** `cd0b10fd53c21b6f967d31d57a0c0b175f0c50c8` (`origin/main`, fetched 2026-10-24). Any tested binary must be tied to its actual build/commit SHA; an older installed build is insufficient.

**Native evidence host:** Windows 11 Home Single Language build 26300; WebView2 runtime `155.0.4283.62`; primary display 2560×1440, additional graphics output 1920×1080. Actual DPI scaling is not yet measured. Source build uses the above `main` SHA plus *qualification-only documentation/runner commits*; no product source change was included in the tested executable. Observations made 2026-10-24 on an isolated D: worktree.

**Packaged artifacts:** Tauri NSIS bundle `Mind Place_0.2.5_x64-setup.exe` is 9.39 MiB (SHA-256 `40e1b7acdd182faaf03349c88ea246ae47617249a6b2b0a13d23717f9ff213e5`). The staged packaged `mind-place.exe` SHA-256 is `37ae2d7dff273737fbbb9d8a85ec71cfe871727dc9526c0de171f2e5a30086d5`; bundled native host SHA-256 is `24c484e3cd4480e7f85c597d8d1061878e23a361c5499e1b19833676b276ce72`. Package generation passed, but NSIS setup execution/uninstall/upgrade was **not run**.

## Acceptance method

Use a Windows x64 **packaged Tauri build** and a real Microsoft WebView2 runtime. A browser-only Vite preview or synthetic Chromium screenshots cannot satisfy this gate. For every observation, record the installed package/executable hash, runtime version, Windows scaling/resolution, test time, actual result, and sanitized evidence. Use `PASS`, `FAIL`, `PARTIAL`, `BLOCKED`, or `NOT_RUN`. Do not equate a successful build or CI run with interactive native acceptance.

For smoke tests with test-only data, run with a dedicated absolute `MIND_PLACE_DATA_DIR` **and a unique parent directory**: Mind Place stores security state and authoritative/index SQLite files beside `archive-v1`, under that parent. Record that the corpus is synthetic. Tests that require a real authorized archive, owner unlock, or meaningful populated search/memory/graph records must be identified separately. Do not modify or expose private archive contents or credentials in the issue, PR, screenshots, or log artifacts.

## Required checks

| Gate | Required observation | Result |
| --- | --- | --- |
| Package provenance | Exact build commit, SHA-256 of packaged EXE/installer, Windows x64 and WebView2 runtime | PASS for build and portable payload, NOT_RUN for installed NSIS execution |
| Cold start | Process and visible WebView2 window start without error; record startup duration | PASS; native CDP smoke on staged package, 1,047 ms in final baseline run (instrumented) |
| Lock/privacy | Locked launch suppresses private data and privileged navigation until owner authorizes it | PASS; first run and restart locked until synthetic unlock |
| Workspace navigation | Main library, search, memories, projects/entities, retrieval/health, Hive route correctly | PASS for nine workspace routes and selected navigation state |
| Functional content | Real authorized archive/index gives coherent source, search, context, memory and health detail | **FAIL for indexed title source preview**; native FTS5 message preview, synthetic archive list/search PASS; owner corpus NOT_RUN |
| Keyboard accessibility | Keyboard navigation, visible focus, Escape/dialog close and focus restoration | PASS for arrow focus + Settings Escape and trigger focus restoration; fuller keyboard/screen reader coverage NOT_RUN |
| Layout | Desktop display scaling/resolution and narrow-window text clipping/overflow checks | PARTIAL; real WebView2 CDP-emulated 390×700 viewport passes document overflow probe, physical DPI/window resizing NOT_RUN |
| Restart/persistence | Close and relaunch; no unintended reset, corruption or lost state | PASS on unique synthetic security/archive parent; final baseline restart to locked gate 506 ms (instrumented) |
| Responsiveness baseline | Record process and WebView2 child memory and representative navigation latency | PARTIAL: instrumented cold/restart timings; representative native memory and broad navigation latency still NOT_RUN |

## Status and interpretation

**Local baseline FAIL (20 PASS / 1 FAIL, 21 executable native checks); overall MP-8L1 remains PARTIAL/BLOCKED pending repair, installed NSIS and owner/device gates.** A fresh synthetic archive and private security state were created under `outputs/mp-8l1/isolated-runtime-*`, independent of a simultaneously running Phase 7 Mind Place process. The executable's real Microsoft WebView2 started, created an unlock secret, displayed nine workspaces, retrieved synthetic conversations, searched indexed message text, previewed an authenticated message, preserved data/lock on restart, and returned no JavaScript page errors. The app did **not** pass the indexed title preview.

Reproduction: open Library search, query `Rust`, click the first result indexed as `kind=title`. Native `read_conversation` succeeds and archive/snapshot identity match, but UI displays `Source preview unavailable or no longer authorized`. The index writes title chunks with reserved `message_id=@title`, while `src/library-workspace.ts` searched for that ID among `capture.messages`. Querying `Tauri` and previewing a normal message works. The cloud-only repair is tracked in [issue #113](https://github.com/KaisAbiyyi/mind-place/issues/113) / [draft PR #114](https://github.com/KaisAbiyyi/mind-place/pull/114); repeat native qualification against the exact fixed SHA after cloud CI/review. Source baseline runner and sanitized machine-readable receipt: `scripts/mp8l1-native-smoke.mjs` and local `outputs/mp-8l1/native-webview2-receipt.json`; screenshots are synthetic and kept locally on D:.

### Cloud repair candidate requalification (separate from baseline)

The cloud GitHub-only repair's exact head is `3590af735b569033849843e2398da859ab17a082` (PR #114). A separate Windows worktree, `D:\projects\mind-place-mp8l1-fix-qualification`, checked out this SHA without local code edits; its untracked copy of the qualification runner is not part of the product build. `npm run dist:win` built the candidate from that head. Candidate staged EXE SHA-256: `c3400185abea0e3d1097ba5db9b831bb8cec519c5ed402d6199e59525455542b`. NSIS SHA-256: `97eed3e2d0c0e2f1d3724e925ed51973735d1e7b46d5596ad2173eea0d3a9b6f` (generated, not installed).

**Native packaged WebView2 synthetic smoke: PASS 21/21**, including title-hit source preview, ordinary message-hit source preview, title/message searches, all workspace routes, modal keyboard/focus, locked startup and restart, and isolated captured archive persistence. No page JavaScript errors. Instrumented candidate startup was **1,537 ms** and restart to locked gate **745 ms**; neither is a controlled release benchmark. Machine-readable receipt remains local under the fix worktree's `outputs/mp-8l1/native-webview2-receipt.json`. Focused cloud-repair unit tests **4/4 PASS**, repo contract and TypeScript typecheck **PASS**. Windows CI and independent review remain separate gates; do not merge PR #114 based only on native synthetic PASS.

The baseline FAIL is reproducible and remains historical evidence; the fix candidate resolves it in native smoke. Overall **MP-8L1 remains PARTIAL** until the reviewed repair lands on main and full installed/owner qualification is completed.

### Owner's actual Windows DPI scaling screenshots (2026-10-24)

The owner supplied **four ordered native Mind Place screenshots**, explicitly identifying display scaling as **100%, 125%, 150%, 175%**. The real loaded Gemini snapshot and reader toolbar remained visible throughout. At 100% and 125% the sidebar conversations list remained usable (**PASS**); at 150% the list was compressed to a thin strip (**PARTIAL**); at 175% it became essentially unusable (**FAIL**). These screenshots contain personal archived text, so the image bytes are intentionally **not** copied into GitHub or artifacts. This is a local display-qualification finding, and overrides the prior "physical DPI scaling NOT_RUN" item in the matrix.

Root cause evidence in `src/style.css` and `src/knowledge-os.css`: the eight-row sidebar grid allocates `minmax(0, 1fr)` to the list after fixed navigation, search, and footer sections have consumed available CSS-pixel height. The independent cloud-only repair is [issue #115](https://github.com/KaisAbiyyi/mind-place/issues/115) / [draft PR #116](https://github.com/KaisAbiyyi/mind-place/pull/116), cloud head `122753b73399f364d24c3f1a95e3a0c36bfcd575`, which retains a minimum list viewport and permits parent sidebar scroll for short desktop heights. Cloud `visual-smoke` (including simulated 100/125/150/175% CSS viewports) **PASS**, `verify:repo`/TypeScript **PASS** locally; Windows CI still requires completion. The owner's four actual screenshots validate the **old candidate**, so **native display scaling must be repeated after #116 is reviewed/merged and a new exact-head package is built**. Do not interpret cloud Chromium smoke as owner Windows acceptance.

## Owner waiver: defer high-DPI visual acceptance (2026-10-24)

**Supersedes earlier statements treating the 150%/175% sidebar collapse as an MP-8L1 functional blocker.** The owner accepts the current navigation layout as provisional and plans a full shell redesign later. Preserve the observed 100/125% usable, 150% PARTIAL, 175% FAIL visual evidence, but do not require this specific scaling fix (#115 / optional draft PR #116) or broad UI polish to pass **functional** MP-8L1 or to advance unrelated functionality/Phase 9/backend/performance tasks. Track future fixed icon-rail / expandable contextual secondary sidebar in research Backlog MP-UXR-01 (#117). Do not incorrectly state that high-DPI rendering itself passed.

Mandatory remaining native/owner functional checks retain their independent status: exact-head packaged Windows install/upgrade, authorized archive/search/preview, privacy/lock, persistence, state and migration safety, and required CI/review; any unrun checks remain NOT_RUN/PARTIAL. MP-8L1 must not be declared Done solely because the user waived cosmetic acceptance. See policy docs PR #118.

Do not substitute the observed partial synthetic pass for real owner-account/index/content acceptance, installed NSIS upgrade behavior, physical display-scaling exercise or MP-8L2 performance qualification. MP-8L2 and MP-8U1 stay blocked until MP-8L1 actually receives its full LOCAL PASS.

Local acceptance is independent of cloud CI. Native UI defects should be triaged into bounded cloud repair units before claiming the local gate `PASS`.

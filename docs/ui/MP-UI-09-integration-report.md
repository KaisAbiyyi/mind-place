# MP-UI-09 integrated Knowledge OS candidate

**PARTIAL overall:** source integration and bounded cloud verification are ready for review; desktop build/full native qualification are BLOCKED in this Linux checkout by absent Rust/Cargo; Windows UI-L1 and owner UI-U1 acceptance are NOT_RUN. This is an owner-authorized pre-acceptance candidate. Formal UI-09 remains **Blocked**, because its AND predecessor feature tasks remain Review, not accepted.

One [Draft integration PR #159](https://github.com/KaisAbiyyi/mind-place/pull/159) targets `main`, from `codex/mp-ui-09-full-integration-20261106`. Main and the original feature branches/PRs are untouched. No merge, source acceptance, production deployment, release, settings/secrets/ruleset change, 100K/1M dispatch or owner acceptance occurred.

## Verified sources and integration strategy

Main/base: `24648c74f3f59e176974d3878049b882478db69b`. Full tree/source and lock details are in the [source manifest](evidence/mp-ui-09/source-manifest.json).

| Source PR | Exact verified HEAD | Scope |
| --- | --- | --- |
| [#156](https://github.com/KaisAbiyyi/mind-place/pull/156) | `bab547fe6e399decc27b55ece7d95b21383ffc0a` | Overview, archive, memory, providers: UI-02/03/06 |
| [#147](https://github.com/KaisAbiyyi/mind-place/pull/147) | `3d2e954244fa847ef50c0f8c19aff8c28696052c` | Hive: UI-04 |
| [#148](https://github.com/KaisAbiyyi/mind-place/pull/148) | `3816b615c8a97c79130688e3a824056fdf35cd48` | Files/Vault: UI-05 |
| [#157](https://github.com/KaisAbiyyi/mind-place/pull/157) | `ec907e490fc124b39e80a1a164fef9ce9135e3a8` | Search/context and Settings: UI-07/08 |
| [#158](https://github.com/KaisAbiyyi/mind-place/pull/158) | `e544aea094cd7cf68b02868f1961befa631f3bf2` | Guided onboarding: MP-8ONB-01 |

Preflight read AGENTS, README/current product/architecture, current phase and roadmap/runtime direction, orchestration/parallel board, UI-00 inventory, shared UI contract, all five live PR bodies/comments/checks, live main/open branches, Notion statuses/claims/locks and the canonical HTML. All PRs were open/unmerged, exact HEAD/base matched the owner inventory, and no submitted reviews were present. Notion UI-00/UI-01 were Done, all feature tasks Review. Integration lock was unclaimed and there was no existing integration PR. Codex Cloud registered owner-assigned generation `mp-ui09-20261106-g1` for `ui-rollout-integration/shared-route-resolution`; source-module claims were retained. No atomic controller is installed: the explicit owner assignment serializes this single integration writer. UI-09 remains Blocked/Up Next, source feature tasks remain Review/Now, and L1/U1 remain blocked without false promotion.

Normal `--no-ff` merges preserve complete source histories in order **#156 → #147 → #148 → #157 → #158**. Each source SHA is an ancestor of the integrated candidate. No squashing or blanket ours/theirs selection was used. `#156` includes the owner's already authorized single-account contribution policy correction, retained as part of its full history.

| Merge | Reconciliation |
| --- | --- |
| #156, #147 | Clean Git merges; retain provider/archive/memory changes and bounded Hive source matching |
| #148 | `reader.ts`: combine Files handle and disposal with Providers/capture-dialog handles; route to dedicated Files and Providers pages |
| #157 | `reader.ts`: preserve archive table/provider workflow plus Settings handle and disposal; all search/Settings CSS and modules retained |
| #158 | `main.ts`, `reader.ts`: retain all imports/styles and handles; Setup opens wizard, Files/Providers remain dedicated routes; full capture/import dialog still reachable; first-run routing and teardown retained |

Post-merge review intentionally checked semantic integration beyond conflict markers. The [feature coverage matrix](MP-UI-09-feature-coverage-matrix.md) maps every primary route and existing capability placement.

## Verified integration corrections

| Severity | Confirmed defect / correction | Regression evidence |
| --- | --- | --- |
| P1 | Search swallowed unavailable security status, and stale query completions could publish obsolete results. Recheck native security/epoch before and after privileged reads, fence query versions, clear retained evidence on lock/destroy | UI09 unavailable-authority and out-of-order tests, RED → GREEN |
| P1 | New Settings collected recovery credentials and could initiate first-run provisioning inside an unlocked reader, bypassing the existing security-gate lifecycle. Reuse the established locked recovery flow; no Settings secret inputs or provisioning action; cancel has no mutation | UI09 recovery-boundary test, RED → GREEN; native backend unchanged |
| P2 | Context presets, filters, budget and result estimates disagreed with Rust strict deserialization. Align native presets, `captured_to`, RFC3339 Historical as-of, `max_estimated_tokens`, `estimated_tokens`; reject date-less Historical locally | Context contract and Historical validation tests, RED → GREEN |
| P2 | Settings confirmation listeners survived Cancel and could execute a rebuild twice. Use one pending callback, canceled on close/Escape and destroyed on navigation | Confirmation cancel regression, RED → GREEN |
| P2 | Late transcript completion could overwrite Settings. Invalidate selection on Settings entry; preserve all workspace lifetimes and dispose on actual route exit | UI09 late-read regression, RED → GREEN |
| P2 | Disposing Providers during its detailed setup dialog removed the focus-return target. Preserve underlying module during modal route presentation/restoration | Existing reader/workspace focus regressions caught failure then passed |
| P2 | Diagnostic failures rendered observed zero/stopped, and listener status was ambiguous. Render unavailable on failed reads, clear stale values, distinguish listener from verified client | Independent review finding + UI09 diagnostic regression, RED → GREEN |
| P2 | Search preview lacked full-revision navigation; reader clipboard was not passed to context workspace. Wire exact archive/snapshot callback and clipboard; global file preview revalidates URI/version via native preview | UI09 source route regression, RED → GREEN; context copy tests |
| P2 | New Settings tabs lacked keyboard/roving focus; settings preferences could diverge from shell state. Add ArrowLeft/Right/Home/End and delegate preferences to shell controls | UI09 keyboard test, RED → GREEN; Settings preference suite |
| P2 | Legacy short-height CSS capped desktop nav at 220px, hiding lower route groups; stretched card grids added excess whitespace. Scope nav height to actual desktop viewport and align card content to start | Real before/after cloud screenshot inspection and responsive checks |
| P2 | Overview lacked observed health. Add native epoch-fenced index/listener summary, explicitly unavailable on failure | UI09 Overview health regression, RED → GREEN |
| P2 | Search policies ignored the HTML hidden state and legacy flex rules stretched date/budget inputs to 220px. Isolate the search form CSS, honor hidden, keep policy inputs control-sized, and make native slider accents monochrome | New real-browser visibility/control-height assertions observed RED → GREEN; screenshot review |

The search/Settings defects existed in imported source behavior; their corrections do not constitute independent acceptance of the original PRs.

## Cloud verification

Final reviewed product source: `1fde14f9a8c1956360cdb5c73bb6ebd641609ae0`, tree `ca9c42ee8bd6e8df673614f950fec24e29a84988`. Earlier TypeScript/product review was at `f0018d92e1cf2606845eb94c40dcb51efdd8d1ad`; the final scoped CSS correction was reviewed at the final product SHA. Earlier test/evidence checkpoint `5a46e7a2f6845e44a6ebc1aa6f88fea1c758b409`, tree `370c006c66753ee6e8c2809b19432336d455f0b4`, has identical TypeScript/native contracts; the later scoped CSS fixes are additionally browser verified. Final documentation-only HEAD and exact-head rerun URLs are recorded in the PR execution receipt to avoid a self-referential commit SHA.

| Command / check | Observed result |
| --- | --- |
| `npm run verify:repo` | PASS, repository 0.2.5 / extension 1.3.1 |
| `npm run typecheck` | PASS |
| `npm test` | PARTIAL/BLOCKED: web portion 261 tests, **260 PASS / 0 FAIL / 1 SKIP** (existing Windows portable payload); then `cargo: not found` |
| `npm run test:web` | Hosted and cloud web suite **260 PASS / 1 SKIP / 0 FAIL** |
| `cargo fmt --all -- --check` | BLOCKED in cloud checkout (Cargo absent); hosted pinned Rust 1.90 job PASS |
| `cargo check --locked -p mind-place-security -p mind-place-state -p mind-place-index -p mind-place-integration` | BLOCKED in checkout; hosted production core check PASS |
| Relevant Rust tests | Full `npm run test:rust` BLOCKED in checkout. Hosted `cargo test --locked -p mind-place-security --lib`: **39 PASS**; available state/index SQLCipher and integration vault feature compile checks PASS. This is not the full native Rust suite |
| `npm run build` | BLOCKED: Rust compiler unavailable; no desktop binary produced |
| `npm run build:web` | PASS; Vite reports its existing large-chunk advisory, no build error |
| `node scripts/mp8f-visual-smoke.mjs` | PASS, 16 synthetic viewport/keyboard screenshots; provider → setup → Escape → provider |
| `node scripts/mp-ui09-visual.mjs` | PASS: 272 candidate states, 172 A/B pairs; no runtime errors or viewport overflow; visibility/control-height browser regressions PASS. See screenshot manifest and final hosted run |
| `git diff --check` | PASS |

Hosted checkpoint checks are associated with exact PR HEAD `5a46e7a...`. Actions checked merge commit `0b364e51d519519848703a08e17885fa2ced0151`; its tree equals the PR HEAD tree, verified by fetching `refs/pull/159/merge`. Main is an ancestor, not an independently diverged source.

| Workflow | Run / job-level result |
| --- | --- |
| CI | [42159511578](https://github.com/KaisAbiyyi/mind-place/actions/runs/42159511578): Fast developer feedback PASS; Fast Rust compile/vault feedback PASS (39 security tests); Native execution evidence PASS (attestation, not native product execution) |
| Visual | [42159511559](https://github.com/KaisAbiyyi/mind-place/actions/runs/42159511559): visual-smoke PASS including integrated comparisons; [artifact](https://github.com/KaisAbiyyi/mind-place/actions/runs/42159511559/artifacts/13476122328), API-reported digest `sha256:4a6302cde42c78ba1341d8cc4eeadafaddffa4d0ee2bc4363a1d4117f75228c2` |
| Workflow trust boundary | [42159511617](https://github.com/KaisAbiyyi/mind-place/actions/runs/42159511617): audit PASS |
| Windows Product | [42159511558](https://github.com/KaisAbiyyi/mind-place/actions/runs/42159511558): candidate-result PASS; **source decision / authority wait / Verify Windows product all SKIPPED** for Draft. No Windows execution or build certified |
| Document scale | [42159511524](https://github.com/KaisAbiyyi/mind-place/actions/runs/42159511524): source decision PASS, Draft pending PASS, Windows 100-file check PASS; **100K runner, prerequisite wait, provenance, acceptance all SKIPPED**. No 100K/1M claim |
| Brave registration | Draft bounded regression workflow SKIPPED; no installed Brave acceptance |

Earlier CI failures are retained in Actions: initial integrated source had an obsolete setup-test entry assertion; a later run caught old “Process running” wording after the truthful listener label changed. The screenshot harness initially assumed a page H1 for an error panel with an H2; this was corrected to recognize semantic state headings. These are not hidden or counted as successful runs.

## Security and independent review

11 new UI09 regressions exercise authority failure, out-of-order queries, strict context contracts, recovery boundary, canceled confirmation, late transcript navigation, unavailable diagnostics, exact revision opening, Settings keyboard focus, Historical date validation and Overview unknown health. Existing suites cover lock failure/retry, late post-lock reads, private provider refusal, immutable capture revisions, sanitizer/remote-image fences, native root revoke/version checks, token-safe provider lifecycle and validated non-sensitive onboarding progress.

An independent read-only reviewer verified final product source `1fde14f...` / tree `ca9c42e...`, confirmed all five source ancestors, and reviewed route/lifecycle, async fences, inert content, credential clearing, context/backup schemas, onboarding and responsive changes. It reproduced four P2 findings (unavailable diagnostics, exact source route, Settings tab keyboard, date-less Historical), then verified their corrections. Focused review checks: UI09 **11**, manual lock **5**, Providers **3**, Hive **2**, Files/Vault **3** = **24 PASS**. No unresolved confirmed P0/P1 in inspected paths. This is advisory technical review, **not independent security approval** and not a replacement for the full native suite.

## HTML fidelity and screenshot evidence

[Canonical Notion reference](https://app.notion.com/40cc4a6aaf73814584b3fa7c2e10ae0d). Original attachment retrieved through Notion, checked locally against SHA-256 `b28acf5d50c62bfbaecc6285c0621e9903c04fe73b5a10de6f22948cdcf47579`, and preserved byte-for-byte as [reference.html](evidence/mp-ui-09/reference.html). No alternate design direction was substituted.

[Local screenshot manifest](evidence/mp-ui-09/screenshot-manifest.json) records immutable screenshot source/tree and per-image hashes. The hosted visual artifact contains `ui09/gallery.html`, `ui09/manifest.json`, original/candidate PNGs and the existing 16-smoke receipt. Download/extract the complete artifact to view the side-by-side gallery. The cloud could inspect artifact metadata and run logs but its direct Azure blob download returned HTTP 403; the API digest is not represented as a locally recomputed ZIP digest. Locally generated screenshots were visually inspected directly.

Final cloud run: **272 candidate states / 172 A/B pairs**, zero captured runtime errors and zero detected viewport overflow. Coverage includes every primary page at **1440/1120/780/390 × light/dark × expanded/collapsed** (128 primary states), transcript/memory details, four onboarding steps, all four Settings sections, palette/search/context/trace, empty/error in both themes at desktop/narrow sizes, Quick Peek, approved-folder inspector/dialog, capture setup, palette, and loading/locked evidence. Original demo data is intentionally different from the explicit synthetic fixture. The browser script checks meaningful headings, runtime errors and viewport overflow; that is layout evidence, not a pixel-diff or subjective acceptance score.

Observed: achromatic app/surface/text anchors, consistent route groups and table/card controls, readable long titles, responsive scrolling and both theme surfaces. The legacy nav cap was corrected after visual inspection. Intentional differences: prototype demo totals/telemetry, claimed encryption/client status, semantic memory collections, unsupported writes/generation and fake folder picker are replaced with native facts or explanatory unavailable states. Existing transcript/recovery controls are retained in the closest shared patterns. Active-route inverse contrast now follows the source. Spacing/density are not certified as pixel-identical; the A/B gallery is provided for owner judgment. System fonts are used when Inter/JetBrains Mono are not installed. Actual WebView2/DPI and assistive-technology behavior remain native gates.

## Remaining limitations and handoff

No unresolved confirmed P0/P1 integration defect remains in reviewed paths. Native qualification remains an acceptance blocker, owned by the Windows qualification implementor. Capability boundaries (opaque memory source resolver, native picker, durable watcher status, accepted MCP write authority, general migration/restore/erasure, graph generation) remain explicit backend limitations; no fake action is added.

See [Windows preview and page-by-page owner acceptance handoff](MP-UI-09-owner-acceptance-handoff.md). The qualifier builds the frozen SHA, records hashes/provenance, launches an isolated profile and delivers the complete portable folder/shortcut; the owner reviews pages and records PASS / CHANGES REQUIRED / BLOCKED. No owner developer debugging or large suite is assigned. Final source approval and manual merge stay owner-controlled.

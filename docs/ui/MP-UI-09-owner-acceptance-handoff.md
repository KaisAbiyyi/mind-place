# MP-UI-09 Windows preview and owner acceptance

**Current gates:** cloud candidate preparation only. Windows qualification NOT_RUN; owner visual/product acceptance NOT_RUN. UI-09 formal predecessor gate remains Blocked; this is the explicitly authorized pre-acceptance integrated preview. Do not merge, release, or mark UI-U1 accepted.

## Immutable source and responsibility

One candidate is in [Draft PR #159](https://github.com/KaisAbiyyi/mind-place/pull/159), branch `codex/mp-ui-09-full-integration-20261106`. The PR's final execution receipt records the immutable integration HEAD and tree; use that frozen SHA, not a later moving branch tip. Frozen Windows preview product source was independently reviewed at `1fde14f9a8c1956360cdb5c73bb6ebd641609ae0`; later test/evidence/document commits preserve that product source unless a subsequent receipt explicitly says otherwise.

The Codex Local / Windows qualification implementor performs the build and prepares a click-to-launch preview. The owner only launches and reviews the application; the owner is not assigned developer debugging or the full test suite.

No downloadable Windows binary is claimed. The existing trusted `windows-product.yml` builds an executable only when its exact-source authority gate permits and uploads a proof JSON, not a distributable preview package. The release workflow is deliberately untouched. Linux cloud checkout lacks Cargo/Rust; use this bounded local handoff rather than claim packaging happened.

## Codex Local qualification assignment

Use Windows x64 with Node 24, Git, Rust **1.90.0** + rustfmt, Visual Studio C++ build tools and Windows SDK, and WebView2 Runtime. Use a fresh task checkout. Close existing Mind Place processes. Never use the owner's live data root or register browser hosts during initial visual inspection.

1. Build frozen integrated source `1fde14f9a8c1956360cdb5c73bb6ebd641609ae0` (tree `ca9c42ee8bd6e8df673614f950fec24e29a84988`) using the literal SHA below. The final PR receipt records the later evidence-only HEAD and verifies identical product bytes. Verify the frozen source before building; do not substitute a moving branch tip.
2. Fetch and detach at that exact SHA in an isolated checkout:

```powershell
# Frozen integrated application source; later evidence-only commits do not change product bytes.
$Candidate = '1fde14f9a8c1956360cdb5c73bb6ebd641609ae0'
git clone https://github.com/KaisAbiyyi/mind-place.git mind-place-ui09-preview
Set-Location mind-place-ui09-preview
git fetch origin $Candidate
if ($LASTEXITCODE -ne 0) { throw 'Cannot fetch frozen candidate' }
git checkout --detach $Candidate
if ((git rev-parse HEAD).Trim() -ne $Candidate) { throw 'Wrong source' }
git status --porcelain
rustup toolchain install 1.90.0 --profile minimal --component rustfmt --no-self-update
rustup override set 1.90.0
npm ci
if ($LASTEXITCODE -ne 0) { throw 'Dependency install failed' }
npm run dist:win
if ($LASTEXITCODE -ne 0) { throw 'Windows preview build failed; return logs to cloud' }
npm run verify:distribution-license
if ($LASTEXITCODE -ne 0) { throw 'Distribution verification failed' }
```

`dist:win` runs the existing wrapper, builds the capture sidecar, verifies/stages the pinned tunnel helper and notices, builds the frontend and NSIS installer, and stages `release\win32-x64\`. It does not publish a release. Do not edit source, signing settings, secrets, registry policies, or release workflows locally.

3. Expected artifacts: `release\win32-x64\mind-place.exe`, `mind-place-capture-host.exe`, extension, helper/resources, `LICENSE`, `THIRD-PARTY-NOTICES.txt`; NSIS installer under `target\release\bundle\nsis\`. Check actual paths; never invent an installer filename. Zip the **entire** staged directory, not a lone EXE. Record SHA-256 for the ZIP, installer and both app/host executables, source HEAD/tree, Node/Rust/WebView2/Windows versions, build log, and helper provenance receipt. Keep generated binaries out of Git.
4. Create a safe portable launcher beside the staged app:

```powershell
# Save as Launch-UI09-Preview.ps1 inside release\win32-x64.
$ErrorActionPreference = 'Stop'
$PreviewRoot = Join-Path $env:LOCALAPPDATA 'MindPlace-UI09-Isolated-Preview'
New-Item -ItemType Directory -Force $PreviewRoot | Out-Null
$env:MIND_PLACE_DATA_DIR = Join-Path $PreviewRoot 'archive'
$env:WEBVIEW2_USER_DATA_FOLDER = Join-Path $PreviewRoot 'webview'
Start-Process -FilePath (Join-Path $PSScriptRoot 'mind-place.exe') -WorkingDirectory $PSScriptRoot
```

These process-local overrides are supported by the app. They preserve the owner's ordinary profile. Do not set a real password/code on a command line, in logs, fixtures or screenshots. Do not enable debugging ports for the owner walkthrough. Provide a shortcut that launches this script under the machine's normal PowerShell policy; do not weaken execution policy.
5. Qualification implementor runs the bounded native startup/lock/unlock/restart, all primary navigation, isolated root approve/index/preview/revoke, browser-setup cancel, Settings recovery cancel, focus and DPI checks. Existing `scripts/mp8l1-native-smoke.mjs` can assist but contains older selectors: inspect compatibility first and do not count filtered/empty/skipped results as PASS. Do not dispatch 100K/1M. Provide concise PASS/FAIL/BLOCKED evidence per action and return any defect to cloud for correction.
6. Upload the verified ZIP/installer plus SHA-256/provenance receipt to an authorized artifact location linked from PR #159. Before delivery, extract and launch the ZIP once on Windows x64 and record screenshot/version identity. Leave native acceptance NOT_RUN until performed; “build succeeded” alone is insufficient.

## Owner installation, launch and rollback

Preferred visual preview: download and verify the qualification agent's ZIP hash, extract to a new folder, and open the prepared shortcut. This avoids replacing the existing installation. If an installer is used later, the qualifier must first demonstrate profile isolation and document its actual current-user install location; the same product identifier can replace an existing app.

Rollback: exit the preview and remove its extracted folder/shortcut. The isolated data is under `%LOCALAPPDATA%\MindPlace-UI09-Isolated-Preview`; retain it for evidence or delete it only after owner consent. The normal application/profile stays separate. If the owner explicitly enabled a browser registration or provider during qualification, revoke/stop it through the app and restore the documented previous registration before deleting the preview. Never delete the owner's existing archive.

## Page-by-page owner walkthrough

Use the artifact's `ui09/gallery.html` beside its manifest for original-versus-integrated cloud screenshots. Those images are synthetic; Windows screenshots from the qualifier are the native evidence. Compare light/dark, sidebar expanded/collapsed, and widths near 1440/1120/780/390, plus Windows scaling 100/150/175% and zoom as practical.

| Page / flow | Inspect and expected behavior | Evidence / known limit |
| --- | --- | --- |
| First launch / lock | Starts behind security gate; isolated first run explains protection; cancel/retry never opens archive without confirmed unlock | Onboarding and manual-lock tests; no owner secret screenshots |
| Onboarding | Welcome → Vault → optional capture/providers → first useful result; skip/resume; returning users not forced back | `*-onboarding-*.png`; empty archive never claims a verified result |
| Overview | Actual saved counts, recent captures, navigation, factual runtime health | `*-home-*.png`; no prototype telemetry or fabricated totals |
| Conversations | Provider filter, page 1/2, long title, full transcript, revision selector, back, copy/export | `*-conversations-*.png`, `*-transcript-*.png`; partial captures labeled |
| Curated Memory | Collections/cards/Quick Peek, detail/history/as-of, conflict/related memory, Projects/People/Topics | `*-memory-*.png`, `*-memory-detail-*.png`; opaque proof refs copy only; writes disabled where unsupported |
| Hive | Query, clusters and isolated nodes, filters, zoom/pan/branch/back, evidence, matched conversation | `*-hive-*.png`; bounded native graph; no fabricated generation/edges |
| Files & Vault | Authorize a disposable folder → Stage → Approve → Index → search/exact preview → revoke; original files remain | `*-files-*.png`; manual absolute path, no native picker; external files not implied encrypted |
| Providers & MCP | Capture support, registration, import, setup/cancel/return, start/stop/revoke only if explicitly chosen | `*-providers-*.png`; listener/configuration ≠ tested client connection |
| Global search | Ctrl/Meta K, keyboard selection, results, exact saved revision, multi-query/context/trace, copy | `*-search-results.png`, `*-context.png`, `*-trace.png`; bounded index results; JSON copy, no fabricated file export |
| Settings | Theme/density/provenance, tab keyboard focus, actual diagnostics, rebuild confirmation/cancel, storage/recovery cancel | `*-settings-*.png`; unknown service health unavailable; recovery uses locked gate; legacy backup disabled |
| Cross-module / lock | Repeated page changes, modal Escape and return focus; lock clears transcript/search/drawers, revoked content cannot reopen | Native qualifier adds screen/receipt; backend authorization tests ≠ real device approval |
| Empty / error / responsiveness | Clear next step, no phantom data, readable narrow pages, scrolling controls and keyboard access | `empty-*`, `error-*`; cloud layout does not certify WebView2/DPI |

Record one decision per row: **PASS / CHANGES REQUIRED / BLOCKED**, with candidate SHA, Windows build/hash, page/theme/width/scaling, expected versus observed, and screenshot when useful. “CHANGES REQUIRED” should name a concrete issue; “BLOCKED” should state missing setup/capability. Final overall owner decision remains **NOT_RUN** until the actual complete candidate is opened and reviewed. A recorded visual PASS does not accept unrelated security/native gates or authorize a merge.

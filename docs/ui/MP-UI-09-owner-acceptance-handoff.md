# MP-UI-09 Windows Preview & Page-by-Page Owner Acceptance Runbook

**Current gate status:**
- **Prior candidate (`c1ce4a586bfdc0520a53248fe9276ad82331808e` / product `1fde14f9a8c1956360cdb5c73bb6ebd641609ae0`):** **CHANGES REQUIRED** (owner rejected visual fidelity).
- **Remastered candidate (frozen product source `77abfa45f37d14bf558e235e47b48cf67468fa95`, tree `d2f5664ce280445810e8a9121d0694e6f0b90a2f`):** **Cloud verification PASS**; **Windows qualification (`MP-UI-L1`) NOT_RUN**; **Owner visual/product acceptance (`MP-UI-U1`) NOT_RUN**.
- Formal `MP-UI-09` task status remains **Blocked** until predecessor feature tasks and native/owner gates are accepted. Do not merge PR #159 or mark `MP-UI-U1` accepted without explicit owner sign-off.

---

## 1. Immutable Source & Responsibility Split

- **Draft PR:** [#159](https://github.com/KaisAbiyyi/mind-place/pull/159) (`codex/mp-ui-09-full-integration-20261106`)
- **Frozen Product Source SHA:** `77abfa45f37d14bf558e235e47b48cf67468fa95`
- **Frozen Product Tree SHA:** `d2f5664ce280445810e8a9121d0694e6f0b90a2f`
- **Canonical HTML Reference:** `docs/ui/evidence/mp-ui-09/reference.html` (SHA-256 `b28acf5d50c62bfbaecc6285c0621e9903c04fe73b5a10de6f22948cdcf47579`)

**Role separation:**
1. **Codex Local / Windows Qualification Agent:** Checks out the frozen SHA on Windows x64, verifies toolchain prerequisites (including Perl for vendored OpenSSL), builds the portable preview package (`npm run dist:win`), verifies license/provenance receipts, runs bounded native qualification in an isolated profile, and prepares a click-to-launch shortcut for the owner.
2. **Repository Owner:** Launches the prepared isolated preview, walks through the UI pages against the canonical monochrome Knowledge OS design, and records **PASS / CHANGES REQUIRED / BLOCKED**. The owner is not assigned developer builds, debugging, or test suite execution.

---

## 2. Codex Local / Windows x64 Qualification Runbook

### 2.1 Required Windows x64 Prerequisites
Ensure the qualification machine has:
- **Windows x64** with **WebView2 Runtime** installed
- **Node.js 24** and **Git**
- **Rust `1.90.0`** (`rustfmt` component installed)
- **Visual Studio C++ Build Tools** and **Windows 10/11 SDK**
- **Perl (`perl.exe` on `PATH`)** — **REQUIRED** because `openssl-sys` (pulled in by `reqwest` in `mind-place-integration` / `src-tauri`) compiles vendored OpenSSL via `perl Configure`. (An earlier local build attempt failed with `Command 'perl' not found`; verify `Get-Command perl` before running `npm run dist:win`. Strawberry Perl or Git for Windows `/usr/bin/perl` added to `PATH` satisfies this requirement.)

### 2.2 Exact Frozen-Source Build Commands (PowerShell)

```powershell
$ErrorActionPreference = 'Stop'

# 1. Verify Perl is available on PATH for vendored OpenSSL (openssl-sys)
if (-not (Get-Command perl -ErrorAction SilentlyContinue)) {
  if (Test-Path 'C:\Program Files\Git\usr\bin\perl.exe') {
    $env:PATH = "C:\Program Files\Git\usr\bin;$env:PATH"
  } else {
    throw "Perl is required on PATH to build vendored OpenSSL (openssl-sys). Install Strawberry Perl or add Git\usr\bin to PATH."
  }
}
perl -v | Select-Object -First 2

# 2. Fetch and checkout the exact frozen product source SHA
$Candidate = '77abfa45f37d14bf558e235e47b48cf67468fa95'
$ExpectedSrcTree = '60c514b3f7b764a43f7a3d3329e271b9ccfa7bb3'
git clone https://github.com/KaisAbiyyi/mind-place.git mind-place-ui09-preview
Set-Location mind-place-ui09-preview
git fetch origin $Candidate
if ($LASTEXITCODE -ne 0) { throw 'Cannot fetch frozen candidate' }
git checkout --detach $Candidate
if ((git rev-parse HEAD).Trim() -ne $Candidate) { throw 'HEAD mismatch' }
if ((git rev-parse 'HEAD:src').Trim() -ne $ExpectedSrcTree) { throw 'src tree mismatch' }
git status --porcelain

# 3. Pin Rust 1.90.0 and install locked npm dependencies
rustup toolchain install 1.90.0 --profile minimal --component rustfmt --no-self-update
rustup override set 1.90.0
npm ci
if ($LASTEXITCODE -ne 0) { throw 'Dependency install failed' }

# 4. Run full native verification and build portable Windows x64 package
npm run verify:repo
npm run typecheck
npm test
if ($LASTEXITCODE -ne 0) { throw 'Full test suite failed; return logs to cloud' }
npm run dist:win
if ($LASTEXITCODE -ne 0) { throw 'Windows preview build failed; return logs to cloud' }
npm run verify:distribution-license
if ($LASTEXITCODE -ne 0) { throw 'Distribution license verification failed' }
```

### 2.3 Expected Staged Outputs & Isolated Preview Launcher
1. Verify staged files in `release\win32-x64\`:
   - `mind-place.exe`
   - `mind-place-capture-host.exe`
   - `extension\` (unpacked browser extension)
   - `resources\` / verified `cloudflared.exe` helper and provenance receipt
   - `LICENSE` and `THIRD-PARTY-NOTICES.txt`
   - NSIS installer under `target\release\bundle\nsis\`
2. Save `Launch-UI09-Preview.ps1` inside `release\win32-x64\`:

```powershell
$ErrorActionPreference = 'Stop'
$PreviewRoot = Join-Path $env:LOCALAPPDATA 'MindPlace-UI09-Isolated-Preview'
New-Item -ItemType Directory -Force $PreviewRoot | Out-Null
$env:MIND_PLACE_DATA_DIR = Join-Path $PreviewRoot 'archive'
$env:WEBVIEW2_USER_DATA_FOLDER = Join-Path $PreviewRoot 'webview'
Start-Process -FilePath (Join-Path $PSScriptRoot 'mind-place.exe') -WorkingDirectory $PSScriptRoot
```

3. Zip the **entire** `release\win32-x64\` directory (never a lone `.exe`), compute SHA-256 hashes for the ZIP, NSIS installer, `mind-place.exe`, and `mind-place-capture-host.exe`, and execute the bounded `MP-UI-L1` native smoke checks in the isolated profile before handing the ready-to-open folder/shortcut to the owner.

---

## 3. Owner Launch, Walkthrough & Rollback

### 3.1 Safe Launch & Rollback
- **Launch:** Extract the verified preview ZIP to a temporary folder and double-click the prepared `Launch-UI09-Preview` shortcut. Your existing Mind Place installation and real archive remain untouched because data is isolated under `%LOCALAPPDATA%\MindPlace-UI09-Isolated-Preview`.
- **Rollback:** Close the preview window and delete the extracted preview folder and `%LOCALAPPDATA%\MindPlace-UI09-Isolated-Preview`.

### 3.2 Page-by-Page Owner Visual & Functional Checklist

Compare against the canonical monochrome Knowledge OS design (`docs/ui/evidence/mp-ui-09/reference.html` and the `ui09/gallery.html` A/B artifact in the `Visual` GitHub Actions run) across Light and Dark themes, Expanded (`256px`) and Collapsed (`64px`) sidebar, and responsive widths:

| Surface / Flow | What to Verify | Factual Boundary / Expected Behavior |
| --- | --- | --- |
| **1. Security Gate (`#vaultLockOverlay`)** | Centered monochrome card on zinc backdrop; first-run secret creation, unlock, and recovery mode | Starts locked; never exposes archive or MCP before unlock |
| **2. Application Shell & Topbar** | `256px`/`64px` sidebar with brand mark, vault pill, grouped nav with dynamic counts, footer, and single `58px` topbar | Single topbar with breadcrumb, `Ctrl K` search trigger, and `Technical`/`Density`/`Theme`/`Lock` pills |
| **3. Setup & Onboarding (`page-setup`)** | 4-step horizontal stepper (`Welcome`, `Secure vault`, `Capture & AI providers`, `First useful result`) | Never asks for secrets outside lock screen; requires a real `read_conversation` before marking verified |
| **4. Overview (`page-home`)** | Overline, header + CTAs, `.status-strip`, 4 metric cards, 3-column lower grid (`Recent conversations`, `Quick access`, `Runtime health`) | Shows real archive counts and observed local index/MCP state; zero fabricated demo numbers |
| **5. Conversations Archive & Detail** | Header with `Capture & import setup` CTA, filter bar, table with `.tag.outline.mono` provider pills, and 2-column detail view (`1fr 340px` right provenance rail) | Preserves immutable snapshot selector, partial-capture warnings, and Markdown copy/export |
| **6. Curated Memory & Quick Peek** | Segmented view toggle, 3-column collection cards, 2-column list + `380px` detail inspector, right-hand Quick Peek drawer, and `Projects`/`People`/`Topics` lenses | Monochrome zinc styling throughout; opaque proof refs offer copy; unsupported writes stay disabled/draft-only |
| **7. Hive Mind Graph (`page-hive`)** | Radial dot-matrix canvas with concentric orbital rings, node labels, floating controls, and side-by-side `330px` right node inspector | Bounded 120-node/240-edge read-only graph; opens conversation only on exact archive/import identity match |
| **8. Files & Secure Vault (`page-files`)** | Storage protection banner with policy pill, 3-column step cards (`01 Stage`, `02 Approve`, `03 Index`), folder list, modal authorize dialog, and provenance inspector | Closed dialog stays hidden until clicked; manual absolute path input; original external files remain untouched on revoke |
| **9. AI Providers & MCP (`page-providers`)** | 3-column provider cards (`ChatGPT`, `Claude`, `Gemini`), transport status, and `Browser & host setup` dialog | Clearly distinguishes browser capture adapter support from MCP client handshakes |
| **10. Global Search (`Ctrl+K` → `Search all sources`)** | Command palette dialog, search input + `Multi-Query & Policies`, segmented source tabs, result cards, provenance preview, `Context Pack` and `Technical Trace` drawers | Collapsed multi-query panel stays hidden; selected filter tabs use monochrome surface contrast |
| **11. Settings (`page-settings`)** | `220px` left category nav (`General & Appearance`, `Storage & Vault`, `Privacy & security`, `Operational diagnostics`) + right cards | Theme/density/technical toggles sync with topbar; recovery uses locked security gate; online backup requires encrypted unlocked vault |

Record **PASS / CHANGES REQUIRED / BLOCKED** for the candidate. Final approval and manual merge of PR #159 remain exclusively with the repository owner.

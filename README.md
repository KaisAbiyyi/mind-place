# Mind Place

[![CI](https://github.com/KaisAbiyyi/mind-place/actions/workflows/ci.yml/badge.svg)](https://github.com/KaisAbiyyi/mind-place/actions/workflows/ci.yml)
[![License: AGPL v3+](https://img.shields.io/badge/License-AGPL_v3%2B-blue.svg)](./LICENSE)

Mind Place is a local-first archive for AI conversations. It saves ChatGPT conversations into an immutable local archive, then lets you read, search, inspect revisions, copy, and export them from a small desktop app.

The current product is a Rust archive core, a Tauri 2 desktop reader, and a dedicated Manifest V3 browser extension. Windows x64 is the qualified platform.

## What it does

- **Manual capture** — save the current ChatGPT conversation from the extension.
- **Bounded history loading** — an explicit manual Save can scroll through rendered history with progress, Stop & save, and Cancel controls.
- **Optional automatic saving** — when enabled, Mind Place saves only messages already loaded in the current ChatGPT DOM. It never scrolls or loads older history automatically.
- **Immutable revisions** — distinct captures are preserved instead of overwriting earlier evidence.
- **Offline reader** — browse and search saved conversations without keeping the browser or capture host running.
- **Markdown export** — export a selected saved revision with local image assets where available.
- **Legacy import** — import compatible archives from an earlier Mind Place format while preserving the original source data.

Automatic saving is **off by default** and requires optional access to `https://chatgpt.com/*`. The extension otherwise uses only `activeTab`, `scripting`, `nativeMessaging`, and `storage`.

## Architecture

```text
ChatGPT tab
    │
    │ explicit Save / optional loaded-message observation
    ▼
Browser extension
    │ native messaging
    ▼
Capture host ──────► immutable local archive
                         │
                         ▼
                   Tauri reader
                  search / revisions
                    copy / export
```

Raw captures are the archive authority. Markdown and search text are derived views that can be rebuilt. Automatic captures are intentionally marked partial because the loaded DOM cannot prove that the complete remote conversation is present.

See [docs/current-product.md](./docs/current-product.md) for the storage and capture model, and [docs/README.md](./docs/README.md) for the broader architecture roadmap.

## Development

### Requirements

- Windows 11 x64 for the qualified desktop build
- Node.js 24+
- Rust 1.90+
- MSVC C++ build tools
- Microsoft WebView2

Install dependencies and start the desktop app:

```powershell
npm ci
npm run dev
```

Run the reader in a browser without the Tauri shell:

```powershell
npm run dev:web
```

### Verification

```powershell
npm run verify:repo
npm run typecheck
npm test
cargo fmt --all -- --check
npm run build
```

`npm run verify:repo` checks version alignment, extension permissions, and repository boundaries before the broader test suite runs.

## Build and package

Build the desktop app without an installer:

```powershell
npm run build
```

Build the Windows NSIS installer and portable payload:

```powershell
npm run dist:win
```

Generated output is written under `target/`, `release/`, `dist/`, and `outputs/`; these paths are intentionally not versioned. Third-party license notices are regenerated as part of the build.

Windows x64 builds include the official OpenAI tunnel helper for optional hosted ChatGPT access. The build downloads a pinned release, verifies its archive/executable/notices against `scripts/tunnel-client.lock.json`, and stages the helper in both the NSIS installer and portable payload. Later builds revalidate and reuse the local cache. The installed app verifies the executable before launch; users do not download or copy a helper themselves. The helper is never downloaded at runtime. Tunnel/workspace setup, unlock and explicit remote-access consent are still required.

The GitHub `Release build` workflow repeats the release checks on Windows and uploads the installer, portable ZIP, and SHA-256 checksums as workflow artifacts. Tag names must match the desktop version, for example `v0.2.5`.

## Browser setup

1. Open Mind Place and select **Manual capture setup**.
2. Register Chrome or Edge. This installs the native messaging registration for the current Windows user.
3. Open the browser's extensions page, enable Developer mode, and load the extension folder shown by Mind Place.
4. Open a ChatGPT conversation and use **Save this conversation**.

The desktop app does not need to remain open while saving. The browser starts the native capture host for the request and the host exits after responding.

## Local storage

On Windows, archives are stored under:

```text
%APPDATA%\Mind Place\archive-v1\
```

The archive contains immutable capture revisions, conversation metadata, derived Markdown/search views, local image assets, exports, and preserved legacy evidence. User archives are never part of the application package or Git repository.

## Privacy and security

- Mind Place is local-first; conversation archives stay on the user's device.
- Automatic saving is opt-in and limited to loaded ChatGPT content.
- Automatic saving never scrolls, changes the scroll position, or loads older history.
- Manual history loading runs only inside an explicit Save job and is bounded and cancellable.
- Proven Temporary Chat/private state blocks capture.
- Saved Markdown/HTML is sanitized before rendering, and the desktop reader uses a restrictive Content Security Policy.
- The extension does not declare unconditional content scripts or broad host access.

See [SECURITY.md](./SECURITY.md) for vulnerability reporting.

## Project status

Current desktop version: **0.2.5**

Current browser extension version: **1.3.1**

The manual and opt-in automatic capture paths have been exercised against signed-in ChatGPT on Windows. Windows packaging is verified. The project remains pre-1.0 and does not claim complete coverage of every future ChatGPT layout, route, branch, attachment type, or streaming state.

See [CHANGELOG.md](./CHANGELOG.md) for release notes.

## Contributing

Contributions are welcome. Read [CONTRIBUTING.md](./CONTRIBUTING.md) before changing capture permissions, archive persistence, automatic saving, or packaging behavior.

Mind Place builds on lessons from the open-source AI tooling ecosystem, especially [Chat On Steroids](https://github.com/totec448-spec/chat-on-steroids). See [ACKNOWLEDGEMENTS.md](./ACKNOWLEDGEMENTS.md) for project credits and architectural influence.

## License

Mind Place is released under the [GNU Affero General Public License v3.0 or later](./LICENSE). Distributed modified versions must remain under the same copyleft terms, and AGPL section 13 also requires Corresponding Source to be offered to users who interact with a modified version over a network. Private modifications that are neither distributed nor offered as a network service are not forced public by the AGPL.

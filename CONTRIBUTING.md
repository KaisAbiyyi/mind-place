# Contributing to Mind Place

Mind Place is a local archive for AI conversations. Contributions should preserve user control, local-first storage, and the distinction between raw capture evidence and derived views.

## Development setup

Windows x64 is the qualified desktop target. Install Node.js 24+, Rust 1.90+, the MSVC C++ build tools, and Microsoft WebView2, then run:

```powershell
npm ci
npm run dev
```

For reader-only frontend work, use `npm run dev:web`.

## Required checks

Before opening a pull request:

```powershell
npm run verify:repo
npm run typecheck
npm test
cargo fmt --all -- --check
npm run build
```

The final build is required when desktop, native host, extension packaging, or release resources change.

## Design constraints

- Automatic saving is opt-in and may inspect only the currently loaded ChatGPT DOM.
- Automatic saving must never scroll the page or load older history.
- Manual history loading is allowed only during an explicit Save job and remains bounded and cancellable.
- Raw capture revisions are immutable. Derived Markdown and search data are rebuildable views.
- Temporary/private conversations must not be silently archived.
- Extension permissions must remain minimal; changes require explicit justification and tests.

## Commits and pull requests

Use Conventional Commits where practical, keep unrelated changes separate, and explain user-visible behavior plus verification in the pull request. Avoid committing generated build output, local archives, release artifacts, or credentials.

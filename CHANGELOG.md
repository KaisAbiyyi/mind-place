# Changelog

All notable user-facing changes to Mind Place are documented here.

The project follows Semantic Versioning while the desktop app remains pre-1.0. The browser extension has an independent version because Chrome extension updates may ship without changing the desktop binary.

## 0.2.5 — 2026-10-16

### Added

- Optional automatic saving for currently loaded ChatGPT messages, disabled by default and enabled through optional origin permission.
- Immutable automatic capture revisions with explicit partial-capture provenance.
- Browser activation and status handling for automatic saving.

### Improved

- Faster manual history capture with bounded bottom-first traversal and progress controls.
- Original conversation/message timestamps for archive ordering when ChatGPT exposes them.
- Local archive revision browsing, Markdown export, search, and legacy archive import.
- Windows x64 packaging with a Tauri desktop reader and native capture host.

### Privacy and safety

- Automatic saving never scrolls the page or loads older messages.
- Manual scrolling runs only during an explicit Save job and remains stoppable/cancellable.
- Temporary/private conversation checks and minimal extension permissions remain enforced.

# Current product architecture

This document describes the behavior shipped by the current Mind Place desktop app and browser extension. The numbered architecture documents in this directory include future roadmap work and should not be read as implemented features.

**MP-9B branch qualification (2026-10-28, Draft #127):** the encrypted-vault candidate includes a separately invoked Windows source maintenance command for explicitly confirmed offline legacy migration, authenticated resume/rollback and coherent encrypted backup/restore. Read the [owner runtime contract](implementation-plan/2026-10-28-mp9b-owner-runtime-and-writers.md) and [implementation evidence](implementation-summary/2026-10-28-mp9b-ci100k-local-takeover.md). These candidate changes are not a shipped release or approval to migrate owner data. Installed-owner recovery/migration and actual hostile Windows-account acceptance remain separate.

## Components

Mind Place has four product components:

1. **Browser extension** — captures the active ChatGPT conversation after an explicit Save, or observes already-loaded messages after the user enables automatic saving.
2. **Native capture host** — receives validated native-messaging requests and writes them through the archive core.
3. **Archive core** — owns immutable capture revisions, metadata, derived readable views, assets, search text, exports, and legacy import.
4. **Tauri reader** — provides local browsing, search, revision selection, copy, export, browser registration, and archive setup.

The browser extension and native host communicate only for a capture request. No background desktop daemon is required for normal operation.

## Optional AI connections

The unlocked owner can explicitly start the read-only local MCP listener and optional outbound OpenAI Secure MCP Tunnel. Windows x64 packages include the pinned official `tunnel-client.exe`, its upstream license/notice files and a receipt under `tunnel/`. No manual helper download/copy is needed. Build-time and pre-launch SHA-256 checks reject missing or modified helper bytes; repair/reinstall the package if verification fails. Other platforms are not qualified for bundled-helper distribution.

Tunnel/workspace provisioning and a runtime API key remain the owner's account setup. Remote access requires explicit consent and a separate ephemeral read-only identity; stop, lock and revoke close that access. A running helper is not proof of account/network connectivity. Real local MCP, live tunnel and owner acceptance remain separate 3L1/3L2/3U gates.

## Capture modes

### Manual save

The extension receives `activeTab` access from the user's action and captures the current conversation. Optional history loading is part of that explicit job only. When enabled, the extension restores the bottom position first, traverses upward through rendered history in bounded steps, retains messages before virtualization removes them, and exposes progress plus **Stop & save** and **Cancel** controls.

Manual capture remains partial when the page cannot prove that all remote history, branches, or original attachments were exposed.

### Automatic loaded-message save

Automatic saving starts disabled. Enabling it requests optional `https://chatgpt.com/*` access. While enabled, conversation navigation and loaded DOM changes are observed with debouncing/checkpoints so streaming responses do not produce a write for every token.

Automatic saving has a strict boundary: it may inspect only what ChatGPT has already rendered. It must not scroll, change scroll offsets, or request older history. Every automatic revision is therefore stored with partial provenance.

Disabling automatic saving removes the observer and optional origin access. A native write that was already submitted may still finish; an uncertain acknowledgement is reported rather than silently retried.

## Archive model

Distinct capture contents are immutable revisions. Repeating an identical capture deduplicates by content. A newer partial automatic window may become the latest view without deleting earlier larger/manual revisions.

On Windows the default archive root is:

```text
%APPDATA%\Mind Place\archive-v1\
```

Representative layout:

```text
archive-v1/
  conversations/<id>/captures/<content-hash>.jsonl
  conversations/<id>/meta.json
  conversations/<id>/conversation.md
  conversations/<id>/search.txt
  conversations/<id>/assets/<hash>.<ext>
  exports/
  legacy/
```

Raw capture records preserve source identity, timestamps when exposed by the provider, capture method, completeness, warnings, message content, and attachment references. Markdown and search text are derived data.

## Privacy boundaries

- Automatic saving requires explicit opt-in.
- The extension declares no unconditional content script and no required host permission.
- Proven Temporary Chat/private state blocks capture.
- Automatic saving never scrolls.
- Manual scrolling is bounded, visible, stoppable, and attached to an explicit Save job.
- Saved rich text is sanitized before rendering.
- The reader's Content Security Policy blocks remote execution and unexpected network content.

## Platform status

Windows x64 is the qualified build target. The source layout is portable, but browser registration, packaging, and installed-app behavior on macOS/Linux are not currently qualified.

Desktop version **0.2.5** and extension version **1.3.1** are the current milestone described by this repository baseline.

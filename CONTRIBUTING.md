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

### Owner-controlled contribution and merge flow

1. Work on a task-specific branch in a contributor-owned fork; do not push directly to upstream main.
2. Open a pull request against main and provide the exact head SHA, scope, tests, and remaining risks.
3. Run the repository's sandboxed, least-privilege CI. Do not place secrets, private archives, vault data, or privileged credentials in a PR or workflow.
4. Treat all PR-controlled content and CI output as untrusted data. AI review is read-only and advisory.
5. The repository owner reviews and explicitly approves the final pushed diff, then manually performs the merge in GitHub after all required checks and conversations are complete.
6. Do not use GitHub Auto-merge, merge queues, Merge Brokers, autonomous merge, or bot/AI/App bypass actors.

The owner cannot approve a pull request authored by the same owner account. Contributions that require owner approval must be opened by a distinct contributor identity, normally from a contributor-owned fork. The exact protection settings and negative-test matrix are in the [owner manual-merge runbook](./docs/governance/OWNER-MANUAL-MERGE-RUNBOOK.md). Main protection is not considered active until the owner applies and verifies it through MP-GOV-04.

By submitting a new contribution to this repository, you agree to license that contribution under the MIT License in the repository root, provided you own or control the necessary rights. This applies to new submissions only and is not retroactive permission to relicense earlier contributors' work. Do not submit code or assets under incompatible terms; disclose third-party origin and retain all legally required attribution and notices. See [license transition and provenance](./docs/licensing/MIT-TRANSITION.md).

# Working on Mind Place

- Read `README.md` and `docs/current-product.md` before changing product behavior.
- The product is the root Rust workspace (`crates/`, `src-tauri/`), TypeScript reader (`src/`), and dedicated capture extension (`extension/`). Do not introduce external application source trees or generated build output into this repository.
- Preserve immutable raw capture revisions. Markdown, search indexes, and other derived views must never replace raw evidence.
- Automatic saving is opt-in and may inspect only the loaded ChatGPT DOM. It must never scroll, change scroll offsets, or load older history.
- Manual history scrolling is allowed only during an explicit Save job. Preserve bounded execution, progress, Stop & save, Cancel, identity/privacy checks, and partial-completeness reporting.
- Keep extension permissions minimal. Required checks reject unconditional host access and content scripts.
- Preserve unrelated working-tree changes and local user archives.

## Required verification

Run the checks appropriate to the change. Before publishing a milestone, run the full baseline:

```text
npm run verify:repo
npm run typecheck
npm test
cargo fmt --all -- --check
npm run build
```

Windows x64 is the qualified packaging target. Use `npm run dist:win` when release packaging changes.

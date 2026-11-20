# Mind Place — Claude Code project instructions

@AGENTS.md

## UI work (read before touching `src/` UI, CSS, copy or layout)

The canonical rules are in `AGENTS.md` ("UI work rules") and `docs/ui/UI-PRINCIPLES.md`. Short version for Claude Code sessions:

- The UI is **human-first**: calm, plain, tidy. It is not a console for agents. The engineering priority list in `AGENTS.md` is about code and storage, not about what the user sees.
- The in-repo HTML guide is ground truth for what it shows. For anything it does **not** show, run the **gap protocol** (job, classify, place in an existing destination, build only from the pattern catalog and tokens, human copy through i18n, record in the PR design gap ledger). If nothing fits, stop and ask; do not improvise layouts.
- English and Indonesian ship together. No raw user-facing strings, no new colors or font sizes, no hashes/revisions/protocol names in default views.
- Never edit the guide to make your work fit. Never add a top-level destination.
- When asked to "implement feature X" in the UI and the guide is silent, your first output is the ledger entry and the chosen pattern, then the code.

## Working agreements for this repository

- GitHub is the source of truth; local Windows checkouts are qualification targets unless the owner authorizes local implementation. Do not commit, push or merge unless asked.
- Do not read, list or copy private archive/corpus contents; folder names and sizes are fine when needed.
- Report verification honestly: a test that was not run is `NOT_RUN`, a partial result is `PARTIAL`.

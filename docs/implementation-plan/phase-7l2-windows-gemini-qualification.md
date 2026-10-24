# MP-7L2 — Windows Gemini live-session qualification

**Status: BLOCKED (local automated checks PASS; real Gemini session NOT_RUN).**

- Gate: MP-7L2; [issue #97](https://github.com/KaisAbiyyi/mind-place/issues/97).
- Prerequisite: MP-7E [PR #75](https://github.com/KaisAbiyyi/mind-place/pull/75), merged.
- Session: `mp7l2-local-20261024-1325-wib`; owner: ChatGPT GPT-6 through Chat On Steroids Core.
- Exclusive resource: `gemini-account`. MP-7L1 independently owns `claude-account` and a separate worktree; do not change its branch, checkout, browser session or archived evidence.
- Test source SHA: `f07c90a70a1baf2374c28a25619c0d63d040c55d` (`origin/main` when worktree created).
- Isolated worktree: `D:/projects/mind-place-mp7l2-qualification`.
- Branch: `work/mp-7l2-gemini-qualification-20261024`.
- Platform: local Windows; Node `v24.21.0`, npm `11.19.0`, rustc `1.91.1`.

## Verified on local Windows

| Check | Result | Evidence |
| --- | --- | --- |
| `npm ci --no-audit --no-fund` | PASS | 78 packages installed into isolated worktree. |
| `npm run verify:repo` | PASS | Repository contract OK; desktop 0.2.5 and extension 1.3.1. |
| `node --test test/gemini-capture.test.mjs test/provider-integration.test.mjs test/extension-popup.test.mjs test/extension-worker.test.mjs` | PASS | 34 tests; 0 failures, 0 skips. Synthetic/isolated fixtures only. |
| `npm run typecheck` | PASS | `tsc --noEmit`. |
| `cargo test -p mind-place-capture-host --test provider_integration` | PASS | 2 tests; 0 failures. Native framed host checks provider isolation, revisions and spoof rejection using test data. |
| `cargo fmt --all -- --check` | PASS | Exit code 0. |
| Chrome/native messaging registration | PARTIAL | Chrome executable exists and runs; `HKCU` Chrome native messaging host key `com.mindplace.capture` exists. Registry presence does not prove the installed binary/extension is this source SHA. |

The supplied browser tab listing contained no Gemini tab. No signed-in Gemini page, real conversation, installed-extension version or live native receipt was observed. The agent did not load another browser profile, access a private archive, register/replace the installed host, modify browser state, or interfere with MP-7L1.

## Required real-session evidence (pending)

Use an authorized **regular Chrome** Gemini session and a harmless purpose-created test conversation. Ensure the installed extension/native host correspond to the target SHA, and perform the following with consent and safe per-session access. Redact conversation text, IDs, account identifiers and private paths before committing evidence.

1. Record exact extension/native build identity and Windows/Chrome versions. Confirm the manually invoked Mind Place popup starts from a saved HTTPS Gemini `/app/<id>` or `/u/<n>/app/<id>` conversation; unsupported/new/shared/lookalike routes must be rejected before native submission.
2. With the normal saved chat open, invoke explicit **Save this conversation**. Verify only rendered user and model turns are emitted, in order, with `provider=gemini`, the correct exact conversation identity, `capture_method=manual-dom`, `completeness=partial`, and nonempty omission warnings. Ensure no auto-save, scrolling, sidebar, hidden thinking, extra tab or provider API fetch occurs.
3. Confirm an actual native host **durable receipt** for the one-shot save and a visible revision in the Mind Place reader. Save unchanged content again and verify content deduplication without discarding the earlier evidence.
4. Regenerate or edit an answer in the test chat (if the live Gemini UI supports it), save again and verify that a distinct revision can be selected and older raw evidence remains immutable. Do not infer model identity, timestamps, variants, tool calls or remote attachments if the current UI does not expose them.
5. Test a genuinely active temporary/private Gemini mode, if offered: it must fail closed without a new archive revision. Separately verify rejection of non-Gemini URLs and navigation/identity changes. Do not put real private content in logs.
6. Search/fetch the saved Gemini test conversation through the authorized Mind Place retrieval/MCP path, verify provider isolation from ChatGPT/Claude, and confirm an unavailable/locked/revoked client cannot read it. Record redacted outcome and any product defect.

**Gate verdict remains `BLOCKED / live account NOT_RUN`** until every applicable live condition has an evidence-backed result. MP-7U2 remains blocked on MP-7L2. Run this qualification without taking the working browser/profile from the MP-7L1 owner. The documentation and branch may be reviewed independently; do not merge a statement of `PASS` or close issue #97 on synthetic-only proof.

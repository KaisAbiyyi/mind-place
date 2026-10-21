# MP-3L1-FIX-LOCK — manual desktop Lock
Date: 2026-10-21 (Asia/Jakarta). Status: IMPLEMENTED / CI AND WINDOWS PACKAGE QUALIFICATION IN PROGRESS.

## Scope and identity
- Task MP-3L1-FIX-LOCK; Codex GitHub-backed implementer; session 20261021-manual-lock; generation 1.
- Base `9e69d9721469040a67c251af004ec3473b015635`; branch `feature/mp-3l1-manual-lock`; [PR #18](https://github.com/KaisAbiyyi/mind-place/pull/18).
- Durable regression checkpoint `ff6e22387ebe1621f83f25a4153330ae875ed200`; implementation/test checkpoint `6e52b2734e497f764c65161ba24e439ad7bf3053`.
- Canonical changes authored through GitHub Git APIs. The isolated local worktree only checks out pushed commits for qualification; primary checkout and private archives remain unchanged.
- GitHub commit author verified as Kais Abiyyi, owner noreply identity. Actual agent provenance retained here and in PR.

## Behavior
- Lock is available in both the reader toolbar and setup dialog.
- Click immediately destroys the old reader and clears its transcript, setup, one-time credentials, input values and retained loaded state.
- Reader-scoped invocations reject work before/after disposal; late read/setup/credential results cannot populate a later unlock epoch.
- Existing native `lock_security` remains authoritative for epoch invalidation and MCP/tunnel shutdown. The unlock screen is displayed only after native lock and a fresh security-status check both confirm locked.
- Duplicate clicks issue one native lock. Failure or an unexpected unlocked result keeps the archive hidden and shows Retry Lock, with an accurate warning that services may still be running.
- Successful unlock creates a fresh reader and never automatically starts MCP or tunnel.
- No raw archive/state/index schema, extension permission or native authorization changes.

## Verification observed
- RED: existing reader failed the accessible Lock assertion at `ff6e22387ebe1621f83f25a4153330ae875ed200`; lifecycle tests also identified the missing application coordinator.
- GREEN: 4/4 behavioral regressions at `6e52b2734e497f764c65161ba24e439ad7bf3053`: reachable controls, duplicate clicks and late credential completion, failure/unconfirmed/retry, stale archive result plus fresh unlock without auto-start.
- `npm run verify:repo`, `npm run typecheck`, full `npm test` (107 web tests and Rust suite; one pre-existing storage benchmark intentionally ignored), and `cargo fmt --all -- --check`: PASS locally on the pushed checkpoint.
- Independent gpt-6-luna/max read-only review: no Critical/Important issues. Minor deferred suggestion: restore a safe background-index refresh warning (the refresh is presently silent on failure).
- Rendered browser fixture QA: installed Edge via Playwright, no Browser plugin available; desktop 1280x900 and 390x844 unlock screen. Setup Lock removed reader/dialog, focused PIN input, fresh unlock did not start MCP/tunnel. Native calls used synthetic fixtures; this is not installed native or real-client Lock acceptance. One resource 404 is being identified before final QA receipt.
- Full Windows GitHub CI and Windows build/package outcome: PENDING. Do not infer installed acceptance from fixture/browser/source proof.

## Qualification limits and recovery
- An inherited Cargo.lock mismatch regenerates 2 packages (httpdate/tokio-macros plus existing dependency edges) when the unmodified Rust baseline resolves dependencies. This is the same unrelated drift present in the primary checkout; this unit does not commit that WIP or claim a frozen --locked dependency qualification.
- Generated builds remain ignored. Private archives and real credentials never enter GitHub or fixtures.
- MP-3L1 remains PARTIAL until an updated installed build is explicitly unlocked, local MCP/client access works, owner clicks Lock and the next real client request loses access.
- MP-3L2 and MP-3U remain NOT RUN. No merge, package or hosted CI bypasses them.

NEXT: finish Windows CI and package verification, hand off exact final SHA, install/qualify the corrected build, then return to the remaining real-client Lock gate.

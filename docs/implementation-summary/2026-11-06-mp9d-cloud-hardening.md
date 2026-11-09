# MP-9D — Cloud parser and connector attack hardening

Date: 2026-11-07 (Asia/Jakarta). Claim: ChatGPT GPT-6 Cloud, generation 2 (reclaim). Tracking: [Issue #150](https://github.com/KaisAbiyyi/mind-place/issues/150), [Notion execution task](https://app.notion.com/p/400c4a6aaf7381808bfac7eea41bc76d).

## Boundaries

- Required predecessors: MP-9B accepted source foundation (merged via PR #132) and MP-7E (merged PR #75).
- Resource lock: `privacy-boundaries`. Implementation uses the GitHub cloud branch only; no local Windows checkout, real vault, personal exports, credentials, installed browser, or real tunnel connection.
- Lock and unlock epoch, read-only MCP catalog, loopback bind, origin/bearer restrictions, index/source authorization, and existing parser size/depth limits remain authoritative.
- Owner-controlled main merge/release, native Windows/real connector certification, and repository settings are **not** authorized by this task.

## Changes

1. MCP loopback transport: cap active `spawn_blocking` dispatch workers at four with a permit owned by each blocking closure, so connection timeout/disconnect does not prematurely return a worker slot. Saturation fails closed with HTTP 503. The existing maximum eight concurrent sockets and 35-second HTTP deadline remain.
2. MCP header boundary: parse exact application/json and text/event-stream Accept ranges, validate RFC q-values (including default q=1 and explicit q=0), quoted optional parameters, duplicate q parameters, and malformed values; reject substring/wildcard spoofing. Existing Host/Origin and forwarded-header denials remain unchanged.
3. PDF scanner: add bounded ctx.poll() checks every 4 KiB while scanning long comments in content_text() and comments, name tokens, and whitespace in stream_filter(). Existing literal/hex token polling, 64-level literal nesting, extraction output budget, and fail-closed no-partial-output behavior remain intact.
4. Adversarial regression: preserve the existing deterministic PDF/OOXML/Markdown mutation corpus and add explicit Accept quality-value/HTTP regressions, long comment/name/whitespace revocation and deadline checks, PDF output compatibility, and deterministic four-slot dispatch timeout/disconnect/saturation/recovery tests.

## Validation requirements and limitations

- **Required before any acceptance:** `cargo test -p mind-place-documents -p mind-place-integration`, `cargo fmt --check`, relevant workspace CI, exact-HEAD review/security diff, and document output compatibility checks.
- The cloud implementation environment cannot run native Rust directly; hosted PR checks are the authoritative compile/test evidence.
- These changes provide **cooperative bounded-work hardening**, not an OS subprocess sandbox or proof of hard kill for a stalled parser, DB engine, or already-running blocking worker. Four stuck requests can still exhaust the dispatch slots until restart; this favors bounded load and fail-closed responses over unbounded work.
- Actual Windows installed host, private production input, real hosted tunnel, malicious office parser resource profiles at scale, and real-world performance are **NOT RUN** here. Do not promote them to PASS from synthetic CI.

## Reclaim and self-review — 2026-11-07

The existing PR #154 was reclaimed after checking the current claim and the exclusive
privacy-boundaries resource lock. MP-9C and MP-9E hold different locks. The branch
was inspected at the supplied initial HEAD before changes; no new PR, main-branch
write, local Windows checkout, merge, or release was performed.

### P2-A — Accept quality-value correctness

- Root cause: accepts_mcp_media_types() previously stripped parameters and compared
  only media-type names, so explicit q=0 and malformed q-values were treated as
  acceptable.
- Changed file/functions: crates/integration-core/src/transport.rs —
  is_http_token(), split_quoted(), valid_quoted_string(), parse_quality(),
  parse_accept_item(), and accepts_mcp_media_types().
- Behavior: exact case-insensitive media types only; q defaults to 1; q-values
  use integer thousandths and RFC three-decimal precision; q=0 is rejected;
  duplicate q parameters, malformed/out-of-range values, malformed parameters,
  duplicate required ranges, substring impostors, and wildcard-only acceptance
  fail closed. Valid quoted optional parameters are preserved and parsed.
- Regression tests: accept_media_types_require_both_explicit_positive_qualities,
  accept_media_types_reject_zero_and_invalid_quality_values,
  accept_media_types_reject_duplicate_and_malformed_parameters,
  accept_media_types_reject_substring_and_wildcard_spoofing, plus the
  HTTP-level weighted-positive and malformed/zero-quality negative cases in
  real_http_deny_rebinding_origin_and_locked_epoch.
- Result: PASS in hosted Rust and MCP HTTP tests.

### P2-B — bounded PDF inner scans

- Root cause: long comment, PDF name-token, and whitespace loops in
  content_text() and stream_filter() advanced without periodic authorization
  or deadline polling.
- Changed file/functions: crates/document-core/src/pdf.rs —
  shared skip_while() now calls ctx.poll() at bounded 4 KiB intervals and
  is used for content comments, dictionary comments, name tokens, and filter
  whitespace. Parser stop positions and delimiter ownership are unchanged.
- Regression tests: content_text_long_comment_scan_observes_revocation,
  stream_filter_long_comment_name_and_whitespace_scans_observe_revocation,
  and pdf_inner_comment_name_and_whitespace_scans_observe_deadlines.
  pdf_long_dictionary_and_content_skips_preserve_visible_text verifies that
  ordinary Flate PDF extraction still returns the same visible text.
- Result: PASS. The test context expires the deadline after its initial
  authorization check without wall-clock sleeps; long scanning paths observe
  the next bounded poll.

### P2-C — deterministic dispatch permit lifecycle

- Root cause: production already moved the permit into the blocking closure,
  but there was no focused test proving that request timeout/disconnect cannot
  return capacity while the worker continues.
- Changed file/functions: crates/integration-core/src/transport.rs —
  acquire_dispatch_permit() is the single fail-closed admission path used by
  handle_bounded(). The four-worker production limit is unchanged and the
  owned permit remains inside the spawn_blocking closure.
- Regression tests: four_dispatch_workers_exhaust_capacity_and_recovery_admits_next_request,
  http_timeout_does_not_release_permit_while_blocking_worker_is_active, and
  disconnected_client_does_not_release_permit_before_worker_exit.
  Tests use start/release channels and worker handles rather than arbitrary
  sleeps.
- Result: PASS. Four workers hold all four slots; the fifth admission returns
  HTTP 503; timeout and disconnect retain permits until actual worker exit; a
  later request acquires a returned slot.

### Adjacent security audit

- Host is exactly one 127.0.0.1:43731 value; Origin and forwarded headers are
  rejected before body processing.
- MCP requires POST /mcp, one exact JSON Content-Type, bounded body size,
  bounded output, and the now-strict Accept contract.
- mcp::dispatch() authorizes the bearer for Read before initialize/discovery/
  tools enumeration and rechecks live authorization before disclosure.
- The HTTP exchange pins and validates the unlock lease before and after dispatch.
- Connection admission remains capped at 8; blocking dispatch remains capped at
  4; HTTP deadline remains 35 seconds. A stalled worker is not force-killed:
  the slot remains occupied until it exits, so four stuck workers can still
  exhaust admission. This is bounded fail-closed admission, not process-level
  sandboxing.

### Qualification and residual limits

- Hosted Rust formatting, production-core compile, SQLCipher/vault feature
  compile, focused security tests, document tests, integration/MCP tests, and
  fast developer feedback were executed; exact-head workflow IDs are recorded
  in PR #154 and the Notion task.
- GitHub Actions trust-boundary checks were executed and passed on the requalified
  PR merge ref. The PR base metadata was refreshed to the current main head
  before final requalification; the tested merge ref uses the current main
  parent and exact PR source.
- Native Windows owner acceptance, real tunnel/production account, real private
  documents, 100K/1M scale qualification, hard process kill, and coverage-guided
  fuzzing remain NOT RUN. No native, production, or process-sandbox certification
  is claimed.
- PR #154 remains Draft / Review. Independent re-review is pending. No merge or
  release authorization is granted.

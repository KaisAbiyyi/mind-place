# MP-9D — Cloud parser and connector attack hardening

Date: 2026-11-06 (Asia/Jakarta). Claim: ChatGPT GPT-6 Cloud, generation 1. Tracking: [Issue #150](https://github.com/KaisAbiyyi/mind-place/issues/150), [Notion execution task](https://app.notion.com/p/400c4a6aaf7381808bfac7eea41bc76d).

## Boundaries

- Required predecessors: MP-9B accepted source foundation (merged via PR #132) and MP-7E (merged PR #75).
- Resource lock: `privacy-boundaries`. Implementation uses the GitHub cloud branch only; no local Windows checkout, real vault, personal exports, credentials, installed browser, or real tunnel connection.
- Lock and unlock epoch, read-only MCP catalog, loopback bind, origin/bearer restrictions, index/source authorization, and existing parser size/depth limits remain authoritative.
- Owner-controlled main merge/release, native Windows/real connector certification, and repository settings are **not** authorized by this task.

## Changes

1. MCP loopback transport: cap active `spawn_blocking` dispatch workers at four with a permit owned by each blocking closure, so connection timeout/disconnect does not prematurely return a worker slot. Saturation fails closed with HTTP 503. The existing maximum eight concurrent sockets and 35-second HTTP deadline remain.
2. MCP header boundary: match actual Accept media type tokens instead of substrings; reject forwarded source/proto/real-IP headers as well as existing Host/Origin and forwarded-host violations.
3. PDF scanner: periodically revalidate live cancellation/lease and elapsed-time checks while scanning long literal strings, hex tokens, and dictionary filters. Cap PDF literal nesting at 64 and token buffering using the existing extraction output budget; reject instead of exposing partial results on limit or revocation.
4. Adversarial regression: synthetic deterministic mutation corpus for PDF/OOXML/Markdown, long-token mid-parse cancellation, nested literal limit, tunnel forwarding header spoofing, and MIME impostors.

## Validation requirements and limitations

- **Required before any acceptance:** `cargo test -p mind-place-documents -p mind-place-integration`, `cargo fmt --check`, relevant workspace CI, exact-HEAD review/security diff, and document output compatibility checks.
- The cloud implementation environment cannot run native Rust directly; hosted PR checks are the authoritative compile/test evidence.
- These changes provide **cooperative bounded-work hardening**, not an OS subprocess sandbox or proof of hard kill for a stalled parser, DB engine, or already-running blocking worker. Four stuck requests can still exhaust the dispatch slots until restart; this favors bounded load and fail-closed responses over unbounded work.
- Actual Windows installed host, private production input, real hosted tunnel, malicious office parser resource profiles at scale, and real-world performance are **NOT RUN** here. Do not promote them to PASS from synthetic CI.

## Self-review checkpoint

Review against the exact PR HEAD after CI finishes. Reconfirm no public listener, no new API authority, no mutation or exfiltration path, and no disclosure on revoked/expired lease. Any failing check is CHANGES REQUIRED; leave PR Draft, task in Review, and do not merge.

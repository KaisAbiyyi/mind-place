# Phase 3A — transport-independent shared-brain integration core

Status: **DONE**. [PR #10](https://github.com/KaisAbiyyi/mind-place/pull/10) squash-merged at `a42c90fc4f874e870f3f0a669340af27bd672bcc`. [Windows product CI](https://github.com/KaisAbiyyi/mind-place/actions/runs/39125313066) passed in full.

## Boundary

New crate: `crates/integration-core` (`mind-place-integration`).
Its only three v1 tools are `memory_search`, `memory_fetch` and
`context_build`. They forward typed, strictly validated requests to the
Phase 2A/2B/2C index primitives. `tool_catalog()` exposes JSON-schema
descriptions for future MCP, HTTP, or other adapters. Tool output preserves
source, archive, immutable snapshot and message provenance. No derived-row IDs,
write/admin tools, arbitrary paths, graph, maintenance, file imports or network
listeners are introduced here.

`IntegrationCore::execute_batch` accepts 1–8 ordered calls, checks a live
Mind Place unlock epoch, *requires* an injected client read-authorization
callback, and rechecks both between calls, during source fetch/context work,
and before releasing the complete result. An error returns **no partial
batch response**. Search queries use the existing parameterized FTS5 engine;
unknown metadata filters fail closed instead of silently broadening retrieval.
The authorization callback must check current rights on every invocation,
not a stale session-level allow flag.

Limits: input envelope <= 64 KiB, serialized response <= 512 KiB, maximum
8 calls, plus all preexisting Phase 2 per-tool query, filter, source, byte,
token-proxy, latency, and cursor bounds. The serializer is bounded after each
call; transports must enforce raw request-body bounds before JSON parsing.
A latency budget is cooperative (blocking disk reads are not preemptible).

Security reality: **3A is not an authorized MCP server**. No external client
can invoke the core yet. Phase 3B must provide authenticated client identity,
scoped capabilities, durable revocation and audit before connecting any
transport. Phase 3C adds local MCP compatibility and Phase 3D hosted tunnel.
No credentials, tunnel secrets, account setup, or local environment changes
are required at 3A. This crate must never be exposed from a network handler
using an unconditional authorization callback.

## Verification / handoff

- `cargo test -p mind-place-integration` exercises schema/tool discovery,
  locked and epoch-invalidated requests, source/snapshot-provenanced
  search/fetch/context, batch authorization rechecks, invalid arguments and
  size/count bounds.
- `npm test` includes this package in the full Rust suite.
- Windows CI: repository verification, TypeScript checks, Rust formatting,
  all tests, desktop app build.
- Phase 3A does not require a local Windows or user-account acceptance gate.
  Proceed to **Cloud 3B — client authorization**, not directly to a listener.

## Final Phase 3A handoff

- Code/merge SHA: `a42c90fc4f874e870f3f0a669340af27bd672bcc` (`main`).
- Product CI: `39125313066` — all gates **PASS**.
- Local/manual qualification: **not required for Cloud 3A** (no executable MCP listener/hosted client surface in this unit).
- Next cloud unit: **3B — client authorization**; make capability checks fail-closed and immediately revocable before any transport can call the integration core.

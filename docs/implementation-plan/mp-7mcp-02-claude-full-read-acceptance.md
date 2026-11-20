# MP-7MCP-02 — Claude client read compatibility and acceptance

Status: **cloud source candidate; actual Claude client qualification NOT RUN**.
Task: [MP-7MCP-02](https://app.notion.com/p/40cc4a6aaf738123a7a2d157a415d5a6).
Execution: GitHub cloud API only. No owner vault, Windows runtime, live Claude identity, or secrets were accessed.

## Scope of this candidate

- Support handshake-era Streamable HTTP MCP `2025-03-26`, `2025-06-18`, and `2025-11-25` alongside existing modern `2026-07-28`.
- Negotiate the client's supported revision from its `initialize.params.protocolVersion` even if the initial HTTP request has no `MCP-Protocol-Version` header.
- Default **unversioned** HTTP to the baseline handshake era rather than incorrectly inferring the latest revision.
- Preserve mandatory read bearer authorization for initialize, tool discovery/list and calls. Modern `Mcp-Method` / `Mcp-Name` mirrors remain strict; older handshake clients do not need them.
- Keep only `memory_search`, `memory_fetch`, `context_build` in the read-only tool catalog. No write, key, maintenance, or raw filesystem tool is added.
- Keep existing source/snapshot identity, provenance, immutable revision fetch, bounded responses, `truncated` and `next_cursor` continuation. This patch does **not** remove resource bounds to make large data appear complete.
- Preserve the existing listener's localhost-only Host, Origin, deadline, body-size, unlock lease, revoke and request-time disclosure checks.

## Cloud tests added

`crates/integration-core/tests/mcp.rs` exercises all three handshake versions with valid credentials, initialize negotiation, read-only tool listing, indexed search, exact revision fetch in multiple cursor chunks, context building, initialized notification, unsupported protocol denial, and unauthenticated initialize denial. The live TCP test checks genuine HTTP initialization without a version header for all three older revisions.

These are **synthetic tests** and are not evidence that Claude itself connected. See the PR's exact-head CI results for execution status; a written test is not a PASS receipt.

## Known acceptance blockers — do not claim full-read PASS yet

1. **Actual client gate:** run the supported Claude MCP client (e.g. local Claude Code with Streamable HTTP) against the packaged Mind Place instance in an owner-authorized disposable dataset. Record client version, platform, transport, server binary SHA, negotiated protocol, initialization, tool list and request/response receipts. Claude browser-capture is a different subsystem.
2. **Per-collection grant gate (backend IMPLEMENTED, synthetic tests only; owner UI NOT built):** reads are now default deny. A credential needs the `read` capability **and** individual source grants (`ClientAuthority::grant_read_sources`; no wildcard). Search and context apply the allow-list in SQL before ranking and `LIMIT`; an exact fetch of an ungranted source fails exactly like an unknown source; every result is rechecked against the grants; any grant, revoke, disable, rotation or capability change during a request fails the whole batch closed. Grants are stored as namespaced `client_capabilities` rows (no schema bump: encrypted state cannot be migrated in place). Tests: `crates/integration-core/tests/read_grants.rs` (11) plus existing suites, native Windows SQLCipher run included. Owner-only Tauri commands exist (`grant_local_mcp_client_sources`, `revoke_local_mcp_client_sources`, `revoke_all_local_mcp_client_sources`, `list_local_mcp_client_sources`, typed wrappers in `src/mcp-read-grants.ts`), but **no screen calls them yet**. **Still missing before this gate can be PASS:** the owner-facing grant screen (needs the UI gap protocol, expected in UI2 Connections), exact-head hosted CI and owner review. **Until a screen exists, every credential issued by the current app reads nothing.** Source granularity is the grant unit; a collection/layer model and the cloud-tier curated-only narrowing from MP-ADR-006-A1 need the curated layer and tier/transport binding and are not part of this slice. Residual: `bm25` scores are computed over the whole index, so scoped scores depend on corpus statistics of ungranted sources.
3. **Negative security gate:** test unknown credential, wrong client, disallowed collection, revoked reader, Lock, lock/unlock epoch change, and in-flight responses under actual client traffic. The current code covers bearer/read-capability and unlock lease, but collection isolation is not established.
4. **Full-content gate:** use an owner-approved disposable conversation with many messages, large multi-byte text, multiple immutable revisions and provider/source metadata. Verify every byte is recoverable with repeated `memory_fetch` cursors; do not treat an individual truncated page as a complete revision. Validate search-to-fetch identity, timestamps, completeness and provenance.
5. **Rollout gate:** independently review and qualify the exact source against Windows Product and the encrypted vault before merging. Do not use an orphaned tunnel helper, HTTP 200 alone or a synthetic client as proof of Claude acceptance.

## Owner qualification checklist

- Start from the exact reviewed binary; unlock Mind Place and start the local MCP listener.
- Through the trusted Mind Place UI, issue a fresh *read-only* credential for a dedicated Claude identity. Never put its value in a Git command, issue, URL, test log, repository file or unprotected shell history.
- Configure the real Claude client using its secure credential mechanism and the loopback endpoint `http://127.0.0.1:43731/mcp`; do not substitute the OpenAI-only hosted tunnel.
- Preserve redacted evidence of the actual MCP initialize/version, tools/list, search, paged exact fetch, context build, wrong-scope denial, revoke/lock/epoch denial and reconnect behavior.
- Mark individual claims `PASS`, `FAIL` or `NOT_RUN`. The overall MP-7MCP-02 gate stays pending until every required claim is independently supported.

## Governance

No merge, main modification, production authorization, scope expansion or release is approved by this candidate. This is a **partial implementation** of MP-7MCP-02; do not enable MP-7MCP-03 Claude writes until both its separate write-authority contract and MP-7MCP-02 acceptance are complete.

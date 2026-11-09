# MP-7MCP-02 — Claude client read compatibility and acceptance

Status: **cloud source candidate with server-enforced per-client/per-collection read grants; actual Claude client and native owner qualification NOT RUN**.
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

## Per-client / per-collection read grants (added after the 9 Nov 2026 review)

A *collection* is an owner-defined, explicit, closed set of canonical `src:v1:` source IDs (max 64 collections, 2048 sources each, 16 grants per client). A reader sees exactly the union of the collections granted to it. **Deny by default:** a valid `Read` credential with no grant, a grant naming a deleted collection, or an empty collection reads nothing. `Read` capability alone is never authority over a source.

- **Resolution:** `StateStore::client_read_scope(hash)` authorizes the credential and resolves its source set in one read transaction, per request, uncached. Grants live in the existing `settings` table under reserved keys, so **no state schema bump** (an encrypted vault state cannot migrate in place without separately consented staging). A damaged record is `Corrupt` and fails closed; it never widens.
- **Enforcement points:** search applies the scope as a SQL predicate before ranking/`LIMIT` (`retrieve_scoped`); exact fetch refuses an out-of-scope `source_id` before any index/archive access and returns the same `InvalidQuery` as an unknown source (opaque); `context_build` scopes both candidate retrieval and every per-excerpt fetch. Cursors carry no authority (`{message_index, byte_offset}` only); each continuation re-checks scope for the requested source.
- **Live fence:** every authorization callback re-checks the credential AND that `security_policy.revision` still equals the value captured when the scope was resolved. Any grant, collection, capability, credential rotation, disable or revoke moves that revision, so in-flight work is discarded (`MP_INTEGRATION_DENIED`, HTTP 403). Consequence: an unrelated owner policy change during a request can abort it; the client simply retries.
- **Owner control:** trusted Tauri commands (`create/list/delete_local_mcp_collection`, `grant_local_mcp_collection`, `revoke_local_mcp_collection_grant`, `list_local_mcp_client_collections`) and a *Read access: collections* panel under Setup → Local AI connections. None is reachable through MCP.
- **Audit:** grant/revoke are recorded as `capability_granted` / `capability_revoked` (read) in the append-only client audit; no source IDs, names or tokens are stored.
- **Behavior change to disclose:** readers issued before this change have no grants and therefore read nothing until the owner grants a collection. The hosted ChatGPT tunnel issues an ephemeral per-start reader, so it also reads nothing until a follow-up lets the owner choose collections at tunnel start. `IntegrationCore::execute_batch` remains an UNSCOPED trusted in-process entry point; credentialed clients must use `execute_client_batch`.
- **Not covered here:** MP-9C redaction coordination (physical erasure/redaction is a separate task), Win+L semantics (follows the accepted ADR; only observed behavior is recorded), and per-collection filtering of non-evidence surfaces (none exist on the read MCP).

## Cloud tests added

`crates/integration-core/tests/scopes.rs` is the negative matrix (two synthetic collections, three readers): search filtering, filter-widening attempts and opacity vs. a fabricated source, `context_build` aggregation, direct fetch with known exact IDs, ungranted/unknown/malformed credentials, cursor reuse across clients/sources/revocation, grant revoke/re-grant/shrink/delete, capability revoke, rotation/disable/terminal revoke, lock then unlock epoch, owner-input validation and bounds, and audit hygiene. A unit test in `src/clients.rs` revokes a grant *during* a multi-call batch and proves every result is discarded. `crates/state-core/tests/read_scopes.rs` covers storage (union, persistence across reopen, fence movement, bounds, fail-closed on damage), and `tests/mcp_vault.rs` covers the encrypted-vault path with `--features vault-mcp`. Sabotaging enforcement makes 9 of the 11 matrix tests fail.

`crates/integration-core/tests/mcp.rs` exercises all three handshake versions with valid credentials, initialize negotiation, read-only tool listing, indexed search, exact revision fetch in multiple cursor chunks, context building, initialized notification, unsupported protocol denial, and unauthenticated initialize denial. The live TCP test checks genuine HTTP initialization without a version header for all three older revisions.

These are **synthetic tests** and are not evidence that Claude itself connected. See the PR's exact-head CI results for execution status; a written test is not a PASS receipt.

## Known acceptance blockers — do not claim full-read PASS yet

1. **Actual client gate:** run the supported Claude MCP client (e.g. local Claude Code with Streamable HTTP) against the packaged Mind Place instance in an owner-authorized disposable dataset. Record client version, platform, transport, server binary SHA, negotiated protocol, initialization, tool list and request/response receipts. Claude browser-capture is a different subsystem.
2. **Per-collection grant gate — cloud source implemented, synthetic-tested; native/owner proof NOT RUN:** see the section above. Independent review of the grant model and Windows/native confirmation on the exact packaged binary remain required.
3. **Negative security gate — synthetic matrix PASS in cloud tests; real-client traffic NOT RUN:** unknown credential, wrong client, ungranted/private collection, revoked grant, Lock/unlock epoch, cursors and in-flight revocation are covered synthetically. They must be repeated under actual Claude traffic (see the Indonesian owner tutorial, `mp-7mcp-02-owner-tutorial-id.md`).
4. **Full-content gate:** use an owner-approved disposable conversation with many messages, large multi-byte text, multiple immutable revisions and provider/source metadata. Verify every byte is recoverable with repeated `memory_fetch` cursors; do not treat an individual truncated page as a complete revision. Validate search-to-fetch identity, timestamps, completeness and provenance.
5. **Rollout gate:** independently review and qualify the exact source against Windows Product and the encrypted vault before merging. Do not use an orphaned tunnel helper, HTTP 200 alone or a synthetic client as proof of Claude acceptance.

## Owner qualification checklist

- Start from the exact reviewed binary; unlock Mind Place and start the local MCP listener.
- Through the trusted Mind Place UI, issue a fresh *read-only* credential for a dedicated Claude identity. Never put its value in a Git command, issue, URL, test log, repository file or unprotected shell history.
- Configure the real Claude client using its secure credential mechanism and the loopback endpoint `http://127.0.0.1:43731/mcp`; do not substitute the OpenAI-only hosted tunnel.
- Preserve redacted evidence of the actual MCP initialize/version, tools/list, search, paged exact fetch, context build, wrong-scope denial, revoke/lock/epoch denial and reconnect behavior.
- Create two disposable collections (one granted, one never granted) and verify the wrong-scope denial is indistinguishable from an unknown source.
- Mark individual claims `PASS`, `FAIL` or `NOT_RUN`. The overall MP-7MCP-02 gate stays pending until every required claim is independently supported.

## Governance

No merge, main modification, production authorization, scope expansion or release is approved by this candidate. This is a **partial implementation** of MP-7MCP-02; do not enable MP-7MCP-03 Claude writes until both its separate write-authority contract and MP-7MCP-02 acceptance are complete.

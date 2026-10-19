# Phase 3E — MCP and hosted tunnel cloud security qualification

**Status:** cloud implementation in PR qualification; no real remote ChatGPT or user-device proof.

## Threat model and security invariants

Mind Place is an explicitly unlocked, owner-operated local archive. Its only MCP origin is
`127.0.0.1:43731/mcp`. The optional official OpenAI Secure MCP Tunnel subprocess originates
an outbound private connection and forwards approved MCP calls to that loopback server; it does
not create a public listener on the user's machine. The remotely granted client is a separate
ephemeral **read-only** Phase 3B identity. The three exposed tools are `memory_search`,
`memory_fetch`, and `context_build`; owner Tauri commands never enter the MCP catalog.

Cloud 3E models malicious/buggy clients, forged Host/Origin/Forwarded headers, duplicated
Authorization headers, guessed admin tool names, client capability downgrades, stale/rotated
bearers, lock/stop races, unexpected helper termination, and a failed credential-revocation
database. No tests bypass the production MCP HTTP handler or its credential checks when
claiming transport coverage.

## Automated cloud qualification

The Windows product CI runs, in addition to its baseline gates:

1. `cargo test -p mind-place-integration --test mcp -- --nocapture` — HTTP-boundary
   simulated tunnel tests through actual local TCP and the production MCP router, plus
   modern/legacy protocol and source provenance checks.
2. `cargo test -p mind-place --bin mind-place hosted_tunnel::tests -- --nocapture` —
   owner-side tunnel command isolation, secret argv/env boundaries, invalid tunnel inputs,
   and mock subprocess unexpected-exit detection. An ignored test fixture is executed only
   as a child by its parent test.
3. `npm test`, repository verification, formatting, TypeScript typecheck, Windows Tauri
   compilation and desktop build — baseline compatibility gates.

| Abuse / requirement | Cloud verification |
|---|---|
| Private loopback only; Host + Origin + Forwarded hostility | TCP request suite asserts 403 before evidence |
| Read-only catalog / guessed owner/admin commands | Exactly 3 tools, all read-only; guessed names return 404 |
| Real source access over MCP | Discovery, list, search, exact source fetch and context pack |
| Escalation by claiming client capabilities | Admin-only bearer denied and privileged names inaccessible |
| Stale / disabled / revoked / rotated hosted reader | Same TCP origin must deny the next request (401) |
| Duplicated Authorization / modern version mismatch | HTTP 401 / RPC 400 |
| Lock invalidates reads | Existing TCP lock test rejects/disconnects, no evidence |
| Unexpected tunnel helper exit | Native subprocess fixture + dead-child detection; runtime watcher polls |
| Helper restart / old watcher racing new identity | Stop selection requires matching client identity |
| Local authority revocation persistence error | Fails closed by locking security and stopping MCP |
| Inherited admin / alternate proxy settings | Native command env removal checks, no shell or public URL |

**Transport boundary:** Cloud tests simulate the remote-to-loopback hop through an ordinary
HTTP client; they do NOT connect to OpenAI's live control plane, use a real tunnel ID, exercise
a real ChatGPT account or prove an Internet-facing service is unreachable. The app never
creates an Internet-facing listener by design, but real endpoint/account entitlement and
network routing need **3L2/3U**.

**Unexpected exit:** The desktop proactively checks its child process and revokes the
matching identity when the helper is observed dead; it does not wait for the user to click
status. The watchdog polls every 250 ms (not a hard real-time guarantee). Explicit lock,
stop, client revoke and local MCP stop remain synchronous. If a revoke cannot be persisted,
the session is locked and local MCP is stopped to avoid allowing stale authority.

**Operational limits:** The executable is selected from the fixed adjacent app directory
(no PATH lookup, shell or auto-download). This is not an automatic cryptographic
publisher-signature verification. The user must independently verify the official helper
and its release checksum before installing it. The API key is passed only in child process
environment and is not claimed to be inaccessible to privileged OS inspection. No
encryption-at-rest claim is made for existing archives.

## Local/user handoff: no cloud agent may mark these passed

### 3L1 — Windows direct MCP (local executor)

1. Pull qualified `main` on Windows and build/package the actual Tauri app. Open it and
   unlock using the owner's secret; import or refresh a **real existing** archive.
2. From the Mind Place MCP owner panel, start local MCP at
   `http://127.0.0.1:43731/mcp` and issue a one-time read-only credential.
3. Configure an actual Codex or Claude Code MCP client for Streamable HTTP at that URL,
   with `Authorization: Bearer <one-time-token>` in protected client configuration,
   not a repository, URL or chat.
4. Perform `tools/list`, `memory_search`, `memory_fetch`, `context_build`; verify
   provenance/source identity against the real archive. Disconnect, restart, lock and
   revoke the credential; confirm the next request is refused. Record test version,
   local Windows build SHA, response redactions and disposition.

### 3L2 — official hosted private tunnel (local/network executor)

1. Use the MP-3F Windows installer or portable package with its bundled official
   helper. Build-time pins and pre-launch integrity verification replace manual
   helper download/copy. Repair/reinstall the package if the helper is missing or damaged.
2. Provision an authorized Secure MCP Tunnel in the owner's OpenAI Platform workspace,
   assign the intended ChatGPT workspace, and obtain the private tunnel ID and runtime
   API key with appropriate Tunnels Read + Use permission. Do not share keys in chat.
3. Unlock Mind Place, start local MCP, enter tunnel ID/API key and explicitly consent
   to remote access; click Start secure tunnel. The process-running display alone is
   NOT evidence of a usable connection.
4. Connect through ChatGPT's available custom MCP/Tunnel setup and make an authenticated
   remote search/fetch call. Verify source evidence, no write/admin tool, wrong-token denial,
   stop/restart/revoke and lock behavior with a real network connection. Record the
   redacted request proof, app version and Windows/network environment.

### 3U — owner's real ChatGPT acceptance

In a new ChatGPT conversation with no Mind Place chat context, ask a small set of
preselected questions about your **actual** archived project history. Inspect precise
source and snapshot provenance, not just plausible prose. Explicitly revoke the hosted
client and confirm its very next tool request fails. Acceptance requires the user to
approve those observations, not a CI-green badge.

## Phase exit criteria

Cloud 3E can be marked **DONE / cloud qualified** only when the GitHub PR is reviewed,
all relevant Windows CI checks pass on the final SHA, and the report links the exact
commit and workflow evidence. Phase 3 as a whole remains **LOCAL/USER PENDING** until
3L1, 3L2, and 3U are separately accepted. No agent should imply a remotely connected
ChatGPT account or publish a public URL as a substitute.

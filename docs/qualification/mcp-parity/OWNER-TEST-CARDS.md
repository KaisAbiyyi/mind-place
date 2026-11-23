# Owner test cards: Release 1 read-only MCP clients

These cards cover the clients an agent cannot drive from a development machine (hosted apps that need your
account, your tunnel, or a public HTTPS endpoint). Each card is executable against a **disposable synthetic
archive** and never touches your real vault or archive.

## Before any card

1. Build or take the exact release under test and record its commit SHA and `mind-place.exe` sha256 (the matrix records both).
2. Close any running Mind Place (the local port `43731` must be free).
3. From the repository root, launch the disposable conformance instance:

   ```powershell
   node scripts/mcp-conformance/run.mjs serve --proxy
   ```

   This opens a **disposable** Mind Place with four synthetic chats, already unlocked, with local sharing on and
   a local recording proxy (useful when testing local or forwarded HTTPS connectors that can target the proxy).
   Press `Ctrl-C` when finished; it writes a scrubbed, SHA-bound session receipt under
   `outputs/mcp-conformance/<sha>/` and discards the temporary archive.
4. In that disposable app, open **Connections**:
   - Create **Key 1** (`<client>-main`), and **do NOT allow any chats yet** (leave it at 0 allowed chats for `P0` default deny!).
   - You will create a **separate Key 2** (`<client>-revoke-test`) later at `P4` so whole-client revocation never shares a credential with `P5`/`P6a`.
5. **Credential and tier hygiene:**
   - Never paste an access key, tunnel key, or API key into a chat, issue, command line, URL, screenshot, or file.
   - Use only the client's own credential input or an environment variable.
   - Preserve the **hosted vs local authorization boundary**: never pass a local-tier key to a hosted model through a local shell/CLI tool, and never use your real archive keys here.

## What every card checks (same order and wording for every client)

| # | Setup in disposable Mind Place | Prompt to give the client | Pass looks like |
| --- | --- | --- | --- |
| `P0` (default deny, BEFORE any grant) | Connect **Key 1** with **0 chats allowed** in Choose chats. | "Use the Mind Place connector. Search for `sentinel-medium-3`, then try to fetch it." | Zero search results; fetch returns an unknown-id style error; nothing from any chat is disclosed. |
| `P1` (scoped search) | Now open **Choose chats** for **Key 1** and allow ONLY **Conformance medium conversation**. | "Search Mind Place for `sentinel-medium-3`, then for `sentinel-canary-2`, then for `sentinel-second-2`. Give the number of results for each." | Hits for the first (`sentinel-medium-3`), **0** for the other two (`sentinel-canary-2` and `sentinel-second-2`). |
| `P2` (exact paged fetch) | Keep only **Conformance medium conversation** granted to **Key 1**. | "Read the whole conversation 'Conformance medium conversation' with the fetch tool. Use pages of at most 3000 bytes and keep following `next_cursor` until it is empty. Report the number of calls. Do the latest revision, then the earlier one." | 5 or more calls for the latest revision, ends with an empty `next_cursor`, earlier revision also readable. |
| `P3` (per-chat revoke) | Under **Choose chats** for **Key 1**, switch **Conformance medium conversation** OFF. | "Search Mind Place for `sentinel-medium-3` and try to fetch that conversation again." | 0 search results; fetch refused like an unknown id. |
| `P4` (whole-client revoke with **distinct Key 2**) | Create **Key 2** (`<client>-revoke-test`) and allow **Conformance medium conversation** on Key 2. Confirm Key 2 can search `sentinel-medium-3`. Then press **Revoke access** on **Key 2** (confirm). | "Search Mind Place for `sentinel-medium-3`." (using **Key 2**). | Refused / unauthorized (`401`/`403`); 0 results. |
| `P5` (Lock using **Key 1**) | Re-allow **Conformance medium conversation** on **Key 1** (verify Key 1 works again, leaving **Key 2** revoked). Then press **Lock** in Mind Place. | "Search Mind Place for `sentinel-medium-3`." (using **Key 1**). | Unreachable or refused; 0 results disclosed. |
| `P6a` (Unlock & reconnect **Key 1** without escalation) | Unlock Mind Place with the disposable session secret, start sharing again (do NOT restore Key 2). | "Search Mind Place for `sentinel-medium-3`, then `sentinel-canary-2`." (using **Key 1**). | **Key 1** reconnects with its existing connection and reads `sentinel-medium-3`; `sentinel-canary-2` still returns 0. |
| `P6b` (Revoked **Key 2** stays revoked) | Keep the same unlocked/restarted state from `P6a`. | Attempt a search or `tools/list` using **Key 2** (or verify on the wire/server audit that Key 2 still gets `401`/`403`). | **Key 2** remains permanently refused (`401`/`403`) after Lock/Unlock and reconnect. |

## Observable evidence requirement (hosted & manual clients)

- **Model text answers and screenshots alone are NOT sufficient** to mark a hosted client cell wire-verified `PASS` in `PARITY-MATRIX.md`; without sanitized transport/server tool-call evidence they may be recorded only as manual partial observations.
- Wire-verified `PASS` requires sanitized observable tool-call / server-side evidence bound to the exact commit SHA and `mind-place.exe` sha256 (for example, the scrubbed `owner-serve-*.json` proxy wire receipt from `run.mjs serve --proxy`, or a sanitized server-side MCP audit/transport summary from the disposable session with scoped client identity), plus `owner-result-template.json` filled out.

## Card: ChatGPT (hosted, Secure MCP Tunnel)

- **Needs:** Your OpenAI tunnel ID, a runtime API key (not an admin key), and ChatGPT developer mode on your plan.
- **Credential tier:** Hosted / cloud-tier tunnel credential only (never a local-tier key).
- **Steps:** In the disposable app, open **Connections > Advanced: connect hosted ChatGPT**, read the consent, tick it, enter the tunnel ID and runtime key in the app's own fields (never share them anywhere else), and start the tunnel. In ChatGPT, add the custom MCP server through the Tunnel option, then execute `P0` through `P6b`.
- **Note:** The app only reports that its helper is running. `Connected` means ChatGPT listed the tools and executed observable `tools/call` requests on the Mind Place endpoint (`P0`–`P6b`).

## Card: Claude app (desktop or web custom connector)

- **Needs:** An account that can add a custom connector, and an HTTPS MCP URL reachable by Claude. Mind Place Release 1 ships no general public gateway, so this surface is **NOT_RUN** unless you front the disposable loopback endpoint with your own TLS/HTTPS reverse proxy (pointing to the `run.mjs serve --proxy` loopback port so wire exchanges are captured without exposing secrets).
- **Steps:** Place the disposable Key 1 (and later Key 2 for `P4`/`P6b`) only in the connector's own credential field, then run `P0` through `P6b`. If the client cannot reach a local/tunneled address or your plan disallows custom connectors, record **PROVIDER_BLOCKED** with the exact vendor restriction.

## Card: Gemini (hosted app)

- **Needs:** A Gemini account that offers "Connect to an MCP server" or Developer Mode (`gemini.google.com/devmode`), and HTTPS reachability as in the Claude app card.
- **Schema check:** Use Developer Mode to verify whether Gemini accepts Mind Place's three read-only tool input schemas (`oneOf`, type arrays `["string", "null"]`, `exclusiveMinimum`, `format: "date-time"`, `additionalProperties: false`). Strict Gemini schema validators may reject some of these keywords — record any schema rejection verbatim as a finding.
- **Fallback/blocked rule:** Record **PROVIDER_BLOCKED** with the vendor message if your account is not eligible.

## Card: Antigravity CLI

- Antigravity's direct HTTP `mcp_config.json` does not expand `${ENV_VAR}` inside `headers`, so **never** pass a bearer key via command-line flags or store a literal bearer key in `~/.gemini/config/mcp_config.json`.
- Instead, use the workspace loopback stdio bridge (`scripts/mcp-stdio-bridge.mjs`), which reads the disposable key **only** from an environment variable (`MP_CONFORMANCE_TOKEN` or `MIND_PLACE_MCP_TOKEN`) and forwards only to `127.0.0.1`:
  - Automated conformance run: `node scripts/mcp-conformance/run.mjs real antigravity` (writes workspace-only `.agents/mcp_config.json` + session-only `--settings` allowlisting only `mcp(mpconformance/memory_search)`, `mcp(mpconformance/memory_fetch)`, and `mcp(mpconformance/context_build)`).
  - If you run manually in a disposable folder with `run.mjs serve --proxy`, configure `<workDir>/.agents/mcp_config.json` to launch `node <repo>/scripts/mcp-stdio-bridge.mjs --url http://127.0.0.1:<proxyPort>/mcp --token-env MIND_PLACE_MCP_TOKEN` and keep the disposable token in process memory only (do not write it to disk or command arguments). If your Antigravity account/mode blocks execution, leave the cell `NOT_RUN` or `PROVIDER_BLOCKED` with the sanitized reason.

## Card: Gemini CLI (eligible account)

- The harness writes project settings with `trust: false` for the server (no provider-side per-server trust). Approve only the three read-only Mind Place tools (`memory_search`, `memory_fetch`, `context_build`) when asked, or allow only those three tool names. Never enable `trust: true` or a blanket auto-approve mode.
- Run `node scripts/mcp-conformance/run.mjs real gemini-cli` on an eligible Workspace/paid Google account.

## Card: any local CLI that failed in the agent run

- If `PARITY-MATRIX.md` shows `PROVIDER_BLOCKED` or `NOT_RUN` for a local CLI (`claude-code`, `codex`, `gemini-cli`, `antigravity`), run `node scripts/mcp-conformance/run.mjs real <client-id>` from a terminal where that client is signed in with an eligible account, then `node scripts/mcp-conformance/run.mjs matrix` to regenerate the matrix on one exact SHA.

## Returning evidence

Send back: the filled `owner-result-template.json`, the scrubbed `owner-serve-*.json` / `real-*.json` receipts (they contain no keys), the exact commit SHA, and `mind-place.exe` sha256. Never send screenshots or logs showing an access key, unlock secret, tunnel ID, API key, or private archive data.

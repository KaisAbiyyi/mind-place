# Owner test cards: Release 1 read-only MCP clients

These cards cover the clients an agent cannot drive from a development machine (hosted apps that need your
account, your tunnel or a public HTTPS endpoint). Each card is executable. Nothing here uses your real archive.

## Before any card

1. Build or take the exact release under test and note its commit SHA and `mind-place.exe` sha256 (the matrix records both).
2. Close any running Mind Place (the local port 43731 must be free).
3. From the repository root, run:

   ```powershell
   node scripts/mcp-conformance/run.mjs serve
   ```

   This opens a **disposable** Mind Place with four synthetic chats, already unlocked, sharing on. It never touches your vault or archive. Press Ctrl-C when finished; the archive is discarded.
4. In that app open **Connections**, **Create access** for the client you test, then **Choose chats** and allow only **Conformance medium conversation**.
5. Never paste an access key, tunnel key or API key into a chat, issue, screenshot or file. Use the client's own credential field.

## What every card checks (same wording for every client)

| # | Prompt to give the client | Pass looks like |
| --- | --- | --- |
| P0 (before choosing chats) | "Use the Mind Place connector. Search for `sentinel-medium-3`, then fetch the first result." | zero results, or a not-found style error; nothing from any chat |
| P1 | "Search Mind Place for `sentinel-medium-3`, then for `sentinel-canary-2`, then for `sentinel-second-2`. Give the number of results for each." | a few results for the first, **0** for the other two |
| P2 | "Read the whole conversation 'Conformance medium conversation' with the fetch tool. Use pages of at most 3000 bytes and keep following `next_cursor` until it is empty. Report the number of calls. Do the latest revision, then the earlier one." | 5 or more calls for the latest revision, ends with an empty `next_cursor`, earlier revision also readable |
| P3 | In the app, switch the chat off under **Choose chats**. Repeat P1's first search and a fetch. | 0 results, fetch refused |
| P4 | In the app press **Revoke access** (confirm). Ask the client to search again. | refused or unauthorised; no results |
| P5 | In the app press **Lock**. Ask the client to search again. | unreachable or refused; no results |
| P6 | Unlock, start sharing again, allow the chat again. Ask P1's first search. | works again with the same connection; the canary and second chats still return 0 |

Record, for each prompt: PASS, FAIL or NOT_RUN, the client name and version (from its About screen), the date, the surface (app, web, CLI), and one redacted screenshot. Use `owner-result-template.json` in this folder.

## Card: ChatGPT (hosted, Secure MCP Tunnel)

Needs: your OpenAI tunnel ID, a runtime API key (not an admin key), ChatGPT developer mode on your plan.
Steps: in the disposable app, **Connections > Advanced: connect hosted ChatGPT**, read the consent, tick it, enter the tunnel ID and runtime key in the app's own fields (do not share them anywhere else), start the tunnel. In ChatGPT, add the custom MCP server through the Tunnel option, then run P0 to P6.
Note: the app only reports that its helper is running. Connected means ChatGPT actually listed the tools and answered P1.

## Card: Claude app (desktop or web custom connector)

Needs: an account that can add a custom connector, and a connector URL reachable by Claude over HTTPS. Mind Place Release 1 has no general public gateway, so this surface is expected to be **NOT_RUN** unless you provide your own HTTPS front door for the disposable instance. If you do, add it as a custom connector with the access key placed in the connector's own credential field, then run P0 to P6. If the app cannot reach a loopback address, record **PROVIDER_BLOCKED** and say why.

## Card: Gemini (hosted app)

Needs: a Gemini account that offers "Connect to an MCP server" or Developer Mode (`gemini.google.com/devmode`), and the same HTTPS reachability as the Claude card. Use Developer Mode to confirm tool schemas are accepted (this server's input schemas use `oneOf`, type arrays, `exclusiveMinimum` and `format`; strict schema subsets may reject them, and that is a finding to record, not hide). Record **PROVIDER_BLOCKED** with the vendor message if your account is not eligible.

## Card: Antigravity CLI

Antigravity's MCP configuration stores the Authorization header literally (no environment-variable expansion), so the agent run could not deliver the key without writing it somewhere persistent. You can, with a **disposable** key from the disposable app (never your real archive's key):

1. In the disposable app create access named "Antigravity test" and allow only "Conformance medium conversation".
2. In your own PowerShell, with the key held in a variable you do not echo, add the server for this test only (replace the URL port if you front it with the recording proxy), for example `agy mcp add --type http --header "Authorization: Bearer <key>" mpconformance http://127.0.0.1:43731/mcp`.
3. Run the P0 to P6 prompts in an interactive `agy` session, approving the Mind Place tool calls when asked.
4. Afterwards remove it: `agy mcp remove mpconformance`, then revoke the access in the app. Close the PowerShell window so the command history is not kept.

Record results in `owner-result-template.json`. (This card exists because the key is on a command line here; use a disposable key only.)

## Card: any local CLI that failed in the agent run

If a row in `PARITY-MATRIX.md` shows PROVIDER_BLOCKED or NOT_RUN for a local CLI, run `node scripts/mcp-conformance/run.mjs real <client-id>` from a normal terminal on a machine where that client is signed in with an eligible account (`claude-code`, `codex`, `gemini-cli`, `antigravity`). It writes a receipt under `outputs/mcp-conformance/<sha>/`, then `node scripts/mcp-conformance/run.mjs matrix` merges it.

## Returning evidence

Send back: the filled `owner-result-template.json`, the receipt files (they contain no keys), the SHA and exe hash. Do not send screenshots that show a key, a tunnel ID you consider private, or your account details.

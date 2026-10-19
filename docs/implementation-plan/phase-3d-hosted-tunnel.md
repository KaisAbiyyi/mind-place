# Phase 3D — Hosted AI tunnel and owner setup

**Status:** cloud implementation; Windows CI and downstream local/user qualification pending.

## Transport selection

Use the **official OpenAI Secure MCP Tunnel** (`openai/tunnel-client`) rather than creating an
Internet-facing listener or a Cloudflare Quick Tunnel. The helper makes outbound requests to
OpenAI's tunnel service and forwards the permitted `main` MCP channel to the existing
`http://127.0.0.1:43731/mcp` listener from Phase 3C. There is **no public Mind Place
endpoint** and no reverse proxy, ngrok URL, or inbound firewall exception.

Official references:

- https://developers.openai.com/api/docs/guides/secure-mcp-tunnels
- https://github.com/openai/tunnel-client
- https://github.com/openai/tunnel-client/blob/master/docs/configuration.md
- https://help.openai.com/en/articles/12584461-connecting-custom-mcp-servers-in-chatgpt

The desktop owns the single hosted helper process. Startup is explicit and requires an unlocked
session, a running local MCP listener, an existing tunnel ID, a runtime API key, and affirmative
consent that selected archive content will traverse the remote AI service. There is no hosted
write/admin route. Owner-only local Tauri commands are NEVER in the MCP catalog.

## Trust and credential boundaries

- The helper binary must be the verified official `tunnel-client.exe` placed beside the Mind
  Place executable on Windows (on development Unix, `tunnel-client` beside the executable).
  No arbitrary shell command, path from the webview, automatic binary download, or `PATH`
  lookup is accepted. Do not install an unverified third-party binary.
- The owner supplies an **OpenAI runtime API key** (`sk-...`, not an admin key) on each run;
  Mind Place does not store it. Key and backend bearer are provided only through the helper's
  process environment, never CLI arguments, URLs, disk config, raw logs, or Git commits.
- The desktop auto-issues a distinct **read-only** Phase 3B client credential for the hosted
  helper. `MCP_EXTRA_HEADERS` and `MCP_DISCOVERY_EXTRA_HEADERS` use an indirect
  `Authorization: env:MIND_PLACE_HOSTED_BEARER` value, resolved by the helper on the final
  origin hop only. Remote-provided Authorization headers may override this static value;
  Phase 3B then verifies the provided credential or rejects the request. No secret appears in
  the user-facing setup UI.
- Stopping local MCP, explicitly stopping the tunnel, revoking the helper client, and locking
  Mind Place kill the outbound process and revoke the distinct hosted credential. A helper
  exit is detected on the next local status poll and its credential is revoked. The underlying
  Phase 3C listener is still bound to `127.0.0.1`, still rejects Origin/browser requests and
  forwarding headers, and still enforces Host, size, deadlines, unlock epoch and live read grant.
- There is no claim of end-to-end ciphertext secrecy from OpenAI: tool queries, selected raw
  evidence and results traverse the OpenAI-hosted tunnel and the chosen AI client. A tunnel ID
  and the associated Platform/workspace permissions control who can attach to the tunnel.
  Users must trust the hosted provider before enabling it.
- Process-alive status is **not** proof of tunnel connectivity. A remote client must complete
  an authenticated tool call during Phase 3L2/3U to establish real-world request proof.
- Client process exits or crashes cannot be mistaken for network-ready. Status checks reap an
  exited helper; the secret is never re-shown. If revocation cannot be persisted, check the
  local client inventory and revoke the affected helper identity after unlocking.
- Deliberately no OAuth authorization server, no public OAuth-protected-resource endpoint,
  no anonymous discovery endpoint and no token in the ChatGPT URL. Those are separate
  protocols, not required to use the private Secure MCP Tunnel with the installed client.

## Owner setup (Windows / later Phase 3L2)

1. Install an official compatible `tunnel-client` release beside `Mind Place.exe`.
   Verify its origin and checksum using the project's official release information; do not
   substitute a similarly named executable from another publisher.
2. In your OpenAI Platform organization, create/assign a Secure MCP Tunnel to the intended
   ChatGPT workspace and obtain a **runtime** API key with Tunnels Read + Use. Tunnel
   creation or administrative edits may require separately privileged account access.
3. Unlock Mind Place and click **Start MCP**. Confirm the local reader is reachable and
   indexed. No remote tunnel auto-starts on unlock.
4. In the **Hosted ChatGPT connection** section, enter the full tunnel ID and runtime key,
   read the disclosure and affirm consent; click **Start secure tunnel**. Keys are immediately
   cleared from the input. The UI reports only that the helper is running.
5. In ChatGPT, open **Plugins → Add custom MCP server → Tunnel**; select the authorized
   tunnel or paste its ID. Configure an appropriate connection, review risk and tool
   permissions. Account/workspace entitlements may vary.
6. Test a known archived question using read-only `memory_search`, `memory_fetch` and
   `context_build`. Inspect provenance and check it reflects the expected archive. This
   last step remains explicitly **unverified** until performed with a real hosted account.
7. Stop the helper and confirm the same remote client cannot read. Restart only with new
   explicit consent and a new auto-issued read credential. Lock Mind Place and repeat
   revocation tests. The definitive live proof is Phase 3L2 + 3U, not cloud CI.

## Phase 3E and local handoff

Phase 3E needs simulated remote transport tests, unexpected helper-exit tests, stale/rotated
client checks, authorization header override cases, concurrency cancellation, lock/stop races,
and a security review of origin-only forwarding. Phase 3L1 independently qualifies a local
Codex/Claude Code connection; Phase 3L2 qualifies the outbound helper using a real account,
network, and current official binary. Phase 3U is final hosted ChatGPT end-user acceptance.

No account, tunnel, external domain, or external binary was provisioned by the cloud agent.
No Windows qualification has yet been performed.

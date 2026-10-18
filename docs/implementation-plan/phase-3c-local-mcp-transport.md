# Phase 3C — Local MCP transport

Status: cloud implementation under qualification.

Explicit trusted-owner start after unlock; binds only IPv4 127.0.0.1:43731 at http://127.0.0.1:43731/mcp. A locked process cannot start; lock/stop cancels admissions and prevents in-flight disclosure. No internet-facing listeners or tunnels.

Streamable HTTP JSON-RPC supports modern 2026-07-28 stateless server/discover, tools/list and tools/call plus legacy 2025-11-25 initialize/initialized and tool calls. No session IDs, SSE subscription, write tools, CORS, browser origins, or remote clients. Every method requires the 3B bearer client credential with live read capability. Origin rejected, Host required exactly 127.0.0.1:43731, POST only. Body cap 64 KiB, output cap 1 MiB, eight concurrent connections, 35-second per-request timeout.

Trusted desktop UI can start/stop server and issue one-time read-only credentials. Credentials are not written to any database, files, logs, URLs or command-line argument. Configure a local MCP client using Streamable HTTP endpoint and an Authorization Bearer token in its protected settings. Users must unlock and explicitly start service each app run; no background daemon in this phase.

Cloud qualification: MCP dispatcher regression tests and live TCP HTTP security cases, Windows product CI. Real device/client acceptance is Phase 3L1, not 3C. Hosted tunnel and remote setup belong to Phase 3D.

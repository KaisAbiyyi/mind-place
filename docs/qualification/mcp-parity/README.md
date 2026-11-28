# Release 1 read-only MCP parity qualification

Scope: **read only** (`memory_search`, `memory_fetch`, `context_build`). No write tool exists or is added. Tracks MP-7MCP-02, MP-7MCP-04, MP-7MCP-06, MP-PAR-01 and the read half of MP-PAR-02.

## Files

| File | What it is |
| --- | --- |
| `PARITY-MATRIX.md` | Generated matrix: clients by claims C1 to C9 at one frozen commit (see Honesty rules) |
| `matrix.json` | Machine-readable matrix (validated by `scripts/mcp-conformance/matrix.mjs`) |
| `receipts/` | Scrubbed receipts the matrix cites, with sha256 in `matrix.json` |
| `static-cells.json` | Cells that cannot be run from here, each with its reason or vendor blocker |
| `OWNER-TEST-CARDS.md`, `owner-result-template.json` | Executable cards for clients that need the owner's account or network |
| `LOCAL-READ-WP02.md` | MP-R1-WP02: one setup source for Claude Code and Antigravity, and the local-read cells (`run.mjs local-read`) |

## Honesty rules

- A provider cell is PASS only for a **real** provider client, seen on the wire by the recording proxy with its own announced client name and version, on the exact SHA. The synthetic HTTP row is a server reference and can never turn a provider cell green (enforced by the matrix validator and its tests).
- The model's own words are never evidence. Tool calls, cursors and results are taken from the proxy log.
- Every cell is explicit: PASS, FAIL, NOT_RUN or PROVIDER_BLOCKED. A cell without a receipt must say why.
- **One frozen version.** Every executed (PASS or FAIL) cell ran at the matrix commit, with the same conformance harness tree (`sourceIdentity().harness` binds `scripts/mcp-conformance`, `scripts/lib`, and `scripts/mcp-stdio-bridge.mjs` alongside the exact frozen Git commit) and the same product trees and executable. `validateMatrix` and the `matrix` phase refuse anything else. The commit that records the matrix adds only `docs/qualification/mcp-parity/`; the source SHA a reader should check out is the one printed in `PARITY-MATRIX.md`.
- **Encrypted store.** The runner provisions a fresh vault before any capture, verifies the app reports encrypted mode, and checks that the three vault databases have no plaintext SQLite header. It compares each owner-read revision with the in-memory capture submitted to the native host before using that text to check paged MCP results. Older receipts on the PR exercised the packaged legacy store.
- **Scope is stated per cell.** C4 for the synthetic row reads a long chat (over 256 KB, three revisions). A real client reads the medium multi-byte two-revision chat in small pages; the long-chat read through a real client is **NOT_RUN** (its result stream does not fit a CLI model's context) and each real C4 cell carries that in its `scope`.
- **A denial counts only with valid arguments.** An unknown id and a malformed call both answer 400 invalid params. C6 and C7 therefore prove the canonical valid arguments are accepted (HTTP 200 with content) while granted, are denied exactly like an unknown id before the grant and after the revoke, and record **both** the complete canonical request digest (`fullArgsDigest`, covering every argument field) and the revision identity digest (`identityArgs`), explicitly reporting `complete_args_equal` vs `identity_only_match` (and replaying `identity_only_match` arguments verbatim while granted). Receipts carry digests only, never ids.
- **No provider-side trust.** Drivers never switch on a client's per-server trust or auto-approve option (`trust: false` for Gemini CLI, enforced by a test; Antigravity uses a session `--settings` allowlist restricted to the three read-only Mind Place tool names, never `mcp(*)` or `--dangerously-skip-permissions`). Claude Code runs with an allow-list of exactly the three read-only tool names, and Codex with a read-only sandbox and the user's other servers disabled; what a client may read is decided only by Mind Place grants.
- **Proxy and diagnostic hygiene.** The recording proxy keeps an allowlist of request headers and strips query strings on every branch (including upstream 502 errors). All blocked-client diagnostics, failed-check console lines, proxy/driver previews, receipts, and optional debug artifacts (`MP_DEBUG_DIR`) pass through `scripts/mcp-conformance/hygiene.mjs` with fail-closed secret checks. Full RPC bodies stay in the harness process memory only; receipts carry counts, statuses, tool names, short snapshot prefixes and digests.
- **Deterministic concurrency & security regression (Rust).** `crates/integration-core/src/transport.rs` and `crates/integration-core/tests/mcp.rs` exercise parallel TCP requests, queued requests across `Lock` and `Unlock` epoch changes, per-chat grant revocation while queued, whole-client revocation while queued, request timeout and client disconnect permit preservation (`MAX_DISPATCH_TASKS`), connection-cap bursts (`MAX_CONNECTIONS`), and two distinct credentials with disjoint grants. Unbounded multi-hour soak stress is not claimed.
- **No secrets on CLI, URLs, or shared configs.** Clients receive bearer credentials only via environment variables (`Claude Code`, `Codex`, `Gemini CLI`) or via the loopback-only `scripts/mcp-stdio-bridge.mjs` (`Antigravity`), which reads the token strictly from an environment variable and rejects non-loopback hosts, query strings, and inline credential flags.

## Supported release installation for `mcp-stdio-bridge.mjs`

- **Standalone script (not bundled in installer):** `scripts/mcp-stdio-bridge.mjs` is currently a standalone Node.js (`>= 20`) script shipped in the repository and used by the conformance harness; it is **NOT yet bundled into the Windows desktop installer** (`mind-place.exe` / MSI / NSIS). Do not assume or claim installer integration from qualification runs.
- **Supported installation with a released build (without a repo checkout):**
  1. Ensure Node.js `>= 20` is installed.
  2. Copy `scripts/mcp-stdio-bridge.mjs` from the exact release tag/commit into a user-owned local directory (for example `%LOCALAPPDATA%\MindPlace\bin\mcp-stdio-bridge.mjs`) and verify its SHA-256 against the release commit.
  3. Point the client's workspace `.agents/mcp_config.json` at that installed script via `node <installed-path>\mcp-stdio-bridge.mjs --url http://127.0.0.1:43731/mcp --token-env MIND_PLACE_MCP_TOKEN`.
  4. Provide the bearer token solely through the named environment variable (`MIND_PLACE_MCP_TOKEN`) in process memory — never place credentials on command lines, URLs, or shared config files.

## Claims

C1 handshake and initialization · C2 tool discovery · C3 scoped search · C4 exact full-content fetch (paged, multi-byte) · C5 revisions, identity, provenance · C6 default deny and isolation · C7 immediate revocation (chat grant and whole client) · C8 Lock · C9 reconnect without escalation.

## How to reproduce

Prerequisites: Windows x64, `npm ci`, the packaged app from the exact SHA (`npm run build`, PowerShell so Strawberry Perl is used for OpenSSL), port 43731 free, the client installed and signed in.

```powershell
node scripts/mcp-conformance/run.mjs synthetic
node scripts/mcp-conformance/run.mjs real claude-code
node scripts/mcp-conformance/run.mjs real codex
node scripts/mcp-conformance/run.mjs real antigravity
node scripts/mcp-conformance/run.mjs local-read
node scripts/mcp-conformance/run.mjs matrix
```

The runner launches the packaged app with a disposable data directory and profile, provisions and unlocks a vault, and seeds it through the packaged native capture host. It drives the same commands as Connections and discards the synthetic vault after the run. Run from a clean, committed tree. The matrix requires receipts from that source SHA and store mode.

Framework tests (no app needed): `node --import tsx --test test/mcp-conformance-framework.test.mjs`.

## Encrypted-vault run on `dadedd82528c`

- The synthetic HTTP reference and Claude Code passed C1–C9. Codex passed C1–C9 on its second attempt after the first attempt duplicated a page and used a cursor that differed from the server response. The [first Codex receipt](receipts/attempt-1-codex.json) and the [passing retry](receipts/real-codex.json) record both outcomes.
- Antigravity's full run began on CLI 1.5.15. It negotiated the protocol and listed tools. The proxy observed zero tool calls. A [single-search diagnostic](receipts/diagnostic-antigravity.json) on CLI 1.5.17 also observed zero tool calls and reported that the tool was unavailable. The matrix records the full run.
- Gemini CLI 0.34.0 also negotiated the protocol and listed tools but exited before any tool call. Its [single-search diagnostic](receipts/diagnostic-gemini-cli.json) did not establish the cause. The account restriction described in `static-cells.json` was observed during an earlier legacy-store run.
- The synthetic suite verified a normal burst of six parallel searches. During the whole-client revoke probe, all six overlapping reads from the surviving credential received temporary HTTP 503 responses with no content. The revoked credential's next request was denied and the surviving credential recovered.
- Real-client reads of chats larger than 256 KB and the hosted app rows remain NOT_RUN. This run used a disposable corpus in a staged Windows package.

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

## Honesty rules

- A provider cell is PASS only for a **real** provider client, seen on the wire by the recording proxy with its own announced client name and version, on the exact SHA. The synthetic HTTP row is a server reference and can never turn a provider cell green (enforced by the matrix validator and its tests).
- The model's own words are never evidence. Tool calls, cursors and results are taken from the proxy log.
- Every cell is explicit: PASS, FAIL, NOT_RUN or PROVIDER_BLOCKED. A cell without a receipt must say why.
- **One frozen version.** Every executed (PASS or FAIL) cell ran at the matrix commit, with the same conformance harness tree and the same product trees and executable. `validateMatrix` and the `matrix` phase refuse anything else. The commit that records the matrix adds only `docs/qualification/mcp-parity/`; the source SHA a reader should check out is the one printed in `PARITY-MATRIX.md`.
- **Scope is stated per cell.** C4 for the synthetic row reads a long chat (over 256 KB, three revisions). A real client reads the medium multi-byte two-revision chat in small pages; the long-chat read through a real client is **NOT_RUN** (its result stream does not fit a CLI model's context) and each real C4 cell carries that in its `scope`.
- **A denial counts only with valid arguments.** An unknown id and a malformed call both answer 400 invalid params. C6 and C7 therefore prove the same arguments are accepted (HTTP 200 with content) while granted, are denied exactly like an unknown id before the grant and after the revoke, and that the real client's own call used those same arguments (compared by digest; receipts carry no ids).
- **No provider-side trust.** Drivers never switch on a client's per-server trust or auto-approve option (`trust: false` for Gemini CLI, enforced by a test). Claude Code runs with an allow-list of exactly the three read-only tool names, and Codex with a read-only sandbox and the user's other servers disabled; what a client may read is decided only by Mind Place grants.
- **Proxy hygiene.** The recording proxy keeps an allowlist of request headers and no query string. Full RPC bodies stay in the harness process memory only; receipts carry counts, statuses, tool names, short snapshot prefixes and digests.
- Not claimed: concurrency stress qualification (Lock or revoke while many requests are queued). Six parallel requests are covered by a Rust regression test only.
- Keys never reach a command line, URL, log, receipt or screenshot. Clients get them through an environment variable their own config expands (Antigravity is the exception documented in its receipt notes).

## Claims

C1 handshake and initialization · C2 tool discovery · C3 scoped search · C4 exact full-content fetch (paged, multi-byte) · C5 revisions, identity, provenance · C6 default deny and isolation · C7 immediate revocation (chat grant and whole client) · C8 Lock · C9 reconnect without escalation.

## How to reproduce

Prerequisites: Windows x64, `npm ci`, the packaged app from the exact SHA (`npm run build`, PowerShell so Strawberry Perl is used for OpenSSL), port 43731 free, the client installed and signed in.

```powershell
node scripts/mcp-conformance/run.mjs synthetic
node scripts/mcp-conformance/run.mjs real claude-code
node scripts/mcp-conformance/run.mjs real codex
node scripts/mcp-conformance/run.mjs real antigravity
node scripts/mcp-conformance/run.mjs matrix
```

The runner seeds a disposable synthetic archive through the real capture host, launches the packaged app, unlocks it with a disposable secret, and drives the same owner commands the Connections screen uses. Run from a clean, committed tree: `matrix` refuses a dirty tree and receipts from another SHA.

Framework tests (no app needed): `node --import tsx --test test/mcp-conformance-framework.test.mjs`.

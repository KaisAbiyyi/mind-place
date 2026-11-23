# Release 1 read-only MCP parity qualification

Scope: **read only** (`memory_search`, `memory_fetch`, `context_build`). No write tool exists or is added. Tracks MP-7MCP-02, MP-7MCP-04, MP-7MCP-06, MP-PAR-01 and the read half of MP-PAR-02.

## Files

| File | What it is |
| --- | --- |
| `PARITY-MATRIX.md` | Generated matrix: clients by claims C1 to C9 on one exact SHA |
| `matrix.json` | Machine-readable matrix (validated by `scripts/mcp-conformance/matrix.mjs`) |
| `receipts/` | Scrubbed receipts the matrix cites, with sha256 in `matrix.json` |
| `static-cells.json` | Cells that cannot be run from here, each with its reason or vendor blocker |
| `OWNER-TEST-CARDS.md`, `owner-result-template.json` | Executable cards for clients that need the owner's account or network |

## Honesty rules

- A provider cell is PASS only for a **real** provider client, seen on the wire by the recording proxy with its own announced client name and version, on the exact SHA. The synthetic HTTP row is a server reference and can never turn a provider cell green (enforced by the matrix validator and its tests).
- The model's own words are never evidence. Tool calls, cursors and results are taken from the proxy log.
- Every cell is explicit: PASS, FAIL, NOT_RUN or PROVIDER_BLOCKED. A cell without a receipt must say why.
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

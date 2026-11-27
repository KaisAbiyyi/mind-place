# MP-R1-WP02 local-read cells: Claude Code and Antigravity

Status: **setup source, guides and harness ready; both real-client cells NOT_RUN.** This file is not
qualification evidence. The committed receipts, `matrix.json` and `PARITY-MATRIX.md` are unchanged; they are
regenerated only from a real run on one frozen SHA.

Out of scope: Gemini CLI and Claude Desktop (the remote surfaces are MP-R1-WP09).

## What changed

| Piece | Where |
| --- | --- |
| One setup source: three read tools by exact name, no wildcard or whole-server rule, no provider-side auto-approve, no per-tool `eager`, key by environment variable only, loopback `127.0.0.1` only | `scripts/lib/mcp-local-read-setup.mjs` |
| Owner command: `print`, `write` (never overwrites), `guides [--check]` | `scripts/mcp-local-read-setup.mjs` |
| EN/ID guides rendered from that source (drift fails `npm run verify:repo`) | `docs/setup/mcp-local-read.en.md`, `docs/setup/mcp-local-read.id.md` |
| Conformance drivers build their Claude Code and Antigravity files from the same source (harness server name, proxy URL and token variable only differ) | `scripts/mcp-conformance/clients.mjs` |
| `run.mjs local-read`: both clients back to back on one clean SHA, versions probed before and after, summary receipt | `scripts/mcp-conformance/run.mjs`, `scripts/mcp-conformance/local-read.mjs` |
| Auto-update pause requested for the client child process only | `UPDATE_PAUSE_ENV` in `clients.mjs` |

The bridge (`scripts/mcp-stdio-bridge.mjs`) and the MCP tool catalog are unchanged.

## Auto-update

- Claude Code: the harness sets `DISABLE_AUTOUPDATER=1` (documented by Claude Code) for its child process.
- Antigravity 1.5.18 documents no switch. The harness sets `AGY_CLI_DISABLE_AUTO_UPDATE=1`, a name found in
  the 1.5.18 executable; **whether it works is not verified.** It is a request only.
- The gate is the version check, not the pause: a run whose client reports two versions fails C1
  (`clientVersionDrift`, MP-7MCP-06), and `local-read` also fails a cell whose version differs between the
  probe before the first run and the probe after the last.
- If you can pause Antigravity updates another way, set `MP_ANTIGRAVITY_AUTO_UPDATE=paused` before the run; the
  summary then records the owner's declaration. Otherwise it records `recorded (version probed before and after)`.

Versions seen on the owner machine when this was prepared (2026-11-27, not a run): Claude Code 2.1.322,
Antigravity 1.5.18.

## How to run (owner, Windows x64)

1. Check out the exact PR head SHA, clean tree, `npm ci`, then `npm run build` from PowerShell.
2. Close Mind Place (port `43731` free). Both clients installed and signed in.
3. Do not use Antigravity interactively during the run (an interactive session can update it).

```powershell
node scripts/mcp-conformance/run.mjs local-read
```

4. Send back `outputs/mcp-conformance/<sha12>/local-read-summary.json` and the two `real-*.json` receipts it
   cites (scrubbed; they contain no keys). The cell is PASS only if the summary says `PASS`.

## Cells

| Cell | Result | Note |
| --- | --- | --- |
| Claude Code local read (C1-C9) on one SHA | NOT_RUN | needs the owner's Windows run above |
| Antigravity local read (C1-C9) on one SHA | NOT_RUN | needs the owner's Windows run above |
| Drift check (no version change in or between runs) | NOT_RUN | evaluated by `local-read` |
| Setup source and guide checks (`node --test test/mcp-conformance-framework.test.mjs`) | see PR | automated, no client |

/**
 * Real provider-client drivers. Each driver launches the ACTUAL vendor client
 * (not a reimplementation) pointed at a Mind Place endpoint (normally the
 * recording proxy) and returns what the client printed. Evidence of what the
 * client did comes from the proxy log, not from the client's own words.
 *
 * Credentials: the bearer key reaches a client only through an environment
 * variable that the client's own config expands, never a command line, URL or
 * file written by this harness (Antigravity is handled separately, see below).
 */
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn, spawnSync } from 'node:child_process';

export const TOKEN_ENV = 'MP_CONFORMANCE_TOKEN';
// Distinct from any connector the account may already have (for example a hosted "mind_place" app).
const SERVER_NAME = 'mpconformance';

function which(command) {
  const result = spawnSync('where.exe', [command], { encoding: 'utf8', windowsHide: true });
  if (result.status !== 0) return null;
  const lines = result.stdout.split(/\r?\n/).map((l) => l.trim()).filter(Boolean);
  return lines.find((l) => /\.(exe|cmd)$/i.test(l)) ?? lines[0] ?? null;
}

/**
 * npm installs CLIs as .cmd shims. Running them through a shell breaks on paths with spaces and
 * mangles quoting, so resolve the Node entry script the shim points at and run it directly.
 */
function launcher(command) {
  if (!/\.cmd$/i.test(command)) return { command, prefix: [] };
  const text = fs.readFileSync(command, 'utf8');
  const entryMatch = text.match(/"%dp0%\\([^"]+\.js)"/i);
  if (!entryMatch) throw new Error(`Cannot resolve the Node entry behind ${path.basename(command)}`);
  const flagMatch = text.match(/"%_prog%"\s+((?:--[\w=.-]+\s+)*)"%dp0%/i);
  const flags = flagMatch ? flagMatch[1].trim().split(/\s+/).filter(Boolean) : [];
  return { command: process.execPath, prefix: [...flags, path.join(path.dirname(command), entryMatch[1])] };
}

function version(command, args = ['--version'], timeout = 90000) {
  const l = launcher(command);
  const result = spawnSync(l.command, [...l.prefix, ...args], { encoding: 'utf8', windowsHide: true, timeout });
  return result.status === 0 ? String(result.stdout).trim().split(/\r?\n/).pop() : null;
}

/**
 * Runs a client ASYNCHRONOUSLY. This matters: the recording proxy and the app driver live in this same
 * process, so a blocking spawn would freeze the event loop and the client could never reach the proxy.
 */
function run(command, args, { input, env, cwd, timeoutMs }) {
  const l = launcher(command);
  return new Promise((resolve) => {
    const child = spawn(l.command, [...l.prefix, ...args], { cwd, env, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
    const out = [];
    const err = [];
    let timedOut = false;
    const timer = setTimeout(() => {
      timedOut = true;
      spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { windowsHide: true });
    }, timeoutMs ?? 300000);
    child.stdout.on('data', (c) => out.push(c));
    child.stderr.on('data', (c) => err.push(c));
    child.on('error', (error) => { err.push(Buffer.from(String(error.message))); });
    child.on('close', (code) => {
      clearTimeout(timer);
      resolve({ status: timedOut ? null : code, timedOut, stdout: Buffer.concat(out).toString('utf8'), stderr: Buffer.concat(err).toString('utf8') });
    });
    if (input) child.stdin.end(input); else child.stdin.end();
  });
}

function jsonLines(text) {
  return String(text).split(/\r?\n/).filter(Boolean).map((line) => { try { return JSON.parse(line); } catch { return null; } }).filter(Boolean);
}

/** @typedef {{url: string, token: string, prompt: string, timeoutMs?: number, workDir: string}} RunOptions */

export const DRIVERS = {
  'claude-code': {
    label: 'Claude Code',
    transport: 'streamable-http-loopback',
    detect() {
      const exe = which('claude');
      return exe ? { exe, version: version(exe) } : null;
    },
    /** @param {RunOptions} o */
    async run(o) {
      const found = this.detect();
      if (!found) throw new Error('Claude Code is not installed');
      const config = path.join(o.workDir, 'claude-mcp.json');
      fs.writeFileSync(config, JSON.stringify({
        mcpServers: { [SERVER_NAME]: { type: 'http', url: o.url, headers: { Authorization: `Bearer \${${TOKEN_ENV}}` } } },
      }));
      const tools = ['memory_search', 'memory_fetch', 'context_build'].map((t) => `mcp__${SERVER_NAME}__${t}`);
      const result = await run(found.exe, ['-p', '--mcp-config', config, '--strict-mcp-config', '--allowedTools', ...tools,
        '--model', 'haiku', '--max-turns', '60', '--output-format', 'stream-json', '--verbose'], {
        input: o.prompt, cwd: o.workDir, timeoutMs: o.timeoutMs ?? 300000,
        env: { ...process.env, [TOKEN_ENV]: o.token },
      });
      const events = jsonLines(result.stdout);
      const init = events.find((e) => e.type === 'system' && e.subtype === 'init');
      const server = (init?.mcp_servers ?? []).find((m) => m.name === SERVER_NAME);
      const discovered = (init?.tools ?? []).filter((t) => String(t).startsWith(`mcp__${SERVER_NAME}__`)).map((t) => String(t).slice(`mcp__${SERVER_NAME}__`.length));
      return {
        exit: result.status,
        clientVersion: found.version,
        connected: server?.status === 'connected',
        serverStatus: server?.status ?? 'absent',
        discoveredTools: discovered,
        finalText: String(events.find((e) => e.type === 'result')?.result ?? '').slice(0, 400),
        stderr: String(result.stderr).slice(0, 300),
      };
    },
  },

  codex: {
    label: 'Codex',
    transport: 'streamable-http-loopback',
    detect() {
      const exe = which('codex');
      return exe ? { exe, version: version(exe) } : null;
    },
    async run(o) {
      const found = this.detect();
      if (!found) throw new Error('Codex is not installed');
      // Config via -c overrides only: nothing is written to the user's ~/.codex.
      // The user's other MCP servers are switched off for this run (names only are read from the config).
      const others = codexConfiguredServers().filter((name) => name !== SERVER_NAME).map((name) => `mcp_servers.${name}.enabled=false`);
      const overrides = [
        `mcp_servers.${SERVER_NAME}.url=${JSON.stringify(o.url)}`,
        `mcp_servers.${SERVER_NAME}.bearer_token_env_var=${JSON.stringify(TOKEN_ENV)}`,
        'approval_policy="never"',
        // Account-level hosted connectors (codex_apps) must not be reachable from a conformance run.
        'features.apps=false',
        ...others,
      ].flatMap((kv) => ['-c', kv]);
      // The account's default model may not be available to Codex with a ChatGPT login; pick one that is.
      const model = process.env.MP_CODEX_MODEL ?? 'gpt-6-luna';
      const result = await run(found.exe, ['exec', '--json', '--skip-git-repo-check', '-s', 'read-only', '-m', model, ...overrides, '-'], {
        input: o.prompt, cwd: o.workDir, timeoutMs: o.timeoutMs ?? 300000,
        env: { ...process.env, [TOKEN_ENV]: o.token },
      });
      if (process.env.MP_DEBUG_DIR) fs.writeFileSync(path.join(process.env.MP_DEBUG_DIR, 'codex-stderr.log'), String(result.stderr).replace(new RegExp(String(o.token), 'g'), '[redacted]'));
      const events = jsonLines(result.stdout);
      return {
        exit: result.status,
        clientVersion: found.version,
        connected: null,
        events: events.length,
        summary: events.map((e) => [e.type, e.item?.type, e.item?.server, e.item?.tool, e.item?.status].filter(Boolean).join('/')),
        finalText: String(events.filter((e) => e.item?.type === 'agent_message').map((e) => e.item.text).at(-1) ?? '').slice(0, 400),
        stderr: String(result.stderr).slice(-400),
        stdoutTail: String(result.stdout).slice(-900),
      };
    },
  },

  'gemini-cli': {
    label: 'Gemini CLI',
    transport: 'streamable-http-loopback',
    detect() {
      const exe = which('gemini');
      return exe ? { exe, version: version(exe) } : null;
    },
    async run(o) {
      const found = this.detect();
      if (!found) throw new Error('Gemini CLI is not installed');
      // Project-scope settings inside a disposable working directory. The header value
      // references an environment variable that the CLI expands itself.
      const dir = path.join(o.workDir, '.gemini');
      fs.mkdirSync(dir, { recursive: true });
      fs.writeFileSync(path.join(dir, 'settings.json'), JSON.stringify({
        mcpServers: { [SERVER_NAME]: { httpUrl: o.url, headers: { Authorization: `Bearer \${${TOKEN_ENV}}` }, timeout: 60000, trust: true } },
      }));
      const result = await run(found.exe, ['-p', o.prompt, '--output-format', 'stream-json', '--allowed-mcp-server-names', SERVER_NAME], {
        cwd: o.workDir, timeoutMs: o.timeoutMs ?? 300000,
        env: { ...process.env, [TOKEN_ENV]: o.token },
      });
      const events = jsonLines(result.stdout);
      return {
        exit: result.status,
        clientVersion: found.version,
        connected: null,
        events: events.length,
        finalText: String(events.filter((e) => e.type === 'message' && e.role === 'assistant').map((e) => e.content).join('')).slice(-400),
        stderr: String(result.stderr).slice(-400),
        stdoutTail: String(result.stdout).slice(-900),
      };
    },
  },

  antigravity: {
    label: 'Antigravity',
    transport: 'streamable-http-loopback',
    detect() {
      const exe = which('agy') ?? path.join(os.homedir(), 'AppData', 'Local', 'agy', 'bin', 'agy.exe');
      if (!fs.existsSync(exe)) return null;
      return { exe, version: version(exe) };
    },
    async run(o) {
      const found = this.detect();
      if (!found) throw new Error('Antigravity is not installed');
      const env = { ...process.env, [TOKEN_ENV]: o.token };
      // Antigravity keeps MCP servers in its user configuration. The entry is added for this run only and
      // removed in finally. The header carries the NAME of an environment variable, not the key; whether
      // Antigravity expands it is part of what this run establishes.
      const placeholder = 'Authorization: Bearer ${' + TOKEN_ENV + '}';
      const add = await run(found.exe, ['mcp', 'add', '--type', 'http', '--header', placeholder, SERVER_NAME, o.url], { env, cwd: o.workDir, timeoutMs: 60000 });
      try {
        const result = await run(found.exe, ['--output-format', 'stream-json', '--mode', 'auto', `--print=${o.prompt}`], {
          cwd: o.workDir, timeoutMs: o.timeoutMs ?? 300000, env,
        });
        const events = jsonLines(result.stdout);
        return {
          exit: result.status,
          clientVersion: found.version,
          connected: null,
          addExit: add.status,
          events: events.length,
          finalText: String(result.stdout).slice(-300),
          stderr: String(result.stderr).slice(-400),
          stdoutTail: String(result.stdout).slice(-900),
        };
      } finally {
        await run(found.exe, ['mcp', 'remove', SERVER_NAME], { env, cwd: o.workDir, timeoutMs: 60000 });
      }
    },
  },
};

function codexConfiguredServers() {
  try {
    const text = fs.readFileSync(path.join(os.homedir(), '.codex', 'config.toml'), 'utf8');
    return [...text.matchAll(/^\[mcp_servers\.([A-Za-z0-9_-]+)\]\s*$/gm)].map((m) => m[1]);
  } catch { return []; }
}

export function makeWorkDir(label) {
  return fs.mkdtempSync(path.join(os.tmpdir(), `mp-conf-${label}-`));
}

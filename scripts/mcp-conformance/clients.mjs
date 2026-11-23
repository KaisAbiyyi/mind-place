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
import { spawnSync } from 'node:child_process';

export const TOKEN_ENV = 'MP_CONFORMANCE_TOKEN';
const SERVER_NAME = 'mindplace';

function which(command) {
  const result = spawnSync('where.exe', [command], { encoding: 'utf8', windowsHide: true });
  if (result.status !== 0) return null;
  const lines = result.stdout.split(/\r?\n/).map((l) => l.trim()).filter(Boolean);
  return lines.find((l) => /\.(exe|cmd)$/i.test(l)) ?? lines[0] ?? null;
}

function version(command, args = ['--version'], timeout = 60000) {
  const run = spawnSync(command, args, { encoding: 'utf8', windowsHide: true, timeout, shell: /\.cmd$/i.test(command) });
  return run.status === 0 ? String(run.stdout).trim().split(/\r?\n/).pop() : null;
}

function run(command, args, { input, env, cwd, timeoutMs }) {
  const shell = /\.cmd$/i.test(command);
  return spawnSync(command, args, {
    input,
    cwd,
    env,
    encoding: 'utf8',
    windowsHide: true,
    timeout: timeoutMs,
    maxBuffer: 64 * 1024 * 1024,
    shell,
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
    run(o) {
      const found = this.detect();
      if (!found) throw new Error('Claude Code is not installed');
      const config = path.join(o.workDir, 'claude-mcp.json');
      fs.writeFileSync(config, JSON.stringify({
        mcpServers: { [SERVER_NAME]: { type: 'http', url: o.url, headers: { Authorization: `Bearer \${${TOKEN_ENV}}` } } },
      }));
      const tools = ['memory_search', 'memory_fetch', 'context_build'].map((t) => `mcp__${SERVER_NAME}__${t}`);
      const result = run(found.exe, ['-p', '--mcp-config', config, '--strict-mcp-config', '--allowedTools', ...tools,
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
    run(o) {
      const found = this.detect();
      if (!found) throw new Error('Codex is not installed');
      // Config via -c overrides only: nothing is written to the user's ~/.codex.
      const overrides = [
        `mcp_servers.${SERVER_NAME}.url=${JSON.stringify(o.url)}`,
        `mcp_servers.${SERVER_NAME}.bearer_token_env_var=${JSON.stringify(TOKEN_ENV)}`,
        'approval_policy="never"',
      ].flatMap((kv) => ['-c', kv]);
      const result = run(found.exe, ['exec', '--json', '--skip-git-repo-check', '-s', 'read-only', ...overrides, '-'], {
        input: o.prompt, cwd: o.workDir, timeoutMs: o.timeoutMs ?? 300000,
        env: { ...process.env, [TOKEN_ENV]: o.token },
      });
      const events = jsonLines(result.stdout);
      return {
        exit: result.status,
        clientVersion: found.version,
        connected: null,
        events: events.length,
        finalText: String(events.filter((e) => e.item?.type === 'agent_message').map((e) => e.item.text).at(-1) ?? '').slice(0, 400),
        stderr: String(result.stderr).slice(-400),
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
    run(o) {
      const found = this.detect();
      if (!found) throw new Error('Gemini CLI is not installed');
      // Project-scope settings inside a disposable working directory. The header value
      // references an environment variable that the CLI expands itself.
      const dir = path.join(o.workDir, '.gemini');
      fs.mkdirSync(dir, { recursive: true });
      fs.writeFileSync(path.join(dir, 'settings.json'), JSON.stringify({
        mcpServers: { [SERVER_NAME]: { httpUrl: o.url, headers: { Authorization: `Bearer \${${TOKEN_ENV}}` }, timeout: 60000, trust: true } },
      }));
      const result = run(found.exe, ['-p', o.prompt, '--output-format', 'stream-json', '--allowed-mcp-server-names', SERVER_NAME], {
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
    run(o) {
      const found = this.detect();
      if (!found) throw new Error('Antigravity is not installed');
      const settings = path.join(o.workDir, 'agy-settings.json');
      fs.writeFileSync(settings, JSON.stringify({
        mcpServers: { [SERVER_NAME]: { type: 'http', url: o.url, headers: { Authorization: `Bearer \${${TOKEN_ENV}}` } } },
      }));
      const result = run(found.exe, ['--print', '--settings', settings, '--output-format', 'stream-json', '--mode', 'auto'], {
        input: o.prompt, cwd: o.workDir, timeoutMs: o.timeoutMs ?? 300000,
        env: { ...process.env, [TOKEN_ENV]: o.token },
      });
      const events = jsonLines(result.stdout);
      return {
        exit: result.status,
        clientVersion: found.version,
        connected: null,
        events: events.length,
        finalText: String(result.stdout).slice(-300),
        stderr: String(result.stderr).slice(-400),
      };
    },
  },
};

export function makeWorkDir(label) {
  return fs.mkdtempSync(path.join(os.tmpdir(), `mp-conf-${label}-`));
}

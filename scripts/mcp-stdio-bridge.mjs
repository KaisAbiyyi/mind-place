/**
 * Loopback-only stdio-to-Streamable-HTTP MCP bridge for clients whose config
 * does not expand environment variables in HTTP headers (such as Antigravity).
 *
 * Installation status: standalone Node.js (>= 20) script in the repository
 * (used by the conformance harness); it is NOT yet bundled into the Windows
 * desktop installer (`mind-place.exe` / MSI / NSIS). See
 * `docs/qualification/mcp-parity/README.md` for supported release usage.
 *
 * Security invariants:
 * - Reads the bearer credential ONLY from an environment variable (`--token-env`).
 *   Rejects `--token`, `--bearer`, `--header`, or any inline credential flag.
 * - Accepts ONLY loopback HTTP URLs (`127.0.0.1`, `localhost`, `[::1]`) with no
 *   query string and no embedded URL username/password.
 * - Never writes the bearer token to stdout, stderr, or errors.
 */
import http from 'node:http';
import path from 'node:path';
import readline from 'node:readline';
import { fileURLToPath } from 'node:url';

const DEFAULT_URL = 'http://127.0.0.1:43731/mcp';
const DEFAULT_TOKEN_ENV = 'MIND_PLACE_MCP_TOKEN';
const DEFAULT_PROTOCOL = '2025-03-26';
const REQUEST_TIMEOUT_MS = 35000;

export function parseBridgeArgs(argv) {
  let url = DEFAULT_URL;
  let tokenEnv = DEFAULT_TOKEN_ENV;
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--url') {
      url = argv[++i];
      if (!url) throw new Error('--url requires a value');
    } else if (arg === '--token-env') {
      tokenEnv = argv[++i];
      if (!tokenEnv) throw new Error('--token-env requires a value');
    } else if (/^--(token|bearer|header|authorization|secret|key)\b/i.test(arg)) {
      throw new Error(`Refusing inline credential flag ${arg}; use --token-env instead`);
    } else {
      throw new Error(`Unknown argument: ${arg}`);
    }
  }
  if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(tokenEnv)) {
    throw new Error('Invalid --token-env variable name');
  }
  const parsed = validateLoopbackUrl(url);
  return { url: parsed, tokenEnv };
}

export function validateLoopbackUrl(rawUrl) {
  let parsed;
  try {
    parsed = new URL(String(rawUrl));
  } catch {
    throw new Error('Invalid --url');
  }
  if (parsed.protocol !== 'http:') {
    throw new Error('Bridge only supports local http:// loopback URLs');
  }
  if (!['127.0.0.1', 'localhost', '[::1]', '::1'].includes(parsed.hostname)) {
    throw new Error(`Refusing non-loopback host: ${parsed.hostname}`);
  }
  if (parsed.username || parsed.password) {
    throw new Error('URL must not embed credentials');
  }
  if (parsed.search || parsed.hash) {
    throw new Error('URL must not contain query string or fragment');
  }
  return parsed;
}

function scrubBridgeError(msg, token) {
  let text = String(msg ?? 'Bridge error').replace(
    /mpc_v1_[0-9A-Za-z]{8,}|Bearer\s+[A-Za-z0-9._~+/=-]{12,}|sk-[A-Za-z0-9_-]{16,}/g,
    '[redacted]',
  );
  if (token && token.length >= 8) text = text.split(token).join('[redacted]');
  return text.slice(0, 240);
}

function forwardJsonRpc(targetUrl, token, bodyText, protocolVersion) {
  const headers = {
    'Content-Type': 'application/json',
    Accept: 'application/json, text/event-stream',
    'Content-Length': Buffer.byteLength(bodyText),
  };
  if (token) headers.Authorization = `Bearer ${token}`;
  if (protocolVersion) headers['MCP-Protocol-Version'] = protocolVersion;
  return new Promise((resolve, reject) => {
    const req = http.request(
      {
        host: targetUrl.hostname === '::1' || targetUrl.hostname === '[::1]' ? '::1' : targetUrl.hostname,
        port: Number(targetUrl.port || 80),
        path: targetUrl.pathname || '/mcp',
        method: 'POST',
        headers,
      },
      (res) => {
        const chunks = [];
        res.on('data', (chunk) => chunks.push(chunk));
        res.on('end', () => {
          resolve({ status: res.statusCode ?? 0, text: Buffer.concat(chunks).toString('utf8') });
        });
      },
    );
    req.setTimeout(REQUEST_TIMEOUT_MS, () => req.destroy(new Error('Upstream MCP timeout')));
    req.on('error', reject);
    req.write(bodyText);
    req.end();
  });
}

export function createBridgeSession({ targetUrl, token, writeOut, writeErr }) {
  let negotiatedProtocol = null;
  // Serialize stdin request forwarding so negotiatedProtocol updates deterministically.
  let queue = Promise.resolve();

  const handleLine = (line) => {
    const trimmed = String(line ?? '').trim();
    if (!trimmed) return Promise.resolve();
    queue = queue.then(async () => {
      let msg;
      try {
        msg = JSON.parse(trimmed);
      } catch {
        writeOut(JSON.stringify({ jsonrpc: '2.0', id: null, error: { code: -32700, message: 'Parse error' } }));
        return;
      }
      const isInit = msg?.method === 'initialize';
      const headerProto = isInit ? undefined : (negotiatedProtocol ?? DEFAULT_PROTOCOL);
      try {
        const reply = await forwardJsonRpc(targetUrl, token, trimmed, headerProto);
        if (reply.status === 202 || !reply.text.trim()) {
          return;
        }
        let parsedReply = null;
        try {
          parsedReply = JSON.parse(reply.text);
        } catch {
          parsedReply = null;
        }
        if (parsedReply) {
          if (isInit && reply.status === 200 && parsedReply?.result?.protocolVersion) {
            negotiatedProtocol = String(parsedReply.result.protocolVersion);
          }
          writeOut(JSON.stringify(parsedReply));
        } else if (msg?.id !== undefined && msg?.id !== null) {
          writeOut(JSON.stringify({
            jsonrpc: '2.0',
            id: msg.id,
            error: { code: -32001, message: reply.status === 401 || reply.status === 403 ? 'Unauthorized' : `HTTP ${reply.status}` },
          }));
        }
      } catch (error) {
        writeErr(`mcp-stdio-bridge: ${scrubBridgeError(error?.message ?? error, token)}\n`);
        if (msg?.id !== undefined && msg?.id !== null) {
          writeOut(JSON.stringify({
            jsonrpc: '2.0',
            id: msg.id,
            error: { code: -32000, message: 'MCP listener unreachable' },
          }));
        }
      }
    });
    return queue;
  };

  return { handleLine, flush: () => queue };
}

async function main() {
  const { url, tokenEnv } = parseBridgeArgs(process.argv.slice(2));
  const token = process.env[tokenEnv] ?? '';
  if (!token) {
    process.stderr.write(`mcp-stdio-bridge: environment variable ${tokenEnv} is unset\n`);
    process.exitCode = 1;
    return;
  }
  const session = createBridgeSession({
    targetUrl: url,
    token,
    writeOut: (line) => process.stdout.write(`${line}\n`),
    writeErr: (msg) => process.stderr.write(msg),
  });
  const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
  for await (const line of rl) {
    await session.handleLine(line);
  }
  await session.flush();
}

if (process.argv[1] && pathCompare(process.argv[1], fileURLToPath(import.meta.url))) {
  main().catch((err) => {
    process.stderr.write(`mcp-stdio-bridge: ${scrubBridgeError(err?.message ?? err, '')}\n`);
    process.exitCode = 1;
  });
}

function pathCompare(a, b) {
  return path.resolve(a).toLowerCase() === path.resolve(b).toLowerCase();
}

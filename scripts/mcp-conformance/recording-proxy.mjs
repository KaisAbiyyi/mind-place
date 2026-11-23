/**
 * Recording reverse proxy between a REAL MCP client and the Mind Place listener.
 *
 * Why it exists: a real provider client is an LLM-driven program. What it said
 * about a tool result is not evidence; what crossed the wire is. The proxy
 * records every JSON-RPC exchange (including the client's own `initialize`
 * `clientInfo`/`protocolVersion`, which a synthetic client cannot fake without
 * being detectable) so the harness can verify the real client's behaviour
 * independently of the model's transcript.
 *
 * Neutral: it never branches on a provider, forwards bytes unchanged apart from
 * the Host header (the listener enforces its own loopback Host), and NEVER
 * records or logs the Authorization header or any bearer value.
 */
import http from 'node:http';
import { scrubText } from '../lib/e2e-secret-guard.mjs';

const MAX_BODY = 1024 * 1024;

/**
 * @param {{targetPort?: number, secrets?: string[]}} [options]
 */
export async function startRecordingProxy(options = {}) {
  const targetPort = options.targetPort ?? 43731;
  const secrets = options.secrets ?? [];
  /** @type {Array<Record<string, any>>} */
  const exchanges = [];
  const sockets = new Set();
  let mode = 'forward'; // forward | drop (simulate a broken connection)

  const server = http.createServer((req, res) => {
    const chunks = [];
    let size = 0;
    req.on('data', (chunk) => {
      size += chunk.length;
      if (size > MAX_BODY) { req.destroy(); return; }
      chunks.push(chunk);
    });
    req.on('end', () => {
      const body = Buffer.concat(chunks);
      if (mode === 'drop') { req.socket.destroy(); return; }
      let rpc = null;
      try { rpc = body.length ? JSON.parse(body.toString('utf8')) : null; } catch { /* not JSON */ }
      const headers = { ...req.headers, host: `127.0.0.1:${targetPort}` };
      const upstream = http.request({ host: '127.0.0.1', port: targetPort, path: req.url, method: req.method, headers }, (reply) => {
        const parts = [];
        reply.on('data', (chunk) => parts.push(chunk));
        reply.on('end', () => {
          const out = Buffer.concat(parts);
          let json = null;
          try { json = out.length ? JSON.parse(out.toString('utf8')) : null; } catch { /* sse or empty */ }
          exchanges.push({
            at: new Date().toISOString(),
            http: req.method,
            path: req.url,
            protocolHeader: req.headers['mcp-protocol-version'] ?? null,
            userAgent: scrubText(req.headers['user-agent'] ?? '', secrets).slice(0, 160) || null,
            hadAuthorization: Boolean(req.headers.authorization),
            rpcMethod: rpc?.method ?? null,
            toolName: rpc?.method === 'tools/call' ? (rpc.params?.name ?? null) : null,
            params: rpc?.params ?? null,
            initialize: rpc?.method === 'initialize'
              ? { protocolVersion: rpc.params?.protocolVersion ?? null, clientInfo: rpc.params?.clientInfo ?? null }
              : null,
            status: reply.statusCode ?? 0,
            result: json?.result ?? null,
            error: json?.error ?? null,
            responseBytes: out.length,
          });
          const forwardHeaders = { ...reply.headers };
          res.writeHead(reply.statusCode ?? 502, forwardHeaders);
          res.end(out);
        });
      });
      upstream.on('error', () => {
        exchanges.push({ at: new Date().toISOString(), http: req.method, path: req.url, rpcMethod: rpc?.method ?? null, status: 502, upstreamError: true });
        res.writeHead(502); res.end();
      });
      upstream.end(body);
    });
  });
  server.on('connection', (socket) => { sockets.add(socket); socket.on('close', () => sockets.delete(socket)); });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const port = server.address().port;

  return {
    port,
    url: `http://127.0.0.1:${port}/mcp`,
    exchanges,
    /** Snapshot index, used to scope assertions to one client turn. */
    mark: () => exchanges.length,
    since: (mark) => exchanges.slice(mark),
    setMode(next) { mode = next; },
    dropConnections() { for (const s of sockets) s.destroy(); },
    async close() {
      for (const s of sockets) s.destroy();
      await new Promise((resolve) => server.close(resolve));
    },
  };
}

/** Extract the client identity a real client announced on the wire. */
export function observedClientIdentity(exchanges) {
  const init = exchanges.find((e) => e.initialize?.clientInfo);
  if (!init) return null;
  const { name, version } = init.initialize.clientInfo;
  return { name: String(name ?? ''), version: String(version ?? ''), protocolVersion: init.initialize.protocolVersion ?? null };
}

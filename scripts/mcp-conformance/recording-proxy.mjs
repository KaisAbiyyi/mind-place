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
 * The only request header names the proxy keeps. Full RPC params and results stay in this process's memory
 * (the harness needs them to verify content byte for byte) and are never written to a receipt; receipts carry
 * only counts, statuses, tool names, hashes and short ids (see real.mjs `exchange_summary`).
 */
export const RECORDED_HEADERS = ['content-type', 'accept', 'content-length', 'mcp-protocol-version', 'mcp-session-id', 'user-agent', 'host'];

/**
 * @param {{targetPort?: number, secrets?: string[]}} [options]
 */
export async function startRecordingProxy(options = {}) {
  const targetPort = options.targetPort ?? 43731;
  const secrets = options.secrets ?? [];
  /** @type {Array<Record<string, any>>} */
  const exchanges = [];
  /** Every request as it arrived (headers minus credentials), even if the upstream never answers. */
  const received = [];
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
      // Allowlist, not blocklist: only header names the conformance checks need are kept. Cookies, alternate
      // auth headers (x-api-key, proxy-authorization, ...) and anything unknown are dropped, and so is any
      // query string (a credential could ride in it).
      const safeHeaders = {};
      for (const name of RECORDED_HEADERS) if (req.headers[name] !== undefined) safeHeaders[name] = scrubText(String(req.headers[name]), secrets).slice(0, 160);
      const safePath = String(req.url ?? '').split('?')[0];
      received.push({ at: new Date().toISOString(), http: req.method, path: safePath, hadQuery: String(req.url ?? '').includes('?'), headers: safeHeaders, bodyBytes: body.length });
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
            path: safePath,
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
    received,
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

/**
 * Extract the client identity a real client announced on the wire. Handshake-era clients send it in
 * `initialize`; modern stateless clients carry it per request in `_meta`, and every HTTP client sends a
 * User-Agent. The source is returned so a reader can see how strong the evidence is.
 */
export function observedClientIdentity(exchanges) {
  const init = exchanges.find((e) => e.initialize?.clientInfo);
  if (init) {
    const { name, version } = init.initialize.clientInfo;
    return { name: String(name ?? ''), version: String(version ?? ''), protocolVersion: init.initialize.protocolVersion ?? null, source: 'initialize.clientInfo' };
  }
  for (const e of exchanges) {
    const meta = e.params?._meta ?? {};
    const key = Object.keys(meta).find((k) => /clientInfo$/i.test(k));
    if (key && meta[key]?.name) {
      return { name: String(meta[key].name), version: String(meta[key].version ?? ''), protocolVersion: meta['io.modelcontextprotocol/protocolVersion'] ?? e.protocolHeader ?? null, source: '_meta.clientInfo' };
    }
  }
  const ua = exchanges.map((e) => e.userAgent).find(Boolean);
  const m = ua && /^([A-Za-z][\w.-]*)\/(\d[\w.+-]*)/.exec(ua);
  if (m) return { name: m[1], version: m[2], protocolVersion: exchanges.find((e) => e.protocolHeader)?.protocolHeader ?? null, source: 'user-agent' };
  return null;
}

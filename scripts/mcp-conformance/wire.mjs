/**
 * Minimal MCP Streamable HTTP wire client for conformance checks. It speaks
 * both protocol eras the Mind Place listener supports and never branches on a
 * provider name: a "client" is only a bearer credential plus a protocol era.
 *
 * This is a SYNTHETIC client. Anything it observes is evidence about the
 * server's wire behaviour, never proof that a real provider client works.
 */
import http from 'node:http';

export const MODERN = '2026-07-28';
export const LEGACY_ERAS = ['2025-11-25', '2025-06-18', '2025-03-26'];
export const ENDPOINT = { host: '127.0.0.1', port: 43731, path: '/mcp' };

/**
 * @param {{port?: number, host?: string, path?: string, hostHeader?: string, timeoutMs?: number}} [options]
 */
export function createWire(options = {}) {
  const port = options.port ?? ENDPOINT.port;
  const host = options.host ?? ENDPOINT.host;
  const urlPath = options.path ?? ENDPOINT.path;
  const hostHeader = options.hostHeader ?? `${ENDPOINT.host}:${ENDPOINT.port}`;
  const timeoutMs = options.timeoutMs ?? 20000;
  let nextId = 1;

  /** One JSON-RPC POST. `era` is MODERN or a legacy handshake revision. */
  function post(token, method, params = {}, { era = MODERN, notification = false, extraHeaders = {} } = {}) {
    const modern = era === MODERN;
    const message = { jsonrpc: '2.0', method, params: { ...params } };
    if (!notification) message.id = nextId++;
    if (modern) {
      message.params._meta = {
        'io.modelcontextprotocol/protocolVersion': MODERN,
        'io.modelcontextprotocol/clientCapabilities': {},
      };
    }
    const body = JSON.stringify(message);
    const headers = {
      Host: hostHeader,
      'Content-Type': 'application/json',
      Accept: 'application/json, text/event-stream',
      'MCP-Protocol-Version': era,
      'Content-Length': Buffer.byteLength(body),
      ...extraHeaders,
    };
    if (token) headers.Authorization = `Bearer ${token}`;
    if (modern) {
      headers['Mcp-Method'] = method;
      if (method === 'tools/call') headers['Mcp-Name'] = params.name;
    }
    return new Promise((resolve, reject) => {
      const request = http.request({ host, port, path: urlPath, method: 'POST', headers }, (response) => {
        const chunks = [];
        response.on('data', (chunk) => chunks.push(chunk));
        response.on('end', () => {
          const text = Buffer.concat(chunks).toString('utf8');
          let json = null;
          try { json = text ? JSON.parse(text) : null; } catch { /* non-JSON body */ }
          resolve({ status: response.statusCode ?? 0, json, bytes: text.length });
        });
      });
      request.setTimeout(timeoutMs, () => request.destroy(new Error('MCP request timed out')));
      request.on('error', reject);
      request.write(body);
      request.end();
    });
  }

  const wire = {
    post,
    /** Legacy handshake: initialize then notifications/initialized. Modern is stateless (server/discover). */
    async handshake(token, era = MODERN) {
      if (era === MODERN) {
        const discover = await post(token, 'server/discover', {}, { era });
        return { era, discover };
      }
      const init = await post(
        token,
        'initialize',
        { protocolVersion: era, capabilities: {}, clientInfo: { name: 'mind-place-conformance', version: '1' } },
        { era },
      );
      const initialized = await post(token, 'notifications/initialized', {}, { era, notification: true });
      return { era, init, initialized };
    },
    toolsList: (token, era = MODERN) => post(token, 'tools/list', {}, { era }),
    call: (token, name, args, era = MODERN) => post(token, 'tools/call', { name, arguments: args }, { era }),
    search: async (token, queries, extra = {}, era = MODERN) => {
      const reply = await post(token, 'tools/call', { name: 'memory_search', arguments: { queries, ...extra } }, { era });
      return { ...reply, hits: reply.json?.result?.structuredContent?.hits ?? [] };
    },
    /** Reads a whole revision through repeated cursor calls; returns every page. */
    async fetchRevision(token, ref, { maxBytes, era = MODERN, maxPages = 400 } = {}) {
      const pages = [];
      let cursor = null;
      for (let i = 0; i < maxPages; i += 1) {
        const args = {
          source_id: ref.source_id,
          archive_id: ref.archive_id,
          snapshot_id: ref.snapshot_id,
          granularity: 'revision',
          ...(maxBytes ? { budget: { max_bytes: maxBytes } } : {}),
          ...(cursor ? { cursor } : {}),
        };
        const reply = await wire.call(token, 'memory_fetch', args, era);
        pages.push(reply);
        const result = reply.json?.result?.structuredContent;
        if (reply.status !== 200 || !result) break;
        cursor = result.next_cursor ?? null;
        if (!cursor) break;
      }
      return pages;
    },
  };
  return wire;
}

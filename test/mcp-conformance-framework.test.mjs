import assert from 'node:assert/strict';
import http from 'node:http';
import test from 'node:test';
import { CLAIMS, CLIENTS, renderMatrixMarkdown, validateCell, validateMatrix } from '../scripts/mcp-conformance/matrix.mjs';
import { observedClientIdentity, startRecordingProxy } from '../scripts/mcp-conformance/recording-proxy.mjs';
import { createWire } from '../scripts/mcp-conformance/wire.mjs';

const SHA = 'a'.repeat(40);
const HASH = 'b'.repeat(64);
const TOKEN = `mpc_v1_${'0123abcd'.repeat(8)}`;

const real = (over = {}) => ({
  result: 'PASS',
  sha: SHA,
  evidence: { kind: 'REAL_CLIENT', receipt: 'receipt.json', receipt_sha256: HASH, transport: 'streamable-http-loopback', observed_client: { name: 'claude-code', version: '2.1.322' } },
  ...over,
});

test('a provider row can never be PASS on synthetic evidence', () => {
  const problems = validateCell('codex', 'C3', { ...real(), evidence: { ...real().evidence, kind: 'SYNTHETIC_HTTP' } });
  assert.ok(problems.some((p) => /REAL_CLIENT evidence/.test(p)));
});

test('real-client PASS needs the client identity observed on the wire, the exact SHA and a receipt hash', () => {
  assert.deepEqual(validateCell('claude-code', 'C1', real()), []);
  assert.ok(validateCell('claude-code', 'C1', real({ sha: 'abc' })).some((p) => /exact 40-hex/.test(p)));
  assert.ok(validateCell('claude-code', 'C1', real({ evidence: { ...real().evidence, observed_client: { name: '', version: '' } } })).some((p) => /name and version/.test(p)));
  assert.ok(validateCell('claude-code', 'C1', real({ evidence: { ...real().evidence, receipt_sha256: 'x' } })).some((p) => /receipt/.test(p)));
});

test('the synthetic reference row only accepts synthetic evidence', () => {
  const synthetic = { result: 'PASS', sha: SHA, evidence: { kind: 'SYNTHETIC_HTTP', receipt: 'r.json', receipt_sha256: HASH } };
  assert.deepEqual(validateCell('synthetic-http', 'C4', synthetic), []);
  assert.ok(validateCell('synthetic-http', 'C4', real()).some((p) => /only carry SYNTHETIC_HTTP/.test(p)));
});

test('NOT_RUN and PROVIDER_BLOCKED are explicit and carry what the owner needs', () => {
  assert.ok(validateCell('chatgpt', 'C1', { result: 'NOT_RUN', reason: 'no tunnel' }).some((p) => /owner test card/.test(p)));
  assert.deepEqual(validateCell('chatgpt', 'C1', { result: 'NOT_RUN', reason: 'no tunnel', owner_card: 'OWNER-TEST-CARDS.md#chatgpt' }), []);
  assert.ok(validateCell('gemini-app', 'C1', { result: 'PROVIDER_BLOCKED' }).length >= 2);
  assert.deepEqual(validateCell('gemini-app', 'C1', { result: 'PROVIDER_BLOCKED', blocker: 'account not eligible', fallback: 'Gemini CLI' }), []);
});

test('a cell that contains a credential is rejected', () => {
  assert.ok(validateCell('claude-code', 'C1', real({ note: `used ${TOKEN}` })).some((p) => /secret/.test(p)));
});

test('a matrix must be complete, on one SHA, and renders every client', () => {
  const cells = {};
  for (const client of CLIENTS) {
    cells[client.id] = {};
    for (const claim of CLAIMS) {
      cells[client.id][claim.id] = client.synthetic
        ? { result: 'PASS', sha: SHA, evidence: { kind: 'SYNTHETIC_HTTP', receipt: 'r.json', receipt_sha256: HASH } }
        : { result: 'NOT_RUN', reason: 'not exercised', owner_card: 'OWNER-TEST-CARDS.md' };
    }
  }
  const matrix = { sha: SHA, generated_at: '2026-11-24T00:00:00Z', cells };
  assert.deepEqual(validateMatrix(matrix), []);
  const md = renderMatrixMarkdown(matrix);
  for (const client of CLIENTS) assert.match(md, new RegExp(client.label.replace(/[()]/g, '\\$&')));
  assert.match(md, /NOT_RUN/);
  delete matrix.cells.codex.C4;
  assert.ok(validateMatrix(matrix).some((p) => /codex\/C4: cell missing/.test(p)));
  matrix.cells.codex.C4 = { result: 'PASS', sha: 'c'.repeat(40), evidence: real().evidence };
  assert.ok(validateMatrix(matrix).some((p) => /differs from the matrix SHA/.test(p)));
});

/** Fake MCP listener that insists on its own Host and records what it received. */
async function fakeUpstream() {
  const seen = [];
  const server = http.createServer((req, res) => {
    const chunks = [];
    req.on('data', (c) => chunks.push(c));
    req.on('end', () => {
      const body = JSON.parse(Buffer.concat(chunks).toString('utf8'));
      seen.push({ host: req.headers.host, authorization: req.headers.authorization, method: body.method });
      if (req.headers.authorization !== `Bearer ${TOKEN}`) { res.writeHead(401); res.end(JSON.stringify({ jsonrpc: '2.0', id: body.id, error: { code: -32001, message: 'Unauthorized' } })); return; }
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ jsonrpc: '2.0', id: body.id, result: { protocolVersion: body.params?.protocolVersion ?? 'x' } }));
    });
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  return { seen, port: server.address().port, close: () => new Promise((resolve) => server.close(resolve)) };
}

test('recording proxy captures the real client identity and exchanges, rewrites Host, and never records the bearer', async () => {
  const upstream = await fakeUpstream();
  const proxy = await startRecordingProxy({ targetPort: upstream.port, secrets: [TOKEN] });
  try {
    const wire = createWire({ port: proxy.port, hostHeader: `127.0.0.1:${proxy.port}` });
    const init = await wire.post(TOKEN, 'initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'claude-code', version: '9.9.9' } }, { era: '2025-06-18' });
    assert.equal(init.status, 200);
    assert.equal(upstream.seen[0].host, `127.0.0.1:${upstream.port}`, 'Host is rewritten to the listener loopback host');
    assert.deepEqual(observedClientIdentity(proxy.exchanges), { name: 'claude-code', version: '9.9.9', protocolVersion: '2025-06-18', source: 'initialize.clientInfo' });
    const denied = await wire.post('wrong-token-0000000000', 'tools/list', {}, { era: '2025-06-18' });
    assert.equal(denied.status, 401);
    assert.equal(proxy.exchanges.at(-1).status, 401);
    assert.equal(proxy.exchanges[0].hadAuthorization, true);
    assert.doesNotMatch(JSON.stringify(proxy.exchanges), /mpc_v1_|wrong-token|Bearer/, 'no credential is recorded');
  } finally {
    await proxy.close();
    await upstream.close();
  }
});

test('recording proxy can simulate a dropped connection for reconnect checks', async () => {
  const upstream = await fakeUpstream();
  const proxy = await startRecordingProxy({ targetPort: upstream.port });
  try {
    const wire = createWire({ port: proxy.port, hostHeader: `127.0.0.1:${proxy.port}`, timeoutMs: 3000 });
    proxy.setMode('drop');
    await assert.rejects(() => wire.post(TOKEN, 'tools/list', {}, { era: '2025-06-18' }));
    proxy.setMode('forward');
    const again = await wire.post(TOKEN, 'tools/list', {}, { era: '2025-06-18' });
    assert.equal(again.status, 200);
  } finally {
    await proxy.close();
    await upstream.close();
  }
});

import { schemaFeatures, verifyRevision } from '../scripts/mcp-conformance/suite.mjs';

const seg = (i, text, start, end, total) => ({ message_id: `m-${i + 1}`, message_index: i, role: 'user', text, byte_start: start, byte_end: end, message_total_bytes: total });
const page = (segments, next) => ({ status: 200, json: { result: { structuredContent: { snapshot_id: 's1', segments, truncated: Boolean(next), next_cursor: next ?? null } } } });
const bytes = (t) => Buffer.byteLength(t);

test('verifyRevision accepts an exact multi-page, multi-byte reassembly', () => {
  const m0 = 'héllo 🙂 wörld';
  const m1 = 'second';
  const half = Buffer.from(m0).subarray(0, 8).toString('utf8');
  assert.ok(half.length > 0);
  const pages = [
    page([seg(0, 'héllo ', 0, bytes('héllo '), bytes(m0))], { message_index: 0, byte_offset: bytes('héllo ') }),
    page([seg(0, '🙂 wörld', bytes('héllo '), bytes(m0), bytes(m0)), seg(1, m1, 0, bytes(m1), bytes(m1))]),
  ];
  const verdict = verifyRevision(pages, [m0, m1], { snapshot_id: 's1' });
  assert.equal(verdict.ok, true, verdict.reason);
  assert.equal(verdict.pages, 2);
});

test('verifyRevision rejects gaps, silent truncation, wrong text and wrong counts', () => {
  const good = [page([seg(0, 'abc', 0, 3, 3)])];
  assert.equal(verifyRevision(good, ['abc'], { snapshot_id: 's1' }).ok, true);
  assert.match(verifyRevision(good, ['abd'], { snapshot_id: 's1' }).reason, /differs/);
  assert.match(verifyRevision(good, ['abc', 'x'], { snapshot_id: 's1' }).reason, /count/);
  assert.match(verifyRevision([page([seg(0, 'abc', 0, 3, 6)])], ['abc'], { snapshot_id: 's1' }).reason, /incomplete/);
  assert.match(verifyRevision([page([seg(0, 'abc', 0, 3, 3)], { message_index: 1, byte_offset: 0 })], ['abc'], { snapshot_id: 's1' }).reason, /silent truncation|truncated/);
  const gap = [page([seg(0, 'ab', 0, 2, 4)], { message_index: 0, byte_offset: 2 }), page([seg(0, 'd', 3, 4, 4)])];
  assert.match(verifyRevision(gap, ['abd'], { snapshot_id: 's1' }).reason, /gap or overlap/);
  assert.match(verifyRevision([{ status: 500, json: null }], ['abc'], {}).reason, /status 500/);
  assert.match(verifyRevision(good, ['abc'], { snapshot_id: 'other' }).reason, /snapshot differs/);
});

test('schemaFeatures reports the keywords strict clients may reject', () => {
  const features = schemaFeatures([{ inputSchema: { type: 'object', properties: { a: { oneOf: [{ type: 'string', minLength: 1 }, { type: ['string', 'null'] }] }, b: { type: 'number', exclusiveMinimum: 0 } }, additionalProperties: false } }]);
  assert.deepEqual(features, ['additionalProperties', 'exclusiveMinimum', 'minLength', 'oneOf', 'type-array']);
});

test('client identity falls back to modern _meta clientInfo, then the User-Agent, and says which', () => {
  const meta = [{ params: { _meta: { 'io.modelcontextprotocol/clientInfo': { name: 'codex', version: '0.164.0' }, 'io.modelcontextprotocol/protocolVersion': '2026-07-28' } } }];
  assert.deepEqual(observedClientIdentity(meta), { name: 'codex', version: '0.164.0', protocolVersion: '2026-07-28', source: '_meta.clientInfo' });
  const ua = [{ userAgent: 'codex_cli_rs/0.164.0 (Windows)', protocolHeader: '2026-07-28' }];
  assert.deepEqual(observedClientIdentity(ua), { name: 'codex_cli_rs', version: '0.164.0', protocolVersion: '2026-07-28', source: 'user-agent' });
  assert.equal(observedClientIdentity([{ rpcMethod: 'tools/list' }]), null);
});

import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
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
  store_mode: 'encrypted',
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
  const synthetic = { result: 'PASS', sha: SHA, store_mode: 'encrypted', evidence: { kind: 'SYNTHETIC_HTTP', receipt: 'r.json', receipt_sha256: HASH } };
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
        ? { result: 'PASS', sha: SHA, store_mode: 'encrypted', evidence: { kind: 'SYNTHETIC_HTTP', receipt: 'r.json', receipt_sha256: HASH } }
        : { result: 'NOT_RUN', reason: 'not exercised', owner_card: 'OWNER-TEST-CARDS.md' };
    }
  }
  const matrix = { sha: SHA, store_mode: 'encrypted', generated_at: '2026-11-24T00:00:00Z', cells };
  assert.deepEqual(validateMatrix(matrix), []);
  const md = renderMatrixMarkdown(matrix);
  for (const client of CLIENTS) assert.ok(md.includes(client.label), `rendered matrix lists ${client.id}`);
  assert.match(md, /NOT_RUN/);
  delete matrix.cells.codex.C4;
  assert.ok(validateMatrix(matrix).some((p) => /codex\/C4: cell missing/.test(p)));
  matrix.product_tree_id = 'tree-a';
  matrix.cells.codex.C4 = { result: 'PASS', sha: 'c'.repeat(40), product_tree_id: 'tree-b', evidence: real().evidence };
  assert.ok(validateMatrix(matrix).some((p) => /different product/.test(p)));
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

import { antigravitySessionSettings, antigravityWorkspaceConfig, geminiProjectSettings } from '../scripts/mcp-conformance/clients.mjs';
import { formatSafeJson, formatSafeLine, safePreview, safeUrlPath, sanitizeBlocked, scrubDiagnostic, writeDebugArtifact } from '../scripts/mcp-conformance/hygiene.mjs';
import { argsDigest, classifyFetchArgsMatch, fullArgsDigest, identityArgs } from '../scripts/mcp-conformance/real.mjs';
import { sourceIdentity, validateReceiptRun, verifyVaultFiles } from '../scripts/mcp-conformance/run.mjs';
import { isChatGptConversationUrl } from '../scripts/mcp-conformance/suite.mjs';
import { createBridgeSession, parseBridgeArgs, validateLoopbackUrl } from '../scripts/mcp-stdio-bridge.mjs';

test('Gemini CLI is never configured with provider-side per-server trust', () => {
  const settings = geminiProjectSettings('http://127.0.0.1:1/mcp');
  const servers = Object.values(settings.mcpServers);
  assert.ok(servers.length > 0);
  for (const server of servers) assert.equal(server.trust, false, 'trust must be explicitly false');
  // The key is referenced by environment variable name only.
  assert.doesNotMatch(JSON.stringify(settings), /mpc_v1_[0-9a-f]{64}/);
  // Static guard: no driver may turn a trust or auto-approve switch on.
  const source = fs.readFileSync(new URL('../scripts/mcp-conformance/clients.mjs', import.meta.url), 'utf8');
  assert.doesNotMatch(source, /trust\s*:\s*true/);
  assert.doesNotMatch(source, /--yolo|dangerously|bypassPermissions|danger-full-access/);
});

test('a provenance URL is accepted only for the exact chatgpt.com host and a conversation path', () => {
  assert.equal(isChatGptConversationUrl('https://chatgpt.com/c/abc-123'), true);
  for (const bad of ['https://evilchatgpt.com/c/abc', 'https://chatgpt.com.evil.test/c/abc', 'https://evil.test/?u=chatgpt.com/c/abc', 'http://chatgpt.com/c/abc', 'https://chatgpt.com/c/', 'https://chatgpt.com/other', 'not a url', '', null, undefined]) {
    assert.equal(isChatGptConversationUrl(bad), false, String(bad));
  }
});

test('the denial proof compares argument identity by digest, ignoring key order, cursor and budget', () => {
  const valid = { source_id: 'src:a', archive_id: 'ar:b', snapshot_id: 'sn:c', granularity: 'revision' };
  const same = { granularity: 'revision', snapshot_id: 'sn:c', archive_id: 'ar:b', source_id: 'src:a', cursor: { byte_offset: 3, message_index: 0 }, budget: { max_bytes: 3000 } };
  assert.equal(argsDigest(identityArgs(same)), argsDigest(valid));
  for (const wrong of [{ ...valid, snapshot_id: 'sn:other' }, { ...valid, granularity: 'message' }, { ...valid, source_id: undefined }]) {
    assert.notEqual(argsDigest(identityArgs(wrong)), argsDigest(valid));
  }
  assert.match(argsDigest(valid), /^[0-9a-f]{12}$/);
});

test('the matrix is one frozen version: every executed cell must run at the matrix commit and harness', () => {
  const OTHER = 'c'.repeat(40);
  const build = (over = {}) => {
    const cells = {};
    for (const client of CLIENTS) {
      cells[client.id] = {};
      for (const claim of CLAIMS) {
        if (client.synthetic) cells[client.id][claim.id] = { result: 'PASS', sha: SHA, store_mode: 'encrypted', harness_tree_id: 'h1', evidence: { kind: 'SYNTHETIC_HTTP', receipt: 'r.json', receipt_sha256: HASH } };
        else cells[client.id][claim.id] = { result: 'NOT_RUN', reason: 'not exercised', owner_card: 'OWNER-TEST-CARDS.md' };
      }
    }
    return { sha: SHA, store_mode: 'encrypted', harness_tree_id: 'h1', cells, ...over };
  };
  assert.deepEqual(validateMatrix(build()), []);
  const legacy = build();
  legacy.cells.codex.C1 = real({ harness_tree_id: 'h1', store_mode: 'legacy' });
  assert.ok(validateMatrix(legacy).some((p) => /executed cells require verified encrypted-vault evidence/.test(p)));
  const staleCommit = build();
  staleCommit.cells.codex.C1 = real({ sha: OTHER, harness_tree_id: 'h1' });
  assert.ok(validateMatrix(staleCommit).some((p) => /codex\/C1: cell ran at ccccccccc but the matrix is for aaaaaaaaa/.test(p)));
  const otherHarness = build();
  otherHarness.cells.codex.C1 = real({ harness_tree_id: 'h2' });
  assert.ok(validateMatrix(otherHarness).some((p) => /different conformance harness/.test(p)));
  const noScope = build();
  noScope.cells.codex.C4 = real({ harness_tree_id: 'h1' });
  assert.ok(validateMatrix(noScope).some((p) => /C4 cell must state its measured scope/.test(p)));
  noScope.cells.codex.C4 = real({ harness_tree_id: 'h1', scope: 'medium chat only; long chat NOT_RUN' });
  assert.ok(!validateMatrix(noScope).some((p) => /measured scope/.test(p)));
});

test('vault proof rejects a missing database, a plaintext SQLite header and a legacy archive', (t) => {
  const runtime = fs.mkdtempSync(path.join(os.tmpdir(), 'mp-vault-proof-'));
  t.after(() => fs.rmSync(runtime, { recursive: true, force: true }));
  const legacy = path.join(runtime, 'archive-v1');
  const vault = path.join(runtime, 'encrypted-vault-v1');
  fs.mkdirSync(vault);
  assert.throws(() => verifyVaultFiles(runtime, legacy), { code: 'ENOENT' });
  for (const name of ['archive-index.sqlite3', 'state.sqlite3', 'index.sqlite3']) {
    fs.writeFileSync(path.join(vault, name), Buffer.alloc(16, 1));
  }
  assert.doesNotThrow(() => verifyVaultFiles(runtime, legacy));
  fs.writeFileSync(path.join(vault, 'state.sqlite3'), Buffer.from('SQLite format 3\0'));
  assert.throws(() => verifyVaultFiles(runtime, legacy), /plaintext SQLite header/);
  fs.mkdirSync(legacy);
  assert.throws(() => verifyVaultFiles(runtime, legacy), /a legacy archive appeared/);
});

test('an interrupted run cannot publish passing checks gathered before the failure', () => {
  const receipt = { store_mode: 'encrypted', claims: { C7: { result: 'PASS' } } };
  assert.equal(validateReceiptRun(receipt), null);
  assert.match(validateReceiptRun({ ...receipt, harness_failure: 'Interrupted during C7' }), /did not complete/);
  assert.match(validateReceiptRun({ ...receipt, failure: 'App did not launch' }), /did not complete/);
  assert.match(validateReceiptRun({ ...receipt, store_mode: 'legacy' }), /encrypted vault/);
});

test('recording proxy keeps only allowlisted headers and no query string', async () => {
  const upstream = await fakeUpstream();
  const proxy = await startRecordingProxy({ targetPort: upstream.port, secrets: [TOKEN] });
  try {
    const body = JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'tools/list', params: {} });
    await new Promise((resolve, reject) => {
      const req = http.request({ host: '127.0.0.1', port: proxy.port, path: `/mcp?access_token=${TOKEN}`, method: 'POST', headers: {
        'content-type': 'application/json', authorization: `Bearer ${TOKEN}`, cookie: 'session=SECRETCOOKIE', 'x-api-key': 'SECRETAPIKEY', 'proxy-authorization': 'Basic SECRETPROXY', 'x-custom-token': 'SECRETCUSTOM', 'user-agent': 'unit/1.0',
      } }, (res) => { res.resume(); res.on('end', resolve); });
      req.on('error', reject);
      req.end(body);
    });
    const text = JSON.stringify({ received: proxy.received, exchanges: proxy.exchanges });
    assert.doesNotMatch(text, /SECRETCOOKIE|SECRETAPIKEY|SECRETPROXY|SECRETCUSTOM|access_token|mpc_v1_|Bearer/);
    assert.equal(proxy.received[0].path, '/mcp');
    assert.equal(proxy.received[0].hadQuery, true);
    assert.deepEqual(Object.keys(proxy.received[0].headers).sort(), ['content-length', 'content-type', 'host', 'user-agent']);
    assert.equal(proxy.exchanges[0].path, '/mcp');
  } finally {
    await proxy.close();
    await upstream.close();
  }
});

test('negative secret canary: upstream error, blocked diagnostics, failed check lines, and debug logs never leak secrets', async () => {
  const CANARY_TOKEN = `mpc_v1_${'cafe1234'.repeat(8)}`;
  const CANARY_UNLOCK = 'unlock-canary-9988776655';
  const CANARY_OPENAI = 'sk-canaryproj1234567890abcdef';
  const secrets = [CANARY_TOKEN, CANARY_UNLOCK, CANARY_OPENAI];

  // Dead local port to force upstream.on('error') -> 502 branch in recording-proxy.
  const dead = http.createServer();
  await new Promise((resolve) => dead.listen(0, '127.0.0.1', resolve));
  const deadPort = dead.address().port;
  await new Promise((resolve) => dead.close(resolve));

  const proxy = await startRecordingProxy({ targetPort: deadPort, secrets });
  try {
    const status = await new Promise((resolve, reject) => {
      const req = http.request({
        host: '127.0.0.1',
        port: proxy.port,
        path: `/mcp?access_token=${CANARY_TOKEN}&unlock=${CANARY_UNLOCK}`,
        method: 'POST',
        headers: {
          'content-type': 'application/json',
          authorization: `Bearer ${CANARY_TOKEN}`,
          cookie: `sid=${CANARY_UNLOCK}`,
          'x-api-key': CANARY_OPENAI,
          'user-agent': `canary-ua/${CANARY_UNLOCK}`,
        },
      }, (res) => { res.resume(); res.on('end', () => resolve(res.statusCode)); });
      req.on('error', reject);
      req.end(JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'tools/list', params: {} }));
    });
    assert.equal(status, 502);
    assert.equal(proxy.exchanges.length, 1);
    assert.equal(proxy.exchanges[0].upstreamError, true);
    assert.equal(proxy.exchanges[0].path, '/mcp');
    assert.equal(proxy.exchanges[0].hadQuery, true);
    const dumped = JSON.stringify({ received: proxy.received, exchanges: proxy.exchanges });
    assert.doesNotMatch(dumped, /cafe1234|unlock-canary|sk-canaryproj|access_token|mpc_v1_|Bearer/);
  } finally {
    await proxy.close();
  }

  // Blocked diagnostic, failed check console formatting, and debug artifact scrubbing.
  const dirtyBlocked = sanitizeBlocked({
    kind: 'CLIENT_COULD_NOT_USE_THE_SERVER',
    detail: `failed with ${CANARY_OPENAI}`,
    reason: `Authorization: Bearer ${CANARY_TOKEN} at /mcp?token=${CANARY_UNLOCK}`,
    requestsSeen: 2,
    exit: 1,
    serverStatus: 'refused',
    received: 2,
    stderr: `stderr tail ${CANARY_TOKEN} ${CANARY_UNLOCK}`,
    stdoutTail: `stdout tail ${CANARY_OPENAI} Bearer ${CANARY_TOKEN}`,
    extraForbiddenField: 'should-be-dropped',
  }, secrets);
  assert.equal('extraForbiddenField' in dirtyBlocked, false);
  const printedBlocked = formatSafeJson({ clientId: 'test', blocked: dirtyBlocked }, secrets);
  const failedLine = formatSafeLine(`FAIL C6 check :: detail ${CANARY_TOKEN} ${CANARY_UNLOCK} ${CANARY_OPENAI}`, secrets);
  assert.doesNotMatch(printedBlocked, /cafe1234|unlock-canary|sk-canaryproj|mpc_v1_|Bearer/);
  assert.doesNotMatch(failedLine, /cafe1234|unlock-canary|sk-canaryproj|mpc_v1_|Bearer/);
  assert.equal(safeUrlPath(`/mcp?token=${CANARY_TOKEN}`, secrets).path, '/mcp');
  assert.equal(scrubDiagnostic(`https://x.test/?key=${CANARY_UNLOCK}`, secrets).includes(CANARY_UNLOCK), false);

  const debugDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mp-hygiene-'));
  try {
    const file = writeDebugArtifact(debugDir, 'codex-stderr.log', `line1 ${CANARY_TOKEN}\nline2 ${CANARY_UNLOCK}\n`, secrets);
    const onDisk = fs.readFileSync(file, 'utf8');
    assert.doesNotMatch(onDisk, /cafe1234|unlock-canary|mpc_v1_/);
  } finally {
    fs.rmSync(debugDir, { recursive: true, force: true });
  }
});

test('permission evidence distinguishes complete canonical request equality from revision identity match', () => {
  const valid = { source_id: 'src:a', archive_id: 'ar:b', snapshot_id: 'sn:c', granularity: 'revision' };
  const exact = classifyFetchArgsMatch({ granularity: 'revision', snapshot_id: 'sn:c', archive_id: 'ar:b', source_id: 'src:a' }, valid);
  assert.equal(exact.mode, 'complete_args_equal');
  assert.equal(exact.completeArgsMatch, true);
  assert.equal(exact.identityMatches, true);
  assert.equal(exact.clientFullDigest, fullArgsDigest(valid));

  const extraFields = classifyFetchArgsMatch({ ...valid, budget: { max_bytes: 3000 }, cursor: null }, valid);
  assert.equal(extraFields.mode, 'identity_only_match');
  assert.equal(extraFields.completeArgsMatch, false);
  assert.equal(extraFields.identityMatches, true);
  assert.notEqual(extraFields.clientFullDigest, extraFields.canonicalFullDigest);

  const wrong = classifyFetchArgsMatch({ ...valid, snapshot_id: 'sn:other' }, valid);
  assert.equal(wrong.mode, 'mismatch');
  assert.equal(wrong.completeArgsMatch, false);
  assert.equal(wrong.identityMatches, false);
});

test('stdio bridge rejects non-loopback URLs, queries, and inline credentials, and forwards safely with env token', async () => {
  assert.throws(() => parseBridgeArgs(['--url', 'https://evil.example/mcp']), /http:\/\/ loopback/);
  assert.throws(() => parseBridgeArgs(['--url', 'http://192.168.1.5:43731/mcp']), /non-loopback/);
  assert.throws(() => parseBridgeArgs(['--url', 'http://127.0.0.1:43731/mcp?token=abc']), /query string/);
  assert.throws(() => parseBridgeArgs(['--token', TOKEN]), /inline credential flag/);
  assert.throws(() => parseBridgeArgs(['--header', `Authorization: Bearer ${TOKEN}`]), /inline credential flag/);
  assert.equal(validateLoopbackUrl('http://127.0.0.1:43731/mcp').hostname, '127.0.0.1');

  const upstream = await fakeUpstream();
  const proxy = await startRecordingProxy({ targetPort: upstream.port, secrets: [TOKEN] });
  const outs = [];
  const errs = [];
  try {
    const session = createBridgeSession({
      targetUrl: new URL(proxy.url),
      token: TOKEN,
      writeOut: (line) => outs.push(JSON.parse(line)),
      writeErr: (msg) => errs.push(msg),
    });
    await session.handleLine(JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'initialize', params: { protocolVersion: '2025-11-25', capabilities: {}, clientInfo: { name: 'antigravity', version: '1.5.15' } } }));
    assert.equal(outs.length, 1);
    assert.equal(outs[0].result.protocolVersion, '2025-11-25');
    assert.equal(upstream.seen[0].authorization, `Bearer ${TOKEN}`);
    assert.doesNotMatch(JSON.stringify({ outs, errs }), /mpc_v1_|Bearer/);
  } finally {
    await proxy.close();
    await upstream.close();
  }
});

test('Antigravity driver and owner test cards never place bearer tokens on CLI or grant wildcard mcp(*)', () => {
  const cfg = antigravityWorkspaceConfig('http://127.0.0.1:1/mcp');
  const server = cfg.mcpServers.mpconformance;
  assert.ok(server.args.some((a) => a.endsWith('mcp-stdio-bridge.mjs')));
  assert.ok(server.args.includes('--token-env'));
  assert.doesNotMatch(JSON.stringify(cfg), /mpc_v1_|Bearer|--header/);

  const settings = antigravitySessionSettings();
  assert.deepEqual(settings.permissions.allow, [
    'mcp(mpconformance/memory_search)',
    'mcp(mpconformance/memory_fetch)',
    'mcp(mpconformance/context_build)',
  ]);

  const clientsSource = fs.readFileSync(new URL('../scripts/mcp-conformance/clients.mjs', import.meta.url), 'utf8');
  assert.doesNotMatch(clientsSource, /mcp\s+add|--header|mcp\(\*\)|--dangerously-skip-permissions/);

  const cards = fs.readFileSync(new URL('../docs/qualification/mcp-parity/OWNER-TEST-CARDS.md', import.meta.url), 'utf8');
  assert.doesNotMatch(cards, /agy mcp add|--header\s+"Authorization|Close the PowerShell window/i);
  assert.match(cards, /P0.*BEFORE any grant/i);
  assert.match(cards, /Key 2/);
  assert.match(cards, /P6b/);
  assert.match(cards, /Do \*\*not\*\* expose the local loopback listener or local-tier keys over HTTPS/i);
  assert.match(cards, /owner-serve-\*\.json.*does \*\*not\*\* record byte-exact paged reassembly/is);
  assert.match(cards, /NOT yet bundled into the Windows desktop installer/i);
});

test('safePreview scrubs before truncating across head and tail boundary splits for all secret classes', async () => {
  const knownUnlock = 'unlock-boundary-9988776655443322';
  const knownMcp = `mpc_v1_${'cafe9876'.repeat(8)}`;
  const unknownMcp = `mpc_v1_${'deadbeef'.repeat(8)}`;
  const knownApiKey = 'sk-boundaryknown1234567890abcdef';
  const unknownApiKey = 'sk-boundaryunknown0987654321zyxw';
  const bearerHeader = `Bearer ${'AbCdEf0123456789'.repeat(3)}`;
  const secrets = [knownUnlock, knownMcp, knownApiKey];

  const cases = [
    { label: 'known-unlock', secret: knownUnlock, fragments: ['unlock-boundary', '99887766', '55443322'] },
    { label: 'known-mcp', secret: knownMcp, fragments: ['mpc_v1_', 'cafe9876'] },
    { label: 'unknown-mcp', secret: unknownMcp, fragments: ['mpc_v1_', 'deadbeef'] },
    { label: 'known-sk', secret: knownApiKey, fragments: ['sk-boundaryknown', '1234567890abcdef'] },
    { label: 'unknown-sk', secret: unknownApiKey, fragments: ['sk-boundaryunknown', '0987654321zyxw'] },
    { label: 'bearer-header', secret: bearerHeader, fragments: ['Bearer ', 'AbCdEf0123456789'] },
  ];

  for (const c of cases) {
    // Head preview split: raw position 28..40 only captures a 12-char prefix of the secret;
    // scrub-before-truncate must turn the full secret into [redacted] before capping at 40.
    const headInput = `${'x'.repeat(28)}${c.secret}${'y'.repeat(20)}`;
    const headOut = safePreview(headInput, secrets, 40, false);
    assert.ok(headOut.includes('[redacted]'), `${c.label} head should contain [redacted]: ${headOut}`);
    for (const frag of c.fragments) {
      assert.equal(headOut.includes(frag), false, `${c.label} head leaked fragment ${frag}: ${headOut}`);
    }

    // Tail preview split: raw last 40 chars start inside the secret (suffix + 26 'q's).
    const tailInput = `${'p'.repeat(25)}${c.secret}${'q'.repeat(26)}`;
    const tailOut = safePreview(tailInput, secrets, 40, true);
    assert.ok(tailOut.includes('[redacted]'), `${c.label} tail should contain [redacted]: ${tailOut}`);
    for (const frag of c.fragments) {
      assert.equal(tailOut.includes(frag), false, `${c.label} tail leaked fragment ${frag}: ${tailOut}`);
    }
  }

  // sanitizeBlocked applies scrub-before-truncate across head (280) and tail (240) preview boundaries.
  const blocked = sanitizeBlocked({
    kind: 'BLOCKED',
    detail: `${'d'.repeat(272)}${knownUnlock} tail`,
    reason: `${'r'.repeat(272)}${unknownApiKey} tail`,
    stderr: `head ${knownMcp}${'s'.repeat(232)}`,
    stdoutTail: `head ${bearerHeader}${'o'.repeat(230)}`,
  }, secrets);
  const serialized = JSON.stringify(blocked);
  assert.doesNotMatch(serialized, /unlock-boundary|99887766|55443322|sk-boundaryunknown|0987654321|mpc_v1_|cafe9876|Bearer |AbCdEf01/);

  // recording-proxy userAgent (capped at 160 chars) scrubs boundary-split secrets before truncating.
  const upstream = await fakeUpstream();
  const proxy = await startRecordingProxy({ targetPort: upstream.port, secrets });
  try {
    const boundaryUa = `${'agent/1.0 '.padEnd(154, 'a')}${knownUnlock}z`;
    await new Promise((resolve, reject) => {
      const req = http.request({
        host: '127.0.0.1',
        port: proxy.port,
        path: '/mcp',
        method: 'POST',
        headers: {
          'content-type': 'application/json',
          authorization: `Bearer ${TOKEN}`,
          'user-agent': boundaryUa,
        },
      }, (res) => { res.resume(); res.on('end', resolve); });
      req.on('error', reject);
      req.end(JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'tools/list', params: {} }));
    });
    const proxyDump = JSON.stringify({ received: proxy.received, exchanges: proxy.exchanges });
    assert.doesNotMatch(proxyDump, /unlock-boundary|99887766|55443322/);
  } finally {
    await proxy.close();
    await upstream.close();
  }
});

test('sourceIdentity binds scripts/mcp-stdio-bridge.mjs in harness provenance', () => {
  const id = sourceIdentity();
  assert.match(id.harness.conformance, /^[0-9a-f]{40}$/);
  assert.match(id.harness.lib, /^[0-9a-f]{40}$/);
  assert.match(id.harness.stdio_bridge, /^[0-9a-f]{40}$/);
});

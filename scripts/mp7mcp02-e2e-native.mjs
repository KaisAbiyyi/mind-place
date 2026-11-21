/**
 * MP-7MCP-02 end-to-end: the packaged Windows app, its real WebView2 UI2
 * Connections page, the real local MCP listener, and an HTTP MCP client.
 * Disposable archive and WebView2 profile; synthetic conversations written
 * through the real native-messaging host. No owner data, no real secrets: the
 * unlock secret and the access key live only in this process and are never
 * logged or written to the receipt.
 *
 * Run after `npm run dist:win`:  node scripts/mp7mcp02-e2e-native.mjs
 * Needs the MCP port (43731) free, so close any running Mind Place first.
 */
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import { spawn, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { chromium } from '@playwright/test';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const out = path.join(root, 'outputs', 'mp7mcp02-e2e');
const executable = path.join(root, 'release', 'win32-x64', 'mind-place.exe');
const host = path.join(root, 'release', 'win32-x64', 'mind-place-capture-host.exe');
const checks = [];
const pageErrors = [];
let app;
let browser;
let page;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const record = (gate, result, detail) => checks.push({ gate, result, detail });
async function gate(name, check) {
  try {
    record(name, 'PASS', (await check()) ?? 'Verified');
  } catch (error) {
    record(name, 'FAIL', String(error.message ?? error).slice(0, 400));
  }
}

function mcp(token, method, params = {}) {
  return new Promise((resolve, reject) => {
    const body = JSON.stringify({
      jsonrpc: '2.0', id: 1, method,
      params: { ...params, _meta: { 'io.modelcontextprotocol/protocolVersion': '2026-07-28', 'io.modelcontextprotocol/clientCapabilities': {} } },
    });
    const headers = {
      Host: '127.0.0.1:43731', 'Content-Type': 'application/json', Accept: 'application/json, text/event-stream',
      Authorization: `Bearer ${token}`, 'MCP-Protocol-Version': '2026-07-28', 'Mcp-Method': method,
      'Content-Length': Buffer.byteLength(body),
    };
    if (method === 'tools/call') headers['Mcp-Name'] = params.name;
    const request = http.request({ host: '127.0.0.1', port: 43731, path: '/mcp', method: 'POST', headers }, (response) => {
      let data = '';
      response.on('data', (chunk) => { data += chunk; });
      response.on('end', () => {
        try { resolve({ status: response.statusCode, json: data ? JSON.parse(data) : null }); } catch { resolve({ status: response.statusCode, json: null }); }
      });
    });
    request.setTimeout(8000, () => request.destroy(new Error('MCP request timed out')));
    request.on('error', reject);
    request.write(body);
    request.end();
  });
}
const search = async (token, text) => {
  const reply = await mcp(token, 'tools/call', { name: 'memory_search', arguments: { queries: [text] } });
  return { status: reply.status, hits: reply.json?.result?.structuredContent?.hits ?? [], reply };
};

async function connect(port) {
  const endpoint = `http://127.0.0.1:${port}`;
  let ok = false;
  for (let i = 0; i < 150 && !ok; i++) {
    if (app.exitCode !== null) throw new Error(`App exited early: ${app.exitCode}`);
    try { ok = (await fetch(`${endpoint}/json/version`, { signal: AbortSignal.timeout(600) })).ok; } catch { await sleep(200); }
  }
  assert.ok(ok, 'WebView2 CDP endpoint unavailable');
  browser = await chromium.connectOverCDP(endpoint);
  for (let i = 0; i < 80 && !page; i++) {
    page = browser.contexts().flatMap((c) => c.pages()).find((p) => p.url() !== 'about:blank');
    if (!page) await sleep(200);
  }
  assert.ok(page, 'App page unavailable');
  page.setDefaultTimeout(15000);
  page.on('pageerror', (error) => pageErrors.push(String(error.message).slice(0, 250)));
}

async function shot(name) {
  await page.screenshot({ path: path.join(out, name), fullPage: true });
}

try {
  assert.equal(process.platform, 'win32');
  fs.mkdirSync(out, { recursive: true });
  assert.ok(fs.statSync(executable).isFile() && fs.statSync(host).isFile(), 'Missing packaged app (run npm run dist:win)');
  const busy = await new Promise((resolve) => {
    const probe = http.request({ host: '127.0.0.1', port: 43731, method: 'GET', path: '/' }, () => resolve(true));
    probe.on('error', () => resolve(false));
    probe.end();
  });
  assert.equal(busy, false, 'MCP port 43731 is in use: close any running Mind Place first');

  // Short data root: capture paths are very long and must stay under MAX_PATH.
  const shortBase = process.env.MP_E2E_DIR ?? 'D:/mp-e2e';
  fs.mkdirSync(shortBase, { recursive: true });
  const fixture = spawnSync(process.execPath, [path.join(root, 'scripts', 'smoke-native.mjs'), host], {
    cwd: root, encoding: 'utf8', windowsHide: true, timeout: 60000,
    env: { ...process.env, MP_SMOKE_OUTPUT: shortBase },
  });
  const fixtureError = String(fixture.stderr).split('\n').find((line) => line.startsWith('Error')) ?? String(fixture.stderr).slice(-200);
  assert.equal(fixture.status, 0, `Fixture failed: ${fixtureError}`);
  const seeded = JSON.parse(fixture.stdout.trim().split(/\r?\n/).at(-1));
  const runtime = fs.mkdtempSync(path.join(shortBase, 'rt-'));
  fs.renameSync(seeded.archive_path, path.join(runtime, 'archive-v1'));
  const cdpPort = 49000 + crypto.randomInt(1000);
  app = spawn(executable, [], {
    cwd: path.dirname(executable), windowsHide: false, stdio: 'ignore',
    env: {
      ...process.env,
      MIND_PLACE_DATA_DIR: path.join(runtime, 'archive-v1'),
      WEBVIEW2_USER_DATA_FOLDER: path.join(runtime, 'webview'),
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-address=127.0.0.1 --remote-debugging-port=${cdpPort}`,
    },
  });
  await connect(cdpPort);

  // First run: create a disposable unlock secret that never leaves this process.
  const secret = crypto.randomBytes(24).toString('hex');
  await page.locator('#security-secret').waitFor({ timeout: 40000 });
  await page.locator('#security-secret').fill(secret);
  await page.locator('#security-secret-confirm').fill(secret);
  await page.locator('#security-submit').click();
  await page.locator('#knowledge-os-nav').waitFor({ timeout: 60000 });
  record('first-run-unlock', 'PASS', 'Disposable profile unlocked');

  // UI2 is an opt-in preview. Enable it for this disposable profile and reload;
  // the native process stays unlocked, so the UI2 shell mounts directly.
  await page.evaluate(() => localStorage.setItem('mind_place.ui2.shell.preview', 'enabled'));
  await page.reload();
  await page.getByRole('button', { name: 'Connections' }).first().waitFor({ timeout: 40000 });
  await page.getByRole('button', { name: 'Connections' }).first().click();
  await page.getByText('Let an AI app read your memory').first().waitFor();
  await shot('01-connections.png');
  record('ui2-connections-opens', 'PASS', 'Real WebView2 shows the Connections page');

  // Sharing on (starts the real local MCP listener).
  const sharing = page.getByRole('switch', { name: 'Share memory with AI apps' });
  await sharing.waitFor();
  if ((await sharing.getAttribute('aria-checked')) !== 'true') await sharing.click();
  await page.getByText('Sharing is on').first().waitFor();
  record('sharing-on', 'PASS', 'Switch started the local MCP listener');

  // Create access through the UI; read the one-time key only inside this process.
  await page.getByRole('button', { name: 'Create access' }).click();
  await page.locator('.ui2-tech-code').waitFor();
  const token = (await page.locator('.ui2-tech-code').first().innerText()).trim();
  assert.match(token, /^mpc_v1_[0-9a-f]{64}$/);
  await shot('02-key-shown-once.png');
  record('create-access-shows-key-once', 'PASS', 'Key displayed once in the UI (value not recorded)');
  await page.getByText('My AI app').first().waitFor();

  // Default deny, proven through the real HTTP listener.
  await gate('mcp-tools-list-works', async () => {
    const reply = await mcp(token, 'tools/list');
    assert.equal(reply.status, 200);
    assert.deepEqual(reply.json.result.tools.map((t) => t.name), ['memory_search', 'memory_fetch', 'context_build']);
    return 'Authenticated client lists the three read-only tools';
  });
  await gate('default-deny-before-any-grant', async () => {
    for (const word of ['Rust', 'offline']) {
      const result = await search(token, word);
      assert.equal(result.status, 200);
      assert.equal(result.hits.length, 0, `"${word}" must return nothing before a grant`);
    }
    return 'Both synthetic conversations are invisible until granted';
  });
  await page.getByText('Cannot read any chats yet').first().waitFor();

  // Grant one chat through the Choose chats dialog.
  await page.getByRole('button', { name: 'Choose chats' }).first().click();
  await page.getByText('Choose chats for My AI app').waitFor();
  await shot('03-choose-chats-dialog.png');
  await page.getByRole('switch', { name: 'Allow Rust archive decision' }).click();
  await page.getByRole('button', { name: 'Review changes' }).click();
  await page.getByText('Will be allowed: 1 chat').waitFor();
  await shot('04-review-step.png');
  await page.getByRole('button', { name: 'Approve and save' }).click();
  await page.getByText('Can read 1 chat').first().waitFor();
  record('grant-via-ui', 'PASS', 'Choose, Review, Approve saved one grant');

  await gate('granted-chat-visible-only', async () => {
    const granted = await search(token, 'Rust');
    assert.ok(granted.hits.length > 0, 'granted conversation must be searchable');
    assert.ok(granted.hits.every((h) => h.title === 'Rust archive decision'));
    // Words from the OTHER conversation (title and body). Any hit must belong to the granted one.
    const titles = new Set();
    for (const word of ['offline', 'reading', 'notes', 'export', 'Archive reading notes']) {
      for (const hit of (await search(token, word)).hits) titles.add(hit.title);
    }
    assert.ok(!titles.has('Archive reading notes'), 'ungranted conversation leaked: ' + [...titles].join(', '));
    return 'Only the granted conversation is returned; hit titles seen for the other conversation words: ' + ([...titles].join(', ') || 'none');
  });

  // Optional: a REAL Claude client (Claude Code CLI) as the MCP client, over the same
  // listener. The access key reaches it only through an environment variable that
  // the MCP config expands, never on a command line or in a file.
  if (process.env.MP_REAL_CLAUDE === '1') {
    await gate('real-claude-client-reads-only-granted', async () => {
      const claude = spawnSync('where.exe', ['claude'], { encoding: 'utf8' }).stdout.split(/\r?\n/).find((line) => /\.exe$/i.test(line.trim()))?.trim() ?? 'claude';
      const version = spawnSync(claude, ['--version'], { encoding: 'utf8' }).stdout.trim();
      const config = path.join(runtime, 'claude-mcp.json');
      fs.writeFileSync(config, JSON.stringify({
        mcpServers: { mindplace: { type: 'http', url: 'http://127.0.0.1:43731/mcp', headers: { Authorization: 'Bearer ${MP_E2E_TOKEN}' } } },
      }));
      const prompt = 'Call the mindplace memory_search tool twice, once with queries ["Rust"] and once with queries ["reading"]. Reply with one short line per call: the query and the number of hits. Do not call any other tool.';
      const run = spawnSync(claude, ['-p', '--mcp-config', config, '--strict-mcp-config',
        '--allowedTools', 'mcp__mindplace__memory_search', '--model', 'haiku', '--max-turns', '5',
        '--output-format', 'stream-json', '--verbose'], {
        input: prompt, encoding: 'utf8', timeout: 180000, maxBuffer: 8 * 1024 * 1024,
        env: { ...process.env, MP_E2E_TOKEN: token },
      });
      const events = String(run.stdout).split('\n').filter(Boolean).map((line) => { try { return JSON.parse(line); } catch { return null; } }).filter(Boolean);
      const init = events.find((e) => e.type === 'system' && e.subtype === 'init');
      assert.ok(init, 'Claude Code did not start: ' + String(run.stderr).slice(0, 200));
      const server = (init.mcp_servers ?? []).find((m) => m.name === 'mindplace');
      assert.equal(server?.status, 'connected', 'MCP server status: ' + server?.status);
      const toolNames = (init.tools ?? []).filter((t) => String(t).startsWith('mcp__mindplace__'));
      assert.ok(toolNames.includes('mcp__mindplace__memory_search'), 'tools discovered: ' + toolNames.join(','));
      const uses = [];
      const results = [];
      for (const event of events) {
        for (const block of event.message?.content ?? []) {
          if (block.type === 'tool_use') uses.push(block);
          if (block.type === 'tool_result') results.push(block);
        }
      }
      const searches = uses.filter((u) => u.name === 'mcp__mindplace__memory_search');
      const finalResult = events.find((e) => e.type === 'result');
      assert.ok(searches.length >= 2, 'Claude issued ' + searches.length + ' search calls; tool uses: ' + uses.map((u) => u.name).join(',') + '; denials: ' + JSON.stringify(finalResult?.permission_denials ?? []).slice(0, 200) + '; reply: ' + String(finalResult?.result).slice(0, 200));
      const summary = [];
      for (const result of results) {
        const text = Array.isArray(result.content) ? result.content.map((c) => c.text ?? '').join('') : String(result.content);
        let hits = [];
        try { hits = JSON.parse(text).hits ?? []; } catch { /* non-JSON result */ }
        summary.push(hits.length);
        assert.ok(hits.every((h) => h.title === 'Rust archive decision'), 'Claude received a hit from an ungranted chat');
      }
      assert.ok(summary.some((n) => n > 0), 'Claude received no hit for the granted chat');
      return `Claude Code ${version} (model alias haiku) connected over Streamable HTTP, discovered ${toolNames.length} mindplace tools, made ${searches.length} search calls; hit counts per result ${JSON.stringify(summary)}; every hit belonged to the granted chat`;
    });
  }

  await gate('ungranted-fetch-looks-unknown', async () => {
    const granted = (await search(token, 'Rust')).hits[0];
    const invoke = (command, args) => page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [command, args]);
    const listing = await invoke('list_local_mcp_grantable_chats', { clientId: await page.evaluate(() => null), query: null, offset: 0, limit: 25 }).catch(() => null);
    // The owner UI knows the opaque ids; use them as the guessable ids a hostile client might try.
    const clients = await invoke('list_local_mcp_clients', {});
    const clientId = clients.find((c) => c.display_name === 'My AI app').id;
    const rows = (await invoke('list_local_mcp_grantable_chats', { clientId, query: null, offset: 0, limit: 25 })).rows;
    const blocked = rows.find((r) => r.title === 'Archive reading notes');
    assert.ok(blocked && blocked.granted === false);
    void listing;
    const conversations = await invoke('list_conversations', { query: null });
    const blockedConversation = conversations.find((c) => c.title === 'Archive reading notes');
    const snapshots = await invoke('list_snapshots', { id: blockedConversation.id });
    const fetchArgs = (source, archive, snapshot) => ({ name: 'memory_fetch', arguments: { source_id: source, archive_id: archive, snapshot_id: snapshot, granularity: 'revision' } });
    const denied = await mcp(token, 'tools/call', fetchArgs(blocked.source_id, blockedConversation.id, snapshots[0].snapshot_id));
    const unknown = await mcp(token, 'tools/call', fetchArgs(`src:v1:${'0'.repeat(64)}`, blockedConversation.id, snapshots[0].snapshot_id));
    assert.equal(denied.status, unknown.status);
    assert.deepEqual(denied.json.error, unknown.json.error);
    const ok = await mcp(token, 'tools/call', fetchArgs(granted.source_id, granted.archive_id, granted.snapshot_id));
    assert.equal(ok.status, 200);
    return `Ungranted fetch and unknown-id fetch both answer ${denied.status} with the same error; granted fetch answers 200`;
  });

  // Revoke the grant through the UI; the next request sees nothing.
  await page.getByRole('button', { name: 'Choose chats' }).first().click();
  await page.getByRole('switch', { name: 'Allow Rust archive decision' }).click();
  await page.getByRole('button', { name: 'Review changes' }).click();
  await page.getByRole('button', { name: 'Approve and save' }).click();
  await page.getByText('Cannot read any chats yet').first().waitFor();
  await gate('revoked-grant-takes-effect-immediately', async () => {
    assert.equal((await search(token, 'Rust')).hits.length, 0);
    return 'Next request after revoking the chat returns nothing';
  });

  // Re-grant, then revoke the whole app through the inline two-step.
  await page.getByRole('button', { name: 'Choose chats' }).first().click();
  await page.getByRole('switch', { name: 'Allow Rust archive decision' }).click();
  await page.getByRole('button', { name: 'Review changes' }).click();
  await page.getByRole('button', { name: 'Approve and save' }).click();
  await page.getByText('Can read 1 chat').first().waitFor();
  assert.ok((await search(token, 'Rust')).hits.length > 0);
  await page.getByRole('button', { name: 'Revoke access' }).first().click();
  await shot('05-revoke-inline-confirm.png');
  await page.locator('.ui2-inline-confirm-box').getByRole('button', { name: 'Revoke access' }).click();
  await page.getByText(/Revoked apps \(1\)/).waitFor();
  await gate('revoked-app-is-denied', async () => {
    const reply = await mcp(token, 'tools/list');
    assert.ok([401, 403].includes(reply.status), `expected 401/403, got ${reply.status}`);
    return `Revoked key answers ${reply.status}`;
  });

  // Lock closes the listener and clears the page.
  await page.getByRole('button', { name: 'Lock' }).first().click();
  await sleep(1500);
  await gate('lock-closes-mcp', async () => {
    let status = 'closed';
    try { status = String((await mcp(token, 'tools/list')).status); } catch { status = 'closed'; }
    assert.ok(['closed', '401', '403'].includes(status), `after Lock the listener answered ${status}`);
    return `After Lock the listener is ${status}`;
  });
  record('page-errors', pageErrors.length === 0 ? 'PASS' : 'FAIL', pageErrors.length === 0 ? 'No uncaught page errors' : pageErrors.join(' | '));
} catch (error) {
  record('harness', 'FAIL', String(error.message ?? error).slice(0, 500));
  try { if (page) await shot('failure.png'); } catch { /* ignore */ }
} finally {
  await browser?.close().catch(() => {});
  if (app?.exitCode === null) app.kill();
  const receipt = {
    kind: 'packaged app + real WebView2 UI2 Connections page + real local MCP listener + HTTP MCP client (disposable synthetic data)',
    executable_sha256: crypto.createHash('sha256').update(fs.readFileSync(executable)).digest('hex'),
    checks,
    verdict: checks.every((c) => c.result === 'PASS') ? 'PASS' : 'FAIL',
  };
  fs.writeFileSync(path.join(out, 'receipt.json'), JSON.stringify(receipt, null, 2) + '\n');
  console.log(JSON.stringify({ verdict: receipt.verdict, checks: checks.map((c) => `${c.result} ${c.gate}${c.result === 'FAIL' ? ' :: ' + c.detail : ''}`) }, null, 2));
  process.exitCode = receipt.verdict === 'PASS' ? 0 : 1;
}

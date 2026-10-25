/**
 * MP-8L2 Windows qualification. Exercise an actual packaged Tauri/WebView2
 * executable with synthetic saves made through its packaged capture host.
 * Runs against an isolated security/archive/profile root; never copies or
 * touches the owner's archive. Output contains metadata and timings only.
 *
 * Usage (from a checked out source tree):
 *   $env:MP8L2_PACKAGE_ROOT = 'D:\\projects\\mind-place-mp8l1-final-qualification-20261025'
 *   node scripts/mp8l2-native-performance.mjs --conversations 800
 */
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { performance } from 'node:perf_hooks';
import { chromium } from '@playwright/test';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const sourcePackageRoot = path.resolve(process.env.MP8L2_PACKAGE_ROOT || root);
const executable = path.join(sourcePackageRoot, 'release', 'win32-x64', 'mind-place.exe');
const host = path.join(sourcePackageRoot, 'release', 'win32-x64', 'mind-place-capture-host.exe');
const output = path.join(root, 'outputs', 'mp-8l2');
const args = process.argv.slice(2);
const option = name => args.includes(name) ? args[args.indexOf(name) + 1] : undefined;
const count = Number(option('--conversations') ?? '800');
const runId = new Date().toISOString().replace(/[:.]/g, '-');
const runDir = path.join(output, runId);
// Keep native fixture paths short: capture filenames include two 64-char hashes,
// and Windows still has tooling that fails around MAX_PATH on long worktree paths.
const archiveParent = path.join(path.dirname(root), 'mp8l2-native-runs', runId);
const archive = path.join(archiveParent, 'archive-v1');
const profile = path.join(archiveParent, 'webview-profile');
const checks = [];
const samples = [];
const pageErrors = [];
let app, browser, page;
let secret;
const sleep = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds));
const sha256 = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const git = directory => {
  const result = spawnSync('git', ['rev-parse', 'HEAD'], { cwd: directory, encoding: 'utf8', windowsHide: true });
  return result.status === 0 ? result.stdout.trim() : null;
};
const percentile = (values, proportion) => {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.floor((sorted.length - 1) * proportion))] ?? null;
};
const gate = (name, verdict, detail = '') => checks.push({ name, verdict, detail });
const timed = async (name, fn) => {
  const start = performance.now();
  const result = await fn();
  const ms = Math.round((performance.now() - start) * 10) / 10;
  samples.push({ name, ms });
  return result;
};

// A process tree sample includes the main process and its WebView2 children.
// Windows working sets are snapshots, not allocations or a leak proof.
function sampleMemory(label) {
  const ps = 'Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,Name,WorkingSetSize | ConvertTo-Json -Compress';
  const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', ps],
    { encoding: 'utf8', timeout: 20_000, windowsHide: true, maxBuffer: 3 * 1024 * 1024 });
  if (result.status !== 0) throw Error('Windows process-tree memory telemetry failed');
  const all = JSON.parse(result.stdout.trim());
  const processes = Array.isArray(all) ? all : [all];
  const owned = new Set([app.pid]);
  for (let iteration = 0; iteration < 8; iteration++) {
    for (const proc of processes) if (owned.has(proc.ParentProcessId)) owned.add(proc.ProcessId);
  }
  const matches = processes.filter(proc => owned.has(proc.ProcessId));
  const bytes = matches.reduce((sum, proc) => sum + Number(proc.WorkingSetSize ?? 0), 0);
  const observation = { name: label, workingSetMiB: Math.round(bytes / 1048576 * 10) / 10,
    processes: matches.length, mainPid: app.pid };
  samples.push(observation);
  return observation;
}

// Native Messaging is a length-prefixed message to the genuine capture host.
// A fresh host is launched for every save; no production archive is accessed.
function saveCapture(capture) {
  const message = Buffer.from(JSON.stringify({ protocol: 1, operation: 'save', capture }));
  const prefix = Buffer.alloc(4);
  prefix.writeUInt32LE(message.length);
  const received = spawnSync(host, ['chrome-extension://dninfeidggaaioioficnigfpfjkdicjh/'], {
    env: { ...process.env, MIND_PLACE_DATA_DIR: archive }, cwd: path.dirname(host),
    windowsHide: true, input: Buffer.concat([prefix, message]), timeout: 20_000,
    maxBuffer: 1024 * 1024,
  });
  if (received.status !== 0 || received.stdout.length < 4) {
    throw Error('Synthetic native save failed (host/process)');
  }
  assert.equal(received.stdout.readUInt32LE(0), received.stdout.length - 4);
  const reply = JSON.parse(received.stdout.subarray(4).toString('utf8'));
  if (!reply.ok) throw Error(`Synthetic native save rejected: ${String(reply.error ?? reply.code ?? 'unknown').slice(0, 140)}`);
  return reply;
}

async function connect(port) {
  const endpoint = `http://127.0.0.1:${port}`;
  let ready = false;
  for (let i = 0; i < 300; i++) {
    if (app.exitCode !== null) throw Error('Packaged Windows process exited during startup');
    try {
      if ((await fetch(`${endpoint}/json/version`, { signal: AbortSignal.timeout(500) })).ok) {
        ready = true; break;
      }
    } catch { /* WebView2 is still starting */ }
    await sleep(200);
  }
  assert.ok(ready, 'Native WebView2 debugger not available');
  browser = await chromium.connectOverCDP(endpoint);
  for (let i = 0; i < 100; i++) {
    page = browser.contexts().flatMap(context => context.pages())
      .find(candidate => !['', 'about:blank'].includes(candidate.url()));
    if (page) break;
    await sleep(200);
  }
  assert.ok(page, 'Native page missing');
  page.setDefaultTimeout(30_000);
  page.on('pageerror', error => pageErrors.push(String(error.message).slice(0, 150)));
  await page.locator('#security-form').waitFor({ timeout: 45_000 });
}

async function closeApp() {
  await browser?.close().catch(() => {});
  if (app?.exitCode === null) {
    app.kill();
    for (let i = 0; i < 30 && app.exitCode === null; i++) await sleep(100);
  }
}

try {
  assert.equal(process.platform, 'win32', 'Native Windows qualification only');
  assert.ok(Number.isSafeInteger(count) && count >= 200 && count <= 10_000, 'Invalid corpus size');
  assert.equal(path.parse(root).root.toLowerCase(), 'd:\\', 'Qualification worktree must reside on D:');
  assert.ok(fs.existsSync(executable) && fs.existsSync(host), 'Packaged EXE/host not found');
  const packageSha = git(sourcePackageRoot);
  assert.equal(packageSha, 'bfff35463880533e25866efae7e4f14e004cafd6',
    'Packaged build source must be verified baseline main SHA');
  fs.mkdirSync(runDir, { recursive: true });
  fs.mkdirSync(archive, { recursive: true });
  fs.mkdirSync(profile, { recursive: true });

  const seedArchive = path.join(sourcePackageRoot, 'outputs');
  const candidate = fs.readdirSync(seedArchive).find(name => name.startsWith('native-smoke-'));
  assert.ok(candidate, 'An isolated synthetic smoke-native source fixture is required');
  const sourceConversations = path.join(seedArchive, candidate, 'conversations');
  const seed = fs.readdirSync(sourceConversations)[0];
  const seedCaptureDir = path.join(sourceConversations, seed, 'captures');
  const seedCapture = JSON.parse(fs.readFileSync(path.join(seedCaptureDir, fs.readdirSync(seedCaptureDir)[0]), 'utf8'));
  assert.equal(seedCapture.provider, 'chatgpt');
  const createStart = performance.now();
  let seedArchiveId, graphCapture;
  for (let i = 0; i < count; i++) {
    const id = crypto.randomUUID();
    const capture = structuredClone(seedCapture);
    capture.conversation_id = id;
    capture.source_url = `https://chatgpt.com/c/${id}`;
    capture.title = `Synthetic library corpus item ${String(i).padStart(5, '0')}`;
    capture.messages = capture.messages.map((message, index) => ({
      ...message, id: `synthetic-msg-${index}-${i}`,
      text: `Synthetic archive retrieval benchmark ${i}. ${index === 0 ? 'Request' : 'Reply'} about native performance and source provenance.`,
      attachments: [],
    }));
    const saved = saveCapture(capture);
    if (i === 0) { seedArchiveId = saved.id; graphCapture = structuredClone(capture); }
    if ((i + 1) % 100 === 0) process.stdout.write(`Fixture: ${i + 1}/${count}\n`);
  }
  gate('packaged-native-capture', 'PASS', `${count} unique synthetic archive saves through packaged native host`);
  assert.ok(seedArchiveId, 'No seed archived');
  // A single real archive with 131 immutable snapshots yields a connected Hive
  // neighborhood above the actual 120-node/240-edge retrieval limits. It is
  // stored through the same packaged host and indexed by the real Tauri app.
  const extraRevisions = 130;
  for (let i = 0; i < extraRevisions; i++) {
    const revision = structuredClone(graphCapture);
    revision.messages[0].text += ` Snapshot revision ${i}.`;
    const saved = saveCapture(revision);
    assert.equal(saved.id, seedArchiveId);
  }
  gate('real-native-graph-fixture', 'PASS', `${extraRevisions + 1} immutable revisions on one synthetic archive`);
  samples.push({ name: 'fixture-generation', ms: Math.round(performance.now() - createStart) });

  const port = 51000 + crypto.randomInt(1000);
  app = spawn(executable, [], {
    cwd: path.dirname(executable), windowsHide: true, stdio: 'ignore',
    env: { ...process.env, MIND_PLACE_DATA_DIR: archive,
      WEBVIEW2_USER_DATA_FOLDER: profile,
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-address=127.0.0.1 --remote-debugging-port=${port}` },
  });
  await timed('webview2-cold-locked', () => connect(port));
  assert.equal(await page.locator('#security-title').innerText(), 'Protect Mind Place');
  gate('locked-at-startup', 'PASS');
  secret = crypto.randomBytes(24).toString('hex');
  await page.locator('#security-secret').fill(secret);
  await page.locator('#security-secret-confirm').fill(secret);
  await timed('unlock-large-corpus', async () => {
    await page.locator('#security-submit').click();
    await page.locator('#conversation-list.is-virtualized').waitFor({ timeout: 120_000 });
  });
  const rowCount = await page.locator('#conversation-list [data-conversation-id]').count();
  const first = await page.locator('#conversation-list [data-conversation-id]').first().getAttribute('aria-setsize');
  assert.equal(Number(first), count, 'Virtual list must expose full synthetic corpus');
  assert.ok(rowCount < 150, 'Virtualized DOM row budget exceeded');
  gate('native-large-corpus-virtualization', 'PASS', `${count} entries, ${rowCount} rendered rows`);
  sampleMemory('after-native-unlock');

  for (let iteration = 0; iteration < 20; iteration++) {
    await timed('virtual-scroll', async () => {
      const info = await page.locator('#conversation-list').evaluate((element, i) => {
        const ratio = (i % 2 === 0) ? 0.93 : 0.07;
        element.scrollTop = (element.scrollHeight - element.clientHeight) * ratio;
        element.dispatchEvent(new Event('scroll', { bubbles: true }));
        return { max: element.scrollHeight - element.clientHeight };
      }, iteration);
      assert.ok(info.max > 0, 'Virtualized list must scroll');
      await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
      const rendered = await page.locator('#conversation-list [data-conversation-id]').count();
      assert.ok(rendered > 0 && rendered < 150, 'Virtual DOM row count unbounded');
    });
  }
  gate('native-scroll-window', 'PASS', '20 alternating distant scrolls, bounded DOM row count');
  sampleMemory('after-scroll');

  for (let iteration = 0; iteration < 12; iteration++) {
    await timed('native-workspace-switch', async () => {
      const workspace = ['conversations', 'library', 'graph', 'health'][iteration % 4];
      await page.locator(`[data-workspace="${workspace}"]`).click();
      assert.equal(await page.locator('#knowledge-os-nav [aria-current="page"]').getAttribute('data-workspace'), workspace);
    });
  }
  gate('native-navigation', 'PASS', '12 switches among 4 workspaces');
  sampleMemory('after-navigation');

  // Archive listing is independent of the derived index. Explicitly complete
  // native FTS/graph synchronization before claiming a native Hive result.
  const indexStats = await timed('native-index-synchronization', () => page.evaluate(async () =>
    window.__TAURI_INTERNALS__.invoke('refresh_archive_index')));
  assert.ok(indexStats.sources >= count, 'Index did not include entire test corpus');
  assert.ok(indexStats.revisions >= count + extraRevisions,
    'Index did not include all synthetic graph revisions');
  gate('native-derived-index-ready', 'PASS',
    `${indexStats.sources} sources / ${indexStats.revisions} revisions / ${indexStats.chunks} chunks`);

  await page.locator('[data-workspace="graph"]').click();
  // The archive receipt is the graph Import node's canonical key. Conversation
  // nodes instead use provider-neutral source IDs from the authority store.
  await page.locator('.hive-search-form select').selectOption('import');
  await page.locator('.hive-search-form input').fill(seedArchiveId);
  await timed('native-index-graph-neighbors', async () => {
    await page.locator('.hive-search-form button[type="submit"]').click();
    await page.waitForFunction(() => {
      const status = document.querySelector('.hive-workspace [role="status"]')?.textContent ?? '';
      return /nodes|unavailable/i.test(status) && !/Loading/i.test(status);
    }, null, { timeout: 45_000 });
  });
  const nativeGraphStatus = await page.locator('.hive-workspace [role="status"]').first().innerText();
  assert.match(nativeGraphStatus, /\d+ nodes/, 'Native archive graph query must succeed');
  gate('native-hive-query', 'PASS', nativeGraphStatus.slice(0, 100));

  await timed('bounded-native-hive-rerender', async () => {
    await page.locator('.hive-search-form button[type="submit"]').click();
    await page.waitForFunction(() => document.querySelectorAll('.hive-node-choice').length >= 100);
  });
  const svgNodes = await page.locator('.hive-map circle').count();
  const svgEdges = await page.locator('.hive-map line').count();
  assert.ok(svgNodes >= 100 && svgNodes <= 120, 'Native Hive node count out of expected bounds');
  assert.ok(svgEdges >= 100 && svgEdges <= 240, 'Native Hive edge count out of expected bounds');
  for (const value of [60, 200, 80, 160, 100, 200, 60, 100]) {
    await timed('bounded-hive-semantic-zoom', () => page.locator('.hive-controls input[type="range"]').first()
      .evaluate((el, setting) => {
        el.value = String(setting);
        el.dispatchEvent(new Event('input', { bubbles: true }));
      }, value));
  }
  for (const revision of [1, 4, 8, 0, 2, 0]) {
    await timed('bounded-hive-revision-filter', () => page.locator('.hive-controls input[type="range"]').nth(1)
      .evaluate((el, setting) => {
        el.value = String(setting);
        el.dispatchEvent(new Event('input', { bubbles: true }));
      }, revision));
  }
  gate('native-webview2-bounded-hive-ui', 'PASS', `${svgNodes} nodes / ${svgEdges} edges from real native graph, zoom and revision UI`);
  sampleMemory('after-bounded-hive');

  for (let iteration = 0; iteration < 10; iteration++) {
    await page.locator('[data-workspace="health"]').click();
    await page.locator('.health-inspection-form input').fill('benchmark');
    await timed('native-retrieval-inspector', async () => {
      await page.locator('.health-inspection-form button[type="submit"]').click();
      await page.waitForFunction(() => {
        const result = document.querySelector('.health-inspection-result')?.textContent ?? '';
        return result.length > 0 && !/Inspecting/i.test(result);
      }, null, { timeout: 30_000 });
    });
  }
  gate('native-repeated-retrieval-inspector', 'PASS', '10 native health/retrieval inspector runs');
  sampleMemory('after-retrieval-inspector');
  assert.equal(pageErrors.length, 0, 'Unexpected native WebView2 JavaScript exceptions');
  gate('javascript-errors', 'PASS', 'None');

  const metric = name => {
    const durations = samples.filter(s => s.name === name).map(s => s.ms);
    return { count: durations.length, medianMs: percentile(durations, 0.5), p95Ms: percentile(durations, 0.95), maxMs: Math.max(...durations) };
  };
  const mem = samples.filter(s => s.workingSetMiB !== undefined);
  const growthMiB = Math.round((mem.at(-1).workingSetMiB - mem[0].workingSetMiB) * 10) / 10;
  // No arbitrary threshold pass: the owner/reviewer must determine acceptable budgets
  // on representative hardware, workload and load after collecting repeat measurements.
  const verdict = 'PARTIAL';
  const receipt = {
    task: 'MP-8L2', verdict, native_windows: true, synthetic_corpus: true,
    measured_at: new Date().toISOString(), host: { os: os.release(), arch: process.arch,
      cpus: os.cpus().length, totalRamGiB: Math.round(os.totalmem() / 1073741824) },
    package_source_sha: packageSha, runner_source_sha: git(root),
    executable_sha256: sha256(executable), host_sha256: sha256(host),
    conversations: count, graph: { native_status: nativeGraphStatus,
      native_nodes: svgNodes, native_edges: svgEdges, archived_revisions: extraRevisions + 1 },
    timings: Object.fromEntries(['webview2-cold-locked', 'unlock-large-corpus',
      'virtual-scroll', 'native-workspace-switch', 'native-index-synchronization', 'native-index-graph-neighbors',
      'bounded-native-hive-rerender', 'bounded-hive-semantic-zoom',
      'bounded-hive-revision-filter', 'native-retrieval-inspector'].map(name => [name, metric(name)])),
    memory: { snapshots: mem.map(({ name, workingSetMiB, processes }) => ({ name, workingSetMiB, processes })),
      firstToLastMiB: growthMiB },
    checks, pageErrors, limitations: [
      'One instrumented run; repeat with baseline idle and loaded host to assess variance',
      'Fixture is synthetic; no private owner corpus was accessed',
      'Hive nodes/edges are genuine derived index responses to synthetic native-host revisions',
      'WebView2 process-tree working set includes browser subprocesses and shared pages',
      'No fixed product memory/latency budgets have been accepted; growth not automatically a leak',
    ],
  };
  fs.writeFileSync(path.join(runDir, 'receipt.json'), JSON.stringify(receipt, null, 2) + '\n');
  console.log(`MP-8L2 local Windows native measurements collected; verdict ${verdict}; receipt ${path.join(runDir, 'receipt.json')}`);
} catch (error) {
  gate('runner', 'FAIL', String(error.message ?? error).slice(0, 220));
  fs.mkdirSync(runDir, { recursive: true });
  fs.writeFileSync(path.join(runDir, 'failure.json'), JSON.stringify({
    task: 'MP-8L2', verdict: 'FAIL', checks, pageErrors, package_source_sha: git(sourcePackageRoot),
    error: String(error.message ?? error).slice(0, 250),
  }, null, 2) + '\n');
  console.error('MP-8L2 FAIL:', error);
  process.exitCode = 1;
} finally {
  secret = undefined;
  await closeApp();
}

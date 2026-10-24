/**
 * MP-8L1: actual packaged Windows WebView2 smoke, with a disposable native-host
 * fixture and an isolated application data root. No owner corpus or passwords.
 *
 * Run after `npm run dist:win` on Windows. Uses CDP to inspect the WebView2
 * instance owned by the packaged Tauri executable, not a Chromium test browser.
 * CDP instrumentation makes startup timings indicative, not a release benchmark.
 */
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { spawn, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { performance } from 'node:perf_hooks';
import { chromium } from '@playwright/test';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const out = path.join(root, 'outputs', 'mp-8l1');
const executable = path.join(root, 'release', 'win32-x64', 'mind-place.exe');
const host = path.join(root, 'release', 'win32-x64', 'mind-place-capture-host.exe');
const receiptPath = path.join(out, 'native-webview2-receipt.json');
const checks = [];
const pageErrors = [];
let app;
let browser;
let page;
let syntheticSecret;

function sha(file) { return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex'); }
function record(gate, result, detail) { checks.push({ gate, result, detail }); }
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

async function run(gate, check) {
  try { const detail = await check(); record(gate, 'PASS', detail ?? 'Verified'); }
  catch (error) { record(gate, 'FAIL', String(error.message ?? error).slice(0, 350)); }
}

async function connectToNativeWebView(port) {
  const endpoint = `http://127.0.0.1:${port}`;
  let available = false;
  for (let attempt = 0; attempt < 130; attempt++) {
    if (app.exitCode !== null) throw new Error(`Packaged app exited before WebView2 connected: ${app.exitCode}`);
    try { const result = await fetch(`${endpoint}/json/version`, { signal: AbortSignal.timeout(600) });
      if (result.ok) { available = true; break; }
    } catch { /* native WebView2 is still starting */ }
    await sleep(200);
  }
  assert.ok(available, 'Native WebView2 CDP endpoint was unavailable');
  browser = await chromium.connectOverCDP(endpoint);
  for (let attempt = 0; attempt < 70; attempt++) {
    page = browser.contexts().flatMap(context => context.pages()).find(candidate => candidate.url() !== 'about:blank');
    if (page) break;
    await sleep(200);
  }
  assert.ok(page, 'Packaged WebView2 application page was unavailable');
  page.setDefaultTimeout(8_000);
  page.on('pageerror', error => pageErrors.push(String(error.message).slice(0, 250)));
  await page.locator('#security-form').waitFor({ timeout: 30_000 });
}

function launch(archiveRoot, profile, port) {
  const env = {
    ...process.env,
    MIND_PLACE_DATA_DIR: archiveRoot,
    WEBVIEW2_USER_DATA_FOLDER: profile,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-address=127.0.0.1 --remote-debugging-port=${port}`,
  };
  app = spawn(executable, [], { cwd: path.dirname(executable), env, windowsHide: false, stdio: 'ignore' });
  return app.pid;
}

async function closeApp() {
  await browser?.close().catch(() => {});
  browser = undefined; page = undefined;
  if (app?.exitCode === null) {
    app.kill();
    for (let i = 0; i < 30 && app.exitCode === null; i++) await sleep(100);
  }
  app = undefined;
}

try {
  assert.equal(process.platform, 'win32', 'This test only qualifies native Windows WebView2');
  fs.mkdirSync(out, { recursive: true });
  assert.ok(fs.statSync(executable).isFile(), 'Missing packaged Mind Place executable');
  assert.ok(fs.statSync(host).isFile(), 'Missing packaged capture host');

  // Existing production fixture saves two synthetic conversations and multiple
  // immutable revisions through the real packaged native-messaging host.
  const fixture = spawnSync(process.execPath, [path.join(root, 'scripts', 'smoke-native.mjs'), host], {
    cwd: root, encoding: 'utf8', windowsHide: true, timeout: 30_000, maxBuffer: 1024 * 1024,
  });
  assert.equal(fixture.status, 0, `Fixture failed: ${fixture.stderr.slice(-400)}`);
  const fixtureReceipt = JSON.parse(fixture.stdout.trim().split(/\r?\n/).at(-1));
  assert.equal(fixtureReceipt.conversations, 2);
  const fixtureArchive = fixtureReceipt.archive_path;
  assert.ok(path.isAbsolute(fixtureArchive) && fixtureArchive.startsWith(path.join(root, 'outputs')),
    'Synthetic data root must remain beneath the isolated D: qualification worktree');
  // Security and authoritative/index SQLite state live in the archive's
  // parent directory. Isolate that parent too, to avoid sharing test secrets
  // across smoke runs or with any existing Mind Place process.
  const testParent = fs.mkdtempSync(path.join(out, 'isolated-runtime-'));
  const archive = path.join(testParent, 'archive-v1');
  fs.renameSync(fixtureArchive, archive);
  const profile = path.join(testParent, 'webview-profile');
  const basePort = 49000 + crypto.randomInt(1000);
  const start = performance.now();

  const pid = launch(archive, profile, basePort);
  await connectToNativeWebView(basePort);
  const firstPaintMs = Math.round(performance.now() - start);
  const heading = await page.locator('#security-title').innerText();
  assert.equal(heading, 'Protect Mind Place', 'Isolated test archive must be unconfigured');
  record('packaged-native-startup', 'PASS', `Native WebView2 reached first-run gate in ${firstPaintMs} ms (instrumented); PID ${pid}`);
  record('locked-private-data', 'PASS', 'Navigation and archive content unavailable before first-run authorization');
  assert.equal(await page.locator('#knowledge-os-nav').count(), 0);
  await page.screenshot({ path: path.join(out, 'native-first-run.png') });

  // Test-only randomly generated secret lives in this process, never in argv,
  // logs, receipts, GitHub, or fixture output.
  syntheticSecret = crypto.randomBytes(24).toString('hex');
  await page.locator('#security-secret').fill(syntheticSecret);
  await page.locator('#security-secret-confirm').fill(syntheticSecret);
  await page.locator('#security-submit').click();
  await page.locator('#knowledge-os-nav').waitFor({ timeout: 60_000 });
  await page.locator('#conversation-list').waitFor();
  record('native-initialize-unlock', 'PASS', 'First-run secret creation unlocked the native reader');

  for (const workspace of ['conversations','library','memories','documents','projects','people','topics','graph','health']) {
    await run(`navigate-${workspace}`, async () => {
      await page.locator(`[data-workspace="${workspace}"]`).click();
      await page.waitForTimeout(120);
      assert.equal(await page.locator(`#knowledge-os-nav [aria-current="page"]`).count(), 1);
      assert.equal(await page.locator(`#knowledge-os-nav [aria-current="page"]`).getAttribute('data-workspace'), workspace);
      // Documents opens the dedicated Settings/Documents modal while retaining
      // the document workspace selection. Close it before visiting other tabs.
      if (workspace === 'documents') {
        assert.equal(await page.locator('#setup-dialog').evaluate(element => element.open), true);
        await page.keyboard.press('Escape');
        assert.equal(await page.locator('#setup-dialog').evaluate(element => element.open), false);
      }
      const content = page.locator('#reader-view');
      assert.ok((await content.innerText())?.trim(), 'Workspace has visible content');
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth + 1);
      assert.equal(overflow, false, 'Unexpected horizontal viewport overflow');
      return 'Single selected navigation item, visible content and no horizontal overflow';
    });
  }

  await run('native-webview2-narrow-viewport', async () => {
    await page.setViewportSize({ width: 390, height: 700 });
    for (const workspace of ['library', 'memories', 'graph', 'health']) {
      await page.locator(`[data-workspace="${workspace}"]`).click();
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth + 1);
      assert.equal(overflow, false, `390px viewport overflow in ${workspace}`);
    }
    await page.screenshot({ path: path.join(out, 'native-webview2-narrow-emulated.png') });
    await page.setViewportSize({ width: 1180, height: 820 });
    return '390x700 CDP-emulated viewport on native WebView2, four workspaces';
  });

  await run('keyboard-navigation', async () => {
    const first = page.locator('[data-workspace="conversations"]');
    await first.focus();
    await page.keyboard.press('ArrowRight');
    const target = await page.evaluate(() => document.activeElement?.getAttribute('data-workspace'));
    assert.equal(target, 'library');
    return 'ArrowRight moves visible focus to the next workspace';
  });
  await run('settings-escape-focus', async () => {
    const trigger = page.locator('[data-workspace="connections"]');
    await trigger.click();
    assert.equal(await page.locator('#setup-dialog').evaluate(element => element.open), true);
    await page.keyboard.press('Escape');
    assert.equal(await page.locator('#setup-dialog').evaluate(element => element.open), false);
    assert.equal(await trigger.evaluate(element => element === document.activeElement), true);
    return 'Escape closes Settings and restores focus to its trigger';
  });
  await run('native-index-title-source-preview', async () => {
    await page.locator('[data-workspace="library"]').click();
    await page.locator('#library-query').fill('Rust');
    await page.locator('#library-query').press('Enter');
    const hit = page.locator('.knowledge-library-results [data-library-archive]').first();
    await hit.waitFor({ timeout: 15_000 });
    assert.match(await hit.innerText(), /Rust archive decision/);
    await hit.click();
    const preview = page.locator('.knowledge-library-preview');
    await page.waitForFunction(() => {
      const content = document.querySelector('.knowledge-library-preview')?.textContent ?? '';
      return content !== '' && !content.includes('Checking selected snapshot') &&
        !content.includes('Choose a match to preview');
    }, null, { timeout: 15_000 });
    await page.screenshot({ path: path.join(out, 'native-library-preview.png') });
    const previewText = await preview.innerText();
    if (!/Archive ID:.*Snapshot:/s.test(previewText)) {
      const diagnostic = await page.evaluate(async () => {
        const invoke = window.__TAURI_INTERNALS__?.invoke;
        if (typeof invoke !== 'function') return { error: 'Native invoke unavailable' };
        try {
          const hits = await invoke('search_archive_index', { query: 'Rust', includeHistory: true, limit: 40 });
          const hit = hits.find(item => item.title === 'Rust archive decision');
          if (!hit) return { error: 'Native hit missing' };
          try {
            const conversation = await invoke('read_conversation', { id: hit.archive_id, snapshotId: hit.snapshot_id });
            return {
              hit_kind: hit.kind,
              summary_matches: conversation.summary?.id === hit.archive_id,
              snapshot_matches: conversation.snapshot_id === hit.snapshot_id,
              message_matches: conversation.capture?.messages?.some(item => item.id === hit.message_id),
              conversation_keys: Object.keys(conversation).slice(0, 8),
            };
          } catch (error) { return { error_code: String(error).slice(0, 120) }; }
        } catch (error) { return { search_error: String(error).slice(0, 120) }; }
      });
      throw new Error(`Native preview: ${previewText.slice(0, 100)}; ${JSON.stringify(diagnostic)}`);
    }
    return 'Real native FTS5 title match has an inspectable original source';
  });
  await run('native-index-message-source-preview', async () => {
    await page.locator('#library-query').fill('Tauri');
    await page.locator('#library-query').press('Enter');
    const hit = page.locator('.knowledge-library-results [data-library-archive]').first();
    await hit.waitFor({ timeout: 15_000 });
    await hit.click();
    await page.waitForFunction(() => {
      const content = document.querySelector('.knowledge-library-preview')?.textContent ?? '';
      return content !== '' && !content.includes('Checking selected snapshot') &&
        !content.includes('Choose a match to preview');
    }, null, { timeout: 15_000 });
    const previewText = await page.locator('.knowledge-library-preview').innerText();
    assert.match(previewText, /Archive ID:.*Snapshot:/s, `Message preview: ${previewText.slice(0, 120)}`);
    return 'FTS5 message match resolves the canonical snapshot and original message';
  });
  await run('native-conversation-search', async () => {
    await page.locator('[data-workspace="conversations"]').click();
    const input = page.locator('#archive-search');
    await input.fill('Rust archive');
    await page.getByText('Rust archive decision', { exact: true }).first().waitFor({ timeout: 12_000 });
    assert.equal(await page.locator('#conversation-list').getByText('Archive reading notes').count(), 0);
    await input.fill('');
    return 'Conversation list filters the native fixture without exposing nonmatches';
  });
  await run('native-archive-list', async () => {
    await page.locator('[data-workspace="conversations"]').click();
    await page.getByText('Rust archive decision', { exact: false }).first().waitFor({ timeout: 15_000 });
    return 'Synthetic immutable native-host capture appeared in actual native reader';
  });
  await page.screenshot({ path: path.join(out, 'native-reader.png') });

  await closeApp();
  const secondStart = performance.now();
  launch(archive, path.join(testParent, 'webview-restart-profile'), basePort + 1);
  await connectToNativeWebView(basePort + 1);
  const restartMs = Math.round(performance.now() - secondStart);
  await run('locked-on-restart', async () => {
    assert.equal(await page.locator('#security-title').innerText(), 'Unlock Mind Place');
    assert.equal(await page.locator('#knowledge-os-nav').count(), 0);
    return `Restarted locked in ${restartMs} ms (instrumented)`;
  });
  await run('restart-unlock-persistence', async () => {
    await page.locator('#security-secret').fill(syntheticSecret);
    await page.locator('#security-submit').click();
    await page.locator('#knowledge-os-nav').waitFor({ timeout: 60_000 });
    await page.getByText('Rust archive decision', { exact: false }).first().waitFor({ timeout: 15_000 });
    return 'Security settings and synthetic archive survived native process restart';
  });
  assert.equal(pageErrors.length, 0, `Unexpected WebView2 JavaScript errors: ${pageErrors.join('; ')}`);

  const result = checks.some(check => check.result === 'FAIL') ? 'FAIL' : 'PASS';
  fs.writeFileSync(receiptPath, JSON.stringify({
    task: 'MP-8L1', status: result, platform: 'Windows packaged Tauri + real WebView2 via CDP',
    source: 'Synthetic fixture through packaged native host (not private owner archive)',
    executable_sha256: sha(executable), host_sha256: sha(host), firstPaintMs, restartMs,
    checks, pageErrors, owner_archive_qualified: false, nsis_installer_execution_qualified: false,
  }, null, 2) + '\n');
  console.log(`MP-8L1 native WebView2 synthetic smoke ${result}; ${checks.length} checks; evidence: ${receiptPath}`);
  if (result !== 'PASS') process.exitCode = 1;
} catch (error) {
  fs.mkdirSync(out, { recursive: true });
  record('runner', 'FAIL', String(error.message ?? error).slice(0, 350));
  fs.writeFileSync(receiptPath, JSON.stringify({ task: 'MP-8L1', status: 'FAIL', checks, pageErrors,
    owner_archive_qualified: false, nsis_installer_execution_qualified: false }, null, 2) + '\n');
  console.error(`MP-8L1 native WebView2 smoke FAIL: ${String(error.message ?? error)}`);
  process.exitCode = 1;
} finally {
  syntheticSecret = undefined;
  await closeApp();
}

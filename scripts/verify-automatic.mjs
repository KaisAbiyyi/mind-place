// Isolated fixture/native integration. Never opens the user's browser profile or archive.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { chromium } from '@playwright/test';

const project = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const output = fs.mkdtempSync(path.join(project, 'outputs', 'automatic-browser-'));
const archive = path.join(output, 'archive');
const extension = path.join(output, 'fixture-extension');
fs.cpSync(path.join(project, 'extension'), extension, { recursive: true });
const id = 'dninfeidggaaioioficnigfpfjkdicjh', origin = `chrome-extension://${id}/`;
const manifest = JSON.parse(fs.readFileSync(path.join(extension, 'manifest.json')));
// Test-only origin grant replaces Chromium's native permission dialog. Shipped access stays optional.
manifest.host_permissions = ['https://chatgpt.com/*'];
manifest.background.service_worker = 'qa-worker.mjs';
fs.writeFileSync(path.join(extension, 'manifest.json'), JSON.stringify(manifest));
fs.writeFileSync(path.join(extension, 'qa-worker.mjs'), "import {initAutomaticSaving} from './automatic.mjs'; globalThis.__qaAuto=initAutomaticSaving(chrome);\n");
const requests = [], errors = [];
const host = path.join(project, 'target/release/mind-place-capture-host.exe');
function roundtrip(request) {
  const bytes = Buffer.from(JSON.stringify(request)), header = Buffer.alloc(4);
  header.writeUInt32LE(bytes.length);
  const result = spawnSync(host, [origin], { input: Buffer.concat([header, bytes]),
    env: { ...process.env, MIND_PLACE_DATA_DIR: archive }, windowsHide: true, timeout: 15000, maxBuffer: 1024 * 1024 });
  assert.equal(result.status, 0, `Native host failed: ${result.error ?? result.stderr}`);
  assert.equal(result.stdout.readUInt32LE(0), result.stdout.length - 4);
  const receipt = JSON.parse(result.stdout.subarray(4));
  assert.equal(receipt.ok, true, JSON.stringify(receipt));
  requests.push({ request, receipt });
  return receipt;
}
// Disable the fixture page's own scroll anchoring so DOM edits do not move it independently of the extension.
const fixture = saved => `<!doctype html><html><head><title>Automatic fixture</title><style>body{margin:0;background:#17191c;color:#eee;font:16px system-ui}main{margin:20px}#scroller{height:400px;overflow:auto;overflow-anchor:none}p{padding:20px}</style></head><body><main><section id="scroller">${saved ? '<div data-message-author-role="user" data-message-id="uA"><time datetime="2025-09-03T10:00:00Z"></time><p>Loaded question A</p></div><div data-message-author-role="assistant" data-message-id="aA"><p>Loaded answer A</p></div>' : ''}<div style="height:3000px"></div></section></main><script>window.fixtureScrollEvents=0;document.querySelector('#scroller').scrollTop=500;setTimeout(()=>{document.addEventListener('scroll',()=>window.fixtureScrollEvents++,true)},100);</script></body></html>`;
const chatA = 'https://chatgpt.com/c/123e4567-e89b-12d3-a456-426614174000';
const chatB = chatA.replace('123e4567', '223e4567'), chatC = chatA.replace('123e4567', '323e4567');
async function waitFor(check, label) {
  const deadline = Date.now() + 15000;
  while (!check() && Date.now() < deadline) await new Promise(resolve => setTimeout(resolve, 50));
  assert.ok(check(), label);
}
const executable = process.env.MIND_PLACE_QA_CHROMIUM;
const context = await chromium.launchPersistentContext(path.join(output, 'profile'), { ...(executable ? { executablePath: executable } : {}),
  channel: 'chromium', headless: true, viewport: { width: 1000, height: 800 },
  args: [`--disable-extensions-except=${extension}`, `--load-extension=${extension}`] });
try {
  await context.route('https://chatgpt.com/**', route => route.fulfill({ contentType: 'text/html', body: fixture(new URL(route.request().url()).pathname !== '/') }));
  context.on('page', page => page.on('pageerror', error => errors.push(error.message)));
  await context.exposeBinding('qaNativeRoundtrip', ({ page }, request) => { assert.ok(page.url().startsWith(origin)); return roundtrip(request); });
  const worker = context.serviceWorkers()[0] ?? await context.waitForEvent('serviceworker');
  const bridge = await context.newPage(); await bridge.goto(origin + 'popup.html');
  await bridge.evaluate(() => chrome.runtime.onMessage.addListener((message, sender, respond) => {
    if (message.type !== '__qa-native') return false;
    window.qaNativeRoundtrip(message.request).then(respond, error => respond({ ok: false, error: error.message })); return true;
  }));
  await worker.evaluate(() => {
    globalThis.__qaMessages = [];
    chrome.runtime.onMessage.addListener((message, sender) => {
      if (message.type === 'auto-capture') globalThis.__qaMessages.push({ sourceUrl: message.sourceUrl, senderUrl: sender.url, tabUrl: sender.tab?.url, frameId: sender.frameId });
      return false;
    });
    chrome.runtime.sendNativeMessage = (_host, request) => chrome.runtime.sendMessage({ type: '__qa-native', request });
    // Test popup is a tab; normalize only that sender to the actual action-popup shape.
    chrome.runtime.onMessage.addListener((message, sender, respond) => sender.tab && sender.url === chrome.runtime.getURL('popup.html')
      ? globalThis.__qaAuto.onMessage(message, { ...sender, tab: undefined }, respond) : false);
  });
  const chat = await context.newPage(); await chat.goto(chatA); await chat.waitForTimeout(1500);
  assert.equal(requests.length, 0, 'OFF must not save on opening a chat');
  // Trap all extension-world scroll APIs and property writes, in addition to observing real offsets.
  await worker.evaluate(async () => chrome.scripting.executeScript({ target: { tabId: (await chrome.tabs.query({ url: 'https://chatgpt.com/*' }))[0].id }, func: () => {
    const deny = () => { throw new Error('Automatic saving attempted a forbidden scroll'); };
    for (const proto of [Element.prototype, HTMLElement.prototype]) for (const method of ['scrollTo', 'scrollBy', 'scrollIntoView']) Object.defineProperty(proto, method, { value: deny, configurable: true });
    const descriptor = Object.getOwnPropertyDescriptor(Element.prototype, 'scrollTop');
    Object.defineProperty(Element.prototype, 'scrollTop', { ...descriptor, set: deny });
    window.scrollTo = window.scrollBy = deny;
  } }));
  const popup = await context.newPage(); await popup.setViewportSize({ width: 380, height: 700 }); await popup.goto(origin + 'popup.html');
  await popup.locator('#auto-save:not(:disabled)').waitFor(); await popup.locator('#auto-save').check();
  await waitFor(() => requests.length === 1, 'opening a loaded conversation must auto-save after opt-in');
  assert.equal(requests[0].request.capture.messages.length, 2);
  assert.equal(requests[0].request.capture.capture_method, 'automatic-dom');
  const first = requests[0].receipt;
  const firstRawPath = path.join(archive, 'conversations', first.id, 'captures', first.snapshot_id + '.jsonl');
  const firstRawHash = createHash('sha256').update(fs.readFileSync(firstRawPath)).digest('hex');
  await chat.bringToFront(); await chat.waitForTimeout(100);
  assert.equal(await chat.locator('#scroller').evaluate(node => node.scrollTop), 500);
  await chat.evaluate(() => { window.fixtureScrollEvents = 0; });
  await chat.evaluate(() => { const answer = document.querySelector('[data-message-id="aA"]'); for (const text of ['B', 'Bu', 'Burst finished']) answer.textContent = text; });
  await waitFor(() => requests.length === 2, 'batched content must persist');
  const beforeStream = requests.length;
  await chat.evaluate(async () => { const node = document.querySelector('[data-message-id="aA"]'); for (let i = 0; i < 60; i++) { node.textContent = 'Streaming token ' + i; await new Promise(resolve => setTimeout(resolve, 100)); } });
  await chat.waitForTimeout(1600);
  assert.ok(requests.length - beforeStream >= 2 && requests.length - beforeStream <= 4, 'continuous streaming must checkpoint and save final content without a write per token');
  assert.ok(requests.at(-1).request.capture.messages.some(message => message.text.includes('Streaming token 59')));
  assert.equal(await chat.locator('#scroller').evaluate(node => node.scrollTop), 500);
  assert.equal(await chat.evaluate(() => window.fixtureScrollEvents), 0);
  const beforeRoute = requests.length;
  await chat.evaluate(next => { history.pushState({}, '', next); document.querySelector('#scroller').insertAdjacentHTML('afterbegin', '<div data-message-author-role="user" data-message-id="uB">Question B</div>'); }, chatB);
  await chat.waitForTimeout(1600); assert.equal(requests.length, beforeRoute, 'mixed old/new messages must not save during SPA transition');
  await chat.evaluate(() => { document.querySelector('[data-message-id="uA"]').remove(); document.querySelector('[data-message-id="aA"]').remove(); });
  await waitFor(() => requests.length === beforeRoute + 1, 'new opened conversation must save after old messages disappear').catch(async error => {
    console.log(JSON.stringify(await worker.evaluate(async () => ({ messages: globalThis.__qaMessages, state: await chrome.storage.local.get(null), tabs: (await chrome.tabs.query({ url: 'https://chatgpt.com/*' })).map(tab => ({ id: tab.id, url: tab.url })) })), null, 2));
    throw error;
  });
  assert.equal(requests.at(-1).request.capture.conversation_id, new URL(chatB).pathname.split('/').at(-1));
  assert.deepEqual(requests.at(-1).request.capture.messages.map(message => message.id), ['uB']);
  await chat.goto('https://chatgpt.com/');
  const beforeCreate = requests.length;
  await chat.evaluate(next => { history.pushState({}, '', next); document.querySelector('#scroller').insertAdjacentHTML('afterbegin', '<div data-message-author-role="user" data-message-id="uC">Newly created conversation C</div>'); }, chatC);
  await waitFor(() => requests.length === beforeCreate + 1, 'a new chat must save after acquiring a conversation URL and loaded content');
  await popup.screenshot({ path: path.join(output, 'automatic-popup.png') });
  await popup.locator('#auto-save').uncheck(); await popup.locator('#auto-save:not(:disabled)').waitFor();
  assert.equal(await worker.evaluate(() => chrome.storage.local.get('automaticSavingEnabled').then(value => value.automaticSavingEnabled)), false);
  assert.equal((await worker.evaluate(() => chrome.scripting.getRegisteredContentScripts())).length, 0);
  const beforeOff = requests.length;
  await chat.evaluate(() => { document.querySelector('[data-message-id="uC"]').textContent = 'Changed after OFF'; });
  await chat.waitForTimeout(1600); assert.equal(requests.length, beforeOff);
  assert.equal(createHash('sha256').update(fs.readFileSync(firstRawPath)).digest('hex'), firstRawHash);
  assert.equal(errors.length, 0, JSON.stringify(errors));
  const receipt = { status: 'PASS', date: new Date().toISOString(), output, nativeRequests: requests.length,
    offByDefault: true, loadedOnly: true, automaticScrollCalls: 0, scrollOffset: 500, scrollEvents: 0,
    streamingCheckpoints: requests.length - beforeStream - 2, spaMixedMessagesRejected: true, newChatSaved: true,
    optOutStopsSaving: true, registrationRemoved: true, previousRawPreserved: true, pageErrors: errors,
    browserVersion: context.browser()?.version(), nativeHost: host,
    fixtureOriginGrant: true, fixtureNativeTransport: true, signedInCapture: 'NOT RUN',
    screenshots: [path.join(output, 'automatic-popup.png')] };
  fs.writeFileSync(path.join(project, 'outputs', 'automatic-saving-browser-qa.json'), JSON.stringify(receipt, null, 2));
  console.log(JSON.stringify(receipt, null, 2));
} finally { await context.close(); }

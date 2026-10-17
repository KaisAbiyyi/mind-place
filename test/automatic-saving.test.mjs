import test from 'node:test';
import assert from 'node:assert/strict';
import { JSDOM } from 'jsdom';
import { readFile } from 'node:fs/promises';
import { captureConversation } from '../extension/capture.mjs';

const url = 'https://chatgpt.com/c/123e4567-e89b-12d3-a456-426614174000';
const other = url.replace('123e4567', '223e4567');
const runtimeId = 'dninfeidggaaioioficnigfpfjkdicjh';
const popupSender = { id: runtimeId, url: `chrome-extension://${runtimeId}/popup.html` };
const pageSender = { id: runtimeId, url, frameId: 0, tab: { id: 7, url, incognito: false } };
const pause = (ms = 25) => new Promise(resolve => setTimeout(resolve, ms));
async function until(check, message) {
  const deadline = Date.now() + 1500;
  while (!check() && Date.now() < deadline) await pause(10);
  assert.ok(check(), message);
}
function event() { const callbacks = new Set(); return { addListener: fn => callbacks.add(fn), removeListener: fn => callbacks.delete(fn), emit: (...args) => [...callbacks].forEach(fn => fn(...args)) }; }
function page(pageUrl = url) {
  const dom = new JSDOM('<title>Auto example</title><main><div data-message-author-role="user" data-message-id="u1">Question</div><div data-message-author-role="assistant" data-message-id="a1">Answer</div></main>', { url: pageUrl, runScripts: 'outside-only' });
  dom.window.TextEncoder = TextEncoder;
  dom.window.scrollTo = () => assert.fail('automatic mode must never scroll');
  dom.window.HTMLElement.prototype.scrollTo = () => assert.fail('automatic mode must never scroll');
  return dom;
}

test('automatic DOM capture has separate provenance and cannot enter history scrolling', () => {
  const dom = page();
  try {
    const read = options => dom.window.eval(`(${captureConversation.toString()})(${JSON.stringify(options)})`);
    const result = read({ automatic: true });
    assert.equal(result.capture.capture_method, 'automatic-dom');
    assert.equal(result.capture.completeness, 'partial');
    assert.match(result.capture.warnings[0], /automatic.*loaded|automatic.*rendered/i);
    assert.equal(read({ automatic: true, loadHistory: true }).ok, false);
    assert.equal(dom.window.__mindPlaceManualScroll, undefined);
  } finally { dom.window.close(); }
});

async function workerFixture({ enabled = false, permission = true, navigate = false, nativeFails = false, currentUrl = url, stalledAttachment = false } = {}) {
  const { initAutomaticSaving } = await import('../extension/automatic.mjs');
  const stored = { automaticSavingEnabled: enabled };
  const reads = [], saves = [], registrations = [], stops = [];
  const chrome = {
    runtime: { id: runtimeId, getURL: f => `chrome-extension://${runtimeId}/${f}`, onMessage: event(), onStartup: event(), onInstalled: event(),
      sendNativeMessage: async (host, request) => { saves.push(request); if (nativeFails) throw Error('Host missing'); return { ok: true, id: 'a'.repeat(64), snapshot_id: 'b'.repeat(64), message_count: request.capture.messages.length, completeness: 'partial', unchanged: false, warnings: [] }; } },
    storage: { local: { get: async () => ({ ...stored }), set: async values => Object.assign(stored, values) } },
    permissions: { contains: async () => permission, remove: async () => true, onRemoved: event() },
    scripting: { getRegisteredContentScripts: async () => [], registerContentScripts: async entries => registrations.push(...entries), unregisterContentScripts: async () => {},
      executeScript: async ({ func, files, args }) => { if (files) return stalledAttachment ? new Promise(() => {}) : []; reads.push(args); const dom = page(currentUrl); try { const result = dom.window.eval(`(${func.toString()})(${JSON.stringify(args[0])})`); return [{ frameId: 0, result }]; } finally { dom.window.close(); } } },
    tabs: { query: async () => [{ id: 7, url: currentUrl, incognito: false }], get: async () => ({ id: 7, url: navigate && reads.length ? other : currentUrl, incognito: false }),
      sendMessage: async (id, message) => stops.push(message), onUpdated: event(), onRemoved: event() },
  };
  const controller = initAutomaticSaving(chrome, { saveTimeoutMs: 100, attachmentTimeoutMs: 30 });
  const send = (message, sender = popupSender) => new Promise(resolve => { if (controller.onMessage(message, sender, resolve) !== true) resolve(undefined); });
  await controller.ready;
  return { chrome, controller, send, stored, reads, saves, registrations, stops };
}

test('automatic saving is off by default and cannot be enabled without optional origin permission', async () => {
  const fake = await workerFixture({ permission: false });
  await fake.send({ type: 'auto-capture', sourceUrl: url }, pageSender);
  const result = await fake.send({ type: 'set-auto-saving', enabled: true });
  assert.equal(result.enabled, false);
  assert.equal(fake.reads.length, 0);
  assert.equal(fake.saves.length, 0);
  assert.equal(fake.registrations.length, 0);
});

test('opt-in registers only ChatGPT top-frame observation, attaches existing tabs and disable stops it', async () => {
  const fake = await workerFixture();
  assert.equal((await fake.send({ type: 'set-auto-saving', enabled: true })).enabled, true);
  assert.deepEqual(fake.registrations[0].matches, ['https://chatgpt.com/*']);
  assert.equal(fake.registrations[0].allFrames, false);
  assert.deepEqual(fake.registrations[0].js, ['auto-observer.js']);
  assert.equal((await fake.send({ type: 'set-auto-saving', enabled: false })).enabled, false);
  assert.ok(fake.stops.some(message => message.type === 'auto-saving-disabled'));
  await fake.send({ type: 'auto-capture', sourceUrl: url }, pageSender);
  assert.equal(fake.saves.length, 0);
});

test('a frozen existing ChatGPT tab cannot hold the activation response open', async () => {
  const fake = await workerFixture({ stalledAttachment: true });
  const result = await Promise.race([fake.send({ type: 'set-auto-saving', enabled: true }), pause(200).then(() => ({ timedOut: true }))]);
  assert.equal(result.timedOut, undefined);
  assert.equal(result.enabled, true);
});

test('loaded changes save once per distinct content, with exact page identity and no scrolling', async () => {
  const fake = await workerFixture({ enabled: true });
  await fake.send({ type: 'auto-capture', sourceUrl: url }, pageSender);
  await fake.send({ type: 'auto-capture', sourceUrl: url }, pageSender);
  assert.equal(fake.saves.length, 1);
  assert.equal(fake.saves[0].capture.capture_method, 'automatic-dom');
  assert.equal(fake.reads[0][0].loadHistory, false);
  await fake.send({ type: 'auto-capture', sourceUrl: url }, { ...pageSender, frameId: 1 });
  await fake.send({ type: 'auto-capture', sourceUrl: url }, { ...pageSender, tab: { ...pageSender.tab, incognito: true } });
  assert.equal(fake.saves.length, 1);
});

test('navigation between automatic read and save rejects the stale window', async () => {
  const fake = await workerFixture({ enabled: true, navigate: true });
  await fake.send({ type: 'auto-capture', sourceUrl: url }, pageSender);
  assert.equal(fake.saves.length, 0);
});

test('SPA capture uses the current tab URL even if MessageSender retains the document startup URL', async () => {
  const fake = await workerFixture({ enabled: true, currentUrl: other });
  await fake.send({ type: 'auto-capture', sourceUrl: other }, pageSender);
  assert.equal(fake.saves.length, 1);
  assert.equal(fake.saves[0].capture.source_url, other);
});

test('native failure is reported and has no automatic retry loop', async () => {
  const fake = await workerFixture({ enabled: true, nativeFails: true });
  const result = await fake.send({ type: 'auto-capture', sourceUrl: url }, pageSender);
  assert.equal(result.ok, false);
  await pause(80);
  assert.equal(fake.saves.length, 1);
  assert.match(fake.stored.automaticSavingStatus.message, /host|setup|register/i);
});

async function observerFixture(enabled, html) {
  const dom = page(), calls = [];
  if (html) dom.window.document.querySelector('main').innerHTML = html;
  const source = await readFile(new URL('../extension/auto-observer.js', import.meta.url), 'utf8');
  dom.window.eval(source);
  dom.window.chrome = { runtime: { id: runtimeId, onMessage: event(), sendMessage: async message => {
    if (message.type === 'auto-page-state') return { enabled };
    calls.push(message); return { ok: true };
  } }, storage: { onChanged: event() } };
  const controller = dom.window.eval('installAutomaticObserver({ quietMs: 20, maxWaitMs: 65, tickMs: 15 })');
  try {
    if (enabled) await until(() => calls.length > 0, 'enabled observer must notify for the initial loaded window');
    else await pause(40);
  } catch (error) { dom.window.close(); throw error; }
  return { dom, calls, controller };
}

test('page observer is dormant when disabled, batches changes and stops without moving the page', async () => {
  const off = await observerFixture(false);
  assert.equal(off.calls.length, 0); off.dom.window.close();
  const fake = await observerFixture(true);
  try {
    assert.equal(fake.calls.length, 1);
    const answer = fake.dom.window.document.querySelector('[data-message-id="a1"]');
    for (const text of ['A', 'An', 'Answer changed']) answer.textContent = text;
    await until(() => fake.calls.length === 2, 'batched loaded changes must be observed');
    assert.equal(fake.calls.length, 2);
    fake.controller.stop(); answer.textContent = 'After disable'; await pause(40);
    assert.equal(fake.calls.length, 2);
    assert.equal(fake.dom.window.document.documentElement.scrollTop, 0);
  } finally { fake.dom.window.close(); }
});

test('SPA URL change never saves retained old content under the new conversation identity', async () => {
  const fake = await observerFixture(true);
  try {
    fake.dom.window.history.pushState({}, '', other);
    await pause(45);
    assert.equal(fake.calls.length, 1);
    fake.dom.window.document.querySelector('main').innerHTML = '<div data-message-author-role="user" data-message-id="new-u">New conversation</div>';
    await until(() => fake.calls.length === 2, 'new conversation content must be observed');
    assert.equal(fake.calls.length, 2);
    assert.equal(fake.calls[1].sourceUrl, other);
  } finally { fake.dom.window.close(); }
});

test('manual scrolling pauses automatic observation until the manual job finishes', async () => {
  const fake = await observerFixture(true);
  try {
    fake.dom.window.__mindPlaceManualScroll = { operationId: 'manual' };
    fake.dom.window.document.querySelector('[data-message-id="a1"]').textContent = 'Manual loading';
    await pause(50); assert.equal(fake.calls.length, 1);
    delete fake.dom.window.__mindPlaceManualScroll;
    await until(() => fake.calls.length === 2, 'automatic observation must resume after manual loading');
  } finally { fake.dom.window.close(); }
});

test('classic native turns are observed even without message-role or search-unit attributes', async () => {
  const fake = await observerFixture(true, '<article data-testid="conversation-turn-0" data-turn="user">Classic question</article><article data-testid="conversation-turn-1" data-turn="assistant">Classic answer</article>');
  try { assert.equal(fake.calls.length, 1); }
  finally { fake.dom.window.close(); }
});

test('positional fallback turn keys do not suppress an opened different conversation', async () => {
  const layout = text => `<article data-turn-key="fallback-turn-0"><div data-message-author-role="user">${text}</div></article><article data-turn-key="fallback-turn-1"><div data-message-author-role="assistant">Answer</div></article>`;
  const fake = await observerFixture(true, layout('First conversation question'));
  try {
    fake.dom.window.history.pushState({}, '', other);
    await pause(40); assert.equal(fake.calls.length, 1);
    fake.dom.window.document.querySelector('main').innerHTML = layout('Second conversation question');
    await until(() => fake.calls.length === 2, 'fallback identities must allow distinct new conversation content');
    assert.equal(fake.calls[1].sourceUrl, other);
  } finally { fake.dom.window.close(); }
});

test('SPA transition waits until retained visible old messages disappear before saving new messages', async () => {
  const fake = await observerFixture(true);
  try {
    fake.dom.window.history.pushState({}, '', other);
    fake.dom.window.document.querySelector('main').insertAdjacentHTML('beforeend', '<div data-message-author-role="user" data-message-id="new-u">New conversation question</div>');
    await pause(100);
    assert.equal(fake.calls.length, 1, 'mixed visible old/new conversations must not be saved');
    fake.dom.window.document.querySelector('[data-message-id="u1"]').remove();
    fake.dom.window.document.querySelector('[data-message-id="a1"]').remove();
    await until(() => fake.calls.length === 2, 'new conversation can save after old messages disappear');
  } finally { fake.dom.window.close(); }
});

test('automatic capture refuses message DOM explicitly owned by another conversation', () => {
  const dom = page();
  try {
    dom.window.document.querySelector('main').setAttribute('data-conversation-id', '223e4567-e89b-12d3-a456-426614174000');
    const result = dom.window.eval(`(${captureConversation.toString()})({ automatic: true })`);
    assert.equal(result.ok, false);
    assert.match(result.error, /conversation|identity|owner/i);
  } finally { dom.window.close(); }
});

test('a replaced fallback conversation with the same first prompt can auto-save under its new URL', async () => {
  const layout = '<article data-turn-key="fallback-turn-0"><div data-message-author-role="user">Same first prompt</div></article><article data-turn-key="fallback-turn-1"><div data-message-author-role="assistant">Same answer</div></article>';
  const fake = await observerFixture(true, layout);
  try {
    fake.dom.window.history.pushState({}, '', other);
    fake.dom.window.document.querySelector('main').innerHTML = layout;
    await until(() => fake.calls.length === 2, 'fully replaced fallback messages must not be suppressed by identical prompt text');
    assert.equal(fake.calls[1].sourceUrl, other);
  } finally { fake.dom.window.close(); }
});

import test from 'node:test';
import assert from 'node:assert/strict';
import { JSDOM } from 'jsdom';
import { initPopup } from '../extension/popup.mjs';
import { readFile } from 'node:fs/promises';

const conversationId = '123e4567-e89b-12d3-a456-426614174000';
const conversationUrl = `https://chatgpt.com/c/${conversationId}`;

function makePopup() {
  return new JSDOM(`
    <main>
      <button id="save" type="button">Save this conversation</button>
      <p id="status" role="status" aria-live="polite">Ready.</p>
      <p id="warning" role="alert" hidden></p>
    </main>`, { url: 'chrome-extension://test/popup.html', runScripts: 'outside-only' });
}

function makeConversationPage(url = conversationUrl, assistantText = 'Answer') {
  const dom = new JSDOM(`
    <title>Manual capture example - ChatGPT</title>
    <main>
      <div data-message-author-role="user" data-message-id="u-1">Question</div>
      <div data-message-author-role="assistant" data-message-id="a-1">${assistantText}</div>
    </main>`, { url, runScripts: 'outside-only' });
  dom.window.TextEncoder = TextEncoder;
  return dom;
}

function makeChrome({ page, tabUrl = conversationUrl, incognito = false, queryReply, nativeReply, onNative, afterReadUrl, mutateCapture, frameId = 0 } = {}) {
  const nativeRequests = [];
  let currentUrl = tabUrl;
  let injected = 0;
  const chromeApi = {
    tabs: {
      query: async () => queryReply ?? [{ id: 7, active: true, url: tabUrl, incognito }],
      get: async () => ({ id: 7, url: afterReadUrl ?? currentUrl, incognito }),
    },
    scripting: {
      executeScript: async ({ target, func }) => {
        assert.deepEqual(target, { tabId: 7 });
        injected += 1;
        const result = page.window.eval(`(${func.toString()})()`);
        if (mutateCapture && result.capture) mutateCapture(result.capture);
        if (afterReadUrl) currentUrl = afterReadUrl;
        return [{ frameId, result }];
      },
    },
    runtime: {
      sendNativeMessage: async (host, request) => {
        nativeRequests.push({ host, request });
        onNative?.();
        if (nativeReply instanceof Error) throw nativeReply;
        if (nativeReply && typeof nativeReply.then === 'function') return nativeReply;
        return nativeReply ?? {
          ok: true,
          id: 'a'.repeat(64),
          snapshot_id: 'b'.repeat(64),
          message_count: 2,
          unchanged: false,
          completeness: 'partial',
          warnings: ['Only currently rendered messages were saved.'],
        };
      },
    },
  };
  return { chromeApi, nativeRequests, get injected() { return injected; } };
}

function startPopup(chromeApi, options) {
  const dom = makePopup();
  const controller = initPopup({ chromeApi, documentRef: dom.window.document, ...options });
  return { dom, controller };
}

test('only an explicit click performs a one-shot DOM read and sends the native save request', async () => {
  const page = makeConversationPage();
  const fake = makeChrome({ page });
  const { dom, controller } = startPopup(fake.chromeApi);

  await Promise.resolve();
  assert.equal(fake.injected, 0);
  assert.equal(fake.nativeRequests.length, 0);

  dom.window.document.querySelector('#save').click();
  await controller.whenIdle();

  assert.equal(fake.injected, 1);
  assert.equal(fake.nativeRequests.length, 1);
  assert.equal(fake.nativeRequests[0].host, 'com.mindplace.capture');
  assert.deepEqual(Object.keys(fake.nativeRequests[0].request).sort(), ['capture', 'operation', 'protocol']);
  assert.equal(fake.nativeRequests[0].request.protocol, 1);
  assert.equal(fake.nativeRequests[0].request.operation, 'save');
  assert.equal(fake.nativeRequests[0].request.capture.capture_method, 'manual-dom');
  assert.match(dom.window.document.querySelector('#status').textContent, /saved locally/i);
});

test('sends the complete rendered answer above 48 KB to the native save request', async () => {
  const longAnswer = 'x'.repeat(80_000);
  const page = makeConversationPage(conversationUrl, longAnswer);
  const fake = makeChrome({ page });
  const { dom, controller } = startPopup(fake.chromeApi);
  dom.window.document.querySelector('#save').click();
  await controller.whenIdle();

  assert.equal(fake.nativeRequests.length, 1);
  assert.equal(fake.nativeRequests[0].request.capture.messages[1].text, longAnswer);
  assert.match(dom.window.document.querySelector('#status').textContent, /saved locally/i);
});

test('does not call the native host when a rendered message exceeds the per-message limit', async () => {
  const oversizedAnswer = 'x'.repeat(4 * 1024 * 1024 + 1);
  const page = makeConversationPage(conversationUrl, oversizedAnswer);
  const fake = makeChrome({ page });
  const { dom, controller } = startPopup(fake.chromeApi);
  dom.window.document.querySelector('#save').click();
  await controller.whenIdle();

  assert.equal(fake.nativeRequests.length, 0);
  assert.match(dom.window.document.querySelector('#status').textContent, /4\s*MiB|4\s*MB|4\s*megabyte/i);
  assert.match(dom.window.document.querySelector('#status').textContent, /nothing was saved/i);
});

test('does not read or save private tabs, Temporary Chat, or unrelated pages', async () => {
  for (const setup of [
    { tabUrl: conversationUrl, incognito: true },
    { tabUrl: 'https://chatgpt.com/' },
    { tabUrl: conversationUrl, temporary: true },
  ]) {
    const page = makeConversationPage(setup.temporary ? conversationUrl : setup.tabUrl);
    if (setup.temporary) {
      const marker = page.window.document.createElement('button');
      marker.setAttribute('aria-label', 'Temporary Chat');
      marker.setAttribute('aria-pressed', 'true');
      marker.textContent = 'Temporary Chat';
      page.window.document.body.prepend(marker);
    }
    const fake = makeChrome({ page, ...setup });
    const { dom, controller } = startPopup(fake.chromeApi);
    dom.window.document.querySelector('#save').click();
    await controller.whenIdle();

    assert.equal(fake.nativeRequests.length, 0);
    assert.match(dom.window.document.querySelector('#status').textContent, /private|temporary|conversation/i);
  }
});

test('rechecks the exact URL after the DOM read and refuses a navigation race', async () => {
  const page = makeConversationPage();
  const fake = makeChrome({
    page,
    afterReadUrl: `https://chatgpt.com/c/223e4567-e89b-12d3-a456-426614174000`,
  });
  const { dom, controller } = startPopup(fake.chromeApi);
  dom.window.document.querySelector('#save').click();
  await controller.whenIdle();

  assert.equal(fake.injected, 1);
  assert.equal(fake.nativeRequests.length, 0);
  assert.match(dom.window.document.querySelector('#status').textContent, /navigat|changed/i);
});

test('quick save rejects malformed normalized captures before native submission', async () => {
  for (const patch of [{ schema_version: 2 }, { warnings: [] }, { messages: [] }, { completeness: 'complete' }]) {
    const page = makeConversationPage();
    const fake = makeChrome({ page, mutateCapture: capture => Object.assign(capture, patch) });
    const { dom, controller } = startPopup(fake.chromeApi);
    try {
      dom.window.document.querySelector('#save').click();
      await controller.whenIdle();
      assert.equal(fake.nativeRequests.length, 0);
      assert.match(dom.window.document.querySelector('#status').textContent, /contract|identity|nothing.*sent/i);
    } finally { controller.destroy(); dom.window.close(); page.window.close(); }
  }
});

test('quick save cannot accept a subframe result as top-level evidence', async () => {
  const page = makeConversationPage();
  const fake = makeChrome({ page, frameId: 1 });
  const { dom, controller } = startPopup(fake.chromeApi);
  try {
    dom.window.document.querySelector('#save').click();
    await controller.whenIdle();
    assert.equal(fake.nativeRequests.length, 0);
  } finally { controller.destroy(); dom.window.close(); page.window.close(); }
});

test('keeps the save pending until the native host acknowledges its durable receipt', async () => {
  let acknowledge;
  const nativeReply = new Promise((resolve) => { acknowledge = resolve; });
  const page = makeConversationPage();
  const fake = makeChrome({ page, nativeReply });
  const { dom, controller } = startPopup(fake.chromeApi);
  const button = dom.window.document.querySelector('#save');
  button.click();

  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(button.disabled, true);
  assert.match(dom.window.document.querySelector('#status').textContent, /saving locally/i);
  assert.doesNotMatch(dom.window.document.querySelector('#status').textContent, /saved locally/i);

  acknowledge({
    ok: true,
    id: 'a'.repeat(64),
    snapshot_id: 'c'.repeat(64),
    message_count: 2,
    unchanged: true,
    completeness: 'partial',
    warnings: ['Only rendered messages are included.'],
  });
  await controller.whenIdle();
  assert.match(dom.window.document.querySelector('#status').textContent, /already saved|unchanged/i);
});

test('prevents duplicate saves while a native acknowledgement is pending', async () => {
  let acknowledge;
  const nativeReply = new Promise((resolve) => { acknowledge = resolve; });
  const page = makeConversationPage();
  const fake = makeChrome({ page, nativeReply });
  const { dom, controller } = startPopup(fake.chromeApi);
  const button = dom.window.document.querySelector('#save');
  button.click();
  button.dispatchEvent(new dom.window.Event('click', { bubbles: true }));
  await new Promise((resolve) => setImmediate(resolve));

  assert.equal(fake.injected, 1);
  assert.equal(fake.nativeRequests.length, 1);
  acknowledge({
    ok: true,
    id: 'a'.repeat(64),
    snapshot_id: 'b'.repeat(64),
    message_count: 2,
    unchanged: false,
    completeness: 'partial',
    warnings: [],
  });
  await controller.whenIdle();
});

test('does not accept a native receipt that upgrades a manual partial capture to complete', async () => {
  const page = makeConversationPage();
  const fake = makeChrome({
    page,
    nativeReply: {
      ok: true,
      id: 'a'.repeat(64),
      snapshot_id: 'b'.repeat(64),
      message_count: 2,
      unchanged: false,
      completeness: 'complete',
      warnings: [],
    },
  });
  const { dom, controller } = startPopup(fake.chromeApi);
  dom.window.document.querySelector('#save').click();
  await controller.whenIdle();

  assert.match(dom.window.document.querySelector('#status').textContent, /valid save receipt/i);
  assert.doesNotMatch(dom.window.document.querySelector('#status').textContent, /saved locally/i);
});

test('quick capture rejects a native receipt that reports a different number of messages', async () => {
  const page = makeConversationPage();
  const fake = makeChrome({ page, nativeReply: { ok: true, id: 'a'.repeat(64), snapshot_id: 'b'.repeat(64), message_count: 99, unchanged: false, completeness: 'partial', warnings: [] } });
  const { dom, controller } = startPopup(fake.chromeApi);
  dom.window.document.querySelector('#save').click();
  await controller.whenIdle();
  assert.match(dom.window.document.querySelector('#status').textContent, /valid save receipt/i);
  assert.doesNotMatch(dom.window.document.querySelector('#status').textContent, /saved locally/i);
});

test('reports missing native host setup and uncertain acknowledgement truthfully', async () => {
  const page = makeConversationPage();
  const missing = makeChrome({ page, nativeReply: new Error('Specified native messaging host not found.') });
  const first = startPopup(missing.chromeApi);
  first.dom.window.document.querySelector('#save').click();
  await first.controller.whenIdle();
  assert.match(first.dom.window.document.querySelector('#status').textContent, /native host is unavailable/i);
  assert.match(first.dom.window.document.querySelector('#status').textContent, /Mind Place/i);

  const neverAcknowledges = new Promise(() => {});
  const uncertain = makeChrome({ page, nativeReply: neverAcknowledges });
  const second = startPopup(uncertain.chromeApi, { saveTimeoutMs: 100 });
  second.dom.window.document.querySelector('#save').click();
  await second.controller.whenIdle();
  assert.match(second.dom.window.document.querySelector('#status').textContent, /may have been saved|outcome is unknown/i);
  assert.doesNotMatch(second.dom.window.document.querySelector('#status').textContent, /saved locally/i);
});

test('bounds the whole save flow when a browser API stalls before the native request', async () => {
  const page = makeConversationPage();
  const fake = makeChrome({ page, queryReply: new Promise(() => {}) });
  const { dom, controller } = startPopup(fake.chromeApi, { saveTimeoutMs: 10 });
  dom.window.document.querySelector('#save').click();
  await controller.whenIdle();

  assert.equal(fake.nativeRequests.length, 0);
  assert.match(dom.window.document.querySelector('#status').textContent, /timed out|nothing was sent/i);
  assert.doesNotMatch(dom.window.document.querySelector('#status').textContent, /saved locally/i);
});

test('the shipped popup defaults to manually triggered history loading and delegates the job', async () => {
  const dom = new JSDOM(await readFile(new URL('../extension/popup.html', import.meta.url), 'utf8'), { url: 'chrome-extension://test/popup.html' });
  const sent = [];
  let active = false;
  const chromeApi = { runtime: { sendMessage: async (message) => {
    sent.push(message);
    if (message.type === 'start-capture') active = true;
    return { job: active ? { active: true, operationId: 'job-1', phase: 'loading', status: 'Loading conversation history…', warning: '', messageCount: 4 } : null };
  } } };
  const controller = initPopup({ chromeApi, documentRef: dom.window.document });
  try {
    await controller.whenIdle();
    const checkbox = dom.window.document.querySelector('#load-history');
    assert.equal(checkbox?.checked, true);
    assert.equal(sent.some((message) => message.type === 'start-capture'), false);
    dom.window.document.querySelector('#save').click();
    await controller.whenIdle();
    assert.deepEqual(sent.find((message) => message.type === 'start-capture'), { type: 'start-capture', loadHistory: true });
    assert.equal(dom.window.document.querySelector('#save').disabled, true);
    assert.equal(dom.window.document.querySelector('#stop').hidden, false);
    dom.window.document.querySelector('#stop').click();
    await new Promise((resolve) => setImmediate(resolve));
    assert.ok(sent.some((message) => message.type === 'capture-control' && message.action === 'stop' && message.operationId === 'job-1'));
  } finally { controller.destroy(); dom.window.close(); }
});

test('reopening the popup reconnects to progress and Cancel controls without starting a second capture', async () => {
  const dom = new JSDOM(await readFile(new URL('../extension/popup.html', import.meta.url), 'utf8'), { url: 'chrome-extension://test/popup.html' });
  const sent = [];
  const chromeApi = { runtime: { sendMessage: async (message) => {
    sent.push(message);
    return { job: { active: true, operationId: 'previous-job', phase: 'loading', status: 'Loading… 60 messages collected.', warning: '', messageCount: 60 } };
  } } };
  const controller = initPopup({ chromeApi, documentRef: dom.window.document });
  try {
    await controller.whenIdle();
    assert.match(dom.window.document.querySelector('#status').textContent, /60 messages/i);
    assert.equal(sent.some((message) => message.type === 'start-capture'), false);
    dom.window.document.querySelector('#cancel').click();
    await new Promise((resolve) => setImmediate(resolve));
    assert.ok(sent.some((message) => message.type === 'capture-control' && message.action === 'cancel'));
  } finally { controller.destroy(); dom.window.close(); }
});

test('a restarted worker releases stale loading controls and lets the popup start a fresh manual job', async () => {
  const dom = new JSDOM(await readFile(new URL('../extension/popup.html', import.meta.url), 'utf8'), { url: 'chrome-extension://test/popup.html' });
  let statusReads = 0;
  let starts = 0;
  const chromeApi = { runtime: { sendMessage: async (message) => {
    if (message.type === 'start-capture') { starts += 1; return { job: null }; }
    statusReads += 1;
    return { job: statusReads === 1 ? { active: true, operationId: 'lost-job', phase: 'loading', status: 'Loading…', warning: '', messageCount: 3 } : null };
  } } };
  const controller = initPopup({ chromeApi, documentRef: dom.window.document });
  try {
    await controller.whenIdle();
    await new Promise((resolve) => setTimeout(resolve, 850));
    assert.equal(dom.window.document.querySelector('#save').disabled, false);
    assert.equal(dom.window.document.querySelector('#cancel').hidden, true);
    assert.match(dom.window.document.querySelector('#status').textContent, /check|archive|interrupted/i);
    dom.window.document.querySelector('#save').click();
    await controller.whenIdle();
    assert.equal(starts, 1);
  } finally { controller.destroy(); dom.window.close(); }
});

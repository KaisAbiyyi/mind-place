import test from 'node:test';
import assert from 'node:assert/strict';

const conversationUrl = 'https://chatgpt.com/c/123e4567-e89b-12d3-a456-426614174000';
const runtimeId = 'dninfeidggaaioioficnigfpfjkdicjh';
const popupSender = { id: runtimeId, url: `chrome-extension://${runtimeId}/popup.html` };
const capture = { schema_version: 1, provider: 'chatgpt', conversation_id: '123e4567-e89b-12d3-a456-426614174000', source_url: conversationUrl, title: 'Long chat', captured_at: '2026-10-15T12:00:00Z', capture_method: 'manual-dom', completeness: 'partial', warnings: ['Scrolling cannot prove completeness.'], messages: [{ id: 'a1', role: 'assistant', text: 'Loaded history', format: 'markdown', attachments: [] }] };
const receipt = { ok: true, id: 'a'.repeat(64), snapshot_id: 'b'.repeat(64), message_count: 1, unchanged: false, completeness: 'partial', warnings: [] };

function listeners() {
  const callbacks = new Set();
  return { addListener: (fn) => callbacks.add(fn), removeListener: (fn) => callbacks.delete(fn), emit: (...args) => { for (const callback of callbacks) callback(...args); } };
}

async function fixture({ waitForRead, afterReadUrl, nativeReply = receipt, incognito = false, pageCapture = capture } = {}) {
  let createCaptureWorker;
  try { ({ createCaptureWorker } = await import('../extension/worker.mjs')); }
  catch { assert.fail('manual capture worker must coordinate explicit scroll/save jobs independently of the popup'); }
  let readCalls = 0;
  let connections = 0;
  const requests = [];
  const controls = [];
  const port = { onMessage: listeners(), onDisconnect: listeners(), disconnect() {}, postMessage(request) { requests.push(request); if (nativeReply) queueMicrotask(() => port.onMessage.emit(nativeReply)); } };
  const chrome = {
    tabs: { query: async () => [{ id: 7, url: conversationUrl, incognito }], get: async () => ({ id: 7, url: afterReadUrl ?? conversationUrl, incognito }) },
    scripting: { executeScript: async ({ func, args, target }) => {
      assert.equal(target.tabId, 7);
      if (func.name === 'captureConversation') { readCalls += 1; if (waitForRead) await waitForRead; return [{ frameId: 0, result: { ok: true, capture: structuredClone(pageCapture) } }]; }
      controls.push({ name: func.name, args });
      return [{ frameId: 0, result: true }];
    } },
    runtime: { id: runtimeId, getURL: (file) => `chrome-extension://${runtimeId}/${file}`, connectNative: (name) => { assert.equal(name, 'com.mindplace.capture'); connections += 1; return port; } },
  };
  const listener = createCaptureWorker(chrome, { saveTimeoutMs: 60, captureTimeoutMs: 1000 });
  const send = (message, sender = popupSender) => new Promise((resolve) => {
    const pending = listener(message, sender, resolve);
    if (pending !== true) resolve(undefined);
  });
  const settled = async () => {
    for (let attempt = 0; attempt < 300; attempt += 1) {
      const response = await send({ type: 'capture-status' });
      if (response?.job && !response.job.active) return response.job;
      await new Promise((resolve) => setTimeout(resolve, 5));
    }
    assert.fail('manual worker did not settle within its bounded test deadline');
  };
  return { send, settled, port, requests, controls, get reads() { return readCalls; }, get connections() { return connections; } };
}

test('worker does not read a tab or start a native process until a trusted popup starts a capture', async () => {
  const fake = await fixture();
  await fake.send({ type: 'capture-status' });
  await fake.send({ type: 'start-capture', loadHistory: true }, { ...popupSender, tab: { id: 7 }, url: conversationUrl });
  assert.equal(fake.reads, 0);
  assert.equal(fake.connections, 0);
  await fake.send({ type: 'start-capture', loadHistory: true });
  const job = await fake.settled();
  assert.equal(job.phase, 'saved');
  assert.equal(fake.requests.length, 1);
  assert.equal(fake.requests[0].capture.messages[0].text, 'Loaded history');
});

test('a scrolling save survives a closed popup and rejects a second concurrent start', async () => {
  let release;
  const fake = await fixture({ waitForRead: new Promise((resolve) => { release = resolve; }) });
  const first = await fake.send({ type: 'start-capture', loadHistory: true });
  const second = await fake.send({ type: 'start-capture', loadHistory: true });
  assert.equal(first.job.operationId, second.job.operationId);
  assert.equal(fake.requests.length, 0);
  release();
  const job = await fake.settled();
  assert.equal(job.phase, 'saved');
  assert.equal(fake.connections, 1);
  assert.equal(fake.reads, 1);
});

test('worker rejects navigation after scrolling before sending a native payload', async () => {
  const fake = await fixture({ afterReadUrl: conversationUrl.replace('123e4567', '223e4567') });
  await fake.send({ type: 'start-capture', loadHistory: true });
  const job = await fake.settled();
  assert.equal(job.phase, 'error');
  assert.match(job.status, /changed|navigat/i);
  assert.equal(fake.requests.length, 0);
});

test('worker refuses private tabs before reading or starting the native host', async () => {
  const fake = await fixture({ incognito: true });
  await fake.send({ type: 'start-capture', loadHistory: true });
  const job = await fake.settled();
  assert.match(job.status, /private/i);
  assert.equal(fake.reads, 0);
  assert.equal(fake.connections, 0);
});

test('manual coordinator rejects malformed normalized captures before native submission', async () => {
  for (const patch of [
    { schema_version: 2 },
    { warnings: 'not a warning list' },
    { messages: [] },
    { messages: [{ ...capture.messages[0], role: 'invented-role' }] },
    { messages: [capture.messages[0], capture.messages[0]] },
    { messages: [{ ...capture.messages[0], attachments: [{ id: 'untrusted', asset_path: 'assets/local.png' }] }] },
  ]) {
    const fake = await fixture({ pageCapture: { ...capture, ...patch } });
    await fake.send({ type: 'start-capture', loadHistory: false });
    const job = await fake.settled();
    assert.equal(job.phase, 'error');
    assert.equal(fake.requests.length, 0, JSON.stringify(patch));
  }
});

test('cancelling a loading job prevents a late script result from being saved', async () => {
  let release;
  const fake = await fixture({ waitForRead: new Promise((resolve) => { release = resolve; }) });
  const started = await fake.send({ type: 'start-capture', loadHistory: true });
  await new Promise((resolve) => setImmediate(resolve));
  await fake.send({ type: 'capture-control', operationId: started.job.operationId, action: 'cancel' });
  release();
  const job = await fake.settled();
  assert.equal(job.phase, 'error');
  assert.match(job.status, /cancel/i);
  assert.equal(fake.requests.length, 0);
});

test('only a matching page job can report progress, and a missing native acknowledgement stays uncertain', async () => {
  const fake = await fixture({ nativeReply: null });
  const started = await fake.send({ type: 'start-capture', loadHistory: true });
  await fake.send({ type: 'capture-progress', operationId: started.job.operationId, messageCount: 999, text: 'forged' }, { id: runtimeId, url: conversationUrl, tab: { id: 99 } });
  const job = await fake.settled();
  assert.equal(job.phase, 'error');
  assert.match(job.status, /may have been saved|check.*archive/i);
  assert.doesNotMatch(job.status, /saved locally/i);
});

test('a disconnected native host cancels loading without accepting a later capture', async () => {
  let release;
  const fake = await fixture({ waitForRead: new Promise((resolve) => { release = resolve; }) });
  await fake.send({ type: 'start-capture', loadHistory: true });
  await new Promise((resolve) => setImmediate(resolve));
  fake.port.onDisconnect.emit();
  const job = await fake.settled();
  release();
  assert.equal(job.phase, 'error');
  assert.equal(fake.requests.length, 0);
});

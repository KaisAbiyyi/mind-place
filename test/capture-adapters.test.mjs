import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { JSDOM } from 'jsdom';
import { captureConversation } from '../extension/capture.mjs';

const fixture = JSON.parse(await readFile(new URL('./fixtures/capture/chatgpt-v1.json', import.meta.url)));
const html = await readFile(new URL('./fixtures/capture/chatgpt-v1.html', import.meta.url), 'utf8');

test('adapter selection fails closed for unqualified providers and hostile origins/routes', async () => {
  const { resolveCaptureAdapter, captureAdapterForPage } = await import('../extension/providers/index.mjs');
  for (const url of [
    fixture.source_url.replace('chatgpt.com', 'claude.ai'),
    fixture.source_url.replace('chatgpt.com', 'gemini.google.com'),
    fixture.source_url.replace('chatgpt.com', 'chatgpt.com.evil.test'),
    fixture.source_url.replace('https:', 'http:'),
    fixture.source_url.replace('chatgpt.com', 'user@chatgpt.com'),
    fixture.source_url.replace('chatgpt.com', 'chatgpt.com:444'),
    'https://chatgpt.com/', 'https://chatgpt.com/share/123', 'not a URL',
  ]) assert.equal(resolveCaptureAdapter(url), null, url);
  assert.equal(captureAdapterForPage('https://chatgpt.com.evil.test/'), null);
  // A homepage observer may watch subsequent SPA navigation, but cannot capture it.
  assert.equal(captureAdapterForPage('https://chatgpt.com/').id, 'chatgpt');
});

test('ChatGPT adapter serializes without module closures and preserves the pre-refactor V1 fixture', async () => {
  const { resolveCaptureAdapter, captureMatchesIdentity } = await import('../extension/providers/index.mjs');
  const { adapter, identity } = resolveCaptureAdapter(fixture.source_url);
  assert.equal(adapter.capture, captureConversation, 'legacy entry point uses the same implementation');
  const dom = new JSDOM(html, { url: fixture.source_url, runScripts: 'outside-only' });
  dom.window.TextEncoder = TextEncoder;
  dom.window.fetch = () => assert.fail('capture must not fetch remote evidence');
  dom.window.scrollTo = () => assert.fail('loaded capture must not scroll');
  dom.window.HTMLElement.prototype.scrollTo = () => assert.fail('loaded capture must not scroll');
  try {
    const result = dom.window.eval(`(${adapter.capture.toString()})()`);
    assert.equal(result.ok, true, result.error);
    result.capture.captured_at = fixture.captured_at;
    assert.deepEqual(JSON.parse(JSON.stringify(result.capture)), fixture);
    assert.equal(captureMatchesIdentity(adapter, identity, result.capture, false), true);
    const automatic = dom.window.eval(`(${adapter.capture.toString()})({automatic:true,loadHistory:false})`);
    assert.equal(captureMatchesIdentity(adapter, identity, automatic.capture, true), true);
    assert.equal(captureMatchesIdentity(adapter, identity, automatic.capture, false), false);
    assert.equal(dom.window.__mindPlaceManualScroll, undefined);
  } finally { dom.window.close(); }
});

test('normalized admission rejects malformed payloads without rewriting evidence', async () => {
  const { resolveCaptureAdapter, captureMatchesIdentity } = await import('../extension/providers/index.mjs');
  const { adapter, identity } = resolveCaptureAdapter(fixture.source_url);
  const before = structuredClone(fixture);
  assert.equal(captureMatchesIdentity(adapter, identity, fixture, false), true);
  assert.deepEqual(fixture, before);
  for (const patch of [
    { schema_version: 2 }, { provider: 'claude' }, { conversation_id: 'another' },
    { source_url: fixture.source_url + '?changed=true' },
    { capture_method: 'legacy-import' }, { capture_method: 'automatic-dom' }, { completeness: 'complete' },
    { warnings: [] }, { warnings: ['x'.repeat(2049)] }, { messages: [] },
    { captured_at: 'not-a-time' }, { conversation_created_at: 'not-a-time' },
    { messages: [{ ...fixture.messages[0], authored_at: 'not-a-time' }] },
    { messages: [{ ...fixture.messages[0], text: 'x'.repeat(4 * 1024 * 1024 + 1) }] },
    { messages: [{ ...fixture.messages[0], role: 'unknown' }] },
    { messages: [fixture.messages[0], fixture.messages[0]] },
    { messages: [{ ...fixture.messages[0], attachments: [{ id: 'a', name: 'local', mime_type: 'image/png', asset_path: 'local.png' }] }] },
  ]) assert.equal(captureMatchesIdentity(adapter, identity, { ...fixture, ...patch }, false), false);
});

test('ChatGPT privacy and automatic no-scroll guards still run behind the adapter', async () => {
  const { resolveCaptureAdapter } = await import('../extension/providers/index.mjs');
  const { adapter } = resolveCaptureAdapter(fixture.source_url);
  for (const [url, content, options] of [
    [fixture.source_url + '?temporary-chat=true', html, {}],
    [fixture.source_url, '<button aria-pressed="true" aria-label="Temporary Chat">Temporary Chat</button>' + html, {}],
    [fixture.source_url, html, { automatic: true, loadHistory: true }],
  ]) {
    const dom = new JSDOM(content, { url, runScripts: 'outside-only' });
    dom.window.TextEncoder = TextEncoder;
    dom.window.scrollTo = () => assert.fail('blocked capture must not scroll');
    dom.window.HTMLElement.prototype.scrollTo = () => assert.fail('blocked capture must not scroll');
    try {
      assert.equal(dom.window.eval(`(${adapter.capture.toString()})(${JSON.stringify(options)})`).ok, false);
    } finally { dom.window.close(); }
  }
});

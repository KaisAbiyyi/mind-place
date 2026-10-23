import test from 'node:test';
import assert from 'node:assert/strict';
import { JSDOM } from 'jsdom';
import { parseGeminiConversationUrl, captureGeminiConversation } from '../extension/providers/gemini.mjs';
import { resolveCaptureAdapter, captureMatchesIdentity } from '../extension/providers/index.mjs';

const url = 'https://gemini.google.com/app/AbCd1234efGh5678';
const html = '<title>Design notes - Gemini</title><nav><user-query>Private sidebar</user-query></nav>' +
  '<main data-conversation-id="AbCd1234efGh5678">' +
  '<user-query data-message-id="u1"><div class="query-text">Question <strong>one</strong></div><button>Copy</button></user-query>' +
  '<model-response data-response-id="a1" data-model="gemini-synthetic">' +
  '<div class="markdown"><p>Answer <em>one</em>.</p><pre><code>const n = 1;</code></pre></div>' +
  '<button>Feedback</button><div hidden>Hidden thinking</div></model-response></main>';

function page(source = url, markup = html) {
  const dom = new JSDOM(markup, { url: source, runScripts: 'outside-only' });
  dom.window.TextEncoder = TextEncoder;
  dom.window.fetch = () => assert.fail('Gemini capture must not fetch');
  dom.window.scrollTo = () => assert.fail('Gemini capture must not scroll');
  dom.window.HTMLElement.prototype.scrollTo = () => assert.fail('Gemini capture must not scroll');
  return dom;
}

test('strict Gemini saved-conversation URL selection and identity casing', () => {
  assert.deepEqual(parseGeminiConversationUrl(url), { conversationId: 'AbCd1234efGh5678', sourceUrl: url });
  assert.equal(parseGeminiConversationUrl('https://gemini.google.com/u/2/app/AbCd1234efGh5678')?.conversationId, 'AbCd1234efGh5678');
  for (const bad of ['http://gemini.google.com/app/AbCd1234efGh5678',
    'https://gemini.google.com.evil.test/app/AbCd1234efGh5678',
    'https://user@gemini.google.com/app/AbCd1234efGh5678',
    'https://gemini.google.com:444/app/AbCd1234efGh5678',
    'https://gemini.google.com/', 'https://gemini.google.com/app',
    'https://gemini.google.com/share/AbCd1234efGh5678',
    url + '?temporary=true', url.replace('AbCd1234efGh5678','../../admin')]) {
    assert.equal(parseGeminiConversationUrl(bad), null, bad);
    assert.equal(resolveCaptureAdapter(bad), null, bad);
  }
  assert.equal(resolveCaptureAdapter(url)?.adapter.id, 'gemini');
});

test('serialized Gemini adapter captures only visible conversation turns without scrolling or network', () => {
  const { adapter, identity } = resolveCaptureAdapter(url);
  assert.equal(adapter.capture, captureGeminiConversation);
  assert.deepEqual(adapter.capabilities, { manualDom: true, manualHistory: false, automaticLoadedDom: false });
  const dom = page();
  try {
    const result = dom.window.eval('(' + adapter.capture.toString() + ')()');
    assert.equal(result.ok, true, result.error);
    const c = result.capture;
    assert.equal(c.provider, 'gemini');
    assert.equal(c.source_url, url);
    assert.equal(c.capture_method, 'manual-dom');
    assert.equal(c.completeness, 'partial');
    assert.equal(c.title, 'Design notes');
    assert.equal(c.messages.length, 2);
    assert.deepEqual(c.messages.map(m => m.id), ['gemini-user-u1', 'gemini-assistant-a1']);
    assert.equal(c.messages[0].text, 'Question **one**');
    assert.match(c.messages[1].text, /Answer \*one\*\./);
    assert.match(c.messages[1].text, /const n = 1/);
    assert.equal(c.messages[1].model, 'gemini-synthetic');
    assert.equal(c.messages.some(m => /Private sidebar|Hidden thinking|Feedback|Copy/.test(m.text)), false);
    assert.equal(captureMatchesIdentity(adapter, identity, c, false), true);
    assert.equal(captureMatchesIdentity(adapter, identity, c, true), false);
    assert.equal(dom.window.eval('(' + adapter.capture.toString() + ')({automatic:true})').ok, false);
    assert.equal(dom.window.eval('(' + adapter.capture.toString() + ')({loadHistory:true})').ok, false);
  } finally { dom.window.close(); }
});

test('Gemini page identity mismatch or unsupported layout never emits archive evidence', () => {
  for (const markup of [
    html.replace('data-conversation-id="AbCd1234efGh5678"', 'data-conversation-id="OtherConversationId"'),
    '<main><div>Homepage with other users</div></main>',
    '<main><user-query hidden>Unloaded turn</user-query></main>',
  ]) {
    const dom = page(url, markup);
    try { assert.equal(dom.window.eval('(' + captureGeminiConversation.toString() + ')()').ok, false); }
    finally { dom.window.close(); }
  }
});

test('Gemini regenerated answer changes revision evidence without rewriting previous observation', () => {
  const dom = page();
  try {
    const run = () => dom.window.eval('(' + captureGeminiConversation.toString() + ')()').capture;
    const original = run();
    const fixed = structuredClone(original);
    fixed.captured_at = '';
    dom.window.document.querySelector('.markdown p').textContent = 'Regenerated visible answer.';
    const regenerated = run();
    regenerated.captured_at = '';
    assert.equal(original.messages[1].id, regenerated.messages[1].id);
    assert.notDeepEqual(regenerated, fixed);
    assert.match(regenerated.messages[1].text, /Regenerated visible answer/);
    assert.match(original.messages[1].text, /Answer \*one\*/);
  } finally { dom.window.close(); }
});

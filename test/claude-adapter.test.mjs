import test from 'node:test';
import assert from 'node:assert/strict';
import { JSDOM } from 'jsdom';
import { resolveCaptureAdapter, captureAdapterForPage, captureMatchesIdentity } from '../extension/providers/index.mjs';

const id = '123e4567-e89b-12d3-a456-426614174000';
const source = 'https://claude.ai/chat/' + id;
const html = '<title>Example thread - Claude</title><main>'
  + '<div data-testid="user-message" data-message-id="u1"><p>Question</p></div>'
  + '<div data-testid="assistant-message" data-message-id="a1" data-branch-id="b2" data-regeneration-index="2" data-model="claude-test">'
  + '<p>Reply <strong>one</strong></p><button>Copy</button><div data-testid="thinking">hidden reasoning</div></div></main>';

test('Claude registry recognizes only exact saved routes and official origin', () => {
  const selected = resolveCaptureAdapter(source);
  assert.equal(selected.adapter.id, 'claude');
  assert.equal(selected.identity.conversationId, id);
  assert.equal(selected.adapter.capabilities.manualHistory, false);
  assert.equal(selected.adapter.capabilities.automaticLoadedDom, false);
  assert.equal(captureAdapterForPage('https://claude.ai/').id, 'claude');
  assert.equal(resolveCaptureAdapter('https://claude.ai/project/' + id + '/chat/' + id).identity.conversationId, id);
  for (const url of [
    source + '/other', source + '?temporary=1', source + '#fragment',
    source.replace('claude.ai','claude.ai.evil.test'), source.replace('https:','http:'),
    source.replace('claude.ai','alice@claude.ai'), source.replace('claude.ai','claude.ai:8888'),
    'https://claude.ai/project/nope/chat/' + id, 'https://claude.ai/', 'https://claude.ai/chat/not-uuid',
  ]) assert.equal(resolveCaptureAdapter(url), null, url);
});

test('self-contained Claude reader is loaded-only and preserves explicit observed model/branch metadata', () => {
  const { adapter, identity } = resolveCaptureAdapter(source);
  const dom = new JSDOM(html, { url: source, runScripts: 'outside-only' });
  dom.window.TextEncoder = TextEncoder;
  dom.window.fetch = () => assert.fail('provider network must not be accessed');
  dom.window.scrollTo = () => assert.fail('history must not be scrolled');
  try {
    const read = options => dom.window.eval('(' + adapter.capture.toString() + ')(' + JSON.stringify(options) + ')');
    const result = read({});
    assert.equal(result.ok, true, result.error);
    assert.equal(captureMatchesIdentity(adapter, identity, result.capture, false), true);
    assert.equal(result.capture.capture_method, 'manual-dom');
    assert.equal(result.capture.completeness, 'partial');
    assert.equal(result.capture.messages.length, 2);
    assert.equal(result.capture.messages[0].id, 'claude:user:u1');
    assert.equal(result.capture.messages[1].id, 'claude:assistant:a1:branch-b2:regen-2');
    assert.equal(result.capture.messages[1].model, 'claude-test');
    assert.equal(result.capture.messages[1].text, 'Reply one');
    assert.ok(!JSON.stringify(result).includes('hidden reasoning'));
    assert.equal(read({ automatic:true }).ok, false);
    assert.equal(read({ loadHistory:true }).ok, false);
    const privateTag = dom.window.document.createElement('button');
    privateTag.setAttribute('aria-pressed','true');
    privateTag.setAttribute('aria-label','Temporary Chat');
    dom.window.document.body.append(privateTag);
    assert.equal(read({}).ok, false);
  } finally { dom.window.close(); }
});

test('Claude refuses ambiguous messages and never invents absent provider metadata', () => {
  const { adapter } = resolveCaptureAdapter(source);
  const dom = new JSDOM('<title>Partial - Claude</title><main><div data-testid="user-message">Text</div></main>', { url: source, runScripts:'outside-only' });
  dom.window.TextEncoder = TextEncoder;
  try {
    const result = dom.window.eval('(' + adapter.capture.toString() + ')()');
    assert.equal(result.ok, true);
    assert.match(result.capture.warnings.join(' '), /positional/);
    assert.equal(result.capture.messages[0].model, undefined);
    assert.equal(result.capture.messages[0].authored_at, undefined);
    assert.equal(result.capture.messages[0].id, 'claude:user:rendered-0');
    dom.window.document.querySelector('main').innerHTML = '<div data-testid="assistant-message" data-message-id="same">One</div>'
      + '<div data-testid="assistant-message" data-message-id="same">Two</div>';
    assert.equal(dom.window.eval('(' + adapter.capture.toString() + ')()').ok, false);
  } finally { dom.window.close(); }
});

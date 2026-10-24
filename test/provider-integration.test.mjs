import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { JSDOM } from 'jsdom';
import { CAPTURE_ADAPTERS, resolveCaptureAdapter, captureMatchesIdentity } from '../extension/providers/index.mjs';
import { PROVIDER_CAPABILITIES } from '../extension/providers/capability-matrix.mjs';
import { AUTO_ORIGINS } from '../extension/automatic.mjs';

const chatFixture = JSON.parse(await readFile(new URL('./fixtures/capture/chatgpt-v1.json', import.meta.url), 'utf8'));
const chatHtml = await readFile(new URL('./fixtures/capture/chatgpt-v1.html', import.meta.url), 'utf8');
const claudeId = '123e4567-e89b-12d3-a456-426614174000';
const geminiId = 'AbCd1234efGh5678';
const cases = [
  { provider: 'chatgpt', url: chatFixture.source_url, html: chatHtml },
  {
    provider: 'claude',
    url: 'https://claude.ai/chat/' + claudeId,
    html: '<title>Sample - Claude</title><nav>PRIVATE SIDEBAR</nav><main>' +
      '<div data-testid="user-message" data-message-id="u1">A question</div>' +
      '<div data-testid="assistant-message" data-message-id="a1" data-branch-id="b2" data-regeneration-index="2">' +
      '<p>Claude answer</p><div data-testid="thinking">PRIVATE REASONING</div></div></main>',
  },
  {
    provider: 'gemini',
    url: 'https://gemini.google.com/app/' + geminiId,
    html: '<title>Sample - Gemini</title><nav>PRIVATE SIDEBAR</nav><main data-conversation-id="' + geminiId + '">' +
      '<user-query data-message-id="u1"><div class="query-text">A question</div></user-query>' +
      '<model-response data-response-id="a1"><div class="markdown">Gemini answer</div>' +
      '<div hidden>PRIVATE REASONING</div></model-response></main>',
  },
];

function isolatedPage(entry) {
  const dom = new JSDOM(entry.html, { url: entry.url, runScripts: 'outside-only' });
  dom.window.TextEncoder = TextEncoder;
  let network = 0;
  let scroll = 0;
  dom.window.fetch = () => { network++; throw Error('Provider API access is forbidden'); };
  dom.window.scrollTo = () => { scroll++; throw Error('Implicit scrolling is forbidden'); };
  dom.window.HTMLElement.prototype.scrollTo = () => { scroll++; throw Error('Implicit scrolling is forbidden'); };
  return { dom, metrics: () => ({ network, scroll }) };
}

function run(dom, adapter, options = {}) {
  return dom.window.eval('(' + adapter.capture.toString() + ')(' + JSON.stringify(options) + ')');
}

test('capability matrix is a frozen projection of bundled adapters, generic is file-only', async () => {
  assert.ok(Object.isFrozen(PROVIDER_CAPABILITIES));
  assert.deepEqual(CAPTURE_ADAPTERS.map(adapter => adapter.id), ['chatgpt', 'gemini', 'claude']);
  assert.deepEqual(PROVIDER_CAPABILITIES.map(entry => entry.provider), ['chatgpt', 'gemini', 'claude', 'generic']);
  const expectations = {
    chatgpt: ['https://chatgpt.com', true, true, true],
    gemini: ['https://gemini.google.com', true, false, false],
    claude: ['https://claude.ai', true, false, false],
  };
  for (const adapter of CAPTURE_ADAPTERS) {
    const entry = PROVIDER_CAPABILITIES.find(row => row.provider === adapter.id);
    assert.ok(Object.isFrozen(entry));
    assert.ok(Object.isFrozen(entry.pageOrigins));
    assert.ok(Object.isFrozen(entry.automaticOrigins));
    assert.deepEqual(
      [entry.pageOrigins[0], entry.manualLoadedDom, entry.manualHistory, entry.automaticLoadedDom],
      expectations[adapter.id],
    );
    assert.deepEqual(entry.pageOrigins, [...adapter.pageOrigins]);
    assert.deepEqual(entry.automaticOrigins, [...(adapter.automatic?.origins ?? [])]);
    assert.equal(entry.captureMethod, 'manual-dom');
    assert.equal(entry.surface, 'browser-active-tab');
  }
  const generic = PROVIDER_CAPABILITIES.at(-1);
  assert.equal(generic.surface, 'explicit-owner-file');
  assert.equal(generic.captureMethod, 'generic-import');
  assert.deepEqual(generic.formats, ['json', 'html', 'markdown']);
  assert.equal(generic.manualLoadedDom, false);
  assert.equal(generic.manualHistory, false);
  assert.equal(generic.automaticLoadedDom, false);
  assert.deepEqual(generic.pageOrigins, []);
  assert.deepEqual(generic.automaticOrigins, []);
  assert.ok(Object.isFrozen(generic.formats));
  assert.equal(resolveCaptureAdapter('mindplace-import://generic/' + 'a'.repeat(64)), null);
  const manifest = JSON.parse(await readFile(new URL('../extension/manifest.json', import.meta.url), 'utf8'));
  assert.deepEqual(manifest.permissions, ['activeTab', 'scripting', 'nativeMessaging', 'storage']);
  assert.equal(Object.hasOwn(manifest, 'host_permissions'), false);
  assert.equal(Object.hasOwn(manifest, 'content_scripts'), false);
  assert.deepEqual(AUTO_ORIGINS, ['https://chatgpt.com/*']);
  assert.deepEqual(manifest.optional_host_permissions, AUTO_ORIGINS);
});

test('cross-provider synthetic captures preserve partial provenance and never read outside visible loaded turns', () => {
  for (const entry of cases) {
    const selected = resolveCaptureAdapter(entry.url);
    assert.ok(selected, entry.provider);
    assert.equal(selected.adapter.id, entry.provider);
    const { dom, metrics } = isolatedPage(entry);
    try {
      const result = run(dom, selected.adapter);
      assert.equal(result.ok, true, entry.provider + ': ' + result.error);
      const capture = JSON.parse(JSON.stringify(result.capture));
      assert.equal(capture.provider, entry.provider);
      assert.equal(capture.source_url, entry.url);
      assert.equal(capture.conversation_id, selected.identity.conversationId);
      assert.equal(capture.capture_method, 'manual-dom');
      assert.equal(capture.completeness, 'partial');
      assert.ok(capture.warnings.length > 0);
      assert.ok(capture.messages.length >= 2);
      assert.equal(new Set(capture.messages.map(m => m.id)).size, capture.messages.length);
      assert.equal(captureMatchesIdentity(selected.adapter, selected.identity, capture, false), true);
      assert.ok(!JSON.stringify(capture).includes('PRIVATE SIDEBAR'));
      assert.ok(!JSON.stringify(capture).includes('PRIVATE REASONING'));
      assert.deepEqual(metrics(), { network: 0, scroll: 0 });
      if (entry.provider === 'claude') {
        assert.match(capture.messages[1].id, /branch-b2:regen-2/);
        assert.equal(capture.messages[1].model, undefined);
        assert.equal(capture.messages[1].authored_at, undefined);
      }
      if (entry.provider === 'gemini') {
        assert.equal(capture.messages[1].model, undefined);
        assert.equal(capture.messages[1].authored_at, undefined);
      }
    } finally { dom.window.close(); }
  }
});

test('browser admission never rewrites evidence or accepts cross-provider, mode or attachment spoofing', () => {
  for (const entry of cases) {
    const selected = resolveCaptureAdapter(entry.url);
    const { dom } = isolatedPage(entry);
    try {
      const output = run(dom, selected.adapter);
      assert.equal(output.ok, true);
      const capture = JSON.parse(JSON.stringify(output.capture));
      const before = structuredClone(capture);
      const other = cases.find(candidate => candidate.provider !== entry.provider);
      const replacements = [
        { provider: other.provider },
        { source_url: other.url },
        { conversation_id: 'invalid-mismatched-id' },
        { capture_method: 'generic-import' },
        { capture_method: 'legacy-import' },
        { completeness: 'complete' },
        { warnings: [] },
        { messages: [capture.messages[0], capture.messages[0]] },
        { messages: [{ ...capture.messages[0], attachments: [{ id: 'x', name: 'x', mime_type: 'text/plain', asset_path: '../private' }] }] },
      ];
      for (const patch of replacements) {
        assert.equal(captureMatchesIdentity(selected.adapter, selected.identity, { ...capture, ...patch }, false), false, entry.provider + ': ' + Object.keys(patch)[0]);
      }
      if (!selected.adapter.capabilities.automaticLoadedDom) {
        const fakeAuto = { ...capture, capture_method: 'automatic-dom' };
        assert.equal(captureMatchesIdentity(selected.adapter, selected.identity, fakeAuto, true), false);
        assert.equal(run(dom, selected.adapter, { automatic: true }).ok, false);
        assert.equal(run(dom, selected.adapter, { loadHistory: true }).ok, false);
      } else {
        assert.equal(run(dom, selected.adapter, { automatic: true, loadHistory: true }).ok, false);
      }
      assert.deepEqual(capture, before, 'admission must not rewrite original source');
    } finally { dom.window.close(); }
  }
});

test('hostile lookalikes and visibly active private sessions fail before any native submission', () => {
  for (const entry of cases) {
    const u = new URL(entry.url);
    for (const forged of [
      entry.url.replace('https:', 'http:'),
      entry.url.replace(u.hostname, u.hostname + '.attacker.example'),
      entry.url.replace(u.hostname, 'user@' + u.hostname),
      entry.url.replace(u.hostname, u.hostname + ':444'),
      'https://' + u.hostname + '/share/public-example',
    ]) assert.equal(resolveCaptureAdapter(forged), null, forged);

    const selected = resolveCaptureAdapter(entry.url);
    const { dom, metrics } = isolatedPage(entry);
    try {
      const marker = dom.window.document.createElement('button');
      marker.setAttribute('aria-label', 'Temporary Chat');
      marker.setAttribute('aria-pressed', 'true');
      marker.textContent = 'Temporary Chat';
      dom.window.document.body.prepend(marker);
      assert.equal(run(dom, selected.adapter).ok, false, entry.provider + ' active private');
      assert.deepEqual(metrics(), { network: 0, scroll: 0 });
    } finally { dom.window.close(); }
  }
  const gemini = cases.find(entry => entry.provider === 'gemini');
  const { dom } = isolatedPage(gemini);
  try {
    const marker = dom.window.document.createElement('button');
    marker.setAttribute('aria-label', 'Turn on Temporary Chat');
    marker.setAttribute('aria-pressed', 'true');
    dom.window.document.body.prepend(marker);
    assert.equal(run(dom, resolveCaptureAdapter(gemini.url).adapter).ok, true,
      'offer to enable private mode must not be treated as active');
    marker.setAttribute('aria-label', 'Private conversation');
    assert.equal(run(dom, resolveCaptureAdapter(gemini.url).adapter).ok, false);
    marker.hidden = true;
    assert.equal(run(dom, resolveCaptureAdapter(gemini.url).adapter).ok, true,
      'invisible stale marker must not block saved conversation');
  } finally { dom.window.close(); }
});

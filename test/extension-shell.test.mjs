import test from 'node:test';
import assert from 'node:assert/strict';
import { JSDOM } from 'jsdom';
import { captureConversation } from '../extension/capture.mjs';

const url = 'https://chatgpt.com/c/11111111-2222-4333-8444-555555555555';
const turnKey = 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee';

// Structure/attributes were inspected on the failing signed-in ChatGPT page.
// Authored text is replaced; no account transcript is committed as a test fixture.
function exchange({ id = turnKey, position = 0, unitAttribute = 'data-content-search-unit-key', mixed = false, providerIds = mixed, repeated = false } = {}) {
  return `<div data-turn-key="${id}"><div data-content-search-turn-key="fallback-turn-${position}">
    <div ${unitAttribute}="fallback-turn-${position}:0:user" ${mixed ? `data-message-author-role="user" ${providerIds ? 'data-message-id="provider-user"' : ''}` : ''}>
      <h4 class="sr-only">You said:</h4><div data-user-message-bubble><div class="text-size-chat whitespace-pre-wrap">${repeated ? 'Same question' : 'Keep this question in full.'}</div></div>
      <button>Copy message</button><div data-clf-user-text>Legacy presentation copy must not become raw evidence</div>
    </div>
    <div ${unitAttribute}="fallback-turn-${position}:2:assistant" ${mixed ? `data-message-author-role="assistant" ${providerIds ? 'data-message-id="provider-assistant"' : ''}` : ''}>
      <h4 class="sr-only">ChatGPT said:</h4><div><p>Worked for 23s</p><div data-markdown-text-style="assistant-message" class="MarkdownRoot-new-hash">
        <p>${repeated ? 'Same answer' : 'The real rendered answer.'}</p><pre><code class="language-rust">let saved = true;</code></pre>
        <div class="clf-stream">Duplicated legacy activity must not become raw evidence</div>
      </div><img src="https://example.test/generated.png" alt="generated image"><button>Sources</button></div>
    </div>
  </div></div>`;
}

function run(html) {
  const dom = new JSDOM(`<title>Mind Place Plan</title>${html}`, { url, runScripts: 'outside-only' });
  dom.window.TextEncoder = TextEncoder;
  try { return dom.window.eval(`(${captureConversation.toString()})()`); }
  finally { dom.window.close(); }
}

const shell = (content) => `<main data-app-shell-main-surface><div data-thread-find-target="conversation">${content}</div></main>`;

test('reads the live app-shell shape without legacy role attributes or hashed class dependencies', () => {
  const result = run(shell(exchange()));
  assert.equal(result.ok, true, result.error);
  assert.deepEqual(Array.from(result.capture.messages, (message) => message.role), ['user', 'assistant']);
  assert.equal(result.capture.messages[0].text, 'Keep this question in full.');
  assert.match(result.capture.messages[1].text, /The real rendered answer\.[\s\S]*```rust[\s\S]*let saved = true;/);
  assert.doesNotMatch(result.capture.messages.map((message) => message.text).join('\n'), /said:|Worked for|Copy message|Legacy|Sources/);
  assert.equal(result.capture.messages[1].attachments[0].source_url, 'https://example.test/generated.png');
  assert.equal(result.capture.completeness, 'partial');
});

test('native turn separator dates outside message units preserve original user timestamps', () => {
  const html = exchange().replace('<div data-content-search-turn-key="fallback-turn-0">', '<div data-content-search-turn-key="fallback-turn-0"><div role="separator"><time datetime="2026-09-03T20:52:20.094+07:00">Original chat date</time></div>');
  const result = run(shell(html));
  assert.equal(result.ok, true, result.error);
  assert.equal(result.capture.messages[0].authored_at, '2026-09-03T13:52:20.094Z');
  assert.equal(result.capture.messages[1].authored_at, undefined, 'a turn start must not invent an assistant authorship time');
  assert.notEqual(result.capture.messages[0].authored_at, result.capture.captured_at);
});

test('source conversation dates require an exact conversation identity on the active surface', () => {
  const result = run(`<main data-app-shell-main-surface data-conversation-id="11111111-2222-4333-8444-555555555555" data-conversation-created-at="2026-08-01T02:00:00Z" data-conversation-updated-at="2026-09-03T13:52:20Z"><div data-thread-find-target="conversation">${exchange()}</div></main>`);
  assert.equal(result.ok, true, result.error);
  assert.equal(result.capture.conversation_created_at, '2026-08-01T02:00:00.000Z');
  assert.equal(result.capture.conversation_updated_at, '2026-09-03T13:52:20.000Z');
  const other = run(`<main data-app-shell-main-surface data-conversation-id="other-chat" data-conversation-created-at="2026-08-01T02:00:00Z"><div data-thread-find-target="conversation">${exchange()}</div></main>`);
  assert.equal(other.capture.conversation_created_at, undefined);
});

test('mixed old/new attributes identify each authored message exactly once', () => {
  const result = run(shell(exchange({ mixed: true })));
  assert.equal(result.ok, true, result.error);
  assert.deepEqual(Array.from(result.capture.messages, (message) => message.id), ['provider-user', 'provider-assistant']);
});

test('mixed attributes without provider IDs retain native identities for repeated exchanges', () => {
  const first = run(shell(exchange({ mixed: true, providerIds: false, repeated: true })));
  const second = run(shell(exchange({ mixed: true, providerIds: false, repeated: true, id: 'other-native-turn' })
    + exchange({ mixed: true, providerIds: false, repeated: true, position: 8 })));
  assert.equal(first.ok, true, first.error); assert.equal(second.ok, true, second.error);
  assert.equal(second.capture.messages.length, 4);
  assert.equal(new Set(second.capture.messages.map((message) => message.id)).size, 4);
  assert.deepEqual(Array.from(second.capture.messages.slice(2), (message) => message.id), Array.from(first.capture.messages, (message) => message.id));
  assert.match(first.capture.warnings.join(' '), /native turn\/unit keys/);
});

test('native turn identity survives positional search-key renumbering and keeps repeated exchanges distinct', () => {
  const first = run(shell(exchange({ position: 0, repeated: true })));
  const second = run(shell(exchange({ id: 'bbbbbbbb-cccc-4ddd-8eee-ffffffffffff', position: 0, repeated: true }) + exchange({ position: 7, repeated: true })));
  assert.equal(first.ok, true, first.error); assert.equal(second.ok, true, second.error);
  assert.equal(second.capture.messages.length, 4);
  assert.equal(new Set(second.capture.messages.map((message) => message.id)).size, 4);
  assert.deepEqual(Array.from(second.capture.messages.slice(2), (message) => message.id), Array.from(first.capture.messages, (message) => message.id));
});

test('hidden retained pages and role-like sidebar content cannot be captured as the current conversation', () => {
  const result = run(`<div data-app-shell-page-surface style="display:none">${shell(exchange({ id: 'old-chat' }))}</div>
    <aside><div data-message-author-role="user">Sidebar must not be archived</div></aside>${shell(exchange())}`);
  assert.equal(result.ok, true, result.error);
  assert.equal(result.capture.messages.length, 2);
  assert.doesNotMatch(result.capture.messages.map((message) => message.text).join('\n'), /Sidebar/);
});

test('search-unit attribute variant uses the same role/content contract', () => {
  const result = run(shell(exchange({ unitAttribute: 'data-chatgpt-search-unit-key' })));
  assert.equal(result.ok, true, result.error);
  assert.deepEqual(Array.from(result.capture.messages, (message) => message.role), ['user', 'assistant']);
});

test('separate assistant units within one native exchange are retained with distinct identities', () => {
  const result = run(shell(`<div data-turn-key="${turnKey}">
    <div data-content-search-unit-key="fallback-turn-0:1:assistant"><div data-markdown-text-style="assistant-message">First authored part.</div></div>
    <div data-content-search-unit-key="fallback-turn-0:2:assistant"><div data-markdown-text-style="assistant-message">Second authored part.</div></div>
  </div>`));
  assert.equal(result.ok, true, result.error);
  assert.equal(result.capture.messages.length, 2);
  assert.notEqual(result.capture.messages[0].id, result.capture.messages[1].id);
});

test('classic conversation-turn role attributes work when explicit message-role nodes are absent', () => {
  const result = run('<main><section data-testid="conversation-turn-1" data-turn="user"><div class="whitespace-pre-wrap">Classic question</div></section><section data-testid="conversation-turn-2" data-turn="assistant"><div class="markdown">Classic answer</div></section></main>');
  assert.equal(result.ok, true, result.error);
  assert.deepEqual(Array.from(result.capture.messages, (message) => message.text), ['Classic question', 'Classic answer']);
});

import test from 'node:test';
import assert from 'node:assert/strict';
import { JSDOM } from 'jsdom';
import { captureConversation, parseChatGptConversationUrl } from '../extension/capture.mjs';

const conversationId = '123e4567-e89b-12d3-a456-426614174000';
const directUrl = `https://chatgpt.com/c/${conversationId}`;
const projectUrl = `https://chatgpt.com/g/project_workspace/c/${conversationId}`;

function runCapture(html, url = directUrl) {
  const dom = new JSDOM(html, { url, runScripts: 'outside-only' });
  dom.window.TextEncoder = TextEncoder;
  let remoteRequests = 0;
  dom.window.fetch = () => { remoteRequests += 1; throw new Error('remote access is forbidden in capture tests'); };
  const result = dom.window.eval(`(${captureConversation.toString()})()`);
  return { dom, result, get remoteRequests() { return remoteRequests; } };
}

test('accepts only the exact ChatGPT conversation routes in direct and project form', () => {
  assert.deepEqual(parseChatGptConversationUrl(directUrl), {
    conversationId,
    sourceUrl: directUrl,
  });
  assert.deepEqual(parseChatGptConversationUrl(projectUrl), {
    conversationId,
    sourceUrl: projectUrl,
  });
  for (const url of [
    'https://chatgpt.com/',
    'https://chatgpt.com/share/123',
    `https://chatgpt.com/c/${conversationId}/extra`,
    `https://evilchatgpt.com/c/${conversationId}`,
    `http://chatgpt.com/c/${conversationId}`,
    `https://chatgpt.com:444/c/${conversationId}`,
    `https://person@chatgpt.com/c/${conversationId}`,
    'https://chatgpt.com/c/not-a-uuid',
  ]) {
    assert.equal(parseChatGptConversationUrl(url), null, url);
  }
});

test('captures only rendered user and assistant messages with Markdown semantics and partial provenance', () => {
  const { result } = runCapture(`
    <title>Example conversation - ChatGPT</title>
    <main>
      <div data-message-author-role="user" data-message-id="user-identity-1">
        <p>Please inspect this <a href="https://example.test/docs">guide</a>.</p>
      </div>
      <div data-message-author-role="assistant" data-message-id="assistant-identity-1">
        <p>Here is code:</p>
        <pre><code class="language-js">const answer = 42;\nconsole.log(answer);</code></pre>
        <p>Inline <code>const id = 1;</code> and <code>tick \`mark\`</code>.</p>
        <p>Math: <span class="katex"><annotation encoding="application/x-tex">x^2 + y^2</annotation></span></p>
        <p><strong>Rendered answer</strong></p>
      </div>
    </main>`);

  assert.equal(result.ok, true);
  const capture = result.capture;
  assert.equal(capture.schema_version, 1);
  assert.equal(capture.provider, 'chatgpt');
  assert.equal(capture.conversation_id, conversationId);
  assert.equal(capture.source_url, directUrl);
  assert.equal(capture.capture_method, 'manual-dom');
  assert.equal(capture.completeness, 'partial');
  assert.ok(capture.warnings.some((warning) => /currently rendered|loaded in the page/i.test(warning)));
  assert.equal(capture.messages.length, 2);
  assert.equal(capture.messages[0].id, 'user-identity-1');
  assert.equal(capture.messages[0].role, 'user');
  assert.equal(capture.messages[0].format, 'markdown');
  assert.match(capture.messages[0].text, /\[guide\]\(https:\/\/example\.test\/docs\)/);
  assert.equal(capture.messages[1].id, 'assistant-identity-1');
  assert.match(capture.messages[1].text, /```js[\s\S]*const answer = 42;[\s\S]*```/);
  assert.match(capture.messages[1].text, /`const id = 1;`/);
  assert.match(capture.messages[1].text, /``tick `mark```/);
  assert.match(capture.messages[1].text, /\$x\^2 \+ y\^2\$/);
  assert.match(capture.messages[1].text, /\*\*Rendered answer\*\*/);
});

test('retains image references without fetching remote media and inlines only small safe image data', () => {
  const { result, remoteRequests } = runCapture(`
    <main><div data-message-author-role="assistant">
      <p>See the picture.</p>
      <img src="https://files.example.test/image.png?token=short" alt="reference image">
      <img src="data:image/png;base64,iVBORw0KGgo=" alt="small inline image">
      <img src="data:image/svg+xml;base64,PHN2Zy8+" alt="unsafe inline image">
      <img src="data:image/png;base64,not-base64!" alt="malformed inline image">
      <a download="plan.pdf" href="https://files.example.test/plan.pdf">plan.pdf</a>
    </div></main>`);

  assert.equal(result.ok, true);
  const attachments = result.capture.messages[0].attachments;
  assert.equal(attachments.length, 5);
  assert.equal(attachments[0].source_url, 'https://files.example.test/image.png?token=short');
  assert.equal(Object.hasOwn(attachments[0], 'data_url'), false);
  assert.equal(attachments[1].mime_type, 'image/png');
  assert.equal(attachments[1].data_url, 'data:image/png;base64,iVBORw0KGgo=');
  assert.equal(attachments[2].data_url, undefined);
  assert.match(attachments[2].unavailable_reason, /not supported|unsafe/i);
  assert.equal(attachments[3].data_url, undefined);
  assert.match(attachments[3].unavailable_reason, /malformed/i);
  assert.equal(attachments[4].name, 'plan.pdf');
  assert.equal(attachments[4].mime_type, 'application/pdf');
  assert.equal(attachments[4].source_url, 'https://files.example.test/plan.pdf');
  assert.equal(remoteRequests, 0);
});

test('keeps unsupported AVIF as an unavailable reference while retaining conversation text', () => {
  const { result } = runCapture(`
    <main><div data-message-author-role="assistant">
      <p>The answer remains available.</p>
      <img src="data:image/avif;base64,AAAA" alt="AVIF attachment">
    </div></main>`);

  assert.equal(result.ok, true);
  const message = result.capture.messages[0];
  assert.match(message.text, /^The answer remains available\./);
  assert.equal(message.attachments[0].mime_type, 'image/avif');
  assert.match(message.attachments[0].unavailable_reason, /not supported/i);
  assert.equal(Object.hasOwn(message.attachments[0], 'data_url'), false);
  assert.equal(Object.hasOwn(message.attachments[0], 'source_url'), false);
  assert.ok(result.capture.warnings.some((warning) => /could not be copied/i.test(warning)));
});

test('fallback message identities remain stable when more earlier messages are rendered', () => {
  const { result: first } = runCapture(`
    <main><div data-message-author-role="assistant">The same answer</div></main>`);
  const { result: second } = runCapture(`
    <main>
      <div data-message-author-role="user">An earlier question</div>
      <div data-message-author-role="assistant">The same answer</div>
    </main>`);

  assert.equal(first.ok, true);
  assert.equal(second.ok, true);
  assert.equal(first.capture.messages[0].id, second.capture.messages[1].id);
});

test('rejects non-conversation and active Temporary Chat pages before producing a capture', () => {
  const page = '<main><div data-message-author-role="user">Hello</div></main>';
  const ordinary = runCapture(page, 'https://chatgpt.com/');
  assert.equal(ordinary.result.ok, false);
  assert.match(ordinary.result.error, /conversation/i);

  const temporary = runCapture(`
    <header><button aria-label="Temporary Chat" aria-pressed="true">Temporary Chat</button></header>
    ${page}`);
  assert.equal(temporary.result.ok, false);
  assert.match(temporary.result.error, /temporary/i);

  const urlMarkedTemporary = runCapture(page, `${directUrl}?temporary-chat=true`);
  assert.equal(urlMarkedTemporary.result.ok, false);
  assert.match(urlMarkedTemporary.result.error, /temporary/i);
});

test('does not treat a hidden inactive Temporary Chat option as an active private chat', () => {
  const { result } = runCapture(`
    <header>
      <button hidden aria-hidden="true">Turn on Temporary Chat</button>
      <div role="menu" hidden><div role="menuitem">Temporary Chat</div></div>
      <button>Turn on Temporary Chat</button>
    </header>
    <main><div data-message-author-role="user">Visible conversation</div></main>`);

  assert.equal(result.ok, true);
  assert.equal(result.capture.messages[0].text, 'Visible conversation');
});

test('retains an 80 KB rendered answer in full rather than silently truncating it', () => {
  const longText = 'x'.repeat(80_000);
  const { result } = runCapture(`<main><div data-message-author-role="assistant">${longText}</div></main>`);

  assert.equal(result.ok, true);
  assert.equal(result.capture.messages[0].text, longText);
});

test('fails clearly when one rendered message exceeds the 4 MiB cap', () => {
  const oversizedText = 'x'.repeat(4 * 1024 * 1024 + 1);
  const { result } = runCapture(`<main><div data-message-author-role="assistant">${oversizedText}</div></main>`);

  assert.equal(result.ok, false);
  assert.match(result.error, /4\s*MiB|4\s*MB|4\s*megabyte/i);
  assert.match(result.error, /nothing was saved/i);
});

test('fails clearly instead of silently dropping messages past the 10,000 message cap', () => {
  const messages = Array.from({ length: 10_001 }, (_, index) =>
    `<div data-message-author-role="user" data-message-id="u-${index}">Question ${index}</div>`).join('');
  const { result } = runCapture(`<main>${messages}</main>`);

  assert.equal(result.ok, false);
  assert.match(result.error, /10,000 rendered messages/i);
  assert.match(result.error, /nothing was saved/i);
});

test('fails clearly when complete rendered text exceeds the whole-capture JSON cap', () => {
  const largeAnswer = 'x'.repeat(4 * 1024 * 1024);
  const messages = Array.from({ length: 8 }, (_, index) =>
    `<div data-message-author-role="assistant" data-message-id="a-${index}">${largeAnswer}</div>`).join('');
  const { result } = runCapture(`<main>${messages}</main>`);

  assert.equal(result.ok, false);
  assert.match(result.error, /31 MiB .* limit/i);
  assert.match(result.error, /nothing was saved/i);
});

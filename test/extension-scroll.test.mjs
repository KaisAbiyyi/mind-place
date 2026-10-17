import test from 'node:test';
import assert from 'node:assert/strict';
import { JSDOM } from 'jsdom';
import { captureConversation } from '../extension/capture.mjs';

const url = 'https://chatgpt.com/c/123e4567-e89b-12d3-a456-426614174000';
const options = { loadHistory: true, operationId: 'scroll-test', pollIntervalMs: 10, settleMs: 60, maxDurationMs: 3000 };

// Models a scroll container that replaces its DOM window after asynchronous loading.
function virtualPage({ count = 12, delayMs = 20, onScroll, missingIds = false, missingTurnIds = false, startPosition, media = false, shellLayout = false, mixedRole = false, layoutDelayMs = 0, logicalClock = false } = {}) {
  const dom = new JSDOM('<title>Long history - ChatGPT</title><main><section id="scroller" style="overflow-y:auto"><div id="turns"></div></section></main>', { url, runScripts: 'outside-only' });
  dom.window.TextEncoder = TextEncoder;
  if (logicalClock) {
    // Isolate deadline behavior from jsdom's synchronous CSS cost. Real pacing
    // remains checked by the nine-second test and the Chromium/native fixture.
    let now = 0;
    const schedule = dom.window.setTimeout.bind(dom.window);
    Object.defineProperty(dom.window.performance, 'now', { value: () => now });
    dom.window.setTimeout = (callback, delay = 0, ...args) => {
      const due = now + delay;
      return schedule(() => { now = Math.max(now, due); callback(...args); }, delay);
    };
  }
  let position = startPosition ?? (count - 3) * 100;
  let moves = 0;
  let networkRequests = 0;
  const scroller = dom.window.document.querySelector('#scroller');
  const mountedAt = performance.now();
  if (shellLayout) {
    dom.window.document.querySelector('main').setAttribute('data-app-shell-main-surface', '');
    scroller.setAttribute('data-thread-find-target', 'conversation');
  }
  const turns = dom.window.document.querySelector('#turns');
  const render = () => {
    const start = Math.min(count - 3, Math.max(0, Math.floor(position / 100)));
    turns.innerHTML = Array.from({ length: 3 }, (_, offset) => {
      const index = start + offset;
      const images = media ? Array.from({ length: 5 }, (_, image) => `<img alt="image ${image}" src="https://example.test/${index}-${image}.png">`).join('') : '';
      if (shellLayout) {
        const role = index % 2 ? 'assistant' : 'user';
        const explicit = `data-message-author-role="${role}"`;
        return `<div data-turn-key="native-turn-${index}"><div data-content-search-unit-key="fallback-turn-${start}:${index % 3}:${role}" ${mixedRole === 'unit' ? explicit : ''}><div ${mixedRole === 'nested' ? explicit : ''} ${index % 2 ? 'data-markdown-text-style="assistant-message"' : 'data-user-message-bubble'}>${missingIds ? 'Repeated text' : `Turn ${index}`}</div></div></div>`;
      }
      return `<article ${missingTurnIds ? '' : `data-testid="conversation-turn-${index}"`}><div data-message-author-role="${index % 2 ? 'assistant' : 'user'}" ${missingIds ? '' : `data-message-id="message-${index}"`}>${missingIds ? 'Repeated text' : `Turn ${index}`}${images}</div></article>`;
    }).join('');
    for (const element of turns.querySelectorAll('[data-message-author-role],[data-content-search-unit-key]')) {
      const index = Number(element.closest('[data-turn-key]')?.getAttribute('data-turn-key').match(/\d+$/)?.[0]
        ?? element.closest('article')?.getAttribute('data-testid')?.match(/\d+$/)?.[0] ?? 0);
      element.getBoundingClientRect = () => ({ top: index * 100 - position, bottom: index * 100 - position + 100, height: 100, width: 500 });
    }
  };
  scroller.getBoundingClientRect = () => ({ top: 0, bottom: 300, height: 300, width: 500 });
  Object.defineProperties(scroller, {
    clientHeight: { get: () => performance.now() - mountedAt >= layoutDelayMs ? 300 : 0 },
    scrollHeight: { get: () => count * 100 },
    scrollTop: { get: () => position, set: (value) => {
      position = Math.max(0, Math.min((count - 3) * 100, value));
      moves += 1;
      onScroll?.({ dom, scroller, moves });
      dom.window.setTimeout(render, delayMs);
    } },
  });
  scroller.scrollTo = ({ top }) => { scroller.scrollTop = top; };
  dom.window.fetch = () => { networkRequests += 1; throw new Error('capture must not fetch provider data'); };
  render();
  const run = (overrides = {}) => dom.window.eval(`(${captureConversation.toString()})(${JSON.stringify({ ...options, ...overrides })})`);
  return { dom, scroller, turns, run, get moves() { return moves; }, get networkRequests() { return networkRequests; } };
}

test('manual scrolling accumulates virtualized older and newer turns in chronological order', async () => {
  const page = virtualPage();
  try {
    const result = await page.run();
    assert.equal(result.ok, true, result.error);
    assert.deepEqual(Array.from(result.capture.messages, (message) => message.text), Array.from({ length: 12 }, (_, index) => `Turn ${index}`));
    assert.equal(new Set(result.capture.messages.map((message) => message.id)).size, 12);
    assert.equal(result.capture.completeness, 'partial');
    assert.match(result.capture.warnings.join(' '), /scroll/i);
    assert.equal(page.scroller.scrollTop, 900);
    assert.equal(page.networkRequests, 0);
    assert.ok(page.moves > 5);
  } finally { page.dom.window.close(); }
});

test('scrolling preserves distinct repeated messages with stable conversation turn identities', async () => {
  const page = virtualPage({ missingIds: true });
  try {
    const result = await page.run();
    assert.equal(result.ok, true, result.error);
    assert.equal(result.capture.messages.length, 12);
    assert.equal(new Set(result.capture.messages.map((message) => message.id)).size, 12);
  } finally { page.dom.window.close(); }
});

test('Stop and save returns collected partial history and restores the scroll position', async () => {
  const page = virtualPage({ onScroll: ({ dom, moves }) => {
    if (moves === 2) dom.window.document.querySelector('[data-mind-place-stop]')?.click();
  } });
  try {
    const result = await page.run();
    assert.equal(result.ok, true, result.error);
    assert.ok(result.capture.messages.length >= 3);
    assert.match(result.capture.warnings.join(' '), /stopped|stop.*save/i);
    assert.equal(page.scroller.scrollTop, 900);
  } finally { page.dom.window.close(); }
});

test('Cancel discards the pending capture and restores the scroll position', async () => {
  const page = virtualPage({ onScroll: ({ dom, moves }) => {
    if (moves === 1) dom.window.document.querySelector('[data-mind-place-cancel]')?.click();
  } });
  try {
    const result = await page.run();
    assert.equal(result.ok, false);
    assert.match(result.error, /cancel/i);
    assert.equal(page.scroller.scrollTop, 900);
  } finally { page.dom.window.close(); }
});

test('navigation during loading aborts before returning any archive payload', async () => {
  const page = virtualPage({ onScroll: ({ dom, moves }) => {
    if (moves === 1) dom.reconfigure({ url: url.replace('123e4567', '223e4567') });
  } });
  try {
    const result = await page.run();
    assert.equal(result.ok, false);
    assert.match(result.error, /changed|navigat/i);
    assert.equal(result.capture, undefined);
    assert.equal(page.moves, 1, 'must not restore/scroll a different conversation');
  } finally { page.dom.window.close(); }
});

test('time budget stops loading with an explicit partial warning instead of running indefinitely', async () => {
  const page = virtualPage({ count: 200 });
  try {
    const result = await page.run({ maxDurationMs: 80 });
    assert.equal(result.ok, true, result.error);
    assert.ok(result.capture.messages.length < 200);
    assert.match(result.capture.warnings.join(' '), /time|budget|limit/i);
    assert.equal(page.scroller.scrollTop, 19700);
  } finally { page.dom.window.close(); }
});

test('ordinary quick capture never scrolls the page', () => {
  const page = virtualPage();
  try {
    const result = page.dom.window.eval(`(${captureConversation.toString()})()`);
    assert.equal(result.ok, true);
    assert.equal(result.capture.messages.length, 3);
    assert.equal(page.moves, 0);
  } finally { page.dom.window.close(); }
});

test('a newly opened conversation waits for its native scroll container to mount', async () => {
  const page = virtualPage({ shellLayout: true });
  const parent = page.scroller.parentElement;
  page.scroller.remove();
  page.dom.window.setTimeout(() => parent.append(page.scroller), 90);
  try {
    const result = await page.run();
    assert.equal(result.ok, true, result.error);
    assert.equal(result.capture.messages.length, 12);
    assert.ok(page.moves > 3);
  } finally { page.dom.window.close(); }
});

test('an initially zero-height conversation waits for layout instead of locking onto document scrolling', async () => {
  const page = virtualPage({ shellLayout: true, layoutDelayMs: 90 });
  try {
    const result = await page.run();
    assert.equal(result.ok, true, result.error);
    assert.equal(result.capture.messages.length, 12);
    assert.ok(page.moves > 3);
  } finally { page.dom.window.close(); }
});

test('Stop and save during layout readiness keeps the currently readable messages without scrolling', async () => {
  const page = virtualPage({ shellLayout: true, layoutDelayMs: 500 });
  page.dom.window.setTimeout(() => page.dom.window.document.querySelector('[data-mind-place-stop]')?.click(), 50);
  try {
    const result = await page.run();
    assert.equal(result.ok, true, result.error);
    assert.equal(result.capture.messages.length, 3);
    assert.equal(result.capture.completeness, 'partial');
    assert.match(result.capture.warnings.join(' '), /stopped.*scroll area|scroll area.*stopped/i);
    assert.equal(page.moves, 0);
  } finally { page.dom.window.close(); }
});

test('initial ChatGPT scroll restoration settles before the upward history sweep starts', async () => {
  const page = virtualPage({ count: 24, shellLayout: true, startPosition: 600, onScroll: ({ dom, scroller, moves }) => {
    if (moves === 1) dom.window.setTimeout(() => { scroller.scrollTop = 600; }, 400);
  } });
  try {
    const result = await page.run({ pollIntervalMs: undefined, settleMs: undefined, maxDurationMs: 8000 });
    assert.equal(result.ok, true, result.error);
    assert.deepEqual(Array.from(result.capture.messages, (message) => message.text), Array.from({ length: 24 }, (_, index) => `Turn ${index}`));
    assert.equal(page.scroller.scrollTop, 600);
  } finally { page.dom.window.close(); }
});

test('app-shell history uses its actual nested scroll container and retains virtualized turns', async () => {
  const page = virtualPage({ shellLayout: true });
  try {
    const result = await page.run();
    assert.equal(result.ok, true, result.error);
    assert.deepEqual(Array.from(result.capture.messages, (message) => message.text), Array.from({ length: 12 }, (_, index) => `Turn ${index}`));
    assert.equal(new Set(result.capture.messages.map((message) => message.id)).size, 12);
    assert.equal(page.scroller.scrollTop, 900);
    assert.ok(page.moves > 5);
  } finally { page.dom.window.close(); }
});

test('scroll capture goes to the newest boundary first, then sweeps upward once', async () => {
  const targets = [];
  const page = virtualPage({ count: 24, startPosition: 1000, onScroll: ({ scroller }) => targets.push(scroller.scrollTop) });
  try {
    const result = await page.run();
    assert.equal(result.ok, true, result.error);
    assert.equal(targets[0], 2100, 'the first move must load the newest boundary');
    const sweep = targets.slice(0, -1); // The final move restores the original reading position.
    assert.ok(sweep.every((top, index) => index === 0 || top <= sweep[index - 1]), 'the fixed-height sweep must move only upward after the bottom jump');
    assert.deepEqual(Array.from(result.capture.messages, (message) => message.text), Array.from({ length: 24 }, (_, index) => `Turn ${index}`));
    assert.equal(page.scroller.scrollTop, 1000);
  } finally { page.dom.window.close(); }
});

test('production scroll pacing collects a long virtualized fixture within a short bounded budget', async () => {
  const page = virtualPage({ count: 90, shellLayout: true });
  try {
    const result = await page.run({ pollIntervalMs: undefined, settleMs: undefined, maxDurationMs: 9000 });
    assert.equal(result.ok, true, result.error);
    assert.equal(result.capture.messages.length, 90, 'default pacing should finish this delayed fixture before the nine-second budget');
    assert.deepEqual(Array.from(result.capture.messages, (message) => message.text), Array.from({ length: 90 }, (_, index) => `Turn ${index}`));
    assert.doesNotMatch(result.capture.warnings.join(' '), /time budget was reached/);
    assert.equal(page.networkRequests, 0);
  } finally { page.dom.window.close(); }
});

test('fast pacing waits for a delayed virtualized DOM window even without a loading indicator', async () => {
  const page = virtualPage({ count: 12, shellLayout: true, delayMs: 220 });
  try {
    const result = await page.run({ pollIntervalMs: undefined, settleMs: undefined, maxDurationMs: 5000 });
    assert.equal(result.ok, true, result.error);
    assert.deepEqual(Array.from(result.capture.messages, (message) => message.text), Array.from({ length: 12 }, (_, index) => `Turn ${index}`));
    assert.equal(page.scroller.scrollTop, 900);
  } finally { page.dom.window.close(); }
});

for (const startPosition of [600, 850]) {
  test(`a delayed bottom jump from ${startPosition} retains the newest messages before sweeping upward`, async () => {
    const page = virtualPage({ count: 12, shellLayout: true, delayMs: 400, startPosition });
    try {
      const result = await page.run({ pollIntervalMs: undefined, settleMs: undefined, maxDurationMs: 7000 });
      assert.equal(result.ok, true, result.error);
      assert.deepEqual(Array.from(result.capture.messages, (message) => message.text), Array.from({ length: 12 }, (_, index) => `Turn ${index}`));
      assert.doesNotMatch(result.capture.warnings.join(' '), /lagged behind/);
      assert.equal(page.scroller.scrollTop, startPosition);
    } finally { page.dom.window.close(); }
  });
}

for (const mixedRole of ['unit', 'nested']) {
  test(`mixed ${mixedRole} role attributes preserve repeated virtualized messages without provider IDs`, async () => {
    const page = virtualPage({ shellLayout: true, mixedRole, missingIds: true });
    try {
      const result = await page.run();
      assert.equal(result.ok, true, result.error);
      assert.equal(result.capture.messages.length, 12);
      assert.equal(new Set(result.capture.messages.map((message) => message.id)).size, 12);
      assert.deepEqual(Array.from(result.capture.messages, (message) => message.id), Array.from({ length: 12 }, (_, index) => `native-turn-native-turn-${index}-${index % 3}-${index % 2 ? 'assistant' : 'user'}`));
      assert.equal(page.scroller.scrollTop, 900);
    } finally { page.dom.window.close(); }
  });
}

test('waits for a visible history loader before deciding that the oldest boundary is stable', async () => {
  const page = virtualPage({ count: 8 });
  let loaded = false;
  let requested = false;
  const originalScroll = page.scroller.scrollTo;
  page.scroller.scrollTo = ({ top }) => {
    originalScroll({ top });
    if (top !== 0 || loaded || requested) return;
    requested = true;
    const loader = page.dom.window.document.createElement('div');
    loader.setAttribute('role', 'progressbar');
    page.dom.window.document.querySelector('main').append(loader);
    page.dom.window.setTimeout(() => {
      loaded = true;
      loader.remove();
      const older = page.dom.window.document.createElement('div');
      older.dataset.messageAuthorRole = 'user';
      older.dataset.messageId = 'older-lazy-message';
      older.textContent = 'Older history loaded after the short quiet interval';
      page.turns.prepend(older);
    }, 130);
  };
  try {
    const result = await page.run();
    assert.equal(result.ok, true, result.error);
    assert.ok(result.capture.messages.some((message) => message.id === 'older-lazy-message'));
  } finally { page.dom.window.close(); }
});

test('Temporary Chat becoming active during scrolling discards every collected message', async () => {
  const page = virtualPage({ onScroll: ({ dom, moves }) => {
    if (moves !== 1) return;
    const marker = dom.window.document.createElement('button');
    marker.setAttribute('aria-label', 'Temporary Chat');
    marker.setAttribute('aria-pressed', 'true');
    dom.window.document.body.append(marker);
  } });
  try {
    const result = await page.run();
    assert.equal(result.ok, false);
    assert.match(result.error, /temporary/i);
    assert.equal(result.capture, undefined);
  } finally { page.dom.window.close(); }
});

test('a capture started at the top loads the newest boundary before its simulated 100ms deadline', async () => {
  const targets = [];
  const page = virtualPage({ count: 200, startPosition: 0, logicalClock: true, onScroll: ({ scroller }) => targets.push(scroller.scrollTop) });
  try {
    const result = await page.run({ maxDurationMs: 100 });
    assert.equal(result.ok, true, result.error);
    assert.equal(targets[0], 19700);
    assert.ok(result.capture.messages.some((message) => message.id === 'message-199'));
    assert.match(result.capture.warnings.join(' '), /time budget/i);
  } finally { page.dom.window.close(); }
});

test('a capture started in the middle collects both older and newer turns without losing their order', async () => {
  const page = virtualPage({ count: 24, startPosition: 1000 });
  try {
    const result = await page.run();
    assert.equal(result.ok, true, result.error);
    assert.deepEqual(Array.from(result.capture.messages, (message) => message.id), Array.from({ length: 24 }, (_, index) => `message-${index}`));
    assert.equal(page.scroller.scrollTop, 1000);
  } finally { page.dom.window.close(); }
});

test('the attachment reference budget applies across all collected windows without cutting message text', async () => {
  const page = virtualPage({ count: 30, media: true });
  try {
    const result = await page.run({ maxDurationMs: 6000 });
    assert.equal(result.ok, true, result.error);
    assert.equal(result.capture.messages.length, 30);
    assert.equal(result.capture.messages.reduce((total, message) => total + message.attachments.length, 0), 120);
    assert.match(result.capture.warnings.join(' '), /120.*collected|collected.*120/i);
  } finally { page.dom.window.close(); }
});

test('unidentifiable repeated virtualized turns keep an explicit identity and ordering limitation', async () => {
  const page = virtualPage({ missingIds: true, missingTurnIds: true });
  try {
    const result = await page.run();
    assert.equal(result.ok, true, result.error);
    assert.equal(result.capture.completeness, 'partial');
    assert.match(result.capture.warnings.join(' '), /lack stable.*repeated identical turns.*order cannot be verified/i);
  } finally { page.dom.window.close(); }
});

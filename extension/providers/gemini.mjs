// MP-7C: bounded, opt-in, currently rendered Gemini conversation capture.
// No provider API, remote media requests, history scrolling, or persistent observer.
export function parseGeminiConversationUrl(value) {
  try {
    const url = new URL(value);
    if (url.origin !== 'https://gemini.google.com' || url.username || url.password) return null;
    const match = url.pathname.match(/^\/(?:u\/[0-9]{1,3}\/)?app\/([A-Za-z0-9_-]{8,128})$/);
    if (!match) return null;
    if ([...url.searchParams].some(([key, value]) =>
      /temporary|incognito|private/i.test(key) && !/^(false|0|no)$/i.test(value))) return null;
    return { conversationId: match[1], sourceUrl: url.href };
  } catch { return null; }
}

// Must remain self-contained: Chrome serializes this function into the tab's main document.
export function captureGeminiConversation(options = {}) {
  const fail = error => ({ ok: false, error });
  if (options.automatic || options.loadHistory) return fail('Gemini supports explicit loaded-only capture; automatic saving and history scrolling are not enabled.');
  if (globalThis.__mindPlaceManualScroll) return fail('A manual capture is already in progress.');
  if (!globalThis.document || !globalThis.location) return fail('No readable Gemini conversation is active.');
  const sourceUrl = location.href;
  let conversationId;
  try {
    const url = new URL(sourceUrl);
    if (url.origin !== 'https://gemini.google.com' || url.username || url.password) return fail('Open a saved Gemini conversation.');
    const match = url.pathname.match(/^\/(?:u\/[0-9]{1,3}\/)?app\/([A-Za-z0-9_-]{8,128})$/);
    if (!match) return fail('Open a saved Gemini conversation, not a new chat or shared link.');
    if ([...url.searchParams].some(([key, value]) =>
      /temporary|incognito|private/i.test(key) && !/^(false|0|no)$/i.test(value))) {
      return fail('Temporary or private conversations are excluded.');
    }
    conversationId = match[1];
  } catch { return fail('Invalid Gemini conversation URL.'); }
  const encoder = new TextEncoder();
  const byteLength = value => encoder.encode(value).length;
  const visible = node => {
    for (let item = node; item?.nodeType === 1; item = item.parentElement) {
      if (item.hidden || item.hasAttribute('inert') || item.getAttribute('aria-hidden') === 'true') return false;
      const style = globalThis.getComputedStyle?.(item);
      if (style?.display === 'none' || style?.visibility === 'hidden' || style?.visibility === 'collapse') return false;
    }
    return true;
  };
  // Only a positively active, visible private-session marker blocks capture.
  // Mere prompts to enable a private mode are not proof that it is active.
  const privateUi = () => [...document.querySelectorAll(
    '[aria-pressed="true"],[aria-checked="true"],[aria-current="true"],[data-state="active"],[role="status"],[role="alert"]',
  )].some(element => {
    if (!visible(element)) return false;
    const hint = [
      element.getAttribute('aria-label'), element.getAttribute('title'),
      element.getAttribute('data-testid'), (element.textContent ?? '').trim().slice(0, 120),
    ].join(' ');
    return /\b(?:temporary\s+(?:chat|conversation)|private\s+(?:chat|conversation)|incognito)\b/i.test(hint)
      && !/\b(?:off|disabled|not active|turn on|enable)\b/i.test(hint);
  });
  if (privateUi()) return fail('Private or temporary Gemini conversations are excluded.');
  // Never harvest the entire body/sidebar: require an explicit conversation surface.
  const root = [...document.querySelectorAll('main,[role="main"],infinite-scroller,.conversation-container')]
    .find(element => visible(element) && element.querySelector('user-query,model-response'));
  if (!root) return fail('No Gemini conversation turns were found in the visible conversation surface.');
  const identityOwner = root.closest('[data-conversation-id]')?.getAttribute('data-conversation-id')
    ?? root.getAttribute('data-conversation-id');
  if (identityOwner && identityOwner !== conversationId) return fail('The rendered conversation identity does not match the URL.');
  const warnings = [
    'Only currently rendered Gemini turns were captured; history, other response variants and unloaded content may be missing.',
    'Gemini media, tool results and hidden reasoning are not downloaded or inferred from the page.',
  ];
  const markup = node => {
    const read = current => {
      if (current.nodeType === 3) return current.nodeValue ?? '';
      if (current.nodeType !== 1 || !visible(current)) return '';
      const el = current;
      const tag = el.tagName.toLowerCase();
      if (['script', 'style', 'noscript', 'button', 'input', 'textarea', 'select', 'svg', 'nav', 'aside'].includes(tag)
        || el.matches('[contenteditable="true"],[data-mind-place-capture],.thoughts,.model-thoughts,[data-test-id*="thought" i],[aria-label*="thinking" i]')) return '';
      if (tag === 'br') return '\n';
      if (tag === 'img') return '[Image]';
      if (tag === 'pre') {
        const source = (el.querySelector('code')?.textContent ?? el.textContent ?? '').trimEnd();
        return '\n\n~~~\n' + source + '\n~~~\n\n';
      }
      const children = [...el.childNodes].map(read).join('');
      if (tag === 'a') {
        const href = el.getAttribute('href');
        try {
          const link = new URL(href ?? '', sourceUrl);
          if (href && ['https:', 'http:'].includes(link.protocol) && link.href.length <= 2048) {
            return '[' + (children.trim() || 'Link') + '](' + link.href.replace(/\)/g, '%29') + ')';
          }
        } catch { /* Omit unsafe link target. */ }
        return children;
      }
      if (tag === 'strong' || tag === 'b') return children.trim() ? '**' + children.trim() + '**' : '';
      if (tag === 'em' || tag === 'i') return children.trim() ? '*' + children.trim() + '*' : '';
      if (tag === 'li') return '\n- ' + children.trim() + '\n';
      if (['p', 'div', 'section', 'article', 'blockquote', 'ul', 'ol', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6'].includes(tag)) return '\n' + children + '\n';
      return children;
    };
    return read(node).replace(/\u00a0/g, ' ').replace(/[ \t]+\n/g, '\n')
      .replace(/\n[ \t]+/g, '\n').replace(/\n{3,}/g, '\n\n').trim();
  };
  const messages = [], identifiers = new Set();
  for (const element of root.querySelectorAll('user-query,model-response')) {
    if (!visible(element) || element.closest('nav,aside,[data-mind-place-capture]')) continue;
    if (element.parentElement?.closest('user-query,model-response')) continue;
    const role = element.tagName.toLowerCase() === 'user-query' ? 'user' : 'assistant';
    const contentRoot = element.querySelector(role === 'user'
      ? '[data-test-id="query-text"],.query-text,.query-content,.user-query-content'
      : '[data-test-id="response-text"],.markdown,.response-container-content,.response-content,.model-response-text') ?? element;
    const content = markup(contentRoot);
    if (!content) continue;
    if (byteLength(content) > 4 * 1024 * 1024) return fail('A Gemini turn exceeds the 4 MiB message limit.');
    if (messages.length >= 10_000) return fail('The visible Gemini conversation exceeds the 10,000-message limit.');
    const rawId = element.getAttribute('data-message-id') ?? element.getAttribute('data-response-id')
      ?? element.getAttribute('data-id');
    const stable = rawId && /^[A-Za-z0-9_-]{1,128}$/.test(rawId) ? rawId : null;
    const id = stable ? 'gemini-' + role + '-' + stable : 'gemini-' + role + '-position-' + messages.length;
    if (identifiers.has(id)) return fail('Duplicate Gemini message identifiers; nothing was saved.');
    identifiers.add(id);
    const message = { id, role, text: content, format: 'markdown', attachments: [] };
    const timestamp = element.querySelector('time[datetime]')?.getAttribute('datetime');
    if (timestamp && timestamp.length <= 80 && /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$/i.test(timestamp)
      && Number.isFinite(Date.parse(timestamp))) message.authored_at = new Date(timestamp).toISOString();
    const model = role === 'assistant' ? element.getAttribute('data-model') : null;
    if (model && byteLength(model) <= 256) message.model = model;
    messages.push(message);
  }
  if (!messages.length) return fail('No readable Gemini turns were found; nothing was saved.');
  if (messages.some(m => m.id.includes('-position-'))) {
    warnings.push('Some Gemini turns had no stable DOM ID; positional IDs may change as turns are loaded.');
  }
  let title = (document.title ?? '').replace(/\s*[-–|]\s*Gemini\s*$/i, '').trim();
  if (!title || /^Gemini$/i.test(title)) title = messages.find(m => m.role === 'user')?.text.slice(0, 120) ?? 'Gemini conversation';
  title = title.slice(0, 500).trim() || 'Gemini conversation';
  const capture = {
    schema_version: 1, provider: 'gemini', conversation_id: conversationId, source_url: sourceUrl,
    title, captured_at: new Date().toISOString(), capture_method: 'manual-dom',
    completeness: 'partial', warnings, messages,
  };
  if (byteLength(JSON.stringify(capture)) > 31 * 1024 * 1024) return fail('Gemini capture exceeds the 31 MiB limit.');
  if (privateUi()) return fail('The Gemini conversation entered private mode during capture.');
  if (location.href !== sourceUrl) return fail('The Gemini conversation navigated during capture.');
  return { ok: true, capture };
}

/** @type {import('./contract.mjs').CaptureAdapter} */
export const geminiAdapter = Object.freeze({
  contractVersion: 1,
  id: 'gemini',
  pageOrigins: Object.freeze(['https://gemini.google.com']),
  parseConversationUrl: parseGeminiConversationUrl,
  capabilities: Object.freeze({ manualDom: true, manualHistory: false, automaticLoadedDom: false }),
  capture: captureGeminiConversation,
  control: () => false,
  finish: () => {},
  automatic: null,
});

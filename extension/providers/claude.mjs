// MP-7B: self-contained manual Claude DOM reader. No provider API or history scrolling.
export function parseClaudeConversationUrl(value) {
  try {
    const url = new URL(value);
    if (url.origin !== 'https://claude.ai' || url.username || url.password || url.search || url.hash) return null;
    const uuid = '[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}';
    const a = url.pathname.match(new RegExp('^/chat/(' + uuid + ')$', 'i'));
    const b = url.pathname.match(new RegExp('^/project/(' + uuid + ')/chat/(' + uuid + ')$', 'i'));
    const id = a?.[1] ?? b?.[2];
    return id ? { conversationId: id.toLowerCase(), sourceUrl: url.href } : null;
  } catch { return null; }
}

// Serialized by chrome.scripting.executeScript; keep all helpers in this function.
export function captureClaudeConversation(options = {}) {
  const fail = error => ({ ok: false, error });
  if (options.automatic || options.loadHistory) return fail('Claude supports explicit loaded-only capture; no automatic saving or history scrolling.');
  if (globalThis.self !== globalThis.top) return fail('Only top-frame capture is allowed.');
  const startingUrl = globalThis.location?.href;
  if (!startingUrl || !globalThis.document) return fail('No readable page.');
  let conversationId;
  try {
    const url = new URL(startingUrl);
    const uuid = '[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}';
    const direct = url.pathname.match(new RegExp('^/chat/(' + uuid + ')$', 'i'));
    const project = url.pathname.match(new RegExp('^/project/(' + uuid + ')/chat/(' + uuid + ')$', 'i'));
    conversationId = (direct?.[1] ?? project?.[2])?.toLowerCase();
    if (url.origin !== 'https://claude.ai' || url.username || url.password || url.search || url.hash || !conversationId)
      return fail('Open a saved Claude conversation at claude.ai.');
  } catch { return fail('Invalid Claude conversation URL.'); }
  const visible = element => {
    for (let el = element; el?.nodeType === 1; el = el.parentElement) {
      const style = globalThis.getComputedStyle?.(el);
      if (el.hidden || el.hasAttribute('inert') || el.getAttribute('aria-hidden') === 'true'
        || style?.display === 'none' || style?.visibility === 'hidden') return false;
    }
    return true;
  };
  const privateMode = () => [...document.querySelectorAll('[aria-pressed="true"],[aria-checked="true"],[data-state="active"],[role="status"],[role="alert"]')]
    .some(el => {
      if (!visible(el)) return false;
      const hint = [el.getAttribute('aria-label'), el.getAttribute('title'), el.getAttribute('data-testid'), (el.textContent ?? '').trim().slice(0, 100)].join(' ');
      return /\b(?:temporary chat|private chat|incognito)\b/i.test(hint)
        && !/\b(?:off|disabled|not active|turn on|enable)\b/i.test(hint);
    });
  if (privateMode()) return fail('Private or temporary Claude conversation is excluded.');
  const messages = [], ids = new Set();
  let fallback = false, hasBranch = false;
  const encoder = new TextEncoder();
  const selector = '[data-testid="user-message"],[data-testid="assistant-message"],[data-message-author-role="user"],[data-message-author-role="assistant"]';
  const safeToken = (el, attr) => {
    const value = el.getAttribute(attr);
    return value && /^[a-zA-Z0-9_-]{1,80}$/.test(value) ? value : null;
  };
  const render = root => {
    const walk = node => {
      if (node.nodeType === 3) return node.nodeValue ?? '';
      if (node.nodeType !== 1) return '';
      const tag = node.tagName.toLowerCase();
      if (['script','style','noscript','button','input','textarea','select','svg'].includes(tag)
        || node.hasAttribute('inert') || node.getAttribute('aria-hidden') === 'true'
        || node.matches('[data-testid*="thinking"],[data-testid*="reasoning"],[contenteditable="true"]')) return '';
      const style = globalThis.getComputedStyle?.(node);
      if (node.hidden || style?.display === 'none' || style?.visibility === 'hidden') return '';
      if (tag === 'br') return '\n';
      const children = [...node.childNodes].map(walk).join('');
      return ['p','div','pre','li','blockquote','h1','h2','h3','tr'].includes(tag) ? children + '\n' : children;
    };
    return walk(root).replace(/\u00a0/g, ' ').replace(/[ \t]+\n/g, '\n').replace(/\n{3,}/g, '\n\n').trim();
  };
  for (const el of (document.querySelector('main') ?? document).querySelectorAll(selector)) {
    if (!visible(el) || el.closest('[contenteditable="true"],[data-mind-place-capture]') || el.parentElement?.closest(selector)) continue;
    const role = el.matches('[data-testid="user-message"],[data-message-author-role="user"]') ? 'user' : 'assistant';
    const text = render(el);
    if (!text) continue;
    if (encoder.encode(text).length > 4 * 1024 * 1024) return fail('Claude message exceeds 4 MiB.');
    const sourceId = safeToken(el, 'data-message-id');
    const branchId = safeToken(el, 'data-branch-id');
    const regen = safeToken(el, 'data-regeneration-index');
    if (!sourceId) fallback = true;
    if (branchId || regen) hasBranch = true;
    // V1 keeps observed branch/regeneration evidence in the message identity.
    // Positional fallbacks explicitly do not claim stable provider identity.
    const id = 'claude:' + role + ':' + (sourceId ?? 'rendered-' + messages.length)
      + (branchId ? ':branch-' + branchId : '') + (regen ? ':regen-' + regen : '');
    if (id.length > 256 || ids.has(id)) return fail('Ambiguous Claude message IDs.');
    ids.add(id);
    const message = { id, role, text, format: 'plain', attachments: [] };
    const model = safeToken(el, 'data-model');
    if (model && role === 'assistant') message.model = model;
    const timestamp = el.querySelector('time[datetime]')?.getAttribute('datetime');
    if (timestamp && timestamp.length <= 80 && /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$/i.test(timestamp) && Number.isFinite(Date.parse(timestamp)))
      message.authored_at = new Date(timestamp).toISOString();
    messages.push(message);
    if (messages.length > 10000) return fail('Claude capture exceeds 10,000 messages.');
  }
  if (globalThis.location.href !== startingUrl || privateMode()) return fail('Claude identity or privacy mode changed during capture.');
  if (!messages.length) return fail('No recognizable Claude messages were found; nothing saved.');
  const warnings = ['Only currently rendered Claude messages were captured. No history scrolling; unseen branches and media may be missing.'];
  if (fallback) warnings.push('Some message IDs are positional DOM observations, not verified Claude identities; reordering can create another immutable revision.');
  if (hasBranch) warnings.push('Observed branch/regeneration tokens are encoded in message IDs; unseen versions are not inferred.');
  const title = (document.title ?? '').replace(/\s*[-|–]\s*Claude\s*$/i, '').trim().slice(0, 1024) || 'Claude conversation ' + conversationId;
  const capture = { schema_version: 1, provider: 'claude', conversation_id: conversationId, source_url: startingUrl,
    title, captured_at: new Date().toISOString(), capture_method: 'manual-dom', completeness: 'partial', warnings, messages };
  if (encoder.encode(JSON.stringify(capture)).length > 31 * 1024 * 1024) return fail('Claude capture exceeds 31 MiB.');
  return { ok: true, capture };
}

export const claudeAdapter = Object.freeze({
  contractVersion: 1, id: 'claude', pageOrigins: Object.freeze(['https://claude.ai']),
  parseConversationUrl: parseClaudeConversationUrl,
  capabilities: Object.freeze({manualDom:true,manualHistory:false,automaticLoadedDom:false}),
  capture: captureClaudeConversation, control: () => false, finish: () => {}, automatic: null,
});

export function parseChatGptConversationUrl(value) {
  try {
    const url = new URL(value);
    if (url.origin !== 'https://chatgpt.com' || url.username || url.password) return null;

    const uuid = '[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}';
    const direct = url.pathname.match(new RegExp(`^/c/(${uuid})$`, 'i'));
    const project = url.pathname.match(new RegExp(`^/g/([a-z0-9_-]+)/c/(${uuid})$`, 'i'));
    const conversationId = direct?.[1] ?? project?.[2];
    if (!conversationId) return null;
    return { conversationId, sourceUrl: url.href };
  } catch {
    return null;
  }
}

// This function is serialized by chrome.scripting.executeScript. Keep it self-contained.
export function captureConversation(options = {}) {
  if (options.automatic && options.loadHistory) return { ok: false, error: 'Automatic saving is loaded-only and cannot scroll.' };
  if (options.automatic && globalThis.__mindPlaceManualScroll) return { ok: false, error: 'Automatic saving is paused during manual history loading.' };
  const MAX_MESSAGES = 10_000;
  const MAX_MESSAGE_TEXT_BYTES = 4 * 1024 * 1024;
  const MAX_ATTACHMENTS_PER_MESSAGE = 16;
  const MAX_TOTAL_ATTACHMENTS = 120;
  const MAX_INLINE_IMAGE_BYTES = 8 * 1024 * 1024;
  const MAX_INLINE_MEDIA_BYTES = 16 * 1024 * 1024;
  const MAX_CAPTURE_JSON_BYTES = 31 * 1024 * 1024;

  const failure = (error) => ({ ok: false, error });
  const startingUrl = globalThis.location?.href;
  if (!startingUrl || !globalThis.document) return failure('No readable page is active.');

  let parsedUrl;
  try {
    const url = new URL(startingUrl);
    if (url.origin !== 'https://chatgpt.com' || url.username || url.password) {
      return failure('Open a ChatGPT conversation at chatgpt.com before saving.');
    }
    const uuid = '[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}';
    const direct = url.pathname.match(new RegExp(`^/c/(${uuid})$`, 'i'));
    const project = url.pathname.match(new RegExp(`^/g/([a-z0-9_-]+)/c/(${uuid})$`, 'i'));
    const conversationId = direct?.[1] ?? project?.[2];
    if (!conversationId) return failure('Open a ChatGPT conversation before saving.');
    parsedUrl = { conversationId, sourceUrl: url.href, url };
  } catch {
    return failure('The active page URL is not a valid ChatGPT conversation.');
  }

  const temporaryQuery = [...parsedUrl.url.searchParams.entries()].some(([key, value]) =>
    /temporary.?chat/i.test(key) && !/^(false|0|no)$/i.test(value));
  const temporaryUi = () => [...document.querySelectorAll(
    '[aria-pressed="true"],[aria-checked="true"],[aria-current="true"],[data-state="active" i],[data-state="on" i],[data-state="checked" i],[data-state="selected" i],[role="status"],[role="alert"]',
  )]
    .some((element) => {
      const label = `${element.getAttribute('aria-label') ?? ''} ${element.getAttribute('title') ?? ''} ${element.getAttribute('data-testid') ?? ''}`;
      const text = (element.textContent ?? '').trim();
      const description = `${label} ${text}`;
      if (!/\btemporary\s+chat\b/i.test(description)
        || /\b(?:turn on|enable|start|activate|off|disabled|not active)\b/i.test(description)) return false;
      for (let ancestor = element; ancestor?.nodeType === Node.ELEMENT_NODE; ancestor = ancestor.parentElement) {
        if (ancestor.hidden || ancestor.hasAttribute('inert') || ancestor.getAttribute('aria-hidden') === 'true') return false;
        const style = globalThis.getComputedStyle?.(ancestor);
        if (style?.display === 'none' || style?.visibility === 'hidden' || style?.visibility === 'collapse') return false;
      }
      const activeState = element.matches('[aria-pressed="true"],[aria-checked="true"],[aria-current="true"],[data-state="active" i],[data-state="on" i],[data-state="checked" i],[data-state="selected" i]');
      return activeState || ['status', 'alert'].includes(element.getAttribute('role'));
    });
  if (temporaryQuery || temporaryUi()) return failure('Temporary Chat is excluded from Mind Place captures.');

  const warnings = [
    options.automatic
      ? 'This automatic capture contains only loaded messages currently rendered in the page; no scrolling was performed and older or virtualized turns may be missing.'
      : 'This manual capture contains only messages currently rendered in the page; older or virtualized turns may be missing.',
  ];
  const addWarning = (message) => {
    if (!warnings.includes(message)) warnings.push(message);
  };
  const byteLength = (value) => new TextEncoder().encode(value).length;
  const originalTime = (value) => {
    if (typeof value !== 'string' || value.length > 80
      || !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?(?:Z|[+-]\d{2}:\d{2})$/i.test(value)) return undefined;
    const time = Date.parse(value);
    return Number.isFinite(time) ? new Date(time).toISOString() : undefined;
  };

  const safeLink = (href) => {
    try {
      const url = new URL(href, startingUrl);
      if (url.protocol !== 'https:' && url.protocol !== 'http:') return null;
      return url.href.replace(/\)/g, '%29').replace(/\s/g, '%20');
    } catch {
      return null;
    }
  };
  const markdownText = (root) => {
    const render = (node) => {
      if (node.nodeType === Node.TEXT_NODE) return node.nodeValue ?? '';
      if (node.nodeType !== Node.ELEMENT_NODE) return '';
      const element = node;
      const tag = element.tagName.toLowerCase();
      if (['script', 'style', 'noscript', 'button', 'input', 'textarea', 'select', 'svg'].includes(tag)) return '';

      const annotation = element.matches('.katex, .katex-display')
        ? element.querySelector('annotation[encoding="application/x-tex"]')
        : null;
      if (annotation) {
        const latex = (annotation.textContent ?? '').trim();
        if (latex) return element.matches('.katex-display') ? `\n\n$$\n${latex}\n$$\n\n` : `$${latex}$`;
      }

      const children = [...element.childNodes].map(render).join('');
      if (tag === 'br') return '\n';
      if (tag === 'img') {
        const alt = (element.getAttribute('alt') ?? '').trim();
        return alt ? `[Image: ${alt}]` : '[Image]';
      }
      if (tag === 'pre') {
        const code = element.querySelector('code');
        const source = (code?.textContent ?? element.textContent ?? '').replace(/\s+$/, '');
        const language = [...(code?.classList ?? [])].map((name) => name.match(/^(?:language|lang)-([\w+-]+)$/)?.[1]).find(Boolean) ?? '';
        const longestFence = Math.max(2, ...[...source.matchAll(/`+/g)].map((match) => match[0].length));
        const fence = '`'.repeat(longestFence + 1);
        return `\n\n${fence}${language}\n${source}\n${fence}\n\n`;
      }
      if (tag === 'code') {
        const source = children.replace(/\s+/g, ' ').trim();
        const longestFence = Math.max(0, ...[...source.matchAll(/`+/g)].map((match) => match[0].length));
        const fence = '`'.repeat(longestFence + 1);
        return `${fence}${source}${fence}`;
      }
      if (tag === 'a') {
        const href = safeLink(element.getAttribute('href') ?? '');
        return href ? `[${children.trim() || href}](${href})` : children;
      }
      if (tag === 'strong' || tag === 'b') return children.trim() ? `**${children.trim()}**` : '';
      if (tag === 'em' || tag === 'i') return children.trim() ? `*${children.trim()}*` : '';
      if (tag === 'del' || tag === 's' || tag === 'strike') return children.trim() ? `~~${children.trim()}~~` : '';
      if (tag === 'li') return `\n- ${children.trim()}\n`;
      if (['p', 'div', 'section', 'article', 'blockquote', 'ul', 'ol', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'table', 'tr'].includes(tag)) {
        return `\n${children}\n`;
      }
      if (tag === 'td' || tag === 'th') return `${children.trim()} | `;
      return children;
    };

    return render(root)
      .replace(/\u00a0/g, ' ')
      .replace(/[ \t]+\n/g, '\n')
      .replace(/\n[ \t]+/g, '\n')
      .replace(/\n{3,}/g, '\n\n')
      .trim();
  };

  const inferMimeType = (url, kind) => {
    try {
      const path = new URL(url, startingUrl).pathname.toLowerCase();
      const match = path.match(/\.([a-z0-9]+)$/)?.[1];
      const byExtension = {
        png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg', gif: 'image/gif', webp: 'image/webp', avif: 'image/avif',
        mp4: 'video/mp4', webm: 'video/webm', mp3: 'audio/mpeg', wav: 'audio/wav', m4a: 'audio/mp4',
        pdf: 'application/pdf', csv: 'text/csv', txt: 'text/plain',
        docx: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
        xlsx: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
        pptx: 'application/vnd.openxmlformats-officedocument.presentationml.presentation',
      };
      return byExtension[match] ?? (kind === 'video' || kind === 'audio' ? `${kind}/unknown` : 'application/octet-stream');
    } catch {
      return kind === 'video' || kind === 'audio' ? `${kind}/unknown` : 'application/octet-stream';
    }
  };
  const makeAttachment = (element, index, inlineBytes) => {
    const tag = element.tagName.toLowerCase();
    const kind = tag === 'img' ? 'image' : tag === 'a' ? 'file' : tag;
    const rawSource = tag === 'img'
      ? element.getAttribute('src')
      : tag === 'a'
        ? element.getAttribute('href')
        : element.getAttribute('src') ?? element.querySelector('source')?.getAttribute('src') ?? element.getAttribute('poster');
    const name = (element.getAttribute('download') || element.getAttribute('alt') || element.getAttribute('aria-label') || element.getAttribute('title') || (tag === 'a' ? element.textContent?.trim() : '') || `${kind}-${index + 1}`).slice(0, 255);
    const attachment = {
      id: `attachment-${index + 1}`,
      name,
      mime_type: inferMimeType(rawSource ?? '', tag),
    };
    if (!rawSource) {
      attachment.unavailable_reason = 'The rendered media element has no readable source reference.';
      return { attachment, inlineBytes };
    }

    if (rawSource.startsWith('data:')) {
      const declaredMimeType = rawSource.match(/^data:([a-z0-9!#$&^_.+-]+\/[a-z0-9!#$&^_.+-]+)(?:;|,)/i)?.[1];
      if (declaredMimeType) attachment.mime_type = declaredMimeType.toLowerCase();
      if (rawSource.length > Math.ceil(MAX_INLINE_IMAGE_BYTES * 4 / 3) + 128) {
        attachment.unavailable_reason = 'The inline image exceeds the manual capture size limit.';
        return { attachment, inlineBytes };
      }
      const data = rawSource.match(/^data:(image\/(?:png|jpeg|gif|webp));base64,([\s\S]*)$/i);
      if (!data) {
        attachment.unavailable_reason = 'This inline media type is not supported for safe local capture.';
      } else {
        const encoded = data[2];
        const validBase64 = encoded.length % 4 === 0
          && /^(?:[a-z0-9+/]{4})*(?:[a-z0-9+/]{2}==|[a-z0-9+/]{3}=)?$/i.test(encoded);
        const estimate = Math.max(0, Math.floor(encoded.length * 3 / 4) - (encoded.endsWith('==') ? 2 : encoded.endsWith('=') ? 1 : 0));
        if (!validBase64) {
          attachment.unavailable_reason = 'The inline image data is malformed.';
        } else if (estimate > MAX_INLINE_IMAGE_BYTES || inlineBytes + estimate > MAX_INLINE_MEDIA_BYTES) {
          attachment.unavailable_reason = 'The inline image exceeds the manual capture size limit.';
        } else {
          attachment.mime_type = data[1].toLowerCase();
          attachment.data_url = rawSource;
          inlineBytes += estimate;
        }
      }
      return { attachment, inlineBytes };
    }

    if (rawSource.length > 4096) {
      attachment.unavailable_reason = 'The media source reference exceeds the manual capture size limit.';
      return { attachment, inlineBytes };
    }
    try {
      const url = new URL(rawSource, startingUrl);
      if (url.protocol === 'https:' || url.protocol === 'http:') {
        if (url.href.length > 1024) {
          attachment.unavailable_reason = 'The media source reference exceeds the manual capture size limit.';
        } else {
          attachment.source_url = url.href;
        }
      } else {
        attachment.unavailable_reason = 'Media is available only through a transient browser reference.';
      }
    } catch {
      attachment.unavailable_reason = 'The media source is not a valid web URL.';
    }
    return { attachment, inlineBytes };
  };

  // Read native ChatGPT layout boundaries directly for a one-shot capture.
  // Do not depend on provider internals, Fiber stamps, or persistent observers.
  const unitSelector = '[data-content-search-unit-key],[data-chatgpt-search-unit-key]';
  const classicTurnSelector = '[data-testid^="conversation-turn-"][data-turn]';
  const messageSelector = `[data-message-author-role],${unitSelector},${classicTurnSelector}`;
  const ownSurfaces = '.clf-stream,.clf-stage,.clf-composer,.clf-boot,[data-clf-user-text],[data-mind-place-capture]';
  const messageCandidates = () => {
    const visibility = new WeakMap();
    const visible = (element) => {
      if (!element) return true;
      if (visibility.has(element)) return visibility.get(element);
      const style = globalThis.getComputedStyle?.(element);
      const value = !element.hidden && !element.hasAttribute('inert') && element.getAttribute('aria-hidden') !== 'true'
        && style?.display !== 'none' && style?.visibility !== 'hidden' && style?.visibility !== 'collapse'
        && visible(element.parentElement);
      visibility.set(element, value);
      return value;
    };
    const surface = [...document.querySelectorAll('[data-app-shell-main-surface]')].find(visible)
      ?? [...document.querySelectorAll('main')].find(visible) ?? document;
    const nodes = [...(surface.matches?.(messageSelector) ? [surface] : []), ...surface.querySelectorAll(messageSelector)];
    const candidates = [];
    for (const element of nodes) {
      if (!visible(element) || element.closest(`${ownSurfaces},[contenteditable="true"]`)) continue;
      const explicit = element.getAttribute('data-message-author-role');
      let role = (explicit ?? '').trim().toLowerCase();
      if (explicit !== null) {
        if (element.parentElement?.closest('[data-message-author-role]')) continue;
      } else if (element.matches(unitSelector)) {
        const turn = element.closest('[data-turn-key]');
        if (!turn || !element.closest('[data-thread-find-target="conversation"]')) continue;
        if (element.closest('[data-message-author-role]') || element.querySelector('[data-message-author-role]')) continue;
        if (element.parentElement?.closest(`${unitSelector},.markdown,[data-markdown-text-style]`)) continue;
        const key = element.getAttribute('data-content-search-unit-key') || element.getAttribute('data-chatgpt-search-unit-key') || '';
        role = /:(user|assistant)$/.exec(key)?.[1] ?? '';
      } else {
        if (element.closest('[data-message-author-role]') || element.querySelector(`[data-message-author-role],${unitSelector}`)) continue;
        role = (element.getAttribute('data-turn') ?? '').trim().toLowerCase();
      }
      // Explicit role attributes can coexist with shell units without provider IDs.
      // Resolve the enclosing native unit for either shape so repeated text cannot
      // merge across virtualized exchanges merely because the role is explicit.
      let nativeId = null;
      const unit = element.closest(unitSelector);
      const turn = unit?.closest('[data-turn-key]');
      const key = unit?.getAttribute('data-content-search-unit-key') || unit?.getAttribute('data-chatgpt-search-unit-key') || '';
      if (turn && unit.closest('[data-thread-find-target="conversation"]') && /:(user|assistant)$/.exec(key)?.[1] === role) {
        let turnKey = turn.getAttribute('data-turn-key') ?? '';
        if (!turnKey || /^fallback-turn-\d+$/.test(turnKey)) {
          turnKey = unit.closest('[data-content-search-turn-key]')?.getAttribute('data-content-search-turn-key') ?? '';
        }
        if (/^[a-z0-9_-]{1,160}$/i.test(turnKey) && !/^fallback-turn-\d+$/.test(turnKey)) {
          const slot = /:(\d{1,12}):(user|assistant)$/.exec(key)?.[1] ?? 'authored';
          nativeId = `native-turn-${turnKey}-${slot}-${role}`;
        }
      }
      candidates.push({ element, role, nativeId });
    }
    return candidates;
  };

  let renderedCandidates = [];
  const readSnapshot = () => {
    if (location.href !== startingUrl) return failure('The ChatGPT conversation changed while it was being read; nothing was saved.');
    if (temporaryUi()) return failure('Temporary Chat is excluded from Mind Place captures.');
    const candidateNodes = messageCandidates();
    if (options.automatic && candidateNodes.some(({ element }) => {
      const owner = element.closest('[data-conversation-id],[data-thread-id]');
      const ownerId = owner?.getAttribute('data-conversation-id') ?? owner?.getAttribute('data-thread-id');
      return ownerId && ownerId.toLowerCase() !== parsedUrl.conversationId.toLowerCase();
    })) return failure('Loaded messages belong to a different conversation; automatic saving is waiting for navigation to finish.');
    const recognizedNodes = candidateNodes.filter(({ role }) => ['user', 'assistant', 'system', 'tool'].includes(role));
    renderedCandidates = recognizedNodes;
    if (recognizedNodes.length > MAX_MESSAGES) {
      return failure(`More than ${MAX_MESSAGES.toLocaleString()} rendered messages exceed the manual capture limit; nothing was saved.`);
    }
    const messages = [];
    const usedIds = new Set();
    let totalAttachments = 0;
    let inlineBytes = 0;
    const fingerprint = (value) => {
      let hash = 2166136261;
      for (let index = 0; index < value.length; index += 1) {
        hash = Math.imul(hash ^ value.charCodeAt(index), 16777619);
      }
      return (hash >>> 0).toString(16).padStart(8, '0');
    };

    for (const { element, role, nativeId } of candidateNodes) {
      if (!['user', 'assistant', 'system', 'tool'].includes(role)) {
        addWarning('Some rendered message blocks had an unknown role and were omitted.');
        continue;
      }

      const content = element.cloneNode(true);
      for (const control of content.querySelectorAll(`button,input,textarea,select,script,style,noscript,svg,.sr-only,[data-testid*="action" i],${ownSurfaces}`)) control.remove();
      const mediaElements = [...new Set(content.querySelectorAll('img,video,audio,a[download],a[data-testid*="attachment" i],a[data-testid*="file" i]'))];
      const elements = mediaElements.slice(0, MAX_ATTACHMENTS_PER_MESSAGE);
      if (mediaElements.length > MAX_ATTACHMENTS_PER_MESSAGE) {
        addWarning('Some media references were omitted because a message exceeded the attachment count limit.');
      }
      const attachments = [];
      for (const media of elements) {
        if (totalAttachments >= MAX_TOTAL_ATTACHMENTS) {
          addWarning(`Media references after the first ${MAX_TOTAL_ATTACHMENTS} were omitted by the capture size limit.`);
          break;
        }
        const result = makeAttachment(media, attachments.length, inlineBytes);
        inlineBytes = result.inlineBytes;
        attachments.push(result.attachment);
        totalAttachments += 1;
      }

      // Shell slots include speaker labels, progress and toolbar chrome around the
      // authored subtree. Keep media from the whole slot, but text from native content.
      const preferredSelector = role === 'user' ? '[data-user-message-bubble],.whitespace-pre-wrap'
        : role === 'assistant' ? '.markdown,[data-markdown-text-style="assistant-message"]' : null;
      const preferred = preferredSelector ? [...(content.matches(preferredSelector) ? [content] : []), ...content.querySelectorAll(preferredSelector)]
        .filter((part) => {
          const outer = part.parentElement?.closest(preferredSelector);
          return !outer || !content.contains(outer);
        }) : [];
      const authored = document.createElement('div');
      for (const part of preferred) authored.append(part.cloneNode(true));
      let text = markdownText(preferred.length ? authored : content);
      if (byteLength(text) > MAX_MESSAGE_TEXT_BYTES) {
        return failure(`A rendered message exceeds the 4 MiB per-message limit; nothing was saved.`);
      }
      if (!text && attachments.length === 0) continue;

      const providerId = (element.getAttribute('data-message-id') ?? '').trim().slice(0, 256);
      const turnId = options.loadHistory ? element.closest('[data-testid^="conversation-turn-"]')?.getAttribute('data-testid') : null;
      let id = providerId || nativeId || (turnId ? `turn-${turnId.slice(0, 200)}-${role}` : `manual-${role}-${fingerprint(text)}`);
      if (nativeId && !providerId) addWarning('Some message identities are derived from native turn/unit keys; provider message IDs are not verified.');
      if (options.loadHistory && !providerId && !nativeId && !turnId) {
        addWarning('Some messages lack stable page identities; repeated identical turns or their order cannot be verified.');
      }
      if (usedIds.has(id)) {
        let suffix = 2;
        while (usedIds.has(`${id}-${suffix}`)) suffix += 1;
        id = `${id}-${suffix}`;
      }
      usedIds.add(id);
      const message = { id, role, text, format: 'markdown', attachments };
      let authoredAt = [...element.querySelectorAll('time[datetime]')]
        .filter((time) => !time.closest(ownSurfaces))
        .map((time) => originalTime(time.getAttribute('datetime'))).find(Boolean);
      if (!authoredAt && role === 'user') {
        // Current ChatGPT renders the exchange's original user time in a native
        // separator outside both message units. It is not an assistant send time.
        const turn = element.closest('[data-turn-key],[data-testid^="conversation-turn-"]');
        authoredAt = [...(turn?.querySelectorAll('[role="separator"] time[datetime]') ?? [])]
          .filter((time) => !time.closest(`${ownSurfaces},${messageSelector}`)
            && time.closest('[data-turn-key],[data-testid^="conversation-turn-"]') === turn)
          .map((time) => originalTime(time.getAttribute('datetime'))).find(Boolean);
      }
      if (authoredAt) message.authored_at = authoredAt;
      messages.push(message);
    }

    if (location.href !== startingUrl) return failure('The ChatGPT conversation changed while it was being read; nothing was saved.');
    if (messages.length === 0) return failure('No readable conversation messages are currently rendered. Scroll the conversation until messages appear, then try again.');
    if (totalAttachments && messages.some((message) => message.attachments.some((attachment) => attachment.source_url))) {
      addWarning('Remote media are stored as references only; Mind Place does not download them during capture.');
    }
    if (messages.some((message) => message.attachments.some((attachment) => attachment.unavailable_reason))) {
      addWarning('Some media could not be copied and are recorded with an unavailable reason.');
    }

    let title = (document.querySelector('main h1')?.textContent ?? '').trim();
    if (!title) title = (document.title ?? '').replace(/\s*[|–-]\s*ChatGPT\s*$/i, '').trim();
    if (!title || /^chatgpt$/i.test(title)) title = `ChatGPT conversation ${parsedUrl.conversationId}`;
    title = title.slice(0, 500);

    const capture = {
      schema_version: 1,
      provider: 'chatgpt',
      conversation_id: parsedUrl.conversationId,
      source_url: parsedUrl.sourceUrl,
      title,
      captured_at: new Date().toISOString(),
      capture_method: options.automatic ? 'automatic-dom' : 'manual-dom',
      completeness: 'partial',
      warnings,
      messages,
    };
    const sourceMetadata = candidateNodes[0]?.element.closest('[data-conversation-id]');
    if (sourceMetadata?.getAttribute('data-conversation-id')?.toLowerCase() === parsedUrl.conversationId.toLowerCase()) {
      const created = originalTime(sourceMetadata.getAttribute('data-conversation-created-at'));
      const updated = originalTime(sourceMetadata.getAttribute('data-conversation-updated-at'));
      if (created) capture.conversation_created_at = created;
      if (updated) capture.conversation_updated_at = updated;
    }
    if (byteLength(JSON.stringify(capture)) > MAX_CAPTURE_JSON_BYTES) {
      return failure('The rendered capture exceeds the 31 MiB native message limit; nothing was saved. Scroll to a smaller section and try again.');
    }
    return { ok: true, capture };
  };

  if (!options.loadHistory) return readSnapshot();

  // Everything needed by this async job stays in the serialized isolated-world function.
  return (async () => {
    const stateKey = '__mindPlaceManualScroll';
    if (globalThis[stateKey]) return failure('Another manual capture is already loading in this tab.');
    const operationId = String(options.operationId ?? 'manual');
    const state = { operationId, stop: false, cancelled: false };
    globalThis[stateKey] = state;
    const started = performance.now();
    const duration = Math.max(40, Math.min(240_000, Number(options.maxDurationMs) || 180_000));
    const pollMs = Math.max(10, Math.min(1000, Number(options.pollIntervalMs) || 40));
    const settleMs = Math.max(40, Math.min(10_000, Number(options.settleMs) || 80));
    const boundarySettleMs = options.settleMs ? settleMs : 750;
    const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
    const timeUp = () => performance.now() - started >= duration;
    let panel;
    let progress;
    let scroller;
    let originalTop = 0;
    let bottomMoved = false;
    let originalBehavior = '';
    let originalPriority = '';
    let latestCapture;
    let totalBytes = 0;
    const messages = new Map();
    const order = [];
    // Establish the newest window first, then traverse older history once.
    // Overlapping viewport steps retain virtualized turns without a second sweep.
    let phase = 'bottom';
    let reason = 'The newest window and oldest scroll boundary stayed stable; unloaded history, branches, and original remote media are still not proven complete.';
    const publishProgress = (text) => {
      if (progress?.textContent === text) return;
      if (progress) progress.textContent = text;
      try {
        globalThis.chrome?.runtime?.sendMessage({ type: 'capture-progress', operationId, messageCount: messages.size, text })?.catch?.(() => {});
      } catch { /* Closing a popup does not cancel the page job. */ }
    };
    const checkPage = () => {
      if (location.href !== startingUrl) throw new Error('The ChatGPT conversation changed during loading; nothing was saved.');
      if (temporaryUi()) throw new Error('Temporary Chat is excluded; nothing was saved.');
      if (state.cancelled) throw new Error('Capture cancelled. Nothing was saved.');
    };
    const collect = () => {
      checkPage();
      const result = readSnapshot();
      if (!result.ok) {
        if (/No readable conversation messages/.test(result.error)) return '';
        throw new Error(result.error);
      }
      latestCapture = result.capture;
      const incoming = result.capture.messages;
      // Before the bottom jump commits, the DOM can still show the old reading
      // window. Observe it for settling, then seed chronology at the newest window;
      // mixing these disjoint starting windows would misorder the upward sweep.
      if (phase === 'bottom') {
        publishProgress('Loading newest messages… Stop & save keeps the rendered window.');
        return incoming.map((message) => `${message.id}:${message.text.length}`).join('|');
      }
      let previous;
      for (let index = 0; index < incoming.length; index += 1) {
        const message = incoming[index];
        const prior = messages.get(message.id);
        if (!prior) {
          const next = incoming.slice(index + 1).find((item) => messages.has(item.id));
          const insertion = previous ? order.indexOf(previous) + 1 : next ? order.indexOf(next.id) : phase === 'up' ? 0 : order.length;
          order.splice(insertion, 0, message.id);
        }
        // Never replace a previously observed full answer by a loading placeholder.
        const retained = prior && prior.text.length > message.text.length ? prior : message;
        totalBytes += byteLength(JSON.stringify(retained)) - (prior ? byteLength(JSON.stringify(prior)) : 0);
        messages.set(message.id, retained);
        previous = message.id;
      }
      if (messages.size > MAX_MESSAGES) throw new Error('More than 10,000 collected messages exceed the manual capture limit; nothing was saved.');
      if (totalBytes > MAX_CAPTURE_JSON_BYTES - 16_384) throw new Error('The collected history exceeds the 31 MiB capture limit; nothing was saved.');
      publishProgress(`Loading ${phase === 'up' ? 'older' : 'newest'} messages… ${messages.size} collected. Stop & save keeps the collected history.`);
      return incoming.map((message) => `${message.id}:${message.text.length}`).join('|');
    };
    const position = () => `${Math.round(scroller.scrollTop)}:${scroller.scrollHeight}:${scroller.clientHeight}`;
    const historyLoading = () => [...document.querySelectorAll('main[aria-busy="true"],main [aria-busy="true"],main [role="progressbar"]')].some((element) => {
      for (let ancestor = element; ancestor; ancestor = ancestor.parentElement) {
        const style = getComputedStyle(ancestor);
        if (ancestor.hidden || ancestor.getAttribute('aria-hidden') === 'true' || style.display === 'none' || style.visibility === 'hidden') return false;
      }
      return true;
    });
    const moveTo = (top) => {
      if (typeof scroller.scrollTo === 'function') scroller.scrollTo({ top, behavior: 'instant' });
      else scroller.scrollTop = top;
    };
    const resolveScroller = () => {
      const first = messageCandidates().find(({ role }) => ['user', 'assistant', 'system', 'tool'].includes(role))?.element;
      if (!first) return null;
      const ancestors = [];
      for (let ancestor = first.parentElement; ancestor; ancestor = ancestor.parentElement) ancestors.push(ancestor);
      const native = ancestors.find((element) => element.matches('.thread-scroll-container'));
      if (native) return native.clientHeight > 0 ? native : null;
      const nested = ancestors.find((element) => /(auto|scroll|overlay)/.test(getComputedStyle(element).overflowY) && element.clientHeight > 0);
      if (nested) return nested;
      const page = document.scrollingElement ?? document.documentElement;
      return page.clientHeight > 0 ? page : null;
    };
    const windowPending = () => {
      const first = renderedCandidates[0]?.element.getBoundingClientRect();
      const last = renderedCandidates.at(-1)?.element.getBoundingClientRect();
      if (!first?.height || !last?.height) return false;
      const documentScroll = scroller === document.scrollingElement || scroller === document.documentElement || scroller === document.body;
      const top = documentScroll ? 0 : scroller.getBoundingClientRect().top;
      const height = scroller.clientHeight || globalThis.innerHeight;
      // The old DOM window can remain at the previous position after scrollTop
      // changes. Only pause when it leaves a clear gap at the leading edge.
      return phase === 'up' ? first.top > top + Math.max(120, height * 0.25)
        : bottomMoved && last.bottom < top + height * 0.75;
    };
    const settle = async (quietMs) => {
      let signature = `${position()}|${collect()}`;
      let stableSince = performance.now();
      const renderDeadline = performance.now() + 1500;
      while (!state.stop && !timeUp() && (historyLoading() || performance.now() - stableSince < quietMs
        || (windowPending() && performance.now() < renderDeadline))) {
        await sleep(Math.min(pollMs, Math.max(1, duration - (performance.now() - started))));
        const nextSignature = `${position()}|${collect()}`;
        if (nextSignature !== signature) { signature = nextSignature; stableSince = performance.now(); }
      }
      checkPage();
      if (windowPending()) addWarning('A rendered message window lagged behind scrolling; some virtualized history may be missing.');
    };
    try {
      panel = document.createElement('aside');
      panel.setAttribute('data-mind-place-capture', operationId);
      panel.setAttribute('aria-label', 'Mind Place manual capture');
      panel.style.cssText = 'box-sizing:border-box;position:fixed;z-index:2147483647;right:16px;bottom:16px;max-width:min(360px,calc(100vw - 32px));padding:16px;border:1px solid #777;border-radius:12px;background:#202326;color:#f5f5f5;font:14px/1.5 system-ui;box-shadow:0 4px 24px #0006;';
      const heading = document.createElement('strong');
      heading.textContent = 'Mind Place · manual capture';
      progress = document.createElement('p');
      progress.setAttribute('role', 'status');
      progress.setAttribute('aria-live', 'polite');
      const stop = document.createElement('button');
      stop.setAttribute('data-mind-place-stop', '');
      stop.textContent = 'Stop & save';
      stop.addEventListener('click', () => { state.stop = true; stop.disabled = true; });
      const cancel = document.createElement('button');
      cancel.setAttribute('data-mind-place-cancel', '');
      cancel.textContent = 'Cancel';
      cancel.addEventListener('click', () => { state.cancelled = true; cancel.disabled = true; });
      for (const button of [stop, cancel]) button.style.cssText = 'margin:4px 8px 0 0;padding:8px 12px;border:1px solid #888;border-radius:6px;background:#30363b;color:white;font:inherit;cursor:pointer;';
      panel.append(heading, progress, stop, cancel);
      document.body.append(panel);

      const readyDeadline = Math.min(started + duration, performance.now() + 5000);
      while (!scroller && !state.stop && performance.now() < readyDeadline) {
        checkPage();
        scroller = resolveScroller();
        if (!scroller) {
          publishProgress('Waiting for the conversation and its scroll area to finish loading…');
          await sleep(pollMs);
        }
      }
      checkPage();
      if (!scroller) {
        if (state.stop) {
          const rendered = readSnapshot();
          if (!rendered.ok) throw new Error(rendered.error);
          addWarning('Loading was stopped before the scroll area was ready; only currently rendered messages are included.');
          publishProgress(`Collected ${rendered.capture.messages.length} rendered messages. Saving locally…`);
          stop.disabled = true;
          cancel.disabled = true;
          return rendered;
        }
        throw new Error('The conversation scroll area is still loading; nothing was saved. Wait for the conversation to appear, then try again.');
      }
      originalTop = scroller.scrollTop;
      originalBehavior = scroller.style.getPropertyValue('scroll-behavior');
      originalPriority = scroller.style.getPropertyPriority('scroll-behavior');
      scroller.style.setProperty('scroll-behavior', 'auto', 'important');
      collect();
      // A loaded bottom can grow while its DOM window commits. Re-establish the
      // actual newest boundary before changing direction; all waits share the budget.
      for (let step = 0; step < 2000 && !state.stop && !timeUp(); step += 1) {
        const bottom = Math.max(0, scroller.scrollHeight - scroller.clientHeight);
        const moved = Math.abs(scroller.scrollTop - bottom) > 1;
        bottomMoved = moved;
        moveTo(bottom);
        await settle(moved && !options.settleMs ? Math.max(boundarySettleMs, settleMs) : settleMs);
        if (scroller.scrollTop >= scroller.scrollHeight - scroller.clientHeight - 1) break;
      }
      phase = 'up';
      collect();
      for (let step = 0; step < 2000 && !state.stop && !timeUp(); step += 1) {
        checkPage();
        const beforeTop = scroller.scrollTop;
        const target = Math.max(0, beforeTop - Math.max(1, scroller.clientHeight * 0.9));
        moveTo(target);
        await settle(target <= 1 ? boundarySettleMs : settleMs);
        if (scroller.scrollTop <= 1 && !state.stop && !timeUp()) break;
        if (step === 1999) reason = 'The scroll step limit was reached; additional history may be missing.';
      }
      checkPage();
      collect();
      if (!latestCapture || messages.size === 0) throw new Error('No readable conversation messages appeared; nothing was saved.');
      if (state.stop) reason = 'Loading was stopped by the user; only the history collected before Stop & save is included.';
      else if (timeUp()) reason = 'The loading time budget was reached; this snapshot includes only the history collected within that time.';
      warnings[0] = 'Manual scrolling collected messages across rendered page windows; unloaded/virtualized turns, other branches, and remote media may still be missing.';
      addWarning(reason);
      latestCapture.messages = order.map((id) => messages.get(id));
      latestCapture.captured_at = new Date().toISOString();
      // Limit media across all observed windows as well as the complete serialized request.
      let mediaBytes = 0;
      let referenceCount = 0;
      for (const message of latestCapture.messages) {
        const remaining = Math.max(0, MAX_TOTAL_ATTACHMENTS - referenceCount);
        if (message.attachments.length > remaining) {
          message.attachments = message.attachments.slice(0, remaining);
          addWarning(`Media references after the first ${MAX_TOTAL_ATTACHMENTS} collected attachments were omitted by the capture limit.`);
        }
        referenceCount += message.attachments.length;
        for (const attachment of message.attachments) {
          if (!attachment.data_url) continue;
          const encoded = attachment.data_url.split(',')[1];
          const size = Math.floor(encoded.length * 3 / 4) - (encoded.endsWith('==') ? 2 : encoded.endsWith('=') ? 1 : 0);
          if (mediaBytes + size <= MAX_INLINE_MEDIA_BYTES) mediaBytes += size;
          else { delete attachment.data_url; attachment.unavailable_reason = 'The collected inline media exceeds the 16 MiB capture limit.'; addWarning('Some inline media exceeded the total collected media limit and remain unavailable references.'); }
        }
      }
      if (byteLength(JSON.stringify(latestCapture)) > MAX_CAPTURE_JSON_BYTES) throw new Error('The collected history exceeds the 31 MiB capture limit; nothing was saved.');
      publishProgress(`Collected ${messages.size} messages. Saving locally…`);
      stop.disabled = true;
      cancel.disabled = true;
      return { ok: true, capture: latestCapture };
    } catch (error) {
      panel?.remove();
      return failure(error?.message ?? 'Loading failed; nothing was saved.');
    } finally {
      if (scroller && location.href === startingUrl) {
        moveTo(originalTop);
        if (originalBehavior) scroller.style.setProperty('scroll-behavior', originalBehavior, originalPriority);
        else scroller.style.removeProperty('scroll-behavior');
      }
      if (globalThis[stateKey] === state) delete globalThis[stateKey];
    }
  })();
}

export function controlCapture(operationId, action) {
  const state = globalThis.__mindPlaceManualScroll;
  if (!state || state.operationId !== operationId) return false;
  if (action === 'stop') state.stop = true;
  else if (action === 'cancel') state.cancelled = true;
  return true;
}

export function finishCapture(operationId, text) {
  const panel = [...document.querySelectorAll('[data-mind-place-capture]')]
    .find((element) => element.getAttribute('data-mind-place-capture') === operationId);
  if (!panel) return;
  panel.querySelector('[role="status"]').textContent = text;
  for (const button of panel.querySelectorAll('button')) button.remove();
  const close = document.createElement('button');
  close.textContent = 'Close';
  close.addEventListener('click', () => panel.remove());
  panel.append(close);
}

/** @type {import('./contract.mjs').CaptureAdapter} */
export const chatGptAdapter = Object.freeze({
  contractVersion: 1,
  id: 'chatgpt',
  pageOrigins: Object.freeze(['https://chatgpt.com']),
  parseConversationUrl: parseChatGptConversationUrl,
  capabilities: Object.freeze({ manualDom: true, manualHistory: true, automaticLoadedDom: true }),
  capture: captureConversation,
  control: controlCapture,
  finish: finishCapture,
  automatic: Object.freeze({
    scriptId: 'mind-place-loaded-auto-save',
    origins: Object.freeze(['https://chatgpt.com/*']),
    observerFile: 'auto-observer.js',
  }),
});

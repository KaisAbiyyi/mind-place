// Runs only after optional ChatGPT access and the explicit automatic-saving opt-in.
// This observer never scrolls, changes page styles, reads provider state, or opens a native port.
function installAutomaticObserver({ quietMs = 1200, maxWaitMs = 5000, tickMs = 1000 } = {}) {
  if (globalThis.__mindPlaceAutoObserver) {
    globalThis.__mindPlaceAutoObserver.wake();
    return globalThis.__mindPlaceAutoObserver;
  }
  let stopped = false, active = false, root, observer, titleObserver, quietTimer, maxTimer, tickTimer;
  let observedUrl = location.href, lastRecords = [], staleRecords = null, dirty = false, wasManual = false;
  const selector = '[data-message-author-role],[data-content-search-unit-key],[data-chatgpt-search-unit-key],[data-testid^="conversation-turn-"][data-turn]';
  const visible = node => {
    for (let current = node; current?.nodeType === 1; current = current.parentElement) {
      if (current.hidden || current.hasAttribute('inert') || current.getAttribute('aria-hidden') === 'true') return false;
      const style = getComputedStyle(current);
      if (style.display === 'none' || style.visibility === 'hidden') return false;
    }
    return true;
  };
  // Navigation identity deliberately ignores assistant streaming text: old tokens
  // must not make retained old nodes look like a newly opened conversation.
  const records = () => [...(root?.querySelectorAll(selector) ?? [])].filter(node => visible(node)
    && !node.closest('.clf-stream,.clf-stage,.clf-composer,.clf-boot,[data-mind-place-capture],[contenteditable="true"]'))
    .map((node, index) => {
      const native = node.closest('[data-turn-key]')?.getAttribute('data-turn-key');
      const stable = node.getAttribute('data-message-id') || (native && !/^fallback-turn-\d+$/.test(native) ? native : null);
      const role = node.getAttribute('data-message-author-role') ?? node.getAttribute('data-turn')
        ?? node.getAttribute('data-content-search-unit-key') ?? node.getAttribute('data-chatgpt-search-unit-key') ?? '';
      return { node, stable: Boolean(stable), user: /user/.test(role),
        key: stable || `${index}:${role}:${node.textContent}` };
    });
  const clearPending = () => { clearTimeout(quietTimer); clearTimeout(maxTimer); quietTimer = maxTimer = undefined; };
  const stop = () => {
    stopped = true; active = false; clearPending(); clearInterval(tickTimer);
    observer?.disconnect(); titleObserver?.disconnect();
    chrome.runtime.onMessage.removeListener(onMessage);
    chrome.storage?.onChanged?.removeListener(onStorage);
    if (globalThis.__mindPlaceAutoObserver === controller) delete globalThis.__mindPlaceAutoObserver;
  };
  const checkRoute = () => {
    if (location.href === observedUrl) return;
    observedUrl = location.href;
    staleRecords = lastRecords;
    clearPending(); dirty = true;
  };
  const flush = () => {
    clearPending();
    if (!active || stopped) return;
    checkRoute();
    if (globalThis.__mindPlaceManualScroll) { dirty = true; wasManual = true; return; }
    const current = records();
    if (!current.length || staleRecords?.some(old => current.some(item =>
      old.stable && item.stable ? old.key === item.key : old.node === item.node))) return;
    staleRecords = null; lastRecords = current; dirty = false;
    let request;
    try { request = chrome.runtime.sendMessage({ type: 'auto-capture', sourceUrl: location.href }); }
    catch { stop(); return; }
    Promise.resolve(request).then(reply => { if (reply?.enabled === false) stop(); }, stop);
  };
  const schedule = () => {
    if (!active || stopped) return;
    checkRoute(); dirty = true;
    if (staleRecords === null) lastRecords = records();
    if (globalThis.__mindPlaceManualScroll) { wasManual = true; return; }
    clearTimeout(quietTimer); quietTimer = setTimeout(flush, quietMs);
    if (!maxTimer) maxTimer = setTimeout(flush, maxWaitMs);
  };
  const wake = () => {
    if (!active || stopped) return;
    try { if (!chrome.runtime.id) { stop(); return; } } catch { stop(); return; }
    checkRoute();
    const nextRoot = [...document.querySelectorAll('[data-app-shell-main-surface]')].find(visible)
      ?? [...document.querySelectorAll('main')].find(visible);
    if (nextRoot !== root) {
      observer?.disconnect(); root = nextRoot;
      if (root) {
        observer = new MutationObserver(schedule);
        observer.observe(root, { childList: true, characterData: true, subtree: true, attributes: true,
          attributeFilter: ['hidden', 'aria-hidden', 'datetime', 'src', 'href', 'data-message-id', 'data-turn-key', 'data-is-streaming'] });
        schedule();
      }
    }
    if (!globalThis.__mindPlaceManualScroll && wasManual) { wasManual = false; schedule(); }
    if (dirty && !quietTimer && !maxTimer && root && !globalThis.__mindPlaceManualScroll) schedule();
  };
  const onMessage = message => { if (message?.type === 'auto-saving-disabled') stop(); };
  const onStorage = (changes, area) => {
    if (area === 'local' && changes.automaticSavingEnabled?.newValue !== undefined
      && changes.automaticSavingEnabled.newValue !== true) stop();
  };
  const controller = { stop, wake };
  globalThis.__mindPlaceAutoObserver = controller;
  chrome.runtime.onMessage.addListener(onMessage);
  chrome.storage?.onChanged?.addListener(onStorage);
  Promise.resolve(chrome.runtime.sendMessage({ type: 'auto-page-state' })).then(reply => {
    if (stopped) return;
    if (reply?.enabled !== true) { stop(); return; }
    active = true; wake(); lastRecords = records();
    const title = document.querySelector('title');
    if (title) { titleObserver = new MutationObserver(schedule); titleObserver.observe(title, { childList: true, subtree: true, characterData: true }); }
    tickTimer = setInterval(wake, tickMs);
  }, stop);
  return controller;
}

if (globalThis.chrome?.runtime) installAutomaticObserver();

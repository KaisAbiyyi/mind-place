import { CAPTURE_ADAPTERS, captureAdapterForPage, resolveCaptureAdapter, captureMatchesIdentity } from './providers/index.mjs';
import { NATIVE_HOST, validReceipt, withTimeout } from './native.mjs';

const automaticAdapters = CAPTURE_ADAPTERS.filter(adapter => adapter.capabilities.automaticLoadedDom && adapter.automatic);
export const AUTO_ORIGINS = Object.freeze(automaticAdapters.flatMap(adapter => adapter.automatic.origins));
const scriptIds = automaticAdapters.map(adapter => adapter.automatic.scriptId);

export function initAutomaticSaving(chromeApi, { saveTimeoutMs = 15_000, attachmentTimeoutMs = 5000 } = {}) {
  let generation = 0;
  const pending = new Map();
  const lastSaved = new Map();
  const local = chromeApi.storage.local;
  const permitted = () => chromeApi.permissions.contains({ origins: AUTO_ORIGINS });
  const enabled = async () => (await local.get('automaticSavingEnabled')).automaticSavingEnabled === true && await permitted();
  const isPage = sender => sender?.id === chromeApi.runtime.id && sender.frameId === 0
    && Number.isInteger(sender.tab?.id) && !sender.tab.incognito
    && captureAdapterForPage(sender.url)?.capabilities.automaticLoadedDom === true;
  const isPopup = sender => sender?.id === chromeApi.runtime.id && !sender.tab
    && sender.url === chromeApi.runtime.getURL('popup.html');
  const status = async (value) => {
    await local.set({ automaticSavingStatus: { ...value, at: new Date().toISOString() } });
  };
  const attach = async tab => {
    if (!Number.isInteger(tab?.id) || tab.incognito) return;
    try {
      const adapter = captureAdapterForPage(tab.url);
      if (!adapter?.capabilities.automaticLoadedDom || !adapter.automatic) return;
      await withTimeout(chromeApi.scripting.executeScript({ target: { tabId: tab.id }, files: [adapter.automatic.observerFile] }), attachmentTimeoutMs);
    } catch { /* A closed/loading tab is attached on its next completed navigation. */ }
  };
  const synchronize = async () => {
    const active = await enabled();
    const registered = await chromeApi.scripting.getRegisteredContentScripts({ ids: scriptIds });
    if (!active) {
      if (registered.length) await chromeApi.scripting.unregisterContentScripts({ ids: scriptIds });
      return false;
    }
    const missing = automaticAdapters.filter(adapter => !registered.some(script => script.id === adapter.automatic.scriptId));
    if (missing.length) await chromeApi.scripting.registerContentScripts(missing.map(adapter => ({
      id: adapter.automatic.scriptId, matches: adapter.automatic.origins, js: [adapter.automatic.observerFile],
      allFrames: false, runAt: 'document_idle', persistAcrossSessions: true,
    })));
    const tabs = await chromeApi.tabs.query({ url: AUTO_ORIGINS });
    // A frozen/discarded tab must not delay the user's setting acknowledgement.
    void Promise.allSettled(tabs.map(attach));
    return true;
  };
  const setEnabled = async requested => {
    generation += 1;
    lastSaved.clear();
    if (requested && !await permitted()) {
      await local.set({ automaticSavingEnabled: false });
      return { enabled: false, error: 'Allow access to chatgpt.com to enable automatic saving.' };
    }
    await local.set({ automaticSavingEnabled: requested });
    if (!requested) {
      const tabs = await chromeApi.tabs.query({ url: AUTO_ORIGINS });
      void Promise.allSettled(tabs.filter(tab => !tab.incognito).map(tab =>
        withTimeout(chromeApi.tabs.sendMessage(tab.id, { type: 'auto-saving-disabled' }), attachmentTimeoutMs)));
    }
    await synchronize();
    if (!requested) await chromeApi.permissions.remove({ origins: AUTO_ORIGINS });
    return { enabled: requested };
  };
  const saveWindow = async (sourceUrl, sender) => {
    const token = generation;
    if (!await enabled()) return { ok: false, enabled: false };
    const selected = resolveCaptureAdapter(sourceUrl);
    // A same-document navigation can retain the sender's document-start URL.
    // Trust the live tab URL checked here and after capture, never a requested URL alone.
    if (!selected || !selected.adapter.capabilities.automaticLoadedDom) return { ok: false };
    const { adapter, identity } = selected;
    const tab = await chromeApi.tabs.get(sender.tab.id);
    if (!tab || tab.incognito || tab.url !== sourceUrl) return { ok: false };
    const frames = await withTimeout(chromeApi.scripting.executeScript({
      target: { tabId: tab.id }, func: adapter.capture,
      args: [{ automatic: true, loadHistory: false }],
    }), 5000);
    const result = frames.find(frame => frame.frameId === 0)?.result;
    if (!result?.ok) return { ok: false, error: result?.error };
    const capture = result.capture;
    if (!captureMatchesIdentity(adapter, identity, capture, true)) return { ok: false };
    const current = await chromeApi.tabs.get(tab.id);
    if (token !== generation || !await enabled() || !current || current.incognito || current.url !== sourceUrl) return { ok: false };
    const bytes = new TextEncoder().encode(JSON.stringify({ ...capture, captured_at: '' }));
    const signature = [...new Uint8Array(await crypto.subtle.digest('SHA-256', bytes))]
      .map(byte => byte.toString(16).padStart(2, '0')).join('');
    if (lastSaved.get(tab.id) === signature) return { ok: true, unchanged: true };
    // Recheck opt-out immediately before submitting. An already submitted durable write cannot be recalled.
    if (token !== generation || !await enabled()) return { ok: false, enabled: false };
    let receipt;
    try {
      receipt = await withTimeout(chromeApi.runtime.sendNativeMessage(NATIVE_HOST, {
        protocol: 1, operation: 'save', capture,
      }), saveTimeoutMs);
    } catch (error) {
      throw Object.assign(new Error(String(error?.message ?? error)), { nativeSubmitted: true });
    }
    if (receipt?.ok === false) throw new Error(String(receipt.error ?? 'Native host rejected the save.').slice(0, 300));
    if (!validReceipt(receipt, 'partial', capture.messages.length)) throw Object.assign(new Error('No valid save receipt.'), { nativeSubmitted: true });
    if (token === generation) lastSaved.set(tab.id, signature);
    await status({ state: 'saved', message: `${receipt.unchanged ? 'Already saved' : 'Saved'} automatically: ${receipt.message_count} loaded messages; partial capture.`, messageCount: receipt.message_count });
    return { ok: true, unchanged: receipt.unchanged };
  };
  const queueWindow = (message, sender) => {
    const tabId = sender.tab.id;
    const existing = pending.get(tabId);
    if (existing) { existing.next = { message, sender }; return existing.promise; }
    const task = { next: { message, sender } };
    task.promise = (async () => {
      let result;
      while (task.next) {
        const next = task.next;
        task.next = null;
        try {
          result = await saveWindow(next.message.sourceUrl, next.sender);
          if (result.error) await status({ state: 'error', message: result.error });
        } catch (error) {
          const uncertain = error?.nativeSubmitted || error?.message === 'MIND_PLACE_TIMEOUT';
          const text = uncertain ? 'Automatic native host acknowledgement is unavailable; it may have been saved. Check Mind Place and its capture setup before retrying.'
            : `Automatic save failed. Check Mind Place → Manual capture setup / native host registration. ${String(error?.message ?? '').slice(0, 300)}`;
          await status({ state: 'error', message: text });
          result = { ok: false, error: text };
        }
      }
      return result;
    })().finally(() => pending.delete(tabId));
    pending.set(tabId, task);
    return task.promise;
  };
  const onMessage = (message, sender, respond) => {
    if (!message || !['auto-page-state', 'auto-saving-state', 'set-auto-saving', 'auto-capture'].includes(message.type)) return false;
    let operation;
    if (message.type === 'auto-page-state' && isPage(sender)) operation = enabled().then(value => ({ enabled: value }));
    else if (message.type === 'auto-saving-state' && isPopup(sender)) operation = Promise.all([enabled(), local.get('automaticSavingStatus')]).then(([value, stored]) => ({ enabled: value, status: stored.automaticSavingStatus }));
    else if (message.type === 'set-auto-saving' && isPopup(sender)) operation = setEnabled(message.enabled === true);
    else if (message.type === 'auto-capture' && isPage(sender)) operation = queueWindow(message, sender);
    else return false;
    operation.then(respond, error => respond({ ok: false, enabled: false, error: String(error?.message ?? error).slice(0, 300) }));
    return true;
  };
  chromeApi.runtime.onMessage.addListener(onMessage);
  const reconcile = () => { void synchronize().catch(() => {}); };
  chromeApi.runtime.onStartup.addListener(reconcile);
  chromeApi.runtime.onInstalled.addListener(reconcile);
  chromeApi.tabs.onUpdated.addListener((id, change, tab) => {
    if (change.status === 'complete' || change.url) void enabled().then(active => active && attach({ ...tab, id })).catch(() => {});
  });
  chromeApi.tabs.onRemoved.addListener(id => { lastSaved.delete(id); });
  chromeApi.permissions.onRemoved.addListener(() => {
    void permitted().then(granted => { if (!granted) return setEnabled(false); }).catch(() => {});
  });
  const ready = synchronize().catch(() => false);
  return { onMessage, ready };
}

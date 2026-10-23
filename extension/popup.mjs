import { resolveCaptureAdapter, captureMatchesIdentity } from './providers/index.mjs';
import { NATIVE_HOST, validReceipt, withTimeout } from './native.mjs';
import { AUTO_ORIGINS } from './automatic.mjs';
const DEFAULT_TIMEOUT_MS = 15_000;

export function initPopup({ chromeApi = globalThis.chrome, documentRef = globalThis.document, saveTimeoutMs = DEFAULT_TIMEOUT_MS } = {}) {
  const button = documentRef?.querySelector('#save');
  const status = documentRef?.querySelector('#status');
  const warning = documentRef?.querySelector('#warning');
  const loadHistory = documentRef?.querySelector('#load-history');
  const stop = documentRef?.querySelector('#stop');
  const cancel = documentRef?.querySelector('#cancel');
  const autoSave = documentRef?.querySelector('#auto-save');
  const autoStatus = documentRef?.querySelector('#auto-status');
  if (!button || !status) return { whenIdle: () => Promise.resolve(), destroy() {} };

  let inFlight = null;
  let autoFlight = null;
  let workerJob = null;
  let pollTimer;
  let destroyed = false;
  let initializing = Boolean(loadHistory && chromeApi?.runtime?.sendMessage);
  const showStatus = (message) => { status.textContent = message; };
  const showWarning = (message = '') => {
    if (!warning) return;
    warning.textContent = message;
    warning.hidden = !message;
  };
  const showFailure = (message) => {
    showWarning('');
    showStatus(message);
  };
  const releaseLostWorker = () => {
    workerJob = null;
    button.disabled = false;
    if (loadHistory) loadHistory.disabled = false;
    if (stop) stop.hidden = true;
    if (cancel) cancel.hidden = true;
    clearTimeout(pollTimer);
    showFailure('Capture status was interrupted. Check the page controls and the archive in Mind Place before retrying.');
  };
  const renderWorkerJob = (job) => {
    if (destroyed || !job) return;
    workerJob = job;
    showStatus(job.status);
    showWarning(job.warning);
    button.disabled = Boolean(job.active);
    if (loadHistory) loadHistory.disabled = Boolean(job.active);
    const canControl = job.active && ['loading', 'stopping'].includes(job.phase);
    if (stop) { stop.hidden = !canControl; stop.disabled = job.phase !== 'loading'; }
    if (cancel) { cancel.hidden = !canControl; cancel.disabled = false; }
    clearTimeout(pollTimer);
    if (job.active) pollTimer = setTimeout(() => { void refreshWorker(); }, 700);
  };
  const refreshWorker = async () => {
    if (destroyed || !chromeApi?.runtime?.sendMessage || !loadHistory) return;
    try {
      const result = await withTimeout(chromeApi.runtime.sendMessage({ type: 'capture-status' }), 5000);
      if (!result?.job && workerJob?.active) { releaseLostWorker(); return; }
      renderWorkerJob(result?.job);
    } catch {
      if (workerJob?.active) releaseLostWorker();
    }
  };
  const runScrollingSave = async () => {
    showWarning('');
    showStatus('Starting manual history loading…');
    const result = await withTimeout(chromeApi.runtime.sendMessage({ type: 'start-capture', loadHistory: true }), 5000);
    if (!result?.job) throw new Error('The manual capture worker could not start.');
    renderWorkerJob(result.job);
  };
  const onControl = (action) => {
    if (!workerJob?.active) return;
    void withTimeout(chromeApi.runtime.sendMessage({ type: 'capture-control', operationId: workerJob.operationId, action }), 5000)
      .then((response) => renderWorkerJob(response?.job))
      .catch(() => showStatus('Use the Stop & save or Cancel controls on the conversation page.'));
  };
  const onStop = () => onControl('stop');
  const onCancel = () => onControl('cancel');
  stop?.addEventListener('click', onStop);
  cancel?.addEventListener('click', onCancel);
  if (initializing) button.disabled = true;
  const initialization = refreshWorker().finally(() => {
    initializing = false;
    if (!workerJob?.active && !inFlight) button.disabled = false;
  });
  const renderAutomatic = response => {
    if (destroyed || !autoSave) return;
    autoSave.checked = response?.enabled === true;
    if (autoStatus) autoStatus.textContent = response?.error || (response?.enabled
      ? response.status?.message || 'ON. Saves loaded messages after changes; never scrolls.'
      : 'OFF. Use Save this conversation to capture manually.');
  };
  const automaticInitialization = autoSave && chromeApi?.runtime?.sendMessage
    ? withTimeout(chromeApi.runtime.sendMessage({ type: 'auto-saving-state' }), 5000)
      .then(renderAutomatic, error => { if (autoStatus) autoStatus.textContent = `Automatic saving status unavailable: ${String(error?.message ?? error).slice(0, 300)}`; })
      .finally(() => { if (!destroyed) autoSave.disabled = false; })
    : Promise.resolve();
  if (autoSave) autoSave.disabled = true;
  const onAutoChange = () => {
    if (autoFlight || !autoSave) return;
    const requested = autoSave.checked;
    let activationStage = requested ? 'requesting ChatGPT permission' : 'updating automatic saving';
    autoSave.disabled = true;
    // Request in the checkbox gesture, before any await, as required by Chromium.
    let permission;
    try { permission = requested ? chromeApi.permissions.request({ origins: AUTO_ORIGINS }) : Promise.resolve(true); }
    catch (error) { permission = Promise.reject(error); }
    autoFlight = Promise.resolve(permission).then(async granted => {
      if (!granted) { renderAutomatic({ enabled: false, error: 'OFF. ChatGPT access was not granted.' }); return; }
      activationStage = 'updating automatic saving';
      const response = await withTimeout(chromeApi.runtime.sendMessage({ type: 'set-auto-saving', enabled: requested }), 10_000);
      if (typeof response?.enabled !== 'boolean') throw new Error('The capture worker returned no setting acknowledgement. Reload the extension.');
      renderAutomatic(response);
    }).catch(async error => {
      if (activationStage === 'requesting ChatGPT permission') autoSave.checked = false;
      const detail = error?.message === 'MIND_PLACE_TIMEOUT' ? 'The setting acknowledgement timed out.' : String(error?.message ?? error).slice(0, 300);
      if (autoStatus) autoStatus.textContent = `Could not change automatic saving (${activationStage}): ${detail}`;
      if (activationStage !== 'requesting ChatGPT permission') {
        try {
          const current = await withTimeout(chromeApi.runtime.sendMessage({ type: 'auto-saving-state' }), 3000);
          if (!destroyed && typeof current?.enabled === 'boolean') autoSave.checked = current.enabled;
        } catch { /* Keep the explicit uncertain acknowledgement visible. */ }
      }
    }).finally(() => { if (!destroyed) autoSave.disabled = false; autoFlight = null; });
  };
  autoSave?.addEventListener('click', onAutoChange);
  const onAutoStorage = (changes, area) => {
    if (destroyed || area !== 'local' || !autoSave) return;
    if (changes.automaticSavingEnabled) autoSave.checked = changes.automaticSavingEnabled.newValue === true;
    if (changes.automaticSavingStatus && autoSave.checked && autoStatus) autoStatus.textContent = changes.automaticSavingStatus.newValue?.message || 'ON. Loaded messages only; never scrolls.';
  };
  chromeApi.storage?.onChanged?.addListener(onAutoStorage);

  const runSave = async () => {
    if (!chromeApi?.tabs || !chromeApi?.scripting || !chromeApi?.runtime) {
      showFailure('Mind Place capture APIs are unavailable. Reopen the extension popup and try again.');
      return;
    }
    const deadline = Date.now() + saveTimeoutMs;
    const bounded = (startRequest) => {
      const remainingMs = deadline - Date.now();
      if (remainingMs <= 0) return Promise.reject(new Error('MIND_PLACE_TIMEOUT'));
      try {
        return withTimeout(Promise.resolve(startRequest()), remainingMs);
      } catch (error) {
        return Promise.reject(error);
      }
    };
    showWarning('');
    showStatus('Checking the active tab…');

    let tab;
    try {
      const activeTabs = await bounded(() => chromeApi.tabs.query({ active: true, currentWindow: true }));
      tab = activeTabs?.[0];
    } catch {
      showFailure('Capture timed out or could not read the active browser tab. Nothing was sent to Mind Place. Open the ChatGPT conversation and try again.');
      return;
    }
    if (!tab || !Number.isInteger(tab.id) || typeof tab.url !== 'string') {
      showFailure('No active web tab is available to capture.');
      return;
    }
    if (tab.incognito === true) {
      showFailure('Private browsing is excluded. Open this conversation in a regular browser window to save it.');
      return;
    }
    const selected = resolveCaptureAdapter(tab.url);
    if (!selected || !selected.adapter.capabilities.manualDom) {
      showFailure('Open a supported saved conversation before saving.');
      return;
    }
    const { adapter, identity } = selected;

    showStatus('Reading the currently rendered conversation…');
    let frames;
    try {
      frames = await bounded(() => chromeApi.scripting.executeScript({
        target: { tabId: tab.id },
        func: adapter.capture,
      }));
    } catch {
      showFailure('Capture timed out or could not read this page. Nothing was sent to Mind Place. Make sure the conversation is open, then try again.');
      return;
    }
    const result = frames?.find(frame => frame.frameId === 0)?.result;
    if (!result?.ok || !result.capture) {
      showFailure(result?.error || 'The conversation could not be read from this page.');
      return;
    }
    const capture = result.capture;
    if (!captureMatchesIdentity(adapter, identity, capture, false)) {
      showFailure('The page identity or capture contract changed during capture. Nothing was sent to Mind Place.');
      return;
    }

    let currentTab;
    try {
      currentTab = await bounded(() => chromeApi.tabs.get(tab.id));
    } catch {
      showFailure('Capture timed out or the browser tab closed while it was being read. Nothing was sent to Mind Place.');
      return;
    }
    if (!currentTab || currentTab.incognito === true || currentTab.url !== identity.sourceUrl) {
      showFailure('The browser tab navigated or changed while the conversation was being read. Nothing was sent to Mind Place.');
      return;
    }

    showStatus('Saving locally and waiting for Mind Place to confirm…');
    let receipt;
    try {
      const request = { protocol: 1, operation: 'save', capture };
      receipt = await bounded(() => chromeApi.runtime.sendNativeMessage(NATIVE_HOST, request));
    } catch (error) {
      if (error?.message === 'MIND_PLACE_TIMEOUT') {
        showFailure('The save acknowledgement timed out. The conversation may have been saved; check Mind Place before retrying.');
      } else {
        showFailure('Mind Place native host is unavailable. Open Mind Place → Settings → Manual capture setup and register this extension.');
      }
      return;
    }
    if (receipt?.ok === false) {
      const detail = typeof receipt.error === 'string' ? ` ${receipt.error.slice(0, 300)}` : '';
      showFailure(`Mind Place rejected the save.${detail}`);
      return;
    }
    if (!validReceipt(receipt, capture.completeness, capture.messages.length)) {
      showFailure('Mind Place did not return a valid save receipt. Check the archive in Mind Place before retrying.');
      return;
    }

    const completeness = receipt.completeness === 'partial' ? 'partial capture' : 'capture';
    const resultPrefix = receipt.unchanged ? 'Already saved locally.' : 'Saved locally.';
    showStatus(`${resultPrefix} ${receipt.message_count} messages; ${completeness}.`);
    const receiptWarnings = [...new Set([...(capture.warnings ?? []), ...receipt.warnings])].slice(0, 4);
    showWarning(receiptWarnings.join(' '));
  };

  const onClick = () => {
    if (initializing || inFlight || workerJob?.active) return;
    button.disabled = true;
    const providerSave = async () => {
      // One-shot saves already bound every browser operation to a deadline.
      // Only query first when the user requested history scrolling, and bound
      // this new routing query too (a suspended Chrome tab may never reply).
      if (!loadHistory?.checked) return runSave();
      if (!chromeApi?.tabs?.query) return runScrollingSave();
      let tabs;
      try {
        tabs = await withTimeout(
          chromeApi.tabs.query({ active: true, currentWindow: true }),
          Math.min(saveTimeoutMs, 5000),
        );
      } catch {
        showFailure('Capture timed out while checking the active tab. Nothing was sent to Mind Place.');
        return;
      }
      const selected = resolveCaptureAdapter(tabs?.[0]?.url ?? '');
      if (!selected || selected.adapter.capabilities.manualHistory) return runScrollingSave();
      loadHistory.checked = false;
      return runSave();
    };
    inFlight = providerSave().catch(() => {
      showFailure('The save could not be completed. Check Mind Place and try again.');
    }).finally(() => {
      button.disabled = Boolean(workerJob?.active);
      inFlight = null;
    });
  };
  button.addEventListener('click', onClick);
  return {
    whenIdle: () => inFlight ?? autoFlight ?? Promise.all([initialization, automaticInitialization]),
    destroy: () => {
      destroyed = true;
      clearTimeout(pollTimer);
      button.removeEventListener('click', onClick);
      stop?.removeEventListener('click', onStop);
      cancel?.removeEventListener('click', onCancel);
      autoSave?.removeEventListener('click', onAutoChange);
      chromeApi.storage?.onChanged?.removeListener(onAutoStorage);
    },
  };
}

if (typeof document !== 'undefined' && globalThis.chrome) initPopup({ chromeApi: globalThis.chrome, documentRef: document });

import { captureConversation, controlCapture, finishCapture, parseChatGptConversationUrl } from './capture.mjs';
import { NATIVE_HOST, validReceipt, withTimeout } from './native.mjs';
import { initAutomaticSaving } from './automatic.mjs';

// Manual jobs require a popup action. The separate automatic coordinator is dormant unless opted in.
export function createCaptureWorker(chromeApi, { saveTimeoutMs = 15_000, captureTimeoutMs = 195_000 } = {}) {
  let job = null;
  const publicJob = () => job && ({ operationId: job.operationId, phase: job.phase, active: job.active, status: job.status, warning: job.warning, messageCount: job.messageCount });
  const setStatus = (target, phase, status) => { target.phase = phase; target.status = status; };
  const control = async (target, action) => {
    if (!Number.isInteger(target.tabId)) return;
    try {
      await withTimeout(chromeApi.scripting.executeScript({ target: { tabId: target.tabId }, func: controlCapture, args: [target.operationId, action] }), 3000);
    } catch { /* A closed/navigated tab has no job left to control. */ }
  };
  const run = async (target, loadHistory) => {
    let port;
    let submitted = false;
    let injectionStarted = false;
    let onMessage;
    let onDisconnect;
    try {
      const tabs = await withTimeout(chromeApi.tabs.query({ active: true, currentWindow: true }), 5000);
      const tab = tabs?.[0];
      if (!tab || !Number.isInteger(tab.id)) throw new Error('No active web tab is available to capture.');
      if (tab.incognito) throw new Error('Private browsing is excluded. Use a regular browser window.');
      const identity = parseChatGptConversationUrl(tab.url);
      if (!identity) throw new Error('Open a saved ChatGPT conversation at chatgpt.com before saving.');
      target.tabId = tab.id;
      target.sourceUrl = identity.sourceUrl;
      if (target.cancelled) throw new Error('Capture cancelled. Nothing was saved.');

      // connectNative keeps the worker alive only for this bounded job, including popup closure.
      // The one-request Rust host waits for input, then exits after its durable response.
      port = chromeApi.runtime.connectNative(NATIVE_HOST);
      const response = new Promise((resolve, reject) => {
        onMessage = resolve;
        onDisconnect = () => {
          const detail = chromeApi.runtime.lastError?.message;
          reject(new Error(submitted
            ? 'The save acknowledgement was lost. The conversation may have been saved; check the archive before retrying.'
            : `Mind Place native host is unavailable. Open Manual capture setup and register Chrome/Edge.${detail ? ` ${detail.slice(0, 160)}` : ''}`));
        };
        port.onMessage.addListener(onMessage);
        port.onDisconnect.addListener(onDisconnect);
      });
      // A disconnect can arrive during browser API calls; retain the rejection until awaited.
      response.catch(() => {});
      setStatus(target, 'loading', loadHistory ? 'Loading conversation history… You can close this popup.' : 'Reading currently loaded messages…');
      injectionStarted = true;
      const frames = await withTimeout(Promise.race([
        chromeApi.scripting.executeScript({
          target: { tabId: tab.id }, func: captureConversation,
          args: [{ loadHistory, operationId: target.operationId, maxDurationMs: Math.max(40, Math.min(180_000, captureTimeoutMs - 15_000)) }],
        }),
        response.then(() => { throw new Error('Unexpected native response before a save request.'); }),
      ]), captureTimeoutMs);
      if (target.cancelled) throw new Error('Capture cancelled. Nothing was saved.');
      const result = frames?.find((frame) => frame.frameId === 0)?.result;
      if (!result?.ok || !result.capture) throw new Error(result?.error || 'The page could not be read; nothing was saved.');
      const capture = result.capture;
      if (capture.provider !== 'chatgpt' || capture.conversation_id !== identity.conversationId
        || capture.source_url !== identity.sourceUrl || capture.capture_method !== 'manual-dom'
        || capture.completeness !== 'partial') throw new Error('The page identity changed during capture. Nothing was sent to Mind Place.');
      const currentTab = await withTimeout(chromeApi.tabs.get(tab.id), 5000);
      if (target.cancelled) throw new Error('Capture cancelled. Nothing was saved.');
      if (!currentTab || currentTab.incognito || currentTab.url !== identity.sourceUrl) throw new Error('The browser tab navigated or changed during capture. Nothing was saved.');
      setStatus(target, 'saving', 'Saving locally and waiting for Mind Place to confirm…');
      submitted = true;
      port.postMessage({ protocol: 1, operation: 'save', capture });
      const receipt = await withTimeout(response, saveTimeoutMs);
      if (receipt?.ok === false) throw new Error(`Mind Place rejected the save. ${String(receipt.error ?? '').slice(0, 300)}`);
      if (!validReceipt(receipt, capture.completeness, capture.messages.length)) throw new Error('No valid save receipt was returned. Check the archive before retrying.');
      target.messageCount = receipt.message_count;
      target.warning = [...new Set([...(capture.warnings ?? []), ...receipt.warnings])].slice(0, 4).join(' ');
      setStatus(target, 'saved', `${receipt.unchanged ? 'Already saved locally.' : 'Saved locally.'} ${receipt.message_count} messages; partial capture.`);
    } catch (error) {
      if (injectionStarted && !submitted) await control(target, 'cancel');
      const message = error?.message === 'MIND_PLACE_TIMEOUT'
        ? submitted ? 'The save acknowledgement timed out. The conversation may have been saved; check the archive before retrying.'
          : 'Loading timed out. Nothing was sent to Mind Place; the page job was cancelled.'
        : error?.message || 'Capture failed. Check Mind Place before retrying.';
      setStatus(target, 'error', message);
    } finally {
      if (port) {
        port.onMessage.removeListener(onMessage);
        port.onDisconnect.removeListener(onDisconnect);
        try { port.disconnect(); } catch { /* Already disconnected. */ }
      }
      target.active = false;
      if (Number.isInteger(target.tabId) && loadHistory) {
        try {
          await withTimeout(chromeApi.scripting.executeScript({ target: { tabId: target.tabId }, func: finishCapture, args: [target.operationId, target.status] }), 3000);
        } catch { /* The final result remains available when the popup is reopened. */ }
      }
    }
  };

  return (message, sender, sendResponse) => {
    if (!message || sender?.id !== chromeApi.runtime.id) return false;
    if (message.type === 'capture-progress') {
      if (job?.active && sender.tab?.id === job.tabId && sender.frameId === 0
        && sender.url === job.sourceUrl && message.operationId === job.operationId
        && Number.isInteger(message.messageCount) && message.messageCount >= 0 && message.messageCount <= 10_000) {
        job.messageCount = message.messageCount;
        job.status = job.phase === 'stopping' ? `Stopping loading… ${job.messageCount} messages collected.`
          : `Loading conversation history… ${job.messageCount} messages collected. You can close this popup.`;
      }
      sendResponse({ ok: true });
      return true;
    }
    if (sender.tab || sender.url !== chromeApi.runtime.getURL('popup.html')) return false;
    if (message.type === 'capture-status') sendResponse({ job: publicJob() });
    else if (message.type === 'start-capture') {
      if (!job?.active) {
        job = { operationId: crypto.randomUUID(), active: true, phase: 'checking', status: 'Checking the active tab…', warning: '', messageCount: 0, cancelled: false };
        void run(job, message.loadHistory === true);
      }
      sendResponse({ job: publicJob() });
    } else if (message.type === 'capture-control' && job?.active && message.operationId === job.operationId) {
      if (!['loading', 'stopping'].includes(job.phase) || !['stop', 'cancel'].includes(message.action)) { sendResponse({ job: publicJob() }); return true; }
      if (message.action === 'cancel') job.cancelled = true;
      else setStatus(job, 'stopping', 'Stopping loading and preparing the collected history…');
      void control(job, message.action);
      sendResponse({ job: publicJob() });
    } else return false;
    return true;
  };
}

if (globalThis.chrome?.runtime?.onMessage) {
  chrome.runtime.onMessage.addListener(createCaptureWorker(chrome));
  initAutomaticSaving(chrome);
}

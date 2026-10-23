import { chatGptAdapter } from './chatgpt.mjs';
import { claudeAdapter } from './claude.mjs';
export { captureMatchesIdentity } from './contract.mjs';

// Statically bundled adapters only. Tab/page messages cannot register code or origins.
/** @type {readonly import('./contract.mjs').CaptureAdapter[]} */
export const CAPTURE_ADAPTERS = Object.freeze([chatGptAdapter, claudeAdapter]);

export function captureAdapterForPage(sourceUrl) {
  try {
    const url = new URL(sourceUrl);
    if (url.protocol !== 'https:' || url.username || url.password) return null;
    return CAPTURE_ADAPTERS.find(adapter => adapter.pageOrigins.includes(url.origin)) ?? null;
  } catch { return null; }
}

export function resolveCaptureAdapter(sourceUrl) {
  const adapter = captureAdapterForPage(sourceUrl);
  const identity = adapter?.parseConversationUrl(sourceUrl);
  return identity ? { adapter, identity } : null;
}

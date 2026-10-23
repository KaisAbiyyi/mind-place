// MP-7E: discoverable, read-only projection of the shipped provider adapters.
// This is NOT an authorization list: browser admission and native validation are
// independent, and a matrix entry must never grant a new site or capture mode.
import { CAPTURE_ADAPTERS } from './index.mjs';

export const PROVIDER_CAPABILITIES = Object.freeze([
  ...CAPTURE_ADAPTERS.map(adapter => Object.freeze({
    provider: adapter.id,
    surface: 'browser-active-tab',
    captureMethod: 'manual-dom',
    manualLoadedDom: adapter.capabilities.manualDom,
    manualHistory: adapter.capabilities.manualHistory,
    automaticLoadedDom: adapter.capabilities.automaticLoadedDom,
    pageOrigins: Object.freeze([...adapter.pageOrigins]),
    automaticOrigins: Object.freeze([...(adapter.automatic?.origins ?? [])]),
  })),
  Object.freeze({
    provider: 'generic',
    surface: 'explicit-owner-file',
    captureMethod: 'generic-import',
    manualLoadedDom: false,
    manualHistory: false,
    automaticLoadedDom: false,
    pageOrigins: Object.freeze([]),
    automaticOrigins: Object.freeze([]),
    formats: Object.freeze(['json', 'html', 'markdown']),
  }),
]);

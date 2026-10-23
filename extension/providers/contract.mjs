// Browser admission only. The native host independently validates before storage.
// Never normalize here: preserve the adapter's raw observation and V1 identities.
const bytes = value => new TextEncoder().encode(value).length;
const text = (value, max, nonempty = false) => typeof value === 'string'
  && (!nonempty || value.trim().length > 0) && bytes(value) <= max;
const timestamp = value => text(value, 80, true)
  && /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$/i.test(value)
  && Number.isFinite(Date.parse(value));
const optional = (value, check) => value === undefined || check(value);

/**
 * @typedef {{conversationId: string, sourceUrl: string}} CaptureIdentity
 * @typedef {object} CaptureAdapter
 * @property {1} contractVersion Browser adapter interface, independent of archive schema.
 * @property {string} id Exact persisted provider ID.
 * @property {readonly string[]} pageOrigins Exact HTTPS origins, not suffix matches.
 * @property {(url: string) => CaptureIdentity | null} parseConversationUrl No I/O.
 * @property {{manualDom: boolean, manualHistory: boolean, automaticLoadedDom: boolean}} capabilities
 * @property {Function} capture Self-contained executeScript function returning {ok,capture? ,error?}.
 * @property {Function} control Self-contained bounded job Stop/Cancel control.
 * @property {Function} finish Self-contained job UI cleanup.
 * @property {{scriptId: string, origins: readonly string[], observerFile: string} | null} automatic
 */

/** Validate a normalized V1 browser capture against the selected identity/mode.
 * This returns a boolean and does not rewrite evidence or grant any authority.
 * @param {CaptureAdapter} adapter
 * @param {CaptureIdentity} identity
 * @param {unknown} capture
 * @param {boolean} automatic
 */
export function captureMatchesIdentity(adapter, identity, capture, automatic) {
  if (!capture || typeof capture !== 'object' || adapter.contractVersion !== 1
    || !(automatic ? adapter.capabilities.automaticLoadedDom : adapter.capabilities.manualDom)) return false;
  const c = capture;
  if (c.schema_version !== 1 || c.provider !== adapter.id
    || c.conversation_id !== identity.conversationId || c.source_url !== identity.sourceUrl
    || c.capture_method !== (automatic ? 'automatic-dom' : 'manual-dom') || c.completeness !== 'partial'
    || !text(c.source_url, 32768, true) || !text(c.title, 1024, true) || !timestamp(c.captured_at)
    || !optional(c.conversation_created_at, timestamp) || !optional(c.conversation_updated_at, timestamp)
    || !Array.isArray(c.warnings) || !c.warnings.length || c.warnings.length > 32
    || !c.warnings.every(w => text(w, 2048))
    || !Array.isArray(c.messages) || !c.messages.length || c.messages.length > 10_000) return false;
  const ids = new Set();
  for (const m of c.messages) {
    if (!m || !text(m.id, 256, true) || ids.has(m.id)
      || !['user', 'assistant', 'system', 'tool'].includes(m.role)
      || !['plain', 'markdown'].includes(m.format) || !text(m.text, 4 * 1024 * 1024)
      || !optional(m.authored_at, timestamp) || !optional(m.model, value => text(value, 256))
      || !Array.isArray(m.attachments) || m.attachments.length > 100) return false;
    ids.add(m.id);
    for (const a of m.attachments) {
      if (!a || !text(a.id, 256, true) || !text(a.name, 1024) || !text(a.mime_type, 128)
        // Browser observations may contain references/inline media, never archive paths.
        || Object.hasOwn(a, 'asset_path')
        || !optional(a.source_url, value => text(value, 32768))
        || !optional(a.data_url, value => text(value, 8 * 1024 * 1024 * 4 / 3 + 128))
        || !optional(a.unavailable_reason, value => text(value, 2048))) return false;
    }
  }
  try { return bytes(JSON.stringify(c)) <= 31 * 1024 * 1024; }
  catch { return false; }
}

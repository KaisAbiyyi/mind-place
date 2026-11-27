/**
 * MP-SEC-11 untrusted-output envelope verifier (provider-neutral).
 * Pure functions over a tool result. They judge the SHAPE of the retrieval-output
 * boundary (docs/security/mp-sec-11-output-boundary.md); they never judge a
 * model's behaviour. The same rules are pinned by
 * test/vectors/mp-sec-11-output-boundary-vectors.v1.json.
 */
export const ENVELOPE_KEY = '_mp_boundary';
export const ENVELOPE_SCHEMA = 'mp-sec-11/untrusted-envelope/v1';
export const CONTENT_ORIGINS = ['archived_capture', 'memory_claim'];
export const ASSERTED_BY = ['user', 'model', 'import', 'deterministic', 'unknown'];
export const MAX_PROVENANCE_ENTRIES = 64;
const REQUIRED = ['schema', 'trust', 'instructions_allowed', 'notice', 'tool', 'content_origin', 'provenance', 'provenance_complete'];

const isObject = (v) => v !== null && typeof v === 'object' && !Array.isArray(v);
const nonEmpty = (v) => typeof v === 'string' && v.length > 0;

/** @returns {{ok: boolean, reason?: string}} */
export function verifyBoundaryEnvelope(structured, tool) {
  if (!isObject(structured)) return { ok: false, reason: 'result is not an object' };
  const env = structured[ENVELOPE_KEY];
  if (!isObject(env)) return { ok: false, reason: 'untrusted-data envelope missing' };
  for (const field of REQUIRED) if (!(field in env)) return { ok: false, reason: `envelope field ${field} missing` };
  if (env.schema !== ENVELOPE_SCHEMA) return { ok: false, reason: 'unknown envelope schema' };
  if (env.trust !== 'untrusted_data') return { ok: false, reason: 'content not marked untrusted_data' };
  if (env.instructions_allowed !== false) return { ok: false, reason: 'envelope allows instructions' };
  if (!nonEmpty(env.notice)) return { ok: false, reason: 'notice missing' };
  if (tool !== undefined && env.tool !== tool) return { ok: false, reason: `envelope names tool ${String(env.tool)}` };
  if (!CONTENT_ORIGINS.includes(env.content_origin)) return { ok: false, reason: 'unknown content_origin' };
  if (typeof env.provenance_complete !== 'boolean') return { ok: false, reason: 'provenance_complete is not boolean' };
  if (!Array.isArray(env.provenance) || env.provenance.length > MAX_PROVENANCE_ENTRIES) return { ok: false, reason: 'provenance not a bounded array' };
  for (const [i, row] of env.provenance.entries()) {
    if (!isObject(row) || !nonEmpty(row.source_id)) return { ok: false, reason: `provenance[${i}] has no source_id` };
    if (env.content_origin === 'memory_claim' && !(nonEmpty(row.claim_id) && ASSERTED_BY.includes(row.asserted_by))) {
      return { ok: false, reason: `provenance[${i}] memory claim lacks claim_id or asserted_by` };
    }
    if (env.content_origin === 'archived_capture' && row.asserted_by != null) return { ok: false, reason: `provenance[${i}] archive evidence must not carry asserted_by` };
  }
  return { ok: true };
}

/**
 * The text block is what a model actually reads: it must be the same object with
 * the envelope serialized FIRST, before any retrieved content.
 */
export function verifyBoundaryText(text, structured) {
  if (typeof text !== 'string' || !text.startsWith(`{"${ENVELOPE_KEY}":{`)) return { ok: false, reason: 'text block does not start with the envelope' };
  let parsed;
  try { parsed = JSON.parse(text); } catch { return { ok: false, reason: 'text block is not JSON' }; }
  if (JSON.stringify(parsed[ENVELOPE_KEY]) !== JSON.stringify(structured?.[ENVELOPE_KEY])) return { ok: false, reason: 'text envelope differs from structuredContent' };
  if (Object.keys(parsed).sort().join() !== Object.keys(structured ?? {}).sort().join()) return { ok: false, reason: 'text and structuredContent differ in keys' };
  return { ok: true };
}

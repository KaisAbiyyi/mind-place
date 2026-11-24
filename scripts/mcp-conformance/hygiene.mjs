/**
 * Shared credential and diagnostic output hygiene for the MCP conformance harness.
 *
 * Every diagnostic string, blocked-client record, console line, error message,
 * and optional debug artifact MUST pass through this module before being printed
 * or written to disk.
 */
import fs from 'node:fs';
import path from 'node:path';
import { containsSecret, scrubText } from '../lib/e2e-secret-guard.mjs';

const QUERY_SECRET_RX = /([?&](?:access_token|token|api_key|apikey|key|secret|auth|password)=)([^&\s"']+)/gi;
const HEADER_SECRET_RX = /\b(authorization|proxy-authorization|x-api-key|cookie|set-cookie)\s*:\s*([^\r\n]+)/gi;

/** Scrub known secrets, secret-shaped tokens, query secret params, and sensitive header values. */
export function scrubDiagnostic(text, secrets = []) {
  // Scrub pattern-matched secret shapes (e.g. "Bearer <token>") before literal values
  // so "Bearer [redacted]" does not leave the "Bearer " prefix behind.
  let out = scrubText(String(text ?? ''), []);
  out = scrubText(out, secrets);
  out = out.replace(QUERY_SECRET_RX, '$1[redacted]');
  out = out.replace(HEADER_SECRET_RX, '$1: [redacted]');
  if (containsSecret(out, secrets)) return '[redacted-diagnostic]';
  return out;
}

/** Fail-closed cap + scrub for any bounded diagnostic preview string. Always scrubs BEFORE truncating. */
export function safePreview(text, secrets = [], maxLen = 240, fromEnd = false) {
  const scrubbed = scrubDiagnostic(String(text ?? ''), secrets);
  if (maxLen <= 0) return '';
  const clipped = fromEnd ? scrubbed.slice(-maxLen) : scrubbed.slice(0, maxLen);
  if (containsSecret(clipped, secrets)) return '[redacted-diagnostic]';
  return clipped;
}

/** Strip query strings and scrub the path segment from an HTTP URL. */
export function safeUrlPath(rawUrl, secrets = []) {
  const full = String(rawUrl ?? '');
  const qIndex = full.indexOf('?');
  const pathOnly = qIndex >= 0 ? full.slice(0, qIndex) : full;
  return {
    path: safePreview(pathOnly, secrets, 240, false),
    hadQuery: qIndex >= 0,
  };
}

/** Recursively scrub string values inside plain objects/arrays. */
export function scrubValue(value, secrets = []) {
  if (typeof value === 'string') return scrubDiagnostic(value, secrets);
  if (Array.isArray(value)) return value.map((item) => scrubValue(item, secrets));
  if (value && typeof value === 'object') {
    const out = {};
    for (const [k, v] of Object.entries(value)) {
      out[k] = scrubValue(v, secrets);
    }
    return out;
  }
  return value;
}

/**
 * Build a minimal allowlisted blocked-client diagnostic object.
 * Never passes raw stdout/stderr or arbitrary nested properties through unscrubbed.
 */
export function sanitizeBlocked(blocked, secrets = []) {
  if (!blocked || typeof blocked !== 'object') return null;
  const out = {
    kind: safePreview(blocked.kind ?? 'UNKNOWN', secrets, 80),
    detail: safePreview(blocked.detail ?? '', secrets, 280),
    reason: safePreview(blocked.reason ?? '', secrets, 280),
    requestsSeen: Number.isFinite(blocked.requestsSeen) ? blocked.requestsSeen : 0,
    exit: blocked.exit ?? null,
    serverStatus: blocked.serverStatus == null ? null : safePreview(blocked.serverStatus, secrets, 60),
    received: Number.isFinite(blocked.received) ? blocked.received : 0,
  };
  if (blocked.stderrPreview != null || blocked.stderr != null) {
    out.stderrPreview = safePreview(blocked.stderrPreview ?? blocked.stderr, secrets, 240, true);
  }
  if (blocked.stdoutPreview != null || blocked.stdoutTail != null) {
    out.stdoutPreview = safePreview(blocked.stdoutPreview ?? blocked.stdoutTail, secrets, 240, true);
  }
  return out;
}

/** Format a single console line safely; fail-closed if a secret still remains. */
export function formatSafeLine(text, secrets = []) {
  const scrubbed = scrubDiagnostic(text, secrets);
  return containsSecret(scrubbed, secrets) ? '[redacted-line]' : scrubbed;
}

/** Format JSON safely for console or log output; fail-closed if a secret still remains. */
export function formatSafeJson(value, secrets = [], indent = 2) {
  const scrubbed = scrubValue(value, secrets);
  const json = JSON.stringify(scrubbed, null, indent);
  const text = scrubDiagnostic(json, secrets);
  if (containsSecret(text, secrets)) {
    throw new Error('Refused to emit JSON containing a secret');
  }
  return text;
}

/**
 * Write an optional debug artifact (when MP_DEBUG_DIR is enabled) through the shared
 * scrubbing + fail-closed check. Never writes raw secrets to disk.
 */
export function writeDebugArtifact(dir, filename, content, secrets = []) {
  if (!dir) return null;
  if (!/^[\w.-]+\.(log|txt|json)$/.test(filename)) {
    throw new Error(`Unsafe debug artifact filename: ${filename}`);
  }
  const scrubbed = scrubDiagnostic(content, secrets);
  if (containsSecret(scrubbed, secrets)) {
    throw new Error(`Debug artifact ${filename} refused: secret remained after scrubbing`);
  }
  fs.mkdirSync(dir, { recursive: true });
  const target = path.join(dir, filename);
  fs.writeFileSync(target, scrubbed, 'utf8');
  return target;
}

/**
 * Fail-closed secret hygiene for end-to-end harness evidence.
 *
 * Rule: a live credential (MCP access key, unlock secret, hosted key) must never
 * reach a screenshot, log line, receipt, config file or failure artifact.
 *  - Every screenshot goes through `guardedScreenshot`. It hides every element
 *    that shows a secret (text, form value, attribute), re-scans, and refuses to
 *    capture when anything secret-shaped is still visible or the scan itself fails.
 *  - Every string that leaves the process goes through `scrubText`.
 *  - After a run, `assertNoSecretsOnDisk` scans the output directory and rejects
 *    files that were not produced through the guard.
 */
import fs from 'node:fs';
import path from 'node:path';

/** Shapes of secrets this product issues or accepts. Matched even when the literal value is unknown. */
export const SECRET_SOURCES = [
  'mpc_v1_[0-9A-Za-z]{8,}', // local MCP access key
  'Bearer\\s+[A-Za-z0-9._~+/=-]{12,}', // any bearer header value
  'sk-[A-Za-z0-9_-]{16,}', // hosted-provider API key
];
const PATTERN = new RegExp(SECRET_SOURCES.join('|'), 'g');
const LITERAL_MIN = 8;

const usable = (secrets) => (secrets ?? []).filter((s) => typeof s === 'string' && s.length >= LITERAL_MIN);

/** Replace every known literal and every secret-shaped token. */
export function scrubText(text, secrets = []) {
  let out = String(text ?? '');
  for (const secret of usable(secrets)) out = out.split(secret).join('[redacted]');
  return out.replace(PATTERN, '[redacted]');
}

export function containsSecret(text, secrets = []) {
  const value = String(text ?? '');
  return usable(secrets).some((s) => value.includes(s)) || new RegExp(SECRET_SOURCES.join('|')).test(value);
}

/**
 * Runs inside the page. Hides every element that carries a secret, then reports
 * how many secret carriers are still visible. Returns a token used to restore.
 */
function hideSecretsInPage({ literals, sources }) {
  const rx = new RegExp(sources.join('|'));
  const has = (value) => {
    if (!value) return false;
    const text = String(value);
    return literals.some((s) => text.includes(s)) || rx.test(text);
  };
  const carriers = [];
  const seen = new Set();
  const consider = (element) => {
    if (!element || seen.has(element)) return;
    seen.add(element);
    carriers.push(element);
  };
  const visit = (root) => {
    for (const element of root.querySelectorAll('*')) {
      if (element.shadowRoot) visit(element.shadowRoot);
      const tag = element.tagName;
      if (tag === 'SCRIPT' || tag === 'STYLE') continue;
      for (const attribute of element.getAttributeNames()) {
        if (has(element.getAttribute(attribute))) consider(element);
      }
      if ('value' in element && has(element.value)) consider(element);
      for (const node of element.childNodes) {
        if (node.nodeType === 3 && has(node.nodeValue)) consider(element);
      }
    }
  };
  visit(document);
  const restore = [];
  for (const element of carriers) {
    restore.push([element, element.style.getPropertyValue('visibility'), element.style.getPropertyPriority('visibility')]);
    element.style.setProperty('visibility', 'hidden', 'important');
  }
  window.__mpGuardRestore = restore;
  // Anything secret-shaped that is still rendered visible after hiding is a leak.
  const stillVisible = carriers.filter((element) => {
    const style = getComputedStyle(element);
    return style.visibility !== 'hidden' && style.display !== 'none';
  });
  // Descendants of a hidden carrier inherit visibility unless they override it.
  const overridden = [];
  for (const element of carriers) {
    for (const child of element.querySelectorAll('*')) {
      const style = getComputedStyle(child);
      if (style.visibility === 'visible' && (has(child.textContent) || ('value' in child && has(child.value)))) overridden.push(child);
    }
  }
  return { hidden: carriers.length, leaked: stillVisible.length + overridden.length };
}

function restoreSecretsInPage() {
  const restore = window.__mpGuardRestore ?? [];
  for (const [element, value, priority] of restore) {
    if (value) element.style.setProperty('visibility', value, priority);
    else element.style.removeProperty('visibility');
  }
  delete window.__mpGuardRestore;
}

/** Names of images this process wrote through the guard, per output directory. */
const registry = new Map();
const dirKey = (dir) => path.resolve(dir);
export function recordGuarded(dir, name) {
  const key = dirKey(dir);
  if (!registry.has(key)) registry.set(key, new Set());
  registry.get(key).add(name);
}
export function guardedNames(dir) {
  return [...(registry.get(dirKey(dir)) ?? [])];
}

/**
 * Capture `name` into `dir` with every secret hidden. Fail-closed: if the page
 * cannot be scanned, or a secret is still visible after hiding, nothing is written.
 * Returns true when a file was written.
 */
export async function guardedScreenshot(page, dir, name, { secrets = [], fullPage = true } = {}) {
  if (!/^[\w.-]+\.png$/.test(name)) throw new Error('Unsafe evidence file name');
  const literals = usable(secrets);
  let scan;
  try {
    scan = await page.evaluate(hideSecretsInPage, { literals, sources: SECRET_SOURCES });
  } catch (error) {
    // The page could not be inspected: do not capture. Try to undo any partial hiding.
    try { await page.evaluate(restoreSecretsInPage); } catch { /* ignore */ }
    throw new Error('Secret guard could not inspect the page; screenshot skipped (' + scrubText(error?.message, secrets).slice(0, 120) + ')');
  }
  try {
    if (!scan || scan.leaked > 0) {
      throw new Error(`Secret guard refused the screenshot: ${scan?.leaked ?? 'unknown'} secret carrier(s) still visible after hiding`);
    }
    fs.mkdirSync(dir, { recursive: true });
    await page.screenshot({ path: path.join(dir, name), fullPage });
    recordGuarded(dir, name);
    return true;
  } finally {
    try { await page.evaluate(restoreSecretsInPage); } catch { /* the page is going away */ }
  }
}

const TEXT_FILE = /\.(json|txt|log|md|mjs|js|ts|yml|yaml|toml|cfg|conf|env|ini|html|csv)$/i;

/**
 * Scan an evidence directory after a run. Text files must contain no secrets, and
 * every image must have been produced through `guardedScreenshot` in this process.
 * Returns a list of problems (empty means clean).
 */
export function findSecretsOnDisk(dir, secrets = []) {
  const problems = [];
  if (!fs.existsSync(dir)) return problems;
  const allowed = new Set(guardedNames(dir));
  const walk = (current) => {
    for (const entry of fs.readdirSync(current, { withFileTypes: true })) {
      const full = path.join(current, entry.name);
      const rel = path.relative(dir, full).replace(/\\/g, '/');
      if (entry.isDirectory()) { walk(full); continue; }
      if (/\.(png|jpe?g|webp|gif|webm|mp4|zip)$/i.test(entry.name)) {
        if (!allowed.has(rel)) problems.push(`${rel}: image not produced through the secret guard`);
        continue;
      }
      const looksText = TEXT_FILE.test(entry.name);
      const raw = fs.readFileSync(full);
      const body = looksText ? raw.toString('utf8') : raw.toString('latin1');
      if (containsSecret(body, secrets)) problems.push(`${rel}: contains a secret`);
    }
  };
  walk(dir);
  return problems;
}

/** Remove every unguarded image and any secret-bearing file. Returns what was removed. */
export function purgeUnsafeEvidence(dir, secrets = []) {
  const removed = [];
  for (const problem of findSecretsOnDisk(dir, secrets)) {
    const rel = problem.split(':')[0];
    try { fs.rmSync(path.join(dir, rel), { force: true }); removed.push(rel); } catch { /* ignore */ }
  }
  return removed;
}

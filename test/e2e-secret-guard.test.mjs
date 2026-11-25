import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { JSDOM } from 'jsdom';
import {
  findSecretsOnDisk,
  guardedNames,
  guardedScreenshot,
  purgeUnsafeEvidence,
  scrubText,
} from '../scripts/lib/e2e-secret-guard.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const TOKEN = `mpc_v1_${'c0ffee12'.repeat(8)}`;
const UNLOCK = 'unlock-secret-0123456789abcdef';

/** A page whose evaluate() runs inside jsdom and whose screenshot() records what is visible at capture time. */
function fakePage(html, { failEvaluate = false, forceLeak = false, secrets = [TOKEN, UNLOCK] } = {}) {
  const dom = new JSDOM(`<!doctype html><body>${html}</body>`, { pretendToBeVisual: true });
  const visibleAtCapture = [];
  let evaluations = 0;
  const page = {
    dom,
    visibleAtCapture,
    shots: 0,
    async evaluate(fn, arg) {
      evaluations += 1;
      if (failEvaluate) throw new Error(`cannot inspect ${TOKEN}`);
      Object.assign(globalThis, { document: dom.window.document, window: dom.window, getComputedStyle: dom.window.getComputedStyle.bind(dom.window) });
      const result = fn(arg);
      return forceLeak && result && typeof result.leaked === 'number' ? { ...result, leaked: 1 } : result;
    },
    async screenshot({ path: file }) {
      this.shots += 1;
      for (const element of dom.window.document.querySelectorAll('*')) {
        const style = dom.window.getComputedStyle(element);
        const own = [...element.childNodes].filter((n) => n.nodeType === 3).map((n) => n.nodeValue).join('');
        const value = 'value' in element ? String(element.value) : '';
        if (style.visibility !== 'hidden' && secrets.some((s) => own.includes(s) || value.includes(s))) {
          visibleAtCapture.push(element.tagName);
        }
      }
      fs.writeFileSync(file, Buffer.from('89504e470d0a1a0a', 'hex'));
    },
    get evaluations() { return evaluations; },
  };
  return page;
}

const tmp = () => fs.mkdtempSync(path.join(os.tmpdir(), 'mp-guard-'));

test('scrubText removes literals and secret-shaped tokens everywhere', () => {
  const text = `key=${TOKEN} header=Authorization: Bearer abcdefghijklmnop12 openai=sk-abcdefghijklmnopqrstuvwx pw=${UNLOCK}`;
  const clean = scrubText(text, [UNLOCK]);
  assert.doesNotMatch(clean, /mpc_v1_|abcdefghijklmnop12|sk-abcdef|unlock-secret/);
  assert.equal(scrubText(undefined, [TOKEN]), '');
  assert.equal(scrubText('nothing here', [TOKEN]), 'nothing here');
});

test('a screenshot taken while the key is on screen hides the key at capture time and restores the page', async () => {
  const dir = tmp();
  const page = fakePage(`<p>Copy your key now</p><code class="ui2-tech-code">${TOKEN}</code><input value="${TOKEN}"><button aria-label="${TOKEN}">x</button>`);
  const wrote = await guardedScreenshot(page, dir, '05-revoke.png', { secrets: [TOKEN] });
  assert.equal(wrote, true);
  assert.deepEqual(page.visibleAtCapture, [], 'no element showing the key was visible while capturing');
  assert.ok(fs.existsSync(path.join(dir, '05-revoke.png')));
  assert.deepEqual(guardedNames(dir), ['05-revoke.png']);
  // The page is left exactly as it was.
  assert.equal(page.dom.window.document.querySelector('code').style.visibility, '');
  assert.equal(page.dom.window.document.querySelector('code').textContent, TOKEN);
});

test('induced failure after issuing the key: the failure capture also hides the key', async () => {
  const dir = tmp();
  const page = fakePage(`<code>${TOKEN}</code><input type="password" value="${UNLOCK}">`);
  // Same call the harness makes in its catch path.
  await guardedScreenshot(page, dir, 'failure.png', { secrets: [TOKEN, UNLOCK] });
  assert.deepEqual(page.visibleAtCapture, []);
});

test('fail-closed: if the page cannot be inspected, no image is written and the error carries no secret', async () => {
  const dir = tmp();
  const page = fakePage(`<code>${TOKEN}</code>`, { failEvaluate: true });
  await assert.rejects(
    () => guardedScreenshot(page, dir, 'failure.png', { secrets: [TOKEN] }),
    (error) => /could not inspect/.test(error.message) && !error.message.includes(TOKEN),
  );
  assert.equal(page.shots, 0);
  assert.equal(fs.existsSync(path.join(dir, 'failure.png')), false);
});

test('fail-closed: a secret that is still visible after hiding blocks the capture', async () => {
  const dir = tmp();
  const page = fakePage(`<code>${TOKEN}</code>`, { forceLeak: true });
  await assert.rejects(() => guardedScreenshot(page, dir, '03.png', { secrets: [TOKEN] }), /refused the screenshot/);
  assert.equal(page.shots, 0);
  assert.equal(fs.existsSync(path.join(dir, '03.png')), false);
});

test('unsafe evidence names are rejected before anything runs', async () => {
  const page = fakePage('<p>x</p>');
  await assert.rejects(() => guardedScreenshot(page, tmp(), '../escape.png', { secrets: [] }), /Unsafe evidence file name/);
  assert.equal(page.evaluations, 0);
});

test('findSecretsOnDisk flags secret text, unguarded images, and purge removes them', () => {
  const dir = tmp();
  fs.writeFileSync(path.join(dir, 'receipt.json'), JSON.stringify({ detail: `leaked ${TOKEN}` }));
  fs.writeFileSync(path.join(dir, '02-key-shown-once.png'), Buffer.from('89504e47', 'hex'));
  fs.writeFileSync(path.join(dir, 'ok.json'), JSON.stringify({ detail: 'clean' }));
  const problems = findSecretsOnDisk(dir, [TOKEN]);
  assert.equal(problems.length, 2);
  assert.ok(problems.some((p) => p.startsWith('receipt.json')));
  assert.ok(problems.some((p) => p.startsWith('02-key-shown-once.png')));
  assert.deepEqual(purgeUnsafeEvidence(dir, [TOKEN]).sort(), ['02-key-shown-once.png', 'receipt.json']);
  assert.deepEqual(findSecretsOnDisk(dir, [TOKEN]), []);
});

test('the native E2E harness only captures through the guard and never prints a secret', () => {
  const source = fs.readFileSync(path.join(root, 'scripts', 'mp7mcp02-e2e-native.mjs'), 'utf8');
  assert.doesNotMatch(source, /page\.screenshot\(/, 'screenshots must go through guardedScreenshot');
  assert.match(source, /guardedScreenshot\(/);
  assert.doesNotMatch(source, /02-key-shown-once/, 'the key page is never captured');
  assert.doesNotMatch(source, /assert\.match\(token/, 'a failed match would print the live key');
  assert.match(source, /scrubText\(JSON\.stringify\(receipt/);
  assert.match(source, /findSecretsOnDisk\(out, secrets\)/);
  // Every screenshot call site runs after the key exists, so each must be the guarded helper.
  for (const line of source.split('\n')) {
    if (/\bshot\(/.test(line) && !/async function shot/.test(line)) assert.match(line, /await shot\('[\w.-]+\.png'\)/);
  }
});

test('tracked evidence contains no live credential and no unguarded post-issuance screenshot', () => {
  const evidence = path.join(root, 'docs', 'ui', 'evidence');
  const textFiles = [];
  const walk = (dir) => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) walk(full);
      else if (/\.(json|md|txt|html|log)$/i.test(entry.name)) textFiles.push(full);
    }
  };
  walk(evidence);
  for (const file of textFiles) {
    const body = fs.readFileSync(file, 'utf8');
    assert.doesNotMatch(body, /mpc_v1_[0-9a-f]{16,}/, `${path.relative(root, file)} contains an access key`);
    assert.doesNotMatch(body, /Bearer\s+[A-Za-z0-9._~+/=-]{12,}/, `${path.relative(root, file)} contains a bearer value`);
  }
  // The packaged-app E2E screenshots that remain were captured before the key existed
  // or while it was covered by a dialog. The one that showed a real (revoked, disposable)
  // key, 05-revoke-inline-confirm.png, must stay deleted.
  const e2e = fs.readdirSync(path.join(evidence, 'mp-7mcp-02', 'e2e')).sort();
  assert.equal(e2e.includes('05-revoke-inline-confirm.png'), false);
  assert.equal(e2e.includes('02-key-shown-once.png'), false);
  assert.equal(e2e.includes('failure.png'), false);
});

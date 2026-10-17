import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { createHash } from 'node:crypto';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('automatic capture origin access is optional and manual capture keeps its narrow capabilities', async () => {
  const manifest = JSON.parse(await readFile(path.join(root, 'extension', 'manifest.json'), 'utf8'));
  const publicKey = (await readFile(path.join(root, 'extension', 'public-key.txt'), 'utf8')).trim();
  const extensionId = [...createHash('sha256').update(Buffer.from(publicKey, 'base64')).digest('hex').slice(0, 32)]
    .map((digit) => String.fromCharCode(97 + Number.parseInt(digit, 16)))
    .join('');

  assert.equal(manifest.manifest_version, 3);
  assert.equal(manifest.key, publicKey);
  assert.equal(extensionId, 'dninfeidggaaioioficnigfpfjkdicjh');
  assert.deepEqual(manifest.permissions, ['activeTab', 'scripting', 'nativeMessaging', 'storage']);
  assert.equal(manifest.action.default_popup, 'popup.html');
  assert.equal(Object.hasOwn(manifest, 'content_scripts'), false);
  assert.deepEqual(manifest.background, { service_worker: 'worker.mjs', type: 'module' });
  assert.ok(Number(manifest.minimum_chrome_version) >= 105);
  assert.equal(Object.hasOwn(manifest, 'host_permissions'), false);
  assert.deepEqual(manifest.optional_host_permissions, ['https://chatgpt.com/*']);
  assert.equal(manifest.permissions.includes('debugger'), false);
});

import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { test } from 'node:test';
import { stageProduct } from '../scripts/tauri.mjs';
import { downloadPinnedAsset, verifyPinnedFile } from '../scripts/tunnel-helper.mjs';

test('pinned downloads reject corruption/oversize and never trust a stale cache', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mp-helper-download-'));
  try {
    const destination = path.join(root, 'helper.zip');
    const payload = Buffer.from('verified fixture');
    const pin = { bytes: payload.length, sha256: createHash('sha256').update(payload).digest('hex') };
    const url = 'https://github.com/openai/tunnel-client/releases/download/fixture/helper.zip';
    await assert.rejects(downloadPinnedAsset(url, destination, pin, async () => new Response('corrupt fixture')), /checksum|size/);
    assert.equal(fs.existsSync(destination), false);
    await assert.rejects(downloadPinnedAsset(url, destination, pin, async () => new Response(Buffer.alloc(100))), /exceeds/);
    fs.writeFileSync(destination, 'stale');
    let downloads = 0;
    await downloadPinnedAsset(url, destination, pin, async () => { downloads++; return new Response(payload); });
    verifyPinnedFile(destination, pin.sha256, pin.bytes);
    await downloadPinnedAsset(url, destination, pin, async () => assert.fail('valid cache should work offline'));
    assert.equal(downloads, 1);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});

test('Windows portable payload includes the verified helper, notices and receipt', { skip: process.platform !== 'win32' }, () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mp-package-'));
  try {
    const source = path.join(root, 'source');
    const extension = path.join(root, 'extension');
    const helper = path.join(root, 'helper');
    for (const dir of [source, extension, helper]) fs.mkdirSync(dir);
    for (const name of ['mind-place.exe', 'mind-place-capture-host.exe']) fs.writeFileSync(path.join(source, name), 'app fixture');
    fs.writeFileSync(path.join(extension, 'manifest.json'), JSON.stringify({
      permissions: ['activeTab', 'scripting', 'nativeMessaging', 'storage'],
      optional_host_permissions: ['https://chatgpt.com/*'],
      background: { service_worker: 'worker.mjs', type: 'module' },
    }));
    fs.writeFileSync(path.join(root, 'notices'), 'product notices');
    fs.writeFileSync(path.join(root, 'license'), 'AGPL');
    fs.writeFileSync(path.join(helper, 'tunnel-client.exe'), 'helper fixture');
    fs.mkdirSync(path.join(helper, 'resources'));
    fs.writeFileSync(path.join(helper, 'resources', 'LICENSES.txt'), 'upstream licenses fixture');
    const hash = createHash('sha256').update('helper fixture').digest('hex');
    const pin = { executable: { name: 'tunnel-client.exe', sha256: hash, bytes: 14 }, resources: { 'LICENSES.txt': createHash('sha256').update('upstream licenses fixture').digest('hex') } };
    fs.writeFileSync(path.join(helper, 'resources', 'receipt.json'), JSON.stringify({ executable: pin.executable, resources: pin.resources }, null, 2) + '\n');
    const output = path.join(root, 'output');
    stageProduct(source, output, extension, path.join(root, 'notices'), path.join(root, 'license'), { directory: helper, pin });
    assert.equal(fs.readFileSync(path.join(output, 'tunnel-client.exe'), 'utf8'), 'helper fixture');
    assert.ok(fs.existsSync(path.join(output, 'tunnel', 'LICENSES.txt')));
    assert.ok(fs.existsSync(path.join(output, 'tunnel', 'receipt.json')));
    fs.writeFileSync(path.join(output, 'cloudflared.exe'), 'unrelated executable');
    assert.throws(() => stageProduct(source, output, extension, path.join(root, 'notices'), path.join(root, 'license'), { directory: helper, pin }), /Unexpected legacy payload/);
    fs.unlinkSync(path.join(output, 'cloudflared.exe'));
    fs.writeFileSync(path.join(helper, 'resources', 'receipt.json'), 'corrupt receipt');
    assert.throws(() => stageProduct(source, output, extension, path.join(root, 'notices'), path.join(root, 'license'), { directory: helper, pin }), /receipt/);
    fs.writeFileSync(path.join(helper, 'resources', 'receipt.json'), JSON.stringify({ executable: pin.executable, resources: pin.resources }, null, 2) + '\n');
    // Corruption must fail instead of emitting a package without a usable helper.
    fs.writeFileSync(path.join(helper, 'tunnel-client.exe'), 'tampered');
    assert.throws(() => stageProduct(source, output, extension, path.join(root, 'notices'), path.join(root, 'license'), { directory: helper, pin }), /helper|checksum/i);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});

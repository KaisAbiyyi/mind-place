import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { test } from 'node:test';
import { spawnSync } from 'node:child_process';
import { stageProduct } from '../scripts/tauri.mjs';
import { downloadPinnedAsset, resolvePinnedAssetUrl, verifyPinnedFile } from '../scripts/tunnel-helper.mjs';

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
    fs.writeFileSync(path.join(root, 'license'), 'MIT License\nfixture');
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
    assert.equal(fs.readFileSync(path.join(output, 'LICENSE'), 'utf8'), 'MIT License\nfixture');
    assert.equal(fs.readFileSync(path.join(output, 'THIRD-PARTY-NOTICES.txt'), 'utf8'), 'product notices');
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


test('official asset discovery binds tag, name, size and optional metadata digest to the unchanged pin', async () => {
  const pin = { repository: 'https://github.com/openai/tunnel-client', version: 'fixture', archive: { name: 'helper.zip', bytes: 7, sha256: 'a'.repeat(64) } };
  const asset = { id: 123, name: 'helper.zip', size: 7, digest: `sha256:${pin.archive.sha256}`, url: 'https://untrusted.invalid/ignored' };
  const metadata = { tag_name: 'vfixture', assets: [asset] };
  const fetcher = async (url, options) => {
    assert.equal(url, 'https://api.github.com/repos/openai/tunnel-client/releases/tags/vfixture');
    assert.equal(options.headers.Accept, 'application/vnd.github+json');
    return Response.json(metadata);
  };
  assert.equal(await resolvePinnedAssetUrl(pin, fetcher), 'https://api.github.com/repos/openai/tunnel-client/releases/assets/123');
  let metadataCalls = 0;
  assert.equal(await resolvePinnedAssetUrl(pin, async () => {
    metadataCalls++;
    return metadataCalls === 1 ? new Response('', { status: 503 }) : Response.json(metadata);
  }), 'https://api.github.com/repos/openai/tunnel-client/releases/assets/123');
  assert.equal(metadataCalls, 2);
  for (const patch of [{ id: -1 }, { id: 1.5 }, { name: 'other.zip' }, { size: 8 }, { digest: 'sha256:wrong' }]) {
    await assert.rejects(resolvePinnedAssetUrl(pin, async () => Response.json({ ...metadata, assets: [{ ...asset, ...patch }] })), /does not match pin/);
  }
  await assert.rejects(resolvePinnedAssetUrl(pin, async () => Response.json({ ...metadata, tag_name: 'wrong' })), /does not match pin/);
  await assert.rejects(resolvePinnedAssetUrl(pin, async () => Response.json({ ...metadata, assets: [asset, asset] })), /does not match pin/);
  await assert.rejects(resolvePinnedAssetUrl(pin, async () => new Response('x'.repeat(1024 * 1024 + 1))), /exceeds/);
});

test('transient helper downloads retry boundedly, preserve exact verification and keep verified caches offline', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mp-helper-retry-'));
  try {
    const payload = Buffer.from('pinned fixture');
    const pin = { bytes: payload.length, sha256: createHash('sha256').update(payload).digest('hex') };
    const destination = path.join(root, 'helper.zip');
    let calls = 0;
    await downloadPinnedAsset('https://api.github.com/fixture', destination, pin, async (_url, options) => {
      assert.equal(options.headers.Accept, 'application/octet-stream');
      calls++;
      if (calls === 1) return new Response('', { status: 504 });
      if (calls === 2) throw new TypeError('connection reset');
      return new Response(payload);
    });
    assert.equal(calls, 3);
    verifyPinnedFile(destination, pin.sha256, pin.bytes);
    await downloadPinnedAsset(async () => assert.fail('verified archive must not need discovery'), destination, pin);
    fs.unlinkSync(destination);
    calls = 0;
    await assert.rejects(downloadPinnedAsset('https://api.github.com/fixture', destination, pin, async () => {
      calls++; return new Response('', { status: 504 });
    }), /HTTP 504/);
    assert.equal(calls, 3);
    assert.equal(fs.existsSync(destination), false);
    assert.deepEqual(fs.readdirSync(root), []);
    calls = 0;
    await assert.rejects(downloadPinnedAsset('https://api.github.com/fixture', destination, pin, async () => {
      calls++; return new Response(Buffer.alloc(payload.length));
    }), /checksum/);
    assert.equal(calls, 1, 'pin mismatches are terminal, not a reason to seek other bytes');
    calls = 0;
    await downloadPinnedAsset('https://api.github.com/fixture', destination, pin, async () => {
      calls++;
      return calls === 1 ? new Response(new ReadableStream({ start(controller) { controller.error(new Error('interrupted')); } })) : new Response(payload);
    });
    assert.equal(calls, 2);
    verifyPinnedFile(destination, pin.sha256, pin.bytes);
    fs.unlinkSync(destination);
    assert.equal(fs.existsSync(destination), false);
    calls = 0;
    await assert.rejects(downloadPinnedAsset('https://api.github.com/fixture', destination, pin, async () => {
      calls++; return new Response('', { status: 404 });
    }), /HTTP 404/);
    assert.equal(calls, 1);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});

test('Windows staging stops at helper failure before notices, web build or desktop qualification', { skip: process.platform !== 'win32' }, () => {
  const workflow = fs.readFileSync(new URL('../.github/workflows/windows-product.yml', import.meta.url), 'utf8');
  const stage = workflow.split('- name: Stage capture sidecar for native desktop tests')[1].split('- name: Qualify Phase 3E')[0];
  const script = stage.match(/run: '([\s\S]*?)'\n\s+if:/)?.[1];
  assert.ok(script, 'actual Windows staging run block must be testable');
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mp-helper-stage-'));
  try {
    fs.mkdirSync(path.join(root, 'target/debug'), { recursive: true });
    fs.writeFileSync(path.join(root, 'target/debug/mind-place-capture-host.exe'), 'fixture');
    const trace = path.join(root, 'trace.txt');
    // Real native child exit codes, with the shell's optional automatic native
    // error promotion disabled: the workflow itself must enforce fail-fast.
    const prelude = `
      $PSNativeCommandUseErrorActionPreference = $false
      function cargo { & $env:MP_STAGE_NODE -e 'process.exit(0)' }
      function node {
        if ($args[0] -eq 'scripts/tunnel-helper.mjs') {
          Add-Content $env:MP_STAGE_TRACE 'helper'
          & $env:MP_STAGE_NODE -e 'process.exit(Number(process.env.MP_STAGE_FAILURE))'
        } else {
          Add-Content $env:MP_STAGE_TRACE 'notices'
          & $env:MP_STAGE_NODE -e 'process.exit(0)'
        }
      }
      function npm { Add-Content $env:MP_STAGE_TRACE 'web'; & $env:MP_STAGE_NODE -e 'process.exit(0)' }
    `;
    for (const failure of ['23', '0']) {
      fs.rmSync(trace, { force: true });
      const result = spawnSync('pwsh', ['-NoProfile', '-NonInteractive', '-Command', prelude + script + "\nAdd-Content $env:MP_STAGE_TRACE 'after-stage'"], {
        cwd: root, encoding: 'utf8', env: { ...process.env, MP_STAGE_NODE: process.execPath, MP_STAGE_TRACE: trace, MP_STAGE_FAILURE: failure },
      });
      assert.equal(result.error, undefined);
      assert.equal(result.status, Number(failure), result.stderr);
      const events = fs.readFileSync(trace, 'utf8').trim().split(/\r?\n/);
      assert.deepEqual(events, failure === '23' ? ['helper'] : ['helper', 'notices', 'web', 'after-stage']);
    }
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});

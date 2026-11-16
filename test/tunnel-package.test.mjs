import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { test } from 'node:test';
import { spawnSync } from 'node:child_process';
import { stageProduct } from '../scripts/tauri.mjs';
import {
  downloadPinnedAsset,
  prepareTunnelHelper,
  readVerifiedHelperBundle,
  readVerifiedRegularFile,
  resolvePinnedAssetUrl,
  sameFileIdentity,
  verifyHelperBundle,
  verifyPinnedFile,
} from '../scripts/tunnel-helper.mjs';

test('pinned downloads reject corruption/oversize and never trust a stale cache', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mp-helper-download-'));
  try {
    const rejected = path.join(root, 'rejected.zip');
    const payload = Buffer.from('verified fixture');
    const pin = { bytes: payload.length, sha256: createHash('sha256').update(payload).digest('hex') };
    const url = 'https://github.com/openai/tunnel-client/releases/download/fixture/helper.zip';
    await assert.rejects(downloadPinnedAsset(url, rejected, pin, async () => new Response('corrupt fixture')), /checksum|size/);
    assert.deepEqual(fs.readdirSync(root), []);
    await assert.rejects(downloadPinnedAsset(url, rejected, pin, async () => new Response(Buffer.alloc(100))), /exceeds/);
    assert.deepEqual(fs.readdirSync(root), []);
    const destination = path.join(root, 'helper.zip');
    fs.writeFileSync(destination, 'stale', { flag: 'wx' });
    let downloads = 0;
    await downloadPinnedAsset(url, destination, pin, async () => { downloads++; return new Response(payload); });
    verifyPinnedFile(destination, pin.sha256, pin.bytes);
    await downloadPinnedAsset(url, destination, pin, async () => assert.fail('valid cache should work offline'));
    assert.equal(downloads, 1);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});

test('descriptor-bound verification enforces regular files, size bounds and non-symlinks', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mp-helper-fd-'));
  try {
    const sample = path.join(root, 'notice.txt');
    fs.writeFileSync(sample, 'ok-bytes');
    assert.equal(readVerifiedRegularFile(sample, { maxBytes: 8 }).toString('utf8'), 'ok-bytes');
    assert.throws(() => readVerifiedRegularFile(sample, { maxBytes: 7, sizeTypeError: 'License file too large: sample' }), /License file too large/);
    assert.throws(() => readVerifiedRegularFile(root, { maxBytes: 1024 }), /Invalid tunnel helper/);
    const exeDir = path.join(root, 'bundle');
    fs.mkdirSync(path.join(exeDir, 'resources'), { recursive: true });
    const exeBytes = Buffer.from('helper-bin');
    const noticeBytes = Buffer.from('notice-text');
    const pin = {
      repository: 'https://github.com/openai/tunnel-client',
      version: 'fixture',
      platform: 'win32-x64',
      archive: { name: 'helper.zip', bytes: 1, sha256: 'a'.repeat(64) },
      executable: { name: 'tunnel-client.exe', bytes: exeBytes.length, sha256: createHash('sha256').update(exeBytes).digest('hex') },
      resources: { 'LICENSES.txt': createHash('sha256').update(noticeBytes).digest('hex') },
      excluded_archive_members: [],
    };
    fs.writeFileSync(path.join(exeDir, 'tunnel-client.exe'), exeBytes);
    fs.writeFileSync(path.join(exeDir, 'resources', 'LICENSES.txt'), noticeBytes);
    fs.writeFileSync(path.join(exeDir, 'resources', 'receipt.json'), 'not matching');
    assert.throws(() => verifyHelperBundle(exeDir, pin), /Invalid helper receipt/);
    const symlinkPath = path.join(root, 'notice-link.txt');
    let symlinkCreated = false;
    try {
      fs.symlinkSync(sample, symlinkPath, 'file');
      symlinkCreated = true;
    } catch {
      symlinkCreated = false;
    }
    if (symlinkCreated) {
      assert.throws(() => verifyPinnedFile(symlinkPath, createHash('sha256').update('ok-bytes').digest('hex'), 8), /ELOOP|Invalid tunnel helper/);
    }
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
    for (const status of [404, 429]) {
      calls = 0;
      await assert.rejects(downloadPinnedAsset('https://api.github.com/fixture', destination, pin, async () => {
        calls++; return new Response('', { status, headers: { 'Retry-After': '60' } });
      }), new RegExp(`HTTP ${status}`));
      assert.equal(calls, 1, 'terminal/rate-limit responses must not be retried early');
    }
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

test('sameFileIdentity fails closed on zero inode or mismatched identity', () => {
  const validA = { isFile: () => true, isSymbolicLink: () => false, dev: 1n, ino: 42n };
  const validB = { isFile: () => true, isSymbolicLink: () => false, dev: 1n, ino: 42n };
  const zeroA = { isFile: () => true, isSymbolicLink: () => false, dev: 1n, ino: 0n };
  const zeroB = { isFile: () => true, isSymbolicLink: () => false, dev: 1n, ino: 0n };
  const swapped = { isFile: () => true, isSymbolicLink: () => false, dev: 1n, ino: 99n };
  const symlinkLeaf = { isFile: () => true, isSymbolicLink: () => true, dev: 1n, ino: 42n };
  assert.equal(sameFileIdentity(validA, validB), true);
  assert.equal(sameFileIdentity(zeroA, zeroB), false);
  assert.equal(sameFileIdentity(validA, zeroB), false);
  assert.equal(sameFileIdentity(validA, swapped), false);
  assert.equal(sameFileIdentity(validA, symlinkLeaf), false);
});

test('prepareTunnelHelper and stageProduct reject concurrent destination swaps at every staged path', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mp-stage-toctou-'));
  try {
    const source = path.join(root, 'source');
    const extension = path.join(root, 'extension');
    const helper = path.join(root, 'helper');
    const binDir = path.join(root, 'src-tauri-binaries');
    for (const dir of [source, extension, helper, binDir]) fs.mkdirSync(dir, { recursive: true });
    for (const name of ['mind-place.exe', 'mind-place-capture-host.exe']) fs.writeFileSync(path.join(source, name), 'app fixture');
    fs.writeFileSync(path.join(extension, 'manifest.json'), JSON.stringify({
      permissions: ['activeTab', 'scripting', 'nativeMessaging', 'storage'],
      optional_host_permissions: ['https://chatgpt.com/*'],
      background: { service_worker: 'worker.mjs', type: 'module' },
    }));
    fs.writeFileSync(path.join(root, 'notices'), 'product notices');
    fs.writeFileSync(path.join(root, 'license'), 'MIT License\nfixture');

    const exeBytes = Buffer.from('verified helper binary');
    const licenseBytes = Buffer.from('verified notice payload');
    const pin = {
      repository: 'https://github.com/openai/tunnel-client',
      version: 'fixture',
      platform: 'win32-x64',
      archive: { name: 'helper.zip', bytes: 1, sha256: 'a'.repeat(64) },
      executable: { name: 'tunnel-client.exe', sha256: createHash('sha256').update(exeBytes).digest('hex'), bytes: exeBytes.length },
      resources: { 'LICENSES.txt': createHash('sha256').update(licenseBytes).digest('hex') },
      excluded_archive_members: [],
    };
    const expectedReceipt = JSON.stringify(pin, null, 2) + '\n';
    fs.writeFileSync(path.join(helper, 'tunnel-client.exe'), exeBytes);
    fs.mkdirSync(path.join(helper, 'resources'));
    fs.writeFileSync(path.join(helper, 'resources', 'LICENSES.txt'), licenseBytes);
    fs.writeFileSync(path.join(helper, 'resources', 'receipt.json'), expectedReceipt);

    // Clean prepareTunnelHelper stages identical verified bytes to both target directories.
    await prepareTunnelHelper('x86_64-pc-windows-msvc', source, { cacheDir: helper, pin, binDir });
    const stagedSourceExe = path.join(source, 'tunnel-client.exe');
    const stagedBinExe = path.join(binDir, 'tunnel-client-x86_64-pc-windows-msvc.exe');
    verifyPinnedFile(stagedSourceExe, pin.executable.sha256, pin.executable.bytes);
    verifyPinnedFile(stagedBinExe, pin.executable.sha256, pin.executable.bytes);

    // Swap attack on each prepareTunnelHelper staging destination fails closed and removes tampered bytes.
    for (const victim of [stagedSourceExe, stagedBinExe]) {
      fs.rmSync(victim, { force: true });
      await assert.rejects(
        prepareTunnelHelper('x86_64-pc-windows-msvc', source, {
          cacheDir: helper,
          pin,
          binDir,
          onBeforeStageVerify: dest => {
            if (dest === victim) fs.writeFileSync(dest, 'swapped-' + 'x'.repeat(exeBytes.length - 8));
          },
        }),
        /checksum|size|mismatch/i,
      );
      assert.equal(fs.existsSync(victim), false, `tampered staged file removed: ${victim}`);
    }

    // stageProduct stages verified helper bundle into output and rejects swaps at every helper target.
    const outClean = path.join(root, 'out-clean');
    stageProduct(source, outClean, extension, path.join(root, 'notices'), path.join(root, 'license'), { directory: helper, pin });
    const verifiedOut = readVerifiedHelperBundle(outClean, pin, 'tunnel');
    assert.ok(verifiedOut.executable.equals(exeBytes));
    assert.ok(verifiedOut.resources.get('LICENSES.txt').equals(licenseBytes));

    const outVictims = [
      path.join(root, 'out-swap-exe', 'tunnel-client.exe'),
      path.join(root, 'out-swap-notice', 'tunnel', 'LICENSES.txt'),
      path.join(root, 'out-swap-receipt', 'tunnel', 'receipt.json'),
    ];
    for (const victim of outVictims) {
      const outDir = path.dirname(victim).endsWith('tunnel') ? path.dirname(path.dirname(victim)) : path.dirname(victim);
      assert.throws(() => stageProduct(
        source,
        outDir,
        extension,
        path.join(root, 'notices'),
        path.join(root, 'license'),
        { directory: helper, pin },
        {
          onBeforeStageVerify: dest => {
            if (dest === victim) fs.writeFileSync(dest, 'tampered');
          },
        },
      ), /checksum|size|receipt/i);
      assert.equal(fs.existsSync(victim), false, `tampered output file removed: ${victim}`);
    }

    const outExtra = path.join(root, 'out-extra');
    fs.mkdirSync(path.join(outExtra, 'tunnel'), { recursive: true });
    fs.writeFileSync(path.join(outExtra, 'tunnel', 'unexpected.txt'), 'stray');
    assert.throws(() => stageProduct(
      source,
      outExtra,
      extension,
      path.join(root, 'notices'),
      path.join(root, 'license'),
      { directory: helper, pin },
    ), /Unexpected tunnel helper resource payload/);

    if (process.platform === 'win32') {
      const outJunction = path.join(root, 'out-junction');
      const outside = path.join(root, 'outside-target');
      fs.mkdirSync(outJunction, { recursive: true });
      fs.mkdirSync(outside, { recursive: true });
      let junctioned = false;
      try {
        fs.symlinkSync(outside, path.join(outJunction, 'tunnel'), 'junction');
        junctioned = true;
      } catch {
        junctioned = false;
      }
      if (junctioned) {
        assert.throws(() => stageProduct(
          source,
          outJunction,
          extension,
          path.join(root, 'notices'),
          path.join(root, 'license'),
          { directory: helper, pin },
        ), /Invalid helper resources|ELOOP/);
        assert.deepEqual(fs.readdirSync(outside), []);
      }
    }
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});

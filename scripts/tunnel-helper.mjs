import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
export const helperPin = JSON.parse(fs.readFileSync(path.join(root, 'scripts/tunnel-client.lock.json'), 'utf8'));
export const helperDirectory = path.join(root, 'target/tunnel-helper/windows-x64');
export function verifyPinnedFile(file, digest, bytes) {
  const stat = fs.lstatSync(file);
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size > 64 * 1024 * 1024 || (bytes !== undefined && stat.size !== bytes)) throw new Error('Invalid tunnel helper file size/type');
  if (createHash('sha256').update(fs.readFileSync(file)).digest('hex') !== digest) throw new Error('Tunnel helper checksum mismatch');
}
function receipt(pin) {
  return JSON.stringify({ repository: pin.repository, version: pin.version, platform: pin.platform, archive: pin.archive, executable: pin.executable, resources: pin.resources, excluded_archive_members: pin.excluded_archive_members }, null, 2) + '\n';
}
export function verifyHelperBundle(directory, pin = helperPin) {
  verifyPinnedFile(path.join(directory, pin.executable.name), pin.executable.sha256, pin.executable.bytes);
  const resources = path.join(directory, 'resources');
  if (fs.lstatSync(resources).isSymbolicLink()) throw new Error('Invalid helper resources');
  const expected = [...Object.keys(pin.resources), 'receipt.json'].sort();
  if (JSON.stringify(fs.readdirSync(resources).sort()) !== JSON.stringify(expected)) throw new Error('Unexpected tunnel helper resource payload');
  for (const [name, hash] of Object.entries(pin.resources)) verifyPinnedFile(path.join(resources, name), hash);
  const receiptPath = path.join(resources, 'receipt.json');
  if (!fs.lstatSync(receiptPath).isFile() || fs.lstatSync(receiptPath).isSymbolicLink() || fs.readFileSync(receiptPath, 'utf8') !== receipt(pin)) throw new Error('Invalid helper receipt');
}
// Retry transport failures only. Bad metadata, oversized bodies and pin mismatches
// are terminal; no unverified bytes are ever published to the download cache.
class TransientDownloadError extends Error {}
async function retryDownload(operation) {
  for (let attempt = 0; ; attempt++) {
    try { return await operation(); } catch (error) {
      if (!(error instanceof TransientDownloadError) || attempt === 2) throw error;
      await delay(250 * (attempt + 1));
    }
  }
}
async function request(url, accept, fetcher) {
  let response;
  try {
    response = await fetcher(url, { headers: { Accept: accept, 'User-Agent': 'MindPlace-build' }, signal: AbortSignal.timeout(120_000) });
  } catch (error) {
    throw new TransientDownloadError('Helper request failed', { cause: error });
  }
  if (!response.ok || !response.body) {
    await response.body?.cancel();
    const ErrorType = [500, 502, 503, 504].includes(response.status) ? TransientDownloadError : Error;
    throw new ErrorType(`Helper download failed: HTTP ${response.status}`);
  }
  return response;
}
async function readBounded(response, limit) {
  const parts = [];
  let size = 0;
  const reader = response.body.getReader();
  try {
    while (true) {
      let part;
      try { part = await reader.read(); } catch (error) {
        throw new TransientDownloadError('Helper response interrupted', { cause: error });
      }
      if (part.done) break;
      size += part.value.length;
      if (size > limit) {
        await reader.cancel();
        throw new Error('Helper download exceeds pinned size');
      }
      parts.push(part.value);
    }
  } finally { reader.releaseLock(); }
  return Buffer.concat(parts);
}
export async function resolvePinnedAssetUrl(pin = helperPin, fetcher = fetch) {
  const repository = new URL(pin.repository);
  if (repository.origin !== 'https://github.com' || !/^\/[\w.-]+\/[\w.-]+$/.test(repository.pathname)) throw new Error('Invalid helper repository');
  const api = `https://api.github.com/repos${repository.pathname}`;
  const metadata = await retryDownload(async () => {
    const response = await request(`${api}/releases/tags/${encodeURIComponent('v' + pin.version)}`, 'application/vnd.github+json', fetcher);
    return JSON.parse((await readBounded(response, 1024 * 1024)).toString('utf8'));
  });
  const assets = Array.isArray(metadata.assets) ? metadata.assets.filter(asset => asset.name === pin.archive.name) : [];
  const asset = assets[0];
  if (metadata.tag_name !== 'v' + pin.version || assets.length !== 1 || !Number.isSafeInteger(asset.id) || asset.id <= 0
      || asset.size !== pin.archive.bytes || (asset.digest != null && asset.digest !== `sha256:${pin.archive.sha256}`)) {
    throw new Error('Helper release asset does not match pin');
  }
  // Construct the official API URL from the numeric identity; never follow a
  // metadata-supplied arbitrary URL. Archive bytes still require the local pin.
  return `${api}/releases/assets/${asset.id}`;
}
export async function downloadPinnedAsset(url, destination, { sha256, bytes }, fetcher = fetch) {
  if (fs.existsSync(destination)) {
    try { verifyPinnedFile(destination, sha256, bytes); return; } catch { /* Re-fetch a corrupt cache; never execute it. */ }
  }
  // Resolve lazily so a verified archive cache can still be extracted offline.
  const resolvedUrl = typeof url === 'function' ? await url() : url;
  const data = await retryDownload(async () => {
    const response = await request(resolvedUrl, 'application/octet-stream', fetcher);
    const data = await readBounded(response, bytes);
    if (data.length !== bytes || createHash('sha256').update(data).digest('hex') !== sha256) throw new Error('Tunnel helper download checksum/size mismatch');
    return data;
  });
  fs.mkdirSync(path.dirname(destination), { recursive: true });
  const temporary = `${destination}.${process.pid}.tmp`;
  try {
    fs.writeFileSync(temporary, data, { flag: 'wx' });
    fs.renameSync(temporary, destination);
  } finally { fs.rmSync(temporary, { force: true }); }
}
export async function prepareTunnelHelper(triple, source) {
  if (triple !== 'x86_64-pc-windows-msvc') {
    if (process.platform === 'win32') throw new Error('Bundled tunnel helper supports Windows x64 only');
    return undefined;
  }
  try { verifyHelperBundle(helperDirectory); } catch {
    const archive = path.join(root, 'target/tunnel-helper/downloads', helperPin.archive.name);
    await downloadPinnedAsset(() => resolvePinnedAssetUrl(), archive, helperPin.archive);
    const extracted = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', path.join(root, 'scripts/extract-tunnel-helper.ps1'), '-Archive', archive, '-Destination', helperDirectory, '-PinFile', path.join(root, 'scripts/tunnel-client.lock.json')], { encoding: 'utf8', windowsHide: true, timeout: 60_000 });
    if (extracted.status !== 0) throw new Error(extracted.stderr || 'Helper extraction failed');
    fs.writeFileSync(path.join(helperDirectory, 'resources/receipt.json'), receipt(helperPin));
    verifyHelperBundle(helperDirectory);
  }
  const executable = path.join(helperDirectory, helperPin.executable.name);
  fs.mkdirSync(source, { recursive: true });
  fs.copyFileSync(executable, path.join(source, 'tunnel-client.exe'));
  fs.mkdirSync(path.join(root, 'src-tauri/binaries'), { recursive: true });
  fs.copyFileSync(executable, path.join(root, `src-tauri/binaries/tunnel-client-${triple}.exe`));
  console.log(`Bundled official tunnel-client ${helperPin.version} (${helperPin.platform}); SHA-256 verified`);
  return { directory: helperDirectory, pin: helperPin };
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  prepareTunnelHelper('x86_64-pc-windows-msvc', path.join(root, 'target/debug')).catch(error => { console.error(error.message); process.exitCode = 1; });
}

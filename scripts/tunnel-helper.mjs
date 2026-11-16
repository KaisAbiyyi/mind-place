import fs from 'node:fs';
import path from 'node:path';
import { createHash, randomBytes } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
export const helperPin = JSON.parse(fs.readFileSync(path.join(root, 'scripts/tunnel-client.lock.json'), 'utf8'));
export const helperDirectory = path.join(root, 'target/tunnel-helper/windows-x64');
const HELPER_FILE_MAX_BYTES = 64 * 1024 * 1024;
const RECEIPT_MAX_BYTES = 1024 * 1024;

function hasUsableIdentity(stat) {
  return Boolean(stat) && typeof stat.dev === 'bigint' && typeof stat.ino === 'bigint' && stat.ino > 0n;
}

export function sameFileIdentity(left, right) {
  return left.isFile() && right.isFile() && !right.isSymbolicLink()
    && hasUsableIdentity(left) && hasUsableIdentity(right)
    && left.dev === right.dev && left.ino === right.ino;
}

function sameDirIdentity(left, right) {
  return left.isDirectory() && right.isDirectory() && !right.isSymbolicLink()
    && hasUsableIdentity(left) && hasUsableIdentity(right)
    && left.dev === right.dev && left.ino === right.ino;
}

function verifyDirectoryAndList(dir, invalidMessage = 'Invalid helper resources') {
  const flags = fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW ?? 0);
  const fd = fs.openSync(dir, flags);
  try {
    const stat = fs.fstatSync(fd, { bigint: true });
    const leaf = fs.lstatSync(dir, { bigint: true });
    if (!sameDirIdentity(stat, leaf)) throw new Error(invalidMessage);
    const names = fs.readdirSync(dir);
    const after = fs.lstatSync(dir, { bigint: true });
    if (!sameDirIdentity(stat, after)) throw new Error(invalidMessage);
    return names;
  } finally {
    fs.closeSync(fd);
  }
}

export function readVerifiedRegularFile(file, {
  maxBytes,
  exactBytes,
  sizeTypeError = 'Invalid tunnel helper file size/type',
  overflowError = sizeTypeError,
}) {
  const flags = fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW ?? 0);
  const fd = fs.openSync(file, flags);
  try {
    const stat = fs.fstatSync(fd, { bigint: true });
    const leaf = fs.lstatSync(file, { bigint: true });
    if (!sameFileIdentity(stat, leaf)) throw new Error(sizeTypeError);
    if (stat.size > BigInt(maxBytes) || (exactBytes !== undefined && stat.size !== BigInt(exactBytes))) {
      throw new Error(sizeTypeError);
    }
    const chunks = [];
    let total = 0;
    const scratch = Buffer.allocUnsafe(64 * 1024);
    while (true) {
      const bytesRead = fs.readSync(fd, scratch, 0, scratch.length, total);
      if (bytesRead === 0) break;
      total += bytesRead;
      if (total > maxBytes) throw new Error(overflowError);
      chunks.push(Buffer.from(scratch.subarray(0, bytesRead)));
    }
    if (exactBytes !== undefined && total !== exactBytes) throw new Error(sizeTypeError);
    return Buffer.concat(chunks, total);
  } finally {
    fs.closeSync(fd);
  }
}

export function readVerifiedPinnedFile(file, digest, bytes) {
  const data = readVerifiedRegularFile(file, { maxBytes: HELPER_FILE_MAX_BYTES, exactBytes: bytes });
  if (createHash('sha256').update(data).digest('hex') !== digest) throw new Error('Tunnel helper checksum mismatch');
  return data;
}

export function verifyPinnedFile(file, digest, bytes) {
  readVerifiedPinnedFile(file, digest, bytes);
}
function receipt(pin) {
  return JSON.stringify({ repository: pin.repository, version: pin.version, platform: pin.platform, archive: pin.archive, executable: pin.executable, resources: pin.resources, excluded_archive_members: pin.excluded_archive_members }, null, 2) + '\n';
}
export function readVerifiedHelperBundle(directory, pin = helperPin, resourceDirName = 'resources') {
  const executable = readVerifiedPinnedFile(path.join(directory, pin.executable.name), pin.executable.sha256, pin.executable.bytes);
  const resourcesDir = path.join(directory, resourceDirName);
  const expected = [...Object.keys(pin.resources), 'receipt.json'].sort();
  const actual = verifyDirectoryAndList(resourcesDir, 'Invalid helper resources').sort();
  if (JSON.stringify(actual) !== JSON.stringify(expected)) throw new Error('Unexpected tunnel helper resource payload');
  const resources = new Map();
  for (const [name, hash] of Object.entries(pin.resources)) {
    resources.set(name, readVerifiedPinnedFile(path.join(resourcesDir, name), hash));
  }
  const receiptPath = path.join(resourcesDir, 'receipt.json');
  const receiptBytes = readVerifiedRegularFile(receiptPath, {
    maxBytes: RECEIPT_MAX_BYTES,
    sizeTypeError: 'Invalid helper receipt',
    overflowError: 'Invalid helper receipt',
  });
  if (receiptBytes.toString('utf8') !== receipt(pin)) throw new Error('Invalid helper receipt');
  return {
    executable,
    resources,
    receipt: receiptBytes,
    receiptSha256: createHash('sha256').update(receiptBytes).digest('hex'),
  };
}
export function verifyHelperBundle(directory, pin = helperPin, resourceDirName = 'resources') {
  readVerifiedHelperBundle(directory, pin, resourceDirName);
}
export function writeVerifiedFileAtomically(destination, data, digest, exactBytes, { onBeforeVerify } = {}) {
  const actualDigest = createHash('sha256').update(data).digest('hex');
  if (actualDigest !== digest || (exactBytes !== undefined && data.length !== exactBytes)) {
    throw new Error('Tunnel helper checksum mismatch');
  }
  fs.mkdirSync(path.dirname(destination), { recursive: true });
  const temporary = `${destination}.${process.pid}.${randomBytes(6).toString('hex')}.tmp`;
  try {
    fs.writeFileSync(temporary, data, { flag: 'wx' });
    verifyPinnedFile(temporary, digest, exactBytes);
    fs.renameSync(temporary, destination);
  } finally {
    fs.rmSync(temporary, { force: true });
  }
  if (onBeforeVerify) onBeforeVerify(destination);
  try {
    verifyPinnedFile(destination, digest, exactBytes);
  } catch (error) {
    fs.rmSync(destination, { force: true });
    throw error;
  }
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
export async function prepareTunnelHelper(triple, source, {
  cacheDir = helperDirectory,
  pin = helperPin,
  binDir = path.join(root, 'src-tauri/binaries'),
  onBeforeStageVerify,
} = {}) {
  if (triple !== 'x86_64-pc-windows-msvc') {
    if (process.platform === 'win32') throw new Error('Bundled tunnel helper supports Windows x64 only');
    return undefined;
  }
  let bundle;
  try {
    bundle = readVerifiedHelperBundle(cacheDir, pin);
  } catch {
    if (cacheDir !== helperDirectory || pin !== helperPin) {
      throw new Error('Tunnel helper bundle failed verification');
    }
    const archive = path.join(root, 'target/tunnel-helper/downloads', pin.archive.name);
    await downloadPinnedAsset(() => resolvePinnedAssetUrl(pin), archive, pin.archive);
    const extracted = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', path.join(root, 'scripts/extract-tunnel-helper.ps1'), '-Archive', archive, '-Destination', cacheDir, '-PinFile', path.join(root, 'scripts/tunnel-client.lock.json')], { encoding: 'utf8', windowsHide: true, timeout: 60_000 });
    if (extracted.status !== 0) throw new Error(extracted.stderr || 'Helper extraction failed');
    fs.writeFileSync(path.join(cacheDir, 'resources/receipt.json'), receipt(pin));
    bundle = readVerifiedHelperBundle(cacheDir, pin);
  }
  const stageExe = (destination) => writeVerifiedFileAtomically(
    destination,
    bundle.executable,
    pin.executable.sha256,
    pin.executable.bytes,
    onBeforeStageVerify ? { onBeforeVerify: onBeforeStageVerify } : undefined,
  );
  stageExe(path.join(source, 'tunnel-client.exe'));
  stageExe(path.join(binDir, `tunnel-client-${triple}.exe`));
  console.log(`Bundled official tunnel-client ${pin.version} (${pin.platform}); SHA-256 verified`);
  return { directory: cacheDir, pin, bundle };
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  prepareTunnelHelper('x86_64-pc-windows-msvc', path.join(root, 'target/debug')).catch(error => { console.error(error.message); process.exitCode = 1; });
}

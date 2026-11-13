// Minimal, dependency-free ZIP reader and writer (stored/deflate, ZIP64 for counts and offsets).
//
// Reader: random access through the central directory, streaming extraction with CRC and size
// verification (so a lying header or a zip bomb fails instead of being trusted).
// Writer: sequential, deterministic (fixed DOS time, input order), streaming through deflate.
import fs from 'node:fs';
import { once } from 'node:events';
import { Readable } from 'node:stream';
import zlib from 'node:zlib';

const CRC_TABLE = (() => {
  const table = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c >>> 0;
  }
  return table;
})();

export class Crc32 {
  constructor() { this.value = 0xffffffff; }
  update(buffer) {
    let c = this.value;
    for (let i = 0; i < buffer.length; i++) c = CRC_TABLE[(c ^ buffer[i]) & 0xff] ^ (c >>> 8);
    this.value = c >>> 0;
  }
  digest() { return (this.value ^ 0xffffffff) >>> 0; }
}

export class ZipError extends Error {
  constructor(code) {
    super(code);
    this.code = code;
  }
}

const SIG_LOCAL = 0x04034b50;
const SIG_CENTRAL = 0x02014b50;
const SIG_EOCD = 0x06054b50;
const SIG_EOCD64 = 0x06064b50;
const SIG_LOCATOR64 = 0x07064b50;
const SIG_DESCRIPTOR = 0x08074b50;
const MAX_SAFE = Number.MAX_SAFE_INTEGER;

function safeNumber(big) {
  if (big > BigInt(MAX_SAFE)) throw new ZipError('E_ZIP_TOO_LARGE');
  return Number(big);
}

export const READER_LIMITS = Object.freeze({
  maxEntries: 2_000_000,
  maxCentralDirectoryBytes: 512 * 1024 * 1024,
  maxEntryBytes: 64 * 1024 * 1024 * 1024,
});

async function readAt(handle, position, length) {
  const buffer = Buffer.alloc(length);
  let done = 0;
  while (done < length) {
    const { bytesRead } = await handle.read(buffer, done, length - done, position + done);
    if (bytesRead === 0) throw new ZipError('E_ZIP_TRUNCATED');
    done += bytesRead;
  }
  return buffer;
}

function parseExtra(extra, entry, need) {
  for (let at = 0; at + 4 <= extra.length;) {
    const id = extra.readUInt16LE(at);
    const size = extra.readUInt16LE(at + 2);
    if (at + 4 + size > extra.length) break;
    if (id === 0x0001) {
      let cursor = at + 4;
      const next = () => {
        if (cursor + 8 > at + 4 + size) throw new ZipError('E_ZIP_BAD_ZIP64_EXTRA');
        const value = extra.readBigUInt64LE(cursor);
        cursor += 8;
        return safeNumber(value);
      };
      if (need.uncompressed) entry.uncompressedSize = next();
      if (need.compressed) entry.compressedSize = next();
      if (need.offset) entry.offset = next();
    }
    at += 4 + size;
  }
}

/** Opens a zip file. Returns { entries, openEntry(entry) -> Readable, close() }. */
export async function openZip(file, limits = READER_LIMITS) {
  const handle = await fs.promises.open(file, 'r');
  try {
    const { size } = await handle.stat();
    if (size < 22) throw new ZipError('E_ZIP_NOT_A_ZIP');
    const tailLength = Math.min(size, 22 + 0xffff + 20 + 56);
    const tail = await readAt(handle, size - tailLength, tailLength);
    let eocd = -1;
    for (let i = tail.length - 22; i >= 0; i--) {
      if (tail.readUInt32LE(i) === SIG_EOCD && i + 22 + tail.readUInt16LE(i + 20) === tail.length) { eocd = i; break; }
    }
    if (eocd < 0) throw new ZipError('E_ZIP_NOT_A_ZIP');
    let count = tail.readUInt16LE(eocd + 10);
    let cdSize = tail.readUInt32LE(eocd + 12);
    let cdOffset = tail.readUInt32LE(eocd + 16);
    if (count === 0xffff || cdSize === 0xffffffff || cdOffset === 0xffffffff) {
      const locator = eocd - 20;
      if (locator < 0 || tail.readUInt32LE(locator) !== SIG_LOCATOR64) throw new ZipError('E_ZIP_BAD_ZIP64');
      const recordAt = safeNumber(tail.readBigUInt64LE(locator + 8));
      const record = await readAt(handle, recordAt, 56);
      if (record.readUInt32LE(0) !== SIG_EOCD64) throw new ZipError('E_ZIP_BAD_ZIP64');
      count = safeNumber(record.readBigUInt64LE(32));
      cdSize = safeNumber(record.readBigUInt64LE(40));
      cdOffset = safeNumber(record.readBigUInt64LE(48));
    }
    if (count > limits.maxEntries || cdSize > limits.maxCentralDirectoryBytes) throw new ZipError('E_ZIP_LIMIT');
    const directory = await readAt(handle, cdOffset, cdSize);
    const entries = [];
    let at = 0;
    for (let n = 0; n < count; n++) {
      if (at + 46 > directory.length || directory.readUInt32LE(at) !== SIG_CENTRAL) throw new ZipError('E_ZIP_BAD_DIRECTORY');
      const flags = directory.readUInt16LE(at + 8);
      const nameLength = directory.readUInt16LE(at + 28);
      const extraLength = directory.readUInt16LE(at + 30);
      const commentLength = directory.readUInt16LE(at + 32);
      const entry = {
        flags,
        method: directory.readUInt16LE(at + 10),
        crc: directory.readUInt32LE(at + 16),
        compressedSize: directory.readUInt32LE(at + 20),
        uncompressedSize: directory.readUInt32LE(at + 24),
        offset: directory.readUInt32LE(at + 42),
      };
      const end = at + 46 + nameLength + extraLength + commentLength;
      if (end > directory.length) throw new ZipError('E_ZIP_BAD_DIRECTORY');
      entry.name = directory.toString(flags & 0x800 ? 'utf8' : 'latin1', at + 46, at + 46 + nameLength);
      parseExtra(directory.subarray(at + 46 + nameLength, at + 46 + nameLength + extraLength), entry, {
        uncompressed: entry.uncompressedSize === 0xffffffff,
        compressed: entry.compressedSize === 0xffffffff,
        offset: entry.offset === 0xffffffff,
      });
      entry.isDirectory = entry.name.endsWith('/');
      entries.push(entry);
      at = end;
    }
    return {
      entries,
      size,
      openEntry: entry => openEntry(file, handle, entry, limits),
      close: () => handle.close(),
    };
  } catch (error) {
    await handle.close();
    throw error;
  }
}

async function openEntry(file, handle, entry, limits) {
  if (entry.flags & 0x1) throw new ZipError('E_ZIP_ENCRYPTED');
  if (entry.method !== 0 && entry.method !== 8) throw new ZipError('E_ZIP_METHOD');
  if (entry.uncompressedSize > limits.maxEntryBytes) throw new ZipError('E_ZIP_LIMIT');
  const local = await readAt(handle, entry.offset, 30);
  if (local.readUInt32LE(0) !== SIG_LOCAL) throw new ZipError('E_ZIP_BAD_LOCAL_HEADER');
  const start = entry.offset + 30 + local.readUInt16LE(26) + local.readUInt16LE(28);
  async function* chunks() {
    if (entry.compressedSize === 0) {
      if (entry.uncompressedSize !== 0) throw new ZipError('E_ZIP_SIZE_MISMATCH');
      return;
    }
    const raw = fs.createReadStream(file, { start, end: start + entry.compressedSize - 1 });
    const source = entry.method === 8 ? raw.pipe(zlib.createInflateRaw()) : raw;
    raw.on('error', error => source.destroy(error));
    const crc = new Crc32();
    let total = 0;
    for await (const chunk of source) {
      total += chunk.length;
      if (total > entry.uncompressedSize) throw new ZipError('E_ZIP_SIZE_MISMATCH');
      crc.update(chunk);
      yield chunk;
    }
    if (total !== entry.uncompressedSize || crc.digest() !== entry.crc) throw new ZipError('E_ZIP_CRC_MISMATCH');
  }
  return Readable.from(chunks(), { objectMode: false });
}

// ---------------------------------------------------------------- writer

const DOS_TIME = 0;
const DOS_DATE = (0 << 9) | (1 << 5) | 1; // 1980-01-01, fixed for deterministic output
const UNIX_FILE_ATTR = ((0o100644 << 16) >>> 0);

export class ZipWriter {
  #stream;
  #offset = 0;
  #entries = [];
  #names = new Set();

  constructor(file) {
    this.#stream = fs.createWriteStream(file, { flags: 'wx', mode: 0o600 });
  }

  async #write(buffer) {
    this.#offset += buffer.length;
    if (!this.#stream.write(buffer)) await once(this.#stream, 'drain');
  }

  /** Adds a file entry from a Readable/async iterable of Buffers, or a Buffer. */
  async addFile(name, content) {
    this.#checkName(name);
    const nameBytes = Buffer.from(name, 'utf8');
    const offset = this.#offset;
    const header = Buffer.alloc(30);
    header.writeUInt32LE(SIG_LOCAL, 0);
    header.writeUInt16LE(20, 4);
    header.writeUInt16LE(0x0808, 6); // data descriptor + UTF-8 names
    header.writeUInt16LE(8, 8);
    header.writeUInt16LE(DOS_TIME, 10);
    header.writeUInt16LE(DOS_DATE, 12);
    header.writeUInt16LE(nameBytes.length, 26);
    await this.#write(Buffer.concat([header, nameBytes]));

    const crc = new Crc32();
    let uncompressed = 0;
    let compressed = 0;
    const deflate = zlib.createDeflateRaw({ level: 6 });
    const source = Buffer.isBuffer(content) ? [content] : content;
    const feeding = (async () => {
      try {
        for await (const chunk of source) {
          const buffer = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
          crc.update(buffer);
          uncompressed += buffer.length;
          if (!deflate.write(buffer)) await once(deflate, 'drain');
        }
        deflate.end();
      } catch (error) {
        deflate.destroy(error);
      }
    })();
    for await (const out of deflate) {
      compressed += out.length;
      await this.#write(out);
    }
    await feeding;
    if (uncompressed >= 0xffffffff || compressed >= 0xffffffff) throw new ZipError('E_ZIP_ENTRY_TOO_LARGE');
    const descriptor = Buffer.alloc(16);
    descriptor.writeUInt32LE(SIG_DESCRIPTOR, 0);
    descriptor.writeUInt32LE(crc.digest(), 4);
    descriptor.writeUInt32LE(compressed, 8);
    descriptor.writeUInt32LE(uncompressed, 12);
    await this.#write(descriptor);
    this.#entries.push({ nameBytes, offset, crc: crc.digest(), compressed, uncompressed, method: 8, directory: false });
  }

  async addDirectory(name) {
    if (!name.endsWith('/')) throw new ZipError('E_ZIP_BAD_NAME');
    this.#checkName(name);
    const nameBytes = Buffer.from(name, 'utf8');
    const offset = this.#offset;
    const header = Buffer.alloc(30);
    header.writeUInt32LE(SIG_LOCAL, 0);
    header.writeUInt16LE(20, 4);
    header.writeUInt16LE(0x0800, 6);
    header.writeUInt16LE(0, 8);
    header.writeUInt16LE(DOS_TIME, 10);
    header.writeUInt16LE(DOS_DATE, 12);
    header.writeUInt16LE(nameBytes.length, 26);
    await this.#write(Buffer.concat([header, nameBytes]));
    this.#entries.push({ nameBytes, offset, crc: 0, compressed: 0, uncompressed: 0, method: 0, directory: true });
  }

  #checkName(name) {
    if (!name || name.startsWith('/') || name.split('/').includes('..') || name.includes('\\') || name.includes('\0')) {
      throw new ZipError('E_ZIP_BAD_NAME');
    }
    if (this.#names.has(name)) throw new ZipError('E_ZIP_DUPLICATE_NAME');
    this.#names.add(name);
  }

  async finish() {
    const cdOffset = this.#offset;
    for (const e of this.#entries) {
      const bigOffset = e.offset >= 0xffffffff;
      const extra = bigOffset ? Buffer.alloc(12) : Buffer.alloc(0);
      if (bigOffset) {
        extra.writeUInt16LE(0x0001, 0);
        extra.writeUInt16LE(8, 2);
        extra.writeBigUInt64LE(BigInt(e.offset), 4);
      }
      const record = Buffer.alloc(46);
      record.writeUInt32LE(SIG_CENTRAL, 0);
      record.writeUInt16LE((3 << 8) | (bigOffset ? 45 : 20), 4);
      record.writeUInt16LE(bigOffset ? 45 : 20, 6);
      record.writeUInt16LE(e.directory ? 0x0800 : 0x0808, 8);
      record.writeUInt16LE(e.method, 10);
      record.writeUInt16LE(DOS_TIME, 12);
      record.writeUInt16LE(DOS_DATE, 14);
      record.writeUInt32LE(e.crc, 16);
      record.writeUInt32LE(e.compressed, 20);
      record.writeUInt32LE(e.uncompressed, 24);
      record.writeUInt16LE(e.nameBytes.length, 28);
      record.writeUInt16LE(extra.length, 30);
      record.writeUInt32LE(e.directory ? ((0o040755 << 16) >>> 0) : UNIX_FILE_ATTR, 38);
      record.writeUInt32LE(bigOffset ? 0xffffffff : e.offset, 42);
      await this.#write(Buffer.concat([record, e.nameBytes, extra]));
    }
    const cdSize = this.#offset - cdOffset;
    const count = this.#entries.length;
    const needs64 = count >= 0xffff || cdSize >= 0xffffffff || cdOffset >= 0xffffffff;
    if (needs64) {
      const record = Buffer.alloc(56);
      record.writeUInt32LE(SIG_EOCD64, 0);
      record.writeBigUInt64LE(44n, 4);
      record.writeUInt16LE((3 << 8) | 45, 12);
      record.writeUInt16LE(45, 14);
      record.writeBigUInt64LE(BigInt(count), 24);
      record.writeBigUInt64LE(BigInt(count), 32);
      record.writeBigUInt64LE(BigInt(cdSize), 40);
      record.writeBigUInt64LE(BigInt(cdOffset), 48);
      const recordAt = this.#offset;
      await this.#write(record);
      const locator = Buffer.alloc(20);
      locator.writeUInt32LE(SIG_LOCATOR64, 0);
      locator.writeBigUInt64LE(BigInt(recordAt), 8);
      locator.writeUInt32LE(1, 16);
      await this.#write(locator);
    }
    const end = Buffer.alloc(22);
    end.writeUInt32LE(SIG_EOCD, 0);
    end.writeUInt16LE(needs64 ? 0xffff : count, 8);
    end.writeUInt16LE(needs64 ? 0xffff : count, 10);
    end.writeUInt32LE(needs64 ? 0xffffffff : cdSize, 12);
    end.writeUInt32LE(needs64 ? 0xffffffff : cdOffset, 16);
    await this.#write(end);
    await new Promise((resolve, reject) => {
      this.#stream.once('error', reject);
      this.#stream.end(resolve);
    });
    return { entries: count, bytes: this.#offset, zip64: needs64 };
  }

  abort() {
    this.#stream.destroy();
  }
}

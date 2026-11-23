/**
 * Disposable synthetic corpus for MCP conformance. Conversations are produced by
 * the production extension DOM capture function and written through the real
 * native-messaging host, exactly like the existing native smoke fixture. The
 * ground truth for "exact content" is the raw immutable capture file on disk,
 * never the generator input.
 *
 * No owner data, no private archive content: everything here is generated.
 */
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { JSDOM } from 'jsdom';
import { captureConversation } from '../../extension/capture.mjs';

const ORIGIN = 'chrome-extension://dninfeidggaaioioficnigfpfjkdicjh/';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');

const esc = (text) => text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

/** Multi-byte and awkward-script filler: CJK, Indonesian, emoji (astral plane), combining marks, RTL. */
const FILLER = [
  '日本語のメモ：アーカイブの決定を記録する。',
  'Catatan dalam bahasa Indonesia untuk uji kelengkapan.',
  'Emoji 🙂🚀🧠 and an astral-plane pair 𝔘𝔫𝔦𝔠𝔬𝔡𝔢.',
  'Combining marks: é ñ ä stay as typed.',
  'مرحبا بالعالم — right-to-left text stays intact.',
  'Ünïcödé àccénts and ß ñ ø å are preserved.',
];

export function messageText(tag, index, bytesTarget = 0) {
  const base = `[${tag} #${index}] ${FILLER[index % FILLER.length]} sentinel-${tag}-${index}`;
  if (!bytesTarget) return base;
  let text = base;
  let n = 0;
  while (Buffer.byteLength(text) < bytesTarget) {
    text += ` ${FILLER[(index + n) % FILLER.length]}`;
    n += 1;
  }
  return text;
}

function captureOf(id, title, texts) {
  const turns = texts.map((text, i) => {
    const role = i % 2 === 0 ? 'user' : 'assistant';
    return `<article data-testid="conversation-turn-${i + 1}"><div data-message-id="m-${i + 1}" data-message-author-role="${role}"><div class="markdown"><p>${esc(text)}</p></div></div></article>`;
  });
  const dom = new JSDOM(
    `<!doctype html><html><head><title>${esc(title)} - ChatGPT</title></head><body><main>${turns.join('')}</main></body></html>`,
    { url: `https://chatgpt.com/c/${id}`, runScripts: 'outside-only' },
  );
  dom.window.TextEncoder = TextEncoder;
  const result = dom.window.eval(`(${captureConversation.toString()})()`);
  dom.window.close();
  if (!result?.ok) throw new Error(result?.error || 'Production DOM capture failed');
  return result.capture;
}

/** One native-messaging round trip to the packaged capture host. */
export function hostRoundtrip(host, archiveRoot, request) {
  const body = Buffer.from(JSON.stringify(request));
  const prefix = Buffer.alloc(4);
  prefix.writeUInt32LE(body.length);
  const result = spawnSync(host, [ORIGIN], {
    input: Buffer.concat([prefix, body]),
    env: { ...process.env, MIND_PLACE_DATA_DIR: archiveRoot },
    windowsHide: true,
    timeout: 60000,
    maxBuffer: 16 * 1024 * 1024,
  });
  if (result.status !== 0) throw new Error(`Capture host failed: ${result.error?.message || String(result.stderr).slice(0, 200)}`);
  if (result.stdout.length < 4 || result.stdout.readUInt32LE(0) !== result.stdout.length - 4) throw new Error('Invalid native framing');
  return JSON.parse(result.stdout.subarray(4).toString());
}

function saveRevision(host, archiveRoot, id, title, texts) {
  const saved = hostRoundtrip(host, archiveRoot, { protocol: 1, operation: 'save', capture: captureOf(id, title, texts) });
  if (!saved.ok) throw new Error(`Save failed: ${JSON.stringify(saved).slice(0, 200)}`);
  const file = path.join(archiveRoot, 'conversations', saved.id, 'captures', `${saved.snapshot_id}.jsonl`);
  const raw = JSON.parse(fs.readFileSync(file, 'utf8'));
  return {
    archive_id: saved.id,
    snapshot_id: saved.snapshot_id,
    message_count: raw.messages.length,
    /** Ground truth: the exact text of every message as stored in the immutable raw capture. */
    messages: raw.messages.map((m) => m.text),
    raw_sha256: crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex'),
  };
}

/**
 * Four conversations:
 *  long   - many multi-byte messages, three revisions (append, then edit in the middle); used with the default page size
 *  medium - modest size, two revisions; used with small pages so real clients page through it
 *  second - a different collection that only a second credential is granted
 *  other  - never granted to anyone (canary for default deny and isolation)
 */
export function seedCorpus({ host, archiveRoot, longMessages = 240, longBytes = 2200, mediumMessages = 30, mediumBytes = 1400 }) {
  fs.mkdirSync(archiveRoot, { recursive: true });
  const ids = {
    long: '1a2b3c4d-1111-4111-8111-aaaaaaaaaaa1',
    medium: '1a2b3c4d-2222-4222-8222-bbbbbbbbbbb2',
    second: '1a2b3c4d-3333-4333-8333-ccccccccccc3',
    other: '1a2b3c4d-4444-4444-8444-ddddddddddd4',
  };
  const gen = (tag, n, bytes) => Array.from({ length: n }, (_, i) => messageText(tag, i + 1, bytes));
  const out = {};

  const longTitle = 'Conformance long conversation';
  const long1 = gen('long', longMessages, longBytes);
  const long2 = [...long1, ...gen('long-added', 20, longBytes)];
  const long3 = long2.map((t, i) => (i === 120 ? messageText('long-edited', 121, longBytes) : t));
  out.long = { title: longTitle, revisions: [long1, long2, long3].map((texts) => saveRevision(host, archiveRoot, ids.long, longTitle, texts)) };

  const mediumTitle = 'Conformance medium conversation';
  const med1 = gen('medium', mediumMessages, mediumBytes);
  const med2 = [...med1, ...gen('medium-added', 4, mediumBytes)];
  out.medium = { title: mediumTitle, revisions: [med1, med2].map((texts) => saveRevision(host, archiveRoot, ids.medium, mediumTitle, texts)) };

  out.second = { title: 'Conformance second collection', revisions: [saveRevision(host, archiveRoot, ids.second, 'Conformance second collection', gen('second', 6, 400))] };
  out.other = { title: 'Conformance private canary', revisions: [saveRevision(host, archiveRoot, ids.other, 'Conformance private canary', gen('canary', 6, 400))] };
  return out;
}

export { root as repoRoot };

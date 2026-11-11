// Sanitization rules: what is kept, what is pseudonymized, and what is replaced.
//
// Design principle: DEFAULT-DENY. A string survives only when an explicit allow-list rule
// keeps it (structural enums, class names, known labels, known hosts). Everything else is
// replaced by a placeholder whose length class matches, or by a keyed pseudonym whose shape
// matches (identifiers). The pseudonym key never leaves the process and no low-entropy value
// (names, e-mail addresses, text) is ever hashed, so a pseudonym cannot be dictionary-checked.
import { createHmac } from 'node:crypto';
import { LeakRegistry } from './leak-check.mjs';

export const DEFAULT_SALT = 'mind-place-import-sanitizer-v1';

const VOCAB = ['lorem', 'ipsum', 'dolor', 'sit', 'amet', 'elit', 'sed', 'do', 'tempor', 'ut', 'enim', 'ad',
  'minim', 'veniam', 'quis', 'nisi', 'aliquip', 'ex', 'ea', 'duis', 'aute', 'irure', 'in', 'esse', 'cillum',
  'fugiat', 'nulla'];
export const VOCAB_SET = new Set(VOCAB);
const CLASS_BOUNDS = [0, 1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384];
export const MAX_PLACEHOLDER_CHARS = 16384;

/**
 * Representative length of the length class `n` falls into: the class's lower bound, capped.
 * Rounding down guarantees a placeholder is never longer than the text it replaces, so the
 * sanitized copy is never larger than the original.
 */
export function lengthClass(n) {
  let representative = 0;
  for (const bound of CLASS_BOUNDS) if (bound <= n) representative = bound;
  return representative;
}

export function placeholderWords(length) {
  if (length <= 0) return '';
  let out = '';
  for (let i = 0; out.length < length; i++) out += (out ? ' ' : '') + VOCAB[i % VOCAB.length];
  out = out.slice(0, length);
  return out.endsWith(' ') ? `${out.slice(0, -1)}x` : out;
}

const MARKER = /^([ \t]*)(#{1,6} |[-*+] |\d{1,3}[.)] |> |```)?([\s\S]*)$/;

/** Replaces text with filler, keeping line structure, markdown markers and per-line length class. */
export function neutralizeText(text) {
  if (!text) return text;
  return text.split('\n').map(rawLine => {
    const cr = rawLine.endsWith('\r');
    const line = cr ? rawLine.slice(0, -1) : rawLine;
    const [, lead = '', marker = '', rest = ''] = MARKER.exec(line) ?? [];
    return `${lead}${marker}${rest ? placeholderWords(lengthClass(rest.length)) : ''}${cr ? '\r' : ''}`;
  }).join('\n');
}

export const KEEP_HOSTS = new Set(['gemini.google.com', 'chatgpt.com', 'chat.openai.com', 'www.google.com',
  'myactivity.google.com', 'policies.google.com', 'support.google.com', 'files.oaiusercontent.com']);
const KEEP_SCHEMES = new Set(['http', 'https', 'file-service', 'sediment', 'mindplace-import', 'data']);
const KEEP_PATH_SEGMENTS = new Set(['app', 'c', 'share', 'g', 'gem', 'gems', 'chat', 'files', 'download', 'u', 'backend-api',
  'conversation', 'gemini', 'activity', 'myactivity', 'canvas', 'gemini-takeout', 'gemini-canvas', 'chatgpt-export']);

// Enumerated structural values (role, content type, ...). Kept only when the VALUE also looks
// like an enum, so free text stored under such a key is still replaced.
export const ENUM_KEYS = new Set(['role', 'content_type', 'status', 'recipient', 'channel', 'default_model_slug',
  'model_slug', 'conversation_origin', 'memory_scope', 'gizmo_type', 'type', 'language', 'format', 'mime_type',
  'mimetype', 'kind', 'source', 'command', 'message_type', 'finish_type', 'async_status', 'resolved_model_slug',
  'parent_model_slug']);
const ENUM_VALUE = /^[A-Za-z0-9_.:\-/+]{0,48}$/;
const ID_KEYS = new Set(['id', 'parent', 'children', 'current_node', 'conversation_id', 'message_id', 'parent_id',
  'root', 'async_task_id', 'request_id', 'turn_exchange_id', 'file_id', 'asset_pointer', 'gizmo_id',
  'conversation_template_id', 'plugin_ids', 'sugar_item_id', 'pointer', 'parent_message_id', 'msg_id', 'share_id',
  'link_id', 'user_id', 'workspace_id', 'account_id', 'org_id', 'project_id', 'sharing_id']);
const ID_CONTAINER_KEYS = new Set(['mapping']);
const KEEP_ID_LITERALS = new Set(['client-created-root', 'root']);
const TIMESTAMP_KEY = /(time|timestamp|created|updated|date)$/i;

const UUID = /^([0-9a-fA-F]{8})-([0-9a-fA-F]{4})-([0-9a-fA-F])([0-9a-fA-F]{3})-([0-9a-fA-F])([0-9a-fA-F]{3})-([0-9a-fA-F]{12})$/;
const URL_SHAPE = /^([A-Za-z][A-Za-z0-9+.-]*):(?:\/\/([^/?#]*))?([^?#]*)(\?[^#]*)?(#[\s\S]*)?$/;
const ID_LIKE_SEGMENT = /^(?:[0-9a-fA-F]{12,}|[A-Za-z0-9_-]{16,}|(?:file|user|org|conv|msg|node)[-_][A-Za-z0-9_-]{6,})$/;
const ID_SHAPED_KEY = /^(?:(?:file|user|conv|msg|node)[-_][A-Za-z0-9_-]{6,}|[0-9a-fA-F]{16,})$/;
const ID_PREFIX = /^(?:file|user|org|conv|msg|node|chatcmpl|resp|req|run|sess|task|gen|gizmo|asst|thread|project|share)[-_]/;
const FILE_ID_IN_NAME = /^((?:file|user)[-_][A-Za-z0-9]{6,})/;

const STAND_IN_PNG = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==', 'base64');
const STAND_IN_GIF = Buffer.from('R0lGODlhAQABAAAAACH5BAEKAAEALAAAAAABAAEAAAICTAEAOw==', 'base64');
const STAND_IN_WEBP = Buffer.from('UklGRhoAAABXRUJQVlA4TA0AAAAvAAAAEAcQERGIiP4HAA==', 'base64');
const STAND_IN_JPEG = Buffer.from('/9j/4AAQSkZJRgABAQEASABIAAD/2wBDAP//////////////////////////////////////////////////////////////////////////////////////wgALCAABAAEBAREA/8QAFBABAAAAAAAAAAAAAAAAAAAAAP/aAAgBAQABPxA=', 'base64');
const STAND_IN_PDF = Buffer.from('%PDF-1.4\n% sanitized stand-in\n%%EOF\n');
const STAND_IN_BINARY = Buffer.from('SANITIZED-MEDIA-STAND-IN\n');
const MEDIA_STAND_INS = new Map([
  ['png', STAND_IN_PNG], ['gif', STAND_IN_GIF], ['webp', STAND_IN_WEBP], ['jpg', STAND_IN_JPEG],
  ['jpeg', STAND_IN_JPEG], ['pdf', STAND_IN_PDF],
]);
const DATA_URI_STAND_INS = new Map([
  ['image/png', STAND_IN_PNG], ['image/gif', STAND_IN_GIF], ['image/webp', STAND_IN_WEBP], ['image/jpeg', STAND_IN_JPEG],
]);

export const TEXT_MEMBER = /\.(?:txt|md|csv|tsv|log)$/i;
export const JSON_MEMBER = /\.(?:json|jsonl|ndjson)$/i;
export const HTML_MEMBER = /\.(?:html?|xhtml)$/i;
const STRUCTURAL_NAMES = new Set(['conversations.json', 'user.json', 'message_feedback.json', 'shared_conversations.json',
  'export_manifest.json', 'model_comparisons.json', 'chat.html', 'library.json', 'projects.json', 'gizmos.json',
  'Takeout', 'My Activity', 'Gemini Apps', 'MyActivity.html', 'archive_browser.html', 'index.html', 'dalle-generations',
  'Gemini']);
const STRUCTURAL_NAME_PATTERN = /^(?:conversations(?:-\d{1,6})?\.json|(?:user|message_feedback|shared_conversations|export_manifest)\.jsonl?)$/;

export function isIdKey(key) {
  return ID_KEYS.has(key) || /(?:_ids?|Id|Ids)$/.test(key);
}

export function isIdValue(value, key) {
  return UUID.test(value) || isIdKey(key);
}

export function isIdContainerKey(key) {
  return ID_CONTAINER_KEYS.has(key);
}

/** Property names that carry data or identifiers rather than structure. */
export function isDataKey(key) {
  return UUID.test(key) || ID_SHAPED_KEY.test(key)
    || key.length > 64 || /[^A-Za-z0-9_.:@\- /]/.test(key) || /\s/.test(key) || key.includes('@');
}

export const CLOCK_NODE = /^(\s*)(\d{1,2} \p{L}{3,10} \d{4}, )(\d{1,2}):(\d{2}):(\d{2})( [A-Za-z]{1,6}|\s?[+-]\d{2}:?\d{2})?(\s*)$/u;

export class Rules {
  constructor({ salt = DEFAULT_SALT, timestamps = 'day', keepLabels = [], keepKeys = [], leak = new LeakRegistry() } = {}) {
    if (!['day', 'keep'].includes(timestamps)) throw new Error('E_OPTION timestamps must be day or keep');
    this.salt = salt;
    this.timestamps = timestamps;
    this.keepLabels = ['Prompted', ...keepLabels];
    this.keepKeys = new Set(keepKeys);
    this.leak = leak;
    this.shortIds = new Map(); // original -> pseudonym, only for short IDs, to keep the mapping injective
    this.shortIdTaken = new Map(); // pseudonym -> original
    this.counters = { stringsReplaced: 0, idsPseudonymized: 0, mediaStandIns: 0, timestampsCoarsened: 0 };
    this.keptEnums = new Map(); // key -> Map(value -> count): exactly which enum-like values were preserved
    this.keptEnumOverflow = false;
  }

  /** Records a string as intentionally preserved by an allow-list rule. */
  kept(value) {
    this.leak.markKept(value);
    return value;
  }

  // ---- identifiers ----

  #bytes(label, value, count) {
    const out = Buffer.alloc(count);
    for (let block = 0, at = 0; at < count; block++) {
      const digest = createHmac('sha256', this.salt).update(`${label}\0${block}\0${value}`).digest();
      digest.copy(out, at, 0, Math.min(32, count - at));
      at += 32;
    }
    return out;
  }

  #shape(value, label, attempt = 0) {
    const hexOnly = /^[0-9a-fA-F]+$/.test(value);
    const upper = value === value.toUpperCase() && /[A-F]/.test(value);
    const bytes = this.#bytes(`${label}${attempt}`, value, value.length);
    // A well-known identifier prefix (file-, user-, ...) is structure, not data.
    const prefix = ID_PREFIX.exec(value)?.[0] ?? '';
    let out = prefix ? this.kept(prefix) : '';
    for (let i = prefix.length; i < value.length; i++) {
      const c = value[i];
      const b = bytes[i];
      if (/[0-9]/.test(c)) out += hexOnly ? '0123456789abcdef'[b & 15] : String(b % 10);
      else if (/[a-z]/.test(c)) out += hexOnly ? 'abcdef'[b % 6] : String.fromCharCode(97 + (b % 26));
      else if (/[A-Z]/.test(c)) out += hexOnly ? 'ABCDEF'[b % 6] : String.fromCharCode(65 + (b % 26));
      else if (c.charCodeAt(0) > 127) out += String.fromCharCode(97 + (b % 26));
      else out += c;
    }
    return upper ? out.toUpperCase() : out;
  }

  /** Same-shape keyed pseudonym; equal inputs map to equal outputs, distinct inputs to distinct outputs. */
  pseudonymize(value, label = 'id') {
    if (KEEP_ID_LITERALS.has(value)) return this.kept(value);
    this.leak.protect(value);
    this.counters.idsPseudonymized++;
    const uuid = UUID.exec(value);
    if (uuid) {
      const bytes = this.#bytes(`${label}-uuid`, value, 16).toString('hex');
      const [, , , version, , variant] = uuid;
      const out = `${bytes.slice(0, 8)}-${bytes.slice(8, 12)}-${version}${bytes.slice(13, 16)}-${variant}${bytes.slice(17, 20)}-${bytes.slice(20, 32)}`;
      return /[A-F]/.test(value) && value === value.toUpperCase() ? out.toUpperCase() : out;
    }
    const alnum = value.replace(/[^A-Za-z0-9]/g, '').length;
    if (alnum >= 12) return this.#shape(value, label);
    // Short IDs have a small output space: guarantee injectivity with an explicit table.
    const known = this.shortIds.get(`${label}\0${value}`);
    if (known !== undefined) return known;
    for (let attempt = 0; ; attempt++) {
      const candidate = this.#shape(value, label, attempt);
      const owner = this.shortIdTaken.get(`${label}\0${candidate}`);
      if (owner === undefined || owner === value) {
        this.shortIds.set(`${label}\0${value}`, candidate);
        this.shortIdTaken.set(`${label}\0${candidate}`, value);
        return candidate;
      }
      if (attempt > 64) throw new Error('E_ID_SPACE short identifier space exhausted');
    }
  }

  // ---- URLs ----

  neutralizeUrl(value) {
    const data = /^data:([^;,]*)(;[^,]*)?,/.exec(value);
    if (data) {
      this.counters.mediaStandIns++;
      this.leak.protect(value);
      const mime = data[1].toLowerCase();
      const stand = DATA_URI_STAND_INS.get(mime);
      return this.kept(stand ? `data:${mime};base64,${stand.toString('base64')}` : 'data:application/octet-stream;base64,U0FOSVRJWkVE');
    }
    const m = URL_SHAPE.exec(value);
    if (!m) return null;
    const [, scheme, authority, path = '', query, fragment] = m;
    if (!KEEP_SCHEMES.has(scheme.toLowerCase())) return null;
    this.leak.protect(value);
    let out = `${this.kept(scheme.toLowerCase())}:`;
    if (authority !== undefined) {
      const host = authority.toLowerCase();
      if (KEEP_HOSTS.has(host)) out += `//${this.kept(host)}`;
      else if (/^(file-service|sediment)$/i.test(scheme)) out += `//${this.pseudonymize(authority, 'file-host')}`;
      else out += `//${this.#hostPlaceholder(authority)}.invalid`;
    }
    out += path.split('/').map(segment => this.#pathSegment(segment)).join('/');
    if (query) out += `?${query.slice(1).split('&').map(pair => this.#queryPair(pair)).join('&')}`;
    if (fragment) out += '#x';
    return out;
  }

  #hostPlaceholder(host) {
    return this.#shape(host.replace(/[^A-Za-z0-9]/g, 'x').toLowerCase() || 'x', 'host');
  }

  #pathSegment(segment) {
    if (!segment) return segment;
    if (KEEP_PATH_SEGMENTS.has(segment)) return this.kept(segment);
    if (ID_LIKE_SEGMENT.test(segment)) return this.pseudonymize(segment, 'path-id');
    this.leak.protect(segment);
    return this.#shape(segment.toLowerCase(), 'path');
  }

  #queryPair(pair) {
    const [name = ''] = pair.split('=');
    const keep = /^[a-z_]{1,24}$/.test(name) ? this.kept(name) : 'p';
    return pair.includes('=') ? `${keep}=x` : keep;
  }

  // ---- JSON ----

  isIdKey(key) {
    return isIdKey(key);
  }

  isIdValue(value, key) {
    return isIdValue(value, key);
  }

  /** Object property names: structural unless they carry data or identifiers. */
  jsonKey(key, frame) {
    // Keys that name nodes (mapping) or look like identifiers must map exactly as the same string
    // does when it appears as a value (`id`, `parent`, `children`), or references would break.
    if (frame?.idKeys || UUID.test(key) || ID_SHAPED_KEY.test(key)) return this.pseudonymize(key);
    if (isDataKey(key)) return this.pseudonymize(key, 'data-key');
    return this.kept(key);
  }

  isIdContainer(key) {
    return isIdContainerKey(key);
  }

  isDataKeyName(key) {
    return isDataKey(key);
  }

  #keepEnum(key, value) {
    let values = this.keptEnums.get(key);
    if (!values) {
      if (this.keptEnums.size >= 200) { this.keptEnumOverflow = true; return this.kept(value); }
      values = new Map();
      this.keptEnums.set(key, values);
    }
    if (values.size < 100 || values.has(value)) values.set(value, (values.get(value) ?? 0) + 1);
    else this.keptEnumOverflow = true;
    return this.kept(value);
  }

  /** Kept enum-like values by key, for the owner's review before sharing. */
  keptEnumReport() {
    const byKey = {};
    for (const [key, values] of [...this.keptEnums].sort(([a], [b]) => (a < b ? -1 : 1))) {
      byKey[key] = Object.fromEntries([...values].sort(([a], [b]) => (a < b ? -1 : 1)));
    }
    return { byKey, truncated: this.keptEnumOverflow };
  }

  jsonString(value, ctx) {
    const key = ctx.key ?? '';
    if (this.keepKeys.has(key) && ENUM_VALUE.test(value)) return this.#keepEnum(key, value);
    if ((ENUM_KEYS.has(key) || (key === 'name' && ctx.parentKey === 'author')) && ENUM_VALUE.test(value)) {
      return this.#keepEnum(key === 'name' ? 'author.name' : key, value);
    }
    if (KEEP_ID_LITERALS.has(value) && (this.isIdKey(key) || ctx.inIdContext)) return this.kept(value);
    const url = this.neutralizeUrl(value);
    if (url !== null) return url;
    if (UUID.test(value) || this.isIdKey(key) || ctx.inIdContext) {
      if (value === '') return value;
      return this.pseudonymize(value);
    }
    return this.replaceText(value);
  }

  replaceText(value) {
    this.leak.protect(value);
    this.counters.stringsReplaced++;
    return neutralizeText(value);
  }

  /** Numbers under time-like keys keep their representation but lose time-of-day. */
  jsonNumber(raw, ctx) {
    const key = ctx.key ?? '';
    if (this.timestamps !== 'day' || !TIMESTAMP_KEY.test(key)) return raw;
    const m = /^(-?)(\d{9,13})(?:\.(\d+))?$/.exec(raw);
    if (!m) return raw;
    const [, sign, whole, fraction] = m;
    const millis = whole.length >= 13;
    const unit = millis ? 86_400_000 : 86_400;
    const floored = String(Math.floor(Number(whole) / unit) * unit).padStart(whole.length, '0');
    this.counters.timestampsCoarsened++;
    return `${sign}${floored}${fraction === undefined ? '' : `.${'0'.repeat(fraction.length)}`}`;
  }

  // ---- HTML ----

  coarsenClock(text) {
    const m = CLOCK_NODE.exec(text);
    if (!m) return null;
    if (this.timestamps !== 'day') return this.kept(text);
    this.counters.timestampsCoarsened++;
    const [, lead, date, h, , , zone = '', trail] = m;
    return this.kept(`${lead}${date}${h.length === 2 ? '00' : '0'}:00:00${zone}${trail}`);
  }

  htmlText(text) {
    if (!text.trim()) return text;
    const clock = this.coarsenClock(text);
    if (clock !== null) return clock;
    const lead = /^\s*/.exec(text)[0];
    const trail = /\s*$/.exec(text)[0];
    const body = text.slice(lead.length, text.length - trail.length);
    for (const label of this.keepLabels) {
      if (body === label || (body.startsWith(label) && /^\s/.test(body.slice(label.length)))) {
        const rest = body.slice(label.length);
        const space = /^\s*/.exec(rest)[0];
        return `${lead}${this.kept(label)}${space}${this.replaceText(rest.slice(space.length))}${trail}`;
      }
    }
    return `${lead}${this.replaceText(body)}${trail}`;
  }

  htmlRawText(tag, text) {
    if (!text.trim()) return text;
    this.leak.protect(text);
    this.counters.stringsReplaced++;
    if (tag === 'script' || tag === 'style') return `/*${placeholderWords(Math.max(1, lengthClass(text.length) - 4))}*/`;
    return this.htmlText(text);
  }

  htmlComment(text) {
    this.leak.protect(text);
    return placeholderWords(lengthClass(text.length));
  }

  htmlAttr(tag, name, value) {
    const lower = name.toLowerCase();
    if (lower === 'class' || lower === 'lang' || lower === 'dir' || lower === 'charset' || lower === 'type'
      || lower === 'role' || lower === 'target' || lower === 'colspan' || lower === 'rowspan' || lower === 'rel') {
      return /^[A-Za-z0-9_\-:. ]{0,200}$/.test(value) ? this.kept(value) : this.replaceText(value);
    }
    if (['href', 'src', 'srcset', 'action', 'poster', 'cite', 'data'].includes(lower)) {
      return this.neutralizeUrl(value.trim()) ?? this.replaceText(value);
    }
    if (lower === 'id' || lower === 'for' || lower === 'name') return value ? this.pseudonymize(value, 'attr-id') : value;
    return value ? this.replaceText(value) : value;
  }

  // ---- members ----

  memberName(rawName) {
    const name = rawName.replaceAll('\\', '/'); // some zip tools store Windows separators
    const isDir = name.endsWith('/');
    const parts = (isDir ? name.slice(0, -1) : name).split('/');
    const out = parts.map((segment, index) => this.#segmentName(segment, index === parts.length - 1 && !isDir));
    return out.join('/') + (isDir ? '/' : '');
  }

  #segmentName(segment, isFile) {
    if (!segment || segment === '.' || segment === '..') return 'x';
    if (STRUCTURAL_NAMES.has(segment) || STRUCTURAL_NAME_PATTERN.test(segment)) return this.kept(segment);
    this.leak.protect(segment);
    const dot = isFile ? segment.lastIndexOf('.') : -1;
    const ext = dot > 0 && /^\.[A-Za-z0-9]{1,8}$/.test(segment.slice(dot)) ? segment.slice(dot) : '';
    const stem = ext ? segment.slice(0, dot) : segment;
    const id = FILE_ID_IN_NAME.exec(stem);
    if (id) {
      const rest = stem.slice(id[1].length);
      return `${this.pseudonymize(id[1])}${rest ? this.#portable(this.#shape(rest, 'name')) : ''}${this.kept(ext)}`;
    }
    return `${this.#portable(this.#shape(stem, 'name'))}${this.kept(ext)}`;
  }

  /** Output names must be valid on Windows too: no reserved characters, no trailing dot or space. */
  #portable(name) {
    return name.replace(/[^A-Za-z0-9._\-() []/g, '_').replace(/[. ]+$/, '_') || 'x';
  }

  /** Bytes that replace a media/binary member. */
  standIn(name) {
    this.counters.mediaStandIns++;
    const ext = /\.([A-Za-z0-9]{1,8})$/.exec(name)?.[1]?.toLowerCase() ?? '';
    return MEDIA_STAND_INS.get(ext) ?? STAND_IN_BINARY;
  }
}

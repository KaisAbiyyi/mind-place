// Streaming JSON tokenizer and structure-preserving rewriter.
//
// The rewriter never builds a document: tokens are classified in context (object key, array
// element, enclosing key) and re-emitted. Whitespace, key order, nesting, number formats and
// the escaping of preserved strings are kept byte-for-byte; every other string is replaced by
// the rules. Memory is bounded by the largest single token, not by the file.
import { StringDecoder } from 'node:string_decoder';

export const MAX_TOKEN_CHARS = 128 * 1024 * 1024;

export class JsonError extends Error {
  constructor(code, offset) {
    super(`${code}${offset === undefined ? '' : ` at character ${offset}`}`); // never includes input text
    this.code = code;
  }
}

const PUNCT = new Set(['{', '}', '[', ']', ':', ',']);
const WS = /[ \t\r\n﻿]/;

/** Yields { type: 'ws' | 'punct' | 'string' | 'scalar', raw } without ever holding the whole input. */
export async function* scanJson(readable) {
  const decoder = new StringDecoder('utf8');
  let buf = '';
  let pos = 0;
  let consumed = 0; // characters dropped from the front of `buf`, for error offsets

  function* tokens(eof) {
    while (pos < buf.length) {
      const c = buf[pos];
      if (WS.test(c)) {
        let end = pos + 1;
        while (end < buf.length && WS.test(buf[end])) end++;
        yield { type: 'ws', raw: buf.slice(pos, end) };
        pos = end;
      } else if (PUNCT.has(c)) {
        yield { type: 'punct', raw: c };
        pos++;
      } else if (c === '"') {
        let from = pos + 1;
        let end = -1;
        for (;;) {
          const quote = buf.indexOf('"', from);
          if (quote < 0) break;
          let slashes = 0;
          for (let j = quote - 1; j > pos && buf[j] === '\\'; j--) slashes++;
          if (slashes % 2 === 0) { end = quote + 1; break; }
          from = quote + 1;
        }
        if (end < 0) {
          if (eof) throw new JsonError('E_JSON_UNTERMINATED_STRING', consumed + pos);
          if (buf.length - pos > MAX_TOKEN_CHARS) throw new JsonError('E_JSON_TOKEN_TOO_LARGE', consumed + pos);
          return;
        }
        yield { type: 'string', raw: buf.slice(pos, end) };
        pos = end;
      } else {
        let end = pos + 1;
        while (end < buf.length && !WS.test(buf[end]) && !PUNCT.has(buf[end]) && buf[end] !== '"') end++;
        if (end >= buf.length && !eof) return; // the scalar may continue in the next chunk
        yield { type: 'scalar', raw: buf.slice(pos, end) };
        pos = end;
      }
    }
  }

  for await (const chunk of readable) {
    buf = buf.slice(pos) + decoder.write(chunk);
    consumed += pos;
    pos = 0;
    yield* tokens(false);
  }
  buf = buf.slice(pos) + decoder.end();
  consumed += pos;
  pos = 0;
  yield* tokens(true);
}

function decodeString(raw, offsetHint) {
  try {
    return JSON.parse(raw);
  } catch {
    throw new JsonError('E_JSON_BAD_STRING', offsetHint); // JSON.parse messages can quote input; drop them
  }
}

const OUT_FLUSH_CHARS = 256 * 1024;

/**
 * Rewrites a JSON stream through `rules`. `events` (optional) receives structure callbacks used
 * for the inventory: container(path, type), value(path, type, isId, original, result),
 * key(path, segment, original, result).
 * `multiRoot` accepts several top-level values (JSON Lines).
 */
export async function* rewriteJson(readable, rules, { events = null, multiRoot = false } = {}) {
  const frames = [];
  let out = '';
  let rootValues = 0;
  let offset = 0;

  const top = () => frames[frames.length - 1];
  const valueKey = () => {
    const frame = top();
    if (!frame) return '';
    return frame.kind === 'object' ? (frame.key ?? '') : frame.arrayKey;
  };
  const valuePath = () => {
    const frame = top();
    if (!frame) return '$';
    return frame.kind === 'object' ? frame.valuePath : `${frame.path}[]`;
  };
  const beginValue = () => {
    if (!frames.length) {
      if (rootValues >= 1 && !multiRoot) throw new JsonError('E_JSON_MULTIPLE_ROOTS', offset);
      rootValues++;
    }
  };

  for await (const token of scanJson(readable)) {
    const { type, raw } = token;
    offset += raw.length;
    if (type === 'ws') {
      out += raw;
    } else if (type === 'punct') {
      out += raw;
      if (raw === '{' || raw === '[') {
        beginValue();
        const path = valuePath();
        const parentKey = valueKey();
        events?.container(path, raw === '{' ? 'object' : 'array');
        frames.push(raw === '{'
          ? { kind: 'object', key: null, expectKey: true, parentKey, path, valuePath: path, idKeys: rules.isIdContainer(parentKey) }
          : { kind: 'array', arrayKey: parentKey, parentKey, path });
      } else if (raw === '}' || raw === ']') {
        const frame = frames.pop();
        if (!frame || (raw === '}') !== (frame.kind === 'object')) throw new JsonError('E_JSON_MISMATCHED_BRACKET', offset);
      } else if (raw === ',') {
        const frame = top();
        if (!frame) throw new JsonError('E_JSON_UNEXPECTED_COMMA', offset);
        if (frame.kind === 'object') { frame.expectKey = true; frame.key = null; }
      } else if (raw === ':') {
        const frame = top();
        if (!frame || frame.kind !== 'object' || frame.expectKey) throw new JsonError('E_JSON_UNEXPECTED_COLON', offset);
      }
    } else if (type === 'string') {
      const frame = top();
      const decoded = decodeString(raw, offset);
      rules.leak?.protect(decoded); // every input string is tracked; only allow-listed output is exempt
      if (frame?.kind === 'object' && frame.expectKey) {
        const key = rules.jsonKey(decoded, frame);
        frame.key = decoded;
        frame.expectKey = false;
        frame.valuePath = `${frame.path}.${frame.idKeys || rules.isDataKeyName(decoded) ? '*' : decoded}`;
        events?.key(frame.path, frame.idKeys || rules.isDataKeyName(decoded) ? '*' : decoded, decoded, key);
        out += key === decoded ? raw : JSON.stringify(key);
      } else {
        beginValue();
        const ctx = { key: valueKey(), parentKey: frame ? frame.parentKey : '', inIdContext: false };
        const result = rules.jsonString(decoded, ctx);
        events?.value(valuePath(), 'string', rules.isIdValue(decoded, ctx.key), decoded, result);
        out += result === decoded ? raw : JSON.stringify(result);
      }
    } else {
      beginValue();
      const frame = top();
      const ctx = { key: valueKey(), parentKey: frame ? frame.parentKey : '', inIdContext: false };
      let emitted = raw;
      if (raw === 'true' || raw === 'false') events?.value(valuePath(), 'boolean', false, raw, raw);
      else if (raw === 'null') events?.value(valuePath(), 'null', false, raw, raw);
      else if (/^-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?$/.test(raw)) {
        emitted = rules.jsonNumber(raw, ctx);
        events?.value(valuePath(), 'number', false, raw, emitted);
      } else throw new JsonError('E_JSON_BAD_VALUE', offset);
      out += emitted;
    }
    if (out.length >= OUT_FLUSH_CHARS) {
      yield out;
      out = '';
    }
  }
  if (frames.length) throw new JsonError('E_JSON_UNTERMINATED', offset);
  if (!rootValues) throw new JsonError('E_JSON_EMPTY', offset);
  if (out) yield out;
}

/** Strings of a JSON stream (keys and values), decoded, for the independent output leak scan. */
export async function* jsonStrings(readable) {
  for await (const token of scanJson(readable)) {
    if (token.type === 'string') yield decodeString(token.raw);
  }
}

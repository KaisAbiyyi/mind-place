// Streaming HTML scanner and structure-preserving rewriter.
//
// This is a deliberately small lexer, not an HTML5 tree builder: it keeps every tag, attribute
// name, attribute quoting style, comment boundary and raw-text element exactly where they were,
// and hands only the *values* (text nodes, attribute values, comment/script bodies) to the rules.
// The scan is independent of how the input is chunked.
import { StringDecoder } from 'node:string_decoder';

import { DEFAULT_BUDGETS } from './budgets.mjs';
const RAW_TEXT = new Set(['script', 'style', 'textarea', 'title']);

export class HtmlError extends Error {
  constructor(code) {
    super(code);
    this.code = code;
  }
}

/** Parses a tag starting at buf[pos] === '<'. Returns null when more input is needed, false when not a tag. */
function parseTag(buf, pos) {
  let i = pos + 1;
  const closing = buf[i] === '/';
  if (closing) i++;
  const name = /[A-Za-z][^\s/>]*/y;
  name.lastIndex = i;
  const nameMatch = name.exec(buf);
  if (!nameMatch) return i >= buf.length ? null : false;
  i += nameMatch[0].length;
  const parts = [{ lit: buf.slice(pos, i) }];
  for (;;) {
    const gap = /[\s/]*/y;
    gap.lastIndex = i;
    const gapText = gap.exec(buf)[0];
    if (gapText) { parts.push({ lit: gapText }); i += gapText.length; }
    if (i >= buf.length) return null;
    if (buf[i] === '>') {
      parts.push({ lit: '>' });
      return { end: i + 1, closing, name: nameMatch[0].toLowerCase(), parts, selfClosing: /\/\s*$/.test(gapText) };
    }
    const attr = /[^\s=/>]+/y;
    attr.lastIndex = i;
    const attrName = attr.exec(buf)?.[0];
    if (!attrName) { parts.push({ lit: buf[i] }); i++; continue; }
    i += attrName.length;
    const eq = /(\s*)=(\s*)/y;
    eq.lastIndex = i;
    const eqMatch = eq.exec(buf);
    if (!eqMatch) {
      if (i >= buf.length) return null;
      parts.push({ lit: attrName });
      continue;
    }
    i += eqMatch[0].length;
    if (i >= buf.length) return null;
    const quote = buf[i] === '"' || buf[i] === "'" ? buf[i] : '';
    let value;
    if (quote) {
      const close = buf.indexOf(quote, i + 1);
      if (close < 0) return null;
      value = buf.slice(i + 1, close);
      i = close + 1;
    } else {
      const bare = /[^\s>]*/y;
      bare.lastIndex = i;
      value = bare.exec(buf)[0];
      i += value.length;
      if (i >= buf.length) return null;
    }
    parts.push({ lit: `${attrName}${eqMatch[1]}=${eqMatch[2]}`, attr: attrName, quote, value });
  }
}

/**
 * Yields segments: { type: 'text'|'comment'|'decl'|'start'|'end'|'rawtext', raw, ... }.
 * start/end carry `name` and `parts` ({lit} or {lit, attr, quote, value}); rawtext carries `tag`.
 */
export async function* scanHtml(readable, { maxTokenChars = DEFAULT_BUDGETS.maxTokenChars } = {}) {
  const decoder = new StringDecoder('utf8');
  let buf = '';
  let pos = 0;
  let rawTag = null;

  function* segments(eof) {
    while (pos < buf.length) {
      if (rawTag) {
        const close = new RegExp(`</${rawTag}(?![A-Za-z0-9])`, 'i');
        const rest = buf.slice(pos);
        const found = close.exec(rest);
        if (!found && !eof) {
          if (rest.length > maxTokenChars) throw new HtmlError('E_HTML_TOKEN_TOO_LARGE');
          return;
        }
        const end = found ? pos + found.index : buf.length;
        if (end > pos) yield { type: 'rawtext', raw: buf.slice(pos, end), tag: rawTag };
        pos = end;
        rawTag = null;
        continue;
      }
      if (buf[pos] === '<') {
        if (buf.startsWith('<!--', pos)) {
          const close = buf.indexOf('-->', pos + 4);
          if (close < 0) {
            if (!eof) { if (buf.length - pos > maxTokenChars) throw new HtmlError('E_HTML_TOKEN_TOO_LARGE'); return; }
            yield { type: 'comment', raw: buf.slice(pos), body: buf.slice(pos + 4) };
            pos = buf.length;
          } else {
            yield { type: 'comment', raw: buf.slice(pos, close + 3), body: buf.slice(pos + 4, close) };
            pos = close + 3;
          }
          continue;
        }
        if (buf[pos + 1] === '!' || buf[pos + 1] === '?') {
          const close = buf.indexOf('>', pos);
          if (close < 0) {
            if (!eof) {
              // An unfinished <! or <? construct must not accumulate without bound either.
              if (buf.length - pos > maxTokenChars) throw new HtmlError('E_HTML_TOKEN_TOO_LARGE');
              return;
            }
            yield { type: 'text', raw: buf.slice(pos) };
            pos = buf.length;
          } else {
            if (close + 1 - pos > maxTokenChars) throw new HtmlError('E_HTML_TOKEN_TOO_LARGE');
            yield { type: 'decl', raw: buf.slice(pos, close + 1) };
            pos = close + 1;
          }
          continue;
        }
        const tag = parseTag(buf, pos);
        if (tag === null) {
          if (!eof) { if (buf.length - pos > maxTokenChars) throw new HtmlError('E_HTML_TOKEN_TOO_LARGE'); return; }
          yield { type: 'text', raw: buf.slice(pos) };
          pos = buf.length;
          continue;
        }
        if (tag) {
          yield { type: tag.closing ? 'end' : 'start', raw: buf.slice(pos, tag.end), name: tag.name, parts: tag.parts, selfClosing: tag.selfClosing };
          pos = tag.end;
          if (!tag.closing && !tag.selfClosing && RAW_TEXT.has(tag.name)) rawTag = tag.name;
          continue;
        }
      }
      // Text runs until the next '<' that begins a real construct.
      let end = pos + 1;
      for (;;) {
        const next = buf.indexOf('<', end);
        if (next < 0) {
          if (!eof) {
            if (buf.length - pos > maxTokenChars) throw new HtmlError('E_HTML_TOKEN_TOO_LARGE');
            return;
          }
          end = buf.length;
          break;
        }
        const after = buf[next + 1];
        if (after === undefined && !eof) return;
        if (after !== undefined && /[A-Za-z!?/]/.test(after)) { end = next; break; }
        end = next + 1;
      }
      yield { type: 'text', raw: buf.slice(pos, end) };
      pos = end;
    }
  }

  for await (const chunk of readable) {
    buf = buf.slice(pos) + decoder.write(chunk);
    pos = 0;
    yield* segments(false);
  }
  buf = buf.slice(pos) + decoder.end();
  pos = 0;
  yield* segments(true);
}

const OUT_FLUSH_CHARS = 256 * 1024;
const safeAttr = (value, quote) => value.replace(quote ? new RegExp(quote, 'g') : /[\s>"']/g, 'x');

/** Rewrites an HTML stream through `rules`. `events` receives structure callbacks for the inventory. */
export async function* rewriteHtml(readable, rules, { events = null, budgets = DEFAULT_BUDGETS } = {}) {
  let out = '';
  for await (const segment of scanHtml(readable, budgets)) {
    if (segment.raw.length > budgets.maxTokenChars) throw new HtmlError('E_HTML_TOKEN_TOO_LARGE');
    // Every input string is tracked by the leak registry; only allow-listed output is exempt.
    if (rules.leak) {
      if (segment.type === 'text') rules.leak.protect(segment.raw.trim());
      else if (segment.type === 'comment') rules.leak.protect(segment.body);
      else if (segment.type === 'rawtext') rules.leak.protect(segment.raw);
      else if (segment.type === 'start') for (const part of segment.parts) if (part.attr !== undefined) rules.leak.protect(part.value);
    }
    switch (segment.type) {
      case 'text': {
        const result = rules.htmlText(segment.raw);
        events?.text(segment.raw, result);
        out += result;
        break;
      }
      case 'comment':
        events?.comment();
        out += `<!--${rules.htmlComment(segment.body)}-->`;
        break;
      case 'decl':
        events?.decl();
        out += /^<!doctype\s+html\s*>$/i.test(segment.raw) ? segment.raw : segment.raw.startsWith('<?') ? '<?x?>' : '<!x>';
        break;
      case 'end':
        events?.end(segment.name);
        out += `</${/^<\/([^\s/>]+)/.exec(segment.raw)?.[1] ?? segment.name}>`; // keep the original tag-name case
        break;
      case 'rawtext':
        events?.rawtext(segment.tag);
        out += rules.htmlRawText(segment.tag, segment.raw);
        break;
      default: {
        events?.start(segment.name);
        for (const part of segment.parts) {
          if (part.attr === undefined) { out += part.lit; continue; }
          const value = safeAttr(rules.htmlAttr(segment.name, part.attr, part.value), part.quote);
          events?.attr(segment.name, part.attr.toLowerCase(), value);
          out += `${part.lit}${part.quote}${value}${part.quote}`;
        }
      }
    }
    if (out.length >= OUT_FLUSH_CHARS) {
      yield out;
      out = '';
    }
  }
  if (out) yield out;
}

/** Strings of an HTML stream (text, attribute values, comments, raw text) for the output leak scan. */
export async function* htmlStrings(readable, budgets = DEFAULT_BUDGETS) {
  for await (const segment of scanHtml(readable, budgets)) {
    if (segment.raw.length > budgets.maxTokenChars) throw new HtmlError('E_HTML_TOKEN_TOO_LARGE');
    if (segment.type === 'text') yield segment.raw.trim();
    else if (segment.type === 'comment') yield segment.body;
    else if (segment.type === 'rawtext') yield segment.raw;
    else if (segment.type === 'start') for (const part of segment.parts) if (part.attr !== undefined) yield part.value;
  }
}

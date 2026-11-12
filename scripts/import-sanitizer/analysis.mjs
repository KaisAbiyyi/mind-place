// Structure inventory and the two integrity checks built on it:
//  1. structure equality: the sanitized output has exactly the structure of the input
//     (paths, value types, counts, number of distinct identifiers);
//  2. independent leak scan: the output is re-read from disk and every string is checked
//     against the registry of input strings.
//
// The inventory describes the *sanitized output only*, so it is safe to share by construction.
import { DEFAULT_BUDGETS } from './budgets.mjs';
import { DistinctCounter } from './leak-check.mjs';
import { CLOCK_NODE, isDataKey, isIdContainerKey, isIdKey, isIdValue, isKnownClassToken } from './rules.mjs';
import { jsonStrings, rewriteJson } from './json-stream.mjs';
import { htmlStrings, rewriteHtml } from './html-stream.mjs';
import { boundedLines } from './text-stream.mjs';

const MAX_PATHS = 20_000;
const MAX_NAMES = 5_000;

/** Rules that change nothing, used to walk an already-sanitized stream with the same path logic. */
export const IDENTITY_RULES = Object.freeze({
  jsonKey: key => key,
  jsonString: value => value,
  jsonNumber: raw => raw,
  isIdContainer: isIdContainerKey,
  isIdKey,
  isIdValue,
  isDataKeyName: isDataKey,
  htmlText: text => text,
  htmlAttr: (_tag, _name, value) => value,
  htmlRawText: (_tag, text) => text,
  htmlComment: text => text,
});

function bump(map, key, by = 1) {
  if (map.size >= MAX_NAMES && !map.has(key)) { map.overflow = true; return; }
  map.set(key, (map.get(key) ?? 0) + by);
}

export class JsonStructure {
  constructor(budgets = DEFAULT_BUDGETS) {
    this.paths = new Map();
    this.keys = new Map();
    this.ids = new DistinctCounter({ max: budgets.maxDistinctIdentifiers, budget: 'maxDistinctIdentifiers' });
    this.overflow = false;
  }

  #count(path, type) {
    let entry = this.paths.get(path);
    if (!entry) {
      if (this.paths.size >= MAX_PATHS) { this.overflow = true; return; }
      entry = {};
      this.paths.set(path, entry);
    }
    entry[type] = (entry[type] ?? 0) + 1;
  }

  events() {
    return {
      container: (path, type) => this.#count(path, type),
      key: (_path, segment, original) => {
        if (segment === '*') this.ids.add(original);
        else bump(this.keys, segment);
      },
      value: (path, type, isId, original) => {
        this.#count(path, type);
        if (isId && type === 'string' && original) this.ids.add(original);
      },
    };
  }

  summary() {
    const paths = Object.fromEntries([...this.paths].sort(([a], [b]) => (a < b ? -1 : 1)));
    return {
      paths,
      records: Object.values(this.paths.get('$[]') ?? {}).reduce((sum, n) => sum + n, 0),
      distinctIdentifiers: this.ids.size,
      fieldNames: Object.fromEntries([...this.keys].sort(([a], [b]) => (a < b ? -1 : 1))),
      truncated: this.overflow || this.keys.overflow === true,
    };
  }
}

export class HtmlStructure {
  constructor(labels = ['Prompted'], budgets = DEFAULT_BUDGETS) {
    this.labels = labels;
    this.tags = new Map();
    this.attributes = new Map();
    this.classes = new Map();
    this.unknownClassTokens = new DistinctCounter({ max: budgets.maxUnknownClassTokens, budget: 'maxUnknownClassTokens' }); // pseudonymized in the output, so only their number is comparable
    this.hosts = new Map();
    this.counts = { textNodes: 0, clockNodes: 0, labelNodes: 0, comments: 0, rawText: 0, declarations: 0, endTags: 0 };
  }

  events() {
    return {
      start: name => bump(this.tags, name),
      end: () => { this.counts.endTags++; },
      attr: (_tag, name, value) => {
        bump(this.attributes, name);
        if (name === 'class') {
          for (const token of value.split(/\s+/).filter(Boolean)) {
            if (isKnownClassToken(token)) bump(this.classes, token);
            else { bump(this.classes, '*'); this.unknownClassTokens.add(token); }
          }
        }
        if (name === 'href' || name === 'src') {
          const host = /^[a-z][a-z0-9+.-]*:\/\/([^/?#]*)/i.exec(value)?.[1];
          if (host) bump(this.hosts, host.toLowerCase());
        }
      },
      text: raw => {
        const body = raw.trim();
        if (!body) return;
        this.counts.textNodes++;
        if (CLOCK_NODE.test(raw)) this.counts.clockNodes++;
        if (this.labels.some(label => body === label || (body.startsWith(label) && /^\s/.test(body.slice(label.length))))) this.counts.labelNodes++;
      },
      comment: () => { this.counts.comments++; },
      rawtext: () => { this.counts.rawText++; },
      decl: () => { this.counts.declarations++; },
    };
  }

  summary() {
    const sorted = map => Object.fromEntries([...map].sort(([a], [b]) => (a < b ? -1 : 1)));
    return {
      tags: sorted(this.tags),
      attributes: sorted(this.attributes),
      classTokens: sorted(this.classes),
      distinctUnknownClassTokens: this.unknownClassTokens.size,
      linkHosts: sorted(this.hosts),
      ...this.counts,
      truncated: [this.tags, this.attributes, this.classes, this.hosts].some(map => map.overflow === true),
    };
  }
}

async function drain(generator) {
  // eslint-disable-next-line no-unused-vars
  for await (const _ of generator) { /* output discarded: only the events matter */ }
}

export async function analyzeJson(readable, { multiRoot = false, budgets = DEFAULT_BUDGETS } = {}) {
  const structure = new JsonStructure(budgets);
  await drain(rewriteJson(readable, IDENTITY_RULES, { events: structure.events(), multiRoot, budgets }));
  return structure;
}

export async function analyzeHtml(readable, labels, budgets = DEFAULT_BUDGETS) {
  const structure = new HtmlStructure(labels, budgets);
  await drain(rewriteHtml(readable, IDENTITY_RULES, { events: structure.events(), budgets }));
  return structure;
}

/** Number of entries a summary holds, charged against the cross-member summary budget. */
export function summaryEntries(summary) {
  if (!summary) return 0;
  return Object.values(summary).reduce((sum, value) => sum + (value && typeof value === 'object' ? Object.keys(value).length : 1), 0);
}

export function structuresEqual(a, b) {
  return JSON.stringify(a) === JSON.stringify(b);
}

/** First few differing paths between two summaries, for a content-free failure message. */
export function structureDifferences(a, b, limit = 5) {
  const out = [];
  const walk = (x, y, at) => {
    if (out.length >= limit) return;
    if (x && y && typeof x === 'object' && typeof y === 'object') {
      for (const key of new Set([...Object.keys(x), ...Object.keys(y)])) walk(x[key], y[key], `${at}/${key}`);
    } else if (x !== y) out.push(`${at}: input ${JSON.stringify(x)} vs output ${JSON.stringify(y)}`);
  };
  walk(a, b, '');
  return out;
}

/** Counts leaking strings in a sanitized output entry (kind: json | html | text | name). */
export async function scanForLeaks(kind, readable, registry, budgets = DEFAULT_BUDGETS) {
  let leaks = 0;
  if (kind === 'json') for await (const text of jsonStrings(readable, budgets)) leaks += registry.countLeaks(text);
  else if (kind === 'html') for await (const text of htmlStrings(readable, budgets)) leaks += registry.countLeaks(text);
  else for await (const { line } of boundedLines(readable, budgets)) leaks += registry.countLeaks(line);
  return leaks;
}

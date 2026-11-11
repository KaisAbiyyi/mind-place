// Orchestrator: input (zip | directory | single file) -> sanitized copy + inventory + checks.
//
// Pipeline per member: classify by name -> stream-rewrite into a spool file -> add to the output.
// After the output is complete it is re-read from disk and (1) its structure is compared with the
// structure recorded from the input and (2) every string in it is checked against the registry of
// input strings. Only a run that passes both publishes its output; otherwise nothing is left behind.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { pipeline } from 'node:stream/promises';
import { Readable } from 'node:stream';
import { StringDecoder } from 'node:string_decoder';
import {
  analyzeHtml, analyzeJson, HtmlStructure, JsonStructure, scanForLeaks, structureDifferences, structuresEqual,
} from './analysis.mjs';
import { LeakError, LeakRegistry } from './leak-check.mjs';
import { rewriteHtml } from './html-stream.mjs';
import { rewriteJson } from './json-stream.mjs';
import { HTML_MEMBER, JSON_MEMBER, neutralizeText, Rules, TEXT_MEMBER } from './rules.mjs';
import { openZip, ZipError, ZipWriter } from './zip.mjs';

export const TOOL = 'mind-place-import-sanitizer';
export const TOOL_VERSION = 1;
export const MAX_DIRECTORY_ENTRIES = 1_000_000;
const READ_CHUNK = 1 << 20;

export class SanitizerError extends Error {
  constructor(code, message = code, exitCode = 1) {
    super(message);
    this.code = code;
    this.exitCode = exitCode;
  }
}

// Field names of the ChatGPT export documented by its own conversations.json; anything else is
// reported as "unknown" so the owner can review it before sharing.
const KNOWN_FIELD_NAMES = new Set(`id title create_time update_time mapping moderation_results current_node plugin_ids
  conversation_id conversation_template_id gizmo_id gizmo_type is_archived is_starred safe_urls blocked_urls
  default_model_slug conversation_origin voice async_status disabled_tool_ids is_do_not_remember memory_scope
  sugar_item_id message parent children author role name metadata content content_type parts text language
  status end_turn weight recipient channel model_slug finish_details type stop_tokens request_id message_type
  timestamp_ parent_id is_complete citations content_references search_result_groups attachments size mime_type
  asset_pointer width height size_bytes fovea metadata_ dalle gen_id prompt seed edit_op serialization_title
  parent_gen_id user_editable_context user_profile user_instructions about_user_message about_model_message
  result summary_title tether_id domain url pub_date reasoning_status`.split(/\s+/));

const kindOf = name => {
  if (JSON_MEMBER.test(name)) return 'json';
  if (HTML_MEMBER.test(name)) return 'html';
  if (TEXT_MEMBER.test(name)) return 'text';
  return 'media';
};

const sizeClass = bytes => (bytes <= 0 ? 0 : 2 ** Math.ceil(Math.log2(bytes)));

// ---------------------------------------------------------------- sources

async function walk(root) {
  const out = [];
  const skipped = { symlinks: 0 };
  async function visit(dir, depth) {
    if (depth > 64) throw new SanitizerError('E_DEPTH', 'Directory nesting is too deep');
    const names = (await fs.promises.readdir(dir)).sort();
    for (const name of names) {
      const file = path.join(dir, name);
      const stat = await fs.promises.lstat(file);
      if (stat.isSymbolicLink()) { skipped.symlinks++; continue; }
      if (stat.isDirectory()) await visit(file, depth + 1);
      else if (stat.isFile()) {
        out.push({ name: path.relative(root, file).split(path.sep).join('/'), size: stat.size, open: async () => fs.createReadStream(file, { highWaterMark: READ_CHUNK }) });
        if (out.length > MAX_DIRECTORY_ENTRIES) throw new SanitizerError('E_ENTRY_LIMIT', 'Too many files');
      }
    }
  }
  await visit(root, 0);
  return { entries: out, skipped };
}

export async function openSource(input) {
  const stat = await fs.promises.lstat(input).catch(() => null);
  if (!stat) throw new SanitizerError('E_INPUT_MISSING', 'Input path does not exist');
  if (stat.isSymbolicLink()) throw new SanitizerError('E_INPUT_SYMLINK', 'Input path must not be a symbolic link');
  if (stat.isDirectory()) {
    const { entries, skipped } = await walk(input);
    return { kind: 'dir', entries, skipped, close: async () => {} };
  }
  if (!stat.isFile()) throw new SanitizerError('E_INPUT_TYPE', 'Input must be a file or directory');
  const head = Buffer.alloc(4);
  const handle = await fs.promises.open(input, 'r');
  await handle.read(head, 0, 4, 0);
  await handle.close();
  if (head.readUInt32LE(0) === 0x04034b50 || head.readUInt32LE(0) === 0x06054b50) {
    const zip = await openZip(input);
    const entries = zip.entries.map(entry => ({
      name: entry.name,
      size: entry.uncompressedSize,
      isDirectory: entry.isDirectory,
      open: () => zip.openEntry(entry),
    }));
    return { kind: 'zip', entries, skipped: { symlinks: 0 }, close: () => zip.close() };
  }
  return {
    kind: 'file',
    entries: [{ name: path.basename(input), size: stat.size, open: async () => fs.createReadStream(input, { highWaterMark: READ_CHUNK }) }],
    skipped: { symlinks: 0 },
    close: async () => {},
  };
}

// ---------------------------------------------------------------- member processing

async function* neutralizeTextStream(readable, rules) {
  let carry = '';
  const decoder = new StringDecoder('utf8');
  for await (const chunk of readable) {
    const lines = (carry + decoder.write(chunk)).split('\n');
    carry = lines.pop();
    for (const line of lines) yield `${neutralizeText(line)}\n`;
    for (const line of lines) rules.leak.protect(line);
  }
  carry += decoder.end();
  if (carry) { rules.leak.protect(carry); yield neutralizeText(carry); }
}

const EMPTY_STAND_IN = { json: '{}\n', html: '<!doctype html><html></html>\n', text: '' };

async function sanitizeMember(entry, kind, outName, rules, spool, options) {
  const record = { kind, status: 'ok', structure: null };
  if (kind === 'media') {
    const bytes = rules.standIn(entry.name);
    await fs.promises.writeFile(spool, bytes, { mode: 0o600 });
    record.status = 'stand-in';
    return record;
  }
  try {
    const source = await entry.open();
    let generator;
    if (kind === 'json') {
      const structure = new JsonStructure();
      record.structure = structure;
      generator = rewriteJson(source, rules, { events: structure.events(), multiRoot: /\.(?:jsonl|ndjson)$/i.test(entry.name) });
    } else if (kind === 'html') {
      const structure = new HtmlStructure(rules.keepLabels);
      record.structure = structure;
      const hints = options.hints;
      const events = structure.events();
      generator = rewriteHtml(source, rules, {
        events: hints ? { ...events, text: (raw, result) => { events.text(raw, result); hints.add(raw); } } : events,
      });
    } else {
      generator = neutralizeTextStream(source, rules);
    }
    await pipeline(Readable.from(generator, { objectMode: false }), fs.createWriteStream(spool, { mode: 0o600 }));
    if (record.structure) {
      record.inputSummary = record.structure.summary();
      record.structure = null; // release the hash tables: only the summary is needed from here on
    }
  } catch (error) {
    // Content-free reason only: parser messages can quote the input.
    record.status = `failed:${error?.code ?? 'E_PROCESSING'}`;
    record.structure = null;
    record.inputSummary = null;
    await fs.promises.writeFile(spool, EMPTY_STAND_IN[kind] ?? '', { mode: 0o600 });
  }
  return record;
}

class LeadingWordHints {
  constructor() { this.counts = new Map(); }
  add(raw) {
    const word = /^\s*(\S{2,24})(?:\s|$)/.exec(raw)?.[1];
    if (word && (this.counts.size < 50_000 || this.counts.has(word))) this.counts.set(word, (this.counts.get(word) ?? 0) + 1);
  }
  top(minCount = 5, limit = 20) {
    return [...this.counts].filter(([, n]) => n >= minCount).sort((a, b) => b[1] - a[1]).slice(0, limit);
  }
}

// ---------------------------------------------------------------- sinks

class FileSink {
  constructor(output) { this.output = output; this.partial = `${output}.partial`; }
  async add(name, spool) { await fs.promises.copyFile(spool, this.partial, fs.constants.COPYFILE_EXCL); }
  async finish() { return { entries: 1 }; }
  async publish() { await fs.promises.rename(this.partial, this.output); }
  async discard() { await fs.promises.rm(this.partial, { force: true }); }
}

class DirSink {
  constructor(output) { this.output = output; this.partial = `${output}.partial`; }
  async init() { await fs.promises.mkdir(this.partial, { mode: 0o700 }); }
  async add(name, spool) {
    const target = path.join(this.partial, ...name.split('/'));
    if (!target.startsWith(this.partial + path.sep)) throw new SanitizerError('E_UNSAFE_NAME', 'Unsafe output name');
    await fs.promises.mkdir(path.dirname(target), { recursive: true });
    await fs.promises.copyFile(spool, target, fs.constants.COPYFILE_EXCL);
  }
  async finish() { return {}; }
  async publish() { await fs.promises.rename(this.partial, this.output); }
  async discard() { await fs.promises.rm(this.partial, { recursive: true, force: true }); }
}

class ZipSink {
  constructor(output) { this.output = output; this.partial = `${output}.partial`; this.writer = new ZipWriter(this.partial); }
  async add(name, spool) {
    if (name === null) return;
    await this.writer.addFile(name, fs.createReadStream(spool));
  }
  async addDirectory(name) { await this.writer.addDirectory(name); }
  async finish() { return this.writer.finish(); }
  async publish() { await fs.promises.rename(this.partial, this.output); }
  async discard() { this.writer.abort(); await fs.promises.rm(this.partial, { force: true }); }
}

// ---------------------------------------------------------------- run

function assertPaths(input, output) {
  const resolvedIn = path.resolve(input);
  const resolvedOut = path.resolve(output);
  if (resolvedIn === resolvedOut || resolvedOut.startsWith(resolvedIn + path.sep)) {
    throw new SanitizerError('E_OUTPUT_INSIDE_INPUT', 'Output must not be the input or inside it');
  }
}

export async function sanitize({
  input, output, provider = 'auto', salt, timestamps = 'day', keepLabels = [], keepKeys = [],
  dryRun = false, force = false, ownerHints = false,
}) {
  if (!input) throw new SanitizerError('E_USAGE', 'An input path is required');
  const workRoot = dryRun ? os.tmpdir() : path.dirname(path.resolve(output ?? '.'));
  if (!dryRun) {
    if (!output) throw new SanitizerError('E_USAGE', 'An output path is required');
    assertPaths(input, output);
    if (fs.existsSync(output) && !force) throw new SanitizerError('E_OUTPUT_EXISTS', 'Output already exists; pass --force to replace it');
    if (fs.existsSync(output) && force) await fs.promises.rm(output, { recursive: true, force: true });
    if (fs.existsSync(`${output}.partial`)) await fs.promises.rm(`${output}.partial`, { recursive: true, force: true });
  }
  const leak = new LeakRegistry();
  const rules = new Rules({ salt, timestamps, keepLabels, keepKeys, leak });
  const source = await openSource(input);
  const outputPath = dryRun ? path.join(await fs.promises.mkdtemp(path.join(workRoot, 'mp-sanitize-dry-')), 'out') : output;
  const sink = source.kind === 'zip' ? new ZipSink(outputPath) : source.kind === 'dir' ? new DirSink(outputPath) : new FileSink(outputPath);
  const spoolDir = await fs.promises.mkdtemp(path.join(dryRun ? workRoot : path.dirname(path.resolve(output)), '.mp-sanitize-spool-'));
  const hints = ownerHints ? new LeadingWordHints() : null;
  const members = [];
  const used = new Set();
  let ok = false;
  try {
    if (sink.init) await sink.init();
    let index = 0;
    for (const entry of source.entries) {
      const outName = source.kind === 'file' ? path.basename(outputPath) : rules.memberName(entry.name);
      if (entry.isDirectory) {
        if (!used.has(outName)) { used.add(outName); await sink.addDirectory?.(outName); }
        continue;
      }
      if (used.has(outName)) {
        members.push({ name: outName, kind: kindOf(entry.name), status: 'failed:E_NAME_COLLISION', outputBytes: 0, sourceSizeClass: sizeClass(entry.size) });
        continue;
      }
      used.add(outName);
      const kind = kindOf(entry.name);
      const spool = path.join(spoolDir, String(index++));
      const record = await sanitizeMember(entry, kind, outName, rules, spool, { hints });
      const outputBytes = (await fs.promises.stat(spool)).size;
      await sink.add(outName, spool);
      await fs.promises.rm(spool, { force: true });
      members.push({ name: outName, ...record, outputBytes, sourceSizeClass: sizeClass(entry.size) });
      delete members[members.length - 1].structure;
    }
    const written = await sink.finish();

    // ---- verification pass over the bytes that will actually be shared
    const verify = await openSource(sink.partial);
    const checks = { structure: 'PASS', leak: 'PASS', structureDifferences: [], leakCount: 0 };
    const byName = new Map(members.map(member => [member.name, member]));
    for (const entry of verify.entries) {
      const outName = verify.kind === 'file' ? path.basename(outputPath) : entry.name.replace(/\/$/, '');
      const member = byName.get(outName);
      if (!member) continue;
      checks.leakCount += leak.countLeaks(outName.split('/').join(' '));
      for (const segment of outName.split('/')) checks.leakCount += leak.countLeaks(segment);
      if (member.kind === 'json' && member.inputSummary) {
        const after = await analyzeJson(await entry.open(), { multiRoot: /\.(?:jsonl|ndjson)$/i.test(outName) });
        const before = member.inputSummary;
        const afterSummary = after.summary();
        if (!structuresEqual(before, afterSummary)) {
          checks.structure = 'FAIL';
          checks.structureDifferences.push({ member: outName, differences: structureDifferences(before, afterSummary) });
        }
        member.output = afterSummary;
      } else if (member.kind === 'html' && member.inputSummary) {
        const after = await analyzeHtml(await entry.open(), rules.keepLabels);
        const before = member.inputSummary;
        const afterSummary = after.summary();
        if (!structuresEqual(before, afterSummary)) {
          checks.structure = 'FAIL';
          checks.structureDifferences.push({ member: outName, differences: structureDifferences(before, afterSummary) });
        }
        member.output = afterSummary;
      }
      if (member.kind !== 'media') checks.leakCount += await scanForLeaks(member.kind, await entry.open(), leak);
    }
    await verify.close();
    if (checks.leakCount > 0) {
      checks.leak = 'FAIL';
      throw Object.assign(new LeakError(checks.leakCount), { exitCode: 3, checks });
    }
    if (checks.structure === 'FAIL') {
      throw Object.assign(new SanitizerError('E_STRUCTURE', 'Sanitized output does not have the structure of the input', 4), { checks });
    }
    if (!dryRun) await sink.publish();
    ok = true;

    const failed = members.filter(member => member.status.startsWith('failed'));
    const report = buildReport({ rules, leak, members, checks, source, provider, written, dryRun, timestamps, salt });
    return {
      exitCode: failed.length ? 2 : 0,
      report,
      hints: hints?.top() ?? null,
      outputPath: dryRun ? null : output,
    };
  } finally {
    await source.close();
    await fs.promises.rm(spoolDir, { recursive: true, force: true });
    if (!ok || dryRun) {
      await sink.discard();
      if (dryRun) await fs.promises.rm(path.dirname(outputPath), { recursive: true, force: true });
    }
  }
}

function buildReport({ rules, leak, members, checks, source, provider, written, dryRun, timestamps, salt }) {
  const unknown = {};
  const out = members.map(member => {
    const summary = member.output ?? null;
    if (member.kind === 'json' && summary) {
      const names = Object.keys(summary.fieldNames).filter(name => !KNOWN_FIELD_NAMES.has(name) && !KNOWN_FIELD_NAMES.has(`${name}_`));
      if (names.length) unknown[member.name] = names;
    }
    return {
      name: member.name,
      kind: member.kind,
      status: member.status,
      outputBytes: member.outputBytes,
      sourceSizeClassBytes: member.sourceSizeClass,
      structure: summary,
    };
  });
  const detected = provider !== 'auto' ? provider
    : members.some(m => /conversations(-\d+)?\.json$/.test(m.name)) ? 'chatgpt'
      : members.some(m => m.kind === 'html') ? 'gemini' : 'unknown';
  return {
    tool: TOOL,
    reportVersion: TOOL_VERSION,
    shareable: true,
    note: 'Describes the sanitized output only. It contains no input text; review it with the sanitized files before sharing.',
    provider: detected,
    inputKind: source.kind,
    outputKind: source.kind,
    skippedSymbolicLinks: source.skipped.symlinks,
    options: { timestamps, salt: salt ? 'custom' : 'default', keepLabels: rules.keepLabels.length - 1 },
    totals: {
      members: members.length,
      jsonMembers: members.filter(m => m.kind === 'json').length,
      htmlMembers: members.filter(m => m.kind === 'html').length,
      mediaStandIns: members.filter(m => m.status === 'stand-in').length,
      failedMembers: members.filter(m => m.status.startsWith('failed')).length,
      outputBytes: members.reduce((sum, m) => sum + m.outputBytes, 0),
      zip64: Boolean(written.zip64),
    },
    counters: rules.counters,
    keptValues: rules.keptEnumReport(),
    checks: {
      structure: checks.structure,
      leak: checks.leak,
      leakRegistry: leak.summary(),
      outputKept: dryRun ? 'dry-run: nothing written' : 'published',
    },
    unknownFields: unknown,
    members: out,
  };
}

export { ZipError };

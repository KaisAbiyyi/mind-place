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
import {
  analyzeHtml, analyzeJson, HtmlStructure, JsonStructure, scanForLeaks, structureDifferences, structuresEqual, summaryEntries,
} from './analysis.mjs';
import { BudgetError, structureEstimateBytes, resolveBudgets, totalStructureEstimateBytes } from './budgets.mjs';
import { LeakError, LeakRegistry } from './leak-check.mjs';
import { boundedLines } from './text-stream.mjs';
import { rewriteHtml } from './html-stream.mjs';
import { rewriteJson } from './json-stream.mjs';
import { HTML_MEMBER, JSON_MEMBER, neutralizeText, Rules, TEXT_MEMBER } from './rules.mjs';
import { openZip, READER_LIMITS, ZipError, ZipWriter } from './zip.mjs';

export const TOOL = 'mind-place-import-sanitizer';
export const TOOL_VERSION = 1;
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

async function walk(root, maxMembers) {
  const out = [];
  const skipped = { symlinks: 0 };
  let enumerated = 0; // every name seen (files, directories, links) counts against the member budget
  async function visit(dir, depth) {
    if (depth > 64) throw new SanitizerError('E_DEPTH', 'Directory nesting is too deep');
    // Enumerate incrementally with a hard cap BEFORE sorting: a directory with millions of entries
    // is refused after maxMembers names, never materialized (readdir would build the whole array).
    const names = [];
    const handle = await fs.promises.opendir(dir);
    try {
      for await (const dirent of handle) {
        if (++enumerated > maxMembers) throw new BudgetError('maxMembers', maxMembers);
        names.push(dirent.name);
      }
    } finally {
      await handle.close().catch(() => {});
    }
    names.sort();
    for (const name of names) {
      const file = path.join(dir, name);
      const stat = await fs.promises.lstat(file);
      if (stat.isSymbolicLink()) { skipped.symlinks++; continue; }
      if (stat.isDirectory()) await visit(file, depth + 1);
      else if (stat.isFile()) {
        out.push({ name: path.relative(root, file).split(path.sep).join('/'), size: stat.size, open: async () => fs.createReadStream(file, { highWaterMark: READ_CHUNK }) });
        if (out.length > maxMembers) throw new BudgetError('maxMembers', maxMembers);
      }
    }
  }
  await visit(root, 0);
  return { entries: out, skipped };
}

export async function openSource(input, budgets = resolveBudgets()) {
  const stat = await fs.promises.lstat(input).catch(() => null);
  if (!stat) throw new SanitizerError('E_INPUT_MISSING', 'Input path does not exist');
  if (stat.isSymbolicLink()) throw new SanitizerError('E_INPUT_SYMLINK', 'Input path must not be a symbolic link');
  if (stat.isDirectory()) {
    const { entries, skipped } = await walk(input, budgets.maxMembers);
    return { kind: 'dir', entries, skipped, close: async () => {} };
  }
  if (!stat.isFile()) throw new SanitizerError('E_INPUT_TYPE', 'Input must be a file or directory');
  const head = Buffer.alloc(4);
  const handle = await fs.promises.open(input, 'r');
  await handle.read(head, 0, 4, 0);
  await handle.close();
  if (head.readUInt32LE(0) === 0x04034b50 || head.readUInt32LE(0) === 0x06054b50) {
    const zip = await openZip(input, { ...READER_LIMITS, maxEntries: budgets.maxMembers, maxCentralDirectoryBytes: Math.min(READER_LIMITS.maxCentralDirectoryBytes, budgets.maxMembers * 1024) })
      .catch(error => { throw error?.code === 'E_ZIP_LIMIT' ? new BudgetError('maxMembers', budgets.maxMembers) : error; });
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

async function* neutralizeTextStream(readable, rules, budgets) {
  for await (const { line, newline } of boundedLines(readable, budgets)) {
    rules.leak.protect(line);
    yield `${neutralizeText(line)}${newline ? '\n' : ''}`;
  }
}

const EMPTY_STAND_IN = { json: '{}\n', html: '<!doctype html><html></html>\n', text: '' };

async function sanitizeMember(entry, kind, outName, rules, spool, options) {
  const { budgets } = options;
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
      const structure = new JsonStructure(budgets);
      record.structure = structure;
      generator = rewriteJson(source, rules, { events: structure.events(), multiRoot: /\.(?:jsonl|ndjson)$/i.test(entry.name), budgets });
    } else if (kind === 'html') {
      const structure = new HtmlStructure(rules.keepLabels, budgets);
      record.structure = structure;
      const hints = options.hints;
      const events = structure.events();
      generator = rewriteHtml(source, rules, {
        events: hints ? { ...events, text: (raw, result) => { events.text(raw, result); hints.add(raw); } } : events,
        budgets,
      });
    } else {
      generator = neutralizeTextStream(source, rules, budgets);
    }
    await pipeline(Readable.from(generator, { objectMode: false }), fs.createWriteStream(spool, { mode: 0o600 }));
    if (record.structure) {
      record.inputSummary = record.structure.summary();
      record.structure = null; // release the hash tables: only the summary is needed from here on
    }
  } catch (error) {
    if (error instanceof BudgetError) throw error; // a resource limit stops the whole run, never a quiet stand-in
    const exceeded = { E_JSON_TOO_DEEP: 'maxJsonDepth', E_JSON_TOKEN_TOO_LARGE: 'maxTokenChars', E_HTML_TOKEN_TOO_LARGE: 'maxTokenChars' }[error?.code];
    if (exceeded) throw new BudgetError(exceeded, budgets[exceeded]);
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
  async publish(force) { return publishAtomically(this.partial, this.output, force); }
  async discard() { await fs.promises.rm(this.partial, { force: true }); }
}

class DirSink {
  constructor(output) { this.output = output; this.partial = `${output}.partial`; }
  async init() {
    await fs.promises.mkdir(this.partial, { mode: 0o700 });
    // Lets a later --force recognize this directory as sanitizer output it may replace.
    await fs.promises.writeFile(path.join(this.partial, OUTPUT_MARKER), 'mind-place-import-sanitizer output\n', { mode: 0o600 });
  }
  async add(name, spool) {
    const target = path.join(this.partial, ...name.split('/'));
    if (!target.startsWith(this.partial + path.sep)) throw new SanitizerError('E_UNSAFE_NAME', 'Unsafe output name');
    await fs.promises.mkdir(path.dirname(target), { recursive: true });
    await fs.promises.copyFile(spool, target, fs.constants.COPYFILE_EXCL);
  }
  async finish() { return {}; }
  async publish(force) { return publishAtomically(this.partial, this.output, force); }
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
  async publish(force) { return publishAtomically(this.partial, this.output, force); }
  async discard() { this.writer.abort(); await fs.promises.rm(this.partial, { force: true }); }
}

// ---------------------------------------------------------------- run

const coverageError = () => new SanitizerError('E_LEAK_COVERAGE', 'Leak tracking overflowed; verification is incomplete and the output was not published', 3);
const OUTPUT_MARKER = '.mp-sanitizer-output';
// Kept structural names (conversations.json, user.json, ...) are never renamed: a repeat is a real duplicate.
const KEPT_STRUCTURAL = /(?:^|\/)(?:conversations(?:-\d{1,6})?\.json|(?:user|message_feedback|shared_conversations|export_manifest|model_comparisons|library|projects|gizmos)\.jsonl?|chat\.html|MyActivity\.html|archive_browser\.html|index\.html)$/
const fold = value => (process.platform === 'win32' || process.platform === 'darwin' ? value.toLowerCase() : value);

/** Canonical path of something that may not exist yet: real path of the nearest existing ancestor + the rest. */
async function resolveReal(target) {
  const absolute = path.resolve(target);
  try {
    return await fs.promises.realpath(absolute);
  } catch (error) {
    if (error.code !== 'ENOENT' && error.code !== 'ENOTDIR') throw error;
    const parent = path.dirname(absolute);
    if (parent === absolute) throw error;
    return path.join(await resolveReal(parent), path.basename(absolute));
  }
}

async function assertDisjoint(first, second, message, code) {
  const realFirst = fold(await resolveReal(first));
  const realSecond = fold(await resolveReal(second));
  const within = (child, parent) => child === parent || child.startsWith(parent.endsWith(path.sep) ? parent : parent + path.sep);
  if (within(realFirst, realSecond) || within(realSecond, realFirst)) throw new SanitizerError(code, message);
}

/**
 * Refuses a write target that is the input, inside it, or an ANCESTOR of it, after resolving
 * symbolic links (and, on Windows, junctions) in every existing component.
 */
export async function assertOutputDoesNotOverlap(input, target, what = 'Output') {
  await assertDisjoint(input, target, `${what} must not be the input, inside it, or a directory that contains it`, 'E_OUTPUT_OVERLAPS_INPUT');
}

/**
 * The report is a separate artifact: it must be disjoint from the input AND from the sanitized
 * output (equal, inside it, or containing it, on canonical paths), and must not be a staging or
 * aside name the tool creates next to the output. Otherwise it could replace or add to the files
 * whose verification the printed PASS describes.
 */
export async function assertReportPath(input, output, report) {
  await assertDisjoint(input, report, 'The report path must not be the input, inside it, or a directory that contains it', 'E_REPORT_OVERLAPS_INPUT');
  if (!output) return;
  await assertDisjoint(output, report, 'The report path must not be the output, inside it, or a directory that contains it', 'E_REPORT_OVERLAPS_OUTPUT');
  const realReport = fold(await resolveReal(report));
  const realOutput = fold(await resolveReal(output));
  if (realReport === `${realOutput}.partial` || realReport.startsWith(`${realOutput}.replaced-`)) {
    throw new SanitizerError('E_REPORT_OVERLAPS_OUTPUT', 'The report path must not be a staging name used for the output');
  }
}

async function assertReplaceable(output, stat) {
  if (stat.isSymbolicLink()) throw new SanitizerError('E_OUTPUT_SYMLINK', 'Refusing to replace a symbolic link');
  if (stat.isDirectory()) {
    const marked = await fs.promises.lstat(path.join(output, OUTPUT_MARKER)).then(m => m.isFile(), () => false);
    if (!marked) throw new SanitizerError('E_OUTPUT_NOT_REPLACEABLE', 'Refusing to replace a directory that was not created by this tool');
  } else if (!stat.isFile()) {
    throw new SanitizerError('E_OUTPUT_NOT_REPLACEABLE', 'Refusing to replace something that is not a regular file');
  }
}

/**
 * Publishes a fully written and verified output. An existing output is moved aside first and
 * removed only after the replacement is in place, so a failure never destroys it.
 */
async function publishAtomically(partial, output, force) {
  const existing = await fs.promises.lstat(output).catch(() => null);
  if (!existing) {
    await fs.promises.rename(partial, output);
    return {
      commit: async () => {},
      undo: async () => { await fs.promises.rename(output, partial); }, // the caller then discards the partial
    };
  }
  if (!force) throw new SanitizerError('E_OUTPUT_EXISTS', 'Output already exists; pass --force to replace it');
  await assertReplaceable(output, existing);
  const aside = `${output}.replaced-${process.pid}-${Date.now()}`;
  await fs.promises.rename(output, aside);
  try {
    await fs.promises.rename(partial, output);
  } catch (error) {
    await fs.promises.rename(aside, output);
    throw error;
  }
  // The previous output is kept until the caller commits, so a later failure can restore it.
  return {
    commit: async () => { await fs.promises.rm(aside, { recursive: true, force: true }).catch(() => {}); },
    undo: async () => {
      await fs.promises.rename(output, partial);
      await fs.promises.rename(aside, output);
    },
  };
}

/**
 * Stages the report next to its final path (exclusive create) BEFORE the output is published, so a
 * report that cannot be written stops the run while nothing has been published yet.
 */
async function stageReport(reportPath, json, force) {
  const existing = await fs.promises.lstat(reportPath).catch(() => null);
  if (existing && !force) throw new SanitizerError('E_REPORT_EXISTS', 'The report already exists; pass --force to replace it');
  if (existing && !existing.isFile()) throw new SanitizerError('E_REPORT_NOT_REPLACEABLE', 'Refusing to replace a report path that is not a regular file');
  const staged = `${reportPath}.partial-${process.pid}`;
  try {
    await fs.promises.writeFile(staged, json, { flag: 'wx', mode: 0o600 });
  } catch (error) {
    throw new SanitizerError('E_REPORT_WRITE', `The report could not be staged (${error?.code ?? 'error'}); nothing was published`);
  }
  let done = false;
  return {
    async commit() {
      if (force) await fs.promises.rename(staged, reportPath);
      else {
        // no-clobber: link fails if something appeared in the meantime
        await fs.promises.link(staged, reportPath);
        await fs.promises.rm(staged, { force: true });
      }
      done = true;
    },
    async discard() { if (!done) await fs.promises.rm(staged, { force: true }).catch(() => {}); },
  };
}

export async function sanitize({
  input, output, provider = 'auto', salt, timestamps = 'day', keepLabels = [], keepEnums = [], keepClasses = [],
  dryRun = false, force = false, ownerHints = false, budgets: budgetOverrides, report: reportPath,
}) {
  const budgets = resolveBudgets(budgetOverrides);
  if (!input) throw new SanitizerError('E_USAGE', 'An input path is required');
  const workRoot = dryRun ? os.tmpdir() : path.dirname(path.resolve(output ?? '.'));
  if (!dryRun) {
    if (!output) throw new SanitizerError('E_USAGE', 'An output path is required');
    // All checks happen before anything is created, moved or deleted.
    await assertOutputDoesNotOverlap(input, output);
    if (reportPath) await assertReportPath(input, output, reportPath);
    const existing = await fs.promises.lstat(output).catch(() => null);
    if (existing && !force) throw new SanitizerError('E_OUTPUT_EXISTS', 'Output already exists; pass --force to replace it');
    if (existing) await assertReplaceable(output, existing);
    if (await fs.promises.lstat(`${output}.partial`).catch(() => null)) {
      throw new SanitizerError('E_OUTPUT_EXISTS', 'A leftover .partial output exists; remove it first');
    }
  }
  if (dryRun && reportPath) await assertReportPath(input, null, reportPath);
  const leak = new LeakRegistry({ maxHashes: budgets.maxProtectedPieces, maxKept: budgets.maxKeptPieces });
  const rules = new Rules({ salt, timestamps, keepLabels, keepEnums, keepClasses, budgets, leak });
  const source = await openSource(input, budgets);
  const outputPath = dryRun ? path.join(await fs.promises.mkdtemp(path.join(workRoot, 'mp-sanitize-dry-')), 'out') : output;
  const sink = source.kind === 'zip' ? new ZipSink(outputPath) : source.kind === 'dir' ? new DirSink(outputPath) : new FileSink(outputPath);
  const spoolDir = await fs.promises.mkdtemp(path.join(dryRun ? workRoot : path.dirname(path.resolve(output)), '.mp-sanitize-spool-'));
  const hints = ownerHints ? new LeadingWordHints() : null;
  const members = [];
  const used = new Set();
  let summaryTotal = 0;
  let stagedReport = null;
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
      let finalName = outName;
      // Different input names can map to the same shaped name (short names collide often). Re-key the
      // mapping deterministically until the name is free; a name that cannot change (a kept structural
      // name appearing twice) is a genuine duplicate.
      for (let attempt = 1; used.has(finalName) && source.kind !== 'file' && attempt <= 64; attempt++) {
        const candidate = rules.memberName(entry.name, attempt);
        if (candidate === finalName) break;
        finalName = candidate;
      }
      // Guaranteed uniqueness even when the shaped name space is nearly full: a deterministic counter.
      if (used.has(finalName) && source.kind !== 'file') {
        const dot = finalName.lastIndexOf('.');
        const [stem, ext] = dot > finalName.lastIndexOf('/') + 1 ? [finalName.slice(0, dot), finalName.slice(dot)] : [finalName, ''];
        for (let n = 1; used.has(finalName) && n <= 1_000_000 && !KEPT_STRUCTURAL.test(entry.name); n++) finalName = `${stem}_${n}${ext}`;
      }
      if (used.has(finalName)) {
        members.push({ name: finalName, kind: kindOf(entry.name), status: 'failed:E_NAME_COLLISION', outputBytes: 0, sourceSizeClass: sizeClass(entry.size) });
        continue;
      }
      used.add(finalName);
      const kind = kindOf(entry.name);
      const spool = path.join(spoolDir, String(index++));
      if (members.length >= budgets.maxMembers) throw new BudgetError('maxMembers', budgets.maxMembers);
      const record = await sanitizeMember(entry, kind, finalName, rules, spool, { hints, budgets });
      summaryTotal += summaryEntries(record.inputSummary);
      if (summaryTotal > budgets.maxSummaryEntries) throw new BudgetError('maxSummaryEntries', budgets.maxSummaryEntries);
      if (leak.partial) throw Object.assign(coverageError(), { checks: { structure: 'NOT_RUN', leak: 'INCOMPLETE' } }); // stop early: part of the input can no longer be checked
      const outputBytes = (await fs.promises.stat(spool)).size;
      await sink.add(finalName, spool);
      await fs.promises.rm(spool, { force: true });
      members.push({ name: finalName, ...record, outputBytes, sourceSizeClass: sizeClass(entry.size) });
      delete members[members.length - 1].structure;
    }
    const written = await sink.finish();

    // ---- verification pass over the bytes that will actually be shared
    const verify = await openSource(sink.partial, budgets);
    const checks = { structure: 'PASS', leak: 'PASS', structureDifferences: [], leakCount: 0 };
    const byName = new Map(members.map(member => [member.name, member]));
    for (const entry of verify.entries) {
      const outName = verify.kind === 'file' ? path.basename(outputPath) : entry.name.replace(/\/$/, '');
      const member = byName.get(outName);
      if (!member) continue;
      checks.leakCount += leak.countLeaks(outName.split('/').join(' '));
      for (const segment of outName.split('/')) checks.leakCount += leak.countLeaks(segment);
      if (member.kind === 'json' && member.inputSummary) {
        const after = await analyzeJson(await entry.open(), { multiRoot: /\.(?:jsonl|ndjson)$/i.test(outName), budgets });
        const before = member.inputSummary;
        const afterSummary = after.summary();
        if (!structuresEqual(before, afterSummary)) {
          checks.structure = 'FAIL';
          checks.structureDifferences.push({ member: outName, differences: structureDifferences(before, afterSummary) });
        }
        member.output = afterSummary;
      } else if (member.kind === 'html' && member.inputSummary) {
        const after = await analyzeHtml(await entry.open(), rules.keepLabels, budgets);
        const before = member.inputSummary;
        const afterSummary = after.summary();
        if (!structuresEqual(before, afterSummary)) {
          checks.structure = 'FAIL';
          checks.structureDifferences.push({ member: outName, differences: structureDifferences(before, afterSummary) });
        }
        member.output = afterSummary;
      }
      if (member.kind !== 'media') checks.leakCount += await scanForLeaks(member.kind, await entry.open(), leak, budgets);
    }
    await verify.close();
    if (checks.leakCount > 0) {
      checks.leak = 'FAIL';
      throw Object.assign(new LeakError(checks.leakCount), { exitCode: 3, checks });
    }
    if (leak.partial) {
      // Tracking overflowed: part of the input was never checked, so this is not a pass.
      checks.leak = 'INCOMPLETE';
      throw Object.assign(coverageError(), { checks });
    }
    if (checks.structure === 'FAIL') {
      throw Object.assign(new SanitizerError('E_STRUCTURE', 'Sanitized output does not have the structure of the input', 4), { checks });
    }
    const failed = members.filter(member => member.status.startsWith('failed'));
    const report = buildReport({ rules, leak, members, checks, source, provider, written, dryRun, timestamps, budgets });
    // Transaction: stage the report, publish the verified output, commit the report. If the report
    // cannot be committed the output publication is rolled back (a previous output is restored), so a
    // report failure never leaves a published output the caller was told had failed.
    stagedReport = reportPath ? await stageReport(reportPath, `${JSON.stringify(report, null, 2)}\n`, force) : null;
    const publication = dryRun ? null : await sink.publish(force);
    try {
      await stagedReport?.commit();
    } catch (error) {
      await publication?.undo().catch(() => {});
      throw new SanitizerError('E_REPORT_WRITE', `The report could not be written (${error?.code ?? 'error'}); the output was not published and any previous output was restored`);
    }
    await publication?.commit();
    ok = true;

    return {
      exitCode: failed.length ? 2 : 0,
      report,
      hints: hints?.top() ?? null,
      outputPath: dryRun ? null : output,
    };
  } finally {
    await stagedReport?.discard();
    await source.close();
    await fs.promises.rm(spoolDir, { recursive: true, force: true });
    if (!ok || dryRun) {
      await sink.discard();
      if (dryRun) await fs.promises.rm(path.dirname(outputPath), { recursive: true, force: true });
    }
  }
}

function buildReport({ rules, leak, members, checks, source, provider, written, dryRun, timestamps, budgets }) {
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
    options: { timestamps, pseudonymKey: rules.saltSource, keepLabels: rules.keepLabels.length - 1, keepEnums: [...rules.extraEnums].reduce((n, [, values]) => n + values.size, 0) },
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
    resourceBudgets: { limits: budgets, enumeratedStructuresEstimateBytes: totalStructureEstimateBytes(budgets), note: 'Input-dependent structures are capped; exceeding any cap stops the run (exit 5) without publishing. The byte figure is an estimate for those enumerated structures, not a bound on process memory.' },
    // Structure fidelity is not a privacy qualification. PASS below means "nothing outside the
    // allow-lists survived"; whatever the allow-lists keep is listed here for the owner to judge.
    qualification: {
      structureFidelity: checks.structure,
      leakCheck: checks.leak,
      contentFreeAssertion: 'NOT_ASSERTED',
      meaning: 'PASS means no input string of 8+ characters outside the allow-lists survived and the structure is unchanged. It is not a guarantee of anonymity: the residual metadata below stays in the output.',
    },
    residualMetadata: rules.residualReport(),
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

export { BudgetError, structureEstimateBytes, ZipError };

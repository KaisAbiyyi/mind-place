#!/usr/bin/env node
// Command line entry point. Offline: this module tree imports no network API.
// Output is limited to counts, sanitized member names and check results; no input text is printed.
import fs from 'node:fs';
import { parseArgs } from 'node:util';
import { sanitize } from './sanitize.mjs';

const HELP = `Mind Place import sanitizer: structure-preserving, content-free copy of a real export.

Usage:
  node scripts/import-sanitizer/cli.mjs --input <export> --output <sanitized> [options]
  node scripts/import-sanitizer/cli.mjs --input <export> --dry-run [options]

  --input <path>        ChatGPT export .zip, a directory, or a conversations*.json file;
                        Gemini Takeout .html file, directory or .zip
  --output <path>       Sanitized copy (zip -> zip, directory -> directory, file -> file). Must not exist.
  --dry-run             Run every check and print the inventory; keep no output
  --provider <name>     chatgpt | gemini | auto (default auto; only labels the report)
  --timestamps <mode>   day (default: keep the date, drop time of day) | keep
  --keep-label <text>   Also keep this structural HTML label (repeatable), e.g. a localized "Prompted"
  --keep-class <name>   Also keep this reviewed HTML class token (repeatable); unknown class names are replaced by default
  --keep-enum <k=v>     Also keep this enum value verbatim, e.g. status=queued (repeatable; review the report first)
  --salt-file <path>    File holding the pseudonym key. Default: a fresh random key per run (never stored or printed)
  --report <path>       Write the shareable JSON report here. Must be outside the input AND the output; it is staged
                        before the output is published and the whole run is rolled back if it cannot be written
  --owner-hints         Print frequent leading words of HTML text nodes to the terminal ONLY (never to the report)
  --force               Replace an existing output, only after the new one is fully written and verified; refuses
                        directories not created by this tool, symlinks, and anything that contains or is the input
  --help

Environment: MIND_PLACE_SANITIZER_SALT supplies the pseudonym key instead of --salt-file.
Exit codes: 0 PASS | 1 usage/IO error | 2 PARTIAL (some members replaced by stand-ins) | 3 leak check FAILED or INCOMPLETE | 4 structure check FAILED | 5 resource budget exceeded

PASS is structure fidelity plus "nothing outside the allow-lists survived". It is NOT an anonymity guarantee: read the
residual metadata the tool prints before sharing anything.
`;

function fail(message, code = 1) {
  process.stderr.write(`${message}\n`);
  process.exit(code);
}

let args;
try {
  args = parseArgs({
    options: {
      input: { type: 'string' }, output: { type: 'string' }, 'dry-run': { type: 'boolean' },
      provider: { type: 'string' }, timestamps: { type: 'string' }, 'keep-label': { type: 'string', multiple: true },
      'keep-enum': { type: 'string', multiple: true }, 'keep-class': { type: 'string', multiple: true }, 'salt-file': { type: 'string' }, report: { type: 'string' },
      'owner-hints': { type: 'boolean' }, force: { type: 'boolean' }, help: { type: 'boolean' },
    },
    allowPositionals: false,
  }).values;
} catch (error) {
  fail(`${error.message}\n\n${HELP}`);
}
if (args.help) { process.stdout.write(HELP); process.exit(0); }
if (!args.input) fail(`--input is required\n\n${HELP}`);
if (!args.output && !args['dry-run']) fail('--output (or --dry-run) is required');
if (args.provider && !['chatgpt', 'gemini', 'auto'].includes(args.provider)) fail('--provider must be chatgpt, gemini or auto');

let salt = process.env.MIND_PLACE_SANITIZER_SALT || undefined;
if (args['salt-file']) {
  try { salt = fs.readFileSync(args['salt-file'], 'utf8').trim() || undefined; } catch { fail('Cannot read the salt file'); }
}

const bar = text => `${text}${' '.repeat(Math.max(0, 30 - text.length))}`;
const units = bytes => (bytes >= 2 ** 20 ? `${(bytes / 2 ** 20).toFixed(1)} MiB` : bytes >= 1024 ? `${(bytes / 1024).toFixed(1)} KiB` : `${bytes} B`);

try {
  const result = await sanitize({
    input: args.input,
    output: args.output,
    provider: args.provider ?? 'auto',
    salt,
    timestamps: args.timestamps ?? 'day',
    keepLabels: args['keep-label'] ?? [],
    keepEnums: args['keep-enum'] ?? [],
    keepClasses: args['keep-class'] ?? [],
    dryRun: Boolean(args['dry-run']),
    force: Boolean(args.force),
    ownerHints: Boolean(args['owner-hints']),
    report: args.report,
  });
  const { report } = result;

  const out = [];
  out.push(`Mind Place import sanitizer: ${report.provider} ${report.inputKind}`);
  out.push(`${bar('MEMBER')} ${bar('KIND')} ${bar('OUTPUT')} STATUS`);
  for (const member of report.members.slice(0, 200)) {
    const records = member.structure?.paths?.$?.array ? member.structure.records : null;
    out.push(`${bar(member.name.slice(0, 30))} ${bar(member.kind + (records === null ? '' : ` (${records} records)`))} ${bar(units(member.outputBytes))} ${member.status}`);
  }
  if (report.members.length > 200) out.push(`... ${report.members.length - 200} more members (see the report)`);
  out.push('');
  out.push(`Members ${report.totals.members} (json ${report.totals.jsonMembers}, html ${report.totals.htmlMembers}, media stand-ins ${report.totals.mediaStandIns}, failed ${report.totals.failedMembers})`);
  out.push(`Strings replaced ${report.counters.stringsReplaced}, identifiers pseudonymized ${report.counters.idsPseudonymized}, timestamps coarsened ${report.counters.timestampsCoarsened}`);
  out.push(`Structure check: ${report.checks.structure}`);
  out.push(`Leak check (nothing outside the allow-lists survived): ${report.checks.leak} (${report.checks.leakRegistry.distinctProtectedPieces} input pieces of at least ${report.checks.leakRegistry.minChars} characters tracked, coverage ${report.checks.leakRegistry.coverage})`);
  const residual = report.residualMetadata;
  out.push('', 'RESIDUAL METADATA that stays in the output (review before sharing; this is what PASS does not cover):');
  const line = (label, values) => { if (values.length) out.push(`  ${label}: ${values.slice(0, 14).join(', ')}${values.length > 14 ? ', ...' : ''}`); };
  for (const [key, values] of Object.entries(residual.enumValues.byKey)) line(`enum ${key}`, Object.keys(values));
  line('member names', residual.memberNames);
  line('HTML class tokens', residual.htmlClassTokens);
  line('HTML attribute values', residual.htmlAttributeValues);
  line('link hosts', residual.linkHosts);
  out.push(`  timestamps: ${residual.timestampPrecision}`);
  if (report.counters.enumValuesReplaced) out.push(`  enum-like values outside the known vocabulary were replaced: ${report.counters.enumValuesReplaced} (use --keep-enum to keep a reviewed value)`);
  if (residual.truncated) out.push('  (lists truncated; see the report)');
  const unknown = Object.entries(report.unknownFields);
  if (unknown.length) {
    out.push('Unknown field names (kept as structure; review before sharing):');
    for (const [member, names] of unknown) out.push(`  ${member}: ${names.slice(0, 30).join(', ')}${names.length > 30 ? ', ...' : ''}`);
  }
  if (result.hints?.length) {
    out.push('', 'OWNER-ONLY HINT (not in the report): frequent leading words of HTML text nodes that were replaced.');
    out.push('If one is a structural label (for example a localized "Prompted"), rerun with --keep-label <word>.');
    for (const [word, count] of result.hints) out.push(`  ${count}x ${word}`);
  }
  out.push('', result.outputPath ? `Sanitized output: ${result.outputPath}` : 'Dry run: no output was kept.');
  if (result.exitCode === 2) out.push('PARTIAL: some members could not be parsed and were replaced by empty stand-ins (see STATUS).');
  out.push('Running this on a real export is an owner-local step; record it as NOT_RUN until done.');
  process.stdout.write(`${out.join('\n')}\n`);
  process.exit(result.exitCode);
} catch (error) {
  // Only error codes and structural counts are printed; messages never contain input text.
  const code = error?.code ?? 'E_UNEXPECTED';
  fail(`${code}: ${error?.exitCode === 3 ? error.message : error?.message ?? 'failed'}`, error?.exitCode ?? 1);
}

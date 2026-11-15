import test from 'node:test';
import assert from 'node:assert/strict';
import { evaluateAudit, compareFindings, auditArguments } from '../scripts/supply-chain/npm-audit.mjs';
import { evaluateSarif } from '../scripts/supply-chain/sarif.mjs';
import { readFileSync, writeFileSync, cpSync, mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
function report(severity) {
  const counts = { info: 0, low: 0, moderate: 0, high: 0, critical: 0, total: severity ? 1 : 0 };
  if (severity) counts[severity] = 1;
  return { auditReportVersion: 2, vulnerabilities: severity ? { example: { severity, range: '<2', nodes: ['node_modules/example'], via: [{ source: 123, url: 'https://github.com/advisories/GHSA-example' }], fixAvailable: true } } : {}, metadata: { vulnerabilities: counts } };
}
function result(severity) { return { status: ['high', 'critical'].includes(severity) ? 1 : 0, stdout: JSON.stringify(report(severity)) }; }

test('npm includes development, optional and peer dependencies even in production environments', () => {
  for (const required of ['--include=dev', '--include=optional', '--include=peer', '--ignore-scripts', '--audit-level=high', '--package-lock-only']) assert.ok(auditArguments.includes(required));
  assert.ok(!auditArguments.some(arg => arg.startsWith('--omit')));
});
for (const severity of [undefined, 'low', 'moderate', 'high', 'critical']) {
  test(`npm threshold: ${severity ?? 'clean'}`, () => {
    assert.equal(evaluateAudit(result(severity)).status, ['high', 'critical'].includes(severity) ? 'FAIL' : 'PASS');
  });
}
for (const [label, mutation] of [
  ['network failure', r => { r.status = 2; }],
  ['spawn error', r => { r.error = new Error('ENOENT'); }],
  ['invalid JSON', r => { r.stdout = '<html>502</html>'; }],
  ['registry error', r => { r.stdout = JSON.stringify({ error: { code: 'EAUDIT' } }); }],
  ['missing report', r => { r.stdout = '{}'; }],
  ['wrong report version', r => { const d = JSON.parse(r.stdout); d.auditReportVersion = 1; r.stdout = JSON.stringify(d); }],
  ['negative counts', r => { const d = JSON.parse(r.stdout); d.metadata.vulnerabilities.low = -1; r.stdout = JSON.stringify(d); }],
  ['inconsistent total', r => { const d = JSON.parse(r.stdout); d.metadata.vulnerabilities.total = 0; r.stdout = JSON.stringify(d); }],
  ['hidden severity', r => { const d = JSON.parse(r.stdout); d.vulnerabilities.example.severity = 'unknown'; r.stdout = JSON.stringify(d); }],
  ['severity count mismatch', r => { const d = JSON.parse(r.stdout); d.vulnerabilities.example.severity = 'critical'; r.stdout = JSON.stringify(d); }],
  ['exit falsely green', r => { r.status = 0; }],
]) test(`npm fails closed: ${label}`, () => { const r = result('high'); mutation(r); assert.throws(() => evaluateAudit(r), /NPM_AUDIT_/); });

test('existing high findings remain blocking; changed severity is a regression', () => {
  const base = evaluateAudit(result('high'));
  const same = evaluateAudit(result('high'));
  assert.equal(compareFindings(same, base).existing.length, 1);
  assert.equal(same.status, 'FAIL');
  assert.equal(compareFindings(evaluateAudit(result('critical')), base).introducedOrChanged.length, 1);
  assert.equal(compareFindings(evaluateAudit(result()), base).resolved.length, 1);
});

const cleanSarif = () => ({ version: '2.1.0', runs: [{ tool: { driver: { name: 'CodeQL' } }, results: [] }] });
test('CodeQL clean scan passes while even suppressed findings require disposition', () => {
  assert.equal(evaluateSarif(cleanSarif()).status, 'PASS');
  const report = cleanSarif(); report.runs[0].results.push({ suppressions: [{ status: 'accepted' }] });
  assert.equal(evaluateSarif(report).status, 'FAIL');
});
test('CodeQL incomplete or failed scans cannot pass', () => {
  for (const report of [{}, { version: '2.1.0', runs: [] }, { version: '2.1.0', runs: [{ tool: { driver: { name: 'CodeQL' } } }] }]) assert.throws(() => evaluateSarif(report), /CODEQL_BLOCKED/);
  const report = cleanSarif(); report.runs[0].invocations = [{ executionSuccessful: false }];
  assert.throws(() => evaluateSarif(report), /CODEQL_BLOCKED/);
});

for (const [label, filename, mutate] of [
  ['CodeQL mutable action', 'codeql.yml', s => s.replace(/github\/codeql-action\/init@[a-f0-9]{40}/, 'github/codeql-action/init@v4')],
  ['CodeQL upload write permission', 'codeql.yml', s => s.replace('contents: read', 'contents: read\n  security-events: write')],
  ['Scorecard OIDC permission', 'scorecard.yml', s => s.replace('contents: read', 'contents: read\n  id-token: write')],
  ['SBOM checkout credentials', 'release-sbom.yml', s => s.replace('persist-credentials: false', 'persist-credentials: true')],
  ['privileged audit trigger', 'supply-chain.yml', s => s.replace('  pull_request:', '  pull_request_target:')],
]) test(`supply-chain trust boundary rejects ${label}`, t => {
  const fixture = mkdtempSync(join(tmpdir(), 'mp-sec-12-policy-'));
  t.after(() => rmSync(fixture, { recursive: true, force: true }));
  mkdirSync(join(fixture, 'scripts')); mkdirSync(join(fixture, '.github'));
  cpSync(join(root, '.github/workflows'), join(fixture, '.github/workflows'), { recursive: true });
  cpSync(join(root, 'scripts/verify-workflow-security.mjs'), join(fixture, 'scripts/verify-workflow-security.mjs'));
  const file = join(fixture, '.github/workflows', filename);
  const before = readFileSync(file, 'utf8'); const after = mutate(before); assert.notEqual(after, before);
  writeFileSync(file, after);
  const result = spawnSync(process.execPath, [join(fixture, 'scripts/verify-workflow-security.mjs')], { encoding: 'utf8' });
  assert.notEqual(result.status, 0, result.stdout + result.stderr);
});

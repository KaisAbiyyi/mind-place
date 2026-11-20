import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const levels = ['info', 'low', 'moderate', 'high', 'critical'];
export const auditArguments = ['audit', '--json', '--package-lock-only', '--ignore-scripts', '--include=dev', '--include=optional', '--include=peer', '--audit-level=high'];

export function evaluateAudit(result) {
  if (result.error || ![0, 1].includes(result.status)) throw new Error('NPM_AUDIT_UNAVAILABLE: process/network/registry failure');
  let audit;
  try { audit = JSON.parse(result.stdout); } catch { throw new Error('NPM_AUDIT_INVALID: invalid JSON'); }
  if (audit.error || audit.auditReportVersion !== 2 || !audit.vulnerabilities || !audit.metadata?.vulnerabilities) {
    throw new Error('NPM_AUDIT_INVALID: missing v2 report or registry error');
  }
  const counts = audit.metadata.vulnerabilities;
  for (const key of [...levels, 'total']) {
    if (!Number.isSafeInteger(counts[key]) || counts[key] < 0) throw new Error('NPM_AUDIT_INVALID: invalid severity counts');
  }
  const entries = Object.entries(audit.vulnerabilities);
  if (levels.reduce((n, level) => n + counts[level], 0) !== counts.total || entries.length !== counts.total) {
    throw new Error('NPM_AUDIT_INVALID: contradictory counts');
  }
  const observed = Object.fromEntries(levels.map(level => [level, 0]));
  const findings = entries.map(([name, finding]) => {
    if (!levels.includes(finding.severity) || !Array.isArray(finding.via) || !Array.isArray(finding.nodes)) {
      throw new Error('NPM_AUDIT_INVALID: malformed finding');
    }
    observed[finding.severity]++;
    const advisories = finding.via.filter(v => typeof v === 'object').map(v => {
      if (!Number.isSafeInteger(v.source) || typeof v.url !== 'string') throw new Error('NPM_AUDIT_INVALID: malformed advisory');
      return { source: v.source, url: v.url, title: v.title, severity: v.severity };
    });
    const chain = finding.via.filter(v => typeof v === 'string').sort();
    return { name, severity: finding.severity, range: finding.range, nodes: finding.nodes, advisories, chain, fixAvailable: finding.fixAvailable };
  }).sort((a, b) => a.name.localeCompare(b.name));
  if (levels.some(level => observed[level] !== counts[level])) throw new Error('NPM_AUDIT_INVALID: severity count mismatch');
  const blocked = counts.high + counts.critical > 0;
  if ((result.status === 1) !== blocked) throw new Error('NPM_AUDIT_INVALID: exit code disagrees with high threshold');
  return { status: blocked ? 'FAIL' : 'PASS', threshold: 'high', includesDevelopment: true, counts, findings };
}

// Time-boxed, owner-authorized acceptance of an advisory that has NO patched version. It never hides a finding:
// the raw report and status stay in the receipt, an acceptance covers only the exact advisory URL on the exact
// package while npm still reports no fixed range (`range: "*"`), and transitive findings are covered only when
// every link of their chain is covered. Expired, malformed or non-matching entries cover nothing.
export const acceptanceFile = resolve(dirname(fileURLToPath(import.meta.url)), 'npm-accepted-advisories.json');
const levelIndex = level => levels.indexOf(level);

export function validateAcceptances(document) {
  if (!document || document.schema !== 1 || !Array.isArray(document.accepted)) throw new Error('NPM_ACCEPTANCE_INVALID: schema');
  for (const e of document.accepted) {
    const ok = e && typeof e === 'object' && /^https:\/\/github\.com\/advisories\/GHSA-[a-z0-9-]+$/.test(e.url) &&
      typeof e.package === 'string' && e.package && e.noPatchedVersionRange === '*' && levels.includes(e.maxSeverity) &&
      e.scope === 'build-time-dependency' && typeof e.reason === 'string' && e.reason.length >= 40 &&
      typeof e.decidedBy === 'string' && e.decidedBy && /^\d{4}-\d{2}-\d{2}$/.test(e.expires) && Number.isFinite(Date.parse(e.expires + 'T00:00:00Z'));
    if (!ok) throw new Error('NPM_ACCEPTANCE_INVALID: entry');
  }
  return document.accepted;
}

export function applyAcceptances(report, document, now = new Date()) {
  const entries = validateAcceptances(document).filter(e => Date.parse(e.expires + 'T00:00:00Z') > now.getTime());
  const blocking = report.findings.filter(f => levelIndex(f.severity) >= levelIndex('high'));
  const byName = new Map(report.findings.map(f => [f.name, f]));
  const memo = new Map();
  const covered = (finding, trail = new Set()) => {
    if (memo.has(finding.name)) return memo.get(finding.name);
    if (trail.has(finding.name)) return false;
    let result;
    if (finding.advisories.length > 0) {
      result = finding.advisories.every(a => entries.some(e => e.url === a.url && e.package === finding.name &&
        finding.range === e.noPatchedVersionRange && levelIndex(a.severity) <= levelIndex(e.maxSeverity)));
    } else {
      const next = new Set(trail).add(finding.name);
      result = finding.chain.length > 0 && finding.chain.every(name => byName.has(name) && covered(byName.get(name), next));
    }
    memo.set(finding.name, result);
    return result;
  };
  const acceptedFindings = blocking.filter(f => covered(f)).map(f => f.name);
  const unaccepted = blocking.filter(f => !covered(f)).map(f => f.name);
  return {
    status: report.status === 'PASS' || unaccepted.length === 0 ? 'PASS' : 'FAIL',
    rawStatus: report.status,
    acceptedFindings,
    unaccepted,
    activeAcceptances: entries.map(e => ({ url: e.url, package: e.package, expires: e.expires, decidedBy: e.decidedBy })),
  };
}

export function compareFindings(head, base) {
  const identity = f => JSON.stringify([f.name, f.range, f.severity, f.advisories.map(a => a.source).sort()]);
  const previous = new Set(base.findings.map(identity));
  const current = new Set(head.findings.map(identity));
  return {
    existing: head.findings.filter(f => previous.has(identity(f))),
    introducedOrChanged: head.findings.filter(f => !previous.has(identity(f))),
    resolved: base.findings.filter(f => !current.has(identity(f))),
  };
}

function audit(directory) {
  // No shell and no lifecycle scripts. npm includes dev deps even under NODE_ENV=production.
  const executable = process.platform === 'win32' ? process.execPath : 'npm';
  const args = process.platform === 'win32'
    ? [resolve(dirname(process.execPath), 'node_modules/npm/bin/npm-cli.js'), ...auditArguments]
    : auditArguments;
  const result = spawnSync(executable, args, { cwd: directory, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024, timeout: 180_000, windowsHide: true });
  return { report: evaluateAudit(result), raw: result.stdout };
}

function main() {
  const args = process.argv.slice(2);
  if (args.length !== 3) throw new Error('Usage: npm-audit.mjs HEAD_DIRECTORY BASE_DIRECTORY OUTPUT_JSON');
  const [headDirectory, baseDirectory, output] = args.map(p => resolve(p));
  mkdirSync(dirname(output), { recursive: true });
  try {
    const base = audit(baseDirectory);
    writeFileSync(output + '.base.raw.json', base.raw);
    const head = audit(headDirectory);
    writeFileSync(output + '.head.raw.json', head.raw);
    const acceptance = applyAcceptances(head.report, JSON.parse(readFileSync(acceptanceFile, 'utf8')));
    const receipt = { schema: 2, headSha: process.env.SOURCE_HEAD, baseSha: process.env.SOURCE_BASE, head: head.report, base: base.report, comparison: compareFindings(head.report, base.report), acceptance };
    writeFileSync(output, JSON.stringify(receipt, null, 2) + '\n');
    console.log(JSON.stringify(receipt, null, 2));
    process.exitCode = acceptance.status === 'PASS' ? 0 : 1;
  } catch (error) {
    writeFileSync(output, JSON.stringify({ schema: 1, status: 'BLOCKED', error: error.message }) + '\n');
    throw error;
  }
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();

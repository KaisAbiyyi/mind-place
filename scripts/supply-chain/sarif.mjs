import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export function evaluateSarif(report) {
  if (report.version !== '2.1.0' || !Array.isArray(report.runs) || report.runs.length === 0) throw new Error('CODEQL_BLOCKED: missing SARIF runs');
  let findings = 0;
  for (const run of report.runs) {
    if (!run.tool?.driver?.name || !Array.isArray(run.results) ||
        run.invocations?.some(invocation => invocation.executionSuccessful === false)) throw new Error('CODEQL_BLOCKED: incomplete analysis');
    // No suppressions or baseline exclusions: all security-extended results require review.
    findings += run.results.length;
  }
  return { status: findings ? 'FAIL' : 'PASS', findings, policy: 'All security-extended findings block; no automatic suppression or regression classification.' };
}

export function repositoryPath(uri) {
  if (typeof uri !== 'string') throw new Error('CODEQL_BLOCKED: missing location URI');
  let path;
  try { path = decodeURIComponent(uri); } catch { throw new Error('CODEQL_BLOCKED: invalid location URI'); }
  if (typeof path !== 'string' || !path || path.includes('\\') || path.includes(':') || path.startsWith('/') || path.includes('\0') || path.split('/').some(part => !part || part === '.' || part === '..')) {
    throw new Error('CODEQL_BLOCKED: location must name a tracked relative path');
  }
  return path;
}

function checkedCommit(sha, cwd) {
  if (!/^[0-9a-f]{40}$/.test(sha ?? '')) throw new Error('CODEQL_BLOCKED: missing exact source commit');
  const result = spawnSync('git', ['cat-file', '-e', `${sha}^{commit}`], { cwd, windowsHide: true });
  if (result.status !== 0) throw new Error('CODEQL_BLOCKED: source commit unavailable');
}

export function sourceProvenance(report, head, base, cwd = process.cwd()) {
  checkedCommit(head, cwd); checkedCommit(base, cwd);
  const digest = (sha, path) => {
    // Read committed blobs, never SARIF-controlled filesystem paths. No shell.
    const result = spawnSync('git', ['cat-file', 'blob', `${sha}:${path}`], { cwd, maxBuffer: 32 * 1024 * 1024, windowsHide: true });
    if (result.error || ![0, 128].includes(result.status)) throw new Error('CODEQL_BLOCKED: source blob read failed');
    return result.status === 0 ? createHash('sha256').update(result.stdout).digest('hex') : null;
  };
  return report.runs.flatMap(run => run.results.map(result => ({
    ruleId: result.ruleId ?? null,
    message: result.message?.text ?? result.message?.markdown ?? null,
    requiresIndependentDisposition: true,
    locations: (result.locations ?? []).map(location => {
      const path = repositoryPath(location.physicalLocation?.artifactLocation?.uri);
      const headSha256 = digest(head, path), baseSha256 = digest(base, path);
      if (!headSha256) throw new Error('CODEQL_BLOCKED: result source absent from head');
      return { path, line: location.physicalLocation.region?.startLine ?? null, headSha256, baseSha256,
        classification: headSha256 === baseSha256 ? 'UNCHANGED_SOURCE_REQUIRES_TRIAGE' : 'CHANGED_SOURCE_REQUIRES_TRIAGE' };
    }),
  })));
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const directory = process.argv[2];
  const files = readdirSync(directory).filter(file => file.endsWith('.sarif'));
  if (!files.length) throw new Error('CODEQL_BLOCKED: no SARIF output');
  let failed = false;
  const summaries = [];
  for (const file of files) {
    const report = JSON.parse(readFileSync(join(directory, file), 'utf8'));
    const result = evaluateSarif(report);
    const details = sourceProvenance(report, process.env.SOURCE_HEAD, process.env.SOURCE_BASE);
    const summary = { file, ...result, details };
    summaries.push(summary);
    console.log(JSON.stringify(summary));
    failed ||= result.status !== 'PASS';
  }
  writeFileSync(join(directory, 'triage.json'), JSON.stringify({ schema: 1, headSha: process.env.SOURCE_HEAD, baseSha: process.env.SOURCE_BASE, reports: summaries,
    limitation: 'File equality is source provenance, not an exploitability verdict, query exclusion, baseline waiver or acceptance.' }, null, 2) + '\n');
  process.exitCode = failed ? 1 : 0;
}

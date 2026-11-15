import { readdirSync, readFileSync } from 'node:fs';
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

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const directory = process.argv[2];
  const files = readdirSync(directory).filter(file => file.endsWith('.sarif'));
  if (!files.length) throw new Error('CODEQL_BLOCKED: no SARIF output');
  let failed = false;
  for (const file of files) {
    const result = evaluateSarif(JSON.parse(readFileSync(join(directory, file), 'utf8')));
    console.log(JSON.stringify({ file, ...result }));
    failed ||= result.status !== 'PASS';
  }
  process.exitCode = failed ? 1 : 0;
}

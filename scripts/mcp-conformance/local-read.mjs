/**
 * MP-R1-WP02 local-read cells: Claude Code and Antigravity, run back to back on ONE exact SHA.
 *
 * `run.mjs local-read` runs both real clients, probes each client's version before the first run and after
 * the last, and records a summary with `localReadVerdict`. The verdict is PASS only when both clients passed
 * every claim (C1-C9) on the same clean commit and no client changed version (inside a run, which
 * real.mjs/clientVersionDrift already fails, or between the probes). It never turns a missing, blocked or
 * partial run into a pass.
 */
export const LOCAL_READ_CELL_CLIENTS = Object.freeze(['claude-code', 'antigravity']);
const SHA_RE = /^[0-9a-f]{40}$/;

/**
 * @param {{ sha: string, dirty: boolean, runs: Record<string, any>, versionsBefore: Record<string, string|null>,
 *           versionsAfter: Record<string, string|null> }} input
 */
export function localReadVerdict({ sha, dirty, runs = {}, versionsBefore = {}, versionsAfter = {} }) {
  const problems = [];
  if (!SHA_RE.test(String(sha ?? ''))) problems.push('the run is not bound to an exact 40-hex commit');
  if (dirty) problems.push('the tracked tree was dirty; rerun from a clean, committed tree');
  const cells = {};
  for (const client of LOCAL_READ_CELL_CLIENTS) {
    const run = runs[client];
    const before = versionsBefore[client] ?? null;
    const after = versionsAfter[client] ?? null;
    const cellProblems = [];
    let result;
    if (!run) {
      result = 'NOT_RUN';
      cellProblems.push('no run recorded');
    } else if (run.blocked) {
      result = 'NOT_RUN';
      cellProblems.push(`blocked: ${run.blocked.kind ?? 'unknown'}`);
    } else {
      const claims = run.claims ?? {};
      const failed = ['C1', 'C2', 'C3', 'C4', 'C5', 'C6', 'C7', 'C8', 'C9'].filter((id) => claims[id]?.result !== 'PASS');
      if (failed.length) cellProblems.push(`claims not PASS: ${failed.join(', ')}`);
      if (run.harness_failure) cellProblems.push(`harness: ${run.harness_failure}`);
      if (run.source?.sha !== sha) cellProblems.push('receipt is from a different commit');
      if (!before || !after) cellProblems.push('client version was not recorded before and after');
      else if (before !== after) cellProblems.push(`client version changed between probes (${before} -> ${after}); pause updates and rerun`);
      result = cellProblems.length ? 'FAIL' : 'PASS';
    }
    cells[client] = { result, version_before: before, version_after: after, receipt: run?.receipt ?? null, receipt_sha256: run?.receipt_sha256 ?? null, problems: cellProblems };
  }
  const pass = problems.length === 0 && LOCAL_READ_CELL_CLIENTS.every((c) => cells[c].result === 'PASS');
  const anyRun = LOCAL_READ_CELL_CLIENTS.some((c) => cells[c].result !== 'NOT_RUN');
  return { result: pass ? 'PASS' : anyRun || problems.length ? 'FAIL' : 'NOT_RUN', sha: sha ?? null, problems, cells };
}

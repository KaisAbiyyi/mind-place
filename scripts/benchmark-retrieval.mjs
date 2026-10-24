/**
 * MP-10B: run the real Rust FTS5 retrieval test on isolated synthetic data,
 * feed all ranked source IDs to the independent MP-10A scorer and compare cost.
 *
 * NOT a deployed product benchmark: the fixture bypasses normal capture/authority,
 * and project_graph_proxy / hybrid_fts5_project_proxy are in-memory experiments.
 */
import { spawnSync } from 'node:child_process';
import { writeFileSync } from 'node:fs';
import { evaluate, loadSuite } from './benchmark-continuity.mjs';

const strategies = ['lexical_fts5', 'structured_fts5', 'project_graph_proxy', 'hybrid_fts5_project_proxy'];
const run = (exe, args) => spawnSync(exe, args, {
  encoding: 'utf8', maxBuffer: 16 * 1024 * 1024, shell: false,
});
export function parseRuns(output) {
  const predictions = new Map();
  for (const line of output.split(/\r?\n/)) {
    const offset = line.indexOf('MP10B_PREDICTIONS|');
    if (offset === -1) continue;
    const marker = line.slice(offset);
    const match = marker.match(/^MP10B_PREDICTIONS\|([a-z0-9_]+)\|(\{.*\})$/);
    if (!match || predictions.has(match[1]) || !strategies.includes(match[1])) {
      throw new Error('Invalid or duplicate MP10B prediction marker');
    }
    predictions.set(match[1], JSON.parse(match[2]));
  }
  if (predictions.size !== strategies.length || strategies.some(s => !predictions.has(s))) {
    throw new Error('Missing MP10B prediction strategy; Rust test may not have executed');
  }
  return predictions;
}

export function summarize(suite, outputs) {
  const evaluated = {};
  for (const strategy of strategies) {
    const prediction = outputs.get(strategy);
    const report = evaluate(suite, prediction); // fail closed on scope/IDs/missing cases
    if (report.summary.privacy_violation_count !== 0) {
      throw new Error(strategy + ' exposed a restricted or excluded synthetic source');
    }
    evaluated[strategy] = {
      engine: strategy.includes('proxy') ? 'SYNTHETIC_PROXY_NOT_PRODUCT_GRAPH' : 'MIND_PLACE_FTS5_ISOLATED_INDEX',
      summary: report.summary,
      misses: report.cases.filter(c => c.relevant_total > 0 && c.recall_at_k < 1)
        .map(c => ({ id: c.id, recall_at_k: c.recall_at_k, rank: c.reciprocal_rank })),
      cases: report.cases,
    };
  }
  const candidates = ['lexical_fts5', 'structured_fts5'].map(name => ({
    name, score: evaluated[name].summary.macro_ndcg_at_k ?? -1,
    tokens: evaluated[name].summary.mean_context_estimated_tokens ?? Infinity,
  }));
  candidates.sort((a, b) => b.score - a.score || a.tokens - b.tokens || a.name.localeCompare(b.name));
  return {
    schema: 'mind-place-retrieval-bakeoff/v1',
    dataset: suite.corpus.dataset,
    mode: 'isolated_synthetic_real_fts5_plus_labeled_project_adjacency_proxies',
    fixture_sha256: suite.fixture_sha256,
    code_head_sha: process.env.MP10B_HEAD_SHA || null,
    platform: process.platform,
    arch: process.arch,
    node: process.version,
    rankings: evaluated,
    lexical_tuning_candidate: candidates[0].name,
    deployment_decision: 'NOT_AUTHORIZED__requires_real_index_ingestion_and_qualification',
    caveats: [
      '18 questions / 24 synthetic records, not 1M-chunk scale or native hardware proof.',
      'FTS5 is real IndexStore::retrieve; test-only synthetic injection skips state/capture authorization.',
      'No user archive, approved-root I/O, provider session, live permission check or network call.',
      'Project edge graph is a documented in-memory proxy, not production GraphNeighborhoodRequest.',
      'No generated answers or verified claims: answer accuracy/support and abstention are NOT qualified.',
      'Latency is warm in-process retrieval, not startup/UI/packaged Windows SLA.',
      'Context tokens are estimated as ceil(UTF-8 string length / 4), not billed tokens.',
    ],
  };
}

export function buildReport(stdout, suite = loadSuite()) {
  return summarize(suite, parseRuns(stdout));
}

function main() {
  const args = process.argv.slice(2);
  if (args.length !== 0 && (args.length !== 2 || args[0] !== '--output')) {
    throw new Error('usage: node scripts/benchmark-retrieval.mjs [--output OUTPUT.json]');
  }
  const result = run('cargo', ['test', '-p', 'mind-place-index', '--test', 'continuity_bakeoff', '--', '--nocapture']);
  if (result.error || result.status !== 0) {
    throw new Error('Rust synthetic FTS5 test failed:\n' + (result.stderr || result.error || result.stdout).toString());
  }
  const report = buildReport(result.stdout);
  const json = JSON.stringify(report, null, 2) + '\n';
  if (args.length === 2) writeFileSync(args[1], json, { flag: 'w' });
  process.stdout.write(json);
}
if (process.argv[1] && import.meta.url === new URL('file://' + process.argv[1].replace(/\\/g, '/')).href) {
  try { main(); } catch (error) {
    process.stderr.write(String(error.stack ?? error) + '\n');
    process.exitCode = 1;
  }
}

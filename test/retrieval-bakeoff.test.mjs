import test from 'node:test';
import assert from 'node:assert/strict';
import { buildReport, parseRuns, summarize } from '../scripts/benchmark-retrieval.mjs';
import { loadSuite, oraclePredictions } from '../scripts/benchmark-continuity.mjs';

const suite = loadSuite();
const STRATEGIES = ['lexical_fts5', 'structured_fts5', 'project_graph_proxy', 'hybrid_fts5_project_proxy'];
function markerRuns() {
  // Schema-fixture oracle is used ONLY to test parser + scorer interop.
  // It is not a measured benchmark candidate, and no run is published as product quality.
  return STRATEGIES.map(name =>
    'MP10B_PREDICTIONS|' + name + '|' + JSON.stringify(oraclePredictions(suite))).join('\n');
}

test('MP-10B scorer integration accepts four distinct strategy receipts', () => {
  const runs = parseRuns(markerRuns());
  assert.deepEqual([...runs.keys()], STRATEGIES);
  const result = buildReport(markerRuns(), suite);
  assert.equal(result.schema, 'mind-place-retrieval-bakeoff/v1');
  assert.equal(result.fixture_sha256.corpus, suite.fixture_sha256.corpus);
  assert.equal(result.rankings.lexical_fts5.engine, 'MIND_PLACE_FTS5_ISOLATED_INDEX');
  assert.equal(result.rankings.project_graph_proxy.engine, 'SYNTHETIC_PROXY_NOT_PRODUCT_GRAPH');
  assert.match(result.deployment_decision, /NOT_AUTHORIZED/);
  assert.equal(result.rankings.lexical_fts5.summary.privacy_violation_count, 0);
});

test('MP-10B refuses incomplete, unrecognized and repeated result markers', () => {
  assert.throws(() => parseRuns(''), /Missing/);
  assert.throws(() => parseRuns(markerRuns().replace(/project_graph_proxy.*/, '')), /Missing/);
  assert.throws(() => parseRuns(markerRuns() + '\n' + markerRuns().split('\n')[0]), /duplicate/);
  assert.throws(() => parseRuns(markerRuns().replace('lexical_fts5', 'unknown_strategy')), /Invalid/);
});

test('MP-10B fails closed on scope leakage, missing cases, unknown source and bad timestamps', () => {
  const invalid = [
    p => { p.cases[0].hits = ['e20']; },
    p => { p.cases.pop(); },
    p => { p.cases[0].hits = ['unknown']; },
    p => { p.cases[0].latency_ms = -3; },
  ];
  for (const edit of invalid) {
    const outputs = new Map(STRATEGIES.map(s => [s, oraclePredictions(suite)]));
    edit(outputs.get('lexical_fts5'));
    assert.throws(() => summarize(suite, outputs), /restricted|Invalid continuity benchmark/i);
  }
});

test('MP-10B token/ranking statistics do not give a proxy permission to deploy', () => {
  const outputs = new Map(STRATEGIES.map(s => [s, oraclePredictions(suite)]));
  const summary = summarize(suite, outputs);
  assert.equal(summary.deployment_decision, 'NOT_AUTHORIZED__requires_real_index_ingestion_and_qualification');
  assert.ok(STRATEGIES.every(s => summary.rankings[s].summary.cases === 18));
  assert.deepEqual(Object.keys(summary.rankings), STRATEGIES);
});

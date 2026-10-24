import test from 'node:test';
import assert from 'node:assert/strict';
import { evaluate, loadSuite, oraclePredictions, validateSuite } from '../scripts/benchmark-continuity.mjs';

const clone = (value) => structuredClone(value);
const suite = loadSuite();

test('MP-10A frozen synthetic data covers cross-provider, projects, time, source scopes and abstention', () => {
  assert.equal(validateSuite(suite), true);
  assert.equal(suite.corpus.sources.length, 24);
  assert.equal(suite.gold.cases.length, 18);
  assert.deepEqual([...new Set(suite.corpus.sources.map(s => s.provider))].sort(), ['chatgpt', 'claude', 'gemini', 'generic']);
  assert.deepEqual([...new Set(suite.corpus.sources.map(s => s.project))].sort(), ['aster', 'beryl']);
  assert.ok(suite.corpus.sources.some(s => s.scope === 'restricted'));
  assert.ok(suite.gold.cases.some(q => q.abstain));
  assert.ok(suite.gold.cases.some(q => q.prohibited_claims.length > 0));
  assert.match(suite.fixture_sha256.corpus, /^[a-f0-9]{64}$/);
  assert.match(suite.fixture_sha256.gold, /^[a-f0-9]{64}$/);
});

test('oracle proves case-to-evidence and scoring contracts, NOT product retrieval quality', () => {
  const oracle = oraclePredictions(suite);
  const report = evaluate(suite, oracle);
  assert.equal(report.summary.answer_accuracy, 1);
  assert.equal(report.summary.macro_recall_at_k, 1);
  assert.equal(report.summary.mean_reciprocal_rank, 1);
  assert.equal(report.summary.macro_ndcg_at_k, 1);
  assert.equal(report.summary.privacy_violation_count, 0);
  assert.equal(report.summary.unsupported_claim_count, 0);
  assert.equal(report.summary.abstention_accuracy, 1);
  assert.equal(report.summary.abstention_cases, 2);
  assert.deepEqual(report.cases.map(c => c.id), suite.gold.cases.map(c => c.id));
});

test('ranking metrics penalize misplaced and missing relevant evidence', () => {
  const predictions = oraclePredictions(suite);
  const entry = predictions.cases.find(c => c.id === 'q01');
  entry.hits = ['e10', 'e03'];
  const report = evaluate(suite, predictions);
  const q = report.cases.find(c => c.id === 'q01');
  assert.equal(q.recall_at_k, 0.5);
  assert.equal(q.precision_at_k, 0.2);
  assert.equal(q.reciprocal_rank, 0.5);
  assert.ok(q.ndcg_at_k > 0 && q.ndcg_at_k < 1);
  assert.equal(q.answer_correct, true); // e03 supports the required final decision
});

test('privacy and restricted evidence fail even when an answer abstains', () => {
  const predictions = oraclePredictions(suite);
  predictions.cases.find(c => c.id === 'q14').hits = ['e20'];
  const report = evaluate(suite, predictions);
  assert.equal(report.summary.privacy_violation_count, 1);
  assert.equal(report.cases.find(c => c.id === 'q14').answer_correct, false);
  assert.deepEqual(report.cases.find(c => c.id === 'q14').forbidden_hits, ['e20']);
});

test('hallucinated, stale or wrong-project claims are flagged separately from retrieval', () => {
  const predictions = oraclePredictions(suite);
  predictions.cases.find(c => c.id === 'q01').claims = ['old_vector_proposal'];
  predictions.cases.find(c => c.id === 'q13').claims = ['beryl_memory_budget'];
  predictions.cases.find(c => c.id === 'q13').abstained = false;
  const report = evaluate(suite, predictions);
  assert.equal(report.cases.find(c => c.id === 'q01').answer_correct, false);
  assert.ok(report.cases.find(c => c.id === 'q01').prohibited_claims.includes('old_vector_proposal'));
  assert.equal(report.cases.find(c => c.id === 'q13').answer_correct, false);
  assert.ok(report.summary.unsupported_claim_count >= 2);
  assert.ok(report.summary.prohibited_claim_count >= 2);
});

test('unknown, duplicate, incomplete or non-finite prediction input fails closed', () => {
  const oracle = oraclePredictions(suite);
  const incorrect = [
    () => { const p = clone(oracle); p.dataset = 'another-dataset'; return p; },
    () => { const p = clone(oracle); p.cases.pop(); return p; },
    () => { const p = clone(oracle); p.cases[0].id = 'q02'; return p; },
    () => { const p = clone(oracle); p.cases[0].hits = ['nonexistent']; return p; },
    () => { const p = clone(oracle); p.cases[0].hits = ['e03', 'e03']; return p; },
    () => { const p = clone(oracle); p.cases[0].latency_ms = -1; return p; },
    () => { const p = clone(oracle); p.cases[0].context_estimated_tokens = 1.4; return p; },
  ];
  for (const make of incorrect) assert.throws(() => evaluate(suite, make()), /Invalid continuity benchmark/);
});

test('gold suite rejects unproven required claims and relevance to restricted sources', () => {
  const badClaim = clone(suite);
  badClaim.gold.cases[0].required_claims.push('nonexistent_claim');
  assert.throws(() => validateSuite(badClaim), /unproven required claim/);

  const badEvidence = clone(suite);
  badEvidence.gold.cases[0].relevant.e20 = 3;
  assert.throws(() => validateSuite(badEvidence), /relevant evidence/);

  const badAbstain = clone(suite);
  badAbstain.gold.cases.find(c => c.id === 'q14').abstain = false;
  assert.throws(() => validateSuite(badAbstain), /abstention contract/);
});

test('scores are deterministic under prediction input case reorder', () => {
  const p = oraclePredictions(suite);
  const original = evaluate(suite, p);
  p.cases.reverse();
  assert.deepEqual(evaluate(suite, p), original);
});

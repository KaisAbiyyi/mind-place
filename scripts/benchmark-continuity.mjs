/**
 * MP-10A: deterministic, dependency-free scorer for synthetic continuity evidence.
 * This scores supplied predictions; it does NOT run the product retrieval engine.
 * --oracle is a fixture/schema smoke check, not a measured product score.
 */
import { readFileSync, statSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';

const corpusUrl = new URL('../benchmarks/continuity/v1/corpus.json', import.meta.url);
const goldUrl = new URL('../benchmarks/continuity/v1/gold.json', import.meta.url);
const safeId = /^[a-z][a-z0-9_-]{0,79}$/;
const isObject = (value) => value !== null && typeof value === 'object' && !Array.isArray(value);
const unique = (items) => new Set(items).size === items.length;
const requireValid = (condition, detail) => {
  if (!condition) throw new Error('Invalid continuity benchmark: ' + detail);
};
const nonEmpty = (s, max = 10000) => typeof s === 'string' && s.trim().length > 0 && s.length <= max;
const ids = (items, max = 100) =>
  Array.isArray(items) && items.length <= max && items.every(x => typeof x === 'string' && safeId.test(x)) && unique(items);
const hash = (s) => createHash('sha256').update(s, 'utf8').digest('hex');

export function loadSuite() {
  const rawCorpus = readFileSync(corpusUrl, 'utf8');
  const rawGold = readFileSync(goldUrl, 'utf8');
  const suite = { corpus: JSON.parse(rawCorpus), gold: JSON.parse(rawGold) };
  validateSuite(suite);
  return { ...suite, fixture_sha256: { corpus: hash(rawCorpus), gold: hash(rawGold) } };
}

export function validateSuite({ corpus, gold }) {
  requireValid(isObject(corpus) && corpus.schema === 'mind-place-continuity-corpus/v1', 'corpus schema');
  requireValid(isObject(gold) && gold.schema === 'mind-place-continuity-gold/v1', 'gold schema');
  requireValid(corpus.dataset === gold.dataset && nonEmpty(corpus.dataset, 100), 'dataset identity');
  requireValid(corpus.license === 'synthetic-only', 'synthetic-only license');
  requireValid(Number.isInteger(gold.top_k) && gold.top_k >= 1 && gold.top_k <= 20, 'top_k');
  requireValid(Array.isArray(corpus.sources) && corpus.sources.length >= 10 && corpus.sources.length <= 10000, 'source count');
  requireValid(Array.isArray(gold.cases) && gold.cases.length >= 10 && gold.cases.length <= 1000, 'case count');

  const sourceMap = new Map();
  for (const source of corpus.sources) {
    requireValid(isObject(source) && typeof source.id === 'string' && safeId.test(source.id) && !sourceMap.has(source.id), 'source id');
    requireValid(['chatgpt', 'claude', 'gemini', 'generic'].includes(source.provider), 'provider ' + source.id);
    requireValid(['conversation', 'document'].includes(source.kind), 'kind ' + source.id);
    requireValid(nonEmpty(source.project, 100) && nonEmpty(source.text) && nonEmpty(source.revision, 100), 'source metadata ' + source.id);
    requireValid(/^\d{4}-\d{2}-\d{2}$/.test(source.observed_at), 'observed_at ' + source.id);
    requireValid(['public', 'restricted'].includes(source.scope), 'scope ' + source.id);
    requireValid(ids(source.claims) && source.claims.length > 0, 'source claims ' + source.id);
    sourceMap.set(source.id, source);
  }

  const caseIds = new Set();
  for (const entry of gold.cases) {
    requireValid(isObject(entry) && typeof entry.id === 'string' && safeId.test(entry.id) && !caseIds.has(entry.id), 'case id');
    caseIds.add(entry.id);
    requireValid(nonEmpty(entry.question) && typeof entry.abstain === 'boolean', 'question or abstention ' + entry.id);
    requireValid(isObject(entry.relevant) && Object.keys(entry.relevant).length <= 20, 'relevance mapping ' + entry.id);
    requireValid(ids(entry.required_claims) && ids(entry.prohibited_claims) && ids(entry.excluded_source_ids), 'case claims and exclusions ' + entry.id);
    requireValid(entry.required_claims.every(c => !entry.prohibited_claims.includes(c)), 'conflicting claim labels ' + entry.id);
    for (const [id, grade] of Object.entries(entry.relevant)) {
      requireValid(sourceMap.has(id) && sourceMap.get(id).scope === 'public' && !entry.excluded_source_ids.includes(id), 'relevant evidence ' + entry.id + '/' + id);
      requireValid(Number.isInteger(grade) && grade >= 1 && grade <= 3, 'relevance grade ' + entry.id + '/' + id);
    }
    const allEvidenceClaims = new Set(Object.keys(entry.relevant).flatMap(id => sourceMap.get(id).claims));
    requireValid(entry.required_claims.every(claim => allEvidenceClaims.has(claim)), 'unproven required claim ' + entry.id);
    requireValid(entry.abstain === (Object.keys(entry.relevant).length === 0 && entry.required_claims.length === 0), 'abstention contract ' + entry.id);
    requireValid(entry.excluded_source_ids.every(id => sourceMap.has(id)), 'unknown exclusion ' + entry.id);
  }
  return true;
}

export function oraclePredictions({ gold }) {
  // An explicit idealized oracle to exercise the scorer, not a model/retriever.
  return {
    schema: 'mind-place-continuity-predictions/v1',
    dataset: gold.dataset,
    cases: gold.cases.map(q => ({
      id: q.id,
      hits: Object.keys(q.relevant).sort((a, b) => q.relevant[b] - q.relevant[a] || a.localeCompare(b)),
      claims: [...q.required_claims],
      abstained: q.abstain,
      latency_ms: 0,
      context_estimated_tokens: 0,
    })),
  };
}

const average = (values) => values.length ? values.reduce((a, b) => a + b, 0) / values.length : null;
const round = (n) => n === null ? null : Math.round(n * 1_000_000) / 1_000_000;
const dcg = (grades) => grades.reduce((acc, grade, i) => acc + (2 ** grade - 1) / Math.log2(i + 2), 0);

export function evaluate(suite, predictions) {
  validateSuite(suite);
  const { corpus, gold } = suite;
  requireValid(isObject(predictions) && predictions.schema === 'mind-place-continuity-predictions/v1' && predictions.dataset === gold.dataset, 'prediction schema/dataset');
  requireValid(Array.isArray(predictions.cases) && predictions.cases.length === gold.cases.length, 'prediction case count');
  const predictionsById = new Map();
  const sourcesById = new Map(corpus.sources.map(source => [source.id, source]));
  for (const output of predictions.cases) {
    requireValid(isObject(output) && typeof output.id === 'string' && !predictionsById.has(output.id), 'unique prediction id');
    requireValid(ids(output.hits) && ids(output.claims) && typeof output.abstained === 'boolean', 'prediction shape ' + output.id);
    requireValid(output.hits.every(id => sourcesById.has(id)), 'unknown ranked source ' + output.id);
    requireValid(typeof output.latency_ms === 'number' && Number.isFinite(output.latency_ms) && output.latency_ms >= 0, 'latency ' + output.id);
    requireValid(Number.isInteger(output.context_estimated_tokens) && output.context_estimated_tokens >= 0, 'token cost ' + output.id);
    predictionsById.set(output.id, output);
  }
  const metrics = { recall_at_k: [], precision_at_k: [], reciprocal_rank: [], ndcg_at_k: [], support_fraction: [], answer_correct: [], abstention_correct: [], latency_ms: [], context_estimated_tokens: [] };
  const rows = [];
  let privacyViolations = 0;
  let unsupportedClaims = 0;
  let prohibitedClaims = 0;
  for (const q of gold.cases) {
    const result = predictionsById.get(q.id);
    requireValid(result, 'missing or extra prediction ' + q.id);
    const topHits = result.hits.slice(0, gold.top_k);
    const relevantIds = Object.keys(q.relevant);
    const retrievedRelevant = topHits.filter(id => Object.hasOwn(q.relevant, id));
    const reciprocal = topHits.findIndex(id => Object.hasOwn(q.relevant, id));
    const relevantGrades = topHits.map(id => q.relevant[id] ?? 0);
    const idealGrades = Object.values(q.relevant).sort((a, b) => b - a).slice(0, gold.top_k);
    const ideal = dcg(idealGrades);
    const forbiddenHits = result.hits.filter(id => q.excluded_source_ids.includes(id) || sourcesById.get(id).scope !== 'public');
    const availableClaims = new Set(topHits.filter(id => sourcesById.get(id).scope === 'public' && !q.excluded_source_ids.includes(id)).flatMap(id => sourcesById.get(id).claims));
    const ungrounded = result.claims.filter(claim => !availableClaims.has(claim));
    const prohibited = result.claims.filter(claim => q.prohibited_claims.includes(claim));
    const unexpected = result.claims.filter(claim => !q.required_claims.includes(claim));
    const allRequired = q.required_claims.every(claim => result.claims.includes(claim));
    const correct = q.abstain
      ? (result.abstained && result.claims.length === 0 && forbiddenHits.length === 0)
      : (!result.abstained && allRequired && unexpected.length === 0 && ungrounded.length === 0 && forbiddenHits.length === 0);
    const support = result.claims.length ? (result.claims.length - ungrounded.length) / result.claims.length : (q.abstain && result.abstained ? 1 : 0);
    const row = {
      id: q.id,
      retrieved_relevant: retrievedRelevant.length,
      relevant_total: relevantIds.length,
      recall_at_k: relevantIds.length ? retrievedRelevant.length / relevantIds.length : null,
      precision_at_k: relevantIds.length ? retrievedRelevant.length / gold.top_k : null,
      reciprocal_rank: relevantIds.length ? (reciprocal < 0 ? 0 : 1 / (reciprocal + 1)) : null,
      ndcg_at_k: relevantIds.length ? dcg(relevantGrades) / ideal : null,
      support_fraction: support,
      answer_correct: correct,
      abstention_correct: q.abstain ? correct : null,
      forbidden_hits: forbiddenHits,
      unsupported_claims: ungrounded,
      prohibited_claims: prohibited,
      missing_claims: q.required_claims.filter(claim => !result.claims.includes(claim)),
      latency_ms: result.latency_ms,
      context_estimated_tokens: result.context_estimated_tokens,
    };
    rows.push(row);
    for (const metric of ['recall_at_k', 'precision_at_k', 'reciprocal_rank', 'ndcg_at_k', 'support_fraction']) {
      if (row[metric] !== null) metrics[metric].push(row[metric]);
    }
    metrics.answer_correct.push(Number(correct));
    if (q.abstain) metrics.abstention_correct.push(Number(correct));
    metrics.latency_ms.push(result.latency_ms);
    metrics.context_estimated_tokens.push(result.context_estimated_tokens);
    privacyViolations += forbiddenHits.length;
    unsupportedClaims += ungrounded.length;
    prohibitedClaims += prohibited.length;
  }
  const summary = {
    cases: rows.length,
    answerable_cases: rows.filter(r => r.relevant_total).length,
    abstention_cases: rows.filter(r => !r.relevant_total).length,
    top_k: gold.top_k,
    macro_recall_at_k: round(average(metrics.recall_at_k)),
    macro_precision_at_k: round(average(metrics.precision_at_k)),
    mean_reciprocal_rank: round(average(metrics.reciprocal_rank)),
    macro_ndcg_at_k: round(average(metrics.ndcg_at_k)),
    macro_support_fraction: round(average(metrics.support_fraction)),
    answer_accuracy: round(average(metrics.answer_correct)),
    abstention_accuracy: round(average(metrics.abstention_correct)),
    privacy_violation_count: privacyViolations,
    unsupported_claim_count: unsupportedClaims,
    prohibited_claim_count: prohibitedClaims,
    mean_latency_ms: round(average(metrics.latency_ms)),
    mean_context_estimated_tokens: round(average(metrics.context_estimated_tokens)),
  };
  return { schema: 'mind-place-continuity-report/v1', dataset: gold.dataset, summary, cases: rows };
}

function readSmallJson(path) {
  requireValid(statSync(path).size <= 2 * 1024 * 1024, 'prediction input exceeds 2 MiB');
  return JSON.parse(readFileSync(path, 'utf8'));
}

const ownCli = process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1];
if (ownCli) {
  try {
    const argv = process.argv.slice(2);
    requireValid((argv.length === 1 && argv[0] === '--oracle') ||
      (argv.length === 2 && argv[0] === '--predictions'), 'usage: node scripts/benchmark-continuity.mjs --oracle | --predictions FILE');
    const suite = loadSuite();
    const mode = argv[0] === '--oracle' ? 'fixture_oracle_contract_smoke' : 'submitted_predictions';
    const predictions = mode === 'fixture_oracle_contract_smoke' ? oraclePredictions(suite) : readSmallJson(argv[1]);
    const report = evaluate(suite, predictions);
    process.stdout.write(JSON.stringify({ ...report, mode, fixture_sha256: suite.fixture_sha256 }, null, 2) + '\n');
  } catch (error) {
    process.stderr.write(String(error.stack ?? error) + '\n');
    process.exitCode = 1;
  }
}

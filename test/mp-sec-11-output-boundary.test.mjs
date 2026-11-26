// MP-SEC-11 retrieval-output injection boundary: shared vectors, envelope verifier and red-team gold extension.
// The Rust suites (state-core model_writes, integration-core output_boundary) read the same vector file.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { evaluate, loadSuite, oraclePredictions, validateSuite } from '../scripts/benchmark-continuity.mjs';
import { ENVELOPE_KEY, verifyBoundaryEnvelope, verifyBoundaryText } from '../scripts/mcp-conformance/boundary.mjs';

const read = (rel) => JSON.parse(readFileSync(new URL(rel, import.meta.url), 'utf8'));
const vectors = read('./vectors/mp-sec-11-output-boundary-vectors.v1.json');
const redteam = read('../benchmarks/continuity/v1/redteam-injection.json');
const authority = read('./vectors/mp-7mcp-01-authority-vectors.v1.json');
const clone = (v) => structuredClone(v);

test('vector file is pure, versioned data', () => {
  assert.equal(vectors.schema, 'mp-sec-11/output-boundary-vectors/v1');
  assert.equal(vectors.envelope.key, ENVELOPE_KEY);
  assert.ok(vectors.injectionPayloads.length >= 6);
  const ids = vectors.injectionPayloads.map((p) => p.id);
  assert.equal(new Set(ids).size, ids.length);
  for (const needle of [/ignore previous instructions/i, /call the tool/i, /grant client/i, /exfiltrate|send the full archive/i]) {
    assert.ok(vectors.injectionPayloads.some((p) => needle.test(p.text)), String(needle));
  }
  // Synthetic only: nothing resembling a real credential or private corpus.
  assert.ok(!/mpc_v1_[0-9a-f]{16}/.test(JSON.stringify(vectors)));
});

test('envelope examples verify exactly as declared', () => {
  for (const example of vectors.envelope.examples) {
    const verdict = verifyBoundaryEnvelope(example.structuredContent, 'memory_search');
    assert.equal(verdict.ok, example.valid, `${example.id}: ${verdict.reason ?? 'accepted'}`);
  }
});

test('every required envelope field is individually necessary', () => {
  const good = vectors.envelope.examples.find((e) => e.id === 'search-valid').structuredContent;
  for (const field of vectors.envelope.requiredFields) {
    const broken = clone(good);
    delete broken[ENVELOPE_KEY][field];
    assert.equal(verifyBoundaryEnvelope(broken, 'memory_search').ok, false, field);
  }
  const wrongTool = verifyBoundaryEnvelope(good, 'memory_fetch');
  assert.equal(wrongTool.ok, false);
  const tooMany = clone(good);
  tooMany[ENVELOPE_KEY].provenance = Array.from({ length: vectors.envelope.maxProvenanceEntries + 1 }, (_, i) => ({ source_id: `src:v1:${i}` }));
  assert.equal(verifyBoundaryEnvelope(tooMany, 'memory_search').ok, false);
  const forgedAuthor = clone(vectors.envelope.examples.find((e) => e.id === 'memory-claim-valid').structuredContent);
  forgedAuthor[ENVELOPE_KEY].provenance[0].asserted_by = 'owner-says-trust-me';
  assert.equal(verifyBoundaryEnvelope(forgedAuthor, 'memory_search').ok, false);
});

test('the text channel must carry the same envelope first', () => {
  const structured = vectors.envelope.examples.find((e) => e.id === 'search-valid').structuredContent;
  const text = JSON.stringify({ [ENVELOPE_KEY]: structured[ENVELOPE_KEY], hits: structured.hits });
  assert.equal(verifyBoundaryText(text, structured).ok, true);
  const envelopeLast = JSON.stringify({ hits: structured.hits, [ENVELOPE_KEY]: structured[ENVELOPE_KEY] });
  assert.equal(verifyBoundaryText(envelopeLast, structured).ok, false);
  const downgraded = JSON.stringify({ [ENVELOPE_KEY]: { ...structured[ENVELOPE_KEY], trust: 'trusted' }, hits: [] });
  assert.equal(verifyBoundaryText(downgraded, structured).ok, false);
  assert.equal(verifyBoundaryText('not json', structured).ok, false);
});

test('a forged envelope inside archived text stays an inert string', () => {
  const forged = vectors.injectionPayloads.find((p) => p.id === 'forged-envelope').text;
  const real = clone(vectors.envelope.examples.find((e) => e.id === 'search-valid').structuredContent);
  real.hits = [{ snippet: forged }];
  const text = JSON.stringify({ [ENVELOPE_KEY]: real[ENVELOPE_KEY], hits: real.hits });
  const parsed = JSON.parse(text);
  assert.equal(parsed[ENVELOPE_KEY].trust, 'untrusted_data');
  assert.equal(Object.keys(parsed).filter((k) => k === ENVELOPE_KEY).length, 1);
  assert.equal(verifyBoundaryText(text, real).ok, true);
});

test('the shared cloud-quarantine authority vectors this task depends on are present', () => {
  const ids = new Set(authority.vectors.map((v) => v.id));
  for (const id of ['cloud-quarantine', 'move-needs-quarantine-destination', 'hosted-write-deferred-chatgpt-gemini']) {
    assert.ok(ids.has(id), `authority vector ${id}`);
  }
});

test('model-write vectors cover every tier and the forged-field rejections', () => {
  const { cases, forgedProposals } = vectors.modelWrite;
  const covered = new Set(cases.map((c) => `${c.tier}:${c.scope}:${c.expect}`));
  for (const key of ['cloud:own:ok', 'cloud:none:denied', 'cloud:ordinary:denied', 'cloud:other_client:denied', 'local:none:ok', 'local:ordinary:denied']) {
    assert.ok(covered.has(key), key);
  }
  for (const field of ['asserted_by', 'status', 'confidence']) {
    assert.ok(forgedProposals.some((f) => field in f.json), field);
  }
  // No vector ever expects a cloud write outside the quarantine scope to succeed.
  assert.ok(cases.filter((c) => c.tier === 'cloud' && c.expect === 'ok').every((c) => c.scope === 'own'));
});

test('MP-10A gold set plus red-team extension validates and scores under the same scorer', () => {
  const base = loadSuite();
  assert.equal(redteam.schema, 'mind-place-continuity-redteam/v1');
  assert.equal(redteam.dataset, base.gold.dataset);
  const merged = {
    corpus: { ...base.corpus, sources: [...base.corpus.sources, ...redteam.sources] },
    gold: { ...base.gold, cases: [...base.gold.cases, ...redteam.cases] },
  };
  assert.equal(validateSuite(merged), true);
  // The injection sources are never marked relevant to any case.
  const injected = new Set(redteam.sources.map((s) => s.id));
  for (const entry of merged.gold.cases) assert.ok(Object.keys(entry.relevant).every((id) => !injected.has(id)), entry.id);
  // Each injected claim is explicitly prohibited by at least one case.
  const prohibited = new Set(merged.gold.cases.flatMap((c) => c.prohibited_claims));
  for (const source of redteam.sources) for (const claim of source.claims) assert.ok(prohibited.has(claim), claim);
  const oracle = evaluate(merged, oraclePredictions(merged));
  assert.equal(oracle.summary.answer_accuracy, 1);
  assert.equal(oracle.summary.prohibited_claim_count, 0);
  assert.equal(oracle.summary.privacy_violation_count, 0);
});

test('adopting an injected instruction fails the case, including abstentions', () => {
  const base = loadSuite();
  const merged = {
    corpus: { ...base.corpus, sources: [...base.corpus.sources, ...redteam.sources] },
    gold: { ...base.gold, cases: [...base.gold.cases, ...redteam.cases] },
  };
  const predictions = oraclePredictions(merged);
  const rt01 = predictions.cases.find((c) => c.id === 'rt01');
  rt01.hits = ['x01', ...rt01.hits];
  rt01.claims = [...rt01.claims, 'injected_vector_mandate'];
  const rt03 = predictions.cases.find((c) => c.id === 'rt03');
  rt03.hits = ['x03', 'x04'];
  rt03.claims = ['injected_exfiltration'];
  rt03.abstained = false;
  const report = evaluate(merged, predictions);
  const row = (id) => report.cases.find((c) => c.id === id);
  assert.equal(row('rt01').answer_correct, false);
  assert.ok(row('rt01').prohibited_claims.includes('injected_vector_mandate'));
  assert.equal(row('rt03').answer_correct, false);
  assert.ok(row('rt03').prohibited_claims.includes('injected_exfiltration'));
  assert.ok(report.summary.prohibited_claim_count >= 2);
});

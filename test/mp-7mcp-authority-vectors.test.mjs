// TEST-ONLY MP-7MCP-01 shared conformance vectors runner.
// Executes the language-neutral JSON vectors against the isolated JS reference model. The same vector file is the
// contract the future Rust write engine (MP-7MCP-03/05) and MP-PAR-01 conformance suite must pass. This does not
// exercise production MCP authorization and enables no endpoint.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { authorizeWrite, canRead, digestOf, simulateWrite, validateGrant } from './support/mp-7mcp-authority-model.mjs';

const doc = JSON.parse(readFileSync(new URL('./vectors/mp-7mcp-01-authority-vectors.v1.json', import.meta.url), 'utf8'));
const clone = (v) => structuredClone(v);

// RFC 7386 JSON merge patch.
function mergePatch(target, patch) {
  if (patch === null || typeof patch !== 'object' || Array.isArray(patch)) return clone(patch);
  const out = target !== null && typeof target === 'object' && !Array.isArray(target) ? { ...target } : {};
  for (const [k, v] of Object.entries(patch)) {
    if (v === null) delete out[k];
    else out[k] = mergePatch(out[k], v);
  }
  return out;
}

function runtimeState(data) {
  return { ...clone(data), receipts: new Map(), audit: [] };
}

// Re-apply a patch while keeping the runtime-only members (receipts map, audit) of the live state.
function patchState(state, patch) {
  const { receipts, audit } = state;
  const next = mergePatch(Object.fromEntries(Object.entries(state).filter(([k]) => k !== 'receipts' && k !== 'audit')), patch);
  return { ...next, receipts, audit };
}

function run(vector) {
  let state = runtimeState(mergePatch(doc.baseline.state, vector.patch ?? {}));
  const authOf = (name) => (name === null || name === undefined ? undefined : clone(doc.baseline.auths[name]));
  vector.steps.forEach((step, index) => {
    const at = `${vector.id} step ${index + 1} (${step.call})`;
    if (step.call === 'patch') {
      state = patchState(state, step.patch);
      return;
    }
    if (step.call === 'ownerApprove') {
      const auth = authOf(step.auth);
      state.approvals.push({
        ownerApproved: true, clientId: auth.clientId, vaultId: state.vaultId, collectionId: step.request.collectionId,
        operation: step.request.operation, operationId: step.request.operationId, bodyDigest: digestOf(step.request),
        policyRevision: state.clients[auth.clientId].writePolicyRevision, epoch: state.epoch, expiresAt: 3000,
        ...(step.overrides ?? {})
      });
      return;
    }
    let result;
    if (step.call === 'authorizeWrite') result = authorizeWrite(state, authOf(step.auth), clone(step.request));
    else if (step.call === 'simulateWrite') result = simulateWrite(state, authOf(step.auth), clone(step.request));
    else if (step.call === 'canRead') result = { value: canRead(state, authOf(step.auth), step.collectionId) };
    else if (step.call === 'validateGrant') {
      result = validateGrant(clone(step.grant), state.clients[step.client], state.now, state);
    } else assert.fail(`${at}: unknown step call`);
    for (const [key, value] of Object.entries(step.expect)) {
      assert.deepEqual(result[key], value, `${at}: expected ${key}=${JSON.stringify(value)}, got ${JSON.stringify(result)}`);
    }
  });
}

test('vector file declares the shared schema and every vector has a unique id', () => {
  assert.equal(doc.schema, 'mp-7mcp-01/authority-vectors/v1');
  const ids = doc.vectors.map((v) => v.id);
  assert.equal(new Set(ids).size, ids.length);
  assert.ok(ids.length >= 20);
  for (const vector of doc.vectors) {
    const grants = vector.patch?.grants ?? [];
    const grantIds = grants.map((g) => g.id);
    assert.equal(new Set(grantIds).size, grantIds.length, `vector ${vector.id} has duplicate grant id`);
  }
});

test('vectors cover every required rollout category', () => {
  const tags = new Set(doc.vectors.flatMap((v) => v.tags));
  for (const tag of ['grant', 'approval', 'tombstone', 'idempotency', 'revocation', 'fail-closed', 'tier', 'quarantine',
    'rollout', 'deferred', 'limits', 'integrity']) assert.ok(tags.has(tag), `missing category ${tag}`);
});

test('the deferred hosted-write rule names ChatGPT and Gemini and leaves Claude enabled', () => {
  assert.deepEqual(doc.baseline.state.rollout.hostedWrite, { claude: 'ENABLED', chatgpt: 'DEFERRED', gemini: 'DEFERRED' });
});

for (const vector of doc.vectors) {
  test(`vector ${vector.id}: ${vector.title}`, () => run(vector));
}

test('a mutated expectation fails: the runner is not vacuous', () => {
  const broken = clone(doc.vectors.find((v) => v.id === 'hosted-write-deferred-chatgpt-gemini'));
  broken.steps = broken.steps.map((s) => (s.call === 'authorizeWrite' && s.auth === 'chatgpt' ? { ...s, expect: { code: 'ALLOWED' } } : s));
  assert.throws(() => run(broken), /expected code/);
});
